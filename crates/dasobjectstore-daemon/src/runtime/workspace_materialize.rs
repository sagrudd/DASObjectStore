use super::workspace_provision::{WorkspaceProvisionError, WorkspaceProvisionWorkerConfig};
use dasobjectstore_core::utc::{add_seconds_to_utc_timestamp, format_utc_timestamp_seconds};
use dasobjectstore_core::workspace::{
    WorkspaceOperationKind, WorkspaceOperationState, WorkspaceRecoveryDisposition,
};
use dasobjectstore_metadata::{
    checkpoint_workspace_operation, claim_workspace_operation, finish_workspace_materialization,
    list_active_workspace_materializations, list_workspace_operations,
    publish_workspace_materialization_state, read_workspace_operation,
    renew_workspace_operation_lease,
};
use dasobjectstore_workspace_host::{
    request_broker, BrokerRequest, MaterializationPlan, MaterializationRecoveryState,
    WorkspaceHostOperation, PROTOCOL_VERSION,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceMaterializationReport {
    pub operation_id: String,
    pub workspace_id: String,
    pub state: String,
    pub completed_bytes: u64,
    pub expected_size_bytes: u64,
    pub reason: String,
}

pub fn reconcile_workspace_materializations(
    config: &WorkspaceProvisionWorkerConfig,
) -> Result<Vec<WorkspaceMaterializationReport>, WorkspaceProvisionError> {
    let materializations = list_active_workspace_materializations(&config.live_sqlite_path)
        .map_err(|error| WorkspaceProvisionError::Metadata(error.to_string()))?;
    let operations = list_workspace_operations(&config.live_sqlite_path, None)
        .map_err(|error| WorkspaceProvisionError::Metadata(error.to_string()))?;
    let generations = operations
        .into_iter()
        .filter(|operation| {
            operation.kind == WorkspaceOperationKind::Materialize
                && operation.state == WorkspaceOperationState::Queued
        })
        .map(|operation| (operation.operation_id, operation.generation))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut reports = Vec::new();
    for materialization in materializations {
        let Some(generation) = generations.get(&materialization.operation_id).copied() else {
            continue;
        };
        reports.push(execute_materialization(
            config,
            materialization,
            generation,
        )?);
    }
    Ok(reports)
}

fn execute_materialization(
    config: &WorkspaceProvisionWorkerConfig,
    materialization: dasobjectstore_metadata::WorkspaceMaterializationSnapshot,
    generation: u64,
) -> Result<WorkspaceMaterializationReport, WorkspaceProvisionError> {
    let claimed_at = now_utc();
    let expires = add_seconds_to_utc_timestamp(&claimed_at, 60)
        .ok_or_else(|| WorkspaceProvisionError::InvalidAuthority("invalid clock".to_string()))?;
    let mut operation = claim_workspace_operation(
        &config.live_sqlite_path,
        &materialization.operation_id,
        &config.lease_owner,
        generation,
        &claimed_at,
        &expires,
    )
    .map_err(|error| WorkspaceProvisionError::Metadata(error.to_string()))?;
    if materialization.state == "queued" {
        publish_workspace_materialization_state(
            &config.live_sqlite_path,
            &materialization.operation_id,
            "queued",
            "copying",
            None,
            None,
        )
        .map_err(|error| WorkspaceProvisionError::Metadata(error.to_string()))?;
    }
    let plan = MaterializationPlan {
        source_object_id: materialization.source_object_id.clone(),
        source_placement_id: materialization.source_placement_id.clone(),
        destination_relative_path: materialization.destination_relative_path.clone(),
        expected_size_bytes: materialization.expected_size_bytes,
        expected_sha256: materialization.expected_sha256.clone(),
    };
    loop {
        let current =
            read_workspace_operation(&config.live_sqlite_path, &materialization.operation_id)
                .map_err(|error| WorkspaceProvisionError::Metadata(error.to_string()))?;
        if current.cancellation_requested {
            let now = now_utc();
            finish_workspace_materialization(
                &config.live_sqlite_path,
                &materialization.operation_id,
                &config.lease_owner,
                operation.generation,
                "cancelled",
                "cancelled",
                None,
                None,
                Some("workspace_materialization_cancelled"),
                Some("cancellation was observed between durable copy steps"),
                &now,
            )
            .map_err(|error| WorkspaceProvisionError::Metadata(error.to_string()))?;
            return Ok(report(
                &materialization,
                "cancelled",
                operation.completed_bytes,
                "cancelled partial retained for governed cleanup",
            ));
        }
        let request_id = broker_request_id(&materialization.operation_id, operation.generation);
        let inspect = call_broker(
            config,
            &request_id,
            &materialization.workspace_id,
            WorkspaceHostOperation::MaterializeInspect {
                materialization: plan.clone(),
            },
        )?;
        let observed = match inspect.state {
            MaterializationRecoveryState::Ready
            | MaterializationRecoveryState::Absent
            | MaterializationRecoveryState::Copying => call_broker(
                config,
                &request_id,
                &materialization.workspace_id,
                WorkspaceHostOperation::MaterializeStep {
                    materialization: plan.clone(),
                },
            )?,
            _ => {
                let now = now_utc();
                finish_workspace_materialization(
                    &config.live_sqlite_path,
                    &materialization.operation_id,
                    &config.lease_owner,
                    operation.generation,
                    "needs_review",
                    "needs_review",
                    inspect.observed_sha256.as_deref(),
                    None,
                    Some("workspace_materialization_conflict"),
                    Some("broker inspection found conflicting or unsafe materialization state"),
                    &now,
                )
                .map_err(|error| WorkspaceProvisionError::Metadata(error.to_string()))?;
                return Ok(report(
                    &materialization,
                    "needs_review",
                    inspect.completed_bytes,
                    "unsafe materialization evidence retained",
                ));
            }
        };
        let now = now_utc();
        let checkpoint_json = serde_json::json!({
            "source_object_id": materialization.source_object_id,
            "completed_bytes": observed.completed_bytes,
            "expected_size_bytes": observed.expected_size_bytes,
            "observed_sha256": observed.observed_sha256,
        })
        .to_string();
        let checkpoint_digest = format!("sha256:{:x}", Sha256::digest(checkpoint_json.as_bytes()));
        operation = checkpoint_workspace_operation(
            &config.live_sqlite_path,
            &materialization.operation_id,
            &config.lease_owner,
            operation.generation,
            if observed.state == MaterializationRecoveryState::Ready {
                "verified"
            } else {
                "copying"
            },
            observed.completed_bytes,
            u64::from(observed.state == MaterializationRecoveryState::Ready),
            WorkspaceRecoveryDisposition::ResumeCheckpoint,
            &checkpoint_digest,
            &checkpoint_json,
            &now,
        )
        .map_err(|error| WorkspaceProvisionError::Metadata(error.to_string()))?;
        if observed.state == MaterializationRecoveryState::Ready {
            let hash = observed.observed_sha256.as_deref().ok_or_else(|| {
                WorkspaceProvisionError::InvalidAuthority(
                    "ready materialization omitted checksum evidence".to_string(),
                )
            })?;
            let result = serde_json::json!({
                "source_object_id": materialization.source_object_id,
                "size_bytes": observed.completed_bytes,
                "sha256": hash,
            })
            .to_string();
            finish_workspace_materialization(
                &config.live_sqlite_path,
                &materialization.operation_id,
                &config.lease_owner,
                operation.generation,
                "completed",
                "succeeded",
                Some(hash),
                Some(&result),
                None,
                None,
                &now,
            )
            .map_err(|error| WorkspaceProvisionError::Metadata(error.to_string()))?;
            return Ok(report(
                &materialization,
                "completed",
                observed.completed_bytes,
                "verified materialization published atomically",
            ));
        }
        let expires = add_seconds_to_utc_timestamp(&now, 60).ok_or_else(|| {
            WorkspaceProvisionError::InvalidAuthority("invalid clock".to_string())
        })?;
        operation = renew_workspace_operation_lease(
            &config.live_sqlite_path,
            &materialization.operation_id,
            &config.lease_owner,
            operation.generation,
            &now,
            &expires,
        )
        .map_err(|error| WorkspaceProvisionError::Metadata(error.to_string()))?;
    }
}

fn call_broker(
    config: &WorkspaceProvisionWorkerConfig,
    request_id: &str,
    workspace_id: &str,
    operation: WorkspaceHostOperation,
) -> Result<dasobjectstore_workspace_host::MaterializationInspection, WorkspaceProvisionError> {
    let request = BrokerRequest {
        protocol_version: PROTOCOL_VERSION,
        request_id: request_id.to_string(),
        workspace_id: workspace_id.to_string(),
        operation,
    };
    let response = request_broker(&config.broker_socket_path, &request)
        .map_err(|error| WorkspaceProvisionError::Broker(error.to_string()))?;
    if response.protocol_version != PROTOCOL_VERSION
        || response.request_id != request.request_id
        || response.workspace_id != request.workspace_id
        || !response.ok
    {
        return Err(WorkspaceProvisionError::Broker(
            response
                .error_message
                .unwrap_or_else(|| "materialization broker identity mismatch".to_string()),
        ));
    }
    response.materialization.ok_or_else(|| {
        WorkspaceProvisionError::Broker(
            "materialization broker omitted inspection evidence".to_string(),
        )
    })
}

fn broker_request_id(operation_id: &str, generation: u64) -> String {
    format!(
        "materialize-{:x}",
        Sha256::digest(format!("{operation_id}:{generation}").as_bytes())
    )
}

fn now_utc() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format_utc_timestamp_seconds(i64::try_from(seconds).unwrap_or(i64::MAX))
}

fn report(
    materialization: &dasobjectstore_metadata::WorkspaceMaterializationSnapshot,
    state: &str,
    completed_bytes: u64,
    reason: &str,
) -> WorkspaceMaterializationReport {
    WorkspaceMaterializationReport {
        operation_id: materialization.operation_id.clone(),
        workspace_id: materialization.workspace_id.clone(),
        state: state.to_string(),
        completed_bytes,
        expected_size_bytes: materialization.expected_size_bytes,
        reason: reason.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broker_request_identity_is_stable_bounded_and_generation_fenced() {
        let first = broker_request_id("operation/a", 7);
        assert_eq!(first, broker_request_id("operation/a", 7));
        assert_ne!(first, broker_request_id("operation/a", 8));
        assert!(first.len() <= 128);
        assert!(first
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-'));
    }

    #[test]
    fn recovery_report_is_path_private() {
        let materialization = dasobjectstore_metadata::WorkspaceMaterializationSnapshot {
            workspace_id: "workspace-a".to_string(),
            operation_id: "operation-a".to_string(),
            source_object_id: "logical/object".to_string(),
            source_placement_id: "placement-a".to_string(),
            destination_relative_path: "inputs/private.bin".to_string(),
            expected_size_bytes: 64,
            expected_sha256: "a".repeat(64),
            observed_sha256: None,
            state: "copying".to_string(),
        };
        let value = serde_json::to_value(report(&materialization, "copying", 32, "resuming"))
            .expect("report");
        let encoded = value.to_string();
        assert!(!encoded.contains("private.bin"));
        assert!(!encoded.contains("placement-a"));
        assert!(!encoded.contains("/srv/"));
    }
}
