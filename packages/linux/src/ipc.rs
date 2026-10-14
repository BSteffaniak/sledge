//! Conservative same-user IPC binding; never unlink an existing path.
mod discovery;
pub use discovery::client_socket_path;
use nix::fcntl::{AtFlags, OFlag, open};
use nix::sys::stat::{Mode, fstat, fstatat};
use nix::unistd::{UnlinkatFlags, unlinkat};
use std::fs;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};

/// Resolve the same-user runtime socket without creating directories or opening devices.
/// No /tmp fallback: an absent or insecure runtime directory is an explicit error.
pub fn default_socket_path() -> io::Result<PathBuf> {
    let parent = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| denied("XDG_RUNTIME_DIR is unavailable"))?;
    if !parent.is_absolute() {
        return Err(denied("XDG_RUNTIME_DIR must be absolute"));
    }
    let raw = open(
        &parent,
        OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(io::Error::from)?;
    // SAFETY: open returned a new descriptor transferred exactly once.
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    let metadata = fstat(fd.as_raw_fd()).map_err(io::Error::from)?;
    if metadata.st_uid != nix::unistd::geteuid().as_raw() || metadata.st_mode & 0o077 != 0 {
        return Err(denied("XDG_RUNTIME_DIR is not private and same-user"));
    }
    Ok(parent.join("sledge.sock"))
}

/// Holds an open directory fd so cleanup cannot follow a substituted parent path.
pub struct SocketGuard {
    directory: OwnedFd,
    name: PathBuf,
    device: u64,
    inode: u64,
    uid: u32,
}
impl SocketGuard {
    /// Parent must already be private and owned by this user. Bind fails on every
    /// pre-existing path, including sockets and symlinks. No process-wide umask changes.
    pub fn bind(path: &Path) -> io::Result<(UnixListener, Self)> {
        if !path.is_absolute() {
            return Err(denied("IPC path must be absolute"));
        }
        let parent = path
            .parent()
            .ok_or_else(|| denied("socket has no parent"))?;
        let name = path
            .file_name()
            .ok_or_else(|| denied("socket has no name"))?;
        let raw_fd = open(
            parent,
            OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
            Mode::empty(),
        )
        .map_err(io::Error::from)?;
        // SAFETY: open returned a new owned descriptor, transferred exactly once.
        let directory = unsafe { OwnedFd::from_raw_fd(raw_fd) };
        let metadata = fstat(directory.as_raw_fd()).map_err(io::Error::from)?;
        let uid = nix::unistd::geteuid().as_raw();
        if metadata.st_uid != uid || metadata.st_mode & 0o077 != 0 {
            return Err(denied(
                "socket parent must be a private same-user directory",
            ));
        }
        let anchored = PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd())).join(name);
        match fs::symlink_metadata(&anchored) {
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "refusing existing IPC path",
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let listener = UnixListener::bind(&anchored)?;
        let metadata = fstatat(
            Some(directory.as_raw_fd()),
            Path::new(name),
            AtFlags::AT_SYMLINK_NOFOLLOW,
        )
        .map_err(io::Error::from)?;
        let guard = Self {
            directory,
            name: PathBuf::from(name),
            device: metadata.st_dev,
            inode: metadata.st_ino,
            uid,
        };
        fs::set_permissions(&anchored, fs::Permissions::from_mode(0o600))?;
        Ok((listener, guard))
    }
}
impl Drop for SocketGuard {
    fn drop(&mut self) {
        if let Ok(metadata) = fstatat(
            Some(self.directory.as_raw_fd()),
            &self.name,
            AtFlags::AT_SYMLINK_NOFOLLOW,
        ) {
            if metadata.st_dev == self.device
                && metadata.st_ino == self.inode
                && metadata.st_uid == self.uid
            {
                let _ = unlinkat(
                    Some(self.directory.as_raw_fd()),
                    &self.name,
                    UnlinkatFlags::NoRemoveDir,
                );
            }
        }
    }
}
fn denied(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    #[test]
    fn private_socket_and_identity_cleanup() {
        let directory =
            std::env::temp_dir().join(format!("sledge-ipc-test-{}", std::process::id()));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .unwrap();
        let path = directory.join("socket");
        let (listener, guard) = SocketGuard::bind(&path).unwrap();
        assert_eq!(fs::symlink_metadata(&path).unwrap().mode() & 0o777, 0o600);
        assert!(SocketGuard::bind(&path).is_err());
        let _client = std::os::unix::net::UnixStream::connect(&path).unwrap();
        let _peer = listener.accept().unwrap();
        // Substitute a file: Drop must leave it alone.
        fs::remove_file(&path).unwrap();
        fs::write(&path, "replacement").unwrap();
        drop(guard);
        assert_eq!(fs::read_to_string(&path).unwrap(), "replacement");
        fs::remove_file(&path).unwrap();
        drop(listener);
        let (_, guard) = SocketGuard::bind(&path).unwrap();
        drop(guard);
        assert!(!path.exists());
        fs::remove_dir(directory).unwrap();
    }
}
