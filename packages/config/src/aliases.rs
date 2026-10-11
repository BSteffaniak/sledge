//! Exact platform identities and deterministic alias validation.

use std::collections::{BTreeMap, HashMap};

use sledge_config_models::{AppAlias, InputSourceAlias};

use crate::{ConfigError, Platform};

/// A native application identity. Categories never match across protocols.
/// Legacy Linux strings must be explicitly selected by the consumer; they are
/// not implicitly interpreted as either Wayland app IDs or X11 classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AppIdentity<'a> {
    MacOS(&'a str),
    LinuxLegacy(&'a str),
    LinuxWayland(&'a str),
    LinuxX11Class(&'a str),
    Windows(&'a str),
}

impl<'a> AppIdentity<'a> {
    const fn value(self) -> &'a str {
        match self {
            Self::MacOS(id)
            | Self::LinuxLegacy(id)
            | Self::LinuxWayland(id)
            | Self::LinuxX11Class(id)
            | Self::Windows(id) => id,
        }
    }
}

fn app_identities(alias: &AppAlias) -> impl Iterator<Item = AppIdentity<'_>> {
    let linux = alias.linux_ids.as_ref();
    [
        alias.macos.as_deref().map(AppIdentity::MacOS),
        alias.linux.as_deref().map(AppIdentity::LinuxLegacy),
        linux
            .and_then(|ids| ids.wayland.as_deref())
            .map(AppIdentity::LinuxWayland),
        linux
            .and_then(|ids| ids.x11_class.as_deref())
            .map(AppIdentity::LinuxX11Class),
        alias.windows.as_deref().map(AppIdentity::Windows),
    ]
    .into_iter()
    .flatten()
}

/// Resolve an exact native identity to its logical app alias.
///
/// Validated configurations have unique identifiers in each category. For
/// unvalidated maps with duplicates, the lexicographically first alias wins.
#[must_use]
pub fn resolve_app_identity<'a>(
    aliases: &'a HashMap<String, AppAlias>,
    identity: AppIdentity<'_>,
) -> Option<&'a str> {
    aliases
        .iter()
        .filter(|(_, alias)| app_identities(alias).any(|id| id == identity))
        .map(|(name, _)| name.as_str())
        .min()
}

/// Resolve a logical input source to a platform's native identifier.
#[must_use]
pub fn resolve_input_source<'a>(
    aliases: &'a HashMap<String, InputSourceAlias>,
    name: &str,
    platform: Platform,
) -> Option<&'a str> {
    input_source_id(aliases.get(name)?, platform)
}

fn input_source_id(alias: &InputSourceAlias, platform: Platform) -> Option<&str> {
    match platform {
        Platform::MacOS => alias.macos.as_deref(),
        Platform::Linux => alias.linux.as_deref(),
        Platform::Windows => alias.windows.as_deref(),
    }
}

fn sorted_aliases<T>(aliases: &HashMap<String, T>) -> Vec<(&String, &T)> {
    let mut entries: Vec<_> = aliases.iter().collect();
    entries.sort_unstable_by_key(|(name, _)| *name);
    entries
}

pub(crate) fn validate_app_aliases(aliases: &HashMap<String, AppAlias>) -> Result<(), ConfigError> {
    let mut seen = BTreeMap::new();
    for (name, alias) in sorted_aliases(aliases) {
        for identity in app_identities(alias) {
            validate_identifier("app", name, identity.value())?;
            if let Some(first) = seen.insert(identity, name) {
                return Err(ConfigError::DuplicateIdentifier {
                    kind: "app",
                    identity: format!("{identity:?}"),
                    first: first.clone(),
                    second: name.clone(),
                });
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_input_sources(
    aliases: &HashMap<String, InputSourceAlias>,
) -> Result<(), ConfigError> {
    let mut seen = BTreeMap::new();
    for (name, alias) in sorted_aliases(aliases) {
        for platform in [Platform::MacOS, Platform::Linux, Platform::Windows] {
            if let Some(id) = input_source_id(alias, platform) {
                validate_identifier("input source", name, id)?;
                if let Some(first) = seen.insert((platform, id), name) {
                    return Err(ConfigError::DuplicateIdentifier {
                        kind: "input source",
                        identity: format!("{platform:?}({id:?})"),
                        first: first.clone(),
                        second: name.clone(),
                    });
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_identifier(
    kind: &'static str,
    name: &str,
    id: &str,
) -> Result<(), ConfigError> {
    if id.trim().is_empty() {
        return Err(ConfigError::EmptyIdentifier {
            kind,
            name: name.to_owned(),
        });
    }
    Ok(())
}
