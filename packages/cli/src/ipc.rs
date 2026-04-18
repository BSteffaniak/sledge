//! Unix-domain-socket IPC for `sledge status` / `sledge reload`.
//!
//! Protocol: a client connects, sends a single JSON line (`{"op": "status"}`
//! or `{"op": "reload"}`), and the daemon replies with one JSON line.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tracing::{debug, error, warn};

/// Request shapes the daemon understands.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Status,
    Reload,
}

/// Daemon response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Response {
    Status(StatusPayload),
    Reloaded,
    Error { message: String },
}

/// Payload for `status` responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusPayload {
    pub version: String,
    pub rules_loaded: usize,
    pub uptime_secs: u64,
    pub focused_app: Option<String>,
    pub permissions: StatusPermissions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusPermissions {
    pub accessibility: bool,
    pub input_monitoring: bool,
}

/// Shared state the IPC server can query.
pub struct ServerState {
    pub started_at: std::time::Instant,
    pub rules_loaded: Arc<Mutex<usize>>,
    pub focused_app: Arc<dyn Fn() -> Option<String> + Send + Sync>,
    pub reload: Arc<dyn Fn() -> Result<(), String> + Send + Sync>,
    pub check_permissions: Arc<dyn Fn() -> StatusPermissions + Send + Sync>,
}

/// Bind the socket. Removes any stale socket file first.
///
/// # Errors
///
/// Returns an error if the socket cannot be bound.
pub fn bind(path: &Path) -> Result<UnixListener> {
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let listener =
        UnixListener::bind(path).with_context(|| format!("binding {}", path.display()))?;
    debug!(socket = %path.display(), "IPC listener bound");
    Ok(listener)
}

/// Serve requests forever. Cancelled by dropping the listener.
pub async fn serve(listener: UnixListener, state: Arc<ServerState>) {
    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let state = state.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle(stream, state).await {
                        warn!(error = %e, "IPC request failed");
                    }
                });
            }
            Err(e) => {
                error!(error = %e, "IPC accept failed");
                return;
            }
        }
    }
}

async fn handle(stream: UnixStream, state: Arc<ServerState>) -> Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    let req: Request = serde_json::from_str(line.trim())?;

    let resp = match req {
        Request::Status => {
            let perms = (state.check_permissions)();
            let rules = *state.rules_loaded.lock();
            let focused = (state.focused_app)();
            let uptime = state.started_at.elapsed().as_secs();
            Response::Status(StatusPayload {
                version: env!("CARGO_PKG_VERSION").to_string(),
                rules_loaded: rules,
                uptime_secs: uptime,
                focused_app: focused,
                permissions: perms,
            })
        }
        Request::Reload => match (state.reload)() {
            Ok(()) => Response::Reloaded,
            Err(m) => Response::Error { message: m },
        },
    };
    let mut buf = serde_json::to_vec(&resp)?;
    buf.push(b'\n');
    write_half.write_all(&buf).await?;
    write_half.flush().await?;
    Ok(())
}

/// One-shot client: send `req` to `path` and return the parsed response.
///
/// # Errors
///
/// Returns an error if the daemon is not reachable or the response cannot
/// be parsed.
pub async fn send_request(path: &Path, req: &Request) -> Result<Response> {
    let stream = UnixStream::connect(path)
        .await
        .with_context(|| format!("connecting to {}", path.display()))?;
    let (read_half, mut write_half) = stream.into_split();
    let mut line = serde_json::to_vec(req)?;
    line.push(b'\n');
    write_half.write_all(&line).await?;
    write_half.shutdown().await.ok();

    let mut reader = BufReader::new(read_half);
    let mut buf = String::new();
    reader.read_line(&mut buf).await?;
    let resp: Response = serde_json::from_str(buf.trim())?;
    Ok(resp)
}

/// Synchronous wrapper for [`send_request`] that spins up a single-threaded
/// Tokio runtime. Used by the `status` / `reload` CLI subcommands.
///
/// # Errors
///
/// Propagates transport or decoding errors.
pub fn send_request_blocking(path: &Path, req: &Request) -> Result<Response> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(send_request(path, req))
}

#[doc(hidden)]
pub fn _unused_pathbuf() -> PathBuf {
    PathBuf::new()
}
