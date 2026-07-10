use crate::copy::{
    write_hdd_copies_with_inline_hash_fanout_with_controlled_progress,
    write_verified_hdd_copy_with_controlled_progress, HddCopyError, HddCopyRequest,
    HddInlineHashCopyProgress, HddInlineHashCopyRequest,
};
use crate::evacuation::DiskCopyRoot;
use crate::ingest::{encode_path_component, IngestStagingLayout};
use crate::secure_fs::{create_private_dir_all, set_private_dir_permissions};
use dasobjectstore_core::ids::{IngestJobId, InvalidId, ObjectId};
use dasobjectstore_core::object_type::ObjectType;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};
use std::fs::{self, File};
use std::io;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectPutRequest {
    pub object_id: ObjectId,
    pub source_path: PathBuf,
    pub ssd_root: PathBuf,
    pub disk_roots: Vec<DiskCopyRoot>,
    pub copy_count: u8,
    pub object_type: ObjectType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirectObjectPutRequest {
    pub object_id: ObjectId,
    pub source_path: PathBuf,
    pub disk_roots: Vec<DiskCopyRoot>,
    pub copy_count: u8,
    pub object_type: ObjectType,
}

impl DirectObjectPutRequest {
    pub fn new(
        object_id: ObjectId,
        source_path: impl Into<PathBuf>,
        disk_roots: Vec<DiskCopyRoot>,
        copy_count: u8,
    ) -> Self {
        Self {
            object_id,
            source_path: source_path.into(),
            disk_roots,
            copy_count,
            object_type: ObjectType::Naive,
        }
    }

    pub fn with_object_type(mut self, object_type: ObjectType) -> Self {
        self.object_type = object_type;
        self
    }
}

impl ObjectPutRequest {
    pub fn new(
        object_id: ObjectId,
        source_path: impl Into<PathBuf>,
        ssd_root: impl Into<PathBuf>,
        disk_roots: Vec<DiskCopyRoot>,
        copy_count: u8,
    ) -> Self {
        Self {
            object_id,
            source_path: source_path.into(),
            ssd_root: ssd_root.into(),
            disk_roots,
            copy_count,
            object_type: ObjectType::Naive,
        }
    }

    pub fn with_object_type(mut self, object_type: ObjectType) -> Self {
        self.object_type = object_type;
        self
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObjectPutReport {
    pub object_id: ObjectId,
    pub object_type: ObjectType,
    pub source_path: PathBuf,
    pub staged_payload_path: PathBuf,
    pub bytes_staged: u64,
    pub content_hash_algorithm: String,
    pub content_hash: String,
    pub placements: Vec<ObjectPutPlacementReport>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StagedObjectPut {
    pub object_id: ObjectId,
    pub object_type: ObjectType,
    pub source_path: PathBuf,
    pub job_root: PathBuf,
    pub staged_payload_path: PathBuf,
    pub bytes_staged: u64,
    pub content_hash_algorithm: String,
    pub content_hash: String,
    pub disk_roots: Vec<DiskCopyRoot>,
    pub copy_count: u8,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObjectPutPlacementReport {
    pub disk_id: String,
    pub copy_number: u8,
    pub destination_path: PathBuf,
    pub bytes_written: u64,
    pub content_hash: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectPutProgress {
    pub object_id: ObjectId,
    pub stage: ObjectPutProgressStage,
    pub bytes_written: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObjectPutProgressStage {
    SsdIngest,
    SsdFlush,
    HddCopy {
        disk_id: String,
        copy_number: u8,
    },
    HddFsync {
        disk_id: String,
        copy_number: u8,
        duration_millis: Option<u64>,
    },
    HddRename {
        disk_id: String,
        copy_number: u8,
        duration_millis: Option<u64>,
    },
}

pub fn put_object_ssd_first(request: &ObjectPutRequest) -> Result<ObjectPutReport, ObjectPutError> {
    put_object_ssd_first_with_progress(request, |_| {})
}

pub fn put_object_ssd_first_with_progress(
    request: &ObjectPutRequest,
    mut progress: impl FnMut(ObjectPutProgress),
) -> Result<ObjectPutReport, ObjectPutError> {
    put_object_ssd_first_with_controlled_progress(request, |object_progress| {
        progress(object_progress);
        Ok(())
    })
}

pub fn put_object_ssd_first_with_controlled_progress(
    request: &ObjectPutRequest,
    mut progress: impl FnMut(ObjectPutProgress) -> Result<(), ObjectPutError>,
) -> Result<ObjectPutReport, ObjectPutError> {
    let staged = stage_object_on_ssd_with_controlled_progress(request, &mut progress)?;
    settle_staged_object_to_hdd_with_controlled_progress(staged, progress)
}

pub fn stage_object_on_ssd_with_controlled_progress(
    request: &ObjectPutRequest,
    mut progress: impl FnMut(ObjectPutProgress) -> Result<(), ObjectPutError>,
) -> Result<StagedObjectPut, ObjectPutError> {
    let staged = stage_object_on_ssd_unsynced_with_controlled_progress(request, &mut progress)?;
    sync_staged_object_on_ssd_with_controlled_progress(staged, progress)
}

pub fn stage_object_on_ssd_unsynced_with_controlled_progress(
    request: &ObjectPutRequest,
    mut progress: impl FnMut(ObjectPutProgress) -> Result<(), ObjectPutError>,
) -> Result<StagedObjectPut, ObjectPutError> {
    validate_request(request)?;

    let layout = IngestStagingLayout::for_ssd_root(&request.ssd_root);
    layout.create_base_directories()?;
    let job_id = IngestJobId::new(format!("put-{}", request.object_id.as_str()))?;
    let job_paths = layout.job_paths(&job_id);

    let staged = stage_object_on_ssd_inner(request, &job_paths, &mut progress);
    if staged.is_err() {
        let _ = fs::remove_dir_all(&job_paths.job_root);
    }
    staged
}

pub fn sync_staged_object_on_ssd_with_controlled_progress(
    staged: StagedObjectPut,
    mut progress: impl FnMut(ObjectPutProgress) -> Result<(), ObjectPutError>,
) -> Result<StagedObjectPut, ObjectPutError> {
    let layout = crate::ingest::IngestJobPaths {
        job_root: staged.job_root.clone(),
        payload_path: staged.staged_payload_path.clone(),
        scratch_dir: staged.job_root.join(crate::ingest::INGEST_SCRATCH_DIR_NAME),
    };
    layout
        .sync_payload_with_progress(|bytes_written| {
            progress(ObjectPutProgress {
                object_id: staged.object_id.clone(),
                stage: ObjectPutProgressStage::SsdFlush,
                bytes_written,
            })
            .map_err(object_put_error_to_io)
        })
        .map_err(object_put_error_from_io)?;
    Ok(staged)
}

pub fn settle_staged_object_to_hdd_with_controlled_progress(
    staged: StagedObjectPut,
    mut progress: impl FnMut(ObjectPutProgress) -> Result<(), ObjectPutError>,
) -> Result<ObjectPutReport, ObjectPutError> {
    let report = settle_staged_object_to_hdd_inner(&staged, &mut progress);
    let _ = fs::remove_dir_all(&staged.job_root);
    report
}

pub fn put_object_direct_to_hdd_with_controlled_progress(
    request: DirectObjectPutRequest,
    mut progress: impl FnMut(ObjectPutProgress) -> Result<(), ObjectPutError>,
) -> Result<ObjectPutReport, ObjectPutError> {
    validate_direct_request(&request)?;
    let placements = write_direct_requested_copies(&request, &mut progress)?;
    let content_hash = placements
        .first()
        .map(|placement| placement.content_hash.clone())
        .unwrap_or_default();

    Ok(ObjectPutReport {
        object_id: request.object_id,
        object_type: request.object_type,
        source_path: request.source_path.clone(),
        staged_payload_path: request.source_path,
        bytes_staged: placements
            .first()
            .map(|placement| placement.bytes_written)
            .unwrap_or(0),
        content_hash_algorithm: crate::copy::HDD_COPY_CONTENT_HASH_ALGORITHM.to_string(),
        content_hash,
        placements,
    })
}

fn stage_object_on_ssd_inner(
    request: &ObjectPutRequest,
    job_paths: &crate::ingest::IngestJobPaths,
    progress: &mut impl FnMut(ObjectPutProgress) -> Result<(), ObjectPutError>,
) -> Result<StagedObjectPut, ObjectPutError> {
    let mut source = File::open(&request.source_path)?;
    let write_report = job_paths
        .write_payload_with_hash_unsynced_controlled_progress(&mut source, |bytes_written| {
            progress(ObjectPutProgress {
                object_id: request.object_id.clone(),
                stage: ObjectPutProgressStage::SsdIngest,
                bytes_written,
            })
            .map_err(object_put_error_to_io)
        })
        .map_err(object_put_error_from_io)?;

    Ok(StagedObjectPut {
        object_id: request.object_id.clone(),
        object_type: request.object_type,
        source_path: request.source_path.clone(),
        job_root: job_paths.job_root.clone(),
        staged_payload_path: job_paths.payload_path.clone(),
        bytes_staged: write_report.bytes_written,
        content_hash_algorithm: write_report.content_hash_algorithm,
        content_hash: write_report.content_hash,
        disk_roots: request.disk_roots.clone(),
        copy_count: request.copy_count,
    })
}

fn validate_direct_request(request: &DirectObjectPutRequest) -> Result<(), ObjectPutError> {
    if request.copy_count == 0 {
        return Err(ObjectPutError::InvalidCopyCount);
    }
    if request.disk_roots.len() < request.copy_count as usize {
        return Err(ObjectPutError::NotEnoughDiskRoots {
            requested_copies: request.copy_count,
            disk_roots: request.disk_roots.len(),
        });
    }
    Ok(())
}

fn settle_staged_object_to_hdd_inner(
    staged: &StagedObjectPut,
    progress: &mut impl FnMut(ObjectPutProgress) -> Result<(), ObjectPutError>,
) -> Result<ObjectPutReport, ObjectPutError> {
    let placements = write_requested_copies(staged, progress)?;

    Ok(ObjectPutReport {
        object_id: staged.object_id.clone(),
        object_type: staged.object_type,
        source_path: staged.source_path.clone(),
        staged_payload_path: staged.staged_payload_path.clone(),
        bytes_staged: staged.bytes_staged,
        content_hash_algorithm: staged.content_hash_algorithm.clone(),
        content_hash: staged.content_hash.clone(),
        placements,
    })
}

fn write_direct_requested_copies(
    request: &DirectObjectPutRequest,
    progress: &mut impl FnMut(ObjectPutProgress) -> Result<(), ObjectPutError>,
) -> Result<Vec<ObjectPutPlacementReport>, ObjectPutError> {
    let copy_requests = request
        .disk_roots
        .iter()
        .take(request.copy_count as usize)
        .enumerate()
        .map(|(index, disk_root)| {
            let copy_number = (index + 1) as u8;
            let temporary_path =
                direct_object_copy_temporary_path(disk_root, &request.object_id, copy_number);
            HddInlineHashCopyRequest::new(
                request.object_id.clone(),
                disk_root.disk_id.clone(),
                copy_number,
                &request.source_path,
                temporary_path,
            )
        })
        .collect::<Vec<_>>();
    let mut copy_reports = write_hdd_copies_with_inline_hash_fanout_with_controlled_progress(
        &copy_requests,
        |disk_id, copy_number, copy_progress| {
            let (stage, bytes_written) = match copy_progress {
                HddInlineHashCopyProgress::BytesWritten { bytes_written } => (
                    ObjectPutProgressStage::HddCopy {
                        disk_id: disk_id.as_str().to_string(),
                        copy_number,
                    },
                    bytes_written,
                ),
                HddInlineHashCopyProgress::FsyncStarted { bytes_written } => (
                    ObjectPutProgressStage::HddFsync {
                        disk_id: disk_id.as_str().to_string(),
                        copy_number,
                        duration_millis: None,
                    },
                    bytes_written,
                ),
                HddInlineHashCopyProgress::FsyncComplete {
                    bytes_written,
                    duration_millis,
                } => (
                    ObjectPutProgressStage::HddFsync {
                        disk_id: disk_id.as_str().to_string(),
                        copy_number,
                        duration_millis: Some(duration_millis),
                    },
                    bytes_written,
                ),
            };
            progress(ObjectPutProgress {
                object_id: request.object_id.clone(),
                stage,
                bytes_written,
            })
            .map_err(object_put_error_to_hdd_copy_error)
        },
    )?;

    let mut placements = Vec::with_capacity(copy_reports.len());
    for copy_report in &mut copy_reports {
        let disk_root = &request.disk_roots[(copy_report.copy_number - 1) as usize];
        let final_path = object_copy_path(disk_root, &request.object_id, &copy_report.content_hash);
        progress(ObjectPutProgress {
            object_id: request.object_id.clone(),
            stage: ObjectPutProgressStage::HddRename {
                disk_id: copy_report.disk_id.as_str().to_string(),
                copy_number: copy_report.copy_number,
                duration_millis: None,
            },
            bytes_written: copy_report.bytes_written,
        })?;
        let rename_started_at = Instant::now();
        move_direct_copy_into_place(
            &copy_report.destination_path,
            &final_path,
            copy_report.bytes_written,
        )?;
        progress(ObjectPutProgress {
            object_id: request.object_id.clone(),
            stage: ObjectPutProgressStage::HddRename {
                disk_id: copy_report.disk_id.as_str().to_string(),
                copy_number: copy_report.copy_number,
                duration_millis: Some(
                    rename_started_at
                        .elapsed()
                        .as_millis()
                        .min(u128::from(u64::MAX)) as u64,
                ),
            },
            bytes_written: copy_report.bytes_written,
        })?;
        copy_report.destination_path = final_path;
        placements.push(ObjectPutPlacementReport {
            disk_id: copy_report.disk_id.as_str().to_string(),
            copy_number: copy_report.copy_number,
            destination_path: copy_report.destination_path.clone(),
            bytes_written: copy_report.bytes_written,
            content_hash: copy_report.content_hash.clone(),
        });
    }
    Ok(placements)
}

pub fn object_payload_path(
    disk_root: &DiskCopyRoot,
    object_id: &ObjectId,
    content_hash: &str,
) -> PathBuf {
    object_copy_path(disk_root, object_id, content_hash)
}

pub fn existing_object_payload_candidate_paths(
    disk_root: &DiskCopyRoot,
    object_id: &ObjectId,
) -> Result<Vec<PathBuf>, io::Error> {
    let objects_root = disk_root.root_path.join("objects");
    if !objects_root.exists() {
        return Ok(Vec::new());
    }
    let encoded_object_id = encode_path_component(object_id.as_str());
    let mut candidates = Vec::new();
    for entry in fs::read_dir(objects_root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let payload_path = entry.path().join(&encoded_object_id).join("payload");
        if payload_path.exists() {
            candidates.push(payload_path);
        }
    }
    Ok(candidates)
}

fn validate_request(request: &ObjectPutRequest) -> Result<(), ObjectPutError> {
    if request.copy_count == 0 {
        return Err(ObjectPutError::InvalidCopyCount);
    }
    if request.disk_roots.len() < request.copy_count as usize {
        return Err(ObjectPutError::NotEnoughDiskRoots {
            requested_copies: request.copy_count,
            disk_roots: request.disk_roots.len(),
        });
    }

    Ok(())
}

fn write_requested_copies(
    staged: &StagedObjectPut,
    progress: &mut impl FnMut(ObjectPutProgress) -> Result<(), ObjectPutError>,
) -> Result<Vec<ObjectPutPlacementReport>, ObjectPutError> {
    staged
        .disk_roots
        .iter()
        .take(staged.copy_count as usize)
        .enumerate()
        .map(|(index, disk_root)| {
            let copy_number = (index + 1) as u8;
            let destination_path =
                object_copy_path(disk_root, &staged.object_id, &staged.content_hash);
            if destination_path
                .metadata()
                .map(|metadata| metadata.len() == staged.bytes_staged)
                .unwrap_or(false)
            {
                // The staged copy already computed this content hash while it
                // was read and written. A content-addressed destination with
                // the same hash and expected size is therefore a safe
                // post-copy deduplication; do not reread either payload.
                progress(ObjectPutProgress {
                    object_id: staged.object_id.clone(),
                    stage: ObjectPutProgressStage::HddCopy {
                        disk_id: disk_root.disk_id.as_str().to_string(),
                        copy_number,
                    },
                    bytes_written: staged.bytes_staged,
                })?;
                return Ok(ObjectPutPlacementReport {
                    disk_id: disk_root.disk_id.as_str().to_string(),
                    copy_number,
                    destination_path,
                    bytes_written: staged.bytes_staged,
                    content_hash: staged.content_hash.clone(),
                });
            }
            let copy_report = write_verified_hdd_copy_with_controlled_progress(
                &HddCopyRequest::new(
                    staged.object_id.clone(),
                    disk_root.disk_id.clone(),
                    copy_number,
                    &staged.staged_payload_path,
                    destination_path,
                    staged.content_hash.clone(),
                ),
                |bytes_written| {
                    progress(ObjectPutProgress {
                        object_id: staged.object_id.clone(),
                        stage: ObjectPutProgressStage::HddCopy {
                            disk_id: disk_root.disk_id.as_str().to_string(),
                            copy_number,
                        },
                        bytes_written,
                    })
                    .map_err(object_put_error_to_hdd_copy_error)
                },
            )?;

            Ok(ObjectPutPlacementReport {
                disk_id: copy_report.disk_id.as_str().to_string(),
                copy_number,
                destination_path: copy_report.destination_path,
                bytes_written: copy_report.bytes_written,
                content_hash: copy_report.content_hash,
            })
        })
        .collect()
}

fn object_copy_path(disk_root: &DiskCopyRoot, object_id: &ObjectId, content_hash: &str) -> PathBuf {
    let prefix = content_hash.get(0..2).unwrap_or("xx");
    disk_root
        .root_path
        .join("objects")
        .join(prefix)
        .join(encode_path_component(object_id.as_str()))
        .join("payload")
}

fn direct_object_copy_temporary_path(
    disk_root: &DiskCopyRoot,
    object_id: &ObjectId,
    copy_number: u8,
) -> PathBuf {
    disk_root
        .root_path
        .join(".dasobjectstore")
        .join("direct-import")
        .join(encode_path_component(object_id.as_str()))
        .join(format!("copy-{copy_number}.payload"))
}

fn move_direct_copy_into_place(
    temporary_path: &std::path::Path,
    final_path: &std::path::Path,
    expected_bytes: u64,
) -> Result<(), ObjectPutError> {
    if let Some(parent) = final_path.parent() {
        create_private_dir_all(parent)?;
        restrict_object_tree_dirs(parent)?;
    }
    if final_path.exists() {
        if final_path.metadata()?.len() == expected_bytes {
            fs::remove_file(temporary_path)?;
            // Direct imports compute the content hash while writing the
            // temporary payload. A matching, same-sized content-addressed
            // destination is a safe dedupe; avoid a second source read or
            // treating an idempotent retry as an ingest failure.
            return Ok(());
        }
        return Err(ObjectPutError::Io(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "object payload already exists with unexpected size: {}",
                final_path.display()
            ),
        )));
    }
    fs::rename(temporary_path, final_path)?;
    if let Some(parent) = temporary_path.parent() {
        let _ = fs::remove_dir_all(parent);
    }
    Ok(())
}

fn restrict_object_tree_dirs(payload_parent: &std::path::Path) -> Result<(), ObjectPutError> {
    set_private_dir_permissions(payload_parent)?;
    if let Some(prefix_dir) = payload_parent.parent() {
        set_private_dir_permissions(prefix_dir)?;
        if let Some(objects_dir) = prefix_dir.parent() {
            set_private_dir_permissions(objects_dir)?;
        }
    }

    Ok(())
}

#[derive(Debug)]
pub enum ObjectPutError {
    Io(std::io::Error),
    Cancelled,
    InvalidCopyCount,
    InvalidIngestJobId(InvalidId),
    NotEnoughDiskRoots {
        requested_copies: u8,
        disk_roots: usize,
    },
    Copy(HddCopyError),
}

impl Display for ObjectPutError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(formatter, "object put IO failed: {err}"),
            Self::Cancelled => formatter.write_str("object put cancelled"),
            Self::InvalidCopyCount => formatter.write_str("object put requires at least one copy"),
            Self::InvalidIngestJobId(err) => write!(formatter, "invalid ingest job id: {err}"),
            Self::NotEnoughDiskRoots {
                requested_copies,
                disk_roots,
            } => write!(
                formatter,
                "object put requested {requested_copies} copies but only {disk_roots} disk roots were provided"
            ),
            Self::Copy(err) => write!(formatter, "{err}"),
        }
    }
}

impl std::error::Error for ObjectPutError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Cancelled => None,
            Self::InvalidCopyCount => None,
            Self::InvalidIngestJobId(err) => Some(err),
            Self::NotEnoughDiskRoots { .. } => None,
            Self::Copy(err) => Some(err),
        }
    }
}

fn object_put_error_to_io(error: ObjectPutError) -> io::Error {
    match error {
        ObjectPutError::Io(error) => error,
        ObjectPutError::Cancelled => {
            io::Error::new(io::ErrorKind::Interrupted, "object put cancelled")
        }
        ObjectPutError::InvalidCopyCount
        | ObjectPutError::InvalidIngestJobId(_)
        | ObjectPutError::NotEnoughDiskRoots { .. }
        | ObjectPutError::Copy(_) => io::Error::other(error.to_string()),
    }
}

fn object_put_error_from_io(error: io::Error) -> ObjectPutError {
    if error.kind() == io::ErrorKind::Interrupted {
        ObjectPutError::Cancelled
    } else {
        ObjectPutError::Io(error)
    }
}

fn object_put_error_to_hdd_copy_error(error: ObjectPutError) -> HddCopyError {
    match error {
        ObjectPutError::Io(error) => HddCopyError::Io(error),
        ObjectPutError::Cancelled => HddCopyError::Cancelled,
        ObjectPutError::Copy(error) => error,
        ObjectPutError::InvalidCopyCount
        | ObjectPutError::InvalidIngestJobId(_)
        | ObjectPutError::NotEnoughDiskRoots { .. } => {
            HddCopyError::Io(io::Error::other(error.to_string()))
        }
    }
}

impl From<std::io::Error> for ObjectPutError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<InvalidId> for ObjectPutError {
    fn from(err: InvalidId) -> Self {
        Self::InvalidIngestJobId(err)
    }
}

impl From<HddCopyError> for ObjectPutError {
    fn from(err: HddCopyError) -> Self {
        Self::Copy(err)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        put_object_direct_to_hdd_with_controlled_progress, put_object_ssd_first,
        put_object_ssd_first_with_controlled_progress, DirectObjectPutRequest, ObjectPutError,
        ObjectPutRequest,
    };
    use crate::evacuation::DiskCopyRoot;
    use crate::hash::hash_file_sha256;
    use dasobjectstore_core::ids::{DiskId, ObjectId};
    use dasobjectstore_core::object_type::ObjectType;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn stages_on_ssd_then_writes_verified_hdd_copies() {
        let root = temp_root("object-put");
        let source_path = root.join("source.fastq.gz");
        let ssd_root = root.join("ssd");
        let disk_a = root.join("disk-a");
        let disk_b = root.join("disk-b");
        let payload = b"bioinformatics object payload";
        fs::create_dir_all(&root).expect("create temp root");
        fs::write(&source_path, payload).expect("write source");
        let request = ObjectPutRequest::new(
            ObjectId::new("object-a").expect("object id"),
            &source_path,
            &ssd_root,
            vec![
                DiskCopyRoot::new(DiskId::new("disk-a").expect("disk id"), &disk_a),
                DiskCopyRoot::new(DiskId::new("disk-b").expect("disk id"), &disk_b),
            ],
            2,
        );

        let report = put_object_ssd_first(&request).expect("object put succeeds");

        let expected_hash = hash_file_sha256(&source_path).expect("hash source");
        assert_eq!(report.object_id.as_str(), "object-a");
        assert_eq!(report.object_type, ObjectType::Naive);
        assert_eq!(report.bytes_staged, payload.len() as u64);
        assert_eq!(report.content_hash, expected_hash);
        assert_eq!(report.placements.len(), 2);
        assert!(
            !report.staged_payload_path.exists(),
            "verified HDD settlement should remove the temporary SSD staging payload"
        );
        for placement in &report.placements {
            assert_eq!(placement.bytes_written, payload.len() as u64);
            assert_eq!(placement.content_hash, expected_hash);
            assert_eq!(
                fs::read(&placement.destination_path).expect("read placement"),
                payload
            );
        }

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn rejects_copy_count_without_enough_disk_roots() {
        let root = temp_root("object-put-not-enough-roots");
        let request = ObjectPutRequest::new(
            ObjectId::new("object-a").expect("object id"),
            root.join("source"),
            root.join("ssd"),
            vec![DiskCopyRoot::new(
                DiskId::new("disk-a").expect("disk id"),
                root.join("disk-a"),
            )],
            2,
        );

        let err = put_object_ssd_first(&request).expect_err("disk roots required");

        assert!(matches!(
            err,
            ObjectPutError::NotEnoughDiskRoots {
                requested_copies: 2,
                disk_roots: 1
            }
        ));
    }

    #[test]
    fn direct_hdd_put_hashes_inline_and_moves_payload_into_content_addressed_path() {
        let root = temp_root("object-put-direct-hdd");
        let source_path = root.join("source.fastq.gz");
        let disk_a = root.join("disk-a");
        let payload = b"direct HDD object payload";
        fs::create_dir_all(&root).expect("create temp root");
        fs::write(&source_path, payload).expect("write source");
        let request = DirectObjectPutRequest::new(
            ObjectId::new("object-a").expect("object id"),
            &source_path,
            vec![DiskCopyRoot::new(
                DiskId::new("disk-a").expect("disk id"),
                &disk_a,
            )],
            1,
        );
        let mut progress_events = Vec::new();

        let report = put_object_direct_to_hdd_with_controlled_progress(request, |progress| {
            progress_events.push(progress);
            Ok(())
        })
        .expect("direct HDD put succeeds");

        let expected_hash = hash_file_sha256(&source_path).expect("hash source");
        assert_eq!(report.content_hash, expected_hash);
        assert_eq!(report.placements.len(), 1);
        assert_eq!(report.placements[0].content_hash, expected_hash);
        assert_eq!(
            fs::read(&report.placements[0].destination_path).expect("read placement"),
            payload
        );
        assert!(
            !disk_a
                .join(".dasobjectstore/direct-import/object-a")
                .exists(),
            "direct import should remove temporary HDD copy path after content-addressed rename"
        );
        assert!(progress_events
            .iter()
            .any(|event| matches!(event.stage, super::ObjectPutProgressStage::HddCopy { .. })));
        assert!(progress_events.iter().any(|event| matches!(
            event.stage,
            super::ObjectPutProgressStage::HddFsync {
                duration_millis: None,
                ..
            }
        )));
        assert!(progress_events.iter().any(|event| matches!(
            event.stage,
            super::ObjectPutProgressStage::HddFsync {
                duration_millis: Some(_),
                ..
            }
        )));
        assert!(progress_events.iter().any(|event| matches!(
            event.stage,
            super::ObjectPutProgressStage::HddRename {
                duration_millis: None,
                ..
            }
        )));
        assert!(progress_events.iter().any(|event| matches!(
            event.stage,
            super::ObjectPutProgressStage::HddRename {
                duration_millis: Some(_),
                ..
            }
        )));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn direct_hdd_put_fans_out_to_three_distinct_disks() {
        let root = temp_root("object-put-direct-hdd-fanout");
        let source_path = root.join("source.fastq.gz");
        let payload = vec![0x5a_u8; 192 * 1024];
        fs::create_dir_all(&root).expect("create temp root");
        fs::write(&source_path, &payload).expect("write source");
        let request = DirectObjectPutRequest::new(
            ObjectId::new("object-a").expect("object id"),
            &source_path,
            vec![
                DiskCopyRoot::new(DiskId::new("disk-a").expect("disk id"), root.join("disk-a")),
                DiskCopyRoot::new(DiskId::new("disk-b").expect("disk id"), root.join("disk-b")),
                DiskCopyRoot::new(DiskId::new("disk-c").expect("disk id"), root.join("disk-c")),
            ],
            3,
        );
        let mut active_targets = Vec::new();

        let report = put_object_direct_to_hdd_with_controlled_progress(request, |progress| {
            if let super::ObjectPutProgressStage::HddCopy {
                disk_id,
                copy_number,
            } = progress.stage
            {
                active_targets.push((disk_id, copy_number));
            }
            Ok(())
        })
        .expect("direct HDD fan-out succeeds");

        assert_eq!(report.placements.len(), 3);
        for placement in &report.placements {
            assert_eq!(
                fs::read(&placement.destination_path).expect("read placement"),
                payload
            );
            assert!(active_targets
                .iter()
                .any(|(disk_id, copy_number)| disk_id == &placement.disk_id
                    && *copy_number == placement.copy_number));
        }

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn removes_active_ssd_job_root_when_object_put_is_cancelled() {
        let root = temp_root("object-put-cancelled");
        let source_path = root.join("source.fastq.gz");
        let ssd_root = root.join("ssd");
        let disk_a = root.join("disk-a");
        fs::create_dir_all(&root).expect("create temp root");
        fs::write(&source_path, vec![11_u8; 128 * 1024]).expect("write source");
        let request = ObjectPutRequest::new(
            ObjectId::new("object-a").expect("object id"),
            &source_path,
            &ssd_root,
            vec![DiskCopyRoot::new(
                DiskId::new("disk-a").expect("disk id"),
                &disk_a,
            )],
            1,
        );

        let err = put_object_ssd_first_with_controlled_progress(&request, |_| {
            Err(ObjectPutError::Cancelled)
        })
        .expect_err("object put cancelled");

        assert!(matches!(err, ObjectPutError::Cancelled));
        assert!(
            !ssd_root
                .join(".dasobjectstore")
                .join("ingest")
                .join("jobs")
                .join("put-object-a")
                .exists(),
            "cancelled object put should remove active SSD ingest job root"
        );

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    fn temp_root(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!("dasobjectstore-{name}-{nonce}"))
    }
}
