//! Passive peer listener. Private same-user directory, no unlink-on-bind.
use super::transport::{Peer, Transport};
use nix::fcntl::{AtFlags, OFlag, open};
use nix::sys::socket::{
    AddressFamily, Backlog, SockFlag, SockType, UnixAddr, accept4, bind, listen, socket,
};
use nix::sys::stat::{Mode, fstat, fstatat};
use nix::unistd::{UnlinkatFlags, unlinkat};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// One bounded authenticated accept; endpoint cleanup is inode/directory pinned.
pub struct Listener {
    transport: Transport,
    directory: OwnedFd,
    name: PathBuf,
    device: u64,
    inode: u64,
    uid: u32,
}
fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "passive endpoint must be private and same-user",
    )
}
impl Listener {
    /// Bind an explicit absolute endpoint. Never removes an existing path.
    /// # Errors
    /// Rejects symlink/nonprivate parent, unsafe name and existing endpoint.
    pub fn bind(path: &Path) -> io::Result<Self> {
        if !path.is_absolute() {
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
        // SAFETY: new owned descriptor from open, transferred once.
        let directory = unsafe { OwnedFd::from_raw_fd(raw) };
        let stat = fstat(directory.as_raw_fd()).map_err(io::Error::from)?;
        let uid = nix::unistd::geteuid().as_raw();
        if stat.st_uid != uid || stat.st_mode & 0o077 != 0 {
            return Err(denied());
        }
        let anchored = PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd())).join(name);
        let fd = socket(
            AddressFamily::Unix,
            SockType::SeqPacket,
            SockFlag::SOCK_CLOEXEC | SockFlag::SOCK_NONBLOCK,
            None,
        )
        .map_err(io::Error::from)?;
        bind(
            fd.as_raw_fd(),
            &UnixAddr::new(&anchored).map_err(io::Error::from)?,
        )
        .map_err(io::Error::from)?;
        let stat = fstatat(
            Some(directory.as_raw_fd()),
            Path::new(name),
            AtFlags::AT_SYMLINK_NOFOLLOW,
        )
        .map_err(io::Error::from)?;
        let listener = Self {
            transport: Transport::from_owned_fd(fd),
            directory,
            name: name.into(),
            device: stat.st_dev,
            inode: stat.st_ino,
            uid,
        };
        std::fs::set_permissions(&anchored, std::fs::Permissions::from_mode(0o600))?;
        listen(
            listener.transport.fd(),
            Backlog::new(1).map_err(io::Error::from)?,
        )
        .map_err(io::Error::from)?;
        Ok(listener)
    }
    /// Wait for one peer with the specified compositor PID/UID. Authentication
    /// failure terminates this admission; never loops accepting other peers.
    /// # Errors
    /// Reports aggregate deadline, wrong credentials and accept failures.
    fn verify_endpoint(&self) -> io::Result<()> {
        let parent = fstat(self.directory.as_raw_fd()).map_err(io::Error::from)?;
        let endpoint = fstatat(
            Some(self.directory.as_raw_fd()),
            self.name.as_path(),
            AtFlags::AT_SYMLINK_NOFOLLOW,
        )
        .map_err(io::Error::from)?;
        if parent.st_uid != self.uid
            || parent.st_mode & 0o077 != 0
            || endpoint.st_uid != self.uid
            || endpoint.st_mode & 0o077 != 0
            || endpoint.st_dev != self.device
            || endpoint.st_ino != self.inode
        {
            return Err(denied());
        }
        Ok(())
    }
    pub fn accept(&self, peer: Peer, deadline_ns: u64) -> io::Result<Transport> {
        self.verify_endpoint()?;
        self.transport.wait_readable(deadline_ns)?;
        self.verify_endpoint()?;
        let raw = accept4(
            self.transport.fd().as_raw_fd(),
            SockFlag::SOCK_CLOEXEC | SockFlag::SOCK_NONBLOCK,
        )
        .map_err(io::Error::from)?;
        // SAFETY: accept4 returned a new owned descriptor, transferred once.
        let transport = Transport::from_owned_fd(unsafe { OwnedFd::from_raw_fd(raw) });
        transport.authenticate(peer)?;
        self.verify_endpoint()?;
        if super::transport::monotonic_ns()? >= deadline_ns {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "passive accept deadline expired",
            ));
        }
        Ok(transport)
    }
}
impl Drop for Listener {
    fn drop(&mut self) {
        if let Ok(stat) = fstatat(
            Some(self.directory.as_raw_fd()),
            self.name.as_path(),
            AtFlags::AT_SYMLINK_NOFOLLOW,
        ) && stat.st_dev == self.device
            && stat.st_ino == self.inode
            && stat.st_uid == self.uid
        {
            let _ = unlinkat(
                Some(self.directory.as_raw_fd()),
                self.name.as_path(),
                UnlinkatFlags::NoRemoveDir,
            );
        }
    }
}
