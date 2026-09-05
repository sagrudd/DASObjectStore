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
    read_managed_credential_registry, read_store_registry_with_custody_catalog,
    CustodyCatalogBinding,
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

/// The exact normal registry and immutable custody binding used for a
/// reconciliation. Keeping them together prevents an active custody plane
/// from accidentally pairing a caller-selected registry with a fallback
/// catalog guard.
pub(super) struct ReconciliationRegistryBinding<'a> {
    pub registry_path: &'a Path,
    pub custody_catalog: &'a CustodyCatalogBinding,
}

pub(super) fn reconcile_store_s3<R: ServiceCommandRunner>(
    config: &GarageServiceRuntimeConfig,
    runner: &R,
    registry: ReconciliationRegistryBinding<'_>,
    store_id: StoreId,
    prefix: Option<String>,
    expectation: Option<&crate::api::StoreRepairS3Expectation>,
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
    let definitions =
        read_store_registry_with_custody_catalog(registry.registry_path, registry.custody_catalog)?;
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
        expectation,
        dry_run,
        accepted_at_utc,
        capacity_provider.clone(),
        emit_progress,
    )? {
        return Ok(adoption);
    }
    let live_sqlite_path = crate::runtime::default_ssd_root()
        .join(dasobjectstore_metadata::METADATA_DIR_NAME)
        .join(dasobjectstore_metadata::LIVE_SQLITE_FILE_NAME);
    if let Some(catalogued) = completed_exact_prefix_catalogue_response(
        &live_sqlite_path,
        &reconciliation_root,
        &bucket_name,
        &store_id,
        prefix.clone(),
        dry_run,
    )? {
        return Ok(catalogued);
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
    if let Some(expectation) = expectation {
        validate_expected_provider_group(&objects, expectation)?;
    }
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
    if let Some(expectation) = expectation {
        let payload_path = staging_path.join(&expectation.payload_key);
        let actual = dasobjectstore_metadata::hash_file_sha256(&payload_path).map_err(|error| {
            DaemonServiceRuntimeError::UnsupportedOperation {
                operation: format!(
                    "reconciliation payload checksum verification failed for {}: {error}",
                    expectation.payload_key
                ),
            }
        })?;
        if !actual.eq_ignore_ascii_case(&expectation.expected_sha256) {
            return Err(DaemonServiceRuntimeError::UnsupportedOperation {
                operation: format!(
                    "reconciliation payload checksum mismatch for {}",
                    expectation.payload_key
                ),
            });
        }
    }
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
        Some(registry.custody_catalog.clone()),
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

include!("service_reconciliation/adoption.rs");
include!("service_reconciliation/garbage_collection.rs");
include!("service_reconciliation/provider_transfer.rs");

#[cfg(test)]
mod tests {
    use super::{
        append_range_download, classify_completed_snapshot_catalogue,
        completed_exact_prefix_catalogue_response, deterministic_adoption_id,
        discover_reusable_complete_manifest, garbage_collect_reconciliation_staging,
        garbage_collect_reconciliation_staging_inner, reclaim_proven_completed_snapshot,
        reconciliation_download_args, reconciliation_staging_blockers, validate_sha256_sidecars,
        GarageReconciliationProvider, ReconciliationDownloadRequest,
        ReconciliationGarbageCollectionDisposition, ReconciliationProvider, SnapshotCatalogueState,
    };
    use crate::api::CompletedSnapshotOutcome;
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
            commit_verified_ssd_and_enqueue_with_capacity_claims, DiskCapacityClaimAllocation,
            DiskCapacityClaimKind, DiskCapacityClaimRequest, VerifiedSsdCommitRequest,
            LIVE_SCHEMA_SQL,
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
        connection
            .execute(
                "INSERT OR IGNORE INTO disks (disk_id,pool_id,role,state,size_bytes,created_at_utc,updated_at_utc)
                 VALUES ('disk-a','pool-a','Hdd','Healthy',1000,'now','now')",
                [],
            )
            .expect("disk");
        drop(connection);
        let store_id = StoreId::new("epic_collection").expect("store id");
        let object_id = ObjectId::new("epic_collection/archive.bin").expect("object id");
        let ssd_root = super::ssd_root_for_live_catalogue(live_sqlite_path).expect("SSD root");
        let payload = ssd_root.join(".dasobjectstore/ingest/jobs/job-a/payload");
        fs::create_dir_all(payload.parent().expect("payload parent")).expect("payload directory");
        fs::write(&payload, b"payload").expect("managed payload");
        let content_hash =
            dasobjectstore_metadata::hash_file_sha256(&payload).expect("payload hash");
        commit_verified_ssd_and_enqueue_with_capacity_claims(
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
                s3_key: Some("archive.bin"),
                s3_version: 1,
            },
            &DiskCapacityClaimRequest {
                live_sqlite_path: live_sqlite_path.to_path_buf(),
                kind: DiskCapacityClaimKind::Destage,
                owner_id: object_id.to_string(),
                request_id: "destage:destage-archive".to_string(),
                request_digest: "archive-capacity-v1".to_string(),
                lease_owner: None,
                lease_expires_at_utc: None,
                created_at_utc: "2026-07-19T00:00:00Z".to_string(),
                allocations: vec![DiskCapacityClaimAllocation {
                    disk_id: dasobjectstore_core::ids::DiskId::new("disk-a").expect("disk id"),
                    measured_available_bytes: 1000,
                    requested_bytes: 7,
                }],
            },
        )
        .expect("SSD acknowledgement");
    }

    fn mark_archive_hdd_durable(live_sqlite_path: &std::path::Path) {
        let connection = rusqlite::Connection::open(live_sqlite_path).expect("catalogue");
        connection
            .execute(
                "INSERT OR IGNORE INTO disks (
                    disk_id,pool_id,role,state,size_bytes,created_at_utc,updated_at_utc
                 ) VALUES ('disk-a','pool-a','Hdd','Active',1000,'now','now')",
                [],
            )
            .expect("disk");
        connection
            .execute(
                "INSERT INTO placements (
                    placement_id,object_id,disk_id,relative_path,content_hash,
                    verified_at_utc,created_at_utc
                 ) VALUES (
                    'placement-a','epic_collection/archive.bin','disk-a',
                    'epic_collection/archive.bin',NULL,'now','now'
                 )",
                [],
            )
            .expect("HDD placement");
        connection
            .execute(
                "UPDATE objects SET state='HddCopyVerified' \
                 WHERE object_id='epic_collection/archive.bin'",
                [],
            )
            .expect("object state");
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
    fn exact_catalogued_prefix_blocks_provider_retry_until_hdd_durable() {
        let root = validation_root("exact-prefix-catalogued");
        let live_sqlite_path = root.join("live.sqlite");
        write_ssd_acknowledgement(&live_sqlite_path);
        let store_id = dasobjectstore_core::ids::StoreId::new("epic_collection").expect("store id");

        let response = completed_exact_prefix_catalogue_response(
            &live_sqlite_path,
            &root.join("remote-s3-reconcile/epic_collection"),
            "epic-collection",
            &store_id,
            Some("archive.bin".to_string()),
            false,
        )
        .expect("catalogue proof")
        .expect("provider retry must be blocked");

        assert_eq!(
            response.completed_snapshot_outcome,
            CompletedSnapshotOutcome::RetainedUnsafe
        );
        assert!(response
            .outcome_detail
            .as_deref()
            .is_some_and(|detail| detail.contains("provider access skipped")));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn absent_exact_prefix_does_not_block_provider_discovery() {
        let root = validation_root("exact-prefix-absent");
        let live_sqlite_path = root.join("live.sqlite");
        write_ssd_acknowledgement(&live_sqlite_path);
        let store_id = dasobjectstore_core::ids::StoreId::new("epic_collection").expect("store id");

        assert!(completed_exact_prefix_catalogue_response(
            &live_sqlite_path,
            &root.join("remote-s3-reconcile/epic_collection"),
            "epic-collection",
            &store_id,
            Some("new-object.bin".to_string()),
            false,
        )
        .expect("catalogue proof")
        .is_none());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn exact_hdd_durable_prefix_reports_already_durable_without_provider_access() {
        let root = validation_root("exact-prefix-durable");
        let live_sqlite_path = root.join("live.sqlite");
        write_ssd_acknowledgement(&live_sqlite_path);
        mark_archive_hdd_durable(&live_sqlite_path);
        let store_id = dasobjectstore_core::ids::StoreId::new("epic_collection").expect("store id");

        let response = completed_exact_prefix_catalogue_response(
            &live_sqlite_path,
            &root.join("remote-s3-reconcile/epic_collection"),
            "epic-collection",
            &store_id,
            Some("archive.bin".to_string()),
            false,
        )
        .expect("catalogue proof")
        .expect("durable provider retry must be blocked");

        assert_eq!(
            response.completed_snapshot_outcome,
            CompletedSnapshotOutcome::AlreadyDurable
        );
        assert!(response
            .outcome_detail
            .as_deref()
            .is_some_and(|detail| detail.contains("provider access skipped")));
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
    fn adoption_reclaims_only_the_proven_hard_linked_checkpoint() {
        let root = validation_root("adoption-reclaim-hard-link");
        let reconcile_root = root.join("remote-s3-reconcile");
        let stage = write_complete_snapshot(&reconcile_root, "snapshot-a");
        let managed = root.join("managed/payload");
        fs::create_dir_all(managed.parent().unwrap()).expect("managed parent");
        fs::hard_link(stage.join("archive.bin"), &managed).expect("managed hard link");

        reclaim_proven_completed_snapshot(&reconcile_root.join("epic_collection"), &stage)
            .expect("reclaim checkpoint");

        assert!(!stage.exists());
        assert_eq!(
            fs::read(&managed).expect("managed payload survives"),
            b"payload"
        );
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

    #[test]
    fn exact_remote_group_preflight_requires_all_sidecars_and_payload_size() {
        let expectation = crate::api::StoreRepairS3Expectation {
            payload_key: "EPICv1/GSE224365_RAW.tar".to_string(),
            expected_bytes: 42,
            expected_sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                .to_string(),
        };
        let object = |key: &str, size_bytes| ReconciliationObject {
            key: key.to_string(),
            size_bytes,
            source_revision: Some(format!("etag-{key}")),
        };
        let complete = vec![
            object(&expectation.payload_key, Some(42)),
            object(
                &format!("{}.manifest.json", expectation.payload_key),
                Some(9),
            ),
            object(&format!("{}.sha256", expectation.payload_key), Some(64)),
        ];
        super::validate_expected_provider_group(&complete, &expectation).expect("complete group");

        assert!(super::validate_expected_provider_group(&complete[..2], &expectation).is_err());
        let mut wrong_size = complete;
        wrong_size[0].size_bytes = Some(41);
        assert!(super::validate_expected_provider_group(&wrong_size, &expectation).is_err());
    }
}
