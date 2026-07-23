//! Garage S3 reconciliation transfer orchestration.

use super::capacity_provider::CapacityAdmissionProvider;
use super::reconciliation::{
    discover_complete_reconciliation_manifest, discover_incomplete_reconciliation_manifest,
    plan_reconciliation, ReconciliationAction, ReconciliationEntryState, ReconciliationManifest,
    ReconciliationManifestError, ReconciliationObject,
};
use super::service::{DaemonServiceRuntimeError, GarageServiceRuntimeConfig, ServiceCommandRunner};
use crate::api::{
    CompletedSnapshotOutcome, DaemonIngestConflictPolicy, DaemonIngestResourceGate,
    DaemonIngressOrigin, StoreRepairS3Reconciliation, SubmitIngestFilesRequest,
};
use crate::runtime::ingest_files::resource_gate::submit_ingest_files_with_resource_gate;
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_core::object_type::ObjectType;
use dasobjectstore_object_service::{
    bucket_name_for_definition, default_garage_credential_registry_path,
    default_store_registry_path, read_managed_credential_registry, read_store_registry,
};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) fn reconcile_store_s3<R: ServiceCommandRunner>(
    config: &GarageServiceRuntimeConfig,
    runner: &R,
    store_id: StoreId,
    prefix: Option<String>,
    dry_run: bool,
    accepted_at_utc: &str,
    is_cancelled: &dyn Fn() -> bool,
    capacity_provider: Option<std::sync::Arc<dyn CapacityAdmissionProvider>>,
    resource_gate: Option<std::sync::Arc<DaemonIngestResourceGate>>,
    emit_progress: &mut dyn FnMut(
        crate::api::DaemonIngestProgressEvent,
    ) -> Result<(), crate::runtime::DaemonIngestFilesRuntimeError>,
) -> Result<StoreRepairS3Reconciliation, DaemonServiceRuntimeError> {
    config.validate()?;
    let definitions = read_store_registry(default_store_registry_path())?;
    let definition = definitions
        .iter()
        .find(|definition| definition.store_id == store_id)
        .ok_or_else(|| DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!("S3 reconciliation store {} is not registered", store_id),
        })?;
    let bucket_name = bucket_name_for_definition(definition)?;
    let stage_name = accepted_at_utc
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    let reconciliation_root = crate::runtime::default_ssd_root()
        .join(".dasobjectstore")
        .join("remote-s3-reconcile")
        .join(store_id.as_str());
    if let Some(adoption) = adopt_completed_reconciliation_snapshot(
        &reconciliation_root,
        &bucket_name,
        definition.policy.copies,
        store_id.clone(),
        prefix.clone(),
        dry_run,
        accepted_at_utc,
        capacity_provider.clone(),
        emit_progress,
    )? {
        return Ok(adoption);
    }
    enforce_reconciliation_staging_bound(&reconciliation_root)?;
    let requested_staging_path = reconciliation_root.join(stage_name);
    let mut staging_path = requested_staging_path.clone();
    let manifest_path = staging_path
        .join(".dasobjectstore")
        .join("reconciliation-manifest.json");
    if dry_run {
        return Ok(StoreRepairS3Reconciliation {
            bucket_name,
            prefix,
            staging_path: staging_path.display().to_string(),
            manifest_path: Some(manifest_path.display().to_string()),
            ingest_job_id: None,
            dry_run: true,
            completed_snapshot_outcome: CompletedSnapshotOutcome::NotApplicable,
            outcome_detail: None,
        });
    }

    let mut reused_checkpoint = false;
    let mut manifest_path = if let Some(existing_manifest) =
        discover_incomplete_reconciliation_manifest(
            &reconciliation_root,
            store_id.as_str(),
            prefix.as_deref(),
        )
        .map_err(reconciliation_manifest_error)?
    {
        reused_checkpoint = true;
        staging_path = existing_manifest
            .parent()
            .and_then(|path| path.parent())
            .map(std::path::Path::to_path_buf)
            .ok_or_else(|| DaemonServiceRuntimeError::UnsupportedOperation {
                operation: format!(
                    "reconciliation checkpoint has no staging root: {}",
                    existing_manifest.display()
                ),
            })?;
        existing_manifest
    } else {
        manifest_path
    };

    let credential_registry = read_managed_credential_registry(
        default_garage_credential_registry_path(),
        accepted_at_utc,
    )?;
    let credential = credential_registry
        .credentials
        .iter()
        .find(|credential| credential.store_id == store_id && credential.bucket_name == bucket_name)
        .ok_or_else(|| DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!(
                "S3 reconciliation requires provisioned Garage credentials for {}",
                store_id
            ),
        })?;
    std::fs::create_dir_all(&staging_path).map_err(|error| {
        DaemonServiceRuntimeError::CommandIo {
            program: "create remote S3 staging directory".to_string(),
            message: error.to_string(),
        }
    })?;
    let environment = vec![
        (
            "AWS_ACCESS_KEY_ID".to_string(),
            credential.access_key_id.clone(),
        ),
        (
            "AWS_SECRET_ACCESS_KEY".to_string(),
            credential.secret_access_key.clone(),
        ),
        ("AWS_DEFAULT_REGION".to_string(), "garage".to_string()),
    ];
    let mut manifest = if manifest_path.exists() {
        ReconciliationManifest::load(&manifest_path).map_err(reconciliation_manifest_error)?
    } else {
        ReconciliationManifest::new(store_id.as_str(), prefix.clone())
    };
    if manifest.store_id != store_id.as_str() || manifest.prefix != prefix {
        return Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!(
                "reconciliation manifest identity mismatch at {}",
                manifest_path.display()
            ),
        });
    }
    let provider = GarageReconciliationProvider {
        runner,
        endpoint: &config.endpoint,
        bucket_name: &bucket_name,
        environment: &environment,
    };
    let objects = provider.list_objects(ReconciliationListRequest {
        prefix: prefix.as_deref(),
    })?;
    if !reused_checkpoint {
        if let Some(reusable_manifest) = discover_reusable_complete_manifest(
            &reconciliation_root,
            store_id.as_str(),
            prefix.as_deref(),
            &objects,
        )? {
            let reusable_staging = reusable_manifest
                .parent()
                .and_then(Path::parent)
                .map(Path::to_path_buf)
                .ok_or_else(|| DaemonServiceRuntimeError::UnsupportedOperation {
                    operation: format!(
                        "reconciliation checkpoint has no staging root: {}",
                        reusable_manifest.display()
                    ),
                })?;
            if requested_staging_path != reusable_staging {
                let _ = fs::remove_dir(&requested_staging_path);
            }
            staging_path = reusable_staging;
            manifest_path = reusable_manifest;
            manifest = ReconciliationManifest::load(&manifest_path)
                .map_err(reconciliation_manifest_error)?;
        }
    }
    let plan = plan_reconciliation(&mut manifest, &objects);
    if let Some(action) = plan.actions.iter().find(|action| {
        matches!(
            action,
            ReconciliationAction::InvalidKey { .. } | ReconciliationAction::Collision { .. }
        )
    }) {
        return Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!("reconciliation key rejected: {action:?}"),
        });
    }
    manifest
        .save_atomic(&manifest_path)
        .map_err(reconciliation_manifest_error)?;
    execute_reconciliation_plan(
        &provider,
        &mut manifest,
        &manifest_path,
        &staging_path,
        &store_id,
        &plan.actions,
        is_cancelled,
        emit_progress,
    )?;
    let ingest = submit_ingest_files_with_resource_gate(
        SubmitIngestFilesRequest {
            endpoint: store_id.clone(),
            source_path: staging_path.clone(),
            object_type: ObjectType::Naive,
            copies: None,
            hdd_workers: None,
            ingress_origin: DaemonIngressOrigin::RemoteS3,
            conflict_policy: DaemonIngestConflictPolicy::Lazy,
            dry_run: false,
            client_request_id: Some(format!("garage-reconcile-{accepted_at_utc}")),
        },
        accepted_at_utc,
        emit_progress,
        capacity_provider,
        resource_gate,
    )
    .map_err(|error| DaemonServiceRuntimeError::UnsupportedOperation {
        operation: format!("S3 reconciliation ingest failed: {error}"),
    })?;
    cleanup_completed_staging(
        &reconciliation_root,
        &staging_path,
        store_id.as_str(),
        prefix.as_deref(),
        &ingest,
    )?;
    Ok(StoreRepairS3Reconciliation {
        bucket_name,
        prefix,
        staging_path: staging_path.display().to_string(),
        manifest_path: Some(manifest_path.display().to_string()),
        ingest_job_id: Some(ingest.job_id.to_string()),
        dry_run: false,
        completed_snapshot_outcome: CompletedSnapshotOutcome::NotApplicable,
        outcome_detail: None,
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct ReconciliationAdoptionJournal {
    schema_version: String,
    adoption_id: String,
    store_id: String,
    prefix: Option<String>,
    snapshot_identity: String,
    phase: String,
    verified_hashes: BTreeMap<String, String>,
}

enum SnapshotCatalogueState {
    Durable,
    NeedsAdoption,
    Unsafe(String),
}

#[allow(clippy::too_many_arguments)]
fn adopt_completed_reconciliation_snapshot(
    reconciliation_root: &Path,
    bucket_name: &str,
    required_copies: u8,
    store_id: StoreId,
    prefix: Option<String>,
    dry_run: bool,
    accepted_at_utc: &str,
    capacity_provider: Option<std::sync::Arc<dyn CapacityAdmissionProvider>>,
    emit_progress: &mut dyn FnMut(
        crate::api::DaemonIngestProgressEvent,
    ) -> Result<(), crate::runtime::DaemonIngestFilesRuntimeError>,
) -> Result<Option<StoreRepairS3Reconciliation>, DaemonServiceRuntimeError> {
    let _adoption_guard = reconciliation_adoption_lock().lock().map_err(|_| {
        DaemonServiceRuntimeError::UnsupportedOperation {
            operation: "completed reconciliation adoption lock is poisoned".to_string(),
        }
    })?;
    let Some(manifest_path) = discover_complete_reconciliation_manifest(
        reconciliation_root,
        store_id.as_str(),
        prefix.as_deref(),
    )
    .map_err(reconciliation_manifest_error)?
    else {
        return Ok(None);
    };
    let staging_path = manifest_path
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!(
                "completed reconciliation manifest has no staging root: {}",
                manifest_path.display()
            ),
        })?;
    let manifest =
        ReconciliationManifest::load(&manifest_path).map_err(reconciliation_manifest_error)?;
    let live_sqlite_path = crate::runtime::default_ssd_root()
        .join(dasobjectstore_metadata::METADATA_DIR_NAME)
        .join(dasobjectstore_metadata::LIVE_SQLITE_FILE_NAME);
    let state = classify_completed_snapshot_catalogue(&live_sqlite_path, &staging_path, &manifest);
    match state {
        SnapshotCatalogueState::Unsafe(reason) => {
            return Ok(Some(completed_snapshot_response(
                bucket_name,
                prefix,
                &staging_path,
                &manifest_path,
                None,
                dry_run,
                CompletedSnapshotOutcome::RetainedUnsafe,
                Some(reason),
            )));
        }
        SnapshotCatalogueState::Durable => {
            if let Err(reason) = prove_reconciliation_snapshot_durable(&live_sqlite_path, &manifest)
            {
                return Ok(Some(completed_snapshot_response(
                    bucket_name,
                    prefix,
                    &staging_path,
                    &manifest_path,
                    None,
                    dry_run,
                    CompletedSnapshotOutcome::RetainedUnsafe,
                    Some(reason),
                )));
            }
            if dry_run {
                return Ok(Some(completed_snapshot_response(
                    bucket_name,
                    prefix,
                    &staging_path,
                    &manifest_path,
                    None,
                    true,
                    CompletedSnapshotOutcome::AlreadyDurable,
                    Some(
                        "completed snapshot already has independent durable catalogue evidence"
                            .to_string(),
                    ),
                )));
            }
            let global_root = reconciliation_root.parent().ok_or_else(|| {
                DaemonServiceRuntimeError::UnsupportedOperation {
                    operation: "reconciliation root has no managed parent".to_string(),
                }
            })?;
            garbage_collect_reconciliation_staging_inner(
                global_root,
                &live_sqlite_path,
                false,
                None,
            )?;
            return Ok(Some(completed_snapshot_response(
                bucket_name,
                prefix,
                &staging_path,
                &manifest_path,
                None,
                false,
                CompletedSnapshotOutcome::Reclaimed,
                Some(
                    "already durable completed snapshot reclaimed without provider access"
                        .to_string(),
                ),
            )));
        }
        SnapshotCatalogueState::NeedsAdoption => {}
    }
    if dry_run {
        return Ok(Some(completed_snapshot_response(
            bucket_name,
            prefix,
            &staging_path,
            &manifest_path,
            None,
            true,
            CompletedSnapshotOutcome::RetainedUnsafe,
            Some("completed snapshot is locally adoptable; apply repair to publish it".to_string()),
        )));
    }

    let adoption_id = deterministic_adoption_id(&staging_path, &manifest);
    let journal_path = staging_path
        .join(".dasobjectstore")
        .join("reconciliation-adoption.json");
    let managed_disk_roots = crate::runtime::discover_managed_hdd_roots(
        &crate::runtime::default_hdd_root(),
    )
    .map_err(|error| DaemonServiceRuntimeError::UnsupportedOperation {
        operation: format!("completed snapshot HDD discovery failed: {error}"),
    })?;
    if required_copies == 0 || managed_disk_roots.len() < usize::from(required_copies) {
        return Ok(Some(completed_snapshot_response(
            bucket_name,
            prefix,
            &staging_path,
            &manifest_path,
            Some(adoption_id),
            false,
            CompletedSnapshotOutcome::RetainedUnsafe,
            Some("completed snapshot cannot be adopted because required HDD destinations are unavailable".to_string()),
        )));
    }
    let mut staged_objects = BTreeMap::new();
    let mut verified_hashes = BTreeMap::new();
    for entry in manifest.entries.values() {
        let relative = validate_completed_manifest_entry(&staging_path, entry)?;
        let source_path = staging_path.join(&relative);
        let expected_size =
            entry
                .size_bytes
                .ok_or_else(|| DaemonServiceRuntimeError::UnsupportedOperation {
                    operation: format!("{} has no declared size", entry.source_key),
                })?;
        let object_id =
            dasobjectstore_core::ids::ObjectId::new(format!("{}/{}", store_id, relative)).map_err(
                |error| DaemonServiceRuntimeError::UnsupportedOperation {
                    operation: format!("invalid adopted object identity: {error}"),
                },
            )?;
        // The provider transfer already consumed and accounted for these SSD
        // bytes. Adoption creates a second directory entry for the same inode,
        // not another allocation, so it must not reserve the payload again.
        let _ = &capacity_provider;
        let put_request = dasobjectstore_metadata::ObjectPutRequest::new(
            object_id.clone(),
            &source_path,
            crate::runtime::default_ssd_root(),
            managed_disk_roots.clone(),
            required_copies,
        )
        .with_object_type(ObjectType::Naive);
        let per_object_job = format!("{adoption_id}-{}", short_hash(&relative));
        let managed_ingest_job_id = dasobjectstore_core::ids::IngestJobId::new(
            per_object_job.clone(),
        )
        .map_err(|error| DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!("invalid deterministic adoption identity: {error}"),
        })?;
        let staged =
            dasobjectstore_metadata::adopt_object_on_ssd_by_hard_link_with_controlled_progress(
                &put_request,
                &managed_ingest_job_id,
                expected_size,
                |progress| {
                    emit_reconciliation_key_progress(
                        emit_progress,
                        store_id.clone(),
                        0,
                        manifest.entries.len(),
                        progress.bytes_written,
                        expected_size,
                        &entry.source_key,
                        "verifying completed snapshot for zero-copy adoption",
                    )
                    .map_err(|error| {
                        dasobjectstore_metadata::ObjectPutError::Io(io::Error::other(
                            error.to_string(),
                        ))
                    })
                },
            )
            .map_err(|error| DaemonServiceRuntimeError::UnsupportedOperation {
                operation: format!("completed snapshot adoption failed: {error}"),
            })?;
        verified_hashes.insert(relative.clone(), staged.content_hash.clone());
        staged_objects.insert(relative, (per_object_job, staged));
    }
    validate_sha256_sidecars(&staging_path, &verified_hashes)?;
    save_adoption_journal(
        &journal_path,
        &ReconciliationAdoptionJournal {
            schema_version: "dasobjectstore.reconciliation_adoption.v1".to_string(),
            adoption_id: adoption_id.clone(),
            store_id: store_id.to_string(),
            prefix: prefix.clone(),
            snapshot_identity: staging_path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_string(),
            phase: "verified".to_string(),
            verified_hashes: verified_hashes.clone(),
        },
    )?;
    for (per_object_job, staged) in staged_objects.values() {
        let placement_relative = staged
            .staged_payload_path
            .strip_prefix(crate::runtime::default_ssd_root())
            .map_err(|_| DaemonServiceRuntimeError::UnsupportedOperation {
                operation: "adopted managed SSD path escaped configured root".to_string(),
            })?
            .to_string_lossy()
            .into_owned();
        let result = dasobjectstore_metadata::commit_verified_ssd_and_enqueue(
            &live_sqlite_path,
            dasobjectstore_metadata::VerifiedSsdCommitRequest {
                destage_job_id: &format!("destage-{}", staged.object_id),
                store_id: &store_id,
                object_id: &staged.object_id,
                object_type: &ObjectType::Naive.to_string(),
                relative_path: &placement_relative,
                size_bytes: staged.bytes_staged,
                content_hash_algorithm: &staged.content_hash_algorithm,
                content_hash: &staged.content_hash,
                acknowledgement_policy: "after_ssd_ingest",
                required_copy_count: required_copies,
                max_attempts: 8,
                priority: 0,
                committed_at_utc: accepted_at_utc,
                ingest_job_id: Some(per_object_job),
                ingress_origin: Some("remote_s3"),
            },
        );
        if let Err(error) = result {
            return Ok(Some(completed_snapshot_response(
                bucket_name,
                prefix,
                &staging_path,
                &manifest_path,
                Some(adoption_id),
                false,
                CompletedSnapshotOutcome::RetainedUnsafe,
                Some(format!(
                    "adoption publication retained for retry without provider access: {error}"
                )),
            )));
        }
    }
    save_adoption_journal(
        &journal_path,
        &ReconciliationAdoptionJournal {
            schema_version: "dasobjectstore.reconciliation_adoption.v1".to_string(),
            adoption_id: adoption_id.clone(),
            store_id: store_id.to_string(),
            prefix: prefix.clone(),
            snapshot_identity: staging_path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_string(),
            phase: "ssd_acknowledged".to_string(),
            verified_hashes,
        },
    )?;
    if let Err(reason) = prove_reconciliation_snapshot_durable(&live_sqlite_path, &manifest) {
        return Ok(Some(completed_snapshot_response(
            bucket_name,
            prefix,
            &staging_path,
            &manifest_path,
            Some(adoption_id),
            false,
            CompletedSnapshotOutcome::RetainedUnsafe,
            Some(format!(
                "adoption metadata proof incomplete; retry safely: {reason}"
            )),
        )));
    }
    let global_root = reconciliation_root.parent().ok_or_else(|| {
        DaemonServiceRuntimeError::UnsupportedOperation {
            operation: "reconciliation root has no managed parent".to_string(),
        }
    })?;
    garbage_collect_reconciliation_staging_inner(global_root, &live_sqlite_path, false, None)?;
    Ok(Some(completed_snapshot_response(
        bucket_name,
        prefix,
        &staging_path,
        &manifest_path,
        Some(adoption_id),
        false,
        CompletedSnapshotOutcome::CompletedSnapshotAdopted,
        Some(
            "completed snapshot adopted in place; catalogue visible and HDD destage queued"
                .to_string(),
        ),
    )))
}

fn reconciliation_adoption_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn completed_snapshot_response(
    bucket_name: &str,
    prefix: Option<String>,
    staging_path: &Path,
    manifest_path: &Path,
    ingest_job_id: Option<String>,
    dry_run: bool,
    outcome: CompletedSnapshotOutcome,
    outcome_detail: Option<String>,
) -> StoreRepairS3Reconciliation {
    StoreRepairS3Reconciliation {
        bucket_name: bucket_name.to_string(),
        prefix,
        staging_path: staging_path.display().to_string(),
        manifest_path: Some(manifest_path.display().to_string()),
        ingest_job_id,
        dry_run,
        completed_snapshot_outcome: outcome,
        outcome_detail,
    }
}

fn validate_completed_manifest_entry(
    staging_path: &Path,
    entry: &super::reconciliation::ReconciliationManifestEntry,
) -> Result<String, DaemonServiceRuntimeError> {
    if entry.state != ReconciliationEntryState::Complete {
        return Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!("{} is not a complete checkpoint", entry.source_key),
        });
    }
    let relative = entry.relative_path.as_deref().ok_or_else(|| {
        DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!("{} has no managed relative path", entry.source_key),
        }
    })?;
    if !is_safe_reconciliation_relative_path(Path::new(relative)) {
        return Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!("{} has an unsafe relative path", entry.source_key),
        });
    }
    if entry.source_revision.as_deref().is_none_or(str::is_empty) {
        return Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!("{} has no captured source revision", entry.source_key),
        });
    }
    let expected_size =
        entry
            .size_bytes
            .ok_or_else(|| DaemonServiceRuntimeError::UnsupportedOperation {
                operation: format!("{} has no declared size", entry.source_key),
            })?;
    if entry.downloaded_bytes != expected_size {
        return Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!(
                "{} checkpoint bytes {} do not match declared size {expected_size}",
                entry.source_key, entry.downloaded_bytes
            ),
        });
    }
    let path = staging_path.join(relative);
    let metadata =
        fs::symlink_metadata(&path).map_err(|error| reconciliation_file_error(&path, error))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() != expected_size {
        return Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!(
                "{} is not a regular file with declared size {expected_size}",
                path.display()
            ),
        });
    }
    Ok(relative.to_string())
}

fn classify_completed_snapshot_catalogue(
    live_sqlite_path: &Path,
    staging_path: &Path,
    manifest: &ReconciliationManifest,
) -> SnapshotCatalogueState {
    use dasobjectstore_metadata::{read_destage, read_object_inspect, ObjectInspectError};
    let mut missing = false;
    for entry in manifest.entries.values() {
        let relative = match validate_completed_manifest_entry(staging_path, entry) {
            Ok(relative) => relative,
            Err(error) => return SnapshotCatalogueState::Unsafe(error.to_string()),
        };
        let expected_size = entry.size_bytes.expect("validated size");
        let object_id = match dasobjectstore_core::ids::ObjectId::new(format!(
            "{}/{}",
            manifest.store_id, relative
        )) {
            Ok(object_id) => object_id,
            Err(error) => {
                return SnapshotCatalogueState::Unsafe(format!(
                    "{} has invalid object identity: {error}",
                    entry.source_key
                ))
            }
        };
        match read_destage(live_sqlite_path, &object_id) {
            Ok(Some(queue)) => {
                if queue.store_id.as_str() != manifest.store_id
                    || queue.expected_size_bytes != expected_size
                    || queue.content_hash_algorithm != "sha256"
                {
                    return SnapshotCatalogueState::Unsafe(format!(
                        "ambiguous catalogue/destage identity for {object_id}"
                    ));
                }
            }
            Ok(None) => match read_object_inspect(live_sqlite_path, &object_id) {
                Err(ObjectInspectError::ObjectNotFound(_)) => missing = true,
                Ok(object)
                    if object.store_id.as_str() == manifest.store_id
                        && object.size_bytes == Some(expected_size)
                        && object.state == "HddCopyVerified"
                        && !object.placements.is_empty() => {}
                Ok(_) => {
                    return SnapshotCatalogueState::Unsafe(format!(
                        "partially committed catalogue state is ambiguous for {object_id}"
                    ))
                }
                Err(error) => {
                    return SnapshotCatalogueState::Unsafe(format!(
                        "catalogue proof unavailable for {object_id}: {error}"
                    ))
                }
            },
            Err(error) => {
                return SnapshotCatalogueState::Unsafe(format!(
                    "destage proof unavailable for {object_id}: {error}"
                ))
            }
        }
    }
    if missing {
        SnapshotCatalogueState::NeedsAdoption
    } else {
        SnapshotCatalogueState::Durable
    }
}

fn deterministic_adoption_id(staging_path: &Path, manifest: &ReconciliationManifest) -> String {
    let mut digest = Sha256::new();
    digest.update(b"dasobjectstore.reconciliation_adoption.v1\0");
    digest.update(manifest.store_id.as_bytes());
    digest.update(b"\0");
    digest.update(manifest.prefix.as_deref().unwrap_or_default().as_bytes());
    digest.update(b"\0");
    digest.update(
        staging_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .as_bytes(),
    );
    for entry in manifest.entries.values() {
        digest.update(b"\0");
        digest.update(entry.source_key.as_bytes());
        digest.update(b"\0");
        digest.update(
            entry
                .source_revision
                .as_deref()
                .unwrap_or_default()
                .as_bytes(),
        );
        digest.update(b"\0");
        digest.update(entry.size_bytes.unwrap_or_default().to_be_bytes());
        digest.update(b"\0");
        digest.update(
            entry
                .relative_path
                .as_deref()
                .unwrap_or_default()
                .as_bytes(),
        );
    }
    format!(
        "reconcile-adopt-{}",
        &format!("{:x}", digest.finalize())[..32]
    )
}

fn short_hash(value: &str) -> String {
    let digest = format!("{:x}", Sha256::digest(value.as_bytes()));
    digest[..12].to_string()
}

fn validate_sha256_sidecars(
    staging_path: &Path,
    verified_hashes: &BTreeMap<String, String>,
) -> Result<(), DaemonServiceRuntimeError> {
    for relative in verified_hashes
        .keys()
        .filter(|relative| relative.ends_with(".sha256"))
    {
        let path = staging_path.join(relative);
        let bytes = fs::read(&path).map_err(|error| reconciliation_file_error(&path, error))?;
        if bytes.len() > 64 * 1024 {
            return Err(DaemonServiceRuntimeError::UnsupportedOperation {
                operation: format!("checksum sidecar is unreasonably large: {}", path.display()),
            });
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| {
            DaemonServiceRuntimeError::UnsupportedOperation {
                operation: format!("checksum sidecar is not UTF-8: {}", path.display()),
            }
        })?;
        let mut fields = text.split_whitespace();
        let expected = fields.next().unwrap_or_default();
        let named = fields.next().unwrap_or_default().trim_start_matches('*');
        if expected.len() != 64
            || !expected.bytes().all(|byte| byte.is_ascii_hexdigit())
            || named.is_empty()
            || fields.next().is_some()
        {
            return Err(DaemonServiceRuntimeError::UnsupportedOperation {
                operation: format!("checksum sidecar is malformed: {}", path.display()),
            });
        }
        let parent = Path::new(relative)
            .parent()
            .unwrap_or_else(|| Path::new(""));
        let target = parent.join(named);
        let target = target.to_string_lossy().into_owned();
        let Some(actual) = verified_hashes.get(&target) else {
            return Err(DaemonServiceRuntimeError::UnsupportedOperation {
                operation: format!(
                    "checksum sidecar {} references absent payload {target}",
                    path.display()
                ),
            });
        };
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(DaemonServiceRuntimeError::UnsupportedOperation {
                operation: format!("checksum verification failed for {target}"),
            });
        }
    }
    Ok(())
}

fn save_adoption_journal(
    path: &Path,
    journal: &ReconciliationAdoptionJournal,
) -> Result<(), DaemonServiceRuntimeError> {
    let parent = path
        .parent()
        .ok_or_else(|| DaemonServiceRuntimeError::UnsupportedOperation {
            operation: "adoption journal has no parent".to_string(),
        })?;
    fs::create_dir_all(parent).map_err(|error| reconciliation_file_error(parent, error))?;
    let temporary = path.with_extension(format!("tmp-{}", reconciliation_temp_suffix()));
    let bytes = serde_json::to_vec_pretty(journal).map_err(|error| {
        DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!("adoption journal serialization failed: {error}"),
        }
    })?;
    fs::write(&temporary, bytes).map_err(|error| reconciliation_file_error(&temporary, error))?;
    fs::File::open(&temporary)
        .and_then(|file| file.sync_all())
        .map_err(|error| reconciliation_file_error(&temporary, error))?;
    fs::rename(&temporary, path).map_err(|error| reconciliation_file_error(path, error))?;
    fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| reconciliation_file_error(parent, error))
}

fn cleanup_completed_staging(
    root: &Path,
    completed_staging: &Path,
    _store_id: &str,
    _prefix: Option<&str>,
    ingest: &crate::api::SubmitIngestFilesResponse,
) -> Result<(), DaemonServiceRuntimeError> {
    if ingest.objects.is_empty()
        || !ingest
            .objects
            .iter()
            .all(|object| object.local_copy_may_be_deleted)
    {
        return Ok(());
    }
    let global_root =
        root.parent()
            .ok_or_else(|| DaemonServiceRuntimeError::UnsupportedOperation {
                operation: format!(
                    "reconciliation root has no managed parent: {}",
                    root.display()
                ),
            })?;
    let live_sqlite_path = crate::runtime::default_ssd_root()
        .join(dasobjectstore_metadata::METADATA_DIR_NAME)
        .join(dasobjectstore_metadata::LIVE_SQLITE_FILE_NAME);
    let report =
        garbage_collect_reconciliation_staging_inner(global_root, &live_sqlite_path, false, None)?;
    if completed_staging.exists() {
        let reason = report
            .snapshots
            .iter()
            .find(|snapshot| snapshot.staging_path == completed_staging)
            .map(|snapshot| snapshot.reason.as_str())
            .unwrap_or("completed staging was not classified by garbage collection");
        return Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!(
                "S3 reconciliation cleanup hard fail for {}: {reason}; refusing further staging growth",
                completed_staging.display()
            ),
        });
    }
    Ok(())
}

fn enforce_reconciliation_staging_bound(
    reconciliation_root: &Path,
) -> Result<(), DaemonServiceRuntimeError> {
    let global_root = reconciliation_root.parent().ok_or_else(|| {
        DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!(
                "reconciliation root has no managed parent: {}",
                reconciliation_root.display()
            ),
        }
    })?;
    let live_sqlite_path = crate::runtime::default_ssd_root()
        .join(dasobjectstore_metadata::METADATA_DIR_NAME)
        .join(dasobjectstore_metadata::LIVE_SQLITE_FILE_NAME);
    let report =
        garbage_collect_reconciliation_staging_inner(global_root, &live_sqlite_path, false, None)?;
    let (blocked_snapshots, blocked_bytes) =
        reconciliation_staging_blockers(&report, reconciliation_root);
    if blocked_snapshots == 0 {
        return Ok(());
    }
    Err(DaemonServiceRuntimeError::UnsupportedOperation {
        operation: format!(
            "S3 reconciliation staging hard fail: {blocked_snapshots} retained non-resumable snapshot(s) use {blocked_bytes} bytes below {}; resolve catalogue/durability proof before accepting another reconciliation",
            reconciliation_root.display()
        ),
    })
}

fn reconciliation_staging_blockers(
    report: &ReconciliationGarbageCollectionReport,
    reconciliation_root: &Path,
) -> (usize, u64) {
    let blockers = report
        .snapshots
        .iter()
        .filter(|snapshot| {
            snapshot.staging_path.starts_with(reconciliation_root)
                && snapshot.disposition == ReconciliationGarbageCollectionDisposition::Retained
                && snapshot.reason != "incomplete resumable manifest"
        })
        .collect::<Vec<_>>();
    let blocked_bytes = blockers
        .iter()
        .map(|snapshot| snapshot.size_bytes)
        .sum::<u64>();
    (blockers.len(), blocked_bytes)
}

/// Inventory and, when requested, remove completed remote-S3 reconciliation
/// snapshots after independent managed-placement proof. Incomplete manifests
/// remain resumable checkpoints and are never collection candidates.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct ReconciliationGarbageCollectionReport {
    pub dry_run: bool,
    pub scanned_snapshots: u64,
    pub retained_snapshots: u64,
    pub reclaimable_snapshots: u64,
    pub reclaimed_snapshots: u64,
    pub reclaimable_bytes: u64,
    pub reclaimed_bytes: u64,
    pub snapshots: Vec<ReconciliationGarbageCollectionSnapshot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconciliationGarbageCollectionDisposition {
    Retained,
    Reclaimable,
    Reclaimed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ReconciliationGarbageCollectionSnapshot {
    pub staging_path: PathBuf,
    pub store_id: Option<String>,
    pub object_count: u64,
    pub size_bytes: u64,
    pub disposition: ReconciliationGarbageCollectionDisposition,
    pub reason: String,
}

#[derive(Debug)]
struct CompletedReconciliationSnapshot {
    staging_path: PathBuf,
    manifest: ReconciliationManifest,
    size_bytes: u64,
}

/// Perform a fail-closed reconciliation staging collection pass.
///
/// `dry_run` performs the exact same discovery and durability proof without
/// deleting anything. A completed snapshot is eligible only when every object
/// is independently proven durable in the live catalogue. Unknown files,
/// symlinks, malformed manifests, incomplete transfers, active protected
/// snapshots, and metadata read failures are retained.
pub fn garbage_collect_reconciliation_staging(
    reconciliation_root: &Path,
    live_sqlite_path: &Path,
    dry_run: bool,
) -> Result<ReconciliationGarbageCollectionReport, DaemonServiceRuntimeError> {
    garbage_collect_reconciliation_staging_inner(
        reconciliation_root,
        live_sqlite_path,
        dry_run,
        None,
    )
}

fn garbage_collect_reconciliation_staging_inner(
    reconciliation_root: &Path,
    live_sqlite_path: &Path,
    dry_run: bool,
    protected_staging: Option<&Path>,
) -> Result<ReconciliationGarbageCollectionReport, DaemonServiceRuntimeError> {
    let mut report = ReconciliationGarbageCollectionReport {
        dry_run,
        ..ReconciliationGarbageCollectionReport::default()
    };
    let store_directories = match fs::read_dir(reconciliation_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(report),
        Err(error) => return Err(reconciliation_file_error(reconciliation_root, error)),
    };

    let mut completed = Vec::new();
    for store_entry in store_directories {
        let store_entry =
            store_entry.map_err(|error| reconciliation_file_error(reconciliation_root, error))?;
        let store_path = store_entry.path();
        if store_entry.file_name() == dasobjectstore_metadata::METADATA_DIR_NAME {
            continue;
        }
        let store_type = store_entry
            .file_type()
            .map_err(|error| reconciliation_file_error(&store_path, error))?;
        if !store_type.is_dir() || store_type.is_symlink() {
            continue;
        }
        for stage_entry in fs::read_dir(&store_path)
            .map_err(|error| reconciliation_file_error(&store_path, error))?
        {
            let stage_entry =
                stage_entry.map_err(|error| reconciliation_file_error(&store_path, error))?;
            let staging_path = stage_entry.path();
            let stage_type = stage_entry
                .file_type()
                .map_err(|error| reconciliation_file_error(&staging_path, error))?;
            if !stage_type.is_dir() || stage_type.is_symlink() {
                continue;
            }
            report.scanned_snapshots = report.scanned_snapshots.saturating_add(1);
            let size_bytes = match crate::runtime::garbage_collection::checked_managed_tree_size(
                reconciliation_root,
                &staging_path,
            ) {
                Ok(size) => size,
                Err(error) => {
                    retain_reconciliation_snapshot(
                        &mut report,
                        staging_path,
                        None,
                        0,
                        0,
                        format!("unsafe snapshot tree: {error}"),
                    );
                    continue;
                }
            };
            let manifest_path = staging_path
                .join(".dasobjectstore")
                .join("reconciliation-manifest.json");
            let manifest = match fs::symlink_metadata(&manifest_path) {
                Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                    match ReconciliationManifest::load(&manifest_path) {
                        Ok(manifest) => manifest,
                        Err(error) => {
                            retain_reconciliation_snapshot(
                                &mut report,
                                staging_path,
                                None,
                                0,
                                size_bytes,
                                format!("manifest unreadable: {error}"),
                            );
                            continue;
                        }
                    }
                }
                _ => {
                    retain_reconciliation_snapshot(
                        &mut report,
                        staging_path,
                        None,
                        0,
                        size_bytes,
                        "manifest missing or unsafe".to_string(),
                    );
                    continue;
                }
            };
            if manifest
                .entries
                .values()
                .any(|entry| entry.state != ReconciliationEntryState::Complete)
            {
                retain_reconciliation_snapshot(
                    &mut report,
                    staging_path,
                    Some(manifest.store_id.clone()),
                    manifest.entries.len() as u64,
                    size_bytes,
                    "incomplete resumable manifest".to_string(),
                );
                continue;
            }
            completed.push(CompletedReconciliationSnapshot {
                staging_path,
                manifest,
                size_bytes,
            });
        }
    }

    completed.sort_by(|left, right| left.staging_path.cmp(&right.staging_path));
    for snapshot in completed {
        if protected_staging == Some(snapshot.staging_path.as_path()) {
            retain_reconciliation_snapshot(
                &mut report,
                snapshot.staging_path,
                Some(snapshot.manifest.store_id),
                snapshot.manifest.entries.len() as u64,
                snapshot.size_bytes,
                "active completed provider checkpoint".to_string(),
            );
            continue;
        }
        match prove_reconciliation_snapshot_durable(live_sqlite_path, &snapshot.manifest) {
            Ok(()) => {
                report.reclaimable_snapshots = report.reclaimable_snapshots.saturating_add(1);
                report.reclaimable_bytes =
                    report.reclaimable_bytes.saturating_add(snapshot.size_bytes);
                let (disposition, reason) = if dry_run {
                    (
                        ReconciliationGarbageCollectionDisposition::Reclaimable,
                        "completed snapshot; every object has durable managed placement evidence",
                    )
                } else {
                    crate::runtime::garbage_collection::reclaim_managed_directory(
                        reconciliation_root,
                        &snapshot.staging_path,
                    )
                    .map_err(|error| {
                        DaemonServiceRuntimeError::UnsupportedOperation {
                            operation: error.to_string(),
                        }
                    })?;
                    report.reclaimed_snapshots = report.reclaimed_snapshots.saturating_add(1);
                    report.reclaimed_bytes =
                        report.reclaimed_bytes.saturating_add(snapshot.size_bytes);
                    (
                        ReconciliationGarbageCollectionDisposition::Reclaimed,
                        "completed snapshot reclaimed after durable placement proof",
                    )
                };
                report
                    .snapshots
                    .push(ReconciliationGarbageCollectionSnapshot {
                        staging_path: snapshot.staging_path,
                        store_id: Some(snapshot.manifest.store_id),
                        object_count: snapshot.manifest.entries.len() as u64,
                        size_bytes: snapshot.size_bytes,
                        disposition,
                        reason: reason.to_string(),
                    });
            }
            Err(reason) => retain_reconciliation_snapshot(
                &mut report,
                snapshot.staging_path,
                Some(snapshot.manifest.store_id),
                snapshot.manifest.entries.len() as u64,
                snapshot.size_bytes,
                reason,
            ),
        }
    }
    report
        .snapshots
        .sort_by(|left, right| left.staging_path.cmp(&right.staging_path));
    Ok(report)
}

fn retain_reconciliation_snapshot(
    report: &mut ReconciliationGarbageCollectionReport,
    staging_path: PathBuf,
    store_id: Option<String>,
    object_count: u64,
    size_bytes: u64,
    reason: String,
) {
    report.retained_snapshots = report.retained_snapshots.saturating_add(1);
    report
        .snapshots
        .push(ReconciliationGarbageCollectionSnapshot {
            staging_path,
            store_id,
            object_count,
            size_bytes,
            disposition: ReconciliationGarbageCollectionDisposition::Retained,
            reason,
        });
}

fn prove_reconciliation_snapshot_durable(
    live_sqlite_path: &Path,
    manifest: &ReconciliationManifest,
) -> Result<(), String> {
    use dasobjectstore_metadata::{
        read_destage, read_object_inspect, read_ssd_placement, DestageState,
    };
    if !live_sqlite_path.is_file() {
        return Err(format!(
            "live catalogue unavailable at {}",
            live_sqlite_path.display()
        ));
    }
    for entry in manifest.entries.values() {
        let relative = entry
            .relative_path
            .as_deref()
            .ok_or_else(|| format!("{} has no managed relative path", entry.source_key))?;
        if !is_safe_reconciliation_relative_path(Path::new(relative)) {
            return Err(format!("{} has an unsafe relative path", entry.source_key));
        }
        let object_id =
            dasobjectstore_core::ids::ObjectId::new(format!("{}/{}", manifest.store_id, relative))
                .map_err(|error| {
                    format!("{} has no valid object identity: {error}", entry.source_key)
                })?;
        let expected_size = entry
            .size_bytes
            .ok_or_else(|| format!("{} has no declared size", entry.source_key))?;
        let queue = read_destage(live_sqlite_path, &object_id)
            .map_err(|error| format!("metadata proof failed for {object_id}: {error}"))?;
        if let Some(queue) = queue {
            if queue.store_id.as_str() != manifest.store_id
                || queue.expected_size_bytes != expected_size
            {
                return Err(format!("durable queue identity mismatch for {object_id}"));
            }
            if queue.state == DestageState::HddCopyVerified
                && queue.verified_copy_count >= queue.required_copy_count
            {
                continue;
            }
            if queue.acknowledgement_policy == "after_ssd_ingest" {
                let placement = read_ssd_placement(live_sqlite_path, &object_id)
                    .map_err(|error| format!("SSD proof failed for {object_id}: {error}"))?
                    .ok_or_else(|| format!("verified SSD placement missing for {object_id}"))?;
                if placement.store_id.as_str() == manifest.store_id
                    && placement.size_bytes == expected_size
                    && placement.evicted_at_utc.is_none()
                {
                    let ssd_root = ssd_root_for_live_catalogue(live_sqlite_path)?;
                    let payload = ssd_root.join(&placement.relative_path);
                    if payload.starts_with(
                        ssd_root
                            .join(dasobjectstore_metadata::METADATA_DIR_NAME)
                            .join("remote-s3-reconcile"),
                    ) {
                        return Err(format!(
                            "managed SSD placement still points into reconciliation staging for {object_id}"
                        ));
                    }
                    let metadata = fs::symlink_metadata(&payload).map_err(|error| {
                        format!("SSD placement proof failed for {object_id}: {error}")
                    })?;
                    if metadata.file_type().is_symlink()
                        || !metadata.is_file()
                        || metadata.len() != expected_size
                    {
                        return Err(format!(
                            "managed SSD placement is missing or unsafe for {object_id}"
                        ));
                    }
                    let actual_hash =
                        dasobjectstore_metadata::hash_file_sha256(&payload).map_err(|error| {
                            format!("SSD hash proof failed for {object_id}: {error}")
                        })?;
                    if placement.content_hash_algorithm != "sha256"
                        || !actual_hash.eq_ignore_ascii_case(&placement.content_hash)
                    {
                        return Err(format!(
                            "managed SSD placement checksum mismatch for {object_id}"
                        ));
                    }
                    continue;
                }
            }
            return Err(format!("{object_id} is not durably acknowledged"));
        }
        let object = read_object_inspect(live_sqlite_path, &object_id)
            .map_err(|error| format!("catalogue proof failed for {object_id}: {error}"))?;
        if object.store_id.as_str() != manifest.store_id
            || object.size_bytes != Some(expected_size)
            || object.state != "HddCopyVerified"
            || object.placements.is_empty()
        {
            return Err(format!("verified HDD placement missing for {object_id}"));
        }
    }
    Ok(())
}

fn ssd_root_for_live_catalogue(live_sqlite_path: &Path) -> Result<&Path, String> {
    let parent = live_sqlite_path
        .parent()
        .ok_or_else(|| "live catalogue has no parent directory".to_string())?;
    if parent.file_name().and_then(|name| name.to_str())
        == Some(dasobjectstore_metadata::METADATA_DIR_NAME)
    {
        parent
            .parent()
            .ok_or_else(|| "live catalogue metadata directory has no SSD root".to_string())
    } else {
        Ok(parent)
    }
}

fn is_safe_reconciliation_relative_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn discover_reusable_complete_manifest(
    root: &Path,
    store_id: &str,
    prefix: Option<&str>,
    objects: &[ReconciliationObject],
) -> Result<Option<std::path::PathBuf>, DaemonServiceRuntimeError> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(reconciliation_file_error(root, error)),
    };
    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| reconciliation_file_error(root, error))?;
        let file_type = entry
            .file_type()
            .map_err(|error| reconciliation_file_error(&entry.path(), error))?;
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        let path = entry
            .path()
            .join(".dasobjectstore/reconciliation-manifest.json");
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            continue;
        }
        let manifest =
            ReconciliationManifest::load(&path).map_err(reconciliation_manifest_error)?;
        if manifest.store_id != store_id
            || manifest.prefix.as_deref() != prefix
            || manifest.entries.len() != objects.len()
        {
            continue;
        }
        let staging = path
            .parent()
            .and_then(Path::parent)
            .expect("manifest layout checked");
        let reusable = objects.iter().all(|object| {
            manifest.entries.get(&object.key).is_some_and(|saved| {
                saved.state == ReconciliationEntryState::Complete
                    && saved.size_bytes == object.size_bytes
                    && saved.source_revision == object.source_revision
                    && saved.relative_path.as_deref().is_some_and(|relative| {
                        let candidate = staging.join(relative);
                        fs::metadata(candidate).ok().is_some_and(|metadata| {
                            metadata.is_file()
                                && object.size_bytes.is_none_or(|size| metadata.len() == size)
                        })
                    })
            })
        });
        if reusable {
            candidates.push((manifest.updated_at_unix_seconds, path));
        }
    }
    Ok(candidates
        .into_iter()
        .max_by_key(|(updated, _)| *updated)
        .map(|(_, path)| path))
}

fn execute_reconciliation_plan<P: ReconciliationProvider>(
    provider: &P,
    manifest: &mut ReconciliationManifest,
    manifest_path: &Path,
    staging_path: &Path,
    store_id: &StoreId,
    actions: &[ReconciliationAction],
    is_cancelled: &dyn Fn() -> bool,
    emit_progress: &mut dyn FnMut(
        crate::api::DaemonIngestProgressEvent,
    ) -> Result<(), crate::runtime::DaemonIngestFilesRuntimeError>,
) -> Result<(), DaemonServiceRuntimeError> {
    let total = actions.len();
    for (index, action) in actions.iter().enumerate() {
        if is_cancelled() {
            return Err(DaemonServiceRuntimeError::UnsupportedOperation {
                operation: "S3 reconciliation cancelled by administrator".to_string(),
            });
        }
        match action {
            ReconciliationAction::SkipComplete { .. } => {}
            ReconciliationAction::Download {
                key,
                relative_path,
                size_bytes,
            }
            | ReconciliationAction::Resume {
                key,
                relative_path,
                size_bytes,
                ..
            } => {
                let resume_offset = match action {
                    ReconciliationAction::Resume {
                        downloaded_bytes, ..
                    } => Some(*downloaded_bytes),
                    _ => None,
                };
                let declared_size = *size_bytes;
                if let (Some(offset), Some(size)) = (resume_offset, declared_size) {
                    if offset > size {
                        return Err(DaemonServiceRuntimeError::UnsupportedOperation {
                            operation: format!(
                                "reconciliation checkpoint offset {offset} exceeds declared size {size} for {key}"
                            ),
                        });
                    }
                } else if resume_offset.is_some() {
                    return Err(DaemonServiceRuntimeError::UnsupportedOperation {
                        operation: format!(
                            "reconciliation resume requires a declared size for {key}"
                        ),
                    });
                }
                let size_bytes = declared_size.unwrap_or_default();
                manifest
                    .checkpoint(
                        manifest_path,
                        key,
                        ReconciliationEntryState::InProgress,
                        Some("provider download in progress".to_string()),
                        manifest
                            .entries
                            .get(key)
                            .map(|entry| entry.downloaded_bytes)
                            .unwrap_or_default(),
                    )
                    .map_err(reconciliation_manifest_error)?;
                emit_reconciliation_key_progress(
                    emit_progress,
                    store_id.clone(),
                    index,
                    total,
                    0,
                    size_bytes,
                    key,
                    "provider download started",
                )?;
                let destination = staging_path.join(relative_path);
                if let Some(parent) = destination.parent() {
                    fs::create_dir_all(parent).map_err(|error| {
                        DaemonServiceRuntimeError::CommandIo {
                            program: "create reconciliation object directory".to_string(),
                            message: error.to_string(),
                        }
                    })?;
                }
                if let Some(offset) = resume_offset.filter(|offset| *offset > 0) {
                    if let Err(error) = validate_partial_offset(&destination, offset, key) {
                        manifest
                            .checkpoint(
                                manifest_path,
                                key,
                                ReconciliationEntryState::Failed,
                                Some(error.to_string()),
                                offset,
                            )
                            .map_err(reconciliation_manifest_error)?;
                        return Err(error);
                    }
                }
                let temporary_range_path = resume_offset.filter(|offset| *offset > 0).map(|_| {
                    destination.with_file_name(format!(
                        ".{}.resume-{}",
                        destination
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("object"),
                        reconciliation_temp_suffix()
                    ))
                });
                if let Err(error) = provider.download(ReconciliationDownloadRequest {
                    key,
                    destination: &destination,
                    resume_offset,
                    range_destination: temporary_range_path.as_deref(),
                    is_cancelled,
                }) {
                    if let Some(path) = &temporary_range_path {
                        let _ = fs::remove_file(path);
                    }
                    manifest
                        .checkpoint(
                            manifest_path,
                            key,
                            ReconciliationEntryState::Failed,
                            Some(error.to_string()),
                            resume_offset.unwrap_or_default(),
                        )
                        .map_err(reconciliation_manifest_error)?;
                    return Err(error);
                }
                if let Some(offset) = resume_offset.filter(|offset| *offset > 0) {
                    let partial = temporary_range_path.as_deref().ok_or_else(|| {
                        DaemonServiceRuntimeError::UnsupportedOperation {
                            operation: format!("missing range staging path for {key}"),
                        }
                    })?;
                    if let Err(error) =
                        append_range_download(&destination, partial, offset, size_bytes)
                    {
                        let _ = fs::remove_file(partial);
                        manifest
                            .checkpoint(
                                manifest_path,
                                key,
                                ReconciliationEntryState::Failed,
                                Some(error.to_string()),
                                offset,
                            )
                            .map_err(reconciliation_manifest_error)?;
                        return Err(error);
                    }
                } else if let Some(size) = declared_size {
                    if let Err(error) = validate_downloaded_size(&destination, size, key) {
                        manifest
                            .checkpoint(
                                manifest_path,
                                key,
                                ReconciliationEntryState::Failed,
                                Some(error.to_string()),
                                resume_offset.unwrap_or_default(),
                            )
                            .map_err(reconciliation_manifest_error)?;
                        return Err(error);
                    }
                }
                manifest
                    .checkpoint(
                        manifest_path,
                        key,
                        ReconciliationEntryState::Complete,
                        None,
                        size_bytes,
                    )
                    .map_err(reconciliation_manifest_error)?;
                emit_reconciliation_key_progress(
                    emit_progress,
                    store_id.clone(),
                    index + 1,
                    total,
                    size_bytes,
                    size_bytes,
                    key,
                    "provider download complete",
                )?;
            }
            ReconciliationAction::InvalidKey { .. } | ReconciliationAction::Collision { .. } => {
                unreachable!("rejected before transfer")
            }
        }
    }
    Ok(())
}

/// Provider-neutral listing and transfer seam used by reconciliation. Garage
/// currently supplies the AWS CLI implementation; other providers can
/// implement the same listing, range/resume, and cancellation contract without
/// changing manifest or checkpoint logic.
pub(crate) struct ReconciliationListRequest<'a> {
    pub(crate) prefix: Option<&'a str>,
}

pub(crate) struct ReconciliationDownloadRequest<'a> {
    pub(crate) key: &'a str,
    pub(crate) destination: &'a Path,
    pub(crate) resume_offset: Option<u64>,
    pub(crate) range_destination: Option<&'a Path>,
    pub(crate) is_cancelled: &'a dyn Fn() -> bool,
}

pub(crate) trait ReconciliationProvider {
    fn list_objects(
        &self,
        request: ReconciliationListRequest<'_>,
    ) -> Result<Vec<ReconciliationObject>, DaemonServiceRuntimeError>;

    fn download(
        &self,
        request: ReconciliationDownloadRequest<'_>,
    ) -> Result<(), DaemonServiceRuntimeError>;
}

struct GarageReconciliationProvider<'a, R> {
    runner: &'a R,
    endpoint: &'a str,
    bucket_name: &'a str,
    environment: &'a [(String, String)],
}

impl<R: ServiceCommandRunner> ReconciliationProvider for GarageReconciliationProvider<'_, R> {
    fn list_objects(
        &self,
        request: ReconciliationListRequest<'_>,
    ) -> Result<Vec<ReconciliationObject>, DaemonServiceRuntimeError> {
        list_garage_objects(
            self.runner,
            self.endpoint,
            self.bucket_name,
            request.prefix,
            self.environment,
        )
    }

    fn download(
        &self,
        request: ReconciliationDownloadRequest<'_>,
    ) -> Result<(), DaemonServiceRuntimeError> {
        let args = reconciliation_download_args(
            self.endpoint,
            self.bucket_name,
            request.key,
            request.destination,
            request.resume_offset,
            request.range_destination,
        );
        self.runner
            .run_with_display_args_and_env_cancellable(
                "aws",
                &args,
                &args,
                self.environment,
                request.is_cancelled,
            )
            .map(|_| ())
    }
}

fn reconciliation_download_args(
    endpoint: &str,
    bucket_name: &str,
    key: &str,
    destination: &Path,
    resume_offset: Option<u64>,
    range_destination: Option<&Path>,
) -> Vec<String> {
    match resume_offset.filter(|offset| *offset > 0) {
        Some(offset) => vec![
            "--endpoint-url".to_string(),
            endpoint.to_string(),
            "s3api".to_string(),
            "get-object".to_string(),
            "--bucket".to_string(),
            bucket_name.to_string(),
            "--key".to_string(),
            key.to_string(),
            "--range".to_string(),
            format!("bytes={offset}-"),
            range_destination
                .expect("range destination is required for a non-zero resume")
                .display()
                .to_string(),
        ],
        _ => vec![
            "--endpoint-url".to_string(),
            endpoint.to_string(),
            "s3".to_string(),
            "cp".to_string(),
            format!("s3://{bucket_name}/{key}"),
            destination.display().to_string(),
            "--no-progress".to_string(),
        ],
    }
}

fn reconciliation_temp_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
}

fn append_range_download(
    destination: &Path,
    partial: &Path,
    offset: u64,
    expected_size: u64,
) -> Result<(), DaemonServiceRuntimeError> {
    let destination_label = destination.display().to_string();
    validate_partial_offset(destination, offset, &destination_label)?;
    let partial_size = fs::metadata(partial)
        .map_err(|error| reconciliation_file_error(partial, error))?
        .len();
    let expected_suffix = expected_size.checked_sub(offset).ok_or_else(|| {
        DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!(
                "reconciliation range offset exceeds size for {}",
                destination.display()
            ),
        }
    })?;
    if partial_size != expected_suffix {
        return Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!(
                "reconciliation range size {partial_size} does not match expected suffix {expected_suffix} for {}",
                destination.display()
            ),
        });
    }
    let mut output = OpenOptions::new()
        .append(true)
        .open(destination)
        .map_err(|error| reconciliation_file_error(destination, error))?;
    let mut input =
        fs::File::open(partial).map_err(|error| reconciliation_file_error(partial, error))?;
    io::copy(&mut input, &mut output)
        .map_err(|error| reconciliation_file_error(destination, error))?;
    output
        .sync_all()
        .map_err(|error| reconciliation_file_error(destination, error))?;
    let final_size = fs::metadata(destination)
        .map_err(|error| reconciliation_file_error(destination, error))?
        .len();
    if final_size != expected_size {
        return Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!(
                "reconciliation destination size {final_size} does not match expected size {expected_size} for {}",
                destination.display()
            ),
        });
    }
    fs::remove_file(partial).map_err(|error| reconciliation_file_error(partial, error))
}

fn validate_partial_offset(
    destination: &Path,
    offset: u64,
    key: &str,
) -> Result<(), DaemonServiceRuntimeError> {
    let destination_size = fs::metadata(destination)
        .map_err(|error| reconciliation_file_error(destination, error))?
        .len();
    if destination_size != offset {
        return Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!(
                "reconciliation partial size {destination_size} does not match checkpoint offset {offset} for {key}"
            ),
        });
    }
    Ok(())
}

fn validate_downloaded_size(
    destination: &Path,
    expected_size: u64,
    key: &str,
) -> Result<(), DaemonServiceRuntimeError> {
    let actual = fs::metadata(destination)
        .map_err(|error| reconciliation_file_error(destination, error))?
        .len();
    if actual != expected_size {
        return Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!(
                "reconciliation download size {actual} does not match expected size {expected_size} for {key}"
            ),
        });
    }
    Ok(())
}

fn reconciliation_file_error(path: &Path, error: io::Error) -> DaemonServiceRuntimeError {
    DaemonServiceRuntimeError::CommandIo {
        program: "reconciliation file".to_string(),
        message: format!("{}: {error}", path.display()),
    }
}

pub(super) fn list_garage_objects<R: ServiceCommandRunner>(
    runner: &R,
    endpoint: &str,
    bucket_name: &str,
    prefix: Option<&str>,
    environment: &[(String, String)],
) -> Result<Vec<ReconciliationObject>, DaemonServiceRuntimeError> {
    let mut objects = Vec::new();
    let mut continuation_token: Option<String> = None;
    loop {
        let mut args = vec![
            "--endpoint-url".to_string(),
            endpoint.to_string(),
            "s3api".to_string(),
            "list-objects-v2".to_string(),
            "--bucket".to_string(),
            bucket_name.to_string(),
            "--output".to_string(),
            "json".to_string(),
        ];
        if let Some(prefix) = prefix.filter(|prefix| !prefix.trim().is_empty()) {
            args.extend(["--prefix".to_string(), prefix.trim_matches('/').to_string()]);
        }
        if let Some(token) = continuation_token.as_deref() {
            args.extend(["--continuation-token".to_string(), token.to_string()]);
        }
        let output = runner.run_with_display_args_and_env("aws", &args, &args, environment)?;
        let value: Value = serde_json::from_str(&output.stdout).map_err(|error| {
            DaemonServiceRuntimeError::UnsupportedOperation {
                operation: format!("Garage object listing returned invalid JSON: {error}"),
            }
        })?;
        if let Some(contents) = value.get("Contents").and_then(Value::as_array) {
            for object in contents {
                let Some(key) = object.get("Key").and_then(Value::as_str) else {
                    return Err(DaemonServiceRuntimeError::UnsupportedOperation {
                        operation: "Garage object listing contained an entry without Key"
                            .to_string(),
                    });
                };
                objects.push(ReconciliationObject {
                    key: key.to_string(),
                    size_bytes: object.get("Size").and_then(Value::as_u64),
                    source_revision: object
                        .get("ETag")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned),
                });
            }
        }
        let truncated = value
            .get("IsTruncated")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !truncated {
            break;
        }
        continuation_token = value
            .get("NextContinuationToken")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        if continuation_token.is_none() {
            return Err(DaemonServiceRuntimeError::UnsupportedOperation {
                operation: "Garage object listing was truncated without a continuation token"
                    .to_string(),
            });
        }
    }
    Ok(objects)
}

fn reconciliation_manifest_error(error: ReconciliationManifestError) -> DaemonServiceRuntimeError {
    DaemonServiceRuntimeError::UnsupportedOperation {
        operation: error.to_string(),
    }
}

fn emit_reconciliation_key_progress(
    emit_progress: &mut dyn FnMut(
        crate::api::DaemonIngestProgressEvent,
    ) -> Result<(), crate::runtime::DaemonIngestFilesRuntimeError>,
    endpoint: StoreId,
    files_done: usize,
    files_total: usize,
    work_bytes_done: u64,
    work_bytes_total: u64,
    key: &str,
    message: &str,
) -> Result<(), DaemonServiceRuntimeError> {
    use dasobjectstore_core::ids::IngestJobId;
    emit_progress(crate::api::DaemonIngestProgressEvent {
        job_id: IngestJobId::new("store-repair-s3-reconcile").expect("static job id"),
        endpoint,
        stage: crate::api::DaemonIngestStage::SsdIngest,
        pipeline_stage: Some(crate::api::DaemonIngestPipelineStage::SsdStage),
        work_bytes_done,
        work_bytes_total: Some(work_bytes_total),
        source_bytes_done: Some(work_bytes_done),
        source_bytes_total: Some(work_bytes_total),
        stage_bytes_done: Some(work_bytes_done),
        stage_bytes_total: Some(work_bytes_total),
        files_done: files_done as u64,
        files_total: Some(files_total as u64),
        current_object_id: None,
        ssd_pressure: None,
        telemetry: None,
        active_hdd_transfers: Vec::new(),
        resource_policy: None,
        message: Some(format!("{message}: {key}")),
    })
    .map_err(|error| DaemonServiceRuntimeError::UnsupportedOperation {
        operation: format!("reconciliation progress delivery failed: {error}"),
    })
}

#[cfg(test)]
mod tests {
    use super::{
        append_range_download, classify_completed_snapshot_catalogue, deterministic_adoption_id,
        discover_reusable_complete_manifest, garbage_collect_reconciliation_staging,
        garbage_collect_reconciliation_staging_inner, reconciliation_download_args,
        reconciliation_staging_blockers, validate_sha256_sidecars, GarageReconciliationProvider,
        ReconciliationDownloadRequest, ReconciliationGarbageCollectionDisposition,
        ReconciliationProvider, SnapshotCatalogueState,
    };
    use crate::runtime::reconciliation::{
        ReconciliationEntryState, ReconciliationManifest, ReconciliationManifestEntry,
        ReconciliationObject,
    };
    use crate::runtime::service::{ServiceCommandOutput, ServiceCommandRunner};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Mutex;

    fn write_complete_snapshot(root: &std::path::Path, stage_name: &str) -> PathBuf {
        write_complete_snapshot_with_prefix(root, stage_name, None)
    }

    fn write_complete_snapshot_with_prefix(
        root: &std::path::Path,
        stage_name: &str,
        prefix: Option<&str>,
    ) -> PathBuf {
        let stage = root.join("epic_collection").join(stage_name);
        fs::create_dir_all(stage.join(".dasobjectstore")).expect("manifest parent");
        fs::write(stage.join("archive.bin"), b"payload").expect("payload");
        let mut manifest =
            ReconciliationManifest::new("epic_collection", prefix.map(str::to_string));
        manifest.entries.insert(
            "archive.bin".to_string(),
            ReconciliationManifestEntry {
                source_key: "archive.bin".to_string(),
                relative_path: Some("archive.bin".to_string()),
                size_bytes: Some(7),
                source_revision: Some("etag-1".to_string()),
                state: ReconciliationEntryState::Complete,
                downloaded_bytes: 7,
                message: None,
            },
        );
        manifest
            .save_atomic(
                &stage
                    .join(".dasobjectstore")
                    .join("reconciliation-manifest.json"),
            )
            .expect("save manifest");
        stage
    }

    fn write_ssd_acknowledgement(live_sqlite_path: &std::path::Path) {
        use dasobjectstore_core::ids::{ObjectId, StoreId};
        use dasobjectstore_metadata::{
            commit_verified_ssd_and_enqueue, VerifiedSsdCommitRequest, LIVE_SCHEMA_SQL,
        };
        let connection = rusqlite::Connection::open(live_sqlite_path).expect("catalogue");
        connection
            .execute_batch(LIVE_SCHEMA_SQL)
            .expect("live schema");
        connection
            .execute(
                "INSERT INTO pools (pool_id, state, created_at_utc, updated_at_utc) VALUES ('pool-a','Healthy','now','now')",
                [],
            )
            .expect("pool");
        connection
            .execute(
                "INSERT INTO stores (store_id,pool_id,class,policy_json,created_at_utc,updated_at_utc) VALUES ('epic_collection','pool-a','GeneratedData','{}','now','now')",
                [],
            )
            .expect("store");
        drop(connection);
        let store_id = StoreId::new("epic_collection").expect("store id");
        let object_id = ObjectId::new("epic_collection/archive.bin").expect("object id");
        let ssd_root = super::ssd_root_for_live_catalogue(live_sqlite_path).expect("SSD root");
        let payload = ssd_root.join(".dasobjectstore/ingest/jobs/job-a/payload");
        fs::create_dir_all(payload.parent().expect("payload parent")).expect("payload directory");
        fs::write(&payload, b"payload").expect("managed payload");
        let content_hash =
            dasobjectstore_metadata::hash_file_sha256(&payload).expect("payload hash");
        commit_verified_ssd_and_enqueue(
            live_sqlite_path,
            VerifiedSsdCommitRequest {
                destage_job_id: "destage-archive",
                store_id: &store_id,
                object_id: &object_id,
                object_type: "naive",
                relative_path: ".dasobjectstore/ingest/jobs/job-a/payload",
                size_bytes: 7,
                content_hash_algorithm: "sha256",
                content_hash: &content_hash,
                acknowledgement_policy: "after_ssd_ingest",
                required_copy_count: 1,
                max_attempts: 8,
                priority: 0,
                committed_at_utc: "2026-07-19T00:00:00Z",
                ingest_job_id: None,
                ingress_origin: None,
            },
        )
        .expect("SSD acknowledgement");
    }

    struct RecordingRunner(Mutex<Vec<Vec<String>>>);

    impl ServiceCommandRunner for RecordingRunner {
        fn run(
            &self,
            _program: &str,
            args: &[String],
        ) -> Result<ServiceCommandOutput, crate::runtime::service::DaemonServiceRuntimeError>
        {
            self.0.lock().expect("runner lock").push(args.to_vec());
            Ok(ServiceCommandOutput {
                stdout: String::new(),
            })
        }
    }

    fn validation_root(label: &str) -> PathBuf {
        let root = std::env::var_os("DASOBJECTSTORE_CODEX_VALIDATION_ROOT")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|home| PathBuf::from(home).join(".dasobjectstore-codex-validation"))
            })
            .unwrap_or_else(std::env::temp_dir)
            .join(format!(
                "service-reconciliation-{label}-{}",
                std::process::id()
            ));
        fs::create_dir_all(&root).expect("validation root");
        root
    }

    #[test]
    fn reuses_complete_staging_only_when_provider_identity_and_payload_match() {
        let root = validation_root("reusable-complete");
        let stage = root.join("stage");
        fs::create_dir_all(stage.join(".dasobjectstore")).expect("manifest parent");
        fs::write(stage.join("archive.bin"), b"payload").expect("payload");
        let mut manifest = ReconciliationManifest::new("epic_collection", None);
        manifest.entries.insert(
            "archive.bin".to_string(),
            ReconciliationManifestEntry {
                source_key: "archive.bin".to_string(),
                relative_path: Some("archive.bin".to_string()),
                size_bytes: Some(7),
                source_revision: Some("etag-1".to_string()),
                state: ReconciliationEntryState::Complete,
                downloaded_bytes: 7,
                message: None,
            },
        );
        let manifest_path = stage.join(".dasobjectstore/reconciliation-manifest.json");
        manifest.save_atomic(&manifest_path).expect("save manifest");
        let objects = vec![ReconciliationObject {
            key: "archive.bin".to_string(),
            size_bytes: Some(7),
            source_revision: Some("etag-1".to_string()),
        }];
        assert_eq!(
            discover_reusable_complete_manifest(&root, "epic_collection", None, &objects)
                .expect("discover"),
            Some(manifest_path)
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn complete_snapshot_without_catalogue_records_is_adoptable_and_stable() {
        let root = validation_root("complete-adoptable");
        let reconcile_root = root.join("remote-s3-reconcile");
        let stage = write_complete_snapshot(&reconcile_root, "snapshot-a");
        let manifest = ReconciliationManifest::load(
            &stage.join(".dasobjectstore/reconciliation-manifest.json"),
        )
        .expect("manifest");
        let live_sqlite_path = root.join("live.sqlite");
        assert!(matches!(
            classify_completed_snapshot_catalogue(&live_sqlite_path, &stage, &manifest),
            SnapshotCatalogueState::NeedsAdoption
        ));
        assert_eq!(
            deterministic_adoption_id(&stage, &manifest),
            deterministic_adoption_id(&stage, &manifest)
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn complete_snapshot_with_verified_ssd_catalogue_is_durable() {
        let root = validation_root("complete-durable");
        let reconcile_root = root.join("remote-s3-reconcile");
        let stage = write_complete_snapshot(&reconcile_root, "snapshot-a");
        let manifest = ReconciliationManifest::load(
            &stage.join(".dasobjectstore/reconciliation-manifest.json"),
        )
        .expect("manifest");
        let live_sqlite_path = root.join("live.sqlite");
        write_ssd_acknowledgement(&live_sqlite_path);
        assert!(matches!(
            classify_completed_snapshot_catalogue(&live_sqlite_path, &stage, &manifest),
            SnapshotCatalogueState::Durable
        ));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn partially_committed_completed_group_resumes_missing_objects() {
        let root = validation_root("complete-partial");
        let reconcile_root = root.join("remote-s3-reconcile");
        let stage = write_complete_snapshot(&reconcile_root, "snapshot-a");
        fs::write(stage.join("second.bin"), b"second").expect("second payload");
        let manifest_path = stage.join(".dasobjectstore/reconciliation-manifest.json");
        let mut manifest = ReconciliationManifest::load(&manifest_path).expect("manifest");
        manifest.entries.insert(
            "second.bin".to_string(),
            ReconciliationManifestEntry {
                source_key: "second.bin".to_string(),
                relative_path: Some("second.bin".to_string()),
                size_bytes: Some(6),
                source_revision: Some("etag-2".to_string()),
                state: ReconciliationEntryState::Complete,
                downloaded_bytes: 6,
                message: None,
            },
        );
        manifest
            .save_atomic(&manifest_path)
            .expect("updated manifest");
        let live_sqlite_path = root.join("live.sqlite");
        write_ssd_acknowledgement(&live_sqlite_path);

        assert!(matches!(
            classify_completed_snapshot_catalogue(&live_sqlite_path, &stage, &manifest),
            SnapshotCatalogueState::NeedsAdoption
        ));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn checksum_sidecar_validation_fails_closed_on_mismatch() {
        let root = validation_root("sidecar-mismatch");
        let payload = root.join("archive.bin");
        let sidecar = root.join("archive.bin.sha256");
        fs::write(&payload, b"payload").expect("payload");
        fs::write(&sidecar, format!("{}  archive.bin\n", "0".repeat(64))).expect("sidecar");
        let mut hashes = std::collections::BTreeMap::new();
        hashes.insert(
            "archive.bin".to_string(),
            dasobjectstore_metadata::hash_file_sha256(&payload).expect("hash"),
        );
        hashes.insert(
            "archive.bin.sha256".to_string(),
            dasobjectstore_metadata::hash_file_sha256(&sidecar).expect("sidecar hash"),
        );
        assert!(validate_sha256_sidecars(&root, &hashes).is_err());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn garbage_collection_dry_run_reports_every_proven_completed_snapshot() {
        let root = validation_root("gc-dry-run");
        let reconcile_root = root.join("remote-s3-reconcile");
        let old = write_complete_snapshot(&reconcile_root, "a-old");
        let newest = write_complete_snapshot(&reconcile_root, "z-new");
        let live_sqlite_path = root.join("live.sqlite");
        write_ssd_acknowledgement(&live_sqlite_path);

        let report =
            garbage_collect_reconciliation_staging(&reconcile_root, &live_sqlite_path, true)
                .expect("dry-run inventory");

        assert_eq!(report.scanned_snapshots, 2);
        assert_eq!(report.reclaimable_snapshots, 2);
        assert_eq!(report.reclaimed_snapshots, 0);
        assert!(old.exists());
        assert!(newest.exists());
        assert_eq!(
            report
                .snapshots
                .iter()
                .find(|snapshot| snapshot.staging_path == old)
                .expect("old snapshot")
                .disposition,
            ReconciliationGarbageCollectionDisposition::Reclaimable
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn garbage_collection_reclaims_every_proven_completed_snapshot() {
        let root = validation_root("gc-apply");
        let reconcile_root = root.join("remote-s3-reconcile");
        let old = write_complete_snapshot(&reconcile_root, "a-old");
        let newest = write_complete_snapshot(&reconcile_root, "z-new");
        let live_sqlite_path = root.join("live.sqlite");
        write_ssd_acknowledgement(&live_sqlite_path);

        let report =
            garbage_collect_reconciliation_staging(&reconcile_root, &live_sqlite_path, false)
                .expect("collection");

        assert_eq!(report.reclaimed_snapshots, 2);
        assert!(!old.exists());
        assert!(!newest.exists());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn garbage_collection_reclaims_progressive_unique_prefix_snapshots() {
        let root = validation_root("gc-progressive-prefixes");
        let reconcile_root = root.join("remote-s3-reconcile");
        let first = write_complete_snapshot_with_prefix(
            &reconcile_root,
            "first-window",
            Some("EPICv1/GSE000001"),
        );
        let second = write_complete_snapshot_with_prefix(
            &reconcile_root,
            "second-window",
            Some("EPICv1/GSE000002"),
        );
        let live_sqlite_path = root.join("live.sqlite");
        write_ssd_acknowledgement(&live_sqlite_path);

        let report =
            garbage_collect_reconciliation_staging(&reconcile_root, &live_sqlite_path, false)
                .expect("collection");

        assert_eq!(report.reclaimed_snapshots, 2);
        assert!(!first.exists());
        assert!(!second.exists());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn garbage_collection_never_reclaims_the_active_completed_checkpoint() {
        let root = validation_root("gc-active");
        let reconcile_root = root.join("remote-s3-reconcile");
        let active = write_complete_snapshot(&reconcile_root, "a-active");
        let otherwise_newest = write_complete_snapshot(&reconcile_root, "z-new");
        let live_sqlite_path = root.join("live.sqlite");
        write_ssd_acknowledgement(&live_sqlite_path);

        let report = garbage_collect_reconciliation_staging_inner(
            &reconcile_root,
            &live_sqlite_path,
            false,
            Some(&active),
        )
        .expect("protected collection");

        assert_eq!(report.reclaimed_snapshots, 1);
        assert!(active.exists());
        assert!(!otherwise_newest.exists());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn garbage_collection_retains_incomplete_and_unproven_snapshots() {
        let root = validation_root("gc-retain");
        let reconcile_root = root.join("remote-s3-reconcile");
        let old = write_complete_snapshot(&reconcile_root, "a-old");
        let newest = write_complete_snapshot(&reconcile_root, "z-new");
        let incomplete = reconcile_root.join("epic_collection").join("partial");
        fs::create_dir_all(incomplete.join(".dasobjectstore")).expect("manifest parent");
        let mut manifest = ReconciliationManifest::new("epic_collection", None);
        manifest.entries.insert(
            "partial.bin".to_string(),
            ReconciliationManifestEntry {
                source_key: "partial.bin".to_string(),
                relative_path: Some("partial.bin".to_string()),
                size_bytes: Some(10),
                source_revision: None,
                state: ReconciliationEntryState::InProgress,
                downloaded_bytes: 4,
                message: None,
            },
        );
        manifest
            .save_atomic(
                &incomplete
                    .join(".dasobjectstore")
                    .join("reconciliation-manifest.json"),
            )
            .expect("save partial manifest");

        let report = garbage_collect_reconciliation_staging(
            &reconcile_root,
            &root.join("missing-live.sqlite"),
            false,
        )
        .expect("fail-closed collection");

        assert_eq!(report.reclaimed_snapshots, 0);
        assert!(old.exists());
        assert!(newest.exists());
        assert!(incomplete.exists());
        assert!(report
            .snapshots
            .iter()
            .any(|snapshot| snapshot.reason == "incomplete resumable manifest"));
        let (blockers, blocked_bytes) =
            reconciliation_staging_blockers(&report, &reconcile_root.join("epic_collection"));
        assert_eq!(blockers, 2);
        assert!(blocked_bytes >= 14);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn incomplete_resumable_snapshot_does_not_trip_growth_hard_fail() {
        let root = validation_root("gc-incomplete-bound");
        let reconcile_root = root.join("remote-s3-reconcile");
        let incomplete = reconcile_root.join("epic_collection").join("partial");
        fs::create_dir_all(incomplete.join(".dasobjectstore")).expect("manifest parent");
        let mut manifest = ReconciliationManifest::new("epic_collection", None);
        manifest.entries.insert(
            "partial.bin".to_string(),
            ReconciliationManifestEntry {
                source_key: "partial.bin".to_string(),
                relative_path: Some("partial.bin".to_string()),
                size_bytes: Some(10),
                source_revision: None,
                state: ReconciliationEntryState::InProgress,
                downloaded_bytes: 4,
                message: None,
            },
        );
        manifest
            .save_atomic(
                &incomplete
                    .join(".dasobjectstore")
                    .join("reconciliation-manifest.json"),
            )
            .expect("save partial manifest");

        let report = garbage_collect_reconciliation_staging(
            &reconcile_root,
            &root.join("missing-live.sqlite"),
            false,
        )
        .expect("fail-closed collection");
        assert_eq!(
            reconciliation_staging_blockers(&report, &reconcile_root.join("epic_collection")),
            (0, 0)
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn resume_command_requests_only_the_missing_suffix() {
        let destination = PathBuf::from("/var/lib/dasobjectstore/partial.bin");
        let range = PathBuf::from("/var/lib/dasobjectstore/.partial.bin.resume");
        let args = reconciliation_download_args(
            "http://127.0.0.1:3900",
            "bucket-1",
            "reads/sample.fastq",
            &destination,
            Some(12),
            Some(&range),
        );
        assert_eq!(args[2], "s3api");
        assert_eq!(args[3], "get-object");
        assert_eq!(args[8], "--range");
        assert_eq!(args[9], "bytes=12-");
        assert_eq!(args[10], range.display().to_string());
        assert!(!args.iter().any(|arg| arg == "cp"));
    }

    #[test]
    fn appends_and_fsyncs_verified_range_suffix() {
        let root = validation_root("append");
        let destination = root.join("partial.bin");
        let range = root.join("partial.bin.resume");
        fs::write(&destination, b"abc").expect("partial destination");
        fs::write(&range, b"def").expect("range suffix");

        append_range_download(&destination, &range, 3, 6).expect("append range");

        assert_eq!(fs::read(&destination).expect("destination"), b"abcdef");
        assert!(!range.exists());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn rejects_range_when_existing_partial_size_drifted() {
        let root = validation_root("drift");
        let destination = root.join("partial.bin");
        let range = root.join("partial.bin.resume");
        fs::write(&destination, b"ab").expect("partial destination");
        fs::write(&range, b"def").expect("range suffix");

        assert!(append_range_download(&destination, &range, 3, 6).is_err());
        assert_eq!(fs::read(&destination).expect("destination"), b"ab");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn provider_download_adapter_preserves_command_boundary_and_cancellation() {
        let runner = RecordingRunner(Mutex::new(Vec::new()));
        let environment = vec![("AWS_ACCESS_KEY_ID".to_string(), "redacted".to_string())];
        let adapter = GarageReconciliationProvider {
            runner: &runner,
            endpoint: "http://127.0.0.1:3900",
            bucket_name: "bucket-1",
            environment: &environment,
        };
        adapter
            .download(ReconciliationDownloadRequest {
                key: "reads/sample.fastq",
                destination: PathBuf::from("/tmp/object").as_path(),
                resume_offset: Some(12),
                range_destination: Some(PathBuf::from("/tmp/object.resume").as_path()),
                is_cancelled: &|| false,
            })
            .expect("provider command");
        let args = runner.0.lock().expect("runner lock")[0].clone();
        assert_eq!(args[2], "s3api");
        assert_eq!(args[8], "--range");
        assert_eq!(args[9], "bytes=12-");
        assert!(adapter
            .download(ReconciliationDownloadRequest {
                key: "reads/sample.fastq",
                destination: PathBuf::from("/tmp/object").as_path(),
                resume_offset: None,
                range_destination: None,
                is_cancelled: &|| true,
            })
            .is_err());
        assert_eq!(runner.0.lock().expect("runner lock").len(), 1);
    }
}
