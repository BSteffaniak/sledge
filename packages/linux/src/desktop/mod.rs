//! Same-user desktop integrations. Poll and select only on desktop worker threads.
//! No provider here supplies authoritative broker lock permission.
mod bus;
pub mod companion;
pub mod fcitx5;
pub mod hyprland;
pub mod ibus;
pub mod operation;
pub mod sway;
mod transport;
pub mod worker;
pub mod x11;

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Maximum accepted focus age; an expired observation never matches scope.
pub const FOCUS_TTL: Duration = Duration::from_millis(500);
pub(crate) const IPC_TIMEOUT: Duration = Duration::from_millis(400);
pub(crate) const MAX_MESSAGE: usize = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FocusCategory {
    Wayland,
    X11Class,
    /// KWin exposes resourceClass for both protocols without a scripting
    /// protocol discriminator. Resolve only explicit legacy aliases.
    KwinResourceClass,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FocusIdentity {
    pub category: FocusCategory,
    pub value: String,
}

impl FocusIdentity {
    /// Explicit category resolution allows callers to consult typed aliases first,
    /// then the legacy Linux alias in this observed category only.
    pub fn new(category: FocusCategory, value: &str) -> Option<Self> {
        valid_id(value).then(|| Self {
            category,
            value: value.to_owned(),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LockHint {
    Locked,
    Unlocked,
    Unknown,
}

#[derive(Clone, Debug)]
pub struct DesktopSnapshot {
    pub focus: Option<FocusIdentity>,
    pub observed_at: Instant,
    pub lock_hint: LockHint,
}
impl DesktopSnapshot {
    pub fn current_focus(&self) -> Option<&FocusIdentity> {
        (self.observed_at.elapsed() <= FOCUS_TTL)
            .then_some(self.focus.as_ref())
            .flatten()
    }
    /// Hints, polling, and same-user companion messages cannot authorize grabs.
    pub const fn strict_lock_authority(&self) -> bool {
        false
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub id: String,
    pub label: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceCatalog {
    pub provider: String,
    /// Exact inventory/version token, not a process-local hash or display label.
    pub generation: String,
    pub sources: Vec<Source>,
    pub current: Option<String>,
}
impl SourceCatalog {
    pub(crate) fn check(&self, fresh: &Self, id: &str) -> Result<(), DesktopError> {
        if self.provider != fresh.provider
            || self.generation != fresh.generation
            || self.sources != fresh.sources
        {
            return Err(DesktopError::StaleInventory);
        }
        let source = fresh
            .sources
            .iter()
            .find(|source| source.id == id)
            .ok_or(DesktopError::UnknownSource)?;
        if !source.enabled {
            return Err(DesktopError::DisabledSource);
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DesktopError {
    #[error("desktop capability unavailable: {0}")]
    Unavailable(String),
    #[error("desktop protocol error: {0}")]
    Protocol(String),
    #[error("unknown input source")]
    UnknownSource,
    #[error("disabled input source")]
    DisabledSource,
    #[error("source inventory changed")]
    StaleInventory,
    #[error("source selection failed or was not confirmed")]
    SelectionFailed,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Synchronous cooperative desktop adapter. Worker calls share an aggregate
/// deadline and shutdown token through `operation::remaining`. Implementations
/// must check it before effects and bound all waits; non-cooperative custom code
/// cannot be safely killed, and joining it may delay shutdown. No method may
/// spawn detached work that continues provider effects after returning.
pub trait DesktopProvider: Send {
    /// Errors invalidate the caller's cached focus immediately.
    fn poll(&mut self) -> Result<DesktopSnapshot, DesktopError>;
    fn sources(&mut self) -> Result<SourceCatalog, DesktopError>;
    fn select(&mut self, catalog: &SourceCatalog, id: &str) -> Result<(), DesktopError>;
}

pub(crate) fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 4096 && !value.chars().any(char::is_control)
}
pub(crate) fn protocol(message: impl ToString) -> DesktopError {
    DesktopError::Protocol(message.to_string())
}
pub(crate) fn unavailable(message: impl ToString) -> DesktopError {
    DesktopError::Unavailable(message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expired_focus_is_not_scope() {
        let snapshot = DesktopSnapshot {
            focus: FocusIdentity::new(FocusCategory::Wayland, "app"),
            observed_at: Instant::now().checked_sub(FOCUS_TTL * 2).unwrap(),
            lock_hint: LockHint::Unlocked,
        };
        assert!(snapshot.current_focus().is_none());
        assert!(!snapshot.strict_lock_authority());
    }
    #[test]
    fn categories_do_not_cross_match() {
        assert_ne!(
            FocusIdentity::new(FocusCategory::Wayland, "app"),
            FocusIdentity::new(FocusCategory::X11Class, "app")
        );
        assert!(FocusIdentity::new(FocusCategory::Wayland, "bad\napp").is_none());
    }
    #[test]
    fn inventory_changes_fail_closed() {
        let catalog = SourceCatalog {
            provider: "test".into(),
            generation: "1".into(),
            sources: vec![Source {
                id: "0".into(),
                label: "US".into(),
                enabled: true,
            }],
            current: None,
        };
        let mut fresh = catalog.clone();
        fresh.generation = "2".into();
        assert!(matches!(
            catalog.check(&fresh, "0"),
            Err(DesktopError::StaleInventory)
        ));
    }
}
