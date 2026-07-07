use crate::copy::{write_verified_hdd_copy_with_progress, HddCopyError, HddCopyRequest};
use crate::evacuation::DiskCopyRoot;
use crate::ingest::{encode_path_component, IngestStagingLayout, IngestWriteReport};
use dasobjectstore_core::ids::{IngestJobId, InvalidId, ObjectId};
use serde::Serialize;
use std::fmt::{self, Display};
use std::fs::File;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectPutRequest {
    pub object_id: ObjectId,
    pub source_path: PathBuf,
    pub ssd_root: PathBuf,
    pub disk_roots: Vec<DiskCopyRoot>,
    pub copy_count: u8,
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
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ObjectPutReport {
    pub object_id: ObjectId,
    pub source_path: PathBuf,
    pub staged_payload_path: PathBuf,
    pub bytes_staged: u64,
    pub content_hash_algorithm: String,
    pub content_hash: String,
    pub placements: Vec<ObjectPutPlacementReport>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
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
    HddCopy { disk_id: String, copy_number: u8 },
}

pub fn put_object_ssd_first(request: &ObjectPutRequest) -> Result<ObjectPutReport, ObjectPutError> {
    put_object_ssd_first_with_progress(request, |_| {})
}

pub fn put_object_ssd_first_with_progress(
    request: &ObjectPutRequest,
    mut progress: impl FnMut(ObjectPutProgress),
) -> Result<ObjectPutReport, ObjectPutError> {
    validate_request(request)?;

    let layout = IngestStagingLayout::for_ssd_root(&request.ssd_root);
    layout.create_base_directories()?;
    let job_id = IngestJobId::new(format!("put-{}", request.object_id.as_str()))?;
    let job_paths = layout.job_paths(&job_id);

    let mut source = File::open(&request.source_path)?;
    let write_report =
        job_paths.write_payload_with_hash_progress(&mut source, |bytes_written| {
            progress(ObjectPutProgress {
                object_id: request.object_id.clone(),
                stage: ObjectPutProgressStage::SsdIngest,
                bytes_written,
            });
        })?;
    let placements = write_requested_copies(
        request,
        &job_paths.payload_path,
        &write_report,
        &mut progress,
    )?;

    Ok(ObjectPutReport {
        object_id: request.object_id.clone(),
        source_path: request.source_path.clone(),
        staged_payload_path: job_paths.payload_path,
        bytes_staged: write_report.bytes_written,
        content_hash_algorithm: write_report.content_hash_algorithm,
        content_hash: write_report.content_hash,
        placements,
    })
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
    request: &ObjectPutRequest,
    source_path: &Path,
    write_report: &IngestWriteReport,
    progress: &mut impl FnMut(ObjectPutProgress),
) -> Result<Vec<ObjectPutPlacementReport>, ObjectPutError> {
    request
        .disk_roots
        .iter()
        .take(request.copy_count as usize)
        .enumerate()
        .map(|(index, disk_root)| {
            let copy_number = (index + 1) as u8;
            let destination_path =
                object_copy_path(disk_root, &request.object_id, &write_report.content_hash);
            let copy_report = write_verified_hdd_copy_with_progress(
                &HddCopyRequest::new(
                    request.object_id.clone(),
                    disk_root.disk_id.clone(),
                    copy_number,
                    source_path,
                    destination_path,
                    write_report.content_hash.clone(),
                ),
                |bytes_written| {
                    progress(ObjectPutProgress {
                        object_id: request.object_id.clone(),
                        stage: ObjectPutProgressStage::HddCopy {
                            disk_id: disk_root.disk_id.as_str().to_string(),
                            copy_number,
                        },
                        bytes_written,
                    });
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

#[derive(Debug)]
pub enum ObjectPutError {
    Io(std::io::Error),
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
            Self::InvalidCopyCount => None,
            Self::InvalidIngestJobId(err) => Some(err),
            Self::NotEnoughDiskRoots { .. } => None,
            Self::Copy(err) => Some(err),
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
    use super::{put_object_ssd_first, ObjectPutError, ObjectPutRequest};
    use crate::evacuation::DiskCopyRoot;
    use crate::hash::hash_file_sha256;
    use dasobjectstore_core::ids::{DiskId, ObjectId};
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
        assert_eq!(report.bytes_staged, payload.len() as u64);
        assert_eq!(report.content_hash, expected_hash);
        assert_eq!(report.placements.len(), 2);
        assert_eq!(
            fs::read(&report.staged_payload_path).expect("read staged"),
            payload
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

    fn temp_root(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!("dasobjectstore-{name}-{nonce}"))
    }
}
