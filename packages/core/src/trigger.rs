//! Trigger definitions \u2014 what causes a rule to fire.

use crate::event::{KeyCode, Modifiers};

/// A trigger is one of: a hotkey (modifiers + key) or a modifier-tap
/// (same modifier pressed N times within a window).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trigger {
    Hotkey(HotkeyTrigger),
    Tap(TapTrigger),
}

/// A simple modifiers+key hotkey.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyTrigger {
    pub key: KeyCode,
    pub mods: Modifiers,
}

/// A modifier-tap trigger. `count` must be >= 2, `within_ms` > 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapTrigger {
    pub tap: KeyCode,
    pub count: u32,
    pub within_ms: u32,
}
