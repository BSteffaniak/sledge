//! Cached typed observations and serialized source dispatch. Never call provider
//! methods from an input event callback. Clearing on failure is automatic here.
use super::{DesktopError, DesktopProvider, DesktopSnapshot, FocusIdentity, SourceCatalog};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

/// Combine focus from a compositor with explicitly selected IBus/Fcitx5 sources.
/// No input-method framework is automatically preferred or started.
pub enum InputMethodProvider {
    IBus(super::ibus::IBus),
    Fcitx5(super::fcitx5::Fcitx5),
}
pub struct WithInputMethod {
    desktop: Box<dyn DesktopProvider>,
    input_method: InputMethodProvider,
}
impl WithInputMethod {
    pub fn new(desktop: Box<dyn DesktopProvider>, input_method: InputMethodProvider) -> Self {
        Self {
            desktop,
            input_method,
        }
    }
}
impl DesktopProvider for WithInputMethod {
    fn poll(&mut self) -> Result<DesktopSnapshot, DesktopError> {
        self.desktop.poll()
    }
    fn sources(&mut self) -> Result<SourceCatalog, DesktopError> {
        match &mut self.input_method {
            InputMethodProvider::IBus(provider) => provider.sources(),
            InputMethodProvider::Fcitx5(provider) => provider.sources(),
        }
    }
    fn select(&mut self, catalog: &SourceCatalog, id: &str) -> Result<(), DesktopError> {
        match &mut self.input_method {
            InputMethodProvider::IBus(provider) => provider.select(catalog, id),
            InputMethodProvider::Fcitx5(provider) => provider.select(catalog, id),
        }
    }
}

/// Read-only focus cache. Expired observations cannot escape via this API.
#[derive(Clone, Default)]
pub struct FocusCache {
    state: Arc<RwLock<Option<DesktopSnapshot>>>,
}
impl FocusCache {
    pub fn current(&self) -> Option<FocusIdentity> {
        self.state.try_read().ok().and_then(|state| {
            state
                .as_ref()
                .and_then(DesktopSnapshot::current_focus)
                .cloned()
        })
    }
    pub fn clear(&self) {
        if let Ok(mut state) = self.state.write() {
            *state = None;
        }
    }
}
type CachedCatalog = Option<(Arc<SourceCatalog>, std::time::Instant)>;
#[derive(Clone, Default)]
pub struct SourceCache {
    state: Arc<RwLock<CachedCatalog>>,
}
impl SourceCache {
    /// Nonblocking queue-time snapshot. No provider calls or mutex waits; busy,
    /// expired, or failed catalogs return None. Retain the Arc with the action.
    pub fn current(&self) -> Option<Arc<SourceCatalog>> {
        self.state.try_read().ok().and_then(|state| {
            state
                .as_ref()
                .filter(|(_, time)| time.elapsed() <= super::FOCUS_TTL)
                .map(|(catalog, _)| catalog.clone())
        })
    }
    fn clear(&self) {
        if let Ok(mut state) = self.state.write() {
            *state = None;
        }
    }
}

/// Single owner of the selected provider. Clone SourceDispatcher into backend
/// callback; keep this guard alive until backend workers have stopped.
pub struct DesktopWorker {
    dispatcher: SourceDispatcher,
    focus: FocusCache,
    sources: SourceCache,
    stopped: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}
#[derive(Clone)]
pub struct SourceDispatcher {
    provider: Arc<Mutex<Box<dyn DesktopProvider>>>,
}
impl SourceDispatcher {
    pub fn catalog(&self) -> Result<SourceCatalog, DesktopError> {
        self.provider.lock().map_err(super::unavailable)?.sources()
    }
    /// Select with the exact queued inventory generation; never silently
    /// reinterpret an old group index after a map/config change.
    pub fn select(&self, catalog: &SourceCatalog, id: &str) -> Result<(), DesktopError> {
        self.provider
            .lock()
            .map_err(super::unavailable)?
            .select(catalog, id)
    }
    /// Resolve a native ID against current inventory on the injection worker.
    /// Use `select` instead when queue-time inventory is already captured.
    pub fn select_current(&self, id: &str) -> Result<(), DesktopError> {
        let mut provider = self.provider.lock().map_err(super::unavailable)?;
        let catalog = provider.sources()?;
        provider.select(&catalog, id)
    }
}
impl DesktopWorker {
    pub fn start(provider: Box<dyn DesktopProvider>) -> Self {
        let dispatcher = SourceDispatcher {
            provider: Arc::new(Mutex::new(provider)),
        };
        let sources = SourceCache::default();
        let thread_sources = sources.clone();
        let focus = FocusCache::default();
        let stopped = Arc::new(AtomicBool::new(false));
        let thread_provider = dispatcher.provider.clone();
        let thread_focus = focus.clone();
        let thread_stopped = stopped.clone();
        let worker = std::thread::spawn(move || {
            while !thread_stopped.load(Ordering::Acquire) {
                let observation = thread_provider
                    .lock()
                    .ok()
                    .and_then(|mut provider| provider.poll().ok());
                let focus_ok = observation.is_some();
                if let Ok(mut state) = thread_focus.state.write() {
                    *state = observation;
                }
                let started = std::time::Instant::now();
                let catalog = if focus_ok {
                    thread_provider
                        .lock()
                        .ok()
                        .and_then(|mut provider| provider.sources().ok())
                        .map(Arc::new)
                } else {
                    None
                };
                if let Ok(mut state) = thread_sources.state.write() {
                    *state = catalog.map(|catalog| (catalog, started));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            thread_focus.clear();
            thread_sources.clear();
        });
        Self {
            dispatcher,
            focus,
            sources,
            stopped,
            worker: Some(worker),
        }
    }
    pub fn focus(&self) -> FocusCache {
        self.focus.clone()
    }
    pub fn sources(&self) -> SourceCache {
        self.sources.clone()
    }
    pub fn dispatcher(&self) -> SourceDispatcher {
        self.dispatcher.clone()
    }
}
impl Drop for DesktopWorker {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        self.focus.clear();
        self.sources.clear();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::{FocusCategory, LockHint};
    use std::time::Instant;
    #[test]
    fn catalog_snapshot_is_nonblocking_and_expires() {
        let cache = SourceCache::default();
        let catalog = Arc::new(SourceCatalog {
            provider: "test".into(),
            generation: "1".into(),
            sources: vec![],
            current: None,
        });
        *cache.state.write().unwrap() = Some((catalog, Instant::now()));
        assert!(cache.current().is_some());
        let lock = cache.state.write().unwrap();
        assert!(cache.current().is_none());
        drop(lock);
        cache.state.write().unwrap().as_mut().unwrap().1 = Instant::now()
            .checked_sub(super::super::FOCUS_TTL * 2)
            .unwrap();
        assert!(cache.current().is_none());
    }
    #[test]
    fn clearing_drops_identity() {
        let cache = FocusCache::default();
        *cache.state.write().unwrap() = Some(DesktopSnapshot {
            focus: FocusIdentity::new(FocusCategory::Wayland, "app"),
            observed_at: Instant::now(),
            lock_hint: LockHint::Unknown,
        });
        assert!(cache.current().is_some());
        cache.clear();
        assert!(cache.current().is_none());
    }
}
