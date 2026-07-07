use dasobjectstore_core::ids::DiskId;
use dasobjectstore_core::lifecycle::HealthState;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonHealthSummaryRequest {
    pub include_connections: bool,
    pub include_disk_details: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonHealthSummaryResponse {
    pub generated_at_utc: String,
    pub overall_state: HealthState,
    pub disk_count: usize,
    pub suspect_disk_count: usize,
    pub ingest: DaemonIngestSummary,
    pub disks: Vec<DaemonDiskHealthSummary>,
    pub warnings: Vec<DaemonApiWarning>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestSummary {
    pub queued_jobs: usize,
    pub active_jobs: usize,
    pub failed_jobs: usize,
    pub ssd_pressure: DaemonSsdPressure,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonSsdPressure {
    AcceptingWrites,
    High,
    Critical,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonDiskHealthSummary {
    pub disk_id: DiskId,
    pub state: HealthState,
    pub score: u8,
    pub placement_eligible: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonApiWarning {
    pub code: String,
    pub message: String,
}

impl DaemonApiWarning {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}
