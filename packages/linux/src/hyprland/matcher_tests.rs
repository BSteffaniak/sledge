use super::*;
use sledge_core::trigger::{HotkeyTrigger, Trigger};
use sledge_core::{KeyCode, Rule, RuleSet};

fn adapter(action: Action) -> MatcherAdapter {
    MatcherAdapter::new(Matcher::new(RuleSet::new(vec![Rule {
        trigger: Trigger::Hotkey(HotkeyTrigger {
            key: KeyCode::KeyA,
            mods: Modifiers::empty(),
        }),
        action,
        when_app_in: Some(vec!["app".into()]),
    }])))
}
fn scope() -> Scope {
    Scope {
        generation: 1,
        lock_epoch: 1,
        sequence: 1,
        deadline_ns: 100,
        nonce: 7,
    }
}
fn event(state: u32) -> Event {
    Event {
        key: 30,
        state,
        time_ms: 0,
        modifiers: 0,
        focus_epoch: 9,
        focus_kind: 1,
        focus: "app".into(),
    }
}
fn report(result: ResultKind) -> DeliveryReport {
    DeliveryReport::Acknowledged(super::super::wire::Outcome {
        result,
        reason: 0,
        key: 30,
        state: 1,
        focus_epoch: 9,
    })
}
#[test]
fn focus_boundary_preserves_hold_and_waits_exact_nonce_proof() {
    let mut a = adapter(Action::SendKey {
        key: KeyCode::KeyB,
        mods: Modifiers::empty(),
    });
    let mut global = scope();
    global.generation = 0;
    assert!(a.await_reconciliation(global.nonce));
    assert!(a.reconciled(global, &super::super::wire::Body::Reset(8)));
    a.propose(scope(), &event(1), Some("app"), Instant::now())
        .unwrap();
    assert!(a.on_delivery(scope(), &report(ResultKind::Replaced)));
    let mut stale = global;
    stale.nonce += 1;
    assert!(!a.await_lifecycle_reconciliation(stale, &super::super::wire::Body::Reset(2)));
    assert!(a.await_lifecycle_reconciliation(global, &super::super::wire::Body::Reset(2)));
    assert!(!a.transactions.is_reconciled(global.nonce));
    assert!(!a.transactions.needs_reconciliation());
    assert!(matches!(
        a.matcher.dispatch_device(
            DeviceId(1),
            KeyEvent {
                code: KeyCode::KeyA,
                kind: EventKind::KeyDown,
                mods: Modifiers::empty()
            },
            None,
            Instant::now()
        ),
        Verdict::Swallow
    ));
    assert!(!a.reconciled(stale, &super::super::wire::Body::Reset(8)));
    assert!(a.reconciled(global, &super::super::wire::Body::Reset(8)));
    a.propose(scope(), &event(1), Some("app"), Instant::now())
        .unwrap();
    assert!(!a.await_lifecycle_reconciliation(global, &super::super::wire::Body::Reset(2)));
    a.invalidate_speculation();
    assert!(a.transactions.needs_reconciliation());
    assert!(!a.reconciled(global, &super::super::wire::Body::Reset(8)));
}
#[test]
fn unlocked_epoch_keeps_ready_holds_but_never_cures_pending_uncertainty() {
    let mut a = adapter(Action::SendKey {
        key: KeyCode::KeyB,
        mods: Modifiers::empty(),
    });
    let mut global = scope();
    global.generation = 0;
    assert!(a.await_reconciliation(global.nonce));
    assert!(a.reconciled(global, &super::super::wire::Body::Reset(8)));
    a.transactions
        .observe_unlocked_epoch(&mut a.matcher, global.nonce);
    assert!(a.transactions.is_reconciled(global.nonce));
    let down = scope();
    a.propose(down, &event(1), Some("app"), Instant::now())
        .unwrap();
    assert!(a.on_delivery(down, &report(ResultKind::Replaced)));
    a.transactions
        .observe_unlocked_epoch(&mut a.matcher, global.nonce);
    assert!(a.transactions.is_reconciled(global.nonce));
    assert_eq!(
        a.propose(down, &event(1), Some("app"), Instant::now())
            .unwrap(),
        Decision::Swallow
    );
    a.transactions
        .observe_lock_boundary(&mut a.matcher, global.nonce);
    a.transactions
        .observe_unlocked_epoch(&mut a.matcher, global.nonce);
    assert!(a.transactions.needs_reconciliation());
    assert!(!a.transactions.is_reconciled(global.nonce));
    assert!(a.await_reconciliation(8));
    global.nonce = 8;
    assert!(a.reconciled(global, &super::super::wire::Body::Reset(8)));
    let mut fresh = down;
    fresh.nonce = 8;
    a.propose(fresh, &event(1), Some("app"), Instant::now())
        .unwrap();
    a.transactions.observe_unlocked_epoch(&mut a.matcher, 8);
    assert!(a.transactions.needs_reconciliation());
    assert!(!a.on_delivery(fresh, &report(ResultKind::Replaced)));
}
#[test]
fn initial_epoch_lock_preserves_armed_proof_but_not_uncertainty() {
    let mut a = adapter(Action::SendKey {
        key: KeyCode::KeyB,
        mods: Modifiers::empty(),
    });
    assert!(a.await_reconciliation(7));
    a.transactions.observe_lock_boundary(&mut a.matcher, 7);
    assert_eq!(
        a.transactions.recovery_state(),
        RecoveryState::AwaitingNativeProof
    );
    assert!(!a.transactions.needs_reconciliation());
    for reason in [0, 2, 3, 5] {
        let mut global = scope();
        global.generation = 0;
        a.await_lifecycle_reconciliation(global, &super::super::wire::Body::Reset(reason));
        a.invalidate_speculation();
        assert!(!a.transactions.needs_reconciliation());
        assert!(!a.transactions.is_reconciled(7));
    }
    let mut global = scope();
    global.generation = 0;
    assert!(a.reconciled(global, &super::super::wire::Body::Reset(8)));
    assert!(a.transactions.is_reconciled(7));
    a.transactions.observe_lock_boundary(&mut a.matcher, 7);
    assert!(a.transactions.needs_reconciliation());
    assert!(!a.transactions.is_reconciled(7));
    assert!(!a.reconciled(global, &super::super::wire::Body::Reset(8)));
    assert!(a.await_reconciliation(8));
    a.transactions.observe_lock_boundary(&mut a.matcher, 7); // stale nonce cannot preserve proof
    assert!(a.transactions.needs_reconciliation());
}
#[test]
fn reload_late_ack_requests_fresh_reconciliation_without_clearing_hold() {
    let mut a = adapter(Action::SendKey {
        key: KeyCode::KeyB,
        mods: Modifiers::empty(),
    });
    let old = scope();
    assert!(!a.transactions.needs_reconciliation());
    a.propose(old, &event(1), Some("app"), Instant::now())
        .unwrap();
    a.invalidate_speculation();
    assert!(!a.on_delivery(old, &report(ResultKind::Replaced)));
    assert!(a.transactions.needs_reconciliation());
    // Lost settlement remains conservatively consumed until physical proof.
    assert!(matches!(
        a.matcher.dispatch_device(
            DeviceId(old.generation),
            KeyEvent {
                code: KeyCode::KeyA,
                kind: EventKind::KeyDown,
                mods: Modifiers::empty()
            },
            Some("app"),
            Instant::now()
        ),
        Verdict::Swallow
    ));
    a.disconnected();
    assert!(a.transactions.needs_reconciliation());
    assert!(a.await_reconciliation(8));
    assert!(!a.transactions.needs_reconciliation());
    let mut fresh = old;
    fresh.nonce = 8;
    fresh.generation = 0;
    assert!(a.reconciled(fresh, &super::super::wire::Body::Reset(8)));
    fresh.generation = 1;
    assert_eq!(
        a.propose(fresh, &event(1), Some("app"), Instant::now())
            .unwrap(),
        Decision::Replace {
            key: 48,
            modifiers: 0
        }
    );
    assert!(a.on_delivery(fresh, &report(ResultKind::Replaced)));
    assert!(!a.transactions.needs_reconciliation());
    // Normal reload without an in-flight token needs no reconnect.
    a.invalidate_speculation();
    assert!(!a.transactions.needs_reconciliation());
    assert!(!a.on_delivery(fresh, &report(ResultKind::OriginalAdmitted)));
    assert!(!a.transactions.needs_reconciliation());
}

#[test]
fn actual_rejection_restores_repeat_original_and_token_is_one_shot() {
    let mut a = adapter(Action::SendKey {
        key: KeyCode::KeyB,
        mods: Modifiers::SHIFT,
    });
    let s = scope();
    assert_eq!(
        a.propose(s, &event(1), Some("app"), Instant::now())
            .unwrap(),
        Decision::Replace {
            key: 48,
            modifiers: 1
        }
    );
    let mut wrong = s;
    wrong.nonce += 1;
    assert!(!a.on_delivery(wrong, &report(ResultKind::Replaced)));
    assert!(a.on_delivery(s, &report(ResultKind::RejectedOriginalAdmitted)));
    assert!(!a.on_delivery(s, &report(ResultKind::Replaced)));
    let mut repeat = s;
    repeat.sequence += 1;
    assert_eq!(
        a.propose(repeat, &event(2), Some("app"), Instant::now())
            .unwrap(),
        Decision::Pass
    );
}
#[test]
fn source_admission_is_not_key_replacement_completion() {
    for admitted in [false, true] {
        let mut a = adapter(Action::SetInputSource { id: "de".into() });
        let s = scope();
        let mut calls = 0;
        let decision = a
            .propose_with_source(s, &event(1), Some("app"), Instant::now(), |id| {
                assert_eq!(id, "de");
                calls += 1;
                admitted
            })
            .unwrap();
        assert_eq!(calls, 1);
        assert_eq!(
            decision,
            if admitted {
                Decision::Swallow
            } else {
                Decision::Pass
            }
        );
        assert!(a.on_delivery(
            s,
            &report(if admitted {
                ResultKind::SwallowCommitted
            } else {
                ResultKind::OriginalAdmitted
            })
        ));
        let mut next = s;
        next.sequence += 1;
        assert_eq!(
            a.propose(next, &event(2), Some("app"), Instant::now())
                .unwrap(),
            Decision::Pass
        );
    }
}
#[test]
fn uncertain_disables_proposals_without_replay() {
    let mut a = adapter(Action::SendKey {
        key: KeyCode::KeyB,
        mods: Modifiers::empty(),
    });
    let s = scope();
    a.propose(s, &event(1), Some("app"), Instant::now())
        .unwrap();
    assert!(a.on_delivery(s, &DeliveryReport::Uncertain));
    assert!(
        a.propose(s, &event(2), Some("app"), Instant::now())
            .is_err()
    );
}
#[test]
fn authoritative_focus_is_typed_and_unknown_is_not_polled_fallback() {
    let mut e = event(1);
    assert_eq!(
        MatcherAdapter::focus(&e).unwrap().category,
        FocusCategory::Wayland
    );
    e.focus_kind = 2;
    assert_eq!(
        MatcherAdapter::focus(&e).unwrap().category,
        FocusCategory::X11Class
    );
    e.focus_kind = 0;
    e.focus.clear();
    assert!(MatcherAdapter::focus(&e).is_none());
    let mut a = adapter(Action::SendKey {
        key: KeyCode::KeyB,
        mods: Modifiers::empty(),
    });
    assert_eq!(
        a.propose(scope(), &e, None, Instant::now()).unwrap(),
        Decision::Pass
    );
}
#[test]
fn side_modifiers_never_use_xkb_indices() {
    assert_eq!(
        physical_modifiers(1 | 8),
        Modifiers::SHIFT | Modifiers::LEFT_SHIFT | Modifiers::CTRL | Modifiers::RIGHT_CTRL
    );
    assert_eq!(
        requested_modifiers(Modifiers::CTRL | Modifiers::RIGHT_CTRL),
        Some(8)
    );
    assert_eq!(requested_modifiers(Modifiers::FN), None);
}

#[test]
fn speculative_reset_preserves_committed_consumed_and_forwarded_holds() {
    for (outcome, _expected) in [
        (ResultKind::Replaced, Decision::Swallow),
        (ResultKind::RejectedOriginalAdmitted, Decision::Pass),
    ] {
        let mut adapter = adapter(Action::SendKey {
            key: KeyCode::KeyB,
            mods: Modifiers::empty(),
        });
        let mut scope = scope();
        adapter
            .propose(scope, &event(1), Some("app"), Instant::now())
            .unwrap();
        assert!(adapter.on_delivery(scope, &report(outcome)));
        adapter.invalidate_speculation();
        scope.sequence += 1;
        assert_eq!(
            adapter
                .propose(scope, &event(2), None, Instant::now())
                .unwrap(),
            Decision::Pass
        );
        scope.sequence += 1;
        assert_eq!(
            adapter
                .propose(scope, &event(0), None, Instant::now())
                .unwrap(),
            Decision::Pass
        );
    }
}
#[test]
fn pending_reset_invalidates_ack_and_never_reenables_uncertainty() {
    let mut adapter = adapter(Action::SendKey {
        key: KeyCode::KeyB,
        mods: Modifiers::empty(),
    });
    let scope = scope();
    adapter
        .propose(scope, &event(1), Some("app"), Instant::now())
        .unwrap();
    adapter.invalidate_speculation();
    assert!(!adapter.on_delivery(scope, &report(ResultKind::RejectedOriginalAdmitted)));
    adapter.invalidate_speculation();
    assert!(
        adapter
            .propose(scope, &event(2), None, Instant::now())
            .is_err()
    );
}

#[test]
fn malformed_actual_outcome_never_commits_or_replays_matcher_proposal() {
    for change in 0..3 {
        let mut adapter = adapter(Action::SendKey {
            key: KeyCode::KeyB,
            mods: Modifiers::empty(),
        });
        let scope = scope();
        adapter
            .propose(scope, &event(1), Some("app"), Instant::now())
            .unwrap();
        let mut outcome = match report(ResultKind::Replaced) {
            DeliveryReport::Acknowledged(o) => o,
            DeliveryReport::Uncertain => unreachable!(),
        };
        match change {
            0 => outcome.key += 1,
            1 => outcome.state = 0,
            _ => outcome.focus_epoch += 1,
        }
        assert!(!adapter.on_delivery(scope, &DeliveryReport::Acknowledged(outcome)));
        assert!(!adapter.on_delivery(scope, &report(ResultKind::RejectedOriginalAdmitted)));
        adapter.invalidate_speculation();
        assert!(
            adapter
                .propose(scope, &event(2), None, Instant::now())
                .is_err()
        );
    }
}

#[test]
fn borrowed_matcher_transactions_share_reload_and_preserve_committed_holds() {
    let mut matcher = Matcher::new(RuleSet::new(vec![Rule {
        trigger: Trigger::Hotkey(HotkeyTrigger {
            key: KeyCode::KeyA,
            mods: Modifiers::empty(),
        }),
        action: Action::SendKey {
            key: KeyCode::KeyB,
            mods: Modifiers::empty(),
        },
        when_app_in: None,
    }]));
    let mut transactions = super::MatcherTransactions::default();
    let mut scope = scope();
    assert_eq!(
        transactions
            .propose(&mut matcher, scope, &event(1), None, Instant::now())
            .unwrap(),
        Decision::Replace {
            key: 48,
            modifiers: 0
        }
    );
    assert!(transactions.on_delivery(&mut matcher, scope, &report(ResultKind::Replaced)));
    transactions.invalidate_speculation(&mut matcher);
    scope.sequence += 1;
    assert_eq!(
        transactions
            .propose(&mut matcher, scope, &event(2), None, Instant::now())
            .unwrap(),
        Decision::Pass
    );
    // Coordinator reload replaces matcher only after transaction invalidation.
    transactions.invalidate_speculation(&mut matcher);
    matcher = Matcher::new(RuleSet::new(vec![]));
    scope.sequence += 1;
    assert_eq!(
        transactions
            .propose(&mut matcher, scope, &event(1), None, Instant::now())
            .unwrap(),
        Decision::Pass
    );
}
#[test]
fn disconnected_borrowed_state_cannot_be_reenabled_by_new_nonce_or_reset() {
    let mut matcher = Matcher::new(RuleSet::new(vec![]));
    let mut transactions = super::MatcherTransactions::default();
    transactions.disconnected(&mut matcher);
    transactions.invalidate_speculation(&mut matcher);
    let mut scope = scope();
    scope.nonce += 1;
    assert!(
        transactions
            .propose(&mut matcher, scope, &event(1), None, Instant::now())
            .is_err()
    );
}

#[test]
fn reconnect_uncertainty_preserves_matcher_press_ownership_without_reenabling() {
    for outcome in [ResultKind::Replaced, ResultKind::RejectedOriginalAdmitted] {
        let mut matcher = Matcher::new(RuleSet::new(vec![Rule {
            trigger: Trigger::Hotkey(HotkeyTrigger {
                key: KeyCode::KeyA,
                mods: Modifiers::empty(),
            }),
            action: Action::SendKey {
                key: KeyCode::KeyB,
                mods: Modifiers::empty(),
            },
            when_app_in: None,
        }]));
        let mut transactions = super::MatcherTransactions::default();
        let scope = scope();
        transactions
            .propose(&mut matcher, scope, &event(1), None, Instant::now())
            .unwrap();
        assert!(transactions.on_delivery(&mut matcher, scope, &report(outcome)));
        transactions.disconnected(&mut matcher);
        transactions.invalidate_speculation(&mut matcher);
        let mut fresh = scope;
        fresh.nonce += 1;
        assert!(
            transactions
                .propose(&mut matcher, fresh, &event(2), None, Instant::now())
                .is_err()
        );
        // Inspect underlying ownership without re-admitting research proposals.
        for kind in [
            sledge_core::EventKind::KeyDown,
            sledge_core::EventKind::KeyUp,
        ] {
            let verdict = matcher.dispatch_device(
                sledge_core::DeviceId(scope.generation),
                sledge_core::KeyEvent {
                    code: KeyCode::KeyA,
                    kind,
                    mods: Modifiers::empty(),
                },
                None,
                Instant::now(),
            );
            assert!(if outcome == ResultKind::Replaced {
                matches!(verdict, sledge_core::Verdict::Swallow)
            } else {
                matches!(verdict, sledge_core::Verdict::Pass)
            });
        }
    }
}

#[test]
fn borrowed_retirement_only_clears_named_device_ownership() {
    let mut matcher = Matcher::new(RuleSet::new(vec![Rule {
        trigger: Trigger::Hotkey(HotkeyTrigger {
            key: KeyCode::KeyA,
            mods: Modifiers::empty(),
        }),
        action: Action::SendKey {
            key: KeyCode::KeyB,
            mods: Modifiers::empty(),
        },
        when_app_in: None,
    }]));
    let mut transactions = super::MatcherTransactions::default();
    let mut scope = scope();
    for generation in [1, 2] {
        scope.generation = generation;
        transactions
            .propose(&mut matcher, scope, &event(1), None, Instant::now())
            .unwrap();
        assert!(transactions.on_delivery(&mut matcher, scope, &report(ResultKind::Replaced)));
    }
    transactions.retire_device(&mut matcher, 1);
    scope.generation = 2;
    assert_eq!(
        transactions
            .propose(&mut matcher, scope, &event(2), None, Instant::now())
            .unwrap(),
        Decision::Pass
    );
    scope.generation = 1;
    assert_eq!(
        transactions
            .propose(&mut matcher, scope, &event(0), None, Instant::now())
            .unwrap(),
        Decision::Pass
    );
}

#[test]
fn lifecycle_settlement_clears_modifier_tap_without_dispatch_even_when_disabled() {
    use sledge_core::trigger::TapTrigger;
    let mut matcher = Matcher::new(RuleSet::new(vec![Rule {
        trigger: Trigger::Tap(TapTrigger {
            tap: KeyCode::LeftShift,
            count: 2,
            within_ms: 500,
        }),
        action: Action::SendKey {
            key: KeyCode::KeyB,
            mods: Modifiers::empty(),
        },
        when_app_in: None,
    }]));
    let mut transactions = super::MatcherTransactions::default();
    let scope = scope();
    let mut modifier = event(1);
    modifier.key = 42;
    modifier.modifiers = 1;
    assert_eq!(
        transactions
            .propose(&mut matcher, scope, &modifier, None, Instant::now())
            .unwrap(),
        Decision::Pass
    );
    transactions.disconnected(&mut matcher);
    modifier.state = 0;
    modifier.modifiers = 0;
    let ack = DeliveryReport::Acknowledged(super::super::wire::Outcome {
        result: ResultKind::OriginalAdmitted,
        reason: 0,
        key: 42,
        state: 0,
        focus_epoch: 9,
    });
    assert!(transactions.settle_device_event(&mut matcher, scope, &modifier, &ack));
    assert!(!transactions.on_delivery(&mut matcher, scope, &ack));
    assert!(
        transactions
            .propose(&mut matcher, scope, &event(1), None, Instant::now())
            .is_err()
    );
}
#[test]
fn wrong_release_outcome_preserves_consumed_hold_and_disables_future_proposals() {
    let mut matcher = Matcher::new(RuleSet::new(vec![Rule {
        trigger: Trigger::Hotkey(HotkeyTrigger {
            key: KeyCode::KeyA,
            mods: Modifiers::empty(),
        }),
        action: Action::SendKey {
            key: KeyCode::KeyB,
            mods: Modifiers::empty(),
        },
        when_app_in: None,
    }]));
    let mut transactions = super::MatcherTransactions::default();
    let scope = scope();
    transactions
        .propose(&mut matcher, scope, &event(1), None, Instant::now())
        .unwrap();
    assert!(transactions.on_delivery(&mut matcher, scope, &report(ResultKind::Replaced)));
    assert!(!transactions.settle_device_event(
        &mut matcher,
        scope,
        &event(0),
        &report(ResultKind::SwallowCommitted)
    ));
    assert!(
        transactions
            .propose(&mut matcher, scope, &event(1), None, Instant::now())
            .is_err()
    );
    assert!(matches!(
        matcher.dispatch_device(
            sledge_core::DeviceId(scope.generation),
            sledge_core::KeyEvent {
                code: KeyCode::KeyA,
                kind: EventKind::KeyDown,
                mods: Modifiers::empty()
            },
            None,
            Instant::now()
        ),
        Verdict::Swallow
    ));
}

#[test]
fn only_fresh_nonce_global_reconciliation_reenables_and_clears_old_holds() {
    let mut a = adapter(Action::SendKey {
        key: KeyCode::KeyB,
        mods: Modifiers::empty(),
    });
    let old = scope();
    a.propose(old, &event(1), Some("app"), Instant::now())
        .unwrap();
    a.on_delivery(old, &report(ResultKind::Replaced));
    a.disconnected();
    assert!(a.await_reconciliation(8));
    let mut fresh = old;
    fresh.nonce = 8;
    fresh.generation = 0;
    assert!(!a.reconciled(fresh, &super::super::wire::Body::Reset(0)));
    assert!(
        a.propose(old, &event(1), Some("app"), Instant::now())
            .is_err()
    );
    let mut stale = fresh;
    stale.nonce = 7;
    assert!(!a.reconciled(stale, &super::super::wire::Body::Reset(8)));
    assert!(a.reconciled(fresh, &super::super::wire::Body::Reset(8)));
    assert!(!a.reconciled(fresh, &super::super::wire::Body::Reset(8)));
    fresh.generation = 1;
    assert_eq!(
        a.propose(fresh, &event(1), Some("app"), Instant::now())
            .unwrap(),
        Decision::Replace {
            key: 48,
            modifiers: 0
        }
    );
    assert!(!a.await_reconciliation(8));
}

#[test]
fn same_connection_lifecycle_boundary_preserves_hold_until_proof_and_cannot_cure_uncertainty() {
    let mut a = adapter(Action::SendKey {
        key: KeyCode::KeyB,
        mods: Modifiers::empty(),
    });
    let mut scope = scope();
    scope.generation = 0;
    assert!(a.await_reconciliation(scope.nonce));
    assert!(a.reconciled(scope, &super::super::wire::Body::Reset(8)));
    scope.generation = 1;
    a.propose(scope, &event(1), Some("app"), Instant::now())
        .unwrap();
    assert!(a.on_delivery(scope, &report(ResultKind::Replaced)));
    scope.generation = 0;
    assert!(!a.await_lifecycle_reconciliation(scope, &super::super::wire::Body::Reset(0)));
    assert!(a.await_lifecycle_reconciliation(scope, &super::super::wire::Body::Reset(3)));
    scope.generation = 1;
    assert!(
        a.propose(scope, &event(1), Some("app"), Instant::now())
            .is_err()
    );
    scope.generation = 0;
    assert!(a.reconciled(scope, &super::super::wire::Body::Reset(8)));
    assert!(!a.reconciled(scope, &super::super::wire::Body::Reset(8)));
    scope.generation = 1;
    assert_eq!(
        a.propose(scope, &event(1), Some("app"), Instant::now())
            .unwrap(),
        Decision::Replace {
            key: 48,
            modifiers: 0
        }
    );
    assert!(a.on_delivery(scope, &DeliveryReport::Uncertain));
    scope.generation = 0;
    assert!(!a.await_lifecycle_reconciliation(scope, &super::super::wire::Body::Reset(3)));
    assert!(!a.reconciled(scope, &super::super::wire::Body::Reset(8)));
}
