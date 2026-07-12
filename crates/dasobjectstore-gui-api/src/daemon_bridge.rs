use crate::object_browser_routes::StandaloneObjectBrowserClientError;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

const DEFAULT_PERMITS: usize = 8;
const DEFAULT_DEADLINE: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub(crate) struct DaemonBridge {
    permits: Arc<Semaphore>,
    deadline: Duration,
}

#[derive(Debug)]
pub(crate) enum DaemonBridgeError {
    Busy,
    Deadline,
    Join(String),
    Client(StandaloneObjectBrowserClientError),
}

impl DaemonBridge {
    pub(crate) fn packaged() -> Self {
        Self {
            permits: Arc::new(Semaphore::new(DEFAULT_PERMITS)),
            deadline: DEFAULT_DEADLINE,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_capacity_and_deadline(capacity: usize, deadline: Duration) -> Self {
        Self {
            permits: Arc::new(Semaphore::new(capacity)),
            deadline,
        }
    }

    pub(crate) async fn call<T, F>(&self, operation: F) -> Result<T, DaemonBridgeError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, StandaloneObjectBrowserClientError> + Send + 'static,
    {
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
            Ok(Ok(Ok(value))) => Ok(value),
            Ok(Ok(Err(error))) => Err(DaemonBridgeError::Client(error)),
            Ok(Err(error)) => Err(DaemonBridgeError::Join(error.to_string())),
            Err(_) => Err(DaemonBridgeError::Deadline),
        }
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
}
