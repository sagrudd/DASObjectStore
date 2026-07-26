use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BrokerRequest {
    pub protocol_version: u32,
    pub request_id: String,
    pub workspace_id: String,
    pub operation: WorkspaceHostOperation,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkspaceHostOperation {
    Provision { branches: Vec<BranchPlan> },
    Inspect { branches: Vec<BranchPlan> },
    Rollback { branches: Vec<BranchPlan> },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BranchPlan {
    pub disk_id: String,
    pub branch_id: String,
    pub project_id: u32,
    pub quota_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BrokerResponse {
    pub protocol_version: u32,
    pub request_id: String,
    pub workspace_id: String,
    pub ok: bool,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub branches: Vec<BranchInspection>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BranchInspection {
    pub disk_id: String,
    pub branch_id: String,
    pub state: RecoveryState,
    pub marker_matches: bool,
    pub quota_enforced: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryState {
    Absent,
    Ready,
    MarkerMissing,
    MarkerConflict,
    QuotaMissing,
    UnsafeFilesystemEntry,
}
