fn validate_expected_provider_group(
    objects: &[ReconciliationObject],
    expectation: &crate::api::StoreRepairS3Expectation,
) -> Result<(), DaemonServiceRuntimeError> {
    let required = [
        expectation.payload_key.clone(),
        format!("{}.manifest.json", expectation.payload_key),
        format!("{}.sha256", expectation.payload_key),
    ];
    for key in &required {
        let object = objects
            .iter()
            .find(|object| &object.key == key)
            .ok_or_else(|| DaemonServiceRuntimeError::UnsupportedOperation {
                operation: format!("remote S3 payload group is incomplete: missing {key}"),
            })?;
        if key == &expectation.payload_key && object.size_bytes != Some(expectation.expected_bytes)
        {
            return Err(DaemonServiceRuntimeError::UnsupportedOperation {
                operation: format!(
                    "remote S3 payload size mismatch for {}: expected {} bytes",
                    expectation.payload_key, expectation.expected_bytes
                ),
            });
        }
    }
    Ok(())
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
    expectation: Option<&crate::api::StoreRepairS3Expectation>,
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
    if let Some(expectation) = expectation {
        let objects = manifest
            .entries
            .values()
            .map(|entry| ReconciliationObject {
                key: entry.source_key.clone(),
                size_bytes: entry.size_bytes,
                source_revision: entry.source_revision.clone(),
            })
            .collect::<Vec<_>>();
        validate_expected_provider_group(&objects, expectation)?;
        let payload_path = staging_path.join(&expectation.payload_key);
        let actual = dasobjectstore_metadata::hash_file_sha256(&payload_path).map_err(|error| {
            DaemonServiceRuntimeError::UnsupportedOperation {
                operation: format!(
                    "completed reconciliation payload checksum verification failed for {}: {error}",
                    expectation.payload_key
                ),
            }
        })?;
        if !actual.eq_ignore_ascii_case(&expectation.expected_sha256) {
            return Err(DaemonServiceRuntimeError::UnsupportedOperation {
                operation: format!(
                    "completed reconciliation payload checksum mismatch for {}",
                    expectation.payload_key
                ),
            });
        }
    }
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
            reclaim_proven_completed_snapshot(reconciliation_root, &staging_path)?;
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
        let destage_job_id = format!("destage-{}", staged.object_id);
        let capacity = crate::runtime::build_destage_capacity_claim(
            &live_sqlite_path,
            &crate::runtime::default_hdd_root(),
            &staged.object_id,
            &destage_job_id,
            required_copies,
            staged.bytes_staged,
            &staged.content_hash,
            accepted_at_utc,
        );
        let result = capacity.and_then(|capacity| {
            dasobjectstore_metadata::commit_verified_ssd_and_enqueue_with_capacity_claims(
                &live_sqlite_path,
                dasobjectstore_metadata::VerifiedSsdCommitRequest {
                    destage_job_id: &destage_job_id,
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
                ingress_origin: Some("remote_reconcile"),
                s3_key: staged
                    .object_id
                    .as_str()
                    .strip_prefix(&format!("{}/", store_id.as_str())),
                s3_version: 1,
                },
                &capacity,
            )
            .map_err(|error| error.to_string())
        });
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
    reclaim_proven_completed_snapshot(reconciliation_root, &staging_path)?;
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

fn reclaim_proven_completed_snapshot(
    reconciliation_root: &Path,
    staging_path: &Path,
) -> Result<(), DaemonServiceRuntimeError> {
    let global_root = reconciliation_root.parent().ok_or_else(|| {
        DaemonServiceRuntimeError::UnsupportedOperation {
            operation: "reconciliation root has no managed parent".to_string(),
        }
    })?;
    crate::runtime::garbage_collection::reclaim_managed_directory(global_root, staging_path)
        .map_err(|error| DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!("proven completed snapshot reclaim failed: {error}"),
        })
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

fn completed_exact_prefix_catalogue_response(
    live_sqlite_path: &Path,
    reconciliation_root: &Path,
    bucket_name: &str,
    store_id: &StoreId,
    prefix: Option<String>,
    dry_run: bool,
) -> Result<Option<StoreRepairS3Reconciliation>, DaemonServiceRuntimeError> {
    use dasobjectstore_metadata::{read_object_inspect, ObjectInspectError};
    let Some(exact_prefix) = prefix.as_deref() else {
        return Ok(None);
    };
    let object_id =
        match dasobjectstore_core::ids::ObjectId::new(format!("{store_id}/{exact_prefix}")) {
            Ok(object_id) => object_id,
            Err(_) => return Ok(None),
        };
    let object = match read_object_inspect(live_sqlite_path, &object_id) {
        Ok(object) => object,
        Err(ObjectInspectError::ObjectNotFound(_)) => return Ok(None),
        Err(error) => {
            return Err(DaemonServiceRuntimeError::UnsupportedOperation {
                operation: format!(
                    "exact-prefix catalogue proof failed before provider access: {error}"
                ),
            })
        }
    };
    let (outcome, detail) = if object.store_id == *store_id
        && object.state == "HddCopyVerified"
        && !object.placements.is_empty()
    {
        (
            CompletedSnapshotOutcome::AlreadyDurable,
            "exact provider key already has verified HDD catalogue placement; provider access skipped",
        )
    } else {
        (
            CompletedSnapshotOutcome::RetainedUnsafe,
            "exact provider key is already catalogued but not yet HDD durable; provider access skipped",
        )
    };
    Ok(Some(StoreRepairS3Reconciliation {
        bucket_name: bucket_name.to_string(),
        prefix,
        staging_path: reconciliation_root.display().to_string(),
        manifest_path: None,
        ingest_job_id: None,
        dry_run,
        completed_snapshot_outcome: outcome,
        outcome_detail: Some(detail.to_string()),
    }))
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
