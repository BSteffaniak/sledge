//! macOS restart-safe IPC ownership. The lock file is persistent: unlinking it
//! would allow another process to lock a different inode for the same socket.
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};

pub struct SocketGuard {
    path: PathBuf,
    identity: (u64, u64),
    _lock: File,
}

impl SocketGuard {
    pub fn bind(path: &Path) -> io::Result<(UnixListener, Self)> {
        // Canonicalize the parent, not the endpoint (which must not be followed).
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent = fs::canonicalize(parent)?;
        let parent_metadata = fs::metadata(&parent)?;
        // SAFETY: geteuid has no pointer arguments.
        let uid = unsafe { libc::geteuid() };
        let private = parent_metadata.uid() == uid && parent_metadata.mode() & 0o022 == 0;
        let sticky_root = parent_metadata.uid() == 0 && parent_metadata.mode() & 0o1000 != 0;
        if !private && !sticky_root {
            return Err(io::Error::other(
                "IPC directory must be same-user and non-writable by others, or root-owned and sticky",
            ));
        }
        let name = path
            .file_name()
            .ok_or_else(|| io::Error::other("socket name missing"))?;
        let path = parent.join(name);
        let mut lock_name = name.to_os_string();
        lock_name.push(".lock");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(parent.join(lock_name))?;
        let metadata = lock.metadata()?;
        // SAFETY: geteuid has no pointer arguments.
        let uid = unsafe { libc::geteuid() };
        if !metadata.is_file()
            || metadata.uid() != uid
            || metadata.mode() & 0o077 != 0
            || metadata.nlink() != 1
        {
            return Err(io::Error::other(
                "IPC lock must be a private same-user regular file",
            ));
        }
        // SAFETY: lock owns a valid fd, held until the socket is cleaned up.
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(io::Error::last_os_error());
        }
        recover(&path, uid)?;
        let listener = UnixListener::bind(&path)?;
        let metadata = fs::symlink_metadata(&path)?;
        let guard = Self {
            path,
            identity: (metadata.dev(), metadata.ino()),
            _lock: lock,
        };
        fs::set_permissions(&guard.path, fs::Permissions::from_mode(0o600))?;
        Ok((listener, guard))
    }
}

fn recover(path: &Path, uid: u32) -> io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if !metadata.file_type().is_socket() || metadata.uid() != uid {
        return Err(io::Error::other(
            "refusing to replace an IPC path that is not a same-user socket",
        ));
    }
    // Legacy daemons do not hold our lock. Refuse a live listener even when
    // it does not speak our protocol; only ECONNREFUSED proves abandonment.
    let probe = socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None)?;
    let address = socket2::SockAddr::unix(path)?;
    match probe.connect_timeout(&address, std::time::Duration::from_millis(250)) {
        Err(error) if error.raw_os_error() == Some(libc::ECONNREFUSED) => {}
        Err(error) => return Err(error),
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                "IPC listener is already running",
            ));
        }
    }
    let current = fs::symlink_metadata(path)?;
    if (current.dev(), current.ino()) != (metadata.dev(), metadata.ino()) {
        return Err(io::Error::other("IPC socket changed during recovery"));
    }
    fs::remove_file(path)
}

impl Drop for SocketGuard {
    fn drop(&mut self) {
        if let Ok(metadata) = fs::symlink_metadata(&self.path) {
            if metadata.file_type().is_socket() && (metadata.dev(), metadata.ino()) == self.identity
            {
                let _ = fs::remove_file(&self.path);
            }
        }
    }
}

#[cfg(test)]
mod tests;
