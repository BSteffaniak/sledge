//! Text Input Source switching.

use core_foundation::base::{CFRelease, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::string::CFString;
use sledge_core::BackendError;

/// Switch the active input source to the given identifier
/// (e.g. `"com.apple.keylayout.US"`).
///
/// # Errors
///
/// - `BackendError::UnknownInputSource` if no input source matches the id.
/// - `BackendError::Inject` if the source exists but is not enabled, or
///   if `TISSelectInputSource` returns a non-zero OSStatus.
pub fn set_input_source(id: &str) -> Result<(), BackendError> {
    // SAFETY: All FFI calls below obey Core Foundation's Create/Get
    // conventions. We release anything we create.
    unsafe {
        let key = CFString::new("TISPropertyInputSourceID");
        let value = CFString::new(id);
        let props: CFDictionary<CFString, CFString> =
            CFDictionary::from_CFType_pairs(&[(key, value)]);
        let list_ref = TISCreateInputSourceList(props.as_concrete_TypeRef(), true);
        if list_ref.is_null() {
            return Err(BackendError::UnknownInputSource(id.to_string()));
        }

        let count = CFArrayGetCount(list_ref);
        if count <= 0 {
            CFRelease(list_ref.cast());
            return Err(BackendError::UnknownInputSource(id.to_string()));
        }

        let first = CFArrayGetValueAtIndex(list_ref, 0);
        if first.is_null() {
            CFRelease(list_ref.cast());
            return Err(BackendError::UnknownInputSource(id.to_string()));
        }

        // Check `kTISPropertyInputSourceIsEnabled` before attempting to
        // select. Apple's docs require the source to be enabled for
        // `TISSelectInputSource` to succeed; otherwise it returns
        // paramErr (-50), which is uninformative. Detecting the
        // not-enabled case up front lets us surface an actionable
        // message instead of an opaque OSStatus.
        let enabled_key = CFString::new("TISPropertyInputSourceIsEnabled");
        let enabled_ref =
            TISGetInputSourceProperty(first, enabled_key.as_concrete_TypeRef().cast());
        let enabled = if enabled_ref.is_null() {
            // Property not available (e.g. older macOS or non-keyboard
            // source). Assume enabled and let TISSelectInputSource
            // tell us otherwise via its return code.
            true
        } else {
            // `TISGetInputSourceProperty` is a Get-rule function:
            // the returned ref is borrowed, not retained. We must not
            // CFRelease it ourselves. `wrap_under_get_rule` correctly
            // models this ownership.
            bool::from(CFBoolean::wrap_under_get_rule(enabled_ref.cast()))
        };

        if !enabled {
            CFRelease(list_ref.cast());
            return Err(BackendError::Inject(format!(
                "input source '{id}' is installed but not enabled. \
                 Enable it via System Settings > Keyboard > Text Input > Input Sources."
            )));
        }

        let rc = TISSelectInputSource(first);
        CFRelease(list_ref.cast());

        if rc == 0 {
            Ok(())
        } else {
            Err(BackendError::Inject(format!(
                "TISSelectInputSource returned {rc} for '{id}'"
            )))
        }
    }
}

// -- Carbon / HIToolbox FFI ---------------------------------------------------

type TISInputSourceRef = *const core::ffi::c_void;
type CFArrayRef = *const core::ffi::c_void;
type CFIndex = isize;
type OSStatus = i32;

unsafe extern "C" {
    fn TISCreateInputSourceList(
        properties: CFDictionaryRef,
        include_all_installed: bool,
    ) -> CFArrayRef;
    fn TISSelectInputSource(input_source: TISInputSourceRef) -> OSStatus;
    fn TISGetInputSourceProperty(
        input_source: TISInputSourceRef,
        property_key: *const core::ffi::c_void,
    ) -> *const core::ffi::c_void;
    fn CFArrayGetCount(array: CFArrayRef) -> CFIndex;
    fn CFArrayGetValueAtIndex(array: CFArrayRef, idx: CFIndex) -> *const core::ffi::c_void;
}
