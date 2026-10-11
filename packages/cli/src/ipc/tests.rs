use super::*;

#[tokio::test]
async fn framing_requires_newline_and_bounds_input() {
    assert!(read_frame(&b"{\"op\":\"status\"}"[..]).await.is_err());
    let frame = read_frame(&b"{\"op\":\"status\"}\n"[..]).await.unwrap();
    assert!(matches!(
        serde_json::from_slice(&frame).unwrap(),
        Request::Status
    ));
    let oversized = vec![b'x'; MAX_FRAME_BYTES + 1];
    assert!(read_frame(oversized.as_slice()).await.is_err());
}

#[tokio::test]
async fn framing_accepts_exact_limit_and_only_first_line() {
    let mut frame = vec![b' '; MAX_FRAME_BYTES - 1];
    frame.push(b'\n');
    assert_eq!(
        read_frame(frame.as_slice()).await.unwrap().len(),
        MAX_FRAME_BYTES
    );
    assert_eq!(read_frame(&b"{}\nignored\n"[..]).await.unwrap(), b"{}\n");
}

#[test]
fn encoding_bounds_output() {
    let response = Response::Error {
        message: "x".repeat(MAX_FRAME_BYTES),
    };
    assert!(encode_frame(&response).is_err());
    assert_eq!(
        encode_frame(&Request::Status).unwrap(),
        b"{\"op\":\"status\"}\n"
    );
}

#[tokio::test]
async fn bind_does_not_remove_existing_paths() {
    let dir = std::env::temp_dir().join(format!("sledge-ipc-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let regular = dir.join("regular");
    std::fs::write(&regular, b"keep").unwrap();
    assert!(bind(&regular).is_err());
    assert_eq!(std::fs::read(&regular).unwrap(), b"keep");
    let path = dir.join("socket");
    let listener = bind(&path).unwrap();
    assert!(bind(&path).is_err());
    drop(listener);
    assert!(
        bind(&path).is_err(),
        "even stale sockets require explicit recovery"
    );
    std::fs::remove_file(&path).unwrap();
    std::fs::remove_file(&regular).unwrap();
    std::fs::remove_dir(&dir).unwrap();
}

#[test]
fn legacy_status_remains_decodable() {
    let response: Response = serde_json::from_str(
        r#"{"kind":"status","version":"old","rules_loaded":1,"uptime_secs":2,"focused_app":null,"permissions":{"accessibility":true,"input_monitoring":false},"last_reload_secs_ago":null}"#,
    ).unwrap();
    let Response::Status(status) = response else {
        panic!("status expected")
    };
    assert_eq!(status.protocol_version, 1);
    assert!(status.permissions.unwrap().accessibility);
    assert!(status.capabilities.is_none());
}

#[test]
fn linux_status_does_not_invent_tcc_permissions() {
    let payload = StatusPayload {
        protocol_version: 2,
        platform: Some("linux".into()),
        capabilities: Some(serde_json::json!({"verified_session": false})),
        version: "test".into(),
        rules_loaded: 0,
        uptime_secs: 0,
        focused_app: None,
        permissions: None,
        last_reload_secs_ago: None,
    };
    let json = serde_json::to_value(Response::Status(payload)).unwrap();
    assert!(json["permissions"].is_null());
    assert_eq!(json["protocol_version"], 2);
}

#[tokio::test]
async fn stalled_reader_can_be_cancelled() {
    let (_writer, reader) = tokio::io::duplex(16);
    assert!(
        tokio::time::timeout(Duration::from_millis(10), read_frame(reader))
            .await
            .is_err()
    );
}
