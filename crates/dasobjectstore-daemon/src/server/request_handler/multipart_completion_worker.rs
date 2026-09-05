use super::provider_stream::{commit_profile_s3_acceptance_at, MultipartCompletionWorkerContext};
use super::*;
use crate::api::{
    ProfileS3MultipartCompletionError, ProfileS3MultipartCompletionPhase,
    ProfileS3MultipartCompletionRequest, ProfileS3MultipartCompletionState,
    ProfileS3MultipartPartRequest,
};
use crate::runtime::{
    CapacityAdmissionProvider, MultipartCompletionReceipt, MultipartPartJournal,
    ProfileS3MultipartCompletion, ProfileS3MultipartPart, ProfileS3MultipartPartSource,
};
use dasobjectstore_core::backend::ObjectStoreBackend;
use dasobjectstore_core::store::CapacityPolicy;
use dasobjectstore_object_service::StoreServiceDefinition;
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

static ACTIVE_COMPLETIONS: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MultipartCompletionRecoveryReport {
    pub discovered: usize,
    pub requeued: usize,
    pub retained_unsafe: usize,
}

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
    log_completion_event(&work, "worker_started", None, "resume_or_start");
    if let Err((phase, code, message, retryable)) = run_multipart_completion_inner(&work) {
        log_completion_event(
            &work,
            "worker_failed",
            Some((phase, code, retryable)),
            if retryable {
                "retained_for_retry"
            } else {
                "retained_for_operator_review"
            },
        );
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
    } else {
        log_completion_event(&work, "worker_committed", None, "receipt_persisted");
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
    .map_err(|error| {
        failure(
            ProfileS3MultipartCompletionPhase::Queued,
            "multipart_journal_unavailable",
            error,
            true,
        )
    })?;
    journal
        .mark_completion_in_progress(ProfileS3MultipartCompletionPhase::VerifyingParts)
        .map_err(|error| {
            failure(
                ProfileS3MultipartCompletionPhase::VerifyingParts,
                "multipart_checkpoint_failed",
                error,
                true,
            )
        })?;
    let mut backend = FolderBackend::open(
        &work.backend_root,
        work.backend_manifest.clone(),
        work.capacity.clone(),
        0,
    )
    .map_err(|error| {
        failure(
            ProfileS3MultipartCompletionPhase::VerifyingParts,
            "profile_s3_unavailable",
            error,
            true,
        )
    })?;

    let recovered = backend
        .records()
        .map_err(|error| {
            failure(
                ProfileS3MultipartCompletionPhase::VerifyingParts,
                "multipart_backend_inspection_failed",
                error,
                true,
            )
        })?
        .into_iter()
        .find(|record| record.key == work.qualified_key);
    let record = if let Some(record) = recovered {
        let checksum = journal.assembled_checksum().map_err(|error| {
            failure(
                ProfileS3MultipartCompletionPhase::VerifyingParts,
                "multipart_part_verification_failed",
                error,
                false,
            )
        })?;
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
        let checksum = journal.assembled_checksum().map_err(|error| {
            failure(
                ProfileS3MultipartCompletionPhase::VerifyingParts,
                "multipart_part_verification_failed",
                error,
                false,
            )
        })?;
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
        .map_err(|error| {
            failure(
                ProfileS3MultipartCompletionPhase::Publishing,
                "multipart_publication_recovery_failed",
                error,
                true,
            )
        })?
    } else {
        reclaim_stale_assembly_partials(&work.backend_root, &work.request.reservation_id).map_err(
            |error| {
                failure(
                    ProfileS3MultipartCompletionPhase::Assembling,
                    "multipart_stale_assembly_recovery_failed",
                    error,
                    true,
                )
            },
        )?;
        journal
            .mark_completion_progress(ProfileS3MultipartCompletionPhase::Assembling, 0)
            .map_err(|error| {
                failure(
                    ProfileS3MultipartCompletionPhase::Assembling,
                    "multipart_checkpoint_failed",
                    error,
                    true,
                )
            })?;
        let mut sources = Vec::with_capacity(work.request.parts.len());
        for part in &work.request.parts {
            let reader = journal.open_part(part.part_number).map_err(|error| {
                failure(
                    ProfileS3MultipartCompletionPhase::VerifyingParts,
                    "multipart_part_unavailable",
                    error,
                    true,
                )
            })?;
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
        .map_err(|error| {
            failure(
                ProfileS3MultipartCompletionPhase::Assembling,
                "multipart_assembly_failed",
                error,
                true,
            )
        })?
    };
    journal
        .mark_completion_progress(
            ProfileS3MultipartCompletionPhase::Publishing,
            work.request.expected_size_bytes,
        )
        .map_err(|error| {
            failure(
                ProfileS3MultipartCompletionPhase::Publishing,
                "multipart_checkpoint_failed",
                error,
                true,
            )
        })?;
    journal
        .mark_completion_progress(
            ProfileS3MultipartCompletionPhase::Cataloguing,
            work.request.expected_size_bytes,
        )
        .map_err(|error| {
            failure(
                ProfileS3MultipartCompletionPhase::Cataloguing,
                "multipart_checkpoint_failed",
                error,
                true,
            )
        })?;
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
        .map_err(|error| {
            failure(
                ProfileS3MultipartCompletionPhase::PersistingReceipt,
                "multipart_checkpoint_failed",
                error,
                true,
            )
        })?;
    journal
        .mark_committed(MultipartCompletionReceipt {
            object: record.key,
            size_bytes: record.size_bytes,
            checksum: record.checksum,
        })
        .map_err(|error| {
            failure(
                ProfileS3MultipartCompletionPhase::PersistingReceipt,
                "multipart_receipt_failed",
                error,
                true,
            )
        })
}

/// Reclaim only assembly files owned by this immutable reservation. Folder
/// backend assembly uses `<reservation>-<pid>-<counter>.part`; uploaded part
/// files live in a separate multipart namespace and are never touched here.
fn reclaim_stale_assembly_partials(
    backend_root: &std::path::Path,
    reservation_id: &str,
) -> Result<usize, std::io::Error> {
    if reservation_id.is_empty()
        || !reservation_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "unsafe multipart reservation identity",
        ));
    }
    let staging = backend_root.join(".dasobjectstore/staging");
    let entries = match fs::read_dir(&staging) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    let prefix = format!("{reservation_id}-");
    let mut reclaimed = 0;
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !name.starts_with(&prefix) || !name.ends_with(".part") {
            continue;
        }
        let suffix = &name[prefix.len()..name.len() - ".part".len()];
        let mut components = suffix.split('-');
        if components
            .next()
            .is_none_or(|pid| pid.is_empty() || !pid.bytes().all(|byte| byte.is_ascii_digit()))
            || components.next().is_none_or(|counter| {
                counter.is_empty() || !counter.bytes().all(|byte| byte.is_ascii_digit())
            })
            || components.next().is_some()
        {
            continue;
        }
        let file_type = entry.file_type()?;
        if file_type.is_symlink() || !file_type.is_file() {
            return Err(std::io::Error::other(
                "unsafe multipart assembly staging entry",
            ));
        }
        fs::remove_file(entry.path())?;
        reclaimed += 1;
    }
    if reclaimed > 0 {
        fs::File::open(staging)?.sync_all()?;
    }
    Ok(reclaimed)
}

fn failure(
    phase: ProfileS3MultipartCompletionPhase,
    code: &'static str,
    error: impl std::fmt::Display,
    retryable: bool,
) -> WorkerFailure {
    (phase, code, error.to_string(), retryable)
}

fn log_completion_event(
    work: &MultipartCompletionWork,
    event: &str,
    failure: Option<(ProfileS3MultipartCompletionPhase, &'static str, bool)>,
    recovery_action: &str,
) {
    let (phase, failure_classification, retryable) = failure
        .map(|(phase, code, retryable)| (format!("{phase:?}"), code, Some(retryable)))
        .unwrap_or_else(|| ("unknown".to_string(), "none", None));
    eprintln!(
        "{}",
        serde_json::json!({
            "event": "profile_s3_multipart_completion",
            "outcome": event,
            "job_id": work.job_id,
            "upload_id": work.request.reservation_id,
            "store_id": work.store_id.as_str(),
            "key": work.qualified_key.object_id,
            "phase": phase,
            "committed_part_count": work.request.parts.len(),
            "failure_classification": failure_classification,
            "retryable": retryable,
            "recovery_action": recovery_action,
        })
    );
}

impl<S, C> DaemonRequestHandler<S, C>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    /// Requeue every durable, non-terminal completion intent during daemon
    /// startup. Corrupt or incomplete records remain untouched and visible to
    /// operators; recovery never guesses an authorization or capacity scope.
    pub fn recover_multipart_completion_workers(&self) -> MultipartCompletionRecoveryReport {
        let mut report = MultipartCompletionRecoveryReport::default();
        let Some(provider) = self.service_orchestrator.capacity_provider() else {
            return report;
        };
        let Ok(bindings) =
            crate::runtime::read_profile_bindings(&self.profile_binding_registry_path)
        else {
            return report;
        };
        let Ok(definitions) = self.read_normal_store_registry() else {
            return report;
        };
        for binding in bindings {
            let store_id = binding.manifest.store_id.clone();
            let Some(definition) = definitions
                .iter()
                .find(|definition| definition.store_id == store_id)
                .cloned()
            else {
                continue;
            };
            let Ok((backend_root, backend_manifest)) =
                crate::runtime::direct_s3_profile_backend(&binding)
            else {
                continue;
            };
            let Ok(uploads) = crate::runtime::list_recoverable_multipart_uploads(
                &backend_root,
                store_id.as_str(),
            ) else {
                continue;
            };
            for upload in uploads {
                let Some(status) = upload.completion else {
                    continue;
                };
                if matches!(
                    status.state,
                    ProfileS3MultipartCompletionState::Committed
                        | ProfileS3MultipartCompletionState::FailedTerminal
                ) {
                    continue;
                }
                report.discovered += 1;
                let Some(expected_size_bytes) = upload.expected_size_bytes else {
                    report.retained_unsafe += 1;
                    continue;
                };
                if upload.parts.is_empty() {
                    report.retained_unsafe += 1;
                    continue;
                }
                if !upload.scope_recorded {
                    report.retained_unsafe += 1;
                    continue;
                }
                let request = ProfileS3MultipartCompletionRequest {
                    store_id: store_id.clone(),
                    reservation_id: upload.reservation_id,
                    key: upload.object.clone(),
                    expected_size_bytes,
                    parts: upload
                        .parts
                        .into_iter()
                        .map(|part| ProfileS3MultipartPartRequest {
                            part_number: part.part_number,
                            size_bytes: part.size_bytes,
                            checksum: part.checksum,
                        })
                        .collect(),
                };
                let capacity = crate::runtime::direct_s3_profile_capacity(
                    &binding,
                    definition.policy.capacity.clone(),
                );
                let work = MultipartCompletionWork {
                    job_id: status.job_id,
                    backend_root: backend_root.clone(),
                    backend_manifest: backend_manifest.clone(),
                    capacity,
                    provider: Arc::clone(&provider),
                    store_id: store_id.clone(),
                    subobject: upload.subobject,
                    qualified_key: upload.object,
                    definition: definition.clone(),
                    binding: binding.clone(),
                    request,
                    context: self.multipart_completion_worker_context(),
                };
                if ensure_multipart_completion_worker(work) {
                    report.requeued += 1;
                } else {
                    report.retained_unsafe += 1;
                }
            }
        }
        report
    }
}

#[cfg(test)]
mod tests {
    use super::reclaim_stale_assembly_partials;
    use std::fs;

    #[test]
    fn stale_assembly_recovery_is_exactly_reservation_scoped() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-multipart-stale-assembly-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let staging = root.join(".dasobjectstore/staging");
        fs::create_dir_all(&staging).expect("staging");
        let stale = staging.join("upload-a-123-0.part");
        let other = staging.join("upload-b-123-0.part");
        let longer_reservation = staging.join("upload-a-child-123-0.part");
        let suffix_trick = staging.join("upload-a-123-0.part.keep");
        fs::write(&stale, b"partial").expect("stale");
        fs::write(&other, b"other").expect("other");
        fs::write(&longer_reservation, b"longer reservation").expect("longer");
        fs::write(&suffix_trick, b"retain").expect("suffix");

        assert_eq!(
            reclaim_stale_assembly_partials(&root, "upload-a").expect("reclaim"),
            1
        );
        assert!(!stale.exists());
        assert!(other.exists());
        assert!(longer_reservation.exists());
        assert!(suffix_trick.exists());
        assert!(reclaim_stale_assembly_partials(&root, "../unsafe").is_err());
        let _ = fs::remove_dir_all(root);
    }
}
