//! Normalized key event model.
//!
//! All platform backends translate their native key codes into this enum on
//! the way in, and translate back on the way out. Names and values follow the
//! USB HID Keyboard/Keypad Usage Page (0x07) where possible.

/// A single key-down / key-up / flags-changed event delivered by a backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub kind: EventKind,
    pub mods: Modifiers,
}

/// Event kinds we care about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    /// Regular key-down.
    KeyDown,
    /// Regular key-up.
    KeyUp,
    /// A modifier's state changed (macOS `flagsChanged`; generic "modifier
    /// changed" on other platforms). `code` indicates which modifier; `mods`
    /// is the new aggregate state after the change.
    ModifiersChanged,
}

bitflags::bitflags! {
    /// Modifier state. Side-specific bits (`LEFT_*`, `RIGHT_*`) are always
    /// set in lockstep with their generic (`CTRL`, `SHIFT`, ...) counterparts:
    /// if `LEFT_CTRL` is set then `CTRL` is also set.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct Modifiers: u32 {
        const CTRL         = 1 << 0;
        const SHIFT        = 1 << 1;
        const ALT          = 1 << 2;
        const CMD          = 1 << 3;
        const FN           = 1 << 4;

        const LEFT_CTRL    = 1 << 8;
        const RIGHT_CTRL   = 1 << 9;
        const LEFT_SHIFT   = 1 << 10;
        const RIGHT_SHIFT  = 1 << 11;
        const LEFT_ALT     = 1 << 12;
        const RIGHT_ALT    = 1 << 13;
        const LEFT_CMD     = 1 << 14;
        const RIGHT_CMD    = 1 << 15;
    }
}

impl Modifiers {
    /// Returns true if `self` contains every required-side modifier in
    /// `required`. Generic bits in `required` match either side; side-specific
    /// bits require that specific side.
    #[must_use]
    pub const fn matches(self, required: Self) -> bool {
        // Every bit set in `required` must also be set in `self`.
        self.contains(required)
    }

    /// True if any modifier bits are set.
    #[must_use]
    pub const fn any(self) -> bool {
        !self.is_empty()
    }
}

/// Normalized key codes. The discriminant values are stable across releases.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyCode {
    // Letters
    KeyA,
    KeyB,
    KeyC,
    KeyD,
    KeyE,
    KeyF,
    KeyG,
    KeyH,
    KeyI,
    KeyJ,
    KeyK,
    KeyL,
    KeyM,
    KeyN,
    KeyO,
    KeyP,
    KeyQ,
    KeyR,
    KeyS,
    KeyT,
    KeyU,
    KeyV,
    KeyW,
    KeyX,
    KeyY,
    KeyZ,
    // Digit row
    Digit0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
    // Function keys
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    F13,
    F14,
    F15,
    F16,
    F17,
    F18,
    F19,
    F20,
    F21,
    F22,
    F23,
    F24,
    // Arrows
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    // Modifiers (side-specific)
    LeftShift,
    RightShift,
    LeftCtrl,
    RightCtrl,
    LeftAlt,
    RightAlt,
    LeftCmd,
    RightCmd,
    Fn,
    CapsLock,
    // Whitespace / editing
    Return,
    Tab,
    Space,
    Backspace,
    Delete,
    Escape,
    // Punctuation
    Semicolon,
    Quote,
    Comma,
    Period,
    Slash,
    Backslash,
    Backquote,
    Minus,
    Equal,
    LeftBracket,
    RightBracket,
    // Navigation
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
}

impl KeyCode {
    /// Returns `true` if this code represents a modifier key (shift, ctrl,
    /// alt, cmd, fn, or caps-lock).
    #[must_use]
    pub const fn is_modifier(self) -> bool {
        matches!(
            self,
            Self::LeftShift
                | Self::RightShift
                | Self::LeftCtrl
                | Self::RightCtrl
                | Self::LeftAlt
                | Self::RightAlt
                | Self::LeftCmd
                | Self::RightCmd
                | Self::Fn
                | Self::CapsLock
        )
    }

    /// For a modifier key code, return the `Modifiers` bit that represents
    /// the _specific side_ being pressed. Returns `Modifiers::empty()` for
    /// non-modifier keys.
    #[must_use]
    pub const fn modifier_bit(self) -> Modifiers {
        match self {
            Self::LeftShift => Modifiers::LEFT_SHIFT,
            Self::RightShift => Modifiers::RIGHT_SHIFT,
            Self::LeftCtrl => Modifiers::LEFT_CTRL,
            Self::RightCtrl => Modifiers::RIGHT_CTRL,
            Self::LeftAlt => Modifiers::LEFT_ALT,
            Self::RightAlt => Modifiers::RIGHT_ALT,
            Self::LeftCmd => Modifiers::LEFT_CMD,
            Self::RightCmd => Modifiers::RIGHT_CMD,
            _ => Modifiers::empty(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifier_bit_is_empty_for_regular_keys() {
        assert_eq!(KeyCode::KeyA.modifier_bit(), Modifiers::empty());
        assert_eq!(KeyCode::Digit1.modifier_bit(), Modifiers::empty());
        assert_eq!(KeyCode::Return.modifier_bit(), Modifiers::empty());
    }

    #[test]
    fn modifier_bit_is_set_for_modifier_keys() {
        assert_eq!(KeyCode::LeftAlt.modifier_bit(), Modifiers::LEFT_ALT);
        assert_eq!(KeyCode::RightCmd.modifier_bit(), Modifiers::RIGHT_CMD);
    }

    #[test]
    fn modifiers_matches_requires_exact_side_when_set() {
        let state = Modifiers::CTRL | Modifiers::LEFT_CTRL;
        let want_any = Modifiers::CTRL;
        let want_left = Modifiers::CTRL | Modifiers::LEFT_CTRL;
        let want_right = Modifiers::CTRL | Modifiers::RIGHT_CTRL;

        assert!(state.matches(want_any));
        assert!(state.matches(want_left));
        assert!(!state.matches(want_right));
    }
}
