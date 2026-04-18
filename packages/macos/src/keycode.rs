//! macOS HID-keycode <-> `sledge_core::KeyCode` mapping.
//!
//! Uses Carbon "Virtual Key Code" values from `Events.h` (HIToolbox). These
//! are the same values reported by `CGEventGetIntegerValueField(_,
//! kCGKeyboardEventKeycode)`.

use sledge_core::KeyCode;

/// Convert a macOS virtual key code into a `KeyCode`.
///
/// Returns `None` for key codes we don't model.
#[must_use]
#[allow(clippy::too_many_lines)]
pub const fn from_cg_keycode(kc: u16) -> Option<KeyCode> {
    use KeyCode as K;
    Some(match kc {
        // Letters (kVK_ANSI_*)
        0x00 => K::KeyA,
        0x0B => K::KeyB,
        0x08 => K::KeyC,
        0x02 => K::KeyD,
        0x0E => K::KeyE,
        0x03 => K::KeyF,
        0x05 => K::KeyG,
        0x04 => K::KeyH,
        0x22 => K::KeyI,
        0x26 => K::KeyJ,
        0x28 => K::KeyK,
        0x25 => K::KeyL,
        0x2E => K::KeyM,
        0x2D => K::KeyN,
        0x1F => K::KeyO,
        0x23 => K::KeyP,
        0x0C => K::KeyQ,
        0x0F => K::KeyR,
        0x01 => K::KeyS,
        0x11 => K::KeyT,
        0x20 => K::KeyU,
        0x09 => K::KeyV,
        0x0D => K::KeyW,
        0x07 => K::KeyX,
        0x10 => K::KeyY,
        0x06 => K::KeyZ,
        // Digits
        0x1D => K::Digit0,
        0x12 => K::Digit1,
        0x13 => K::Digit2,
        0x14 => K::Digit3,
        0x15 => K::Digit4,
        0x17 => K::Digit5,
        0x16 => K::Digit6,
        0x1A => K::Digit7,
        0x1C => K::Digit8,
        0x19 => K::Digit9,
        // Function keys
        0x7A => K::F1,
        0x78 => K::F2,
        0x63 => K::F3,
        0x76 => K::F4,
        0x60 => K::F5,
        0x61 => K::F6,
        0x62 => K::F7,
        0x64 => K::F8,
        0x65 => K::F9,
        0x6D => K::F10,
        0x67 => K::F11,
        0x6F => K::F12,
        0x69 => K::F13,
        0x6B => K::F14,
        0x71 => K::F15,
        0x6A => K::F16,
        0x40 => K::F17,
        0x4F => K::F18,
        0x50 => K::F19,
        0x5A => K::F20,
        // Arrows
        0x7E => K::ArrowUp,
        0x7D => K::ArrowDown,
        0x7B => K::ArrowLeft,
        0x7C => K::ArrowRight,
        // Modifiers
        0x38 => K::LeftShift,
        0x3C => K::RightShift,
        0x3B => K::LeftCtrl,
        0x3E => K::RightCtrl,
        0x3A => K::LeftAlt,
        0x3D => K::RightAlt,
        0x37 => K::LeftCmd,
        0x36 => K::RightCmd,
        0x3F => K::Fn,
        0x39 => K::CapsLock,
        // Whitespace / editing
        0x24 => K::Return,
        0x30 => K::Tab,
        0x31 => K::Space,
        0x33 => K::Backspace,
        0x75 => K::Delete,
        0x35 => K::Escape,
        // Punctuation
        0x29 => K::Semicolon,
        0x27 => K::Quote,
        0x2B => K::Comma,
        0x2F => K::Period,
        0x2C => K::Slash,
        0x2A => K::Backslash,
        0x32 => K::Backquote,
        0x1B => K::Minus,
        0x18 => K::Equal,
        0x21 => K::LeftBracket,
        0x1E => K::RightBracket,
        // Navigation
        0x73 => K::Home,
        0x77 => K::End,
        0x74 => K::PageUp,
        0x79 => K::PageDown,
        0x72 => K::Insert,
        _ => return None,
    })
}

/// Convert a `KeyCode` to the matching macOS virtual key code.
#[must_use]
#[allow(clippy::too_many_lines)]
pub const fn to_cg_keycode(code: KeyCode) -> u16 {
    use KeyCode as K;
    match code {
        K::KeyA => 0x00,
        K::KeyB => 0x0B,
        K::KeyC => 0x08,
        K::KeyD => 0x02,
        K::KeyE => 0x0E,
        K::KeyF => 0x03,
        K::KeyG => 0x05,
        K::KeyH => 0x04,
        K::KeyI => 0x22,
        K::KeyJ => 0x26,
        K::KeyK => 0x28,
        K::KeyL => 0x25,
        K::KeyM => 0x2E,
        K::KeyN => 0x2D,
        K::KeyO => 0x1F,
        K::KeyP => 0x23,
        K::KeyQ => 0x0C,
        K::KeyR => 0x0F,
        K::KeyS => 0x01,
        K::KeyT => 0x11,
        K::KeyU => 0x20,
        K::KeyV => 0x09,
        K::KeyW => 0x0D,
        K::KeyX => 0x07,
        K::KeyY => 0x10,
        K::KeyZ => 0x06,
        K::Digit0 => 0x1D,
        K::Digit1 => 0x12,
        K::Digit2 => 0x13,
        K::Digit3 => 0x14,
        K::Digit4 => 0x15,
        K::Digit5 => 0x17,
        K::Digit6 => 0x16,
        K::Digit7 => 0x1A,
        K::Digit8 => 0x1C,
        K::Digit9 => 0x19,
        K::F1 => 0x7A,
        K::F2 => 0x78,
        K::F3 => 0x63,
        K::F4 => 0x76,
        K::F5 => 0x60,
        K::F6 => 0x61,
        K::F7 => 0x62,
        K::F8 => 0x64,
        K::F9 => 0x65,
        K::F10 => 0x6D,
        K::F11 => 0x67,
        K::F12 => 0x6F,
        K::F13 => 0x69,
        K::F14 => 0x6B,
        K::F15 => 0x71,
        K::F16 => 0x6A,
        K::F17 => 0x40,
        K::F18 => 0x4F,
        K::F19 => 0x50,
        K::F20 => 0x5A,
        K::F21 | K::F22 | K::F23 | K::F24 => 0,
        K::ArrowUp => 0x7E,
        K::ArrowDown => 0x7D,
        K::ArrowLeft => 0x7B,
        K::ArrowRight => 0x7C,
        K::LeftShift => 0x38,
        K::RightShift => 0x3C,
        K::LeftCtrl => 0x3B,
        K::RightCtrl => 0x3E,
        K::LeftAlt => 0x3A,
        K::RightAlt => 0x3D,
        K::LeftCmd => 0x37,
        K::RightCmd => 0x36,
        K::Fn => 0x3F,
        K::CapsLock => 0x39,
        K::Return => 0x24,
        K::Tab => 0x30,
        K::Space => 0x31,
        K::Backspace => 0x33,
        K::Delete => 0x75,
        K::Escape => 0x35,
        K::Semicolon => 0x29,
        K::Quote => 0x27,
        K::Comma => 0x2B,
        K::Period => 0x2F,
        K::Slash => 0x2C,
        K::Backslash => 0x2A,
        K::Backquote => 0x32,
        K::Minus => 0x1B,
        K::Equal => 0x18,
        K::LeftBracket => 0x21,
        K::RightBracket => 0x1E,
        K::Home => 0x73,
        K::End => 0x77,
        K::PageUp => 0x74,
        K::PageDown => 0x79,
        K::Insert => 0x72,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_common_keys() {
        for k in [
            KeyCode::KeyA,
            KeyCode::Return,
            KeyCode::RightAlt,
            KeyCode::Semicolon,
            KeyCode::Digit1,
            KeyCode::F1,
            KeyCode::ArrowUp,
        ] {
            let cg = to_cg_keycode(k);
            assert_eq!(from_cg_keycode(cg), Some(k), "roundtrip failed for {k:?}");
        }
    }
}
