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
        let left = self.deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "desktop IPC deadline",
            ));
        }
        self.socket.set_read_timeout(Some(left))?;
        self.socket.read(bytes)
    }
}
impl Write for DeadlineStream {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let left = self.deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "desktop IPC deadline",
            ));
        }
        self.socket.set_write_timeout(Some(left))?;
        self.socket.write(bytes)
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
                if poll(&mut fds, 400_u16).map_err(protocol)? == 0 {
                    return Err(protocol("connect deadline"));
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
