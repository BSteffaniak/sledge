//! Bounded source work; callbacks retain the exact queue-time inventory.
use sledge_core::BackendError;
use sledge_linux::desktop::{SourceCatalog, worker::SourceCache};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;
use tracing::warn;
const CAPACITY: usize = 16;

#[derive(Clone)]
pub(super) struct SourceQueue {
    sender: mpsc::SyncSender<SourceAction>,
    cache: SourceCache,
    stopped: Arc<AtomicBool>,
}
struct SourceAction {
    id: String,
    catalog: Arc<SourceCatalog>,
}
impl SourceQueue {
    pub(super) fn submit(&self, id: String) -> Result<(), BackendError> {
        let catalog = self.cache.current().ok_or_else(|| {
            BackendError::Inject("source inventory unavailable or expired".into())
        })?;
        let source = catalog
            .sources
            .iter()
            .find(|source| source.id == id)
            .ok_or_else(|| BackendError::UnknownInputSource(id.clone()))?;
        if !source.enabled {
            return Err(BackendError::Inject("disabled input source".into()));
        }
        self.submit_captured(id, catalog)
    }
    fn submit_captured(&self, id: String, catalog: Arc<SourceCatalog>) -> Result<(), BackendError> {
        if self.stopped.load(Ordering::Acquire) {
            return Err(BackendError::Inject("source worker stopped".into()));
        }
        self.sender
            .try_send(SourceAction { id, catalog })
            .map_err(|error| {
                BackendError::Inject(format!("source queue admission failed: {error}"))
            })
    }
}
pub(super) struct SourceWorker {
    stopped: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl SourceWorker {
    /// `select` must cooperate with desktop::operation cancellation and deadlines;
    /// shutdown joins the worker rather than detaching uninterruptible user code.
    pub(super) fn start(
        cache: SourceCache,
        select: impl Fn(&SourceCatalog, &str) -> Result<(), BackendError> + Send + 'static,
    ) -> std::io::Result<(SourceQueue, Self)> {
        let (sender, receiver) = mpsc::sync_channel::<SourceAction>(CAPACITY);
        let stopped = Arc::new(AtomicBool::new(false));
        let thread_stopped = stopped.clone();
        let thread = std::thread::Builder::new().name("sledge-source-actions".into()).spawn(move || {
            while !thread_stopped.load(Ordering::Acquire) {
                match receiver.recv_timeout(Duration::from_millis(50)) {
                    Ok(action) => {
                        let result = sledge_linux::desktop::operation::run(thread_stopped.clone(), || {
                            select(&action.catalog, &action.id).map_err(|error| sledge_linux::desktop::DesktopError::Unavailable(error.to_string()))
                        });
                        if let Err(error) = result { warn!(%error, source = %action.id, "queued source selection failed"); }
                    },
                    Err(mpsc::RecvTimeoutError::Timeout) => {},
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        })?;
        Ok((
            SourceQueue {
                sender,
                cache,
                stopped: stopped.clone(),
            },
            Self {
                stopped,
                thread: Some(thread),
            },
        ))
    }
}
impl Drop for SourceWorker {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blocked_provider_does_not_block_admission_and_queue_is_bounded() {
        let (entered_sender, entered) = mpsc::channel();
        let (release, released) = mpsc::channel::<()>();
        let (queue, worker) = SourceWorker::start(SourceCache::default(), move |catalog, id| {
            assert_eq!(id, "native-generation-one");
            assert_eq!(catalog.generation, "inventory-one");
            entered_sender.send(()).unwrap();
            loop {
                if released.recv_timeout(Duration::from_millis(5)).is_ok() {
                    break;
                }
                sledge_linux::desktop::operation::remaining()
                    .map_err(|error| BackendError::Inject(error.to_string()))?;
            }
            Ok(())
        })
        .unwrap();
        let catalog = Arc::new(SourceCatalog {
            provider: "fake".into(),
            generation: "inventory-one".into(),
            sources: vec![],
            current: None,
        });
        queue
            .submit_captured("native-generation-one".into(), catalog.clone())
            .unwrap();
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        for _ in 0..CAPACITY {
            queue
                .submit_captured("native-generation-one".into(), catalog.clone())
                .unwrap();
        }
        assert!(
            queue
                .submit_captured("native-generation-two".into(), catalog)
                .is_err()
        );
        let started = std::time::Instant::now();
        drop(worker);
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(
            queue
                .submit_captured(
                    "id".into(),
                    Arc::new(SourceCatalog {
                        provider: "fake".into(),
                        generation: "inventory-two".into(),
                        sources: vec![],
                        current: None,
                    })
                )
                .is_err()
        );
        drop(release);
    }

    #[test]
    fn stale_queued_inventory_is_not_reinterpreted() {
        use sledge_linux::desktop::Source;
        let old = Arc::new(SourceCatalog {
            provider: "fake".into(),
            generation: "one".into(),
            sources: vec![Source {
                id: "0".into(),
                label: "US".into(),
                enabled: true,
            }],
            current: None,
        });
        let mut fresh = (*old).clone();
        fresh.generation = "two".into();
        fresh.sources[0].label = "different layout".into();
        let (entered_sender, entered) = mpsc::channel();
        let (release, released) = mpsc::channel::<()>();
        let (checked_sender, checked) = mpsc::channel();
        let first = AtomicBool::new(true);
        let (queue, worker) = SourceWorker::start(SourceCache::default(), move |catalog, id| {
            if first.swap(false, Ordering::AcqRel) {
                entered_sender.send(()).unwrap();
                released.recv_timeout(Duration::from_secs(1)).unwrap();
            }
            assert_eq!(id, "0");
            let result = if catalog.generation == fresh.generation {
                Ok(())
            } else {
                Err(sledge_linux::desktop::DesktopError::StaleInventory)
            };
            checked_sender
                .send(matches!(
                    result,
                    Err(sledge_linux::desktop::DesktopError::StaleInventory)
                ))
                .unwrap();
            result.map_err(|error| BackendError::Inject(error.to_string()))
        })
        .unwrap();
        queue.submit_captured("0".into(), old.clone()).unwrap();
        entered.recv_timeout(Duration::from_secs(1)).unwrap();
        queue.submit_captured("0".into(), old).unwrap();
        release.send(()).unwrap();
        assert!(checked.recv_timeout(Duration::from_secs(1)).unwrap());
        assert!(checked.recv_timeout(Duration::from_secs(1)).unwrap());
        drop(worker);
    }
}
