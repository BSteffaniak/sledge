//! Stream semantics regressions; these are deterministic engine tests, not desktop acceptance.
use std::time::{Duration, Instant};

use sledge_core::{
    Action, DeviceId, EventKind, HotkeyTrigger, KeyCode, KeyEvent, Matcher, Modifiers, Rule,
    RuleSet, TapTrigger, Trigger, Verdict,
};

fn event(code: KeyCode, kind: EventKind) -> KeyEvent {
    KeyEvent {
        code,
        kind,
        mods: Modifiers::empty(),
    }
}

fn tap_rule(key: KeyCode, count: u32, within_ms: u32, output: KeyCode) -> Rule {
    Rule {
        trigger: Trigger::Tap(TapTrigger {
            tap: key,
            count,
            within_ms,
        }),
        action: Action::SendKey {
            key: output,
            mods: Modifiers::empty(),
        },
        when_app_in: None,
    }
}

fn tap(
    m: &mut Matcher,
    device: DeviceId,
    key: KeyCode,
    base: Instant,
    down: u64,
    up: u64,
) -> Verdict {
    assert!(matches!(
        m.dispatch_device(
            device,
            event(key, EventKind::KeyDown),
            None,
            base + Duration::from_millis(down)
        ),
        Verdict::Pass
    ));
    m.dispatch_device(
        device,
        event(key, EventKind::KeyUp),
        None,
        base + Duration::from_millis(up),
    )
}

fn hotkeys() -> RuleSet {
    RuleSet::new(vec![Rule {
        trigger: Trigger::Hotkey(HotkeyTrigger {
            key: KeyCode::KeyK,
            mods: Modifiers::empty(),
        }),
        action: Action::SendKey {
            key: KeyCode::KeyA,
            mods: Modifiers::empty(),
        },
        when_app_in: None,
    }])
}

#[test]
fn each_rule_uses_its_own_window_including_windows_over_800_ms() {
    let mut m = Matcher::new(RuleSet::new(vec![
        tap_rule(KeyCode::RightAlt, 2, 100, KeyCode::KeyA),
        tap_rule(KeyCode::RightAlt, 2, 1500, KeyCode::KeyB),
    ]));
    let base = Instant::now();
    assert!(matches!(
        tap(&mut m, DeviceId(1), KeyCode::RightAlt, base, 0, 10),
        Verdict::Pass
    ));
    assert!(matches!(
        tap(&mut m, DeviceId(1), KeyCode::RightAlt, base, 1000, 1010),
        Verdict::Replace(Action::SendKey {
            key: KeyCode::KeyB,
            ..
        })
    ));
}

#[test]
fn consecutive_release_intervals_allow_a_longer_total_sequence() {
    let base = Instant::now();
    let mut m = Matcher::new(RuleSet::new(vec![tap_rule(
        KeyCode::Fn,
        3,
        100,
        KeyCode::KeyA,
    )]));
    for (down, up) in [(0, 0), (60, 70)] {
        assert!(matches!(
            tap(&mut m, DeviceId(1), KeyCode::Fn, base, down, up),
            Verdict::Pass
        ));
    }
    assert!(matches!(
        tap(&mut m, DeviceId(1), KeyCode::Fn, base, 130, 140),
        Verdict::Replace(_)
    ));
}

#[test]
fn gap_over_window_resets_but_boundary_gap_matches() {
    let base = Instant::now();
    let mut m = Matcher::new(RuleSet::new(vec![tap_rule(
        KeyCode::Fn,
        2,
        100,
        KeyCode::KeyA,
    )]));
    assert!(matches!(
        tap(&mut m, DeviceId(1), KeyCode::Fn, base, 0, 0),
        Verdict::Pass
    ));
    assert!(matches!(
        tap(&mut m, DeviceId(1), KeyCode::Fn, base, 90, 101),
        Verdict::Pass
    ));
    assert!(matches!(
        tap(&mut m, DeviceId(1), KeyCode::Fn, base, 190, 201),
        Verdict::Replace(_)
    ));
}

#[test]
fn first_tap_has_no_unconfigured_maximum_hold() {
    let base = Instant::now();
    let mut m = Matcher::new(RuleSet::new(vec![tap_rule(
        KeyCode::Fn,
        2,
        100,
        KeyCode::KeyA,
    )]));
    assert!(matches!(
        tap(&mut m, DeviceId(1), KeyCode::Fn, base, 0, 5000),
        Verdict::Pass
    ));
    assert!(matches!(
        tap(&mut m, DeviceId(1), KeyCode::Fn, base, 5050, 5100),
        Verdict::Replace(_)
    ));
}

#[test]
fn fn_and_caps_accept_physical_down_up_without_modifier_bits() {
    let base = Instant::now();
    for key in [KeyCode::Fn, KeyCode::CapsLock] {
        let mut m = Matcher::new(RuleSet::new(vec![tap_rule(key, 2, 100, KeyCode::KeyA)]));
        assert!(matches!(
            tap(&mut m, DeviceId(1), key, base, 0, 10),
            Verdict::Pass
        ));
        assert!(matches!(
            tap(&mut m, DeviceId(1), key, base, 30, 100),
            Verdict::Replace(_)
        ));
    }
}

#[test]
fn caps_lock_flags_never_invent_physical_transitions() {
    let base = Instant::now();
    let mut m = Matcher::new(RuleSet::new(vec![tap_rule(
        KeyCode::CapsLock,
        2,
        100,
        KeyCode::KeyA,
    )]));
    for _ in 0..6 {
        assert!(matches!(
            m.dispatch(
                event(KeyCode::CapsLock, EventKind::ModifiersChanged),
                None,
                base
            ),
            Verdict::Pass
        ));
    }
}

#[test]
fn repeats_and_stray_releases_do_not_count_as_taps() {
    let now = Instant::now();
    let mut m = Matcher::new(RuleSet::new(vec![tap_rule(
        KeyCode::Fn,
        2,
        100,
        KeyCode::KeyA,
    )]));
    for kind in [
        EventKind::KeyUp,
        EventKind::KeyDown,
        EventKind::KeyDown,
        EventKind::KeyUp,
        EventKind::KeyUp,
    ] {
        assert!(matches!(
            m.dispatch(event(KeyCode::Fn, kind), None, now),
            Verdict::Pass
        ));
    }
    assert!(matches!(
        tap(&mut m, DeviceId(0), KeyCode::Fn, now, 10, 20),
        Verdict::Replace(_)
    ));
}

#[test]
fn explicit_modifier_overlap_taints_both_candidates() {
    let now = Instant::now();
    let mut m = Matcher::new(RuleSet::new(vec![tap_rule(
        KeyCode::CapsLock,
        2,
        100,
        KeyCode::KeyA,
    )]));
    for (key, kind) in [
        (KeyCode::CapsLock, EventKind::KeyDown),
        (KeyCode::Fn, EventKind::KeyDown),
        (KeyCode::Fn, EventKind::KeyUp),
        (KeyCode::CapsLock, EventKind::KeyUp),
    ] {
        assert!(matches!(
            m.dispatch(event(key, kind), None, now),
            Verdict::Pass
        ));
    }
    assert!(matches!(
        tap(&mut m, DeviceId(0), KeyCode::CapsLock, now, 10, 20),
        Verdict::Pass
    ));
}

#[test]
fn consumed_press_remains_owned_through_reload_focus_change_and_repeat() {
    let now = Instant::now();
    let mut m = Matcher::new(hotkeys());
    let down = event(KeyCode::KeyK, EventKind::KeyDown);
    assert!(matches!(m.dispatch(down, None, now), Verdict::Replace(_)));
    m.swap_rules(RuleSet::new(vec![]));
    assert!(matches!(
        m.dispatch(down, Some("other"), now),
        Verdict::Swallow
    ));
    assert!(matches!(
        m.dispatch(event(KeyCode::KeyK, EventKind::KeyUp), None, now),
        Verdict::Swallow
    ));
    assert!(matches!(m.dispatch(down, None, now), Verdict::Pass));
}

#[test]
fn device_identity_isolates_owned_presses_and_tap_sequences() {
    let now = Instant::now();
    let mut m = Matcher::new(hotkeys());
    assert!(matches!(
        m.dispatch_device(
            DeviceId(1),
            event(KeyCode::KeyK, EventKind::KeyDown),
            None,
            now
        ),
        Verdict::Replace(_)
    ));
    assert!(matches!(
        m.dispatch_device(
            DeviceId(2),
            event(KeyCode::KeyK, EventKind::KeyUp),
            None,
            now
        ),
        Verdict::Pass
    ));
    assert!(matches!(
        m.dispatch_device(
            DeviceId(1),
            event(KeyCode::KeyK, EventKind::KeyUp),
            None,
            now
        ),
        Verdict::Swallow
    ));
    m.swap_rules(RuleSet::new(vec![tap_rule(
        KeyCode::Fn,
        2,
        100,
        KeyCode::KeyA,
    )]));
    assert!(matches!(
        tap(&mut m, DeviceId(1), KeyCode::Fn, now, 0, 10),
        Verdict::Pass
    ));
    assert!(matches!(
        tap(&mut m, DeviceId(2), KeyCode::Fn, now, 20, 30),
        Verdict::Pass
    ));
    assert!(matches!(
        tap(&mut m, DeviceId(1), KeyCode::Fn, now, 40, 50),
        Verdict::Replace(_)
    ));
}

#[test]
fn reset_device_forgets_only_lost_stream_and_reset_forgets_all() {
    let now = Instant::now();
    let mut m = Matcher::new(hotkeys());
    for id in [DeviceId(1), DeviceId(2)] {
        assert!(matches!(
            m.dispatch_device(id, event(KeyCode::KeyK, EventKind::KeyDown), None, now),
            Verdict::Replace(_)
        ));
    }
    m.reset_device(DeviceId(1));
    assert!(matches!(
        m.dispatch_device(
            DeviceId(1),
            event(KeyCode::KeyK, EventKind::KeyUp),
            None,
            now
        ),
        Verdict::Pass
    ));
    assert!(matches!(
        m.dispatch_device(
            DeviceId(2),
            event(KeyCode::KeyK, EventKind::KeyUp),
            None,
            now
        ),
        Verdict::Swallow
    ));
    m.swap_rules(RuleSet::new(vec![tap_rule(
        KeyCode::Fn,
        2,
        100,
        KeyCode::KeyA,
    )]));
    assert!(matches!(
        tap(&mut m, DeviceId(1), KeyCode::Fn, now, 0, 10),
        Verdict::Pass
    ));
    m.reset_device(DeviceId(1));
    assert!(matches!(
        tap(&mut m, DeviceId(1), KeyCode::Fn, now, 20, 30),
        Verdict::Pass
    ));
    m.reset();
    assert!(matches!(
        tap(&mut m, DeviceId(1), KeyCode::Fn, now, 40, 50),
        Verdict::Pass
    ));
}
