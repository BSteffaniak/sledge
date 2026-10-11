//! Bounded, non-blocking dispatch of potentially blocking TIS operations.

use crossbeam_channel::{Sender, TrySendError, bounded};
use sledge_core::BackendError;
use tracing::warn;

use crate::tis::set_input_source;

const QUEUE_CAPACITY: usize = 32;

pub(crate) struct InputSourceWorker {
    sender: Option<Sender<String>>,
}

impl InputSourceWorker {
    pub(crate) fn new() -> Self {
        let (sender, receiver) = bounded::<String>(QUEUE_CAPACITY);
        let spawned = std::thread::Builder::new()
            .name("sledge-input-source".into())
            .spawn(move || {
                for id in receiver {
                    if let Err(error) = set_input_source(&id) {
                        warn!(%error, %id, "input-source action failed");
                    }
                }
            });
        let sender = match spawned {
            Ok(_) => Some(sender),
            Err(error) => {
                warn!(%error, "could not start input-source worker");
                None
            }
        };
        Self { sender }
    }

    /// Success means accepted, not completed. Never waits for queue capacity.
    pub(crate) fn enqueue(&self, id: &str) -> Result<(), BackendError> {
        let sender = self
            .sender
            .as_ref()
            .ok_or_else(|| BackendError::Inject("input-source worker unavailable".into()))?;
        sender.try_send(id.to_owned()).map_err(|error| {
            let reason = match error {
                TrySendError::Full(_) => "input-source queue full",
                TrySendError::Disconnected(_) => "input-source worker stopped",
            };
            BackendError::Inject(reason.into())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saturated_queue_is_rejected_without_waiting() {
        let (sender, receiver) = bounded(1);
        let worker = InputSourceWorker {
            sender: Some(sender),
        };
        assert!(worker.enqueue("first").is_ok());
        assert!(matches!(
            worker.enqueue("second"),
            Err(BackendError::Inject(_))
        ));
        assert_eq!(receiver.try_recv().unwrap(), "first");
    }

    #[test]
    fn unavailable_or_disconnected_worker_is_rejected() {
        assert!(InputSourceWorker { sender: None }.enqueue("us").is_err());
        let (sender, receiver) = bounded(1);
        drop(receiver);
        assert!(
            InputSourceWorker {
                sender: Some(sender)
            }
            .enqueue("us")
            .is_err()
        );
    }
}
