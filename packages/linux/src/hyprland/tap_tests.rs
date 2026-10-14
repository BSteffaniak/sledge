use super::*;
use sledge_core::{KeyCode, Rule, RuleSet, TapTrigger, Trigger};
use std::time::Duration;

fn fixture() -> (Matcher, MatcherTransactions, Instant) {
    let matcher = Matcher::new(RuleSet::new(vec![Rule {
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
    }]));
    let mut transactions = MatcherTransactions::default();
    transactions.set_modifier_tap_release(true);
    (matcher, transactions, Instant::now())
}
fn scope(device: u64) -> Scope {
    Scope {
        generation: device,
        lock_epoch: 1,
        sequence: 1,
        deadline_ns: 100,
        nonce: 7,
    }
}
fn event(key: u32, state: u32) -> Event {
    Event {
        key,
        state,
        time_ms: 0,
        modifiers: if key == 100 && state == 1 { 32 } else { 0 },
        focus_epoch: 1,
        focus_kind: 1,
        focus: "app".into(),
    }
}
fn feed(
    m: &mut Matcher,
    t: &mut MatcherTransactions,
    base: Instant,
    key: u32,
    state: u32,
    device: u64,
    ms: u64,
) -> Decision {
    t.propose(
        m,
        scope(device),
        &event(key, state),
        None,
        base + Duration::from_millis(ms),
    )
    .unwrap()
}
fn taps(m: &mut Matcher, t: &mut MatcherTransactions, base: Instant, times: &[u64]) -> Decision {
    let mut result = Decision::Pass;
    for ms in times {
        assert_eq!(feed(m, t, base, 100, 1, 1, ms - 1), Decision::Pass);
        result = feed(m, t, base, 100, 0, 1, *ms);
    }
    result
}
#[test]
fn triple_release_is_transactional_and_duplicate_edges_are_inert() {
    let (mut m, mut t, base) = fixture();
    assert_eq!(feed(&mut m, &mut t, base, 100, 1, 1, 0), Decision::Pass);
    assert_eq!(feed(&mut m, &mut t, base, 100, 1, 1, 1), Decision::Pass);
    assert_eq!(feed(&mut m, &mut t, base, 100, 0, 1, 2), Decision::Pass);
    assert_eq!(feed(&mut m, &mut t, base, 100, 0, 1, 3), Decision::Pass);
    assert_eq!(
        taps(&mut m, &mut t, base, &[200, 400]),
        Decision::Replace {
            key: 28,
            modifiers: 4
        }
    );
    let report = DeliveryReport::Acknowledged(super::super::wire::Outcome {
        result: ResultKind::Replaced,
        reason: 0,
        key: 100,
        state: 0,
        focus_epoch: 1,
    });
    assert!(t.on_delivery(&mut m, scope(1), &report));
    assert!(!t.on_delivery(&mut m, scope(1), &report));
}
#[test]
fn long_standalone_hold_counts_only_at_release_and_window_is_between_releases() {
    let (mut m, mut t, base) = fixture();
    assert_eq!(
        taps(&mut m, &mut t, base, &[10, 610, 1210]),
        Decision::Replace {
            key: 28,
            modifiers: 4
        }
    );
    let (mut m, mut t, base) = fixture();
    assert_eq!(feed(&mut m, &mut t, base, 100, 1, 1, 0), Decision::Pass);
    assert_eq!(feed(&mut m, &mut t, base, 100, 2, 1, 900), Decision::Pass);
    assert_eq!(feed(&mut m, &mut t, base, 100, 0, 1, 1000), Decision::Pass);
    assert!(matches!(
        taps(&mut m, &mut t, base, &[1100, 1200]),
        Decision::Replace { .. }
    ));
    let (mut m, mut t, base) = fixture();
    assert_eq!(taps(&mut m, &mut t, base, &[10, 611, 700]), Decision::Pass);
}
#[test]
fn chords_unknown_input_and_other_devices_disqualify() {
    for key in [30, 42, 9999] {
        let (mut m, mut t, base) = fixture();
        assert_eq!(taps(&mut m, &mut t, base, &[10, 20]), Decision::Pass);
        feed(&mut m, &mut t, base, 100, 1, 1, 30);
        feed(&mut m, &mut t, base, key, 1, 1, 31);
        feed(&mut m, &mut t, base, key, 0, 1, 32);
        assert_eq!(feed(&mut m, &mut t, base, 100, 0, 1, 40), Decision::Pass);
    }
    let (mut m, mut t, base) = fixture();
    taps(&mut m, &mut t, base, &[10, 20]);
    feed(&mut m, &mut t, base, 100, 1, 2, 30);
    assert_eq!(feed(&mut m, &mut t, base, 100, 0, 2, 40), Decision::Pass);
    assert_eq!(taps(&mut m, &mut t, base, &[50]), Decision::Pass);
    let (mut m, mut t, base) = fixture();
    feed(&mut m, &mut t, base, 30, 1, 1, 0);
    assert_eq!(taps(&mut m, &mut t, base, &[10, 20, 30]), Decision::Pass);
}
#[test]
fn lifecycle_and_old_capability_never_complete_a_sequence() {
    for reset in 0..5 {
        let (mut m, mut t, base) = fixture();
        taps(&mut m, &mut t, base, &[10, 20]);
        match reset {
            0 => t.invalidate_speculation(&mut m),
            1 => t.retire_device(&mut m, 1),
            2 => m.swap_rules(m.rules().clone()),
            3 => {
                let mut e = event(100, 1);
                e.focus_epoch = 2;
                t.propose(&mut m, scope(1), &e, None, base + Duration::from_millis(30))
                    .unwrap();
            }
            _ => {
                t.set_modifier_tap_release(false);
            }
        }
        assert_eq!(taps(&mut m, &mut t, base, &[40]), Decision::Pass);
    }
    let (mut m, mut t, base) = fixture();
    taps(&mut m, &mut t, base, &[10, 20]);
    t.disconnected(&mut m);
    assert!(
        t.propose(&mut m, scope(1), &event(100, 0), None, base)
            .is_err()
    );
}
