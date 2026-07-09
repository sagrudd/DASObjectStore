//! Shared remote-upload backpressure policy.

use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteUploadBackpressurePolicy {
    pub max_s3_transfer_concurrency: u16,
    pub max_multipart_part_concurrency: u16,
    pub max_browser_handoff_files: u32,
    pub max_browser_handoff_bytes: u64,
    pub max_ssd_stage_queue_depth: u32,
    pub max_hdd_landing_queue_depth: u32,
    pub max_verification_queue_depth: u32,
    pub ssd_high_pressure_action: RemoteUploadBackpressureAction,
    pub ssd_critical_pressure_action: RemoteUploadBackpressureAction,
}

impl Default for RemoteUploadBackpressurePolicy {
    fn default() -> Self {
        Self {
            max_s3_transfer_concurrency: 2,
            max_multipart_part_concurrency: 2,
            max_browser_handoff_files: 100_000,
            max_browser_handoff_bytes: 8 * 1024 * 1024 * 1024 * 1024,
            max_ssd_stage_queue_depth: 4,
            max_hdd_landing_queue_depth: 8,
            max_verification_queue_depth: 4,
            ssd_high_pressure_action: RemoteUploadBackpressureAction::PauseNewTransfers,
            ssd_critical_pressure_action: RemoteUploadBackpressureAction::RejectNewTransfers,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteUploadBackpressureAction {
    Accept,
    PauseNewTransfers,
    RejectNewTransfers,
}

impl Display for RemoteUploadBackpressureAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Accept => "accept",
            Self::PauseNewTransfers => "pause_new_transfers",
            Self::RejectNewTransfers => "reject_new_transfers",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{RemoteUploadBackpressureAction, RemoteUploadBackpressurePolicy};

    #[test]
    fn default_remote_upload_backpressure_policy_is_bounded() {
        let policy = RemoteUploadBackpressurePolicy::default();

        assert_eq!(policy.max_s3_transfer_concurrency, 2);
        assert_eq!(policy.max_multipart_part_concurrency, 2);
        assert!(policy.max_browser_handoff_files > 0);
        assert!(policy.max_browser_handoff_bytes > 0);
        assert_eq!(policy.max_ssd_stage_queue_depth, 4);
        assert_eq!(policy.max_hdd_landing_queue_depth, 8);
        assert_eq!(policy.max_verification_queue_depth, 4);
        assert_eq!(
            policy.ssd_high_pressure_action,
            RemoteUploadBackpressureAction::PauseNewTransfers
        );
        assert_eq!(
            policy.ssd_critical_pressure_action,
            RemoteUploadBackpressureAction::RejectNewTransfers
        );
    }

    #[test]
    fn serializes_backpressure_actions_with_stable_names() {
        assert_eq!(
            serde_json::to_value(RemoteUploadBackpressureAction::PauseNewTransfers)
                .expect("action serializes"),
            serde_json::json!("pause_new_transfers")
        );
        assert_eq!(
            serde_json::to_value(RemoteUploadBackpressureAction::RejectNewTransfers)
                .expect("action serializes"),
            serde_json::json!("reject_new_transfers")
        );
    }
}
