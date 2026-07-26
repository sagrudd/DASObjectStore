//! Durable orchestration of privileged compute-workspace provisioning.
//!
//! SQLite is the operation authority. The root-owned broker is only invoked
//! outside database transactions, and every replay begins with inspection.

use dasobjectstore_core::workspace::{
    ComputeWorkspaceState, WorkspaceOperationKind, WorkspaceOperationState,
    WorkspaceRecoveryDisposition,
};
use dasobjectstore_metadata::{
    checkpoint_workspace_operation, claim_workspace_operation, finish_workspace_operation,
    list_workspace_operations, read_workspace_reservation, recover_expired_workspace_operations,
    transition_workspace, WorkspaceOperationRecoveryAction,
};
use dasobjectstore_workspace_host::{
    request_broker, BranchInspection, BranchPlan, BrokerRequest, BrokerResponse, RecoveryState,
    WorkspaceHostOperation, PROTOCOL_VERSION,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::path::PathBuf;

pub const DEFAULT_WORKSPACE_HOST_SOCKET: &str = "/run/dasobjectstore/workspace-host.sock";

#[derive(Clone, Debug)]
pub struct WorkspaceProvisionWorkerConfig {
    pub live_sqlite_path: PathBuf,
    pub broker_socket_path: PathBuf,
    pub lease_owner: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceProvisionRecoveryReport {
    pub schema_version: String,
    pub inspected_operations: usize,
    pub completed_operations: usize,
    pub retained_for_review: usize,
    pub deferred_operations: usize,
    pub operations: Vec<WorkspaceProvisionOperationReport>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceProvisionOperationReport {
    pub operation_id: String,
    pub workspace_id: String,
    pub outcome: String,
    pub reason: String,
    pub branches: Vec<BranchInspection>,
}

#[derive(Debug)]
pub enum WorkspaceProvisionError {
    Metadata(String),
    Broker(String),
    InvalidAuthority(String),
}

impl fmt::Display for WorkspaceProvisionError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Metadata(message) => write!(output, "workspace metadata: {message}"),
            Self::Broker(message) => write!(output, "workspace host broker: {message}"),
            Self::InvalidAuthority(message) => write!(output, "workspace authority: {message}"),
        }
    }
}

impl std::error::Error for WorkspaceProvisionError {}

/// Reconcile all provision operations after daemon startup.
///
/// An unexpired lease is never stolen. Expired operations are first classified
/// by the metadata authority; only operations proven replayable are claimed.
pub fn reconcile_workspace_provision_operations(
    config: &WorkspaceProvisionWorkerConfig,
    now_utc: &str,
    lease_expires_at_utc: &str,
) -> Result<WorkspaceProvisionRecoveryReport, WorkspaceProvisionError> {
    reconcile_workspace_provision_operations_with(
        config,
        now_utc,
        lease_expires_at_utc,
        |request| {
            request_broker(&config.broker_socket_path, request).map_err(|error| error.to_string())
        },
    )
}

fn reconcile_workspace_provision_operations_with<F>(
    config: &WorkspaceProvisionWorkerConfig,
    now_utc: &str,
    lease_expires_at_utc: &str,
    mut broker: F,
) -> Result<WorkspaceProvisionRecoveryReport, WorkspaceProvisionError>
where
    F: FnMut(&BrokerRequest) -> Result<BrokerResponse, String>,
{
    let recovery = recover_expired_workspace_operations(&config.live_sqlite_path, now_utc)
        .map_err(metadata_error)?;
    let recovery_by_id = recovery
        .into_iter()
        .map(|item| (item.operation_id, item.action))
        .collect::<std::collections::BTreeMap<_, _>>();
    let operations =
        list_workspace_operations(&config.live_sqlite_path, None).map_err(metadata_error)?;
    let mut report = WorkspaceProvisionRecoveryReport {
        schema_version: "dasobjectstore.workspace_provision_recovery.v1".to_string(),
        inspected_operations: 0,
        completed_operations: 0,
        retained_for_review: 0,
        deferred_operations: 0,
        operations: Vec::new(),
    };
    for operation in operations
        .into_iter()
        .filter(|operation| operation.kind == WorkspaceOperationKind::Provision)
    {
        report.inspected_operations += 1;
        if operation.state.is_terminal() {
            report.deferred_operations += 1;
            report.operations.push(operation_report(
                &operation.operation_id,
                operation.workspace_id.as_str(),
                "terminal",
                "operation is already terminal",
                Vec::new(),
            ));
            continue;
        }
        let action = recovery_by_id
            .get(&operation.operation_id)
            .cloned()
            .unwrap_or(WorkspaceOperationRecoveryAction::Runnable);
        if !matches!(
            action,
            WorkspaceOperationRecoveryAction::Runnable
                | WorkspaceOperationRecoveryAction::ResumeFromCheckpoint
                | WorkspaceOperationRecoveryAction::RetryIdempotent
        ) {
            report.deferred_operations += 1;
            report.operations.push(operation_report(
                &operation.operation_id,
                operation.workspace_id.as_str(),
                "deferred",
                "operation is leased, cancelled, or requires review",
                Vec::new(),
            ));
            continue;
        }
        match execute_one(
            config,
            &operation.operation_id,
            now_utc,
            lease_expires_at_utc,
            &mut broker,
        ) {
            Ok(item) => {
                if item.outcome == "ready" {
                    report.completed_operations += 1;
                } else if item.outcome == "needs_review" {
                    report.retained_for_review += 1;
                } else {
                    report.deferred_operations += 1;
                }
                report.operations.push(item);
            }
            Err(error) => {
                report.deferred_operations += 1;
                report.operations.push(operation_report(
                    &operation.operation_id,
                    operation.workspace_id.as_str(),
                    "deferred",
                    &error.to_string(),
                    Vec::new(),
                ));
            }
        }
    }
    Ok(report)
}

fn execute_one<F>(
    config: &WorkspaceProvisionWorkerConfig,
    operation_id: &str,
    now_utc: &str,
    lease_expires_at_utc: &str,
    broker: &mut F,
) -> Result<WorkspaceProvisionOperationReport, WorkspaceProvisionError>
where
    F: FnMut(&BrokerRequest) -> Result<BrokerResponse, String>,
{
    let queued =
        dasobjectstore_metadata::read_workspace_operation(&config.live_sqlite_path, operation_id)
            .map_err(metadata_error)?;
    let mut operation = claim_workspace_operation(
        &config.live_sqlite_path,
        operation_id,
        &config.lease_owner,
        queued.generation,
        now_utc,
        lease_expires_at_utc,
    )
    .map_err(metadata_error)?;
    let mut workspace =
        read_workspace_reservation(&config.live_sqlite_path, &operation.workspace_id)
            .map_err(metadata_error)?;
    let branches = workspace
        .allocations
        .iter()
        .map(|allocation| {
            Ok(BranchPlan {
                disk_id: allocation.disk_id.as_str().to_string(),
                branch_id: allocation.branch_id.clone(),
                project_id: allocation.project_id.ok_or_else(|| {
                    WorkspaceProvisionError::InvalidAuthority(format!(
                        "branch {} has no allocated project identity",
                        allocation.branch_id
                    ))
                })?,
                quota_bytes: allocation.project_quota_bytes.ok_or_else(|| {
                    WorkspaceProvisionError::InvalidAuthority(format!(
                        "branch {} has no allocated project quota",
                        allocation.branch_id
                    ))
                })?,
            })
        })
        .collect::<Result<Vec<_>, WorkspaceProvisionError>>()?;

    let inspected = call_broker(
        broker,
        operation_id,
        operation.workspace_id.as_str(),
        WorkspaceHostOperation::Inspect {
            branches: branches.clone(),
        },
    )?;
    if has_unsafe_state(&inspected) {
        let result = serde_json::to_string(&inspected).map_err(protocol_error)?;
        finish_workspace_operation(
            &config.live_sqlite_path,
            operation_id,
            &config.lease_owner,
            operation.generation,
            WorkspaceOperationState::NeedsReview,
            Some(&result),
            Some("workspace_host_state_ambiguous"),
            Some("host inspection found a marker or filesystem safety conflict"),
            now_utc,
        )
        .map_err(metadata_error)?;
        return Ok(operation_report(
            operation_id,
            operation.workspace_id.as_str(),
            "needs_review",
            "unsafe host state retained without rollback",
            inspected,
        ));
    }

    if workspace.state == ComputeWorkspaceState::CapacityReserved {
        workspace = transition_workspace(
            &config.live_sqlite_path,
            &operation.workspace_id,
            workspace.generation,
            ComputeWorkspaceState::Provisioning,
            now_utc,
            None,
        )
        .map_err(metadata_error)?;
    }
    if !inspected.iter().all(is_ready) {
        call_broker(
            broker,
            operation_id,
            operation.workspace_id.as_str(),
            WorkspaceHostOperation::Provision {
                branches: branches.clone(),
            },
        )?;
    }
    let verified = call_broker(
        broker,
        operation_id,
        operation.workspace_id.as_str(),
        WorkspaceHostOperation::Inspect {
            branches: branches.clone(),
        },
    )?;
    if !verified.iter().all(is_ready) {
        let rollback = call_broker(
            broker,
            &format!("{operation_id}.rollback"),
            operation.workspace_id.as_str(),
            WorkspaceHostOperation::Rollback {
                branches: branches.clone(),
            },
        );
        let reason = match rollback {
            Ok(_) => "provision verification failed; empty marker-owned branches rolled back",
            Err(_) => "provision verification failed; unsafe rollback retained for review",
        };
        let result = serde_json::to_string(&verified).map_err(protocol_error)?;
        finish_workspace_operation(
            &config.live_sqlite_path,
            operation_id,
            &config.lease_owner,
            operation.generation,
            WorkspaceOperationState::NeedsReview,
            Some(&result),
            Some("workspace_provision_verification_failed"),
            Some(reason),
            now_utc,
        )
        .map_err(metadata_error)?;
        return Ok(operation_report(
            operation_id,
            operation.workspace_id.as_str(),
            "needs_review",
            reason,
            verified,
        ));
    }
    let checkpoint_json = serde_json::to_string(&verified).map_err(protocol_error)?;
    let checkpoint_digest = hex_sha256(checkpoint_json.as_bytes());
    operation = checkpoint_workspace_operation(
        &config.live_sqlite_path,
        operation_id,
        &config.lease_owner,
        operation.generation,
        "host_provisioned",
        0,
        branches.len() as u64,
        WorkspaceRecoveryDisposition::ResumeCheckpoint,
        &checkpoint_digest,
        &checkpoint_json,
        now_utc,
    )
    .map_err(metadata_error)?;
    if workspace.state == ComputeWorkspaceState::Provisioning {
        transition_workspace(
            &config.live_sqlite_path,
            &operation.workspace_id,
            workspace.generation,
            ComputeWorkspaceState::Ready,
            now_utc,
            None,
        )
        .map_err(metadata_error)?;
    }
    let result = serde_json::to_string(&verified).map_err(protocol_error)?;
    finish_workspace_operation(
        &config.live_sqlite_path,
        operation_id,
        &config.lease_owner,
        operation.generation,
        WorkspaceOperationState::Succeeded,
        Some(&result),
        None,
        None,
        now_utc,
    )
    .map_err(metadata_error)?;
    Ok(operation_report(
        operation_id,
        operation.workspace_id.as_str(),
        "ready",
        "broker state verified and workspace published ready",
        verified,
    ))
}

fn call_broker<F>(
    broker: &mut F,
    request_id: &str,
    workspace_id: &str,
    operation: WorkspaceHostOperation,
) -> Result<Vec<BranchInspection>, WorkspaceProvisionError>
where
    F: FnMut(&BrokerRequest) -> Result<BrokerResponse, String>,
{
    let expected = match &operation {
        WorkspaceHostOperation::Provision { branches }
        | WorkspaceHostOperation::Inspect { branches }
        | WorkspaceHostOperation::Rollback { branches } => branches
            .iter()
            .map(|branch| (branch.disk_id.clone(), branch.branch_id.clone()))
            .collect::<std::collections::BTreeSet<_>>(),
    };
    let request = BrokerRequest {
        protocol_version: PROTOCOL_VERSION,
        request_id: request_id.to_string(),
        workspace_id: workspace_id.to_string(),
        operation,
    };
    let response = broker(&request).map_err(WorkspaceProvisionError::Broker)?;
    if response.protocol_version != PROTOCOL_VERSION
        || response.request_id != request.request_id
        || response.workspace_id != request.workspace_id
        || !response.ok
    {
        return Err(WorkspaceProvisionError::Broker(
            response
                .error_message
                .unwrap_or_else(|| "broker response identity or protocol mismatch".to_string()),
        ));
    }
    let observed = response
        .branches
        .iter()
        .map(|branch| (branch.disk_id.clone(), branch.branch_id.clone()))
        .collect::<std::collections::BTreeSet<_>>();
    if response.branches.len() != expected.len() || observed != expected {
        return Err(WorkspaceProvisionError::Broker(
            "broker response did not contain the exact requested branch identities".to_string(),
        ));
    }
    Ok(response.branches)
}

fn is_ready(branch: &BranchInspection) -> bool {
    branch.state == RecoveryState::Ready && branch.marker_matches && branch.quota_enforced
}

fn has_unsafe_state(branches: &[BranchInspection]) -> bool {
    branches.iter().any(|branch| {
        matches!(
            branch.state,
            RecoveryState::MarkerMissing
                | RecoveryState::MarkerConflict
                | RecoveryState::UnsafeFilesystemEntry
        )
    })
}

fn operation_report(
    operation_id: &str,
    workspace_id: &str,
    outcome: &str,
    reason: &str,
    branches: Vec<BranchInspection>,
) -> WorkspaceProvisionOperationReport {
    WorkspaceProvisionOperationReport {
        operation_id: operation_id.to_string(),
        workspace_id: workspace_id.to_string(),
        outcome: outcome.to_string(),
        reason: reason.to_string(),
        branches,
    }
}

fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn metadata_error(error: impl fmt::Display) -> WorkspaceProvisionError {
    WorkspaceProvisionError::Metadata(error.to_string())
}

fn protocol_error(error: impl fmt::Display) -> WorkspaceProvisionError {
    WorkspaceProvisionError::InvalidAuthority(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dasobjectstore_core::ids::{DiskId, PoolId, WorkspaceId};
    use dasobjectstore_core::lifecycle::HealthState;
    use dasobjectstore_metadata::{
        reserve_workspace, submit_workspace_operation, MeasuredWorkspaceDisk,
        ReserveWorkspaceRequest, SubmitWorkspaceOperationRequest, LIVE_SCHEMA_SQL,
    };
    use rusqlite::Connection;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn unsafe_states_are_never_treated_as_replayable() {
        for state in [
            RecoveryState::MarkerMissing,
            RecoveryState::MarkerConflict,
            RecoveryState::UnsafeFilesystemEntry,
        ] {
            assert!(has_unsafe_state(&[BranchInspection {
                disk_id: "disk-a".to_string(),
                branch_id: "branch-a".to_string(),
                state,
                marker_matches: false,
                quota_enforced: false,
            }]));
        }
    }

    #[test]
    fn readiness_requires_marker_and_quota_evidence() {
        let mut branch = BranchInspection {
            disk_id: "disk-a".to_string(),
            branch_id: "branch-a".to_string(),
            state: RecoveryState::Ready,
            marker_matches: true,
            quota_enforced: true,
        };
        assert!(is_ready(&branch));
        branch.quota_enforced = false;
        assert!(!is_ready(&branch));
    }

    #[test]
    fn checkpoint_digest_is_stable() {
        assert_eq!(hex_sha256(b"workspace"), hex_sha256(b"workspace"));
        assert_ne!(hex_sha256(b"workspace"), hex_sha256(b"other"));
    }

    #[test]
    fn broker_response_must_cover_exact_requested_branches() {
        let mut broker = |request: &BrokerRequest| {
            let mut response = response(request, RecoveryState::Ready);
            response.branches.clear();
            Ok(response)
        };
        assert!(call_broker(
            &mut broker,
            "operation-a",
            "workspace-a",
            WorkspaceHostOperation::Inspect {
                branches: vec![BranchPlan {
                    disk_id: "disk-a".to_string(),
                    branch_id: "branch-a".to_string(),
                    project_id: 1001,
                    quota_bytes: 4096,
                }],
            },
        )
        .is_err());
    }

    #[test]
    fn provision_worker_inspects_provisions_verifies_and_publishes_ready() {
        let (root, config) = fixture("success");
        let mut calls = Vec::new();
        let report = reconcile_workspace_provision_operations_with(
            &config,
            "2026-07-26T00:01:00Z",
            "2026-07-26T00:02:00Z",
            |request| {
                calls.push(match &request.operation {
                    WorkspaceHostOperation::Inspect { .. } => "inspect",
                    WorkspaceHostOperation::Provision { .. } => "provision",
                    WorkspaceHostOperation::Rollback { .. } => "rollback",
                });
                let state = if calls.len() == 1 {
                    RecoveryState::Absent
                } else {
                    RecoveryState::Ready
                };
                Ok(response(request, state))
            },
        )
        .expect("reconcile");
        assert_eq!(calls, vec!["inspect", "provision", "inspect"]);
        assert_eq!(report.completed_operations, 1);
        assert_eq!(
            read_workspace_reservation(
                &config.live_sqlite_path,
                &WorkspaceId::new("workspace-a").expect("workspace id")
            )
            .expect("workspace")
            .state,
            ComputeWorkspaceState::Ready
        );
        assert_eq!(
            dasobjectstore_metadata::read_workspace_operation(
                &config.live_sqlite_path,
                "operation-a"
            )
            .expect("operation")
            .state,
            WorkspaceOperationState::Succeeded
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn restart_inspection_retains_marker_conflict_without_provision_or_rollback() {
        let (root, config) = fixture("conflict");
        let mut calls = 0;
        let report = reconcile_workspace_provision_operations_with(
            &config,
            "2026-07-26T00:01:00Z",
            "2026-07-26T00:02:00Z",
            |request| {
                calls += 1;
                Ok(response(request, RecoveryState::MarkerConflict))
            },
        )
        .expect("reconcile");
        assert_eq!(calls, 1);
        assert_eq!(report.retained_for_review, 1);
        assert_eq!(
            dasobjectstore_metadata::read_workspace_operation(
                &config.live_sqlite_path,
                "operation-a"
            )
            .expect("operation")
            .state,
            WorkspaceOperationState::NeedsReview
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    fn response(request: &BrokerRequest, state: RecoveryState) -> BrokerResponse {
        let branch = match &request.operation {
            WorkspaceHostOperation::Provision { branches }
            | WorkspaceHostOperation::Inspect { branches }
            | WorkspaceHostOperation::Rollback { branches } => &branches[0],
        };
        BrokerResponse {
            protocol_version: PROTOCOL_VERSION,
            request_id: request.request_id.clone(),
            workspace_id: request.workspace_id.clone(),
            ok: true,
            error_code: None,
            error_message: None,
            branches: vec![BranchInspection {
                disk_id: branch.disk_id.clone(),
                branch_id: branch.branch_id.clone(),
                state,
                marker_matches: state == RecoveryState::Ready,
                quota_enforced: state == RecoveryState::Ready,
            }],
        }
    }

    fn fixture(name: &str) -> (PathBuf, WorkspaceProvisionWorkerConfig) {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-workspace-worker-{name}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("root");
        let database = root.join("live.sqlite");
        let connection = Connection::open(&database).expect("database");
        connection.execute_batch(LIVE_SCHEMA_SQL).expect("schema");
        connection
            .execute_batch(
                "INSERT INTO pools (pool_id, state, created_at_utc, updated_at_utc)
                 VALUES ('pool-a', 'Clean', '2026-07-26T00:00:00Z', '2026-07-26T00:00:00Z');
                 INSERT INTO disks (
                    disk_id, pool_id, role, state, created_at_utc, updated_at_utc
                 ) VALUES (
                    'disk-a', 'pool-a', 'hdd_capacity', 'Healthy',
                    '2026-07-26T00:00:00Z', '2026-07-26T00:00:00Z'
                 );",
            )
            .expect("authority");
        drop(connection);
        let workspace_id = WorkspaceId::new("workspace-a").expect("workspace id");
        reserve_workspace(&ReserveWorkspaceRequest {
            live_sqlite_path: database.clone(),
            workspace_id: workspace_id.clone(),
            request_id: "reservation-a".to_string(),
            request_digest: "a".repeat(64),
            pool_id: PoolId::new("pool-a").expect("pool id"),
            promotion_store_id: None,
            owner: "operator".to_string(),
            project: "fixture".to_string(),
            purpose: "test".to_string(),
            requested_capacity_bytes: 4096,
            quota_bytes: 4096,
            minimum_free_bytes_per_disk: 0,
            aggregation_provider: "mergerfs".to_string(),
            close_cleanup_policy_json: "{}".to_string(),
            workflow_id: None,
            workflow_run_id: None,
            repository_revision: None,
            created_at_utc: "2026-07-26T00:00:00Z".to_string(),
            expires_at_utc: "2026-07-27T00:00:00Z".to_string(),
            candidates: vec![MeasuredWorkspaceDisk {
                disk_id: DiskId::new("disk-a").expect("disk id"),
                health_state: HealthState::Healthy,
                available_bytes: 8192,
            }],
        })
        .expect("reserve");
        submit_workspace_operation(&SubmitWorkspaceOperationRequest {
            live_sqlite_path: database.clone(),
            operation_id: "operation-a".to_string(),
            workspace_id,
            kind: WorkspaceOperationKind::Provision,
            request_id: "provision-a".to_string(),
            request_digest: "b".repeat(64),
            initial_stage: "reserved".to_string(),
            total_bytes: None,
            total_units: Some(1),
            max_attempts: 3,
            recovery_disposition: WorkspaceRecoveryDisposition::RetryIdempotent,
            created_at_utc: "2026-07-26T00:00:00Z".to_string(),
        })
        .expect("operation");
        (
            root,
            WorkspaceProvisionWorkerConfig {
                live_sqlite_path: database,
                broker_socket_path: PathBuf::from("/unused"),
                lease_owner: "worker-a".to_string(),
            },
        )
    }
}
