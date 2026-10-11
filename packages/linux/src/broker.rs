//! Privileged device plumbing. No configuration parsing and no client-supplied paths.
//! Activation is unavailable until a session adapter can provide qualified proof.
use crate::authorization::{PeerIdentity, SessionAuthority, peer_identity};
use crate::protocol::{DECISION_TIMEOUT, Message, Operation, RawEvent};
use crate::state::KeyState;
use evdev::{EventType, InputEvent, KeyCode, raw_stream::RawDevice, uinput::VirtualDevice};
use nix::poll::{PollFd, PollFlags, poll};
use std::collections::BTreeSet;
use std::io;
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::os::unix::net::UnixStream;

const OUTPUT_NAME: &str = "sledge broker virtual keyboard";

/// A proxy owns exactly one physical input and one virtual output.
/// Constructed from a broker-selected fd, not from a wire path or user config.
pub struct DeviceProxy {
    input: RawDevice,
    output: VirtualDevice,
    state: KeyState,
    generation: u64,
    epoch: u64,
    sequence: u64,
    grabbed: bool,
    pending: Vec<InputEvent>,
}

impl DeviceProxy {
    pub fn from_authorized_fd(
        fd: OwnedFd,
        generation: u64,
        peer: PeerIdentity,
        authority: &mut dyn SessionAuthority,
    ) -> io::Result<Self> {
        verify(authority, peer)?;
        authority.verify_device(&fd, peer)?;
        nonblocking(&fd)?;
        let input = RawDevice::from_fd(fd)?;
        // Reject synthetic inputs and mixed devices whose capabilities we cannot reproduce.
        if input
            .name()
            .is_some_and(|name| name.starts_with("sledge broker"))
            || input.input_id().bus_type() == evdev::BusType::BUS_VIRTUAL
            || input.supported_events().iter().any(|kind| {
                !matches!(
                    kind,
                    EventType::SYNCHRONIZATION | EventType::KEY | EventType::MISC
                )
            })
        {
            return Err(denied("unsupported or synthetic input device"));
        }
        let keys = input
            .supported_keys()
            .ok_or_else(|| denied("not a keyboard"))?;
        if !keys.contains(KeyCode::KEY_A) || !keys.contains(KeyCode::KEY_ENTER) {
            return Err(denied("not a keyboard"));
        }
        // Do not take ownership while a physical key is already held.
        if input.get_key_state()?.iter().next().is_some() {
            return Err(denied("keyboard is not quiescent"));
        }
        let mut builder = VirtualDevice::builder()?
            .name(OUTPUT_NAME)
            .with_keys(keys)?;
        if let Some(misc) = input.misc_properties() {
            builder = builder.with_msc(misc)?;
        }
        let output = builder.build()?;
        nonblocking(&output)?;
        Ok(Self {
            input,
            output,
            state: KeyState::default(),
            generation,
            epoch: 1,
            sequence: 0,
            grabbed: false,
            pending: Vec::new(),
        })
    }

    /// Recheck authorization immediately before capture. Never called by the default binary.
    pub fn activate(
        &mut self,
        peer: PeerIdentity,
        authority: &mut dyn SessionAuthority,
    ) -> io::Result<()> {
        verify(authority, peer)?;
        authority.verify_device(&self.input, peer)?;
        if self.input.get_key_state()?.iter().next().is_some() {
            return Err(denied("keyboard is not quiescent"));
        }
        authority.verify_output_ready(&self.output, peer)?;
        self.input.grab()?;
        self.grabbed = true;
        let result = self
            .check_post_grab()
            .and_then(|()| authority.verify_output_ready(&self.output, peer));
        if result.is_err() {
            self.bypass();
        }
        result
    }

    fn check_post_grab(&mut self) -> io::Result<()> {
        let queued = match self.input.fetch_events() {
            Ok(mut events) => events.next().is_some(),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => false,
            Err(error) => return Err(error),
        };
        quiescent(!queued, self.input.get_key_state()?.iter().next().is_none())
    }

    /// Bounded polling allows the owner to recheck session state and discover hotplug.
    /// Any client, authorization or output failure tears down capture before returning.
    pub fn pump(
        &mut self,
        socket: &mut UnixStream,
        authority: &mut dyn SessionAuthority,
    ) -> io::Result<()> {
        let result = self.pump_inner(socket, authority);
        if result.is_err() {
            self.bypass();
        }
        result
    }

    fn pump_inner(
        &mut self,
        socket: &mut UnixStream,
        authority: &mut dyn SessionAuthority,
    ) -> io::Result<()> {
        let peer = peer_identity(socket)?;
        verify(authority, peer)?;
        authority.verify_device(&self.input, peer)?;
        authority.verify_output_ready(&self.output, peer)?;
        if !self.grabbed {
            return Err(denied("proxy is not active"));
        }
        let mut peer_poll = [PollFd::new(socket.as_fd(), PollFlags::POLLIN)];
        poll(&mut peer_poll, 0_u16).map_err(io::Error::from)?;
        if peer_poll[0].revents().is_some_and(|flags| {
            flags.intersects(PollFlags::POLLHUP | PollFlags::POLLERR | PollFlags::POLLNVAL)
        }) {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "client disconnected",
            ));
        }
        let mut fds = [PollFd::new(self.input.as_fd(), PollFlags::POLLIN)];
        if poll(&mut fds, 25_u16).map_err(io::Error::from)? == 0 {
            return Ok(());
        }
        if fds[0].revents().is_some_and(|flags| {
            flags.intersects(PollFlags::POLLHUP | PollFlags::POLLERR | PollFlags::POLLNVAL)
        }) {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "input removed"));
        }
        let events: Vec<_> = self.input.fetch_events()?.take(257).collect();
        if events.len() > 256 {
            return Err(denied("input batch exceeds bound"));
        }
        socket.set_read_timeout(Some(DECISION_TIMEOUT))?;
        socket.set_write_timeout(Some(DECISION_TIMEOUT))?;
        for event in events {
            verify(authority, peer)?;
            let raw = RawEvent {
                event_type: event.event_type().0,
                code: event.code(),
                value: event.value(),
            };
            if self.state.dropping() && raw.event_type == 0 && raw.code == 0 {
                let keys: BTreeSet<_> = self
                    .input
                    .get_key_state()?
                    .iter()
                    .map(KeyCode::code)
                    .collect();
                self.epoch = self
                    .epoch
                    .checked_add(1)
                    .ok_or_else(|| denied("epoch exhausted"))?;
                let reconciled = self.state.resync(keys.clone());
                self.reset_client(socket)?;
                for code in keys {
                    self.control(socket, Operation::SnapshotKey, crate::state::key(code, 1))?;
                }
                self.emit(&reconciled)?;
                self.emit(&[RawEvent::default()])?;
                continue;
            }
            self.sequence = self
                .sequence
                .checked_add(1)
                .ok_or_else(|| denied("sequence exhausted"))?;
            let request = Message {
                operation: Operation::Event,
                generation: self.generation,
                epoch: self.epoch,
                sequence: self.sequence,
                event: raw,
            };
            let decision = if raw.event_type == 1 && !self.state.dropping() {
                let deadline = std::time::Instant::now() + DECISION_TIMEOUT;
                request.write_socket(socket, deadline)?;
                let reply = Message::read_socket(socket, deadline)?;
                admitted_decision(request, reply)?
            } else {
                Operation::Pass
            };
            if raw.event_type == 0 && raw.code == 3 {
                self.pending.clear();
                self.state.rollback_output();
            }
            let output = self.state.apply(raw, decision);
            self.emit(&output)?;
        }
        Ok(())
    }

    pub fn retire_client(&mut self, socket: &mut UnixStream) -> io::Result<()> {
        self.control(socket, Operation::DeviceRetire, RawEvent::default())
    }

    pub fn reset_client(&mut self, socket: &mut UnixStream) -> io::Result<()> {
        self.control(socket, Operation::DeviceReset, RawEvent::default())
    }

    fn control(
        &mut self,
        socket: &mut UnixStream,
        operation: Operation,
        event: RawEvent,
    ) -> io::Result<()> {
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| denied("sequence exhausted"))?;
        let request = Message {
            operation,
            generation: self.generation,
            epoch: self.epoch,
            sequence: self.sequence,
            event,
        };
        let deadline = std::time::Instant::now() + DECISION_TIMEOUT;
        request.write_socket(socket, deadline)?;
        let reply = Message::read_socket(socket, deadline)?;
        if reply.operation != Operation::Pass || !reply.answers(request) {
            return Err(denied("invalid reset/snapshot acknowledgement"));
        }
        Ok(())
    }

    fn emit(&mut self, events: &[RawEvent]) -> io::Result<()> {
        for event in events {
            if event.event_type == 0 && event.code == 0 {
                self.output.emit(&self.pending)?;
                self.state.commit_output();
                self.pending.clear();
            } else if event.event_type != 0 {
                if self.pending.len() >= 256 {
                    return Err(denied("output frame exceeds bound"));
                }
                self.pending
                    .push(InputEvent::new(event.event_type, event.code, event.value));
            }
        }
        Ok(())
    }

    /// Best effort output cleanup must never prevent ungrab on broken uinput.
    pub fn bypass(&mut self) {
        self.pending.clear();
        let mut releases = self.state.release_all();
        releases.push(RawEvent::default());
        let _ = self.emit(&releases);
        if self.grabbed {
            let _ = self.input.ungrab();
            self.grabbed = false;
        }
    }
}
impl Drop for DeviceProxy {
    fn drop(&mut self) {
        self.bypass();
    }
}

/// Only echoing pass/swallow decisions are supported. Replacement is reserved,
/// not authorized: no synthetic output can occur without a qualified cleanup ledger.
fn admitted_decision(request: Message, reply: Message) -> io::Result<Operation> {
    if !matches!(reply.operation, Operation::Pass | Operation::Swallow)
        || !reply.answers(request)
        || reply.event != request.event
    {
        return Err(denied("unsupported or malformed broker decision"));
    }
    Ok(reply.operation)
}

fn quiescent(queue_empty: bool, keys_empty: bool) -> io::Result<()> {
    if !queue_empty || !keys_empty {
        return Err(denied("input changed during grab; capture refused"));
    }
    Ok(())
}

fn nonblocking(fd: &impl AsRawFd) -> io::Result<()> {
    use nix::fcntl::{FcntlArg, OFlag, fcntl};
    let flags = fcntl(fd.as_raw_fd(), FcntlArg::F_GETFL).map_err(io::Error::from)?;
    fcntl(
        fd.as_raw_fd(),
        FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK),
    )
    .map_err(io::Error::from)?;
    Ok(())
}

fn verify(authority: &mut dyn SessionAuthority, peer: PeerIdentity) -> io::Result<()> {
    if !authority.verify(peer)?.permits(peer) {
        return Err(denied("peer session is not verified local active unlocked"));
    }
    Ok(())
}
fn denied(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn privileged_decisions_reject_replace_and_malformed_echoes() {
        let request = Message {
            operation: Operation::Event,
            generation: 1,
            epoch: 1,
            sequence: 1,
            event: crate::state::key(30, 1),
        };
        for operation in [Operation::Pass, Operation::Swallow] {
            assert_eq!(
                admitted_decision(
                    request,
                    Message {
                        operation,
                        ..request
                    }
                )
                .unwrap(),
                operation
            );
        }
        for operation in [
            Operation::Replace,
            Operation::Event,
            Operation::DeviceReset,
            Operation::DeviceRetire,
            Operation::SnapshotKey,
        ] {
            assert!(
                admitted_decision(
                    request,
                    Message {
                        operation,
                        ..request
                    }
                )
                .is_err()
            );
        }
        assert!(
            admitted_decision(
                request,
                Message {
                    operation: Operation::Pass,
                    sequence: 2,
                    ..request
                }
            )
            .is_err()
        );
        assert!(
            admitted_decision(
                request,
                Message {
                    operation: Operation::Pass,
                    event: crate::state::key(31, 1),
                    ..request
                }
            )
            .is_err()
        );
        let mut bytes = Vec::new();
        request.write(&mut bytes).unwrap();
        bytes[4] = 255;
        assert!(Message::read(&mut bytes.as_slice()).is_err());
        bytes[4] = Operation::Replace as u8;
        let reply = Message::read(&mut bytes.as_slice()).unwrap();
        assert!(admitted_decision(request, reply).is_err());
    }
    #[test]
    fn post_grab_held_key_or_queued_transition_denies_activation() {
        assert!(quiescent(true, true).is_ok());
        assert!(quiescent(false, true).is_err());
        assert!(quiescent(true, false).is_err());
        assert!(quiescent(false, false).is_err());
    }
    #[test]
    fn default_authority_cannot_claim_output_readiness() {
        let (socket, _) = UnixStream::pair().unwrap();
        let peer = peer_identity(&socket).unwrap();
        let mut authority = crate::authorization::UnavailableAuthority;
        assert!(authority.verify_output_ready(&socket, peer).is_err());
    }
}
