//! Human-readable Linux backend state; these labels never grant admission.
use serde_json::Value;

pub(super) fn describe(caps: &Value) -> Option<&'static str> {
    let yes = |key| caps.get(key).and_then(Value::as_bool) == Some(true);
    if !yes("hyprland_supported_configured")
        && !yes("hyprland_candidate_configured")
        && !yes("hyprland_research_configured")
    {
        return None;
    }
    if !yes("hyprland_supported_configured") {
        return Some("unqualified — candidate/research build; not supported desktop capture");
    }
    if !yes("hyprland_authenticated") {
        return Some("waiting for compositor connection — no authenticated current session");
    }
    if yes("hyprland_locked") {
        return Some("paused — compositor locked; remapping excluded");
    }
    if yes("hyprland_awaiting_reconciliation") || !yes("hyprland_operational") {
        return Some("waiting for safe reconciliation — remapping not operational");
    }
    Some("ready — accepted build and operational compositor session")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn readiness_never_promotes_unknown_or_unqualified_state() {
        assert_eq!(describe(&serde_json::json!({})), None);
        let mut caps =
            serde_json::json!({"hyprland_candidate_configured":true,"hyprland_operational":true});
        assert!(describe(&caps).unwrap().starts_with("unqualified"));
        caps["hyprland_supported_configured"] = true.into();
        assert!(
            describe(&caps)
                .unwrap()
                .starts_with("waiting for compositor")
        );
        caps["hyprland_authenticated"] = true.into();
        caps["hyprland_locked"] = true.into();
        assert!(describe(&caps).unwrap().starts_with("paused"));
        caps["hyprland_locked"] = false.into();
        caps["hyprland_awaiting_reconciliation"] = true.into();
        assert!(describe(&caps).unwrap().starts_with("waiting for safe"));
        caps["hyprland_awaiting_reconciliation"] = false.into();
        assert!(describe(&caps).unwrap().starts_with("ready"));
        caps["hyprland_operational"] = false.into();
        assert!(describe(&caps).unwrap().starts_with("waiting for safe"));
    }
}
