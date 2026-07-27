use super::provider_stream::{commit_profile_s3_acceptance_at, MultipartCompletionWorkerContext};
use super::*;
use crate::api::{
    ProfileS3MultipartCompletionError, ProfileS3MultipartCompletionPhase,
    ProfileS3MultipartCompletionRequest,
};
use crate::runtime::{
    CapacityAdmissionProvider, MultipartCompletionReceipt, MultipartPartJournal,
    ProfileS3MultipartCompletion, ProfileS3MultipartPart, ProfileS3MultipartPartSource,
};
use dasobjectstore_core::backend::ObjectStoreBackend;
use dasobjectstore_core::store::CapacityPolicy;
use dasobjectstore_object_service::StoreServiceDefinition;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

static ACTIVE_COMPLETIONS: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();

pub(super) struct MultipartCompletionWork {
    pub job_id: String,
    pub backend_root: PathBuf,
    pub backend_manifest: dasobjectstore_core::manifest::ObjectStoreManifest,
    pub capacity: CapacityPolicy,
    pub provider: Arc<dyn CapacityAdmissionProvider>,
    pub store_id: StoreId,
    pub subobject: Option<String>,
    pub qualified_key: dasobjectstore_core::backend::BackendObjectKey,
    pub definition: StoreServiceDefinition,
    pub binding: BackendProfileBinding,
    pub request: ProfileS3MultipartCompletionRequest,
    pub context: MultipartCompletionWorkerContext,
}

/// Enqueue one process-owned worker. The durable journal, rather than this
/// registry, is the authority: after a daemon restart a repeated submission
/// deterministically schedules the same operation again.
pub(super) fn ensure_multipart_completion_worker(work: MultipartCompletionWork) -> bool {
    let active = ACTIVE_COMPLETIONS.get_or_init(|| Mutex::new(BTreeSet::new()));
    let Ok(mut active) = active.lock() else {
        return false;
    };
    if !active.insert(work.job_id.clone()) {
        return true;
    }
    let job_id = work.job_id.clone();
    let registry_job_id = job_id.clone();
    let spawned = std::thread::Builder::new()
        .name(format!(
            "multipart-{}",
            &job_id[job_id.len().saturating_sub(12)..]
        ))
        .spawn(move || {
            run_multipart_completion(work);
            if let Ok(mut active) = ACTIVE_COMPLETIONS
                .get_or_init(|| Mutex::new(BTreeSet::new()))
                .lock()
            {
                active.remove(&job_id);
            }
        })
        .is_ok();
    if !spawned {
        if let Ok(mut active) = ACTIVE_COMPLETIONS
            .get_or_init(|| Mutex::new(BTreeSet::new()))
            .lock()
        {
            active.remove(&registry_job_id);
        }
    }
    spawned
}

pub(super) fn multipart_completion_worker_active(job_id: &str) -> bool {
    ACTIVE_COMPLETIONS
        .get_or_init(|| Mutex::new(BTreeSet::new()))
        .lock()
        .is_ok_and(|active| active.contains(job_id))
}

fn run_multipart_completion(work: MultipartCompletionWork) {
    if let Err((phase, code, message, retryable)) = run_multipart_completion_inner(&work) {
        if let Ok(mut journal) = MultipartPartJournal::open_for_completion(
            &work.backend_root,
            work.store_id.as_str(),
            &work.request.reservation_id,
            work.qualified_key.clone(),
            work.request.expected_size_bytes,
        ) {
            let _ = journal.mark_completion_failed(
                phase,
                ProfileS3MultipartCompletionError {
                    code: code.to_string(),
                    message,
                    retryable,
                },
            );
        }
    }
}

type WorkerFailure = (
    ProfileS3MultipartCompletionPhase,
    &'static str,
    String,
    bool,
);

fn run_multipart_completion_inner(work: &MultipartCompletionWork) -> Result<(), WorkerFailure> {
    let mut journal = MultipartPartJournal::open_for_completion(
        &work.backend_root,
        work.store_id.as_str(),
        &work.request.reservation_id,
        work.qualified_key.clone(),
        work.request.expected_size_bytes,
    )
    .map_err(|error| failure("multipart_journal_unavailable", error, true))?;
    journal
        .mark_completion_in_progress(ProfileS3MultipartCompletionPhase::VerifyingParts)
        .map_err(|error| failure("multipart_checkpoint_failed", error, true))?;
    let mut backend = FolderBackend::open(
        &work.backend_root,
        work.backend_manifest.clone(),
        work.capacity.clone(),
        0,
    )
    .map_err(|error| failure("profile_s3_unavailable", error, true))?;

    let recovered = backend
        .records()
        .map_err(|error| failure("multipart_backend_inspection_failed", error, true))?
        .into_iter()
        .find(|record| record.key == work.qualified_key);
    let record = if let Some(record) = recovered {
        let checksum = journal
            .assembled_checksum()
            .map_err(|error| failure("multipart_part_verification_failed", error, false))?;
        if record.size_bytes != work.request.expected_size_bytes || record.checksum != checksum {
            return Err((
                ProfileS3MultipartCompletionPhase::VerifyingParts,
                "multipart_publication_conflict",
                "published object conflicts with the durable multipart intent".to_string(),
                false,
            ));
        }
        record
    } else if backend.verify(&work.qualified_key).is_ok() {
        let checksum = journal
            .assembled_checksum()
            .map_err(|error| failure("multipart_part_verification_failed", error, false))?;
        crate::runtime::recover_profile_s3_published_object(
            work.provider.as_ref(),
            work.store_id.as_str(),
            work.subobject.as_deref(),
            &mut backend,
            &work.request.reservation_id,
            &work.qualified_key,
            work.request.expected_size_bytes,
            &checksum,
        )
        .map_err(|error| failure("multipart_publication_recovery_failed", error, true))?
    } else {
        journal
            .mark_completion_progress(ProfileS3MultipartCompletionPhase::Assembling, 0)
            .map_err(|error| failure("multipart_checkpoint_failed", error, true))?;
        let mut sources = Vec::with_capacity(work.request.parts.len());
        for part in &work.request.parts {
            let reader = journal
                .open_part(part.part_number)
                .map_err(|error| failure("multipart_part_unavailable", error, true))?;
            sources.push(ProfileS3MultipartPartSource {
                part: ProfileS3MultipartPart {
                    part_number: part.part_number,
                    size_bytes: part.size_bytes,
                    checksum: part.checksum.clone(),
                },
                reader: Box::new(reader),
            });
        }
        let completion = ProfileS3MultipartCompletion {
            reservation_id: work.request.reservation_id.clone(),
            key: work.qualified_key.clone(),
            expected_size_bytes: work.request.expected_size_bytes,
            parts: work
                .request
                .parts
                .iter()
                .map(|part| ProfileS3MultipartPart {
                    part_number: part.part_number,
                    size_bytes: part.size_bytes,
                    checksum: part.checksum.clone(),
                })
                .collect(),
        };
        crate::runtime::complete_profile_s3_multipart_with_admitted_capacity_scope(
            work.provider.as_ref(),
            work.store_id.as_str(),
            work.subobject.as_deref(),
            &mut backend,
            &completion,
            sources,
        )
        .map_err(|error| failure("multipart_assembly_failed", error, true))?
    };
    journal
        .mark_completion_progress(
            ProfileS3MultipartCompletionPhase::Cataloguing,
            work.request.expected_size_bytes,
        )
        .map_err(|error| failure("multipart_checkpoint_failed", error, true))?;
    commit_profile_s3_acceptance_at(
        &work.context.live_sqlite_path,
        &work.context.hdd_root_path,
        &work.context.accepted_at_utc,
        &work.definition,
        &work.binding,
        &backend,
        &record,
        &work.request.reservation_id,
    )
    .map_err(|message| {
        (
            ProfileS3MultipartCompletionPhase::Cataloguing,
            "multipart_catalogue_acceptance_failed",
            message,
            true,
        )
    })?;
    journal
        .mark_completion_progress(
            ProfileS3MultipartCompletionPhase::PersistingReceipt,
            work.request.expected_size_bytes,
        )
        .map_err(|error| failure("multipart_checkpoint_failed", error, true))?;
    journal
        .mark_committed(MultipartCompletionReceipt {
            object: record.key,
            size_bytes: record.size_bytes,
            checksum: record.checksum,
        })
        .map_err(|error| failure("multipart_receipt_failed", error, true))
}

fn failure(code: &'static str, error: impl std::fmt::Display, retryable: bool) -> WorkerFailure {
    (
        ProfileS3MultipartCompletionPhase::VerifyingParts,
        code,
        error.to_string(),
        retryable,
    )
}
