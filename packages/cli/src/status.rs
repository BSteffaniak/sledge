//! `sledge status`, `sledge reload`, `sledge validate`, `sledge check-permissions`.

use std::path::Path;

use anyhow::{Context, Result};

use crate::cli::Cli;
use crate::ipc::{Request, Response, send_request_blocking};
use crate::logging;

/// Print daemon status by querying the IPC socket.
///
/// # Errors
///
/// Returns an error if the daemon is not reachable.
pub fn print_status(cli: &Cli) -> Result<()> {
    let path = cli
        .socket
        .clone()
        .unwrap_or_else(logging::default_socket_path);
    let resp = send_request_blocking(&path, &Request::Status)?;
    match resp {
        Response::Status(s) => {
            println!("sledge {}", s.version);
            println!("  uptime         : {}s", s.uptime_secs);
            println!("  rules loaded   : {}", s.rules_loaded);
            println!(
                "  focused app    : {}",
                s.focused_app.as_deref().unwrap_or("<unknown>")
            );
            println!("  accessibility  : {}", yn(s.permissions.accessibility));
            println!("  input monitor  : {}", yn(s.permissions.input_monitoring));
            Ok(())
        }
        Response::Error { message } => anyhow::bail!("daemon error: {message}"),
        Response::Reloaded => anyhow::bail!("unexpected response"),
    }
}

/// Ask the daemon to reload its configuration.
///
/// # Errors
///
/// Returns an error if the daemon is not reachable or reload fails.
pub fn send_reload(cli: &Cli) -> Result<()> {
    let path = cli
        .socket
        .clone()
        .unwrap_or_else(logging::default_socket_path);
    let resp = send_request_blocking(&path, &Request::Reload)?;
    match resp {
        Response::Reloaded => {
            println!("reloaded");
            Ok(())
        }
        Response::Error { message } => anyhow::bail!("reload failed: {message}"),
        Response::Status(_) => anyhow::bail!("unexpected response"),
    }
}

/// Validate a config file.
///
/// # Errors
///
/// Returns an error if the file cannot be read or the config fails
/// validation.
pub fn validate_config(path: &Path) -> Result<()> {
    let cfg = sledge_config::load_from_file(path)
        .with_context(|| format!("validating config file: {}", path.display()))?;
    println!(
        "OK: {} rule(s), {} alias(es), log_level={}",
        cfg.rules.len(),
        cfg.app_aliases.len(),
        cfg.daemon.log_level
    );
    Ok(())
}

/// Report macOS permission status. On non-macOS just prints a note.
///
/// # Errors
///
/// Currently infallible.
pub fn print_permissions() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let p = sledge_macos::check_permissions();
        println!("accessibility  : {}", yn(p.accessibility));
        println!("input monitor  : {}", yn(p.input_monitoring));
        if !p.ok() {
            anyhow::bail!("one or more permissions missing");
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        println!("permission checks are only meaningful on macOS");
    }
    Ok(())
}

fn yn(b: bool) -> &'static str {
    if b { "yes" } else { "NO" }
}
