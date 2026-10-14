//! Rule definitions and matcher.
//!
//! A [`RuleSet`] is compiled from configuration and consumes [`KeyEvent`]s
//! along with focused-app information, emitting [`Verdict`]s that tell the
//! platform backend what to do.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use tracing::{debug, trace};

use crate::action::Action;
use crate::event::{DeviceId, EventKind, KeyCode, KeyEvent};
use crate::tap_fsm::{TapFsm, TapResult};
use crate::trigger::{HotkeyTrigger, Trigger};

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

/// Opaque acknowledgement identity, scoped to the matcher that issued it.
/// Never reuse a token with a different matcher instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplacementToken {
    device: DeviceId,
    key: KeyCode,
    generation: u64,
}

/// Actual compositor delivery result, not queue admission or an IPC timeout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplacementDelivery {
    /// The complete replacement was committed; keep consumed ownership.
    Delivered,
    /// No replacement output committed and the original was actually forwarded.
    /// A timeout qualifies only if the backend can prove both conditions.
    RejectedOriginalForwarded,
    /// Partial output or uncertain commit. NEVER replay the original; backend
    /// output reconciliation is required. Keep consumed ownership meanwhile.
    Uncertain,
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
    tap_fsms: HashMap<DeviceId, Vec<TapFsm>>,
    /// Track which modifier keys are currently down so the FSM can be fed
    /// `was_down` correctly.
    modifier_state: HashMap<(DeviceId, KeyCode), bool>,
    /// Track key codes whose most recent `KeyDown` was consumed by a rule
    /// (`Verdict::Replace` or `Verdict::Swallow`). When the matching
    /// `KeyUp` arrives we also swallow it so the consumer app never sees
    /// a bare up-without-down. Without this, apps watching for matched
    /// press/release pairs (e.g. zellij via the kitty keyboard protocol)
    /// observe a spurious lingering key-up event, which they may mis-
    /// interpret as triggering their own binding for that key.
    swallowed_downs: HashSet<(DeviceId, KeyCode)>,
    /// Failed replacements whose original down reached the consumer.
    forwarded_downs: HashSet<(DeviceId, KeyCode)>,
    pending_deliveries: HashMap<(DeviceId, KeyCode), u64>,
    /// Cached nonempty pending/forwarded state gates legacy event overhead.
    delivery_active: bool,
    // Never reset/reuse generations, including after device/global reset.
    delivery_generation: u64,
}

impl Matcher {
    /// Create a new matcher over `rules`.
    #[must_use]
    pub fn new(rules: RuleSet) -> Self {
        Self {
            rules,
            tap_fsms: HashMap::new(),
            modifier_state: HashMap::new(),
            swallowed_downs: HashSet::new(),
            forwarded_downs: HashSet::new(),
            pending_deliveries: HashMap::new(),
            delivery_active: false,
            delivery_generation: 0,
        }
    }

    /// Replace the live rule set atomically.
    pub fn swap_rules(&mut self, new_rules: RuleSet) {
        debug!(count = new_rules.len(), "swapping rule set");
        self.rules = new_rules;
        // Reset tap state; no reason to preserve it across reloads.
        self.tap_fsms.clear();
        // Consumed physical presses remain ours until release, even across reload.
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
        self.dispatch_device(DeviceId::default(), event, focused_app, now)
    }

    /// Dispatch a compositor replacement decision with a one-shot delivery token.
    ///
    /// Only `Replace` issues a token. Until acknowledgement, consumed downs
    /// stay consumed. Serialize dispatch and delivery reports with physical
    /// events: rejection must be reported after forwarding the original and
    /// before routing its repeats/release. No rollback of tap recognition occurs.
    /// Release-triggered replacements can be acknowledged but own no held down.
    /// Existing [`Self::dispatch_device`] callers need not use this API.
    ///
    /// # Panics
    ///
    /// Panics if the matcher exhausts its non-reusable u64 token generations.
    pub fn dispatch_device_with_delivery(
        &mut self,
        device: DeviceId,
        event: KeyEvent,
        focused_app: Option<&str>,
        now: Instant,
    ) -> (Verdict, Option<ReplacementToken>) {
        let verdict = self.dispatch_device(device, event, focused_app, now);
        let token = if matches!(verdict, Verdict::Replace(_)) {
            self.delivery_generation = self
                .delivery_generation
                .checked_add(1)
                .expect("replacement token generations exhausted");
            self.pending_deliveries
                .insert((device, event.code), self.delivery_generation);
            self.delivery_active = true;
            Some(ReplacementToken {
                device,
                key: event.code,
                generation: self.delivery_generation,
            })
        } else {
            None
        };
        (verdict, token)
    }

    /// Settle exactly one replacement. Returns false for stale, duplicate or
    /// reset tokens without changing ownership. Tokens survive rule reload and
    /// focus changes, but not release/new press or stream reset. Tokens must
    /// originate from this matcher instance.
    ///
    /// Only definite precommit rejection with actual original delivery permits
    /// fallback. This changes only that press; unrelated ownership and tap FSMs
    /// are untouched. Delivered/uncertain results never permit original replay.
    pub fn report_delivery(
        &mut self,
        token: ReplacementToken,
        delivery: ReplacementDelivery,
    ) -> bool {
        let press = (token.device, token.key);
        if self.pending_deliveries.get(&press) != Some(&token.generation) {
            return false;
        }
        self.pending_deliveries.remove(&press);
        if delivery == ReplacementDelivery::RejectedOriginalForwarded
            && self.swallowed_downs.remove(&press)
        {
            self.forwarded_downs.insert(press);
        }
        self.refresh_delivery_active();
        true
    }

    /// Invalidate speculative decisions after focus, connection, or keymap changes.
    ///
    /// Unlike [`Self::reset`], this preserves committed press ownership and
    /// physical modifier transitions until release. Outstanding replacements
    /// become uncertain: stale acknowledgements cannot authorize original replay.
    /// Backends must reconcile output separately; this does not recover a lost
    /// device or revoke lock routing.
    pub fn invalidate_speculation(&mut self) {
        self.tap_fsms.clear();
        self.pending_deliveries.clear();
        self.refresh_delivery_active();
    }

    /// Disqualify speculative tap recognition without retiring press ownership.
    /// Backends use this for physical input that cannot be represented as a key.
    pub fn disqualify_taps(&mut self) {
        for fsms in self.tap_fsms.values_mut() {
            for fsm in fsms {
                fsm.on_other_key();
            }
        }
    }

    /// Observe a backend-owned repeat/release without evaluating rules or
    /// producing actions. The backend must still reconcile its own output.
    /// Releases retire only the exact physical press; speculative tap state for
    /// that device is discarded rather than firing a release-triggered action.
    /// This is not an input replay or a delivery acknowledgment.
    pub fn settle_device_event(&mut self, device: DeviceId, event: KeyEvent) {
        if event.kind != EventKind::KeyUp {
            return;
        }
        let press = (device, event.code);
        self.swallowed_downs.remove(&press);
        self.forwarded_downs.remove(&press);
        self.pending_deliveries.remove(&press);
        self.modifier_state.remove(&press);
        self.tap_fsms.remove(&device);
        self.refresh_delivery_active();
    }

    /// Clear a lost/disconnected stream. The backend must reconcile any
    /// forwarded or injected held keys itself before resuming delivery.
    pub fn reset_device(&mut self, device: DeviceId) {
        self.tap_fsms.remove(&device);
        self.modifier_state.retain(|(id, _), _| *id != device);
        self.swallowed_downs.retain(|(id, _)| *id != device);
        self.forwarded_downs.retain(|(id, _)| *id != device);
        self.pending_deliveries.retain(|(id, _), _| *id != device);
        self.refresh_delivery_active();
    }

    /// Clear all stream state after a global discontinuity.
    pub fn reset(&mut self) {
        self.tap_fsms.clear();
        self.modifier_state.clear();
        self.swallowed_downs.clear();
        self.forwarded_downs.clear();
        self.pending_deliveries.clear();
        self.delivery_active = false;
    }

    fn refresh_delivery_active(&mut self) {
        self.delivery_active =
            !self.pending_deliveries.is_empty() || !self.forwarded_downs.is_empty();
    }

    /// Also used by legacy dispatch when callers mix entry points.
    fn reconcile_delivery_event(&mut self, press: (DeviceId, KeyCode), kind: EventKind) -> bool {
        // Release (including tap-triggered replacement) retires the previous
        // decision. A fresh down also invalidates a release-triggered token.
        if kind != EventKind::KeyDown
            || (!self.swallowed_downs.contains(&press) && !self.forwarded_downs.contains(&press))
        {
            self.pending_deliveries.remove(&press);
        }
        let forwarded = match kind {
            EventKind::KeyUp => self.forwarded_downs.remove(&press),
            EventKind::KeyDown => self.forwarded_downs.contains(&press),
            EventKind::ModifiersChanged => false,
        };
        self.refresh_delivery_active();
        forwarded
    }

    /// Device-aware dispatch. Modifier `KeyDown`/`KeyUp` are physical
    /// transitions; `ModifiersChanged` is supported only when a nonempty
    /// modifier bit can prove the transition (not Caps Lock lock-state).
    pub fn dispatch_device(
        &mut self,
        device: DeviceId,
        event: KeyEvent,
        focused_app: Option<&str>,
        now: Instant,
    ) -> Verdict {
        trace!(?event, ?focused_app, "matcher dispatch");
        let press = (device, event.code);
        if self.delivery_active && self.reconcile_delivery_event(press, event.kind) {
            return Verdict::Pass;
        }
        if event.kind == EventKind::KeyUp && self.swallowed_downs.remove(&press) {
            return Verdict::Swallow;
        }
        if event.kind == EventKind::KeyDown && self.swallowed_downs.contains(&press) {
            return Verdict::Swallow;
        }

        let fsms = self.tap_fsms.entry(device).or_insert_with(|| {
            self.rules
                .taps
                .iter()
                .map(|rule| {
                    let Trigger::Tap(trig) = &rule.trigger else {
                        unreachable!()
                    };
                    TapFsm::with_window(Duration::from_millis(u64::from(trig.within_ms)))
                })
                .collect()
        });
        if event.code.is_modifier() {
            let was_down = *self.modifier_state.get(&press).unwrap_or(&false);
            let is_down = match event.kind {
                EventKind::KeyDown => true,
                EventKind::KeyUp => false,
                EventKind::ModifiersChanged => {
                    let bit = event.code.modifier_bit();
                    if bit.is_empty() {
                        return Verdict::Pass;
                    }
                    event.mods.contains(bit)
                }
            };
            self.modifier_state.insert(press, is_down);
            if is_down
                && !was_down
                && self
                    .modifier_state
                    .iter()
                    .any(|((id, key), down)| *id == device && *key != event.code && *down)
            {
                for fsm in fsms {
                    fsm.on_other_key();
                }
                return Verdict::Pass;
            }
            let mut action = None;
            for (rule, fsm) in self.rules.taps.iter().zip(fsms.iter_mut()) {
                let result = fsm.on_transition(event.code, was_down, is_down, event.mods, now);
                if let (TapResult::Tapped { key, count }, Trigger::Tap(trig)) =
                    (result, &rule.trigger)
                {
                    if action.is_none()
                        && trig.tap == key
                        && trig.count == count
                        && RuleSet::scope_matches(rule, focused_app)
                    {
                        action = Some(rule.action.clone());
                    }
                }
            }
            if let Some(action) = action {
                return Verdict::Replace(action);
            }
            return Verdict::Pass;
        }

        // Non-modifier events taint any in-flight tap candidate.
        if matches!(event.kind, EventKind::KeyDown) {
            for fsm in fsms {
                fsm.on_other_key();
            }
        }

        // Hotkey matching on key-down only.
        if matches!(event.kind, EventKind::KeyDown) {
            for rule in &self.rules.hotkeys {
                if let Trigger::Hotkey(trig) = &rule.trigger {
                    if RuleSet::hotkey_matches(trig, event)
                        && RuleSet::scope_matches(rule, focused_app)
                    {
                        // Record that we're consuming this code's down so
                        // the matching up is also suppressed (see the
                        // KeyUp branch below for rationale).
                        self.swallowed_downs.insert(press);
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
    use crate::{Modifiers, TapTrigger};

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

    #[test]
    fn triple_tap_right_alt_fires_with_production_mods_shape() {
        // Regression test for a bug where `flags_to_mods` on macOS did not
        // set side-specific modifier bits, causing the tap FSM to see
        // `is_down=false` for every RightAlt transition and never register
        // a tap. This test exercises the matcher with the exact `Modifiers`
        // shape the production backend now emits (generic + side bits).
        let rule = Rule {
            trigger: Trigger::Tap(TapTrigger {
                tap: KeyCode::RightAlt,
                count: 3,
                within_ms: 600,
            }),
            action: Action::SendKey {
                key: KeyCode::Return,
                mods: Modifiers::CTRL,
            },
            when_app_in: None,
        };
        let rules = RuleSet::new(vec![rule]);
        let mut m = Matcher::new(rules);

        let base = Instant::now();
        let down = Modifiers::ALT | Modifiers::RIGHT_ALT;
        let up = Modifiers::empty();

        let mk = |mods: Modifiers| KeyEvent {
            code: KeyCode::RightAlt,
            kind: EventKind::ModifiersChanged,
            mods,
        };

        // Tap 1: down, up (0-100ms)
        assert!(matches!(m.dispatch(mk(down), None, base), Verdict::Pass));
        assert!(matches!(
            m.dispatch(mk(up), None, base + std::time::Duration::from_millis(100)),
            Verdict::Pass
        ));

        // Tap 2: down, up (150-250ms)
        assert!(matches!(
            m.dispatch(mk(down), None, base + std::time::Duration::from_millis(150)),
            Verdict::Pass
        ));
        assert!(matches!(
            m.dispatch(mk(up), None, base + std::time::Duration::from_millis(250)),
            Verdict::Pass
        ));

        // Tap 3: down, up (300-400ms) \u2014 the third up must fire the rule.
        assert!(matches!(
            m.dispatch(mk(down), None, base + std::time::Duration::from_millis(300)),
            Verdict::Pass
        ));
        match m.dispatch(mk(up), None, base + std::time::Duration::from_millis(400)) {
            Verdict::Replace(Action::SendKey { key, mods }) => {
                assert_eq!(key, KeyCode::Return);
                assert_eq!(mods, Modifiers::CTRL);
            }
            other => panic!("expected Replace(Ctrl+Return), got {other:?}"),
        }
    }

    #[test]
    fn hotkey_replace_also_swallows_matching_key_up() {
        // Regression test: a hotkey rule that replaces Alt+K with Alt+T
        // must swallow not only the Alt+K KeyDown but also its matching
        // KeyUp. Otherwise consumers that track press/release pairs
        // (e.g. zellij with the kitty keyboard protocol) observe a bare
        // up-without-down and may mis-interpret it as a spurious trigger
        // for their own binding on that key.
        let rule = hk(
            KeyCode::KeyK,
            Modifiers::ALT,
            Action::SendKey {
                key: KeyCode::KeyT,
                mods: Modifiers::ALT,
            },
        );
        let rules = RuleSet::new(vec![rule]);
        let mut m = Matcher::new(rules);

        let now = Instant::now();

        // KeyDown should produce a Replace verdict.
        let down = kdown(KeyCode::KeyK, Modifiers::ALT);
        match m.dispatch(down, None, now) {
            Verdict::Replace(Action::SendKey { key, mods }) => {
                assert_eq!(key, KeyCode::KeyT);
                assert_eq!(mods, Modifiers::ALT);
            }
            other => panic!("expected Replace(Alt+T), got {other:?}"),
        }

        // KeyUp for the same code must be swallowed, even if the modifier
        // bits have changed (e.g. user released Alt before K).
        let up = KeyEvent {
            code: KeyCode::KeyK,
            kind: EventKind::KeyUp,
            mods: Modifiers::empty(),
        };
        let v = m.dispatch(up, None, now + std::time::Duration::from_millis(50));
        assert!(
            matches!(v, Verdict::Swallow),
            "expected KeyUp to be Swallow, got {v:?}",
        );

        // A subsequent unrelated KeyUp should NOT be swallowed.
        let other_up = KeyEvent {
            code: KeyCode::KeyJ,
            kind: EventKind::KeyUp,
            mods: Modifiers::empty(),
        };
        let v = m.dispatch(other_up, None, now + std::time::Duration::from_millis(100));
        assert!(
            matches!(v, Verdict::Pass),
            "expected unrelated KeyUp to Pass, got {v:?}",
        );

        // A KeyUp for our code that has NOT had a matching swallowed
        // down should Pass (state was cleared by the first matching up).
        let repeat_up = KeyEvent {
            code: KeyCode::KeyK,
            kind: EventKind::KeyUp,
            mods: Modifiers::empty(),
        };
        let v = m.dispatch(repeat_up, None, now + std::time::Duration::from_millis(150));
        assert!(
            matches!(v, Verdict::Pass),
            "expected stale KeyUp to Pass, got {v:?}",
        );
    }

    #[test]
    fn hotkey_out_of_scope_does_not_swallow_key_up() {
        // If the hotkey rule is scoped to a specific app and we're not
        // in that app, the KeyDown should Pass and the KeyUp should also
        // Pass (no swallow bookkeeping recorded).
        let mut rule = hk(
            KeyCode::KeyK,
            Modifiers::ALT,
            Action::SendKey {
                key: KeyCode::KeyT,
                mods: Modifiers::ALT,
            },
        );
        rule.when_app_in = Some(vec!["ghostty".to_string()]);
        let rules = RuleSet::new(vec![rule]);
        let mut m = Matcher::new(rules);

        let now = Instant::now();
        let down = kdown(KeyCode::KeyK, Modifiers::ALT);
        assert!(matches!(
            m.dispatch(down, Some("safari"), now),
            Verdict::Pass
        ));

        let up = KeyEvent {
            code: KeyCode::KeyK,
            kind: EventKind::KeyUp,
            mods: Modifiers::ALT,
        };
        assert!(matches!(
            m.dispatch(
                up,
                Some("safari"),
                now + std::time::Duration::from_millis(50)
            ),
            Verdict::Pass
        ));
    }
}
