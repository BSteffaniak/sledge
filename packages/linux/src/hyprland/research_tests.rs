use super::matcher::MatcherAdapter;
use super::*;
use sledge_core::trigger::{HotkeyTrigger, Trigger};
use sledge_core::{Action, KeyCode, Matcher, Modifiers, Rule, RuleSet};
use std::time::Instant;
struct MatchingHandler {
    adapter: MatcherAdapter,
    reports: Vec<DeliveryReport>,
    disconnected: bool,
    fault: ResearchFault,
}
impl MatchingHandler {
    fn new() -> Self {
        Self {
            adapter: MatcherAdapter::new(Matcher::new(RuleSet::new(vec![Rule {
                trigger: Trigger::Hotkey(HotkeyTrigger {
                    key: KeyCode::KeyA,
                    mods: Modifiers::empty(),
                }),
                action: Action::SendKey {
                    key: KeyCode::KeyB,
                    mods: Modifiers::empty(),
                },
                when_app_in: None,
            }]))),
            reports: vec![],
            disconnected: false,
            fault: ResearchFault::None,
        }
    }
}
impl EventHandler for MatchingHandler {
    fn research_fault(&mut self, _: Scope, _: &Event) -> ResearchFault {
        self.fault
    }
    fn propose(&mut self, scope: Scope, event: &Event) -> Decision {
        self.adapter
            .propose(scope, event, None, Instant::now())
            .unwrap_or(Decision::Pass)
    }
    fn delivery(&mut self, scope: Scope, event: &Event, _: Decision, report: DeliveryReport) {
        if event.state != 1 {
            self.adapter.settle_device_event(scope, event, &report);
        } else {
            self.adapter.on_delivery(scope, &report);
        }
        self.reports.push(report);
    }
    fn reset(&mut self, _: Scope, _: &Body) {
        self.adapter.invalidate_speculation();
    }
    fn disconnected(&mut self) {
        self.disconnected = true;
        self.adapter.disconnected();
    }
}
#[test]
fn research_real_matcher_commit_reset_and_repeat_keep_consumed_hold() {
    let (transport, peer) = super::tests::pair();
    let mut bridge = super::tests::bridge(transport);
    let mut handler = MatchingHandler::new();
    let mut packet = super::tests::packet();
    let outcome = Outcome {
        result: ResultKind::Replaced,
        reason: 0,
        key: 30,
        state: 1,
        focus_epoch: 9,
    };
    super::tests::raw(&peer, &packet.encode().unwrap());
    super::tests::raw(
        &peer,
        &Packet {
            scope: packet.scope,
            body: Body::Outcome(outcome),
        }
        .encode()
        .unwrap(),
    );
    bridge
        .research_step(&mut handler, packet.scope.deadline_ns)
        .unwrap();
    let peer = Transport::from_fd(peer);
    assert_eq!(
        peer.receive(packet.scope.deadline_ns).unwrap().body,
        Body::Decision(Decision::Replace {
            key: 48,
            modifiers: 0
        })
    );
    packet.scope.sequence += 1;
    packet.body = Body::Reset(2);
    peer.send(&packet, packet.scope.deadline_ns).unwrap();
    bridge
        .research_step(&mut handler, packet.scope.deadline_ns)
        .unwrap();
    packet.scope.sequence += 1;
    let mut event = super::tests::event();
    event.state = 2;
    packet.body = Body::Event(event);
    peer.send(&packet, packet.scope.deadline_ns).unwrap();
    peer.send(
        &Packet {
            scope: packet.scope,
            body: Body::Outcome(Outcome {
                result: ResultKind::SwallowCommitted,
                reason: 0,
                key: 30,
                state: 2,
                focus_epoch: 9,
            }),
        },
        packet.scope.deadline_ns,
    )
    .unwrap();
    bridge
        .research_step(&mut handler, packet.scope.deadline_ns)
        .unwrap();
    assert_eq!(
        peer.receive(packet.scope.deadline_ns).unwrap().body,
        Body::Decision(Decision::Pass)
    );
}
#[test]
fn research_timeout_partial_and_stale_outcome_never_replay_or_reenable() {
    for mode in 0..3 {
        let (transport, peer) = super::tests::pair();
        let mut bridge = super::tests::bridge(transport);
        let mut handler = MatchingHandler::new();
        let packet = super::tests::packet();
        super::tests::raw(&peer, &packet.encode().unwrap());
        if mode != 0 {
            let mut scope = packet.scope;
            if mode == 2 {
                scope.lock_epoch += 1;
            }
            super::tests::raw(
                &peer,
                &Packet {
                    scope,
                    body: Body::Outcome(Outcome {
                        result: ResultKind::PartialUncertainFatal,
                        reason: 6,
                        key: 30,
                        state: 1,
                        focus_epoch: 9,
                    }),
                }
                .encode()
                .unwrap(),
            );
        }
        assert!(
            bridge
                .research_step(&mut handler, packet.scope.deadline_ns)
                .is_err()
        );
        assert!(handler.disconnected);
        assert_eq!(handler.reports.len(), 1);
        handler.adapter.invalidate_speculation();
        assert!(
            handler
                .adapter
                .propose(packet.scope, &super::tests::event(), None, Instant::now())
                .is_err()
        );
        assert!(bridge.transport.is_none());
    }
}

#[test]
fn research_idle_expiry_preserves_connection_and_consumed_hold() {
    let (transport, peer) = super::tests::pair();
    let mut bridge = super::tests::bridge(transport);
    let mut handler = MatchingHandler::new();
    let packet = super::tests::packet();
    super::tests::raw(&peer, &packet.encode().unwrap());
    super::tests::raw(
        &peer,
        &Packet {
            scope: packet.scope,
            body: Body::Outcome(Outcome {
                result: ResultKind::Replaced,
                reason: 0,
                key: 30,
                state: 1,
                focus_epoch: 9,
            }),
        }
        .encode()
        .unwrap(),
    );
    assert!(
        bridge
            .research_step_if_ready(&mut handler, packet.scope.deadline_ns)
            .unwrap()
    );
    let peer = Transport::from_fd(peer);
    peer.receive(packet.scope.deadline_ns).unwrap();
    let sequence = bridge.sequence;
    let epoch = bridge.epoch;
    assert!(
        !bridge
            .research_step_if_ready(&mut handler, monotonic_ns().unwrap() + 1_000_000)
            .unwrap()
    );
    assert_eq!(bridge.sequence, sequence);
    assert_eq!(bridge.epoch, epoch);
    assert!(bridge.transport.is_some());
    assert!(!handler.disconnected);
    assert_eq!(handler.reports.len(), 1);
    let mut scope = packet.scope;
    scope.sequence += 1;
    scope.deadline_ns = monotonic_ns().unwrap() + MAX_WAIT_NS;
    let mut event = super::tests::event();
    event.state = 2;
    peer.send(
        &Packet {
            scope,
            body: Body::Event(event),
        },
        scope.deadline_ns,
    )
    .unwrap();
    peer.send(
        &Packet {
            scope,
            body: Body::Outcome(Outcome {
                result: ResultKind::SwallowCommitted,
                reason: 0,
                key: 30,
                state: 2,
                focus_epoch: 9,
            }),
        },
        scope.deadline_ns,
    )
    .unwrap();
    assert!(
        bridge
            .research_step_if_ready(&mut handler, scope.deadline_ns)
            .unwrap()
    );
    assert_eq!(
        peer.receive(scope.deadline_ns).unwrap().body,
        Body::Decision(Decision::Pass)
    );
    assert!(!handler.disconnected);
}
#[test]
fn research_idle_api_actual_transaction_timeout_disconnects_uncertain() {
    let (transport, peer) = super::tests::pair();
    let mut bridge = super::tests::bridge(transport);
    let mut handler = MatchingHandler::new();
    let packet = super::tests::packet();
    super::tests::raw(&peer, &packet.encode().unwrap());
    assert!(
        bridge
            .research_step_if_ready(&mut handler, monotonic_ns().unwrap() + 100_000_000)
            .is_err()
    );
    assert!(handler.disconnected);
    assert_eq!(handler.reports, vec![DeliveryReport::Uncertain]);
    assert!(bridge.transport.is_none());
}

#[test]
fn lifecycle_release_allows_two_fresh_committed_replacements() {
    let (transport, peer) = super::tests::pair();
    let mut bridge = super::tests::bridge(transport);
    let peer = Transport::from_fd(peer);
    let mut handler = MatchingHandler::new();
    let mut scope = super::tests::scope();
    for state in [1, 2, 0, 1, 0] {
        scope.deadline_ns = monotonic_ns().unwrap() + MAX_WAIT_NS;
        let mut event = super::tests::event();
        event.state = state;
        peer.send(
            &Packet {
                scope,
                body: Body::Event(event),
            },
            scope.deadline_ns,
        )
        .unwrap();
        peer.send(
            &Packet {
                scope,
                body: Body::Outcome(Outcome {
                    result: if state == 1 {
                        ResultKind::Replaced
                    } else {
                        ResultKind::SwallowCommitted
                    },
                    reason: 0,
                    key: 30,
                    state,
                    focus_epoch: 9,
                }),
            },
            scope.deadline_ns,
        )
        .unwrap();
        bridge
            .research_step(&mut handler, scope.deadline_ns)
            .unwrap();
        assert_eq!(
            peer.receive(scope.deadline_ns).unwrap().body,
            Body::Decision(if state == 1 {
                Decision::Replace {
                    key: 48,
                    modifiers: 0,
                }
            } else {
                Decision::Pass
            })
        );
        scope.sequence += 1;
    }
    assert!(!handler.disconnected);
}

#[test]
fn selected_peer_faults_report_uncertain_and_never_replay() {
    for fault in [
        ResearchFault::DelayDecision(std::time::Duration::from_millis(20)),
        ResearchFault::DropDecision,
        ResearchFault::DisconnectBeforeDecision,
        ResearchFault::DisconnectAfterDecision,
    ] {
        let (transport, peer) = super::tests::pair();
        let mut bridge = super::tests::bridge(transport);
        let mut handler = MatchingHandler::new();
        handler.fault = fault;
        let packet = super::tests::packet();
        super::tests::raw(&peer, &packet.encode().unwrap());
        assert!(
            bridge
                .research_step(&mut handler, packet.scope.deadline_ns)
                .is_err()
        );
        assert_eq!(handler.reports, vec![DeliveryReport::Uncertain]);
        assert!(handler.disconnected);
        assert!(
            handler
                .adapter
                .propose(packet.scope, &super::tests::event(), None, Instant::now())
                .is_err()
        );
    }
}
#[test]
fn explicit_reconnect_preserves_tombstones_and_uncertain_matcher_until_native_reconciliation() {
    let (transport, peer) = super::tests::pair();
    let mut bridge = super::tests::bridge(transport);
    let mut handler = MatchingHandler::new();
    bridge.retired.insert(42);
    drop(peer);
    assert!(
        bridge
            .research_step(&mut handler, monotonic_ns().unwrap() + MAX_WAIT_NS)
            .is_err()
    );
    let identity = Identity {
        capabilities: 0,
        api_hash: "api".into(),
        artifact: "artifact".into(),
    };
    for nonce in [7, 8, 7] {
        let (transport, peer) = super::tests::pair();
        let scope = Scope {
            generation: 0,
            lock_epoch: 0,
            sequence: 0,
            deadline_ns: 0,
            nonce,
        };
        super::tests::raw(
            &peer,
            &Packet {
                scope,
                body: Body::Hello(identity.clone()),
            }
            .encode()
            .unwrap(),
        );
        let result = bridge.reconnect(transport, &identity, monotonic_ns().unwrap() + MAX_WAIT_NS);
        if nonce == 8 {
            result.unwrap();
            assert!(bridge.retired.contains(&42));
            assert!(
                handler
                    .adapter
                    .propose(scope, &super::tests::event(), None, Instant::now())
                    .is_err()
            );
            bridge.transport.take();
        } else {
            assert!(result.is_err());
        }
    }
}
