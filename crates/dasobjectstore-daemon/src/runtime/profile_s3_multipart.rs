//! Durable daemon-owned staging for reservation-bound multipart parts.
//!
//! The journal is deliberately independent of HTTP and provider SDKs. It
//! stores only logical identity and private relative filenames under the
//! profile's managed namespace. A completion handler can reopen the journal
//! after a request boundary and obtain verified part readers without trusting
//! client paths or keeping bytes in memory.

use crate::api::{
    ProviderStreamChunkHeader, ProviderStreamMultipartPartUploadOpenRequest,
    ProviderStreamValidationError, ProviderStreamVerifier,
};
use dasobjectstore_core::backend::BackendObjectKey;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::{self, Display};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const JOURNAL_SCHEMA_VERSION: &str = "dasobjectstore.profile_s3.multipart_journal.v1";
const NAMESPACE: &str = ".dasobjectstore";
const MULTIPART_DIR: &str = "multipart";
const MANIFEST_FILE: &str = "manifest.json";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct JournalManifest {
    schema_version: String,
    store_id: String,
    reservation_id: String,
    object: BackendObjectKey,
    reservation_size_bytes: u64,
    parts: Vec<JournalPart>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct JournalPart {
    part_number: u32,
    size_bytes: u64,
    checksum: String,
    file_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MultipartPartRecord {
    pub part_number: u32,
    pub size_bytes: u64,
    pub checksum: String,
}

pub struct MultipartPartJournal {
    directory: PathBuf,
    manifest: JournalManifest,
}

impl MultipartPartJournal {
    pub fn open(
        root: impl AsRef<Path>,
        request: &ProviderStreamMultipartPartUploadOpenRequest,
    ) -> Result<Self, MultipartPartJournalError> {
        validate_identity(request)?;
        let directory = root
            .as_ref()
            .join(NAMESPACE)
            .join(MULTIPART_DIR)
            .join(&request.reservation_id);
        fs::create_dir_all(&directory).map_err(io_error)?;
        let manifest_path = directory.join(MANIFEST_FILE);
        let manifest = if manifest_path.exists() {
            let bytes = fs::read(&manifest_path).map_err(io_error)?;
            let manifest: JournalManifest = serde_json::from_slice(&bytes)
                .map_err(|error| MultipartPartJournalError::Manifest(error.to_string()))?;
            validate_manifest(&manifest)?;
            if manifest.store_id != request.store_id.as_str()
                || manifest.reservation_id != request.reservation_id
                || manifest.object != request.object
                || manifest.reservation_size_bytes != request.reservation_size_bytes
            {
                return Err(MultipartPartJournalError::IdentityMismatch);
            }
            manifest
        } else {
            JournalManifest {
                schema_version: JOURNAL_SCHEMA_VERSION.to_string(),
                store_id: request.store_id.as_str().to_string(),
                reservation_id: request.reservation_id.clone(),
                object: request.object.clone(),
                reservation_size_bytes: request.reservation_size_bytes,
                parts: Vec::new(),
            }
        };
        Ok(Self {
            directory,
            manifest,
        })
    }

    pub fn staged_bytes(&self) -> u64 {
        self.manifest.parts.iter().map(|part| part.size_bytes).sum()
    }

    pub fn open_for_completion(
        root: impl AsRef<Path>,
        store_id: &str,
        reservation_id: &str,
        object: BackendObjectKey,
        reservation_size_bytes: u64,
    ) -> Result<Self, MultipartPartJournalError> {
        let store_id = dasobjectstore_core::ids::StoreId::new(store_id.to_string())
            .map_err(|_| MultipartPartJournalError::IdentityMismatch)?;
        let request = ProviderStreamMultipartPartUploadOpenRequest {
            schema_version: crate::api::PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
            request_id: "completion".to_string(),
            reservation_id: reservation_id.to_string(),
            reservation_size_bytes,
            part_number: 1,
            store_id,
            object,
            expected_size_bytes: reservation_size_bytes,
            expected_sha256: format!("sha256:{}", "0".repeat(64)),
            chunk_size_bytes: crate::api::PROVIDER_STREAM_MAX_CHUNK_BYTES,
        };
        let directory = root
            .as_ref()
            .join(NAMESPACE)
            .join(MULTIPART_DIR)
            .join(reservation_id);
        if !directory.join(MANIFEST_FILE).exists() {
            return Err(MultipartPartJournalError::Manifest(
                "multipart reservation journal is missing".to_string(),
            ));
        }
        Self::open(root, &request)
    }

    pub fn parts(&self) -> impl Iterator<Item = MultipartPartRecord> + '_ {
        self.manifest.parts.iter().map(|part| MultipartPartRecord {
            part_number: part.part_number,
            size_bytes: part.size_bytes,
            checksum: part.checksum.clone(),
        })
    }

    /// Consume and verify one bounded frame stream, then atomically publish
    /// the part file and manifest. A matching existing part is idempotent:
    /// the incoming frames are still consumed and verified, but the durable
    /// bytes are left untouched.
    pub fn stage_part(
        &mut self,
        request: &ProviderStreamMultipartPartUploadOpenRequest,
        read_frame: &mut dyn FnMut() -> Result<
            (ProviderStreamChunkHeader, Vec<u8>),
            MultipartPartJournalError,
        >,
    ) -> Result<MultipartPartRecord, MultipartPartJournalError> {
        validate_identity(request)?;
        if request.store_id.as_str() != self.manifest.store_id
            || request.reservation_id != self.manifest.reservation_id
            || request.object != self.manifest.object
            || request.reservation_size_bytes != self.manifest.reservation_size_bytes
        {
            return Err(MultipartPartJournalError::IdentityMismatch);
        }
        let temp_path = self
            .directory
            .join(format!(".part-{:08}.tmp", request.part_number));
        let final_name = format!("part-{:08}.bin", request.part_number);
        let final_path = self.directory.join(&final_name);
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp_path).map_err(io_error)?;
        let mut verifier = ProviderStreamVerifier::new(request.request_id.clone())
            .map_err(MultipartPartJournalError::Verification)?;
        let mut checksum = Sha256::new();
        let mut size = 0_u64;
        loop {
            let (header, payload) = read_frame()?;
            if header.final_chunk {
                verifier
                    .finish(&header, &payload)
                    .map_err(MultipartPartJournalError::Verification)?;
                file.write_all(&payload).map_err(io_error)?;
                checksum.update(&payload);
                size = size
                    .checked_add(payload.len() as u64)
                    .ok_or(MultipartPartJournalError::SizeOverflow)?;
                break;
            }
            verifier
                .push(&header, &payload)
                .map_err(MultipartPartJournalError::Verification)?;
            file.write_all(&payload).map_err(io_error)?;
            checksum.update(&payload);
            size = size
                .checked_add(payload.len() as u64)
                .ok_or(MultipartPartJournalError::SizeOverflow)?;
        }
        file.sync_all().map_err(io_error)?;
        let checksum = format!("sha256:{:x}", checksum.finalize());
        if size != request.expected_size_bytes || checksum != request.expected_sha256 {
            let _ = fs::remove_file(&temp_path);
            return Err(MultipartPartJournalError::Verification(
                crate::api::ProviderStreamVerificationError::InvalidHeader(
                    ProviderStreamValidationError::InvalidField {
                        field: "expected part size or checksum",
                    },
                ),
            ));
        }
        let record = MultipartPartRecord {
            part_number: request.part_number,
            size_bytes: size,
            checksum: checksum.clone(),
        };
        if let Some(existing) = self
            .manifest
            .parts
            .iter()
            .find(|part| part.part_number == request.part_number)
        {
            let _ = fs::remove_file(&temp_path);
            if existing.size_bytes != record.size_bytes || existing.checksum != record.checksum {
                return Err(MultipartPartJournalError::PartConflict);
            }
            return Ok(record);
        }
        if self
            .staged_bytes()
            .checked_add(size)
            .is_none_or(|total| total > self.manifest.reservation_size_bytes)
        {
            let _ = fs::remove_file(&temp_path);
            return Err(MultipartPartJournalError::ReservationExceeded);
        }
        fs::rename(&temp_path, &final_path).map_err(io_error)?;
        self.manifest.parts.push(JournalPart {
            part_number: request.part_number,
            size_bytes: size,
            checksum,
            file_name: final_name,
        });
        self.manifest.parts.sort_by_key(|part| part.part_number);
        self.persist().map_err(|error| {
            let _ = fs::remove_file(&final_path);
            error
        })?;
        Ok(record)
    }

    pub fn open_part(&self, part_number: u32) -> Result<File, MultipartPartJournalError> {
        let part = self
            .manifest
            .parts
            .iter()
            .find(|part| part.part_number == part_number)
            .ok_or(MultipartPartJournalError::PartNotFound)?;
        Ok(File::open(self.directory.join(&part.file_name)).map_err(io_error)?)
    }

    pub fn remove(self) -> Result<(), MultipartPartJournalError> {
        fs::remove_dir_all(self.directory).map_err(io_error)
    }

    fn persist(&self) -> Result<(), MultipartPartJournalError> {
        let temporary = self.directory.join(format!(".{MANIFEST_FILE}.tmp"));
        let encoded = serde_json::to_vec_pretty(&self.manifest)
            .map_err(|error| MultipartPartJournalError::Manifest(error.to_string()))?;
        {
            let mut options = OpenOptions::new();
            options.create(true).truncate(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temporary).map_err(io_error)?;
            file.write_all(&encoded).map_err(io_error)?;
            file.sync_all().map_err(io_error)?;
        }
        fs::rename(&temporary, self.directory.join(MANIFEST_FILE)).map_err(io_error)?;
        if let Some(parent) = self.directory.parent() {
            let directory = File::open(parent).map_err(io_error)?;
            directory.sync_all().map_err(io_error)?;
        }
        Ok(())
    }
}

/// Discover durable multipart reservations that must retain their capacity
/// lease across daemon request and restart boundaries. Any malformed or
/// mismatched journal fails the scan closed so maintenance cannot reclaim
/// accounting while staged parts may still be recoverable.
pub fn discover_multipart_reservation_ids(
    root: impl AsRef<Path>,
    expected_store_id: &str,
) -> Result<Vec<String>, MultipartPartJournalError> {
    let directory = root.as_ref().join(NAMESPACE).join(MULTIPART_DIR);
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(io_error(error)),
    };
    let mut reservation_ids = Vec::new();
    for entry in entries {
        let entry = entry.map_err(io_error)?;
        let file_type = entry.file_type().map_err(io_error)?;
        if !file_type.is_dir() || file_type.is_symlink() {
            return Err(MultipartPartJournalError::Manifest(
                "multipart namespace contains a non-directory entry".to_string(),
            ));
        }
        let directory_reservation_id = entry
            .file_name()
            .into_string()
            .map_err(|_| MultipartPartJournalError::UnsafeReservationId)?;
        if !safe_reservation_id(&directory_reservation_id) {
            return Err(MultipartPartJournalError::UnsafeReservationId);
        }
        let bytes = fs::read(entry.path().join(MANIFEST_FILE)).map_err(io_error)?;
        let manifest: JournalManifest = serde_json::from_slice(&bytes)
            .map_err(|error| MultipartPartJournalError::Manifest(error.to_string()))?;
        validate_manifest(&manifest)?;
        if manifest.reservation_id != directory_reservation_id {
            return Err(MultipartPartJournalError::IdentityMismatch);
        }
        if manifest.store_id != expected_store_id {
            // A named appliance pool is shared by multiple ObjectStores. The
            // manifest is valid but belongs to another store's lease scan.
            continue;
        }
        reservation_ids.push(manifest.reservation_id);
    }
    reservation_ids.sort();
    Ok(reservation_ids)
}

fn validate_identity(
    request: &ProviderStreamMultipartPartUploadOpenRequest,
) -> Result<(), MultipartPartJournalError> {
    request
        .validate()
        .map_err(MultipartPartJournalError::Request)?;
    if !safe_reservation_id(&request.reservation_id) {
        return Err(MultipartPartJournalError::UnsafeReservationId);
    }
    Ok(())
}

fn safe_reservation_id(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn validate_manifest(manifest: &JournalManifest) -> Result<(), MultipartPartJournalError> {
    if manifest.schema_version != JOURNAL_SCHEMA_VERSION
        || manifest.store_id.trim().is_empty()
        || manifest.reservation_id.trim().is_empty()
        || manifest.reservation_size_bytes == 0
    {
        return Err(MultipartPartJournalError::Manifest(
            "invalid multipart journal identity".to_string(),
        ));
    }
    let total = manifest.parts.iter().try_fold(0_u64, |total, part| {
        if part.part_number == 0
            || part.size_bytes == 0
            || !part.file_name.starts_with("part-")
            || !part.file_name.ends_with(".bin")
        {
            return Err(MultipartPartJournalError::Manifest(
                "invalid multipart journal part".to_string(),
            ));
        }
        total
            .checked_add(part.size_bytes)
            .filter(|total| *total <= manifest.reservation_size_bytes)
            .ok_or(MultipartPartJournalError::ReservationExceeded)
    })?;
    if total > manifest.reservation_size_bytes {
        return Err(MultipartPartJournalError::ReservationExceeded);
    }
    Ok(())
}

fn io_error(error: std::io::Error) -> MultipartPartJournalError {
    MultipartPartJournalError::Io(error.to_string())
}

#[derive(Debug)]
pub enum MultipartPartJournalError {
    Request(ProviderStreamValidationError),
    Verification(crate::api::ProviderStreamVerificationError),
    Io(String),
    Manifest(String),
    IdentityMismatch,
    UnsafeReservationId,
    PartConflict,
    PartNotFound,
    ReservationExceeded,
    SizeOverflow,
}

impl Display for MultipartPartJournalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Request(error) => Display::fmt(error, formatter),
            Self::Verification(error) => Display::fmt(error, formatter),
            Self::Io(error) => write!(formatter, "multipart journal IO failed: {error}"),
            Self::Manifest(error) => write!(formatter, "invalid multipart journal: {error}"),
            Self::IdentityMismatch => formatter.write_str("multipart journal identity mismatch"),
            Self::UnsafeReservationId => formatter.write_str("multipart reservation id is unsafe"),
            Self::PartConflict => {
                formatter.write_str("multipart part retry conflicts with staged part")
            }
            Self::PartNotFound => formatter.write_str("multipart part is not staged"),
            Self::ReservationExceeded => formatter.write_str("multipart reservation size exceeded"),
            Self::SizeOverflow => formatter.write_str("multipart part size overflowed"),
        }
    }
}

impl std::error::Error for MultipartPartJournalError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{write_provider_stream_frame, PROVIDER_STREAM_SCHEMA_VERSION};
    use dasobjectstore_core::ids::StoreId;
    use std::io::{Cursor, Read};

    fn request(part_number: u32, checksum: &str) -> ProviderStreamMultipartPartUploadOpenRequest {
        ProviderStreamMultipartPartUploadOpenRequest {
            schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
            request_id: format!("request-{part_number}"),
            reservation_id: "reservation-1".to_string(),
            reservation_size_bytes: 10,
            part_number,
            store_id: StoreId::new("store-1").expect("store"),
            object: BackendObjectKey {
                object_id: "object.bin".to_string(),
                version: 1,
            },
            expected_size_bytes: 5,
            expected_sha256: checksum.to_string(),
            chunk_size_bytes: 1024,
        }
    }

    #[test]
    fn stages_verified_part_and_reopens_after_request_boundary() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-multipart-journal-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root");
        let request = request(
            1,
            "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
        );
        let mut journal = MultipartPartJournal::open(&root, &request).expect("journal");
        let header = ProviderStreamChunkHeader {
            schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
            request_id: request.request_id.clone(),
            offset: 0,
            payload_len: 5,
            final_chunk: true,
            total_size: Some(5),
            sha256: Some(request.expected_sha256.clone()),
        };
        let mut frame = Vec::new();
        write_provider_stream_frame(&mut frame, &header, b"hello").expect("frame");
        let record = journal
            .stage_part(&request, &mut || {
                crate::api::read_provider_stream_frame(&mut Cursor::new(frame.clone()))
                    .map_err(|error| MultipartPartJournalError::Io(error.to_string()))
            })
            .expect("stage");
        let retry = journal
            .stage_part(&request, &mut || {
                crate::api::read_provider_stream_frame(&mut Cursor::new(frame.clone()))
                    .map_err(|error| MultipartPartJournalError::Io(error.to_string()))
            })
            .expect("idempotent retry");
        assert_eq!(retry, record);
        assert_eq!(journal.staged_bytes(), 5);
        drop(journal);
        let reopened = MultipartPartJournal::open(&root, &request).expect("reopen");
        let mut reader = reopened.open_part(1).expect("part");
        let mut payload = Vec::new();
        reader.read_to_end(&mut payload).expect("read");
        assert_eq!(payload, b"hello");
        drop(reopened);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn discovers_only_valid_store_bound_multipart_reservations() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-multipart-discovery-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root");
        let request = request(
            1,
            "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
        );
        let journal = MultipartPartJournal::open(&root, &request).expect("journal");
        journal.persist().expect("persist journal identity");
        drop(journal);
        let mut other_request = request.clone();
        other_request.store_id = StoreId::new("other-store").expect("other store");
        other_request.reservation_id = "reservation-2".to_string();
        let other = MultipartPartJournal::open(&root, &other_request).expect("other journal");
        other.persist().expect("persist other journal identity");
        drop(other);

        assert_eq!(
            discover_multipart_reservation_ids(&root, "store-1").expect("discover"),
            vec!["reservation-1".to_string()]
        );
        assert!(discover_multipart_reservation_ids(&root, "other-store")
            .expect("other store scan")
            .contains(&"reservation-2".to_string()));
        let _ = std::fs::remove_dir_all(root);
    }
}
