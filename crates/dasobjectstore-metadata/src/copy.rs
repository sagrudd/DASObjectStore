use crate::hash::{copy_and_hash_with_controlled_progress, hash_file_sha256, SHA256_ALGORITHM};
use crate::secure_fs::{create_private_dir_all, create_private_file, set_private_dir_permissions};
use dasobjectstore_core::ids::{DiskId, ObjectId};
use std::fmt::{self, Display};
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

pub const HDD_COPY_CONTENT_HASH_ALGORITHM: &str = SHA256_ALGORITHM;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HddCopyRequest {
    pub object_id: ObjectId,
    pub disk_id: DiskId,
    pub copy_number: u8,
    pub source_path: PathBuf,
    pub destination_path: PathBuf,
    pub expected_content_hash: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HddInlineHashCopyRequest {
    pub object_id: ObjectId,
    pub disk_id: DiskId,
    pub copy_number: u8,
    pub source_path: PathBuf,
    pub destination_path: PathBuf,
}

impl HddInlineHashCopyRequest {
    pub fn new(
        object_id: ObjectId,
        disk_id: DiskId,
        copy_number: u8,
        source_path: impl Into<PathBuf>,
        destination_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            object_id,
            disk_id,
            copy_number,
            source_path: source_path.into(),
            destination_path: destination_path.into(),
        }
    }
}

impl HddCopyRequest {
    pub fn new(
        object_id: ObjectId,
        disk_id: DiskId,
        copy_number: u8,
        source_path: impl Into<PathBuf>,
        destination_path: impl Into<PathBuf>,
        expected_content_hash: impl Into<String>,
    ) -> Self {
        Self {
            object_id,
            disk_id,
            copy_number,
            source_path: source_path.into(),
            destination_path: destination_path.into(),
            expected_content_hash: expected_content_hash.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HddCopyReport {
    pub object_id: ObjectId,
    pub disk_id: DiskId,
    pub copy_number: u8,
    pub destination_path: PathBuf,
    pub bytes_written: u64,
    pub content_hash_algorithm: String,
    pub content_hash: String,
}

#[derive(Debug)]
pub enum HddCopyError {
    Io(std::io::Error),
    Cancelled,
    HashMismatch { expected: String, actual: String },
}

impl Display for HddCopyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(formatter, "HDD copy IO failed: {err}"),
            Self::Cancelled => formatter.write_str("HDD copy cancelled"),
            Self::HashMismatch { expected, actual } => {
                write!(
                    formatter,
                    "HDD copy hash mismatch: expected {expected}, got {actual}"
                )
            }
        }
    }
}

impl std::error::Error for HddCopyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Cancelled => None,
            Self::HashMismatch { .. } => None,
        }
    }
}

impl From<std::io::Error> for HddCopyError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

pub fn write_verified_hdd_copy(request: &HddCopyRequest) -> Result<HddCopyReport, HddCopyError> {
    write_verified_hdd_copy_with_progress(request, |_| {})
}

pub fn write_verified_hdd_copy_with_progress(
    request: &HddCopyRequest,
    mut progress: impl FnMut(u64),
) -> Result<HddCopyReport, HddCopyError> {
    write_verified_hdd_copy_with_controlled_progress(request, |bytes_written| {
        progress(bytes_written);
        Ok(())
    })
}

pub fn write_verified_hdd_copy_with_controlled_progress(
    request: &HddCopyRequest,
    progress: impl FnMut(u64) -> Result<(), HddCopyError>,
) -> Result<HddCopyReport, HddCopyError> {
    if let Some(parent) = request.destination_path.parent() {
        create_private_dir_all(parent)?;
        restrict_object_tree_dirs(parent)?;
    }

    let report = write_verified_hdd_copy_inner(request, progress);
    if report.is_err() {
        let _ = fs::remove_file(&request.destination_path);
    }

    report
}

pub fn write_hdd_copy_with_inline_hash_with_controlled_progress(
    request: &HddInlineHashCopyRequest,
    progress: impl FnMut(u64) -> Result<(), HddCopyError>,
) -> Result<HddCopyReport, HddCopyError> {
    if let Some(parent) = request.destination_path.parent() {
        create_private_dir_all(parent)?;
        restrict_object_tree_dirs(parent)?;
    }

    let report = write_hdd_copy_with_inline_hash_inner(request, progress);
    if report.is_err() {
        let _ = fs::remove_file(&request.destination_path);
    }

    report
}

fn write_verified_hdd_copy_inner(
    request: &HddCopyRequest,
    mut progress: impl FnMut(u64) -> Result<(), HddCopyError>,
) -> Result<HddCopyReport, HddCopyError> {
    let mut source = File::open(&request.source_path)?;
    let mut destination = create_private_file(&request.destination_path)?;
    let write_report =
        copy_and_hash_with_controlled_progress(&mut source, &mut destination, |bytes_written| {
            progress(bytes_written).map_err(hdd_copy_error_to_io)
        })
        .map_err(hdd_copy_error_from_io)?;
    destination.sync_all()?;

    if write_report.content_hash != request.expected_content_hash {
        return Err(HddCopyError::HashMismatch {
            expected: request.expected_content_hash.clone(),
            actual: write_report.content_hash,
        });
    }

    Ok(HddCopyReport {
        object_id: request.object_id.clone(),
        disk_id: request.disk_id.clone(),
        copy_number: request.copy_number,
        destination_path: request.destination_path.clone(),
        bytes_written: write_report.bytes_written,
        content_hash_algorithm: HDD_COPY_CONTENT_HASH_ALGORITHM.to_string(),
        content_hash: request.expected_content_hash.clone(),
    })
}

fn write_hdd_copy_with_inline_hash_inner(
    request: &HddInlineHashCopyRequest,
    mut progress: impl FnMut(u64) -> Result<(), HddCopyError>,
) -> Result<HddCopyReport, HddCopyError> {
    let mut source = File::open(&request.source_path)?;
    let mut destination = create_private_file(&request.destination_path)?;
    let write_report =
        copy_and_hash_with_controlled_progress(&mut source, &mut destination, |bytes_written| {
            progress(bytes_written).map_err(hdd_copy_error_to_io)
        })
        .map_err(hdd_copy_error_from_io)?;
    destination.sync_all()?;

    Ok(HddCopyReport {
        object_id: request.object_id.clone(),
        disk_id: request.disk_id.clone(),
        copy_number: request.copy_number,
        destination_path: request.destination_path.clone(),
        bytes_written: write_report.bytes_written,
        content_hash_algorithm: HDD_COPY_CONTENT_HASH_ALGORITHM.to_string(),
        content_hash: write_report.content_hash,
    })
}

fn hdd_copy_error_to_io(error: HddCopyError) -> io::Error {
    match error {
        HddCopyError::Io(error) => error,
        HddCopyError::Cancelled => io::Error::new(io::ErrorKind::Interrupted, "HDD copy cancelled"),
        HddCopyError::HashMismatch { expected, actual } => io::Error::other(format!(
            "HDD copy hash mismatch: expected {expected}, got {actual}"
        )),
    }
}

fn hdd_copy_error_from_io(error: io::Error) -> HddCopyError {
    if error.kind() == io::ErrorKind::Interrupted {
        HddCopyError::Cancelled
    } else {
        HddCopyError::Io(error)
    }
}

fn restrict_object_tree_dirs(payload_parent: &Path) -> Result<(), HddCopyError> {
    set_private_dir_permissions(payload_parent)?;
    if let Some(prefix_dir) = payload_parent.parent() {
        set_private_dir_permissions(prefix_dir)?;
        if let Some(objects_dir) = prefix_dir.parent() {
            set_private_dir_permissions(objects_dir)?;
        }
    }

    Ok(())
}

pub fn verify_hdd_copy_hash(
    copy_path: impl AsRef<Path>,
    expected_content_hash: &str,
) -> Result<String, HddCopyError> {
    let actual = hash_file_sha256(copy_path)?;
    if actual != expected_content_hash {
        return Err(HddCopyError::HashMismatch {
            expected: expected_content_hash.to_string(),
            actual,
        });
    }

    Ok(actual)
}

#[cfg(test)]
mod tests {
    use super::{
        write_verified_hdd_copy, write_verified_hdd_copy_with_controlled_progress, HddCopyError,
        HddCopyRequest,
    };
    use crate::hash::hash_file_sha256;
    #[cfg(unix)]
    use crate::secure_fs::{PRIVATE_DIR_MODE, PRIVATE_FILE_MODE};
    use dasobjectstore_core::ids::{DiskId, ObjectId};
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn writes_hdd_copy_and_verifies_hash_inline() {
        let root = temp_root("hdd-copy-ok");
        let source_path = root.join("ssd").join("payload");
        let destination_path = root.join("hdd-a").join("objects").join("object-a");
        let payload = b"bioinformatics object payload";
        fs::create_dir_all(source_path.parent().expect("source parent")).expect("source dir");
        fs::write(&source_path, payload).expect("source payload");
        let expected_hash = hash_file_sha256(&source_path).expect("source hash");
        let request = request(source_path, destination_path.clone(), expected_hash.clone());

        let report = write_verified_hdd_copy(&request).expect("verified copy");

        assert_eq!(report.object_id.as_str(), "object-a");
        assert_eq!(report.disk_id.as_str(), "disk-a");
        assert_eq!(report.copy_number, 1);
        assert_eq!(report.destination_path, destination_path);
        assert_eq!(report.bytes_written, payload.len() as u64);
        assert_eq!(report.content_hash_algorithm, "sha256");
        assert_eq!(report.content_hash, expected_hash);
        assert_eq!(
            fs::read(report.destination_path).expect("destination payload"),
            payload
        );
        #[cfg(unix)]
        assert_private_payload_tree(&destination_path);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reports_hash_mismatch_for_invalid_copy_payload() {
        let root = temp_root("hdd-copy-mismatch");
        let source_path = root.join("ssd").join("payload");
        let destination_path = root.join("hdd-a").join("objects").join("object-a");
        fs::create_dir_all(source_path.parent().expect("source parent")).expect("source dir");
        fs::write(&source_path, b"unexpected payload").expect("source payload");
        let request = request(
            source_path,
            destination_path,
            "not-the-real-hash".to_string(),
        );

        let err = write_verified_hdd_copy(&request).expect_err("hash mismatch");

        assert!(matches!(err, HddCopyError::HashMismatch { .. }));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn removes_partial_destination_when_hdd_copy_is_cancelled() {
        let root = temp_root("hdd-copy-cancelled");
        let source_path = root.join("ssd").join("payload");
        let destination_path = root.join("hdd-a").join("objects").join("object-a");
        fs::create_dir_all(source_path.parent().expect("source parent")).expect("source dir");
        fs::write(&source_path, vec![7_u8; 128 * 1024]).expect("source payload");
        let expected_hash = hash_file_sha256(&source_path).expect("source hash");
        let request = request(source_path, destination_path.clone(), expected_hash);

        let err = write_verified_hdd_copy_with_controlled_progress(&request, |_| {
            Err(HddCopyError::Cancelled)
        })
        .expect_err("copy cancelled");

        assert!(matches!(err, HddCopyError::Cancelled));
        assert!(
            !destination_path.exists(),
            "cancelled HDD copy should remove partial destination payload"
        );

        let _ = fs::remove_dir_all(root);
    }

    fn request(
        source_path: PathBuf,
        destination_path: PathBuf,
        expected_hash: String,
    ) -> HddCopyRequest {
        HddCopyRequest::new(
            ObjectId::new("object-a").expect("object id"),
            DiskId::new("disk-a").expect("disk id"),
            1,
            source_path,
            destination_path,
            expected_hash,
        )
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

    #[cfg(unix)]
    fn assert_private_payload_tree(payload_path: &std::path::Path) {
        assert_eq!(
            fs::metadata(payload_path)
                .expect("payload metadata")
                .permissions()
                .mode()
                & 0o777,
            PRIVATE_FILE_MODE
        );

        let object_dir = payload_path.parent().expect("object dir");
        let prefix_dir = object_dir.parent().expect("prefix dir");
        let objects_dir = prefix_dir.parent().expect("objects dir");
        for directory in [object_dir, prefix_dir, objects_dir] {
            assert_eq!(
                fs::metadata(directory)
                    .expect("directory metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                PRIVATE_DIR_MODE
            );
        }
    }
}
