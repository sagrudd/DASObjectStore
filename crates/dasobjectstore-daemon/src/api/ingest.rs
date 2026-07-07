use crate::api::health::DaemonSsdPressure;
use dasobjectstore_core::ids::{DiskId, IngestJobId, ObjectId, StoreId};
use dasobjectstore_core::lifecycle::IngestJobState;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SubmitIngestFilesRequest {
    pub endpoint: StoreId,
    pub source_path: PathBuf,
    pub copies: Option<u8>,
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestResourcePolicy {
    pub worker_counts: DaemonIngestWorkerCounts,
    pub memory_budget_bytes: u64,
    pub ssd_reserve_bytes: u64,
    pub hdd_queue_depth: u32,
    pub verification_parallelism: u16,
    pub system_safety_reserve: DaemonIngestSystemSafetyReserve,
}

impl Default for DaemonIngestResourcePolicy {
    fn default() -> Self {
        let worker_counts = DaemonIngestWorkerCounts::default();

        Self {
            worker_counts,
            memory_budget_bytes: 1024 * 1024 * 1024,
            ssd_reserve_bytes: 10 * 1024 * 1024 * 1024,
            hdd_queue_depth: 64,
            verification_parallelism: worker_counts.verification,
            system_safety_reserve: DaemonIngestSystemSafetyReserve::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestWorkerCounts {
    pub scan: u16,
    pub source_read: u16,
    pub ssd_stage: u16,
    pub checksum_manifest: u16,
    pub hdd_placement: u16,
    pub hdd_write: u16,
    pub verification: u16,
    pub finalization: u16,
}

impl Default for DaemonIngestWorkerCounts {
    fn default() -> Self {
        let cores = std::thread::available_parallelism()
            .map(|cores| cores.get().min(u16::MAX as usize) as u16)
            .unwrap_or(1)
            .max(1);
        let coordination_workers = 1;
        let disk_workers = cores.clamp(1, 8);
        let cpu_workers = cores.saturating_sub(1).max(1).min(8);

        Self {
            scan: coordination_workers,
            source_read: disk_workers.min(4),
            ssd_stage: disk_workers.min(4),
            checksum_manifest: cpu_workers,
            hdd_placement: coordination_workers,
            hdd_write: disk_workers,
            verification: cpu_workers,
            finalization: coordination_workers,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestSystemSafetyReserve {
    pub cpu_cores: u16,
    pub memory_bytes: u64,
}

impl Default for DaemonIngestSystemSafetyReserve {
    fn default() -> Self {
        let cpu_cores = std::thread::available_parallelism()
            .map(|cores| u16::from(cores.get() > 2))
            .unwrap_or(0);

        Self {
            cpu_cores,
            memory_bytes: 512 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestTelemetry {
    pub queue_depths: DaemonIngestQueueDepths,
    pub workers: DaemonIngestWorkerTelemetry,
    pub system: DaemonIngestSystemTelemetry,
    pub bottleneck: DaemonIngestBottleneck,
    pub throughput: DaemonIngestThroughputTelemetry,
    pub progress_fractions: DaemonIngestProgressFractions,
    pub pressure: DaemonIngestPipelinePressure,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestQueueDepths {
    pub scan: u32,
    pub source_read: u32,
    pub ssd_stage: u32,
    pub hdd_write: u32,
    pub verification: u32,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestWorkerTelemetry {
    pub scan: DaemonIngestWorkerActivity,
    pub source_read: DaemonIngestWorkerActivity,
    pub ssd_stage: DaemonIngestWorkerActivity,
    pub hdd_write: DaemonIngestWorkerActivity,
    pub verification: DaemonIngestWorkerActivity,
    pub finalization: DaemonIngestWorkerActivity,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestWorkerActivity {
    pub active: u16,
    pub idle: u16,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestSystemTelemetry {
    pub cpu_percent: u16,
    pub memory_used_bytes: u64,
    pub memory_budget_bytes: Option<u64>,
}

impl DaemonIngestSystemTelemetry {
    pub fn bounded_cpu_percent(&self) -> u8 {
        self.cpu_percent.min(100) as u8
    }

    pub fn memory_percent(&self) -> Option<u8> {
        let total = self.memory_budget_bytes?;
        if total == 0 {
            return Some(100);
        }

        Some(((self.memory_used_bytes.saturating_mul(100)) / total).min(100) as u8)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonIngestBottleneck {
    None,
    Scan,
    SourceRead,
    SsdStage,
    ChecksumManifest,
    HddPlacement,
    HddWrite,
    Verification,
    Cpu,
    Memory,
    SsdPressure,
    HddPressure,
    VerificationBacklog,
}

impl Default for DaemonIngestBottleneck {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestThroughputTelemetry {
    pub current_bytes_per_second: u64,
    pub moving_average_bytes_per_second: u64,
    pub recent_high_bytes_per_second: u64,
    pub recent_low_bytes_per_second: u64,
    pub trend: DaemonIngestThroughputTrend,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonIngestThroughputTrend {
    Up,
    Down,
    Flat,
}

impl Default for DaemonIngestThroughputTrend {
    fn default() -> Self {
        Self::Flat
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestProgressFractions {
    pub staged_bytes: DaemonIngestCompletionFraction,
    pub staged_files: DaemonIngestCompletionFraction,
    pub written_bytes: DaemonIngestCompletionFraction,
    pub written_files: DaemonIngestCompletionFraction,
    pub verified_bytes: DaemonIngestCompletionFraction,
    pub verified_files: DaemonIngestCompletionFraction,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestCompletionFraction {
    pub done: u64,
    pub total: Option<u64>,
}

impl DaemonIngestCompletionFraction {
    pub fn percent_complete(&self) -> Option<u8> {
        let total = self.total?;
        if total == 0 {
            return Some(100);
        }

        Some(((self.done.saturating_mul(100)) / total).min(100) as u8)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestPipelinePressure {
    pub ssd: DaemonSsdPressure,
    pub hdd: DaemonIngestPressure,
    pub verification: DaemonIngestPressure,
}

impl Default for DaemonIngestPipelinePressure {
    fn default() -> Self {
        Self {
            ssd: DaemonSsdPressure::AcceptingWrites,
            hdd: DaemonIngestPressure::Normal,
            verification: DaemonIngestPressure::Normal,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonIngestPressure {
    Normal,
    Elevated,
    High,
    Critical,
}

impl Default for DaemonIngestPressure {
    fn default() -> Self {
        Self::Normal
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DaemonRequestValidationError {
    RelativeSourcePath { path: PathBuf },
    InvalidCopyCount { copies: u8 },
    BlankClientRequestId,
    BlankCancellationReason,
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
        CancelIngestJobRequest, DaemonIngestBottleneck, DaemonIngestCompletionFraction,
        DaemonIngestPipelineStage, DaemonIngestPressure, DaemonIngestProgressEvent,
        DaemonIngestResourcePolicy, DaemonIngestStage, DaemonIngestSystemTelemetry,
        DaemonIngestTelemetry, DaemonIngestThroughputTrend, DaemonRequestValidationError,
        SubmitIngestFilesRequest,
    };
    use crate::api::health::DaemonSsdPressure;
    use dasobjectstore_core::ids::{DiskId, IngestJobId, ObjectId, StoreId};

    #[test]
    fn validates_absolute_ingest_submission() {
        let request = SubmitIngestFilesRequest {
            endpoint: StoreId::new("zymo").expect("store id"),
            source_path: "/mnt/external/zymo".into(),
            copies: Some(1),
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
}
