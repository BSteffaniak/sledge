//! Windows backend stub.
//!
//! Scaffolded for future use. Currently unimplemented.

#[cfg(target_os = "windows")]
mod imp {
    use sledge_core::{Action, BackendError, EventSink, InputBackend};

    /// Placeholder backend that returns errors for everything.
    pub struct WindowsBackend;

    impl WindowsBackend {
        #[must_use]
        pub const fn new() -> Self {
            Self
        }
    }

    impl Default for WindowsBackend {
        fn default() -> Self {
            Self::new()
        }
    }

    impl InputBackend for WindowsBackend {
        fn run(&mut self, _sink: Box<dyn EventSink>) -> Result<(), BackendError> {
            Err(BackendError::Other(
                "sledge_windows backend is not yet implemented".into(),
            ))
        }

        fn inject(&self, _action: &Action) -> Result<(), BackendError> {
            Err(BackendError::Other(
                "sledge_windows backend is not yet implemented".into(),
            ))
        }
    }
}

#[cfg(target_os = "windows")]
pub use imp::WindowsBackend;
