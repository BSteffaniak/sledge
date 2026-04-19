//! CGEventTap installation, watchdog, and event dispatch.
//!
//! The tap is installed at the HID level with head-insert placement so we
//! see events before any app does. Our callback:
//!
//! 1. Checks the event-source userdata for the self-event sentinel; if
//!    present, returns `Pass` immediately — we never re-process our
//!    own injected output.
//! 2. Handles `TapDisabledByTimeout` / `TapDisabledByUserInput` by
//!    re-enabling the tap in place.
//! 3. Maps the `CGEvent` to a normalized [`KeyEvent`].
//! 4. Calls the [`EventSink`] for a verdict.
//! 5. Applies the verdict: drops the event, passes it through, or queues
//!    a synthesized replacement to run on a worker thread.
//!
//! We create the tap via raw `CGEventTapCreate` FFI rather than the
//! `core-graphics` crate's `CGEventTap::new` wrapper, because the wrapper
//! does not support returning a NULL event from the callback (which is
//! how a CGEventTap signals "swallow this event" to the OS). Its `None`
//! return value is mapped to "pass through the original event unchanged,"
//! which silently breaks `Verdict::Swallow` and `Verdict::Replace`
//! (injection followed by original-event leak).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use core_foundation::base::{CFRelease, TCFType};
use core_foundation::runloop::{
    CFRunLoop, CFRunLoopRef, CFRunLoopSourceRef, kCFRunLoopCommonModes,
};
use core_graphics::event::{CGEvent, CGEventFlags, CGEventType, EventField};
use foreign_types::ForeignType;
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
        let initial = check_permissions();
        if !initial.accessibility || !initial.input_monitoring {
            warn!(
                accessibility = initial.accessibility,
                input_monitoring = initial.input_monitoring,
                "permissions missing; firing prompts and waiting"
            );
            // Fire the user-facing prompts exactly once. Both calls
            // return the current (unchanged) state synchronously; the
            // dialogs are dispatched asynchronously by the system. We
            // discard the returns because we only use them to decide
            // whether to fire in the first place.
            //
            // Crucially, we do NOT re-fire prompts on each polling
            // iteration below: TCC treats each call as a fresh prompt
            // request, and launchd's KeepAlive policy combined with a
            // fast exit-on-failure would produce a prompt storm. Firing
            // once-per-process and then waiting is the UX-correct
            // behaviour that keyboard tools like Karabiner and
            // Hammerspoon use.
            if !initial.accessibility {
                let _ = crate::permission::accessibility_trusted(true);
            }
            if !initial.input_monitoring {
                let _ = crate::permission::input_monitoring_request();
            }

            info!(
                "Waiting for Accessibility and Input Monitoring grants. \
                 Grant both in System Settings > Privacy & Security; \
                 the daemon will automatically proceed once granted."
            );

            // Poll until both permissions are granted. We sleep 2s
            // between checks so the daemon is essentially idle while
            // waiting. Every 30 iterations (~1 minute) we emit a DEBUG
            // line so the log shows the daemon is still alive and
            // waiting rather than silently stalled.
            let mut tick: u64 = 0;
            loop {
                std::thread::sleep(std::time::Duration::from_secs(2));
                tick = tick.saturating_add(1);
                let p = check_permissions();
                if p.accessibility && p.input_monitoring {
                    info!(
                        accessibility = p.accessibility,
                        input_monitoring = p.input_monitoring,
                        "permissions granted; installing event tap"
                    );
                    break;
                }
                if tick.is_multiple_of(30) {
                    debug!(
                        accessibility = p.accessibility,
                        input_monitoring = p.input_monitoring,
                        elapsed_secs = tick * 2,
                        "still waiting for permissions"
                    );
                }
            }
        }

        *self.sink.lock() = Some(sink);
        self._poller = Some(spawn_poller(self.focus.clone()));

        let focus = self.focus.clone();
        let sink_ref = self.sink.clone();

        // Event-type mask: KeyDown | KeyUp | FlagsChanged. The values are
        // the bit indices of the respective CGEventType variants; the mask
        // is `1 << variant` per Apple's documentation.
        let event_mask: u64 =
            (1u64 << 10/* KeyDown */) | (1u64 << 11/* KeyUp */) | (1u64 << 12/* FlagsChanged */);

        // Build the callback context. The Box is leaked into the tap and
        // owned by the CFMachPort's lifetime \u2014 it's freed when the
        // tap itself goes away. For a daemon this is process lifetime.
        let ctx = Box::new(CallbackCtx { focus, sink_ref });
        let ctx_ptr = Box::into_raw(ctx);

        // SAFETY: We pass a valid C function pointer and the raw boxed
        // context. `CGEventTapCreate` is a standard CF/CG call; we check
        // the return for NULL. The `ctx` pointer is valid for the lifetime
        // of the mach port we create.
        let port = unsafe {
            CGEventTapCreate(
                K_CG_HID_EVENT_TAP,
                K_CG_HEAD_INSERT_EVENT_TAP,
                K_CG_EVENT_TAP_OPTION_DEFAULT,
                event_mask,
                tap_trampoline,
                ctx_ptr.cast(),
            )
        };
        if port.is_null() {
            // Reclaim the leaked context so we don't leak it on failure.
            // SAFETY: ctx_ptr was freshly leaked above and was not consumed
            // by a successful tap install; safe to reconstitute.
            drop(unsafe { Box::from_raw(ctx_ptr) });
            return Err(BackendError::TapInstall("CGEventTapCreate failed".into()));
        }

        // Attach the tap to the current run loop.
        // SAFETY: `port` is valid until we release it; the returned run
        // loop source is owned by us. We add it to the current run loop
        // and release our local reference; the run loop retains what it
        // needs.
        let current = CFRunLoop::get_current();
        let current_ref = current.as_concrete_TypeRef();
        unsafe {
            let source = CFMachPortCreateRunLoopSource(core::ptr::null(), port, 0);
            if source.is_null() {
                CFRelease(port.cast());
                drop(Box::from_raw(ctx_ptr));
                return Err(BackendError::TapInstall(
                    "CFMachPortCreateRunLoopSource failed".into(),
                ));
            }
            CFRunLoopAddSource(current_ref, source, kCFRunLoopCommonModes.cast());
            CFRelease(source.cast());
            CGEventTapEnable(port, true);
        }

        info!("CGEventTap installed and enabled");

        // Spawn the watchdog. We pass the raw mach-port ref wrapped in a
        // Send-safe newtype; CGEventTapIsEnabled and CGEventTapEnable are
        // both thread-safe on their CFMachPortRef argument.
        self.watchdog_running.store(true, Ordering::Relaxed);
        let wd_flag = self.watchdog_running.clone();
        let port_raw = PortRef(port as usize);
        thread::Builder::new()
            .name("sledge-tap-watchdog".into())
            .spawn(move || watchdog(port_raw, wd_flag))
            .map_err(|e| BackendError::Other(format!("watchdog spawn failed: {e}")))?;

        // Run forever on the calling thread (expected to be the daemon's
        // main thread, which owns the CFRunLoop).
        CFRunLoop::run_current();

        // If we ever return, shut down the watchdog and release our refs.
        self.watchdog_running.store(false, Ordering::Relaxed);
        // SAFETY: `port` and `ctx_ptr` have remained valid while the run
        // loop was active. After `run_current` returns, we own them.
        unsafe {
            CFRelease(port.cast());
            drop(Box::from_raw(ctx_ptr));
        }
        Ok(())
    }

    fn inject(&self, action: &Action) -> Result<(), BackendError> {
        match action {
            Action::SendKey { key, mods } => send_key(*key, *mods),
            Action::SetInputSource { id } => set_input_source(id),
        }
    }
}

struct CallbackCtx {
    focus: Arc<FocusTracker>,
    sink_ref: Arc<Mutex<Option<Box<dyn EventSink>>>>,
}

/// Trampoline called by `CGEventTapCreate`.
///
/// The return value semantics are: returning the event pointer passes the
/// event through; returning NULL swallows it. We decide based on the
/// [`handle_event`] result.
///
/// # Safety
///
/// Called by the OS from the CFRunLoop thread. `event_ref` is a valid
/// `CGEventRef`; `user_info` is the raw pointer to the `CallbackCtx`
/// we leaked when installing the tap.
unsafe extern "C" fn tap_trampoline(
    _proxy: *mut core::ffi::c_void,
    etype: u32,
    event_ref: *const core::ffi::c_void,
    user_info: *mut core::ffi::c_void,
) -> *const core::ffi::c_void {
    // Translate the raw event-type integer into the Rust enum. Unknown
    // values (e.g. TapDisabledByTimeout / ByUserInput at 0xFFFFFFFE /
    // 0xFFFFFFFF) cannot be constructed via the Rust enum, so we handle
    // them explicitly before the conversion.
    const TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
    const TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;

    // SAFETY: `event_ref` is a borrowed CGEventRef owned by the caller.
    // `CGEvent::from_ptr` wraps it under get-rule semantics (no retain),
    // so we must not CFRelease it ourselves \u2014 the OS owns it.
    let event = unsafe { CGEvent::from_ptr(event_ref.cast_mut().cast()) };

    let ctx = unsafe { &*(user_info as *const CallbackCtx) };

    let typed = match etype {
        TAP_DISABLED_BY_TIMEOUT => {
            warn!("tap disabled: timeout");
            // Return the event pointer (re-enable is handled by watchdog).
            // ManuallyDrop so we don't CFRelease what we don't own.
            let _ = std::mem::ManuallyDrop::new(event);
            return event_ref;
        }
        TAP_DISABLED_BY_USER_INPUT => {
            warn!("tap disabled: user input");
            let _ = std::mem::ManuallyDrop::new(event);
            return event_ref;
        }
        10 => CGEventType::KeyDown,
        11 => CGEventType::KeyUp,
        12 => CGEventType::FlagsChanged,
        _ => {
            // Unknown type we didn't subscribe to; pass through.
            let _ = std::mem::ManuallyDrop::new(event);
            return event_ref;
        }
    };

    let verdict = handle_event(typed, &event, &ctx.focus, &ctx.sink_ref);
    // Don't drop the CGEvent wrapper: the CGEventRef is owned by the
    // caller (the OS), not by us.
    let _ = std::mem::ManuallyDrop::new(event);

    match verdict {
        TapVerdict::Pass => event_ref,
        TapVerdict::Swallow => core::ptr::null(),
    }
}

enum TapVerdict {
    Pass,
    Swallow,
}

// -- Raw CF/CG FFI ------------------------------------------------------------

const K_CG_HID_EVENT_TAP: u32 = 0;
const K_CG_HEAD_INSERT_EVENT_TAP: u32 = 0;
const K_CG_EVENT_TAP_OPTION_DEFAULT: u32 = 0;

type CGEventTapCallBack = unsafe extern "C" fn(
    proxy: *mut core::ffi::c_void,
    etype: u32,
    event: *const core::ffi::c_void,
    user_info: *mut core::ffi::c_void,
) -> *const core::ffi::c_void;

unsafe extern "C" {
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: CGEventTapCallBack,
        user_info: *mut core::ffi::c_void,
    ) -> *mut core::ffi::c_void;

    fn CGEventTapEnable(tap: *mut core::ffi::c_void, enable: bool);

    fn CGEventTapIsEnabled(tap: *mut core::ffi::c_void) -> bool;

    fn CFMachPortCreateRunLoopSource(
        allocator: *const core::ffi::c_void,
        port: *mut core::ffi::c_void,
        order: isize,
    ) -> CFRunLoopSourceRef;

    fn CFRunLoopAddSource(
        rl: CFRunLoopRef,
        source: CFRunLoopSourceRef,
        mode: *const core::ffi::c_void,
    );
}

fn watchdog(port: PortRef, running: Arc<AtomicBool>) {
    debug!("tap watchdog started");
    while running.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(500));
        let port_ptr = port.0 as *mut core::ffi::c_void;
        // SAFETY: The mach port is owned by the main thread's CGEventTap
        // which outlives this watchdog. Both functions are thread-safe.
        let enabled = unsafe { CGEventTapIsEnabled(port_ptr) };
        if !enabled {
            warn!("CGEventTap was disabled; re-enabling");
            unsafe {
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
) -> TapVerdict {
    // Tap-disabled events are handled by the trampoline (via raw etype
    // matching on `TAP_DISABLED_BY_*`), not here, because the typed
    // `CGEventType` enum doesn't include them on all crate versions.
    match etype {
        CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput => {
            return TapVerdict::Pass;
        }
        _ => {}
    }

    // Self-event filter: if we posted this event, pass it through.
    let userdata = event.get_integer_value_field(EventField::EVENT_SOURCE_USER_DATA);
    if userdata == SLEDGE_EVENT_SOURCE_TAG {
        trace!("passing through self-event (sentinel matched)");
        return TapVerdict::Pass;
    }

    // Translate.
    let Some(kevent) = translate(etype, event) else {
        return TapVerdict::Pass;
    };

    let focused = focus.current();
    let verdict = {
        let mut guard = sink.lock();
        let Some(sink) = guard.as_mut() else {
            return TapVerdict::Pass;
        };
        sink.on_event(kevent, focused.as_deref())
    };

    match verdict {
        BackendVerdict::Pass => TapVerdict::Pass,
        BackendVerdict::Swallow => TapVerdict::Swallow,
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
