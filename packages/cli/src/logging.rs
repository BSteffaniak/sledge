//! Tracing subscriber setup.
//!
//! The subscriber is installed once at daemon startup but uses a
//! [`tracing_subscriber::reload::Layer`] so the `EnvFilter` (log-level
//! filter) can be swapped at runtime without reinstalling the global
//! subscriber. The reload handle is exposed via [`LogGuard::reload_handle`]
//! so the config-reload path can re-apply the `log_level` from the TOML
//! file whenever the config changes.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing::info;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::prelude::*;
use tracing_subscriber::reload;

/// Handle used to hot-swap the `EnvFilter` at runtime.
///
/// Cloneable and `Send + Sync` so it can be captured by the config-reload
/// closure and the IPC server without extra synchronisation.
pub type FilterReloadHandle = reload::Handle<EnvFilter, tracing_subscriber::Registry>;

/// Guard returned by [`init`]. Keep it alive for the daemon's lifetime to
/// flush log writes on shutdown.
#[must_use]
pub struct LogGuard {
    _file: Option<tracing_appender::non_blocking::WorkerGuard>,
    reload_handle: FilterReloadHandle,
}

impl LogGuard {
    /// A clone of the `EnvFilter` reload handle.
    #[must_use]
    pub fn reload_handle(&self) -> FilterReloadHandle {
        self.reload_handle.clone()
    }
}

/// Attempt to swap the active `EnvFilter` to the directive `level`. If the
/// directive is invalid, falls back to `info` and returns an error string
/// describing the parse failure.
///
/// Logs the level change at INFO on success. Safe to call repeatedly.
///
/// # Errors
///
/// Returns an error string if the reload handle has been dropped or if the
/// new filter cannot be installed.
pub fn apply_level(handle: &FilterReloadHandle, level: &str) -> Result<(), String> {
    let filter = match EnvFilter::try_new(level) {
        Ok(f) => f,
        Err(e) => {
            // Fall back to info so the daemon keeps producing logs.
            let fallback = EnvFilter::new("info");
            handle
                .reload(fallback)
                .map_err(|re| format!("reload handle error: {re}"))?;
            return Err(format!("invalid log_level {level:?}: {e}"));
        }
    };
    handle
        .reload(filter)
        .map_err(|e| format!("reload handle error: {e}"))?;
    info!(level = %level, "log level applied");
    Ok(())
}

/// Initialise tracing. Logs are written to a rotating file under
/// `~/Library/Logs/sledge/` on macOS and `$XDG_STATE_HOME/sledge/` elsewhere.
/// If `also_stdout` is true, logs are also mirrored to stderr.
///
/// The returned [`LogGuard`] exposes a [`FilterReloadHandle`] via
/// [`LogGuard::reload_handle`] for runtime log-level changes.
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

    let initial_filter = EnvFilter::try_new(level).unwrap_or_else(|_| EnvFilter::new("info"));
    let (filter_layer, reload_handle) = reload::Layer::new(initial_filter);

    let registry = tracing_subscriber::registry()
        .with(filter_layer)
        .with(file_layer);

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
        reload_handle,
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
///
/// Resolution order:
///   1. `$XDG_CONFIG_HOME/sledge/config.toml` if `XDG_CONFIG_HOME` is set.
///   2. `~/.config/sledge/config.toml` if that file exists (preferred and
///      what the project's nix home-manager integration writes to).
///   3. `~/Library/Application Support/sledge/config.toml` on macOS,
///      platform-native fallback, if that file exists.
///   4. If none of the above exist, return `~/.config/sledge/config.toml`
///      (the canonical path) so error messages point at the preferred
///      location.
#[must_use]
pub fn default_config_path() -> Option<PathBuf> {
    // 1. Explicit XDG_CONFIG_HOME override.
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg).join("sledge").join("config.toml"));
        }
    }

    let home = dirs::home_dir()?;
    let xdg_path = home.join(".config").join("sledge").join("config.toml");

    // 2. Preferred location, if it exists.
    if xdg_path.exists() {
        return Some(xdg_path);
    }

    // 3. macOS platform-native fallback, if it exists.
    #[cfg(target_os = "macos")]
    {
        let app_support = home
            .join("Library")
            .join("Application Support")
            .join("sledge")
            .join("config.toml");
        if app_support.exists() {
            return Some(app_support);
        }
    }

    // 4. Nothing exists yet; report the canonical path.
    Some(xdg_path)
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
