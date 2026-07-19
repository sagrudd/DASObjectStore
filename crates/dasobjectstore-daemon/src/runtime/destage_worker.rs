//! Restart-safe execution of durable managed-SSD to HDD placement work.

use crate::runtime::ingest_files::discover_managed_hdd_roots;
use dasobjectstore_core::ids::ObjectId;
use dasobjectstore_core::object_type::ObjectType;
use dasobjectstore_core::utc::add_seconds_to_utc_timestamp;
use dasobjectstore_metadata::{
    claim_next_destage, fail_destage, list_ssd_eviction_candidates, mark_ssd_evicted,
    promote_hdd_settlement, read_ssd_placement,
    settle_staged_object_to_hdd_preserving_ssd_with_controlled_progress, DestageMetadataError,
    DestageQueueRecord, HddSettlementPromotionRequest, ObjectPutError, StagedObjectPut,
    VerifiedHddPlacement,
};
use sha2::{Digest, Sha256};
use std::fmt::{self, Display};
use std::fs;
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
    if job_root.exists() {
        remove_managed_ssd_job_root(&config.ssd_root, job_root)?;
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
    let object_type = record.object_type.parse::<ObjectType>().map_err(|error| {
        DurableDestageWorkerError::InvalidObjectType {
            value: record.object_type.clone(),
            message: error.to_string(),
        }
    })?;
    let roots = discover_managed_hdd_roots(&config.hdd_root)?;
    if roots.len() < record.required_copy_count as usize {
        return Err(DurableDestageWorkerError::InsufficientHddRoots {
            required: record.required_copy_count,
            available: roots.len(),
        });
    }
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
    use super::{remove_managed_ssd_job_root, retry_delay_seconds, safe_relative_path};
    use std::fs;

    #[test]
    fn retry_backoff_is_bounded() {
        assert_eq!(retry_delay_seconds(1), 30);
        assert_eq!(retry_delay_seconds(2), 60);
        assert_eq!(retry_delay_seconds(99), 3600);
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
}
