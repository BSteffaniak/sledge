//! Fcitx5 Controller1. Does not start Fcitx or alter persistent configuration.
use super::bus::Bus;
use super::{DesktopError, Source, SourceCatalog, protocol};
pub struct Fcitx5 {
    bus: Bus,
}
type Entry = (String, String, String, String, String, String, bool);
impl Fcitx5 {
    pub fn connect() -> Result<Self, DesktopError> {
        Ok(Self {
            bus: Bus::session(
                "org.fcitx.Fcitx5",
                "/controller",
                "org.fcitx.Fcitx.Controller1",
            )?,
        })
    }
    pub fn sources(&mut self) -> Result<SourceCatalog, DesktopError> {
        let entries: Vec<Entry> = self
            .bus
            .call("AvailableInputMethods", &())?
            .body()
            .deserialize()
            .map_err(protocol)?;
        let groups: Vec<String> = self
            .bus
            .call("InputMethodGroups", &())?
            .body()
            .deserialize()
            .map_err(protocol)?;
        if entries.len() > 4096 || groups.len() > 64 {
            return Err(protocol("input-method inventory too large"));
        }
        let active_group: String = self
            .bus
            .call("CurrentInputMethodGroup", &())?
            .body()
            .deserialize()
            .map_err(protocol)?;
        let mut enabled = Vec::new();
        let mut group_state = Vec::new();
        for group in groups {
            let info: (String, Vec<(String, String)>) = self
                .bus
                .call("InputMethodGroupInfo", &(&group,))?
                .body()
                .deserialize()
                .map_err(protocol)?;
            if group == active_group {
                enabled.extend(info.1.iter().map(|entry| entry.0.clone()));
            }
            group_state.push((group, info));
        }
        let current: String = self
            .bus
            .call("CurrentInputMethod", &())?
            .body()
            .deserialize()
            .map_err(protocol)?;
        let generation =
            serde_json::to_string(&(&self.bus.owner, &active_group, &group_state, &entries))
                .map_err(protocol)?;
        let sources = entries
            .into_iter()
            .map(|(id, label, _, _, _, _, _)| Source {
                enabled: enabled.contains(&id),
                id,
                label,
            })
            .collect();
        Ok(SourceCatalog {
            provider: "fcitx5".into(),
            generation,
            sources,
            current: Some(current),
        })
    }
    pub fn select(&mut self, old: &SourceCatalog, id: &str) -> Result<(), DesktopError> {
        old.check(&self.sources()?, id)?;
        self.bus.call("SetCurrentIM", &(id,))?;
        if self.sources()?.current.as_deref() != Some(id) {
            return Err(DesktopError::SelectionFailed);
        }
        Ok(())
    }
}
