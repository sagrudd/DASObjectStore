use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

pub const INGEST_CONTROL_CONFIRMATION: &str = "confirm ingest control";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonIngestControlAction {
    Pause,
    Throttle,
    Resume,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IngestControlRequest {
    pub action: DaemonIngestControlAction,
    pub reason: String,
    pub dry_run: bool,
    pub confirmation_marker: String,
}

impl IngestControlRequest {
    pub fn validate(&self) -> Result<(), IngestControlValidationError> {
        if self.reason.trim().is_empty() {
            return Err(IngestControlValidationError::BlankReason);
        }
        if !self.dry_run && self.confirmation_marker != INGEST_CONTROL_CONFIRMATION {
            return Err(IngestControlValidationError::ConfirmationMismatch);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonIngestControlState {
    #[default]
    Running,
    Throttled,
    Paused,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IngestControlResponse {
    pub state: DaemonIngestControlState,
    pub changed: bool,
    pub dry_run: bool,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IngestControlValidationError {
    BlankReason,
    ConfirmationMismatch,
}

impl std::fmt::Display for IngestControlValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BlankReason => formatter.write_str("reason must not be blank"),
            Self::ConfirmationMismatch => write!(
                formatter,
                "confirmation_marker must equal {INGEST_CONTROL_CONFIRMATION:?}"
            ),
        }
    }
}

impl std::error::Error for IngestControlValidationError {}

#[derive(Debug, Default)]
struct RuntimeState {
    state: DaemonIngestControlState,
}

fn runtime_state() -> &'static Mutex<RuntimeState> {
    static STATE: OnceLock<Mutex<RuntimeState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(RuntimeState::default()))
}

pub(crate) fn apply(
    action: DaemonIngestControlAction,
    reason: String,
    dry_run: bool,
) -> IngestControlResponse {
    let next = match action {
        DaemonIngestControlAction::Pause => DaemonIngestControlState::Paused,
        DaemonIngestControlAction::Throttle => DaemonIngestControlState::Throttled,
        DaemonIngestControlAction::Resume => DaemonIngestControlState::Running,
    };
    let mut state = runtime_state().lock().expect("ingest control lock");
    let changed = state.state != next;
    if !dry_run {
        state.state = next;
    }
    IngestControlResponse {
        state: if dry_run { next } else { state.state },
        changed,
        dry_run,
        reason,
    }
}

pub(crate) fn current() -> DaemonIngestControlState {
    runtime_state().lock().expect("ingest control lock").state
}

/// Wait between source objects while an operator pause is active. The current
/// object is never interrupted, preserving checksum and rename durability.
pub(crate) fn wait_for_source_admission() {
    while current() == DaemonIngestControlState::Paused {
        thread::sleep(Duration::from_millis(100));
    }
    if current() == DaemonIngestControlState::Throttled {
        thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .expect("test lock")
    }

    #[test]
    fn mutating_control_requires_confirmation() {
        let _lock = test_lock();
        let request = IngestControlRequest {
            action: DaemonIngestControlAction::Pause,
            reason: "operator incident".to_string(),
            dry_run: false,
            confirmation_marker: String::new(),
        };
        assert_eq!(
            request.validate(),
            Err(IngestControlValidationError::ConfirmationMismatch)
        );
    }

    #[test]
    fn dry_run_does_not_change_runtime_state() {
        let _lock = test_lock();
        apply(
            DaemonIngestControlAction::Resume,
            "reset".to_string(),
            false,
        );
        let response = apply(
            DaemonIngestControlAction::Pause,
            "preview".to_string(),
            true,
        );
        assert_eq!(response.state, DaemonIngestControlState::Paused);
        assert_eq!(current(), DaemonIngestControlState::Running);
    }

    #[test]
    fn pause_blocks_new_source_admission_until_resume() {
        let _lock = test_lock();
        apply(
            DaemonIngestControlAction::Resume,
            "reset".to_string(),
            false,
        );
        apply(
            DaemonIngestControlAction::Pause,
            "incident".to_string(),
            false,
        );
        let (sender, receiver) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            wait_for_source_admission();
            sender.send(()).expect("worker reports admission");
        });
        assert!(receiver.recv_timeout(Duration::from_millis(40)).is_err());
        apply(
            DaemonIngestControlAction::Resume,
            "resolved".to_string(),
            false,
        );
        receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("resume releases admission");
        worker.join().expect("worker joins");
    }
}
