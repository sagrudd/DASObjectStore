use crate::api::health::DaemonSsdPressure;
use dasobjectstore_core::ids::{DiskId, IngestJobId, ObjectId, StoreId};
use dasobjectstore_core::lifecycle::{HealthState, IngestJobState};
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestSchedulingPolicy {
    pub source_read_backpressure: DaemonSourceReadBackpressurePolicy,
    pub buffer_pools: DaemonIngestBufferPoolPolicySet,
}

impl Default for DaemonIngestSchedulingPolicy {
    fn default() -> Self {
        Self {
            source_read_backpressure: DaemonSourceReadBackpressurePolicy::default(),
            buffer_pools: DaemonIngestBufferPoolPolicySet::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonSourceReadBackpressurePolicy {
    pub throttle_error_rate_per_minute: u32,
    pub block_error_rate_per_minute: u32,
}

impl Default for DaemonSourceReadBackpressurePolicy {
    fn default() -> Self {
        Self {
            throttle_error_rate_per_minute: 3,
            block_error_rate_per_minute: 10,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestBufferPoolPolicySet {
    pub read: DaemonIngestBoundedBufferPolicy,
    pub write: DaemonIngestBoundedBufferPolicy,
    pub verify: DaemonIngestBoundedBufferPolicy,
}

impl Default for DaemonIngestBufferPoolPolicySet {
    fn default() -> Self {
        Self {
            read: DaemonIngestBoundedBufferPolicy {
                queue_depth: 32,
                buffer_bytes: 4 * 1024 * 1024,
                pool_buffers: 32,
                memory_limit_bytes: 128 * 1024 * 1024,
            },
            write: DaemonIngestBoundedBufferPolicy {
                queue_depth: 64,
                buffer_bytes: 4 * 1024 * 1024,
                pool_buffers: 64,
                memory_limit_bytes: 256 * 1024 * 1024,
            },
            verify: DaemonIngestBoundedBufferPolicy {
                queue_depth: 32,
                buffer_bytes: 2 * 1024 * 1024,
                pool_buffers: 32,
                memory_limit_bytes: 64 * 1024 * 1024,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestBoundedBufferPolicy {
    pub queue_depth: u32,
    pub buffer_bytes: u64,
    pub pool_buffers: u32,
    pub memory_limit_bytes: u64,
}

impl DaemonIngestBoundedBufferPolicy {
    pub fn maximum_pool_bytes(&self) -> u64 {
        self.buffer_bytes
            .saturating_mul(u64::from(self.pool_buffers))
            .min(self.memory_limit_bytes)
    }

    pub fn has_bounded_capacity(&self) -> bool {
        self.queue_depth > 0
            && self.buffer_bytes > 0
            && self.pool_buffers > 0
            && self.memory_limit_bytes > 0
            && self.maximum_pool_bytes() <= self.memory_limit_bytes
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

impl DaemonIngestPressure {
    fn severity(self) -> u8 {
        match self {
            Self::Normal => 0,
            Self::Elevated => 1,
            Self::High => 2,
            Self::Critical => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonSourceReadPriority {
    BestEffort,
    Normal,
    SourceToSsdFirst,
    Recovery,
}

impl Default for DaemonSourceReadPriority {
    fn default() -> Self {
        Self::SourceToSsdFirst
    }
}

impl DaemonSourceReadPriority {
    fn tolerates_elevated_downstream_pressure(self) -> bool {
        matches!(self, Self::SourceToSsdFirst | Self::Recovery)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonSourceReadBackpressureAction {
    Run,
    Throttle,
    Block,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonSourceReadBackpressureReason {
    None,
    SsdPressure,
    RamPressure,
    HddBacklog,
    VerificationBacklog,
    ErrorRate,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonSourceReadBackpressureDecision {
    pub action: DaemonSourceReadBackpressureAction,
    pub reason: DaemonSourceReadBackpressureReason,
    pub priority: DaemonSourceReadPriority,
}

impl DaemonSourceReadBackpressureDecision {
    pub fn should_run(&self) -> bool {
        self.action == DaemonSourceReadBackpressureAction::Run
    }

    pub fn should_throttle(&self) -> bool {
        self.action == DaemonSourceReadBackpressureAction::Throttle
    }

    pub fn should_block(&self) -> bool {
        self.action == DaemonSourceReadBackpressureAction::Block
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestErrorRate {
    pub errors: u32,
    pub window_seconds: u32,
}

impl DaemonIngestErrorRate {
    pub fn errors_per_minute(&self) -> u32 {
        if self.window_seconds == 0 {
            return if self.errors == 0 { 0 } else { u32::MAX };
        }

        u64::from(self.errors)
            .saturating_mul(60)
            .div_ceil(u64::from(self.window_seconds))
            .min(u64::from(u32::MAX)) as u32
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonSourceReadBackpressureInput {
    pub priority: DaemonSourceReadPriority,
    pub ssd_pressure: DaemonSsdPressure,
    pub ram_pressure: DaemonIngestPressure,
    pub hdd_backlog: DaemonIngestPressure,
    pub verification_backlog: DaemonIngestPressure,
    pub error_rate: DaemonIngestErrorRate,
}

impl Default for DaemonSourceReadBackpressureInput {
    fn default() -> Self {
        Self {
            priority: DaemonSourceReadPriority::default(),
            ssd_pressure: DaemonSsdPressure::AcceptingWrites,
            ram_pressure: DaemonIngestPressure::Normal,
            hdd_backlog: DaemonIngestPressure::Normal,
            verification_backlog: DaemonIngestPressure::Normal,
            error_rate: DaemonIngestErrorRate::default(),
        }
    }
}

impl DaemonSourceReadBackpressureInput {
    pub fn classify(
        &self,
        policy: &DaemonSourceReadBackpressurePolicy,
    ) -> DaemonSourceReadBackpressureDecision {
        let error_rate = self.error_rate.errors_per_minute();

        if self.ssd_pressure == DaemonSsdPressure::Critical {
            return self.decision(
                DaemonSourceReadBackpressureAction::Block,
                DaemonSourceReadBackpressureReason::SsdPressure,
            );
        }
        if self.ram_pressure == DaemonIngestPressure::Critical {
            return self.decision(
                DaemonSourceReadBackpressureAction::Block,
                DaemonSourceReadBackpressureReason::RamPressure,
            );
        }
        if self.hdd_backlog == DaemonIngestPressure::Critical {
            return self.decision(
                DaemonSourceReadBackpressureAction::Block,
                DaemonSourceReadBackpressureReason::HddBacklog,
            );
        }
        if self.verification_backlog == DaemonIngestPressure::Critical {
            return self.decision(
                DaemonSourceReadBackpressureAction::Block,
                DaemonSourceReadBackpressureReason::VerificationBacklog,
            );
        }
        if error_rate >= policy.block_error_rate_per_minute {
            return self.decision(
                DaemonSourceReadBackpressureAction::Block,
                DaemonSourceReadBackpressureReason::ErrorRate,
            );
        }

        if self.ssd_pressure == DaemonSsdPressure::High {
            return self.decision(
                DaemonSourceReadBackpressureAction::Throttle,
                DaemonSourceReadBackpressureReason::SsdPressure,
            );
        }
        if self.ram_pressure.severity() >= DaemonIngestPressure::High.severity() {
            return self.decision(
                DaemonSourceReadBackpressureAction::Throttle,
                DaemonSourceReadBackpressureReason::RamPressure,
            );
        }
        if self.hdd_backlog.severity() >= DaemonIngestPressure::High.severity()
            || (self.hdd_backlog == DaemonIngestPressure::Elevated
                && !self.priority.tolerates_elevated_downstream_pressure())
        {
            return self.decision(
                DaemonSourceReadBackpressureAction::Throttle,
                DaemonSourceReadBackpressureReason::HddBacklog,
            );
        }
        if self.verification_backlog.severity() >= DaemonIngestPressure::High.severity()
            || (self.verification_backlog == DaemonIngestPressure::Elevated
                && !self.priority.tolerates_elevated_downstream_pressure())
        {
            return self.decision(
                DaemonSourceReadBackpressureAction::Throttle,
                DaemonSourceReadBackpressureReason::VerificationBacklog,
            );
        }
        if error_rate >= policy.throttle_error_rate_per_minute {
            return self.decision(
                DaemonSourceReadBackpressureAction::Throttle,
                DaemonSourceReadBackpressureReason::ErrorRate,
            );
        }

        self.decision(
            DaemonSourceReadBackpressureAction::Run,
            DaemonSourceReadBackpressureReason::None,
        )
    }

    fn decision(
        &self,
        action: DaemonSourceReadBackpressureAction,
        reason: DaemonSourceReadBackpressureReason,
    ) -> DaemonSourceReadBackpressureDecision {
        DaemonSourceReadBackpressureDecision {
            action,
            reason,
            priority: self.priority,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestPlacementSchedulerInput {
    pub required_bytes: u64,
    pub copies: u8,
    pub targets: Vec<DaemonIngestHddTargetQueue>,
}

impl DaemonIngestPlacementSchedulerInput {
    pub fn eligible_targets(&self) -> impl Iterator<Item = &DaemonIngestHddTargetQueue> {
        self.targets
            .iter()
            .filter(|target| target.can_accept(self.required_bytes))
    }

    pub fn has_enough_eligible_targets(&self) -> bool {
        self.eligible_targets().count() >= usize::from(self.copies)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestHddTargetQueue {
    pub target_id: String,
    pub disk_id: DiskId,
    pub capacity: DaemonIngestTargetCapacity,
    pub queue: DaemonIngestHddQueueState,
    pub write_throughput_bytes_per_second: u64,
    pub health: HealthState,
    pub pressure: DaemonIngestPressure,
    pub failure_state: DaemonIngestTargetFailureState,
}

impl DaemonIngestHddTargetQueue {
    pub fn can_accept(&self, required_bytes: u64) -> bool {
        self.capacity.available_bytes >= required_bytes
            && self.queue.queue_depth < self.queue.max_queue_depth
            && self.pressure != DaemonIngestPressure::Critical
            && self.failure_state == DaemonIngestTargetFailureState::Available
            && matches!(self.health, HealthState::Healthy | HealthState::Watch)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestTargetCapacity {
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub reserved_bytes: u64,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestHddQueueState {
    pub queue_depth: u32,
    pub queued_bytes: u64,
    pub max_queue_depth: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonIngestTargetFailureState {
    Available,
    Pressure,
    Failed,
    Suspended,
}

impl Default for DaemonIngestTargetFailureState {
    fn default() -> Self {
        Self::Available
    }
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
        CancelIngestJobRequest, DaemonIngestBottleneck, DaemonIngestCompletionFraction,
        DaemonIngestErrorRate, DaemonIngestHddQueueState, DaemonIngestHddTargetQueue,
        DaemonIngestPipelineStage, DaemonIngestPlacementSchedulerInput, DaemonIngestPressure,
        DaemonIngestProgressEvent, DaemonIngestResourcePolicy, DaemonIngestSchedulingPolicy,
        DaemonIngestStage, DaemonIngestSystemTelemetry, DaemonIngestTargetCapacity,
        DaemonIngestTargetFailureState, DaemonIngestTelemetry, DaemonIngestThroughputTrend,
        DaemonRequestValidationError, DaemonSourceReadBackpressureInput,
        DaemonSourceReadBackpressurePolicy, DaemonSourceReadBackpressureReason,
        DaemonSourceReadPriority, SubmitIngestFilesRequest,
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

    #[test]
    fn default_scheduling_policy_has_bounded_buffer_pools() {
        let policy = DaemonIngestSchedulingPolicy::default();

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
    fn scheduling_dtos_use_stable_snake_case() {
        let decision = DaemonSourceReadBackpressureInput {
            priority: DaemonSourceReadPriority::SourceToSsdFirst,
            ssd_pressure: DaemonSsdPressure::High,
            ..DaemonSourceReadBackpressureInput::default()
        }
        .classify(&DaemonSourceReadBackpressurePolicy::default());
        let target_state = DaemonIngestTargetFailureState::Suspended;

        let decision = serde_json::to_value(decision).expect("decision serializes");
        let target_state = serde_json::to_value(target_state).expect("state serializes");

        assert_eq!(decision["action"], "throttle");
        assert_eq!(decision["reason"], "ssd_pressure");
        assert_eq!(decision["priority"], "source_to_ssd_first");
        assert_eq!(target_state, "suspended");
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
}
