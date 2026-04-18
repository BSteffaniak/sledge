//! Actions that rules can produce.

use crate::event::{KeyCode, Modifiers};

/// Action produced by a matched rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Synthesize a keystroke (down + up) with the given modifiers.
    SendKey { key: KeyCode, mods: Modifiers },
    /// Switch the active input source. macOS-only; no-op elsewhere.
    SetInputSource { id: String },
}
