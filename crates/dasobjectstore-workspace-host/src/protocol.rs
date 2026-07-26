use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 3;

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
    MountAggregate { aggregate: AggregatePlan },
    InspectAggregate { aggregate: AggregatePlan },
    UnmountAggregate { aggregate: AggregatePlan },
    AttachNfs { export: NfsExportPlan },
    InspectNfs { export: NfsExportPlan },
    DetachNfs { export: NfsExportPlan },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NfsExportPlan {
    pub mount_identity: String,
    pub client_id: String,
    pub access_mode: NfsAccessMode,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NfsAccessMode {
    ReadOnly,
    ReadWrite,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BranchPlan {
    pub disk_id: String,
    pub branch_id: String,
    pub project_id: u32,
    pub quota_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AggregatePlan {
    pub mount_identity: String,
    pub branches: Vec<BranchPlan>,
    pub minimum_free_bytes: u64,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aggregate: Option<AggregateInspection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub export: Option<NfsExportInspection>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NfsExportInspection {
    pub mount_identity: String,
    pub client_id: String,
    pub resolved_address_or_cidr: String,
    pub state: NfsExportRecoveryState,
    pub published: bool,
    pub root_squash: bool,
    pub address_matches: bool,
    pub access_mode_matches: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NfsExportRecoveryState {
    Absent,
    Ready,
    FragmentConflict,
    AggregateUnavailable,
    UnsafeFilesystemEntry,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AggregateInspection {
    pub mount_identity: String,
    pub state: AggregateRecoveryState,
    pub mounted: bool,
    pub source_matches: bool,
    pub options_match: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AggregateRecoveryState {
    Absent,
    Ready,
    MarkerMissing,
    MarkerConflict,
    MountConflict,
    BranchUnavailable,
    UnsafeFilesystemEntry,
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
