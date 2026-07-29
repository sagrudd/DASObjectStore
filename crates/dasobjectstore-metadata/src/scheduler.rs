//! Persistent, restart-safe arbitration for daemon-owned data work.

use crate::schema::LIVE_SCHEMA_SQL;
use dasobjectstore_core::ids::{ObjectId, StoreId};
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};
use std::path::{Path, PathBuf};
use std::time::Duration;

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
const DEFICIT_QUANTUM_BYTES: u64 = 64 * 1024 * 1024;
const SCHEDULER_SCHEMA_SQL: &str = LIVE_SCHEMA_SQL;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchedulerClassPolicy {
    pub work_class: String,
    pub weight: u32,
    pub max_active_jobs: u32,
    pub max_active_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchedulerJobRequest {
    pub live_sqlite_path: PathBuf,
    pub scheduler_job_id: String,
    pub idempotency_key: String,
    pub request_digest: String,
    pub class: SchedulerClassPolicy,
    pub origin: String,
    pub store_id: StoreId,
    pub object_id: Option<ObjectId>,
    pub priority: i32,
    pub byte_cost: u64,
    pub acknowledgement_policy: String,
    pub required_copy_count: u8,
    pub max_attempts: u32,
    pub created_at_utc: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SchedulerJob {
    pub scheduler_job_id: String,
    pub idempotency_key: String,
    pub request_digest: String,
    pub work_class: String,
    pub origin: String,
    pub store_id: StoreId,
    pub object_id: Option<ObjectId>,
    pub state: String,
    pub priority: i32,
    pub byte_cost: u64,
    pub acknowledgement_policy: String,
    pub required_copy_count: u8,
    pub cancellation_requested: bool,
    pub attempt_count: u32,
    pub max_attempts: u32,
    pub next_retry_at_utc: Option<String>,
    pub lease_owner: Option<String>,
    pub lease_epoch: u64,
    pub lease_expires_at_utc: Option<String>,
    pub last_error: Option<String>,
    pub created_at_utc: String,
    pub updated_at_utc: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SchedulerClaimRequest {
    pub live_sqlite_path: PathBuf,
    pub worker: String,
    pub now_utc: String,
    pub lease_expires_at_utc: String,
}

#[derive(Debug)]
pub enum SchedulerError {
    Sqlite(rusqlite::Error),
    Invalid(&'static str),
    Conflict(String),
    LeaseConflict(String),
}

impl Display for SchedulerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => write!(formatter, "persistent scheduler failed: {error}"),
            Self::Invalid(field) => write!(formatter, "invalid scheduler {field}"),
            Self::Conflict(id) => write!(formatter, "scheduler identity conflict for {id}"),
            Self::LeaseConflict(id) => write!(formatter, "scheduler lease conflict for {id}"),
        }
    }
}

impl std::error::Error for SchedulerError {}

impl From<rusqlite::Error> for SchedulerError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sqlite(value)
    }
}

pub fn submit_scheduler_job(request: &SchedulerJobRequest) -> Result<SchedulerJob, SchedulerError> {
    validate_submit(request)?;
    let mut connection = open(&request.live_sqlite_path)?;
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute(
        "INSERT INTO scheduler_classes (
             work_class,weight,max_active_jobs,max_active_bytes,updated_at_utc
         ) VALUES (?1,?2,?3,?4,?5)
         ON CONFLICT(work_class) DO UPDATE SET
             weight=excluded.weight,max_active_jobs=excluded.max_active_jobs,
             max_active_bytes=excluded.max_active_bytes,updated_at_utc=excluded.updated_at_utc",
        params![
            request.class.work_class,
            request.class.weight,
            request.class.max_active_jobs,
            to_i64(request.class.max_active_bytes)?,
            request.created_at_utc
        ],
    )?;
    tx.execute(
        "INSERT OR IGNORE INTO scheduler_state(singleton,service_sequence,updated_at_utc)
         VALUES (1,0,?1)",
        [request.created_at_utc.as_str()],
    )?;
    tx.execute(
        "INSERT OR IGNORE INTO scheduler_jobs (
             scheduler_job_id,idempotency_key,request_digest,work_class,origin,
             store_id,object_id,state,priority,byte_cost,acknowledgement_policy,
             required_copy_count,max_attempts,created_at_utc,updated_at_utc
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,'queued',?8,?9,?10,?11,?12,?13,?13)",
        params![
            request.scheduler_job_id,
            request.idempotency_key,
            request.request_digest,
            request.class.work_class,
            request.origin,
            request.store_id.as_str(),
            request.object_id.as_ref().map(ObjectId::as_str),
            request.priority,
            to_i64(request.byte_cost)?,
            request.acknowledgement_policy,
            request.required_copy_count,
            request.max_attempts,
            request.created_at_utc
        ],
    )?;
    let job = read_job_by_idempotency(&tx, &request.idempotency_key)?;
    if !job_matches_request(&job, request) {
        return Err(SchedulerError::Conflict(request.idempotency_key.clone()));
    }
    tx.commit()?;
    Ok(job)
}

pub fn claim_next_scheduler_job(
    request: &SchedulerClaimRequest,
) -> Result<Option<SchedulerJob>, SchedulerError> {
    claim_next_scheduler_job_for_class(request, None)
}

pub fn claim_next_scheduler_job_in_class(
    request: &SchedulerClaimRequest,
    work_class: &str,
) -> Result<Option<SchedulerJob>, SchedulerError> {
    if work_class.trim().is_empty() {
        return Err(SchedulerError::Invalid("work class"));
    }
    claim_next_scheduler_job_for_class(request, Some(work_class))
}

fn claim_next_scheduler_job_for_class(
    request: &SchedulerClaimRequest,
    required_class: Option<&str>,
) -> Result<Option<SchedulerJob>, SchedulerError> {
    if request.worker.trim().is_empty()
        || request.now_utc.trim().is_empty()
        || request.lease_expires_at_utc.trim().is_empty()
    {
        return Err(SchedulerError::Invalid("claim"));
    }
    let mut connection = open(&request.live_sqlite_path)?;
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    recover_expired_tx(&tx, &request.now_utc)?;
    let classes = eligible_classes(&tx, &request.now_utc, required_class)?;
    let Some(class) = classes.into_iter().min_by(|left, right| {
        weighted_sequence(left.last_served, left.weight)
            .cmp(&weighted_sequence(right.last_served, right.weight))
            .then_with(|| left.work_class.cmp(&right.work_class))
    }) else {
        tx.commit()?;
        return Ok(None);
    };
    let job_id: Option<String> = tx
        .query_row(
            "SELECT j.scheduler_job_id
             FROM scheduler_jobs j
             LEFT JOIN scheduler_store_fairness f
               ON f.work_class=j.work_class AND f.store_id=j.store_id
             WHERE j.work_class=?1 AND j.state IN ('queued','retry_wait')
               AND j.cancellation_requested=0
               AND j.attempt_count < j.max_attempts
               AND (j.next_retry_at_utc IS NULL OR j.next_retry_at_utc<=?2)
               AND j.byte_cost <= (
                   SELECT c.max_active_bytes-COALESCE(SUM(a.byte_cost),0)
                   FROM scheduler_classes c
                   LEFT JOIN scheduler_jobs a
                     ON a.work_class=c.work_class AND a.state='running'
                   WHERE c.work_class=j.work_class
               )
             ORDER BY COALESCE(f.last_served_sequence,0),j.priority DESC,
                      j.created_at_utc,j.scheduler_job_id
             LIMIT 1",
            params![class.work_class, request.now_utc],
            |row| row.get(0),
        )
        .optional()?;
    let Some(job_id) = job_id else {
        tx.commit()?;
        return Ok(None);
    };
    let changed = tx.execute(
        "UPDATE scheduler_jobs SET state='running',lease_owner=?1,
             lease_epoch=lease_epoch+1,lease_expires_at_utc=?2,
             attempt_count=attempt_count+1,updated_at_utc=?3
         WHERE scheduler_job_id=?4 AND state IN ('queued','retry_wait')
           AND cancellation_requested=0
           AND byte_cost <= (
               SELECT c.max_active_bytes-COALESCE(SUM(a.byte_cost),0)
               FROM scheduler_classes c
               LEFT JOIN scheduler_jobs a
                 ON a.work_class=c.work_class AND a.state='running'
               WHERE c.work_class=scheduler_jobs.work_class
           )",
        params![
            request.worker,
            request.lease_expires_at_utc,
            request.now_utc,
            job_id
        ],
    )?;
    if changed != 1 {
        return Err(SchedulerError::LeaseConflict(job_id));
    }
    let sequence: u64 = tx.query_row(
        "UPDATE scheduler_state SET service_sequence=service_sequence+1,
             updated_at_utc=?1 WHERE singleton=1 RETURNING service_sequence",
        [request.now_utc.as_str()],
        |row| row.get(0),
    )?;
    let claimed = read_job(&tx, &job_id)?;
    tx.execute(
        "UPDATE scheduler_classes SET last_served_sequence=?1,
             deficit_bytes=deficit_bytes+(weight*?2)-MIN(deficit_bytes+(weight*?2),?3),
             updated_at_utc=?4 WHERE work_class=?5",
        params![
            sequence,
            DEFICIT_QUANTUM_BYTES,
            claimed.byte_cost,
            request.now_utc,
            claimed.work_class
        ],
    )?;
    tx.execute(
        "INSERT INTO scheduler_store_fairness(
             work_class,store_id,last_served_sequence,updated_at_utc
         ) VALUES (?1,?2,?3,?4)
         ON CONFLICT(work_class,store_id) DO UPDATE SET
             last_served_sequence=excluded.last_served_sequence,
             updated_at_utc=excluded.updated_at_utc",
        params![
            claimed.work_class,
            claimed.store_id.as_str(),
            sequence,
            request.now_utc
        ],
    )?;
    tx.commit()?;
    Ok(Some(claimed))
}

pub(crate) struct DestageSchedulerSubmission<'a> {
    pub destage_job_id: &'a str,
    pub store_id: &'a StoreId,
    pub object_id: &'a ObjectId,
    pub byte_cost: u64,
    pub acknowledgement_policy: &'a str,
    pub required_copy_count: u8,
    pub priority: i32,
    pub origin: &'a str,
    pub created_at_utc: &'a str,
}

fn scheduler_policy_for_origin(origin: &str) -> (&'static str, u32, u32, u64) {
    match origin {
        "remote_s3" => ("direct_s3", 4, 8, 2_u64 << 40),
        "workspace_promotion" => ("verification_repair", 1, 2, 1_u64 << 40),
        "remote_reconcile" => ("remote_reconcile", 2, 2, 1_u64 << 40),
        "destage" => ("destage", 1, 2, 1_u64 << 40),
        _ => ("native_ingest", 4, 4, 2_u64 << 40),
    }
}

pub(crate) fn submit_destage_scheduler_job_tx(
    tx: &Transaction<'_>,
    submission: &DestageSchedulerSubmission<'_>,
) -> Result<(), SchedulerError> {
    tx.execute_batch(SCHEDULER_SCHEMA_SQL)?;
    let (work_class, weight, active, active_bytes) = scheduler_policy_for_origin(submission.origin);
    tx.execute(
        "INSERT INTO scheduler_classes(
             work_class,weight,max_active_jobs,max_active_bytes,updated_at_utc
         ) VALUES(?1,?2,?3,?4,?5)
         ON CONFLICT(work_class) DO UPDATE SET weight=excluded.weight,
             max_active_jobs=excluded.max_active_jobs,
             max_active_bytes=excluded.max_active_bytes,updated_at_utc=excluded.updated_at_utc",
        params![
            work_class,
            weight,
            active,
            to_i64(active_bytes)?,
            submission.created_at_utc
        ],
    )?;
    tx.execute(
        "INSERT OR IGNORE INTO scheduler_state(singleton,service_sequence,updated_at_utc)
         VALUES(1,0,?1)",
        [submission.created_at_utc],
    )?;
    let scheduler_job_id = format!("scheduler-{}", submission.destage_job_id);
    let idempotency_key = format!("destage:{}", submission.object_id.as_str());
    let byte_cost = submission.byte_cost.max(1);
    let request_digest = format!(
        "{}:{}:{}:{}",
        submission.object_id.as_str(),
        byte_cost,
        submission.required_copy_count,
        submission.acknowledgement_policy
    );
    tx.execute(
        "INSERT OR IGNORE INTO scheduler_jobs(
             scheduler_job_id,idempotency_key,request_digest,work_class,origin,
             store_id,object_id,state,priority,byte_cost,acknowledgement_policy,
             required_copy_count,max_attempts,created_at_utc,updated_at_utc
         ) VALUES(?1,?2,?3,?4,?5,?6,?7,'queued',?8,?9,?10,?11,8,?12,?12)",
        params![
            scheduler_job_id,
            idempotency_key,
            request_digest,
            work_class,
            submission.origin,
            submission.store_id.as_str(),
            submission.object_id.as_str(),
            submission.priority,
            to_i64(byte_cost)?,
            submission.acknowledgement_policy,
            submission.required_copy_count,
            submission.created_at_utc,
        ],
    )?;
    let stored: (String, String, String, String, u64, u8) = tx.query_row(
        "SELECT scheduler_job_id,request_digest,store_id,object_id,byte_cost,
                required_copy_count FROM scheduler_jobs WHERE idempotency_key=?1",
        [&idempotency_key],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        },
    )?;
    if stored
        != (
            scheduler_job_id,
            request_digest,
            submission.store_id.as_str().to_string(),
            submission.object_id.as_str().to_string(),
            byte_cost,
            submission.required_copy_count,
        )
    {
        return Err(SchedulerError::Conflict(idempotency_key));
    }
    Ok(())
}

pub(crate) fn cancel_destage_scheduler_job_tx(
    tx: &Transaction<'_>,
    object_id: &ObjectId,
    updated_at_utc: &str,
) -> Result<(), SchedulerError> {
    tx.execute_batch(SCHEDULER_SCHEMA_SQL)?;
    tx.execute(
        "UPDATE scheduler_jobs SET cancellation_requested=1,
             state=CASE WHEN state IN ('queued','retry_wait') THEN 'cancelled' ELSE state END,
             updated_at_utc=?1 WHERE idempotency_key=?2
             AND state NOT IN ('completed','cancelled','needs_review')",
        params![updated_at_utc, format!("destage:{}", object_id.as_str())],
    )?;
    Ok(())
}

pub(crate) fn pause_destage_scheduler_job_tx(
    tx: &Transaction<'_>,
    object_id: &ObjectId,
    updated_at_utc: &str,
) -> Result<(), SchedulerError> {
    tx.execute_batch(SCHEDULER_SCHEMA_SQL)?;
    tx.execute(
        "UPDATE scheduler_jobs SET state='paused',next_retry_at_utc=NULL,
             lease_owner=NULL,lease_expires_at_utc=NULL,updated_at_utc=?1
         WHERE idempotency_key=?2 AND state IN ('queued','retry_wait')",
        params![updated_at_utc, format!("destage:{}", object_id.as_str())],
    )?;
    Ok(())
}

pub(crate) fn resume_destage_scheduler_job_tx(
    tx: &Transaction<'_>,
    object_id: &ObjectId,
    updated_at_utc: &str,
) -> Result<(), SchedulerError> {
    tx.execute_batch(SCHEDULER_SCHEMA_SQL)?;
    tx.execute(
        "UPDATE scheduler_jobs SET state='queued',next_retry_at_utc=NULL,
             last_error=NULL,updated_at_utc=?1
         WHERE idempotency_key=?2 AND state='paused' AND cancellation_requested=0",
        params![updated_at_utc, format!("destage:{}", object_id.as_str())],
    )?;
    Ok(())
}

pub(crate) fn retry_destage_scheduler_job_tx(
    tx: &Transaction<'_>,
    object_id: &ObjectId,
    updated_at_utc: &str,
) -> Result<(), SchedulerError> {
    tx.execute_batch(SCHEDULER_SCHEMA_SQL)?;
    tx.execute(
        "UPDATE scheduler_jobs SET state='queued',attempt_count=0,next_retry_at_utc=NULL,
             last_error=NULL,lease_owner=NULL,lease_expires_at_utc=NULL,updated_at_utc=?1
         WHERE idempotency_key=?2 AND state IN ('retry_wait','needs_review')
           AND cancellation_requested=0",
        params![updated_at_utc, format!("destage:{}", object_id.as_str())],
    )?;
    Ok(())
}

pub fn backfill_destage_scheduler_jobs(
    path: impl AsRef<Path>,
    now_utc: &str,
) -> Result<u64, SchedulerError> {
    let mut connection = open(path.as_ref())?;
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute(
        "UPDATE scheduler_jobs SET state='completed',lease_owner=NULL,
             lease_expires_at_utc=NULL,last_error=NULL,updated_at_utc=?1
         WHERE idempotency_key LIKE 'destage:%' AND state!='completed' AND EXISTS(
             SELECT 1 FROM destage_queue d WHERE d.object_id=scheduler_jobs.object_id
             AND d.state='hdd_copy_verified'
         )",
        [now_utc],
    )?;
    tx.execute(
        "UPDATE scheduler_jobs SET state='paused',lease_owner=NULL,
             lease_expires_at_utc=NULL,updated_at_utc=?1
         WHERE idempotency_key LIKE 'destage:%' AND state IN ('queued','retry_wait')
           AND EXISTS(SELECT 1 FROM destage_queue d
                      WHERE d.object_id=scheduler_jobs.object_id AND d.state='paused')",
        [now_utc],
    )?;
    tx.execute(
        "UPDATE scheduler_jobs SET state='needs_review',lease_owner=NULL,
             lease_expires_at_utc=NULL,updated_at_utc=?1
         WHERE idempotency_key LIKE 'destage:%'
           AND state NOT IN ('completed','cancelled','needs_review')
           AND EXISTS(SELECT 1 FROM destage_queue d
                      WHERE d.object_id=scheduler_jobs.object_id AND d.state='needs_review')",
        [now_utc],
    )?;
    tx.execute(
        "UPDATE scheduler_jobs SET state='cancelled',cancellation_requested=1,
             lease_owner=NULL,lease_expires_at_utc=NULL,updated_at_utc=?1
         WHERE idempotency_key LIKE 'destage:%' AND state NOT IN ('completed','cancelled') AND EXISTS(
             SELECT 1 FROM destage_queue d WHERE d.object_id=scheduler_jobs.object_id
             AND d.state='cancelled'
         )",
        [now_utc],
    )?;
    let rows = {
        let mut statement = tx.prepare(
            "SELECT d.destage_job_id,d.store_id,d.object_id,d.expected_size_bytes,
                    d.acknowledgement_policy,d.required_copy_count,d.priority,d.created_at_utc,
                    COALESCE((SELECT i.ingest_mode FROM ingest_jobs i
                              WHERE i.object_id=d.object_id
                              ORDER BY i.created_at_utc,i.ingest_job_id LIMIT 1),'native_ingest')
             FROM destage_queue d LEFT JOIN scheduler_jobs s
               ON s.idempotency_key='destage:'||d.object_id
             WHERE s.scheduler_job_id IS NULL
               AND d.state IN ('queued_for_hdd','destage_failed','hdd_copying')
             ORDER BY d.created_at_utc,d.destage_job_id",
        )?;
        let mapped = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, u64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, u8>(5)?,
                row.get::<_, i32>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
            ))
        })?;
        mapped.collect::<Result<Vec<_>, _>>()?
    };
    let count = rows.len() as u64;
    for (job, store, object, bytes, acknowledgement, copies, priority, created, origin) in rows {
        let store = StoreId::new(store).map_err(|_| SchedulerError::Invalid("store_id"))?;
        let object = ObjectId::new(object).map_err(|_| SchedulerError::Invalid("object_id"))?;
        submit_destage_scheduler_job_tx(
            &tx,
            &DestageSchedulerSubmission {
                destage_job_id: &job,
                store_id: &store,
                object_id: &object,
                byte_cost: bytes,
                acknowledgement_policy: &acknowledgement,
                required_copy_count: copies,
                priority,
                origin: &origin,
                created_at_utc: if created.trim().is_empty() {
                    now_utc
                } else {
                    &created
                },
            },
        )?;
    }
    tx.commit()?;
    Ok(count)
}

pub fn renew_scheduler_lease(
    path: impl AsRef<Path>,
    job_id: &str,
    worker: &str,
    lease_epoch: u64,
    expires_at_utc: &str,
    updated_at_utc: &str,
) -> Result<(), SchedulerError> {
    transition_lease(
        path,
        job_id,
        worker,
        lease_epoch,
        "UPDATE scheduler_jobs SET lease_expires_at_utc=?1,updated_at_utc=?2
         WHERE scheduler_job_id=?3 AND state='running' AND lease_owner=?4
           AND lease_epoch=?5",
        params![expires_at_utc, updated_at_utc, job_id, worker, lease_epoch],
    )
}

pub fn complete_scheduler_job(
    path: impl AsRef<Path>,
    job_id: &str,
    worker: &str,
    lease_epoch: u64,
    updated_at_utc: &str,
) -> Result<(), SchedulerError> {
    transition_lease(
        path,
        job_id,
        worker,
        lease_epoch,
        "UPDATE scheduler_jobs SET state='completed',lease_owner=NULL,
             lease_expires_at_utc=NULL,last_error=NULL,updated_at_utc=?1
         WHERE scheduler_job_id=?2 AND state='running' AND lease_owner=?3
           AND lease_epoch=?4",
        params![updated_at_utc, job_id, worker, lease_epoch],
    )
}

pub fn retry_scheduler_job(
    path: impl AsRef<Path>,
    job_id: &str,
    worker: &str,
    lease_epoch: u64,
    error: &str,
    retry_at_utc: &str,
    updated_at_utc: &str,
) -> Result<(), SchedulerError> {
    transition_lease(
        path,
        job_id,
        worker,
        lease_epoch,
        "UPDATE scheduler_jobs SET state=CASE WHEN attempt_count>=max_attempts
             THEN 'needs_review' ELSE 'retry_wait' END,last_error=?1,
             next_retry_at_utc=?2,lease_owner=NULL,lease_expires_at_utc=NULL,
             updated_at_utc=?3 WHERE scheduler_job_id=?4 AND state='running'
             AND lease_owner=?5 AND lease_epoch=?6",
        params![
            error,
            retry_at_utc,
            updated_at_utc,
            job_id,
            worker,
            lease_epoch
        ],
    )
}

pub fn request_scheduler_cancellation(
    path: impl AsRef<Path>,
    job_id: &str,
    updated_at_utc: &str,
) -> Result<SchedulerJob, SchedulerError> {
    let mut connection = open(path.as_ref())?;
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let changed = tx.execute(
        "UPDATE scheduler_jobs SET cancellation_requested=1,
             state=CASE WHEN state IN ('queued','retry_wait') THEN 'cancelled' ELSE state END,
             updated_at_utc=?1 WHERE scheduler_job_id=?2
             AND state NOT IN ('completed','cancelled','needs_review')",
        params![updated_at_utc, job_id],
    )?;
    if changed == 0 {
        let existing = read_job(&tx, job_id)?;
        tx.commit()?;
        return Ok(existing);
    }
    let job = read_job(&tx, job_id)?;
    tx.commit()?;
    Ok(job)
}

fn eligible_classes(
    tx: &Transaction<'_>,
    now: &str,
    required_class: Option<&str>,
) -> Result<Vec<EligibleClass>, SchedulerError> {
    let mut statement = tx.prepare(
        "SELECT c.work_class,c.weight,c.last_served_sequence
         FROM scheduler_classes c
         WHERE (?2='' OR c.work_class=?2)
         AND EXISTS (
             SELECT 1 FROM scheduler_jobs q WHERE q.work_class=c.work_class
               AND q.state IN ('queued','retry_wait') AND q.cancellation_requested=0
               AND q.attempt_count<q.max_attempts
               AND (q.next_retry_at_utc IS NULL OR q.next_retry_at_utc<=?1)
               AND q.byte_cost <= c.max_active_bytes-COALESCE((
                   SELECT SUM(a.byte_cost) FROM scheduler_jobs a
                   WHERE a.work_class=c.work_class AND a.state='running'
               ),0)
         )
         AND (SELECT COUNT(*) FROM scheduler_jobs a
              WHERE a.work_class=c.work_class AND a.state='running')<c.max_active_jobs
         AND COALESCE((SELECT SUM(a.byte_cost) FROM scheduler_jobs a
              WHERE a.work_class=c.work_class AND a.state='running'),0)<c.max_active_bytes",
    )?;
    let classes = statement
        .query_map(params![now, required_class.unwrap_or("")], |row| {
            Ok(EligibleClass {
                work_class: row.get(0)?,
                weight: row.get(1)?,
                last_served: row.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(classes)
}

fn recover_expired_tx(tx: &Transaction<'_>, now: &str) -> Result<(), SchedulerError> {
    tx.execute(
        "UPDATE scheduler_jobs SET state=CASE
             WHEN cancellation_requested=1 THEN 'needs_review'
             WHEN attempt_count>=max_attempts THEN 'needs_review'
             ELSE 'retry_wait' END,
             next_retry_at_utc=CASE WHEN cancellation_requested=0
                  AND attempt_count<max_attempts THEN ?1 ELSE next_retry_at_utc END,
             lease_owner=NULL,lease_expires_at_utc=NULL,
             last_error='worker lease expired before a terminal checkpoint',
             updated_at_utc=?1
         WHERE state='running' AND lease_expires_at_utc<=?1",
        [now],
    )?;
    Ok(())
}

fn transition_lease(
    path: impl AsRef<Path>,
    job_id: &str,
    _worker: &str,
    _lease_epoch: u64,
    sql: &str,
    parameters: impl rusqlite::Params,
) -> Result<(), SchedulerError> {
    let connection = open(path.as_ref())?;
    if connection.execute(sql, parameters)? != 1 {
        return Err(SchedulerError::LeaseConflict(job_id.to_string()));
    }
    Ok(())
}

fn read_job_by_idempotency(
    tx: &Transaction<'_>,
    key: &str,
) -> Result<SchedulerJob, SchedulerError> {
    let id: String = tx.query_row(
        "SELECT scheduler_job_id FROM scheduler_jobs WHERE idempotency_key=?1",
        [key],
        |row| row.get(0),
    )?;
    read_job(tx, &id)
}

fn read_job(tx: &Transaction<'_>, id: &str) -> Result<SchedulerJob, SchedulerError> {
    let tuple = tx.query_row(
        "SELECT scheduler_job_id,idempotency_key,request_digest,work_class,origin,
                store_id,object_id,state,priority,byte_cost,acknowledgement_policy,
                required_copy_count,cancellation_requested,attempt_count,max_attempts,
                next_retry_at_utc,lease_owner,lease_epoch,lease_expires_at_utc,last_error,
                created_at_utc,updated_at_utc
         FROM scheduler_jobs WHERE scheduler_job_id=?1",
        [id],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, Option<String>>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, i32>(8)?,
                r.get::<_, u64>(9)?,
                r.get::<_, String>(10)?,
                r.get::<_, u8>(11)?,
                r.get::<_, bool>(12)?,
                r.get::<_, u32>(13)?,
                r.get::<_, u32>(14)?,
                r.get::<_, Option<String>>(15)?,
                r.get::<_, Option<String>>(16)?,
                r.get::<_, u64>(17)?,
                r.get::<_, Option<String>>(18)?,
                r.get::<_, Option<String>>(19)?,
                r.get::<_, String>(20)?,
                r.get::<_, String>(21)?,
            ))
        },
    )?;
    Ok(SchedulerJob {
        scheduler_job_id: tuple.0,
        idempotency_key: tuple.1,
        request_digest: tuple.2,
        work_class: tuple.3,
        origin: tuple.4,
        store_id: StoreId::new(tuple.5).map_err(|_| SchedulerError::Invalid("store_id"))?,
        object_id: tuple
            .6
            .map(ObjectId::new)
            .transpose()
            .map_err(|_| SchedulerError::Invalid("object_id"))?,
        state: tuple.7,
        priority: tuple.8,
        byte_cost: tuple.9,
        acknowledgement_policy: tuple.10,
        required_copy_count: tuple.11,
        cancellation_requested: tuple.12,
        attempt_count: tuple.13,
        max_attempts: tuple.14,
        next_retry_at_utc: tuple.15,
        lease_owner: tuple.16,
        lease_epoch: tuple.17,
        lease_expires_at_utc: tuple.18,
        last_error: tuple.19,
        created_at_utc: tuple.20,
        updated_at_utc: tuple.21,
    })
}

fn job_matches_request(job: &SchedulerJob, request: &SchedulerJobRequest) -> bool {
    job.scheduler_job_id == request.scheduler_job_id
        && job.request_digest == request.request_digest
        && job.work_class == request.class.work_class
        && job.origin == request.origin
        && job.store_id == request.store_id
        && job.object_id == request.object_id
        && job.priority == request.priority
        && job.byte_cost == request.byte_cost
        && job.acknowledgement_policy == request.acknowledgement_policy
        && job.required_copy_count == request.required_copy_count
        && job.max_attempts == request.max_attempts
}

fn validate_submit(request: &SchedulerJobRequest) -> Result<(), SchedulerError> {
    if [
        request.scheduler_job_id.as_str(),
        request.idempotency_key.as_str(),
        request.request_digest.as_str(),
        request.class.work_class.as_str(),
        request.origin.as_str(),
        request.acknowledgement_policy.as_str(),
        request.created_at_utc.as_str(),
    ]
    .iter()
    .any(|value| value.trim().is_empty())
        || request.class.weight == 0
        || request.class.max_active_jobs == 0
        || request.class.max_active_bytes == 0
        || request.byte_cost == 0
        || request.required_copy_count == 0
        || request.max_attempts == 0
    {
        return Err(SchedulerError::Invalid("submission"));
    }
    Ok(())
}

fn open(path: &Path) -> Result<Connection, SchedulerError> {
    let connection = Connection::open(path)?;
    connection.busy_timeout(BUSY_TIMEOUT)?;
    connection.execute_batch(LIVE_SCHEMA_SQL)?;
    connection.execute_batch(SCHEDULER_SCHEMA_SQL)?;
    Ok(connection)
}

fn weighted_sequence(sequence: u64, weight: u32) -> u128 {
    (u128::from(sequence) * 1_000_000) / u128::from(weight.max(1))
}

fn to_i64(value: u64) -> Result<i64, SchedulerError> {
    i64::try_from(value).map_err(|_| SchedulerError::Invalid("byte count"))
}

struct EligibleClass {
    work_class: String,
    weight: u32,
    last_served: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LIVE_SCHEMA_SQL;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture(name: &str) -> (PathBuf, StoreId) {
        let path = std::env::temp_dir().join(format!(
            "dasobjectstore-scheduler-{name}-{}-{}.sqlite",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch(LIVE_SCHEMA_SQL).unwrap();
        connection
            .execute(
                "INSERT INTO pools VALUES('pool','Ready',?1,?1)",
                ["2026-01-01T00:00:00Z"],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO stores VALUES('store','pool','generated_data','{}',?1,?1)",
                ["2026-01-01T00:00:00Z"],
            )
            .unwrap();
        (path, StoreId::new("store").unwrap())
    }

    fn request(path: &Path, store: &StoreId, id: &str, class: &str) -> SchedulerJobRequest {
        SchedulerJobRequest {
            live_sqlite_path: path.to_path_buf(),
            scheduler_job_id: id.into(),
            idempotency_key: format!("key-{id}"),
            request_digest: format!("digest-{id}"),
            class: SchedulerClassPolicy {
                work_class: class.into(),
                weight: if class == "direct_s3" { 2 } else { 1 },
                max_active_jobs: 2,
                max_active_bytes: 1_000,
            },
            origin: class.into(),
            store_id: store.clone(),
            object_id: None,
            priority: 0,
            byte_cost: 10,
            acknowledgement_policy: "after_ssd_ingest".into(),
            required_copy_count: 1,
            max_attempts: 3,
            created_at_utc: "2026-01-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn submission_replays_and_rejects_digest_conflicts() {
        let (path, store) = fixture("replay");
        let request = request(&path, &store, "a", "native");
        assert_eq!(
            submit_scheduler_job(&request).unwrap(),
            submit_scheduler_job(&request).unwrap()
        );
        let mut conflict = request;
        conflict.request_digest = "different".into();
        assert!(matches!(
            submit_scheduler_job(&conflict),
            Err(SchedulerError::Conflict(_))
        ));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn fairness_and_lease_fencing_survive_reopen() {
        let (path, store) = fixture("fairness");
        submit_scheduler_job(&request(&path, &store, "native-a", "native")).unwrap();
        submit_scheduler_job(&request(&path, &store, "s3-a", "direct_s3")).unwrap();
        let claim = SchedulerClaimRequest {
            live_sqlite_path: path.clone(),
            worker: "worker".into(),
            now_utc: "2026-01-01T00:01:00Z".into(),
            lease_expires_at_utc: "2026-01-01T00:02:00Z".into(),
        };
        let first = claim_next_scheduler_job(&claim).unwrap().unwrap();
        complete_scheduler_job(
            &path,
            &first.scheduler_job_id,
            "worker",
            first.lease_epoch,
            "2026-01-01T00:01:01Z",
        )
        .unwrap();
        let second = claim_next_scheduler_job(&claim).unwrap().unwrap();
        assert_ne!(first.work_class, second.work_class);
        assert!(matches!(
            complete_scheduler_job(
                &path,
                &second.scheduler_job_id,
                "worker",
                second.lease_epoch + 1,
                "2026-01-01T00:01:01Z"
            ),
            Err(SchedulerError::LeaseConflict(_))
        ));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn store_round_robin_cursor_survives_restart() {
        let (path, store_a) = fixture("store-fairness");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute(
                "INSERT INTO stores VALUES('store-b','pool','generated_data','{}',?1,?1)",
                ["2026-01-01T00:00:00Z"],
            )
            .unwrap();
        drop(connection);
        let store_b = StoreId::new("store-b").unwrap();
        submit_scheduler_job(&request(&path, &store_a, "a-1", "native")).unwrap();
        submit_scheduler_job(&request(&path, &store_a, "a-2", "native")).unwrap();
        submit_scheduler_job(&request(&path, &store_b, "b-1", "native")).unwrap();
        let claim = SchedulerClaimRequest {
            live_sqlite_path: path.clone(),
            worker: "worker".into(),
            now_utc: "2026-01-01T00:01:00Z".into(),
            lease_expires_at_utc: "2026-01-01T00:02:00Z".into(),
        };
        let first = claim_next_scheduler_job(&claim).unwrap().unwrap();
        complete_scheduler_job(
            &path,
            &first.scheduler_job_id,
            "worker",
            first.lease_epoch,
            "2026-01-01T00:01:01Z",
        )
        .unwrap();
        drop(Connection::open(&path).unwrap());
        let second = claim_next_scheduler_job(&claim).unwrap().unwrap();
        assert_ne!(first.store_id, second.store_id);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn concurrent_claims_obey_class_active_limit() {
        let (path, store) = fixture("concurrent");
        let mut first = request(&path, &store, "one", "native");
        first.class.max_active_jobs = 1;
        let mut second = request(&path, &store, "two", "native");
        second.class.max_active_jobs = 1;
        submit_scheduler_job(&first).unwrap();
        submit_scheduler_job(&second).unwrap();
        let handles = ["worker-a", "worker-b"].map(|worker| {
            let path = path.clone();
            std::thread::spawn(move || {
                claim_next_scheduler_job(&SchedulerClaimRequest {
                    live_sqlite_path: path,
                    worker: worker.into(),
                    now_utc: "2026-01-01T00:01:00Z".into(),
                    lease_expires_at_utc: "2026-01-01T00:02:00Z".into(),
                })
                .unwrap()
            })
        });
        let claimed = handles
            .into_iter()
            .filter_map(|handle| handle.join().unwrap())
            .count();
        assert_eq!(claimed, 1);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn claim_selects_a_fitting_job_instead_of_oversized_higher_priority_work() {
        let (path, store) = fixture("byte-fit");
        let mut running = request(&path, &store, "running", "native");
        running.class.max_active_bytes = 100;
        running.byte_cost = 60;
        running.priority = 200;
        let mut oversized = request(&path, &store, "oversized", "native");
        oversized.class.max_active_bytes = 100;
        oversized.byte_cost = 50;
        oversized.priority = 100;
        let mut fitting = request(&path, &store, "fitting", "native");
        fitting.class.max_active_bytes = 100;
        fitting.byte_cost = 40;
        submit_scheduler_job(&running).unwrap();
        submit_scheduler_job(&oversized).unwrap();
        submit_scheduler_job(&fitting).unwrap();
        let claim = SchedulerClaimRequest {
            live_sqlite_path: path.clone(),
            worker: "worker-a".into(),
            now_utc: "2026-01-01T00:01:00Z".into(),
            lease_expires_at_utc: "2026-01-01T00:02:00Z".into(),
        };
        assert_eq!(
            claim_next_scheduler_job(&claim)
                .unwrap()
                .unwrap()
                .scheduler_job_id,
            "running"
        );
        let second = claim_next_scheduler_job(&SchedulerClaimRequest {
            worker: "worker-b".into(),
            ..claim
        })
        .unwrap()
        .unwrap();
        assert_eq!(second.scheduler_job_id, "fitting");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn durable_submission_classes_are_origin_derived_and_weighted() {
        assert_eq!(scheduler_policy_for_origin("remote_s3").0, "direct_s3");
        assert_eq!(
            scheduler_policy_for_origin("remote_reconcile").0,
            "remote_reconcile"
        );
        assert_eq!(
            scheduler_policy_for_origin("workspace_promotion").0,
            "verification_repair"
        );
        assert_eq!(
            scheduler_policy_for_origin("native_ingest"),
            ("native_ingest", 4, 4, 2_u64 << 40)
        );
        assert_eq!(scheduler_policy_for_origin("destage").0, "destage");
    }

    #[test]
    fn production_destage_submissions_receive_cross_class_weighted_service() {
        let (path, store) = fixture("production-class-fairness");
        for (prefix, origin, count) in [
            ("s3", "remote_s3", 8_u8),
            ("repair", "workspace_promotion", 2_u8),
        ] {
            for index in 0..count {
                let object = ObjectId::new(format!("{prefix}-{index}")).unwrap();
                crate::destage::commit_verified_ssd_and_enqueue(
                    &path,
                    crate::destage::VerifiedSsdCommitRequest {
                        destage_job_id: &format!("destage-{prefix}-{index}"),
                        store_id: &store,
                        object_id: &object,
                        object_type: "naive",
                        relative_path: &format!("ingest/{prefix}-{index}"),
                        size_bytes: 10,
                        content_hash_algorithm: "sha256",
                        content_hash: "hash",
                        acknowledgement_policy: "after_ssd_ingest",
                        required_copy_count: 1,
                        max_attempts: 3,
                        priority: 0,
                        committed_at_utc: "2026-01-01T00:00:00Z",
                        ingest_job_id: Some(&format!("ingest-{prefix}-{index}")),
                        ingress_origin: Some(origin),
                        s3_key: None,
                        s3_version: 1,
                    },
                )
                .unwrap();
            }
        }
        let mut classes = Vec::new();
        for index in 0..6 {
            let claimed = claim_next_scheduler_job(&SchedulerClaimRequest {
                live_sqlite_path: path.clone(),
                worker: "worker".into(),
                now_utc: format!("2026-01-01T00:01:{index:02}Z"),
                lease_expires_at_utc: "2026-01-01T00:10:00Z".into(),
            })
            .unwrap()
            .unwrap();
            classes.push(claimed.work_class.clone());
            complete_scheduler_job(
                &path,
                &claimed.scheduler_job_id,
                "worker",
                claimed.lease_epoch,
                "2026-01-01T00:01:59Z",
            )
            .unwrap();
        }
        assert!(classes.contains(&"verification_repair".to_string()));
        assert!(
            classes
                .iter()
                .filter(|class| class.as_str() == "direct_s3")
                .count()
                > classes
                    .iter()
                    .filter(|class| class.as_str() == "verification_repair")
                    .count()
        );
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn cancellation_is_durable_and_expired_running_work_needs_review() {
        let (path, store) = fixture("cancel");
        submit_scheduler_job(&request(&path, &store, "cancelled", "native")).unwrap();
        let cancelled =
            request_scheduler_cancellation(&path, "cancelled", "2026-01-01T00:01:00Z").unwrap();
        assert_eq!(cancelled.state, "cancelled");
        submit_scheduler_job(&request(&path, &store, "expired", "native")).unwrap();
        let claimed = claim_next_scheduler_job(&SchedulerClaimRequest {
            live_sqlite_path: path.clone(),
            worker: "worker".into(),
            now_utc: "2026-01-01T00:01:00Z".into(),
            lease_expires_at_utc: "2026-01-01T00:02:00Z".into(),
        })
        .unwrap()
        .unwrap();
        request_scheduler_cancellation(&path, &claimed.scheduler_job_id, "2026-01-01T00:01:30Z")
            .unwrap();
        let _ = claim_next_scheduler_job(&SchedulerClaimRequest {
            live_sqlite_path: path.clone(),
            worker: "other".into(),
            now_utc: "2026-01-01T00:03:00Z".into(),
            lease_expires_at_utc: "2026-01-01T00:04:00Z".into(),
        })
        .unwrap();
        let connection = Connection::open(&path).unwrap();
        let state: String = connection
            .query_row(
                "SELECT state FROM scheduler_jobs WHERE scheduler_job_id='expired'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(state, "needs_review");
        fs::remove_file(path).unwrap();
    }
}
