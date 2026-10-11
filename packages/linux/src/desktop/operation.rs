//! Cooperative aggregate budgets. Nested adapter calls cannot reset the deadline.
//! Cancellation prevents new local requests; an already sent remote request cannot
//! be recalled. Providers must cooperate: arbitrary blocking user code is not killable.
use super::{DesktopError, IPC_TIMEOUT, unavailable};
use std::cell::RefCell;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::time::{Duration, Instant};

pub(crate) const SLICE: Duration = Duration::from_millis(10);
thread_local! {
    static BUDGETS: RefCell<Vec<Budget>> = const { RefCell::new(Vec::new()) };
}
struct Budget {
    deadline: Instant,
    stopped: Arc<AtomicBool>,
}
pub(crate) struct Scope;
impl Drop for Scope {
    fn drop(&mut self) {
        BUDGETS.with(|budgets| {
            budgets.borrow_mut().pop();
        });
    }
}
pub(crate) fn scope(stopped: Arc<AtomicBool>) -> Scope {
    BUDGETS.with(|budgets| {
        budgets.borrow_mut().push(Budget {
            deadline: Instant::now() + IPC_TIMEOUT,
            stopped,
        });
    });
    Scope
}
pub(crate) fn adapter_scope() -> Scope {
    scope(Arc::new(AtomicBool::new(false)))
}
/// Run a synchronous cooperative provider operation, including nested mutex waits,
/// with one total deadline and a shutdown token. Does not detach or kill threads.
pub fn run<T>(
    stopped: Arc<AtomicBool>,
    operation: impl FnOnce() -> Result<T, DesktopError>,
) -> Result<T, DesktopError> {
    let _scope = scope(stopped);
    remaining()?;
    let result = operation()?;
    remaining()?;
    Ok(result)
}
/// Check cancellation and return the remaining aggregate budget. Custom providers
/// must check before effects and bound every wait by this value (or shorter slices).
pub fn remaining() -> Result<Duration, DesktopError> {
    BUDGETS.with(|budgets| {
        let mut left = IPC_TIMEOUT;
        for budget in budgets.borrow().iter() {
            if budget.stopped.load(Ordering::Acquire) {
                return Err(unavailable("desktop operation cancelled"));
            }
            left = left.min(budget.deadline.saturating_duration_since(Instant::now()));
        }
        if left.is_zero() {
            Err(unavailable("desktop operation deadline"))
        } else {
            Ok(left)
        }
    })
}
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>, DesktopError> {
    loop {
        let left = remaining()?;
        match mutex.try_lock() {
            Ok(guard) => {
                remaining()?;
                return Ok(guard);
            }
            Err(TryLockError::Poisoned(error)) => return Err(unavailable(error)),
            Err(TryLockError::WouldBlock) => std::thread::sleep(left.min(SLICE)),
        }
    }
}
pub(crate) async fn wait<T>(future: impl Future<Output = T>) -> Result<T, DesktopError> {
    tokio::pin!(future);
    loop {
        let left = remaining()?;
        if let Ok(result) = tokio::time::timeout(left.min(SLICE), &mut future).await {
            remaining()?;
            return Ok(result);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_operations_do_not_renew_total_deadline() {
        let _scope = adapter_scope();
        BUDGETS.with(|budgets| budgets.borrow_mut()[0].deadline = Instant::now());
        let _nested = adapter_scope();
        assert!(remaining().is_err());
    }
    #[test]
    fn slow_repeated_calls_share_one_budget() {
        let started = Instant::now();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = run(Arc::new(AtomicBool::new(false)), || {
            for _ in 0..64 {
                let _nested = adapter_scope();
                runtime.block_on(async {
                    wait(tokio::time::sleep(Duration::from_millis(150))).await
                })?;
            }
            Ok(())
        });
        assert!(result.is_err());
        assert!(started.elapsed() < IPC_TIMEOUT + Duration::from_secs(1));
    }
    #[test]
    fn cancelled_future_is_dropped_before_any_more_effects() {
        let stopped = Arc::new(AtomicBool::new(false));
        let _scope = scope(stopped.clone());
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let effects = AtomicBool::new(false);
        runtime.block_on(async {
            let future = async {
                tokio::time::sleep(Duration::from_millis(20)).await;
                stopped.store(true, Ordering::Release);
                std::future::pending::<()>().await;
                effects.store(true, Ordering::Release);
            };
            assert!(wait(future).await.is_err());
        });
        assert!(!effects.load(Ordering::Acquire));
    }
}
