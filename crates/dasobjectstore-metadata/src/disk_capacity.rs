use crate::schema::LIVE_SCHEMA_SQL;
use dasobjectstore_core::ids::DiskId;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Display};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiskCapacityClaimKind {
    Workspace,
    Ingest,
    Destage,
    Repair,
    Evacuation,
}

impl DiskCapacityClaimKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Workspace => "workspace",
            Self::Ingest => "ingest",
            Self::Destage => "destage",
            Self::Repair => "repair",
            Self::Evacuation => "evacuation",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiskCapacityClaimRequest {
    pub live_sqlite_path: PathBuf,
    pub kind: DiskCapacityClaimKind,
    pub owner_id: String,
    pub request_id: String,
    pub request_digest: String,
    pub lease_owner: Option<String>,
    pub lease_expires_at_utc: Option<String>,
    pub created_at_utc: String,
    pub allocations: Vec<DiskCapacityClaimAllocation>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiskCapacityClaimAllocation {
    pub disk_id: DiskId,
    /// Raw filesystem availability measured before the transaction.
    pub measured_available_bytes: u64,
    pub requested_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiskCapacityClaim {
    pub claim_id: String,
    pub kind: DiskCapacityClaimKind,
    pub owner_id: String,
    pub disk_id: DiskId,
    pub reserved_bytes: u64,
    pub consumed_bytes: u64,
    pub state: String,
}

#[derive(Debug)]
pub enum DiskCapacityClaimError {
    Sqlite(rusqlite::Error),
    InvalidRequest {
        field: &'static str,
        reason: String,
    },
    RequestConflict {
        kind: DiskCapacityClaimKind,
        owner_id: String,
    },
    IneligibleDisk {
        disk_id: DiskId,
        state: Option<String>,
    },
    InsufficientCapacity {
        disk_id: DiskId,
        requested_bytes: u64,
        available_after_claims_bytes: u64,
    },
    ClaimNotFound {
        kind: DiskCapacityClaimKind,
        owner_id: String,
    },
    InvalidConsumption {
        disk_id: DiskId,
        consumed_bytes: u64,
        reserved_bytes: u64,
        previous_consumed_bytes: u64,
    },
    InvalidStoredKind(String),
    InvalidStoredDisk(String),
}

impl Display for DiskCapacityClaimError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => write!(formatter, "disk capacity claim failed: {error}"),
            Self::InvalidRequest { field, reason } => {
                write!(formatter, "invalid disk capacity claim {field}: {reason}")
            }
            Self::RequestConflict { kind, owner_id } => write!(
                formatter,
                "conflicting {:?} capacity claim for owner {owner_id}",
                kind
            ),
            Self::IneligibleDisk { disk_id, state } => write!(
                formatter,
                "disk {disk_id} is not eligible for a capacity claim (state {})",
                state.as_deref().unwrap_or("missing")
            ),
            Self::InsufficientCapacity {
                disk_id,
                requested_bytes,
                available_after_claims_bytes,
            } => write!(
                formatter,
                "disk {disk_id} has {available_after_claims_bytes} claimable bytes, requires {requested_bytes}"
            ),
            Self::ClaimNotFound { kind, owner_id } => {
                write!(formatter, "no active {:?} capacity claim for {owner_id}", kind)
            }
            Self::InvalidConsumption {
                disk_id,
                consumed_bytes,
                reserved_bytes,
                previous_consumed_bytes,
            } => write!(
                formatter,
                "invalid consumption {consumed_bytes} for disk {disk_id}; reserved {reserved_bytes}, previously accounted {previous_consumed_bytes}"
            ),
            Self::InvalidStoredKind(value) => {
                write!(formatter, "invalid stored capacity claim kind: {value}")
            }
            Self::InvalidStoredDisk(value) => {
                write!(formatter, "invalid stored capacity claim disk: {value}")
            }
        }
    }
}

impl std::error::Error for DiskCapacityClaimError {}

impl From<rusqlite::Error> for DiskCapacityClaimError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

pub fn acquire_disk_capacity_claims(
    request: &DiskCapacityClaimRequest,
) -> Result<Vec<DiskCapacityClaim>, DiskCapacityClaimError> {
    validate_request(request)?;
    let mut connection = open(&request.live_sqlite_path)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let existing = read_owner_claims(&transaction, request.kind, &request.owner_id)?;
    if !existing.is_empty() {
        let stored_digest: String = transaction.query_row(
            "SELECT request_digest FROM disk_capacity_claims
             WHERE claim_kind=?1 AND owner_id=?2 LIMIT 1",
            params![request.kind.as_str(), request.owner_id],
            |row| row.get(0),
        )?;
        if stored_digest == request.request_digest
            && existing.iter().all(|claim| claim.state == "active")
            && claims_match(request, &existing)
        {
            transaction.execute(
                "UPDATE disk_capacity_claims
                 SET lease_owner=?1, lease_expires_at_utc=?2, updated_at_utc=?3
                 WHERE claim_kind=?4 AND owner_id=?5 AND state='active'",
                params![
                    request.lease_owner,
                    request.lease_expires_at_utc,
                    request.created_at_utc,
                    request.kind.as_str(),
                    request.owner_id,
                ],
            )?;
            transaction.commit()?;
            return Ok(existing);
        }
        return Err(DiskCapacityClaimError::RequestConflict {
            kind: request.kind,
            owner_id: request.owner_id.clone(),
        });
    }

    for allocation in &request.allocations {
        let state = transaction
            .query_row(
                "SELECT state FROM disks WHERE disk_id=?1",
                [allocation.disk_id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if !matches!(state.as_deref(), Some("Healthy" | "Watch")) {
            return Err(DiskCapacityClaimError::IneligibleDisk {
                disk_id: allocation.disk_id.clone(),
                state,
            });
        }
        let outstanding = outstanding_claim_bytes(&transaction, &allocation.disk_id)?;
        let available_after_claims = allocation
            .measured_available_bytes
            .saturating_sub(outstanding);
        if available_after_claims < allocation.requested_bytes {
            return Err(DiskCapacityClaimError::InsufficientCapacity {
                disk_id: allocation.disk_id.clone(),
                requested_bytes: allocation.requested_bytes,
                available_after_claims_bytes: available_after_claims,
            });
        }
    }

    for allocation in &request.allocations {
        transaction.execute(
            "INSERT INTO disk_capacity_claims (
                claim_id, claim_kind, owner_id, request_id, request_digest,
                disk_id, state, reserved_bytes, consumed_bytes, lease_owner,
                lease_expires_at_utc, created_at_utc, updated_at_utc,
                released_at_utc
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', ?7, 0, ?8, ?9,
                       ?10, ?10, NULL)",
            params![
                claim_id(request.kind, &request.owner_id, &allocation.disk_id),
                request.kind.as_str(),
                request.owner_id,
                request.request_id,
                request.request_digest,
                allocation.disk_id.as_str(),
                allocation.requested_bytes,
                request.lease_owner,
                request.lease_expires_at_utc,
                request.created_at_utc,
            ],
        )?;
    }
    let claims = read_owner_claims(&transaction, request.kind, &request.owner_id)?;
    transaction.commit()?;
    Ok(claims)
}

pub fn release_disk_capacity_claims(
    live_sqlite_path: impl AsRef<Path>,
    kind: DiskCapacityClaimKind,
    owner_id: &str,
    released_at_utc: &str,
) -> Result<usize, DiskCapacityClaimError> {
    let connection = open(live_sqlite_path.as_ref())?;
    let changed = connection.execute(
        "UPDATE disk_capacity_claims
         SET state='released', released_at_utc=?1, updated_at_utc=?1,
             lease_owner=NULL, lease_expires_at_utc=NULL
         WHERE claim_kind=?2 AND owner_id=?3 AND state='active'
           AND released_at_utc IS NULL",
        params![released_at_utc, kind.as_str(), owner_id],
    )?;
    if changed == 0 {
        let already_released: bool = connection.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM disk_capacity_claims
                WHERE claim_kind=?1 AND owner_id=?2 AND state='released'
            )",
            params![kind.as_str(), owner_id],
            |row| row.get(0),
        )?;
        if !already_released {
            return Err(DiskCapacityClaimError::ClaimNotFound {
                kind,
                owner_id: owner_id.to_string(),
            });
        }
    }
    Ok(changed)
}

pub fn update_disk_capacity_claim_consumption(
    live_sqlite_path: impl AsRef<Path>,
    kind: DiskCapacityClaimKind,
    owner_id: &str,
    disk_id: &DiskId,
    consumed_bytes: u64,
    updated_at_utc: &str,
) -> Result<DiskCapacityClaim, DiskCapacityClaimError> {
    let mut connection = open(live_sqlite_path.as_ref())?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let (reserved_bytes, previous_consumed_bytes) = transaction
        .query_row(
            "SELECT reserved_bytes, consumed_bytes
             FROM disk_capacity_claims
             WHERE claim_kind=?1 AND owner_id=?2 AND disk_id=?3
               AND state='active' AND released_at_utc IS NULL",
            params![kind.as_str(), owner_id, disk_id.as_str()],
            |row| Ok((row.get::<_, u64>(0)?, row.get::<_, u64>(1)?)),
        )
        .optional()?
        .ok_or_else(|| DiskCapacityClaimError::ClaimNotFound {
            kind,
            owner_id: owner_id.to_string(),
        })?;
    if consumed_bytes < previous_consumed_bytes || consumed_bytes > reserved_bytes {
        return Err(DiskCapacityClaimError::InvalidConsumption {
            disk_id: disk_id.clone(),
            consumed_bytes,
            reserved_bytes,
            previous_consumed_bytes,
        });
    }
    transaction.execute(
        "UPDATE disk_capacity_claims
         SET consumed_bytes=?1, updated_at_utc=?2
         WHERE claim_kind=?3 AND owner_id=?4 AND disk_id=?5
           AND state='active' AND released_at_utc IS NULL",
        params![
            consumed_bytes,
            updated_at_utc,
            kind.as_str(),
            owner_id,
            disk_id.as_str(),
        ],
    )?;
    let claim = read_owner_claims(&transaction, kind, owner_id)?
        .into_iter()
        .find(|claim| &claim.disk_id == disk_id)
        .ok_or_else(|| DiskCapacityClaimError::ClaimNotFound {
            kind,
            owner_id: owner_id.to_string(),
        })?;
    transaction.commit()?;
    Ok(claim)
}

pub fn read_outstanding_disk_capacity(
    live_sqlite_path: impl AsRef<Path>,
) -> Result<BTreeMap<DiskId, u64>, DiskCapacityClaimError> {
    read_outstanding_disk_capacity_excluding(live_sqlite_path, None)
}

pub fn read_outstanding_disk_capacity_excluding(
    live_sqlite_path: impl AsRef<Path>,
    excluded_owner: Option<(DiskCapacityClaimKind, &str)>,
) -> Result<BTreeMap<DiskId, u64>, DiskCapacityClaimError> {
    let connection = open(live_sqlite_path.as_ref())?;
    let mut statement = connection.prepare(
        "SELECT disk_id, SUM(reserved_bytes - consumed_bytes)
         FROM disk_capacity_claims
         WHERE state='active' AND released_at_utc IS NULL
           AND NOT (claim_kind=?1 AND owner_id=?2)
         GROUP BY disk_id ORDER BY disk_id",
    )?;
    let (excluded_kind, excluded_id) = excluded_owner
        .map(|(kind, owner)| (kind.as_str(), owner))
        .unwrap_or(("", ""));
    let rows = statement
        .query_map(params![excluded_kind, excluded_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(value, bytes)| {
            let disk = DiskId::new(value.clone())
                .map_err(|_| DiskCapacityClaimError::InvalidStoredDisk(value))?;
            Ok((disk, bytes))
        })
        .collect()
}

pub(crate) fn outstanding_claim_bytes(
    connection: &Connection,
    disk_id: &DiskId,
) -> Result<u64, rusqlite::Error> {
    connection.query_row(
        "SELECT COALESCE(SUM(reserved_bytes - consumed_bytes), 0)
         FROM disk_capacity_claims
         WHERE disk_id=?1 AND state='active' AND released_at_utc IS NULL",
        [disk_id.as_str()],
        |row| row.get(0),
    )
}

fn open(path: &Path) -> Result<Connection, DiskCapacityClaimError> {
    let connection = Connection::open(path)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.execute_batch(LIVE_SCHEMA_SQL)?;
    Ok(connection)
}

fn validate_request(request: &DiskCapacityClaimRequest) -> Result<(), DiskCapacityClaimError> {
    for (field, value) in [
        ("owner_id", request.owner_id.as_str()),
        ("request_id", request.request_id.as_str()),
        ("request_digest", request.request_digest.as_str()),
        ("created_at_utc", request.created_at_utc.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(DiskCapacityClaimError::InvalidRequest {
                field,
                reason: "must not be blank".to_string(),
            });
        }
    }
    if request.allocations.is_empty() {
        return Err(DiskCapacityClaimError::InvalidRequest {
            field: "allocations",
            reason: "must not be empty".to_string(),
        });
    }
    let mut disks = BTreeSet::new();
    if request.allocations.iter().any(|allocation| {
        allocation.requested_bytes == 0 || !disks.insert(allocation.disk_id.clone())
    }) {
        return Err(DiskCapacityClaimError::InvalidRequest {
            field: "allocations",
            reason: "must contain unique disks with positive byte counts".to_string(),
        });
    }
    Ok(())
}

fn read_owner_claims(
    connection: &Connection,
    kind: DiskCapacityClaimKind,
    owner_id: &str,
) -> Result<Vec<DiskCapacityClaim>, DiskCapacityClaimError> {
    let mut statement = connection.prepare(
        "SELECT claim_id, claim_kind, owner_id, disk_id, reserved_bytes,
                consumed_bytes, state
         FROM disk_capacity_claims
         WHERE claim_kind=?1 AND owner_id=?2
         ORDER BY disk_id",
    )?;
    let rows = statement
        .query_map(params![kind.as_str(), owner_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, u64>(4)?,
                row.get::<_, u64>(5)?,
                row.get::<_, String>(6)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(
            |(claim_id, kind_value, owner_id, disk_value, reserved, consumed, state)| {
                let stored_kind = parse_kind(&kind_value)
                    .ok_or(DiskCapacityClaimError::InvalidStoredKind(kind_value))?;
                let disk_id = DiskId::new(disk_value.clone())
                    .map_err(|_| DiskCapacityClaimError::InvalidStoredDisk(disk_value))?;
                Ok(DiskCapacityClaim {
                    claim_id,
                    kind: stored_kind,
                    owner_id,
                    disk_id,
                    reserved_bytes: reserved,
                    consumed_bytes: consumed,
                    state,
                })
            },
        )
        .collect()
}

fn claims_match(request: &DiskCapacityClaimRequest, existing: &[DiskCapacityClaim]) -> bool {
    let expected = request
        .allocations
        .iter()
        .map(|allocation| (allocation.disk_id.clone(), allocation.requested_bytes))
        .collect::<BTreeMap<_, _>>();
    let actual = existing
        .iter()
        .map(|claim| (claim.disk_id.clone(), claim.reserved_bytes))
        .collect::<BTreeMap<_, _>>();
    expected == actual
}

fn claim_id(kind: DiskCapacityClaimKind, owner_id: &str, disk_id: &DiskId) -> String {
    format!("{}:{owner_id}:{}", kind.as_str(), disk_id.as_str())
}

fn parse_kind(value: &str) -> Option<DiskCapacityClaimKind> {
    match value {
        "workspace" => Some(DiskCapacityClaimKind::Workspace),
        "ingest" => Some(DiskCapacityClaimKind::Ingest),
        "destage" => Some(DiskCapacityClaimKind::Destage),
        "repair" => Some(DiskCapacityClaimKind::Repair),
        "evacuation" => Some(DiskCapacityClaimKind::Evacuation),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        acquire_disk_capacity_claims, read_outstanding_disk_capacity, release_disk_capacity_claims,
        update_disk_capacity_claim_consumption, DiskCapacityClaimAllocation,
        DiskCapacityClaimError, DiskCapacityClaimKind, DiskCapacityClaimRequest,
    };
    use crate::LIVE_SCHEMA_SQL;
    use dasobjectstore_core::ids::DiskId;
    use rusqlite::Connection;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn claims_from_different_subsystems_share_one_capacity_limit() {
        let database = fixture("cross-subsystem");
        acquire_disk_capacity_claims(&request(
            &database,
            DiskCapacityClaimKind::Workspace,
            "workspace-a",
            70,
        ))
        .expect("workspace claim");
        let error = acquire_disk_capacity_claims(&request(
            &database,
            DiskCapacityClaimKind::Destage,
            "object-a",
            40,
        ))
        .expect_err("destage must see workspace claim");
        assert!(matches!(
            error,
            DiskCapacityClaimError::InsufficientCapacity {
                available_after_claims_bytes: 30,
                ..
            }
        ));
        cleanup(&database);
    }

    #[test]
    fn immediate_transactions_prevent_cross_kind_overcommit() {
        let database = fixture("concurrent");
        let barrier = Arc::new(Barrier::new(3));
        let handles = [
            (DiskCapacityClaimKind::Workspace, "workspace-a"),
            (DiskCapacityClaimKind::Repair, "repair-a"),
        ]
        .into_iter()
        .map(|(kind, owner)| {
            let database = database.clone();
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                let request = request(&database, kind, owner, 70);
                barrier.wait();
                acquire_disk_capacity_claims(&request)
            })
        })
        .collect::<Vec<_>>();
        barrier.wait();
        let results = handles
            .into_iter()
            .map(|handle| handle.join().expect("claim thread"))
            .collect::<Vec<_>>();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(
                    result,
                    Err(DiskCapacityClaimError::InsufficientCapacity { .. })
                ))
                .count(),
            1
        );
        cleanup(&database);
    }

    #[test]
    fn identical_replay_is_stable_and_release_is_idempotent() {
        let database = fixture("replay-release");
        let request = request(&database, DiskCapacityClaimKind::Evacuation, "move-a", 60);
        let first = acquire_disk_capacity_claims(&request).expect("claim");
        let replay = acquire_disk_capacity_claims(&request).expect("replay");
        assert_eq!(replay, first);
        assert_eq!(
            read_outstanding_disk_capacity(&database)
                .expect("outstanding claims")
                .values()
                .sum::<u64>(),
            60
        );
        assert_eq!(
            release_disk_capacity_claims(
                &database,
                DiskCapacityClaimKind::Evacuation,
                "move-a",
                "2026-07-25T01:00:00Z",
            )
            .expect("release"),
            1
        );
        assert_eq!(
            release_disk_capacity_claims(
                &database,
                DiskCapacityClaimKind::Evacuation,
                "move-a",
                "2026-07-25T01:00:00Z",
            )
            .expect("idempotent release"),
            0
        );
        assert!(read_outstanding_disk_capacity(&database)
            .expect("released claims excluded")
            .is_empty());
        cleanup(&database);
    }

    #[test]
    fn conflicting_owner_replay_is_rejected() {
        let database = fixture("conflict");
        let request = request(&database, DiskCapacityClaimKind::Ingest, "ingest-a", 50);
        acquire_disk_capacity_claims(&request).expect("claim");
        let mut conflict = request;
        conflict.request_digest = "different".to_string();
        let error = acquire_disk_capacity_claims(&conflict).expect_err("conflict");
        assert!(matches!(
            error,
            DiskCapacityClaimError::RequestConflict { .. }
        ));
        cleanup(&database);
    }

    #[test]
    fn accounted_consumption_reduces_only_the_outstanding_reservation() {
        let database = fixture("consumption");
        acquire_disk_capacity_claims(&request(
            &database,
            DiskCapacityClaimKind::Workspace,
            "workspace-a",
            80,
        ))
        .expect("claim");
        let disk_id = DiskId::new("disk-a").expect("disk");
        update_disk_capacity_claim_consumption(
            &database,
            DiskCapacityClaimKind::Workspace,
            "workspace-a",
            &disk_id,
            30,
            "2026-07-25T00:30:00Z",
        )
        .expect("account consumption");
        assert_eq!(
            read_outstanding_disk_capacity(&database)
                .expect("outstanding")
                .values()
                .sum::<u64>(),
            50
        );
        let error = update_disk_capacity_claim_consumption(
            &database,
            DiskCapacityClaimKind::Workspace,
            "workspace-a",
            &disk_id,
            29,
            "2026-07-25T00:31:00Z",
        )
        .expect_err("consumption cannot move backwards");
        assert!(matches!(
            error,
            DiskCapacityClaimError::InvalidConsumption { .. }
        ));
        cleanup(&database);
    }

    fn request(
        database: &Path,
        kind: DiskCapacityClaimKind,
        owner_id: &str,
        bytes: u64,
    ) -> DiskCapacityClaimRequest {
        DiskCapacityClaimRequest {
            live_sqlite_path: database.to_path_buf(),
            kind,
            owner_id: owner_id.to_string(),
            request_id: format!("request-{owner_id}"),
            request_digest: format!("digest-{owner_id}-{bytes}"),
            lease_owner: Some("worker-a".to_string()),
            lease_expires_at_utc: Some("2026-07-25T02:00:00Z".to_string()),
            created_at_utc: "2026-07-25T00:00:00Z".to_string(),
            allocations: vec![DiskCapacityClaimAllocation {
                disk_id: DiskId::new("disk-a").expect("disk id"),
                measured_available_bytes: 100,
                requested_bytes: bytes,
            }],
        }
    }

    fn fixture(name: &str) -> PathBuf {
        let path = temp_root(name).join("live.sqlite");
        fs::create_dir_all(path.parent().expect("database parent")).expect("create fixture");
        let connection = Connection::open(&path).expect("open fixture");
        connection.execute_batch(LIVE_SCHEMA_SQL).expect("schema");
        connection
            .execute(
                "INSERT INTO pools (pool_id, state, created_at_utc, updated_at_utc)
                 VALUES ('pool-a', 'Clean', '2026-07-25T00:00:00Z',
                         '2026-07-25T00:00:00Z')",
                [],
            )
            .expect("pool");
        connection
            .execute(
                "INSERT INTO disks (
                    disk_id, pool_id, role, state, created_at_utc, updated_at_utc
                 ) VALUES (
                    'disk-a', 'pool-a', 'hdd_capacity', 'Healthy',
                    '2026-07-25T00:00:00Z', '2026-07-25T00:00:00Z'
                 )",
                [],
            )
            .expect("disk");
        drop(connection);
        path
    }

    fn cleanup(database: &Path) {
        fs::remove_dir_all(database.parent().expect("database parent")).expect("cleanup");
    }

    fn temp_root(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dasobjectstore-capacity-claim-{name}-{}-{nanos}",
            std::process::id()
        ))
    }
}
