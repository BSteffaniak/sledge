//! Event injection with self-event-filter tagging.
//!
//! All synthesized events are posted through a private [`CGEventSource`]
//! whose `sourceUserData` is set to [`SLEDGE_EVENT_SOURCE_TAG`]. The tap
//! callback checks this field on every event and passes through anything
//! carrying the tag, so the daemon never re-processes its own output.

use core_graphics::event::{CGEvent, CGEventTapLocation, EventField};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use sledge_core::{BackendError, KeyCode, Modifiers};

use crate::keycode::to_cg_keycode;

/// Private sentinel value stamped into injected events' source-userdata
/// field. Anything carrying this value must be passed through by the tap.
pub const SLEDGE_EVENT_SOURCE_TAG: i64 = 0x0005_1ED6_E001;

/// Synthesize a down+up keystroke for the given key + modifiers.
///
/// # Errors
///
/// Returns `BackendError::Inject` if CGEvent construction fails.
pub fn send_key(key: KeyCode, mods: Modifiers) -> Result<(), BackendError> {
    let source = CGEventSource::new(CGEventSourceStateID::Private)
        .map_err(|_| BackendError::Inject("CGEventSourceCreate failed".into()))?;

    let cg_key = to_cg_keycode(key);
    if cg_key == 0 {
        return Err(BackendError::Inject(format!(
            "no CGKeyCode mapping for {key:?}"
        )));
    }

    let flags = mods_to_cg_flags(mods);

    let down = CGEvent::new_keyboard_event(source.clone(), cg_key, true)
        .map_err(|_| BackendError::Inject("CGEventCreateKeyboardEvent (down) failed".into()))?;
    down.set_flags(flags);
    stamp_userdata(&down);
    down.post(CGEventTapLocation::HID);

    let up = CGEvent::new_keyboard_event(source, cg_key, false)
        .map_err(|_| BackendError::Inject("CGEventCreateKeyboardEvent (up) failed".into()))?;
    up.set_flags(flags);
    stamp_userdata(&up);
    up.post(CGEventTapLocation::HID);

    Ok(())
}

fn mods_to_cg_flags(mods: Modifiers) -> core_graphics::event::CGEventFlags {
    use core_graphics::event::CGEventFlags;
    let mut f = CGEventFlags::CGEventFlagNull;
    if mods.contains(Modifiers::CTRL) {
        f |= CGEventFlags::CGEventFlagControl;
    }
    if mods.contains(Modifiers::SHIFT) {
        f |= CGEventFlags::CGEventFlagShift;
    }
    if mods.contains(Modifiers::ALT) {
        f |= CGEventFlags::CGEventFlagAlternate;
    }
    if mods.contains(Modifiers::CMD) {
        f |= CGEventFlags::CGEventFlagCommand;
    }
    if mods.contains(Modifiers::FN) {
        f |= CGEventFlags::CGEventFlagSecondaryFn;
    }
    f
}

fn stamp_userdata(event: &CGEvent) {
    event.set_integer_value_field(EventField::EVENT_SOURCE_USER_DATA, SLEDGE_EVENT_SOURCE_TAG);
}
