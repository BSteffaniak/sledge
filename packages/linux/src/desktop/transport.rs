use super::{DesktopError, IPC_TIMEOUT, MAX_MESSAGE, protocol};
use serde_json::Value;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use std::time::Instant;

struct DeadlineStream {
    socket: UnixStream,
    deadline: Instant,
}
impl Read for DeadlineStream {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        loop {
            let left = super::operation::remaining()
                .map_err(std::io::Error::other)?
                .min(self.deadline.saturating_duration_since(Instant::now()));
            if left.is_zero() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "desktop IPC deadline",
                ));
            }
            self.socket
                .set_read_timeout(Some(left.min(super::operation::SLICE)))?;
            match self.socket.read(bytes) {
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) => {}
                result => return result,
            }
        }
    }
}
impl Write for DeadlineStream {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        loop {
            let left = super::operation::remaining()
                .map_err(std::io::Error::other)?
                .min(self.deadline.saturating_duration_since(Instant::now()));
            if left.is_zero() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "desktop IPC deadline",
                ));
            }
            self.socket
                .set_write_timeout(Some(left.min(super::operation::SLICE)))?;
            match self.socket.write(bytes) {
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) => {}
                result => return result,
            }
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.socket.flush()
    }
}

use nix::poll::{PollFd, PollFlags, poll};
use nix::sys::socket::{
    AddressFamily, SockFlag, SockType, UnixAddr, connect, getsockopt, socket, sockopt,
};
use std::os::fd::{AsFd, AsRawFd};
use std::os::unix::fs::MetadataExt;

pub(crate) struct SocketTransport {
    pub path: PathBuf,
}
impl SocketTransport {
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_owned(),
        }
    }
    fn connect(&self) -> Result<DeadlineStream, DesktopError> {
        super::operation::remaining()?;
        // Paths are selected by the unprivileged caller, never sent to the broker.
        let deadline = Instant::now() + IPC_TIMEOUT;
        let fd = socket(
            AddressFamily::Unix,
            SockType::Stream,
            SockFlag::SOCK_CLOEXEC | SockFlag::SOCK_NONBLOCK,
            None,
        )
        .map_err(protocol)?;
        let addr = UnixAddr::new(&self.path).map_err(protocol)?;
        match connect(fd.as_raw_fd(), &addr) {
            Ok(()) => {}
            Err(nix::errno::Errno::EINPROGRESS) => {
                let mut fds = [PollFd::new(fd.as_fd(), PollFlags::POLLOUT)];
                while poll(&mut fds, 10_u16).map_err(protocol)? == 0 {
                    super::operation::remaining()?;
                    if Instant::now() >= deadline {
                        return Err(protocol("connect deadline"));
                    }
                }
                let error = getsockopt(&fd, sockopt::SocketError).map_err(protocol)?;
                if error != 0 {
                    return Err(std::io::Error::from_raw_os_error(error).into());
                }
            }
            Err(error) => return Err(protocol(error)),
        }
        let peer = getsockopt(&fd, sockopt::PeerCredentials).map_err(protocol)?;
        if peer.uid() != nix::unistd::geteuid().as_raw() {
            return Err(super::unavailable("desktop socket has different UID"));
        }
        let socket = UnixStream::from(fd);
        socket.set_nonblocking(false)?;
        Ok(DeadlineStream { socket, deadline })
    }
    pub fn epoch(&self) -> Result<String, DesktopError> {
        let metadata = std::fs::metadata(&self.path)?;
        if metadata.uid() != nix::unistd::geteuid().as_raw() {
            return Err(super::unavailable("desktop socket owner differs"));
        }
        Ok(format!(
            "{}:{}:{}:{}",
            metadata.dev(),
            metadata.ino(),
            metadata.ctime(),
            metadata.ctime_nsec()
        ))
    }
    pub fn sway(&self, kind: u32, payload: &str) -> Result<Value, DesktopError> {
        if payload.len() > MAX_MESSAGE {
            return Err(protocol("request too large"));
        }
        let mut socket = self.connect()?;
        let length = u32::try_from(payload.len()).map_err(protocol)?;
        socket.write_all(b"i3-ipc")?;
        socket.write_all(&length.to_le_bytes())?;
        socket.write_all(&kind.to_le_bytes())?;
        socket.write_all(payload.as_bytes())?;
        let mut header = [0; 14];
        socket.read_exact(&mut header)?;
        if &header[..6] != b"i3-ipc" || header[10..14] != kind.to_le_bytes() {
            return Err(protocol("invalid IPC header"));
        }
        let length = u32::from_le_bytes(header[6..10].try_into().map_err(protocol)?) as usize;
        if length > MAX_MESSAGE {
            return Err(protocol("reply too large"));
        }
        let mut body = vec![0; length];
        socket.read_exact(&mut body)?;
        serde_json::from_slice(&body).map_err(protocol)
    }
    pub fn hypr(&self, request: &str) -> Result<String, DesktopError> {
        let mut socket = self.connect()?;
        socket.write_all(request.as_bytes())?;
        let mut body = Vec::new();
        (&mut socket)
            .take((MAX_MESSAGE + 1) as u64)
            .read_to_end(&mut body)?;
        if body.len() > MAX_MESSAGE {
            return Err(protocol("reply too large"));
        }
        // Drop connection immediately, including on parsing errors.
        String::from_utf8(body).map_err(protocol)
    }
    pub fn hypr_json(&self, request: &str) -> Result<Value, DesktopError> {
        serde_json::from_str(&self.hypr(request)?).map_err(protocol)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;
    #[test]
    fn blocked_socket_is_cancelled_and_joined_without_followup_requests() {
        let path =
            std::env::temp_dir().join(format!("sledge-desktop-cancel-{}", std::process::id()));
        let listener = UnixListener::bind(&path).unwrap();
        let stopped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cancel = stopped.clone();
        let (release, released) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 14];
            socket.read_exact(&mut request).unwrap();
            cancel.store(true, std::sync::atomic::Ordering::Release);
            released
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap();
            let mut byte = [0];
            assert_eq!(socket.read(&mut byte).unwrap(), 0);
        });
        let started = Instant::now();
        let result: Result<(), DesktopError> = super::super::operation::run(stopped, || {
            SocketTransport::new(&path).sway(4, "")?;
            panic!("cancelled reply must not permit further calls");
        });
        assert!(result.is_err());
        assert!(started.elapsed() < IPC_TIMEOUT);
        release.send(()).unwrap();
        server.join().unwrap();
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn rejects_oversized_frame_before_allocation() {
        let path =
            std::env::temp_dir().join(format!("sledge-desktop-frame-{}", std::process::id()));
        let listener = UnixListener::bind(&path).unwrap();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 14];
            stream.read_exact(&mut request).unwrap();
            stream.write_all(b"i3-ipc").unwrap();
            stream.write_all(&u32::MAX.to_le_bytes()).unwrap();
            stream.write_all(&4_u32.to_le_bytes()).unwrap();
        });
        assert!(matches!(
            SocketTransport::new(&path).sway(4, ""),
            Err(DesktopError::Protocol(_))
        ));
        worker.join().unwrap();
        std::fs::remove_file(path).unwrap();
    }
}
