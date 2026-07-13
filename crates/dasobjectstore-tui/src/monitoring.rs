use crate::planning::format_size_label;
use dasobjectstore_daemon::api::CapacityStatusResponse;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveIngestTelemetry {
    pub job_id: String,
    pub target_label: String,
    pub state: IngestRunState,
    pub progress: IngestProgressTelemetry,
    pub workers: WorkerTelemetry,
    pub queue_depths: QueueDepthTelemetry,
    pub bottleneck: Bottleneck,
    pub source_throttle: SourceThrottleState,
    pub ssd_pressure: SsdPressureTelemetry,
    pub hdd_pressure: HddPressureTelemetry,
    pub verification: VerificationTelemetry,
    pub throughput: ThroughputTelemetry,
    /// Latest daemon-owned logical-capacity snapshot for this target.
    ///
    /// The snapshot is optional because a TUI can remain useful while the
    /// daemon control plane is reconnecting.  When present it is rendered
    /// alongside the pipeline telemetry rather than inferred from SSD usage.
    pub capacity: Option<CapacityStatusResponse>,
    pub action_support: DaemonActionSupport,
    pub attach_state: AttachState,
    pub errors: Vec<TuiErrorState>,
}

impl LiveIngestTelemetry {
    pub fn display_data(&self) -> LiveMonitoringDisplay {
        let actions = KeyboardActionModel::from_support_flags(self.action_support, self.state);
        let completed_summary = (self.state == IngestRunState::Completed).then(|| {
            format!(
                "Completed: staged {}, written {}, verified {}; final status {}",
                self.progress.staged_bytes.display_bytes(),
                self.progress.written_bytes.display_bytes(),
                self.progress.verified_bytes.display_bytes(),
                self.verification.final_status.label()
            )
        });

        LiveMonitoringDisplay {
            title: format!(
                "Job {} -> {} ({})",
                self.job_id,
                self.target_label,
                self.state.label()
            ),
            progress_lines: self.progress.display_lines(),
            workers_label: self.workers.display_label(),
            queue_depths_label: self.queue_depths.display_label(),
            bottleneck_label: format!("Bottleneck: {}", self.bottleneck.label()),
            throttle_label: format!("Source-to-SSD: {}", self.source_throttle.label()),
            ssd_pressure_label: self.ssd_pressure.display_label(&self.source_throttle),
            hdd_pressure_label: self.hdd_pressure.display_label(),
            verification_label: self.verification.display_label(),
            throughput_label: self.throughput.display_label(),
            capacity_label: self.capacity.as_ref().map(capacity_display_label),
            actions: actions.actions,
            attach_label: self.attach_state.display_label(),
            error_labels: self
                .errors
                .iter()
                .map(TuiErrorState::display_label)
                .collect(),
            completed_summary,
        }
    }

    pub fn snapshot_text(&self) -> String {
        self.display_data().snapshot_text()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LiveMonitoringDisplay {
    pub title: String,
    pub progress_lines: Vec<String>,
    pub workers_label: String,
    pub queue_depths_label: String,
    pub bottleneck_label: String,
    pub throttle_label: String,
    pub ssd_pressure_label: String,
    pub hdd_pressure_label: String,
    pub verification_label: String,
    pub throughput_label: String,
    pub capacity_label: Option<String>,
    pub actions: Vec<KeyboardActionDisplay>,
    pub attach_label: String,
    pub error_labels: Vec<String>,
    pub completed_summary: Option<String>,
}

impl LiveMonitoringDisplay {
    pub fn snapshot_text(&self) -> String {
        let mut lines = Vec::new();
        lines.push(self.title.clone());
        lines.extend(self.progress_lines.clone());
        lines.push(self.workers_label.clone());
        lines.push(self.queue_depths_label.clone());
        lines.push(self.bottleneck_label.clone());
        lines.push(self.throttle_label.clone());
        lines.push(self.ssd_pressure_label.clone());
        lines.push(self.hdd_pressure_label.clone());
        lines.push(self.verification_label.clone());
        lines.push(self.throughput_label.clone());
        if let Some(capacity) = &self.capacity_label {
            lines.push(capacity.clone());
        }
        lines.push(self.attach_label.clone());

        if !self.actions.is_empty() {
            lines.push(format!(
                "Actions: {}",
                self.actions
                    .iter()
                    .map(KeyboardActionDisplay::display_label)
                    .collect::<Vec<_>>()
                    .join(" | ")
            ));
        }

        lines.extend(
            self.error_labels
                .iter()
                .map(|error| format!("Error: {error}")),
        );

        if let Some(summary) = &self.completed_summary {
            lines.push(summary.clone());
        }

        lines.join("\n")
    }
}

fn capacity_display_label(response: &CapacityStatusResponse) -> String {
    let logical_limit = response
        .logical_limit_bytes
        .map(format_size_label)
        .unwrap_or_else(|| "unbounded".to_string());
    let logical_available = response
        .logical_available_bytes
        .map(format_size_label)
        .unwrap_or_else(|| "unbounded".to_string());
    let ssd_available = response
        .ssd_available_bytes
        .map(format_size_label)
        .unwrap_or_else(|| "not required".to_string());
    let block = response
        .admission_block_reason
        .map(|reason| format!(", blocked {reason:?}"))
        .unwrap_or_default();
    let amplification = format!("{:.2}x", f64::from(response.copy_count));

    format!(
        "Capacity {}: pressure {:?}, used {}, reserved {}, logical available {} / limit {}, backend free {} (available {}), SSD available {}, amplification {}, thresholds warning {}bp/critical {}bp{}",
        response.store_id,
        response.pressure,
        format_size_label(response.used_bytes),
        format_size_label(response.reserved_bytes),
        logical_available,
        logical_limit,
        format_size_label(response.backend_free_bytes),
        format_size_label(response.backend_available_bytes),
        ssd_available,
        amplification,
        response.warning_threshold_basis_points,
        response.critical_threshold_basis_points,
        block,
    )
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IngestRunState {
    Queued,
    Running,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

impl IngestRunState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    fn can_pause(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }

    fn can_resume(self) -> bool {
        self == Self::Paused
    }

    fn can_cancel(self) -> bool {
        matches!(self, Self::Queued | Self::Running | Self::Paused)
    }

    fn can_retry(self) -> bool {
        self == Self::Failed
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IngestProgressTelemetry {
    pub discovered_files: CompletionFraction,
    pub scanned_files: CompletionFraction,
    pub staged_bytes: CompletionFraction,
    pub staged_files: CompletionFraction,
    pub written_bytes: CompletionFraction,
    pub written_files: CompletionFraction,
    pub verified_bytes: CompletionFraction,
    pub verified_files: CompletionFraction,
}

impl IngestProgressTelemetry {
    pub fn display_lines(&self) -> Vec<String> {
        vec![
            format!(
                "Discovery: {} discovered, {} scanned",
                self.discovered_files.display_count(),
                self.scanned_files.display_count()
            ),
            format!(
                "Staged on SSD: {} bytes, {} files",
                self.staged_bytes.display_bytes(),
                self.staged_files.display_count()
            ),
            format!(
                "Written to HDD: {} bytes, {} files",
                self.written_bytes.display_bytes(),
                self.written_files.display_count()
            ),
            format!(
                "Verified: {} bytes, {} files",
                self.verified_bytes.display_bytes(),
                self.verified_files.display_count()
            ),
        ]
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CompletionFraction {
    pub done: u64,
    pub total: Option<u64>,
}

impl CompletionFraction {
    pub fn new(done: u64, total: Option<u64>) -> Self {
        Self { done, total }
    }

    pub fn percent_complete(self) -> Option<u8> {
        let total = self.total?;
        if total == 0 {
            return Some(100);
        }

        Some(((self.done.saturating_mul(100)) / total).min(100) as u8)
    }

    pub fn display_count(self) -> String {
        self.display_with(|value| value.to_string())
    }

    pub fn display_bytes(self) -> String {
        self.display_with(format_size_label)
    }

    fn display_with(self, formatter: impl Fn(u64) -> String) -> String {
        match self.total {
            Some(total) => format!(
                "{}/{} ({}%)",
                formatter(self.done),
                formatter(total),
                self.percent_complete().unwrap_or(0)
            ),
            None => formatter(self.done),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WorkerTelemetry {
    pub scan: WorkerActivity,
    pub source_read: WorkerActivity,
    pub ssd_stage: WorkerActivity,
    pub hdd_write: WorkerActivity,
    pub verification: WorkerActivity,
    pub finalization: WorkerActivity,
}

impl WorkerTelemetry {
    pub fn display_label(self) -> String {
        format!(
            "Workers active/idle: scan {}, read {}, stage {}, write {}, verify {}, final {}",
            self.scan.display_label(),
            self.source_read.display_label(),
            self.ssd_stage.display_label(),
            self.hdd_write.display_label(),
            self.verification.display_label(),
            self.finalization.display_label()
        )
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WorkerActivity {
    pub active: u16,
    pub idle: u16,
}

impl WorkerActivity {
    pub fn new(active: u16, idle: u16) -> Self {
        Self { active, idle }
    }

    fn display_label(self) -> String {
        format!("{}/{}", self.active, self.idle)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct QueueDepthTelemetry {
    pub scan: u32,
    pub source_read: u32,
    pub ssd_stage: u32,
    pub hdd_write: u32,
    pub verification: u32,
}

impl QueueDepthTelemetry {
    pub fn display_label(self) -> String {
        format!(
            "Queues: scan {}, read {}, stage {}, write {}, verify {}",
            self.scan, self.source_read, self.ssd_stage, self.hdd_write, self.verification
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Bottleneck {
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

impl Bottleneck {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Scan => "scan",
            Self::SourceRead => "source read",
            Self::SsdStage => "SSD stage",
            Self::ChecksumManifest => "checksum/manifest",
            Self::HddPlacement => "HDD placement",
            Self::HddWrite => "HDD write",
            Self::Verification => "verification",
            Self::Cpu => "CPU",
            Self::Memory => "memory",
            Self::SsdPressure => "SSD pressure",
            Self::HddPressure => "HDD pressure",
            Self::VerificationBacklog => "verification backlog",
        }
    }
}

impl Default for Bottleneck {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceThrottleState {
    Unthrottled,
    Throttled { reason: String },
    Blocked { reason: String },
}

impl SourceThrottleState {
    pub fn label(&self) -> String {
        match self {
            Self::Unthrottled => "unthrottled".to_string(),
            Self::Throttled { reason } => format!("throttled ({reason})"),
            Self::Blocked { reason } => format!("blocked ({reason})"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SsdPressureTelemetry {
    pub level: SsdPressureLevel,
    pub capacity_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
    pub trend: PressureTrend,
}

impl SsdPressureTelemetry {
    pub fn display_label(self, source_throttle: &SourceThrottleState) -> String {
        format!(
            "SSD pressure: {}; capacity {}, used {}, free {}, trend {}, source {}",
            self.level.label(),
            format_size_label(self.capacity_bytes),
            format_size_label(self.used_bytes),
            format_size_label(self.free_bytes),
            self.trend.label(),
            source_throttle.label()
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SsdPressureLevel {
    AcceptingWrites,
    High,
    Critical,
}

impl SsdPressureLevel {
    fn label(self) -> &'static str {
        match self {
            Self::AcceptingWrites => "accepting writes",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HddPressureTelemetry {
    pub level: PipelinePressureLevel,
    pub backlog_files: u32,
    pub write_throughput_bytes_per_second: u64,
    pub retries: u32,
    pub detected_bottleneck: Bottleneck,
}

impl HddPressureTelemetry {
    pub fn display_label(self) -> String {
        format!(
            "HDD pressure: {}; backlog {} files, write {}, retries {}, bottleneck {}",
            self.level.label(),
            self.backlog_files,
            format_rate_label(self.write_throughput_bytes_per_second),
            self.retries,
            self.detected_bottleneck.label()
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PipelinePressureLevel {
    Normal,
    Elevated,
    High,
    Critical,
}

impl PipelinePressureLevel {
    fn label(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Elevated => "elevated",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PressureTrend {
    Rising,
    Falling,
    Flat,
}

impl PressureTrend {
    fn label(self) -> &'static str {
        match self {
            Self::Rising => "rising",
            Self::Falling => "falling",
            Self::Flat => "flat",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerificationTelemetry {
    pub verified_bytes: CompletionFraction,
    pub verified_files: CompletionFraction,
    pub failures: u32,
    pub retries: u32,
    pub final_status: VerificationStatus,
}

impl VerificationTelemetry {
    pub fn display_label(self) -> String {
        format!(
            "Verification: {} bytes, {} files, failures {}, retries {}, status {}",
            self.verified_bytes.display_bytes(),
            self.verified_files.display_count(),
            self.failures,
            self.retries,
            self.final_status.label()
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerificationStatus {
    Pending,
    Running,
    Passed,
    Failed,
}

impl VerificationStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Passed => "passed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThroughputTelemetry {
    pub current_bytes_per_second: u64,
    pub moving_average_bytes_per_second: u64,
    pub recent_high_bytes_per_second: u64,
    pub recent_low_bytes_per_second: u64,
    pub trend: ThroughputTrend,
}

impl ThroughputTelemetry {
    pub fn display_label(self) -> String {
        format!(
            "Throughput: current {}, moving {}, high {}, low {}, trend {}",
            format_rate_label(self.current_bytes_per_second),
            format_rate_label(self.moving_average_bytes_per_second),
            format_rate_label(self.recent_high_bytes_per_second),
            format_rate_label(self.recent_low_bytes_per_second),
            self.trend.label()
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThroughputTrend {
    Up,
    Down,
    Flat,
}

impl ThroughputTrend {
    fn label(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::Flat => "flat",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DaemonActionSupport {
    pub pause: bool,
    pub resume: bool,
    pub cancel: bool,
    pub retry: bool,
    pub job_details: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyboardActionModel {
    pub actions: Vec<KeyboardActionDisplay>,
}

impl KeyboardActionModel {
    pub fn from_support_flags(support: DaemonActionSupport, state: IngestRunState) -> Self {
        Self {
            actions: vec![
                action_display(
                    KeyboardActionKind::Pause,
                    "p",
                    "pause",
                    support.pause,
                    state.can_pause(),
                ),
                action_display(
                    KeyboardActionKind::Resume,
                    "r",
                    "resume",
                    support.resume,
                    state.can_resume(),
                ),
                action_display(
                    KeyboardActionKind::Cancel,
                    "c",
                    "cancel",
                    support.cancel,
                    state.can_cancel(),
                ),
                action_display(
                    KeyboardActionKind::Retry,
                    "R",
                    "retry",
                    support.retry,
                    state.can_retry(),
                ),
                action_display(
                    KeyboardActionKind::JobDetails,
                    "d",
                    "details",
                    support.job_details,
                    true,
                ),
            ],
        }
    }
}

fn action_display(
    kind: KeyboardActionKind,
    key: &'static str,
    label: &'static str,
    daemon_supported: bool,
    state_allowed: bool,
) -> KeyboardActionDisplay {
    KeyboardActionDisplay {
        kind,
        key,
        label,
        enabled: daemon_supported && state_allowed,
        disabled_reason: if !daemon_supported {
            Some("daemon unsupported")
        } else if !state_allowed {
            Some("not valid for job state")
        } else {
            None
        },
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyboardActionKind {
    Pause,
    Resume,
    Cancel,
    Retry,
    JobDetails,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyboardActionDisplay {
    pub kind: KeyboardActionKind,
    pub key: &'static str,
    pub label: &'static str,
    pub enabled: bool,
    pub disabled_reason: Option<&'static str>,
}

impl KeyboardActionDisplay {
    fn display_label(&self) -> String {
        match self.disabled_reason {
            Some(reason) => format!("{} {} disabled ({})", self.key, self.label, reason),
            None => format!("{} {} enabled", self.key, self.label),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AttachState {
    NewJob,
    DiscoveringExistingJobs,
    AttachingExistingJob { job_id: String },
    AttachedNewJob { job_id: String },
    AttachedExistingJob { job_id: String },
    Reconnecting { job_id: String, attempt: u16 },
    ConnectionLost { job_id: String },
    Detached { reason: String },
}

impl AttachState {
    pub fn display_label(&self) -> String {
        match self {
            Self::NewJob => "Attach: launching new job".to_string(),
            Self::DiscoveringExistingJobs => "Attach: discovering existing jobs".to_string(),
            Self::AttachingExistingJob { job_id } => {
                format!("Attach: attaching to existing job {job_id}")
            }
            Self::AttachedNewJob { job_id } => format!("Attach: attached to new job {job_id}"),
            Self::AttachedExistingJob { job_id } => {
                format!("Attach: attached to existing running job {job_id}")
            }
            Self::Reconnecting { job_id, attempt } => {
                format!("Attach: reconnecting to job {job_id} (attempt {attempt})")
            }
            Self::ConnectionLost { job_id } => {
                format!("Attach: connection lost for job {job_id}")
            }
            Self::Detached { reason } => format!("Attach: detached ({reason})"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TuiErrorState {
    pub kind: TuiErrorKind,
    pub detail: Option<String>,
}

impl TuiErrorState {
    pub fn new(kind: TuiErrorKind, detail: Option<impl Into<String>>) -> Self {
        Self {
            kind,
            detail: detail.map(Into::into),
        }
    }

    pub fn display_label(&self) -> String {
        match &self.detail {
            Some(detail) if !detail.is_empty() => format!("{}: {detail}", self.kind.label()),
            _ => self.kind.label().to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TuiErrorKind {
    AuthenticationFailure,
    PermissionDenied,
    LostDaemonConnection,
    LostEventConnection,
    StalledJob,
    SsdPressure,
    HddWriteFailure,
    VerificationFailure,
}

impl TuiErrorKind {
    fn label(self) -> &'static str {
        match self {
            Self::AuthenticationFailure => "authentication failure",
            Self::PermissionDenied => "permission denied",
            Self::LostDaemonConnection => "lost daemon connection",
            Self::LostEventConnection => "lost event connection",
            Self::StalledJob => "stalled job",
            Self::SsdPressure => "SSD pressure",
            Self::HddWriteFailure => "HDD write failure",
            Self::VerificationFailure => "verification failure",
        }
    }
}

pub fn format_rate_label(bytes_per_second: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    const GIB: u64 = MIB * 1024;

    let (unit_bytes, unit) = if bytes_per_second >= GIB {
        (GIB, "GiB/s")
    } else if bytes_per_second >= MIB {
        (MIB, "MiB/s")
    } else if bytes_per_second >= KIB {
        (KIB, "KiB/s")
    } else {
        (1, "B/s")
    };

    if unit_bytes == 1 {
        return format!("{bytes_per_second} {unit}");
    }

    let tenths = ((bytes_per_second * 10) + (unit_bytes / 2)) / unit_bytes;
    format!("{}.{:01} {}", tenths / 10, tenths % 10, unit)
}

#[cfg(test)]
mod tests {
    use super::{
        AttachState, Bottleneck, CompletionFraction, DaemonActionSupport, HddPressureTelemetry,
        IngestProgressTelemetry, IngestRunState, LiveIngestTelemetry, PipelinePressureLevel,
        PressureTrend, QueueDepthTelemetry, SourceThrottleState, SsdPressureLevel,
        SsdPressureTelemetry, ThroughputTelemetry, ThroughputTrend, TuiErrorKind, TuiErrorState,
        VerificationStatus, VerificationTelemetry, WorkerActivity, WorkerTelemetry,
    };

    #[test]
    fn renders_live_monitoring_snapshot() {
        let telemetry = sample_telemetry();

        assert_eq!(
            telemetry.snapshot_text(),
            concat!(
                "Job ingest-42 -> research/run-42 (running)\n",
                "Discovery: 128/256 (50%) discovered, 96/256 (37%) scanned\n",
                "Staged on SSD: 32.0 GiB/64.0 GiB (50%) bytes, 80/160 (50%) files\n",
                "Written to HDD: 16.0 GiB/64.0 GiB (25%) bytes, 40/160 (25%) files\n",
                "Verified: 8.0 GiB/64.0 GiB (12%) bytes, 20/160 (12%) files\n",
                "Workers active/idle: scan 1/0, read 3/1, stage 2/0, write 6/2, verify 2/1, final 0/1\n",
                "Queues: scan 4, read 8, stage 12, write 33, verify 6\n",
                "Bottleneck: HDD write\n",
                "Source-to-SSD: throttled (HDD backlog)\n",
                "SSD pressure: high; capacity 512.0 GiB, used 420.0 GiB, free 92.0 GiB, trend rising, source throttled (HDD backlog)\n",
                "HDD pressure: elevated; backlog 33 files, write 180.0 MiB/s, retries 2, bottleneck HDD write\n",
                "Verification: 8.0 GiB/64.0 GiB (12%) bytes, 20/160 (12%) files, failures 1, retries 1, status running\n",
                "Throughput: current 240.0 MiB/s, moving 210.0 MiB/s, high 260.0 MiB/s, low 140.0 MiB/s, trend up\n",
                "Attach: attached to existing running job ingest-42\n",
                "Actions: p pause enabled | r resume disabled (not valid for job state) | c cancel enabled | R retry disabled (daemon unsupported) | d details enabled"
            )
        );
    }

    #[test]
    fn renders_reconnect_state_for_existing_job() {
        let state = AttachState::Reconnecting {
            job_id: "ingest-42".to_string(),
            attempt: 3,
        };

        assert_eq!(
            state.display_label(),
            "Attach: reconnecting to job ingest-42 (attempt 3)"
        );
    }

    #[test]
    fn renders_completed_summary() {
        let mut telemetry = sample_telemetry();
        telemetry.state = IngestRunState::Completed;
        telemetry.verification.final_status = VerificationStatus::Passed;
        telemetry.progress.staged_bytes =
            CompletionFraction::new(64 * 1024 * 1024 * 1024, Some(64 * 1024 * 1024 * 1024));
        telemetry.progress.written_bytes = telemetry.progress.staged_bytes;
        telemetry.progress.verified_bytes = telemetry.progress.staged_bytes;

        let snapshot = telemetry.snapshot_text();

        assert!(snapshot.contains(
            "Completed: staged 64.0 GiB/64.0 GiB (100%), written 64.0 GiB/64.0 GiB (100%), verified 64.0 GiB/64.0 GiB (100%); final status passed"
        ));
        assert!(snapshot.contains("p pause disabled (not valid for job state)"));
        assert!(snapshot.contains("d details enabled"));
    }

    #[test]
    fn renders_error_display_lines() {
        let mut telemetry = sample_telemetry();
        telemetry.errors = vec![
            TuiErrorState::new(
                TuiErrorKind::LostEventConnection,
                Some("last event 45s ago"),
            ),
            TuiErrorState::new(TuiErrorKind::VerificationFailure, Some("2 files failed")),
        ];

        let snapshot = telemetry.snapshot_text();

        assert!(snapshot.contains("Error: lost event connection: last event 45s ago"));
        assert!(snapshot.contains("Error: verification failure: 2 files failed"));
    }

    #[test]
    fn renders_daemon_capacity_snapshot_without_inferring_logical_usage() {
        let mut telemetry = sample_telemetry();
        telemetry.capacity = Some(dasobjectstore_daemon::api::CapacityStatusResponse {
            store_id: dasobjectstore_core::ids::StoreId::new("research").expect("safe store id"),
            pressure: dasobjectstore_core::store::CapacityPressureState::Warning,
            logical_limit_bytes: Some(1_000_000),
            used_bytes: 400_000,
            reserved_bytes: 100_000,
            logical_available_bytes: Some(500_000),
            backend_free_bytes: 2_000_000,
            backend_available_bytes: 1_900_000,
            ssd_available_bytes: Some(700_000),
            copy_count: 2,
            requires_ssd_staging: true,
            warning_threshold_basis_points: 7_500,
            critical_threshold_basis_points: 9_000,
            admission_block_reason: None,
        });

        assert!(telemetry.snapshot_text().contains(
            "Capacity research: pressure Warning, used 0.4 MiB, reserved 0.1 MiB, logical available 0.5 MiB / limit 1.0 MiB, backend free 1.9 MiB (available 1.8 MiB), SSD available 0.7 MiB, amplification 2.00x, thresholds warning 7500bp/critical 9000bp"
        ));
    }

    fn sample_telemetry() -> LiveIngestTelemetry {
        let total_bytes = 64 * 1024 * 1024 * 1024;
        let staged_bytes = 32 * 1024 * 1024 * 1024;
        let written_bytes = 16 * 1024 * 1024 * 1024;
        let verified_bytes = 8 * 1024 * 1024 * 1024;

        LiveIngestTelemetry {
            job_id: "ingest-42".to_string(),
            target_label: "research/run-42".to_string(),
            state: IngestRunState::Running,
            progress: IngestProgressTelemetry {
                discovered_files: CompletionFraction::new(128, Some(256)),
                scanned_files: CompletionFraction::new(96, Some(256)),
                staged_bytes: CompletionFraction::new(staged_bytes, Some(total_bytes)),
                staged_files: CompletionFraction::new(80, Some(160)),
                written_bytes: CompletionFraction::new(written_bytes, Some(total_bytes)),
                written_files: CompletionFraction::new(40, Some(160)),
                verified_bytes: CompletionFraction::new(verified_bytes, Some(total_bytes)),
                verified_files: CompletionFraction::new(20, Some(160)),
            },
            workers: WorkerTelemetry {
                scan: WorkerActivity::new(1, 0),
                source_read: WorkerActivity::new(3, 1),
                ssd_stage: WorkerActivity::new(2, 0),
                hdd_write: WorkerActivity::new(6, 2),
                verification: WorkerActivity::new(2, 1),
                finalization: WorkerActivity::new(0, 1),
            },
            queue_depths: QueueDepthTelemetry {
                scan: 4,
                source_read: 8,
                ssd_stage: 12,
                hdd_write: 33,
                verification: 6,
            },
            bottleneck: Bottleneck::HddWrite,
            source_throttle: SourceThrottleState::Throttled {
                reason: "HDD backlog".to_string(),
            },
            ssd_pressure: SsdPressureTelemetry {
                level: SsdPressureLevel::High,
                capacity_bytes: 512 * 1024 * 1024 * 1024,
                used_bytes: 420 * 1024 * 1024 * 1024,
                free_bytes: 92 * 1024 * 1024 * 1024,
                trend: PressureTrend::Rising,
            },
            hdd_pressure: HddPressureTelemetry {
                level: PipelinePressureLevel::Elevated,
                backlog_files: 33,
                write_throughput_bytes_per_second: 180 * 1024 * 1024,
                retries: 2,
                detected_bottleneck: Bottleneck::HddWrite,
            },
            verification: VerificationTelemetry {
                verified_bytes: CompletionFraction::new(verified_bytes, Some(total_bytes)),
                verified_files: CompletionFraction::new(20, Some(160)),
                failures: 1,
                retries: 1,
                final_status: VerificationStatus::Running,
            },
            throughput: ThroughputTelemetry {
                current_bytes_per_second: 240 * 1024 * 1024,
                moving_average_bytes_per_second: 210 * 1024 * 1024,
                recent_high_bytes_per_second: 260 * 1024 * 1024,
                recent_low_bytes_per_second: 140 * 1024 * 1024,
                trend: ThroughputTrend::Up,
            },
            capacity: None,
            action_support: DaemonActionSupport {
                pause: true,
                resume: true,
                cancel: true,
                retry: false,
                job_details: true,
            },
            attach_state: AttachState::AttachedExistingJob {
                job_id: "ingest-42".to_string(),
            },
            errors: Vec::new(),
        }
    }
}
