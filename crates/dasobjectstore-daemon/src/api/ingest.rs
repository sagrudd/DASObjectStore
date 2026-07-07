use crate::api::health::DaemonSsdPressure;
use dasobjectstore_core::ids::{DiskId, IngestJobId, ObjectId, StoreId};
use dasobjectstore_core::lifecycle::IngestJobState;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};
use std::path::PathBuf;

mod backpressure;
mod resource;
mod scheduling;
mod telemetry;

pub use backpressure::{
    DaemonIngestErrorRate, DaemonSourceReadBackpressureAction,
    DaemonSourceReadBackpressureDecision, DaemonSourceReadBackpressureInput,
    DaemonSourceReadBackpressurePolicy, DaemonSourceReadBackpressureReason,
    DaemonSourceReadPriority,
};
pub use resource::{
    DaemonIngestResourcePolicy, DaemonIngestSystemSafetyReserve, DaemonIngestWorkerCounts,
};
pub use scheduling::{
    DaemonIngestAdaptiveSchedulerInput, DaemonIngestAdaptiveSchedulingLimit,
    DaemonIngestAdaptiveWorkerSchedule, DaemonIngestBoundedBufferPolicy,
    DaemonIngestBufferPoolPolicySet, DaemonIngestHddQueueState, DaemonIngestHddTargetQueue,
    DaemonIngestPlacementSchedulerInput, DaemonIngestSchedulingPolicy, DaemonIngestTargetCapacity,
    DaemonIngestTargetFailureState, DaemonSourceToSsdPriorityPolicy, DaemonSourceToSsdQueueUsage,
};
pub use telemetry::{
    DaemonIngestBottleneck, DaemonIngestCompletionFraction, DaemonIngestPipelinePressure,
    DaemonIngestPressure, DaemonIngestProgressFractions, DaemonIngestQueueDepths,
    DaemonIngestSystemTelemetry, DaemonIngestTelemetry, DaemonIngestThroughputTelemetry,
    DaemonIngestThroughputTrend, DaemonIngestWorkerActivity, DaemonIngestWorkerTelemetry,
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SubmitIngestFilesRequest {
    pub endpoint: StoreId,
    pub source_path: PathBuf,
    pub copies: Option<u8>,
    #[serde(default)]
    pub conflict_policy: DaemonIngestConflictPolicy,
    pub dry_run: bool,
    pub client_request_id: Option<String>,
}

impl SubmitIngestFilesRequest {
    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        if !self.source_path.is_absolute() {
            return Err(DaemonRequestValidationError::RelativeSourcePath {
                path: self.source_path.clone(),
            });
        }
        if self.copies == Some(0) {
            return Err(DaemonRequestValidationError::InvalidCopyCount { copies: 0 });
        }
        if self
            .client_request_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(DaemonRequestValidationError::BlankClientRequestId);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonIngestConflictPolicy {
    #[default]
    Strict,
    Lazy,
    Force,
}

impl DaemonIngestConflictPolicy {
    pub fn decide_existing_object(
        self,
        existing: &DaemonIngestObjectSnapshot,
        incoming: &DaemonIngestObjectSnapshot,
    ) -> DaemonIngestConflictDecision {
        match self {
            Self::Force => DaemonIngestConflictDecision::new(
                DaemonIngestConflictAction::IngestNewVersion,
                DaemonIngestConflictReason::ForceRequested,
            ),
            Self::Lazy if existing.size_bytes == incoming.size_bytes => {
                DaemonIngestConflictDecision::new(
                    DaemonIngestConflictAction::SkipExistingVersion,
                    DaemonIngestConflictReason::LazySizeMatch,
                )
            }
            Self::Lazy => DaemonIngestConflictDecision::new(
                DaemonIngestConflictAction::IngestNewVersion,
                DaemonIngestConflictReason::SizeMismatch,
            ),
            Self::Strict => strict_existing_object_decision(existing, incoming),
        }
    }
}

impl Display for DaemonIngestConflictPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Strict => "strict",
            Self::Lazy => "lazy",
            Self::Force => "force",
        })
    }
}

fn strict_existing_object_decision(
    existing: &DaemonIngestObjectSnapshot,
    incoming: &DaemonIngestObjectSnapshot,
) -> DaemonIngestConflictDecision {
    match (&existing.content_hash, &incoming.content_hash) {
        (Some(existing_hash), Some(incoming_hash)) if existing_hash == incoming_hash => {
            DaemonIngestConflictDecision::new(
                DaemonIngestConflictAction::SkipExistingVersion,
                DaemonIngestConflictReason::StrictChecksumMatch,
            )
        }
        (Some(_), Some(_)) => DaemonIngestConflictDecision::new(
            DaemonIngestConflictAction::IngestNewVersion,
            DaemonIngestConflictReason::ChecksumMismatch,
        ),
        _ => DaemonIngestConflictDecision::new(
            DaemonIngestConflictAction::IngestNewVersion,
            DaemonIngestConflictReason::ChecksumUnavailable,
        ),
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestObjectSnapshot {
    pub size_bytes: u64,
    pub content_hash: Option<String>,
}

impl DaemonIngestObjectSnapshot {
    pub fn new(size_bytes: u64, content_hash: Option<impl Into<String>>) -> Self {
        Self {
            size_bytes,
            content_hash: content_hash.map(Into::into),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonIngestConflictAction {
    SkipExistingVersion,
    IngestNewVersion,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonIngestConflictReason {
    ForceRequested,
    LazySizeMatch,
    StrictChecksumMatch,
    SizeMismatch,
    ChecksumMismatch,
    ChecksumUnavailable,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestConflictDecision {
    pub action: DaemonIngestConflictAction,
    pub reason: DaemonIngestConflictReason,
    pub preserves_existing_version: bool,
}

impl DaemonIngestConflictDecision {
    fn new(action: DaemonIngestConflictAction, reason: DaemonIngestConflictReason) -> Self {
        Self {
            action,
            reason,
            preserves_existing_version: true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SubmitIngestFilesResponse {
    pub job_id: IngestJobId,
    pub accepted_at_utc: String,
    pub dry_run: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IngestJobStatusRequest {
    pub job_id: IngestJobId,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IngestJobStatusResponse {
    pub job_id: IngestJobId,
    pub endpoint: StoreId,
    pub state: IngestJobState,
    pub progress: DaemonIngestProgressEvent,
    pub updated_at_utc: String,
    pub failure_message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CancelIngestJobRequest {
    pub job_id: IngestJobId,
    pub reason: Option<String>,
}

impl CancelIngestJobRequest {
    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        if self
            .reason
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(DaemonRequestValidationError::BlankCancellationReason);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CancelIngestJobResponse {
    pub job_id: IngestJobId,
    pub accepted: bool,
    pub state: IngestJobState,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestProgressEvent {
    pub job_id: IngestJobId,
    pub endpoint: StoreId,
    pub stage: DaemonIngestStage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pipeline_stage: Option<DaemonIngestPipelineStage>,
    pub work_bytes_done: u64,
    pub work_bytes_total: Option<u64>,
    pub files_done: u64,
    pub files_total: Option<u64>,
    pub current_object_id: Option<ObjectId>,
    pub ssd_pressure: Option<DaemonSsdPressure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telemetry: Option<DaemonIngestTelemetry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_policy: Option<DaemonIngestResourcePolicy>,
    pub message: Option<String>,
}

impl DaemonIngestProgressEvent {
    pub fn percent_complete(&self) -> Option<u8> {
        let total = self.work_bytes_total?;
        if total == 0 {
            return Some(100);
        }
        Some(((self.work_bytes_done.saturating_mul(100)) / total).min(100) as u8)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum DaemonIngestStage {
    Queued,
    SsdIngest,
    HddCopy { disk_id: DiskId, copy_number: u8 },
    Complete,
    Failed,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonIngestPipelineStage {
    Scan,
    SourceRead,
    SsdStage,
    ChecksumManifestCapture,
    HddPlacement,
    HddWrite,
    Verification,
    Finalization,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DaemonRequestValidationError {
    RelativeSourcePath { path: PathBuf },
    InvalidCopyCount { copies: u8 },
    BlankClientRequestId,
    BlankCancellationReason,
    BlankField { field: &'static str },
    UnsafeLocalName { field: &'static str, value: String },
    BlankConfirmationMarker,
    UnsupportedServiceProvider { provider: String },
}

impl Display for DaemonRequestValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RelativeSourcePath { path } => {
                write!(
                    formatter,
                    "ingest source path must be absolute: {}",
                    path.display()
                )
            }
            Self::InvalidCopyCount { copies } => {
                write!(formatter, "copy count must be greater than zero: {copies}")
            }
            Self::BlankClientRequestId => {
                formatter.write_str("client_request_id must not be blank")
            }
            Self::BlankCancellationReason => {
                formatter.write_str("cancellation reason must not be blank")
            }
            Self::BlankField { field } => write!(formatter, "{field} must not be blank"),
            Self::UnsafeLocalName { field, value } => write!(
                formatter,
                "{field} must be a conservative POSIX-style local name: {value}"
            ),
            Self::BlankConfirmationMarker => {
                formatter.write_str("confirmation_marker must not be blank")
            }
            Self::UnsupportedServiceProvider { provider } => write!(
                formatter,
                "unsupported object service provider for daemon lifecycle operation: {provider}"
            ),
        }
    }
}

impl std::error::Error for DaemonRequestValidationError {}

#[cfg(test)]
mod tests {
    use super::{
        CancelIngestJobRequest, DaemonIngestAdaptiveSchedulerInput,
        DaemonIngestAdaptiveSchedulingLimit, DaemonIngestBottleneck,
        DaemonIngestCompletionFraction, DaemonIngestConflictAction, DaemonIngestConflictPolicy,
        DaemonIngestConflictReason, DaemonIngestErrorRate, DaemonIngestHddQueueState,
        DaemonIngestHddTargetQueue, DaemonIngestObjectSnapshot, DaemonIngestPipelinePressure,
        DaemonIngestPipelineStage, DaemonIngestPlacementSchedulerInput, DaemonIngestPressure,
        DaemonIngestProgressEvent, DaemonIngestResourcePolicy, DaemonIngestSchedulingPolicy,
        DaemonIngestStage, DaemonIngestSystemSafetyReserve, DaemonIngestSystemTelemetry,
        DaemonIngestTargetCapacity, DaemonIngestTargetFailureState, DaemonIngestTelemetry,
        DaemonIngestThroughputTrend, DaemonIngestWorkerCounts, DaemonRequestValidationError,
        DaemonSourceReadBackpressureInput, DaemonSourceReadBackpressurePolicy,
        DaemonSourceReadBackpressureReason, DaemonSourceReadPriority, DaemonSourceToSsdQueueUsage,
        SubmitIngestFilesRequest,
    };
    use crate::api::health::DaemonSsdPressure;
    use dasobjectstore_core::ids::{DiskId, IngestJobId, ObjectId, StoreId};
    use dasobjectstore_core::lifecycle::HealthState;

    #[test]
    fn validates_absolute_ingest_submission() {
        let request = SubmitIngestFilesRequest {
            endpoint: StoreId::new("zymo").expect("store id"),
            source_path: "/mnt/external/zymo".into(),
            copies: Some(1),
            conflict_policy: DaemonIngestConflictPolicy::Strict,
            dry_run: false,
            client_request_id: Some("request-a".to_string()),
        };

        request.validate().expect("request is valid");
    }

    #[test]
    fn rejects_relative_ingest_source_path() {
        let request = SubmitIngestFilesRequest {
            endpoint: StoreId::new("zymo").expect("store id"),
            source_path: "relative/source".into(),
            copies: Some(1),
            conflict_policy: DaemonIngestConflictPolicy::Strict,
            dry_run: false,
            client_request_id: None,
        };

        let err = request.validate().expect_err("relative source rejected");

        assert_eq!(
            err,
            DaemonRequestValidationError::RelativeSourcePath {
                path: "relative/source".into()
            }
        );
    }

    #[test]
    fn rejects_zero_copy_override() {
        let request = SubmitIngestFilesRequest {
            endpoint: StoreId::new("zymo").expect("store id"),
            source_path: "/mnt/external/zymo".into(),
            copies: Some(0),
            conflict_policy: DaemonIngestConflictPolicy::Strict,
            dry_run: false,
            client_request_id: None,
        };

        let err = request.validate().expect_err("zero copies rejected");

        assert_eq!(
            err,
            DaemonRequestValidationError::InvalidCopyCount { copies: 0 }
        );
    }

    #[test]
    fn submit_ingest_defaults_legacy_conflict_policy_to_strict() {
        let encoded = serde_json::json!({
            "endpoint": "zymo",
            "source_path": "/mnt/external/zymo",
            "copies": null,
            "dry_run": false,
            "client_request_id": null
        });

        let request: SubmitIngestFilesRequest =
            serde_json::from_value(encoded).expect("legacy request deserializes");

        assert_eq!(request.conflict_policy, DaemonIngestConflictPolicy::Strict);
    }

    #[test]
    fn conflict_policy_serializes_as_stable_snake_case() {
        let request = SubmitIngestFilesRequest {
            endpoint: StoreId::new("zymo").expect("store id"),
            source_path: "/mnt/external/zymo".into(),
            copies: None,
            conflict_policy: DaemonIngestConflictPolicy::Lazy,
            dry_run: true,
            client_request_id: None,
        };

        let encoded = serde_json::to_value(request).expect("request serializes");

        assert_eq!(encoded["conflict_policy"], "lazy");
    }

    #[test]
    fn strict_conflict_policy_skips_only_checksum_matches() {
        let existing = DaemonIngestObjectSnapshot::new(4, Some("sha256:abc"));
        let same = DaemonIngestObjectSnapshot::new(4, Some("sha256:abc"));
        let changed = DaemonIngestObjectSnapshot::new(4, Some("sha256:def"));
        let unhashed = DaemonIngestObjectSnapshot::new(4, None::<String>);

        assert_eq!(
            DaemonIngestConflictPolicy::Strict
                .decide_existing_object(&existing, &same)
                .action,
            DaemonIngestConflictAction::SkipExistingVersion
        );
        assert_eq!(
            DaemonIngestConflictPolicy::Strict
                .decide_existing_object(&existing, &changed)
                .reason,
            DaemonIngestConflictReason::ChecksumMismatch
        );
        assert_eq!(
            DaemonIngestConflictPolicy::Strict
                .decide_existing_object(&existing, &unhashed)
                .reason,
            DaemonIngestConflictReason::ChecksumUnavailable
        );
    }

    #[test]
    fn lazy_and_force_conflict_policies_preserve_existing_versions() {
        let existing = DaemonIngestObjectSnapshot::new(1024, Some("sha256:abc"));
        let same_size = DaemonIngestObjectSnapshot::new(1024, Some("sha256:def"));
        let different_size = DaemonIngestObjectSnapshot::new(2048, Some("sha256:def"));

        let lazy_skip =
            DaemonIngestConflictPolicy::Lazy.decide_existing_object(&existing, &same_size);
        let lazy_ingest =
            DaemonIngestConflictPolicy::Lazy.decide_existing_object(&existing, &different_size);
        let forced =
            DaemonIngestConflictPolicy::Force.decide_existing_object(&existing, &same_size);

        assert_eq!(
            lazy_skip.action,
            DaemonIngestConflictAction::SkipExistingVersion
        );
        assert_eq!(lazy_skip.reason, DaemonIngestConflictReason::LazySizeMatch);
        assert_eq!(
            lazy_ingest.action,
            DaemonIngestConflictAction::IngestNewVersion
        );
        assert_eq!(forced.reason, DaemonIngestConflictReason::ForceRequested);
        assert!(forced.preserves_existing_version);
    }

    #[test]
    fn rejects_blank_cancellation_reason() {
        let request = CancelIngestJobRequest {
            job_id: IngestJobId::new("job-a").expect("job id"),
            reason: Some(" ".to_string()),
        };

        let err = request.validate().expect_err("blank reason rejected");

        assert_eq!(err, DaemonRequestValidationError::BlankCancellationReason);
    }

    #[test]
    fn progress_event_reports_bounded_percent() {
        let event = DaemonIngestProgressEvent {
            job_id: IngestJobId::new("job-a").expect("job id"),
            endpoint: StoreId::new("zymo").expect("store id"),
            stage: DaemonIngestStage::HddCopy {
                disk_id: DiskId::new("qnap-1062").expect("disk id"),
                copy_number: 1,
            },
            pipeline_stage: Some(DaemonIngestPipelineStage::HddWrite),
            work_bytes_done: 150,
            work_bytes_total: Some(100),
            files_done: 1,
            files_total: Some(1),
            current_object_id: Some(ObjectId::new("zymo/sample.fastq.gz").expect("object id")),
            ssd_pressure: Some(DaemonSsdPressure::AcceptingWrites),
            telemetry: None,
            resource_policy: None,
            message: None,
        };

        assert_eq!(event.percent_complete(), Some(100));
    }

    #[test]
    fn parallel_ingest_telemetry_uses_stable_snake_case() {
        let telemetry = DaemonIngestTelemetry {
            bottleneck: DaemonIngestBottleneck::HddWrite,
            throughput: super::DaemonIngestThroughputTelemetry {
                trend: DaemonIngestThroughputTrend::Up,
                ..super::DaemonIngestThroughputTelemetry::default()
            },
            pressure: super::DaemonIngestPipelinePressure {
                ssd: DaemonSsdPressure::High,
                hdd: DaemonIngestPressure::Elevated,
                verification: DaemonIngestPressure::Critical,
            },
            ..DaemonIngestTelemetry::default()
        };
        let encoded = serde_json::to_value(telemetry).expect("telemetry serializes");

        assert_eq!(encoded["bottleneck"], "hdd_write");
        assert_eq!(encoded["throughput"]["trend"], "up");
        assert_eq!(encoded["pressure"]["ssd"], "high");
        assert_eq!(encoded["pressure"]["hdd"], "elevated");
        assert_eq!(encoded["pressure"]["verification"], "critical");
        assert_eq!(
            serde_json::to_value(DaemonIngestPipelineStage::ChecksumManifestCapture)
                .expect("stage serializes"),
            "checksum_manifest_capture"
        );
    }

    #[test]
    fn progress_event_deserializes_without_new_telemetry_fields() {
        let encoded = serde_json::json!({
            "job_id": "job-a",
            "endpoint": "zymo",
            "stage": { "kind": "queued" },
            "work_bytes_done": 0,
            "work_bytes_total": null,
            "files_done": 0,
            "files_total": null,
            "current_object_id": null,
            "ssd_pressure": null,
            "message": null
        });

        let event: DaemonIngestProgressEvent =
            serde_json::from_value(encoded).expect("legacy progress event deserializes");

        assert_eq!(event.pipeline_stage, None);
        assert_eq!(event.telemetry, None);
        assert_eq!(event.resource_policy, None);
    }

    #[test]
    fn telemetry_percent_and_fraction_helpers_are_bounded() {
        let system = DaemonIngestSystemTelemetry {
            cpu_percent: 150,
            memory_used_bytes: 150,
            memory_budget_bytes: Some(100),
        };
        let fraction = DaemonIngestCompletionFraction {
            done: 125,
            total: Some(100),
        };

        assert_eq!(system.bounded_cpu_percent(), 100);
        assert_eq!(system.memory_percent(), Some(100));
        assert_eq!(fraction.percent_complete(), Some(100));
    }

    #[test]
    fn default_resource_policy_has_safe_nonzero_limits() {
        let policy = DaemonIngestResourcePolicy::default();

        assert!(policy.worker_counts.scan > 0);
        assert!(policy.worker_counts.source_read > 0);
        assert!(policy.worker_counts.ssd_stage > 0);
        assert!(policy.worker_counts.checksum_manifest > 0);
        assert!(policy.worker_counts.hdd_placement > 0);
        assert!(policy.worker_counts.hdd_write > 0);
        assert!(policy.worker_counts.verification > 0);
        assert!(policy.worker_counts.finalization > 0);
        assert!(policy.memory_budget_bytes > 0);
        assert!(policy.ssd_reserve_bytes > 0);
        assert!(policy.hdd_queue_depth > 0);
        assert_eq!(
            policy.verification_parallelism,
            policy.worker_counts.verification
        );
        assert!(policy.system_safety_reserve.memory_bytes > 0);
    }

    #[test]
    fn default_scheduling_policy_has_bounded_buffer_pools() {
        let policy = DaemonIngestSchedulingPolicy::default();

        assert_eq!(
            policy.source_to_ssd.priority,
            DaemonSourceReadPriority::SourceToSsdFirst
        );
        assert_eq!(
            policy.source_to_ssd.max_source_read_queue_depth,
            policy.buffer_pools.read.queue_depth
        );
        assert_eq!(
            policy.source_to_ssd.max_ssd_stage_queue_depth,
            policy.buffer_pools.write.queue_depth
        );
        assert_eq!(
            policy.source_to_ssd.max_in_flight_bytes,
            policy
                .buffer_pools
                .read
                .maximum_pool_bytes()
                .saturating_add(policy.buffer_pools.write.maximum_pool_bytes())
        );
        assert!(policy.source_to_ssd.has_bounded_pressure_controls());
        assert!(policy.buffer_pools.read.has_bounded_capacity());
        assert!(policy.buffer_pools.write.has_bounded_capacity());
        assert!(policy.buffer_pools.verify.has_bounded_capacity());
        assert!(
            policy
                .source_read_backpressure
                .throttle_error_rate_per_minute
                < policy.source_read_backpressure.block_error_rate_per_minute
        );
    }

    #[test]
    fn scheduling_policy_deserializes_without_source_to_ssd_field() {
        let encoded = serde_json::json!({
            "source_read_backpressure": {
                "throttle_error_rate_per_minute": 3,
                "block_error_rate_per_minute": 10
            },
            "buffer_pools": {
                "read": {
                    "queue_depth": 32,
                    "buffer_bytes": 4194304,
                    "pool_buffers": 32,
                    "memory_limit_bytes": 134217728
                },
                "write": {
                    "queue_depth": 64,
                    "buffer_bytes": 4194304,
                    "pool_buffers": 64,
                    "memory_limit_bytes": 268435456
                },
                "verify": {
                    "queue_depth": 32,
                    "buffer_bytes": 2097152,
                    "pool_buffers": 32,
                    "memory_limit_bytes": 67108864
                }
            }
        });

        let policy: DaemonIngestSchedulingPolicy =
            serde_json::from_value(encoded).expect("legacy policy deserializes");

        assert_eq!(
            policy.source_to_ssd.priority,
            DaemonSourceReadPriority::SourceToSsdFirst
        );
        assert!(policy.source_to_ssd.has_bounded_pressure_controls());
    }

    #[test]
    fn source_to_ssd_queue_pressure_respects_bounded_limits() {
        let policy = DaemonIngestSchedulingPolicy::default().source_to_ssd;

        assert_eq!(
            policy.queue_pressure(DaemonSourceToSsdQueueUsage::default()),
            DaemonIngestPressure::Normal
        );
        assert_eq!(
            policy.queue_pressure(DaemonSourceToSsdQueueUsage {
                source_read_queue_depth: 1,
                ..DaemonSourceToSsdQueueUsage::default()
            }),
            DaemonIngestPressure::Elevated
        );
        assert_eq!(
            policy.queue_pressure(DaemonSourceToSsdQueueUsage {
                source_read_queue_depth: policy.max_source_read_queue_depth * 3 / 4,
                ..DaemonSourceToSsdQueueUsage::default()
            }),
            DaemonIngestPressure::High
        );
        assert_eq!(
            policy.queue_pressure(DaemonSourceToSsdQueueUsage {
                ssd_stage_queue_depth: policy.max_ssd_stage_queue_depth,
                ..DaemonSourceToSsdQueueUsage::default()
            }),
            DaemonIngestPressure::Critical
        );
    }

    #[test]
    fn adaptive_scheduler_keeps_cpu_bound_workers_within_available_cores() {
        let input = DaemonIngestAdaptiveSchedulerInput {
            available_cpu_cores: 12,
            resource_policy: adaptive_resource_policy(),
            telemetry: DaemonIngestTelemetry::default(),
            hdd_targets: vec![hdd_target("disk-a", DaemonIngestPressure::Normal)],
        };

        let schedule = input.schedule();
        let cpu_bound_workers = schedule
            .worker_counts
            .scan
            .saturating_add(schedule.worker_counts.checksum_manifest)
            .saturating_add(schedule.worker_counts.hdd_placement)
            .saturating_add(schedule.worker_counts.verification)
            .saturating_add(schedule.worker_counts.finalization);

        assert_eq!(schedule.effective_cpu_cores, 10);
        assert!(cpu_bound_workers <= schedule.effective_cpu_cores);
        assert!(schedule.worker_counts.checksum_manifest > 0);
        assert!(schedule.worker_counts.verification > 0);
        assert_eq!(
            schedule.limiting_factor,
            DaemonIngestAdaptiveSchedulingLimit::CpuReserve
        );
    }

    #[test]
    fn adaptive_scheduler_biases_cpu_workers_toward_verification_backlog() {
        let input = DaemonIngestAdaptiveSchedulerInput {
            available_cpu_cores: 12,
            resource_policy: adaptive_resource_policy(),
            telemetry: DaemonIngestTelemetry {
                pressure: DaemonIngestPipelinePressure {
                    verification: DaemonIngestPressure::High,
                    ..DaemonIngestPipelinePressure::default()
                },
                ..DaemonIngestTelemetry::default()
            },
            hdd_targets: vec![hdd_target("disk-a", DaemonIngestPressure::Normal)],
        };

        let schedule = input.schedule();

        assert!(schedule.worker_counts.verification > schedule.worker_counts.checksum_manifest);
        assert_eq!(
            schedule.verification_parallelism,
            schedule.worker_counts.verification
        );
        assert_eq!(
            schedule.limiting_factor,
            DaemonIngestAdaptiveSchedulingLimit::VerificationBacklog
        );
    }

    #[test]
    fn adaptive_scheduler_throttles_disk_workers_for_saturated_hdds() {
        let input = DaemonIngestAdaptiveSchedulerInput {
            available_cpu_cores: 12,
            resource_policy: adaptive_resource_policy(),
            telemetry: DaemonIngestTelemetry::default(),
            hdd_targets: vec![
                hdd_target("disk-a", DaemonIngestPressure::High),
                hdd_target("disk-b", DaemonIngestPressure::Normal),
            ],
        };

        let schedule = input.schedule();

        assert_eq!(schedule.worker_counts.source_read, 1);
        assert_eq!(schedule.worker_counts.ssd_stage, 1);
        assert_eq!(schedule.worker_counts.hdd_write, 1);
        assert_eq!(schedule.hdd_queue_depth, 1);
        assert_eq!(
            schedule.limiting_factor,
            DaemonIngestAdaptiveSchedulingLimit::HddPressure
        );
    }

    #[test]
    fn adaptive_scheduler_stops_hdd_writes_when_all_targets_are_full() {
        let full_target = DaemonIngestHddTargetQueue {
            queue: DaemonIngestHddQueueState {
                queue_depth: 8,
                max_queue_depth: 8,
                queued_bytes: 512,
            },
            ..hdd_target("disk-a", DaemonIngestPressure::Normal)
        };
        let input = DaemonIngestAdaptiveSchedulerInput {
            available_cpu_cores: 12,
            resource_policy: adaptive_resource_policy(),
            telemetry: DaemonIngestTelemetry::default(),
            hdd_targets: vec![full_target],
        };

        let schedule = input.schedule();

        assert_eq!(schedule.worker_counts.hdd_write, 0);
        assert_eq!(schedule.hdd_queue_depth, 0);
        assert_eq!(
            schedule.limiting_factor,
            DaemonIngestAdaptiveSchedulingLimit::HddTargetSaturation
        );
    }

    #[test]
    fn adaptive_scheduler_keeps_writing_to_remaining_eligible_hdd_targets() {
        let full_target = DaemonIngestHddTargetQueue {
            queue: DaemonIngestHddQueueState {
                queue_depth: 8,
                max_queue_depth: 8,
                queued_bytes: 512,
            },
            ..hdd_target("disk-a", DaemonIngestPressure::Normal)
        };
        let input = DaemonIngestAdaptiveSchedulerInput {
            available_cpu_cores: 12,
            resource_policy: adaptive_resource_policy(),
            telemetry: DaemonIngestTelemetry::default(),
            hdd_targets: vec![
                full_target,
                hdd_target("disk-b", DaemonIngestPressure::Normal),
            ],
        };

        let schedule = input.schedule();

        assert!(schedule.worker_counts.hdd_write > 0);
        assert!(schedule.hdd_queue_depth > 0);
        assert_ne!(
            schedule.limiting_factor,
            DaemonIngestAdaptiveSchedulingLimit::HddTargetSaturation
        );
    }

    #[test]
    fn scheduling_dtos_use_stable_snake_case() {
        let decision = DaemonSourceReadBackpressureInput {
            priority: DaemonSourceReadPriority::SourceToSsdFirst,
            ssd_pressure: DaemonSsdPressure::High,
            ..DaemonSourceReadBackpressureInput::default()
        }
        .classify(&DaemonSourceReadBackpressurePolicy::default());
        let target_state = DaemonIngestTargetFailureState::Suspended;
        let policy = DaemonIngestSchedulingPolicy::default();
        let adaptive_limit = DaemonIngestAdaptiveSchedulingLimit::HddTargetSaturation;

        let decision = serde_json::to_value(decision).expect("decision serializes");
        let target_state = serde_json::to_value(target_state).expect("state serializes");
        let policy = serde_json::to_value(policy).expect("policy serializes");
        let adaptive_limit =
            serde_json::to_value(adaptive_limit).expect("adaptive limit serializes");

        assert_eq!(decision["action"], "throttle");
        assert_eq!(decision["reason"], "ssd_pressure");
        assert_eq!(decision["priority"], "source_to_ssd_first");
        assert_eq!(target_state, "suspended");
        assert_eq!(policy["source_to_ssd"]["priority"], "source_to_ssd_first");
        assert_eq!(adaptive_limit, "hdd_target_saturation");
    }

    #[test]
    fn source_reads_run_when_pressure_is_normal() {
        let decision = DaemonSourceReadBackpressureInput::default()
            .classify(&DaemonSourceReadBackpressurePolicy::default());

        assert!(decision.should_run());
        assert_eq!(decision.reason, DaemonSourceReadBackpressureReason::None);
    }

    #[test]
    fn source_reads_throttle_for_high_ssd_pressure() {
        let decision = DaemonSourceReadBackpressureInput {
            ssd_pressure: DaemonSsdPressure::High,
            ..DaemonSourceReadBackpressureInput::default()
        }
        .classify(&DaemonSourceReadBackpressurePolicy::default());

        assert!(decision.should_throttle());
        assert_eq!(
            decision.reason,
            DaemonSourceReadBackpressureReason::SsdPressure
        );
    }

    #[test]
    fn source_reads_block_for_critical_ram_pressure() {
        let decision = DaemonSourceReadBackpressureInput {
            ram_pressure: DaemonIngestPressure::Critical,
            ..DaemonSourceReadBackpressureInput::default()
        }
        .classify(&DaemonSourceReadBackpressurePolicy::default());

        assert!(decision.should_block());
        assert_eq!(
            decision.reason,
            DaemonSourceReadBackpressureReason::RamPressure
        );
    }

    #[test]
    fn source_to_ssd_priority_tolerates_elevated_hdd_backlog() {
        let decision = DaemonSourceReadBackpressureInput {
            priority: DaemonSourceReadPriority::SourceToSsdFirst,
            hdd_backlog: DaemonIngestPressure::Elevated,
            ..DaemonSourceReadBackpressureInput::default()
        }
        .classify(&DaemonSourceReadBackpressurePolicy::default());

        assert!(decision.should_run());
    }

    #[test]
    fn best_effort_source_reads_throttle_for_elevated_verification_backlog() {
        let decision = DaemonSourceReadBackpressureInput {
            priority: DaemonSourceReadPriority::BestEffort,
            verification_backlog: DaemonIngestPressure::Elevated,
            ..DaemonSourceReadBackpressureInput::default()
        }
        .classify(&DaemonSourceReadBackpressurePolicy::default());

        assert!(decision.should_throttle());
        assert_eq!(
            decision.reason,
            DaemonSourceReadBackpressureReason::VerificationBacklog
        );
    }

    #[test]
    fn source_reads_block_for_error_rate_above_policy() {
        let decision = DaemonSourceReadBackpressureInput {
            error_rate: DaemonIngestErrorRate {
                errors: 5,
                window_seconds: 30,
            },
            ..DaemonSourceReadBackpressureInput::default()
        }
        .classify(&DaemonSourceReadBackpressurePolicy::default());

        assert!(decision.should_block());
        assert_eq!(
            decision.reason,
            DaemonSourceReadBackpressureReason::ErrorRate
        );
    }

    #[test]
    fn placement_scheduler_counts_only_eligible_targets() {
        let target = DaemonIngestHddTargetQueue {
            target_id: "pool-a/disk-a".to_string(),
            disk_id: DiskId::new("disk-a").expect("disk id"),
            capacity: DaemonIngestTargetCapacity {
                total_bytes: 1_000,
                available_bytes: 500,
                reserved_bytes: 100,
            },
            queue: DaemonIngestHddQueueState {
                queue_depth: 2,
                queued_bytes: 128,
                max_queue_depth: 8,
            },
            write_throughput_bytes_per_second: 200,
            health: HealthState::Healthy,
            pressure: DaemonIngestPressure::Normal,
            failure_state: DaemonIngestTargetFailureState::Available,
        };
        let blocked_target = DaemonIngestHddTargetQueue {
            target_id: "pool-a/disk-b".to_string(),
            disk_id: DiskId::new("disk-b").expect("disk id"),
            pressure: DaemonIngestPressure::Critical,
            ..target.clone()
        };
        let input = DaemonIngestPlacementSchedulerInput {
            required_bytes: 400,
            copies: 1,
            targets: vec![target, blocked_target],
        };

        assert!(input.has_enough_eligible_targets());
        assert_eq!(input.eligible_targets().count(), 1);
    }

    fn adaptive_resource_policy() -> DaemonIngestResourcePolicy {
        DaemonIngestResourcePolicy {
            worker_counts: DaemonIngestWorkerCounts {
                scan: 2,
                source_read: 4,
                ssd_stage: 4,
                checksum_manifest: 8,
                hdd_placement: 2,
                hdd_write: 8,
                verification: 8,
                finalization: 2,
            },
            memory_budget_bytes: 4 * 1024 * 1024 * 1024,
            ssd_reserve_bytes: 10 * 1024 * 1024 * 1024,
            hdd_queue_depth: 64,
            verification_parallelism: 8,
            system_safety_reserve: DaemonIngestSystemSafetyReserve {
                cpu_cores: 2,
                memory_bytes: 1024 * 1024 * 1024,
            },
        }
    }

    fn hdd_target(id: &str, pressure: DaemonIngestPressure) -> DaemonIngestHddTargetQueue {
        DaemonIngestHddTargetQueue {
            target_id: format!("pool-a/{id}"),
            disk_id: DiskId::new(id).expect("disk id"),
            capacity: DaemonIngestTargetCapacity {
                total_bytes: 10_000,
                available_bytes: 5_000,
                reserved_bytes: 1_000,
            },
            queue: DaemonIngestHddQueueState {
                queue_depth: 1,
                queued_bytes: 128,
                max_queue_depth: 8,
            },
            write_throughput_bytes_per_second: 200,
            health: HealthState::Healthy,
            pressure,
            failure_state: DaemonIngestTargetFailureState::Available,
        }
    }
}
