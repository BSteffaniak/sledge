use super::*;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt, symlink};
use std::os::unix::net::UnixListener;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "sledge-discovery-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .unwrap();
        Self(path)
    }
    fn directory(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .unwrap();
        path
    }
    fn socket(&self, runtime: &Path) -> UnixListener {
        let path = runtime.join("health.sock");
        let listener = UnixListener::bind(&path).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
        listener
    }
    fn marker(runtime: &Path, instance: &str) -> PathBuf {
        let path = runtime.join("session.json");
        Self::write(
            &path,
            &serde_json::json!({"version":1,"backend":"hyprland","ipc_socket":runtime.join("health.sock"),"compositor_pid":std::process::id(),"instance":instance}),
        );
        path
    }
    fn write(path: &Path, value: &serde_json::Value) {
        std::fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
#[test]
fn explicit_override_does_not_consult_discovery_environment() {
    let path = Path::new("explicit.sock");
    assert_eq!(client_socket_path(Some(path)).unwrap(), path);
}
#[test]
fn inherited_runtime_and_explicit_marker_agree_and_stale_selection_never_falls_back() {
    let f = Fixture::new();
    let runtime = f.directory("session");
    let _listener = f.socket(&runtime);
    let marker = Fixture::marker(&runtime, "a");
    assert_eq!(
        resolve(None, Some(&runtime), None, Some("a".as_ref())).unwrap(),
        runtime.join("health.sock")
    );
    assert_eq!(
        resolve(Some(&marker), None, None, None).unwrap(),
        runtime.join("health.sock")
    );
    assert!(resolve(Some(&marker), Some(&f.0), None, None).is_err());
    std::fs::remove_file(runtime.join("health.sock")).unwrap();
    assert!(resolve(Some(&marker), None, Some(&f.0), None).is_err());
}
#[test]
fn multiple_sessions_are_never_scanned_and_instance_mismatch_is_rejected() {
    let f = Fixture::new();
    let a = f.directory("session-a");
    let b = f.directory("session-b");
    let _a = f.socket(&a);
    let _b = f.socket(&b);
    let ma = Fixture::marker(&a, "a");
    Fixture::marker(&b, "b");
    assert_eq!(
        resolve(Some(&ma), None, Some(&f.0), Some("a".as_ref())).unwrap(),
        a.join("health.sock")
    );
    assert!(resolve(Some(&ma), None, Some(&f.0), Some("b".as_ref())).is_err());
    assert!(resolve(None, None, Some(&f.0), Some("a".as_ref())).is_err());
    assert!(resolve(None, Some(&f.0.join("missing")), Some(&f.0), None).is_err());
}
#[test]
fn non_hyprland_keeps_default_without_global_discovery() {
    let f = Fixture::new();
    assert_eq!(
        resolve(None, None, Some(&f.0), None).unwrap(),
        f.0.join("sledge.sock")
    );
    assert!(resolve(None, None, None, None).is_err());
}
#[test]
fn strict_schema_and_stale_process_are_rejected() {
    let f = Fixture::new();
    let runtime = f.directory("session");
    let _listener = f.socket(&runtime);
    let marker = Fixture::marker(&runtime, "a");
    let original: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&marker).unwrap()).unwrap();
    for (key, value) in [
        ("version", 2.into()),
        ("backend", "other".into()),
        ("instance", "".into()),
        ("compositor_pid", 0.into()),
        ("compositor_pid", i32::MAX.into()),
        ("ipc_socket", "/elsewhere/health.sock".into()),
        ("unknown", true.into()),
    ] {
        let mut bad = original.clone();
        bad[key] = value;
        Fixture::write(&marker, &bad);
        assert!(resolve(Some(&marker), None, None, None).is_err(), "{key}");
    }
}
#[test]
fn unsafe_paths_markers_and_sockets_are_rejected() {
    let f = Fixture::new();
    let runtime = f.directory("session");
    let _listener = f.socket(&runtime);
    let marker = Fixture::marker(&runtime, "a");
    std::fs::set_permissions(&marker, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(resolve(Some(&marker), None, None, None).is_err());
    std::fs::remove_file(&marker).unwrap();
    symlink(&runtime, &marker).unwrap();
    assert!(resolve(Some(&marker), None, None, None).is_err());
    let alias = f.0.join("alias");
    symlink(&runtime, &alias).unwrap();
    assert!(resolve(None, Some(&alias), None, None).is_err());
    assert!(resolve(None, Some(Path::new("relative")), None, None).is_err());
    std::fs::remove_file(&marker).unwrap();
    Fixture::marker(&runtime, "a");
    std::fs::set_permissions(
        runtime.join("health.sock"),
        std::fs::Permissions::from_mode(0o666),
    )
    .unwrap();
    assert!(resolve(Some(&marker), None, None, None).is_err());
    std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(resolve(Some(&marker), None, None, None).is_err());
}
