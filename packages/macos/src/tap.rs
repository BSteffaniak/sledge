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
    Action, BackendError, BackendVerdict, EventKind, EventSink, InputBackend, KeyEvent, Modifiers,
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

    /// A cheap `Arc` clone of the focus tracker. The caller can read the
    /// current focused-app bundle id via [`FocusTracker::current`]. The
    /// tracker stays live only while `run()` is active; when `run()`
    /// returns, the internal poller is dropped and `current()` will return
    /// the last observed value (typically `None` after a fresh restart).
    #[must_use]
    pub fn focus_tracker(&self) -> Arc<FocusTracker> {
        self.focus.clone()
    }
}

impl InputBackend for MacOsBackend {
    fn run(&mut self, sink: Box<dyn EventSink>) -> Result<(), BackendError> {
        let perms = check_permissions();
        if !perms.accessibility || !perms.input_monitoring {
            warn!(
                accessibility = perms.accessibility,
                input_monitoring = perms.input_monitoring,
                "permissions missing; firing prompts"
            );
            // Fire the user-facing prompts. Both calls return the current
            // (unchanged) state synchronously; the prompts themselves are
            // handled asynchronously by the system. We discard the returns
            // because we want to short-circuit with a consistent error
            // regardless, leaving the user to grant + relaunch.
            if !perms.accessibility {
                let _ = crate::permission::accessibility_trusted(true);
            }
            if !perms.input_monitoring {
                let _ = crate::permission::input_monitoring_request();
            }
            return Err(BackendError::MissingPermission(format!(
                "accessibility={} input_monitoring={}. Grant both in \
                 System Settings > Privacy & Security, then relaunch sledge.",
                perms.accessibility, perms.input_monitoring
            )));
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

    // Device-independent (generic) bits from `CGEventFlags`. One bit per
    // modifier family, set whenever _any_ side of that family is held.
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

    // Device-dependent (side-specific) bits. These are `NX_DEVICE*KEYMASK`
    // values from `<IOKit/hidsystem/IOLLEvent.h>`; they have been stable
    // ABI since OS X 10.0 and are what `kCGKeyboardEventKeyboardType`
    // events carry alongside the generic bits. Setting them lets the tap
    // FSM distinguish LeftAlt from RightAlt etc., which is required for
    // per-side tap triggers (e.g. "triple-tap right Option").
    //
    // The values below were verified against actual `CGEventFlags.bits()`
    // output on Apple Silicon (macOS 14+) for single-key presses of each
    // modifier. See the `flags_to_mods_*` tests below for the bit patterns.
    const NX_L_CTRL: u64 = 0x0000_0001;
    const NX_L_SHIFT: u64 = 0x0000_0002;
    const NX_R_SHIFT: u64 = 0x0000_0004;
    const NX_L_CMD: u64 = 0x0000_0008;
    const NX_R_CMD: u64 = 0x0000_0010;
    const NX_L_ALT: u64 = 0x0000_0020;
    const NX_R_ALT: u64 = 0x0000_0040;
    const NX_R_CTRL: u64 = 0x0000_2000;

    let raw = flags.bits();
    if raw & NX_L_CTRL != 0 {
        m |= Modifiers::LEFT_CTRL;
    }
    if raw & NX_R_CTRL != 0 {
        m |= Modifiers::RIGHT_CTRL;
    }
    if raw & NX_L_SHIFT != 0 {
        m |= Modifiers::LEFT_SHIFT;
    }
    if raw & NX_R_SHIFT != 0 {
        m |= Modifiers::RIGHT_SHIFT;
    }
    if raw & NX_L_ALT != 0 {
        m |= Modifiers::LEFT_ALT;
    }
    if raw & NX_R_ALT != 0 {
        m |= Modifiers::RIGHT_ALT;
    }
    if raw & NX_L_CMD != 0 {
        m |= Modifiers::LEFT_CMD;
    }
    if raw & NX_R_CMD != 0 {
        m |= Modifiers::RIGHT_CMD;
    }

    m
}

#[cfg(test)]
mod tests {
    use super::*;

    // Raw flag patterns below were captured from live `CGEventFlags.bits()`
    // values on Apple Silicon (macOS 14) for single-modifier keypresses.
    // The `0x100` bit is `CGEventFlagNonCoalesced` \u2014 harmless metadata
    // that is always set on keyboard events.

    fn f(raw: u64) -> CGEventFlags {
        CGEventFlags::from_bits_retain(raw)
    }

    #[test]
    fn flags_to_mods_empty() {
        assert_eq!(flags_to_mods(CGEventFlags::empty()), Modifiers::empty());
    }

    #[test]
    fn flags_to_mods_left_alt() {
        // Left Option down: CGEventFlagAlternate | NX_L_ALT | NonCoalesced
        let m = flags_to_mods(f(0x80120));
        assert!(m.contains(Modifiers::ALT));
        assert!(m.contains(Modifiers::LEFT_ALT));
        assert!(!m.contains(Modifiers::RIGHT_ALT));
    }

    #[test]
    fn flags_to_mods_right_alt() {
        // Right Option down: CGEventFlagAlternate | NX_R_ALT | NonCoalesced
        let m = flags_to_mods(f(0x80140));
        assert!(m.contains(Modifiers::ALT));
        assert!(m.contains(Modifiers::RIGHT_ALT));
        assert!(!m.contains(Modifiers::LEFT_ALT));
    }

    #[test]
    fn flags_to_mods_left_shift() {
        // Left Shift down: CGEventFlagShift | NX_L_SHIFT | NonCoalesced
        let m = flags_to_mods(f(0x20102));
        assert!(m.contains(Modifiers::SHIFT));
        assert!(m.contains(Modifiers::LEFT_SHIFT));
        assert!(!m.contains(Modifiers::RIGHT_SHIFT));
    }

    #[test]
    fn flags_to_mods_right_shift() {
        // Right Shift down: CGEventFlagShift | NX_R_SHIFT | NonCoalesced
        let m = flags_to_mods(f(0x20104));
        assert!(m.contains(Modifiers::SHIFT));
        assert!(m.contains(Modifiers::RIGHT_SHIFT));
        assert!(!m.contains(Modifiers::LEFT_SHIFT));
    }

    #[test]
    fn flags_to_mods_left_ctrl() {
        // Left Ctrl down: CGEventFlagControl | NX_L_CTRL | NonCoalesced
        let m = flags_to_mods(f(0x40101));
        assert!(m.contains(Modifiers::CTRL));
        assert!(m.contains(Modifiers::LEFT_CTRL));
        assert!(!m.contains(Modifiers::RIGHT_CTRL));
    }

    #[test]
    fn flags_to_mods_right_ctrl() {
        // Right Ctrl down: CGEventFlagControl | NX_R_CTRL | NonCoalesced
        // (Pattern synthesized; most Apple keyboards lack a right Ctrl key.
        // The `NX_R_CTRL` bit value is documented in IOLLEvent.h.)
        let m = flags_to_mods(f(0x42100));
        assert!(m.contains(Modifiers::CTRL));
        assert!(m.contains(Modifiers::RIGHT_CTRL));
        assert!(!m.contains(Modifiers::LEFT_CTRL));
    }

    #[test]
    fn flags_to_mods_left_cmd() {
        // Left Cmd down: CGEventFlagCommand | NX_L_CMD | NonCoalesced
        let m = flags_to_mods(f(0x100108));
        assert!(m.contains(Modifiers::CMD));
        assert!(m.contains(Modifiers::LEFT_CMD));
        assert!(!m.contains(Modifiers::RIGHT_CMD));
    }

    #[test]
    fn flags_to_mods_right_cmd() {
        // Right Cmd down: CGEventFlagCommand | NX_R_CMD | NonCoalesced
        let m = flags_to_mods(f(0x100110));
        assert!(m.contains(Modifiers::CMD));
        assert!(m.contains(Modifiers::RIGHT_CMD));
        assert!(!m.contains(Modifiers::LEFT_CMD));
    }

    #[test]
    fn flags_to_mods_both_shifts() {
        // Both shifts down: generic Shift + both side bits.
        let m = flags_to_mods(f(0x20106));
        assert!(m.contains(Modifiers::SHIFT));
        assert!(m.contains(Modifiers::LEFT_SHIFT));
        assert!(m.contains(Modifiers::RIGHT_SHIFT));
    }

    #[test]
    fn flags_to_mods_cmd_shift_combo() {
        // Cmd+Shift (left side of each): Command + Shift + NX_L_CMD + NX_L_SHIFT
        let m = flags_to_mods(f(0x12010A));
        assert!(m.contains(Modifiers::CMD));
        assert!(m.contains(Modifiers::SHIFT));
        assert!(m.contains(Modifiers::LEFT_CMD));
        assert!(m.contains(Modifiers::LEFT_SHIFT));
        assert!(!m.contains(Modifiers::ALT));
        assert!(!m.contains(Modifiers::CTRL));
    }

    #[test]
    fn flags_to_mods_release_is_empty() {
        // Release event: only NonCoalesced set; no modifier bits.
        assert_eq!(flags_to_mods(f(0x100)), Modifiers::empty());
    }
}
