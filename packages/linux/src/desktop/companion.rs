//! GNOME companion D-Bus client and KWin same-user push bridge.
use super::bus::Bus;
use super::{
    DesktopError, DesktopProvider, DesktopSnapshot, FOCUS_TTL, FocusIdentity, LockHint,
    MAX_MESSAGE, Source, SourceCatalog, protocol, unavailable,
};
use serde::Deserialize;
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireSnapshot {
    version: u32,
    epoch: String,
    sequence: u64,
    focus: Option<FocusIdentity>,
    lock_hint: LockHint,
    generation: String,
    sources: Vec<Source>,
    current: Option<String>,
}
fn parse(text: &str) -> Result<WireSnapshot, DesktopError> {
    if text.len() > MAX_MESSAGE {
        return Err(protocol("snapshot oversized"));
    }
    let wire: WireSnapshot = serde_json::from_str(text).map_err(protocol)?;
    if wire.version != 1
        || !super::valid_id(&wire.epoch)
        || wire.sources.len() > 4096
        || wire
            .focus
            .as_ref()
            .is_some_and(|f| !super::valid_id(&f.value))
        || wire.sources.iter().any(|s| !super::valid_id(&s.id))
    {
        return Err(protocol("invalid companion snapshot/version"));
    }
    Ok(wire)
}
fn snapshot(wire: &WireSnapshot, observed_at: Instant) -> DesktopSnapshot {
    DesktopSnapshot {
        focus: wire.focus.clone(),
        observed_at,
        lock_hint: wire.lock_hint,
    }
}
fn catalog(provider: &str, wire: &WireSnapshot) -> SourceCatalog {
    SourceCatalog {
        provider: provider.into(),
        generation: format!("{}:{}", wire.epoch, wire.generation),
        sources: wire.sources.clone(),
        current: wire.current.clone(),
    }
}

pub struct Gnome {
    bus: Bus,
    last: Option<(String, u64)>,
}
impl Gnome {
    pub fn connect() -> Result<Self, DesktopError> {
        let _operation = super::operation::adapter_scope();
        Ok(Self {
            bus: Bus::session(
                "org.sledge.Desktop.Gnome",
                "/org/sledge/Desktop",
                "org.sledge.Desktop1",
            )?,
            last: None,
        })
    }
    fn read(&mut self) -> Result<WireSnapshot, DesktopError> {
        let text: String = self
            .bus
            .call("Snapshot", &())?
            .body()
            .deserialize()
            .map_err(protocol)?;
        let wire = parse(&text)?;
        if self
            .last
            .as_ref()
            .is_some_and(|(epoch, sequence)| *epoch == wire.epoch && *sequence > wire.sequence)
        {
            return Err(protocol("regressed snapshot"));
        }
        self.last = Some((wire.epoch.clone(), wire.sequence));
        Ok(wire)
    }
}
impl DesktopProvider for Gnome {
    fn poll(&mut self) -> Result<DesktopSnapshot, DesktopError> {
        let _operation = super::operation::adapter_scope();
        let observed_at = Instant::now();
        Ok(snapshot(&self.read()?, observed_at))
    }
    fn sources(&mut self) -> Result<SourceCatalog, DesktopError> {
        let _operation = super::operation::adapter_scope();
        Ok(catalog("gnome", &self.read()?))
    }
    fn select(&mut self, old: &SourceCatalog, id: &str) -> Result<(), DesktopError> {
        let _operation = super::operation::adapter_scope();
        let wire = self.read()?;
        old.check(&catalog("gnome", &wire), id)?;
        let selected: bool = self
            .bus
            .call("Select", &(&wire.epoch, &wire.generation, id))?
            .body()
            .deserialize()
            .map_err(protocol)?;
        if !selected || self.read()?.current.as_deref() != Some(id) {
            return Err(DesktopError::SelectionFailed);
        }
        Ok(())
    }
}

#[derive(Clone)]
struct Receiver {
    state: Arc<Mutex<Option<(WireSnapshot, Instant)>>>,
    owner: String,
}
#[zbus::interface(name = "org.sledge.DesktopBridge1")]
impl Receiver {
    /// Messages are scope hints, never broker authorization. The session bus is
    /// same-user access, not authentication against malicious same-user clients.
    fn publish(&self, text: &str, #[zbus(header)] header: zbus::message::Header<'_>) -> bool {
        if header.sender().map(|sender| sender.as_str()) != Some(self.owner.as_str()) {
            return false;
        }
        self.accept(text)
    }
}
impl Receiver {
    fn accept(&self, text: &str) -> bool {
        let Ok(wire) = parse(text) else { return false };
        let Ok(mut state) = self.state.lock() else {
            return false;
        };
        if state
            .as_ref()
            .is_some_and(|(old, _)| old.epoch == wire.epoch && old.sequence >= wire.sequence)
        {
            return false;
        }
        *state = Some((wire, Instant::now()));
        true
    }
}
/// Owns a D-Bus bridge on a dedicated thread. Drop shuts down and joins it.
pub struct KwinBridge {
    state: Arc<Mutex<Option<(WireSnapshot, Instant)>>>,
    stop: Option<std::sync::mpsc::Sender<()>>,
    worker: Option<std::thread::JoinHandle<()>>,
    layouts: Bus,
    kwin: Bus,
}
impl KwinBridge {
    pub fn connect() -> Result<Self, DesktopError> {
        let _operation = super::operation::adapter_scope();
        let layouts = Bus::session("org.kde.keyboard", "/Layouts", "org.kde.KeyboardLayouts")?;
        let state = Arc::new(Mutex::new(None));
        let kwin = Bus::session("org.kde.KWin", "/KWin", "org.kde.KWin")?;
        let receiver = Receiver {
            state: state.clone(),
            owner: kwin.owner.clone(),
        };
        let (stop, stopped) = std::sync::mpsc::channel();
        let (ready, result) = std::sync::mpsc::sync_channel(1);
        let startup_deadline = std::time::Instant::now() + super::operation::remaining()?;
        let worker = std::thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(r) => r,
                Err(e) => {
                    let _ = ready.send(Err(e.to_string()));
                    return;
                }
            };
            runtime.block_on(async {
                let connection = match zbus::connection::Builder::session()
                    .and_then(|b| b.name("org.sledge.DesktopBridge"))
                    .and_then(|b| b.serve_at("/org/sledge/DesktopBridge", receiver))
                    .map(|b| b.max_queued(8).method_timeout(super::IPC_TIMEOUT))
                {
                    Ok(b) => tokio::time::timeout_at(
                        tokio::time::Instant::from_std(startup_deadline),
                        b.build(),
                    )
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|r| r.map_err(|e| e.to_string())),
                    Err(e) => Err(e.to_string()),
                };
                match connection {
                    Ok(connection) => {
                        let _ = ready.send(Ok(()));
                        while stopped.try_recv() == Err(std::sync::mpsc::TryRecvError::Empty) {
                            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                        }
                        drop(connection);
                    }
                    Err(e) => {
                        let _ = ready.send(Err(e));
                    }
                }
            });
        });
        let ready = result.recv_timeout(super::operation::remaining().unwrap_or_default());
        if !matches!(ready, Ok(Ok(()))) {
            let _ = stop.send(());
            let _ = worker.join();
            return Err(unavailable(format!(
                "KWin bridge startup failed: {ready:?}"
            )));
        }
        Ok(Self {
            state,
            stop: Some(stop),
            worker: Some(worker),
            layouts,
            kwin,
        })
    }
}
impl Drop for KwinBridge {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
impl DesktopProvider for KwinBridge {
    fn poll(&mut self) -> Result<DesktopSnapshot, DesktopError> {
        let _operation = super::operation::adapter_scope();
        self.kwin.check_owner()?;
        let state = super::operation::lock(&self.state)?;
        let (wire, time) = state
            .as_ref()
            .ok_or_else(|| unavailable("KWin companion has not published"))?;
        if time.elapsed() > FOCUS_TTL {
            return Err(unavailable("KWin companion expired"));
        }
        Ok(snapshot(wire, *time))
    }
    fn sources(&mut self) -> Result<SourceCatalog, DesktopError> {
        let _operation = super::operation::adapter_scope();
        let names: Vec<(String, String, String)> = self
            .layouts
            .call("getLayoutsList", &())?
            .body()
            .deserialize()
            .map_err(protocol)?;
        let current: u32 = self
            .layouts
            .call("getLayout", &())?
            .body()
            .deserialize()
            .map_err(protocol)?;
        let generation = serde_json::to_string(&(&self.layouts.owner, &names)).map_err(protocol)?;
        let sources = names
            .into_iter()
            .enumerate()
            .map(|(index, (_, label, _))| Source {
                id: index.to_string(),
                label,
                enabled: true,
            })
            .collect();
        Ok(SourceCatalog {
            provider: "kwin".into(),
            generation,
            sources,
            current: Some(current.to_string()),
        })
    }
    fn select(&mut self, old: &SourceCatalog, id: &str) -> Result<(), DesktopError> {
        let _operation = super::operation::adapter_scope();
        old.check(&self.sources()?, id)?;
        let index = id.parse::<u32>().map_err(protocol)?;
        let selected: bool = self
            .layouts
            .call("setLayout", &(index,))?
            .body()
            .deserialize()
            .map_err(protocol)?;
        if !selected || self.sources()?.current.as_deref() != Some(id) {
            return Err(DesktopError::SelectionFailed);
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_version_rejected() {
        assert!(parse(r#"{"version":2,"epoch":"e","sequence":1,"focus":null,"lock_hint":"unknown","generation":"1","sources":[],"current":null}"#).is_err());
    }
    #[test]
    fn duplicate_push_does_not_refresh() {
        let receiver = Receiver {
            owner: ":test".into(),
            state: Arc::new(Mutex::new(None)),
        };
        let text = r#"{"version":1,"epoch":"e","sequence":1,"focus":null,"lock_hint":"unknown","generation":"1","sources":[],"current":null}"#;
        assert!(receiver.accept(text));
        assert!(!receiver.accept(text));
    }
}
