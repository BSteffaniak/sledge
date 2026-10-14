//! `sledge status`, `sledge reload`, `sledge validate`, `sledge check-permissions`.

#[cfg(target_os = "linux")]
mod readiness;
#[cfg(target_os = "linux")]
use readiness::describe as backend_readiness;

use std::path::Path;

use anyhow::{Context, Result};

use crate::cli::Cli;
use crate::ipc::{Request, Response, send_request_blocking};
#[cfg(not(target_os = "linux"))]
use crate::logging;

pub(crate) fn socket_path(cli: &Cli) -> Result<std::path::PathBuf> {
    if let Some(path) = &cli.socket {
        return Ok(path.clone());
    }
    #[cfg(target_os = "linux")]
    {
        sledge_linux::ipc::client_socket_path(None)
            .context("selecting Sledge session; use --socket for an explicit endpoint")
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(logging::default_socket_path())
    }
}

/// Print daemon status by querying the IPC socket.
///
/// # Errors
///
/// Returns an error if the daemon is not reachable.
pub fn print_status(cli: &Cli) -> Result<()> {
    let path = socket_path(cli)?;
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
            if let Some(permissions) = &s.permissions {
                println!("  accessibility  : {}", yn(permissions.accessibility));
                println!("  input monitor  : {}", yn(permissions.input_monitoring));
            }
            if let Some(platform) = &s.platform {
                println!(
                    "  platform       : {platform} (status v{})",
                    s.protocol_version
                );
            }
            if let Some(capabilities) = &s.capabilities {
                #[cfg(target_os = "linux")]
                if let Some(readiness) = backend_readiness(capabilities) {
                    println!("  backend        : {readiness}");
                }
                println!("  capabilities   : {capabilities}");
            }
            match s.last_reload_secs_ago {
                Some(secs) => println!("  last reload    : {secs}s ago"),
                None => println!("  last reload    : never"),
            }
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
    let path = socket_path(cli)?;
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

/// Report platform permission/authorization status without grabbing devices.
///
/// # Errors
///
/// Returns an error when required authorization is missing or unsupported.
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
    #[cfg(target_os = "linux")]
    {
        let backend = sledge_linux::LinuxBackend::new();
        let capabilities = backend.capabilities();
        println!("broker connected : {}", yn(capabilities.broker_connected));
        println!("verified session : {}", yn(capabilities.verified_session));
        println!("source switching : {}", yn(capabilities.source_switching));
        if let Some(reason) = capabilities.reason {
            println!("reason           : {reason}");
        }
        backend
            .check_permissions()
            .context("Linux authorization is not granted")?;
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        anyhow::bail!("permission checks are unsupported on this platform");
    }
    Ok(())
}

fn yn(b: bool) -> &'static str {
    if b { "yes" } else { "NO" }
}
