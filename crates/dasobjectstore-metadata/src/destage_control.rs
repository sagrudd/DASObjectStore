//! Transactional control and lease renewal for durable HDD settlement.

use crate::destage::DestageMetadataError;
use crate::scheduler::{
    pause_destage_scheduler_job_tx, resume_destage_scheduler_job_tx,
    retry_destage_scheduler_job_tx, SchedulerError,
};
use crate::schema::LIVE_SCHEMA_SQL;
use dasobjectstore_core::ids::{ObjectId, StoreId};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The OS error fragment used to identify terminal jobs that became
/// `needs_review` solely because no HDD could accept the copy at that time.
///
/// This is intentionally narrower than a general destage failure: checksum,
/// path, policy, cancellation, and metadata failures require operator review.
pub const CAPACITY_BLOCKED_DESTAGE_ERROR_FRAGMENT: &str = "No space left on device";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DestageRetryReport {
    pub store_id: StoreId,
    pub from_state: String,
    pub matched_object_ids: Vec<ObjectId>,
    pub retried_object_count: usize,
    pub dry_run: bool,
}

pub fn renew_destage_and_scheduler_leases(
    path: impl AsRef<Path>,
    object_id: &ObjectId,
    scheduler_job_id: &str,
    worker: &str,
    scheduler_lease_epoch: u64,
    lease_expires_at_utc: &str,
    updated_at_utc: &str,
) -> Result<(), DestageMetadataError> {
    if worker.trim().is_empty() {
        return Err(DestageMetadataError::BlankField("worker"));
    }
    let mut connection = Connection::open(path)?;
    connection.execute_batch(LIVE_SCHEMA_SQL)?;
    let tx = connection.transaction()?;
    let scheduler_changed = tx.execute(
        "UPDATE scheduler_jobs SET lease_expires_at_utc=?1,updated_at_utc=?2
         WHERE scheduler_job_id=?3 AND state='running' AND lease_owner=?4
           AND lease_epoch=?5",
        params![
            lease_expires_at_utc,
            updated_at_utc,
            scheduler_job_id,
            worker,
            scheduler_lease_epoch
        ],
    )?;
    if scheduler_changed != 1 {
        return Err(DestageMetadataError::ClaimConflict);
    }
    let destage_changed = tx.execute(
        "UPDATE destage_queue SET lease_expires_at_utc=?1,updated_at_utc=?2
         WHERE object_id=?3 AND state='hdd_copying' AND lease_owner=?4",
        params![
            lease_expires_at_utc,
            updated_at_utc,
            object_id.as_str(),
            worker
        ],
    )?;
    if destage_changed != 1 {
        return Err(DestageMetadataError::ClaimConflict);
    }
    tx.commit()?;
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
        false,
        pause_destage_scheduler_job_tx,
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
        false,
        resume_destage_scheduler_job_tx,
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
        true,
        retry_destage_scheduler_job_tx,
    )
}

/// Atomically requeue every exhausted destage job for one store.
///
/// The fixed `needs_review` predicate is deliberate: this operator action
/// cannot disturb active, paused, settled, or cancelled work. The destage and
/// scheduler rows are reset in the same transaction for every selected object.
pub fn retry_needs_review_destage_for_store(
    path: impl AsRef<Path>,
    store_id: &StoreId,
    updated_at_utc: &str,
    dry_run: bool,
) -> Result<DestageRetryReport, DestageMetadataError> {
    let mut connection = Connection::open(path)?;
    connection.execute_batch(LIVE_SCHEMA_SQL)?;
    let tx = connection.transaction()?;
    let matched_object_ids = {
        let mut statement = tx.prepare(
            "SELECT object_id FROM destage_queue
             WHERE store_id=?1 AND state='needs_review'
             ORDER BY created_at_utc,destage_job_id",
        )?;
        let values = statement
            .query_map([store_id.as_str()], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        values
            .into_iter()
            .map(|value| ObjectId::new(value).map_err(|_| DestageMetadataError::InvalidIdentifier))
            .collect::<Result<Vec<_>, _>>()?
    };

    if !dry_run {
        for object_id in &matched_object_ids {
            control_tx(
                &tx,
                object_id,
                "queued_for_hdd",
                updated_at_utc,
                "state='needs_review'",
                true,
                retry_destage_scheduler_job_tx,
            )?;
        }
    }
    tx.commit()?;
    Ok(DestageRetryReport {
        store_id: store_id.clone(),
        from_state: "needs_review".to_string(),
        retried_object_count: if dry_run { 0 } else { matched_object_ids.len() },
        matched_object_ids,
        dry_run,
    })
}

/// Requeue one terminal destage job that failed solely because the enclosure
/// had no HDD capacity at the time.
///
/// This is daemon-owned housekeeping rather than a user mutation. It resets
/// only an un-cancelled `needs_review` row whose recorded failure has the
/// narrow capacity-exhaustion signature. The durable destage worker still
/// verifies the SSD payload, selects a currently eligible HDD, checksums the
/// copied bytes, and deletes the SSD source only after settlement succeeds.
pub fn retry_one_capacity_blocked_destage(
    path: impl AsRef<Path>,
    updated_at_utc: &str,
) -> Result<Option<ObjectId>, DestageMetadataError> {
    let mut connection = Connection::open(path)?;
    connection.execute_batch(LIVE_SCHEMA_SQL)?;
    let tx = connection.transaction()?;
    let object_id = tx
        .query_row(
            "SELECT object_id FROM destage_queue
             WHERE state='needs_review' AND cancellation_requested=0
               AND last_error LIKE '%' || ?1 || '%'
             ORDER BY created_at_utc,destage_job_id LIMIT 1",
            [CAPACITY_BLOCKED_DESTAGE_ERROR_FRAGMENT],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(object_id) = object_id else {
        tx.commit()?;
        return Ok(None);
    };
    let object_id =
        ObjectId::new(object_id).map_err(|_| DestageMetadataError::InvalidIdentifier)?;
    let changed = tx.execute(
        "UPDATE destage_queue SET state='queued_for_hdd',next_retry_at_utc=NULL,
             lease_owner=NULL,lease_expires_at_utc=NULL,attempt_count=0,
             updated_at_utc=?1 WHERE object_id=?2 AND state='needs_review'
               AND cancellation_requested=0 AND last_error LIKE '%' || ?3 || '%'",
        params![
            updated_at_utc,
            object_id.as_str(),
            CAPACITY_BLOCKED_DESTAGE_ERROR_FRAGMENT
        ],
    )?;
    if changed != 1 {
        return Err(DestageMetadataError::InvalidTransition);
    }
    retry_destage_scheduler_job_tx(&tx, &object_id, updated_at_utc)?;
    tx.commit()?;
    Ok(Some(object_id))
}

fn control(
    path: impl AsRef<Path>,
    object_id: &ObjectId,
    state: &str,
    at: &str,
    predicate: &str,
    reset_attempts: bool,
    scheduler_control: fn(&Transaction<'_>, &ObjectId, &str) -> Result<(), SchedulerError>,
) -> Result<(), DestageMetadataError> {
    let mut connection = Connection::open(path)?;
    connection.execute_batch(LIVE_SCHEMA_SQL)?;
    let tx = connection.transaction()?;
    control_tx(
        &tx,
        object_id,
        state,
        at,
        predicate,
        reset_attempts,
        scheduler_control,
    )?;
    tx.commit()?;
    Ok(())
}

fn control_tx(
    tx: &Transaction<'_>,
    object_id: &ObjectId,
    state: &str,
    at: &str,
    predicate: &str,
    reset_attempts: bool,
    scheduler_control: fn(&Transaction<'_>, &ObjectId, &str) -> Result<(), SchedulerError>,
) -> Result<(), DestageMetadataError> {
    let sql = format!(
        "UPDATE destage_queue SET state=?1,next_retry_at_utc=NULL,
         lease_owner=NULL,lease_expires_at_utc=NULL,
         attempt_count=CASE WHEN ?4 THEN 0 ELSE attempt_count END,
         updated_at_utc=?2 WHERE object_id=?3 AND {predicate}"
    );
    if tx.execute(&sql, params![state, at, object_id.as_str(), reset_attempts])? != 1 {
        return Err(DestageMetadataError::InvalidTransition);
    }
    scheduler_control(tx, object_id, at)?;
    Ok(())
}
