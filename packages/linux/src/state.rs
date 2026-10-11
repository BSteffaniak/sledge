//! Per-proxy physical/output ownership. Decisions latch on key-down through key-up.
use crate::protocol::{Operation, RawEvent};
use std::collections::BTreeSet;

pub const MAX_KEY: u16 = 767;

#[derive(Clone, Default)]
pub struct KeyState {
    physical: BTreeSet<u16>,
    output: BTreeSet<u16>,
    committed_output: BTreeSet<u16>,
    swallowed: BTreeSet<u16>,
    dropping: bool,
}

impl KeyState {
    /// Commit only after a complete output frame was accepted by uinput.
    pub fn commit_output(&mut self) {
        self.committed_output = self.output.clone();
    }
    /// Roll back speculative output while preserving physical/swallowed ownership.
    pub fn rollback_output(&mut self) {
        self.output = self.committed_output.clone();
    }
    #[must_use]
    pub fn physical(&self) -> &BTreeSet<u16> {
        &self.physical
    }

    /// Non-key events retain their original fields. Unknown keys pass unchanged.
    pub fn apply(&mut self, event: RawEvent, decision: Operation) -> Vec<RawEvent> {
        if !matches!(decision, Operation::Pass | Operation::Swallow) {
            return Vec::new();
        }
        if event.event_type == 0 && event.code == 3 {
            self.dropping = true;
            return Vec::new();
        }
        if self.dropping {
            return Vec::new();
        }
        if event.event_type != 1 {
            return vec![event];
        }
        if event.code > MAX_KEY || !(0..=2).contains(&event.value) {
            return Vec::new();
        }
        match event.value {
            0 => {
                self.physical.remove(&event.code);
                self.swallowed.remove(&event.code);
                if self.output.remove(&event.code) {
                    vec![event]
                } else {
                    Vec::new()
                }
            }
            2 => {
                if self.output.contains(&event.code) {
                    vec![event]
                } else {
                    Vec::new()
                }
            }
            _ => self.press(event, decision),
        }
    }

    fn press(&mut self, event: RawEvent, decision: Operation) -> Vec<RawEvent> {
        if !self.physical.insert(event.code) {
            return Vec::new();
        }
        if decision == Operation::Pass {
            self.output.insert(event.code);
            return vec![event];
        }
        self.swallowed.insert(event.code);
        Vec::new()
    }

    /// After SYN_DROPPED, query EVIOCGKEY at the next SYN_REPORT. Reconcile
    /// releases first, preserve swallowed ownership, and pass newly discovered keys.
    pub fn resync(&mut self, keys: BTreeSet<u16>) -> Vec<RawEvent> {
        let mut events: Vec<_> = self
            .output
            .difference(&keys)
            .map(|&code| key(code, 0))
            .collect();
        self.output.retain(|code| keys.contains(code));
        self.swallowed.retain(|code| keys.contains(code));
        for &code in &keys {
            if code <= MAX_KEY && !self.output.contains(&code) && !self.swallowed.contains(&code) {
                self.output.insert(code);
                events.push(key(code, 1));
            }
        }
        self.physical = keys;
        self.dropping = false;
        events
    }

    #[must_use]
    pub const fn dropping(&self) -> bool {
        self.dropping
    }

    /// Release only output owned by this proxy, before destroying output/ungrabbing.
    pub fn release_all(&mut self) -> Vec<RawEvent> {
        let events = self
            .output
            .union(&self.committed_output)
            .map(|&code| key(code, 0))
            .collect();
        *self = Self::default();
        events
    }
}

#[must_use]
pub const fn key(code: u16, value: i32) -> RawEvent {
    RawEvent {
        event_type: 1,
        code,
        value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn swallowed_repeat_and_release_remain_swallowed() {
        let mut state = KeyState::default();
        assert!(state.apply(key(30, 1), Operation::Swallow).is_empty());
        assert!(state.apply(key(30, 2), Operation::Pass).is_empty());
        assert!(state.apply(key(30, 0), Operation::Pass).is_empty());
    }
    #[test]
    fn passed_release_cannot_be_swallowed() {
        let mut state = KeyState::default();
        assert_eq!(state.apply(key(30, 1), Operation::Pass), vec![key(30, 1)]);
        assert_eq!(
            state.apply(key(30, 0), Operation::Swallow),
            vec![key(30, 0)]
        );
    }
    #[test]
    fn resync_preserves_swallow_and_releases_owned_modifiers() {
        let mut state = KeyState::default();
        state.apply(key(42, 1), Operation::Pass);
        state.apply(key(30, 1), Operation::Swallow);
        state.apply(
            RawEvent {
                event_type: 0,
                code: 3,
                value: 0,
            },
            Operation::Pass,
        );
        assert!(state.apply(key(31, 1), Operation::Pass).is_empty());
        assert_eq!(
            state.resync(BTreeSet::from([30, 31])),
            vec![key(42, 0), key(31, 1)]
        );
        assert_eq!(state.release_all(), vec![key(31, 0)]);
    }
    #[test]
    fn lost_frame_rolls_output_back_before_resync() {
        let mut state = KeyState::default();
        state.apply(key(30, 1), Operation::Pass);
        state.rollback_output(); // down never emitted
        assert_eq!(state.resync(BTreeSet::from([30])), vec![key(30, 1)]);
        state.commit_output();
        state.apply(key(30, 0), Operation::Pass); // release not emitted
        state.rollback_output();
        assert_eq!(state.resync(BTreeSet::new()), vec![key(30, 0)]);
        state.commit_output();
        assert!(state.release_all().is_empty());
    }
    #[test]
    fn failed_output_cleanup_covers_committed_and_speculative_keys() {
        let mut state = KeyState::default();
        state.apply(key(30, 1), Operation::Pass);
        state.commit_output();
        state.apply(key(30, 0), Operation::Pass);
        state.apply(key(31, 1), Operation::Pass);
        assert_eq!(state.release_all(), vec![key(30, 0), key(31, 0)]);
    }
    #[test]
    fn unsupported_replacement_never_synthesizes_or_mutates_ownership() {
        let mut state = KeyState::default();
        state.apply(key(31, 1), Operation::Pass);
        assert!(state.apply(key(30, 1), Operation::Replace).is_empty());
    }
}
