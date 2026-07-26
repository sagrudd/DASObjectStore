//! Bounded, daemon-owned compute-workspace checkpoint registration.
//!
//! The privileged broker hashes a quiesced logical prefix. SQLite is not held
//! while filesystem content is examined; the returned immutable inventory and
//! logical accounting are committed in one immediate transaction.

use dasobjectstore_core::ids::WorkspaceId;
use dasobjectstore_metadata::{
    read_workspace_health, read_workspace_reservation, register_workspace_checkpoint,
    RegisterWorkspaceCheckpoint, WorkspaceCheckpointMember, WorkspaceCheckpointSnapshot,
    WorkspaceHealthReport,
};
use dasobjectstore_workspace_host::{
    request_broker, AggregatePlan, AggregateRecoveryState, BranchPlan, BrokerRequest,
    BrokerResponse, CheckpointPlan, WorkspaceHostOperation, PROTOCOL_VERSION,
};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct WorkspaceCheckpointConfig {
    pub live_sqlite_path: PathBuf,
    pub broker_socket_path: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceCheckpointRequest {
    pub checkpoint_id: String,
    pub workspace_id: String,
    pub relative_prefix: String,
    pub role: String,
    pub reproducibility_class: String,
    pub max_files: u32,
    pub max_logical_bytes: u64,
    pub removable_after_promotion: bool,
    pub created_at_utc: String,
    pub retention_deadline_utc: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceCheckpointReport {
    pub schema_version: String,
    pub checkpoint: WorkspaceCheckpointSnapshot,
    pub health: WorkspaceHealthReport,
}

#[derive(Debug)]
pub enum WorkspaceCheckpointError {
    InvalidRequest(String),
    Metadata(String),
    Broker(String),
    UnsafeEvidence(String),
}

impl fmt::Display for WorkspaceCheckpointError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest(message) => write!(output, "invalid checkpoint: {message}"),
            Self::Metadata(message) => write!(output, "workspace metadata: {message}"),
            Self::Broker(message) => write!(output, "workspace host broker: {message}"),
            Self::UnsafeEvidence(message) => write!(output, "checkpoint evidence: {message}"),
        }
    }
}

impl std::error::Error for WorkspaceCheckpointError {}

pub fn register_bounded_workspace_checkpoint(
    config: &WorkspaceCheckpointConfig,
    request: &WorkspaceCheckpointRequest,
) -> Result<WorkspaceCheckpointReport, WorkspaceCheckpointError> {
    register_bounded_workspace_checkpoint_with(config, request, |broker_request| {
        request_broker(&config.broker_socket_path, broker_request)
            .map_err(|error| error.to_string())
    })
}

fn register_bounded_workspace_checkpoint_with<F>(
    config: &WorkspaceCheckpointConfig,
    request: &WorkspaceCheckpointRequest,
    mut broker: F,
) -> Result<WorkspaceCheckpointReport, WorkspaceCheckpointError>
where
    F: FnMut(&BrokerRequest) -> Result<BrokerResponse, String>,
{
    validate_request(request)?;
    let workspace_id = WorkspaceId::new(request.workspace_id.clone())
        .map_err(|error| WorkspaceCheckpointError::InvalidRequest(error.to_string()))?;
    let workspace = read_workspace_reservation(&config.live_sqlite_path, &workspace_id)
        .map_err(|error| WorkspaceCheckpointError::Metadata(error.to_string()))?;
    if workspace.state.as_str() != "ready" || workspace.aggregate_mount_identity.is_none() {
        return Err(WorkspaceCheckpointError::UnsafeEvidence(
            "workspace must be ready and aggregated before checkpoint registration".to_string(),
        ));
    }
    let inventory_request = BrokerRequest {
        protocol_version: PROTOCOL_VERSION,
        request_id: format!("checkpoint-{}", request.checkpoint_id),
        workspace_id: request.workspace_id.clone(),
        operation: WorkspaceHostOperation::CheckpointInventory {
            checkpoint: CheckpointPlan {
                relative_prefix: request.relative_prefix.clone(),
                max_files: request.max_files,
                max_logical_bytes: request.max_logical_bytes,
            },
        },
    };
    let response = broker(&inventory_request).map_err(WorkspaceCheckpointError::Broker)?;
    if !response.ok
        || response.protocol_version != PROTOCOL_VERSION
        || response.request_id != inventory_request.request_id
        || response.workspace_id != request.workspace_id
    {
        return Err(WorkspaceCheckpointError::UnsafeEvidence(
            response
                .error_message
                .unwrap_or_else(|| "broker response identity did not match".to_string()),
        ));
    }
    let inventory = response.checkpoint.ok_or_else(|| {
        WorkspaceCheckpointError::UnsafeEvidence("broker omitted checkpoint inventory".to_string())
    })?;
    if inventory.relative_prefix != request.relative_prefix
        || inventory.members.is_empty()
        || inventory.logical_bytes > request.max_logical_bytes
        || inventory.members.len() > request.max_files as usize
    {
        return Err(WorkspaceCheckpointError::UnsafeEvidence(
            "broker inventory exceeded or changed the requested boundary".to_string(),
        ));
    }
    let checkpoint = register_workspace_checkpoint(
        &config.live_sqlite_path,
        &RegisterWorkspaceCheckpoint {
            checkpoint_id: request.checkpoint_id.clone(),
            workspace_id: request.workspace_id.clone(),
            relative_prefix: request.relative_prefix.clone(),
            role: request.role.clone(),
            reproducibility_class: request.reproducibility_class.clone(),
            manifest_sha256: inventory.manifest_sha256,
            logical_bytes: inventory.logical_bytes,
            removable_after_promotion: request.removable_after_promotion,
            created_at_utc: request.created_at_utc.clone(),
            retention_deadline_utc: request.retention_deadline_utc.clone(),
            members: inventory
                .members
                .into_iter()
                .map(|member| WorkspaceCheckpointMember {
                    relative_path: member.relative_path,
                    size_bytes: member.size_bytes,
                    sha256: member.sha256,
                })
                .collect(),
        },
    )
    .map_err(|error| WorkspaceCheckpointError::Metadata(error.to_string()))?;
    let mut health = read_workspace_health(&config.live_sqlite_path, &request.workspace_id)
        .map_err(|error| WorkspaceCheckpointError::Metadata(error.to_string()))?;
    let aggregate_request = BrokerRequest {
        protocol_version: PROTOCOL_VERSION,
        request_id: format!("health-{}", request.checkpoint_id),
        workspace_id: request.workspace_id.clone(),
        operation: WorkspaceHostOperation::InspectAggregate {
            aggregate: aggregate_plan(&workspace)?,
        },
    };
    let aggregate_response =
        broker(&aggregate_request).map_err(WorkspaceCheckpointError::Broker)?;
    if !aggregate_response
        .aggregate
        .is_some_and(|aggregate| aggregate.state == AggregateRecoveryState::Ready)
    {
        health.health = "needs_review".to_string();
        health
            .reasons
            .push("live aggregate inspection did not prove readiness".to_string());
        health.aggregate_ready = false;
    }
    Ok(WorkspaceCheckpointReport {
        schema_version: "dasobjectstore.workspace_checkpoint_registration.v1".to_string(),
        checkpoint,
        health,
    })
}

fn aggregate_plan(
    workspace: &dasobjectstore_metadata::WorkspaceReservationSnapshot,
) -> Result<AggregatePlan, WorkspaceCheckpointError> {
    let branches = workspace
        .allocations
        .iter()
        .map(|allocation| {
            Ok(BranchPlan {
                disk_id: allocation.disk_id.to_string(),
                branch_id: allocation.branch_id.clone(),
                project_id: allocation.project_id.ok_or_else(|| {
                    WorkspaceCheckpointError::UnsafeEvidence(
                        "workspace branch has no project identity".to_string(),
                    )
                })?,
                quota_bytes: allocation.project_quota_bytes.ok_or_else(|| {
                    WorkspaceCheckpointError::UnsafeEvidence(
                        "workspace branch has no project quota".to_string(),
                    )
                })?,
            })
        })
        .collect::<Result<Vec<_>, WorkspaceCheckpointError>>()?;
    Ok(AggregatePlan {
        mount_identity: workspace.aggregate_mount_identity.clone().ok_or_else(|| {
            WorkspaceCheckpointError::UnsafeEvidence(
                "workspace aggregate identity is absent".to_string(),
            )
        })?,
        branches,
        minimum_free_bytes: workspace.minimum_free_bytes_per_disk,
    })
}

fn validate_request(request: &WorkspaceCheckpointRequest) -> Result<(), WorkspaceCheckpointError> {
    if request.max_files == 0
        || request.max_files > 4096
        || request.max_logical_bytes == 0
        || request.max_logical_bytes > 4 * 1024 * 1024 * 1024 * 1024
    {
        return Err(WorkspaceCheckpointError::InvalidRequest(
            "inventory bounds are outside the supported range".to_string(),
        ));
    }
    if !request
        .checkpoint_id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        || request.checkpoint_id.is_empty()
        || request.checkpoint_id.len() > 128
    {
        return Err(WorkspaceCheckpointError::InvalidRequest(
            "checkpoint_id must be a conservative path-free identity".to_string(),
        ));
    }
    Ok(())
}

trait WorkspaceStateName {
    fn as_str(self) -> &'static str;
}

impl WorkspaceStateName for dasobjectstore_core::workspace::ComputeWorkspaceState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            _ => "other",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{validate_request, WorkspaceCheckpointRequest};

    #[test]
    fn registration_requires_explicit_bounded_inventory() {
        assert!(validate_request(&request(4096, 1024)).is_ok());
        assert!(validate_request(&request(0, 1024)).is_err());
        assert!(validate_request(&request(4097, 1024)).is_err());
        assert!(validate_request(&request(1, 0)).is_err());
    }

    #[test]
    fn checkpoint_identity_is_path_free() {
        let mut unsafe_request = request(1, 1);
        unsafe_request.checkpoint_id = "../checkpoint".to_string();
        assert!(validate_request(&unsafe_request).is_err());
    }

    fn request(max_files: u32, max_logical_bytes: u64) -> WorkspaceCheckpointRequest {
        WorkspaceCheckpointRequest {
            checkpoint_id: "checkpoint-a".to_string(),
            workspace_id: "workspace-a".to_string(),
            relative_prefix: "outputs".to_string(),
            role: "result".to_string(),
            reproducibility_class: "derived".to_string(),
            max_files,
            max_logical_bytes,
            removable_after_promotion: true,
            created_at_utc: "2026-07-26T10:00:00Z".to_string(),
            retention_deadline_utc: "2026-08-26T10:00:00Z".to_string(),
        }
    }
}
