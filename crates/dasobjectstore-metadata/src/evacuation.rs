use crate::copy::{write_verified_hdd_copy, HddCopyError, HddCopyReport, HddCopyRequest};
use dasobjectstore_core::ids::{DiskId, ObjectId};
use dasobjectstore_core::repair::EvacuationPlan;
use std::fmt::{self, Display};
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvacuationExecutionRequest {
    pub plan: EvacuationPlan,
    pub object_sources: Vec<EvacuationObjectSource>,
    pub disk_roots: Vec<DiskCopyRoot>,
}

impl EvacuationExecutionRequest {
    pub fn new(
        plan: EvacuationPlan,
        object_sources: Vec<EvacuationObjectSource>,
        disk_roots: Vec<DiskCopyRoot>,
    ) -> Self {
        Self {
            plan,
            object_sources,
            disk_roots,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvacuationObjectSource {
    pub object_id: ObjectId,
    pub source_path: PathBuf,
    pub relative_destination_path: PathBuf,
    pub expected_content_hash: String,
}

impl EvacuationObjectSource {
    pub fn new(
        object_id: ObjectId,
        source_path: impl Into<PathBuf>,
        relative_destination_path: impl Into<PathBuf>,
        expected_content_hash: impl Into<String>,
    ) -> Self {
        Self {
            object_id,
            source_path: source_path.into(),
            relative_destination_path: relative_destination_path.into(),
            expected_content_hash: expected_content_hash.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiskCopyRoot {
    pub disk_id: DiskId,
    pub root_path: PathBuf,
}

impl DiskCopyRoot {
    pub fn new(disk_id: DiskId, root_path: impl Into<PathBuf>) -> Self {
        Self {
            disk_id,
            root_path: root_path.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvacuationExecutionReport {
    pub source_disk_id: DiskId,
    pub copy_reports: Vec<HddCopyReport>,
}

#[derive(Debug)]
pub enum EvacuationExecutionError {
    MissingObjectSource { object_id: ObjectId },
    MissingDiskRoot { disk_id: DiskId },
    Copy(HddCopyError),
}

impl Display for EvacuationExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingObjectSource { object_id } => {
                write!(
                    formatter,
                    "missing evacuation source for object {object_id}"
                )
            }
            Self::MissingDiskRoot { disk_id } => {
                write!(
                    formatter,
                    "missing evacuation destination root for disk {disk_id}"
                )
            }
            Self::Copy(err) => err.fmt(formatter),
        }
    }
}

impl std::error::Error for EvacuationExecutionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Copy(err) => Some(err),
            Self::MissingObjectSource { .. } | Self::MissingDiskRoot { .. } => None,
        }
    }
}

impl From<HddCopyError> for EvacuationExecutionError {
    fn from(err: HddCopyError) -> Self {
        Self::Copy(err)
    }
}

pub fn execute_evacuation_plan(
    request: &EvacuationExecutionRequest,
) -> Result<EvacuationExecutionReport, EvacuationExecutionError> {
    let mut copy_reports = Vec::new();

    for task in &request.plan.tasks {
        let source = object_source_for(request, &task.object_id)?;

        for planned_copy in &task.replacement_plan.planned_copies {
            let disk_root = disk_root_for(request, &planned_copy.disk_id)?;
            let copy_request = HddCopyRequest::new(
                task.object_id.clone(),
                planned_copy.disk_id.clone(),
                planned_copy.copy_number,
                source.source_path.clone(),
                disk_root.root_path.join(&source.relative_destination_path),
                source.expected_content_hash.clone(),
            );

            copy_reports.push(write_verified_hdd_copy(&copy_request)?);
        }
    }

    Ok(EvacuationExecutionReport {
        source_disk_id: request.plan.source_disk_id.clone(),
        copy_reports,
    })
}

fn object_source_for<'a>(
    request: &'a EvacuationExecutionRequest,
    object_id: &ObjectId,
) -> Result<&'a EvacuationObjectSource, EvacuationExecutionError> {
    request
        .object_sources
        .iter()
        .find(|source| &source.object_id == object_id)
        .ok_or_else(|| EvacuationExecutionError::MissingObjectSource {
            object_id: object_id.clone(),
        })
}

fn disk_root_for<'a>(
    request: &'a EvacuationExecutionRequest,
    disk_id: &DiskId,
) -> Result<&'a DiskCopyRoot, EvacuationExecutionError> {
    request
        .disk_roots
        .iter()
        .find(|root| &root.disk_id == disk_id)
        .ok_or_else(|| EvacuationExecutionError::MissingDiskRoot {
            disk_id: disk_id.clone(),
        })
}

#[cfg(test)]
mod tests {
    use super::{
        execute_evacuation_plan, DiskCopyRoot, EvacuationExecutionError,
        EvacuationExecutionRequest, EvacuationObjectSource,
    };
    use crate::copy::HddCopyError;
    use crate::hash::hash_file_sha256;
    use dasobjectstore_core::ids::{DiskId, ObjectId, StoreId};
    use dasobjectstore_core::placement::{CopyPlan, PlacementScore, PlannedCopy};
    use dasobjectstore_core::repair::{EvacuationPlan, EvacuationTask};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn executes_evacuation_plan_with_verified_copy() {
        let root = temp_root("evacuation-copy-ok");
        let source_path = root.join("source").join("object-a");
        let destination_root = root.join("disk-c");
        let relative_destination_path = PathBuf::from("objects/ob/object-a");
        fs::create_dir_all(source_path.parent().expect("source parent")).expect("source dir");
        fs::write(&source_path, b"protected object payload").expect("source payload");
        let expected_hash = hash_file_sha256(&source_path).expect("source hash");
        let request = request(
            plan("disk-a", "disk-c"),
            source(
                "object-a",
                &source_path,
                &relative_destination_path,
                &expected_hash,
            ),
            vec![DiskCopyRoot::new(disk("disk-c"), &destination_root)],
        );

        let report = execute_evacuation_plan(&request).expect("evacuation execution");

        assert_eq!(report.source_disk_id.as_str(), "disk-a");
        assert_eq!(report.copy_reports.len(), 1);
        assert_eq!(report.copy_reports[0].disk_id.as_str(), "disk-c");
        assert_eq!(report.copy_reports[0].content_hash, expected_hash);
        assert_eq!(
            fs::read(destination_root.join(relative_destination_path)).expect("copy payload"),
            b"protected object payload"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reports_missing_destination_root_before_copying() {
        let root = temp_root("evacuation-missing-root");
        let source_path = root.join("source").join("object-a");
        fs::create_dir_all(source_path.parent().expect("source parent")).expect("source dir");
        fs::write(&source_path, b"protected object payload").expect("source payload");
        let expected_hash = hash_file_sha256(&source_path).expect("source hash");
        let request = request(
            plan("disk-a", "disk-c"),
            source("object-a", &source_path, "objects/object-a", &expected_hash),
            Vec::new(),
        );

        let err = execute_evacuation_plan(&request).expect_err("missing destination root");

        assert!(matches!(
            err,
            EvacuationExecutionError::MissingDiskRoot { .. }
        ));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn propagates_hash_mismatch_from_verified_copy() {
        let root = temp_root("evacuation-hash-mismatch");
        let source_path = root.join("source").join("object-a");
        fs::create_dir_all(source_path.parent().expect("source parent")).expect("source dir");
        fs::write(&source_path, b"protected object payload").expect("source payload");
        let request = request(
            plan("disk-a", "disk-c"),
            source("object-a", &source_path, "objects/object-a", "not-the-hash"),
            vec![DiskCopyRoot::new(disk("disk-c"), root.join("disk-c"))],
        );

        let err = execute_evacuation_plan(&request).expect_err("hash mismatch");

        assert!(matches!(
            err,
            EvacuationExecutionError::Copy(HddCopyError::HashMismatch { .. })
        ));

        let _ = fs::remove_dir_all(root);
    }

    fn request(
        plan: EvacuationPlan,
        object_source: EvacuationObjectSource,
        disk_roots: Vec<DiskCopyRoot>,
    ) -> EvacuationExecutionRequest {
        EvacuationExecutionRequest::new(plan, vec![object_source], disk_roots)
    }

    fn source(
        object_id: &str,
        source_path: impl Into<PathBuf>,
        relative_destination_path: impl Into<PathBuf>,
        expected_hash: &str,
    ) -> EvacuationObjectSource {
        EvacuationObjectSource::new(
            ObjectId::new(object_id).expect("object id"),
            source_path,
            relative_destination_path,
            expected_hash,
        )
    }

    fn plan(source_disk_id: &str, destination_disk_id: &str) -> EvacuationPlan {
        EvacuationPlan {
            source_disk_id: disk(source_disk_id),
            tasks: vec![EvacuationTask {
                object_id: ObjectId::new("object-a").expect("object id"),
                store_id: StoreId::new("store-a").expect("store id"),
                source_disk_id: disk(source_disk_id),
                replacement_plan: CopyPlan {
                    requested_copies: 1,
                    planned_copies: vec![PlannedCopy {
                        copy_number: 1,
                        disk_id: disk(destination_disk_id),
                        score: PlacementScore {
                            disk_id: disk(destination_disk_id),
                            total: 250,
                            capacity_score: 50,
                            health_score: 100,
                            performance_score: 70,
                            write_load_score: 30,
                        },
                    }],
                },
            }],
            blocked_objects: Vec::new(),
        }
    }

    fn disk(disk_id: &str) -> DiskId {
        DiskId::new(disk_id).expect("disk id")
    }

    fn temp_root(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dasobjectstore-metadata-{name}-{}-{nanos}",
            std::process::id()
        ))
    }
}
