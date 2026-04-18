//! sledge core engine.
//!
//! Platform-agnostic types and rule-matching machinery. See the top-level
//! README for architecture notes.

pub mod action;
pub mod backend;
pub mod event;
pub mod rule;
pub mod tap_fsm;
pub mod trigger;

pub use action::Action;
pub use backend::{BackendError, BackendVerdict, EventSink, InputBackend};
pub use event::{EventKind, KeyCode, KeyEvent, Modifiers};
pub use rule::{Matcher, Rule, RuleSet, Verdict};
pub use tap_fsm::{TapFsm, TapResult};
pub use trigger::{HotkeyTrigger, TapTrigger, Trigger};
