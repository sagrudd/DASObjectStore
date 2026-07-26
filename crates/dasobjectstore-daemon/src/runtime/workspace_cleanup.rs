//! Restart-safe execution of explicitly confirmed workspace cleanup.

use dasobjectstore_core::workspace::{WorkspaceOperationKind, WorkspaceOperationState};
use dasobjectstore_metadata::{
    cancel_workspace_cleanup, claim_workspace_operation, complete_workspace_cleanup,
    finish_workspace_operation, list_workspace_operations, read_cleanup_plan,
    record_workspace_branch_removed, recover_expired_workspace_operations,
    report_expired_workspaces, WorkspaceExpiryCandidate,
};
use dasobjectstore_workspace_host::{
    request_broker, AggregatePlan, BranchPlan, BrokerRequest, BrokerResponse, RecoveryState,
    WorkspaceHostOperation, PROTOCOL_VERSION,
};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct WorkspaceCleanupWorkerConfig {
    pub live_sqlite_path: PathBuf,
    pub broker_socket_path: PathBuf,
    pub lease_owner: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceCleanupRecoveryReport {
    pub schema_version: String,
    pub inspected_operations: usize,
    pub completed_operations: usize,
    pub deferred_operations: usize,
    pub expiry_candidates: Vec<WorkspaceExpiryCandidate>,
    pub operations: Vec<WorkspaceCleanupOutcome>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceCleanupOutcome {
    pub operation_id: String,
    pub workspace_id: String,
    pub state: String,
    pub reason: String,
}

#[derive(Debug)]
pub struct WorkspaceCleanupError(String);

impl fmt::Display for WorkspaceCleanupError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(&self.0)
    }
}
impl std::error::Error for WorkspaceCleanupError {}

pub fn reconcile_workspace_cleanups(
    config: &WorkspaceCleanupWorkerConfig,
    now_utc: &str,
    lease_expires_at_utc: &str,
) -> Result<WorkspaceCleanupRecoveryReport, WorkspaceCleanupError> {
    recover_expired_workspace_operations(&config.live_sqlite_path, now_utc).map_err(error)?;
    let operations = list_workspace_operations(&config.live_sqlite_path, None).map_err(error)?;
    let mut report = WorkspaceCleanupRecoveryReport {
        schema_version: "dasobjectstore.workspace_cleanup_recovery.v1".to_string(),
        inspected_operations: 0,
        completed_operations: 0,
        deferred_operations: 0,
        expiry_candidates: report_expired_workspaces(&config.live_sqlite_path, now_utc)
            .map_err(error)?,
        operations: Vec::new(),
    };
    for operation in operations
        .into_iter()
        .filter(|operation| operation.kind == WorkspaceOperationKind::Cleanup)
    {
        report.inspected_operations += 1;
        if operation.state == WorkspaceOperationState::Cancelled {
            cancel_workspace_cleanup(
                &config.live_sqlite_path,
                &operation.workspace_id,
                &operation.operation_id,
                &config.lease_owner,
                now_utc,
            )
            .map_err(error)?;
            continue;
        }
        if operation.state.is_terminal() {
            continue;
        }
        if operation.cancellation_requested {
            if operation.state == WorkspaceOperationState::Running
                && operation.lease_owner.as_deref() == Some(&config.lease_owner)
            {
                finish_workspace_operation(
                    &config.live_sqlite_path,
                    &operation.operation_id,
                    &config.lease_owner,
                    operation.generation,
                    WorkspaceOperationState::NeedsReview,
                    None,
                    Some("cleanup_cancellation_ambiguous"),
                    Some("cleanup may have crossed the external deletion boundary"),
                    now_utc,
                )
                .map_err(error)?;
                report.deferred_operations += 1;
                report.operations.push(outcome(
                    &operation.operation_id,
                    operation.workspace_id.as_str(),
                    "needs_review",
                    "cancellation arrived after cleanup execution began",
                ));
                continue;
            }
            let result = cancel_workspace_cleanup(
                &config.live_sqlite_path,
                &operation.workspace_id,
                &operation.operation_id,
                &config.lease_owner,
                now_utc,
            );
            let (state, reason) = match result {
                Ok(_) => (
                    "cancelled",
                    "cancelled before any branch release".to_string(),
                ),
                Err(reason) => {
                    report.deferred_operations += 1;
                    (
                        "needs_review",
                        format!("cancellation requires review: {reason}"),
                    )
                }
            };
            report.operations.push(outcome(
                &operation.operation_id,
                operation.workspace_id.as_str(),
                state,
                &reason,
            ));
            continue;
        }
        if !matches!(
            operation.state,
            WorkspaceOperationState::Queued | WorkspaceOperationState::RetryWait
        ) {
            report.deferred_operations += 1;
            continue;
        }
        let claimed = claim_workspace_operation(
            &config.live_sqlite_path,
            &operation.operation_id,
            &config.lease_owner,
            operation.generation,
            now_utc,
            lease_expires_at_utc,
        )
        .map_err(error)?;
        let plan =
            read_cleanup_plan(&config.live_sqlite_path, &operation.workspace_id).map_err(error)?;
        let branches = plan
            .branches
            .iter()
            .map(|branch| BranchPlan {
                disk_id: branch.disk_id.clone(),
                branch_id: branch.branch_id.clone(),
                project_id: branch.project_id,
                quota_bytes: branch.quota_bytes,
            })
            .collect::<Vec<_>>();
        if let Some(mount_identity) = plan.aggregate_mount_identity {
            call_broker(
                config,
                &operation.operation_id,
                operation.workspace_id.as_str(),
                WorkspaceHostOperation::UnmountAggregate {
                    aggregate: AggregatePlan {
                        mount_identity,
                        branches: branches.clone(),
                        minimum_free_bytes: plan.minimum_free_bytes_per_disk,
                    },
                },
            )?;
        }
        let response = call_broker(
            config,
            &operation.operation_id,
            operation.workspace_id.as_str(),
            WorkspaceHostOperation::Cleanup {
                branches: branches.clone(),
            },
        )?;
        if response.branches.len() != branches.len()
            || response
                .branches
                .iter()
                .any(|branch| branch.state != RecoveryState::Absent)
        {
            return Err(WorkspaceCleanupError(
                "broker did not prove every marker-owned branch absent".to_string(),
            ));
        }
        for branch in &branches {
            record_workspace_branch_removed(
                &config.live_sqlite_path,
                &operation.workspace_id,
                &branch.disk_id,
                &operation.operation_id,
                &config.lease_owner,
                now_utc,
            )
            .map_err(error)?;
        }
        complete_workspace_cleanup(
            &config.live_sqlite_path,
            &operation.workspace_id,
            &operation.operation_id,
            &config.lease_owner,
            now_utc,
        )
        .map_err(error)?;
        report.completed_operations += 1;
        report.operations.push(outcome(
            &claimed.operation_id,
            claimed.workspace_id.as_str(),
            "cleaned",
            "marker-owned branches removed and capacity authority released",
        ));
    }
    Ok(report)
}

fn call_broker(
    config: &WorkspaceCleanupWorkerConfig,
    request_id: &str,
    workspace_id: &str,
    operation: WorkspaceHostOperation,
) -> Result<BrokerResponse, WorkspaceCleanupError> {
    let request = BrokerRequest {
        protocol_version: PROTOCOL_VERSION,
        request_id: format!("cleanup-{request_id}"),
        workspace_id: workspace_id.to_string(),
        operation,
    };
    let response = request_broker(&config.broker_socket_path, &request).map_err(error)?;
    if !response.ok
        || response.protocol_version != PROTOCOL_VERSION
        || response.request_id != request.request_id
        || response.workspace_id != workspace_id
    {
        return Err(WorkspaceCleanupError(
            response
                .error_message
                .unwrap_or_else(|| "broker cleanup identity mismatch".to_string()),
        ));
    }
    Ok(response)
}

fn outcome(
    operation_id: &str,
    workspace_id: &str,
    state: &str,
    reason: &str,
) -> WorkspaceCleanupOutcome {
    WorkspaceCleanupOutcome {
        operation_id: operation_id.to_string(),
        workspace_id: workspace_id.to_string(),
        state: state.to_string(),
        reason: reason.to_string(),
    }
}

fn error(value: impl ToString) -> WorkspaceCleanupError {
    WorkspaceCleanupError(value.to_string())
}
