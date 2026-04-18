//! Focused application tracking via `NSWorkspace`.
//!
//! A dedicated polling thread queries `NSWorkspace.frontmostApplication`
//! periodically and updates the tracker. The tap's hot path reads the
//! tracker through an uncontended `RwLock` \u2014 no Objective-C from the
//! hot path.
//!
//! Polling at 50ms resolution is sufficient because app-scoped bindings
//! don't need sub-millisecond focus tracking; the worst case is a one-frame
//! lag after an app switch, which is imperceptible for hotkeys.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use objc2::rc::Retained;
use objc2_app_kit::{NSRunningApplication, NSWorkspace};
use objc2_foundation::NSString;
use parking_lot::RwLock;
use tracing::{debug, trace};

/// Holds the most-recently-observed focused-app bundle id.
#[derive(Debug, Default)]
pub struct FocusTracker {
    inner: RwLock<Option<String>>,
}

impl FocusTracker {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: RwLock::new(None),
        }
    }

    /// Return the current focused-app bundle id.
    #[must_use]
    pub fn current(&self) -> Option<String> {
        self.inner.read().clone()
    }

    fn set(&self, id: Option<String>) {
        let mut slot = self.inner.write();
        if *slot != id {
            trace!(?id, "focus changed");
        }
        *slot = id;
    }
}

/// Spawn a polling thread that updates the tracker until the returned
/// handle is dropped.
#[must_use]
pub fn spawn_poller(tracker: Arc<FocusTracker>) -> PollerHandle {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();

    let handle = thread::Builder::new()
        .name("sledge-focus".into())
        .spawn(move || {
            debug!("focus poller started");
            while !stop_clone.load(Ordering::Relaxed) {
                let id = current_frontmost_bundle();
                tracker.set(id);
                thread::sleep(Duration::from_millis(50));
            }
            debug!("focus poller stopped");
        })
        .expect("spawn focus poller");

    PollerHandle {
        stop,
        handle: Some(handle),
    }
}

fn current_frontmost_bundle() -> Option<String> {
    unsafe {
        let workspace = NSWorkspace::sharedWorkspace();
        let app: Option<Retained<NSRunningApplication>> = workspace.frontmostApplication();
        let app = app?;
        let bid: Option<Retained<NSString>> = app.bundleIdentifier();
        bid.map(|s| s.to_string())
    }
}

/// Handle to a running poller. Drop to stop the thread.
pub struct PollerHandle {
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Drop for PollerHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}
