//! Hyprland IPC. Lock query is a hint only; there is no atomic broker authority.
use super::transport::SocketTransport;
use super::{
    DesktopError, DesktopProvider, DesktopSnapshot, FocusCategory, FocusIdentity, LockHint, Source,
    SourceCatalog, protocol,
};
use serde_json::Value;
use std::path::Path;
use std::time::Instant;
pub struct Hyprland {
    socket: SocketTransport,
}
impl Hyprland {
    pub fn new(socket: &Path) -> Self {
        Self {
            socket: SocketTransport::new(socket),
        }
    }
}
fn catalog(devices: &Value) -> Result<SourceCatalog, DesktopError> {
    let keyboards = devices["keyboards"]
        .as_array()
        .ok_or_else(|| protocol("missing keyboards"))?;
    let mut sources = Vec::new();
    let mut inventory = Vec::new();
    let mut current = None;
    for keyboard in keyboards {
        let name = keyboard["name"]
            .as_str()
            .ok_or_else(|| protocol("missing keyboard name"))?;
        let layout = keyboard["layout"]
            .as_str()
            .ok_or_else(|| protocol("missing layouts"))?;
        let variants = keyboard["variant"]
            .as_str()
            .unwrap_or("")
            .split(',')
            .collect::<Vec<_>>();
        // A custom keymap cannot be reconstructed from layout/options: fail closed.
        if layout.is_empty() {
            return Err(protocol("custom keymap inventory unavailable"));
        }
        inventory.push(serde_json::json!([
            name,
            keyboard["layout"],
            keyboard["variant"],
            keyboard["options"],
            keyboard["rules"],
            keyboard["model"]
        ]));
        for (index, layout) in layout.split(',').enumerate() {
            let id = serde_json::to_string(&(name, index)).map_err(protocol)?;
            if keyboard["active_layout_index"].as_u64() == u64::try_from(index).ok() {
                current = Some(id.clone());
            }
            sources.push(Source {
                id,
                label: format!("{layout}({})", variants.get(index).copied().unwrap_or("")),
                enabled: true,
            });
        }
    }
    Ok(SourceCatalog {
        provider: "hyprland".into(),
        generation: serde_json::to_string(&inventory).map_err(protocol)?,
        sources,
        current,
    })
}
fn observation(
    focus: Option<FocusIdentity>,
    observed_at: Instant,
    lock_hint: LockHint,
    same_epoch: bool,
) -> DesktopSnapshot {
    let focus = if lock_hint == LockHint::Unlocked
        && observed_at.elapsed() <= super::FOCUS_TTL
        && same_epoch
    {
        focus
    } else {
        None
    };
    DesktopSnapshot {
        focus,
        observed_at,
        lock_hint,
    }
}
impl DesktopProvider for Hyprland {
    fn poll(&mut self) -> Result<DesktopSnapshot, DesktopError> {
        let _operation = super::operation::adapter_scope();
        let epoch = self.socket.epoch()?;
        // Stamp before the focus round trip, not after a subsequent lock call.
        let observed_at = Instant::now();
        let window = self.socket.hypr_json("j/activewindow")?;
        let category = match window["xwayland"].as_bool() {
            Some(true) => Some(FocusCategory::X11Class),
            Some(false) => Some(FocusCategory::Wayland),
            None => None,
        };
        let focus = category.and_then(|category| {
            window["class"]
                .as_str()
                .and_then(|id| FocusIdentity::new(category, id))
        });
        let lock_hint = match self
            .socket
            .hypr_json("j/locked")
            .ok()
            .and_then(|v| v["locked"].as_bool())
        {
            Some(true) => LockHint::Locked,
            Some(false) => LockHint::Unlocked,
            None => LockHint::Unknown,
        };
        // Unknown lock protocol, expired focus, or a replaced compositor socket
        // invalidates scope. Unlocked remains a hint, never broker authority.
        Ok(observation(
            focus,
            observed_at,
            lock_hint,
            self.socket.epoch()? == epoch,
        ))
    }
    fn sources(&mut self) -> Result<SourceCatalog, DesktopError> {
        let _operation = super::operation::adapter_scope();
        let epoch = self.socket.epoch()?;
        let mut result = catalog(&self.socket.hypr_json("j/devices")?)?;
        if self.socket.epoch()? != epoch {
            return Err(DesktopError::StaleInventory);
        }
        result.generation = format!("{epoch}:{}", result.generation);
        Ok(result)
    }
    fn select(&mut self, old: &SourceCatalog, id: &str) -> Result<(), DesktopError> {
        let _operation = super::operation::adapter_scope();
        old.check(&self.sources()?, id)?;
        let (device, index): (String, usize) = serde_json::from_str(id).map_err(protocol)?;
        if !device
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
            || device.is_empty()
        {
            return Err(protocol("unsafe keyboard name"));
        }
        let reply = self
            .socket
            .hypr(&format!("/switchxkblayout {device} {index}"))?;
        if reply.trim() != "ok" {
            return Err(DesktopError::SelectionFailed);
        }
        let devices = self.socket.hypr_json("j/devices")?;
        let epoch = self.socket.epoch()?;
        let mut fresh = catalog(&devices)?;
        fresh.generation = format!("{epoch}:{}", fresh.generation);
        old.check(&fresh, id)?;
        let confirmed = devices["keyboards"].as_array().is_some_and(|items| {
            items.iter().any(|keyboard| {
                keyboard["name"].as_str() == Some(&device)
                    && keyboard["active_layout_index"].as_u64() == u64::try_from(index).ok()
            })
        });
        if !confirmed {
            return Err(DesktopError::SelectionFailed);
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn slow_lock_does_not_refresh_focus() {
        let focus = FocusIdentity::new(FocusCategory::Wayland, "app");
        let time = Instant::now()
            .checked_sub(super::super::FOCUS_TTL * 2)
            .unwrap();
        let result = observation(focus.clone(), time, LockHint::Unlocked, true);
        assert_eq!(result.observed_at, time);
        assert!(result.focus.is_none());
        assert!(
            observation(focus.clone(), Instant::now(), LockHint::Unknown, true)
                .focus
                .is_none()
        );
        assert!(
            observation(focus, Instant::now(), LockHint::Unlocked, false)
                .focus
                .is_none()
        );
    }
    #[test]
    fn source_ids_include_device() {
        let c=catalog(&serde_json::json!({"keyboards":[{"name":"kbd","layout":"us,de","variant":",nodeadkeys","active_layout_index":0}]})).unwrap();
        assert_eq!(c.sources[1].id, "[\"kbd\",1]");
        assert_eq!(c.current.as_deref(), Some("[\"kbd\",0]"));
    }
    #[test]
    fn custom_map_is_unavailable() {
        assert!(catalog(&serde_json::json!({"keyboards":[{"name":"kbd","layout":""}]})).is_err());
    }
}
