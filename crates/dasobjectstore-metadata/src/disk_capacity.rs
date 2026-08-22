use crate::schema::LIVE_SCHEMA_SQL;
use dasobjectstore_core::ids::{DiskId, IngestJobId, ObjectId};
use dasobjectstore_core::utc::parse_canonical_utc_timestamp_seconds;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
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

/// Startup-only recovery of claims owned by the direct file-ingest pipeline.
///
/// These claims protect writes performed by threads in one daemon process and
/// are deliberately distinct from durable destage claims. A daemon restart
/// proves that a claim created by an earlier process can no longer have an
/// active writer. Only claims whose generated identity is internally
/// consistent and whose last update predates this process are released;
/// leased, current, or unfamiliar claims remain untouched.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AbandonedIngestCapacityClaimRecoveryReport {
    pub owners_scanned: u64,
    pub owners_released: u64,
    pub claims_released: u64,
    pub reclaimed_bytes: u64,
    pub current_owners_retained: u64,
    pub leased_owners_retained: u64,
    pub unrecognized_owners_retained: u64,
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
    let claims = acquire_disk_capacity_claims_in_transaction(&transaction, request)?;
    transaction.commit()?;
    Ok(claims)
}

pub fn read_disk_capacity_claims(
    live_sqlite_path: impl AsRef<Path>,
    kind: DiskCapacityClaimKind,
    owner_id: &str,
) -> Result<Vec<DiskCapacityClaim>, DiskCapacityClaimError> {
    let connection = open(live_sqlite_path.as_ref())?;
    read_owner_claims(&connection, kind, owner_id)
}

/// Read disk identities that are currently eligible to receive new settlement
/// writes. `Watch` remains serviceable under the placement contract; suspect,
/// draining, retired, failed, and unregistered disks are excluded.
pub fn read_settlement_eligible_disk_ids(
    live_sqlite_path: impl AsRef<Path>,
) -> Result<BTreeSet<DiskId>, DiskCapacityClaimError> {
    let connection = open(live_sqlite_path.as_ref())?;
    let mut statement = connection
        .prepare("SELECT disk_id FROM disks WHERE state IN ('Healthy','Watch') ORDER BY disk_id")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    let mut disk_ids = BTreeSet::new();
    for row in rows {
        let stored = row?;
        let disk_id =
            DiskId::new(&stored).map_err(|_| DiskCapacityClaimError::InvalidStoredDisk(stored))?;
        disk_ids.insert(disk_id);
    }
    Ok(disk_ids)
}

/// Acquire claims inside an existing immediate transaction. This is used when
/// SSD acknowledgement metadata and its mandatory HDD capacity reservation
/// must become visible atomically.
pub(crate) fn acquire_disk_capacity_claims_in_transaction(
    transaction: &Transaction<'_>,
    request: &DiskCapacityClaimRequest,
) -> Result<Vec<DiskCapacityClaim>, DiskCapacityClaimError> {
    validate_request(request)?;
    let existing = read_owner_claims(transaction, request.kind, &request.owner_id)?;
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
        // Recheck the placement-eligible state inside the transaction to close
        // the race between daemon selection and capacity-claim publication.
        let eligible = matches!(state.as_deref(), Some("Healthy" | "Watch"));
        if !eligible {
            return Err(DiskCapacityClaimError::IneligibleDisk {
                disk_id: allocation.disk_id.clone(),
                state,
            });
        }
        let outstanding = outstanding_claim_bytes(transaction, &allocation.disk_id)?;
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
    let claims = read_owner_claims(transaction, request.kind, &request.owner_id)?;
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

pub fn reconcile_abandoned_ingest_disk_capacity_claims_at_startup(
    live_sqlite_path: impl AsRef<Path>,
    startup_at_utc: &str,
) -> Result<AbandonedIngestCapacityClaimRecoveryReport, DiskCapacityClaimError> {
    let startup_seconds =
        parse_canonical_utc_timestamp_seconds(startup_at_utc).ok_or_else(|| {
            DiskCapacityClaimError::InvalidRequest {
                field: "startup_at_utc",
                reason: "must be a canonical UTC timestamp".to_string(),
            }
        })?;
    let mut connection = open(live_sqlite_path.as_ref())?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let rows = {
        let mut statement = transaction.prepare(
            "SELECT claim_id, owner_id, request_id, request_digest, disk_id,
                    reserved_bytes, consumed_bytes, lease_owner,
                    lease_expires_at_utc, created_at_utc, updated_at_utc
             FROM disk_capacity_claims
             WHERE claim_kind='ingest' AND state='active'
               AND released_at_utc IS NULL
             ORDER BY owner_id, disk_id",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok(AbandonedIngestClaimRow {
                    claim_id: row.get(0)?,
                    owner_id: row.get(1)?,
                    request_id: row.get(2)?,
                    request_digest: row.get(3)?,
                    disk_id: row.get(4)?,
                    reserved_bytes: row.get(5)?,
                    consumed_bytes: row.get(6)?,
                    lease_owner: row.get(7)?,
                    lease_expires_at_utc: row.get(8)?,
                    created_at_utc: row.get(9)?,
                    updated_at_utc: row.get(10)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let mut by_owner = BTreeMap::<String, Vec<AbandonedIngestClaimRow>>::new();
    for row in rows {
        by_owner.entry(row.owner_id.clone()).or_default().push(row);
    }

    let mut report = AbandonedIngestCapacityClaimRecoveryReport {
        owners_scanned: by_owner.len() as u64,
        ..AbandonedIngestCapacityClaimRecoveryReport::default()
    };
    for (owner_id, claims) in by_owner {
        if claims
            .iter()
            .any(|claim| claim.lease_expires_at_utc.is_some())
        {
            report.leased_owners_retained += 1;
            continue;
        }
        let updated_seconds = claims
            .iter()
            .map(|claim| parse_canonical_utc_timestamp_seconds(&claim.updated_at_utc))
            .collect::<Option<Vec<_>>>();
        let created_seconds = claims
            .iter()
            .map(|claim| parse_canonical_utc_timestamp_seconds(&claim.created_at_utc))
            .collect::<Option<Vec<_>>>();
        let Some(updated_seconds) = updated_seconds else {
            report.unrecognized_owners_retained += 1;
            continue;
        };
        let Some(created_seconds) = created_seconds else {
            report.unrecognized_owners_retained += 1;
            continue;
        };
        if updated_seconds
            .iter()
            .any(|updated| *updated >= startup_seconds)
        {
            report.current_owners_retained += 1;
            continue;
        }
        if created_seconds
            .iter()
            .zip(updated_seconds.iter())
            .any(|(created, updated)| created > updated)
            || !recognized_direct_ingest_claim_group(&owner_id, &claims)
        {
            report.unrecognized_owners_retained += 1;
            continue;
        }

        let changed = transaction.execute(
            "UPDATE disk_capacity_claims
             SET state='released', released_at_utc=?1, updated_at_utc=?1,
                 lease_owner=NULL, lease_expires_at_utc=NULL
             WHERE claim_kind='ingest' AND owner_id=?2 AND state='active'
               AND released_at_utc IS NULL",
            params![startup_at_utc, owner_id],
        )?;
        if changed != claims.len() {
            return Err(DiskCapacityClaimError::RequestConflict {
                kind: DiskCapacityClaimKind::Ingest,
                owner_id,
            });
        }
        report.owners_released += 1;
        report.claims_released = report.claims_released.saturating_add(changed as u64);
        report.reclaimed_bytes = report.reclaimed_bytes.saturating_add(
            claims
                .iter()
                .map(|claim| claim.reserved_bytes.saturating_sub(claim.consumed_bytes))
                .sum::<u64>(),
        );
    }
    transaction.commit()?;
    Ok(report)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AbandonedIngestClaimRow {
    claim_id: String,
    owner_id: String,
    request_id: String,
    request_digest: String,
    disk_id: String,
    reserved_bytes: u64,
    consumed_bytes: u64,
    lease_owner: Option<String>,
    lease_expires_at_utc: Option<String>,
    created_at_utc: String,
    updated_at_utc: String,
}

fn recognized_direct_ingest_claim_group(
    owner_id: &str,
    claims: &[AbandonedIngestClaimRow],
) -> bool {
    let Some(first) = claims.first() else {
        return false;
    };
    let Some(lease_owner) = first.lease_owner.as_deref() else {
        return false;
    };
    if IngestJobId::new(lease_owner).is_err() {
        return false;
    }
    let owner_prefix = format!("{lease_owner}:");
    let Some(object_id) = owner_id.strip_prefix(&owner_prefix) else {
        return false;
    };
    if ObjectId::new(object_id).is_err() || first.request_id != format!("ingest:{owner_id}") {
        return false;
    }
    let digest_prefix = format!("{object_id}:");
    let Some(digest_suffix) = first.request_digest.strip_prefix(&digest_prefix) else {
        return false;
    };
    let Some((size, copies)) = digest_suffix.split_once(':') else {
        return false;
    };
    let (Ok(size), Ok(copies)) = (size.parse::<u64>(), copies.parse::<usize>()) else {
        return false;
    };
    if copies == 0 || copies != claims.len() {
        return false;
    }
    let claimed_size = size.max(1);
    claims.iter().all(|claim| {
        claim.owner_id == owner_id
            && claim.lease_owner.as_deref() == Some(lease_owner)
            && claim.request_id == first.request_id
            && claim.request_digest == first.request_digest
            && claim.created_at_utc == first.created_at_utc
            && claim.reserved_bytes == claimed_size
            && claim.claim_id == format!("ingest:{owner_id}:{}", claim.disk_id)
    })
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
        acquire_disk_capacity_claims, read_outstanding_disk_capacity,
        read_settlement_eligible_disk_ids,
        reconcile_abandoned_ingest_disk_capacity_claims_at_startup, release_disk_capacity_claims,
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
    fn startup_releases_only_recognized_abandoned_direct_ingest_claims() {
        let database = fixture("startup-recovery");
        acquire_disk_capacity_claims(&direct_ingest_request(
            &database,
            "ingest-files-2026-07-25t00-00-00z",
            "store-a/object.bin",
            60,
            None,
            "2026-07-25T00:00:00Z",
        ))
        .expect("abandoned direct-ingest claim");

        let report = reconcile_abandoned_ingest_disk_capacity_claims_at_startup(
            &database,
            "2026-07-25T01:00:00Z",
        )
        .expect("startup recovery");
        assert_eq!(report.owners_scanned, 1);
        assert_eq!(report.owners_released, 1);
        assert_eq!(report.claims_released, 1);
        assert_eq!(report.reclaimed_bytes, 60);
        assert!(read_outstanding_disk_capacity(&database)
            .expect("outstanding claims")
            .is_empty());

        let replay = reconcile_abandoned_ingest_disk_capacity_claims_at_startup(
            &database,
            "2026-07-25T01:00:01Z",
        )
        .expect("idempotent startup recovery");
        assert_eq!(replay.owners_scanned, 0);
        cleanup(&database);
    }

    #[test]
    fn startup_recovery_retains_current_leased_and_unrecognized_claims() {
        let database = fixture("startup-recovery-retained");
        acquire_disk_capacity_claims(&direct_ingest_request(
            &database,
            "ingest-files-current",
            "store-a/current.bin",
            10,
            None,
            "2026-07-25T01:00:00Z",
        ))
        .expect("current claim");
        acquire_disk_capacity_claims(&direct_ingest_request(
            &database,
            "ingest-files-leased",
            "store-a/leased.bin",
            20,
            Some("2026-07-25T02:00:00Z"),
            "2026-07-25T00:00:00Z",
        ))
        .expect("leased claim");
        let mut unfamiliar = direct_ingest_request(
            &database,
            "ingest-files-unfamiliar",
            "store-a/unfamiliar.bin",
            30,
            None,
            "2026-07-25T00:00:00Z",
        );
        unfamiliar.request_id = "external-contract".to_string();
        acquire_disk_capacity_claims(&unfamiliar).expect("unrecognized claim");
        acquire_disk_capacity_claims(&request(
            &database,
            DiskCapacityClaimKind::Workspace,
            "workspace-a",
            5,
        ))
        .expect("non-ingest claim");

        let report = reconcile_abandoned_ingest_disk_capacity_claims_at_startup(
            &database,
            "2026-07-25T01:00:00Z",
        )
        .expect("startup recovery");
        assert_eq!(report.owners_scanned, 3);
        assert_eq!(report.owners_released, 0);
        assert_eq!(report.current_owners_retained, 1);
        assert_eq!(report.leased_owners_retained, 1);
        assert_eq!(report.unrecognized_owners_retained, 1);
        assert_eq!(
            read_outstanding_disk_capacity(&database)
                .expect("all retained")
                .values()
                .sum::<u64>(),
            65
        );
        cleanup(&database);
    }

    #[test]
    fn startup_recovery_recognizes_zero_byte_accounting_floor() {
        let database = fixture("startup-recovery-empty-object");
        acquire_disk_capacity_claims(&direct_ingest_request(
            &database,
            "ingest-files-empty",
            "store-a/empty.bin",
            0,
            None,
            "2026-07-25T00:00:00Z",
        ))
        .expect("empty-object claim");

        let report = reconcile_abandoned_ingest_disk_capacity_claims_at_startup(
            &database,
            "2026-07-25T01:00:00Z",
        )
        .expect("startup recovery");
        assert_eq!(report.claims_released, 1);
        assert_eq!(report.reclaimed_bytes, 1);
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

    #[test]
    fn settlement_eligible_registry_view_contains_healthy_and_watch_disks() {
        let database = fixture("healthy-registry-view");
        let connection = Connection::open(&database).expect("open fixture");
        connection
            .execute("UPDATE disks SET state='Watch' WHERE disk_id='disk-a'", [])
            .expect("degrade disk");
        connection
            .execute(
                "INSERT INTO disks (
                    disk_id, pool_id, role, state, created_at_utc, updated_at_utc
                 ) VALUES (
                    'disk-b', 'pool-a', 'hdd_capacity', 'Healthy',
                    '2026-07-25T00:00:00Z', '2026-07-25T00:00:00Z'
                 )",
                [],
            )
            .expect("healthy disk");
        drop(connection);

        let disk_ids =
            read_settlement_eligible_disk_ids(&database).expect("placement-eligible disk ids");
        assert_eq!(
            disk_ids.iter().map(DiskId::as_str).collect::<Vec<_>>(),
            vec!["disk-a", "disk-b"]
        );
        cleanup(&database);
    }

    #[test]
    fn destage_claim_accepts_watch_disk_under_placement_contract() {
        let database = fixture("destage-watch-race");
        let connection = Connection::open(&database).expect("open fixture");
        connection
            .execute("UPDATE disks SET state='Watch' WHERE disk_id='disk-a'", [])
            .expect("degrade disk");
        drop(connection);

        let claims = acquire_disk_capacity_claims(&request(
            &database,
            DiskCapacityClaimKind::Destage,
            "object-a",
            1,
        ))
        .expect("watch disk remains placement-eligible");
        assert_eq!(claims.len(), 1);
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

    fn direct_ingest_request(
        database: &Path,
        job_id: &str,
        object_id: &str,
        bytes: u64,
        lease_expires_at_utc: Option<&str>,
        recorded_at_utc: &str,
    ) -> DiskCapacityClaimRequest {
        let owner_id = format!("{job_id}:{object_id}");
        DiskCapacityClaimRequest {
            live_sqlite_path: database.to_path_buf(),
            kind: DiskCapacityClaimKind::Ingest,
            owner_id: owner_id.clone(),
            request_id: format!("ingest:{owner_id}"),
            request_digest: format!("{object_id}:{bytes}:1"),
            lease_owner: Some(job_id.to_string()),
            lease_expires_at_utc: lease_expires_at_utc.map(str::to_string),
            created_at_utc: recorded_at_utc.to_string(),
            allocations: vec![DiskCapacityClaimAllocation {
                disk_id: DiskId::new("disk-a").expect("disk id"),
                measured_available_bytes: 100,
                requested_bytes: bytes.max(1),
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
