//! Bounded same-user D-Bus calls. Pin each operation to a unique owner;
//! never autostart a desktop daemon or connect on behalf of the broker.
use super::{DesktopError, IPC_TIMEOUT, protocol, unavailable};
use serde::Serialize;
use zbus::{Connection, Message, zvariant::DynamicType};

pub(crate) struct Bus {
    runtime: tokio::runtime::Runtime,
    connection: Connection,
    pub owner: String,
    name: String,
    path: String,
    interface: String,
}
impl Bus {
    pub fn session(name: &str, path: &str, interface: &str) -> Result<Self, DesktopError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let connection = runtime.block_on(async {
            let builder = zbus::connection::Builder::session()
                .map_err(unavailable)?
                .max_queued(8)
                .method_timeout(IPC_TIMEOUT);
            tokio::time::timeout(IPC_TIMEOUT, builder.build())
                .await
                .map_err(unavailable)?
                .map_err(unavailable)
        })?;
        let owner = runtime.block_on(async {
            let msg = tokio::time::timeout(
                IPC_TIMEOUT,
                connection.call_method(
                    Some("org.freedesktop.DBus"),
                    "/org/freedesktop/DBus",
                    Some("org.freedesktop.DBus"),
                    "GetNameOwner",
                    &(name,),
                ),
            )
            .await
            .map_err(unavailable)?
            .map_err(unavailable)?;
            msg.body().deserialize::<String>().map_err(protocol)
        })?;
        Ok(Self {
            runtime,
            connection,
            owner,
            name: name.into(),
            path: path.into(),
            interface: interface.into(),
        })
    }
    pub fn check_owner(&self) -> Result<(), DesktopError> {
        let owner = self.runtime.block_on(async {
            let msg = tokio::time::timeout(
                IPC_TIMEOUT,
                self.connection.call_method(
                    Some("org.freedesktop.DBus"),
                    "/org/freedesktop/DBus",
                    Some("org.freedesktop.DBus"),
                    "GetNameOwner",
                    &(&self.name,),
                ),
            )
            .await
            .map_err(unavailable)?
            .map_err(unavailable)?;
            msg.body().deserialize::<String>().map_err(protocol)
        })?;
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
        self.check_owner()?;
        let message = self.runtime.block_on(async {
            tokio::time::timeout(
                IPC_TIMEOUT,
                self.connection.call_method(
                    Some(self.owner.as_str()),
                    self.path.as_str(),
                    Some(self.interface.as_str()),
                    method,
                    body,
                ),
            )
            .await
            .map_err(unavailable)?
            .map_err(unavailable)
        })?;
        if message.data().len() > super::MAX_MESSAGE {
            return Err(protocol("D-Bus reply oversized"));
        }
        Ok(message)
    }
    pub fn property(&self, name: &str) -> Result<zbus::zvariant::OwnedValue, DesktopError> {
        self.check_owner()?;
        self.runtime.block_on(async {
            let msg = tokio::time::timeout(
                IPC_TIMEOUT,
                self.connection.call_method(
                    Some(self.owner.as_str()),
                    self.path.as_str(),
                    Some("org.freedesktop.DBus.Properties"),
                    "Get",
                    &(&self.interface, name),
                ),
            )
            .await
            .map_err(unavailable)?
            .map_err(unavailable)?;
            if msg.data().len() > super::MAX_MESSAGE {
                return Err(protocol("D-Bus property oversized"));
            }
            msg.body().deserialize().map_err(protocol)
        })
    }
}
