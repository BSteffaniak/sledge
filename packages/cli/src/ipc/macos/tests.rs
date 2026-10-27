use super::*;
use std::os::unix::fs::symlink;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "sledge-macos-ipc-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn socket(&self) -> PathBuf {
        self.0.join("daemon.sock")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn stale_legacy_socket_is_recovered_and_cleaned_on_restart() {
    let fixture = Fixture::new();
    let path = fixture.socket();
    drop(UnixListener::bind(&path).unwrap());
    for _ in 0..3 {
        let (listener, guard) = SocketGuard::bind(&path).unwrap();
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        assert!(UnixStream::connect(&path).is_ok());
        assert!(SocketGuard::bind(&path).is_err());
        drop(listener);
        // Keep the lock until cleanup, even after the listener closes.
        assert!(SocketGuard::bind(&path).is_err());
        drop(guard);
        assert!(!path.exists());
    }
}

#[test]
fn live_legacy_socket_is_not_unlinked() {
    let fixture = Fixture::new();
    let path = fixture.socket();
    let listener = UnixListener::bind(&path).unwrap();
    let inode = fs::symlink_metadata(&path).unwrap().ino();
    assert!(SocketGuard::bind(&path).is_err());
    assert_eq!(fs::symlink_metadata(&path).unwrap().ino(), inode);
    assert!(UnixStream::connect(&path).is_ok());
    drop(listener);
}

#[test]
fn regular_files_and_symlinks_are_preserved() {
    let fixture = Fixture::new();
    let path = fixture.socket();
    fs::write(&path, b"keep").unwrap();
    assert!(SocketGuard::bind(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"keep");
    fs::remove_file(&path).unwrap();
    let target = fixture.0.join("target");
    drop(UnixListener::bind(&target).unwrap());
    symlink(&target, &path).unwrap();
    assert!(SocketGuard::bind(&path).is_err());
    assert!(
        fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(target.exists());
}

#[test]
fn cleanup_does_not_remove_substituted_endpoint() {
    let fixture = Fixture::new();
    let path = fixture.socket();
    let (listener, guard) = SocketGuard::bind(&path).unwrap();
    fs::remove_file(&path).unwrap();
    fs::write(&path, b"replacement").unwrap();
    drop(listener);
    drop(guard);
    assert_eq!(fs::read(&path).unwrap(), b"replacement");
}

#[test]
fn unsafe_lock_paths_are_rejected() {
    let fixture = Fixture::new();
    let path = fixture.socket();
    let lock = fixture.0.join("daemon.sock.lock");
    let target = fixture.0.join("target");
    fs::write(&target, b"keep").unwrap();
    symlink(&target, &lock).unwrap();
    assert!(SocketGuard::bind(&path).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"keep");
    fs::remove_file(&lock).unwrap();
    fs::write(&lock, b"public").unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&lock, fs::Permissions::from_mode(0o666)).unwrap();
    assert!(SocketGuard::bind(&path).is_err());
}
