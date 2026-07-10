use crate::api::health::DaemonSsdPressure;
use serde::{Deserialize, Serialize};

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
    #[serde(default)]
    pub source_read_bytes_per_second: u64,
    #[serde(default)]
    pub ssd_write_bytes_per_second: u64,
    #[serde(default)]
    pub aggregate_hdd_write_bytes_per_second: u64,
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
    pub(super) fn severity(self) -> u8 {
        match self {
            Self::Normal => 0,
            Self::Elevated => 1,
            Self::High => 2,
            Self::Critical => 3,
        }
    }
}
