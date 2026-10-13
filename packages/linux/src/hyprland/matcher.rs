//! Matcher adapter for a future qualified synchronous bridge, not activation.
use super::{
    DeliveryReport,
    wire::{Decision, Event, ResultKind, Scope},
};
use crate::desktop::{FocusCategory, FocusIdentity};
use sledge_core::{
    Action, DeviceId, EventKind, KeyEvent, Matcher, Modifiers, ReplacementDelivery,
    ReplacementToken, Verdict,
};
use std::time::Instant;

/// Recovery progress, without exposing tokens or discarding ownership.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryState {
    Unbound,
    AwaitingNativeProof,
    FreshAdmissionRequired,
    Ready,
}
/// A single outstanding opaque proposal; tokens never cross matcher instances.
pub struct MatcherAdapter {
    matcher: Matcher,
    transactions: MatcherTransactions,
}
#[derive(Clone, Copy)]
struct Pending {
    scope: Scope,
    token: ReplacementToken,
    decision: Decision,
    key: u32,
    state: u32,
    focus_epoch: u64,
}
impl MatcherAdapter {
    #[must_use]
    pub fn new(matcher: Matcher) -> Self {
        Self {
            matcher,
            transactions: MatcherTransactions::default(),
        }
    }
    /// Exact authoritative event identity. No desktop snapshot fallback.
    #[must_use]
    pub fn focus(event: &Event) -> Option<FocusIdentity> {
        let category = match event.focus_kind {
            1 => FocusCategory::Wayland,
            2 => FocusCategory::X11Class,
            _ => return None,
        };
        FocusIdentity::new(category, &event.focus)
    }
    pub fn propose(
        &mut self,
        scope: Scope,
        event: &Event,
        focused_app: Option<&str>,
        now: Instant,
    ) -> std::io::Result<Decision> {
        self.transactions
            .propose(&mut self.matcher, scope, event, focused_app, now)
    }
    pub fn propose_with_source(
        &mut self,
        scope: Scope,
        event: &Event,
        focused_app: Option<&str>,
        now: Instant,
        admit_source: impl FnMut(&str) -> bool,
    ) -> std::io::Result<Decision> {
        self.transactions.propose_with_source(
            &mut self.matcher,
            scope,
            event,
            focused_app,
            now,
            admit_source,
        )
    }
    /// Settle acknowledged native repeat/release without rule dispatch.
    pub fn settle_device_event(
        &mut self,
        scope: Scope,
        event: &Event,
        report: &DeliveryReport,
    ) -> bool {
        self.transactions
            .settle_device_event(&mut self.matcher, scope, event, report)
    }
    pub fn on_delivery(&mut self, scope: Scope, report: &DeliveryReport) -> bool {
        self.transactions
            .on_delivery(&mut self.matcher, scope, report)
    }
    pub fn invalidate_speculation(&mut self) {
        self.transactions.invalidate_speculation(&mut self.matcher);
    }
    pub fn await_reconciliation(&mut self, nonce: u64) -> bool {
        self.transactions
            .await_reconciliation(&mut self.matcher, nonce)
    }
    pub fn reconciled(&mut self, scope: Scope, body: &super::wire::Body) -> bool {
        self.transactions.reconciled(&mut self.matcher, scope, body)
    }
    pub fn await_lifecycle_reconciliation(
        &mut self,
        scope: Scope,
        body: &super::wire::Body,
    ) -> bool {
        self.transactions
            .await_lifecycle_reconciliation(&mut self.matcher, scope, body)
    }
    /// Retire only a native-reconciled generation; unrelated holds survive.
    pub fn retire_device(&mut self, generation: u64) {
        self.transactions
            .retire_device(&mut self.matcher, generation);
    }
    pub fn disconnected(&mut self) {
        self.transactions.disconnected(&mut self.matcher);
    }
}
/// Caller-owned matcher transaction state. Keep this next to the SAME Matcher
/// under the atomic matching-state lock; invalidate before replacing that matcher
/// on reload. Tokens must never be reported to a different matcher instance.
#[derive(Default)]
pub struct MatcherTransactions {
    pending: Option<Pending>,
    replacement_disabled: bool,
    recovery_nonce: Option<u64>,
    previous_recovery_nonce: Option<u64>,
}
impl MatcherTransactions {
    /// Propose synchronously using only the caller's category-aware alias result.
    /// Unknown keys bypass. Unsupported source selection/replacement modifiers
    /// fall back to Pass; no side effect or asynchronous injection is dispatched.
    /// # Errors
    /// Rejects overlapping proposals and use after uncertain delivery.
    pub fn propose(
        &mut self,
        matcher: &mut Matcher,
        scope: Scope,
        event: &Event,
        focused_app: Option<&str>,
        now: Instant,
    ) -> std::io::Result<Decision> {
        self.propose_with_source(matcher, scope, event, focused_app, now, |_| false)
    }
    /// `admit_source` performs only bounded queue admission. True selects
    /// Swallow for a non-key side effect; it does not attest source completion.
    /// # Errors
    /// Rejects overlapping proposals and use after uncertain delivery.
    pub fn propose_with_source(
        &mut self,
        matcher: &mut Matcher,
        scope: Scope,
        event: &Event,
        focused_app: Option<&str>,
        now: Instant,
        mut admit_source: impl FnMut(&str) -> bool,
    ) -> std::io::Result<Decision> {
        if self.pending.is_some() || self.replacement_disabled {
            return Err(super::wire::invalid());
        }
        if event.state != 1 {
            return Ok(Decision::Pass);
        }
        let Some(code) = u16::try_from(event.key)
            .ok()
            .and_then(crate::keymap::from_evdev)
        else {
            return Ok(Decision::Pass);
        };
        let normalized = KeyEvent {
            code,
            kind: if event.state == 0 {
                EventKind::KeyUp
            } else {
                EventKind::KeyDown
            },
            mods: physical_modifiers(event.modifiers),
        };
        let (verdict, token) = matcher.dispatch_device_with_delivery(
            DeviceId(scope.generation),
            normalized,
            focused_app,
            now,
        );
        let decision = match verdict {
            Verdict::Pass => Decision::Pass,
            Verdict::Swallow => Decision::Swallow,
            Verdict::Replace(Action::SendKey { key, mods }) => {
                match (crate::keymap::to_evdev(key), requested_modifiers(mods)) {
                    (Some(key), Some(modifiers)) => Decision::Replace {
                        key: u32::from(key),
                        modifiers,
                    },
                    _ => Decision::Pass,
                }
            }
            Verdict::Replace(Action::SetInputSource { id }) => {
                if admit_source(&id) {
                    Decision::Swallow
                } else {
                    Decision::Pass
                }
            }
        };
        if let Some(token) = token {
            self.pending = Some(Pending {
                scope,
                token,
                decision,
                key: event.key,
                state: event.state,
                focus_epoch: event.focus_epoch,
            });
        }
        Ok(decision)
    }
    /// Consume a token only for the exact proposal scope. Original admission is
    /// route acceptance, never application receipt. Unknown ACK stays uncertain.
    pub fn on_delivery(
        &mut self,
        matcher: &mut Matcher,
        scope: Scope,
        report: &DeliveryReport,
    ) -> bool {
        let Some(pending) = self.pending else {
            return false;
        };
        if pending.scope != scope {
            return false;
        }
        let Pending {
            token,
            decision,
            key,
            state,
            focus_epoch,
            ..
        } = pending;
        self.pending = None;
        if matches!(report, DeliveryReport::Acknowledged(outcome)
            if outcome.key != key || outcome.state != state || outcome.focus_epoch != focus_epoch
                || (outcome.result == ResultKind::Replaced && !matches!(decision, Decision::Replace { .. }))
                || (outcome.result == ResultKind::SwallowCommitted && decision != Decision::Swallow))
        {
            self.replacement_disabled = true;
            matcher.report_delivery(token, ReplacementDelivery::Uncertain);
            return false;
        }
        let result = if decision == Decision::Pass
            && matches!(report, DeliveryReport::Acknowledged(o) if o.result == ResultKind::OriginalAdmitted)
        {
            ReplacementDelivery::RejectedOriginalForwarded
        } else if decision == Decision::Swallow
            && matches!(report, DeliveryReport::Acknowledged(o) if o.result == ResultKind::SwallowCommitted)
        {
            // Token settlement retains consumed ownership only. This is NOT a
            // key replacement delivery or a source-selection completion report.
            ReplacementDelivery::Delivered
        } else {
            report.replacement_delivery()
        };
        if result == ReplacementDelivery::Uncertain {
            self.replacement_disabled = true;
        }
        matcher.report_delivery(token, result)
    }
    /// Settle only an acknowledged native lifecycle event. Repeat is inert;
    /// Up clears the exact press without evaluating modifier taps or actions.
    /// Caller must supply the Bridge-correlated scope/report. Available while
    /// disabled; settlement never re-enables replacement after uncertainty.
    pub fn settle_device_event(
        &mut self,
        matcher: &mut Matcher,
        scope: Scope,
        event: &Event,
        report: &DeliveryReport,
    ) -> bool {
        if event.state == 1 || event.state > 2 || scope.generation == 0 {
            return false;
        }
        let DeliveryReport::Acknowledged(outcome) = report else {
            self.replacement_disabled = true;
            return false;
        };
        if outcome.key != event.key
            || outcome.state != event.state
            || outcome.focus_epoch != event.focus_epoch
            || matches!(
                outcome.result,
                ResultKind::PartialUncertainFatal | ResultKind::Replaced
            )
        {
            self.replacement_disabled = true;
            return false;
        }
        let Some(code) = u16::try_from(event.key)
            .ok()
            .and_then(crate::keymap::from_evdev)
        else {
            return false;
        };
        if event.state == 0
            && self
                .pending
                .is_some_and(|p| p.scope.generation == scope.generation && p.key == event.key)
        {
            self.pending = None;
        }
        matcher.settle_device_event(
            DeviceId(scope.generation),
            KeyEvent {
                code,
                kind: if event.state == 0 {
                    EventKind::KeyUp
                } else {
                    EventKind::KeyDown
                },
                mods: physical_modifiers(event.modifiers),
            },
        );
        true
    }
    /// Bind fresh authenticated Bridge nonce; does not clear committed holds.
    /// Matching remains disabled until verified global Reset(reason8).
    pub fn await_reconciliation(&mut self, matcher: &mut Matcher, nonce: u64) -> bool {
        if nonce == 0
            || self.previous_recovery_nonce == Some(nonce)
            || self.recovery_nonce == Some(nonce)
        {
            return false;
        }
        self.invalidate_speculation(matcher);
        self.replacement_disabled = true;
        self.recovery_nonce = Some(nonce);
        true
    }
    /// Arm same-nonce lifecycle recovery only for a healthy reconciled session.
    /// Uncertainty still requires fresh authenticated reconnect; holds stay live
    /// until a subsequent Bridge-verified global reason8.
    pub fn await_lifecycle_reconciliation(
        &mut self,
        matcher: &mut Matcher,
        scope: Scope,
        body: &super::wire::Body,
    ) -> bool {
        if !matches!(body, super::wire::Body::Reset(2 | 3))
            || scope.generation != 0
            || self.replacement_disabled
            || self.pending.is_some()
            || self.previous_recovery_nonce != Some(scope.nonce)
        {
            return false;
        }
        self.invalidate_speculation(matcher);
        self.replacement_disabled = true;
        self.recovery_nonce = Some(scope.nonce);
        true
    }
    /// Accept only Bridge-verified global reason8 for the bound fresh nonce.
    /// Native attests quiescent physical/output holds and no synthetic uncertainty.
    /// This is the ONLY recovery operation that clears holds/re-enables matching.
    pub fn reconciled(
        &mut self,
        matcher: &mut Matcher,
        scope: Scope,
        body: &super::wire::Body,
    ) -> bool {
        if !matches!(body, super::wire::Body::Reset(8))
            || scope.generation != 0
            || self.recovery_nonce != Some(scope.nonce)
        {
            return false;
        }
        matcher.reset();
        self.pending = None;
        self.replacement_disabled = false;
        self.previous_recovery_nonce = self.recovery_nonce.take();
        true
    }
    /// Native retirement is reported only after native output reconciliation.
    /// Clear this generation's matcher ownership, never unrelated devices.
    /// A pending proposal for that generation stays uncertain/disabled.
    pub fn retire_device(&mut self, matcher: &mut Matcher, generation: u64) {
        if generation == 0 {
            return;
        }
        if self
            .pending
            .is_some_and(|pending| pending.scope.generation == generation)
        {
            self.pending = None;
            self.replacement_disabled = true;
        }
        matcher.reset_device(DeviceId(generation));
    }
    /// Validated already-unlocked epoch synchronization is speculation-only.
    /// Exact enabled nonce/no pending proposal preserves matching and holds.
    /// Uncertainty cannot be cured by an epoch synchronization packet.
    pub fn observe_unlocked_epoch(&mut self, matcher: &mut Matcher, nonce: u64) {
        if self.is_reconciled(nonce) && self.pending.is_none() {
            matcher.invalidate_speculation();
        } else {
            self.observe_lock_boundary(matcher, nonce);
        }
    }
    /// Validated lock/epoch controls may precede the first native reason8.
    /// Preserve an exact already-armed proof wait; never cure uncertainty or
    /// reset physical ownership. Enabled sessions require fresh admission.
    pub fn observe_lock_boundary(&mut self, matcher: &mut Matcher, nonce: u64) {
        if self.recovery_nonce == Some(nonce) && self.replacement_disabled && self.pending.is_none()
        {
            matcher.invalidate_speculation();
        } else {
            self.disconnected(matcher);
        }
    }
    /// Bounded diagnostic distinguishing proof wait from transport uncertainty.
    #[must_use]
    pub const fn recovery_state(&self) -> RecoveryState {
        if self.replacement_disabled {
            if self.recovery_nonce.is_some() {
                RecoveryState::AwaitingNativeProof
            } else {
                RecoveryState::FreshAdmissionRequired
            }
        } else if self.previous_recovery_nonce.is_some() {
            RecoveryState::Ready
        } else {
            RecoveryState::Unbound
        }
    }
    /// Only exact current-nonce native reconciliation enables matching.
    #[must_use]
    pub const fn is_reconciled(&self, nonce: u64) -> bool {
        nonce != 0
            && !self.replacement_disabled
            && self.recovery_nonce.is_none()
            && matches!(self.previous_recovery_nonce, Some(previous) if previous == nonce)
    }
    /// Uncertainty requires fresh admission, not an already armed reason8.
    /// This observation never clears matcher ownership.
    #[must_use]
    pub const fn needs_reconciliation(&self) -> bool {
        self.replacement_disabled && self.recovery_nonce.is_none()
    }
    /// Invalidate focus/reconnect/keymap speculation without clearing physical
    /// matcher holds. A pending unacknowledged decision is uncertainty and keeps
    /// replacement disabled until explicit native reconciliation. An ordinary
    /// reset with no pending decision does not invent uncertain output.
    pub fn invalidate_speculation(&mut self, matcher: &mut Matcher) {
        if self.pending.take().is_some() {
            self.replacement_disabled = true;
        }
        matcher.invalidate_speculation();
    }
    /// Transport loss is uncertain even if no token is pending. No automatic
    /// reenable on reconnect, focus change, or a later acknowledgment.
    pub fn disconnected(&mut self, matcher: &mut Matcher) {
        self.invalidate_speculation(matcher);
        self.recovery_nonce = None;
        self.replacement_disabled = true;
    }
}
#[cfg(test)]
#[path = "matcher_tests.rs"]
mod tests;

fn physical_modifiers(bits: u32) -> Modifiers {
    let mut mods = Modifiers::empty();
    for (index, key) in [42, 54, 29, 97, 56, 100, 125, 126].into_iter().enumerate() {
        if bits & (1 << index) != 0 {
            mods |= crate::keymap::modifier(key);
        }
    }
    mods
}
fn requested_modifiers(mods: Modifiers) -> Option<u32> {
    if mods.contains(Modifiers::FN) {
        return None;
    }
    let mut bits = 0;
    for (left, right, generic, offset) in [
        (
            Modifiers::LEFT_SHIFT,
            Modifiers::RIGHT_SHIFT,
            Modifiers::SHIFT,
            0,
        ),
        (
            Modifiers::LEFT_CTRL,
            Modifiers::RIGHT_CTRL,
            Modifiers::CTRL,
            2,
        ),
        (Modifiers::LEFT_ALT, Modifiers::RIGHT_ALT, Modifiers::ALT, 4),
        (Modifiers::LEFT_CMD, Modifiers::RIGHT_CMD, Modifiers::CMD, 6),
    ] {
        if mods.contains(left) || (mods.contains(generic) && !mods.contains(right)) {
            bits |= 1 << offset;
        }
        if mods.contains(right) {
            bits |= 1 << (offset + 1);
        }
    }
    Some(bits)
}
