use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 6;

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
    Provision {
        branches: Vec<BranchPlan>,
    },
    Inspect {
        branches: Vec<BranchPlan>,
    },
    Rollback {
        branches: Vec<BranchPlan>,
    },
    MountAggregate {
        aggregate: AggregatePlan,
    },
    InspectAggregate {
        aggregate: AggregatePlan,
    },
    UnmountAggregate {
        aggregate: AggregatePlan,
    },
    AttachNfs {
        export: NfsExportPlan,
    },
    InspectNfs {
        export: NfsExportPlan,
    },
    DetachNfs {
        export: NfsExportPlan,
    },
    MaterializeInspect {
        materialization: MaterializationPlan,
    },
    MaterializeStep {
        materialization: MaterializationPlan,
    },
    CheckpointInventory {
        checkpoint: CheckpointPlan,
    },
    PromotionInspect {
        promotion: PromotionPlan,
    },
    PromotionStep {
        promotion: PromotionPlan,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CheckpointPlan {
    pub relative_prefix: String,
    pub max_files: u32,
    pub max_logical_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PromotionPlan {
    pub promotion_id: String,
    pub checkpoint_id: String,
    pub source_relative_path: String,
    pub object_id: String,
    pub ingest_job_id: String,
    pub expected_size_bytes: u64,
    pub expected_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MaterializationPlan {
    pub source_object_id: String,
    pub source_placement_id: String,
    pub destination_relative_path: String,
    pub expected_size_bytes: u64,
    pub expected_sha256: String,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub materialization: Option<MaterializationInspection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<CheckpointInventory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub promotion: Option<PromotionInspection>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CheckpointInventory {
    pub relative_prefix: String,
    pub logical_bytes: u64,
    pub manifest_sha256: String,
    pub members: Vec<CheckpointMember>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CheckpointMember {
    pub relative_path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PromotionInspection {
    pub state: PromotionRecoveryState,
    pub completed_bytes: u64,
    pub expected_size_bytes: u64,
    pub observed_sha256: Option<String>,
    pub staged_relative_path: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PromotionRecoveryState {
    Absent,
    Copying,
    Ready,
    SourceConflict,
    DestinationConflict,
    UnsafeFilesystemEntry,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MaterializationInspection {
    pub state: MaterializationRecoveryState,
    pub completed_bytes: u64,
    pub expected_size_bytes: u64,
    pub observed_sha256: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterializationRecoveryState {
    Absent,
    Copying,
    Ready,
    DestinationConflict,
    SourceUnavailable,
    UnsafeFilesystemEntry,
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
