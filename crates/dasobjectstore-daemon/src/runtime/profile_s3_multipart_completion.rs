use super::*;
use crate::api::{
    ProfileS3MultipartCompletionError, ProfileS3MultipartCompletionPhase,
    ProfileS3MultipartCompletionState, ProfileS3MultipartCompletionStatus,
};
use dasobjectstore_core::backend::BackendObjectKey;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MultipartUploadStatusRecord {
    pub reservation_id: String,
    pub object: BackendObjectKey,
    pub initiated_at_unix_seconds: u64,
    pub completion: Option<ProfileS3MultipartCompletionStatus>,
}

impl MultipartPartJournal {
    pub fn completion_status(
        &self,
    ) -> Result<Option<ProfileS3MultipartCompletionStatus>, MultipartPartJournalError> {
        completion_status_from_manifest(&self.manifest)
    }

    pub fn mark_completion_in_progress(
        &mut self,
        phase: ProfileS3MultipartCompletionPhase,
    ) -> Result<ProfileS3MultipartCompletionStatus, MultipartPartJournalError> {
        self.require_completing()?;
        let status = self.ensure_completion_job()?;
        if status.state == ProfileS3MultipartCompletionState::Committed
            || status.state == ProfileS3MultipartCompletionState::FailedTerminal
        {
            return Err(MultipartPartJournalError::CompletionConflict);
        }
        status.attempts = status
            .attempts
            .checked_add(1)
            .ok_or(MultipartPartJournalError::AttemptOverflow)?;
        status.state = ProfileS3MultipartCompletionState::InProgress;
        status.phase = phase;
        status.error = None;
        status.updated_at_unix_seconds = now_unix_seconds();
        let result = status.clone();
        self.manifest.updated_at_unix_seconds = result.updated_at_unix_seconds;
        self.persist()?;
        Ok(result)
    }

    pub fn mark_completion_progress(
        &mut self,
        phase: ProfileS3MultipartCompletionPhase,
        completed_bytes: u64,
    ) -> Result<ProfileS3MultipartCompletionStatus, MultipartPartJournalError> {
        self.require_completing()?;
        let status = self.ensure_completion_job()?;
        if status.state != ProfileS3MultipartCompletionState::InProgress
            || completed_bytes > status.total_bytes
        {
            return Err(MultipartPartJournalError::InvalidProgress);
        }
        status.phase = phase;
        status.completed_bytes = completed_bytes;
        status.updated_at_unix_seconds = now_unix_seconds();
        let result = status.clone();
        self.manifest.updated_at_unix_seconds = result.updated_at_unix_seconds;
        self.persist()?;
        Ok(result)
    }

    pub fn mark_completion_failed(
        &mut self,
        phase: ProfileS3MultipartCompletionPhase,
        error: ProfileS3MultipartCompletionError,
    ) -> Result<ProfileS3MultipartCompletionStatus, MultipartPartJournalError> {
        self.require_completing()?;
        if error.code.trim().is_empty() || error.message.trim().is_empty() {
            return Err(MultipartPartJournalError::InvalidCompletionError);
        }
        let status = self.ensure_completion_job()?;
        if status.state == ProfileS3MultipartCompletionState::FailedTerminal
            || status.state == ProfileS3MultipartCompletionState::Committed
        {
            return Err(MultipartPartJournalError::CompletionConflict);
        }
        status.state = if error.retryable {
            ProfileS3MultipartCompletionState::FailedRetryable
        } else {
            ProfileS3MultipartCompletionState::FailedTerminal
        };
        status.phase = phase;
        status.error = Some(error);
        status.updated_at_unix_seconds = now_unix_seconds();
        let result = status.clone();
        self.manifest.updated_at_unix_seconds = result.updated_at_unix_seconds;
        self.persist()?;
        Ok(result)
    }
}

pub fn inspect_multipart_completion_status(
    root: impl AsRef<Path>,
    store_id: &str,
    reservation_id: &str,
    object: &BackendObjectKey,
) -> Result<Option<ProfileS3MultipartCompletionStatus>, MultipartPartJournalError> {
    if store_id.trim().is_empty() || !safe_reservation_id(reservation_id) {
        return Err(MultipartPartJournalError::IdentityMismatch);
    }
    let path = root
        .as_ref()
        .join(NAMESPACE)
        .join(MULTIPART_DIR)
        .join(reservation_id)
        .join(MANIFEST_FILE);
    let bytes = fs::read(path).map_err(io_error)?;
    let manifest: JournalManifest = serde_json::from_slice(&bytes)
        .map_err(|error| MultipartPartJournalError::Manifest(error.to_string()))?;
    validate_manifest(&manifest)?;
    if manifest.store_id != store_id
        || manifest.reservation_id != reservation_id
        || &manifest.object != object
    {
        return Err(MultipartPartJournalError::IdentityMismatch);
    }
    completion_status_from_manifest(&manifest)
}

pub fn list_recoverable_multipart_uploads(
    root: impl AsRef<Path>,
    store_id: &str,
) -> Result<Vec<MultipartUploadStatusRecord>, MultipartPartJournalError> {
    let namespace = root.as_ref().join(NAMESPACE).join(MULTIPART_DIR);
    let mut uploads = Vec::new();
    match fs::read_dir(namespace) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry.map_err(io_error)?;
                if entry.file_type().map_err(io_error)?.is_symlink()
                    || !entry.file_type().map_err(io_error)?.is_dir()
                {
                    continue;
                }
                let bytes = fs::read(entry.path().join(MANIFEST_FILE)).map_err(io_error)?;
                let manifest: JournalManifest = serde_json::from_slice(&bytes)
                    .map_err(|error| MultipartPartJournalError::Manifest(error.to_string()))?;
                if manifest.store_id != store_id
                    || matches!(
                        manifest.lifecycle,
                        MultipartLifecycle::Committed | MultipartLifecycle::Aborted
                    )
                {
                    continue;
                }
                validate_manifest(&manifest)?;
                uploads.push(MultipartUploadStatusRecord {
                    reservation_id: manifest.reservation_id.clone(),
                    object: manifest.object.clone(),
                    initiated_at_unix_seconds: manifest.created_at_unix_seconds,
                    completion: completion_status_from_manifest(&manifest)?,
                });
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(uploads),
        Err(error) => return Err(io_error(error)),
    }
    uploads.sort_by(|left, right| {
        left.initiated_at_unix_seconds
            .cmp(&right.initiated_at_unix_seconds)
            .then_with(|| left.reservation_id.cmp(&right.reservation_id))
    });
    Ok(uploads)
}

pub(super) fn completion_status_from_manifest(
    manifest: &JournalManifest,
) -> Result<Option<ProfileS3MultipartCompletionStatus>, MultipartPartJournalError> {
    if let Some(status) = manifest.completion_job.as_ref() {
        return Ok(Some(status.clone()));
    }
    let Some(intent) = manifest.completion_intent.as_ref() else {
        return Ok(None);
    };
    let (state, phase, completed_bytes) = match manifest.lifecycle {
        MultipartLifecycle::Completing => (
            ProfileS3MultipartCompletionState::InProgress,
            ProfileS3MultipartCompletionPhase::Queued,
            0,
        ),
        MultipartLifecycle::Committed => (
            ProfileS3MultipartCompletionState::Committed,
            ProfileS3MultipartCompletionPhase::Complete,
            intent.expected_size_bytes,
        ),
        MultipartLifecycle::Receiving | MultipartLifecycle::Aborted => {
            return Err(MultipartPartJournalError::Manifest(
                "multipart completion intent has no completion lifecycle".to_string(),
            ));
        }
    };
    Ok(Some(completion_status(
        &manifest.store_id,
        &manifest.reservation_id,
        intent,
        state,
        phase,
        u32::from(state != ProfileS3MultipartCompletionState::Accepted),
        completed_bytes,
        None,
    )))
}

pub(super) fn completion_status(
    store_id: &str,
    reservation_id: &str,
    intent: &MultipartCompletionIntent,
    state: ProfileS3MultipartCompletionState,
    phase: ProfileS3MultipartCompletionPhase,
    attempts: u32,
    completed_bytes: u64,
    error: Option<ProfileS3MultipartCompletionError>,
) -> ProfileS3MultipartCompletionStatus {
    ProfileS3MultipartCompletionStatus {
        job_id: completion_job_id(store_id, reservation_id, intent),
        state,
        phase,
        attempts,
        completed_bytes,
        total_bytes: intent.expected_size_bytes,
        error,
        updated_at_unix_seconds: now_unix_seconds(),
    }
}

pub(super) fn completion_job_id(
    store_id: &str,
    reservation_id: &str,
    intent: &MultipartCompletionIntent,
) -> String {
    let mut digest = Sha256::new();
    digest_field(&mut digest, store_id.as_bytes());
    digest_field(&mut digest, reservation_id.as_bytes());
    digest_field(&mut digest, intent.object.object_id.as_bytes());
    digest.update(intent.object.version.to_be_bytes());
    digest.update(intent.expected_size_bytes.to_be_bytes());
    digest.update((intent.parts.len() as u64).to_be_bytes());
    for part in &intent.parts {
        digest.update(part.part_number.to_be_bytes());
        digest.update(part.size_bytes.to_be_bytes());
        digest_field(&mut digest, part.checksum.as_bytes());
    }
    format!("mpc-{:x}", digest.finalize())
}

pub fn multipart_completion_job_id(
    store_id: &str,
    reservation_id: &str,
    object: BackendObjectKey,
    expected_size_bytes: u64,
    parts: Vec<MultipartPartRecord>,
) -> String {
    completion_job_id(
        store_id,
        reservation_id,
        &MultipartCompletionIntent {
            object,
            expected_size_bytes,
            parts,
        },
    )
}

fn digest_field(digest: &mut Sha256, value: &[u8]) {
    digest.update((value.len() as u64).to_be_bytes());
    digest.update(value);
}
