use crate::object_browser_routes::StandaloneObjectBrowserClientError;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

const DEFAULT_PERMITS: usize = 8;
const PRIORITY_PERMITS: usize = 2;
const DEFAULT_DEADLINE: Duration = Duration::from_secs(2);
const CIRCUIT_FAILURE_THRESHOLD: usize = 3;
const CIRCUIT_COOLDOWN: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug)]
enum CircuitMode {
    Closed { failures: usize },
    Open { until: Instant },
    HalfOpen,
}

#[derive(Debug)]
struct CircuitState {
    epoch: u64,
    next_request_id: u64,
    last_failure_request_id: u64,
    last_success_request_id: u64,
    mode: CircuitMode,
}

#[derive(Clone, Copy, Debug)]
struct RequestStamp {
    epoch: u64,
    request_id: u64,
}

#[derive(Clone)]
pub(crate) struct DaemonBridge {
    permits: Arc<Semaphore>,
    deadline: Duration,
    circuit: Arc<Mutex<CircuitState>>,
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
            circuit: Arc::new(Mutex::new(CircuitState {
                epoch: 0,
                next_request_id: 0,
                last_failure_request_id: 0,
                last_success_request_id: 0,
                mode: CircuitMode::Closed { failures: 0 },
            })),
        }
    }

    pub(crate) fn shared_packaged() -> Arc<Self> {
        static BRIDGE: OnceLock<Arc<DaemonBridge>> = OnceLock::new();
        Arc::clone(BRIDGE.get_or_init(|| Arc::new(Self::packaged())))
    }

    pub(crate) fn priority_packaged() -> Self {
        Self {
            permits: Arc::new(Semaphore::new(PRIORITY_PERMITS)),
            deadline: DEFAULT_DEADLINE,
            circuit: Arc::new(Mutex::new(CircuitState {
                epoch: 0,
                next_request_id: 0,
                last_failure_request_id: 0,
                last_success_request_id: 0,
                mode: CircuitMode::Closed { failures: 0 },
            })),
        }
    }

    pub(crate) fn shared_priority_packaged() -> Arc<Self> {
        static BRIDGE: OnceLock<Arc<DaemonBridge>> = OnceLock::new();
        Arc::clone(BRIDGE.get_or_init(|| Arc::new(Self::priority_packaged())))
    }

    #[cfg(test)]
    pub(crate) fn with_capacity_and_deadline(capacity: usize, deadline: Duration) -> Self {
        Self {
            permits: Arc::new(Semaphore::new(capacity)),
            deadline,
            circuit: Arc::new(Mutex::new(CircuitState {
                epoch: 0,
                next_request_id: 0,
                last_failure_request_id: 0,
                last_success_request_id: 0,
                mode: CircuitMode::Closed { failures: 0 },
            })),
        }
    }

    pub(crate) async fn call<T, F>(&self, operation: F) -> Result<T, DaemonBridgeError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, StandaloneObjectBrowserClientError> + Send + 'static,
    {
        self.call_with_deadline(self.deadline, operation).await
    }

    pub(crate) async fn call_message_with_deadline<T, F>(
        &self,
        deadline: Duration,
        operation: F,
    ) -> Result<T, DaemonBridgeError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, String> + Send + 'static,
    {
        self.call_with_deadline(deadline, move || {
            operation().map_err(StandaloneObjectBrowserClientError::bridge_failure)
        })
        .await
    }

    async fn call_with_deadline<T, F>(
        &self,
        deadline: Duration,
        operation: F,
    ) -> Result<T, DaemonBridgeError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, StandaloneObjectBrowserClientError> + Send + 'static,
    {
        let request = self.begin_request()?;
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
        let task = tokio::task::spawn_blocking(move || {
            // Keep the permit inside the blocking closure. A timed-out socket
            // call may still be running, and releasing capacity before it
            // returns would allow unbounded stuck workers.
            let _permit = permit;
            operation()
        });
        match tokio::time::timeout(deadline, task).await {
            Ok(Ok(Ok(value))) => {
                self.complete_success(request);
                Ok(value)
            }
            Ok(Ok(Err(error))) => {
                if error.code == "daemon_bridge_transport_failed" {
                    self.record_failure(request);
                } else {
                    self.complete_non_connectivity(request);
                }
                Err(DaemonBridgeError::Client(error))
            }
            Ok(Err(error)) => {
                self.record_failure(request);
                Err(DaemonBridgeError::Join(error.to_string()))
            }
            Err(_) => {
                self.record_failure(request);
                Err(DaemonBridgeError::Deadline)
            }
        }
    }

    pub(crate) async fn call_message<T, F>(&self, operation: F) -> Result<T, DaemonBridgeError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, String> + Send + 'static,
    {
        self.call_message_with_deadline(self.deadline, operation)
            .await
    }

    fn begin_request(&self) -> Result<RequestStamp, DaemonBridgeError> {
        let mut circuit = self.circuit.lock().expect("daemon bridge circuit lock");
        circuit.next_request_id = circuit.next_request_id.wrapping_add(1);
        let request_id = circuit.next_request_id;
        match circuit.mode {
            CircuitMode::Closed { .. } => Ok(RequestStamp {
                epoch: circuit.epoch,
                request_id,
            }),
            CircuitMode::Open { until } if Instant::now() >= until => {
                circuit.epoch = circuit.epoch.wrapping_add(1);
                circuit.mode = CircuitMode::HalfOpen;
                Ok(RequestStamp {
                    epoch: circuit.epoch,
                    request_id,
                })
            }
            CircuitMode::Open { .. } | CircuitMode::HalfOpen => Err(DaemonBridgeError::CircuitOpen),
        }
    }

    fn complete_success(&self, request: RequestStamp) {
        let mut circuit = self.circuit.lock().expect("daemon bridge circuit lock");
        if circuit.epoch == request.epoch && request.request_id >= circuit.last_failure_request_id {
            circuit.last_success_request_id = request.request_id;
            circuit.mode = CircuitMode::Closed { failures: 0 };
        }
    }

    fn complete_non_connectivity(&self, request: RequestStamp) {
        self.complete_success(request);
    }

    fn record_failure(&self, request: RequestStamp) {
        let mut circuit = self.circuit.lock().expect("daemon bridge circuit lock");
        if circuit.epoch != request.epoch
            || request.request_id <= circuit.last_success_request_id
            || request.request_id <= circuit.last_failure_request_id
        {
            return;
        }
        circuit.last_failure_request_id = request.request_id;
        let failures = match circuit.mode {
            CircuitMode::HalfOpen => CIRCUIT_FAILURE_THRESHOLD,
            CircuitMode::Closed { failures } => failures + 1,
            CircuitMode::Open { .. } => return,
        };
        if failures >= CIRCUIT_FAILURE_THRESHOLD {
            circuit.epoch = circuit.epoch.wrapping_add(1);
            circuit.mode = CircuitMode::Open {
                until: Instant::now() + CIRCUIT_COOLDOWN,
            };
        } else {
            circuit.mode = CircuitMode::Closed { failures };
        }
    }

    #[cfg(test)]
    fn force_cooldown_elapsed(&self) {
        let mut circuit = self.circuit.lock().expect("daemon bridge circuit lock");
        circuit.mode = CircuitMode::Open {
            until: Instant::now() - Duration::from_millis(1),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::{DaemonBridge, DaemonBridgeError};
    use crate::object_browser_routes::StandaloneObjectBrowserClientError;
    use axum::http::StatusCode;
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

    #[tokio::test]
    async fn permits_only_one_half_open_probe_after_cooldown() {
        let bridge = DaemonBridge::with_capacity_and_deadline(1, Duration::from_millis(100));
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
        bridge.force_cooldown_elapsed();
        let (entered_sender, entered_receiver) = tokio::sync::oneshot::channel();
        let (release_sender, release_receiver) = tokio::sync::oneshot::channel();
        let probe = {
            let bridge = bridge.clone();
            tokio::spawn(async move {
                bridge
                    .call(move || {
                        entered_sender.send(()).expect("probe entered signal");
                        release_receiver
                            .blocking_recv()
                            .expect("probe release signal");
                        Ok(())
                    })
                    .await
            })
        };
        tokio::time::timeout(Duration::from_secs(1), entered_receiver)
            .await
            .expect("probe starts")
            .expect("probe entered");
        assert!(matches!(
            bridge.call(|| Ok(())).await,
            Err(DaemonBridgeError::CircuitOpen)
        ));
        release_sender.send(()).expect("release probe");
        assert!(probe.await.expect("probe joins").is_ok());
    }

    #[tokio::test]
    async fn priority_bridge_keeps_cancellation_capacity_when_routine_circuit_opens() {
        let routine = DaemonBridge::with_capacity_and_deadline(1, Duration::from_millis(100));
        let priority = DaemonBridge::priority_packaged();
        for _ in 0..3 {
            let result: Result<(), _> = routine
                .call(
                    || -> Result<(), super::StandaloneObjectBrowserClientError> {
                        panic!("simulated routine worker failure")
                    },
                )
                .await;
            assert!(matches!(result, Err(DaemonBridgeError::Join(_))));
        }
        assert!(matches!(
            routine.call(|| Ok(())).await,
            Err(DaemonBridgeError::CircuitOpen)
        ));
        assert!(priority.call(|| Ok(())).await.is_ok());
    }

    #[tokio::test]
    async fn repeated_transport_failures_open_the_circuit() {
        let bridge = DaemonBridge::with_capacity_and_deadline(1, Duration::from_millis(100));
        for _ in 0..3 {
            let result: Result<(), _> = bridge
                .call(|| {
                    Err(StandaloneObjectBrowserClientError {
                        status: StatusCode::BAD_GATEWAY,
                        code: "daemon_bridge_transport_failed".to_string(),
                        message: "socket unavailable".to_string(),
                    })
                })
                .await;
            assert!(matches!(result, Err(DaemonBridgeError::Client(_))));
        }
        assert!(matches!(
            bridge.call(|| Ok(())).await,
            Err(DaemonBridgeError::CircuitOpen)
        ));
    }

    #[tokio::test]
    async fn domain_errors_do_not_open_the_circuit() {
        let bridge = DaemonBridge::with_capacity_and_deadline(1, Duration::from_millis(100));
        for _ in 0..3 {
            let result: Result<(), _> = bridge
                .call(|| {
                    Err(StandaloneObjectBrowserClientError::bridge_failure(
                        "daemon rejected request",
                    ))
                })
                .await;
            assert!(matches!(result, Err(DaemonBridgeError::Client(_))));
        }
        assert!(bridge.call(|| Ok(())).await.is_ok());
    }

    #[tokio::test]
    async fn failed_half_open_probe_reopens_the_circuit() {
        let bridge = DaemonBridge::with_capacity_and_deadline(1, Duration::from_millis(100));
        for _ in 0..3 {
            let _: Result<(), _> = bridge
                .call(|| {
                    Err(StandaloneObjectBrowserClientError {
                        status: StatusCode::BAD_GATEWAY,
                        code: "daemon_bridge_transport_failed".to_string(),
                        message: "socket unavailable".to_string(),
                    })
                })
                .await;
        }
        bridge.force_cooldown_elapsed();
        let result: Result<(), _> = bridge
            .call(|| {
                Err(StandaloneObjectBrowserClientError {
                    status: StatusCode::BAD_GATEWAY,
                    code: "daemon_bridge_transport_failed".to_string(),
                    message: "socket unavailable".to_string(),
                })
            })
            .await;
        assert!(matches!(result, Err(DaemonBridgeError::Client(_))));
        assert!(matches!(
            bridge.call(|| Ok(())).await,
            Err(DaemonBridgeError::CircuitOpen)
        ));
    }

    #[test]
    fn stale_success_cannot_close_a_newer_open_circuit() {
        let bridge = DaemonBridge::with_capacity_and_deadline(1, Duration::from_millis(100));
        let first = bridge.begin_request().expect("first request admitted");
        bridge.record_failure(first);
        let second = bridge.begin_request().expect("second request admitted");
        bridge.record_failure(second);
        let third = bridge.begin_request().expect("third request admitted");
        bridge.record_failure(third);
        bridge.complete_success(first);
        assert!(matches!(
            bridge.begin_request(),
            Err(DaemonBridgeError::CircuitOpen)
        ));
    }
}
