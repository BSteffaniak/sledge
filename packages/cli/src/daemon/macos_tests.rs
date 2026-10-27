//! Subprocess tests exercise real signal delivery and kernel lock release,
//! without installing a keyboard tap or requesting TCC access.
use super::*;
use std::fs;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

struct Worker {
    child: Child,
    directory: PathBuf,
}
impl Worker {
    fn start(label: &str) -> Self {
        let directory =
            std::env::temp_dir().join(format!("sledge-lifecycle-{}-{label}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "daemon::macos_tests::socket_process"])
            .env("SLEDGE_TEST_SOCKET_DIRECTORY", &directory)
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let mut worker = Self { child, directory };
        let deadline = Instant::now() + Duration::from_secs(10);
        while !worker.directory.join("ready").exists() {
            assert!(
                worker.child.try_wait().unwrap().is_none(),
                "worker exited before readiness"
            );
            assert!(Instant::now() < deadline, "worker startup timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
        worker
    }
    fn socket(&self) -> PathBuf {
        self.directory.join("socket")
    }
    fn wait(&mut self) -> std::process::ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            assert!(Instant::now() < deadline, "worker shutdown timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        fs::remove_dir_all(&self.directory).unwrap();
    }
}

#[test]
fn socket_process() {
    let Some(directory) = std::env::var_os("SLEDGE_TEST_SOCKET_DIRECTORY") else {
        return;
    };
    let directory = PathBuf::from(directory);
    let shutdown = Arc::new(AtomicBool::new(false));
    install_signal_handler(Arc::new(|| Ok(())), shutdown.clone()).unwrap();
    let (_listener, _guard) = ipc::macos::SocketGuard::bind(&directory.join("socket")).unwrap();
    fs::write(directory.join("ready"), b"ready").unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !shutdown.load(Ordering::Relaxed) {
        assert!(Instant::now() < deadline, "worker was not stopped");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn sigterm_releases_owned_socket() {
    let mut worker = Worker::start("sigterm");
    assert!(ipc::macos::SocketGuard::bind(&worker.socket()).is_err());
    // SAFETY: the pid belongs to the live child owned by this test.
    assert_eq!(
        unsafe { libc::kill(i32::try_from(worker.child.id()).unwrap(), libc::SIGTERM) },
        0
    );
    assert!(worker.wait().success());
    assert!(!worker.socket().exists());
    let _replacement = ipc::macos::SocketGuard::bind(&worker.socket()).unwrap();
}

#[test]
fn forced_exit_recovers_socket_and_kernel_lock() {
    let mut worker = Worker::start("sigkill");
    assert!(ipc::macos::SocketGuard::bind(&worker.socket()).is_err());
    worker.child.kill().unwrap();
    assert!(!worker.wait().success());
    assert!(worker.socket().exists());
    let (listener, guard) = ipc::macos::SocketGuard::bind(&worker.socket()).unwrap();
    drop(listener);
    drop(guard);
    assert!(!worker.socket().exists());
}
