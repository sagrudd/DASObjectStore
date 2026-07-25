//! Managed mutable compute workspace domain contracts.
//!
//! Workspaces reserve temporary capacity but never become immutable object
//! placements. Provider-specific mount and export details remain daemon-owned.

use crate::ids::{DiskId, ObjectId, StoreId, WorkspaceId};
use crate::lifecycle::{DiskState, HealthState};
use serde::{Deserialize, Serialize};
use std::fmt;

pub const COMPUTE_WORKSPACE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputeWorkspaceState {
    Requested,
    CapacityReserved,
    Provisioning,
    Ready,
    Attached,
    Active,
    PromotionPending,
    Closing,
    Closed,
    Expired,
    CleanupPending,
    Cleaned,
    Failed,
}

impl ComputeWorkspaceState {
    pub fn can_transition_to(self, next: Self) -> bool {
        use ComputeWorkspaceState as S;
        matches!(
            (self, next),
            (S::Requested, S::CapacityReserved)
                | (S::CapacityReserved, S::Provisioning)
                | (S::Provisioning, S::Ready)
                | (S::Ready, S::Attached)
                | (S::Attached, S::Active)
                | (S::Active, S::Ready)
                | (S::Active | S::Ready | S::Attached, S::PromotionPending)
                | (S::PromotionPending, S::Ready)
                | (S::Ready | S::Attached, S::Closing)
                | (S::Closing, S::Closed)
                | (S::Closed | S::Expired, S::CleanupPending)
                | (S::CleanupPending, S::Cleaned)
                | (
                    S::Requested
                        | S::CapacityReserved
                        | S::Provisioning
                        | S::Ready
                        | S::Attached
                        | S::Active
                        | S::PromotionPending
                        | S::Closing,
                    S::Failed
                )
                | (
                    S::CapacityReserved
                        | S::Provisioning
                        | S::Ready
                        | S::Attached
                        | S::Active
                        | S::PromotionPending,
                    S::Expired
                )
        )
    }

    pub fn transition_to(self, next: Self) -> Result<Self, WorkspaceTransitionError> {
        self.can_transition_to(next)
            .then_some(next)
            .ok_or(WorkspaceTransitionError {
                current: self,
                next,
            })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceTransitionError {
    pub current: ComputeWorkspaceState,
    pub next: ComputeWorkspaceState,
}

impl fmt::Display for WorkspaceTransitionError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            output,
            "invalid compute workspace transition: {:?} -> {:?}",
            self.current, self.next
        )
    }
}

impl std::error::Error for WorkspaceTransitionError {}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ComputeClientIdentity {
    pub client_id: String,
    pub address_or_cidr: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceBranch {
    pub disk_id: DiskId,
    /// Opaque branch identity. Ordinary client reports must not turn this into
    /// a host placement path.
    pub branch_id: String,
    /// Daemon-private, validated relative location beneath the managed disk
    /// root. It is populated during provisioning and is never serialized in
    /// ordinary client reports.
    pub branch_relative_path: Option<String>,
    pub reserved_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceMaterialization {
    pub source_object_id: ObjectId,
    pub destination: String,
    pub expected_size_bytes: u64,
    pub expected_sha256: String,
    pub observed_sha256: Option<String>,
    pub completed_at_utc: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceCheckpoint {
    pub relative_prefix: String,
    pub role: String,
    pub retention_deadline_utc: String,
    pub logical_bytes: Option<u64>,
    pub removable_after_promotion: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspacePromotedOutput {
    pub source_relative_path: String,
    pub store_id: StoreId,
    pub object_id: ObjectId,
    pub sha256: String,
    pub size_bytes: u64,
    pub parent_object_ids: Vec<ObjectId>,
    pub accepted_at_utc: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ComputeWorkspace {
    pub workspace_id: WorkspaceId,
    pub schema_version: u32,
    pub state: ComputeWorkspaceState,
    pub owner: String,
    pub project: String,
    pub purpose: String,
    pub created_at_utc: String,
    pub expires_at_utc: String,
    pub requested_capacity_bytes: u64,
    pub reserved_capacity_bytes: u64,
    pub quota_bytes: u64,
    pub bytes_written: u64,
    pub bytes_reclaimable: u64,
    pub minimum_free_bytes_per_backing_disk: u64,
    pub branches: Vec<WorkspaceBranch>,
    pub aggregate_namespace_id: Option<String>,
    pub permitted_clients: Vec<ComputeClientIdentity>,
    pub materializations: Vec<WorkspaceMaterialization>,
    pub checkpoints: Vec<WorkspaceCheckpoint>,
    pub promoted_outputs: Vec<WorkspacePromotedOutput>,
    pub required_output_ids: Vec<ObjectId>,
    pub workflow_id: Option<String>,
    pub workflow_run_id: Option<String>,
    pub repository_revision: Option<String>,
    pub audit_event_ids: Vec<String>,
    pub close_cleanup_policy: String,
    pub failure_reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceCapacityCandidate {
    pub disk_id: DiskId,
    pub disk_state: DiskState,
    pub health_state: HealthState,
    pub available_bytes: u64,
    pub already_reserved_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceCapacityPlan {
    pub requested_bytes: u64,
    pub branches: Vec<WorkspaceBranch>,
}

impl WorkspaceCapacityPlan {
    pub fn reserved_bytes(&self) -> u64 {
        self.branches
            .iter()
            .map(|branch| branch.reserved_bytes)
            .sum()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceCapacityPlanError {
    ZeroCapacity,
    InsufficientAggregateCapacity {
        requested_bytes: u64,
        available_bytes: u64,
    },
}

/// Select deterministic healthy branches and reserve aggregate capacity.
///
/// The caller must commit this plan and the workspace row in one metadata
/// transaction before any provider directories are created.
pub fn plan_workspace_capacity(
    candidates: &[WorkspaceCapacityCandidate],
    requested_bytes: u64,
    minimum_free_bytes_per_disk: u64,
) -> Result<WorkspaceCapacityPlan, WorkspaceCapacityPlanError> {
    if requested_bytes == 0 {
        return Err(WorkspaceCapacityPlanError::ZeroCapacity);
    }
    let mut eligible = candidates
        .iter()
        .filter(|candidate| {
            candidate.disk_state == DiskState::Healthy
                && candidate.health_state == HealthState::Healthy
        })
        .map(|candidate| {
            let reservable = candidate
                .available_bytes
                .saturating_sub(candidate.already_reserved_bytes)
                .saturating_sub(minimum_free_bytes_per_disk);
            (candidate.disk_id.clone(), reservable)
        })
        .filter(|(_, reservable)| *reservable > 0)
        .collect::<Vec<_>>();
    eligible.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    let aggregate = eligible.iter().map(|(_, bytes)| *bytes).sum::<u64>();
    if aggregate < requested_bytes {
        return Err(WorkspaceCapacityPlanError::InsufficientAggregateCapacity {
            requested_bytes,
            available_bytes: aggregate,
        });
    }
    let mut remaining = requested_bytes;
    let branches = eligible
        .into_iter()
        .filter_map(|(disk_id, available)| {
            if remaining == 0 {
                return None;
            }
            let reserved_bytes = available.min(remaining);
            remaining -= reserved_bytes;
            Some(WorkspaceBranch {
                branch_id: format!("workspace-branch-{disk_id}"),
                disk_id,
                branch_relative_path: None,
                reserved_bytes,
            })
        })
        .collect();
    Ok(WorkspaceCapacityPlan {
        requested_bytes,
        branches,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(
        id: &str,
        available_bytes: u64,
        disk_state: DiskState,
    ) -> WorkspaceCapacityCandidate {
        WorkspaceCapacityCandidate {
            disk_id: DiskId::new(id).unwrap(),
            disk_state,
            health_state: HealthState::Healthy,
            available_bytes,
            already_reserved_bytes: 0,
        }
    }

    #[test]
    fn plans_capacity_larger_than_any_individual_disk() {
        let plan = plan_workspace_capacity(
            &[
                candidate("disk-a", 700, DiskState::Healthy),
                candidate("disk-b", 600, DiskState::Healthy),
                candidate("disk-c", 500, DiskState::Healthy),
            ],
            1_400,
            100,
        )
        .unwrap();
        assert_eq!(plan.reserved_bytes(), 1_400);
        assert_eq!(plan.branches.len(), 3);
    }

    #[test]
    fn excludes_draining_and_unhealthy_disks() {
        let mut unhealthy = candidate("disk-b", 900, DiskState::Healthy);
        unhealthy.health_state = HealthState::Suspect;
        let error = plan_workspace_capacity(
            &[candidate("disk-a", 900, DiskState::Draining), unhealthy],
            1,
            0,
        )
        .unwrap_err();
        assert_eq!(
            error,
            WorkspaceCapacityPlanError::InsufficientAggregateCapacity {
                requested_bytes: 1,
                available_bytes: 0
            }
        );
    }

    #[test]
    fn subtracts_existing_reservations_and_minimum_free_space() {
        let mut disk = candidate("disk-a", 1_000, DiskState::Healthy);
        disk.already_reserved_bytes = 300;
        let plan = plan_workspace_capacity(&[disk], 500, 200).unwrap();
        assert_eq!(plan.reserved_bytes(), 500);
    }

    #[test]
    fn transition_graph_rejects_skipping_provisioning() {
        assert!(ComputeWorkspaceState::Requested
            .transition_to(ComputeWorkspaceState::Ready)
            .is_err());
        assert_eq!(
            ComputeWorkspaceState::Requested
                .transition_to(ComputeWorkspaceState::CapacityReserved)
                .unwrap(),
            ComputeWorkspaceState::CapacityReserved
        );
    }
}
