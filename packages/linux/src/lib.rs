//! Linux backend and narrowly privileged broker components.
#[cfg(target_os = "linux")]
pub mod authorization;
#[cfg(target_os = "linux")]
mod backend;
#[cfg(target_os = "linux")]
pub mod broker;
#[cfg(target_os = "linux")]
pub mod desktop;
#[cfg(target_os = "linux")]
pub mod hyprland;
#[cfg(target_os = "linux")]
pub mod ipc;
#[cfg(target_os = "linux")]
pub mod keymap;
#[cfg(target_os = "linux")]
pub mod lifecycle;
#[cfg(target_os = "linux")]
pub mod logind;
#[cfg(target_os = "linux")]
pub mod protocol;
#[cfg(target_os = "linux")]
pub mod state;
#[cfg(target_os = "linux")]
pub use authorization::authorize_socket_peer;
#[cfg(target_os = "linux")]
pub use backend::{Capabilities, FocusTracker, LinuxBackend, ShutdownHandle, SourceDispatcher};
