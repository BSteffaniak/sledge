//! Same-user, private-endpoint SOCK_SEQPACKET transport; worker-side only.
use super::wire::{PACKET_SIZE, Packet};
use nix::fcntl::{OFlag, open};
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use nix::sys::socket::{
    AddressFamily, MsgFlags, SockFlag, SockType, UnixAddr, connect, getsockopt, recv, send, socket,
    sockopt,
};
use nix::sys::stat::{Mode, SFlag, fstat};
use std::io;
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd};
use std::path::Path;

/// Expected compositor process identity. UID alone is not artifact qualification.
#[derive(Clone, Copy, Debug)]
pub struct Peer {
    pub uid: u32,
    pub pid: i32,
}

pub struct Transport {
    fd: OwnedFd,
}

/// CLOCK_MONOTONIC in the same domain as the frozen native deadlines.
/// # Errors
/// Reports clock failure or overflow.
pub fn monotonic_ns() -> io::Result<u64> {
    let mut t = nix::libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: valid writable timespec; CLOCK_MONOTONIC requires no privileges.
    if unsafe { nix::libc::clock_gettime(nix::libc::CLOCK_MONOTONIC, &raw mut t) } != 0 {
        return Err(io::Error::last_os_error());
    }
    u64::try_from(t.tv_sec)
        .ok()
        .and_then(|s| s.checked_mul(1_000_000_000))
        .and_then(|s| u64::try_from(t.tv_nsec).ok().and_then(|n| s.checked_add(n)))
        .ok_or_else(super::wire::invalid)
}
fn timeout() -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        "Hyprland aggregate deadline expired",
    )
}
fn disconnected() -> io::Error {
    io::Error::new(io::ErrorKind::ConnectionReset, "Hyprland peer disconnected")
}
fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "Hyprland endpoint/peer identity rejected",
    )
}

impl Transport {
    /// Connect without following the final parent or endpoint symlink. The open
    /// parent anchors lookup even if its pathname is replaced. No discovery,
    /// desktop probing, socket creation on disk, or fallback endpoint.
    /// # Errors
    /// Rejects non-private parents, non-sockets, wrong peers and missed deadlines.
    pub fn connect(path: &Path, peer: Peer, deadline_ns: u64) -> io::Result<Self> {
        if !path.is_absolute() || peer.uid != nix::unistd::geteuid().as_raw() || peer.pid <= 0 {
            return Err(denied());
        }
        let parent = path.parent().ok_or_else(denied)?;
        let name = path.file_name().ok_or_else(denied)?;
        let raw = open(
            parent,
            OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
            Mode::empty(),
        )
        .map_err(io::Error::from)?;
        // SAFETY: open returned a newly owned descriptor, transferred once.
        let directory = unsafe { OwnedFd::from_raw_fd(raw) };
        let stat = fstat(directory.as_raw_fd()).map_err(io::Error::from)?;
        if stat.st_uid != peer.uid || stat.st_mode & 0o077 != 0 {
            return Err(denied());
        }
        let anchored =
            std::path::PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd())).join(name);
        let metadata = std::fs::symlink_metadata(&anchored)?;
        use std::os::unix::fs::MetadataExt;
        if metadata.uid() != peer.uid
            || metadata.mode() & 0o077 != 0
            || SFlag::from_bits_truncate(metadata.mode()) != SFlag::S_IFSOCK
        {
            return Err(denied());
        }
        let fd = socket(
            AddressFamily::Unix,
            SockType::SeqPacket,
            SockFlag::SOCK_CLOEXEC | SockFlag::SOCK_NONBLOCK,
            None,
        )
        .map_err(io::Error::from)?;
        let transport = Self { fd };
        match connect(
            transport.fd.as_raw_fd(),
            &UnixAddr::new(&anchored).map_err(io::Error::from)?,
        ) {
            Ok(()) => {}
            Err(nix::errno::Errno::EINPROGRESS) => {
                // Error/hangup readiness also completes a failed connect. Read
                // SO_ERROR before interpreting readiness, preserving errno.
                let readiness = transport.wait_ready(PollFlags::POLLOUT, deadline_ns)?;
                let error =
                    getsockopt(&transport.fd, sockopt::SocketError).map_err(io::Error::from)?;
                if error != 0 {
                    return Err(io::Error::from_raw_os_error(error));
                }
                if !readiness.contains(PollFlags::POLLOUT)
                    || readiness.intersects(PollFlags::POLLHUP | PollFlags::POLLNVAL)
                {
                    return Err(disconnected());
                }
            }
            Err(e) => return Err(e.into()),
        }
        transport.authenticate(peer)?;
        if monotonic_ns()? >= deadline_ns {
            return Err(timeout());
        }
        Ok(transport)
    }
    pub(super) fn authenticate(&self, peer: Peer) -> io::Result<()> {
        let credentials =
            getsockopt(&self.fd, sockopt::PeerCredentials).map_err(io::Error::from)?;
        if peer.uid != nix::unistd::geteuid().as_raw()
            || credentials.uid() != peer.uid
            || credentials.pid() != peer.pid
            || peer.pid <= 0
        {
            return Err(denied());
        }
        Ok(())
    }
    fn wait(&self, flags: PollFlags, deadline_ns: u64) -> io::Result<()> {
        let readiness = self.wait_ready(flags, deadline_ns)?;
        if readiness.contains(flags) {
            Ok(())
        } else {
            Err(disconnected())
        }
    }
    fn wait_ready(&self, flags: PollFlags, deadline_ns: u64) -> io::Result<PollFlags> {
        loop {
            let left = deadline_ns
                .checked_sub(monotonic_ns()?)
                .filter(|n| *n > 0)
                .ok_or_else(timeout)?;
            let millis = left.div_ceil(1_000_000).min(u64::from(u16::MAX));
            let mut descriptors = [PollFd::new(self.fd.as_fd(), flags)];
            match poll(
                &mut descriptors,
                PollTimeout::from(u16::try_from(millis).map_err(|_| timeout())?),
            ) {
                Ok(0) => {}
                Ok(_) => {
                    if monotonic_ns()? >= deadline_ns {
                        return Err(timeout());
                    }
                    return Ok(descriptors[0].revents().unwrap_or_else(PollFlags::empty));
                }
                Err(nix::errno::Errno::EINTR) => {}
                Err(e) => return Err(e.into()),
            }
        }
    }
    /// Bounded idle receive. Only expiry while waiting for readiness is idle;
    /// once ready, any receive/decode/deadline error remains a protocol failure.
    /// # Errors
    /// Reports disconnect, malformed packets and errors after readiness.
    pub fn receive_if_ready(&self, deadline_ns: u64) -> io::Result<Option<Packet>> {
        match self.wait(PollFlags::POLLIN, deadline_ns) {
            Ok(()) => self.receive(deadline_ns).map(Some),
            Err(error) if error.kind() == io::ErrorKind::TimedOut => Ok(None),
            Err(error) => Err(error),
        }
    }
    /// Receive exactly one packet. MSG_TRUNC exposes oversized datagrams.
    /// # Errors
    /// Rejects short/oversized/malformed packets, disconnect and deadline expiry.
    pub fn receive(&self, deadline_ns: u64) -> io::Result<Packet> {
        loop {
            self.wait(PollFlags::POLLIN, deadline_ns)?;
            let mut bytes = [0; PACKET_SIZE];
            match recv(
                self.fd.as_raw_fd(),
                &mut bytes,
                MsgFlags::MSG_DONTWAIT | MsgFlags::MSG_TRUNC,
            ) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "Hyprland peer disconnected",
                    ));
                }
                Ok(PACKET_SIZE) => {
                    if monotonic_ns()? >= deadline_ns {
                        return Err(timeout());
                    }
                    return Packet::decode(&bytes);
                }
                Ok(_) => return Err(super::wire::invalid()),
                Err(nix::errno::Errno::EAGAIN | nix::errno::Errno::EINTR) => {}
                Err(e) => return Err(e.into()),
            }
        }
    }
    /// Send atomically before the aggregate deadline; never retries a partial write.
    /// # Errors
    /// Reports encoding errors, disconnect and deadline expiry.
    pub fn send(&self, packet: &Packet, deadline_ns: u64) -> io::Result<()> {
        let bytes = packet.encode()?;
        loop {
            self.wait(PollFlags::POLLOUT, deadline_ns)?;
            match send(
                self.fd.as_raw_fd(),
                &bytes,
                MsgFlags::MSG_DONTWAIT | MsgFlags::MSG_NOSIGNAL,
            ) {
                Ok(PACKET_SIZE) => {
                    if monotonic_ns()? >= deadline_ns {
                        return Err(timeout());
                    }
                    return Ok(());
                }
                Ok(_) => return Err(super::wire::invalid()),
                Err(nix::errno::Errno::EAGAIN | nix::errno::Errno::EINTR) => {}
                Err(e) => return Err(e.into()),
            }
        }
    }
    pub(super) fn from_owned_fd(fd: OwnedFd) -> Self {
        Self { fd }
    }
    pub(super) const fn fd(&self) -> &OwnedFd {
        &self.fd
    }
    pub(super) fn wait_readable(&self, deadline_ns: u64) -> io::Result<()> {
        self.wait(PollFlags::POLLIN, deadline_ns)
    }
    #[cfg(test)]
    pub(super) fn from_fd(fd: OwnedFd) -> Self {
        Self { fd }
    }
}
