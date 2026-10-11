//! Sway IPC, explicitly selected by the caller; never an XWayland fallback.
use super::transport::SocketTransport;
use super::{
    DesktopError, DesktopProvider, DesktopSnapshot, FocusCategory, FocusIdentity, LockHint, Source,
    SourceCatalog, protocol,
};
use serde_json::Value;
use std::path::Path;
use std::time::Instant;

pub struct Sway {
    socket: SocketTransport,
}
impl Sway {
    pub fn new(socket: &Path) -> Self {
        Self {
            socket: SocketTransport::new(socket),
        }
    }
}
fn focused(node: &Value, depth: usize) -> Option<FocusIdentity> {
    if depth > 128 {
        return None;
    }
    if node["focused"].as_bool() == Some(true) {
        if let Some(id) = node["app_id"].as_str() {
            return FocusIdentity::new(FocusCategory::Wayland, id);
        }
        if let Some(class) = node["window_properties"]["class"].as_str() {
            return FocusIdentity::new(FocusCategory::X11Class, class);
        }
    }
    ["nodes", "floating_nodes"]
        .iter()
        .filter_map(|key| node[*key].as_array())
        .flatten()
        .find_map(|child| focused(child, depth + 1))
}
fn catalog(inputs: &Value) -> Result<SourceCatalog, DesktopError> {
    let inputs = inputs
        .as_array()
        .ok_or_else(|| protocol("expected input array"))?;
    let mut sources = Vec::new();
    let mut inventory = Vec::new();
    let mut current = None;
    for input in inputs {
        if input["type"].as_str() != Some("keyboard") {
            continue;
        }
        let device = input["identifier"]
            .as_str()
            .ok_or_else(|| protocol("missing keyboard identifier"))?;
        let layouts = input["xkb_layout_names"]
            .as_array()
            .ok_or_else(|| protocol("layout enumeration unavailable"))?;
        inventory.push(serde_json::json!([device, layouts]));
        for (index, label) in layouts.iter().enumerate() {
            let id = serde_json::to_string(&(device, index)).map_err(protocol)?;
            if input["xkb_active_layout_index"].as_u64() == u64::try_from(index).ok() {
                current = Some(id.clone());
            }
            sources.push(Source {
                id,
                label: label
                    .as_str()
                    .ok_or_else(|| protocol("invalid layout label"))?
                    .into(),
                enabled: true,
            });
        }
    }
    Ok(SourceCatalog {
        provider: "sway".into(),
        generation: serde_json::to_string(&inventory).map_err(protocol)?,
        sources,
        current,
    })
}
impl DesktopProvider for Sway {
    fn poll(&mut self) -> Result<DesktopSnapshot, DesktopError> {
        let _operation = super::operation::adapter_scope();
        let observed_at = Instant::now();
        let epoch = self.socket.epoch()?;
        let tree = self.socket.sway(4, "")?;
        let focus = if self.socket.epoch()? == epoch {
            focused(&tree, 0)
        } else {
            None
        };
        Ok(DesktopSnapshot {
            focus,
            observed_at,
            lock_hint: LockHint::Unknown,
        })
    }
    fn sources(&mut self) -> Result<SourceCatalog, DesktopError> {
        let _operation = super::operation::adapter_scope();
        let epoch = self.socket.epoch()?;
        let mut result = catalog(&self.socket.sway(100, "")?)?;
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
        // Sway's command language needs its own escaping, not shell quoting.
        if device
            .chars()
            .any(|c| c.is_control() || matches!(c, ';' | ','))
        {
            return Err(protocol("unsafe device identifier"));
        }
        let escaped = device.replace('\\', "\\\\").replace('"', "\\\"");
        let reply = self
            .socket
            .sway(0, &format!("input \"{escaped}\" xkb_switch_layout {index}"))?;
        if reply.as_array().is_none_or(|items| {
            items.is_empty()
                || items
                    .iter()
                    .any(|item| item["success"].as_bool() != Some(true))
        }) {
            return Err(DesktopError::SelectionFailed);
        }
        let inputs = self.socket.sway(100, "")?;
        let confirmed = inputs.as_array().is_some_and(|items| {
            items.iter().any(|input| {
                input["identifier"].as_str() == Some(&device)
                    && input["xkb_active_layout_index"].as_u64() == u64::try_from(index).ok()
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
    fn fake_ipc_selection_is_confirmed() {
        use std::io::{Read, Write};
        use std::os::unix::net::UnixListener;
        let path = std::env::temp_dir().join(format!("sledge-sway-select-{}", std::process::id()));
        let listener = UnixListener::bind(&path).unwrap();
        let server = std::thread::spawn(move || {
            for step in 0..4 {
                let (mut socket, _) = listener.accept().unwrap();
                let mut header = [0; 14];
                socket.read_exact(&mut header).unwrap();
                let length = u32::from_le_bytes(header[6..10].try_into().unwrap()) as usize;
                let mut payload = vec![0; length];
                socket.read_exact(&mut payload).unwrap();
                let kind = u32::from_le_bytes(header[10..14].try_into().unwrap());
                let body = if step == 2 {
                    assert_eq!(kind, 0);
                    assert_eq!(
                        String::from_utf8(payload).unwrap(),
                        "input \"kbd\" xkb_switch_layout 1"
                    );
                    serde_json::json!([{"success":true}])
                } else {
                    assert_eq!(kind, 100);
                    serde_json::json!([{"type":"keyboard","identifier":"kbd","xkb_layout_names":["US","DE"],"xkb_active_layout_index":i32::from(step==3)}])
                };
                let bytes = serde_json::to_vec(&body).unwrap();
                socket.write_all(b"i3-ipc").unwrap();
                socket
                    .write_all(&u32::try_from(bytes.len()).unwrap().to_le_bytes())
                    .unwrap();
                socket.write_all(&kind.to_le_bytes()).unwrap();
                socket.write_all(&bytes).unwrap();
            }
        });
        let mut provider = Sway::new(&path);
        let inventory = provider.sources().unwrap();
        provider.select(&inventory, "[\"kbd\",1]").unwrap();
        server.join().unwrap();
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn no_title_fallback() {
        assert!(focused(&serde_json::json!({"focused":true,"name":"Firefox"}), 0).is_none());
    }
    #[test]
    fn xwayland_is_typed() {
        let f = focused(
            &serde_json::json!({"nodes":[{"focused":true,"window_properties":{"class":"App"}}]}),
            0,
        )
        .unwrap();
        assert_eq!(f.category, FocusCategory::X11Class);
    }
    #[test]
    fn reorder_changes_inventory() {
        let a = serde_json::json!([{"type":"keyboard","identifier":"kbd","xkb_layout_names":["US","DE"],"xkb_active_layout_index":0}]);
        let b = serde_json::json!([{"type":"keyboard","identifier":"kbd","xkb_layout_names":["DE","US"],"xkb_active_layout_index":0}]);
        assert_ne!(
            catalog(&a).unwrap().generation,
            catalog(&b).unwrap().generation
        );
    }
}
