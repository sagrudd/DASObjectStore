//! Transactional control and lease renewal for durable HDD settlement.

use crate::destage::DestageMetadataError;
use crate::scheduler::{
    pause_destage_scheduler_job_tx, resume_destage_scheduler_job_tx,
    retry_destage_scheduler_job_tx, SchedulerError,
};
use crate::schema::LIVE_SCHEMA_SQL;
use dasobjectstore_core::ids::ObjectId;
use rusqlite::{params, Connection, Transaction};
use std::path::Path;

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
    let sql = format!(
        "UPDATE destage_queue SET state=?1,next_retry_at_utc=NULL,
         lease_owner=NULL,lease_expires_at_utc=NULL,
         attempt_count=CASE WHEN ?4 THEN 0 ELSE attempt_count END,
         updated_at_utc=?2 WHERE object_id=?3 AND {predicate}"
    );
    if tx.execute(&sql, params![state, at, object_id.as_str(), reset_attempts])? != 1 {
        return Err(DestageMetadataError::InvalidTransition);
    }
    scheduler_control(&tx, object_id, at)?;
    tx.commit()?;
    Ok(())
}
