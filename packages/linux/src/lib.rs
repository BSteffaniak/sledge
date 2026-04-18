//! Linux backend stub.
//!
//! Scaffolded for future use. Currently unimplemented.

#[cfg(target_os = "linux")]
mod imp {
    use sledge_core::{Action, BackendError, EventSink, InputBackend};

    /// Placeholder backend that returns errors for everything.
    pub struct LinuxBackend;

    impl LinuxBackend {
        #[must_use]
        pub const fn new() -> Self {
            Self
        }
    }

    impl Default for LinuxBackend {
        fn default() -> Self {
            Self::new()
        }
    }

    impl InputBackend for LinuxBackend {
        fn run(&mut self, _sink: Box<dyn EventSink>) -> Result<(), BackendError> {
            Err(BackendError::Other(
                "sledge_linux backend is not yet implemented".into(),
            ))
        }

        fn inject(&self, _action: &Action) -> Result<(), BackendError> {
            Err(BackendError::Other(
                "sledge_linux backend is not yet implemented".into(),
            ))
        }
    }
}

#[cfg(target_os = "linux")]
pub use imp::LinuxBackend;
