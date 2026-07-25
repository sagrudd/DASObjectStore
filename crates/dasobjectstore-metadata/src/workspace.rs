use crate::schema::LIVE_SCHEMA_SQL;
use dasobjectstore_core::ids::{DiskId, PoolId, StoreId, WorkspaceId};
use dasobjectstore_core::lifecycle::{DiskState, HealthState};
use dasobjectstore_core::workspace::{
    plan_workspace_capacity, ComputeWorkspaceState, WorkspaceCapacityCandidate,
    WorkspaceCapacityPlanError, COMPUTE_WORKSPACE_SCHEMA_VERSION,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt::{self, Display};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReserveWorkspaceRequest {
    pub live_sqlite_path: PathBuf,
    pub workspace_id: WorkspaceId,
    pub request_id: String,
    pub request_digest: String,
    pub pool_id: PoolId,
    pub promotion_store_id: Option<StoreId>,
    pub owner: String,
    pub project: String,
    pub purpose: String,
    pub requested_capacity_bytes: u64,
    pub quota_bytes: u64,
    pub minimum_free_bytes_per_disk: u64,
    pub aggregation_provider: String,
    pub close_cleanup_policy_json: String,
    pub workflow_id: Option<String>,
    pub workflow_run_id: Option<String>,
    pub repository_revision: Option<String>,
    pub created_at_utc: String,
    pub expires_at_utc: String,
    /// Filesystem capacity measurements captured immediately before the
    /// transaction. Authoritative disk/pool state and existing reservations
    /// are re-read while the immediate transaction is held.
    pub candidates: Vec<MeasuredWorkspaceDisk>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MeasuredWorkspaceDisk {
    pub disk_id: DiskId,
    pub health_state: HealthState,
    pub available_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceReservationSnapshot {
    pub workspace_id: WorkspaceId,
    pub request_id: String,
    pub pool_id: PoolId,
    pub state: ComputeWorkspaceState,
    pub owner: String,
    pub project: String,
    pub purpose: String,
    pub requested_capacity_bytes: u64,
    pub reserved_capacity_bytes: u64,
    pub quota_bytes: u64,
    pub minimum_free_bytes_per_disk: u64,
    pub generation: u64,
    pub created_at_utc: String,
    pub updated_at_utc: String,
    pub expires_at_utc: String,
    pub allocations: Vec<WorkspaceDiskAllocation>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkspaceDiskAllocation {
    pub disk_id: DiskId,
    pub reserved_bytes: u64,
    pub state: String,
}

#[derive(Debug)]
pub enum WorkspaceMetadataError {
    Sqlite(rusqlite::Error),
    PoolNotFound {
        pool_id: PoolId,
    },
    PoolNotWritable {
        pool_id: PoolId,
        state: String,
    },
    WorkspaceNotFound {
        workspace_id: WorkspaceId,
    },
    RequestIdentityConflict {
        request_id: String,
    },
    WorkspaceIdentityConflict {
        workspace_id: WorkspaceId,
    },
    InvalidRequest {
        field: &'static str,
        reason: String,
    },
    Capacity(WorkspaceCapacityPlanError),
    InvalidStoredValue {
        field: &'static str,
        value: String,
    },
    StaleGeneration {
        expected: u64,
        actual: u64,
    },
    InvalidTransition {
        current: ComputeWorkspaceState,
        next: ComputeWorkspaceState,
    },
}

impl Display for WorkspaceMetadataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => write!(formatter, "workspace metadata failed: {error}"),
            Self::PoolNotFound { pool_id } => write!(formatter, "pool {pool_id} does not exist"),
            Self::PoolNotWritable { pool_id, state } => {
                write!(formatter, "pool {pool_id} is not writable in state {state}")
            }
            Self::WorkspaceNotFound { workspace_id } => {
                write!(formatter, "workspace {workspace_id} does not exist")
            }
            Self::RequestIdentityConflict { request_id } => write!(
                formatter,
                "workspace request identity {request_id} was reused with different content"
            ),
            Self::WorkspaceIdentityConflict { workspace_id } => write!(
                formatter,
                "workspace identity {workspace_id} is already bound to another request"
            ),
            Self::InvalidRequest { field, reason } => {
                write!(formatter, "invalid workspace {field}: {reason}")
            }
            Self::Capacity(error) => write!(formatter, "workspace capacity rejected: {error:?}"),
            Self::InvalidStoredValue { field, value } => {
                write!(formatter, "invalid stored workspace {field}: {value}")
            }
            Self::StaleGeneration { expected, actual } => write!(
                formatter,
                "workspace generation changed: expected {expected}, actual {actual}"
            ),
            Self::InvalidTransition { current, next } => {
                write!(
                    formatter,
                    "invalid workspace transition: {current:?} -> {next:?}"
                )
            }
        }
    }
}

impl std::error::Error for WorkspaceMetadataError {}

impl From<rusqlite::Error> for WorkspaceMetadataError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

pub fn reserve_workspace(
    request: &ReserveWorkspaceRequest,
) -> Result<WorkspaceReservationSnapshot, WorkspaceMetadataError> {
    validate_reservation_request(request)?;
    let mut connection = open_workspace_metadata(&request.live_sqlite_path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;

    if let Some((workspace_id, request_digest)) = transaction
        .query_row(
            "SELECT workspace_id, request_digest
             FROM compute_workspaces
             WHERE request_id = ?1",
            [&request.request_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
    {
        if request_digest != request.request_digest {
            return Err(WorkspaceMetadataError::RequestIdentityConflict {
                request_id: request.request_id.clone(),
            });
        }
        if workspace_id != request.workspace_id.as_str() {
            return Err(WorkspaceMetadataError::WorkspaceIdentityConflict {
                workspace_id: request.workspace_id.clone(),
            });
        }
        let snapshot = read_workspace_in_transaction(&transaction, &request.workspace_id)?;
        transaction.commit()?;
        return Ok(snapshot);
    }

    if transaction
        .query_row(
            "SELECT 1 FROM compute_workspaces WHERE workspace_id = ?1",
            [request.workspace_id.as_str()],
            |_| Ok(()),
        )
        .optional()?
        .is_some()
    {
        return Err(WorkspaceMetadataError::WorkspaceIdentityConflict {
            workspace_id: request.workspace_id.clone(),
        });
    }

    let pool_state = transaction
        .query_row(
            "SELECT state FROM pools WHERE pool_id = ?1",
            [request.pool_id.as_str()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| WorkspaceMetadataError::PoolNotFound {
            pool_id: request.pool_id.clone(),
        })?;
    if !is_writable_pool_state(&pool_state) {
        return Err(WorkspaceMetadataError::PoolNotWritable {
            pool_id: request.pool_id.clone(),
            state: pool_state,
        });
    }

    let candidates = transaction_candidates(&transaction, request)?;
    let plan = plan_workspace_capacity(
        &candidates,
        request.requested_capacity_bytes,
        request.minimum_free_bytes_per_disk,
    )
    .map_err(WorkspaceMetadataError::Capacity)?;
    let reserved_capacity_bytes = plan.reserved_bytes();

    transaction.execute(
        "INSERT INTO compute_workspaces (
            workspace_id, schema_version, request_id, request_digest, pool_id,
            promotion_store_id, state, owner, project, purpose, workflow_id,
            workflow_run_id, repository_revision, requested_capacity_bytes,
            reserved_capacity_bytes, quota_bytes, bytes_written,
            bytes_reclaimable, minimum_free_bytes_per_disk,
            aggregation_provider, aggregate_mount_identity,
            close_cleanup_policy_json, failure_reason, generation,
            created_at_utc, updated_at_utc, expires_at_utc
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14,
            ?15, ?16, 0, 0, ?17, ?18, NULL, ?19, NULL, 1, ?20, ?20, ?21
         )",
        params![
            request.workspace_id.as_str(),
            COMPUTE_WORKSPACE_SCHEMA_VERSION,
            request.request_id,
            request.request_digest,
            request.pool_id.as_str(),
            request.promotion_store_id.as_ref().map(StoreId::as_str),
            state_name(ComputeWorkspaceState::CapacityReserved),
            request.owner,
            request.project,
            request.purpose,
            request.workflow_id,
            request.workflow_run_id,
            request.repository_revision,
            request.requested_capacity_bytes,
            reserved_capacity_bytes,
            request.quota_bytes,
            request.minimum_free_bytes_per_disk,
            request.aggregation_provider,
            request.close_cleanup_policy_json,
            request.created_at_utc,
            request.expires_at_utc,
        ],
    )?;

    for branch in plan.branches {
        transaction.execute(
            "INSERT INTO compute_workspace_branches (
                workspace_id, disk_id, branch_id, branch_relative_path,
                reserved_bytes, state, created_at_utc, released_at_utc
             ) VALUES (?1, ?2, ?3, NULL, ?4, 'reserved', ?5, NULL)",
            params![
                request.workspace_id.as_str(),
                branch.disk_id.as_str(),
                branch.branch_id,
                branch.reserved_bytes,
                request.created_at_utc,
            ],
        )?;
    }

    let snapshot = read_workspace_in_transaction(&transaction, &request.workspace_id)?;
    transaction.commit()?;
    Ok(snapshot)
}

pub fn read_workspace_reservation(
    live_sqlite_path: impl AsRef<Path>,
    workspace_id: &WorkspaceId,
) -> Result<WorkspaceReservationSnapshot, WorkspaceMetadataError> {
    let connection = open_workspace_metadata(live_sqlite_path.as_ref())?;
    read_workspace(&connection, workspace_id)
}

pub fn list_workspace_reservations(
    live_sqlite_path: impl AsRef<Path>,
) -> Result<Vec<WorkspaceReservationSnapshot>, WorkspaceMetadataError> {
    let connection = open_workspace_metadata(live_sqlite_path.as_ref())?;
    let mut statement = connection.prepare(
        "SELECT workspace_id FROM compute_workspaces ORDER BY created_at_utc, workspace_id",
    )?;
    let ids = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    ids.into_iter()
        .map(|value| {
            let workspace_id = WorkspaceId::new(value.clone()).map_err(|_| {
                WorkspaceMetadataError::InvalidStoredValue {
                    field: "workspace_id",
                    value,
                }
            })?;
            read_workspace(&connection, &workspace_id)
        })
        .collect()
}

pub fn transition_workspace(
    live_sqlite_path: impl AsRef<Path>,
    workspace_id: &WorkspaceId,
    expected_generation: u64,
    next: ComputeWorkspaceState,
    updated_at_utc: &str,
    failure_reason: Option<&str>,
) -> Result<WorkspaceReservationSnapshot, WorkspaceMetadataError> {
    let mut connection = open_workspace_metadata(live_sqlite_path.as_ref())?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let current = read_workspace_in_transaction(&transaction, workspace_id)?;
    if current.generation != expected_generation {
        return Err(WorkspaceMetadataError::StaleGeneration {
            expected: expected_generation,
            actual: current.generation,
        });
    }
    if !current.state.can_transition_to(next) {
        return Err(WorkspaceMetadataError::InvalidTransition {
            current: current.state,
            next,
        });
    }
    let changed = transaction.execute(
        "UPDATE compute_workspaces
         SET state = ?1, generation = generation + 1, updated_at_utc = ?2,
             failure_reason = ?3
         WHERE workspace_id = ?4 AND generation = ?5",
        params![
            state_name(next),
            updated_at_utc,
            failure_reason,
            workspace_id.as_str(),
            expected_generation,
        ],
    )?;
    if changed != 1 {
        return Err(WorkspaceMetadataError::StaleGeneration {
            expected: expected_generation,
            actual: read_workspace_in_transaction(&transaction, workspace_id)?.generation,
        });
    }
    let snapshot = read_workspace_in_transaction(&transaction, workspace_id)?;
    transaction.commit()?;
    Ok(snapshot)
}

fn open_workspace_metadata(path: &Path) -> Result<Connection, WorkspaceMetadataError> {
    let connection = Connection::open(path)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.execute_batch(LIVE_SCHEMA_SQL)?;
    Ok(connection)
}

fn validate_reservation_request(
    request: &ReserveWorkspaceRequest,
) -> Result<(), WorkspaceMetadataError> {
    for (field, value) in [
        ("request_id", request.request_id.as_str()),
        ("request_digest", request.request_digest.as_str()),
        ("owner", request.owner.as_str()),
        ("project", request.project.as_str()),
        ("purpose", request.purpose.as_str()),
        (
            "aggregation_provider",
            request.aggregation_provider.as_str(),
        ),
        (
            "close_cleanup_policy_json",
            request.close_cleanup_policy_json.as_str(),
        ),
        ("created_at_utc", request.created_at_utc.as_str()),
        ("expires_at_utc", request.expires_at_utc.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(WorkspaceMetadataError::InvalidRequest {
                field,
                reason: "must not be blank".to_string(),
            });
        }
    }
    if request.quota_bytes == 0 || request.quota_bytes > request.requested_capacity_bytes {
        return Err(WorkspaceMetadataError::InvalidRequest {
            field: "quota_bytes",
            reason: "must be positive and no larger than requested capacity".to_string(),
        });
    }
    let mut disk_ids = BTreeSet::new();
    if request
        .candidates
        .iter()
        .any(|candidate| !disk_ids.insert(candidate.disk_id.clone()))
    {
        return Err(WorkspaceMetadataError::InvalidRequest {
            field: "candidates",
            reason: "disk identities must be unique".to_string(),
        });
    }
    Ok(())
}

fn transaction_candidates(
    transaction: &Transaction<'_>,
    request: &ReserveWorkspaceRequest,
) -> Result<Vec<WorkspaceCapacityCandidate>, WorkspaceMetadataError> {
    request
        .candidates
        .iter()
        .map(|candidate| {
            let state = transaction
                .query_row(
                    "SELECT state FROM disks WHERE disk_id = ?1 AND pool_id = ?2",
                    params![candidate.disk_id.as_str(), request.pool_id.as_str()],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            let disk_state = state
                .as_deref()
                .and_then(parse_disk_state)
                .unwrap_or(DiskState::Failed);
            let already_reserved_bytes = transaction.query_row(
                "SELECT COALESCE(SUM(reserved_bytes), 0)
                 FROM compute_workspace_branches
                 WHERE disk_id = ?1 AND released_at_utc IS NULL
                   AND state != 'released'",
                [candidate.disk_id.as_str()],
                |row| row.get::<_, u64>(0),
            )?;
            Ok(WorkspaceCapacityCandidate {
                disk_id: candidate.disk_id.clone(),
                disk_state,
                health_state: candidate.health_state,
                available_bytes: candidate.available_bytes,
                already_reserved_bytes,
            })
        })
        .collect()
}

fn read_workspace(
    connection: &Connection,
    workspace_id: &WorkspaceId,
) -> Result<WorkspaceReservationSnapshot, WorkspaceMetadataError> {
    let row = connection
        .query_row(
            "SELECT request_id, pool_id, state, owner, project, purpose,
                    requested_capacity_bytes, reserved_capacity_bytes,
                    quota_bytes, minimum_free_bytes_per_disk, generation,
                    created_at_utc, updated_at_utc, expires_at_utc
             FROM compute_workspaces WHERE workspace_id = ?1",
            [workspace_id.as_str()],
            map_workspace_row,
        )
        .optional()?
        .ok_or_else(|| WorkspaceMetadataError::WorkspaceNotFound {
            workspace_id: workspace_id.clone(),
        })?;
    finish_workspace_snapshot(connection, workspace_id, row)
}

fn read_workspace_in_transaction(
    transaction: &Transaction<'_>,
    workspace_id: &WorkspaceId,
) -> Result<WorkspaceReservationSnapshot, WorkspaceMetadataError> {
    let row = transaction
        .query_row(
            "SELECT request_id, pool_id, state, owner, project, purpose,
                    requested_capacity_bytes, reserved_capacity_bytes,
                    quota_bytes, minimum_free_bytes_per_disk, generation,
                    created_at_utc, updated_at_utc, expires_at_utc
             FROM compute_workspaces WHERE workspace_id = ?1",
            [workspace_id.as_str()],
            map_workspace_row,
        )
        .optional()?
        .ok_or_else(|| WorkspaceMetadataError::WorkspaceNotFound {
            workspace_id: workspace_id.clone(),
        })?;
    finish_workspace_snapshot(transaction, workspace_id, row)
}

type WorkspaceRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    u64,
    u64,
    u64,
    u64,
    u64,
    String,
    String,
    String,
);

fn map_workspace_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkspaceRow> {
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
    ))
}

fn finish_workspace_snapshot(
    connection: &Connection,
    workspace_id: &WorkspaceId,
    row: WorkspaceRow,
) -> Result<WorkspaceReservationSnapshot, WorkspaceMetadataError> {
    let pool_id =
        PoolId::new(row.1.clone()).map_err(|_| WorkspaceMetadataError::InvalidStoredValue {
            field: "pool_id",
            value: row.1.clone(),
        })?;
    let state = parse_workspace_state(&row.2).ok_or_else(|| {
        WorkspaceMetadataError::InvalidStoredValue {
            field: "state",
            value: row.2.clone(),
        }
    })?;
    let mut statement = connection.prepare(
        "SELECT disk_id, reserved_bytes, state
         FROM compute_workspace_branches
         WHERE workspace_id = ?1
         ORDER BY disk_id",
    )?;
    let allocation_rows = statement
        .query_map([workspace_id.as_str()], |allocation| {
            Ok((
                allocation.get::<_, String>(0)?,
                allocation.get::<_, u64>(1)?,
                allocation.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let allocations = allocation_rows
        .into_iter()
        .map(|(disk, reserved_bytes, state)| {
            let disk_id = DiskId::new(disk.clone()).map_err(|_| {
                WorkspaceMetadataError::InvalidStoredValue {
                    field: "disk_id",
                    value: disk,
                }
            })?;
            Ok(WorkspaceDiskAllocation {
                disk_id,
                reserved_bytes,
                state,
            })
        })
        .collect::<Result<Vec<_>, WorkspaceMetadataError>>()?;
    Ok(WorkspaceReservationSnapshot {
        workspace_id: workspace_id.clone(),
        request_id: row.0,
        pool_id,
        state,
        owner: row.3,
        project: row.4,
        purpose: row.5,
        requested_capacity_bytes: row.6,
        reserved_capacity_bytes: row.7,
        quota_bytes: row.8,
        minimum_free_bytes_per_disk: row.9,
        generation: row.10,
        created_at_utc: row.11,
        updated_at_utc: row.12,
        expires_at_utc: row.13,
        allocations,
    })
}

fn is_writable_pool_state(state: &str) -> bool {
    state == "Clean"
}

fn parse_disk_state(value: &str) -> Option<DiskState> {
    match value {
        "Candidate" => Some(DiskState::Candidate),
        "Healthy" => Some(DiskState::Healthy),
        "Draining" => Some(DiskState::Draining),
        "Retired" => Some(DiskState::Retired),
        "Failed" => Some(DiskState::Failed),
        _ => None,
    }
}

fn state_name(state: ComputeWorkspaceState) -> &'static str {
    match state {
        ComputeWorkspaceState::Requested => "requested",
        ComputeWorkspaceState::CapacityReserved => "capacity_reserved",
        ComputeWorkspaceState::Provisioning => "provisioning",
        ComputeWorkspaceState::Ready => "ready",
        ComputeWorkspaceState::Attached => "attached",
        ComputeWorkspaceState::Active => "active",
        ComputeWorkspaceState::PromotionPending => "promotion_pending",
        ComputeWorkspaceState::Closing => "closing",
        ComputeWorkspaceState::Closed => "closed",
        ComputeWorkspaceState::Expired => "expired",
        ComputeWorkspaceState::CleanupPending => "cleanup_pending",
        ComputeWorkspaceState::Cleaned => "cleaned",
        ComputeWorkspaceState::Failed => "failed",
    }
}

fn parse_workspace_state(value: &str) -> Option<ComputeWorkspaceState> {
    match value {
        "requested" => Some(ComputeWorkspaceState::Requested),
        "capacity_reserved" => Some(ComputeWorkspaceState::CapacityReserved),
        "provisioning" => Some(ComputeWorkspaceState::Provisioning),
        "ready" => Some(ComputeWorkspaceState::Ready),
        "attached" => Some(ComputeWorkspaceState::Attached),
        "active" => Some(ComputeWorkspaceState::Active),
        "promotion_pending" => Some(ComputeWorkspaceState::PromotionPending),
        "closing" => Some(ComputeWorkspaceState::Closing),
        "closed" => Some(ComputeWorkspaceState::Closed),
        "expired" => Some(ComputeWorkspaceState::Expired),
        "cleanup_pending" => Some(ComputeWorkspaceState::CleanupPending),
        "cleaned" => Some(ComputeWorkspaceState::Cleaned),
        "failed" => Some(ComputeWorkspaceState::Failed),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        list_workspace_reservations, read_workspace_reservation, reserve_workspace,
        transition_workspace, MeasuredWorkspaceDisk, ReserveWorkspaceRequest,
        WorkspaceMetadataError,
    };
    use crate::LIVE_SCHEMA_SQL;
    use dasobjectstore_core::ids::{DiskId, PoolId, WorkspaceId};
    use dasobjectstore_core::lifecycle::HealthState;
    use dasobjectstore_core::workspace::{ComputeWorkspaceState, WorkspaceCapacityPlanError};
    use rusqlite::Connection;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn reserves_aggregate_capacity_atomically_across_disks() {
        let database = fixture_database("aggregate");
        let request = request(&database, "workspace-a", "request-a", 150, 150);

        let snapshot = reserve_workspace(&request).expect("reserve aggregate workspace");

        assert_eq!(snapshot.state, ComputeWorkspaceState::CapacityReserved);
        assert_eq!(snapshot.reserved_capacity_bytes, 150);
        assert_eq!(snapshot.allocations.len(), 2);
        assert_eq!(
            snapshot
                .allocations
                .iter()
                .map(|allocation| allocation.reserved_bytes)
                .sum::<u64>(),
            150
        );
        cleanup_database(&database);
    }

    #[test]
    fn replays_identical_request_and_rejects_conflicting_digest() {
        let database = fixture_database("request-replay");
        let request = request(&database, "workspace-a", "request-a", 80, 80);
        let first = reserve_workspace(&request).expect("first reservation");
        let replay = reserve_workspace(&request).expect("identical replay");
        assert_eq!(replay, first);

        let mut conflicting = request;
        conflicting.request_digest = "different-digest".to_string();
        let error = reserve_workspace(&conflicting).expect_err("digest conflict");
        assert!(matches!(
            error,
            WorkspaceMetadataError::RequestIdentityConflict { .. }
        ));
        cleanup_database(&database);
    }

    #[test]
    fn stale_capacity_measurements_cannot_overbook_active_reservations() {
        let database = fixture_database("stale-measurement");
        reserve_workspace(&request(&database, "workspace-a", "request-a", 90, 90))
            .expect("first reservation");

        let error = reserve_workspace(&request(&database, "workspace-b", "request-b", 120, 120))
            .expect_err("second reservation must subtract active claims");
        assert!(matches!(
            error,
            WorkspaceMetadataError::Capacity(
                WorkspaceCapacityPlanError::InsufficientAggregateCapacity { .. }
            )
        ));
        assert_eq!(
            list_workspace_reservations(&database)
                .expect("list workspaces")
                .len(),
            1
        );
        cleanup_database(&database);
    }

    #[test]
    fn concurrent_immediate_transactions_admit_only_one_claim() {
        let database = fixture_database_with_disks("concurrent", &[("disk-a", "Healthy")]);
        let barrier = Arc::new(Barrier::new(3));
        let handles = ["a", "b"]
            .into_iter()
            .map(|suffix| {
                let database = database.clone();
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    let mut request = request_with_candidates(
                        &database,
                        &format!("workspace-{suffix}"),
                        &format!("request-{suffix}"),
                        80,
                        80,
                        vec![candidate("disk-a", 100, HealthState::Healthy)],
                    );
                    request.minimum_free_bytes_per_disk = 10;
                    barrier.wait();
                    reserve_workspace(&request)
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        let results = handles
            .into_iter()
            .map(|handle| handle.join().expect("reservation thread"))
            .collect::<Vec<_>>();

        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(
                    result,
                    Err(WorkspaceMetadataError::Capacity(
                        WorkspaceCapacityPlanError::InsufficientAggregateCapacity { .. }
                    ))
                ))
                .count(),
            1
        );
        cleanup_database(&database);
    }

    #[test]
    fn excludes_draining_and_unhealthy_disks_inside_transaction() {
        let database = fixture_database_with_disks(
            "eligibility",
            &[("disk-a", "Healthy"), ("disk-b", "Draining")],
        );
        let request = request_with_candidates(
            &database,
            "workspace-a",
            "request-a",
            100,
            100,
            vec![
                candidate("disk-a", 60, HealthState::Suspect),
                candidate("disk-b", 100, HealthState::Healthy),
            ],
        );
        let error = reserve_workspace(&request).expect_err("no eligible capacity");
        assert!(matches!(
            error,
            WorkspaceMetadataError::Capacity(
                WorkspaceCapacityPlanError::InsufficientAggregateCapacity {
                    available_bytes: 0,
                    ..
                }
            )
        ));
        cleanup_database(&database);
    }

    #[test]
    fn rejects_read_only_pool_without_creating_rows() {
        let database = fixture_database("read-only-pool");
        let connection = Connection::open(&database).expect("open fixture");
        connection
            .execute(
                "UPDATE pools SET state = 'ReadOnly' WHERE pool_id = 'pool-a'",
                [],
            )
            .expect("mark pool read only");
        drop(connection);

        let error = reserve_workspace(&request(&database, "workspace-a", "request-a", 80, 80))
            .expect_err("read-only pool must fail");
        assert!(matches!(
            error,
            WorkspaceMetadataError::PoolNotWritable { .. }
        ));
        assert!(list_workspace_reservations(&database)
            .expect("list workspaces")
            .is_empty());
        cleanup_database(&database);
    }

    #[test]
    fn transitions_require_current_generation_and_valid_edge() {
        let database = fixture_database("transition");
        reserve_workspace(&request(&database, "workspace-a", "request-a", 80, 80))
            .expect("reservation");
        let workspace_id = WorkspaceId::new("workspace-a").expect("workspace id");
        let provisioning = transition_workspace(
            &database,
            &workspace_id,
            1,
            ComputeWorkspaceState::Provisioning,
            "2026-07-25T00:01:00Z",
            None,
        )
        .expect("valid transition");
        assert_eq!(provisioning.generation, 2);

        let stale = transition_workspace(
            &database,
            &workspace_id,
            1,
            ComputeWorkspaceState::Ready,
            "2026-07-25T00:02:00Z",
            None,
        )
        .expect_err("stale generation");
        assert!(matches!(
            stale,
            WorkspaceMetadataError::StaleGeneration {
                expected: 1,
                actual: 2
            }
        ));

        let invalid = transition_workspace(
            &database,
            &workspace_id,
            2,
            ComputeWorkspaceState::Attached,
            "2026-07-25T00:02:00Z",
            None,
        )
        .expect_err("skipped ready state");
        assert!(matches!(
            invalid,
            WorkspaceMetadataError::InvalidTransition { .. }
        ));
        cleanup_database(&database);
    }

    #[test]
    fn inspection_is_path_redacted() {
        let database = fixture_database("redaction");
        let request = request(&database, "workspace-a", "request-a", 80, 80);
        reserve_workspace(&request).expect("reservation");
        let workspace_id = WorkspaceId::new("workspace-a").expect("workspace id");
        let snapshot =
            read_workspace_reservation(&database, &workspace_id).expect("inspect workspace");
        let json = serde_json::to_string(&snapshot).expect("serialize snapshot");
        assert!(!json.contains("branch_relative_path"));
        assert!(!json.contains("/srv/"));
        cleanup_database(&database);
    }

    #[test]
    fn reservation_preserves_existing_object_and_placement_metadata() {
        let database = fixture_database("preservation");
        let connection = Connection::open(&database).expect("open fixture");
        connection
            .execute(
                "INSERT INTO stores (
                    store_id, pool_id, class, policy_json, created_at_utc, updated_at_utc
                 ) VALUES (
                    'store-a', 'pool-a', 'generated_data', '{}',
                    '2026-07-25T00:00:00Z', '2026-07-25T00:00:00Z'
                 )",
                [],
            )
            .expect("insert store");
        connection
            .execute(
                "INSERT INTO objects (
                    object_id, store_id, state, size_bytes, content_hash,
                    created_at_utc, updated_at_utc
                 ) VALUES (
                    'object-a', 'store-a', 'Protected', 12, 'abc',
                    '2026-07-25T00:00:00Z', '2026-07-25T00:00:00Z'
                 )",
                [],
            )
            .expect("insert object");
        connection
            .execute(
                "INSERT INTO placements (
                    placement_id, object_id, disk_id, relative_path, content_hash,
                    verified_at_utc, created_at_utc
                 ) VALUES (
                    'placement-a', 'object-a', 'disk-a', 'objects/object-a', 'abc',
                    '2026-07-25T00:00:00Z', '2026-07-25T00:00:00Z'
                 )",
                [],
            )
            .expect("insert placement");
        drop(connection);

        reserve_workspace(&request(&database, "workspace-a", "request-a", 80, 80))
            .expect("reserve workspace");

        let connection = Connection::open(&database).expect("reopen fixture");
        let row = connection
            .query_row(
                "SELECT objects.object_id, placements.relative_path
                 FROM objects JOIN placements USING (object_id)
                 WHERE placements.placement_id = 'placement-a'",
                [],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .expect("existing metadata remains");
        assert_eq!(
            row,
            ("object-a".to_string(), "objects/object-a".to_string())
        );
        cleanup_database(&database);
    }

    fn request(
        database: &Path,
        workspace_id: &str,
        request_id: &str,
        capacity: u64,
        quota: u64,
    ) -> ReserveWorkspaceRequest {
        request_with_candidates(
            database,
            workspace_id,
            request_id,
            capacity,
            quota,
            vec![
                candidate("disk-a", 100, HealthState::Healthy),
                candidate("disk-b", 100, HealthState::Healthy),
            ],
        )
    }

    fn request_with_candidates(
        database: &Path,
        workspace_id: &str,
        request_id: &str,
        capacity: u64,
        quota: u64,
        candidates: Vec<MeasuredWorkspaceDisk>,
    ) -> ReserveWorkspaceRequest {
        ReserveWorkspaceRequest {
            live_sqlite_path: database.to_path_buf(),
            workspace_id: WorkspaceId::new(workspace_id).expect("workspace id"),
            request_id: request_id.to_string(),
            request_digest: format!("digest-{request_id}"),
            pool_id: PoolId::new("pool-a").expect("pool id"),
            promotion_store_id: None,
            owner: "owner-a".to_string(),
            project: "project-a".to_string(),
            purpose: "synthetic test".to_string(),
            requested_capacity_bytes: capacity,
            quota_bytes: quota,
            minimum_free_bytes_per_disk: 0,
            aggregation_provider: "mergerfs".to_string(),
            close_cleanup_policy_json: "{}".to_string(),
            workflow_id: None,
            workflow_run_id: None,
            repository_revision: None,
            created_at_utc: "2026-07-25T00:00:00Z".to_string(),
            expires_at_utc: "2026-08-01T00:00:00Z".to_string(),
            candidates,
        }
    }

    fn candidate(
        disk_id: &str,
        available_bytes: u64,
        health_state: HealthState,
    ) -> MeasuredWorkspaceDisk {
        MeasuredWorkspaceDisk {
            disk_id: DiskId::new(disk_id).expect("disk id"),
            health_state,
            available_bytes,
        }
    }

    fn fixture_database(name: &str) -> PathBuf {
        fixture_database_with_disks(name, &[("disk-a", "Healthy"), ("disk-b", "Healthy")])
    }

    fn fixture_database_with_disks(name: &str, disks: &[(&str, &str)]) -> PathBuf {
        let path = temp_root(name).join("live.sqlite");
        fs::create_dir_all(path.parent().expect("database parent")).expect("create temp root");
        let connection = Connection::open(&path).expect("open fixture database");
        connection.execute_batch(LIVE_SCHEMA_SQL).expect("schema");
        connection
            .execute(
                "INSERT INTO pools (pool_id, state, created_at_utc, updated_at_utc)
                 VALUES ('pool-a', 'Clean', '2026-07-25T00:00:00Z',
                         '2026-07-25T00:00:00Z')",
                [],
            )
            .expect("insert pool");
        for (disk_id, state) in disks {
            connection
                .execute(
                    "INSERT INTO disks (
                        disk_id, pool_id, role, state, created_at_utc, updated_at_utc
                     ) VALUES (?1, 'pool-a', 'hdd_capacity', ?2,
                               '2026-07-25T00:00:00Z', '2026-07-25T00:00:00Z')",
                    [disk_id, state],
                )
                .expect("insert disk");
        }
        drop(connection);
        path
    }

    fn cleanup_database(database: &Path) {
        fs::remove_dir_all(database.parent().expect("database parent")).expect("cleanup fixture");
    }

    fn temp_root(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dasobjectstore-workspace-{name}-{}-{nanos}",
            std::process::id()
        ))
    }
}
