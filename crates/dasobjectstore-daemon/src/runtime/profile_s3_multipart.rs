//! Durable daemon-owned staging for reservation-bound multipart parts.
//!
//! The journal is deliberately independent of HTTP and provider SDKs. It
//! stores only logical identity and private relative filenames under the
//! profile's managed namespace. A completion handler can reopen the journal
//! after a request boundary and obtain verified part readers without trusting
//! client paths or keeping bytes in memory.

use crate::api::{
    ProfileS3MultipartCompletionPhase, ProfileS3MultipartCompletionState,
    ProfileS3MultipartCompletionStatus, ProviderStreamChunkHeader,
    ProviderStreamMultipartPartUploadOpenRequest, ProviderStreamValidationError,
    ProviderStreamVerifier,
};
use dasobjectstore_core::backend::BackendObjectKey;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt::{self, Display};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const JOURNAL_SCHEMA_VERSION: &str = "dasobjectstore.profile_s3.multipart_journal.v1";
const NAMESPACE: &str = ".dasobjectstore";
const MULTIPART_DIR: &str = "multipart";
const MANIFEST_FILE: &str = "manifest.json";

#[path = "profile_s3_multipart_completion.rs"]
mod completion;
use completion::{completion_job_id, completion_status};
pub use completion::{
    inspect_multipart_completion_status, list_recoverable_multipart_uploads,
    multipart_completion_job_id, MultipartUploadStatusRecord,
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct JournalManifest {
    schema_version: String,
    store_id: String,
    reservation_id: String,
    object: BackendObjectKey,
    reservation_size_bytes: u64,
    parts: Vec<JournalPart>,
    #[serde(default)]
    lifecycle: MultipartLifecycle,
    #[serde(default)]
    created_at_unix_seconds: u64,
    #[serde(default)]
    updated_at_unix_seconds: u64,
    #[serde(default)]
    completion_intent: Option<MultipartCompletionIntent>,
    #[serde(default)]
    completion_receipt: Option<MultipartCompletionReceipt>,
    #[serde(default)]
    completion_job: Option<ProfileS3MultipartCompletionStatus>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum MultipartLifecycle {
    #[default]
    Receiving,
    Completing,
    Committed,
    Aborted,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct MultipartCompletionIntent {
    object: BackendObjectKey,
    expected_size_bytes: u64,
    parts: Vec<MultipartPartRecord>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MultipartCompletionReceipt {
    pub object: BackendObjectKey,
    pub size_bytes: u64,
    pub checksum: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MultipartCompletionClaim {
    Started,
    Resuming,
    Committed(MultipartCompletionReceipt),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct JournalPart {
    part_number: u32,
    size_bytes: u64,
    checksum: String,
    file_name: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MultipartPartRecord {
    pub part_number: u32,
    pub size_bytes: u64,
    pub checksum: String,
}

pub struct MultipartPartJournal {
    directory: PathBuf,
    manifest: JournalManifest,
    _active: ActiveMultipartPart,
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
        let active = ActiveMultipartPart::acquire(&directory)?;
        let manifest_path = directory.join(MANIFEST_FILE);
        let manifest = if manifest_path.exists() {
            let bytes = fs::read(&manifest_path).map_err(io_error)?;
            let manifest: JournalManifest = serde_json::from_slice(&bytes)
                .map_err(|error| MultipartPartJournalError::Manifest(error.to_string()))?;
            validate_manifest(&manifest)?;
            if manifest.store_id != request.store_id.as_str()
                || manifest.reservation_id != request.reservation_id
                || manifest.object != request.object
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
                lifecycle: MultipartLifecycle::Receiving,
                created_at_unix_seconds: now_unix_seconds(),
                updated_at_unix_seconds: now_unix_seconds(),
                completion_intent: None,
                completion_receipt: None,
                completion_job: None,
            }
        };
        Ok(Self {
            directory,
            manifest,
            _active: active,
        })
    }

    pub fn staged_bytes(&self) -> u64 {
        self.manifest.parts.iter().map(|part| part.size_bytes).sum()
    }

    pub fn contains_part(&self, part_number: u32) -> bool {
        self.manifest
            .parts
            .iter()
            .any(|part| part.part_number == part_number)
    }

    pub fn resize_reservation(&mut self, bytes: u64) -> Result<(), MultipartPartJournalError> {
        if bytes < self.staged_bytes() {
            return Err(MultipartPartJournalError::ReservationExceeded);
        }
        self.manifest.reservation_size_bytes = bytes;
        self.persist()
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
        {
            return Err(MultipartPartJournalError::IdentityMismatch);
        }
        let manifest_path = self.directory.join(MANIFEST_FILE);
        if !self.directory.exists() {
            return Err(MultipartPartJournalError::Aborted);
        }
        if manifest_path.exists() {
            let bytes = fs::read(&manifest_path).map_err(io_error)?;
            let persisted: JournalManifest = serde_json::from_slice(&bytes)
                .map_err(|error| MultipartPartJournalError::Manifest(error.to_string()))?;
            validate_manifest(&persisted)?;
            if persisted.store_id != self.manifest.store_id
                || persisted.reservation_id != self.manifest.reservation_id
                || persisted.object != self.manifest.object
            {
                return Err(MultipartPartJournalError::IdentityMismatch);
            }
            self.manifest = persisted;
        }
        match self.manifest.lifecycle {
            MultipartLifecycle::Receiving => {}
            MultipartLifecycle::Completing | MultipartLifecycle::Committed => {
                return Err(MultipartPartJournalError::CompletionStarted);
            }
            MultipartLifecycle::Aborted => return Err(MultipartPartJournalError::Aborted),
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
        self.manifest.updated_at_unix_seconds = now_unix_seconds();
        self.persist().inspect_err(|_| {
            let _ = fs::remove_file(&final_path);
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
        File::open(self.directory.join(&part.file_name)).map_err(io_error)
    }

    /// Durably claim this reservation for one immutable completion request.
    /// A retry with the same intent resumes; a different intent fails closed.
    pub fn begin_completion(
        &mut self,
        object: BackendObjectKey,
        expected_size_bytes: u64,
        parts: Vec<MultipartPartRecord>,
    ) -> Result<MultipartCompletionClaim, MultipartPartJournalError> {
        let intent = MultipartCompletionIntent {
            object,
            expected_size_bytes,
            parts,
        };
        if self
            .manifest
            .completion_intent
            .as_ref()
            .is_some_and(|existing| existing != &intent)
        {
            return Err(MultipartPartJournalError::CompletionConflict);
        }
        match self.manifest.lifecycle {
            MultipartLifecycle::Receiving => {
                self.manifest.lifecycle = MultipartLifecycle::Completing;
                self.manifest.completion_intent = Some(intent);
                self.manifest.completion_job = Some(completion_status(
                    &self.manifest.store_id,
                    &self.manifest.reservation_id,
                    self.manifest
                        .completion_intent
                        .as_ref()
                        .expect("completion intent was just assigned"),
                    ProfileS3MultipartCompletionState::Accepted,
                    ProfileS3MultipartCompletionPhase::Queued,
                    0,
                    0,
                    None,
                ));
                self.manifest.updated_at_unix_seconds = now_unix_seconds();
                self.persist()?;
                Ok(MultipartCompletionClaim::Started)
            }
            MultipartLifecycle::Completing => {
                if self.manifest.completion_intent.as_ref() != Some(&intent) {
                    return Err(MultipartPartJournalError::CompletionConflict);
                }
                if self.manifest.completion_job.is_none() {
                    // Upgrade an older v1 completing journal in place. The
                    // immutable intent yields the same operation identity on
                    // every restart.
                    self.manifest.completion_job = Some(completion_status(
                        &self.manifest.store_id,
                        &self.manifest.reservation_id,
                        &intent,
                        ProfileS3MultipartCompletionState::InProgress,
                        ProfileS3MultipartCompletionPhase::Queued,
                        1,
                        0,
                        None,
                    ));
                    self.manifest.updated_at_unix_seconds = now_unix_seconds();
                    self.persist()?;
                }
                Ok(MultipartCompletionClaim::Resuming)
            }
            MultipartLifecycle::Committed => {
                if self.manifest.completion_intent.as_ref() != Some(&intent) {
                    return Err(MultipartPartJournalError::CompletionConflict);
                }
                self.manifest
                    .completion_receipt
                    .clone()
                    .map(MultipartCompletionClaim::Committed)
                    .ok_or_else(|| {
                        MultipartPartJournalError::Manifest(
                            "committed multipart journal has no receipt".to_string(),
                        )
                    })
            }
            MultipartLifecycle::Aborted => Err(MultipartPartJournalError::Aborted),
        }
    }

    /// Re-verify the immutable staged parts and derive the assembled object
    /// checksum without moving or rewriting payload data. This is used only to
    /// recover a `Completing` journal after a daemon restart or interrupted
    /// receipt write.
    pub fn assembled_checksum(&self) -> Result<String, MultipartPartJournalError> {
        let mut assembled = Sha256::new();
        for part in &self.manifest.parts {
            let mut file = File::open(self.directory.join(&part.file_name)).map_err(io_error)?;
            let mut part_hasher = Sha256::new();
            let mut size_bytes = 0_u64;
            let mut buffer = [0_u8; 1024 * 1024];
            loop {
                let read = std::io::Read::read(&mut file, &mut buffer).map_err(io_error)?;
                if read == 0 {
                    break;
                }
                size_bytes = size_bytes
                    .checked_add(read as u64)
                    .ok_or(MultipartPartJournalError::SizeOverflow)?;
                part_hasher.update(&buffer[..read]);
                assembled.update(&buffer[..read]);
            }
            let checksum = format!("sha256:{:x}", part_hasher.finalize());
            if size_bytes != part.size_bytes || checksum != part.checksum {
                return Err(MultipartPartJournalError::PartConflict);
            }
        }
        Ok(format!("sha256:{:x}", assembled.finalize()))
    }

    pub fn mark_committed(
        &mut self,
        receipt: MultipartCompletionReceipt,
    ) -> Result<(), MultipartPartJournalError> {
        if self.manifest.lifecycle != MultipartLifecycle::Completing {
            return Err(MultipartPartJournalError::CompletionConflict);
        }
        if self
            .manifest
            .completion_intent
            .as_ref()
            .is_none_or(|intent| {
                intent.object != receipt.object || intent.expected_size_bytes != receipt.size_bytes
            })
        {
            return Err(MultipartPartJournalError::CompletionConflict);
        }
        self.manifest.lifecycle = MultipartLifecycle::Committed;
        self.manifest.completion_receipt = Some(receipt);
        let status = self.ensure_completion_job()?;
        status.state = ProfileS3MultipartCompletionState::Committed;
        status.phase = ProfileS3MultipartCompletionPhase::Complete;
        status.completed_bytes = status.total_bytes;
        status.error = None;
        status.updated_at_unix_seconds = now_unix_seconds();
        self.manifest.updated_at_unix_seconds = now_unix_seconds();
        self.persist()?;
        // The durable receipt is the idempotency checkpoint. Once it exists,
        // staged parts are redundant and must not remain as a second full-size
        // copy of the committed object.
        let mut cleanup_complete = true;
        for part in &self.manifest.parts {
            match fs::remove_file(self.directory.join(&part.file_name)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => cleanup_complete = false,
            }
        }
        if cleanup_complete {
            File::open(&self.directory)
                .and_then(|directory| directory.sync_all())
                .map_err(io_error)?;
        }
        // Cleanup is downstream of the durable receipt. A retained part is
        // safe, visible to daemon GC, and must never make an already committed
        // completion appear to have failed.
        Ok(())
    }

    /// Abort and reclaim a receiving multipart journal. The exclusive activity
    /// lease serializes this decision with part upload and completion. Once
    /// completion starts, abort must never turn an ambiguous success into loss.
    pub fn abort(mut self) -> Result<(), MultipartPartJournalError> {
        match self.manifest.lifecycle {
            MultipartLifecycle::Receiving => {}
            MultipartLifecycle::Completing => {
                return Err(MultipartPartJournalError::CompletionStarted);
            }
            MultipartLifecycle::Committed => {
                return Err(MultipartPartJournalError::AlreadyCommitted);
            }
            MultipartLifecycle::Aborted => return Ok(()),
        }
        self.manifest.lifecycle = MultipartLifecycle::Aborted;
        self.manifest.updated_at_unix_seconds = now_unix_seconds();
        self.persist()?;
        fs::remove_dir_all(self.directory).map_err(io_error)
    }

    /// Backwards-compatible alias for daemon-owned abort cleanup.
    pub fn remove(self) -> Result<(), MultipartPartJournalError> {
        self.abort()
    }

    fn require_completing(&self) -> Result<(), MultipartPartJournalError> {
        if self.manifest.lifecycle == MultipartLifecycle::Completing
            && self.manifest.completion_intent.is_some()
        {
            Ok(())
        } else {
            Err(MultipartPartJournalError::CompletionConflict)
        }
    }

    fn ensure_completion_job(
        &mut self,
    ) -> Result<&mut ProfileS3MultipartCompletionStatus, MultipartPartJournalError> {
        let intent = self
            .manifest
            .completion_intent
            .as_ref()
            .ok_or(MultipartPartJournalError::CompletionConflict)?;
        if self.manifest.completion_job.is_none() {
            self.manifest.completion_job = Some(completion_status(
                &self.manifest.store_id,
                &self.manifest.reservation_id,
                intent,
                ProfileS3MultipartCompletionState::InProgress,
                ProfileS3MultipartCompletionPhase::Queued,
                1,
                0,
                None,
            ));
        }
        self.manifest
            .completion_job
            .as_mut()
            .ok_or(MultipartPartJournalError::CompletionConflict)
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

static ACTIVE_MULTIPART_PARTS: OnceLock<(Mutex<BTreeSet<PathBuf>>, Condvar)> = OnceLock::new();

struct ActiveMultipartPart {
    directory: PathBuf,
}

impl ActiveMultipartPart {
    fn acquire(directory: &Path) -> Result<Self, MultipartPartJournalError> {
        let (active, available) =
            ACTIVE_MULTIPART_PARTS.get_or_init(|| (Mutex::new(BTreeSet::new()), Condvar::new()));
        let mut active = active
            .lock()
            .map_err(|_| MultipartPartJournalError::ActivityRegistry)?;
        while active.contains(directory) {
            active = available
                .wait(active)
                .map_err(|_| MultipartPartJournalError::ActivityRegistry)?;
        }
        active.insert(directory.to_path_buf());
        Ok(Self {
            directory: directory.to_path_buf(),
        })
    }
}

impl Drop for ActiveMultipartPart {
    fn drop(&mut self) {
        let (active, available) =
            ACTIVE_MULTIPART_PARTS.get_or_init(|| (Mutex::new(BTreeSet::new()), Condvar::new()));
        if let Ok(mut active) = active.lock() {
            active.remove(&self.directory);
            available.notify_all();
        }
    }
}

fn multipart_is_active(directory: &Path) -> Result<bool, MultipartPartJournalError> {
    ACTIVE_MULTIPART_PARTS
        .get_or_init(|| (Mutex::new(BTreeSet::new()), Condvar::new()))
        .0
        .lock()
        .map(|active| active.contains(directory))
        .map_err(|_| MultipartPartJournalError::ActivityRegistry)
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Return true only for an explicitly aborted journal that no active daemon
/// stream can still mutate. Missing lifecycle state from v1 journals remains
/// `receiving` and therefore fails closed.
pub(crate) fn multipart_journal_is_reclaimable(
    directory: &Path,
) -> Result<bool, MultipartPartJournalError> {
    if multipart_is_active(directory)? {
        return Ok(false);
    }
    let bytes = fs::read(directory.join(MANIFEST_FILE)).map_err(io_error)?;
    let manifest: JournalManifest = serde_json::from_slice(&bytes)
        .map_err(|error| MultipartPartJournalError::Manifest(error.to_string()))?;
    validate_manifest(&manifest)?;
    Ok(manifest.lifecycle == MultipartLifecycle::Aborted)
}

pub(crate) fn multipart_journal_matches_store_namespace(
    directory: &Path,
    namespace: &str,
) -> Result<bool, MultipartPartJournalError> {
    let bytes = fs::read(directory.join(MANIFEST_FILE)).map_err(io_error)?;
    let manifest: JournalManifest = serde_json::from_slice(&bytes)
        .map_err(|error| MultipartPartJournalError::Manifest(error.to_string()))?;
    validate_manifest(&manifest)?;
    Ok(format!("{:x}", Sha256::digest(manifest.store_id.as_bytes())) == namespace)
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
        if matches!(
            manifest.lifecycle,
            MultipartLifecycle::Receiving | MultipartLifecycle::Completing
        ) {
            reservation_ids.push(manifest.reservation_id);
        }
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
    if let Some(status) = manifest.completion_job.as_ref() {
        status
            .validate()
            .map_err(MultipartPartJournalError::Manifest)?;
        let intent = manifest.completion_intent.as_ref().ok_or_else(|| {
            MultipartPartJournalError::Manifest(
                "multipart completion job has no immutable intent".to_string(),
            )
        })?;
        if status.job_id != completion_job_id(&manifest.store_id, &manifest.reservation_id, intent)
            || status.total_bytes != intent.expected_size_bytes
        {
            return Err(MultipartPartJournalError::Manifest(
                "multipart completion job identity or size is inconsistent".to_string(),
            ));
        }
        match manifest.lifecycle {
            MultipartLifecycle::Completing
                if status.state != ProfileS3MultipartCompletionState::Committed => {}
            MultipartLifecycle::Committed
                if status.state == ProfileS3MultipartCompletionState::Committed => {}
            _ => {
                return Err(MultipartPartJournalError::Manifest(
                    "multipart completion job lifecycle is inconsistent".to_string(),
                ));
            }
        }
    }
    match manifest.lifecycle {
        MultipartLifecycle::Receiving | MultipartLifecycle::Aborted
            if manifest.completion_intent.is_none() && manifest.completion_receipt.is_none() => {}
        MultipartLifecycle::Completing
            if manifest.completion_intent.is_some() && manifest.completion_receipt.is_none() => {}
        MultipartLifecycle::Committed
            if manifest
                .completion_intent
                .as_ref()
                .zip(manifest.completion_receipt.as_ref())
                .is_some_and(|(intent, receipt)| {
                    intent.object == receipt.object
                        && intent.expected_size_bytes == receipt.size_bytes
                }) => {}
        _ => {
            return Err(MultipartPartJournalError::Manifest(
                "multipart lifecycle evidence is inconsistent".to_string(),
            ));
        }
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
    Active,
    Aborted,
    CompletionStarted,
    AlreadyCommitted,
    CompletionConflict,
    InvalidProgress,
    InvalidCompletionError,
    AttemptOverflow,
    ActivityRegistry,
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
            Self::Active => formatter.write_str("multipart upload is active"),
            Self::Aborted => formatter.write_str("multipart upload was aborted"),
            Self::CompletionStarted => {
                formatter.write_str("multipart completion is already in progress")
            }
            Self::AlreadyCommitted => formatter.write_str("multipart upload is already committed"),
            Self::CompletionConflict => {
                formatter.write_str("multipart completion conflicts with the durable intent")
            }
            Self::InvalidProgress => {
                formatter.write_str("multipart completion progress is invalid")
            }
            Self::InvalidCompletionError => {
                formatter.write_str("multipart completion failure is invalid")
            }
            Self::AttemptOverflow => {
                formatter.write_str("multipart completion attempt count overflowed")
            }
            Self::ActivityRegistry => {
                formatter.write_str("multipart activity registry is unavailable")
            }
        }
    }
}

impl std::error::Error for MultipartPartJournalError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{
        write_provider_stream_frame, ProfileS3MultipartCompletionError,
        PROVIDER_STREAM_SCHEMA_VERSION,
    };
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

    #[test]
    fn garbage_collection_refuses_an_active_multipart_directory() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-multipart-active-abort-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root");
        let request = request(
            1,
            "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
        );
        let journal = MultipartPartJournal::open(&root, &request).expect("journal");
        journal.persist().expect("manifest");
        let directory = journal.directory.clone();
        assert!(!multipart_journal_is_reclaimable(&directory).expect("active classification"));
        assert!(directory.exists());
        journal.remove().expect("exclusive owner abort");
        assert!(!directory.exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn garbage_collection_requires_an_explicit_aborted_lifecycle() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-multipart-gc-lifecycle-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root");
        let request = request(
            1,
            "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
        );
        let mut journal = MultipartPartJournal::open(&root, &request).expect("journal");
        journal.persist().expect("manifest");
        assert!(!multipart_journal_is_reclaimable(&journal.directory).expect("classification"));
        journal.manifest.lifecycle = MultipartLifecycle::Aborted;
        journal.persist().expect("aborted marker");
        let directory = journal.directory.clone();
        drop(journal);
        assert!(multipart_journal_is_reclaimable(&directory).expect("classification"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn completion_receipt_is_durable_and_retry_is_idempotent() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-multipart-completion-receipt-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root");
        let request = request(
            1,
            "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
        );
        let mut journal = MultipartPartJournal::open(&root, &request).expect("journal");
        let staged_part = journal.directory.join("part-00000001.bin");
        std::fs::write(&staged_part, b"0123456789").expect("staged part");
        journal.manifest.parts.push(JournalPart {
            part_number: 1,
            size_bytes: 10,
            checksum: "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
                .to_string(),
            file_name: "part-00000001.bin".to_string(),
        });
        journal.persist().expect("manifest");
        let parts = vec![MultipartPartRecord {
            part_number: 1,
            size_bytes: 10,
            checksum: "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
                .to_string(),
        }];
        assert_eq!(
            journal
                .begin_completion(request.object.clone(), 10, parts.clone())
                .expect("begin"),
            MultipartCompletionClaim::Started
        );
        let receipt = MultipartCompletionReceipt {
            object: request.object.clone(),
            size_bytes: 10,
            checksum: "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .to_string(),
        };
        journal
            .mark_committed(receipt.clone())
            .expect("committed receipt");
        assert!(!staged_part.exists());
        drop(journal);

        let mut reopened = MultipartPartJournal::open_for_completion(
            &root,
            request.store_id.as_str(),
            &request.reservation_id,
            request.object.clone(),
            10,
        )
        .expect("reopen committed journal");
        assert_eq!(
            reopened
                .begin_completion(request.object, 10, parts)
                .expect("idempotent retry"),
            MultipartCompletionClaim::Committed(receipt)
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn abort_cannot_race_a_claimed_completion() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-multipart-completion-abort-race-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root");
        let request = request(
            1,
            "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
        );
        let mut journal = MultipartPartJournal::open(&root, &request).expect("journal");
        journal.persist().expect("manifest");
        journal
            .begin_completion(request.object, 10, Vec::new())
            .expect("claim completion");
        let directory = journal.directory.clone();
        assert!(matches!(
            journal.abort(),
            Err(MultipartPartJournalError::CompletionStarted)
        ));
        assert!(directory.exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn completing_journal_resumes_after_restart_with_verified_assembled_checksum() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-multipart-completion-restart-{}",
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
        let part = journal
            .stage_part(&request, &mut || {
                crate::api::read_provider_stream_frame(&mut Cursor::new(frame.clone()))
                    .map_err(|error| MultipartPartJournalError::Io(error.to_string()))
            })
            .expect("stage");
        assert_eq!(
            journal
                .begin_completion(request.object.clone(), 5, vec![part.clone()])
                .expect("begin"),
            MultipartCompletionClaim::Started
        );
        drop(journal);

        let mut resumed = MultipartPartJournal::open_for_completion(
            &root,
            request.store_id.as_str(),
            &request.reservation_id,
            request.object.clone(),
            5,
        )
        .expect("reopen");
        assert_eq!(
            resumed
                .begin_completion(request.object, 5, vec![part])
                .expect("resume"),
            MultipartCompletionClaim::Resuming
        );
        assert_eq!(
            resumed.assembled_checksum().expect("assembled checksum"),
            "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        drop(resumed);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn completion_job_identity_is_deterministic_and_intent_bound() {
        let intent = MultipartCompletionIntent {
            object: BackendObjectKey {
                object_id: "object.bin".to_string(),
                version: 1,
            },
            expected_size_bytes: 10,
            parts: vec![MultipartPartRecord {
                part_number: 1,
                size_bytes: 10,
                checksum: format!("sha256:{}", "a".repeat(64)),
            }],
        };
        let first = completion_job_id("store-1", "reservation-1", &intent);
        assert_eq!(
            first,
            completion_job_id("store-1", "reservation-1", &intent)
        );
        assert!(first.starts_with("mpc-"));
        assert_eq!(first.len(), 68);

        let mut changed = intent.clone();
        changed.parts[0].checksum = format!("sha256:{}", "b".repeat(64));
        assert_ne!(
            first,
            completion_job_id("store-1", "reservation-1", &changed)
        );
    }

    #[test]
    fn completion_status_inspection_does_not_wait_for_active_worker() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-multipart-completion-inspect-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root");
        let request = request(
            1,
            "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
        );
        let mut journal = MultipartPartJournal::open(&root, &request).expect("journal");
        journal.persist().expect("manifest");
        journal
            .begin_completion(request.object.clone(), 10, Vec::new())
            .expect("begin");

        // `journal` still owns the exclusive activity lease. Inspection reads
        // only the atomically persisted manifest and therefore cannot wait on
        // the worker lease.
        let status = inspect_multipart_completion_status(
            &root,
            request.store_id.as_str(),
            &request.reservation_id,
            &request.object,
        )
        .expect("inspect")
        .expect("completion status");
        assert_eq!(status.state, ProfileS3MultipartCompletionState::Accepted);
        assert_eq!(status.phase, ProfileS3MultipartCompletionPhase::Queued);
        assert_eq!(status.attempts, 0);
        assert_eq!(status.total_bytes, 10);

        drop(journal);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn completion_progress_and_failure_survive_restart_without_releasing_parts() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-multipart-completion-progress-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root");
        let request = request(
            1,
            "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
        );
        let mut journal = MultipartPartJournal::open(&root, &request).expect("journal");
        let part_path = journal.directory.join("part-00000001.bin");
        std::fs::write(&part_path, b"0123456789").expect("part");
        journal.manifest.parts.push(JournalPart {
            part_number: 1,
            size_bytes: 10,
            checksum: format!("sha256:{}", "a".repeat(64)),
            file_name: "part-00000001.bin".to_string(),
        });
        journal.persist().expect("manifest");
        let parts = vec![MultipartPartRecord {
            part_number: 1,
            size_bytes: 10,
            checksum: format!("sha256:{}", "a".repeat(64)),
        }];
        journal
            .begin_completion(request.object.clone(), 10, parts.clone())
            .expect("begin");
        let running = journal
            .mark_completion_in_progress(ProfileS3MultipartCompletionPhase::Assembling)
            .expect("running");
        assert_eq!(running.attempts, 1);
        journal
            .mark_completion_progress(ProfileS3MultipartCompletionPhase::Assembling, 4)
            .expect("progress");
        journal
            .mark_completion_failed(
                ProfileS3MultipartCompletionPhase::Publishing,
                ProfileS3MultipartCompletionError {
                    code: "backend_temporarily_unavailable".to_string(),
                    message: "backend is temporarily unavailable".to_string(),
                    retryable: true,
                },
            )
            .expect("retryable failure");
        drop(journal);

        let mut reopened = MultipartPartJournal::open_for_completion(
            &root,
            request.store_id.as_str(),
            &request.reservation_id,
            request.object.clone(),
            10,
        )
        .expect("reopen");
        assert_eq!(
            reopened
                .begin_completion(request.object, 10, parts)
                .expect("resume"),
            MultipartCompletionClaim::Resuming
        );
        let failed = reopened.completion_status().expect("status").expect("job");
        assert_eq!(
            failed.state,
            ProfileS3MultipartCompletionState::FailedRetryable
        );
        assert_eq!(failed.attempts, 1);
        assert_eq!(failed.completed_bytes, 4);
        assert!(part_path.exists());
        let retried = reopened
            .mark_completion_in_progress(ProfileS3MultipartCompletionPhase::VerifyingParts)
            .expect("retry");
        assert_eq!(retried.attempts, 2);
        assert!(retried.error.is_none());

        drop(reopened);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn legacy_completing_manifest_gets_stable_synthesized_status() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-multipart-completion-legacy-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root");
        let request = request(
            1,
            "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
        );
        let mut journal = MultipartPartJournal::open(&root, &request).expect("journal");
        journal.manifest.lifecycle = MultipartLifecycle::Completing;
        journal.manifest.completion_intent = Some(MultipartCompletionIntent {
            object: request.object.clone(),
            expected_size_bytes: 10,
            parts: Vec::new(),
        });
        journal.manifest.completion_job = None;
        journal.persist().expect("legacy manifest");

        let first = journal
            .completion_status()
            .expect("status")
            .expect("synthesized");
        let second = inspect_multipart_completion_status(
            &root,
            request.store_id.as_str(),
            &request.reservation_id,
            &request.object,
        )
        .expect("inspect")
        .expect("synthesized");
        assert_eq!(first.job_id, second.job_id);
        assert_eq!(first.state, ProfileS3MultipartCompletionState::InProgress);

        drop(journal);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn recoverable_upload_listing_is_store_scoped_and_excludes_committed() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-multipart-listing-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root");
        let request = request(
            1,
            "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
        );
        let journal = MultipartPartJournal::open(&root, &request).expect("journal");
        journal.persist().expect("manifest");
        drop(journal);
        let uploads =
            list_recoverable_multipart_uploads(&root, request.store_id.as_str()).expect("list");
        assert_eq!(uploads.len(), 1);
        assert_eq!(uploads[0].reservation_id, request.reservation_id);
        assert_eq!(uploads[0].object, request.object);
        assert!(uploads[0].completion.is_none());
        assert!(list_recoverable_multipart_uploads(&root, "another-store")
            .expect("other store")
            .is_empty());
        let mut committed = MultipartPartJournal::open(&root, &request).expect("reopen");
        committed
            .begin_completion(request.object.clone(), 0, Vec::new())
            .expect("completion");
        committed
            .mark_committed(MultipartCompletionReceipt {
                object: request.object.clone(),
                size_bytes: 0,
                checksum: "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
                    .to_string(),
            })
            .expect("committed manifest");
        committed.manifest.completion_receipt = None;
        std::fs::write(
            committed.directory.join(MANIFEST_FILE),
            serde_json::to_vec(&committed.manifest).expect("legacy terminal manifest"),
        )
        .expect("write legacy terminal manifest");
        drop(committed);
        assert!(
            list_recoverable_multipart_uploads(&root, request.store_id.as_str())
                .expect("committed list")
                .is_empty()
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
