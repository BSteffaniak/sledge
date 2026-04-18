//! Modifier-tap finite state machine.
//!
//! Tracks, per (modifier-key, side) combination, how many consecutive taps
//! have been observed within a configurable window. Any non-modifier key
//! press, or a press of a _different_ modifier, resets the counter.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::event::{KeyCode, Modifiers};

/// State machine for modifier-tap triggers.
#[derive(Debug, Default)]
pub struct TapFsm {
    /// Per-modifier-key counter state.
    states: HashMap<KeyCode, TapState>,
    /// True after we observed the last modifier key going _down_ but before
    /// the corresponding key-up. Used to distinguish "tap" (down + up with
    /// no other input) from "hold" (down, then other input).
    active_key: Option<KeyCode>,
    /// Timestamp of the key-down of `active_key`.
    active_down_at: Option<Instant>,
    /// If true, the current active-key press is no longer a candidate for
    /// being counted as a tap (because another key went down, or a different
    /// modifier went down).
    active_tainted: bool,
}

#[derive(Debug, Default, Clone, Copy)]
struct TapState {
    count: u32,
    last_tap_at: Option<Instant>,
}

/// Result of feeding an event to the FSM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapResult {
    /// Nothing fired.
    None,
    /// The given modifier has been tapped this many times in a row within
    /// the configured window. Use this plus the rule's configured `count`
    /// and `within_ms` to decide whether a rule fires.
    Tapped { key: KeyCode, count: u32 },
}

impl TapFsm {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a modifier-state-change event.
    ///
    /// `key` is the modifier whose state changed; `was_down` is the state
    /// _before_ the change; `new_mods` is the aggregate modifier state after
    /// the change. `now` is the timestamp of the event.
    pub fn on_modifier_change(
        &mut self,
        key: KeyCode,
        was_down: bool,
        new_mods: Modifiers,
        now: Instant,
    ) -> TapResult {
        debug_assert!(key.is_modifier());
        let is_down = (new_mods & key.modifier_bit()) == key.modifier_bit();

        match (was_down, is_down) {
            (false, true) => {
                // Key going down. If any other modifier is already held,
                // reset this key's counter.
                let other_mods = new_mods - key.modifier_bit() - key.generic_bit();
                if other_mods.any() {
                    self.reset(key);
                    self.active_key = None;
                    return TapResult::None;
                }
                self.active_key = Some(key);
                self.active_down_at = Some(now);
                self.active_tainted = false;
                TapResult::None
            }
            (true, false) => {
                // Key going up. If this release corresponds to the active
                // tap candidate and nothing tainted it, count it.
                if self.active_key == Some(key) && !self.active_tainted {
                    self.active_key = None;
                    self.active_down_at = None;
                    let state = self.states.entry(key).or_default();
                    let within = state
                        .last_tap_at
                        .is_some_and(|t| now.duration_since(t) <= Duration::from_millis(800));
                    if within {
                        state.count += 1;
                    } else {
                        state.count = 1;
                    }
                    state.last_tap_at = Some(now);
                    return TapResult::Tapped {
                        key,
                        count: state.count,
                    };
                }
                // Some other modifier going up, or active press was
                // tainted. Don't count.
                if self.active_key == Some(key) {
                    self.active_key = None;
                    self.active_down_at = None;
                    self.active_tainted = false;
                }
                TapResult::None
            }
            _ => TapResult::None,
        }
    }

    /// Feed a non-modifier key event. Taints the current active tap
    /// candidate so the in-progress modifier press is not counted as a tap.
    pub fn on_other_key(&mut self) {
        if self.active_key.is_some() {
            self.active_tainted = true;
        }
        // Reset all counters: once a non-modifier is pressed, any in-flight
        // tap sequence is over.
        self.states.clear();
    }

    /// Reset the counter for a single modifier key.
    fn reset(&mut self, key: KeyCode) {
        self.states.remove(&key);
    }
}

// Helper extension: map a side-specific modifier code to its "generic" bit
// (e.g. `RightAlt` -> `Modifiers::ALT`).
trait KeyCodeGenericBit {
    fn generic_bit(self) -> Modifiers;
}

impl KeyCodeGenericBit for KeyCode {
    fn generic_bit(self) -> Modifiers {
        match self {
            Self::LeftShift | Self::RightShift => Modifiers::SHIFT,
            Self::LeftCtrl | Self::RightCtrl => Modifiers::CTRL,
            Self::LeftAlt | Self::RightAlt => Modifiers::ALT,
            Self::LeftCmd | Self::RightCmd => Modifiers::CMD,
            Self::Fn => Modifiers::FN,
            _ => Modifiers::empty(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(ms: u64) -> Instant {
        // All tests use a single base; add offsets in ms.
        static BASE: std::sync::LazyLock<Instant> = std::sync::LazyLock::new(Instant::now);
        *BASE + Duration::from_millis(ms)
    }

    #[test]
    fn single_tap_counts_as_one() {
        let mut fsm = TapFsm::new();
        let r1 = fsm.on_modifier_change(
            KeyCode::RightAlt,
            false,
            Modifiers::RIGHT_ALT | Modifiers::ALT,
            t(0),
        );
        let r2 = fsm.on_modifier_change(KeyCode::RightAlt, true, Modifiers::empty(), t(50));
        assert_eq!(r1, TapResult::None);
        assert_eq!(
            r2,
            TapResult::Tapped {
                key: KeyCode::RightAlt,
                count: 1,
            }
        );
    }

    #[test]
    fn triple_tap_increments_count() {
        let mut fsm = TapFsm::new();
        for (i, ms) in [
            (false, 0u64),
            (true, 50),
            (false, 100),
            (true, 150),
            (false, 200),
            (true, 250),
        ]
        .iter()
        .enumerate()
        {
            let was_down = ms.0;
            let at = t(ms.1);
            let mods = if was_down {
                Modifiers::empty()
            } else {
                Modifiers::RIGHT_ALT | Modifiers::ALT
            };
            let r = fsm.on_modifier_change(KeyCode::RightAlt, was_down, mods, at);
            if i % 2 == 1 {
                // up events => should report tap
                let expected = (u32::try_from(i).unwrap() / 2) + 1;
                assert_eq!(
                    r,
                    TapResult::Tapped {
                        key: KeyCode::RightAlt,
                        count: expected,
                    }
                );
            }
        }
    }

    #[test]
    fn another_key_resets_count() {
        let mut fsm = TapFsm::new();
        fsm.on_modifier_change(
            KeyCode::RightAlt,
            false,
            Modifiers::RIGHT_ALT | Modifiers::ALT,
            t(0),
        );
        fsm.on_modifier_change(KeyCode::RightAlt, true, Modifiers::empty(), t(50));
        fsm.on_other_key();
        let r = fsm.on_modifier_change(
            KeyCode::RightAlt,
            false,
            Modifiers::RIGHT_ALT | Modifiers::ALT,
            t(100),
        );
        let r2 = fsm.on_modifier_change(KeyCode::RightAlt, true, Modifiers::empty(), t(150));
        assert_eq!(r, TapResult::None);
        assert_eq!(
            r2,
            TapResult::Tapped {
                key: KeyCode::RightAlt,
                count: 1,
            }
        );
    }

    #[test]
    fn holding_other_modifier_does_not_count() {
        let mut fsm = TapFsm::new();
        // Right Alt down WITH Shift already held -> do not count.
        let r = fsm.on_modifier_change(
            KeyCode::RightAlt,
            false,
            Modifiers::RIGHT_ALT | Modifiers::ALT | Modifiers::SHIFT | Modifiers::LEFT_SHIFT,
            t(0),
        );
        let r2 = fsm.on_modifier_change(
            KeyCode::RightAlt,
            true,
            Modifiers::SHIFT | Modifiers::LEFT_SHIFT,
            t(50),
        );
        assert_eq!(r, TapResult::None);
        assert_eq!(r2, TapResult::None);
    }
}
