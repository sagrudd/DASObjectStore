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
