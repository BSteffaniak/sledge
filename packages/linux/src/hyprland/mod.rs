//! Worker-side native event/actual-outcome bridge. Not an InputBackend and not
//! capture qualification: callers must not enable stock hooks from this API.
pub mod catalogue;
pub mod listener;
pub mod matcher;
pub mod session;
pub mod transport;
pub mod wire;

use std::collections::HashSet;
use std::io;
use transport::{Transport, monotonic_ns};
use wire::{Body, Decision, Event, Identity, MAX_WAIT_NS, Outcome, Packet, ResultKind, Scope};

/// Maximum retirement tombstones per connection. Exhaustion closes the peer;
/// never evict a tombstone and silently allow a reused physical generation.
pub const MAX_RETIRED_GENERATIONS: usize = 4096;
/// Bound replay history across explicitly admitted reconnects; never evict.
pub const MAX_CONNECTION_NONCES: usize = 64;

/// Explicit isolated-peer faults. Never applied by the passive bridge.
#[derive(Clone, Copy, Debug, Default)]
pub enum ResearchFault {
    #[default]
    None,
    DelayDecision(std::time::Duration),
    DropDecision,
    DisconnectBeforeDecision,
    DisconnectAfterDecision,
}

/// Actual route acknowledgement or missing/invalid acknowledgement. Never
/// translate an IPC failure to original-forwarded or replay input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeliveryReport {
    Acknowledged(Outcome),
    Uncertain,
}
impl DeliveryReport {
    /// Conservative mapping for the existing opaque matcher replacement token.
    /// Stock consumption/merge/locker ownership is NOT original forwarding.
    #[must_use]
    pub const fn replacement_delivery(&self) -> sledge_core::ReplacementDelivery {
        use sledge_core::ReplacementDelivery;
        match self {
            Self::Acknowledged(Outcome {
                result: ResultKind::Replaced,
                ..
            }) => ReplacementDelivery::Delivered,
            Self::Acknowledged(Outcome {
                result: ResultKind::RejectedOriginalAdmitted | ResultKind::TimeoutOriginalAdmitted,
                ..
            }) => ReplacementDelivery::RejectedOriginalForwarded,
            _ => ReplacementDelivery::Uncertain,
        }
    }
}

/// Synchronous proposal interface, deliberately separate from async core inject.
/// Keep an opaque matcher token until `delivery`; callbacks share the aggregate
/// native deadline and must not perform I/O or enqueue replacement injection.
/// Physical output ownership and reconciliation remain native responsibilities.
pub trait EventHandler {
    fn propose(&mut self, scope: Scope, event: &Event) -> Decision;
    /// Isolated research only, one selected proposal. Default has no fault.
    fn research_fault(&mut self, _scope: Scope, _event: &Event) -> ResearchFault {
        ResearchFault::None
    }
    fn delivery(&mut self, scope: Scope, event: &Event, decision: Decision, report: DeliveryReport);
    /// Invalidate only speculative matcher proposals. Do not erase native holds.
    fn reset(&mut self, scope: Scope, body: &Body);
    /// Disable future replacement and request explicit native reconciliation.
    fn disconnected(&mut self);
}

/// Probe bridge only. Native capabilities/identity alone cannot qualify capture.
/// The frozen proposal transport can be exercised with mock/native research
/// peers, but production capture stays denied until coordinator qualification.
pub struct Bridge {
    transport: Option<Transport>,
    nonce: u64,
    epoch: u64,
    sequence: u64,
    retired: HashSet<u64>,
    previous_nonces: HashSet<u64>,
    awaiting_reconciliation: bool,
    locked: bool,
}
impl Bridge {
    /// Authenticate transport first, then verify the exact registered identity.
    /// Advertises zero capabilities even if the peer claims a qualified route.
    /// # Errors
    /// Rejects stale/malformed Hello and unregistered native identity.
    pub fn handshake(
        transport: Transport,
        expected: &Identity,
        deadline_ns: u64,
    ) -> io::Result<Self> {
        Self::handshake_inner(transport, expected, deadline_ns, false)
    }
    /// Passive native Hello has global generation/epoch/sequence/deadline zero.
    /// Echo it exactly under a separate bounded local handshake deadline. This
    /// exception never relaxes event/decision/outcome transaction deadlines.
    /// # Errors
    /// Rejects nonzero capabilities/scope, wrong identity or missed local deadline.
    pub fn passive_handshake(
        transport: Transport,
        expected: &Identity,
        deadline_ns: u64,
    ) -> io::Result<Self> {
        if expected.capabilities != 0 {
            return Err(wire::invalid());
        }
        Self::handshake_inner(transport, expected, deadline_ns, true)
    }
    /// Bind caller-owned matcher recovery state to this authenticated nonce.
    #[must_use]
    pub const fn connection_nonce(&self) -> u64 {
        self.nonce
    }
    /// Close locally uncertain transport; retain nonce/epoch/retirement history.
    /// Recovery requires fresh admission and native reason8, not original replay.
    pub fn request_reconciliation(&mut self, handler: &mut impl EventHandler) {
        self.transport.take();
        self.awaiting_reconciliation = true;
        handler.disconnected();
    }
    /// Explicit reconnect to the same native generation namespace. Authenticate
    /// a fresh zero-capability Hello without clearing retirement history or
    /// matcher holds. The caller retains the SAME transaction/matcher state;
    /// this is transport admission, NOT reconciliation or replacement reenable.
    /// # Errors
    /// Rejects live connection, reused nonce, identity mismatch or history budget.
    pub fn reconnect(
        &mut self,
        transport: Transport,
        expected: &Identity,
        deadline_ns: u64,
    ) -> io::Result<()> {
        if expected.capabilities != 0
            || self.transport.is_some()
            || self.previous_nonces.len() >= MAX_CONNECTION_NONCES
        {
            return Err(wire::invalid());
        }
        // Verify nonce before sending HelloAck so a replay is never admitted.
        let hello = transport.receive(deadline_ns)?;
        if hello.scope.nonce == self.nonce || self.previous_nonces.contains(&hello.scope.nonce) {
            return Err(wire::invalid());
        }
        let fresh = Self::handshake_packet(transport, expected, deadline_ns, true, hello)?;
        self.previous_nonces.insert(self.nonce);
        self.nonce = fresh.nonce;
        // Compositor epochs/generations survive socket reconnect. Zero-scope
        // Hello must not silently unlock or reset that authoritative boundary.
        self.sequence = 0;
        self.transport = fresh.transport;
        self.awaiting_reconciliation = true;
        Ok(())
    }
    fn handshake_inner(
        transport: Transport,
        expected: &Identity,
        deadline_ns: u64,
        passive: bool,
    ) -> io::Result<Self> {
        let hello = transport.receive(deadline_ns)?;
        Self::handshake_packet(transport, expected, deadline_ns, passive, hello)
    }
    fn handshake_packet(
        transport: Transport,
        expected: &Identity,
        deadline_ns: u64,
        passive: bool,
        hello: Packet,
    ) -> io::Result<Self> {
        let Body::Hello(identity) = &hello.body else {
            return Err(wire::invalid());
        };
        if passive {
            if hello.scope.lock_epoch != 0 || hello.scope.deadline_ns != 0 {
                return Err(wire::invalid());
            }
        } else {
            check_deadline(hello.scope.deadline_ns)?;
        }
        if identity != expected
            || identity.api_hash.is_empty()
            || identity.artifact.is_empty()
            || hello.scope.generation != 0
            || hello.scope.sequence != 0
            || hello.scope.nonce == 0
        {
            return Err(wire::invalid());
        }
        let ack = Packet {
            scope: hello.scope,
            body: Body::HelloAck(Identity {
                capabilities: 0,
                api_hash: identity.api_hash.clone(),
                artifact: identity.artifact.clone(),
            }),
        };
        transport.send(
            &ack,
            if passive {
                deadline_ns
            } else {
                deadline_ns.min(hello.scope.deadline_ns)
            },
        )?;
        Ok(Self {
            transport: Some(transport),
            nonce: hello.scope.nonce,
            epoch: hello.scope.lock_epoch,
            sequence: 0,
            retired: HashSet::new(),
            previous_nonces: HashSet::new(),
            awaiting_reconciliation: true,
            locked: false,
        })
    }
    /// Process one native packet on a worker thread. Probe mode always proposes
    /// Pass; it never calls a matcher that could consume a physical press.
    /// # Errors
    /// Any protocol/transport/deadline failure closes the connection; replacement
    /// delivery is uncertain after a sent decision, never guessed from timeout.
    pub fn step(&mut self, handler: &mut impl EventHandler, deadline_ns: u64) -> io::Result<()> {
        self.step_mode(handler, deadline_ns, false)
    }
    /// Explicit isolated-research execution. Calls the handler synchronously;
    /// does not qualify capture, advertise capabilities or change passive step.
    /// # Errors
    /// Closes on deadline, malformed ACK or partial uncertainty, without replay.
    pub fn research_step(
        &mut self,
        handler: &mut impl EventHandler,
        deadline_ns: u64,
    ) -> io::Result<()> {
        self.step_mode(handler, deadline_ns, true)
    }
    /// Explicit research worker idle poll. No readiness before idle expiry
    /// returns false without touching state or handler. After receiving a packet,
    /// its native deadline governs the transaction, not the idle poll deadline.
    /// # Errors
    /// Actual receive/transaction failures close the peer and notify disconnect.
    pub fn research_step_if_ready(
        &mut self,
        handler: &mut impl EventHandler,
        idle_deadline_ns: u64,
    ) -> io::Result<bool> {
        self.ready_mode(handler, idle_deadline_ns, true)
    }
    /// Compiled-catalogue session transaction entry. Admission and fault-free
    /// handler wrapping are enforced by Session; no runtime activation flag.
    fn admitted_step_if_ready(
        &mut self,
        handler: &mut impl EventHandler,
        idle_deadline_ns: u64,
    ) -> io::Result<bool> {
        self.ready_mode(handler, idle_deadline_ns, true)
    }
    /// Passive idle poll; never proposes or runs research faults.
    /// # Errors
    /// Actual packet failures close the peer, idle expiry leaves it unchanged.
    pub fn passive_step_if_ready(
        &mut self,
        handler: &mut impl EventHandler,
        idle_deadline_ns: u64,
    ) -> io::Result<bool> {
        self.ready_mode(handler, idle_deadline_ns, false)
    }
    fn ready_mode(
        &mut self,
        handler: &mut impl EventHandler,
        idle_deadline_ns: u64,
        research: bool,
    ) -> io::Result<bool> {
        let result = (|| {
            let Some(packet) = self
                .transport
                .as_ref()
                .ok_or_else(wire::invalid)?
                .receive_if_ready(idle_deadline_ns)?
            else {
                return Ok(false);
            };
            let deadline = packet.scope.deadline_ns;
            self.process_packet(handler, packet, deadline, research)?;
            Ok(true)
        })();
        if result.is_err() {
            self.transport.take();
            handler.disconnected();
        }
        result
    }
    fn step_mode(
        &mut self,
        handler: &mut impl EventHandler,
        deadline_ns: u64,
        research: bool,
    ) -> io::Result<()> {
        let result = self.step_inner(handler, deadline_ns, research);
        if result.is_err() {
            self.transport.take();
            handler.disconnected();
        }
        result
    }
    fn step_inner(
        &mut self,
        handler: &mut impl EventHandler,
        deadline_ns: u64,
        research: bool,
    ) -> io::Result<()> {
        let packet = self
            .transport
            .as_ref()
            .ok_or_else(wire::invalid)?
            .receive(deadline_ns)?;
        self.process_packet(handler, packet, deadline_ns, research)
    }
    fn process_packet(
        &mut self,
        handler: &mut impl EventHandler,
        packet: Packet,
        deadline_ns: u64,
        research: bool,
    ) -> io::Result<()> {
        self.validate(&packet)?;
        match &packet.body {
            Body::Event(event) => self.event(handler, &packet, event, deadline_ns, research),
            Body::Reset(_) | Body::Retire(_) | Body::Lock { .. } => {
                if matches!(packet.body, Body::Reset(3)) && packet.scope.generation == 0 {
                    self.awaiting_reconciliation = true;
                }
                if matches!(packet.body, Body::Reset(8)) {
                    let reason = if packet.scope.generation != 0 {
                        Some("reconciliation generation not global")
                    } else if self.locked {
                        Some("reconciliation while locked")
                    } else if !self.awaiting_reconciliation {
                        Some("unsolicited reconciliation reason8")
                    } else {
                        None
                    };
                    if let Some(reason) = reason {
                        return Err(scope_error(
                            reason,
                            packet.scope,
                            self.nonce,
                            self.sequence,
                            self.epoch,
                        ));
                    }
                    self.awaiting_reconciliation = false;
                }
                if matches!(packet.body, Body::Retire(_))
                    && (self.retired.len() >= MAX_RETIRED_GENERATIONS
                        || !self.retired.insert(packet.scope.generation))
                {
                    return Err(wire::invalid());
                }
                handler.reset(packet.scope, &packet.body);
                Ok(())
            }
            _ => Err(wire::invalid()),
        }
    }
    fn validate(&mut self, packet: &Packet) -> io::Result<()> {
        let scope = packet.scope;
        check_deadline(scope.deadline_ns)?;
        let lock = matches!(packet.body, Body::Lock { .. });
        let reject = |reason| scope_error(reason, scope, self.nonce, self.sequence, self.epoch);
        if scope.nonce != self.nonce {
            return Err(reject("nonce mismatch"));
        }
        if scope.sequence <= self.sequence {
            return Err(reject("sequence not increasing"));
        }
        if self.retired.contains(&scope.generation) {
            return Err(reject("retired generation"));
        }
        if lock && scope.generation != 0 {
            return Err(reject("lock generation must be global"));
        }
        if lock && scope.lock_epoch <= self.epoch {
            return Err(reject("lock epoch not advancing"));
        }
        if !lock && scope.lock_epoch != self.epoch {
            return Err(reject("epoch mismatch"));
        }
        if matches!(packet.body, Body::Event(_)) && scope.generation == 0 {
            return Err(reject("event generation zero"));
        }
        self.sequence = scope.sequence;
        self.epoch = scope.lock_epoch;
        if let Body::Lock { state, .. } = packet.body {
            self.locked = state != 0;
        }
        Ok(())
    }
    fn event(
        &self,
        handler: &mut impl EventHandler,
        packet: &Packet,
        event: &Event,
        deadline_ns: u64,
        research: bool,
    ) -> io::Result<()> {
        // Capture remains unqualified. Do not dispatch to a stateful matcher.
        let decision = if research && !self.locked && event.state == 1 {
            handler.propose(packet.scope, event)
        } else {
            Decision::Pass
        };
        let deadline = deadline_ns.min(packet.scope.deadline_ns);
        let fault = if research && !self.locked && event.state == 1 {
            handler.research_fault(packet.scope, event)
        } else {
            ResearchFault::None
        };
        if let Err(error) = apply_before_fault(fault, deadline) {
            handler.delivery(packet.scope, event, decision, DeliveryReport::Uncertain);
            return Err(error);
        }
        let transport = self.transport.as_ref().ok_or_else(wire::invalid)?;
        if let Err(error) = transport.send(
            &Packet {
                scope: packet.scope,
                body: Body::Decision(decision),
            },
            deadline,
        ) {
            handler.delivery(packet.scope, event, decision, DeliveryReport::Uncertain);
            return Err(error);
        }
        if matches!(fault, ResearchFault::DisconnectAfterDecision) {
            handler.delivery(packet.scope, event, decision, DeliveryReport::Uncertain);
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "selected disconnect after decision",
            ));
        }
        let result = transport.receive(deadline).and_then(|ack| {
            let Body::Outcome(outcome) = ack.body else {
                return Err(wire::invalid());
            };
            if ack.scope != packet.scope
                || outcome.key != event.key
                || outcome.state != event.state
                || outcome.focus_epoch != event.focus_epoch
                || (outcome.result == ResultKind::Replaced
                    && !matches!(decision, Decision::Replace { .. }))
                || (event.state == 1
                    && outcome.result == ResultKind::SwallowCommitted
                    && decision != Decision::Swallow)
            {
                return Err(wire::invalid());
            }
            Ok(outcome)
        });
        let report = result.as_ref().map_or(DeliveryReport::Uncertain, |o| {
            DeliveryReport::Acknowledged(o.clone())
        });
        handler.delivery(packet.scope, event, decision, report);
        if result
            .as_ref()
            .is_ok_and(|outcome| outcome.result == ResultKind::PartialUncertainFatal)
        {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "native partial output requires reconciliation",
            ));
        }
        result.map(|_| ())
    }
}
fn scope_error(reason: &str, scope: Scope, nonce: u64, sequence: u64, epoch: u64) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
            "Hyprland {reason}: generation={} nonce={}/{nonce} sequence={}/{sequence} epoch={}/{epoch} deadline={}",
            scope.generation, scope.nonce, scope.sequence, scope.lock_epoch, scope.deadline_ns
        ),
    )
}
fn apply_before_fault(fault: ResearchFault, deadline: u64) -> io::Result<()> {
    match fault {
        ResearchFault::None | ResearchFault::DisconnectAfterDecision => Ok(()),
        ResearchFault::DelayDecision(delay) => {
            std::thread::sleep(delay);
            check_deadline(deadline)
        }
        ResearchFault::DropDecision => {
            let left = deadline.saturating_sub(monotonic_ns()?);
            std::thread::sleep(std::time::Duration::from_nanos(left));
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "selected dropped decision; outcome uncertain",
            ))
        }
        ResearchFault::DisconnectBeforeDecision => Err(io::Error::new(
            io::ErrorKind::ConnectionAborted,
            "selected disconnect before decision",
        )),
    }
}
fn check_deadline(deadline_ns: u64) -> io::Result<()> {
    let now = monotonic_ns()?;
    match deadline_ns.checked_sub(now) {
        Some(1..=MAX_WAIT_NS) => Ok(()),
        Some(_) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Hyprland deadline outside 1..=10ms budget",
        )),
        None => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "stale Hyprland transaction",
        )),
    }
}

#[cfg(test)]
mod endpoint_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod research_tests;
