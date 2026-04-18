//! Platform backend trait.
//!
//! Each platform implements this to deliver normalized [`KeyEvent`]s to the
//! core engine and execute [`Action`]s the core produces.

use crate::action::Action;
use crate::event::KeyEvent;

/// What the core wants the backend to do with an incoming event.
#[derive(Debug, Clone)]
pub enum BackendVerdict {
    /// Pass the event through unmodified.
    Pass,
    /// Swallow the event (drop it).
    Swallow,
}

/// Platform backend abstraction.
///
/// Backends are expected to run their own event loop on their own thread
/// (the CFRunLoop on macOS, an `epoll`/`inotify` loop on Linux, etc.) and
/// call the provided [`EventSink`] synchronously from that loop.
pub trait InputBackend: Send + 'static {
    /// Run the backend forever. Does not return under normal operation.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot install its system-level
    /// hooks (e.g. missing permissions).
    fn run(&mut self, sink: Box<dyn EventSink>) -> Result<(), BackendError>;

    /// Synthesize an action, typically as the result of a matched rule.
    /// Called from worker threads \u2014 must be thread-safe.
    ///
    /// # Errors
    ///
    /// Returns an error if the action cannot be dispatched (e.g. unknown
    /// input source id).
    fn inject(&self, action: &Action) -> Result<(), BackendError>;
}

/// Callback that the backend invokes for each incoming event. Implementors
/// return a [`BackendVerdict`] telling the backend whether to drop the
/// event. Injection (replacing with a different action) happens out-of-band
/// via [`InputBackend::inject`].
pub trait EventSink: Send + 'static {
    /// Deliver an event. `focused_app` is the logical app identifier; may
    /// be `None` if not yet known.
    fn on_event(&mut self, event: KeyEvent, focused_app: Option<&str>) -> BackendVerdict;
}

/// Errors a backend can raise.
#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("missing permission: {0}")]
    MissingPermission(String),
    #[error("failed to install event tap: {0}")]
    TapInstall(String),
    #[error("failed to inject event: {0}")]
    Inject(String),
    #[error("unknown input source: {0}")]
    UnknownInputSource(String),
    #[error("{0}")]
    Other(String),
}
