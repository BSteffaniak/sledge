//! TOML configuration parser and validator.
//!
//! Reads a [`ConfigDoc`] from disk, validates it, and compiles the bindings
//! into a [`RuleSet`] consumable by the core engine.

use std::collections::HashMap;
use std::path::Path;

use sledge_config_models::{
    ActionDoc, AppAlias, BindingDoc, ConfigDoc, DaemonSection, TriggerDoc, WhenDoc,
};
use sledge_core::{Action, HotkeyTrigger, KeyCode, Modifiers, Rule, RuleSet, TapTrigger, Trigger};
use thiserror::Error;
use tracing::debug;

pub use sledge_config_models as models;

/// Target platform for alias resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacOS,
    Linux,
    Windows,
}

impl Platform {
    /// The platform the binary was compiled for.
    #[must_use]
    pub const fn current() -> Self {
        #[cfg(target_os = "macos")]
        {
            Self::MacOS
        }
        #[cfg(target_os = "linux")]
        {
            Self::Linux
        }
        #[cfg(target_os = "windows")]
        {
            Self::Windows
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            Self::MacOS
        }
    }
}

/// Fully validated configuration. Cheap to clone.
#[derive(Debug, Clone)]
pub struct Config {
    pub daemon: DaemonSection,
    pub app_aliases: HashMap<String, AppAlias>,
    pub rules: RuleSet,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config file {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse config TOML: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("invalid log_level {0:?}")]
    InvalidLogLevel(String),
    #[error("unknown key name {name:?} in binding #{index}")]
    UnknownKey { name: String, index: usize },
    #[error("unknown modifier name {name:?} in binding #{index}")]
    UnknownModifier { name: String, index: usize },
    #[error("binding #{index} references unknown app alias {name:?}")]
    UnknownAlias { name: String, index: usize },
    #[error("binding #{index}: tap count must be >= 2, got {count}")]
    TapCountTooLow { index: usize, count: u32 },
    #[error("binding #{index}: tap within_ms must be > 0")]
    TapWindowZero { index: usize },
    #[error("binding #{index}: tap trigger key {name:?} is not a modifier")]
    TapNotModifier { index: usize, name: String },
}

/// Parse a config from a string.
///
/// # Errors
///
/// Returns a [`ConfigError`] if the TOML fails to parse or validation fails.
pub fn parse_str(text: &str) -> Result<Config, ConfigError> {
    let doc: ConfigDoc = toml::from_str(text)?;
    validate(doc)
}

/// Read and parse a config file.
///
/// # Errors
///
/// Returns a [`ConfigError`] if I/O fails or parsing/validation fails.
pub fn load_from_file(path: &Path) -> Result<Config, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|e| ConfigError::Io {
        path: path.display().to_string(),
        source: e,
    })?;
    parse_str(&text)
}

fn validate(doc: ConfigDoc) -> Result<Config, ConfigError> {
    validate_log_level(&doc.daemon.log_level)?;

    let mut rules = Vec::with_capacity(doc.bindings.len());
    for (idx, b) in doc.bindings.into_iter().enumerate() {
        rules.push(validate_binding(idx, b, &doc.app_aliases)?);
    }

    debug!(count = rules.len(), "compiled rules");

    Ok(Config {
        daemon: doc.daemon,
        app_aliases: doc.app_aliases,
        rules: RuleSet::new(rules),
    })
}

fn validate_log_level(level: &str) -> Result<(), ConfigError> {
    match level {
        "trace" | "debug" | "info" | "warn" | "error" => Ok(()),
        _ => Err(ConfigError::InvalidLogLevel(level.to_string())),
    }
}

fn validate_binding(
    index: usize,
    b: BindingDoc,
    aliases: &HashMap<String, AppAlias>,
) -> Result<Rule, ConfigError> {
    let trigger = parse_trigger(index, &b.trigger)?;
    let action = parse_action(index, &b.action)?;
    let when_app_in = b
        .when
        .map(|w| validate_when(index, &w, aliases))
        .transpose()?;

    Ok(Rule {
        trigger,
        action,
        when_app_in,
    })
}

fn validate_when(
    index: usize,
    w: &WhenDoc,
    aliases: &HashMap<String, AppAlias>,
) -> Result<Vec<String>, ConfigError> {
    for name in &w.app_in {
        if !aliases.contains_key(name) {
            return Err(ConfigError::UnknownAlias {
                name: name.clone(),
                index,
            });
        }
    }
    Ok(w.app_in.clone())
}

fn parse_trigger(index: usize, t: &TriggerDoc) -> Result<Trigger, ConfigError> {
    match t {
        TriggerDoc::Hotkey { key, mods } => {
            let key = parse_keycode(index, key)?;
            let mods = parse_mods(index, mods)?;
            Ok(Trigger::Hotkey(HotkeyTrigger { key, mods }))
        }
        TriggerDoc::Tap {
            tap,
            count,
            within_ms,
        } => {
            let key = parse_keycode(index, tap)?;
            if !key.is_modifier() {
                return Err(ConfigError::TapNotModifier {
                    index,
                    name: tap.clone(),
                });
            }
            if *count < 2 {
                return Err(ConfigError::TapCountTooLow {
                    index,
                    count: *count,
                });
            }
            if *within_ms == 0 {
                return Err(ConfigError::TapWindowZero { index });
            }
            Ok(Trigger::Tap(TapTrigger {
                tap: key,
                count: *count,
                within_ms: *within_ms,
            }))
        }
    }
}

fn parse_action(index: usize, a: &ActionDoc) -> Result<Action, ConfigError> {
    match a {
        ActionDoc::SendKey { key, mods } => {
            let key = parse_keycode(index, key)?;
            let mods = parse_mods(index, mods)?;
            Ok(Action::SendKey { key, mods })
        }
        ActionDoc::SetInputSource { id } => Ok(Action::SetInputSource { id: id.clone() }),
    }
}

fn parse_mods(index: usize, list: &[String]) -> Result<Modifiers, ConfigError> {
    let mut m = Modifiers::empty();
    for name in list {
        m |= mod_from_name(name).ok_or_else(|| ConfigError::UnknownModifier {
            name: name.clone(),
            index,
        })?;
    }
    Ok(m)
}

fn mod_from_name(name: &str) -> Option<Modifiers> {
    Some(match name {
        "ctrl" => Modifiers::CTRL,
        "shift" => Modifiers::SHIFT,
        "alt" => Modifiers::ALT,
        "cmd" | "meta" | "super" => Modifiers::CMD,
        "fn" => Modifiers::FN,
        "left_ctrl" => Modifiers::LEFT_CTRL | Modifiers::CTRL,
        "right_ctrl" => Modifiers::RIGHT_CTRL | Modifiers::CTRL,
        "left_shift" => Modifiers::LEFT_SHIFT | Modifiers::SHIFT,
        "right_shift" => Modifiers::RIGHT_SHIFT | Modifiers::SHIFT,
        "left_alt" => Modifiers::LEFT_ALT | Modifiers::ALT,
        "right_alt" => Modifiers::RIGHT_ALT | Modifiers::ALT,
        "left_cmd" => Modifiers::LEFT_CMD | Modifiers::CMD,
        "right_cmd" => Modifiers::RIGHT_CMD | Modifiers::CMD,
        _ => return None,
    })
}

fn parse_keycode(index: usize, name: &str) -> Result<KeyCode, ConfigError> {
    keycode_from_name(name).ok_or_else(|| ConfigError::UnknownKey {
        name: name.to_string(),
        index,
    })
}

// Exhaustive string -> KeyCode table, matching the PascalCase names in the
// core enum.
fn keycode_from_name(name: &str) -> Option<KeyCode> {
    use KeyCode as K;
    Some(match name {
        "KeyA" => K::KeyA,
        "KeyB" => K::KeyB,
        "KeyC" => K::KeyC,
        "KeyD" => K::KeyD,
        "KeyE" => K::KeyE,
        "KeyF" => K::KeyF,
        "KeyG" => K::KeyG,
        "KeyH" => K::KeyH,
        "KeyI" => K::KeyI,
        "KeyJ" => K::KeyJ,
        "KeyK" => K::KeyK,
        "KeyL" => K::KeyL,
        "KeyM" => K::KeyM,
        "KeyN" => K::KeyN,
        "KeyO" => K::KeyO,
        "KeyP" => K::KeyP,
        "KeyQ" => K::KeyQ,
        "KeyR" => K::KeyR,
        "KeyS" => K::KeyS,
        "KeyT" => K::KeyT,
        "KeyU" => K::KeyU,
        "KeyV" => K::KeyV,
        "KeyW" => K::KeyW,
        "KeyX" => K::KeyX,
        "KeyY" => K::KeyY,
        "KeyZ" => K::KeyZ,
        "Digit0" => K::Digit0,
        "Digit1" => K::Digit1,
        "Digit2" => K::Digit2,
        "Digit3" => K::Digit3,
        "Digit4" => K::Digit4,
        "Digit5" => K::Digit5,
        "Digit6" => K::Digit6,
        "Digit7" => K::Digit7,
        "Digit8" => K::Digit8,
        "Digit9" => K::Digit9,
        "F1" => K::F1,
        "F2" => K::F2,
        "F3" => K::F3,
        "F4" => K::F4,
        "F5" => K::F5,
        "F6" => K::F6,
        "F7" => K::F7,
        "F8" => K::F8,
        "F9" => K::F9,
        "F10" => K::F10,
        "F11" => K::F11,
        "F12" => K::F12,
        "F13" => K::F13,
        "F14" => K::F14,
        "F15" => K::F15,
        "F16" => K::F16,
        "F17" => K::F17,
        "F18" => K::F18,
        "F19" => K::F19,
        "F20" => K::F20,
        "F21" => K::F21,
        "F22" => K::F22,
        "F23" => K::F23,
        "F24" => K::F24,
        "ArrowUp" => K::ArrowUp,
        "ArrowDown" => K::ArrowDown,
        "ArrowLeft" => K::ArrowLeft,
        "ArrowRight" => K::ArrowRight,
        "LeftShift" => K::LeftShift,
        "RightShift" => K::RightShift,
        "LeftCtrl" => K::LeftCtrl,
        "RightCtrl" => K::RightCtrl,
        "LeftAlt" => K::LeftAlt,
        "RightAlt" => K::RightAlt,
        "LeftCmd" => K::LeftCmd,
        "RightCmd" => K::RightCmd,
        "Fn" => K::Fn,
        "CapsLock" => K::CapsLock,
        "Return" => K::Return,
        "Tab" => K::Tab,
        "Space" => K::Space,
        "Backspace" => K::Backspace,
        "Delete" => K::Delete,
        "Escape" => K::Escape,
        "Semicolon" => K::Semicolon,
        "Quote" => K::Quote,
        "Comma" => K::Comma,
        "Period" => K::Period,
        "Slash" => K::Slash,
        "Backslash" => K::Backslash,
        "Backquote" => K::Backquote,
        "Minus" => K::Minus,
        "Equal" => K::Equal,
        "LeftBracket" => K::LeftBracket,
        "RightBracket" => K::RightBracket,
        "Home" => K::Home,
        "End" => K::End,
        "PageUp" => K::PageUp,
        "PageDown" => K::PageDown,
        "Insert" => K::Insert,
        _ => return None,
    })
}

/// Resolve a logical alias (e.g. `"ghostty"`) to the current platform's
/// identifier. Returns `None` if the alias doesn't exist or has no entry
/// for this platform.
#[must_use]
pub fn resolve_alias<'a>(
    aliases: &'a HashMap<String, AppAlias>,
    name: &str,
    platform: Platform,
) -> Option<&'a str> {
    let a = aliases.get(name)?;
    match platform {
        Platform::MacOS => a.macos.as_deref(),
        Platform::Linux => a.linux.as_deref(),
        Platform::Windows => a.windows.as_deref(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r#"
[daemon]
log_level = "debug"

[app_aliases.ghostty]
macos = "com.mitchellh.ghostty"

[[binding]]
trigger = { key = "Digit1", mods = ["cmd", "alt"] }
action = { type = "set_input_source", id = "com.apple.keylayout.US" }

[[binding]]
trigger = { tap = "RightAlt", count = 3, within_ms = 600 }
action = { type = "send_key", key = "Return", mods = ["ctrl"] }

[[binding]]
when = { app_in = ["ghostty"] }
trigger = { key = "Semicolon", mods = ["ctrl"] }
action = { type = "send_key", key = "KeyS", mods = ["ctrl"] }
"#;

    #[test]
    fn parses_example() {
        let cfg = parse_str(EXAMPLE).expect("parse ok");
        assert_eq!(cfg.daemon.log_level, "debug");
        assert_eq!(cfg.rules.len(), 3);
    }

    #[test]
    fn unknown_key_errors() {
        let toml = r#"
[[binding]]
trigger = { key = "KeyBogus", mods = [] }
action = { type = "send_key", key = "Return", mods = [] }
"#;
        let err = parse_str(toml).unwrap_err();
        assert!(matches!(err, ConfigError::UnknownKey { .. }));
    }

    #[test]
    fn tap_count_too_low_errors() {
        let toml = r#"
[[binding]]
trigger = { tap = "RightAlt", count = 1, within_ms = 600 }
action = { type = "send_key", key = "Return", mods = [] }
"#;
        let err = parse_str(toml).unwrap_err();
        assert!(matches!(err, ConfigError::TapCountTooLow { .. }));
    }

    #[test]
    fn tap_non_modifier_errors() {
        let toml = r#"
[[binding]]
trigger = { tap = "KeyA", count = 3, within_ms = 600 }
action = { type = "send_key", key = "Return", mods = [] }
"#;
        let err = parse_str(toml).unwrap_err();
        assert!(matches!(err, ConfigError::TapNotModifier { .. }));
    }

    #[test]
    fn unknown_alias_errors() {
        let toml = r#"
[[binding]]
when = { app_in = ["ghostty"] }
trigger = { key = "KeyK", mods = ["alt"] }
action = { type = "send_key", key = "KeyT", mods = ["alt"] }
"#;
        let err = parse_str(toml).unwrap_err();
        assert!(matches!(err, ConfigError::UnknownAlias { .. }));
    }

    #[test]
    fn resolves_alias_per_platform() {
        let mut aliases = HashMap::new();
        aliases.insert(
            "ghostty".to_string(),
            AppAlias {
                macos: Some("com.mitchellh.ghostty".into()),
                linux: None,
                windows: None,
            },
        );
        assert_eq!(
            resolve_alias(&aliases, "ghostty", Platform::MacOS),
            Some("com.mitchellh.ghostty")
        );
        assert_eq!(resolve_alias(&aliases, "ghostty", Platform::Linux), None);
        assert_eq!(resolve_alias(&aliases, "missing", Platform::MacOS), None);
    }
}
