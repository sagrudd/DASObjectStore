//! Garage S3 reconciliation transfer orchestration.

use super::capacity_provider::CapacityAdmissionProvider;
use super::reconciliation::{
    discover_incomplete_reconciliation_manifest, plan_reconciliation, ReconciliationAction,
    ReconciliationEntryState, ReconciliationManifest, ReconciliationManifestError,
    ReconciliationObject,
};
use super::service::{DaemonServiceRuntimeError, GarageServiceRuntimeConfig, ServiceCommandRunner};
use crate::api::{
    DaemonIngestConflictPolicy, DaemonIngestResourceGate, DaemonIngressOrigin,
    StoreRepairS3Reconciliation, SubmitIngestFilesRequest,
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
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Component, Path, PathBuf};
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
    cleanup_duplicate_completed_staging(
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
    })
}

fn cleanup_duplicate_completed_staging(
    root: &Path,
    retained_staging: &Path,
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
    let _ = garbage_collect_reconciliation_staging_inner(
        global_root,
        &live_sqlite_path,
        false,
        Some(retained_staging),
    )?;
    Ok(())
}

/// Inventory and, when requested, remove redundant completed remote-S3
/// reconciliation snapshots. The newest matching snapshot is always retained
/// so it remains available as the provider checkpoint. Incomplete manifests
/// are never collection candidates.
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
    signature: String,
    size_bytes: u64,
}

/// Perform a fail-closed reconciliation staging collection pass.
///
/// `dry_run` performs the exact same discovery and durability proof without
/// deleting anything. A snapshot is eligible only when it is an older exact
/// duplicate of another completed manifest and every object is independently
/// proven durable in the live catalogue. Unknown files, symlinks, malformed
/// manifests, incomplete transfers, and metadata read failures are retained.
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
            let signature = reconciliation_manifest_signature(&manifest)?;
            completed.push(CompletedReconciliationSnapshot {
                staging_path,
                manifest,
                signature,
                size_bytes,
            });
        }
    }

    let mut groups = BTreeMap::<String, Vec<CompletedReconciliationSnapshot>>::new();
    for snapshot in completed {
        groups
            .entry(snapshot.signature.clone())
            .or_default()
            .push(snapshot);
    }
    for snapshots in groups.values_mut() {
        snapshots.sort_by(|left, right| {
            left.manifest
                .updated_at_unix_seconds
                .cmp(&right.manifest.updated_at_unix_seconds)
                .then_with(|| left.staging_path.cmp(&right.staging_path))
        });
        let protected_index = protected_staging
            .and_then(|protected| {
                snapshots
                    .iter()
                    .position(|snapshot| snapshot.staging_path == protected)
            })
            .unwrap_or_else(|| snapshots.len().saturating_sub(1));
        if snapshots.is_empty() {
            continue;
        }
        let newest = snapshots.remove(protected_index);
        let newest_is_protected = protected_staging == Some(newest.staging_path.as_path());
        retain_reconciliation_snapshot(
            &mut report,
            newest.staging_path,
            Some(newest.manifest.store_id),
            newest.manifest.entries.len() as u64,
            newest.size_bytes,
            if newest_is_protected {
                "active completed provider checkpoint".to_string()
            } else {
                "newest completed provider checkpoint".to_string()
            },
        );
        for snapshot in snapshots.drain(..) {
            match prove_reconciliation_snapshot_durable(live_sqlite_path, &snapshot.manifest) {
                Ok(()) => {
                    report.reclaimable_snapshots = report.reclaimable_snapshots.saturating_add(1);
                    report.reclaimable_bytes =
                        report.reclaimable_bytes.saturating_add(snapshot.size_bytes);
                    let (disposition, reason) = if dry_run {
                        (
                            ReconciliationGarbageCollectionDisposition::Reclaimable,
                            "older duplicate; every object has durable managed placement evidence",
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
                            "older duplicate reclaimed after durable placement proof",
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

fn reconciliation_manifest_signature(
    manifest: &ReconciliationManifest,
) -> Result<String, DaemonServiceRuntimeError> {
    serde_json::to_string(&(
        manifest.store_id.as_str(),
        manifest.prefix.as_deref(),
        &manifest.entries,
    ))
    .map_err(|error| DaemonServiceRuntimeError::UnsupportedOperation {
        operation: format!("failed to fingerprint reconciliation manifest: {error}"),
    })
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
        append_range_download, discover_reusable_complete_manifest,
        garbage_collect_reconciliation_staging, garbage_collect_reconciliation_staging_inner,
        reconciliation_download_args, GarageReconciliationProvider, ReconciliationDownloadRequest,
        ReconciliationGarbageCollectionDisposition, ReconciliationProvider,
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
        let stage = root.join("epic_collection").join(stage_name);
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
                content_hash: "hash-a",
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
    fn garbage_collection_dry_run_reports_only_proven_completed_duplicate() {
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
        assert_eq!(report.reclaimable_snapshots, 1);
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
    fn garbage_collection_reclaims_proven_duplicate_and_retains_newest() {
        let root = validation_root("gc-apply");
        let reconcile_root = root.join("remote-s3-reconcile");
        let old = write_complete_snapshot(&reconcile_root, "a-old");
        let newest = write_complete_snapshot(&reconcile_root, "z-new");
        let live_sqlite_path = root.join("live.sqlite");
        write_ssd_acknowledgement(&live_sqlite_path);

        let report =
            garbage_collect_reconciliation_staging(&reconcile_root, &live_sqlite_path, false)
                .expect("collection");

        assert_eq!(report.reclaimed_snapshots, 1);
        assert!(!old.exists());
        assert!(newest.exists());
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
