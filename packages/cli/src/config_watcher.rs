//! Config-file watcher.
//!
//! Watches the **parent directory** of the config file (rather than the
//! file itself) because most editors save by writing to a sibling tempfile
//! and renaming it over the target \u2014 if we watched the file inode
//! directly, the rename would leave us watching a stale deleted inode.
//!
//! Events are debounced (~250 ms) to coalesce multi-write saves. When any
//! event path matches the config file name, the provided `reload_fn` is
//! invoked. `reload_fn` is the same closure used by the SIGHUP and IPC
//! reload paths, so behaviour stays consistent across triggers.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::thread;
use std::time::Duration;

use notify::{EventKind, RecursiveMode, Watcher, recommended_watcher};
use tracing::{debug, info, warn};

/// Handle to a running watcher. Drop to stop the background thread.
pub struct ConfigWatcher {
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Drop for ConfigWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Debounce window for coalescing save events.
const DEBOUNCE: Duration = Duration::from_millis(250);

/// Start watching `config_path`. Returns a handle that stops the watcher
/// on drop. On any change event whose path basename matches the config
/// file name, invokes `reload_fn` after a short debounce.
///
/// `reload_fn` errors are logged but do not tear down the watcher \u2014
/// a bad edit should not kill auto-reload, and the next valid save will
/// still trigger a reload.
#[must_use]
pub fn spawn(
    config_path: PathBuf,
    reload_fn: Arc<dyn Fn() -> Result<(), String> + Send + Sync>,
) -> Option<ConfigWatcher> {
    let Some(parent) = config_path.parent().map(Path::to_path_buf) else {
        warn!(
            path = %config_path.display(),
            "config file has no parent directory; skipping file watcher"
        );
        return None;
    };

    let target_name = config_path.file_name()?.to_owned();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();

    let handle = thread::Builder::new()
        .name("sledge-config-watcher".into())
        .spawn(move || {
            let (tx, rx) = channel::<notify::Result<notify::Event>>();
            let mut watcher = match recommended_watcher(tx) {
                Ok(w) => w,
                Err(e) => {
                    warn!(error = %e, "failed to create config file watcher");
                    return;
                }
            };

            if let Err(e) = watcher.watch(&parent, RecursiveMode::NonRecursive) {
                warn!(
                    path = %parent.display(),
                    error = %e,
                    "failed to watch config directory"
                );
                return;
            }

            info!(path = %parent.display(), "config file watcher started");

            // Debounce loop. We block waiting for an event; when one
            // arrives that matches the target file, we drain any further
            // events for up to DEBOUNCE, then invoke reload once.
            while !stop_clone.load(Ordering::Relaxed) {
                let evt = match rx.recv_timeout(Duration::from_millis(500)) {
                    Ok(Ok(e)) => e,
                    Ok(Err(e)) => {
                        debug!(error = %e, "notify event error");
                        continue;
                    }
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => break,
                };

                if !event_matches(&evt, &target_name) {
                    continue;
                }

                // Drain the debounce window.
                let deadline = std::time::Instant::now() + DEBOUNCE;
                loop {
                    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                    if remaining.is_zero() {
                        break;
                    }
                    match rx.recv_timeout(remaining) {
                        Ok(Ok(e)) if event_matches(&e, &target_name) => {
                            // extend debounce only while matching events keep arriving
                        }
                        Ok(Ok(_) | Err(_)) => {}
                        Err(_) => break,
                    }
                }

                match (reload_fn)() {
                    Ok(()) => info!("config reloaded via file watcher"),
                    Err(e) => warn!(error = %e, "file-watcher reload failed"),
                }
            }

            debug!("config file watcher stopped");
        })
        .ok()?;

    Some(ConfigWatcher {
        stop,
        handle: Some(handle),
    })
}

fn event_matches(evt: &notify::Event, target: &std::ffi::OsStr) -> bool {
    // Only care about content-ish events; ignore pure metadata changes
    // that don't imply a rewrite.
    match evt.kind {
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {}
        _ => return false,
    }
    evt.paths
        .iter()
        .any(|p| p.file_name().is_some_and(|n| n == target))
}
