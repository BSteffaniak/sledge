//! Unix-domain-socket IPC for `sledge status` / `sledge reload`.
//!
//! Protocol: a client connects, sends a single JSON line (`{"op": "status"}`
//! or `{"op": "reload"}`), and the daemon replies with one JSON line.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tracing::{debug, error, warn};

/// Maximum bytes in a public JSON frame, including its newline.
pub const MAX_FRAME_BYTES: usize = 64 * 1024;
/// Transport deadline; a timed-out reload may still finish on its worker.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_CONNECTIONS: usize = 32;

#[cfg(test)]
mod tests;

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
    /// Status schema version, distinct from the package version.
    #[serde(default = "legacy_protocol_version")]
    pub protocol_version: u32,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub capabilities: Option<serde_json::Value>,
    pub version: String,
    pub rules_loaded: usize,
    pub uptime_secs: u64,
    pub focused_app: Option<String>,
    pub permissions: Option<StatusPermissions>,
    /// Seconds since the last successful config reload, or `None` if no
    /// reload has happened since daemon start.
    pub last_reload_secs_ago: Option<u64>,
}

const fn legacy_protocol_version() -> u32 {
    1
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
    pub check_permissions: Arc<dyn Fn() -> Option<StatusPermissions> + Send + Sync>,
    pub capabilities: Arc<dyn Fn() -> Option<serde_json::Value> + Send + Sync>,
    /// Timestamp of the most recent successful reload, if any.
    pub last_reload_at: Arc<Mutex<Option<std::time::Instant>>>,
}

/// Bind without removing existing paths (including stale sockets).
///
/// Recovery of an abandoned socket requires explicit operator action; blindly
/// unlinking could admit two daemons or delete an unrelated file.
///
/// # Errors
///
/// Returns an error if the socket cannot be bound.
pub fn bind(path: &Path) -> Result<UnixListener> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).context("creating IPC socket directory")?;
    }
    let listener =
        UnixListener::bind(path).with_context(|| format!("binding {}", path.display()))?;
    debug!(socket = %path.display(), "IPC listener bound");
    Ok(listener)
}

/// Serve requests forever. Cancelled by dropping the listener.
pub async fn serve(listener: UnixListener, state: Arc<ServerState>) {
    let capacity = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                #[cfg(target_os = "linux")]
                if let Err(error) = sledge_linux::authorize_socket_peer(&stream) {
                    warn!(%error, "IPC peer rejected");
                    continue;
                }
                let Ok(permit) = capacity.clone().try_acquire_owned() else {
                    warn!("IPC connection limit reached");
                    continue;
                };
                let state = state.clone();
                tokio::spawn(async move {
                    if let Err(e) =
                        tokio::time::timeout(REQUEST_TIMEOUT, handle(stream, state, permit))
                            .await
                            .context("IPC request timed out")
                            .and_then(std::convert::identity)
                    {
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

async fn handle(
    stream: UnixStream,
    state: Arc<ServerState>,
    permit: tokio::sync::OwnedSemaphorePermit,
) -> Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let line = read_frame(read_half).await?;
    let req: Request = serde_json::from_slice(&line)?;
    // File reloads and platform inspection must not block the async reactor.
    let (resp, _permit) = tokio::task::spawn_blocking(move || {
        let resp = response(req, &state);
        (resp, permit)
    })
    .await?;
    let buf = encode_frame(&resp)?;
    write_half.write_all(&buf).await?;
    write_half.flush().await?;
    Ok(())
}

fn response(req: Request, state: &ServerState) -> Response {
    match req {
        Request::Status => {
            let perms = (state.check_permissions)();
            let rules = *state.rules_loaded.lock();
            let focused = (state.focused_app)();
            let uptime = state.started_at.elapsed().as_secs();
            let last_reload_secs_ago = state.last_reload_at.lock().map(|t| t.elapsed().as_secs());
            Response::Status(StatusPayload {
                protocol_version: 2,
                platform: Some(std::env::consts::OS.to_owned()),
                capabilities: (state.capabilities)(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                rules_loaded: rules,
                uptime_secs: uptime,
                focused_app: focused,
                permissions: perms,
                last_reload_secs_ago,
            })
        }
        Request::Reload => match (state.reload)() {
            Ok(()) => Response::Reloaded,
            Err(m) => Response::Error { message: m },
        },
    }
}

async fn read_frame(reader: impl AsyncRead + Unpin) -> Result<Vec<u8>> {
    let mut reader = BufReader::new(reader.take((MAX_FRAME_BYTES + 1) as u64));
    let mut frame = Vec::new();
    reader.read_until(b'\n', &mut frame).await?;
    anyhow::ensure!(frame.len() <= MAX_FRAME_BYTES, "IPC frame too large");
    anyhow::ensure!(frame.last() == Some(&b'\n'), "incomplete IPC frame");
    Ok(frame)
}

fn encode_frame(value: &impl Serialize) -> Result<Vec<u8>> {
    let mut writer = FrameWriter(Vec::new());
    serde_json::to_writer(&mut writer, value)?;
    std::io::Write::write_all(&mut writer, b"\n")?;
    Ok(writer.0)
}

struct FrameWriter(Vec<u8>);

impl std::io::Write for FrameWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_FRAME_BYTES - self.0.len() {
            return Err(std::io::Error::other("IPC frame too large"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// One-shot client: send `req` to `path` and return the parsed response.
///
/// # Errors
///
/// Returns an error if the daemon is not reachable or the response cannot
/// be parsed.
pub async fn send_request(path: &Path, req: &Request) -> Result<Response> {
    tokio::time::timeout(REQUEST_TIMEOUT, exchange(path, req))
        .await
        .context("IPC request timed out")?
}

async fn exchange(path: &Path, req: &Request) -> Result<Response> {
    let stream = UnixStream::connect(path)
        .await
        .with_context(|| format!("connecting to {}", path.display()))?;
    #[cfg(target_os = "linux")]
    sledge_linux::authorize_socket_peer(&stream).context("IPC server identity rejected")?;
    let (read_half, mut write_half) = stream.into_split();
    let line = encode_frame(req)?;
    write_half.write_all(&line).await?;
    write_half.shutdown().await?;

    let frame = read_frame(read_half).await?;
    Ok(serde_json::from_slice(&frame)?)
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
