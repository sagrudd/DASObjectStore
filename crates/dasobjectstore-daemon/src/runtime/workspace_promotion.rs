//! Durable workspace promotion through the normal immutable ingest authority.

use dasobjectstore_core::ids::{IngestJobId, ObjectId, StoreId};
use dasobjectstore_core::store::StorePolicy;
use dasobjectstore_core::workspace::{WorkspaceOperationState, WorkspaceRecoveryDisposition};
use dasobjectstore_metadata::{
    accept_workspace_promotion_member, cancel_workspace_promotion, checkpoint_workspace_operation,
    claim_workspace_operation, commit_verified_ssd_and_enqueue, complete_workspace_promotion,
    list_active_workspace_promotions, measure_ssd_capacity, read_workspace_operation,
    recover_expired_workspace_operations, renew_workspace_operation_lease, IngestStagingLayout,
    SsdCapacityPolicy, SsdPressure, VerifiedSsdCommitRequest, WorkspaceOperationSnapshot,
    WorkspacePromotionMemberSnapshot, WorkspacePromotionSnapshot,
};
use dasobjectstore_workspace_host::{
    request_broker, BrokerRequest, BrokerResponse, PromotionInspection, PromotionPlan,
    PromotionRecoveryState, WorkspaceHostOperation, PROTOCOL_VERSION,
};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use crate::api::{CapacityAdmissionDecision, DaemonIngressOrigin};
use crate::runtime::CapacityAdmissionProvider;

#[derive(Clone)]
pub struct WorkspacePromotionWorkerConfig {
    pub live_sqlite_path: PathBuf,
    pub ssd_root: PathBuf,
    pub broker_socket_path: PathBuf,
    pub lease_owner: String,
    pub capacity_provider: Arc<dyn CapacityAdmissionProvider>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspacePromotionRecoveryReport {
    pub schema_version: String,
    pub inspected_promotions: usize,
    pub accepted_members: usize,
    pub completed_promotions: usize,
    pub deferred_promotions: usize,
    pub promotions: Vec<WorkspacePromotionOutcome>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspacePromotionOutcome {
    pub promotion_id: String,
    pub operation_id: String,
    pub state: String,
    pub reason: String,
    pub completed_members: usize,
    pub total_members: usize,
}

#[derive(Debug)]
pub enum WorkspacePromotionError {
    Metadata(String),
    Broker(String),
    Capacity(String),
    InvalidAuthority(String),
}

impl fmt::Display for WorkspacePromotionError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Metadata(message) => write!(output, "workspace promotion metadata: {message}"),
            Self::Broker(message) => write!(output, "workspace promotion broker: {message}"),
            Self::Capacity(message) => write!(output, "workspace promotion capacity: {message}"),
            Self::InvalidAuthority(message) => {
                write!(output, "workspace promotion authority: {message}")
            }
        }
    }
}

impl std::error::Error for WorkspacePromotionError {}

pub fn reconcile_workspace_promotions(
    config: &WorkspacePromotionWorkerConfig,
    now_utc: &str,
    lease_expires_at_utc: &str,
) -> Result<WorkspacePromotionRecoveryReport, WorkspacePromotionError> {
    reconcile_workspace_promotions_with(config, now_utc, lease_expires_at_utc, |request| {
        request_broker(&config.broker_socket_path, request).map_err(|error| error.to_string())
    })
}

fn reconcile_workspace_promotions_with<F>(
    config: &WorkspacePromotionWorkerConfig,
    now_utc: &str,
    lease_expires_at_utc: &str,
    mut broker: F,
) -> Result<WorkspacePromotionRecoveryReport, WorkspacePromotionError>
where
    F: FnMut(&BrokerRequest) -> Result<BrokerResponse, String>,
{
    recover_expired_workspace_operations(&config.live_sqlite_path, now_utc)
        .map_err(metadata_error)?;
    let promotions =
        list_active_workspace_promotions(&config.live_sqlite_path).map_err(metadata_error)?;
    let mut report = WorkspacePromotionRecoveryReport {
        schema_version: "dasobjectstore.workspace_promotion_recovery.v1".to_string(),
        inspected_promotions: promotions.len(),
        accepted_members: 0,
        completed_promotions: 0,
        deferred_promotions: 0,
        promotions: Vec::with_capacity(promotions.len()),
    };
    for promotion in promotions {
        let mut operation =
            read_workspace_operation(&config.live_sqlite_path, &promotion.operation_id)
                .map_err(metadata_error)?;
        if operation.cancellation_requested || operation.state == WorkspaceOperationState::Cancelled
        {
            let cancelled = cancel_workspace_promotion(
                &config.live_sqlite_path,
                &promotion.promotion_id,
                now_utc,
            )
            .map_err(metadata_error)?;
            report.promotions.push(outcome(
                &cancelled,
                &cancelled.state,
                if cancelled.state == "needs_review" {
                    "cancelled after immutable publication; retained for operator review"
                } else {
                    "cancelled before any immutable member publication"
                },
            ));
            continue;
        }
        operation = match operation.state {
            WorkspaceOperationState::Queued | WorkspaceOperationState::RetryWait => {
                claim_workspace_operation(
                    &config.live_sqlite_path,
                    &operation.operation_id,
                    &config.lease_owner,
                    operation.generation,
                    now_utc,
                    lease_expires_at_utc,
                )
                .map_err(metadata_error)?
            }
            WorkspaceOperationState::Running
                if operation.lease_owner.as_deref() == Some(&config.lease_owner) =>
            {
                renew_workspace_operation_lease(
                    &config.live_sqlite_path,
                    &operation.operation_id,
                    &config.lease_owner,
                    operation.generation,
                    now_utc,
                    lease_expires_at_utc,
                )
                .map_err(metadata_error)?
            }
            WorkspaceOperationState::Running => {
                report.deferred_promotions += 1;
                report.promotions.push(outcome(
                    &promotion,
                    "deferred",
                    "promotion has an authoritative active lease",
                ));
                continue;
            }
            state if state.is_terminal() => continue,
            _ => {
                report.deferred_promotions += 1;
                report.promotions.push(outcome(
                    &promotion,
                    "deferred",
                    "promotion operation is not runnable",
                ));
                continue;
            }
        };
        let Some(member) = promotion
            .members
            .iter()
            .find(|member| member.state != "accepted")
        else {
            if complete_workspace_promotion(
                &config.live_sqlite_path,
                &promotion.promotion_id,
                &config.lease_owner,
                operation.generation,
                now_utc,
            )
            .map_err(metadata_error)?
            {
                report.completed_promotions += 1;
            }
            report.promotions.push(outcome(
                &promotion,
                "completed",
                "all required members are catalogued with durable destage work",
            ));
            continue;
        };
        let reservation_id = promotion_capacity_reservation_id(&promotion, member);
        admit_member_capacity(config, &promotion, member, &reservation_id)?;
        let inspection = stage_one_extent(config, &promotion, member, &mut broker)?;
        operation =
            checkpoint_progress(config, &operation, &promotion, member, &inspection, now_utc)?;
        if inspection.state != PromotionRecoveryState::Ready {
            report.promotions.push(outcome(
                &promotion,
                "copying",
                "bounded verified SSD staging is in progress",
            ));
            continue;
        }
        publish_member(config, &promotion, member, &inspection, now_utc)?;
        let store_id = StoreId::new(promotion.target_store_id.clone())
            .map_err(|error| WorkspacePromotionError::InvalidAuthority(error.to_string()))?;
        config
            .capacity_provider
            .commit(&store_id, &reservation_id)
            .map_err(|error| WorkspacePromotionError::Capacity(error.to_string()))?;
        accept_workspace_promotion_member(
            &config.live_sqlite_path,
            &promotion.promotion_id,
            &member.object_id,
            now_utc,
        )
        .map_err(metadata_error)?;
        report.accepted_members += 1;
        let refreshed = list_active_workspace_promotions(&config.live_sqlite_path)
            .map_err(metadata_error)?
            .into_iter()
            .find(|candidate| candidate.promotion_id == promotion.promotion_id);
        if refreshed.as_ref().is_some_and(|value| {
            value
                .members
                .iter()
                .all(|member| member.state == "accepted")
        }) {
            if complete_workspace_promotion(
                &config.live_sqlite_path,
                &promotion.promotion_id,
                &config.lease_owner,
                operation.generation,
                now_utc,
            )
            .map_err(metadata_error)?
            {
                report.completed_promotions += 1;
            }
            report.promotions.push(outcome(
                &promotion,
                "completed",
                "bundle catalogue and destage authority committed",
            ));
        } else {
            report.promotions.push(outcome(
                &promotion,
                "publishing",
                "member accepted; remaining bundle members are pending",
            ));
        }
    }
    Ok(report)
}

fn admit_member_capacity(
    config: &WorkspacePromotionWorkerConfig,
    promotion: &WorkspacePromotionSnapshot,
    member: &WorkspacePromotionMemberSnapshot,
    reservation_id: &str,
) -> Result<(), WorkspacePromotionError> {
    let store_id = StoreId::new(promotion.target_store_id.clone())
        .map_err(|error| WorkspacePromotionError::InvalidAuthority(error.to_string()))?;
    let policy = read_store_policy(config, &store_id)?;
    let admission = config
        .capacity_provider
        .admit_ingest(
            store_id.as_str(),
            member.size_bytes,
            policy.copies,
            DaemonIngressOrigin::LocalServerSsdFirst,
            reservation_id,
        )
        .map_err(|error| WorkspacePromotionError::Capacity(error.to_string()))?;
    if admission.decision != CapacityAdmissionDecision::Admitted {
        return Err(WorkspacePromotionError::Capacity(
            admission
                .message
                .unwrap_or_else(|| "authoritative capacity policy rejected promotion".to_string()),
        ));
    }
    Ok(())
}

fn stage_one_extent<F>(
    config: &WorkspacePromotionWorkerConfig,
    promotion: &WorkspacePromotionSnapshot,
    member: &WorkspacePromotionMemberSnapshot,
    broker: &mut F,
) -> Result<PromotionInspection, WorkspacePromotionError>
where
    F: FnMut(&BrokerRequest) -> Result<BrokerResponse, String>,
{
    let capacity = measure_ssd_capacity(&config.ssd_root)
        .map_err(|error| WorkspacePromotionError::Capacity(error.to_string()))?;
    let pressure = SsdCapacityPolicy::default()
        .evaluate(&capacity)
        .map_err(|error| WorkspacePromotionError::Capacity(error.to_string()))?;
    if pressure == SsdPressure::Critical || capacity.available_bytes < member.size_bytes {
        return Err(WorkspacePromotionError::Capacity(
            "managed SSD cannot admit the complete checkpoint member".to_string(),
        ));
    }
    let ingest_job_id = promotion_ingest_job_id(promotion, member);
    let typed_job = IngestJobId::new(ingest_job_id.clone())
        .map_err(|error| WorkspacePromotionError::InvalidAuthority(error.to_string()))?;
    IngestStagingLayout::for_ssd_root(&config.ssd_root)
        .job_paths(&typed_job)
        .create_directories()
        .map_err(|error| WorkspacePromotionError::Capacity(error.to_string()))?;
    let plan = PromotionPlan {
        promotion_id: promotion.promotion_id.clone(),
        checkpoint_id: promotion.checkpoint_id.clone(),
        source_relative_path: member.source_relative_path.clone(),
        object_id: member.object_id.clone(),
        ingest_job_id,
        expected_size_bytes: member.size_bytes,
        expected_sha256: member.sha256.clone(),
    };
    let inspect = call_broker(
        broker,
        promotion,
        member,
        WorkspaceHostOperation::PromotionInspect {
            promotion: plan.clone(),
        },
    )?;
    if inspect.state == PromotionRecoveryState::Ready {
        return Ok(inspect);
    }
    if !matches!(
        inspect.state,
        PromotionRecoveryState::Absent | PromotionRecoveryState::Copying
    ) {
        return Err(WorkspacePromotionError::InvalidAuthority(format!(
            "promotion staging is not safely resumable: {:?}",
            inspect.state
        )));
    }
    call_broker(
        broker,
        promotion,
        member,
        WorkspaceHostOperation::PromotionStep { promotion: plan },
    )
}

fn call_broker<F>(
    broker: &mut F,
    promotion: &WorkspacePromotionSnapshot,
    member: &WorkspacePromotionMemberSnapshot,
    operation: WorkspaceHostOperation,
) -> Result<PromotionInspection, WorkspacePromotionError>
where
    F: FnMut(&BrokerRequest) -> Result<BrokerResponse, String>,
{
    let request = BrokerRequest {
        protocol_version: PROTOCOL_VERSION,
        request_id: format!(
            "promote-{}",
            &promotion_ingest_job_id(promotion, member)["workspace-promote-".len()..]
        ),
        workspace_id: promotion.workspace_id.clone(),
        operation,
    };
    let response = broker(&request).map_err(WorkspacePromotionError::Broker)?;
    if !response.ok
        || response.protocol_version != PROTOCOL_VERSION
        || response.request_id != request.request_id
        || response.workspace_id != request.workspace_id
    {
        return Err(WorkspacePromotionError::InvalidAuthority(
            response
                .error_message
                .unwrap_or_else(|| "broker response identity did not match".to_string()),
        ));
    }
    response.promotion.ok_or_else(|| {
        WorkspacePromotionError::InvalidAuthority("broker omitted promotion inspection".to_string())
    })
}

fn checkpoint_progress(
    config: &WorkspacePromotionWorkerConfig,
    operation: &WorkspaceOperationSnapshot,
    promotion: &WorkspacePromotionSnapshot,
    member: &WorkspacePromotionMemberSnapshot,
    inspection: &PromotionInspection,
    now_utc: &str,
) -> Result<WorkspaceOperationSnapshot, WorkspacePromotionError> {
    let accepted_bytes = promotion
        .members
        .iter()
        .filter(|member| member.state == "accepted")
        .map(|member| member.size_bytes)
        .sum::<u64>();
    let accepted_units = promotion
        .members
        .iter()
        .filter(|member| member.state == "accepted")
        .count() as u64;
    let checkpoint_json = serde_json::json!({
        "promotion_id": promotion.promotion_id,
        "member_object_id": member.object_id,
        "member_completed_bytes": inspection.completed_bytes,
        "member_state": inspection.state,
    })
    .to_string();
    let digest = format!("sha256:{:x}", Sha256::digest(checkpoint_json.as_bytes()));
    checkpoint_workspace_operation(
        &config.live_sqlite_path,
        &operation.operation_id,
        &config.lease_owner,
        operation.generation,
        "workspace_promotion",
        accepted_bytes.saturating_add(inspection.completed_bytes),
        accepted_units,
        WorkspaceRecoveryDisposition::ResumeCheckpoint,
        &digest,
        &checkpoint_json,
        now_utc,
    )
    .map_err(metadata_error)
}

fn publish_member(
    config: &WorkspacePromotionWorkerConfig,
    promotion: &WorkspacePromotionSnapshot,
    member: &WorkspacePromotionMemberSnapshot,
    inspection: &PromotionInspection,
    now_utc: &str,
) -> Result<(), WorkspacePromotionError> {
    if inspection.observed_sha256.as_deref() != Some(member.sha256.as_str())
        || inspection.completed_bytes != member.size_bytes
    {
        return Err(WorkspacePromotionError::InvalidAuthority(
            "ready staging evidence does not match promotion manifest".to_string(),
        ));
    }
    let relative = inspection.staged_relative_path.as_deref().ok_or_else(|| {
        WorkspacePromotionError::InvalidAuthority(
            "ready promotion omitted managed staging identity".to_string(),
        )
    })?;
    let store_id = StoreId::new(promotion.target_store_id.clone())
        .map_err(|error| WorkspacePromotionError::InvalidAuthority(error.to_string()))?;
    let object_id = ObjectId::new(member.object_id.clone())
        .map_err(|error| WorkspacePromotionError::InvalidAuthority(error.to_string()))?;
    let policy = read_store_policy(config, &store_id)?;
    let job_id = promotion_ingest_job_id(promotion, member);
    let destage_job_id = format!("destage-{}", hex_identity(&member.object_id));
    commit_verified_ssd_and_enqueue(
        &config.live_sqlite_path,
        VerifiedSsdCommitRequest {
            destage_job_id: &destage_job_id,
            store_id: &store_id,
            object_id: &object_id,
            object_type: &member.object_type,
            relative_path: relative,
            size_bytes: member.size_bytes,
            content_hash_algorithm: "sha256",
            content_hash: &member.sha256,
            acknowledgement_policy: "after_ssd_ingest",
            required_copy_count: policy.copies,
            max_attempts: 8,
            priority: 0,
            committed_at_utc: now_utc,
            ingest_job_id: Some(&job_id),
            ingress_origin: Some("workspace_promotion"),
            s3_key: member
                .object_id
                .strip_prefix(&format!("{}/", promotion.target_store_id)),
            s3_version: 1,
        },
    )
    .map_err(|error| WorkspacePromotionError::Metadata(error.to_string()))?;
    Ok(())
}

fn read_store_policy(
    config: &WorkspacePromotionWorkerConfig,
    store_id: &StoreId,
) -> Result<StorePolicy, WorkspacePromotionError> {
    let connection = Connection::open(&config.live_sqlite_path).map_err(metadata_error)?;
    let json: String = connection
        .query_row(
            "SELECT policy_json FROM stores WHERE store_id = ?1",
            [store_id.as_str()],
            |row| row.get(0),
        )
        .map_err(metadata_error)?;
    serde_json::from_str(&json)
        .map_err(|error| WorkspacePromotionError::InvalidAuthority(error.to_string()))
}

fn promotion_ingest_job_id(
    promotion: &WorkspacePromotionSnapshot,
    member: &WorkspacePromotionMemberSnapshot,
) -> String {
    let digest = Sha256::digest(
        format!(
            "{}\0{}\0{}\0{}",
            promotion.promotion_id, promotion.manifest_digest, member.object_id, member.sha256
        )
        .as_bytes(),
    );
    format!("workspace-promote-{:x}", digest)
}

fn promotion_capacity_reservation_id(
    promotion: &WorkspacePromotionSnapshot,
    member: &WorkspacePromotionMemberSnapshot,
) -> String {
    format!("{}/capacity", promotion_ingest_job_id(promotion, member))
}

fn hex_identity(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn outcome(
    promotion: &WorkspacePromotionSnapshot,
    state: &str,
    reason: &str,
) -> WorkspacePromotionOutcome {
    WorkspacePromotionOutcome {
        promotion_id: promotion.promotion_id.clone(),
        operation_id: promotion.operation_id.clone(),
        state: state.to_string(),
        reason: reason.to_string(),
        completed_members: promotion
            .members
            .iter()
            .filter(|member| member.state == "accepted")
            .count(),
        total_members: promotion.members.len(),
    }
}

fn metadata_error(error: impl ToString) -> WorkspacePromotionError {
    WorkspacePromotionError::Metadata(error.to_string())
}
