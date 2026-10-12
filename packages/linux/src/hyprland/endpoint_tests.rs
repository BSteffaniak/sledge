//! Filesystem endpoint admission tests use only private mock sockets.
use super::*;
use nix::sys::socket::{
    AddressFamily, Backlog, SockFlag, SockType, UnixAddr, bind, listen, socket,
};
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "sledge-hyprland-mock-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::DirBuilder::new().create(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for name in ["peer", "alias"] {
            let _ = std::fs::remove_file(self.0.join(name));
        }
        let _ = std::fs::remove_dir(&self.0);
    }
}
#[test]
fn private_mock_endpoint_and_exact_pid() {
    let fixture = Fixture::new();
    let path = fixture.0.join("peer");
    let listener = socket(
        AddressFamily::Unix,
        SockType::SeqPacket,
        SockFlag::SOCK_CLOEXEC,
        None,
    )
    .unwrap();
    bind(listener.as_raw_fd(), &UnixAddr::new(&path).unwrap()).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    listen(&listener, Backlog::new(8).unwrap()).unwrap();
    let peer = transport::Peer {
        uid: nix::unistd::geteuid().as_raw(),
        pid: i32::try_from(std::process::id()).unwrap(),
    };
    Transport::connect(&path, peer, monotonic_ns().unwrap() + 100_000_000).unwrap();
    assert!(
        Transport::connect(
            &path,
            transport::Peer {
                pid: peer.pid + 1,
                ..peer
            },
            monotonic_ns().unwrap() + 100_000_000
        )
        .is_err()
    );
    let alias = fixture.0.join("alias");
    std::os::unix::fs::symlink(&path, &alias).unwrap();
    assert!(Transport::connect(&alias, peer, monotonic_ns().unwrap() + 100_000_000).is_err());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
    assert!(Transport::connect(&path, peer, monotonic_ns().unwrap() + 100_000_000).is_err());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::set_permissions(&fixture.0, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(Transport::connect(&path, peer, monotonic_ns().unwrap() + 100_000_000).is_err());
}

#[test]
fn passive_listener_matches_native_client_zero_scope_hello() {
    let fixture = Fixture::new();
    let path = fixture.0.join("peer");
    let listener = super::listener::Listener::bind(&path).unwrap();
    assert!(super::listener::Listener::bind(&path).is_err());
    let peer = transport::Peer {
        uid: nix::unistd::geteuid().as_raw(),
        pid: i32::try_from(std::process::id()).unwrap(),
    };
    let deadline = monotonic_ns().unwrap() + 100_000_000;
    let native = Transport::connect(&path, peer, deadline).unwrap();
    let rust = listener.accept(peer, deadline).unwrap();
    let identity = Identity {
        capabilities: 0,
        api_hash: "api".into(),
        artifact: "artifact".into(),
    };
    let scope = Scope {
        generation: 0,
        lock_epoch: 0,
        sequence: 0,
        deadline_ns: 0,
        nonce: 123,
    };
    native
        .send(
            &Packet {
                scope,
                body: Body::Hello(identity.clone()),
            },
            deadline,
        )
        .unwrap();
    let _bridge = Bridge::passive_handshake(rust, &identity, deadline).unwrap();
    assert_eq!(
        native.receive(deadline).unwrap(),
        Packet {
            scope,
            body: Body::HelloAck(identity)
        }
    );
    drop(listener);
    assert!(!path.exists());
}
