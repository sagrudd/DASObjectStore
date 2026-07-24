//! Restart-safe execution of durable managed-SSD to HDD placement work.

use crate::runtime::ingest_files::discover_managed_hdd_roots;
use dasobjectstore_core::ids::ObjectId;
use dasobjectstore_core::object_type::ObjectType;
use dasobjectstore_core::utc::add_seconds_to_utc_timestamp;
use dasobjectstore_metadata::{
    claim_next_destage, fail_destage, list_ssd_eviction_candidates, mark_ssd_evicted,
    measure_ssd_capacity, promote_hdd_settlement, read_ssd_placement,
    settle_staged_object_to_hdd_preserving_ssd_with_controlled_progress, DestageMetadataError,
    DestageQueueRecord, HddSettlementPromotionRequest, ObjectPutError, StagedObjectPut,
    VerifiedHddPlacement,
};
use sha2::{Digest, Sha256};
use std::fmt::{self, Display};
use std::fs::{self, File};
use std::path::{Component, Path, PathBuf};

pub const DEFAULT_DESTAGE_LEASE_SECONDS: u64 = 60 * 60;
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
    let Some(record) = claim_next_destage(
        &config.live_sqlite_path,
        &config.worker_id,
        &lease_expires_at_utc,
        now_utc,
        previously_served_store,
    )?
    else {
        return evict_one_settled_ssd_copy(config, now_utc);
    };

    match settle_claimed_record(config, &record, now_utc) {
        Ok(copies) => Ok(DurableDestageOutcome::Settled {
            store_id: record.store_id.clone(),
            object_id: record.object_id,
            copies,
        }),
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
    now_utc: &str,
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
    let roots = select_managed_hdd_roots_with_capacity(
        &config.hdd_root,
        record.required_copy_count,
        record.expected_size_bytes,
    )?;
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
    let report =
        settle_staged_object_to_hdd_preserving_ssd_with_controlled_progress(&staged, |_| Ok(()))?;

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

/// Select distinct managed HDD roots that can hold a complete copy before
/// beginning a destage write. Discovery order is not a placement policy:
/// roots are first filtered by complete-file capacity, then ranked by their
/// exact fractional free capacity. Disk identity is only a deterministic
/// tiebreaker when two fractions are equal.
pub(crate) fn select_managed_hdd_roots_with_capacity(
    hdd_root: &Path,
    required_copies: u8,
    required_bytes: u64,
) -> Result<Vec<dasobjectstore_metadata::DiskCopyRoot>, DurableDestageWorkerError> {
    select_hdd_roots_with_capacity(
        discover_managed_hdd_roots(hdd_root)?,
        required_copies,
        required_bytes,
        |root| {
            measure_ssd_capacity(&root.root_path)
                .map(|capacity| (capacity.available_bytes, capacity.total_bytes))
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
}

impl Display for DurableDestageWorkerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Metadata(error) => Display::fmt(error, formatter),
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
        }
    }
}

impl std::error::Error for DurableDestageWorkerError {}

impl From<DestageMetadataError> for DurableDestageWorkerError {
    fn from(error: DestageMetadataError) -> Self {
        Self::Metadata(error)
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
    };
    use dasobjectstore_core::ids::{DiskId, StoreId};
    use dasobjectstore_core::object_type::ObjectType;
    use dasobjectstore_metadata::DiskCopyRoot;
    use sha2::Digest;
    use std::collections::BTreeMap;
    use std::fs;

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
}
