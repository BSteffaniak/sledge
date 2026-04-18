//! Rule definitions and matcher.
//!
//! A [`RuleSet`] is compiled from configuration and consumes [`KeyEvent`]s
//! along with focused-app information, emitting [`Verdict`]s that tell the
//! platform backend what to do.

use std::collections::HashMap;
use std::time::Instant;

use tracing::{debug, trace};

use crate::action::Action;
use crate::event::{EventKind, KeyCode, KeyEvent};
use crate::tap_fsm::{TapFsm, TapResult};
use crate::trigger::{HotkeyTrigger, TapTrigger, Trigger};

/// A compiled rule. Cheap to clone.
#[derive(Debug, Clone)]
pub struct Rule {
    pub trigger: Trigger,
    pub action: Action,
    /// If `Some`, rule only fires when the focused app id is in this set.
    pub when_app_in: Option<Vec<String>>,
}

/// Result of feeding an event to a [`RuleSet`].
#[derive(Debug, Clone)]
pub enum Verdict {
    /// Let the event through unmodified.
    Pass,
    /// Drop the event entirely.
    Swallow,
    /// Drop the original and execute this action instead.
    Replace(Action),
}

/// The compiled, immutable set of rules the engine is matching against.
#[derive(Debug, Clone)]
pub struct RuleSet {
    hotkeys: Vec<Rule>,
    taps: Vec<Rule>,
}

impl RuleSet {
    /// Build a rule set from pre-validated rules.
    #[must_use]
    pub fn new(rules: Vec<Rule>) -> Self {
        let mut hotkeys = Vec::new();
        let mut taps = Vec::new();
        for r in rules {
            match r.trigger {
                Trigger::Hotkey(_) => hotkeys.push(r),
                Trigger::Tap(_) => taps.push(r),
            }
        }
        Self { hotkeys, taps }
    }

    /// Number of rules in the set.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.hotkeys.len() + self.taps.len()
    }

    /// Whether the set is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.hotkeys.is_empty() && self.taps.is_empty()
    }

    /// Iterate over all rules.
    pub fn iter(&self) -> impl Iterator<Item = &Rule> {
        self.hotkeys.iter().chain(self.taps.iter())
    }

    fn hotkey_matches(trig: &HotkeyTrigger, evt: KeyEvent) -> bool {
        evt.kind == EventKind::KeyDown && evt.code == trig.key && evt.mods.matches(trig.mods)
    }

    fn scope_matches(rule: &Rule, focused_app: Option<&str>) -> bool {
        match (&rule.when_app_in, focused_app) {
            (None, _) => true,
            (Some(list), Some(app)) => list.iter().any(|a| a == app),
            (Some(_), None) => false,
        }
    }
}

/// The stateful matcher. Owns the tap FSM and a compiled rule set; emits
/// verdicts in response to [`KeyEvent`]s.
pub struct Matcher {
    rules: RuleSet,
    tap_fsm: TapFsm,
    /// Track which modifier keys are currently down so the FSM can be fed
    /// `was_down` correctly.
    modifier_state: HashMap<KeyCode, bool>,
}

impl Matcher {
    /// Create a new matcher over `rules`.
    #[must_use]
    pub fn new(rules: RuleSet) -> Self {
        Self {
            rules,
            tap_fsm: TapFsm::new(),
            modifier_state: HashMap::new(),
        }
    }

    /// Replace the live rule set atomically.
    pub fn swap_rules(&mut self, new_rules: RuleSet) {
        debug!(count = new_rules.len(), "swapping rule set");
        self.rules = new_rules;
        // Reset tap state; no reason to preserve it across reloads.
        self.tap_fsm = TapFsm::new();
    }

    /// Access the current rule set.
    #[must_use]
    pub const fn rules(&self) -> &RuleSet {
        &self.rules
    }

    /// Decide what to do with an incoming event.
    ///
    /// `focused_app` is the logical app identifier (already resolved via
    /// any alias table), or `None` if not known.
    pub fn dispatch(
        &mut self,
        event: KeyEvent,
        focused_app: Option<&str>,
        now: Instant,
    ) -> Verdict {
        trace!(?event, ?focused_app, "matcher dispatch");

        // Modifier changes feed the tap FSM.
        if matches!(event.kind, EventKind::ModifiersChanged) && event.code.is_modifier() {
            let was_down = *self.modifier_state.get(&event.code).unwrap_or(&false);
            let is_down = (event.mods & event.code.modifier_bit()) == event.code.modifier_bit();
            self.modifier_state.insert(event.code, is_down);

            let result = self
                .tap_fsm
                .on_modifier_change(event.code, was_down, event.mods, now);
            if let TapResult::Tapped { key, count } = result {
                for rule in &self.rules.taps {
                    if let Trigger::Tap(TapTrigger {
                        tap,
                        count: wanted,
                        within_ms: _,
                    }) = &rule.trigger
                    {
                        if *tap == key
                            && *wanted == count
                            && RuleSet::scope_matches(rule, focused_app)
                        {
                            return Verdict::Replace(rule.action.clone());
                        }
                    }
                }
            }
            return Verdict::Pass;
        }

        // Non-modifier events taint any in-flight tap candidate.
        if matches!(event.kind, EventKind::KeyDown) && !event.code.is_modifier() {
            self.tap_fsm.on_other_key();
        }

        // Hotkey matching on key-down only.
        if matches!(event.kind, EventKind::KeyDown) {
            for rule in &self.rules.hotkeys {
                if let Trigger::Hotkey(trig) = &rule.trigger {
                    if RuleSet::hotkey_matches(trig, event)
                        && RuleSet::scope_matches(rule, focused_app)
                    {
                        return Verdict::Replace(rule.action.clone());
                    }
                }
            }
        }

        Verdict::Pass
    }
}

// Make the helpers accessible as associated functions for callers with a
// `RuleSet` reference (used in tests).
impl RuleSet {
    #[doc(hidden)]
    pub fn __hotkey_matches(trig: &HotkeyTrigger, evt: KeyEvent) -> bool {
        Self::hotkey_matches(trig, evt)
    }

    #[doc(hidden)]
    pub fn __scope_matches(rule: &Rule, focused_app: Option<&str>) -> bool {
        Self::scope_matches(rule, focused_app)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Modifiers;

    fn hk(key: KeyCode, mods: Modifiers, action: Action) -> Rule {
        Rule {
            trigger: Trigger::Hotkey(HotkeyTrigger { key, mods }),
            action,
            when_app_in: None,
        }
    }

    fn kdown(code: KeyCode, mods: Modifiers) -> KeyEvent {
        KeyEvent {
            code,
            kind: EventKind::KeyDown,
            mods,
        }
    }

    #[test]
    fn hotkey_fires_on_exact_modifier_match() {
        let rules = RuleSet::new(vec![hk(
            KeyCode::Digit1,
            Modifiers::CMD | Modifiers::ALT,
            Action::SetInputSource { id: "us".into() },
        )]);
        let mut m = Matcher::new(rules);
        let v = m.dispatch(
            kdown(KeyCode::Digit1, Modifiers::CMD | Modifiers::ALT),
            None,
            Instant::now(),
        );
        match v {
            Verdict::Replace(Action::SetInputSource { id }) => assert_eq!(id, "us"),
            other => panic!("unexpected verdict: {other:?}"),
        }
    }

    #[test]
    fn hotkey_does_not_fire_without_mods() {
        let rules = RuleSet::new(vec![hk(
            KeyCode::Digit1,
            Modifiers::CMD | Modifiers::ALT,
            Action::SetInputSource { id: "us".into() },
        )]);
        let mut m = Matcher::new(rules);
        let v = m.dispatch(kdown(KeyCode::Digit1, Modifiers::CMD), None, Instant::now());
        assert!(matches!(v, Verdict::Pass));
    }

    #[test]
    fn scope_filters_by_app() {
        let rule = Rule {
            trigger: Trigger::Hotkey(HotkeyTrigger {
                key: KeyCode::KeyK,
                mods: Modifiers::ALT,
            }),
            action: Action::SendKey {
                key: KeyCode::KeyT,
                mods: Modifiers::ALT,
            },
            when_app_in: Some(vec!["ghostty".into()]),
        };
        let rules = RuleSet::new(vec![rule]);
        let mut m = Matcher::new(rules);
        let v_wrong_app = m.dispatch(
            kdown(KeyCode::KeyK, Modifiers::ALT),
            Some("safari"),
            Instant::now(),
        );
        assert!(matches!(v_wrong_app, Verdict::Pass));
        let v_right_app = m.dispatch(
            kdown(KeyCode::KeyK, Modifiers::ALT),
            Some("ghostty"),
            Instant::now(),
        );
        assert!(matches!(v_right_app, Verdict::Replace(_)));
    }
}
