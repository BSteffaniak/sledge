//! Text Input Source switching.

use core_foundation::base::{CFRelease, TCFType};
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::string::CFString;
use sledge_core::BackendError;

/// Switch the active input source to the given identifier
/// (e.g. `"com.apple.keylayout.US"`).
///
/// # Errors
///
/// Returns `BackendError::UnknownInputSource` if no input source matches.
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

        let rc = TISSelectInputSource(first);
        CFRelease(list_ref.cast());

        if rc == 0 {
            Ok(())
        } else {
            Err(BackendError::Inject(format!(
                "TISSelectInputSource returned {rc}"
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
    fn CFArrayGetCount(array: CFArrayRef) -> CFIndex;
    fn CFArrayGetValueAtIndex(array: CFArrayRef, idx: CFIndex) -> *const core::ffi::c_void;
}
