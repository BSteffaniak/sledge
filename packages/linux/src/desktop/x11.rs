//! Explicit local X11 EWMH/XKB connection with bounded stream waits.
use super::{
    DesktopError, DesktopProvider, DesktopSnapshot, FocusCategory, FocusIdentity, IPC_TIMEOUT,
    LockHint, Source, SourceCatalog, protocol, unavailable,
};
use nix::poll::{PollFd, PollFlags, poll};
use std::io;
use std::os::fd::AsFd;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::ConnectionExt;
use x11rb::protocol::{xkb, xproto};
use x11rb::rust_connection::{DefaultStream, PollMode, RustConnection, Stream};
use x11rb::utils::RawFdContainer;
use x11rb::x11_utils::Serialize;

struct TimedStream {
    stream: DefaultStream,
    deadline: Mutex<Instant>,
    poisoned: AtomicBool,
}
impl TimedStream {
    fn invalidate(&self) {
        self.poisoned.store(true, Ordering::Release);
        // Shutdown, not flush: x11rb may still hold an abandoned selection buffer.
        let _ = nix::sys::socket::shutdown(
            self.stream.as_fd().as_raw_fd(),
            nix::sys::socket::Shutdown::Both,
        );
    }
    fn check(&self) -> io::Result<()> {
        if self.poisoned.load(Ordering::Acquire) {
            return Err(io::Error::other(
                "X11 connection invalidated; reconnect required",
            ));
        }
        if let Err(error) = super::operation::remaining() {
            self.invalidate();
            return Err(io::Error::other(error));
        }
        Ok(())
    }

    fn reset(&self) {
        if let Ok(mut deadline) = self.deadline.lock() {
            *deadline = Instant::now() + IPC_TIMEOUT;
        }
    }
}
impl Stream for TimedStream {
    fn poll(&self, mode: PollMode) -> io::Result<()> {
        self.check()?;
        let deadline = *self
            .deadline
            .lock()
            .map_err(|_| io::Error::other("deadline poisoned"))?;
        let mut flags = PollFlags::empty();
        if mode.readable() {
            flags |= PollFlags::POLLIN;
        }
        if mode.writable() {
            flags |= PollFlags::POLLOUT;
        }
        let mut fds = [PollFd::new(self.stream.as_fd(), flags)];
        loop {
            self.check()?;
            let left = deadline
                .saturating_duration_since(Instant::now())
                .min(super::operation::remaining().map_err(io::Error::other)?);
            if left.is_zero() {
                self.invalidate();
                return Err(io::Error::new(io::ErrorKind::TimedOut, "X11 deadline"));
            }
            let timeout = u16::try_from(left.as_millis())
                .unwrap_or(u16::MAX)
                .clamp(1, 10);
            if poll(&mut fds, timeout)? != 0 {
                break;
            }
        }
        Ok(())
    }
    fn read(&self, bytes: &mut [u8], fds: &mut Vec<RawFdContainer>) -> io::Result<usize> {
        self.check()?;
        self.stream.read(bytes, fds)
    }
    fn write(&self, bytes: &[u8], fds: &mut Vec<RawFdContainer>) -> io::Result<usize> {
        self.check()?;
        self.stream.write(bytes, fds)
    }
}

pub struct X11 {
    connection: RustConnection<TimedStream>,
    root: u32,
    active: u32,
    class: u32,
    wm_check: u32,
    supported: u32,
    epoch: String,
}
impl X11 {
    /// Caller supplies an explicitly selected local X socket and its user cookie.
    /// Never use a Wayland session's DISPLAY as proof of desktop-wide X11 focus.
    pub fn connect(
        socket: UnixStream,
        screen: usize,
        auth_name: Vec<u8>,
        auth_data: Vec<u8>,
    ) -> Result<Self, DesktopError> {
        let _operation = super::operation::adapter_scope();
        let (stream, _) = DefaultStream::from_unix_stream(socket)?;
        let connection = RustConnection::connect_to_stream_with_auth_info(
            TimedStream {
                stream,
                deadline: Mutex::new(Instant::now() + IPC_TIMEOUT),
                poisoned: AtomicBool::new(false),
            },
            screen,
            auth_name,
            auth_data,
        )
        .map_err(unavailable)?;
        let root = connection
            .setup()
            .roots
            .get(screen)
            .ok_or_else(|| protocol("screen missing"))?
            .root;
        let extension = xkb::use_extension(&connection, 1, 0)
            .map_err(protocol)?
            .reply()
            .map_err(protocol)?;
        if !extension.supported {
            return Err(unavailable("XKB 1.0 unavailable"));
        }
        let atom = |name: &[u8]| -> Result<u32, DesktopError> {
            Ok(connection
                .intern_atom(true, name)
                .map_err(protocol)?
                .reply()
                .map_err(protocol)?
                .atom)
        };
        let active = atom(b"_NET_ACTIVE_WINDOW")?;
        let class = atom(b"WM_CLASS")?;
        let wm_check = atom(b"_NET_SUPPORTING_WM_CHECK")?;
        let supported = atom(b"_NET_SUPPORTED")?;
        Ok(Self {
            connection,
            root,
            active,
            class,
            wm_check,
            supported,
            epoch: format!("{:?}", Instant::now()),
        })
    }
    fn property32(
        &self,
        window: u32,
        atom: u32,
        kind: xproto::AtomEnum,
    ) -> Result<Vec<u32>, DesktopError> {
        let reply = self
            .connection
            .get_property(false, window, atom, kind, 0, 4096)
            .map_err(protocol)?
            .reply()
            .map_err(protocol)?;
        if reply.bytes_after != 0 {
            return Err(protocol("property oversized"));
        }
        reply
            .value32()
            .map(Iterator::collect)
            .ok_or_else(|| protocol("invalid 32-bit property"))
    }
    fn active(&self) -> Result<Option<u32>, DesktopError> {
        let check = self.property32(self.root, self.wm_check, xproto::AtomEnum::WINDOW)?;
        let wm = *check
            .first()
            .ok_or_else(|| unavailable("EWMH unavailable"))?;
        if wm == 0
            || self
                .property32(wm, self.wm_check, xproto::AtomEnum::WINDOW)?
                .first()
                != Some(&wm)
        {
            return Err(unavailable("EWMH owner invalid"));
        }
        if !self
            .property32(self.root, self.supported, xproto::AtomEnum::ATOM)?
            .contains(&self.active)
        {
            return Err(unavailable("EWMH focus unsupported"));
        }
        let active = self.property32(self.root, self.active, xproto::AtomEnum::WINDOW)?;
        Ok(active.first().copied().filter(|id| *id != 0))
    }
}
fn wm_class(bytes: &[u8]) -> Option<FocusIdentity> {
    let mut fields = bytes.split(|byte| *byte == 0);
    let _instance = fields.next()?;
    let class = fields.next()?;
    // WM_CLASS is Latin-1 (STRING), not UTF-8; preserve its exact codepoints.
    let class: String = class.iter().map(|byte| char::from(*byte)).collect();
    FocusIdentity::new(FocusCategory::X11Class, &class)
}
impl DesktopProvider for X11 {
    fn poll(&mut self) -> Result<DesktopSnapshot, DesktopError> {
        let _operation = super::operation::adapter_scope();
        let result = (|| {
            self.connection.stream().check().map_err(protocol)?;
            let observed_at = Instant::now();
            self.connection.stream().reset();
            let window = self.active()?;
            let mut focus = None;
            if let Some(window) = window {
                let reply = self
                    .connection
                    .get_property(false, window, self.class, xproto::AtomEnum::STRING, 0, 2048)
                    .map_err(protocol)?
                    .reply()
                    .map_err(protocol)?;
                if reply.format == 8 && reply.bytes_after == 0 {
                    focus = wm_class(&reply.value);
                }
                if self.active()? != Some(window) {
                    focus = None;
                }
            }
            Ok(DesktopSnapshot {
                focus,
                observed_at,
                lock_hint: LockHint::Unknown,
            })
        })()
        .and_then(|value| {
            super::operation::remaining()?;
            Ok(value)
        });
        if result.is_err() {
            self.connection.stream().invalidate();
        }
        result
    }
    fn sources(&mut self) -> Result<SourceCatalog, DesktopError> {
        let _operation = super::operation::adapter_scope();
        let result = (|| {
            self.connection.stream().check().map_err(protocol)?;
            self.connection.stream().reset();
            let names = xkb::get_names(
                &self.connection,
                256,
                xkb::NameDetail::GROUP_NAMES | xkb::NameDetail::SYMBOLS,
            )
            .map_err(protocol)?
            .reply()
            .map_err(protocol)?;
            let mut map = xkb::get_map(
                &self.connection,
                256,
                xkb::MapPart::KEY_SYMS | xkb::MapPart::KEY_TYPES,
                0_u16.into(),
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0_u16.into(),
                0,
                0,
                0,
                0,
                0,
                0,
            )
            .map_err(protocol)?
            .reply()
            .map_err(protocol)?
            .serialize();
            // Reply sequence is transport metadata, not inventory identity.
            if map.len() >= 4 {
                map[2] = 0;
                map[3] = 0;
            }
            let groups = names.value_list.groups.unwrap_or_default();
            let mut sources = Vec::new();
            let mut position = 0;
            for index in 0_u8..4 {
                if u8::from(names.group_names) & (1 << index) == 0 {
                    continue;
                }
                let atom = *groups
                    .get(position)
                    .ok_or_else(|| protocol("invalid group names"))?;
                position += 1;
                let label = self
                    .connection
                    .get_atom_name(atom)
                    .map_err(protocol)?
                    .reply()
                    .map_err(protocol)?
                    .name;
                sources.push(Source {
                    id: index.to_string(),
                    label: String::from_utf8_lossy(&label).into(),
                    enabled: true,
                });
            }
            let group = xkb::get_state(&self.connection, 256)
                .map_err(protocol)?
                .reply()
                .map_err(protocol)?
                .group;
            Ok(SourceCatalog {
                provider: "x11".into(),
                generation: format!("{}:{map:?}", self.epoch),
                sources,
                current: Some(u8::from(group).to_string()),
            })
        })()
        .and_then(|value| {
            super::operation::remaining()?;
            Ok(value)
        });
        if result.is_err() {
            self.connection.stream().invalidate();
        }
        result
    }
    fn select(&mut self, old: &SourceCatalog, id: &str) -> Result<(), DesktopError> {
        let _operation = super::operation::adapter_scope();
        let result = (|| {
            self.connection.stream().check().map_err(protocol)?;
            old.check(&self.sources()?, id)?;
            let group = id.parse::<u8>().map_err(protocol)?;
            xkb::latch_lock_state(
                &self.connection,
                256,
                0_u16.into(),
                0_u16.into(),
                true,
                group.into(),
                0_u16.into(),
                false,
                0,
            )
            .map_err(protocol)?
            .check()
            .map_err(protocol)?;
            if self.sources()?.current.as_deref() != Some(id) {
                return Err(DesktopError::SelectionFailed);
            }
            Ok(())
        })()
        .and_then(|value| {
            super::operation::remaining()?;
            Ok(value)
        });
        if result.is_err() {
            self.connection.stream().invalidate();
        }
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn queued_selection_cancellation_cannot_flush_after_reset() {
        use std::io::{Read, Write};
        use x11rb::connection::RequestConnection;
        let (socket, mut peer) = UnixStream::pair().unwrap();
        peer.set_read_timeout(Some(std::time::Duration::from_secs(1)))
            .unwrap();
        let server = std::thread::spawn(move || {
            let mut header = [0; 4];
            peer.read_exact(&mut header).unwrap();
            assert_eq!(header[0], 98); // QueryExtension only, never a selection.
            let len = usize::from(u16::from_ne_bytes([header[2], header[3]])) * 4;
            let mut rest = vec![0; len - 4];
            peer.read_exact(&mut rest).unwrap();
            let mut reply = [0; 32];
            reply[0] = 1;
            reply[2..4].copy_from_slice(&1_u16.to_ne_bytes());
            reply[8] = 1;
            reply[9] = 135;
            peer.write_all(&reply).unwrap();
            let mut bytes = [0; 128];
            assert_eq!(
                peer.read(&mut bytes).unwrap(),
                0,
                "abandoned selection flushed"
            );
        });
        let (stream, _) = DefaultStream::from_unix_stream(socket).unwrap();
        let stream = TimedStream {
            stream,
            deadline: Mutex::new(Instant::now() + IPC_TIMEOUT),
            poisoned: AtomicBool::new(false),
        };
        let connection = RustConnection::for_connected_stream(
            stream,
            xproto::Setup {
                resource_id_mask: 0xff,
                maximum_request_length: u16::MAX,
                ..Default::default()
            },
        )
        .unwrap();
        connection
            .extension_information(xkb::X11_EXTENSION_NAME)
            .unwrap()
            .unwrap();
        let stopped = std::sync::Arc::new(AtomicBool::new(false));
        let result = super::super::operation::run(stopped.clone(), || {
            let cookie = xkb::latch_lock_state(
                &connection,
                256,
                0_u16.into(),
                0_u16.into(),
                true,
                1_u8.into(),
                0_u16.into(),
                false,
                0,
            )
            .unwrap();
            stopped.store(true, Ordering::Release);
            cookie.check().map_err(protocol)
        });
        assert!(result.is_err());
        assert!(connection.stream().poisoned.load(Ordering::Acquire));
        connection.stream().reset();
        assert!(connection.flush().is_err());
        server.join().unwrap();
    }
    #[test]
    fn exact_class_not_instance() {
        assert_eq!(wm_class(b"instance\0Class\0").unwrap().value, "Class");
        assert!(wm_class(b"title").is_none());
    }
    #[test]
    fn latin1_class() {
        assert_eq!(wm_class(b"x\0\xe9\0").unwrap().value, "é");
    }
}
