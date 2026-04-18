//! macOS backend for sledge: CGEventTap + TIS + NSWorkspace.

#![cfg(target_os = "macos")]

// Link the system frameworks we need. `AppKit` is pulled in transitively
// through `objc2-app-kit`; `IOKit` (IOHIDCheckAccess) and `Carbon`
// (Text Input Sources) are not, so we declare them here.
#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {}

pub mod bundle;
pub mod focus;
pub mod inject;
pub mod keycode;
pub mod permission;
pub mod tap;
pub mod tis;

pub use bundle::SLEDGE_BUNDLE_ID;
pub use focus::FocusTracker;
pub use inject::SLEDGE_EVENT_SOURCE_TAG;
pub use permission::{PermissionStatus, check_permissions};
pub use tap::MacOsBackend;
