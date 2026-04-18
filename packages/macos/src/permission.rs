//! macOS permission preflight: Accessibility + Input Monitoring.

use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermissionStatus {
    pub accessibility: bool,
    pub input_monitoring: bool,
}

impl PermissionStatus {
    #[must_use]
    pub const fn ok(self) -> bool {
        self.accessibility && self.input_monitoring
    }
}

/// Check both permissions without prompting.
#[must_use]
pub fn check_permissions() -> PermissionStatus {
    PermissionStatus {
        accessibility: accessibility_trusted(false),
        input_monitoring: input_monitoring_granted(),
    }
}

/// Check Accessibility (`AXIsProcessTrustedWithOptions`). If `prompt` is
/// true, macOS may show the "enable Accessibility" system dialog.
pub fn accessibility_trusted(prompt: bool) -> bool {
    unsafe {
        let key = CFString::new("AXTrustedCheckOptionPrompt");
        let value = if prompt {
            CFBoolean::true_value()
        } else {
            CFBoolean::false_value()
        };
        let opts: CFDictionary<CFString, CFBoolean> =
            CFDictionary::from_CFType_pairs(&[(key, value)]);
        unsafe extern "C" {
            fn AXIsProcessTrustedWithOptions(options: *const core::ffi::c_void) -> bool;
        }
        AXIsProcessTrustedWithOptions(opts.as_concrete_TypeRef().cast())
    }
}

/// Check Input Monitoring via `IOHIDCheckAccess(kIOHIDRequestTypeListenEvent)`.
#[must_use]
pub fn input_monitoring_granted() -> bool {
    // Values from <IOKit/hid/IOHIDLib.h>:
    //   kIOHIDAccessTypeGranted         = 0
    //   kIOHIDAccessTypeDenied          = 1
    //   kIOHIDAccessTypeUnknown         = 2
    //   kIOHIDRequestTypeListenEvent    = 1
    const LISTEN_EVENT: u32 = 1;
    const GRANTED: u32 = 0;
    unsafe extern "C" {
        fn IOHIDCheckAccess(request_type: u32) -> u32;
    }
    // SAFETY: IOHIDCheckAccess is an FFI call with integer in/out and no
    // pointer arguments.
    unsafe { IOHIDCheckAccess(LISTEN_EVENT) == GRANTED }
}
