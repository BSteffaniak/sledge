//! CGEventTap installation, watchdog, and event dispatch.
//!
//! The tap is installed at the HID level with head-insert placement so we
//! see events before any app does. Our callback:
//!
//! 1. Checks the event-source userdata for the self-event sentinel; if
//!    present, returns `Pass` immediately \u2014 we never re-process our
//!    own injected output.
//! 2. Handles `TapDisabledByTimeout` / `TapDisabledByUserInput` by
//!    re-enabling the tap in place.
//! 3. Maps the `CGEvent` to a normalized [`KeyEvent`].
//! 4. Calls the [`EventSink`] for a verdict.
//! 5. Applies the verdict: drops the event, passes it through, or queues
//!    a synthesized replacement to run on a worker thread.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use core_foundation::base::TCFType;
use core_foundation::runloop::{CFRunLoop, kCFRunLoopCommonModes};
use core_graphics::event::{
    CGEvent, CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventType, EventField,
};
use parking_lot::Mutex;
use sledge_core::{
    Action, BackendError, BackendVerdict, EventKind, EventSink, InputBackend, KeyCode, KeyEvent,
    Modifiers,
};
use tracing::{debug, info, trace, warn};

use crate::focus::{FocusTracker, PollerHandle, spawn_poller};
use crate::inject::{SLEDGE_EVENT_SOURCE_TAG, send_key};
use crate::keycode::from_cg_keycode;
use crate::permission::check_permissions;
use crate::tis::set_input_source;

/// macOS implementation of [`InputBackend`].
pub struct MacOsBackend {
    focus: Arc<FocusTracker>,
    _poller: Option<PollerHandle>,
    // Shared live sink set once run() is called.
    sink: Arc<Mutex<Option<Box<dyn EventSink>>>>,
    // Watchdog control.
    watchdog_running: Arc<AtomicBool>,
}

impl Default for MacOsBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl MacOsBackend {
    #[must_use]
    pub fn new() -> Self {
        Self {
            focus: Arc::new(FocusTracker::new()),
            _poller: None,
            sink: Arc::new(Mutex::new(None)),
            watchdog_running: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl InputBackend for MacOsBackend {
    fn run(&mut self, sink: Box<dyn EventSink>) -> Result<(), BackendError> {
        let perms = check_permissions();
        if !perms.accessibility {
            return Err(BackendError::MissingPermission(
                "Accessibility (grant via System Settings > Privacy & Security > Accessibility)"
                    .into(),
            ));
        }
        if !perms.input_monitoring {
            return Err(BackendError::MissingPermission(
                "Input Monitoring (grant via System Settings > Privacy & Security > Input Monitoring)".into(),
            ));
        }

        *self.sink.lock() = Some(sink);
        self._poller = Some(spawn_poller(self.focus.clone()));

        let focus = self.focus.clone();
        let sink_ref = self.sink.clone();

        let event_types: [CGEventType; 3] = [
            CGEventType::KeyDown,
            CGEventType::KeyUp,
            CGEventType::FlagsChanged,
        ];

        // SAFETY: we own the CFRunLoop source and tap for the duration of
        // this function; see watchdog for re-enable semantics.
        let tap = CGEventTap::new(
            CGEventTapLocation::HID,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::Default,
            event_types.to_vec(),
            move |_proxy, etype, event| handle_event(etype, event, &focus, &sink_ref),
        )
        .map_err(|_| BackendError::TapInstall("CGEventTapCreate failed".into()))?;

        // Attach the tap to the current run loop.
        let current = CFRunLoop::get_current();
        // SAFETY: mach ports are valid for the lifetime of `tap` which is
        // moved into the run loop.
        unsafe {
            let loop_source = tap
                .mach_port
                .create_runloop_source(0)
                .map_err(|_| BackendError::TapInstall("create_runloop_source failed".into()))?;
            current.add_source(&loop_source, kCFRunLoopCommonModes);
        }
        tap.enable();

        info!("CGEventTap installed and enabled");

        // Spawn the watchdog. We pass the raw mach-port ref wrapped in a
        // Send-safe newtype; CGEventTapIsEnabled and CGEventTapEnable are
        // both thread-safe on their CFMachPortRef argument.
        self.watchdog_running.store(true, Ordering::Relaxed);
        let wd_flag = self.watchdog_running.clone();
        let port_raw = PortRef(
            tap.mach_port
                .as_concrete_TypeRef()
                .cast::<core::ffi::c_void>() as usize,
        );
        thread::Builder::new()
            .name("sledge-tap-watchdog".into())
            .spawn(move || watchdog(port_raw, wd_flag))
            .map_err(|e| BackendError::Other(format!("watchdog spawn failed: {e}")))?;

        // Keep the tap alive for the lifetime of this function.
        let _tap_alive = tap;

        // Run forever on the calling thread (expected to be the daemon's
        // main thread, which owns the CFRunLoop).
        CFRunLoop::run_current();

        // If we ever return, shut down the watchdog.
        self.watchdog_running.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn inject(&self, action: &Action) -> Result<(), BackendError> {
        match action {
            Action::SendKey { key, mods } => send_key(*key, *mods),
            Action::SetInputSource { id } => set_input_source(id),
        }
    }
}

fn watchdog(port: PortRef, running: Arc<AtomicBool>) {
    debug!("tap watchdog started");
    while running.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(500));
        let port_ptr = port.0 as *const core::ffi::c_void;
        // SAFETY: The mach port is owned by the main thread's CGEventTap
        // which outlives this watchdog. Both functions are thread-safe.
        let enabled = unsafe {
            unsafe extern "C" {
                fn CGEventTapIsEnabled(tap: *const core::ffi::c_void) -> bool;
            }
            CGEventTapIsEnabled(port_ptr)
        };
        if !enabled {
            warn!("CGEventTap was disabled; re-enabling");
            unsafe {
                unsafe extern "C" {
                    fn CGEventTapEnable(tap: *const core::ffi::c_void, enable: bool);
                }
                CGEventTapEnable(port_ptr, true);
            }
        }
    }
    debug!("tap watchdog stopped");
}

/// Raw pointer to a `CFMachPort` we know to outlive the watchdog thread.
/// Wrapped in a `usize` so it is `Send` and `Sync`.
#[derive(Clone, Copy)]
struct PortRef(usize);

// SAFETY: We hold a raw pointer to a CoreFoundation `CFMachPort` whose
// lifetime is bounded by the `CGEventTap` on the main thread. The functions
// we call through this pointer (`CGEventTapIsEnabled`, `CGEventTapEnable`)
// are documented thread-safe on the CFMachPortRef argument.
unsafe impl Send for PortRef {}
unsafe impl Sync for PortRef {}

#[allow(clippy::too_many_lines)]
fn handle_event(
    etype: CGEventType,
    event: &CGEvent,
    focus: &Arc<FocusTracker>,
    sink: &Arc<Mutex<Option<Box<dyn EventSink>>>>,
) -> Option<CGEvent> {
    // Tap-disabled events: re-enable happens in watchdog; we log here too.
    match etype {
        CGEventType::TapDisabledByTimeout => {
            warn!("tap disabled: timeout");
            return Some(event.clone());
        }
        CGEventType::TapDisabledByUserInput => {
            warn!("tap disabled: user input");
            return Some(event.clone());
        }
        _ => {}
    }

    // Self-event filter: if we posted this event, pass it through.
    let userdata = event.get_integer_value_field(EventField::EVENT_SOURCE_USER_DATA);
    if userdata == SLEDGE_EVENT_SOURCE_TAG {
        trace!("passing through self-event (sentinel matched)");
        return Some(event.clone());
    }

    // Translate.
    let Some(kevent) = translate(etype, event) else {
        return Some(event.clone());
    };

    let focused = focus.current();
    let verdict = {
        let mut guard = sink.lock();
        let Some(sink) = guard.as_mut() else {
            return Some(event.clone());
        };
        sink.on_event(kevent, focused.as_deref())
    };

    match verdict {
        BackendVerdict::Pass => Some(event.clone()),
        BackendVerdict::Swallow => None,
    }
}

fn translate(etype: CGEventType, event: &CGEvent) -> Option<KeyEvent> {
    let kc = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE) as u16;
    let code = from_cg_keycode(kc)?;
    let flags = event.get_flags();
    let mods = flags_to_mods(flags);

    let kind = match etype {
        CGEventType::KeyDown => EventKind::KeyDown,
        CGEventType::KeyUp => EventKind::KeyUp,
        CGEventType::FlagsChanged => EventKind::ModifiersChanged,
        _ => return None,
    };
    Some(KeyEvent { code, kind, mods })
}

fn flags_to_mods(flags: CGEventFlags) -> Modifiers {
    let mut m = Modifiers::empty();
    if flags.contains(CGEventFlags::CGEventFlagControl) {
        m |= Modifiers::CTRL;
    }
    if flags.contains(CGEventFlags::CGEventFlagShift) {
        m |= Modifiers::SHIFT;
    }
    if flags.contains(CGEventFlags::CGEventFlagAlternate) {
        m |= Modifiers::ALT;
    }
    if flags.contains(CGEventFlags::CGEventFlagCommand) {
        m |= Modifiers::CMD;
    }
    if flags.contains(CGEventFlags::CGEventFlagSecondaryFn) {
        m |= Modifiers::FN;
    }
    // Side-specific bits: CGEventFlags exposes per-side flags via the
    // NX_DEVICE* bits; we don't currently map them because core rule
    // matching is side-agnostic for modifier-set matching and the tap FSM
    // identifies side via keycode rather than flags.
    let _ = KeyCode::LeftAlt; // silence unused-import warning if any
    m
}
