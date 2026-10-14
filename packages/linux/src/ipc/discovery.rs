//! Session-scoped client discovery. Never start a daemon or scan other sessions.
use super::denied;
use nix::fcntl::{AtFlags, OFlag, open, openat};
use nix::sys::stat::{Mode, SFlag, fstat, fstatat};
use serde::Deserialize;
use std::fs::File;
use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
#[cfg(test)]
mod tests;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Session {
    version: u32,
    backend: String,
    ipc_socket: PathBuf,
    compositor_pid: u32,
    instance: String,
}

/// Resolve a client endpoint. Explicit overrides always win, even with invalid
/// discovery environment. Session selection never falls back on error.
/// # Errors
/// Rejects unsafe, stale, absent or mismatched automatic session selection.
pub fn client_socket_path(explicit: Option<&Path>) -> io::Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path.to_owned());
    }
    let marker = std::env::var_os("SLEDGE_SESSION_FILE").map(PathBuf::from);
    let runtime = std::env::var_os("SLEDGE_HYPRLAND_RUNTIME").map(PathBuf::from);
    let xdg = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
    let signature = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE");
    resolve(
        marker.as_deref(),
        runtime.as_deref(),
        xdg.as_deref(),
        signature.as_deref(),
    )
}

fn resolve(
    marker: Option<&Path>,
    runtime: Option<&Path>,
    xdg: Option<&Path>,
    signature: Option<&std::ffi::OsStr>,
) -> io::Result<PathBuf> {
    let inherited = runtime.map(|path| path.join("session.json"));
    if let (Some(marker), Some(inherited)) = (marker, &inherited) {
        if marker != inherited {
            return Err(denied("conflicting Sledge session marker and runtime"));
        }
    }
    if let Some(marker) = marker.or(inherited.as_deref()) {
        let session = read_session(marker)?;
        if let Some(signature) = signature {
            if signature.to_str() != Some(&session.instance) {
                return Err(denied(
                    "Sledge session instance mismatch; use --socket for explicit selection",
                ));
            }
        }
        return Ok(session.ipc_socket);
    }
    if signature.is_some() {
        return Err(denied(
            "Sledge session is not selected; launch a Sledge login or set SLEDGE_SESSION_FILE, or use --socket",
        ));
    }
    let xdg = xdg.ok_or_else(|| denied("XDG_RUNTIME_DIR unavailable; use --socket"))?;
    let _root = private_directory(xdg)?;
    Ok(xdg.join("sledge.sock"))
}

fn read_session(marker: &Path) -> io::Result<Session> {
    if marker.file_name() != Some(std::ffi::OsStr::new("session.json")) {
        return Err(denied("Sledge session marker must be named session.json"));
    }
    let runtime = marker
        .parent()
        .ok_or_else(|| denied("session marker requires private directory"))?;
    let directory = private_directory(runtime)?;
    let raw = openat(
        Some(directory.as_raw_fd()),
        "session.json",
        OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(io::Error::from)?;
    // SAFETY: openat returned an owned, newly opened descriptor.
    let file = unsafe { File::from_raw_fd(raw) };
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != nix::unistd::geteuid().as_raw()
        || metadata.mode() & 0o777 != 0o600
        || metadata.len() > 4096
    {
        return Err(denied("unsafe Sledge session marker"));
    }
    let mut bytes = Vec::new();
    file.take(4097).read_to_end(&mut bytes)?;
    if bytes.len() > 4096 {
        return Err(denied("Sledge session marker exceeds size limit"));
    }
    let session: Session = serde_json::from_slice(&bytes)
        .map_err(|_| denied("invalid Sledge session marker schema"))?;
    if session.version != 1
        || session.backend != "hyprland"
        || session.compositor_pid == 0
        || session.compositor_pid > i32::MAX.cast_unsigned()
        || session.instance.is_empty()
        || session.instance.len() > 1024
        || session.instance.chars().any(char::is_control)
    {
        return Err(denied("unsupported Sledge session marker"));
    }
    if session.ipc_socket != runtime.join("health.sock") {
        return Err(denied(
            "Sledge session socket must be colocated health.sock",
        ));
    }
    // Metadata only: no command line, environment, credentials or process memory.
    // Liveness is a stale-marker check, not build admission or PID-reuse proof.
    let process = std::fs::metadata(format!("/proc/{}", session.compositor_pid))?;
    if !process.is_dir() || process.uid() != nix::unistd::geteuid().as_raw() {
        return Err(denied(
            "Sledge session compositor is not a live same-user process",
        ));
    }
    health_socket(runtime)?;
    Ok(session)
}
fn health_socket(runtime: &Path) -> io::Result<PathBuf> {
    let fd = private_directory(runtime)?;
    let metadata = fstatat(
        Some(fd.as_raw_fd()),
        "health.sock",
        AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .map_err(io::Error::from)?;
    if SFlag::from_bits_truncate(metadata.st_mode) != SFlag::S_IFSOCK
        || metadata.st_uid != nix::unistd::geteuid().as_raw()
        || metadata.st_mode & 0o077 != 0
    {
        return Err(denied(
            "Sledge session health endpoint is not a private same-user socket",
        ));
    }
    Ok(runtime.join("health.sock"))
}
// Walk with directory fds: no symlink components or traversal through `..`.
fn private_directory(path: &Path) -> io::Result<OwnedFd> {
    if !path.is_absolute() {
        return Err(denied("Sledge runtime path must be absolute"));
    }
    let raw = open(
        Path::new("/"),
        OFlag::O_DIRECTORY | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(io::Error::from)?;
    // SAFETY: every open/openat result is transferred exactly once.
    let mut fd = unsafe { OwnedFd::from_raw_fd(raw) };
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                let raw = openat(
                    Some(fd.as_raw_fd()),
                    Path::new(name),
                    OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
                    Mode::empty(),
                )
                .map_err(io::Error::from)?;
                // SAFETY: newly owned descriptor, replacing and closing the old fd.
                fd = unsafe { OwnedFd::from_raw_fd(raw) };
            }
            _ => return Err(denied("invalid Sledge runtime path component")),
        }
    }
    let metadata = fstat(fd.as_raw_fd()).map_err(io::Error::from)?;
    if metadata.st_uid != nix::unistd::geteuid().as_raw() || metadata.st_mode & 0o077 != 0 {
        return Err(denied(
            "Sledge runtime directory must be private and same-user",
        ));
    }
    Ok(fd)
}
