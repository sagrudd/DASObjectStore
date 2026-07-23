//! Durable managed-SSD acknowledgement and asynchronous HDD settlement.

use crate::schema::LIVE_SCHEMA_SQL;
use dasobjectstore_core::ids::{ObjectId, StoreId};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};
use std::path::Path;
use std::time::Duration;

const PUBLICATION_BUSY_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DestageState {
    QueuedForHdd,
    HddCopying,
    HddCopyVerified,
    DestageFailed,
    NeedsReview,
    Paused,
    Cancelled,
}

impl DestageState {
    fn parse(value: &str) -> Result<Self, DestageMetadataError> {
        match value {
            "queued_for_hdd" => Ok(Self::QueuedForHdd),
            "hdd_copying" => Ok(Self::HddCopying),
            "hdd_copy_verified" => Ok(Self::HddCopyVerified),
            "destage_failed" => Ok(Self::DestageFailed),
            "needs_review" => Ok(Self::NeedsReview),
            "paused" => Ok(Self::Paused),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(DestageMetadataError::InvalidState(value.to_string())),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedSsdCommitRequest<'a> {
    pub destage_job_id: &'a str,
    pub store_id: &'a StoreId,
    pub object_id: &'a ObjectId,
    pub object_type: &'a str,
    pub relative_path: &'a str,
    pub size_bytes: u64,
    pub content_hash_algorithm: &'a str,
    pub content_hash: &'a str,
    pub acknowledgement_policy: &'a str,
    pub required_copy_count: u8,
    pub max_attempts: u32,
    pub priority: i32,
    pub committed_at_utc: &'a str,
    /// Optional durable ingress job identity. Direct S3 uses this to make the
    /// `remote_s3` origin and SSD-accepted state part of the same SQLite
    /// transaction as catalogue visibility and the destage queue.
    pub ingest_job_id: Option<&'a str>,
    pub ingress_origin: Option<&'a str>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct VerifiedSsdCommitReport {
    pub destage_job_id: String,
    pub state: DestageState,
    pub idempotent: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DestageQueueRecord {
    pub destage_job_id: String,
    pub store_id: StoreId,
    pub object_id: ObjectId,
    pub object_type: String,
    pub state: DestageState,
    pub expected_size_bytes: u64,
    pub content_hash_algorithm: String,
    pub content_hash: String,
    pub acknowledgement_policy: String,
    pub required_copy_count: u8,
    pub verified_copy_count: u8,
    pub priority: i32,
    pub attempt_count: u32,
    pub max_attempts: u32,
    pub last_error: Option<String>,
    pub next_retry_at_utc: Option<String>,
    pub lease_owner: Option<String>,
    pub created_at_utc: String,
    pub updated_at_utc: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct DestageQueueDiagnostics {
    pub pending_object_count: u64,
    pub failed_object_count: u64,
    pub queued_bytes: u64,
    pub active_bytes: u64,
    pub oldest_queued_at_utc: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedHddPlacement<'a> {
    pub placement_id: &'a str,
    pub disk_id: &'a str,
    pub relative_path: &'a str,
    pub content_hash: &'a str,
}

pub struct HddSettlementPromotionRequest<'a> {
    pub object_id: &'a ObjectId,
    pub worker: &'a str,
    pub placements: &'a [VerifiedHddPlacement<'a>],
    pub verified_at_utc: &'a str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SsdPlacementRecord {
    pub object_id: ObjectId,
    pub store_id: StoreId,
    pub relative_path: String,
    pub size_bytes: u64,
    pub content_hash_algorithm: String,
    pub content_hash: String,
    pub verified_at_utc: String,
    pub eviction_eligible: bool,
    pub evicted_at_utc: Option<String>,
}

pub fn commit_verified_ssd_and_enqueue(
    path: impl AsRef<Path>,
    request: VerifiedSsdCommitRequest<'_>,
) -> Result<VerifiedSsdCommitReport, DestageMetadataError> {
    validate_ssd_request(&request)?;
    let size = to_i64(request.size_bytes)?;
    let mut connection = Connection::open(path)?;
    connection.busy_timeout(PUBLICATION_BUSY_TIMEOUT)?;
    connection.execute_batch(LIVE_SCHEMA_SQL)?;
    let tx = connection.transaction()?;
    ensure_store(&tx, request.store_id)?;

    if let Some(existing) = read_identity(&tx, request.object_id)? {
        let matches = existing
            == (
                request.store_id.as_str().to_string(),
                size,
                request.content_hash_algorithm.to_string(),
                request.content_hash.to_string(),
                request.acknowledgement_policy.to_string(),
                request.required_copy_count,
            );
        if !matches {
            return Err(DestageMetadataError::ObjectConflict(
                request.object_id.to_string(),
            ));
        }
        insert_ingress_job(&tx, &request, size)?;
        let job_id: String = tx.query_row(
            "SELECT destage_job_id FROM destage_queue WHERE object_id = ?1",
            [request.object_id.as_str()],
            |row| row.get(0),
        )?;
        tx.commit()?;
        return Ok(VerifiedSsdCommitReport {
            destage_job_id: job_id,
            state: DestageState::QueuedForHdd,
            idempotent: true,
        });
    }

    tx.execute("INSERT INTO objects (object_id, store_id, object_type, state, size_bytes, content_hash, created_at_utc, updated_at_utc) VALUES (?1,?2,?3,'PlacementPlanned',?4,?5,?6,?6)", params![request.object_id.as_str(), request.store_id.as_str(), request.object_type, size, request.content_hash, request.committed_at_utc])?;
    insert_ingress_job(&tx, &request, size)?;
    tx.execute("INSERT INTO ssd_object_placements (object_id, store_id, relative_path, size_bytes, content_hash_algorithm, content_hash, verified_at_utc, created_at_utc, updated_at_utc) VALUES (?1,?2,?3,?4,?5,?6,?7,?7,?7)", params![request.object_id.as_str(), request.store_id.as_str(), request.relative_path, size, request.content_hash_algorithm, request.content_hash, request.committed_at_utc])?;
    tx.execute("INSERT INTO destage_queue (destage_job_id, store_id, object_id, state, expected_size_bytes, content_hash_algorithm, content_hash, acknowledgement_policy, required_copy_count, priority, max_attempts, created_at_utc, updated_at_utc) VALUES (?1,?2,?3,'queued_for_hdd',?4,?5,?6,?7,?8,?9,?10,?11,?11)", params![request.destage_job_id, request.store_id.as_str(), request.object_id.as_str(), size, request.content_hash_algorithm, request.content_hash, request.acknowledgement_policy, request.required_copy_count, request.priority, request.max_attempts, request.committed_at_utc])?;
    tx.commit()?;
    Ok(VerifiedSsdCommitReport {
        destage_job_id: request.destage_job_id.to_string(),
        state: DestageState::QueuedForHdd,
        idempotent: false,
    })
}

fn insert_ingress_job(
    tx: &Transaction<'_>,
    request: &VerifiedSsdCommitRequest<'_>,
    size: i64,
) -> Result<(), DestageMetadataError> {
    let (Some(job_id), Some(origin)) = (request.ingest_job_id, request.ingress_origin) else {
        return Ok(());
    };
    tx.execute(
        "INSERT OR IGNORE INTO ingest_jobs (ingest_job_id, store_id, object_id, object_type, state, ingest_mode, acknowledgement_policy, priority, staging_path, expected_size_bytes, received_bytes, content_hash, content_hash_algorithm, created_at_utc, updated_at_utc) VALUES (?1,?2,?3,?4,'ssd_accepted',?5,?6,?7,?8,?9,?9,?10,?11,?12,?12)",
        params![job_id, request.store_id.as_str(), request.object_id.as_str(), request.object_type, origin, request.acknowledgement_policy, request.priority, request.relative_path, size, request.content_hash, request.content_hash_algorithm, request.committed_at_utc],
    )?;
    let identity: (String, String, String, String, String, String, i64, String, String) = tx
        .query_row(
            "SELECT store_id, object_id, object_type, ingest_mode, acknowledgement_policy, staging_path, expected_size_bytes, content_hash, content_hash_algorithm FROM ingest_jobs WHERE ingest_job_id=?1",
            [job_id],
            |row| {
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
                ))
            },
        )?;
    let expected = (
        request.store_id.as_str(),
        request.object_id.as_str(),
        request.object_type,
        origin,
        request.acknowledgement_policy,
        request.relative_path,
        size,
        request.content_hash,
        request.content_hash_algorithm,
    );
    if (
        identity.0.as_str(),
        identity.1.as_str(),
        identity.2.as_str(),
        identity.3.as_str(),
        identity.4.as_str(),
        identity.5.as_str(),
        identity.6,
        identity.7.as_str(),
        identity.8.as_str(),
    ) != expected
    {
        return Err(DestageMetadataError::IngestJobConflict(job_id.to_string()));
    }
    Ok(())
}

/// Claims one runnable row. Supplying the previously served store implements
/// round-robin fairness without weakening priority ordering within a store.
pub fn claim_next_destage(
    path: impl AsRef<Path>,
    worker: &str,
    lease_expires_at_utc: &str,
    now_utc: &str,
    previously_served_store: Option<&StoreId>,
) -> Result<Option<DestageQueueRecord>, DestageMetadataError> {
    if worker.trim().is_empty() {
        return Err(DestageMetadataError::BlankField("worker"));
    }
    let mut connection = Connection::open(path)?;
    connection.execute_batch(LIVE_SCHEMA_SQL)?;
    let tx = connection.transaction()?;
    tx.execute("UPDATE destage_queue SET state='needs_review', lease_owner=NULL, lease_expires_at_utc=NULL, updated_at_utc=?1 WHERE attempt_count>=max_attempts AND state IN ('destage_failed','hdd_copying') AND (lease_owner IS NULL OR lease_expires_at_utc<=?1)",[now_utc])?;
    let excluded = previously_served_store.map(StoreId::as_str).unwrap_or("");
    let object_id: Option<String> = tx.query_row(
        "SELECT object_id FROM destage_queue WHERE state IN ('queued_for_hdd','destage_failed','hdd_copying') AND attempt_count < max_attempts AND cancellation_requested=0 AND (next_retry_at_utc IS NULL OR next_retry_at_utc<=?1) AND (lease_owner IS NULL OR lease_expires_at_utc<=?1) ORDER BY CASE WHEN store_id=?2 THEN 1 ELSE 0 END, priority DESC, created_at_utc, destage_job_id LIMIT 1",
        params![now_utc, excluded], |row| row.get(0)).optional()?;
    let Some(object_id) = object_id else {
        tx.commit()?;
        return Ok(None);
    };
    let changed = tx.execute("UPDATE destage_queue SET state='hdd_copying', lease_owner=?1, lease_expires_at_utc=?2, attempt_count=attempt_count+1, updated_at_utc=?3 WHERE object_id=?4 AND attempt_count < max_attempts AND (lease_owner IS NULL OR lease_expires_at_utc<=?3)", params![worker, lease_expires_at_utc, now_utc, object_id])?;
    if changed != 1 {
        return Err(DestageMetadataError::ClaimConflict);
    }
    tx.execute(
        "UPDATE objects SET state='CopyingToHdd', updated_at_utc=?1 WHERE object_id=?2",
        params![now_utc, object_id],
    )?;
    let record = read_record_tx(&tx, &object_id)?;
    tx.commit()?;
    Ok(Some(record))
}

pub fn fail_destage(
    path: impl AsRef<Path>,
    object_id: &ObjectId,
    worker: &str,
    error: &str,
    next_retry_at_utc: Option<&str>,
    updated_at_utc: &str,
) -> Result<(), DestageMetadataError> {
    if error.trim().is_empty() {
        return Err(DestageMetadataError::BlankField("error"));
    }
    let connection = Connection::open(path)?;
    connection.execute_batch(LIVE_SCHEMA_SQL)?;
    let changed = connection.execute("UPDATE destage_queue SET state='destage_failed', last_error=?1, next_retry_at_utc=?2, lease_owner=NULL, lease_expires_at_utc=NULL, updated_at_utc=?3 WHERE object_id=?4 AND state='hdd_copying' AND lease_owner=?5", params![error, next_retry_at_utc, updated_at_utc, object_id.as_str(), worker])?;
    if changed != 1 {
        return Err(DestageMetadataError::ClaimConflict);
    }
    Ok(())
}

pub fn pause_destage(
    path: impl AsRef<Path>,
    object_id: &ObjectId,
    updated_at_utc: &str,
) -> Result<(), DestageMetadataError> {
    control(
        path,
        object_id,
        "paused",
        updated_at_utc,
        "state IN ('queued_for_hdd','destage_failed')",
    )
}
pub fn resume_destage(
    path: impl AsRef<Path>,
    object_id: &ObjectId,
    updated_at_utc: &str,
) -> Result<(), DestageMetadataError> {
    control(
        path,
        object_id,
        "queued_for_hdd",
        updated_at_utc,
        "state='paused'",
    )
}
pub fn retry_destage(
    path: impl AsRef<Path>,
    object_id: &ObjectId,
    updated_at_utc: &str,
) -> Result<(), DestageMetadataError> {
    control(
        path,
        object_id,
        "queued_for_hdd",
        updated_at_utc,
        "state IN ('destage_failed','needs_review')",
    )
}

pub fn cancel_destage(
    path: impl AsRef<Path>,
    object_id: &ObjectId,
    updated_at_utc: &str,
) -> Result<(), DestageMetadataError> {
    let mut connection = Connection::open(path)?;
    connection.execute_batch(LIVE_SCHEMA_SQL)?;
    let tx = connection.transaction()?;
    let ssd_verified: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM ssd_object_placements WHERE object_id=?1 AND evicted_at_utc IS NULL)",[object_id.as_str()],|r|r.get(0))?;
    let hdd_verified: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM placements WHERE object_id=?1 AND verified_at_utc IS NOT NULL)",[object_id.as_str()],|r|r.get(0))?;
    if !ssd_verified && !hdd_verified {
        return Err(DestageMetadataError::WouldRemoveOnlyVerifiedCopy);
    }
    let changed=tx.execute("UPDATE destage_queue SET state='cancelled', cancellation_requested=1, lease_owner=NULL, lease_expires_at_utc=NULL, updated_at_utc=?1 WHERE object_id=?2 AND state NOT IN ('hdd_copy_verified','cancelled')",params![updated_at_utc,object_id.as_str()])?;
    if changed != 1 {
        return Err(DestageMetadataError::InvalidTransition);
    }
    tx.commit()?;
    Ok(())
}

pub fn promote_hdd_settlement(
    path: impl AsRef<Path>,
    request: HddSettlementPromotionRequest<'_>,
) -> Result<bool, DestageMetadataError> {
    let mut connection = Connection::open(path)?;
    connection.execute_batch(LIVE_SCHEMA_SQL)?;
    let tx = connection.transaction()?;
    let required: u8 = tx.query_row(
        "SELECT required_copy_count FROM destage_queue WHERE object_id=?1",
        [request.object_id.as_str()],
        |r| r.get(0),
    )?;
    let verified_copy_count = u8::try_from(request.placements.len()).unwrap_or(u8::MAX);
    if verified_copy_count < required {
        return Err(DestageMetadataError::InsufficientCopies {
            required,
            verified: verified_copy_count,
        });
    }
    let existing: String = tx.query_row(
        "SELECT state FROM destage_queue WHERE object_id=?1",
        [request.object_id.as_str()],
        |r| r.get(0),
    )?;
    if existing == "hdd_copy_verified" {
        tx.commit()?;
        return Ok(true);
    }
    let expected_hash: String = tx.query_row(
        "SELECT content_hash FROM destage_queue WHERE object_id=?1",
        [request.object_id.as_str()],
        |row| row.get(0),
    )?;
    for placement in request.placements {
        if placement.content_hash != expected_hash {
            return Err(DestageMetadataError::PlacementHashMismatch);
        }
        let disk_exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM disks WHERE disk_id=?1)",
            [placement.disk_id],
            |row| row.get(0),
        )?;
        if !disk_exists {
            return Err(DestageMetadataError::MissingDisk(
                placement.disk_id.to_string(),
            ));
        }
        tx.execute("INSERT INTO placements(placement_id,object_id,disk_id,relative_path,content_hash,verified_at_utc,created_at_utc) VALUES(?1,?2,?3,?4,?5,?6,?6) ON CONFLICT(placement_id) DO UPDATE SET content_hash=excluded.content_hash,verified_at_utc=excluded.verified_at_utc", params![placement.placement_id,request.object_id.as_str(),placement.disk_id,placement.relative_path,placement.content_hash,request.verified_at_utc])?;
    }
    let changed=tx.execute("UPDATE destage_queue SET state='hdd_copy_verified', verified_copy_count=?1, last_error=NULL, next_retry_at_utc=NULL, lease_owner=NULL, lease_expires_at_utc=NULL, updated_at_utc=?2 WHERE object_id=?3 AND state='hdd_copying' AND lease_owner=?4",params![verified_copy_count,request.verified_at_utc,request.object_id.as_str(),request.worker])?;
    if changed != 1 {
        return Err(DestageMetadataError::ClaimConflict);
    }
    tx.execute(
        "UPDATE objects SET state='HddCopyVerified', updated_at_utc=?1 WHERE object_id=?2",
        params![request.verified_at_utc, request.object_id.as_str()],
    )?;
    tx.execute("UPDATE ssd_object_placements SET eviction_eligible=1, updated_at_utc=?1 WHERE object_id=?2 AND evicted_at_utc IS NULL",params![request.verified_at_utc,request.object_id.as_str()])?;
    tx.commit()?;
    Ok(false)
}

pub fn read_destage(
    path: impl AsRef<Path>,
    object_id: &ObjectId,
) -> Result<Option<DestageQueueRecord>, DestageMetadataError> {
    let mut connection = Connection::open(path)?;
    connection.execute_batch(LIVE_SCHEMA_SQL)?;
    let tx = connection.transaction()?;
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM destage_queue WHERE object_id=?1)",
        [object_id.as_str()],
        |row| row.get(0),
    )?;
    let result = exists
        .then(|| read_record_tx(&tx, object_id.as_str()))
        .transpose()?;
    tx.commit()?;
    Ok(result)
}

pub fn list_destage_queue(
    path: impl AsRef<Path>,
    store_id: Option<&StoreId>,
) -> Result<Vec<DestageQueueRecord>, DestageMetadataError> {
    let mut connection = Connection::open(path)?;
    connection.execute_batch(LIVE_SCHEMA_SQL)?;
    let tx = connection.transaction()?;
    let ids = {
        let mut statement = tx.prepare("SELECT object_id FROM destage_queue WHERE (?1='' OR store_id=?1) ORDER BY priority DESC,created_at_utc,destage_job_id")?;
        let values = statement
            .query_map([store_id.map(StoreId::as_str).unwrap_or("")], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        values
    };
    let records = ids
        .iter()
        .map(|id| read_record_tx(&tx, id))
        .collect::<Result<Vec<_>, _>>()?;
    tx.commit()?;
    Ok(records)
}

pub fn read_ssd_placement(
    path: impl AsRef<Path>,
    object_id: &ObjectId,
) -> Result<Option<SsdPlacementRecord>, DestageMetadataError> {
    let connection = Connection::open(path)?;
    connection.execute_batch(LIVE_SCHEMA_SQL)?;
    let raw = connection.query_row("SELECT object_id,store_id,relative_path,size_bytes,content_hash_algorithm,content_hash,verified_at_utc,eviction_eligible,evicted_at_utc FROM ssd_object_placements WHERE object_id=?1", [object_id.as_str()], |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,row.get::<_,i64>(3)?,row.get::<_,String>(4)?,row.get::<_,String>(5)?,row.get::<_,String>(6)?,row.get::<_,bool>(7)?,row.get::<_,Option<String>>(8)?))).optional()?;
    raw.map(|value| {
        Ok(SsdPlacementRecord {
            object_id: ObjectId::new(value.0)
                .map_err(|_| DestageMetadataError::InvalidIdentifier)?,
            store_id: StoreId::new(value.1).map_err(|_| DestageMetadataError::InvalidIdentifier)?,
            relative_path: value.2,
            size_bytes: u64::try_from(value.3).map_err(|_| DestageMetadataError::InvalidSize)?,
            content_hash_algorithm: value.4,
            content_hash: value.5,
            verified_at_utc: value.6,
            eviction_eligible: value.7,
            evicted_at_utc: value.8,
        })
    })
    .transpose()
}

pub fn list_ssd_eviction_candidates(
    path: impl AsRef<Path>,
    limit: usize,
) -> Result<Vec<SsdPlacementRecord>, DestageMetadataError> {
    let connection = Connection::open(path)?;
    connection.execute_batch(LIVE_SCHEMA_SQL)?;
    let mut statement = connection.prepare(
        "SELECT object_id,store_id,relative_path,size_bytes,content_hash_algorithm,content_hash,verified_at_utc,eviction_eligible,evicted_at_utc FROM ssd_object_placements WHERE eviction_eligible=1 AND evicted_at_utc IS NULL ORDER BY updated_at_utc,object_id LIMIT ?1",
    )?;
    let rows = statement.query_map([i64::try_from(limit).unwrap_or(i64::MAX)], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, bool>(7)?,
            row.get::<_, Option<String>>(8)?,
        ))
    })?;
    rows.map(|row| {
        let value = row?;
        Ok(SsdPlacementRecord {
            object_id: ObjectId::new(value.0)
                .map_err(|_| DestageMetadataError::InvalidIdentifier)?,
            store_id: StoreId::new(value.1).map_err(|_| DestageMetadataError::InvalidIdentifier)?,
            relative_path: value.2,
            size_bytes: u64::try_from(value.3).map_err(|_| DestageMetadataError::InvalidSize)?,
            content_hash_algorithm: value.4,
            content_hash: value.5,
            verified_at_utc: value.6,
            eviction_eligible: value.7,
            evicted_at_utc: value.8,
        })
    })
    .collect()
}

/// Call only after the payload has been durably removed by the storage owner.
pub fn mark_ssd_evicted(
    path: impl AsRef<Path>,
    object_id: &ObjectId,
    evicted_at_utc: &str,
) -> Result<(), DestageMetadataError> {
    let connection = Connection::open(path)?;
    connection.execute_batch(LIVE_SCHEMA_SQL)?;
    let changed = connection.execute("UPDATE ssd_object_placements SET evicted_at_utc=?1,updated_at_utc=?1 WHERE object_id=?2 AND eviction_eligible=1 AND evicted_at_utc IS NULL", params![evicted_at_utc, object_id.as_str()])?;
    if changed != 1 {
        return Err(DestageMetadataError::SsdNotEvictionEligible);
    }
    Ok(())
}

pub fn destage_queue_diagnostics(
    path: impl AsRef<Path>,
) -> Result<DestageQueueDiagnostics, DestageMetadataError> {
    let connection = Connection::open(path)?;
    connection.execute_batch(LIVE_SCHEMA_SQL)?;
    connection.query_row("SELECT COUNT(*) FILTER (WHERE state IN ('queued_for_hdd','hdd_copying','destage_failed','paused')), COUNT(*) FILTER (WHERE state IN ('destage_failed','needs_review')), COALESCE(SUM(CASE WHEN state IN ('queued_for_hdd','destage_failed','paused') THEN expected_size_bytes ELSE 0 END),0), COALESCE(SUM(CASE WHEN state='hdd_copying' THEN expected_size_bytes ELSE 0 END),0), MIN(CASE WHEN state IN ('queued_for_hdd','destage_failed','paused') THEN created_at_utc END) FROM destage_queue",[],|r|Ok(DestageQueueDiagnostics{pending_object_count:r.get(0)?,failed_object_count:r.get(1)?,queued_bytes:r.get(2)?,active_bytes:r.get(3)?,oldest_queued_at_utc:r.get(4)?})).map_err(Into::into)
}

fn control(
    path: impl AsRef<Path>,
    object_id: &ObjectId,
    state: &str,
    at: &str,
    predicate: &str,
) -> Result<(), DestageMetadataError> {
    let connection = Connection::open(path)?;
    connection.execute_batch(LIVE_SCHEMA_SQL)?;
    let sql=format!("UPDATE destage_queue SET state=?1, next_retry_at_utc=NULL, lease_owner=NULL, lease_expires_at_utc=NULL, updated_at_utc=?2 WHERE object_id=?3 AND {predicate}");
    if connection.execute(&sql, params![state, at, object_id.as_str()])? != 1 {
        return Err(DestageMetadataError::InvalidTransition);
    }
    Ok(())
}
fn validate_ssd_request(r: &VerifiedSsdCommitRequest<'_>) -> Result<(), DestageMetadataError> {
    for (n, v) in [
        ("destage_job_id", r.destage_job_id),
        ("object_type", r.object_type),
        ("relative_path", r.relative_path),
        ("content_hash_algorithm", r.content_hash_algorithm),
        ("content_hash", r.content_hash),
        ("acknowledgement_policy", r.acknowledgement_policy),
        ("committed_at_utc", r.committed_at_utc),
    ] {
        if v.trim().is_empty() {
            return Err(DestageMetadataError::BlankField(n));
        }
    }
    if r.required_copy_count == 0 {
        return Err(DestageMetadataError::ZeroCopies);
    }
    if r.max_attempts == 0 {
        return Err(DestageMetadataError::ZeroAttempts);
    }
    match (r.ingest_job_id, r.ingress_origin) {
        (None, None) => {}
        (Some(job), Some(origin)) if !job.trim().is_empty() && !origin.trim().is_empty() => {}
        _ => return Err(DestageMetadataError::InvalidIdentifier),
    }
    Ok(())
}
fn ensure_store(tx: &Transaction<'_>, id: &StoreId) -> Result<(), DestageMetadataError> {
    if !tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM stores WHERE store_id=?1)",
        [id.as_str()],
        |r| r.get::<_, bool>(0),
    )? {
        return Err(DestageMetadataError::MissingStore(id.to_string()));
    }
    Ok(())
}
fn read_identity(
    tx: &Transaction<'_>,
    id: &ObjectId,
) -> Result<Option<(String, i64, String, String, String, u8)>, DestageMetadataError> {
    tx.query_row("SELECT q.store_id,q.expected_size_bytes,q.content_hash_algorithm,q.content_hash,q.acknowledgement_policy,q.required_copy_count FROM destage_queue q JOIN ssd_object_placements s ON s.object_id=q.object_id WHERE q.object_id=?1",[id.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional().map_err(Into::into)
}
fn read_record_tx(
    tx: &Transaction<'_>,
    id: &str,
) -> Result<DestageQueueRecord, DestageMetadataError> {
    let tuple=tx.query_row("SELECT q.destage_job_id,q.store_id,q.object_id,o.object_type,q.state,q.expected_size_bytes,q.content_hash_algorithm,q.content_hash,q.acknowledgement_policy,q.required_copy_count,q.verified_copy_count,q.priority,q.attempt_count,q.max_attempts,q.last_error,q.next_retry_at_utc,q.lease_owner,q.created_at_utc,q.updated_at_utc FROM destage_queue q JOIN objects o ON o.object_id=q.object_id WHERE q.object_id=?1",[id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,i64>(5)?,r.get::<_,String>(6)?,r.get::<_,String>(7)?,r.get::<_,String>(8)?,r.get::<_,u8>(9)?,r.get::<_,u8>(10)?,r.get::<_,i32>(11)?,r.get::<_,u32>(12)?,r.get::<_,u32>(13)?,r.get::<_,Option<String>>(14)?,r.get::<_,Option<String>>(15)?,r.get::<_,Option<String>>(16)?,r.get::<_,String>(17)?,r.get::<_,String>(18)?)))?;
    Ok(DestageQueueRecord {
        destage_job_id: tuple.0,
        store_id: StoreId::new(tuple.1).map_err(|_| DestageMetadataError::InvalidIdentifier)?,
        object_id: ObjectId::new(tuple.2).map_err(|_| DestageMetadataError::InvalidIdentifier)?,
        object_type: tuple.3,
        state: DestageState::parse(&tuple.4)?,
        expected_size_bytes: u64::try_from(tuple.5)
            .map_err(|_| DestageMetadataError::InvalidSize)?,
        content_hash_algorithm: tuple.6,
        content_hash: tuple.7,
        acknowledgement_policy: tuple.8,
        required_copy_count: tuple.9,
        verified_copy_count: tuple.10,
        priority: tuple.11,
        attempt_count: tuple.12,
        max_attempts: tuple.13,
        last_error: tuple.14,
        next_retry_at_utc: tuple.15,
        lease_owner: tuple.16,
        created_at_utc: tuple.17,
        updated_at_utc: tuple.18,
    })
}
fn to_i64(v: u64) -> Result<i64, DestageMetadataError> {
    i64::try_from(v).map_err(|_| DestageMetadataError::InvalidSize)
}

#[derive(Debug)]
pub enum DestageMetadataError {
    Sqlite(rusqlite::Error),
    BlankField(&'static str),
    ZeroCopies,
    ZeroAttempts,
    InvalidSize,
    InvalidIdentifier,
    InvalidState(String),
    MissingStore(String),
    MissingDisk(String),
    ObjectConflict(String),
    IngestJobConflict(String),
    ClaimConflict,
    InvalidTransition,
    WouldRemoveOnlyVerifiedCopy,
    InsufficientCopies { required: u8, verified: u8 },
    PlacementHashMismatch,
    SsdNotEvictionEligible,
}
impl From<rusqlite::Error> for DestageMetadataError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(e)
    }
}
impl Display for DestageMetadataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(e) => write!(f, "durable destage metadata failed: {e}"),
            Self::BlankField(v) => write!(f, "{v} must not be blank"),
            Self::ZeroCopies => f.write_str("required copy count must be positive"),
            Self::ZeroAttempts => f.write_str("maximum attempts must be positive"),
            Self::InvalidSize => f.write_str("byte count exceeds SQLite range"),
            Self::InvalidIdentifier => f.write_str("invalid durable destage identifier"),
            Self::InvalidState(v) => write!(f, "invalid durable destage state {v}"),
            Self::MissingStore(v) => write!(f, "missing store {v}"),
            Self::MissingDisk(v) => write!(f, "missing disk {v}"),
            Self::ObjectConflict(v) => write!(f, "immutable object conflict for {v}"),
            Self::IngestJobConflict(v) => {
                write!(f, "immutable ingest job identity conflict for {v}")
            }
            Self::ClaimConflict => f.write_str("destage lease is not owned by this worker"),
            Self::InvalidTransition => f.write_str("invalid durable destage state transition"),
            Self::WouldRemoveOnlyVerifiedCopy => {
                f.write_str("cancellation would leave no verified copy")
            }
            Self::InsufficientCopies { required, verified } => write!(
                f,
                "HDD settlement requires {required} verified copies, got {verified}"
            ),
            Self::PlacementHashMismatch => {
                f.write_str("HDD placement checksum does not match the queued object")
            }
            Self::SsdNotEvictionEligible => {
                f.write_str("SSD placement is not eligible for eviction")
            }
        }
    }
}
impl std::error::Error for DestageMetadataError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn ssd_commit_is_atomic_durable_and_idempotent() {
        let path = database("ssd-commit");
        prepare(&path, &["store-a"]);
        let first = commit(&path, "store-a", "object-a", "job-a", 5).expect("first SSD commit");
        assert!(!first.idempotent);

        drop(Connection::open(&path).expect("simulate reopen"));
        let replay =
            commit(&path, "store-a", "object-a", "different-job", 5).expect("exact replay");
        assert!(replay.idempotent);
        assert_eq!(replay.destage_job_id, "job-a");
        let connection = Connection::open(&path).expect("reopen database");
        assert_eq!(count(&connection, "objects"), 1);
        assert_eq!(count(&connection, "ssd_object_placements"), 1);
        assert_eq!(count(&connection, "destage_queue"), 1);
        cleanup(path);
    }

    #[test]
    fn immutable_conflict_rolls_back_without_partial_rows() {
        let path = database("conflict");
        prepare(&path, &["store-a"]);
        commit(&path, "store-a", "object-a", "job-a", 5).expect("first commit");
        let store = StoreId::new("store-a").expect("store");
        let object = ObjectId::new("object-a").expect("object");
        let error = commit_verified_ssd_and_enqueue(
            &path,
            VerifiedSsdCommitRequest {
                destage_job_id: "job-b",
                store_id: &store,
                object_id: &object,
                object_type: "naive",
                relative_path: "ingest/object-a",
                size_bytes: 6,
                content_hash_algorithm: "sha256",
                content_hash: "different",
                acknowledgement_policy: "after_ssd_ingest",
                required_copy_count: 1,
                max_attempts: 3,
                priority: 0,
                committed_at_utc: "2026-01-01T00:01:00Z",
                ingest_job_id: None,
                ingress_origin: None,
            },
        )
        .expect_err("conflict");
        assert!(matches!(error, DestageMetadataError::ObjectConflict(_)));
        let connection = Connection::open(&path).expect("reopen");
        assert_eq!(count(&connection, "destage_queue"), 1);
        cleanup(path);
    }

    #[test]
    fn deterministic_ingest_identity_rejects_a_different_object() {
        let path = database("ingest-identity");
        prepare(&path, &["store-a"]);
        let store = StoreId::new("store-a").expect("store");
        for (object, destage_job) in [("object-a", "job-a"), ("object-b", "job-b")] {
            let object_id = ObjectId::new(object).expect("object");
            let result = commit_verified_ssd_and_enqueue(
                &path,
                VerifiedSsdCommitRequest {
                    destage_job_id: destage_job,
                    store_id: &store,
                    object_id: &object_id,
                    object_type: "naive",
                    relative_path: &format!("ingest/{object}"),
                    size_bytes: 5,
                    content_hash_algorithm: "sha256",
                    content_hash: "abcde",
                    acknowledgement_policy: "after_ssd_ingest",
                    required_copy_count: 1,
                    max_attempts: 3,
                    priority: 0,
                    committed_at_utc: "2026-01-01T00:00:00Z",
                    ingest_job_id: Some("adopt-fixed"),
                    ingress_origin: Some("remote_s3"),
                },
            );
            if object == "object-a" {
                result.expect("first identity");
            } else {
                assert!(matches!(
                    result,
                    Err(DestageMetadataError::IngestJobConflict(value))
                        if value == "adopt-fixed"
                ));
            }
        }
        let connection = Connection::open(&path).expect("reopen");
        assert_eq!(count(&connection, "ingest_jobs"), 1);
        assert_eq!(count(&connection, "objects"), 1);
        cleanup(path);
    }

    #[test]
    fn locked_catalogue_fails_without_partial_adoption_rows() {
        let path = database("locked-catalogue");
        prepare(&path, &["store-a"]);
        let lock = Connection::open(&path).expect("lock connection");
        lock.execute_batch("BEGIN EXCLUSIVE")
            .expect("exclusive lock");

        let store = StoreId::new("store-a").expect("store");
        let object = ObjectId::new("object-a").expect("object");
        let error = commit_verified_ssd_and_enqueue(
            &path,
            VerifiedSsdCommitRequest {
                destage_job_id: "job-a",
                store_id: &store,
                object_id: &object,
                object_type: "naive",
                relative_path: "ingest/object-a",
                size_bytes: 5,
                content_hash_algorithm: "sha256",
                content_hash: "abcde",
                acknowledgement_policy: "after_ssd_ingest",
                required_copy_count: 1,
                max_attempts: 3,
                priority: 0,
                committed_at_utc: "2026-01-01T00:00:00Z",
                ingest_job_id: Some("adopt-locked"),
                ingress_origin: Some("remote_s3"),
            },
        )
        .expect_err("locked publication");
        assert!(matches!(
            error,
            DestageMetadataError::Sqlite(rusqlite::Error::SqliteFailure(_, _))
        ));
        lock.execute_batch("ROLLBACK").expect("unlock");
        let connection = Connection::open(&path).expect("reopen");
        assert_eq!(count(&connection, "ingest_jobs"), 0);
        assert_eq!(count(&connection, "objects"), 0);
        cleanup(path);
    }

    #[test]
    fn claim_is_leased_retryable_and_fair_across_stores() {
        let path = database("fairness");
        prepare(&path, &["store-a", "store-b"]);
        commit(&path, "store-a", "object-a", "job-a", 5).expect("a");
        commit(&path, "store-b", "object-b", "job-b", 1).expect("b");
        let previous = StoreId::new("store-a").expect("store");
        let claimed = claim_next_destage(
            &path,
            "worker-1",
            "2026-01-01T00:10:00Z",
            "2026-01-01T00:02:00Z",
            Some(&previous),
        )
        .expect("claim")
        .expect("work");
        assert_eq!(claimed.store_id.as_str(), "store-b");
        assert_eq!(claimed.attempt_count, 1);
        fail_destage(
            &path,
            &claimed.object_id,
            "worker-1",
            "disk offline",
            Some("2026-01-01T00:05:00Z"),
            "2026-01-01T00:03:00Z",
        )
        .expect("fail");
        let before_retry = claim_next_destage(
            &path,
            "worker-2",
            "2026-01-01T00:11:00Z",
            "2026-01-01T00:04:00Z",
            None,
        )
        .expect("claim other")
        .expect("other work");
        assert_eq!(before_retry.store_id.as_str(), "store-a");
        cleanup(path);
    }

    #[test]
    fn hdd_promotion_is_atomic_and_marks_ssd_eviction_eligible() {
        let path = database("promotion");
        prepare(&path, &["store-a"]);
        commit(&path, "store-a", "object-a", "job-a", 5).expect("commit");
        let object = ObjectId::new("object-a").expect("object");
        claim_next_destage(
            &path,
            "worker",
            "2026-01-01T00:10:00Z",
            "2026-01-01T00:02:00Z",
            None,
        )
        .expect("claim");
        let placements = [VerifiedHddPlacement {
            placement_id: "placement-a",
            disk_id: "disk-a",
            relative_path: "objects/object-a",
            content_hash: "abcde",
        }];
        assert!(!promote_hdd_settlement(
            &path,
            HddSettlementPromotionRequest {
                object_id: &object,
                worker: "worker",
                placements: &placements,
                verified_at_utc: "2026-01-01T00:03:00Z"
            }
        )
        .expect("promote"));
        assert!(promote_hdd_settlement(
            &path,
            HddSettlementPromotionRequest {
                object_id: &object,
                worker: "any-worker",
                placements: &placements,
                verified_at_utc: "2026-01-01T00:04:00Z"
            }
        )
        .expect("idempotent"));
        let connection = Connection::open(&path).expect("open");
        let row: (String, bool) = connection.query_row("SELECT o.state, s.eviction_eligible FROM objects o JOIN ssd_object_placements s USING(object_id) WHERE o.object_id='object-a'", [], |r| Ok((r.get(0)?, r.get(1)?))).expect("state");
        assert_eq!(row, ("HddCopyVerified".to_string(), true));
        cleanup(path);
    }

    #[test]
    fn expired_lease_is_reclaimed_and_attempt_limit_needs_review() {
        let path = database("lease-restart");
        prepare(&path, &["store-a"]);
        commit(&path, "store-a", "object-a", "job-a", 5).expect("commit");
        let first = claim_next_destage(
            &path,
            "dead-worker",
            "2026-01-01T00:02:00Z",
            "2026-01-01T00:01:00Z",
            None,
        )
        .expect("claim")
        .expect("work");
        let active = claim_next_destage(
            &path,
            "other",
            "2026-01-01T00:03:00Z",
            "2026-01-01T00:01:30Z",
            None,
        )
        .expect("protected");
        assert!(active.is_none());
        let reclaimed = claim_next_destage(
            &path,
            "restart-worker",
            "2026-01-01T00:04:00Z",
            "2026-01-01T00:02:30Z",
            None,
        )
        .expect("reclaim")
        .expect("work");
        assert_eq!(reclaimed.attempt_count, first.attempt_count + 1);
        fail_destage(
            &path,
            &reclaimed.object_id,
            "restart-worker",
            "again",
            None,
            "2026-01-01T00:03:00Z",
        )
        .expect("fail");
        let final_claim = claim_next_destage(
            &path,
            "last-worker",
            "2026-01-01T00:05:00Z",
            "2026-01-01T00:03:30Z",
            None,
        )
        .expect("last")
        .expect("work");
        fail_destage(
            &path,
            &final_claim.object_id,
            "last-worker",
            "exhausted",
            None,
            "2026-01-01T00:04:00Z",
        )
        .expect("fail");
        assert!(claim_next_destage(
            &path,
            "never",
            "2026-01-01T00:06:00Z",
            "2026-01-01T00:05:00Z",
            None
        )
        .expect("none")
        .is_none());
        assert_eq!(
            read_destage(&path, &final_claim.object_id)
                .expect("read")
                .expect("row")
                .state,
            DestageState::NeedsReview
        );
        cleanup(path);
    }

    #[test]
    fn promotion_failure_rolls_back_placements_and_eviction() {
        let path = database("promotion-rollback");
        prepare(&path, &["store-a"]);
        commit(&path, "store-a", "object-a", "job-a", 5).expect("commit");
        let object = ObjectId::new("object-a").expect("object");
        claim_next_destage(
            &path,
            "worker",
            "2026-01-01T00:10:00Z",
            "2026-01-01T00:02:00Z",
            None,
        )
        .expect("claim");
        let placements = [VerifiedHddPlacement {
            placement_id: "bad",
            disk_id: "missing",
            relative_path: "objects/object-a",
            content_hash: "abcde",
        }];
        assert!(matches!(
            promote_hdd_settlement(
                &path,
                HddSettlementPromotionRequest {
                    object_id: &object,
                    worker: "worker",
                    placements: &placements,
                    verified_at_utc: "2026-01-01T00:03:00Z"
                }
            ),
            Err(DestageMetadataError::MissingDisk(_))
        ));
        let connection = Connection::open(&path).expect("open");
        assert_eq!(count(&connection, "placements"), 0);
        assert!(
            !read_ssd_placement(&path, &object)
                .expect("SSD")
                .expect("row")
                .eviction_eligible
        );
        assert!(matches!(
            mark_ssd_evicted(&path, &object, "later"),
            Err(DestageMetadataError::SsdNotEvictionEligible)
        ));
        cleanup(path);
    }

    fn commit(
        path: &Path,
        store: &str,
        object: &str,
        job: &str,
        priority: i32,
    ) -> Result<VerifiedSsdCommitReport, DestageMetadataError> {
        let store_id = StoreId::new(store).expect("store");
        let object_id = ObjectId::new(object).expect("object");
        commit_verified_ssd_and_enqueue(
            path,
            VerifiedSsdCommitRequest {
                destage_job_id: job,
                store_id: &store_id,
                object_id: &object_id,
                object_type: "naive",
                relative_path: &format!("ingest/{object}"),
                size_bytes: 5,
                content_hash_algorithm: "sha256",
                content_hash: "abcde",
                acknowledgement_policy: "after_ssd_ingest",
                required_copy_count: 1,
                max_attempts: 3,
                priority,
                committed_at_utc: "2026-01-01T00:00:00Z",
                ingest_job_id: None,
                ingress_origin: None,
            },
        )
    }

    fn prepare(path: &Path, stores: &[&str]) {
        let connection = Connection::open(path).expect("open");
        connection.execute_batch(LIVE_SCHEMA_SQL).expect("schema");
        connection.execute("INSERT INTO pools(pool_id,state,created_at_utc,updated_at_utc) VALUES('pool-a','Clean','now','now')", []).expect("pool");
        connection.execute("INSERT INTO disks(disk_id,pool_id,role,state,created_at_utc,updated_at_utc) VALUES('disk-a','pool-a','hdd_capacity','Healthy','now','now')", []).expect("disk");
        for store in stores {
            connection.execute("INSERT INTO stores(store_id,pool_id,class,policy_json,created_at_utc,updated_at_utc) VALUES(?1,'pool-a','generated_data','{}','now','now')", [store]).expect("store");
        }
    }
    fn count(connection: &Connection, table: &str) -> u64 {
        connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .expect("count")
    }
    fn database(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dasobjectstore-destage-{name}-{}-{nonce}.sqlite",
            std::process::id()
        ))
    }
    fn cleanup(path: PathBuf) {
        let _ = std::fs::remove_file(path);
    }
}
