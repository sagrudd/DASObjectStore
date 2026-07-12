use crate::object_browser_routes::StandaloneObjectBrowserClientError;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex, OnceLock,
};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

const DEFAULT_PERMITS: usize = 8;
const DEFAULT_DEADLINE: Duration = Duration::from_secs(2);
const CIRCUIT_FAILURE_THRESHOLD: usize = 3;
const CIRCUIT_COOLDOWN: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub(crate) struct DaemonBridge {
    permits: Arc<Semaphore>,
    deadline: Duration,
    consecutive_failures: Arc<AtomicUsize>,
    open_until: Arc<Mutex<Option<Instant>>>,
}

#[derive(Debug)]
pub(crate) enum DaemonBridgeError {
    Busy,
    CircuitOpen,
    Deadline,
    Join(String),
    Client(StandaloneObjectBrowserClientError),
}

impl DaemonBridge {
    pub(crate) fn packaged() -> Self {
        Self {
            permits: Arc::new(Semaphore::new(DEFAULT_PERMITS)),
            deadline: DEFAULT_DEADLINE,
            consecutive_failures: Arc::new(AtomicUsize::new(0)),
            open_until: Arc::new(Mutex::new(None)),
        }
    }

    pub(crate) fn shared_packaged() -> Arc<Self> {
        static BRIDGE: OnceLock<Arc<DaemonBridge>> = OnceLock::new();
        Arc::clone(BRIDGE.get_or_init(|| Arc::new(Self::packaged())))
    }

    #[cfg(test)]
    pub(crate) fn with_capacity_and_deadline(capacity: usize, deadline: Duration) -> Self {
        Self {
            permits: Arc::new(Semaphore::new(capacity)),
            deadline,
            consecutive_failures: Arc::new(AtomicUsize::new(0)),
            open_until: Arc::new(Mutex::new(None)),
        }
    }

    pub(crate) async fn call<T, F>(&self, operation: F) -> Result<T, DaemonBridgeError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, StandaloneObjectBrowserClientError> + Send + 'static,
    {
        if self.circuit_is_open() {
            return Err(DaemonBridgeError::CircuitOpen);
        }
        let permit = self
            .permits
            .clone()
            .try_acquire_owned()
            .map_err(|error| match error {
                tokio::sync::TryAcquireError::NoPermits => DaemonBridgeError::Busy,
                tokio::sync::TryAcquireError::Closed => DaemonBridgeError::Join(
                    "daemon bridge semaphore was unexpectedly closed".to_string(),
                ),
            })?;
        let deadline = self.deadline;
        let task = tokio::task::spawn_blocking(move || {
            // Keep the permit inside the blocking closure. A timed-out socket
            // call may still be running, and releasing capacity before it
            // returns would allow unbounded stuck workers.
            let _permit = permit;
            operation()
        });
        match tokio::time::timeout(deadline, task).await {
            Ok(Ok(Ok(value))) => {
                self.reset_circuit();
                Ok(value)
            }
            Ok(Ok(Err(error))) => Err(DaemonBridgeError::Client(error)),
            Ok(Err(error)) => {
                self.record_failure();
                Err(DaemonBridgeError::Join(error.to_string()))
            }
            Err(_) => {
                self.record_failure();
                Err(DaemonBridgeError::Deadline)
            }
        }
    }

    pub(crate) async fn call_message<T, F>(&self, operation: F) -> Result<T, DaemonBridgeError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, String> + Send + 'static,
    {
        self.call(move || operation().map_err(StandaloneObjectBrowserClientError::bridge_failure))
            .await
    }

    fn circuit_is_open(&self) -> bool {
        let mut open_until = self.open_until.lock().expect("daemon bridge circuit lock");
        match *open_until {
            Some(deadline) if Instant::now() < deadline => true,
            Some(_) => {
                *open_until = None;
                self.consecutive_failures.store(0, Ordering::Release);
                false
            }
            None => false,
        }
    }

    fn record_failure(&self) {
        let failures = self.consecutive_failures.fetch_add(1, Ordering::AcqRel) + 1;
        if failures >= CIRCUIT_FAILURE_THRESHOLD {
            *self.open_until.lock().expect("daemon bridge circuit lock") =
                Some(Instant::now() + CIRCUIT_COOLDOWN);
        }
    }

    fn reset_circuit(&self) {
        self.consecutive_failures.store(0, Ordering::Release);
        *self.open_until.lock().expect("daemon bridge circuit lock") = None;
    }
}

#[cfg(test)]
mod tests {
    use super::{DaemonBridge, DaemonBridgeError};
    use std::time::Duration;

    #[tokio::test]
    async fn timeout_retains_capacity_until_blocking_call_returns() {
        let bridge = DaemonBridge::with_capacity_and_deadline(1, Duration::from_millis(20));
        let (entered_sender, entered_receiver) = tokio::sync::oneshot::channel();
        let (release_sender, release_receiver) = tokio::sync::oneshot::channel();
        let first = {
            let bridge = bridge.clone();
            tokio::spawn(async move {
                bridge
                    .call(move || {
                        entered_sender.send(()).expect("entered signal");
                        release_receiver.blocking_recv().expect("release signal");
                        Ok(())
                    })
                    .await
            })
        };
        tokio::time::timeout(Duration::from_secs(1), entered_receiver)
            .await
            .expect("blocking call entered before timeout")
            .expect("entered signal");
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert!(matches!(
            first.await.expect("first task joins"),
            Err(DaemonBridgeError::Deadline)
        ));
        assert!(matches!(
            bridge.call(|| Ok(())).await,
            Err(DaemonBridgeError::Busy)
        ));

        release_sender.send(()).expect("release blocking call");
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(bridge.call(|| Ok(())).await.is_ok());
    }

    #[tokio::test]
    async fn opens_circuit_after_repeated_blocking_worker_failures() {
        let bridge = DaemonBridge::with_capacity_and_deadline(1, Duration::from_millis(20));
        for _ in 0..3 {
            let result: Result<(), _> = bridge
                .call(
                    || -> Result<(), super::StandaloneObjectBrowserClientError> {
                        panic!("simulated blocking worker failure")
                    },
                )
                .await;
            assert!(matches!(result, Err(DaemonBridgeError::Join(_))));
        }
        let result: Result<(), _> = bridge.call(|| Ok(())).await;
        assert!(matches!(result, Err(DaemonBridgeError::CircuitOpen)));
    }
}
