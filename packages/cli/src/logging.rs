//! Tracing subscriber setup.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::prelude::*;

/// Guard returned by [`init`]. Keep it alive for the daemon's lifetime to
/// flush log writes on shutdown.
#[must_use]
pub struct LogGuard {
    _file: Option<tracing_appender::non_blocking::WorkerGuard>,
}

/// Initialise tracing. Logs are written to a rotating file under
/// `~/Library/Logs/sledge/` on macOS and `$XDG_STATE_HOME/sledge/` elsewhere.
/// If `also_stdout` is true, logs are also mirrored to stderr.
///
/// # Errors
///
/// Returns an error if the log directory cannot be created.
pub fn init(level: &str, also_stdout: bool) -> Result<LogGuard> {
    let log_dir = log_dir()?;
    std::fs::create_dir_all(&log_dir).with_context(|| format!("creating {}", log_dir.display()))?;

    let file_appender = tracing_appender::rolling::daily(&log_dir, "sledge.log");
    let (non_blocking, worker) = tracing_appender::non_blocking(file_appender);

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(true)
        .with_level(true);

    let filter = EnvFilter::try_new(level).unwrap_or_else(|_| EnvFilter::new("info"));

    let registry = tracing_subscriber::registry().with(filter).with(file_layer);

    if also_stdout {
        let stderr_layer = tracing_subscriber::fmt::layer()
            .with_writer(std::io::stderr)
            .with_ansi(true)
            .with_target(false);
        registry.with(stderr_layer).init();
    } else {
        registry.init();
    }

    Ok(LogGuard {
        _file: Some(worker),
    })
}

fn log_dir() -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = dirs::home_dir().context("locating $HOME")?;
        Ok(home.join("Library").join("Logs").join("sledge"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let base = dirs::state_dir()
            .or_else(dirs::data_local_dir)
            .context("locating state dir")?;
        Ok(base.join("sledge"))
    }
}

/// Default system-wide config path.
#[must_use]
pub fn default_config_path() -> Option<PathBuf> {
    let base = dirs::config_dir()?;
    Some(base.join("sledge").join("config.toml"))
}

/// Default IPC socket path.
#[must_use]
pub fn default_socket_path() -> PathBuf {
    // SAFETY: libc::getuid is a plain syscall.
    let uid = unsafe { libc::getuid() };
    if let Ok(run) = std::env::var("XDG_RUNTIME_DIR") {
        return Path::new(&run).join("sledge.sock");
    }
    PathBuf::from(format!("/tmp/sledge-{uid}.sock"))
}
