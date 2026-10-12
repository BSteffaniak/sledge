//! Compositor delivery reconciliation using the real matcher, without devices/services.
use std::time::Instant;

use sledge_core::{
    Action, DeviceId, EventKind, HotkeyTrigger, KeyCode, KeyEvent, Matcher, Modifiers,
    ReplacementDelivery, ReplacementToken, Rule, RuleSet, TapTrigger, Trigger, Verdict,
};

fn event(key: KeyCode, kind: EventKind) -> KeyEvent {
    KeyEvent {
        code: key,
        kind,
        mods: Modifiers::empty(),
    }
}

fn rules() -> RuleSet {
    let hotkey = |key| Rule {
        trigger: Trigger::Hotkey(HotkeyTrigger {
            key,
            mods: Modifiers::empty(),
        }),
        action: Action::SendKey {
            key: KeyCode::KeyB,
            mods: Modifiers::empty(),
        },
        when_app_in: Some(vec!["original".into()]),
    };
    RuleSet::new(vec![
        hotkey(KeyCode::KeyA),
        hotkey(KeyCode::KeyK),
        Rule {
            trigger: Trigger::Tap(TapTrigger {
                tap: KeyCode::Fn,
                count: 2,
                within_ms: 600,
            }),
            action: Action::SendKey {
                key: KeyCode::Return,
                mods: Modifiers::empty(),
            },
            when_app_in: None,
        },
    ])
}

fn dispatch(m: &mut Matcher, device: u64, key: KeyCode, kind: EventKind) -> Verdict {
    m.dispatch_device(
        DeviceId(device),
        event(key, kind),
        Some("original"),
        Instant::now(),
    )
}

fn replacement(m: &mut Matcher, device: u64, key: KeyCode) -> ReplacementToken {
    let (verdict, token) = m.dispatch_device_with_delivery(
        DeviceId(device),
        event(key, EventKind::KeyDown),
        Some("original"),
        Instant::now(),
    );
    assert!(matches!(verdict, Verdict::Replace(_)));
    token.expect("replacement token")
}

#[test]
fn speculative_reset_preserves_consumed_and_forwarded_ownership() {
    let mut matcher = Matcher::new(rules());
    let pending = replacement(&mut matcher, 1, KeyCode::KeyA);
    let forwarded = replacement(&mut matcher, 2, KeyCode::KeyA);
    assert!(matcher.report_delivery(forwarded, ReplacementDelivery::RejectedOriginalForwarded));
    matcher.invalidate_speculation();
    assert!(!matcher.report_delivery(pending, ReplacementDelivery::RejectedOriginalForwarded));
    for kind in [EventKind::KeyDown, EventKind::KeyUp] {
        assert!(matches!(
            dispatch(&mut matcher, 1, KeyCode::KeyA, kind),
            Verdict::Swallow
        ));
        assert!(matches!(
            dispatch(&mut matcher, 2, KeyCode::KeyA, kind),
            Verdict::Pass
        ));
    }
    assert!(matches!(
        dispatch(&mut matcher, 1, KeyCode::KeyA, EventKind::KeyDown),
        Verdict::Replace(_)
    ));
}

#[test]
fn mixed_dispatch_retires_pending_tokens_and_keeps_other_forwarded_presses() {
    let mut m = Matcher::new(rules());
    // Legacy-only matching, before any transactional state exists.
    assert!(matches!(
        dispatch(&mut m, 1, KeyCode::KeyK, EventKind::KeyDown),
        Verdict::Replace(_)
    ));
    assert!(matches!(
        dispatch(&mut m, 1, KeyCode::KeyK, EventKind::KeyUp),
        Verdict::Swallow
    ));
    let pending = replacement(&mut m, 1, KeyCode::KeyA);
    let forwarded = replacement(&mut m, 2, KeyCode::KeyA);
    assert!(m.report_delivery(forwarded, ReplacementDelivery::RejectedOriginalForwarded));
    // Legacy release must retire the pending token without clearing the other
    // keyboard's fallback state.
    assert!(matches!(
        dispatch(&mut m, 1, KeyCode::KeyA, EventKind::KeyUp),
        Verdict::Swallow
    ));
    assert!(matches!(
        dispatch(&mut m, 1, KeyCode::KeyA, EventKind::KeyDown),
        Verdict::Replace(_)
    ));
    assert!(!m.report_delivery(pending, ReplacementDelivery::RejectedOriginalForwarded));
    assert!(matches!(
        dispatch(&mut m, 1, KeyCode::KeyA, EventKind::KeyUp),
        Verdict::Swallow
    ));
    for kind in [EventKind::KeyDown, EventKind::KeyUp] {
        assert!(matches!(
            dispatch(&mut m, 2, KeyCode::KeyA, kind),
            Verdict::Pass
        ));
    }
    // All transactional state has drained: legacy matching resumes normally,
    // and a new transaction must re-enable acknowledgement processing.
    assert!(matches!(
        dispatch(&mut m, 2, KeyCode::KeyA, EventKind::KeyDown),
        Verdict::Replace(_)
    ));
    assert!(matches!(
        dispatch(&mut m, 2, KeyCode::KeyA, EventKind::KeyUp),
        Verdict::Swallow
    ));
    let next = replacement(&mut m, 1, KeyCode::KeyA);
    assert!(m.report_delivery(next, ReplacementDelivery::RejectedOriginalForwarded));
    assert!(matches!(
        dispatch(&mut m, 1, KeyCode::KeyA, EventKind::KeyUp),
        Verdict::Pass
    ));
}

#[test]
fn rejection_preserves_forwarded_repeat_up_across_focus_reload_and_new_press() {
    let mut m = Matcher::new(rules());
    let failed = replacement(&mut m, 1, KeyCode::KeyA);
    let other_keyboard = replacement(&mut m, 2, KeyCode::KeyA);
    let unrelated = replacement(&mut m, 1, KeyCode::KeyK);
    assert!(m.report_delivery(failed, ReplacementDelivery::RejectedOriginalForwarded));
    assert!(m.report_delivery(other_keyboard, ReplacementDelivery::Delivered));
    assert!(m.report_delivery(unrelated, ReplacementDelivery::Delivered));
    m.swap_rules(rules());
    for kind in [EventKind::KeyDown, EventKind::KeyDown, EventKind::KeyUp] {
        assert!(matches!(
            m.dispatch_device(
                DeviceId(1),
                event(KeyCode::KeyA, kind),
                Some("changed"),
                Instant::now()
            ),
            Verdict::Pass
        ));
    }
    for (device, key) in [(2, KeyCode::KeyA), (1, KeyCode::KeyK)] {
        assert!(matches!(
            dispatch(&mut m, device, key, EventKind::KeyDown),
            Verdict::Swallow
        ));
        assert!(matches!(
            dispatch(&mut m, device, key, EventKind::KeyUp),
            Verdict::Swallow
        ));
    }
    let newer = replacement(&mut m, 1, KeyCode::KeyA);
    assert!(!m.report_delivery(failed, ReplacementDelivery::RejectedOriginalForwarded));
    assert!(m.report_delivery(newer, ReplacementDelivery::Delivered));
    assert!(matches!(
        dispatch(&mut m, 1, KeyCode::KeyA, EventKind::KeyUp),
        Verdict::Swallow
    ));
}

#[test]
fn committed_and_uncertain_output_never_replay_original() {
    for outcome in [
        ReplacementDelivery::Delivered,
        ReplacementDelivery::Uncertain,
    ] {
        let mut m = Matcher::new(rules());
        let token = replacement(&mut m, 1, KeyCode::KeyA);
        assert!(m.report_delivery(token, outcome));
        assert!(!m.report_delivery(token, ReplacementDelivery::RejectedOriginalForwarded));
        m.swap_rules(RuleSet::new(vec![]));
        for kind in [EventKind::KeyDown, EventKind::KeyUp] {
            assert!(matches!(
                dispatch(&mut m, 1, KeyCode::KeyA, kind),
                Verdict::Swallow
            ));
        }
        assert!(matches!(
            dispatch(&mut m, 1, KeyCode::KeyA, EventKind::KeyDown),
            Verdict::Pass
        ));
    }
}

#[test]
fn released_pending_ack_cannot_change_newer_press_even_without_prior_ack() {
    let mut m = Matcher::new(rules());
    let old = replacement(&mut m, 1, KeyCode::KeyA);
    // Pending decisions conservatively own repeats/release until settled.
    assert!(matches!(
        dispatch(&mut m, 1, KeyCode::KeyA, EventKind::KeyDown),
        Verdict::Swallow
    ));
    assert!(matches!(
        dispatch(&mut m, 1, KeyCode::KeyA, EventKind::KeyUp),
        Verdict::Swallow
    ));
    let new = replacement(&mut m, 1, KeyCode::KeyA);
    assert_ne!(old, new);
    assert!(!m.report_delivery(old, ReplacementDelivery::RejectedOriginalForwarded));
    assert!(matches!(
        dispatch(&mut m, 1, KeyCode::KeyA, EventKind::KeyDown),
        Verdict::Swallow
    ));
    assert!(m.report_delivery(new, ReplacementDelivery::RejectedOriginalForwarded));
    assert!(matches!(
        dispatch(&mut m, 1, KeyCode::KeyA, EventKind::KeyUp),
        Verdict::Pass
    ));
}

#[test]
fn reset_invalidates_tokens_without_reusing_generations_or_resetting_other_device() {
    let mut m = Matcher::new(rules());
    let old = replacement(&mut m, 1, KeyCode::KeyA);
    let other = replacement(&mut m, 2, KeyCode::KeyA);
    m.reset_device(DeviceId(1));
    let new = replacement(&mut m, 1, KeyCode::KeyA);
    assert_ne!(old, new);
    assert!(!m.report_delivery(old, ReplacementDelivery::RejectedOriginalForwarded));
    assert!(m.report_delivery(other, ReplacementDelivery::Delivered));
    m.reset();
    let newest = replacement(&mut m, 1, KeyCode::KeyA);
    assert_ne!(new, newest);
    assert!(!m.report_delivery(new, ReplacementDelivery::RejectedOriginalForwarded));
    assert!(m.report_delivery(newest, ReplacementDelivery::Delivered));
}

#[test]
fn fallback_does_not_reset_unrelated_modifier_tap_sequence() {
    let mut m = Matcher::new(rules());
    let failed = replacement(&mut m, 1, KeyCode::KeyA);
    let consumed = replacement(&mut m, 1, KeyCode::KeyK);
    // A second keyboard's modifier tap sequence must survive reporting failure.
    for kind in [EventKind::KeyDown, EventKind::KeyUp] {
        assert!(matches!(
            dispatch(&mut m, 2, KeyCode::Fn, kind),
            Verdict::Pass
        ));
    }
    assert!(matches!(
        dispatch(&mut m, 2, KeyCode::Fn, EventKind::KeyDown),
        Verdict::Pass
    ));
    assert!(m.report_delivery(failed, ReplacementDelivery::RejectedOriginalForwarded));
    assert!(m.report_delivery(consumed, ReplacementDelivery::Delivered));
    let (verdict, tap_token) = m.dispatch_device_with_delivery(
        DeviceId(2),
        event(KeyCode::Fn, EventKind::KeyUp),
        None,
        Instant::now(),
    );
    assert!(matches!(verdict, Verdict::Replace(_)));
    let tap_token = tap_token.unwrap();
    assert!(m.report_delivery(tap_token, ReplacementDelivery::RejectedOriginalForwarded));
    // Tap replacements occur at release: there is no consumed held press to
    // convert, and the next physical down still goes through the tap FSM.
    assert!(matches!(
        dispatch(&mut m, 2, KeyCode::Fn, EventKind::KeyDown),
        Verdict::Pass
    ));
    assert!(matches!(
        dispatch(&mut m, 2, KeyCode::Fn, EventKind::KeyUp),
        Verdict::Pass
    ));
    assert!(matches!(
        dispatch(&mut m, 1, KeyCode::KeyK, EventKind::KeyUp),
        Verdict::Swallow
    ));
}

#[test]
fn rejection_preserves_independent_consumed_tap_and_release_token_cannot_touch_new_press() {
    let mut configured: Vec<_> = rules().iter().cloned().collect();
    let mut third_tap = configured.last().unwrap().clone();
    third_tap.trigger = Trigger::Tap(TapTrigger {
        tap: KeyCode::Fn,
        count: 3,
        within_ms: 600,
    });
    configured.push(third_tap);
    let mut m = Matcher::new(RuleSet::new(configured));
    let failed = replacement(&mut m, 1, KeyCode::KeyA);
    for kind in [EventKind::KeyDown, EventKind::KeyUp, EventKind::KeyDown] {
        assert!(matches!(
            dispatch(&mut m, 2, KeyCode::Fn, kind),
            Verdict::Pass
        ));
    }
    let (verdict, consumed_tap) = m.dispatch_device_with_delivery(
        DeviceId(2),
        event(KeyCode::Fn, EventKind::KeyUp),
        None,
        Instant::now(),
    );
    assert!(matches!(verdict, Verdict::Replace(_)));
    let consumed_tap = consumed_tap.unwrap();
    assert!(m.report_delivery(failed, ReplacementDelivery::RejectedOriginalForwarded));
    assert!(m.report_delivery(consumed_tap, ReplacementDelivery::Delivered));
    // A delivered tap action does not reset its physical sequence on rejection
    // of another keyboard's press: the third tap must still fire.
    assert!(matches!(
        dispatch(&mut m, 2, KeyCode::Fn, EventKind::KeyDown),
        Verdict::Pass
    ));
    let (verdict, third) = m.dispatch_device_with_delivery(
        DeviceId(2),
        event(KeyCode::Fn, EventKind::KeyUp),
        None,
        Instant::now(),
    );
    assert!(matches!(verdict, Verdict::Replace(_)));
    let third = third.unwrap();
    // An outstanding release token is retired by the next down, even though
    // that modifier down does not itself issue a replacement token.
    assert!(matches!(
        dispatch(&mut m, 2, KeyCode::Fn, EventKind::KeyDown),
        Verdict::Pass
    ));
    assert!(!m.report_delivery(third, ReplacementDelivery::RejectedOriginalForwarded));
    let current = replacement(&mut m, 2, KeyCode::KeyK);
    assert!(!m.report_delivery(third, ReplacementDelivery::RejectedOriginalForwarded));
    assert!(m.report_delivery(current, ReplacementDelivery::Delivered));
    assert!(matches!(
        dispatch(&mut m, 2, KeyCode::KeyK, EventKind::KeyUp),
        Verdict::Swallow
    ));
    assert!(matches!(
        dispatch(&mut m, 2, KeyCode::Fn, EventKind::KeyUp),
        Verdict::Pass
    ));
}

#[test]
fn pass_swallow_do_not_issue_tokens_and_reload_does_not_invalidate_pending_delivery() {
    let mut m = Matcher::new(rules());
    let token = replacement(&mut m, 1, KeyCode::KeyA);
    let (verdict, repeated) = m.dispatch_device_with_delivery(
        DeviceId(1),
        event(KeyCode::KeyA, EventKind::KeyDown),
        None,
        Instant::now(),
    );
    assert!(matches!(verdict, Verdict::Swallow));
    assert!(repeated.is_none());
    m.swap_rules(RuleSet::new(vec![]));
    assert!(m.report_delivery(token, ReplacementDelivery::RejectedOriginalForwarded));
    let (verdict, passed) = m.dispatch_device_with_delivery(
        DeviceId(1),
        event(KeyCode::KeyA, EventKind::KeyUp),
        None,
        Instant::now(),
    );
    assert!(matches!(verdict, Verdict::Pass));
    assert!(passed.is_none());
}

#[test]
fn settlement_retires_only_released_press_without_rules_or_replay() {
    let mut matcher = Matcher::new(rules());
    let token = replacement(&mut matcher, 1, KeyCode::KeyA);
    assert!(matcher.report_delivery(token, ReplacementDelivery::Delivered));
    let other = replacement(&mut matcher, 2, KeyCode::KeyA);
    assert!(matcher.report_delivery(other, ReplacementDelivery::Delivered));
    matcher.settle_device_event(DeviceId(1), event(KeyCode::KeyA, EventKind::KeyDown));
    assert!(matches!(
        dispatch(&mut matcher, 1, KeyCode::KeyA, EventKind::KeyDown),
        Verdict::Swallow
    ));
    matcher.settle_device_event(DeviceId(1), event(KeyCode::KeyA, EventKind::KeyUp));
    assert!(matches!(
        dispatch(&mut matcher, 2, KeyCode::KeyA, EventKind::KeyDown),
        Verdict::Swallow
    ));
    let next = replacement(&mut matcher, 1, KeyCode::KeyA);
    assert!(!matcher.report_delivery(token, ReplacementDelivery::Delivered));
    assert!(matcher.report_delivery(next, ReplacementDelivery::Delivered));
}

#[test]
fn settlement_does_not_complete_modifier_tap_action() {
    let mut matcher = Matcher::new(rules());
    for _ in 0..2 {
        assert!(matches!(
            dispatch(&mut matcher, 1, KeyCode::Fn, EventKind::KeyDown),
            Verdict::Pass
        ));
        matcher.settle_device_event(DeviceId(1), event(KeyCode::Fn, EventKind::KeyUp));
    }
    assert!(matches!(
        dispatch(&mut matcher, 1, KeyCode::Fn, EventKind::KeyUp),
        Verdict::Pass
    ));
}
