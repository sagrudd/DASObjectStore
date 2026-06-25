use crate::hash::{copy_and_hash, hash_file_sha256, SHA256_ALGORITHM};
use dasobjectstore_core::ids::{DiskId, ObjectId};
use std::fmt::{self, Display};
use std::fs::{self, File};
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
    HashMismatch { expected: String, actual: String },
}

impl Display for HddCopyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(formatter, "HDD copy IO failed: {err}"),
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
    if let Some(parent) = request.destination_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut source = File::open(&request.source_path)?;
    let mut destination = File::create(&request.destination_path)?;
    let write_report = copy_and_hash(&mut source, &mut destination)?;
    destination.sync_all()?;

    let content_hash = verify_hdd_copy_hash(
        &request.destination_path,
        request.expected_content_hash.as_str(),
    )?;

    Ok(HddCopyReport {
        object_id: request.object_id.clone(),
        disk_id: request.disk_id.clone(),
        copy_number: request.copy_number,
        destination_path: request.destination_path.clone(),
        bytes_written: write_report.bytes_written,
        content_hash_algorithm: HDD_COPY_CONTENT_HASH_ALGORITHM.to_string(),
        content_hash,
    })
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
    use super::{write_verified_hdd_copy, HddCopyError, HddCopyRequest};
    use crate::hash::hash_file_sha256;
    use dasobjectstore_core::ids::{DiskId, ObjectId};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn writes_hdd_copy_and_verifies_hash_from_destination_readback() {
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
}
