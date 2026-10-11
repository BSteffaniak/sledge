//! Linux evdev key mapping (physical keys, not layout-dependent characters).
use sledge_core::{KeyCode, Modifiers};

const KEYS: &[(u16, KeyCode)] = &[
    (1, KeyCode::Escape),
    (2, KeyCode::Digit1),
    (3, KeyCode::Digit2),
    (4, KeyCode::Digit3),
    (5, KeyCode::Digit4),
    (6, KeyCode::Digit5),
    (7, KeyCode::Digit6),
    (8, KeyCode::Digit7),
    (9, KeyCode::Digit8),
    (10, KeyCode::Digit9),
    (11, KeyCode::Digit0),
    (12, KeyCode::Minus),
    (13, KeyCode::Equal),
    (14, KeyCode::Backspace),
    (15, KeyCode::Tab),
    (16, KeyCode::KeyQ),
    (17, KeyCode::KeyW),
    (18, KeyCode::KeyE),
    (19, KeyCode::KeyR),
    (20, KeyCode::KeyT),
    (21, KeyCode::KeyY),
    (22, KeyCode::KeyU),
    (23, KeyCode::KeyI),
    (24, KeyCode::KeyO),
    (25, KeyCode::KeyP),
    (26, KeyCode::LeftBracket),
    (27, KeyCode::RightBracket),
    (28, KeyCode::Return),
    (29, KeyCode::LeftCtrl),
    (30, KeyCode::KeyA),
    (31, KeyCode::KeyS),
    (32, KeyCode::KeyD),
    (33, KeyCode::KeyF),
    (34, KeyCode::KeyG),
    (35, KeyCode::KeyH),
    (36, KeyCode::KeyJ),
    (37, KeyCode::KeyK),
    (38, KeyCode::KeyL),
    (39, KeyCode::Semicolon),
    (40, KeyCode::Quote),
    (41, KeyCode::Backquote),
    (42, KeyCode::LeftShift),
    (43, KeyCode::Backslash),
    (44, KeyCode::KeyZ),
    (45, KeyCode::KeyX),
    (46, KeyCode::KeyC),
    (47, KeyCode::KeyV),
    (48, KeyCode::KeyB),
    (49, KeyCode::KeyN),
    (50, KeyCode::KeyM),
    (51, KeyCode::Comma),
    (52, KeyCode::Period),
    (53, KeyCode::Slash),
    (54, KeyCode::RightShift),
    (56, KeyCode::LeftAlt),
    (57, KeyCode::Space),
    (58, KeyCode::CapsLock),
    (59, KeyCode::F1),
    (60, KeyCode::F2),
    (61, KeyCode::F3),
    (62, KeyCode::F4),
    (63, KeyCode::F5),
    (64, KeyCode::F6),
    (65, KeyCode::F7),
    (66, KeyCode::F8),
    (67, KeyCode::F9),
    (68, KeyCode::F10),
    (87, KeyCode::F11),
    (88, KeyCode::F12),
    (97, KeyCode::RightCtrl),
    (100, KeyCode::RightAlt),
    (102, KeyCode::Home),
    (103, KeyCode::ArrowUp),
    (104, KeyCode::PageUp),
    (105, KeyCode::ArrowLeft),
    (106, KeyCode::ArrowRight),
    (107, KeyCode::End),
    (108, KeyCode::ArrowDown),
    (109, KeyCode::PageDown),
    (110, KeyCode::Insert),
    (111, KeyCode::Delete),
    (125, KeyCode::LeftCmd),
    (126, KeyCode::RightCmd),
    (183, KeyCode::F13),
    (184, KeyCode::F14),
    (185, KeyCode::F15),
    (186, KeyCode::F16),
    (187, KeyCode::F17),
    (188, KeyCode::F18),
    (189, KeyCode::F19),
    (190, KeyCode::F20),
    (191, KeyCode::F21),
    (192, KeyCode::F22),
    (193, KeyCode::F23),
    (194, KeyCode::F24),
    (464, KeyCode::Fn),
];

#[must_use]
pub fn from_evdev(code: u16) -> Option<KeyCode> {
    KEYS.iter()
        .find_map(|&(raw, key)| (raw == code).then_some(key))
}
#[must_use]
pub fn to_evdev(key: KeyCode) -> Option<u16> {
    KEYS.iter()
        .find_map(|&(raw, mapped)| (mapped == key).then_some(raw))
}
#[must_use]
pub fn modifier(code: u16) -> Modifiers {
    match code {
        29 => Modifiers::CTRL | Modifiers::LEFT_CTRL,
        97 => Modifiers::CTRL | Modifiers::RIGHT_CTRL,
        42 => Modifiers::SHIFT | Modifiers::LEFT_SHIFT,
        54 => Modifiers::SHIFT | Modifiers::RIGHT_SHIFT,
        56 => Modifiers::ALT | Modifiers::LEFT_ALT,
        100 => Modifiers::ALT | Modifiers::RIGHT_ALT,
        125 => Modifiers::CMD | Modifiers::LEFT_CMD,
        126 => Modifiers::CMD | Modifiers::RIGHT_CMD,
        464 => Modifiers::FN,
        _ => Modifiers::empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mapping_is_bijective() {
        for &(raw, key) in KEYS {
            assert_eq!(from_evdev(raw), Some(key));
            assert_eq!(to_evdev(key), Some(raw));
        }
        assert_eq!(from_evdev(0xffff), None);
    }
}
