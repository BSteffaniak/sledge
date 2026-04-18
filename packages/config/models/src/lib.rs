//! Serde types for the sledge TOML configuration.
//!
//! This crate contains no logic \u2014 just the shape of the config on disk.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Root config document.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigDoc {
    #[serde(default)]
    pub daemon: DaemonSection,
    #[serde(default)]
    pub app_aliases: HashMap<String, AppAlias>,
    #[serde(default, rename = "binding")]
    pub bindings: Vec<BindingDoc>,
}

/// `[daemon]` section.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DaemonSection {
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

impl Default for DaemonSection {
    fn default() -> Self {
        Self {
            log_level: default_log_level(),
        }
    }
}

fn default_log_level() -> String {
    "info".to_string()
}

/// A single logical app identifier, resolvable per-platform.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppAlias {
    #[serde(default)]
    pub macos: Option<String>,
    #[serde(default)]
    pub linux: Option<String>,
    #[serde(default)]
    pub windows: Option<String>,
}

/// One entry in the `[[binding]]` array.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindingDoc {
    #[serde(default)]
    pub when: Option<WhenDoc>,
    pub trigger: TriggerDoc,
    pub action: ActionDoc,
}

/// Scope predicate.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WhenDoc {
    #[serde(default)]
    pub app_in: Vec<String>,
}

/// Trigger is either `{ key, mods }` or `{ tap, count, within_ms }`. We
/// parse as an untagged enum so either shape works, and classify after.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TriggerDoc {
    Hotkey {
        key: String,
        #[serde(default)]
        mods: Vec<String>,
    },
    Tap {
        tap: String,
        count: u32,
        within_ms: u32,
    },
}

/// Action. Tagged by `type`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActionDoc {
    SendKey {
        key: String,
        #[serde(default)]
        mods: Vec<String>,
    },
    SetInputSource {
        id: String,
    },
}
