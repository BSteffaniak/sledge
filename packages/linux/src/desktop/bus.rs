//! Bounded same-user D-Bus calls. Pin each operation to a unique owner;
//! never autostart a desktop daemon or connect on behalf of the broker.
use super::{DesktopError, IPC_TIMEOUT, protocol, unavailable};
use serde::Serialize;
use std::cell::RefCell;
use zbus::{Connection, Message, zvariant::DynamicType};

pub(crate) struct Bus {
    runtime: tokio::runtime::Runtime,
    connection: RefCell<Option<Connection>>,
    pub owner: String,
    name: String,
    path: String,
    interface: String,
}
impl Bus {
    pub fn session(name: &str, path: &str, interface: &str) -> Result<Self, DesktopError> {
        let _operation = super::operation::adapter_scope();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let connection = runtime.block_on(async {
            let builder = zbus::connection::Builder::session()
                .map_err(unavailable)?
                .max_queued(8)
                .method_timeout(IPC_TIMEOUT);
            super::operation::wait(builder.build())
                .await
                .map_err(unavailable)?
                .map_err(unavailable)
        })?;
        let owner = runtime.block_on(async {
            let msg = super::operation::wait(connection.call_method(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                Some("org.freedesktop.DBus"),
                "GetNameOwner",
                &(name,),
            ))
            .await
            .map_err(unavailable)?
            .map_err(unavailable)?;
            msg.body().deserialize::<String>().map_err(protocol)
        })?;
        Ok(Self {
            runtime,
            connection: RefCell::new(Some(connection)),
            owner,
            name: name.into(),
            path: path.into(),
            interface: interface.into(),
        })
    }
    /// A cancelled zbus send may have a future-local partial-frame offset. Never
    /// reuse that connection, including when interruption occurs waiting for reply.
    fn request<B: Serialize + DynamicType>(
        &self,
        destination: &str,
        path: &str,
        interface: &str,
        method: &str,
        body: &B,
    ) -> Result<Message, DesktopError> {
        let connection = self
            .connection
            .borrow()
            .as_ref()
            .cloned()
            .ok_or_else(|| unavailable("D-Bus connection invalidated; reconnect required"))?;
        let result = self.runtime.block_on(async {
            super::operation::wait(connection.call_method(
                Some(destination),
                path,
                Some(interface),
                method,
                body,
            ))
            .await?
            .map_err(unavailable)
        });
        if result.is_err() {
            self.connection.borrow_mut().take();
            // The cancelled call future has released the send lock. Close without
            // resending/flushing its partial frame; never use graceful_shutdown.
            self.runtime.block_on(tokio_close(connection));
        }
        result
    }
    pub fn check_owner(&self) -> Result<(), DesktopError> {
        let _operation = super::operation::adapter_scope();
        let owner: String = self
            .request(
                "org.freedesktop.DBus",
                "/org/freedesktop/DBus",
                "org.freedesktop.DBus",
                "GetNameOwner",
                &(&self.name,),
            )?
            .body()
            .deserialize()
            .map_err(protocol)?;
        if owner != self.owner {
            return Err(unavailable("desktop owner changed; reconnect required"));
        }
        Ok(())
    }
    pub fn call<B: Serialize + DynamicType>(
        &self,
        method: &str,
        body: &B,
    ) -> Result<Message, DesktopError> {
        let _operation = super::operation::adapter_scope();
        self.check_owner()?;
        let message = self.request(&self.owner, &self.path, &self.interface, method, body)?;
        if message.data().len() > super::MAX_MESSAGE {
            return Err(protocol("D-Bus reply oversized"));
        }
        Ok(message)
    }
    pub fn property(&self, name: &str) -> Result<zbus::zvariant::OwnedValue, DesktopError> {
        let _operation = super::operation::adapter_scope();
        self.check_owner()?;
        let message = self.request(
            &self.owner,
            &self.path,
            "org.freedesktop.DBus.Properties",
            "Get",
            &(&self.interface, name),
        )?;
        if message.data().len() > super::MAX_MESSAGE {
            return Err(protocol("D-Bus property oversized"));
        }
        message.body().deserialize().map_err(protocol)
    }
}
async fn tokio_close(connection: Connection) {
    let _ = tokio::time::timeout(super::operation::SLICE, connection.close()).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    };
    use std::time::Duration;
    use zbus::connection::socket::{ReadHalf, WriteHalf};

    #[derive(Clone, Copy)]
    enum RequestPath {
        Method,
        Owner,
        Property,
    }

    fn interrupted_request(partial: bool, path: RequestPath) {
        let (client, server) = UnixStream::pair().unwrap();
        nix::sys::socket::setsockopt(&client, nix::sys::socket::sockopt::SndBuf, &4096).unwrap();
        client.set_nonblocking(true).unwrap();
        server.set_nonblocking(true).unwrap();
        let stopped = Arc::new(AtomicBool::new(false));
        let cancel = stopped.clone();
        let (release, released) = mpsc::channel::<()>();
        let peer = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async {
                let socket = tokio::net::UnixStream::from_std(server).unwrap();
                let (mut read, mut write) = socket.into_split();
                if matches!(path, RequestPath::Property) {
                    let owner = read
                        .receive_message(0, &mut vec![], &mut vec![])
                        .await
                        .unwrap();
                    let reply = Message::method_return(&owner.header())
                        .unwrap()
                        .build(&":1.99")
                        .unwrap();
                    write.send_message(&reply).await.unwrap();
                }
                let received = if partial {
                    let mut prefix = [0; 16];
                    let (count, _) = read.recvmsg(&mut prefix).await.unwrap();
                    assert!(count > 0);
                    count
                } else {
                    read.receive_message(1, &mut vec![], &mut vec![])
                        .await
                        .unwrap()
                        .data()
                        .len()
                };
                cancel.store(true, Ordering::Release);
                released.recv_timeout(Duration::from_secs(2)).unwrap();
                let mut total = received;
                loop {
                    let mut bytes = [0; 4096];
                    let result =
                        tokio::time::timeout(Duration::from_secs(1), read.recvmsg(&mut bytes))
                            .await
                            .unwrap();
                    let count = match result {
                        Ok((count, _)) => count,
                        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => break,
                        Err(error) => panic!("unexpected transport error: {error}"),
                    };
                    if count == 0 {
                        break;
                    }
                    total += count;
                }
                if partial {
                    assert!(total < 1024 * 1024, "abandoned frame was completed");
                } else {
                    assert_eq!(total, received, "later call sent more bytes");
                }
            });
        });
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let connection = runtime.block_on(async {
            zbus::connection::Builder::authenticated_socket(
                tokio::net::UnixStream::from_std(client).unwrap(),
                "0123456789abcdef0123456789abcdef",
            )
            .unwrap()
            .build()
            .await
            .unwrap()
        });
        let bus = Bus {
            runtime,
            connection: RefCell::new(Some(connection)),
            owner: ":1.99".into(),
            name: "org.fake.Service".into(),
            path: "/fake".into(),
            interface: "org.fake.Service".into(),
        };
        let result = super::super::operation::run(stopped, || {
            if matches!(path, RequestPath::Owner) {
                bus.check_owner()?;
            } else if matches!(path, RequestPath::Property) {
                bus.property("Current")?;
            } else {
                bus.request(
                    &bus.owner,
                    &bus.path,
                    &bus.interface,
                    "Select",
                    &vec![0_u8; 1024 * 1024],
                )?;
            }
            Ok(())
        });
        assert!(result.is_err());
        assert!(bus.connection.borrow().is_none());
        assert!(bus.call("Select", &()).is_err());
        assert!(bus.property("Current").is_err());
        assert!(bus.check_owner().is_err());
        release.send(()).unwrap();
        peer.join().unwrap();
    }
    #[test]
    fn partial_send_cancellation_invalidates_connection() {
        interrupted_request(true, RequestPath::Method);
    }
    #[test]
    fn reply_wait_cancellation_invalidates_connection() {
        interrupted_request(false, RequestPath::Method);
    }
    #[test]
    fn property_reply_wait_cancellation_invalidates_connection() {
        interrupted_request(false, RequestPath::Property);
    }
    #[test]
    fn owner_reply_wait_cancellation_invalidates_connection() {
        interrupted_request(false, RequestPath::Owner);
    }
}
