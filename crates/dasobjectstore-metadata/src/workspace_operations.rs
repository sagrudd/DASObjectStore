//! Durable operation authority for managed compute workspaces.
//!
//! Filesystem/provider work is deliberately performed outside these functions.
//! Every worker mutation is fenced by lease ownership and operation generation.

use crate::schema::LIVE_SCHEMA_SQL;
use dasobjectstore_core::ids::WorkspaceId;
use dasobjectstore_core::utc::parse_canonical_utc_timestamp_seconds;
use dasobjectstore_core::workspace::{
    WorkspaceOperationKind, WorkspaceOperationState, WorkspaceRecoveryDisposition,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt::{self, Display};
use std::path::{Path, PathBuf};
use std::time::Duration;

const MAX_STAGE_BYTES: usize = 128;
const MAX_CHECKPOINT_JSON_BYTES: usize = 64 * 1024;
const MAX_RESULT_JSON_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubmitWorkspaceOperationRequest {
    pub live_sqlite_path: PathBuf,
    pub operation_id: String,
    pub workspace_id: WorkspaceId,
    pub kind: WorkspaceOperationKind,
    pub request_id: String,
    pub request_digest: String,
    pub initial_stage: String,
    pub total_bytes: Option<u64>,
    pub total_units: Option<u64>,
    pub max_attempts: u32,
    pub recovery_disposition: WorkspaceRecoveryDisposition,
    pub created_at_utc: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceOperationCheckpointSummary {
    pub sequence: u64,
    pub stage: String,
    pub completed_bytes: u64,
    pub completed_units: u64,
    pub recovery_disposition: WorkspaceRecoveryDisposition,
    pub checkpoint_digest: String,
    pub recorded_at_utc: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceOperationSnapshot {
    pub operation_id: String,
    pub workspace_id: WorkspaceId,
    pub kind: WorkspaceOperationKind,
    pub request_id: String,
    pub state: WorkspaceOperationState,
    pub stage: String,
    pub completed_bytes: u64,
    pub total_bytes: Option<u64>,
    pub completed_units: u64,
    pub total_units: Option<u64>,
    pub cancellation_requested: bool,
    pub retry_count: u32,
    pub max_attempts: u32,
    pub lease_epoch: u64,
    pub lease_owner: Option<String>,
    pub lease_expires_at_utc: Option<String>,
    pub next_retry_at_utc: Option<String>,
    pub recovery_disposition: WorkspaceRecoveryDisposition,
    pub failure_code: Option<String>,
    pub failure_message: Option<String>,
    pub generation: u64,
    pub latest_checkpoint: Option<WorkspaceOperationCheckpointSummary>,
    pub created_at_utc: String,
    pub updated_at_utc: String,
    pub completed_at_utc: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceOperationRecoveryAction {
    Runnable,
    AwaitActiveLease,
    ResumeFromCheckpoint,
    RetryIdempotent,
    CancelPending,
    Completed,
    NeedsReview,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceOperationRecoveryRecord {
    pub operation_id: String,
    pub action: WorkspaceOperationRecoveryAction,
    pub reason: String,
}

#[derive(Debug)]
pub enum WorkspaceOperationError {
    Sqlite(rusqlite::Error),
    InvalidRequest {
        field: &'static str,
        reason: String,
    },
    WorkspaceNotFound {
        workspace_id: WorkspaceId,
    },
    OperationNotFound {
        operation_id: String,
    },
    RequestIdentityConflict {
        request_id: String,
    },
    OperationIdentityConflict {
        operation_id: String,
    },
    LeaseUnavailable {
        operation_id: String,
    },
    LeaseOwnerMismatch {
        operation_id: String,
    },
    StaleGeneration {
        expected: u64,
        actual: u64,
    },
    InvalidState {
        operation_id: String,
        state: WorkspaceOperationState,
    },
    InvalidStoredValue {
        field: &'static str,
        value: String,
    },
}

impl Display for WorkspaceOperationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => {
                write!(formatter, "workspace operation metadata failed: {error}")
            }
            Self::InvalidRequest { field, reason } => {
                write!(formatter, "invalid workspace operation {field}: {reason}")
            }
            Self::WorkspaceNotFound { workspace_id } => {
                write!(formatter, "workspace {workspace_id} does not exist")
            }
            Self::OperationNotFound { operation_id } => {
                write!(
                    formatter,
                    "workspace operation {operation_id} does not exist"
                )
            }
            Self::RequestIdentityConflict { request_id } => write!(
                formatter,
                "workspace operation request {request_id} was reused with different content"
            ),
            Self::OperationIdentityConflict { operation_id } => write!(
                formatter,
                "workspace operation identity {operation_id} is already bound to another request"
            ),
            Self::LeaseUnavailable { operation_id } => {
                write!(
                    formatter,
                    "workspace operation {operation_id} is not leaseable"
                )
            }
            Self::LeaseOwnerMismatch { operation_id } => write!(
                formatter,
                "workspace operation {operation_id} lease belongs to another worker"
            ),
            Self::StaleGeneration { expected, actual } => write!(
                formatter,
                "workspace operation generation changed: expected {expected}, actual {actual}"
            ),
            Self::InvalidState {
                operation_id,
                state,
            } => write!(
                formatter,
                "workspace operation {operation_id} cannot mutate from state {state:?}"
            ),
            Self::InvalidStoredValue { field, value } => {
                write!(
                    formatter,
                    "invalid stored workspace operation {field}: {value}"
                )
            }
        }
    }
}

impl std::error::Error for WorkspaceOperationError {}

impl From<rusqlite::Error> for WorkspaceOperationError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

pub fn submit_workspace_operation(
    request: &SubmitWorkspaceOperationRequest,
) -> Result<WorkspaceOperationSnapshot, WorkspaceOperationError> {
    validate_submit_request(request)?;
    let mut connection = open_operation_metadata(&request.live_sqlite_path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    require_workspace(&transaction, &request.workspace_id)?;

    if let Some((operation_id, request_digest)) = transaction
        .query_row(
            "SELECT operation_id, request_digest
             FROM compute_workspace_operations
             WHERE workspace_id = ?1 AND operation_kind = ?2 AND request_id = ?3",
            params![
                request.workspace_id.as_str(),
                request.kind.as_str(),
                request.request_id
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
    {
        if request_digest != request.request_digest {
            return Err(WorkspaceOperationError::RequestIdentityConflict {
                request_id: request.request_id.clone(),
            });
        }
        if operation_id != request.operation_id {
            return Err(WorkspaceOperationError::OperationIdentityConflict {
                operation_id: request.operation_id.clone(),
            });
        }
        let snapshot = read_operation_in_transaction(&transaction, &operation_id)?;
        transaction.commit()?;
        return Ok(snapshot);
    }
    if transaction
        .query_row(
            "SELECT 1 FROM compute_workspace_operations WHERE operation_id = ?1",
            [&request.operation_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some()
    {
        return Err(WorkspaceOperationError::OperationIdentityConflict {
            operation_id: request.operation_id.clone(),
        });
    }
    transaction.execute(
        "INSERT INTO compute_workspace_operations (
            operation_id, workspace_id, operation_kind, request_id, request_digest,
            state, stage, completed_bytes, total_bytes, completed_units, total_units,
            cancellation_requested, retry_count, max_attempts, lease_epoch,
            recovery_disposition, generation, created_at_utc, updated_at_utc
         ) VALUES (?1, ?2, ?3, ?4, ?5, 'queued', ?6, 0, ?7, 0, ?8, 0, 0, ?9, 0, ?10, 1, ?11, ?11)",
        params![
            request.operation_id,
            request.workspace_id.as_str(),
            request.kind.as_str(),
            request.request_id,
            request.request_digest,
            request.initial_stage,
            optional_u64_to_i64(request.total_bytes, "total_bytes")?,
            optional_u64_to_i64(request.total_units, "total_units")?,
            request.max_attempts,
            request.recovery_disposition.as_str(),
            request.created_at_utc,
        ],
    )?;
    let snapshot = read_operation_in_transaction(&transaction, &request.operation_id)?;
    transaction.commit()?;
    Ok(snapshot)
}

pub fn read_workspace_operation(
    live_sqlite_path: &Path,
    operation_id: &str,
) -> Result<WorkspaceOperationSnapshot, WorkspaceOperationError> {
    let connection = open_operation_metadata(live_sqlite_path)?;
    read_operation(&connection, operation_id)
}

pub fn list_workspace_operations(
    live_sqlite_path: &Path,
    workspace_id: Option<&WorkspaceId>,
) -> Result<Vec<WorkspaceOperationSnapshot>, WorkspaceOperationError> {
    let connection = open_operation_metadata(live_sqlite_path)?;
    let mut statement = connection.prepare(
        "SELECT operation_id
         FROM compute_workspace_operations
         WHERE (?1 IS NULL OR workspace_id = ?1)
         ORDER BY created_at_utc, operation_id",
    )?;
    let ids = statement
        .query_map([workspace_id.map(WorkspaceId::as_str)], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<Result<Vec<_>, _>>()?;
    ids.iter()
        .map(|operation_id| read_operation(&connection, operation_id))
        .collect()
}

pub fn claim_workspace_operation(
    live_sqlite_path: &Path,
    operation_id: &str,
    lease_owner: &str,
    expected_generation: u64,
    now_utc: &str,
    lease_expires_at_utc: &str,
) -> Result<WorkspaceOperationSnapshot, WorkspaceOperationError> {
    validate_identifier("lease_owner", lease_owner)?;
    validate_timestamp("now_utc", now_utc)?;
    validate_timestamp("lease_expires_at_utc", lease_expires_at_utc)?;
    if timestamp_seconds(lease_expires_at_utc) <= timestamp_seconds(now_utc) {
        return Err(invalid(
            "lease_expires_at_utc",
            "must be later than now_utc",
        ));
    }
    let mut connection = open_operation_metadata(live_sqlite_path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current = read_operation_in_transaction(&transaction, operation_id)?;
    require_generation(&current, expected_generation)?;
    let eligible = matches!(current.state, WorkspaceOperationState::Queued)
        || (current.state == WorkspaceOperationState::RetryWait
            && current
                .next_retry_at_utc
                .as_deref()
                .is_none_or(|retry_at| retry_at <= now_utc));
    if !eligible || current.cancellation_requested {
        return Err(WorkspaceOperationError::LeaseUnavailable {
            operation_id: operation_id.to_string(),
        });
    }
    transaction.execute(
        "UPDATE compute_workspace_operations
         SET state = 'running', lease_owner = ?2, lease_expires_at_utc = ?3,
             lease_epoch = lease_epoch + 1, retry_count = retry_count + 1,
             next_retry_at_utc = NULL, generation = generation + 1,
             updated_at_utc = ?4
         WHERE operation_id = ?1 AND generation = ?5",
        params![
            operation_id,
            lease_owner,
            lease_expires_at_utc,
            now_utc,
            u64_to_i64(expected_generation, "generation")?
        ],
    )?;
    let snapshot = read_operation_in_transaction(&transaction, operation_id)?;
    transaction.commit()?;
    Ok(snapshot)
}

pub fn renew_workspace_operation_lease(
    live_sqlite_path: &Path,
    operation_id: &str,
    lease_owner: &str,
    expected_generation: u64,
    now_utc: &str,
    lease_expires_at_utc: &str,
) -> Result<WorkspaceOperationSnapshot, WorkspaceOperationError> {
    validate_identifier("lease_owner", lease_owner)?;
    validate_timestamp("now_utc", now_utc)?;
    validate_timestamp("lease_expires_at_utc", lease_expires_at_utc)?;
    if timestamp_seconds(lease_expires_at_utc) <= timestamp_seconds(now_utc) {
        return Err(invalid(
            "lease_expires_at_utc",
            "must be later than now_utc",
        ));
    }
    let mut connection = open_operation_metadata(live_sqlite_path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current = read_operation_in_transaction(&transaction, operation_id)?;
    require_owned_running(&current, lease_owner, expected_generation)?;
    if current
        .lease_expires_at_utc
        .as_deref()
        .is_some_and(|expires| expires < now_utc)
    {
        return Err(WorkspaceOperationError::LeaseUnavailable {
            operation_id: operation_id.to_string(),
        });
    }
    transaction.execute(
        "UPDATE compute_workspace_operations
         SET lease_expires_at_utc = ?2, generation = generation + 1, updated_at_utc = ?3
         WHERE operation_id = ?1 AND generation = ?4 AND lease_owner = ?5",
        params![
            operation_id,
            lease_expires_at_utc,
            now_utc,
            u64_to_i64(expected_generation, "generation")?,
            lease_owner,
        ],
    )?;
    let snapshot = read_operation_in_transaction(&transaction, operation_id)?;
    transaction.commit()?;
    Ok(snapshot)
}

#[allow(clippy::too_many_arguments)]
pub fn checkpoint_workspace_operation(
    live_sqlite_path: &Path,
    operation_id: &str,
    lease_owner: &str,
    expected_generation: u64,
    stage: &str,
    completed_bytes: u64,
    completed_units: u64,
    recovery_disposition: WorkspaceRecoveryDisposition,
    checkpoint_digest: &str,
    checkpoint_json: &str,
    recorded_at_utc: &str,
) -> Result<WorkspaceOperationSnapshot, WorkspaceOperationError> {
    validate_stage(stage)?;
    validate_digest("checkpoint_digest", checkpoint_digest)?;
    validate_timestamp("recorded_at_utc", recorded_at_utc)?;
    validate_checkpoint_json(checkpoint_json)?;
    let mut connection = open_operation_metadata(live_sqlite_path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current = read_operation_in_transaction(&transaction, operation_id)?;
    if current.lease_owner.as_deref() == Some(lease_owner)
        && current
            .latest_checkpoint
            .as_ref()
            .is_some_and(|checkpoint| {
                checkpoint.stage == stage
                    && checkpoint.completed_bytes == completed_bytes
                    && checkpoint.completed_units == completed_units
                    && checkpoint.recovery_disposition == recovery_disposition
                    && checkpoint.checkpoint_digest == checkpoint_digest
            })
    {
        transaction.commit()?;
        return Ok(current);
    }
    require_owned_running(&current, lease_owner, expected_generation)?;
    validate_progress(&current, completed_bytes, completed_units)?;
    let next_sequence = current
        .latest_checkpoint
        .as_ref()
        .map_or(1, |checkpoint| checkpoint.sequence + 1);
    transaction.execute(
        "INSERT INTO compute_workspace_operation_checkpoints (
            operation_id, sequence, stage, completed_bytes, completed_units,
            recovery_disposition, checkpoint_digest, checkpoint_json, recorded_at_utc
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            operation_id,
            u64_to_i64(next_sequence, "checkpoint_sequence")?,
            stage,
            u64_to_i64(completed_bytes, "completed_bytes")?,
            u64_to_i64(completed_units, "completed_units")?,
            recovery_disposition.as_str(),
            checkpoint_digest,
            checkpoint_json,
            recorded_at_utc,
        ],
    )?;
    transaction.execute(
        "UPDATE compute_workspace_operations
         SET stage = ?2, completed_bytes = ?3, completed_units = ?4,
             recovery_disposition = ?5, generation = generation + 1,
             updated_at_utc = ?6
         WHERE operation_id = ?1 AND generation = ?7 AND lease_owner = ?8",
        params![
            operation_id,
            stage,
            u64_to_i64(completed_bytes, "completed_bytes")?,
            u64_to_i64(completed_units, "completed_units")?,
            recovery_disposition.as_str(),
            recorded_at_utc,
            u64_to_i64(expected_generation, "generation")?,
            lease_owner,
        ],
    )?;
    let snapshot = read_operation_in_transaction(&transaction, operation_id)?;
    transaction.commit()?;
    Ok(snapshot)
}

pub fn request_workspace_operation_cancellation(
    live_sqlite_path: &Path,
    operation_id: &str,
    expected_generation: u64,
    requested_at_utc: &str,
) -> Result<WorkspaceOperationSnapshot, WorkspaceOperationError> {
    validate_timestamp("requested_at_utc", requested_at_utc)?;
    let mut connection = open_operation_metadata(live_sqlite_path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current = read_operation_in_transaction(&transaction, operation_id)?;
    require_generation(&current, expected_generation)?;
    if current.state.is_terminal() {
        transaction.commit()?;
        return Ok(current);
    }
    let next_state = if matches!(
        current.state,
        WorkspaceOperationState::Queued | WorkspaceOperationState::RetryWait
    ) {
        "cancelled"
    } else {
        current.state.as_str()
    };
    let completed_at = (next_state == "cancelled").then_some(requested_at_utc);
    transaction.execute(
        "UPDATE compute_workspace_operations
         SET state = ?2, cancellation_requested = 1,
             completed_at_utc = COALESCE(completed_at_utc, ?3),
             recovery_disposition = CASE WHEN ?2 = 'cancelled' THEN 'terminal' ELSE recovery_disposition END,
             generation = generation + 1, updated_at_utc = ?4
         WHERE operation_id = ?1 AND generation = ?5",
        params![
            operation_id,
            next_state,
            completed_at,
            requested_at_utc,
            u64_to_i64(expected_generation, "generation")?
        ],
    )?;
    let snapshot = read_operation_in_transaction(&transaction, operation_id)?;
    transaction.commit()?;
    Ok(snapshot)
}

pub fn finish_workspace_operation(
    live_sqlite_path: &Path,
    operation_id: &str,
    lease_owner: &str,
    expected_generation: u64,
    terminal_state: WorkspaceOperationState,
    result_json: Option<&str>,
    failure_code: Option<&str>,
    failure_message: Option<&str>,
    completed_at_utc: &str,
) -> Result<WorkspaceOperationSnapshot, WorkspaceOperationError> {
    if !terminal_state.is_terminal() {
        return Err(invalid("terminal_state", "must be terminal"));
    }
    validate_timestamp("completed_at_utc", completed_at_utc)?;
    if result_json.is_some_and(|value| value.len() > MAX_RESULT_JSON_BYTES) {
        return Err(invalid("result_json", "exceeds 64 KiB"));
    }
    if let Some(value) = result_json {
        let _: Value = serde_json::from_str(value)
            .map_err(|error| invalid("result_json", &format!("invalid JSON: {error}")))?;
    }
    let mut connection = open_operation_metadata(live_sqlite_path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current = read_operation_in_transaction(&transaction, operation_id)?;
    if current.state.is_terminal() {
        if current.state != terminal_state {
            return Err(WorkspaceOperationError::InvalidState {
                operation_id: operation_id.to_string(),
                state: current.state,
            });
        }
        transaction.commit()?;
        return Ok(current);
    }
    require_owned_running(&current, lease_owner, expected_generation)?;
    transaction.execute(
        "UPDATE compute_workspace_operations
         SET state = ?2, lease_owner = NULL, lease_expires_at_utc = NULL,
             recovery_disposition = 'terminal', result_json = ?3,
             failure_code = ?4, failure_message = ?5, completed_at_utc = ?6,
             generation = generation + 1, updated_at_utc = ?6
         WHERE operation_id = ?1 AND generation = ?7 AND lease_owner = ?8",
        params![
            operation_id,
            terminal_state.as_str(),
            result_json,
            failure_code,
            failure_message,
            completed_at_utc,
            u64_to_i64(expected_generation, "generation")?,
            lease_owner,
        ],
    )?;
    let snapshot = read_operation_in_transaction(&transaction, operation_id)?;
    transaction.commit()?;
    Ok(snapshot)
}

pub fn recover_expired_workspace_operations(
    live_sqlite_path: &Path,
    now_utc: &str,
) -> Result<Vec<WorkspaceOperationRecoveryRecord>, WorkspaceOperationError> {
    validate_timestamp("now_utc", now_utc)?;
    let mut connection = open_operation_metadata(live_sqlite_path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let ids = {
        let mut statement = transaction.prepare(
            "SELECT operation_id FROM compute_workspace_operations ORDER BY created_at_utc, operation_id",
        )?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        ids
    };
    let mut records = Vec::with_capacity(ids.len());
    for operation_id in ids {
        let current = read_operation_in_transaction(&transaction, &operation_id)?;
        let (mut action, mut reason) = recovery_classification(&current, now_utc);
        if current.state == WorkspaceOperationState::Running
            && current
                .lease_expires_at_utc
                .as_deref()
                .is_some_and(|expires| expires <= now_utc)
        {
            let (next_state, code, message) = match action {
                WorkspaceOperationRecoveryAction::ResumeFromCheckpoint
                | WorkspaceOperationRecoveryAction::RetryIdempotent => {
                    if current.retry_count >= current.max_attempts {
                        action = WorkspaceOperationRecoveryAction::NeedsReview;
                        reason = "operation exhausted its bounded attempts during restart recovery"
                            .to_string();
                        (
                            "needs_review",
                            Some("workspace_operation_retry_exhausted"),
                            Some(
                                "operation exhausted its bounded attempts during restart recovery",
                            ),
                        )
                    } else {
                        ("queued", None, None)
                    }
                }
                WorkspaceOperationRecoveryAction::CancelPending => (
                    {
                        action = WorkspaceOperationRecoveryAction::NeedsReview;
                        reason =
                            "operation lease expired after cancellation was requested".to_string();
                        "needs_review"
                    },
                    Some("workspace_operation_cancel_ambiguous"),
                    Some("operation lease expired after cancellation was requested"),
                ),
                _ => {
                    action = WorkspaceOperationRecoveryAction::NeedsReview;
                    reason =
                        "operation lease expired without proof that replay is safe".to_string();
                    (
                        "needs_review",
                        Some("workspace_operation_external_effect_ambiguous"),
                        Some("operation lease expired without proof that replay is safe"),
                    )
                }
            };
            transaction.execute(
                "UPDATE compute_workspace_operations
                 SET state = ?2, lease_owner = NULL, lease_expires_at_utc = NULL,
                     failure_code = ?3, failure_message = ?4,
                     recovery_disposition = CASE WHEN ?2 = 'needs_review'
                         THEN 'verify_external_effect' ELSE recovery_disposition END,
                     generation = generation + 1, updated_at_utc = ?5
                 WHERE operation_id = ?1 AND generation = ?6",
                params![
                    operation_id,
                    next_state,
                    code,
                    message,
                    now_utc,
                    u64_to_i64(current.generation, "generation")?
                ],
            )?;
        }
        records.push(WorkspaceOperationRecoveryRecord {
            operation_id,
            action,
            reason,
        });
    }
    transaction.commit()?;
    Ok(records)
}

fn recovery_classification(
    operation: &WorkspaceOperationSnapshot,
    now_utc: &str,
) -> (WorkspaceOperationRecoveryAction, String) {
    if operation.state.is_terminal() {
        return (
            WorkspaceOperationRecoveryAction::Completed,
            "operation is terminal".to_string(),
        );
    }
    if matches!(
        operation.state,
        WorkspaceOperationState::Queued | WorkspaceOperationState::RetryWait
    ) {
        return (
            WorkspaceOperationRecoveryAction::Runnable,
            "operation is waiting for a worker lease".to_string(),
        );
    }
    if operation
        .lease_expires_at_utc
        .as_deref()
        .is_some_and(|expires| expires > now_utc)
    {
        return (
            WorkspaceOperationRecoveryAction::AwaitActiveLease,
            "operation has an unexpired worker lease".to_string(),
        );
    }
    if operation.cancellation_requested {
        return (
            WorkspaceOperationRecoveryAction::CancelPending,
            "cancellation was requested before the worker lease expired".to_string(),
        );
    }
    match operation.recovery_disposition {
        WorkspaceRecoveryDisposition::ResumeCheckpoint if operation.latest_checkpoint.is_some() => {
            (
                WorkspaceOperationRecoveryAction::ResumeFromCheckpoint,
                "latest durable checkpoint permits bounded resume".to_string(),
            )
        }
        WorkspaceRecoveryDisposition::RetryIdempotent => (
            WorkspaceOperationRecoveryAction::RetryIdempotent,
            "operation declares its current stage idempotent".to_string(),
        ),
        _ => (
            WorkspaceOperationRecoveryAction::NeedsReview,
            "external effect cannot be proven safe to replay".to_string(),
        ),
    }
}

fn open_operation_metadata(path: &Path) -> Result<Connection, WorkspaceOperationError> {
    let connection = Connection::open(path)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.execute_batch(LIVE_SCHEMA_SQL)?;
    ensure_operation_schema_upgrade(&connection)?;
    Ok(connection)
}

fn ensure_operation_schema_upgrade(connection: &Connection) -> Result<(), WorkspaceOperationError> {
    for (column, definition) in [
        (
            "max_attempts",
            "INTEGER NOT NULL DEFAULT 3 CHECK (max_attempts > 0)",
        ),
        (
            "lease_epoch",
            "INTEGER NOT NULL DEFAULT 0 CHECK (lease_epoch >= 0)",
        ),
        ("next_retry_at_utc", "TEXT"),
        (
            "recovery_disposition",
            "TEXT NOT NULL DEFAULT 'verify_external_effect'",
        ),
        (
            "generation",
            "INTEGER NOT NULL DEFAULT 1 CHECK (generation > 0)",
        ),
        ("completed_at_utc", "TEXT"),
    ] {
        if !table_has_column(connection, "compute_workspace_operations", column)? {
            connection.execute(
                &format!(
                    "ALTER TABLE compute_workspace_operations ADD COLUMN {column} {definition}"
                ),
                [],
            )?;
        }
    }
    Ok(())
}

fn table_has_column(
    connection: &Connection,
    table: &str,
    column: &str,
) -> Result<bool, WorkspaceOperationError> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(columns.iter().any(|value| value == column))
}

fn require_workspace(
    transaction: &Transaction<'_>,
    workspace_id: &WorkspaceId,
) -> Result<(), WorkspaceOperationError> {
    if transaction
        .query_row(
            "SELECT 1 FROM compute_workspaces WHERE workspace_id = ?1",
            [workspace_id.as_str()],
            |_| Ok(()),
        )
        .optional()?
        .is_none()
    {
        return Err(WorkspaceOperationError::WorkspaceNotFound {
            workspace_id: workspace_id.clone(),
        });
    }
    Ok(())
}

fn read_operation(
    connection: &Connection,
    operation_id: &str,
) -> Result<WorkspaceOperationSnapshot, WorkspaceOperationError> {
    let row = connection
        .query_row(OPERATION_SELECT, [operation_id], map_operation_row)
        .optional()?
        .ok_or_else(|| WorkspaceOperationError::OperationNotFound {
            operation_id: operation_id.to_string(),
        })?;
    assemble_snapshot(connection, row)
}

fn read_operation_in_transaction(
    transaction: &Transaction<'_>,
    operation_id: &str,
) -> Result<WorkspaceOperationSnapshot, WorkspaceOperationError> {
    let row = transaction
        .query_row(OPERATION_SELECT, [operation_id], map_operation_row)
        .optional()?
        .ok_or_else(|| WorkspaceOperationError::OperationNotFound {
            operation_id: operation_id.to_string(),
        })?;
    assemble_snapshot(transaction, row)
}

const OPERATION_SELECT: &str = "SELECT
    operation_id, workspace_id, operation_kind, request_id, state, stage,
    completed_bytes, total_bytes, completed_units, total_units,
    cancellation_requested, retry_count, max_attempts, lease_epoch,
    lease_owner, lease_expires_at_utc, next_retry_at_utc, recovery_disposition,
    failure_code, failure_message, generation, created_at_utc, updated_at_utc,
    completed_at_utc
 FROM compute_workspace_operations WHERE operation_id = ?1";

type OperationRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    i64,
    Option<i64>,
    i64,
    Option<i64>,
    i64,
    i64,
    i64,
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
    String,
    Option<String>,
    Option<String>,
    i64,
    String,
    String,
    Option<String>,
);

fn map_operation_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<OperationRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
        row.get(11)?,
        row.get(12)?,
        row.get(13)?,
        row.get(14)?,
        row.get(15)?,
        row.get(16)?,
        row.get(17)?,
        row.get(18)?,
        row.get(19)?,
        row.get(20)?,
        row.get(21)?,
        row.get(22)?,
        row.get(23)?,
    ))
}

fn assemble_snapshot(
    connection: &Connection,
    row: OperationRow,
) -> Result<WorkspaceOperationSnapshot, WorkspaceOperationError> {
    let operation_id = row.0;
    let workspace_id = WorkspaceId::new(row.1.clone()).map_err(|_| {
        WorkspaceOperationError::InvalidStoredValue {
            field: "workspace_id",
            value: row.1,
        }
    })?;
    Ok(WorkspaceOperationSnapshot {
        latest_checkpoint: read_latest_checkpoint(connection, &operation_id)?,
        operation_id,
        workspace_id,
        kind: parse_kind(&row.2)?,
        request_id: row.3,
        state: parse_state(&row.4)?,
        stage: row.5,
        completed_bytes: stored_u64(row.6, "completed_bytes")?,
        total_bytes: stored_optional_u64(row.7, "total_bytes")?,
        completed_units: stored_u64(row.8, "completed_units")?,
        total_units: stored_optional_u64(row.9, "total_units")?,
        cancellation_requested: row.10 != 0,
        retry_count: stored_u32(row.11, "retry_count")?,
        max_attempts: stored_u32(row.12, "max_attempts")?,
        lease_epoch: stored_u64(row.13, "lease_epoch")?,
        lease_owner: row.14,
        lease_expires_at_utc: row.15,
        next_retry_at_utc: row.16,
        recovery_disposition: parse_disposition(&row.17)?,
        failure_code: row.18,
        failure_message: row.19,
        generation: stored_u64(row.20, "generation")?,
        created_at_utc: row.21,
        updated_at_utc: row.22,
        completed_at_utc: row.23,
    })
}

fn read_latest_checkpoint(
    connection: &Connection,
    operation_id: &str,
) -> Result<Option<WorkspaceOperationCheckpointSummary>, WorkspaceOperationError> {
    let row = connection
        .query_row(
            "SELECT sequence, stage, completed_bytes, completed_units,
                    recovery_disposition, checkpoint_digest, recorded_at_utc
             FROM compute_workspace_operation_checkpoints
             WHERE operation_id = ?1 ORDER BY sequence DESC LIMIT 1",
            [operation_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                ))
            },
        )
        .optional()?;
    row.map(|row| {
        Ok(WorkspaceOperationCheckpointSummary {
            sequence: stored_u64(row.0, "checkpoint_sequence")?,
            stage: row.1,
            completed_bytes: stored_u64(row.2, "checkpoint_completed_bytes")?,
            completed_units: stored_u64(row.3, "checkpoint_completed_units")?,
            recovery_disposition: parse_disposition(&row.4)?,
            checkpoint_digest: row.5,
            recorded_at_utc: row.6,
        })
    })
    .transpose()
}

fn validate_submit_request(
    request: &SubmitWorkspaceOperationRequest,
) -> Result<(), WorkspaceOperationError> {
    validate_identifier("operation_id", &request.operation_id)?;
    validate_identifier("request_id", &request.request_id)?;
    validate_digest("request_digest", &request.request_digest)?;
    validate_stage(&request.initial_stage)?;
    validate_timestamp("created_at_utc", &request.created_at_utc)?;
    if request.max_attempts == 0 || request.max_attempts > 100 {
        return Err(invalid("max_attempts", "must be between 1 and 100"));
    }
    Ok(())
}

fn validate_identifier(field: &'static str, value: &str) -> Result<(), WorkspaceOperationError> {
    if value.trim().is_empty() || value.len() > 255 {
        return Err(invalid(field, "must be non-blank and at most 255 bytes"));
    }
    Ok(())
}

fn validate_stage(value: &str) -> Result<(), WorkspaceOperationError> {
    if value.trim().is_empty()
        || value.len() > MAX_STAGE_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(invalid(
            "stage",
            "must be a path-free identifier of at most 128 bytes",
        ));
    }
    Ok(())
}

fn validate_digest(field: &'static str, value: &str) -> Result<(), WorkspaceOperationError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid(field, "must be exactly 64 hexadecimal characters"));
    }
    Ok(())
}

fn validate_timestamp(field: &'static str, value: &str) -> Result<(), WorkspaceOperationError> {
    if parse_canonical_utc_timestamp_seconds(value).is_none() {
        return Err(invalid(field, "must use canonical UTC whole-second format"));
    }
    Ok(())
}

fn timestamp_seconds(value: &str) -> i64 {
    parse_canonical_utc_timestamp_seconds(value)
        .expect("caller must validate canonical timestamps before comparison")
}

fn validate_checkpoint_json(value: &str) -> Result<(), WorkspaceOperationError> {
    if value.len() > MAX_CHECKPOINT_JSON_BYTES {
        return Err(invalid("checkpoint_json", "exceeds 64 KiB"));
    }
    let parsed: Value = serde_json::from_str(value)
        .map_err(|error| invalid("checkpoint_json", &format!("invalid JSON: {error}")))?;
    validate_path_free_json(&parsed)
}

fn validate_path_free_json(value: &Value) -> Result<(), WorkspaceOperationError> {
    match value {
        Value::Object(values) => {
            for (key, value) in values {
                let normalized = key.to_ascii_lowercase();
                if ["path", "root", "secret", "password", "token"]
                    .iter()
                    .any(|forbidden| normalized.contains(forbidden))
                {
                    return Err(invalid(
                        "checkpoint_json",
                        "must not contain paths or secret-bearing fields",
                    ));
                }
                validate_path_free_json(value)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                validate_path_free_json(value)?;
            }
        }
        Value::String(value)
            if value.starts_with('/')
                || value.starts_with("\\\\")
                || value.split(['/', '\\']).any(|part| part == "..") =>
        {
            return Err(invalid(
                "checkpoint_json",
                "must not contain absolute or parent-relative paths",
            ));
        }
        _ => {}
    }
    Ok(())
}

fn validate_progress(
    current: &WorkspaceOperationSnapshot,
    completed_bytes: u64,
    completed_units: u64,
) -> Result<(), WorkspaceOperationError> {
    if completed_bytes < current.completed_bytes
        || completed_units < current.completed_units
        || current
            .total_bytes
            .is_some_and(|total| completed_bytes > total)
        || current
            .total_units
            .is_some_and(|total| completed_units > total)
    {
        return Err(invalid(
            "progress",
            "must be monotonic and cannot exceed declared totals",
        ));
    }
    Ok(())
}

fn require_generation(
    current: &WorkspaceOperationSnapshot,
    expected: u64,
) -> Result<(), WorkspaceOperationError> {
    if current.generation != expected {
        return Err(WorkspaceOperationError::StaleGeneration {
            expected,
            actual: current.generation,
        });
    }
    Ok(())
}

fn require_owned_running(
    current: &WorkspaceOperationSnapshot,
    owner: &str,
    expected_generation: u64,
) -> Result<(), WorkspaceOperationError> {
    require_generation(current, expected_generation)?;
    if current.state != WorkspaceOperationState::Running {
        return Err(WorkspaceOperationError::InvalidState {
            operation_id: current.operation_id.clone(),
            state: current.state,
        });
    }
    if current.lease_owner.as_deref() != Some(owner) {
        return Err(WorkspaceOperationError::LeaseOwnerMismatch {
            operation_id: current.operation_id.clone(),
        });
    }
    Ok(())
}

fn parse_kind(value: &str) -> Result<WorkspaceOperationKind, WorkspaceOperationError> {
    match value {
        "provision" => Ok(WorkspaceOperationKind::Provision),
        "materialize" => Ok(WorkspaceOperationKind::Materialize),
        "promote" => Ok(WorkspaceOperationKind::Promote),
        "cleanup" => Ok(WorkspaceOperationKind::Cleanup),
        _ => Err(WorkspaceOperationError::InvalidStoredValue {
            field: "operation_kind",
            value: value.to_string(),
        }),
    }
}

fn parse_state(value: &str) -> Result<WorkspaceOperationState, WorkspaceOperationError> {
    match value {
        "queued" => Ok(WorkspaceOperationState::Queued),
        "running" => Ok(WorkspaceOperationState::Running),
        "retry_wait" => Ok(WorkspaceOperationState::RetryWait),
        "succeeded" => Ok(WorkspaceOperationState::Succeeded),
        "failed" => Ok(WorkspaceOperationState::Failed),
        "needs_review" => Ok(WorkspaceOperationState::NeedsReview),
        "cancelled" => Ok(WorkspaceOperationState::Cancelled),
        _ => Err(WorkspaceOperationError::InvalidStoredValue {
            field: "operation_state",
            value: value.to_string(),
        }),
    }
}

fn parse_disposition(value: &str) -> Result<WorkspaceRecoveryDisposition, WorkspaceOperationError> {
    match value {
        "retry_idempotent" => Ok(WorkspaceRecoveryDisposition::RetryIdempotent),
        "resume_checkpoint" => Ok(WorkspaceRecoveryDisposition::ResumeCheckpoint),
        "verify_external_effect" => Ok(WorkspaceRecoveryDisposition::VerifyExternalEffect),
        "terminal" => Ok(WorkspaceRecoveryDisposition::Terminal),
        _ => Err(WorkspaceOperationError::InvalidStoredValue {
            field: "recovery_disposition",
            value: value.to_string(),
        }),
    }
}

fn invalid(field: &'static str, reason: &str) -> WorkspaceOperationError {
    WorkspaceOperationError::InvalidRequest {
        field,
        reason: reason.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    const T0: &str = "2026-07-26T10:00:00Z";
    const T1: &str = "2026-07-26T10:01:00Z";
    const T2: &str = "2026-07-26T10:02:00Z";
    const T3: &str = "2026-07-26T10:03:00Z";
    const DIGEST_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const DIGEST_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    struct Fixture {
        path: PathBuf,
        workspace_id: WorkspaceId,
    }

    impl Fixture {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "dasobjectstore-workspace-operations-{}-{unique}.sqlite3",
                std::process::id()
            ));
            let connection = Connection::open(&path).expect("open fixture");
            connection
                .execute_batch(LIVE_SCHEMA_SQL)
                .expect("create schema");
            connection
                .execute(
                    "INSERT INTO pools (pool_id, state, created_at_utc, updated_at_utc)
                     VALUES ('pool-test', 'online', ?1, ?1)",
                    [T0],
                )
                .expect("insert pool");
            connection
                .execute(
                    "INSERT INTO compute_workspaces (
                        workspace_id, schema_version, request_id, request_digest, pool_id,
                        state, owner, project, purpose, requested_capacity_bytes,
                        reserved_capacity_bytes, quota_bytes, minimum_free_bytes_per_disk,
                        aggregation_provider, close_cleanup_policy_json, generation,
                        created_at_utc, updated_at_utc, expires_at_utc
                     ) VALUES (
                        'workspace-test', 1, 'workspace-request', ?1, 'pool-test',
                        'capacity_reserved', 'tester', 'tests', 'operation tests',
                        1024, 1024, 1024, 0, 'mergerfs', '{}', 1, ?2, ?2, ?3
                     )",
                    params![DIGEST_A, T0, "2026-07-27T10:00:00Z"],
                )
                .expect("insert workspace");
            Self {
                path,
                workspace_id: WorkspaceId::new("workspace-test").expect("workspace ID"),
            }
        }

        fn request(
            &self,
            operation_id: &str,
            request_id: &str,
            disposition: WorkspaceRecoveryDisposition,
        ) -> SubmitWorkspaceOperationRequest {
            SubmitWorkspaceOperationRequest {
                live_sqlite_path: self.path.clone(),
                operation_id: operation_id.to_string(),
                workspace_id: self.workspace_id.clone(),
                kind: WorkspaceOperationKind::Provision,
                request_id: request_id.to_string(),
                request_digest: DIGEST_A.to_string(),
                initial_stage: "reserved".to_string(),
                total_bytes: Some(100),
                total_units: Some(2),
                max_attempts: 3,
                recovery_disposition: disposition,
                created_at_utc: T0.to_string(),
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
            let _ = fs::remove_file(self.path.with_extension("sqlite3-shm"));
            let _ = fs::remove_file(self.path.with_extension("sqlite3-wal"));
        }
    }

    #[test]
    fn submission_is_idempotent_and_rejects_changed_request_content() {
        let fixture = Fixture::new();
        let request = fixture.request(
            "operation-one",
            "request-one",
            WorkspaceRecoveryDisposition::VerifyExternalEffect,
        );
        let created = submit_workspace_operation(&request).expect("submit");
        assert_eq!(created.state, WorkspaceOperationState::Queued);
        assert_eq!(
            submit_workspace_operation(&request).expect("replay"),
            created
        );

        let mut changed = request;
        changed.request_digest = DIGEST_B.to_string();
        assert!(matches!(
            submit_workspace_operation(&changed),
            Err(WorkspaceOperationError::RequestIdentityConflict { .. })
        ));
    }

    #[test]
    fn lease_checkpoint_and_completion_are_fenced_and_idempotent() {
        let fixture = Fixture::new();
        let queued = submit_workspace_operation(&fixture.request(
            "operation-two",
            "request-two",
            WorkspaceRecoveryDisposition::VerifyExternalEffect,
        ))
        .expect("submit");
        let running = claim_workspace_operation(
            &fixture.path,
            &queued.operation_id,
            "worker-a",
            queued.generation,
            T1,
            T3,
        )
        .expect("claim");
        assert_eq!(running.lease_epoch, 1);
        assert!(matches!(
            renew_workspace_operation_lease(
                &fixture.path,
                &running.operation_id,
                "worker-b",
                running.generation,
                T1,
                T3
            ),
            Err(WorkspaceOperationError::LeaseOwnerMismatch { .. })
        ));

        let checkpointed = checkpoint_workspace_operation(
            &fixture.path,
            &running.operation_id,
            "worker-a",
            running.generation,
            "branch_created",
            50,
            1,
            WorkspaceRecoveryDisposition::ResumeCheckpoint,
            DIGEST_B,
            r#"{"branch_id":"branch-a"}"#,
            T2,
        )
        .expect("checkpoint");
        let replayed = checkpoint_workspace_operation(
            &fixture.path,
            &running.operation_id,
            "worker-a",
            running.generation,
            "branch_created",
            50,
            1,
            WorkspaceRecoveryDisposition::ResumeCheckpoint,
            DIGEST_B,
            r#"{"branch_id":"branch-a"}"#,
            T2,
        )
        .expect("checkpoint replay");
        assert_eq!(replayed.generation, checkpointed.generation);
        assert_eq!(
            replayed
                .latest_checkpoint
                .as_ref()
                .expect("checkpoint")
                .sequence,
            1
        );
        assert!(matches!(
            checkpoint_workspace_operation(
                &fixture.path,
                &running.operation_id,
                "worker-a",
                checkpointed.generation,
                "bad",
                49,
                1,
                WorkspaceRecoveryDisposition::ResumeCheckpoint,
                DIGEST_A,
                "{}",
                T3
            ),
            Err(WorkspaceOperationError::InvalidRequest { .. })
        ));
        assert!(matches!(
            checkpoint_workspace_operation(
                &fixture.path,
                &running.operation_id,
                "worker-a",
                checkpointed.generation,
                "bad",
                60,
                2,
                WorkspaceRecoveryDisposition::ResumeCheckpoint,
                DIGEST_A,
                r#"{"source_path":"/tmp/secret"}"#,
                T3
            ),
            Err(WorkspaceOperationError::InvalidRequest { .. })
        ));

        let finished = finish_workspace_operation(
            &fixture.path,
            &running.operation_id,
            "worker-a",
            checkpointed.generation,
            WorkspaceOperationState::Succeeded,
            Some(r#"{"published":true}"#),
            None,
            None,
            T3,
        )
        .expect("finish");
        assert_eq!(finished.state, WorkspaceOperationState::Succeeded);
        assert_eq!(
            finish_workspace_operation(
                &fixture.path,
                &running.operation_id,
                "worker-a",
                checkpointed.generation,
                WorkspaceOperationState::Succeeded,
                Some(r#"{"published":true}"#),
                None,
                None,
                T3
            )
            .expect("finish replay")
            .generation,
            finished.generation
        );
    }

    #[test]
    fn restart_recovery_resumes_only_proven_safe_operations() {
        let fixture = Fixture::new();
        let retry = submit_workspace_operation(&fixture.request(
            "operation-retry",
            "request-retry",
            WorkspaceRecoveryDisposition::RetryIdempotent,
        ))
        .expect("submit retry");
        claim_workspace_operation(
            &fixture.path,
            &retry.operation_id,
            "worker",
            retry.generation,
            T0,
            T1,
        )
        .expect("claim retry");

        let ambiguous = submit_workspace_operation(&fixture.request(
            "operation-ambiguous",
            "request-ambiguous",
            WorkspaceRecoveryDisposition::VerifyExternalEffect,
        ))
        .expect("submit ambiguous");
        claim_workspace_operation(
            &fixture.path,
            &ambiguous.operation_id,
            "worker",
            ambiguous.generation,
            T0,
            T1,
        )
        .expect("claim ambiguous");

        let records =
            recover_expired_workspace_operations(&fixture.path, T2).expect("recover expired");
        assert!(records.iter().any(|record| {
            record.operation_id == "operation-retry"
                && record.action == WorkspaceOperationRecoveryAction::RetryIdempotent
        }));
        assert!(records.iter().any(|record| {
            record.operation_id == "operation-ambiguous"
                && record.action == WorkspaceOperationRecoveryAction::NeedsReview
        }));
        assert_eq!(
            read_workspace_operation(&fixture.path, "operation-retry")
                .expect("retry")
                .state,
            WorkspaceOperationState::Queued
        );
        assert_eq!(
            read_workspace_operation(&fixture.path, "operation-ambiguous")
                .expect("ambiguous")
                .state,
            WorkspaceOperationState::NeedsReview
        );
    }

    #[test]
    fn concurrent_claim_has_one_winner() {
        let fixture = Fixture::new();
        let queued = submit_workspace_operation(&fixture.request(
            "operation-race",
            "request-race",
            WorkspaceRecoveryDisposition::RetryIdempotent,
        ))
        .expect("submit");
        let barrier = Arc::new(Barrier::new(3));
        let handles = ["worker-a", "worker-b"].map(|worker| {
            let barrier = Arc::clone(&barrier);
            let path = fixture.path.clone();
            let operation_id = queued.operation_id.clone();
            thread::spawn(move || {
                barrier.wait();
                claim_workspace_operation(&path, &operation_id, worker, queued.generation, T0, T3)
            })
        });
        barrier.wait();
        let outcomes = handles.map(|handle| handle.join().expect("claim thread"));
        assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
        assert_eq!(
            outcomes.iter().filter(|outcome| outcome.is_err()).count(),
            1
        );
    }
}

fn u64_to_i64(value: u64, field: &'static str) -> Result<i64, WorkspaceOperationError> {
    i64::try_from(value).map_err(|_| invalid(field, "exceeds SQLite integer range"))
}

fn optional_u64_to_i64(
    value: Option<u64>,
    field: &'static str,
) -> Result<Option<i64>, WorkspaceOperationError> {
    value.map(|value| u64_to_i64(value, field)).transpose()
}

fn stored_u64(value: i64, field: &'static str) -> Result<u64, WorkspaceOperationError> {
    u64::try_from(value).map_err(|_| WorkspaceOperationError::InvalidStoredValue {
        field,
        value: value.to_string(),
    })
}

fn stored_optional_u64(
    value: Option<i64>,
    field: &'static str,
) -> Result<Option<u64>, WorkspaceOperationError> {
    value.map(|value| stored_u64(value, field)).transpose()
}

fn stored_u32(value: i64, field: &'static str) -> Result<u32, WorkspaceOperationError> {
    u32::try_from(value).map_err(|_| WorkspaceOperationError::InvalidStoredValue {
        field,
        value: value.to_string(),
    })
}
