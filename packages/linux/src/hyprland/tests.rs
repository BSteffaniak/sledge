use super::*;
use nix::sys::socket::{AddressFamily, MsgFlags, SockFlag, SockType, send, socketpair};
use std::os::fd::{AsRawFd, OwnedFd};

pub(super) fn pair() -> (Transport, OwnedFd) {
    let (a, b) = socketpair(
        AddressFamily::Unix,
        SockType::SeqPacket,
        None,
        SockFlag::SOCK_CLOEXEC | SockFlag::SOCK_NONBLOCK,
    )
    .unwrap();
    (Transport::from_fd(a), b)
}
pub(super) fn scope() -> Scope {
    Scope {
        generation: 1,
        lock_epoch: 1,
        sequence: 1,
        deadline_ns: monotonic_ns().unwrap() + MAX_WAIT_NS,
        nonce: 7,
    }
}
pub(super) fn event() -> Event {
    Event {
        key: 30,
        state: 1,
        time_ms: 5,
        modifiers: 0,
        focus_epoch: 9,
        focus_kind: 1,
        focus: "app".into(),
    }
}
pub(super) fn packet() -> Packet {
    Packet {
        scope: scope(),
        body: Body::Event(event()),
    }
}
pub(super) fn raw(fd: &OwnedFd, bytes: &[u8]) {
    assert_eq!(
        send(fd.as_raw_fd(), bytes, MsgFlags::MSG_NOSIGNAL).unwrap(),
        bytes.len()
    );
}
pub(super) fn bridge(transport: Transport) -> Bridge {
    Bridge {
        transport: Some(transport),
        nonce: 7,
        epoch: 1,
        sequence: 0,
        retired: HashSet::new(),
        previous_nonces: HashSet::new(),
        awaiting_reconciliation: true,
        locked: false,
    }
}
#[derive(Default)]
struct Handler {
    reports: Vec<DeliveryReport>,
    disconnected: bool,
    resets: usize,
}
impl EventHandler for Handler {
    fn propose(&mut self, _: Scope, _: &Event) -> Decision {
        panic!("unqualified probe must not dispatch matcher")
    }
    fn delivery(&mut self, _: Scope, _: &Event, _: Decision, r: DeliveryReport) {
        self.reports.push(r);
    }
    fn reset(&mut self, _: Scope, _: &Body) {
        self.resets += 1;
    }
    fn disconnected(&mut self) {
        self.disconnected = true;
    }
}
#[test]
fn strict_framing_and_padding() {
    let p = packet();
    let bytes = p.encode().unwrap();
    assert_eq!(Packet::decode(&bytes).unwrap(), p);
    for position in [0, 4, 12, 56, 64 + 28, 511] {
        let mut malformed = bytes;
        malformed[position] ^= 128;
        assert!(Packet::decode(&malformed).is_err(), "position {position}");
    }
    let mut b = bytes;
    b[64 + 24] = 3;
    assert!(Packet::decode(&b).is_err());
    b = bytes;
    b[64 + 32] = 255;
    assert!(Packet::decode(&b).is_err());
}
#[test]
fn seqpacket_short_oversized_and_malformed() {
    for bytes in [vec![0; 511], vec![0; 513], vec![0; 512]] {
        let (t, peer) = pair();
        raw(&peer, &bytes);
        assert_eq!(
            t.receive(monotonic_ns().unwrap() + MAX_WAIT_NS)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }
}
#[test]
fn peer_credentials_are_required() {
    let (t, _peer) = pair();
    let uid = nix::unistd::geteuid().as_raw();
    let pid = i32::try_from(std::process::id()).unwrap();
    t.authenticate(transport::Peer { uid, pid }).unwrap();
    assert!(
        t.authenticate(transport::Peer {
            uid: uid.wrapping_add(1),
            pid
        })
        .is_err()
    );
    assert!(
        t.authenticate(transport::Peer { uid, pid: pid + 1 })
            .is_err()
    );
}
#[test]
fn expired_deadline_and_disconnect() {
    let (t, peer) = pair();
    assert_eq!(
        t.receive(monotonic_ns().unwrap()).unwrap_err().kind(),
        io::ErrorKind::TimedOut
    );
    drop(peer);
    assert!(t.receive(monotonic_ns().unwrap() + MAX_WAIT_NS).is_err());
}
#[test]
fn stale_scope_closes_connection() {
    for change in 0..5 {
        let (t, peer) = pair();
        let mut b = bridge(t);
        let mut p = packet();
        match change {
            0 => p.scope.nonce += 1,
            1 => p.scope.lock_epoch = 0,
            2 => p.scope.sequence = 0,
            3 => p.scope.deadline_ns = 1,
            _ => p.scope.deadline_ns += MAX_WAIT_NS * 2,
        }
        raw(&peer, &p.encode().unwrap());
        let mut handler = Handler::default();
        assert!(
            b.step(&mut handler, monotonic_ns().unwrap() + MAX_WAIT_NS)
                .is_err()
        );
        assert!(handler.disconnected);
        assert!(b.transport.is_none());
    }
}
#[test]
fn actual_outcome_not_queue_admission() {
    let (t, peer) = pair();
    let mut b = bridge(t);
    let p = packet();
    raw(&peer, &p.encode().unwrap());
    let outcome = Outcome {
        result: ResultKind::StockConsumed,
        reason: 0,
        key: 30,
        state: 1,
        focus_epoch: 9,
    };
    raw(
        &peer,
        &Packet {
            scope: p.scope,
            body: Body::Outcome(outcome.clone()),
        }
        .encode()
        .unwrap(),
    );
    let mut handler = Handler::default();
    b.step(&mut handler, p.scope.deadline_ns).unwrap();
    assert_eq!(handler.reports, vec![DeliveryReport::Acknowledged(outcome)]);
    assert_eq!(
        handler.reports[0].replacement_delivery(),
        sledge_core::ReplacementDelivery::Uncertain
    );
}
#[test]
fn missing_or_stale_ack_is_uncertain() {
    for stale in [false, true] {
        let (t, peer) = pair();
        let mut b = bridge(t);
        let p = packet();
        raw(&peer, &p.encode().unwrap());
        if stale {
            let mut s = p.scope;
            s.nonce += 1;
            raw(
                &peer,
                &Packet {
                    scope: s,
                    body: Body::Outcome(Outcome {
                        result: ResultKind::OriginalAdmitted,
                        reason: 0,
                        key: 30,
                        state: 1,
                        focus_epoch: 9,
                    }),
                }
                .encode()
                .unwrap(),
            );
        }
        let mut handler = Handler::default();
        assert!(b.step(&mut handler, p.scope.deadline_ns).is_err());
        assert_eq!(handler.reports, vec![DeliveryReport::Uncertain]);
        assert!(handler.disconnected);
    }
}
#[test]
fn lock_advances_epoch_and_retire_never_reuses_generation() {
    let (t, peer) = pair();
    let mut b = bridge(t);
    let mut handler = Handler::default();
    let mut s = scope();
    s.generation = 0;
    s.lock_epoch = 2;
    raw(
        &peer,
        &Packet {
            scope: s,
            body: Body::Lock {
                state: 1,
                reason: 6,
            },
        }
        .encode()
        .unwrap(),
    );
    b.step(&mut handler, s.deadline_ns).unwrap();
    s.sequence += 1;
    s.generation = 1;
    raw(
        &peer,
        &Packet {
            scope: s,
            body: Body::Retire(1),
        }
        .encode()
        .unwrap(),
    );
    b.step(&mut handler, s.deadline_ns).unwrap();
    s.sequence += 1;
    raw(
        &peer,
        &Packet {
            scope: s,
            body: Body::Event(event()),
        }
        .encode()
        .unwrap(),
    );
    assert!(b.step(&mut handler, s.deadline_ns).is_err());
    assert_eq!(handler.resets, 2);
}
#[test]
fn causal_trace_orders_ack_and_reports_uncertainty_without_focus() {
    struct Traces(Vec<super::causal::CausalTrace>);
    impl EventHandler for Traces {
        fn propose(&mut self, _: Scope, _: &Event) -> Decision {
            Decision::Pass
        }
        fn delivery(&mut self, _: Scope, _: &Event, _: Decision, _: DeliveryReport) {}
        fn reset(&mut self, _: Scope, _: &Body) {}
        fn disconnected(&mut self) {}
        fn causal_trace(&mut self, t: super::causal::CausalTrace) {
            self.0.push(t);
        }
    }
    for acknowledged in [true, false] {
        let (transport, peer) = pair();
        let mut bridge = bridge(transport);
        let packet = packet();
        raw(&peer, &packet.encode().unwrap());
        if acknowledged {
            raw(
                &peer,
                &Packet {
                    scope: packet.scope,
                    body: Body::Outcome(Outcome {
                        result: ResultKind::OriginalAdmitted,
                        reason: 0,
                        key: 30,
                        state: 1,
                        focus_epoch: 9,
                    }),
                }
                .encode()
                .unwrap(),
            );
        }
        let mut h = Traces(Vec::new());
        let result = bridge.step(&mut h, packet.scope.deadline_ns);
        assert_eq!(result.is_ok(), acknowledged);
        assert_eq!(h.0.len(), 1);
        let t = h.0[0];
        assert_eq!(t.scope, packet.scope);
        assert!(t.received_ns.unwrap() <= t.decision_send_start_ns.unwrap());
        assert!(t.decision_send_start_ns.unwrap() <= t.decision_send_return_ns.unwrap());
        assert_eq!(t.decision_sent_ns, t.decision_send_return_ns);
        assert!(t.decision_sent_ns.unwrap() <= t.terminal_ns.unwrap());
        assert_eq!(t.outcome_received_ns.is_some(), acknowledged);
        assert_eq!(
            t.terminal,
            if acknowledged {
                super::causal::CausalTerminal::Acknowledged
            } else {
                super::causal::CausalTerminal::Uncertain
            }
        );
    }
}
#[test]
fn causal_failed_send_records_start_and_return_without_receipt() {
    struct FailSend {
        peer: Option<OwnedFd>,
        trace: Option<super::causal::CausalTrace>,
    }
    impl EventHandler for FailSend {
        fn propose(&mut self, _: Scope, _: &Event) -> Decision {
            self.peer.take();
            Decision::Pass
        }
        fn delivery(&mut self, _: Scope, _: &Event, _: Decision, _: DeliveryReport) {}
        fn reset(&mut self, _: Scope, _: &Body) {}
        fn disconnected(&mut self) {}
        fn causal_trace(&mut self, t: super::causal::CausalTrace) {
            self.trace = Some(t);
        }
    }
    let (transport, peer) = pair();
    let mut bridge = bridge(transport);
    let packet = packet();
    raw(&peer, &packet.encode().unwrap());
    let mut h = FailSend {
        peer: Some(peer),
        trace: None,
    };
    assert!(
        bridge
            .research_step(&mut h, packet.scope.deadline_ns)
            .is_err()
    );
    let t = h.trace.unwrap();
    assert!(t.decision_send_start_ns.unwrap() <= t.decision_send_return_ns.unwrap());
    assert!(t.decision_send_return_ns.unwrap() <= t.terminal_ns.unwrap());
    assert_eq!(t.decision_sent_ns, None);
    assert_eq!(t.outcome_received_ns, None);
    assert_eq!(t.terminal, super::causal::CausalTerminal::Uncertain);
    // A remote receiver can be scheduled during a sender's syscall interval.
    // Its time must be compared with start, not constrained after return.
    let (start, receive, returned) = (100_u64, 110_u64, 120_u64);
    assert!(receive >= start && receive < returned);
}
#[test]
fn validated_unlocked_epoch_is_distinct_from_real_unlock() {
    struct Controls {
        sync: usize,
        reset: usize,
    }
    impl EventHandler for Controls {
        fn propose(&mut self, _: Scope, _: &Event) -> Decision {
            Decision::Pass
        }
        fn delivery(&mut self, _: Scope, _: &Event, _: Decision, _: DeliveryReport) {}
        fn reset(&mut self, _: Scope, _: &Body) {
            self.reset += 1;
        }
        fn unlocked_epoch(&mut self, _: Scope, _: &Body) {
            self.sync += 1;
        }
        fn disconnected(&mut self) {}
    }
    let (transport, peer) = pair();
    let mut bridge = bridge(transport);
    let mut h = Controls { sync: 0, reset: 0 };
    let mut s = scope();
    s.generation = 0;
    for state in [0, 1, 0] {
        s.sequence += 1;
        s.lock_epoch = bridge.epoch + 1;
        s.deadline_ns = monotonic_ns().unwrap() + MAX_WAIT_NS;
        raw(
            &peer,
            &Packet {
                scope: s,
                body: Body::Lock { state, reason: 0 },
            }
            .encode()
            .unwrap(),
        );
        bridge.step(&mut h, s.deadline_ns).unwrap();
    }
    assert_eq!(h.sync, 1);
    assert_eq!(h.reset, 2);
    // Invalid same epoch is rejected before any semantic callback.
    s.sequence += 1;
    raw(
        &peer,
        &Packet {
            scope: s,
            body: Body::Lock {
                state: 0,
                reason: 0,
            },
        }
        .encode()
        .unwrap(),
    );
    assert!(bridge.step(&mut h, s.deadline_ns).is_err());
    assert_eq!(h.sync, 1);
    assert_eq!(h.reset, 2);
}
#[test]
fn local_reconciliation_request_closes_transport_preserving_history() {
    let (transport, peer) = pair();
    let mut bridge = bridge(transport);
    bridge.retired.insert(42);
    bridge.previous_nonces.insert(6);
    let nonce = bridge.nonce;
    let epoch = bridge.epoch;
    let mut handler = Handler::default();
    bridge.request_reconciliation(&mut handler);
    assert!(bridge.transport.is_none());
    assert!(handler.disconnected);
    assert!(bridge.awaiting_reconciliation);
    assert_eq!(bridge.nonce, nonce);
    assert_eq!(bridge.epoch, epoch);
    assert!(bridge.retired.contains(&42));
    assert!(bridge.previous_nonces.contains(&6));
    drop(peer);
}

#[test]
fn hello_identity_exact_and_capabilities_zero() {
    let (t, peer) = pair();
    let peer = Transport::from_fd(peer);
    let identity = Identity {
        capabilities: 0,
        api_hash: "hash".into(),
        artifact: "closure+digest".into(),
    };
    let mut s = scope();
    s.generation = 0;
    s.sequence = 0;
    peer.send(
        &Packet {
            scope: s,
            body: Body::Hello(identity.clone()),
        },
        s.deadline_ns,
    )
    .unwrap();
    let _b = Bridge::handshake(t, &identity, s.deadline_ns).unwrap();
    let ack = peer.receive(s.deadline_ns).unwrap();
    assert_eq!(ack.body, Body::HelloAck(identity));
}

#[test]
fn partial_output_ack_disables_bridge_and_never_replays_original() {
    let (transport, peer) = pair();
    let mut bridge = bridge(transport);
    let packet = packet();
    raw(&peer, &packet.encode().unwrap());
    let outcome = Outcome {
        result: ResultKind::PartialUncertainFatal,
        reason: 6,
        key: 30,
        state: 1,
        focus_epoch: 9,
    };
    raw(
        &peer,
        &Packet {
            scope: packet.scope,
            body: Body::Outcome(outcome.clone()),
        }
        .encode()
        .unwrap(),
    );
    let mut handler = Handler::default();
    assert!(bridge.step(&mut handler, packet.scope.deadline_ns).is_err());
    assert_eq!(handler.reports, vec![DeliveryReport::Acknowledged(outcome)]);
    assert!(handler.disconnected);
    assert!(bridge.transport.is_none());
}

#[test]
fn interop_vectors_are_exact_canonical_packets() {
    for line in include_str!("interop.hex").lines() {
        let (name, hex) = line.split_once(' ').unwrap();
        assert_eq!(hex.len(), 1024, "{name}");
        let bytes = (0..512)
            .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap())
            .collect::<Vec<_>>();
        let packet = Packet::decode(&bytes).unwrap();
        assert_eq!(packet.scope.nonce, 5);
        assert_eq!(
            packet.scope.lock_epoch,
            if name.starts_with("passive_") { 0 } else { 2 }
        );
        assert_eq!(
            packet.scope.sequence,
            if name.starts_with("passive_") { 0 } else { 3 }
        );
        assert_eq!(packet.encode().unwrap().as_slice(), bytes, "{name}");
        let mut malformed = bytes;
        malformed[511] = 1;
        assert!(Packet::decode(&malformed).is_err());
    }
}
#[test]
fn retired_generation_budget_fails_closed_without_eviction() {
    let (transport, peer) = pair();
    let mut bridge = bridge(transport);
    bridge
        .retired
        .extend(10..10 + u64::try_from(MAX_RETIRED_GENERATIONS).unwrap());
    let scope = scope();
    raw(
        &peer,
        &Packet {
            scope,
            body: Body::Retire(1),
        }
        .encode()
        .unwrap(),
    );
    let mut handler = Handler::default();
    assert!(bridge.step(&mut handler, scope.deadline_ns).is_err());
    assert_eq!(bridge.retired.len(), MAX_RETIRED_GENERATIONS);
    assert!(bridge.retired.contains(&10));
    assert!(handler.disconnected);
}
#[test]
fn malformed_outcome_correlation_and_pass_consistency_are_uncertain() {
    for change in 0..5 {
        let (transport, peer) = pair();
        let mut bridge = bridge(transport);
        let packet = packet();
        raw(&peer, &packet.encode().unwrap());
        let mut outcome = Outcome {
            result: ResultKind::OriginalAdmitted,
            reason: 0,
            key: 30,
            state: 1,
            focus_epoch: 9,
        };
        match change {
            0 => outcome.key += 1,
            1 => outcome.state = 0,
            2 => outcome.focus_epoch += 1,
            3 => outcome.result = ResultKind::Replaced,
            _ => outcome.result = ResultKind::SwallowCommitted,
        }
        raw(
            &peer,
            &Packet {
                scope: packet.scope,
                body: Body::Outcome(outcome),
            }
            .encode()
            .unwrap(),
        );
        let mut handler = Handler::default();
        assert!(bridge.step(&mut handler, packet.scope.deadline_ns).is_err());
        assert_eq!(handler.reports, vec![DeliveryReport::Uncertain]);
        assert!(handler.disconnected);
    }
}
#[test]
fn confirmed_focus_reset_is_reported_without_disconnect() {
    let (transport, peer) = pair();
    let mut bridge = bridge(transport);
    let scope = scope();
    raw(
        &peer,
        &Packet {
            scope,
            body: Body::Reset(2),
        }
        .encode()
        .unwrap(),
    );
    let mut handler = Handler::default();
    bridge.step(&mut handler, scope.deadline_ns).unwrap();
    assert_eq!(handler.resets, 1);
    assert!(!handler.disconnected);
    assert!(handler.reports.is_empty());
}

#[test]
fn reconciliation_control_is_global_once_current_epoch_and_not_locked() {
    for mode in 0..4 {
        let (transport, peer) = pair();
        let mut bridge = bridge(transport);
        let mut scope = scope();
        scope.generation = 0;
        match mode {
            0 => {}
            1 => scope.generation = 1,
            2 => scope.lock_epoch = 0,
            _ => bridge.locked = true,
        }
        raw(
            &peer,
            &Packet {
                scope,
                body: Body::Reset(8),
            }
            .encode()
            .unwrap(),
        );
        let mut handler = Handler::default();
        let result = bridge.research_step(&mut handler, scope.deadline_ns);
        if mode == 0 {
            result.unwrap();
            assert_eq!(handler.resets, 1);
            scope.sequence += 1;
            raw(
                &peer,
                &Packet {
                    scope,
                    body: Body::Reset(8),
                }
                .encode()
                .unwrap(),
            );
            assert!(
                bridge
                    .research_step(&mut handler, scope.deadline_ns)
                    .is_err()
            );
        } else {
            assert!(result.is_err());
            assert_eq!(handler.resets, 0);
        }
    }
}

#[test]
fn focus_and_keymap_rearm_reason8_but_other_resets_and_duplicates_do_not() {
    for reason in [0, 2, 3] {
        let (transport, peer) = pair();
        let mut bridge = bridge(transport);
        let mut handler = Handler::default();
        let mut scope = scope();
        scope.generation = 0;
        for body in [Body::Reset(8), Body::Reset(reason), Body::Reset(8)] {
            scope.deadline_ns = monotonic_ns().unwrap() + MAX_WAIT_NS;
            raw(
                &peer,
                &Packet {
                    scope,
                    body: body.clone(),
                }
                .encode()
                .unwrap(),
            );
            let result = bridge.research_step(&mut handler, scope.deadline_ns);
            if scope.sequence == 3 && reason == 0 {
                assert!(
                    result
                        .unwrap_err()
                        .to_string()
                        .contains("unsolicited reconciliation reason8")
                );
            } else {
                result.unwrap();
            }
            scope.sequence += 1;
        }
        if matches!(reason, 2 | 3) {
            raw(
                &peer,
                &Packet {
                    scope,
                    body: Body::Reset(8),
                }
                .encode()
                .unwrap(),
            );
            assert!(
                bridge
                    .research_step(&mut handler, scope.deadline_ns)
                    .unwrap_err()
                    .to_string()
                    .contains("unsolicited reconciliation reason8")
            );
        }
    }
}
#[test]
fn scope_diagnostics_are_specific_bounded_and_do_not_include_focus() {
    for (mode, expected) in [
        (0, "nonce mismatch"),
        (1, "sequence not increasing"),
        (2, "epoch mismatch"),
        (3, "retired generation"),
    ] {
        let (transport, peer) = pair();
        let mut bridge = bridge(transport);
        let mut packet = packet();
        match mode {
            0 => packet.scope.nonce += 1,
            1 => packet.scope.sequence = 0,
            2 => packet.scope.lock_epoch = 0,
            _ => {
                bridge.retired.insert(packet.scope.generation);
            }
        }
        raw(&peer, &packet.encode().unwrap());
        let message = bridge
            .research_step(&mut Handler::default(), packet.scope.deadline_ns)
            .unwrap_err()
            .to_string();
        assert!(message.contains(expected));
        assert!(message.len() < 400);
        assert!(!message.contains("app"));
    }
}
