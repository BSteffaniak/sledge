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
