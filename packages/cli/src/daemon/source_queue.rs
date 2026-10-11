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
                    Ok(action) => if let Err(error) = select(&action.catalog, &action.id) { warn!(%error, source = %action.id, "queued source selection failed"); },
                    Err(mpsc::RecvTimeoutError::Timeout) => {},
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        })?;
        Ok((
            SourceQueue { sender, cache },
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
        let (release, released) = mpsc::channel();
        let (queue, worker) = SourceWorker::start(SourceCache::default(), move |catalog, id| {
            assert_eq!(id, "native-generation-one");
            assert_eq!(catalog.generation, "inventory-one");
            entered_sender.send(()).unwrap();
            released.recv().unwrap();
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
        worker.stopped.store(true, Ordering::Release);
        release.send(()).unwrap();
        drop(worker);
    }
}
