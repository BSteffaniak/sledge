//! Same-user daemon backend. It never opens evdev or uinput directly.
use crate::protocol::{Message, Operation};
use crate::{authorization, keymap};
use sledge_core::{
    Action, BackendError, BackendVerdict, DeviceId, EventKind, EventSink, InputBackend, KeyEvent,
    Modifiers,
};
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

/// Cached desktop focus. Absence means unknown, never an invented application.
#[derive(Default)]
pub struct FocusTracker {
    current: RwLock<Option<(String, std::time::Instant)>>,
}
impl FocusTracker {
    #[must_use]
    pub fn current(&self) -> Option<String> {
        self.current.read().ok().and_then(|value| {
            value
                .as_ref()
                .filter(|(_, observed)| observed.elapsed() <= Duration::from_millis(500))
                .map(|(application, _)| application.clone())
        })
    }
    pub fn update(&self, application: Option<String>) {
        if let Ok(mut current) = self.current.write() {
            *current = application.map(|application| (application, std::time::Instant::now()));
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Capabilities {
    pub broker_connected: bool,
    pub verified_session: bool,
    pub source_switching: bool,
    pub reason: Option<String>,
}

/// Signal-safe in the sense that a normal signal handling thread can call it;
/// not intended for use inside a raw POSIX signal handler.
#[derive(Clone)]
pub struct ShutdownHandle {
    stopped: Arc<AtomicBool>,
    connection: Arc<Mutex<Option<UnixStream>>>,
}
impl ShutdownHandle {
    pub fn shutdown(&self) {
        self.stopped.store(true, Ordering::Release);
        if let Ok(connection) = self.connection.lock() {
            if let Some(socket) = connection.as_ref() {
                let _ = socket.shutdown(std::net::Shutdown::Both);
            }
        }
    }
}

pub type SourceDispatcher = Arc<dyn Fn(&str) -> Result<(), BackendError> + Send + Sync>;

/// Cloneable shared injection/focus/shutdown handle; only one clone may run.
#[derive(Clone)]
pub struct LinuxBackend {
    focus: Arc<FocusTracker>,
    stopped: Arc<AtomicBool>,
    connection: Arc<Mutex<Option<UnixStream>>>,
    running: Arc<AtomicBool>,
    source: Option<SourceDispatcher>,
}

impl Default for LinuxBackend {
    fn default() -> Self {
        Self::new()
    }
}
impl LinuxBackend {
    #[must_use]
    pub fn new() -> Self {
        Self {
            focus: Arc::default(),
            stopped: Arc::default(),
            connection: Arc::default(),
            running: Arc::default(),
            source: None,
        }
    }
    #[must_use]
    pub fn focus_tracker(&self) -> Arc<FocusTracker> {
        Arc::clone(&self.focus)
    }
    #[must_use]
    pub fn shutdown_handle(&self) -> ShutdownHandle {
        ShutdownHandle {
            stopped: Arc::clone(&self.stopped),
            connection: Arc::clone(&self.connection),
        }
    }
    pub fn set_source_dispatcher(&mut self, dispatcher: SourceDispatcher) {
        self.source = Some(dispatcher);
    }

    /// Non-grabbing inspection. A socket connection is not session authorization.
    #[must_use]
    pub fn capabilities(&self) -> Capabilities {
        Capabilities {
            broker_connected: self.connection.lock().is_ok_and(|socket| socket.is_some()),
            verified_session: false,
            source_switching: self.source.is_some(),
            reason: Some("broker activation requires qualified session/seat/lock authority".into()),
        }
    }
    pub fn check_permissions(&self) -> Result<Capabilities, BackendError> {
        Err(BackendError::MissingPermission(
            "verified Linux broker session/seat/lock authority is not available".into(),
        ))
    }

    /// Attach only a root-owned broker peer; the broker separately verifies this client.
    /// Does not authorize capture, and does not open a device.
    pub fn connect_broker(&mut self, path: &Path) -> Result<(), BackendError> {
        if nix::unistd::geteuid().is_root() {
            return Err(BackendError::MissingPermission(
                "Linux daemon must run as the desktop user, not root".into(),
            ));
        }
        let socket = UnixStream::connect(path).map_err(backend_io)?;
        let peer = authorization::peer_identity(&socket).map_err(backend_io)?;
        if peer.uid != 0 || peer.pid <= 0 {
            return Err(BackendError::MissingPermission(
                "broker peer is not privileged".into(),
            ));
        }
        socket
            .set_read_timeout(Some(Duration::from_millis(100)))
            .map_err(backend_io)?;
        socket
            .set_write_timeout(Some(Duration::from_millis(10)))
            .map_err(backend_io)?;
        *self
            .connection
            .lock()
            .map_err(|_| BackendError::Other("broker lock poisoned".into()))? = Some(socket);
        Ok(())
    }

    fn serve(&self, socket: &mut UnixStream, sink: &mut dyn EventSink) -> Result<(), BackendError> {
        let mut state = ClientState::default();
        while !self.stopped.load(Ordering::Acquire) {
            let mut ready = [nix::poll::PollFd::new(
                std::os::fd::AsFd::as_fd(socket),
                nix::poll::PollFlags::POLLIN,
            )];
            if nix::poll::poll(&mut ready, 100_u16)
                .map_err(io::Error::from)
                .map_err(backend_io)?
                == 0
            {
                continue;
            }
            // Once a frame starts, a single aggregate deadline protects framing.
            let request = Message::read_socket(
                socket,
                std::time::Instant::now() + crate::protocol::DECISION_TIMEOUT,
            )
            .map_err(backend_io)?;
            state.validate(request, sink)?;
            let mut operation = Operation::Pass;
            if matches!(
                request.operation,
                Operation::DeviceReset | Operation::DeviceRetire
            ) {
                state.physical.remove(&request.generation);
                if request.operation == Operation::DeviceRetire {
                    state.retire(request.generation);
                }
                sink.on_device_reset(DeviceId(request.generation));
                Message {
                    operation,
                    ..request
                }
                .write_socket(
                    socket,
                    std::time::Instant::now() + crate::protocol::DECISION_TIMEOUT,
                )
                .map_err(backend_io)?;
                continue;
            }
            if request.operation == Operation::SnapshotKey {
                if request.event.event_type != 1
                    || request.event.value != 1
                    || request.event.code > crate::state::MAX_KEY
                {
                    return Err(BackendError::Other("invalid broker key snapshot".into()));
                }
                state
                    .physical
                    .entry(request.generation)
                    .or_default()
                    .insert(request.event.code);
                Message {
                    operation,
                    ..request
                }
                .write_socket(
                    socket,
                    std::time::Instant::now() + crate::protocol::DECISION_TIMEOUT,
                )
                .map_err(backend_io)?;
                continue;
            }
            operation = state.dispatch(request, sink, &self.focus);
            Message {
                operation,
                ..request
            }
            .write(socket)
            .map_err(backend_io)?;
        }
        Ok(())
    }
}

#[derive(Default)]
struct ClientState {
    physical: BTreeMap<u64, BTreeSet<u16>>,
    epochs: BTreeMap<u64, u64>,
    sequences: BTreeMap<u64, u64>,
    highest_generation: u64,
}
impl ClientState {
    fn retire(&mut self, generation: u64) {
        self.physical.remove(&generation);
        self.epochs.remove(&generation);
        self.sequences.remove(&generation);
    }
    fn validate(&mut self, request: Message, sink: &mut dyn EventSink) -> Result<(), BackendError> {
        if !matches!(
            request.operation,
            Operation::Event
                | Operation::DeviceReset
                | Operation::SnapshotKey
                | Operation::DeviceRetire
        ) {
            return Err(BackendError::Other("unexpected broker operation".into()));
        }
        if request.generation == 0
            || request.epoch == 0
            || request.sequence == 0
            || self
                .sequences
                .get(&request.generation)
                .is_some_and(|last| *last >= request.sequence)
            || self
                .epochs
                .get(&request.generation)
                .is_some_and(|epoch| *epoch > request.epoch)
            || (!self.epochs.contains_key(&request.generation)
                && request.generation <= self.highest_generation)
            || (!self.epochs.contains_key(&request.generation) && self.epochs.len() >= 128)
        {
            return Err(BackendError::Other(
                "invalid or stale broker generation/epoch/sequence".into(),
            ));
        }
        self.highest_generation = self.highest_generation.max(request.generation);
        self.sequences.insert(request.generation, request.sequence);
        if self
            .epochs
            .insert(request.generation, request.epoch)
            .is_some_and(|previous| previous != request.epoch)
        {
            self.physical.remove(&request.generation);
            sink.on_device_reset(DeviceId(request.generation));
        }
        Ok(())
    }
    fn dispatch(
        &mut self,
        request: Message,
        sink: &mut dyn EventSink,
        focus: &FocusTracker,
    ) -> Operation {
        let mut operation = Operation::Pass;
        let raw = request.event;
        // Repeats belong to the existing press; never ask the matcher to establish
        // fresh ownership after focus or rule reload. The broker latches forwarding.
        if raw.event_type == 1 && raw.value == 2 {
            return Operation::Pass;
        }
        if raw.event_type == 1 && (0..=2).contains(&raw.value) {
            let keys = self.physical.entry(request.generation).or_default();
            if raw.value == 0 {
                keys.remove(&raw.code);
            } else {
                keys.insert(raw.code);
            }
            let mods = self
                .physical
                .get(&request.generation)
                .into_iter()
                .flat_map(|keys| keys.iter())
                .fold(Modifiers::empty(), |mods, code| {
                    mods | keymap::modifier(*code)
                });
            if let Some(code) = keymap::from_evdev(raw.code) {
                let kind = if !keymap::modifier(raw.code).is_empty() {
                    EventKind::ModifiersChanged
                } else if raw.value == 0 {
                    EventKind::KeyUp
                } else {
                    EventKind::KeyDown
                };
                let focus = focus.current();
                if matches!(
                    sink.on_device_event(
                        DeviceId(request.generation),
                        KeyEvent { code, kind, mods },
                        focus.as_deref()
                    ),
                    BackendVerdict::Swallow
                ) {
                    operation = Operation::Swallow;
                }
            }
        }
        operation
    }
}

impl InputBackend for LinuxBackend {
    fn run(&mut self, mut sink: Box<dyn EventSink>) -> Result<(), BackendError> {
        if self.running.swap(true, Ordering::AcqRel) {
            return Err(BackendError::Other("Linux backend already running".into()));
        }
        let result = (|| {
            let mut socket = self
                .connection
                .lock()
                .map_err(|_| BackendError::Other("broker lock poisoned".into()))?
                .as_ref()
                .ok_or_else(|| {
                    BackendError::MissingPermission("Linux broker is not connected".into())
                })?
                .try_clone()
                .map_err(backend_io)?;
            self.serve(&mut socket, sink.as_mut())
        })();
        sink.on_stream_reset();
        let requested_shutdown = self.stopped.load(Ordering::Acquire);
        self.shutdown_handle().shutdown();
        self.running.store(false, Ordering::Release);
        if requested_shutdown { Ok(()) } else { result }
    }

    fn inject(&self, action: &Action) -> Result<(), BackendError> {
        match action {
            Action::SetInputSource { id } => self.source.as_ref().ok_or_else(|| BackendError::UnknownInputSource(id.clone()))?(id),
            Action::SendKey { .. } => Err(BackendError::Inject("asynchronous injection is gated until broker output/modifier ownership is qualified".into())),
        }
    }
}
fn backend_io(error: io::Error) -> BackendError {
    BackendError::Other(format!("Linux broker: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::RawEvent;
    #[derive(Default)]
    struct Sink {
        devices: Vec<(DeviceId, KeyEvent)>,
        resets: Vec<DeviceId>,
    }
    impl EventSink for Sink {
        fn on_event(&mut self, _event: KeyEvent, _focus: Option<&str>) -> BackendVerdict {
            BackendVerdict::Pass
        }
        fn on_device_event(
            &mut self,
            device: DeviceId,
            event: KeyEvent,
            _focus: Option<&str>,
        ) -> BackendVerdict {
            self.devices.push((device, event));
            BackendVerdict::Pass
        }
        fn on_device_reset(&mut self, device: DeviceId) {
            self.resets.push(device);
        }
    }
    fn message(generation: u64, sequence: u64, code: u16) -> Message {
        Message {
            operation: Operation::Event,
            generation,
            epoch: 1,
            sequence,
            event: RawEvent {
                event_type: 1,
                code,
                value: 1,
            },
        }
    }
    #[test]
    fn device_modifiers_are_isolated_and_epoch_resets() {
        let mut state = ClientState::default();
        let mut sink = Sink::default();
        let focus = FocusTracker::default();
        let shift = message(1, 1, 42);
        state.validate(shift, &mut sink).unwrap();
        state.dispatch(shift, &mut sink, &focus);
        let other = message(2, 1, 30);
        state.validate(other, &mut sink).unwrap();
        state.dispatch(other, &mut sink, &focus);
        assert!(sink.devices[1].1.mods.is_empty());
        let same = message(1, 2, 30);
        state.validate(same, &mut sink).unwrap();
        state.dispatch(same, &mut sink, &focus);
        assert!(sink.devices[2].1.mods.contains(Modifiers::SHIFT));
        let reset = Message {
            epoch: 2,
            sequence: 3,
            ..same
        };
        state.validate(reset, &mut sink).unwrap();
        assert_eq!(sink.resets, vec![DeviceId(1)]);
        assert!(!state.physical.contains_key(&1));
        assert!(state.validate(same, &mut sink).is_err());
    }
    #[test]
    fn repeats_never_reenter_matching_after_focus_change() {
        let mut state = ClientState::default();
        let mut sink = Sink::default();
        let focus = FocusTracker::default();
        let press = message(1, 1, 30);
        state.validate(press, &mut sink).unwrap();
        state.dispatch(press, &mut sink, &focus);
        focus.update(Some("different-app".into()));
        let repeat = Message {
            sequence: 2,
            event: RawEvent {
                value: 2,
                ..press.event
            },
            ..press
        };
        state.validate(repeat, &mut sink).unwrap();
        assert_eq!(state.dispatch(repeat, &mut sink, &focus), Operation::Pass);
        assert_eq!(sink.devices.len(), 1);
    }
    #[test]
    fn retirement_bounds_active_not_lifetime_generations() {
        let mut state = ClientState::default();
        let mut sink = Sink::default();
        for generation in 1..=512 {
            state
                .validate(message(generation, 1, 30), &mut sink)
                .unwrap();
            state.retire(generation);
        }
        assert!(state.epochs.is_empty());
        assert!(state.sequences.is_empty());
        assert!(state.validate(message(1, 2, 30), &mut sink).is_err());
        for generation in 513..=640 {
            state
                .validate(message(generation, 1, 30), &mut sink)
                .unwrap();
        }
        assert!(state.validate(message(641, 1, 30), &mut sink).is_err());
        state.retire(513);
        assert!(state.validate(message(641, 1, 30), &mut sink).is_ok());
    }
    #[test]
    fn source_and_injection_unknown_fail_explicitly() {
        let backend = LinuxBackend::new();
        assert!(backend.check_permissions().is_err());
        assert!(!backend.capabilities().verified_session);
        assert!(
            backend
                .inject(&Action::SetInputSource {
                    id: "missing".into()
                })
                .is_err()
        );
        assert!(
            backend
                .inject(&Action::SendKey {
                    key: sledge_core::KeyCode::KeyA,
                    mods: Modifiers::empty()
                })
                .is_err()
        );
        backend.focus_tracker().update(Some("app".into()));
        assert_eq!(backend.focus_tracker().current().as_deref(), Some("app"));
        backend.focus_tracker().update(None);
        assert_eq!(backend.focus_tracker().current(), None);
    }
}
