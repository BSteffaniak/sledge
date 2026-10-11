//! IBus source provider. Uses maintained Properties rather than deprecated methods.
//! Explicit enabled engines are ActiveEngines, never every installed engine.
use super::bus::Bus;
use super::{DesktopError, Source, SourceCatalog, protocol};
use zbus::zvariant::{Array, OwnedValue, Structure, Value};
pub struct IBus {
    bus: Bus,
}
fn engine(value: &Value<'_>) -> Result<(String, String), DesktopError> {
    let value = if let Value::Value(inner) = value {
        inner.as_ref()
    } else {
        value
    };
    let structure = <&Structure<'_>>::try_from(value).map_err(protocol)?;
    let fields = structure.fields();
    if fields.len() < 4 || <&str>::try_from(&fields[0]).map_err(protocol)? != "IBusEngineDesc" {
        return Err(protocol("invalid IBusEngineDesc"));
    }
    let id = <&str>::try_from(&fields[2]).map_err(protocol)?;
    let label = <&str>::try_from(&fields[3]).map_err(protocol)?;
    if !super::valid_id(id) {
        return Err(protocol("invalid engine name"));
    }
    Ok((id.into(), label.into()))
}
fn engines(value: &OwnedValue) -> Result<Vec<(String, String)>, DesktopError> {
    let array = <&Array<'_>>::try_from(value).map_err(protocol)?;
    if array.len() > 4096 {
        return Err(protocol("too many engines"));
    }
    array.inner().iter().map(engine).collect()
}
impl IBus {
    /// Session-bus IBus installations only. A private-bus installation without
    /// this name is explicitly unavailable; never discover it via subprocess.
    pub fn connect() -> Result<Self, DesktopError> {
        Ok(Self {
            bus: Bus::session(
                "org.freedesktop.IBus",
                "/org/freedesktop/IBus",
                "org.freedesktop.IBus",
            )?,
        })
    }
    pub fn sources(&mut self) -> Result<SourceCatalog, DesktopError> {
        let installed = engines(&self.bus.property("Engines")?)?;
        let enabled = engines(&self.bus.property("ActiveEngines")?)?;
        let value = self.bus.property("GlobalEngine")?;
        let current = engine(&value)?.0;
        let generation =
            serde_json::to_string(&(&self.bus.owner, &installed, &enabled)).map_err(protocol)?;
        let sources = installed
            .into_iter()
            .map(|(id, label)| Source {
                enabled: enabled.iter().any(|entry| entry.0 == id),
                id,
                label,
            })
            .collect();
        Ok(SourceCatalog {
            provider: "ibus".into(),
            generation,
            sources,
            current: Some(current),
        })
    }
    pub fn select(&mut self, old: &SourceCatalog, id: &str) -> Result<(), DesktopError> {
        old.check(&self.sources()?, id)?;
        self.bus.call("SetGlobalEngine", &(id,))?;
        if self.sources()?.current.as_deref() != Some(id) {
            return Err(DesktopError::SelectionFailed);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn engine_desc_accepts_appended_fields() {
        let value = Value::from((
            "IBusEngineDesc",
            std::collections::HashMap::<String, Value<'_>>::new(),
            "xkb:us::eng",
            "English",
            "appended",
        ));
        assert_eq!(engine(&value).unwrap().0, "xkb:us::eng");
        assert!(engine(&Value::from(("OtherDesc", 0_u32, "id", "label"))).is_err());
    }
}
