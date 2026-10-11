use super::*;

fn source_config(action: &str) -> String {
    format!(
        r#"
[input_source_aliases.us]
macos = "com.apple.keylayout.US"
linux = "us"
windows = "00000409"
[[binding]]
trigger = {{ key = "KeyA" }}
action = {{ type = "set_input_source", {action} }}
"#
    )
}

#[test]
fn compiles_logical_sources_for_each_platform() {
    for (platform, expected) in [
        (Platform::MacOS, "com.apple.keylayout.US"),
        (Platform::Linux, "us"),
        (Platform::Windows, "00000409"),
    ] {
        let cfg = parse_str_for_platform(&source_config("source = 'us'"), platform).unwrap();
        assert_eq!(
            cfg.rules.iter().next().unwrap().action,
            Action::SetInputSource {
                id: expected.into()
            }
        );
    }
}

#[test]
fn rejects_conflicting_missing_empty_and_unknown_action_fields() {
    for action in ["id = 'us', source = 'us'", "", "id = ''", "source = ''"] {
        assert!(parse_str(&source_config(action)).is_err(), "{action}");
    }
    for action in ["id = 'us', typo = 'us'", "source = 1", "id = 1"] {
        assert!(matches!(
            parse_str(&source_config(action)),
            Err(ConfigError::Parse(_))
        ));
    }
    assert!(matches!(
        parse_str(&source_config("source = 'unknown'")),
        Err(ConfigError::UnknownInputSource { name, index: 0 }) if name == "unknown"
    ));
}

#[test]
fn reports_missing_selected_platform_but_keeps_native_offline_configs() {
    let text = source_config("source = 'us'").replace("linux = \"us\"", "");
    assert!(matches!(
        parse_str_for_platform(&text, Platform::Linux),
        Err(ConfigError::MissingInputSourcePlatform { name, platform: Platform::Linux, index: 0 }) if name == "us"
    ));
    assert!(parse_str_for_platform(&text, Platform::MacOS).is_ok());
    let native = text.replace("source = 'us'", "id = 'com.apple.keylayout.US'");
    assert!(parse_str_for_platform(&native, Platform::Linux).is_ok());
}

#[test]
fn exact_typed_and_legacy_app_identities_remain_distinct() {
    let cfg = parse_str(
        r#"
[app_aliases.editor]
linux = "legacy-editor"
[app_aliases.editor.linux_ids]
wayland = "Editor"
x11_class = "EditorClass"
[app_aliases.other.linux_ids]
x11_class = "Editor"
"#,
    )
    .unwrap();
    assert_eq!(
        resolve_alias(&cfg.app_aliases, "editor", Platform::Linux),
        Some("legacy-editor")
    );
    for identity in [
        AppIdentity::LinuxLegacy("legacy-editor"),
        AppIdentity::LinuxWayland("Editor"),
        AppIdentity::LinuxX11Class("EditorClass"),
    ] {
        assert_eq!(
            resolve_app_identity(&cfg.app_aliases, identity),
            Some("editor")
        );
    }
    assert_eq!(
        resolve_app_identity(&cfg.app_aliases, AppIdentity::LinuxX11Class("Editor")),
        Some("other")
    );
    assert_eq!(
        resolve_app_identity(&cfg.app_aliases, AppIdentity::LinuxWayland("editor")),
        None
    );
    assert_eq!(
        resolve_app_identity(&cfg.app_aliases, AppIdentity::LinuxLegacy("Editor")),
        None
    );
}

#[test]
fn rejects_duplicate_ids_deterministically_in_every_category() {
    for field in [
        "macos",
        "linux",
        "windows",
        "linux_ids.wayland",
        "linux_ids.x11_class",
    ] {
        let text =
            format!("[app_aliases.z]\n{field} = 'same'\n[app_aliases.a]\n{field} = 'same'\n");
        assert!(
            matches!(parse_str(&text), Err(ConfigError::DuplicateIdentifier { first, second, .. }) if first == "a" && second == "z")
        );
    }
    for platform in ["macos", "linux", "windows"] {
        let text = format!(
            "[input_source_aliases.z]\n{platform} = 'same'\n[input_source_aliases.a]\n{platform} = 'same'\n"
        );
        assert!(
            matches!(parse_str(&text), Err(ConfigError::DuplicateIdentifier { first, second, .. }) if first == "a" && second == "z")
        );
    }
}

#[test]
fn allows_identical_ids_on_different_platforms_and_exact_case_variants() {
    assert!(
        parse_str(
            r#"
[app_aliases.a]
macos = "same"
linux = "same"
[app_aliases.b]
macos = "Same"
[input_source_aliases.a]
macos = "same"
[input_source_aliases.b]
linux = "same"
"#
        )
        .is_ok()
    );
}

#[test]
fn rejects_empty_alias_identifiers_and_unknown_typed_fields() {
    for text in [
        "[app_aliases.a]\nlinux = ' '",
        "[app_aliases.a.linux_ids]\nwayland = ''",
        "[input_source_aliases.a]\nlinux = ''",
    ] {
        assert!(matches!(
            parse_str(text),
            Err(ConfigError::EmptyIdentifier { .. })
        ));
    }
    assert!(matches!(
        parse_str("[app_aliases.a.linux_ids]\napp_id = 'editor'"),
        Err(ConfigError::Parse(_))
    ));
}

#[test]
fn old_schema_roundtrips_and_empty_config_still_compiles() {
    let text = source_config("id = 'com.apple.keylayout.US'");
    let doc: ConfigDoc = toml::from_str(&text).unwrap();
    let serialized = toml::to_string(&doc).unwrap();
    assert!(parse_str(&serialized).is_ok());
    assert!(parse_str("").is_ok());
}
