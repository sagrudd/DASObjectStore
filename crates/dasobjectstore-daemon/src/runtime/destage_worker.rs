//! Restart-safe execution of durable managed-SSD to HDD placement work.

use crate::runtime::ingest_files::discover_managed_hdd_roots;
use dasobjectstore_core::ids::ObjectId;
use dasobjectstore_core::object_type::ObjectType;
use dasobjectstore_core::utc::{add_seconds_to_utc_timestamp, format_utc_timestamp_seconds};
use dasobjectstore_metadata::{
    acquire_disk_capacity_claims, backfill_destage_scheduler_jobs, claim_destage_for_scheduler,
    claim_next_scheduler_job, complete_scheduler_job, fail_destage, list_ssd_eviction_candidates,
    mark_ssd_evicted, measure_ssd_capacity, promote_hdd_settlement, read_disk_capacity_claims,
    read_outstanding_disk_capacity_excluding, read_settlement_eligible_disk_ids,
    read_ssd_placement, renew_destage_and_scheduler_leases, retry_scheduler_job,
    settle_staged_object_to_hdd_preserving_ssd_with_controlled_progress, DestageMetadataError,
    DestageQueueRecord, DiskCapacityClaimAllocation, DiskCapacityClaimError, DiskCapacityClaimKind,
    DiskCapacityClaimRequest, HddSettlementPromotionRequest, ObjectPutError, SchedulerClaimRequest,
    SchedulerError, StagedObjectPut, VerifiedHddPlacement,
};
use sha2::{Digest, Sha256};
use std::fmt::{self, Display};
use std::fs::{self, File};
use std::path::{Component, Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const DEFAULT_DESTAGE_LEASE_SECONDS: u64 = 60 * 60;
const DESTAGE_LEASE_RENEWAL_SECONDS: u64 = DEFAULT_DESTAGE_LEASE_SECONDS / 3;
pub const MAX_DESTAGE_RETRY_SECONDS: u64 = 60 * 60;

#[derive(Clone, Debug)]
pub struct DurableDestageWorkerConfig {
    pub live_sqlite_path: PathBuf,
    pub ssd_root: PathBuf,
    pub hdd_root: PathBuf,
    pub worker_id: String,
}

impl DurableDestageWorkerConfig {
    pub fn from_environment(worker_id: impl Into<String>) -> Self {
        let ssd_root = std::env::var_os("DASOBJECTSTORE_SSD_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/srv/dasobjectstore/ssd"));
        let hdd_root = std::env::var_os("DASOBJECTSTORE_HDD_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/srv/dasobjectstore/hdd"));
        let live_sqlite_path = ssd_root.join(".dasobjectstore/live.sqlite");
        Self {
            live_sqlite_path,
            ssd_root,
            hdd_root,
            worker_id: worker_id.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DurableDestageOutcome {
    Idle,
    Evicted {
        object_id: ObjectId,
    },
    Settled {
        store_id: dasobjectstore_core::ids::StoreId,
        object_id: ObjectId,
        copies: u8,
    },
    Deferred {
        object_id: ObjectId,
        message: String,
    },
}

pub fn run_one_durable_destage(
    config: &DurableDestageWorkerConfig,
    now_utc: &str,
    previously_served_store: Option<&dasobjectstore_core::ids::StoreId>,
) -> Result<DurableDestageOutcome, DurableDestageWorkerError> {
    let lease_expires_at_utc = add_seconds_to_utc_timestamp(now_utc, DEFAULT_DESTAGE_LEASE_SECONDS)
        .ok_or_else(|| DurableDestageWorkerError::InvalidTimestamp(now_utc.to_string()))?;
    let _ = previously_served_store;
    backfill_destage_scheduler_jobs(&config.live_sqlite_path, now_utc)?;
    let Some(scheduled) = claim_next_scheduler_job(&SchedulerClaimRequest {
        live_sqlite_path: config.live_sqlite_path.clone(),
        worker: config.worker_id.clone(),
        now_utc: now_utc.to_string(),
        lease_expires_at_utc: lease_expires_at_utc.clone(),
    })?
    else {
        return evict_one_settled_ssd_copy(config, now_utc);
    };
    let object_id = scheduled.object_id.clone().ok_or_else(|| {
        DurableDestageWorkerError::SchedulerJobMissingObject(scheduled.scheduler_job_id.clone())
    })?;
    let record = match claim_destage_for_scheduler(
        &config.live_sqlite_path,
        &object_id,
        &config.worker_id,
        &lease_expires_at_utc,
        now_utc,
    ) {
        Ok(record) => record,
        Err(error) => {
            let retry_at = add_seconds_to_utc_timestamp(now_utc, 30)
                .ok_or_else(|| DurableDestageWorkerError::InvalidTimestamp(now_utc.to_string()))?;
            retry_scheduler_job(
                &config.live_sqlite_path,
                &scheduled.scheduler_job_id,
                &config.worker_id,
                scheduled.lease_epoch,
                &error.to_string(),
                &retry_at,
                now_utc,
            )?;
            return Err(error.into());
        }
    };

    match settle_claimed_record(
        config,
        &record,
        &scheduled.scheduler_job_id,
        scheduled.lease_epoch,
        now_utc,
        &lease_expires_at_utc,
    ) {
        Ok(copies) => {
            complete_scheduler_job(
                &config.live_sqlite_path,
                &scheduled.scheduler_job_id,
                &config.worker_id,
                scheduled.lease_epoch,
                now_utc,
            )?;
            Ok(DurableDestageOutcome::Settled {
                store_id: record.store_id.clone(),
                object_id: record.object_id,
                copies,
            })
        }
        Err(error) => {
            let retry_at =
                add_seconds_to_utc_timestamp(now_utc, retry_delay_seconds(record.attempt_count))
                    .ok_or_else(|| {
                        DurableDestageWorkerError::InvalidTimestamp(now_utc.to_string())
                    })?;
            fail_destage(
                &config.live_sqlite_path,
                &record.object_id,
                &config.worker_id,
                &error.to_string(),
                Some(&retry_at),
                now_utc,
            )?;
            retry_scheduler_job(
                &config.live_sqlite_path,
                &scheduled.scheduler_job_id,
                &config.worker_id,
                scheduled.lease_epoch,
                &error.to_string(),
                &retry_at,
                now_utc,
            )?;
            Ok(DurableDestageOutcome::Deferred {
                object_id: record.object_id,
                message: error.to_string(),
            })
        }
    }
}

fn evict_one_settled_ssd_copy(
    config: &DurableDestageWorkerConfig,
    now_utc: &str,
) -> Result<DurableDestageOutcome, DurableDestageWorkerError> {
    let Some(candidate) = list_ssd_eviction_candidates(&config.live_sqlite_path, 1)?
        .into_iter()
        .next()
    else {
        return Ok(DurableDestageOutcome::Idle);
    };
    let relative = safe_relative_path(&candidate.relative_path).ok_or_else(|| {
        DurableDestageWorkerError::UnsafeSsdPlacement(candidate.relative_path.clone())
    })?;
    let payload = config.ssd_root.join(relative);
    let job_root = payload.parent().ok_or_else(|| {
        DurableDestageWorkerError::UnsafeSsdPlacement(candidate.relative_path.clone())
    })?;
    match fs::symlink_metadata(&payload) {
        Ok(_)
            if job_root.parent()
                == Some(
                    config
                        .ssd_root
                        .join(".dasobjectstore/ingest/jobs")
                        .as_path(),
                ) =>
        {
            remove_managed_ssd_job_root(&config.ssd_root, job_root)?;
        }
        Ok(_) => {
            remove_managed_direct_s3_payload(&config.ssd_root, &candidate.store_id, &payload)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    mark_ssd_evicted(&config.live_sqlite_path, &candidate.object_id, now_utc)?;
    Ok(DurableDestageOutcome::Evicted {
        object_id: candidate.object_id,
    })
}

fn settle_claimed_record(
    config: &DurableDestageWorkerConfig,
    record: &DestageQueueRecord,
    scheduler_job_id: &str,
    scheduler_lease_epoch: u64,
    now_utc: &str,
    lease_expires_at_utc: &str,
) -> Result<u8, DurableDestageWorkerError> {
    let ssd = read_ssd_placement(&config.live_sqlite_path, &record.object_id)?
        .ok_or_else(|| DurableDestageWorkerError::MissingSsdPlacement(record.object_id.clone()))?;
    if ssd.evicted_at_utc.is_some() {
        return Err(DurableDestageWorkerError::MissingSsdPlacement(
            record.object_id.clone(),
        ));
    }
    let relative_path = safe_relative_path(&ssd.relative_path)
        .ok_or_else(|| DurableDestageWorkerError::UnsafeSsdPlacement(ssd.relative_path.clone()))?;
    let payload_path = config.ssd_root.join(relative_path);
    let metadata = fs::metadata(&payload_path)?;
    if !metadata.is_file() || metadata.len() != record.expected_size_bytes {
        return Err(DurableDestageWorkerError::SsdPayloadMismatch {
            object_id: record.object_id.clone(),
            expected: record.expected_size_bytes,
            actual: metadata.len(),
        });
    }
    let object_type = parse_queued_object_type(&record.object_type)?;
    let held_claims = read_disk_capacity_claims(
        &config.live_sqlite_path,
        DiskCapacityClaimKind::Destage,
        record.object_id.as_str(),
    )?
    .into_iter()
    .filter(|claim| claim.state == "active")
    .collect::<Vec<_>>();
    let roots = if held_claims.is_empty() {
        // Compatibility for SSD acknowledgements created before reservations
        // became mandatory. New publication paths reserve before returning.
        let roots = select_managed_hdd_roots_with_capacity(
            &config.live_sqlite_path,
            &config.hdd_root,
            record.required_copy_count,
            record.expected_size_bytes,
            Some(record.object_id.as_str()),
        )?;
        acquire_destage_claims(config, record, now_utc, lease_expires_at_utc, &roots)?;
        roots
    } else {
        roots_for_held_claims(config, record, &held_claims)?
    };
    let job_root = payload_path
        .parent()
        .ok_or_else(|| DurableDestageWorkerError::UnsafeSsdPlacement(ssd.relative_path.clone()))?
        .to_path_buf();
    let staged = StagedObjectPut {
        object_id: record.object_id.clone(),
        object_type,
        source_path: payload_path.clone(),
        job_root: job_root.clone(),
        staged_payload_path: payload_path,
        bytes_staged: record.expected_size_bytes,
        content_hash_algorithm: record.content_hash_algorithm.clone(),
        content_hash: record.content_hash.clone(),
        disk_roots: roots.clone(),
        copy_count: record.required_copy_count,
    };
    let lease_error = Arc::new(Mutex::new(None::<String>));
    let (stop_tx, stop_rx) = mpsc::channel();
    let report = std::thread::scope(|scope| {
        let heartbeat_lease_error = Arc::clone(&lease_error);
        scope.spawn(move || loop {
            match stop_rx.recv_timeout(Duration::from_secs(DESTAGE_LEASE_RENEWAL_SECONDS)) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let renewed_at = current_utc_timestamp();
                    let Some(expires_at) =
                        add_seconds_to_utc_timestamp(&renewed_at, DEFAULT_DESTAGE_LEASE_SECONDS)
                    else {
                        *heartbeat_lease_error
                            .lock()
                            .expect("lease error lock poisoned") =
                            Some("could not calculate renewed lease expiry".to_string());
                        break;
                    };
                    if let Err(error) = renew_destage_and_scheduler_leases(
                        &config.live_sqlite_path,
                        &record.object_id,
                        scheduler_job_id,
                        &config.worker_id,
                        scheduler_lease_epoch,
                        &expires_at,
                        &renewed_at,
                    ) {
                        *heartbeat_lease_error
                            .lock()
                            .expect("lease error lock poisoned") = Some(error.to_string());
                        break;
                    }
                }
            }
        });
        let result =
            settle_staged_object_to_hdd_preserving_ssd_with_controlled_progress(&staged, |_| {
                if lease_error
                    .lock()
                    .expect("lease error lock poisoned")
                    .is_some()
                {
                    Err(ObjectPutError::Cancelled)
                } else {
                    Ok(())
                }
            });
        let _ = stop_tx.send(());
        result
    });
    if let Some(message) = lease_error
        .lock()
        .expect("lease error lock poisoned")
        .take()
    {
        return Err(DurableDestageWorkerError::LeaseFenceLost {
            object_id: record.object_id.clone(),
            message,
        });
    }
    let report = report?;

    let placement_values = report
        .placements
        .iter()
        .map(|placement| {
            let root = roots
                .iter()
                .find(|root| root.disk_id.as_str() == placement.disk_id)
                .ok_or_else(|| {
                    DurableDestageWorkerError::UnknownPlacementDisk(placement.disk_id.clone())
                })?;
            let relative = placement
                .destination_path
                .strip_prefix(&root.root_path)
                .map_err(|_| {
                    DurableDestageWorkerError::UnsafeHddPlacement(
                        placement.destination_path.clone(),
                    )
                })?
                .to_string_lossy()
                .into_owned();
            Ok((
                placement_id(&record.object_id, &placement.disk_id, &relative),
                placement.disk_id.clone(),
                relative,
                placement.content_hash.clone(),
            ))
        })
        .collect::<Result<Vec<_>, DurableDestageWorkerError>>()?;
    let placements = placement_values
        .iter()
        .map(
            |(placement_id, disk_id, relative_path, content_hash)| VerifiedHddPlacement {
                placement_id,
                disk_id,
                relative_path,
                content_hash,
            },
        )
        .collect::<Vec<_>>();
    promote_hdd_settlement(
        &config.live_sqlite_path,
        HddSettlementPromotionRequest {
            object_id: &record.object_id,
            worker: &config.worker_id,
            placements: &placements,
            verified_at_utc: now_utc,
        },
    )?;

    // Promotion is the durable policy boundary. SSD eviction is deliberately
    // left to the separate eviction pass so a cleanup failure can never turn
    // a successfully settled queue row back into a failed destage attempt.
    Ok(u8::try_from(placements.len()).unwrap_or(u8::MAX))
}

fn acquire_destage_claims(
    config: &DurableDestageWorkerConfig,
    record: &DestageQueueRecord,
    now_utc: &str,
    lease_expires_at_utc: &str,
    roots: &[dasobjectstore_metadata::DiskCopyRoot],
) -> Result<(), DurableDestageWorkerError> {
    acquire_disk_capacity_claims(&DiskCapacityClaimRequest {
        live_sqlite_path: config.live_sqlite_path.clone(),
        kind: DiskCapacityClaimKind::Destage,
        owner_id: record.object_id.as_str().to_string(),
        request_id: format!("destage:{}", record.destage_job_id),
        request_digest: format!(
            "{}:{}:{}:{}",
            record.object_id.as_str(),
            record.expected_size_bytes,
            record.required_copy_count,
            record.content_hash
        ),
        lease_owner: Some(config.worker_id.clone()),
        lease_expires_at_utc: Some(lease_expires_at_utc.to_string()),
        created_at_utc: now_utc.to_string(),
        allocations: roots
            .iter()
            .map(|root| {
                let capacity = measure_ssd_capacity(&root.root_path).map_err(|error| {
                    DurableDestageWorkerError::CapacityMeasurement {
                        disk_id: root.disk_id.as_str().to_string(),
                        message: error.to_string(),
                    }
                })?;
                Ok(DiskCapacityClaimAllocation {
                    disk_id: root.disk_id.clone(),
                    measured_available_bytes: capacity.available_bytes,
                    requested_bytes: record.expected_size_bytes,
                })
            })
            .collect::<Result<Vec<_>, DurableDestageWorkerError>>()?,
    })?;
    Ok(())
}

fn roots_for_held_claims(
    config: &DurableDestageWorkerConfig,
    record: &DestageQueueRecord,
    claims: &[dasobjectstore_metadata::DiskCapacityClaim],
) -> Result<Vec<dasobjectstore_metadata::DiskCopyRoot>, DurableDestageWorkerError> {
    if claims.len() != usize::from(record.required_copy_count)
        || claims.iter().any(|claim| {
            claim.reserved_bytes != record.expected_size_bytes || claim.consumed_bytes != 0
        })
    {
        return Err(DurableDestageWorkerError::InvalidHeldCapacityClaims {
            object_id: record.object_id.clone(),
        });
    }
    let eligible_disk_ids = read_settlement_eligible_disk_ids(&config.live_sqlite_path)?;
    let discovered = discover_managed_hdd_roots(&config.hdd_root)?;
    claims
        .iter()
        .map(|claim| {
            if !eligible_disk_ids.contains(&claim.disk_id) {
                return Err(DurableDestageWorkerError::ReservedDiskIneligible {
                    disk_id: claim.disk_id.as_str().to_string(),
                });
            }
            discovered
                .iter()
                .find(|root| root.disk_id == claim.disk_id)
                .cloned()
                .ok_or_else(|| DurableDestageWorkerError::ReservedDiskUnavailable {
                    disk_id: claim.disk_id.as_str().to_string(),
                })
        })
        .collect()
}

/// Select distinct managed HDD roots that can hold a complete copy before
/// beginning a destage write. Discovery order is not a placement policy:
/// roots are first filtered by complete-file capacity, then ranked by their
/// exact fractional free capacity. Disk identity is only a deterministic
/// tiebreaker when two fractions are equal.
pub(crate) fn select_managed_hdd_roots_with_capacity(
    live_sqlite_path: &Path,
    hdd_root: &Path,
    required_copies: u8,
    required_bytes: u64,
    excluded_destage_owner: Option<&str>,
) -> Result<Vec<dasobjectstore_metadata::DiskCopyRoot>, DurableDestageWorkerError> {
    let eligible_disk_ids = read_settlement_eligible_disk_ids(live_sqlite_path)?;
    let outstanding = read_outstanding_disk_capacity_excluding(
        live_sqlite_path,
        excluded_destage_owner.map(|owner| (DiskCapacityClaimKind::Destage, owner)),
    )?;
    select_hdd_roots_with_capacity(
        discover_managed_hdd_roots(hdd_root)?
            .into_iter()
            .filter(|root| eligible_disk_ids.contains(&root.disk_id))
            .collect(),
        required_copies,
        required_bytes,
        |root| {
            measure_ssd_capacity(&root.root_path)
                .map(|capacity| {
                    (
                        capacity
                            .available_bytes
                            .saturating_sub(outstanding.get(&root.disk_id).copied().unwrap_or(0)),
                        capacity.total_bytes,
                    )
                })
                .map_err(|error| error.to_string())
        },
    )
}

fn select_hdd_roots_with_capacity(
    roots: Vec<dasobjectstore_metadata::DiskCopyRoot>,
    required_copies: u8,
    required_bytes: u64,
    mut capacity: impl FnMut(&dasobjectstore_metadata::DiskCopyRoot) -> Result<(u64, u64), String>,
) -> Result<Vec<dasobjectstore_metadata::DiskCopyRoot>, DurableDestageWorkerError> {
    if roots.len() < usize::from(required_copies) {
        return Err(DurableDestageWorkerError::InsufficientHddRoots {
            required: required_copies,
            available: roots.len(),
        });
    }
    let measured = roots
        .into_iter()
        .filter_map(|root| {
            capacity(&root)
                .ok()
                .and_then(|(available, total)| (total > 0).then_some((root, available, total)))
        })
        .collect::<Vec<_>>();
    let greatest_available_bytes = measured
        .iter()
        .map(|(_, available, _)| *available)
        .max()
        .unwrap_or(0);
    let mut eligible = measured
        .into_iter()
        .filter(|(_, available, _)| *available >= required_bytes)
        .collect::<Vec<_>>();
    eligible.sort_by(
        |(left_root, left_available, left_total), (right_root, right_available, right_total)| {
            (u128::from(*right_available) * u128::from(*left_total))
                .cmp(&(u128::from(*left_available) * u128::from(*right_total)))
                .then_with(|| left_root.disk_id.cmp(&right_root.disk_id))
        },
    );
    if eligible.len() < usize::from(required_copies) {
        return Err(DurableDestageWorkerError::InsufficientHddCapacity {
            required_copies,
            required_bytes,
            eligible_roots: eligible.len(),
            greatest_available_bytes,
        });
    }
    Ok(eligible
        .into_iter()
        .take(usize::from(required_copies))
        .map(|(root, _, _)| root)
        .collect())
}

fn parse_queued_object_type(value: &str) -> Result<ObjectType, DurableDestageWorkerError> {
    match value.parse::<ObjectType>() {
        Ok(object_type) => Ok(object_type),
        Err(_)
            if value
                .parse::<dasobjectstore_core::store::StoreClass>()
                .is_ok() =>
        {
            // Builds before 0.126.3 accidentally queued the ObjectStore
            // retention class for arbitrary EasyConnect S3 payloads. They
            // carry no stronger semantic type and are safely recovered as
            // the canonical naive type instead of remaining on SSD forever.
            Ok(ObjectType::Naive)
        }
        Err(error) => Err(DurableDestageWorkerError::InvalidObjectType {
            value: value.to_string(),
            message: error.to_string(),
        }),
    }
}

fn safe_relative_path(value: &str) -> Option<PathBuf> {
    let path = Path::new(value);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return None;
    }
    Some(path.to_path_buf())
}

fn remove_managed_ssd_job_root(
    ssd_root: &Path,
    job_root: &Path,
) -> Result<(), DurableDestageWorkerError> {
    let jobs_root = ssd_root.join(".dasobjectstore/ingest/jobs");
    if job_root.parent() != Some(jobs_root.as_path()) {
        return Err(DurableDestageWorkerError::UnsafeSsdEviction(
            job_root.to_path_buf(),
        ));
    }
    fs::remove_dir_all(job_root)?;
    Ok(())
}

/// Remove one settled direct-S3 payload without ever treating its key parent
/// as an ingest job. Object keys may share arbitrarily deep prefixes, so the
/// file is unlinked exactly and only empty parents are pruned.
fn remove_managed_direct_s3_payload(
    ssd_root: &Path,
    store_id: &dasobjectstore_core::ids::StoreId,
    payload: &Path,
) -> Result<(), DurableDestageWorkerError> {
    let namespace = format!("{:x}", Sha256::digest(store_id.as_str().as_bytes()));
    let objects_root = ssd_root
        .join(".dasobjectstore/stores")
        .join(namespace)
        .join("direct-s3/profile/.dasobjectstore/objects");
    let relative = payload
        .strip_prefix(&objects_root)
        .map_err(|_| DurableDestageWorkerError::UnsafeSsdEviction(payload.to_path_buf()))?;
    if relative.as_os_str().is_empty()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(DurableDestageWorkerError::UnsafeSsdEviction(
            payload.to_path_buf(),
        ));
    }
    let root_metadata = fs::symlink_metadata(&objects_root)?;
    let payload_metadata = fs::symlink_metadata(payload)?;
    if root_metadata.file_type().is_symlink()
        || !root_metadata.is_dir()
        || payload_metadata.file_type().is_symlink()
        || !payload_metadata.is_file()
    {
        return Err(DurableDestageWorkerError::UnsafeSsdEviction(
            payload.to_path_buf(),
        ));
    }
    let mut current = objects_root.clone();
    for component in relative.components() {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current)?;
        let final_component = current == payload;
        if metadata.file_type().is_symlink()
            || (!final_component && !metadata.is_dir())
            || (final_component && !metadata.is_file())
        {
            return Err(DurableDestageWorkerError::UnsafeSsdEviction(
                payload.to_path_buf(),
            ));
        }
    }
    fs::remove_file(payload)?;
    sync_directory(
        payload
            .parent()
            .ok_or_else(|| DurableDestageWorkerError::UnsafeSsdEviction(payload.to_path_buf()))?,
    )?;

    let mut parent = payload.parent();
    while let Some(directory) = parent {
        if directory == objects_root {
            break;
        }
        match fs::remove_dir(directory) {
            Ok(()) => {
                let ancestor = directory.parent().ok_or_else(|| {
                    DurableDestageWorkerError::UnsafeSsdEviction(directory.to_path_buf())
                })?;
                sync_directory(ancestor)?;
                parent = Some(ancestor);
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::DirectoryNotEmpty | std::io::ErrorKind::NotFound
                ) =>
            {
                break;
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn sync_directory(path: &Path) -> Result<(), DurableDestageWorkerError> {
    File::open(path)?.sync_all()?;
    Ok(())
}

fn retry_delay_seconds(attempt_count: u32) -> u64 {
    let exponent = attempt_count.saturating_sub(1).min(10);
    (30_u64.saturating_mul(1_u64 << exponent)).min(MAX_DESTAGE_RETRY_SECONDS)
}

fn current_utc_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    format_utc_timestamp_seconds(i64::try_from(seconds).unwrap_or(i64::MAX))
}

fn placement_id(object_id: &ObjectId, disk_id: &str, relative_path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(object_id.as_str().as_bytes());
    hasher.update([0]);
    hasher.update(disk_id.as_bytes());
    hasher.update([0]);
    hasher.update(relative_path.as_bytes());
    format!("placement-{:x}", hasher.finalize())
}

#[derive(Debug)]
pub enum DurableDestageWorkerError {
    Metadata(DestageMetadataError),
    CapacityClaim(DiskCapacityClaimError),
    Scheduler(SchedulerError),
    Ingest(crate::runtime::DaemonIngestFilesRuntimeError),
    ObjectPut(ObjectPutError),
    Io(std::io::Error),
    InvalidTimestamp(String),
    MissingSsdPlacement(ObjectId),
    UnsafeSsdPlacement(String),
    UnsafeSsdEviction(PathBuf),
    SsdPayloadMismatch {
        object_id: ObjectId,
        expected: u64,
        actual: u64,
    },
    InvalidObjectType {
        value: String,
        message: String,
    },
    InsufficientHddRoots {
        required: u8,
        available: usize,
    },
    InsufficientHddCapacity {
        required_copies: u8,
        required_bytes: u64,
        eligible_roots: usize,
        greatest_available_bytes: u64,
    },
    UnknownPlacementDisk(String),
    UnsafeHddPlacement(PathBuf),
    CapacityMeasurement {
        disk_id: String,
        message: String,
    },
    InvalidHeldCapacityClaims {
        object_id: ObjectId,
    },
    ReservedDiskUnavailable {
        disk_id: String,
    },
    ReservedDiskIneligible {
        disk_id: String,
    },
    SchedulerJobMissingObject(String),
    LeaseFenceLost {
        object_id: ObjectId,
        message: String,
    },
}

impl Display for DurableDestageWorkerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Metadata(error) => Display::fmt(error, formatter),
            Self::CapacityClaim(error) => Display::fmt(error, formatter),
            Self::Scheduler(error) => Display::fmt(error, formatter),
            Self::Ingest(error) => Display::fmt(error, formatter),
            Self::ObjectPut(error) => Display::fmt(error, formatter),
            Self::Io(error) => write!(formatter, "durable destage IO failed: {error}"),
            Self::InvalidTimestamp(value) => write!(formatter, "invalid destage timestamp: {value}"),
            Self::MissingSsdPlacement(object_id) => {
                write!(formatter, "verified SSD placement is missing for {object_id}")
            }
            Self::UnsafeSsdPlacement(path) => {
                write!(formatter, "unsafe managed SSD placement path: {path}")
            }
            Self::UnsafeSsdEviction(path) => write!(
                formatter,
                "refusing to evict SSD path outside the managed ingest jobs root: {}",
                path.display()
            ),
            Self::SsdPayloadMismatch {
                object_id,
                expected,
                actual,
            } => write!(
                formatter,
                "verified SSD payload mismatch for {object_id}: expected {expected} bytes, found {actual}"
            ),
            Self::InvalidObjectType { value, message } => {
                write!(formatter, "invalid queued object type {value}: {message}")
            }
            Self::InsufficientHddRoots {
                required,
                available,
            } => write!(
                formatter,
                "destage requires {required} HDD roots, found {available}"
            ),
            Self::InsufficientHddCapacity {
                required_copies,
                required_bytes,
                eligible_roots,
                greatest_available_bytes,
            } => write!(
                formatter,
                "HDD capacity blocked: destage requires {required_copies} distinct copy/copies of {required_bytes} bytes, found {eligible_roots} eligible root(s); greatest available capacity is {greatest_available_bytes} bytes"
            ),
            Self::UnknownPlacementDisk(disk_id) => {
                write!(formatter, "destage returned unknown disk {disk_id}")
            }
            Self::UnsafeHddPlacement(path) => {
                write!(formatter, "HDD placement escaped its managed root: {}", path.display())
            }
            Self::CapacityMeasurement { disk_id, message } => {
                write!(formatter, "failed to measure HDD {disk_id} capacity: {message}")
            }
            Self::InvalidHeldCapacityClaims { object_id } => write!(
                formatter,
                "held destage capacity claims do not cover every copy for {object_id}"
            ),
            Self::ReservedDiskUnavailable { disk_id } => {
                write!(formatter, "reserved HDD disk {disk_id} is unavailable")
            }
            Self::ReservedDiskIneligible { disk_id } => {
                write!(formatter, "reserved HDD disk {disk_id} is no longer placement-eligible")
            }
            Self::SchedulerJobMissingObject(job_id) => {
                write!(formatter, "destage scheduler job {job_id} has no object identity")
            }
            Self::LeaseFenceLost { object_id, message } => write!(
                formatter,
                "destage lease fence was lost for {object_id}; copy stopped: {message}"
            ),
        }
    }
}

impl std::error::Error for DurableDestageWorkerError {}

impl From<DestageMetadataError> for DurableDestageWorkerError {
    fn from(error: DestageMetadataError) -> Self {
        Self::Metadata(error)
    }
}

impl From<DiskCapacityClaimError> for DurableDestageWorkerError {
    fn from(error: DiskCapacityClaimError) -> Self {
        Self::CapacityClaim(error)
    }
}
impl From<SchedulerError> for DurableDestageWorkerError {
    fn from(error: SchedulerError) -> Self {
        Self::Scheduler(error)
    }
}

impl From<crate::runtime::DaemonIngestFilesRuntimeError> for DurableDestageWorkerError {
    fn from(error: crate::runtime::DaemonIngestFilesRuntimeError) -> Self {
        Self::Ingest(error)
    }
}

impl From<ObjectPutError> for DurableDestageWorkerError {
    fn from(error: ObjectPutError) -> Self {
        Self::ObjectPut(error)
    }
}

impl From<std::io::Error> for DurableDestageWorkerError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        parse_queued_object_type, remove_managed_direct_s3_payload, remove_managed_ssd_job_root,
        retry_delay_seconds, safe_relative_path, select_hdd_roots_with_capacity,
        select_managed_hdd_roots_with_capacity,
    };
    use dasobjectstore_core::ids::{DiskId, StoreId};
    use dasobjectstore_core::object_type::ObjectType;
    use dasobjectstore_metadata::{DiskCopyRoot, LIVE_SCHEMA_SQL};
    use rusqlite::Connection;
    use sha2::Digest;
    use std::collections::BTreeMap;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn retry_backoff_is_bounded() {
        assert_eq!(retry_delay_seconds(1), 30);
        assert_eq!(retry_delay_seconds(2), 60);
        assert_eq!(retry_delay_seconds(99), 3600);
    }

    #[test]
    fn legacy_store_class_queue_values_recover_as_naive_objects() {
        assert_eq!(
            parse_queued_object_type("generated_data").expect("legacy class"),
            ObjectType::Naive
        );
        assert_eq!(
            parse_queued_object_type("pod5").expect("typed object"),
            ObjectType::Pod5
        );
        assert!(parse_queued_object_type("not-a-type-or-class").is_err());
    }

    #[test]
    fn eviction_is_confined_to_managed_job_root() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-destage-eviction-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let job = root.join(".dasobjectstore/ingest/jobs/job-a");
        fs::create_dir_all(&job).expect("job root");
        fs::write(job.join("payload"), b"payload").expect("payload");
        remove_managed_ssd_job_root(&root, &job).expect("managed eviction");
        assert!(!job.exists());
        fs::create_dir_all(root.join("unmanaged")).expect("unmanaged");
        assert!(remove_managed_ssd_job_root(&root, &root.join("unmanaged")).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn placement_paths_are_relative_and_non_traversing() {
        assert!(safe_relative_path(".dasobjectstore/ingest/jobs/a/payload").is_some());
        assert!(safe_relative_path("../escape").is_none());
        assert!(safe_relative_path("/absolute").is_none());
    }

    #[test]
    fn destage_skips_a_full_first_disk_and_selects_capacity_eligible_roots() {
        let roots = ["disk-a", "disk-b", "disk-c"]
            .into_iter()
            .map(|id| {
                DiskCopyRoot::new(DiskId::new(id).expect("disk id"), format!("/managed/{id}"))
            })
            .collect::<Vec<_>>();
        let capacity = BTreeMap::from([
            ("disk-a", (19_u64, 1_000_u64)),
            ("disk-b", (1_000_u64, 2_000_u64)),
            ("disk-c", (900_u64, 1_000_u64)),
        ]);
        let selected = select_hdd_roots_with_capacity(roots, 1, 500, |root| {
            Ok(capacity[root.disk_id.as_str()])
        })
        .expect("capacity-eligible root");
        assert_eq!(selected[0].disk_id.as_str(), "disk-c");
    }

    #[test]
    fn durable_destage_accepts_watch_but_excludes_unregistered_managed_roots() {
        let root = temporary_root("registry-health-selection");
        let hdd_root = root.join("hdd");
        for disk_id in ["disk-watch", "disk-healthy", "disk-unregistered"] {
            let marker = hdd_root.join(disk_id).join(".dasobjectstore/device.env");
            fs::create_dir_all(marker.parent().expect("marker parent")).expect("disk root");
            fs::write(marker, format!("role=hdd:{disk_id}\n")).expect("disk marker");
        }
        let database = root.join("live.sqlite");
        let connection = Connection::open(&database).expect("database");
        connection.execute_batch(LIVE_SCHEMA_SQL).expect("schema");
        connection
            .execute(
                "INSERT INTO pools (pool_id,state,created_at_utc,updated_at_utc)
                 VALUES ('pool-a','Clean','now','now')",
                [],
            )
            .expect("pool");
        for (disk_id, state) in [("disk-watch", "Watch"), ("disk-healthy", "Healthy")] {
            connection
                .execute(
                    "INSERT INTO disks (
                        disk_id,pool_id,role,state,created_at_utc,updated_at_utc
                     ) VALUES (?1,'pool-a','hdd_capacity',?2,'now','now')",
                    [disk_id, state],
                )
                .expect("disk registry entry");
        }
        drop(connection);

        let selected = select_managed_hdd_roots_with_capacity(&database, &hdd_root, 2, 1, None)
            .expect("placement-eligible destinations");
        assert_eq!(selected.len(), 2);
        let mut selected_ids = selected
            .iter()
            .map(|root| root.disk_id.as_str())
            .collect::<Vec<_>>();
        // Both test roots share one busy filesystem, so their observed free
        // fractions can legitimately change between measurements. Placement
        // order follows those observations; this assertion concerns only the
        // registry-health eligibility boundary.
        selected_ids.sort_unstable();
        assert_eq!(selected_ids, vec!["disk-healthy", "disk-watch"]);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn destage_reports_capacity_block_before_copy_when_no_root_can_fit() {
        let roots = ["disk-a", "disk-b"]
            .into_iter()
            .map(|id| {
                DiskCopyRoot::new(DiskId::new(id).expect("disk id"), format!("/managed/{id}"))
            })
            .collect::<Vec<_>>();
        let error = select_hdd_roots_with_capacity(roots, 1, 500, |_| Ok((499, 1_000)))
            .expect_err("capacity block");
        assert!(matches!(
            error,
            super::DurableDestageWorkerError::InsufficientHddCapacity {
                required_copies: 1,
                required_bytes: 500,
                eligible_roots: 0,
                greatest_available_bytes: 499,
            }
        ));
        assert!(error.to_string().contains("HDD capacity blocked"));
    }

    #[test]
    fn destage_selects_distinct_roots_for_every_required_copy() {
        let roots = ["disk-a", "disk-b", "disk-c"]
            .into_iter()
            .map(|id| {
                DiskCopyRoot::new(DiskId::new(id).expect("disk id"), format!("/managed/{id}"))
            })
            .collect::<Vec<_>>();
        let capacity = BTreeMap::from([
            ("disk-a", (1_000_u64, 2_000_u64)),
            ("disk-b", (900_u64, 1_000_u64)),
            ("disk-c", (100_u64, 1_000_u64)),
        ]);
        let selected = select_hdd_roots_with_capacity(roots, 2, 500, |root| {
            Ok(capacity[root.disk_id.as_str()])
        })
        .expect("two eligible roots");
        assert_eq!(
            selected
                .iter()
                .map(|root| root.disk_id.as_str())
                .collect::<Vec<_>>(),
            vec!["disk-b", "disk-a"]
        );
    }

    #[test]
    fn destage_uses_disk_id_only_to_break_an_exact_free_fraction_tie() {
        let roots = ["disk-b", "disk-a"]
            .into_iter()
            .map(|id| {
                DiskCopyRoot::new(DiskId::new(id).expect("disk id"), format!("/managed/{id}"))
            })
            .collect::<Vec<_>>();
        let selected = select_hdd_roots_with_capacity(roots, 2, 10, |root| {
            Ok(match root.disk_id.as_str() {
                "disk-a" => (500, 1_000),
                "disk-b" => (1_000, 2_000),
                _ => unreachable!(),
            })
        })
        .expect("ratio-tied roots");
        assert_eq!(
            selected
                .iter()
                .map(|root| root.disk_id.as_str())
                .collect::<Vec<_>>(),
            vec!["disk-a", "disk-b"]
        );
    }

    #[test]
    fn direct_s3_eviction_unlinks_only_the_exact_store_bound_payload() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-direct-s3-eviction-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let store = StoreId::new("epic_collection").expect("store");
        let namespace = format!("{:x}", sha2::Sha256::digest(store.as_str().as_bytes()));
        let objects = root
            .join(".dasobjectstore/stores")
            .join(namespace)
            .join("direct-s3/profile/.dasobjectstore/objects");
        let payload = objects.join("shared/prefix/object-a.tar");
        let sibling = objects.join("shared/prefix/object-b.tar");
        fs::create_dir_all(payload.parent().expect("parent")).expect("objects");
        fs::write(&payload, b"a").expect("payload");
        fs::write(&sibling, b"b").expect("sibling");

        remove_managed_direct_s3_payload(&root, &store, &payload).expect("exact eviction");
        assert!(!payload.exists());
        assert_eq!(fs::read(&sibling).expect("sibling retained"), b"b");
        assert!(objects.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn direct_s3_eviction_rejects_another_store_namespace() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-direct-s3-cross-store-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let expected = StoreId::new("expected").expect("store");
        let other = StoreId::new("other").expect("store");
        let namespace = format!("{:x}", sha2::Sha256::digest(other.as_str().as_bytes()));
        let payload = root
            .join(".dasobjectstore/stores")
            .join(namespace)
            .join("direct-s3/profile/.dasobjectstore/objects/object.tar");
        fs::create_dir_all(payload.parent().expect("parent")).expect("objects");
        fs::write(&payload, b"payload").expect("payload");

        assert!(remove_managed_direct_s3_payload(&root, &expected, &payload).is_err());
        assert!(payload.exists());
        let _ = fs::remove_dir_all(root);
    }

    fn temporary_root(name: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dasobjectstore-destage-{name}-{}-{nanos}",
            std::process::id()
        ))
    }
}
