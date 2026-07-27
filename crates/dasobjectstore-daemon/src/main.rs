use dasobjectstore_daemon::api::DaemonIngestResourceBudget;
use dasobjectstore_daemon::runtime::{
    application_audit_log_path, application_identity_registry_path, application_key_registry_path,
    default_hdd_root, default_ssd_root, garbage_collect_reconciliation_staging,
    profile_binding_registry_path, reconcile_workspace_cleanups,
    reconcile_workspace_materializations, reconcile_workspace_nfs_attachments,
    reconcile_workspace_promotions, reconcile_workspace_provision_operations,
    run_garbage_collection, run_one_durable_destage, spawn_storage_assurance_loop,
    DurableDestageOutcome, DurableDestageWorkerConfig, GarbageCollectDecision, GarbageCollectMode,
    GarbageCollectTrigger, GarbageCollectorConfig, LiveStatusRegistry, StorageAssuranceConfig,
    WorkspaceCleanupWorkerConfig, WorkspacePromotionWorkerConfig, WorkspaceProvisionWorkerConfig,
    DEFAULT_WORKSPACE_HOST_SOCKET,
};
use dasobjectstore_daemon::{
    admin_job_registry_path, appliance_telemetry_state_path, profile_catalogue_live_sqlite_path,
    recover_profile_catalogue_publications, recover_profile_reactivations,
    recover_profile_retirements, AdminJobRegistry, ApplianceTelemetryLoop,
    ApplianceTelemetryLoopConfig, ApplianceTelemetrySink, ApplianceTelemetrySource,
    CapacityReservationLeaseReport, DaemonRequestHandler, DaemonRuntimeConfig,
    FileBackedAdminJobRegistry, FileBackedApplianceTelemetrySink,
    FileBackedCapacityAdmissionProvider, GarageServiceController, GarageServiceRuntimeConfig,
    LinuxProcTelemetryCollector, LiveStatusGarbageCollection, LiveStatusGarbageCollectionRetained,
    SystemDaemonClock, SystemServiceCommandRunner, UnixSocketAdmissionPolicy,
    UnixSocketDaemonServer, DEFAULT_CAPACITY_RESERVATION_LEASE_SECONDS,
    DEFAULT_CAPACITY_RESERVATION_MAINTENANCE_CADENCE_SECONDS, DEFAULT_DAEMON_CONFIG_PATH,
};
use dasobjectstore_object_service::DEFAULT_GARAGE_CONFIG_PATH;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::env;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

fn preserve_pre_logical_identity_metadata(
    live_sqlite_path: &Path,
    migration_root: &Path,
) -> Result<(), String> {
    if !live_sqlite_path.exists() {
        return Ok(());
    }
    let migration_applied = rusqlite::Connection::open_with_flags(
        live_sqlite_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .and_then(|connection| {
        connection.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM metadata_migrations
                 WHERE migration_id=?1 AND name=?2
             ) AND EXISTS(
                 SELECT 1 FROM metadata_format_versions
                 WHERE artifact=?3 AND (
                     major>?4 OR (major=?4 AND minor>=?5)
                 )
             )",
            rusqlite::params![
                dasobjectstore_metadata::LOGICAL_IDENTITY_MIGRATION_ID,
                dasobjectstore_metadata::LOGICAL_IDENTITY_MIGRATION_NAME,
                dasobjectstore_metadata::LIVE_SCHEMA_FORMAT_VERSION
                    .artifact
                    .name(),
                dasobjectstore_metadata::LIVE_SCHEMA_FORMAT_VERSION.major,
                dasobjectstore_metadata::LIVE_SCHEMA_FORMAT_VERSION.minor,
            ],
            |row| row.get::<_, bool>(0),
        )
    })
    .unwrap_or(false);
    if migration_applied {
        return Ok(());
    }
    fs::create_dir_all(migration_root).map_err(|error| {
        format!("could not create metadata migration backup directory: {error}")
    })?;
    protect_migration_directory(migration_root)?;
    let temporary_path = migration_root.join("live-sqlite-pre-0.13.sqlite.tmp");
    match fs::remove_file(&temporary_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "could not remove stale metadata migration backup: {error}"
            ));
        }
    }
    let connection = rusqlite::Connection::open(live_sqlite_path)
        .map_err(|error| format!("could not open live metadata for migration backup: {error}"))?;
    connection
        .execute(
            "VACUUM INTO ?1",
            [temporary_path.to_string_lossy().as_ref()],
        )
        .map_err(|error| format!("could not create logical-identity migration backup: {error}"))?;
    validate_sqlite_backup(&temporary_path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&temporary_path, fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("could not protect metadata migration backup: {error}"))?;
    }
    let backup_digest = sha256_file(&temporary_path)?;
    let backup_path = migration_root.join(format!("live-sqlite-pre-0.13-{backup_digest}.sqlite"));
    if backup_path.exists() {
        validate_sqlite_backup(&backup_path)?;
        fs::remove_file(&temporary_path)
            .map_err(|error| format!("could not remove duplicate migration backup: {error}"))?;
    } else {
        fs::rename(&temporary_path, &backup_path)
            .map_err(|error| format!("could not publish metadata migration backup: {error}"))?;
    }
    File::open(migration_root)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("could not sync metadata migration backup directory: {error}"))?;
    Ok(())
}

fn protect_migration_directory(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("could not inspect metadata migration directory: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("metadata migration backup root is not a regular directory".to_string());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if metadata.uid() != unsafe { libc::geteuid() } {
            return Err("metadata migration backup root has an unexpected owner".to_string());
        }
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|error| {
            format!("could not protect metadata migration backup directory: {error}")
        })?;
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file =
        File::open(path).map_err(|error| format!("could not hash migration backup: {error}"))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("could not hash migration backup: {error}"))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn validate_sqlite_backup(path: &Path) -> Result<(), String> {
    let connection =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|error| format!("could not open metadata migration backup: {error}"))?;
    let integrity = connection
        .query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
        .map_err(|error| format!("could not verify metadata migration backup: {error}"))?;
    if integrity != "ok" {
        return Err(format!(
            "metadata migration backup failed integrity verification: {integrity}"
        ));
    }
    Ok(())
}

fn run() -> Result<(), String> {
    let args = DaemonArgs::parse(env::args().skip(1))?;
    if args.help {
        print_help();
        return Ok(());
    }

    let config = read_config(&args.config_path)?;
    config.validate().map_err(|err| err.to_string())?;

    if args.check_config {
        println!("Daemon config is valid: {}", args.config_path.display());
        return Ok(());
    }

    let capacity_provider = Arc::new(FileBackedCapacityAdmissionProvider::for_daemon(
        &config.state_dir,
    ));
    let garage =
        GarageServiceController::new(garage_runtime_config(&config)?, SystemServiceCommandRunner)
            .with_capacity_admission_provider(capacity_provider.clone())
            .with_ingest_resource_policy(config.ingest_resource_policy);
    let admin_job_registry = Arc::new(FileBackedAdminJobRegistry::new(admin_job_registry_path(
        &config.state_dir,
    )));
    let interrupted = admin_job_registry
        .mark_interrupted_at_startup(&current_utc_timestamp())
        .map_err(|error| error.to_string())?;
    if interrupted > 0 {
        eprintln!("marked {interrupted} interrupted daemon job(s) failed after restart");
    }
    let profile_registry = profile_binding_registry_path(&config.state_dir);
    let startup_timestamp = current_utc_timestamp();
    let live_sqlite_path = profile_catalogue_live_sqlite_path();
    preserve_pre_logical_identity_metadata(
        &live_sqlite_path,
        &config.state_dir.join("metadata-migrations"),
    )?;
    dasobjectstore_metadata::backfill_logical_identities(
        &live_sqlite_path,
        true,
        &startup_timestamp,
    )
    .map_err(|error| format!("logical object identity startup inspection failed: {error}"))?;
    let s3_backfill =
        dasobjectstore_metadata::backfill_s3_object_bindings(&live_sqlite_path, &startup_timestamp)
            .map_err(|error| format!("native S3 binding startup recovery failed: {error}"))?;
    if s3_backfill.bindings_created > 0 || s3_backfill.objects_retained_unmapped > 0 {
        eprintln!(
            "native S3 binding recovery created {} binding(s); retained {} ambiguous object(s) unmapped",
            s3_backfill.bindings_created, s3_backfill.objects_retained_unmapped
        );
    }
    dasobjectstore_metadata::backfill_logical_identities(
        &live_sqlite_path,
        true,
        &startup_timestamp,
    )
    .map_err(|error| format!("post-binding logical identity inspection failed: {error}"))?;
    let logical_identity_backfill = dasobjectstore_metadata::backfill_logical_identities(
        &live_sqlite_path,
        false,
        &startup_timestamp,
    )
    .map_err(|error| format!("logical object identity startup recovery failed: {error}"))?;
    if logical_identity_backfill.logical_versions > 0
        || logical_identity_backfill.placements > 0
        || logical_identity_backfill.needs_review > 0
    {
        eprintln!(
            "logical identity recovery created {} version(s) and {} placement(s); retained {} conflict(s) for review",
            logical_identity_backfill.logical_versions,
            logical_identity_backfill.placements,
            logical_identity_backfill.needs_review
        );
    }
    let retirement_recovery =
        recover_profile_retirements(&profile_registry, profile_catalogue_live_sqlite_path())
            .map_err(|error| format!("profile retirement startup recovery failed: {error}"))?;
    if retirement_recovery.retirements_completed > 0 {
        eprintln!(
            "completed {} interrupted profile retirement(s)",
            retirement_recovery.retirements_completed
        );
    }
    let reactivation_recovery = recover_profile_reactivations(
        &profile_registry,
        dasobjectstore_object_service::default_store_registry_path(),
        profile_catalogue_live_sqlite_path(),
        &current_utc_timestamp(),
    )
    .map_err(|error| format!("profile reactivation startup recovery failed: {error}"))?;
    if reactivation_recovery.reactivations_completed > 0 {
        eprintln!(
            "completed {} interrupted profile reactivation(s)",
            reactivation_recovery.reactivations_completed
        );
    }
    let recovery = recover_profile_catalogue_publications(
        &profile_registry,
        dasobjectstore_object_service::default_store_registry_path(),
        profile_catalogue_live_sqlite_path(),
        &current_utc_timestamp(),
    )
    .map_err(|error| format!("profile catalogue startup recovery failed: {error}"))?;
    if recovery.stores_republished > 0 {
        eprintln!(
            "recovered {} profile catalogue publication(s); removed {} stale journal(s)",
            recovery.stores_republished, recovery.stale_journals_removed
        );
    }
    let live_status_registry = Arc::new(LiveStatusRegistry::default());
    let handler = DaemonRequestHandler::new_with_admin_job_registry(
        garage,
        SystemDaemonClock,
        admin_job_registry,
    )
    .with_profile_binding_registry_path(profile_registry)
    .with_profile_migration_state_root(config.state_dir.join("profile-migrations"))
    .with_application_identity_registry_path(application_identity_registry_path(&config.state_dir))
    .with_application_key_registry_path(application_key_registry_path(&config.state_dir))
    .with_application_audit_log_path(application_audit_log_path(&config.state_dir))
    .with_live_status_registry(Arc::clone(&live_status_registry));
    let _telemetry_loop = spawn_appliance_telemetry_loop(&config)?;
    let _capacity_lease_loop = spawn_capacity_lease_loop(&config, Arc::clone(&capacity_provider));
    let assurance_config = StorageAssuranceConfig::from_environment(&config.state_dir)
        .map_err(|error| error.to_string())?;
    let _assurance_loop = assurance_config.enabled.then(|| {
        spawn_storage_assurance_loop(
            assurance_config,
            Arc::clone(&live_status_registry),
            current_utc_timestamp,
        )
    });
    let _garbage_collection =
        spawn_startup_garbage_collection(&config, Arc::clone(&live_status_registry));
    let _workspace_worker = spawn_workspace_provision_worker(&config);
    let _workspace_materialize_worker = spawn_workspace_materialize_worker(&config);
    let _workspace_promotion_worker = spawn_workspace_promotion_worker(&config, capacity_provider);
    let _workspace_cleanup_worker = spawn_workspace_cleanup_worker(&config);
    let available_cpu_cores = std::thread::available_parallelism()
        .map(|cores| cores.get().min(u16::MAX as usize) as u16)
        .unwrap_or(1);
    let data_stream_connections =
        DaemonIngestResourceBudget::from_policy(config.ingest_resource_policy, available_cpu_cores)
            .concurrent_transaction_limit()
            .max(1) as usize;
    let server = UnixSocketDaemonServer::new(&config.socket_path, handler).with_admission_policy(
        UnixSocketAdmissionPolicy::from_data_stream_budget(data_stream_connections),
    );
    println!(
        "dasobjectstored listening on {}",
        server.socket_path().display()
    );
    server.serve_forever().map_err(|err| err.to_string())
}

fn spawn_workspace_provision_worker(config: &DaemonRuntimeConfig) -> thread::JoinHandle<()> {
    let state_dir = config.state_dir.clone();
    thread::spawn(move || {
        let worker = WorkspaceProvisionWorkerConfig {
            live_sqlite_path: profile_catalogue_live_sqlite_path(),
            broker_socket_path: PathBuf::from(DEFAULT_WORKSPACE_HOST_SOCKET),
            lease_owner: format!("dasobjectstored.{}", std::process::id()),
        };
        loop {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_secs();
            let now_utc = utc_timestamp_for_unix_seconds(now);
            let lease_expires_at_utc = utc_timestamp_for_unix_seconds(now.saturating_add(60));
            match reconcile_workspace_provision_operations(&worker, &now_utc, &lease_expires_at_utc)
            {
                Ok(report) => {
                    let report_path = state_dir.join("workspace-operations/recovery-latest.json");
                    if let Some(parent) = report_path.parent() {
                        if let Err(error) = std::fs::create_dir_all(parent) {
                            eprintln!("workspace recovery report directory failed: {error}");
                            thread::sleep(Duration::from_secs(5));
                            continue;
                        }
                    }
                    match serde_json::to_vec_pretty(&report) {
                        Ok(payload) => {
                            let temporary = report_path.with_extension("json.tmp");
                            if std::fs::read(&report_path).ok().as_deref() != Some(&payload) {
                                if let Err(error) = std::fs::write(&temporary, payload)
                                    .and_then(|_| std::fs::rename(&temporary, &report_path))
                                {
                                    eprintln!(
                                        "workspace recovery report persistence failed: {error}"
                                    );
                                }
                            }
                        }
                        Err(error) => {
                            eprintln!("workspace recovery report encoding failed: {error}")
                        }
                    }
                    if report.completed_operations > 0 || report.retained_for_review > 0 {
                        eprintln!(
                            "workspace worker completed {} operation(s); retained {} for review",
                            report.completed_operations, report.retained_for_review
                        );
                    }
                }
                Err(error) => eprintln!("workspace provision worker cycle deferred: {error}"),
            }
            match reconcile_workspace_nfs_attachments(&worker, &now_utc) {
                Ok(report) => {
                    let report_path =
                        state_dir.join("workspace-operations/nfs-reconciliation-latest.json");
                    if let Ok(payload) = serde_json::to_vec_pretty(&report) {
                        let temporary = report_path.with_extension("json.tmp");
                        if std::fs::read(&report_path).ok().as_deref() != Some(&payload) {
                            if let Err(error) = std::fs::write(&temporary, payload)
                                .and_then(|_| std::fs::rename(&temporary, &report_path))
                            {
                                eprintln!("workspace NFS report persistence failed: {error}");
                            }
                        }
                    }
                }
                Err(error) => eprintln!("workspace NFS reconciliation deferred: {error}"),
            }
            thread::sleep(Duration::from_secs(5));
        }
    })
}

fn spawn_workspace_materialize_worker(config: &DaemonRuntimeConfig) -> thread::JoinHandle<()> {
    let state_dir = config.state_dir.clone();
    thread::spawn(move || {
        let worker = WorkspaceProvisionWorkerConfig {
            live_sqlite_path: profile_catalogue_live_sqlite_path(),
            broker_socket_path: PathBuf::from(DEFAULT_WORKSPACE_HOST_SOCKET),
            lease_owner: format!("dasobjectstored.materialize.{}", std::process::id()),
        };
        loop {
            match reconcile_workspace_materializations(&worker) {
                Ok(report) => {
                    let report_path =
                        state_dir.join("workspace-operations/materialization-latest.json");
                    if let Some(parent) = report_path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    if let Ok(payload) = serde_json::to_vec_pretty(&report) {
                        let temporary = report_path.with_extension("json.tmp");
                        if std::fs::read(&report_path).ok().as_deref() != Some(&payload) {
                            if let Err(error) = std::fs::write(&temporary, payload)
                                .and_then(|_| std::fs::rename(&temporary, &report_path))
                            {
                                eprintln!("workspace materialization report failed: {error}");
                            }
                        }
                    }
                }
                Err(error) => eprintln!("workspace materialization cycle deferred: {error}"),
            }
            thread::sleep(Duration::from_secs(5));
        }
    })
}

fn spawn_workspace_promotion_worker(
    config: &DaemonRuntimeConfig,
    capacity_provider: Arc<FileBackedCapacityAdmissionProvider>,
) -> thread::JoinHandle<()> {
    let state_dir = config.state_dir.clone();
    thread::spawn(move || {
        let worker = WorkspacePromotionWorkerConfig {
            live_sqlite_path: profile_catalogue_live_sqlite_path(),
            ssd_root: default_ssd_root(),
            hdd_root: default_hdd_root(),
            broker_socket_path: PathBuf::from(DEFAULT_WORKSPACE_HOST_SOCKET),
            lease_owner: format!("dasobjectstored.promotion.{}", std::process::id()),
            capacity_provider,
        };
        loop {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_secs();
            let now_utc = utc_timestamp_for_unix_seconds(now);
            let lease_expires_at_utc = utc_timestamp_for_unix_seconds(now.saturating_add(60));
            match reconcile_workspace_promotions(&worker, &now_utc, &lease_expires_at_utc) {
                Ok(report) => {
                    let report_path = state_dir.join("workspace-operations/promotion-latest.json");
                    if let Some(parent) = report_path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    if let Ok(payload) = serde_json::to_vec_pretty(&report) {
                        let temporary = report_path.with_extension("json.tmp");
                        if std::fs::read(&report_path).ok().as_deref() != Some(&payload) {
                            if let Err(error) = std::fs::write(&temporary, payload)
                                .and_then(|_| std::fs::rename(&temporary, &report_path))
                            {
                                eprintln!("workspace promotion report failed: {error}");
                            }
                        }
                    }
                    if report.accepted_members > 0 || report.completed_promotions > 0 {
                        eprintln!(
                            "workspace promotion accepted {} member(s); completed {} bundle(s)",
                            report.accepted_members, report.completed_promotions
                        );
                    }
                }
                Err(error) => eprintln!("workspace promotion cycle deferred: {error}"),
            }
            thread::sleep(Duration::from_secs(5));
        }
    })
}

fn spawn_workspace_cleanup_worker(config: &DaemonRuntimeConfig) -> thread::JoinHandle<()> {
    let state_dir = config.state_dir.clone();
    thread::spawn(move || {
        let worker = WorkspaceCleanupWorkerConfig {
            live_sqlite_path: profile_catalogue_live_sqlite_path(),
            broker_socket_path: PathBuf::from(DEFAULT_WORKSPACE_HOST_SOCKET),
            lease_owner: format!("dasobjectstored.cleanup.{}", std::process::id()),
        };
        loop {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_secs();
            let now_utc = utc_timestamp_for_unix_seconds(now);
            let lease_expires_at_utc = utc_timestamp_for_unix_seconds(now.saturating_add(60));
            match reconcile_workspace_cleanups(&worker, &now_utc, &lease_expires_at_utc) {
                Ok(report) => {
                    let report_path = state_dir.join("workspace-operations/cleanup-latest.json");
                    if let Some(parent) = report_path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    if let Ok(payload) = serde_json::to_vec_pretty(&report) {
                        let temporary = report_path.with_extension("json.tmp");
                        if std::fs::read(&report_path).ok().as_deref() != Some(&payload) {
                            let _ = std::fs::write(&temporary, payload)
                                .and_then(|_| std::fs::rename(&temporary, &report_path));
                        }
                    }
                }
                Err(error) => eprintln!("workspace cleanup cycle deferred: {error}"),
            }
            thread::sleep(Duration::from_secs(5));
        }
    })
}

fn spawn_startup_garbage_collection(
    config: &DaemonRuntimeConfig,
    live_status_registry: Arc<LiveStatusRegistry>,
) -> thread::JoinHandle<()> {
    let state_dir = config.state_dir.clone();
    thread::spawn(move || {
        live_status_registry.record_garbage_collection(LiveStatusGarbageCollection {
            running: true,
            ..LiveStatusGarbageCollection::default()
        });
        let ssd_root = default_ssd_root();
        let gc_config = GarbageCollectorConfig::for_daemon_state(&ssd_root, &state_dir);
        let now_utc = current_utc_timestamp();
        let run_id = format!("startup-{}", current_unix_seconds());
        let result = (|| -> Result<_, String> {
            let inventory = run_garbage_collection(
                &gc_config,
                GarbageCollectMode::Inventory,
                GarbageCollectTrigger::Startup,
                format!("{run_id}-inventory"),
                &now_utc,
                SystemTime::now(),
            )
            .map_err(|error| error.to_string())?;
            let reconciliation_root = ssd_root
                .join(dasobjectstore_metadata::METADATA_DIR_NAME)
                .join("remote-s3-reconcile");
            let reconciliation_inventory = garbage_collect_reconciliation_staging(
                &reconciliation_root,
                &gc_config.live_sqlite_path,
                true,
            )
            .map_err(|error| error.to_string())?;
            let reclaim = run_garbage_collection(
                &gc_config,
                GarbageCollectMode::Reclaim,
                GarbageCollectTrigger::Startup,
                run_id,
                &now_utc,
                SystemTime::now(),
            )
            .map_err(|error| error.to_string())?;
            dasobjectstore_daemon::runtime::persist_garbage_collection_report(
                &gc_config.report_journal_path,
                &reclaim,
            )
            .map_err(|error| error.to_string())?;
            let reconciliation_reclaim = garbage_collect_reconciliation_staging(
                &reconciliation_root,
                &gc_config.live_sqlite_path,
                false,
            )
            .map_err(|error| error.to_string())?;
            persist_reconciliation_garbage_collection_report(
                &state_dir.join("garbage-collection/reconciliation-latest.json"),
                &reconciliation_reclaim,
            )?;
            Ok::<_, String>((
                inventory,
                reclaim,
                reconciliation_inventory,
                reconciliation_reclaim,
            ))
        })();
        match result {
            Ok((inventory, reclaim, reconciliation_inventory, reconciliation_reclaim)) => {
                let scanned_bytes = inventory
                    .items
                    .iter()
                    .map(|item| item.bytes)
                    .sum::<u64>()
                    .saturating_add(
                        reconciliation_inventory
                            .snapshots
                            .iter()
                            .map(|item| item.size_bytes)
                            .sum::<u64>(),
                    );
                let reclaimable_bytes = inventory
                    .candidate_bytes
                    .saturating_add(reconciliation_inventory.reclaimable_bytes);
                let reclaimed_bytes = reclaim
                    .reclaimed_bytes
                    .saturating_add(reconciliation_reclaim.reclaimed_bytes);
                let mut retained = BTreeMap::<(String, String), (u64, u64)>::new();
                for item in reclaim
                    .items
                    .iter()
                    .filter(|item| item.decision == GarbageCollectDecision::Retained)
                {
                    let key = (
                        format!("{:?}", item.kind).to_lowercase(),
                        item.reason.clone(),
                    );
                    let entry = retained.entry(key).or_default();
                    entry.0 = entry.0.saturating_add(1);
                    entry.1 = entry.1.saturating_add(item.bytes);
                }
                for item in reconciliation_reclaim.snapshots.iter().filter(|item| {
                    matches!(item.disposition, dasobjectstore_daemon::runtime::ReconciliationGarbageCollectionDisposition::Retained)
                }) {
                    let entry = retained
                        .entry(("reconciliation".to_string(), item.reason.clone()))
                        .or_default();
                    entry.0 = entry.0.saturating_add(1);
                    entry.1 = entry.1.saturating_add(item.size_bytes);
                }
                live_status_registry.record_garbage_collection(LiveStatusGarbageCollection {
                    running: false,
                    last_completed_at_utc: Some(current_utc_timestamp()),
                    scanned_bytes,
                    reclaimable_bytes,
                    reclaimed_bytes,
                    retained_items: retained.values().map(|(items, _)| *items).sum(),
                    retained_reasons: retained
                        .into_iter()
                        .take(32)
                        .map(|((category, reason), (items, bytes))| {
                            LiveStatusGarbageCollectionRetained {
                                category,
                                reason,
                                items,
                                bytes,
                            }
                        })
                        .collect(),
                    last_error: None,
                });
            }
            Err(error) => {
                eprintln!("startup garbage collection retained all uncertain data: {error}");
                live_status_registry.record_garbage_collection(LiveStatusGarbageCollection {
                    running: false,
                    last_completed_at_utc: Some(current_utc_timestamp()),
                    last_error: Some(
                        "collection failed closed; inspect the daemon journal".to_string(),
                    ),
                    ..LiveStatusGarbageCollection::default()
                });
            }
        }
        // Startup collection owns the initial SSD metadata/removal window. Begin
        // durable destage only after that pass has either completed or failed closed.
        let _ = spawn_durable_destage_loop();
    })
}

fn persist_reconciliation_garbage_collection_report(
    path: &std::path::Path,
    report: &dasobjectstore_daemon::runtime::ReconciliationGarbageCollectionReport,
) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "reconciliation garbage collection report has no parent".to_string())?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temporary = path.with_extension(format!("tmp-{}", current_unix_seconds()));
    let encoded = serde_json::to_vec_pretty(report).map_err(|error| error.to_string())?;
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| error.to_string())?;
    use std::io::Write;
    file.write_all(&encoded)
        .map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    std::fs::rename(&temporary, path).map_err(|error| error.to_string())?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| error.to_string())
}

fn current_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

fn spawn_durable_destage_loop() -> thread::JoinHandle<()> {
    let config = DurableDestageWorkerConfig::from_environment(format!("{}-destage", host_id()));
    thread::spawn(move || {
        let mut previously_served_store = None;
        loop {
            match run_one_durable_destage(
                &config,
                &current_utc_timestamp(),
                previously_served_store.as_ref(),
            ) {
                Ok(DurableDestageOutcome::Settled { store_id, .. }) => {
                    previously_served_store = Some(store_id);
                }
                Ok(DurableDestageOutcome::Idle) => thread::sleep(Duration::from_secs(1)),
                Ok(DurableDestageOutcome::Evicted { .. }) => {}
                Ok(DurableDestageOutcome::Deferred { object_id, message }) => {
                    eprintln!("durable destage deferred for {object_id}: {message}");
                    thread::sleep(Duration::from_secs(1));
                }
                Err(error) => {
                    eprintln!("durable destage worker failed: {error}");
                    thread::sleep(Duration::from_secs(1));
                }
            }
        }
    })
}

fn spawn_capacity_lease_loop(
    config: &DaemonRuntimeConfig,
    provider: Arc<FileBackedCapacityAdmissionProvider>,
) -> thread::JoinHandle<()> {
    let audit_path = dasobjectstore_daemon::capacity_lease_audit_path(&config.state_dir);
    thread::spawn(move || loop {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs();
        match provider
            .maintain_registered_reservation_leases(now, DEFAULT_CAPACITY_RESERVATION_LEASE_SECONDS)
        {
            Ok(report) => record_capacity_lease_report(&audit_path, now, &report),
            Err(error) => eprintln!("capacity reservation lease maintenance failed: {error}"),
        }
        thread::sleep(Duration::from_secs(
            DEFAULT_CAPACITY_RESERVATION_MAINTENANCE_CADENCE_SECONDS,
        ));
    })
}

fn record_capacity_lease_report(
    audit_path: &std::path::Path,
    now_unix_seconds: u64,
    report: &CapacityReservationLeaseReport,
) {
    if let Err(error) = dasobjectstore_daemon::record_capacity_lease_audit_events(
        audit_path,
        now_unix_seconds,
        &report.events,
    ) {
        eprintln!("capacity reservation lease audit failed: {error}");
    }
    if report.expired_reservations > 0 {
        eprintln!(
            "capacity reservation lease maintenance reclaimed {} byte(s) from {} expired reservation(s)",
            report.reclaimed_bytes, report.expired_reservations
        );
    }
}

fn spawn_appliance_telemetry_loop(
    config: &DaemonRuntimeConfig,
) -> Result<Option<thread::JoinHandle<()>>, String> {
    if !config.telemetry.enabled {
        return Ok(None);
    }
    let loop_config = ApplianceTelemetryLoopConfig::new(
        config.telemetry.cadence_seconds,
        ApplianceTelemetrySource {
            appliance_id: "local-appliance".to_string(),
            host_id: host_id(),
            hostname: env::var("HOSTNAME")
                .ok()
                .filter(|value| !value.trim().is_empty()),
        },
    )
    .map_err(|err| err.to_string())?;
    let cadence = loop_config.cadence();
    let telemetry_path = appliance_telemetry_state_path(&config.state_dir);

    Ok(Some(thread::spawn(move || {
        let mut telemetry_loop =
            ApplianceTelemetryLoop::new(loop_config, LinuxProcTelemetryCollector::default());
        let mut sink = FileBackedApplianceTelemetrySink::new(telemetry_path);
        loop {
            match telemetry_loop.collect_once(current_utc_timestamp()) {
                Ok(sample_set) => {
                    if let Err(error) = sink.record(&sample_set) {
                        eprintln!("appliance telemetry write failed: {error}");
                    }
                }
                Err(error) => eprintln!("appliance telemetry collection failed: {error}"),
            }
            thread::sleep(cadence);
        }
    })))
}

fn host_id() -> String {
    env::var("HOSTNAME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "local-host".to_string())
}

fn current_utc_timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    utc_timestamp_for_unix_seconds(seconds)
}

fn utc_timestamp_for_unix_seconds(seconds: u64) -> String {
    let seconds = seconds as libc::time_t;
    let mut tm = std::mem::MaybeUninit::<libc::tm>::uninit();
    let result = unsafe { libc::gmtime_r(&seconds, tm.as_mut_ptr()) };
    if result.is_null() {
        return "1970-01-01T00:00:00Z".to_string();
    }
    let tm = unsafe { tm.assume_init() };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}

fn garage_runtime_config(
    config: &DaemonRuntimeConfig,
) -> Result<GarageServiceRuntimeConfig, String> {
    let config_dir = config.config_path.parent().ok_or_else(|| {
        format!(
            "daemon config path has no parent: {}",
            config.config_path.display()
        )
    })?;
    Ok(GarageServiceRuntimeConfig {
        compose_file: config_dir.join("garage.compose.yml"),
        project_directory: Some(config.state_dir.join("garage")),
        compose_project: config.object_service.compose_project.clone(),
        service_name: "garage".to_string(),
        config_path: PathBuf::from(DEFAULT_GARAGE_CONFIG_PATH),
        metadata_path: PathBuf::from("/srv/dasobjectstore/ssd/garage"),
        data_path: PathBuf::from("/srv/dasobjectstore/hdd/garage"),
        endpoint: config.object_service.endpoint.clone(),
    })
}

fn read_config(path: &PathBuf) -> Result<DaemonRuntimeConfig, String> {
    let file = File::open(path)
        .map_err(|err| format!("failed to open daemon config {}: {err}", path.display()))?;
    serde_json::from_reader(file)
        .map_err(|err| format!("failed to parse daemon config {}: {err}", path.display()))
}

fn print_help() {
    println!("Usage: dasobjectstored [--config <PATH>] [--check-config]");
}

#[derive(Debug)]
struct DaemonArgs {
    config_path: PathBuf,
    check_config: bool,
    help: bool,
}

impl DaemonArgs {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut config_path = PathBuf::from(DEFAULT_DAEMON_CONFIG_PATH);
        let mut check_config = false;
        let mut help = false;
        let mut args = args.into_iter();

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--config" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "--config requires a path".to_string())?;
                    config_path = PathBuf::from(value);
                }
                "--check-config" => check_config = true,
                "-h" | "--help" => help = true,
                value => return Err(format!("unsupported dasobjectstored argument: {value}")),
            }
        }

        Ok(Self {
            config_path,
            check_config,
            help,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        current_utc_timestamp, garage_runtime_config, host_id,
        preserve_pre_logical_identity_metadata, DaemonArgs,
    };
    use dasobjectstore_daemon::DaemonRuntimeConfig;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn parses_config_and_check_flag() {
        let args = DaemonArgs::parse([
            "--config".to_string(),
            "/etc/dasobjectstore/daemon.json".to_string(),
            "--check-config".to_string(),
        ])
        .expect("args parse");

        assert_eq!(
            args.config_path,
            PathBuf::from("/etc/dasobjectstore/daemon.json")
        );
        assert!(args.check_config);
    }

    #[test]
    fn rejects_missing_config_path() {
        let err = DaemonArgs::parse(["--config".to_string()]).expect_err("missing path rejected");

        assert_eq!(err, "--config requires a path");
    }

    #[test]
    fn derives_garage_runtime_paths_from_daemon_config() {
        let config = DaemonRuntimeConfig::linux_packaged();

        let garage = garage_runtime_config(&config).expect("garage config");

        assert_eq!(
            garage.compose_file,
            PathBuf::from("/etc/dasobjectstore/garage.compose.yml")
        );
        assert_eq!(
            garage.project_directory,
            Some(PathBuf::from("/var/lib/dasobjectstore/garage"))
        );
        assert_eq!(
            garage.metadata_path,
            PathBuf::from("/srv/dasobjectstore/ssd/garage")
        );
        assert_eq!(garage.endpoint, "http://127.0.0.1:3900");
        assert_eq!(garage.compose_project, "dasobjectstore");
    }

    #[test]
    fn derives_garage_compose_project_from_daemon_config() {
        let mut config = DaemonRuntimeConfig::linux_packaged();
        config.object_service.compose_project = "dasobjectstore-validation-42".to_string();

        let garage = garage_runtime_config(&config).expect("garage config");

        assert_eq!(garage.compose_project, "dasobjectstore-validation-42");
    }

    #[test]
    fn derives_garage_endpoint_from_daemon_config() {
        let mut config = DaemonRuntimeConfig::linux_packaged();
        config.object_service.endpoint = "http://garage:4900".to_string();

        let garage = garage_runtime_config(&config).expect("garage config");

        assert_eq!(garage.endpoint, "http://garage:4900");
    }

    #[test]
    fn daemon_timestamp_uses_utc_rfc3339_shape() {
        let timestamp = current_utc_timestamp();

        assert_eq!(timestamp.len(), 20);
        assert!(timestamp.ends_with('Z'));
        assert_eq!(&timestamp[4..5], "-");
        assert_eq!(&timestamp[10..11], "T");
    }

    #[test]
    fn daemon_host_id_is_nonblank() {
        assert!(!host_id().trim().is_empty());
    }

    #[test]
    fn startup_preserves_one_private_backup_before_identity_migration() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-logical-identity-backup-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("root");
        let live = root.join("live.sqlite");
        let connection = rusqlite::Connection::open(&live).expect("live sqlite");
        connection
            .execute_batch(
                "CREATE TABLE metadata_migrations (
                     migration_id INTEGER PRIMARY KEY,
                     name TEXT NOT NULL,
                     applied_at_utc TEXT NOT NULL
                 );
                 CREATE TABLE sentinel(value TEXT NOT NULL);
                 INSERT INTO sentinel(value) VALUES ('preserved');",
            )
            .expect("legacy schema");
        drop(connection);

        let backups = root.join("migration-backups");
        preserve_pre_logical_identity_metadata(&live, &backups).expect("backup");
        let backup = fs::read_dir(&backups)
            .expect("backup directory")
            .map(|entry| entry.expect("backup entry").path())
            .find(|path| {
                path.file_name().is_some_and(|name| {
                    let name = name.to_string_lossy();
                    name.starts_with("live-sqlite-pre-0.13-") && name.ends_with(".sqlite")
                })
            })
            .expect("content-bound backup");
        assert!(backup.is_file());
        let backup_connection = rusqlite::Connection::open(&backup).expect("backup sqlite");
        assert_eq!(
            backup_connection
                .query_row("SELECT value FROM sentinel", [], |row| row
                    .get::<_, String>(0))
                .expect("sentinel"),
            "preserved"
        );
        drop(backup_connection);

        preserve_pre_logical_identity_metadata(&live, &backups).expect("idempotent backup");
        fs::remove_file(&backup).expect("remove test backup");
        let connection = rusqlite::Connection::open(&live).expect("live sqlite");
        connection
            .execute(
                "INSERT INTO metadata_migrations(migration_id,name,applied_at_utc)
                 VALUES(?1,?2,'now')",
                rusqlite::params![
                    dasobjectstore_metadata::LOGICAL_IDENTITY_MIGRATION_ID,
                    "conflicting-migration"
                ],
            )
            .expect("migration marker");
        connection
            .execute(
                "CREATE TABLE IF NOT EXISTS metadata_format_versions(
                     artifact TEXT PRIMARY KEY,major INTEGER NOT NULL,
                     minor INTEGER NOT NULL,updated_at_utc TEXT NOT NULL
                 )",
                [],
            )
            .expect("format schema");
        connection
            .execute(
                "INSERT INTO metadata_format_versions(
                     artifact,major,minor,updated_at_utc
                 ) VALUES(?1,?2,?3,'now')",
                rusqlite::params![
                    dasobjectstore_metadata::LIVE_SCHEMA_FORMAT_VERSION
                        .artifact
                        .name(),
                    dasobjectstore_metadata::LIVE_SCHEMA_FORMAT_VERSION.major,
                    dasobjectstore_metadata::LIVE_SCHEMA_FORMAT_VERSION.minor
                ],
            )
            .expect("format marker");
        drop(connection);
        preserve_pre_logical_identity_metadata(&live, &backups)
            .expect("conflicting marker still requires backup");
        for entry in fs::read_dir(&backups).expect("conflict backups") {
            let path = entry.expect("conflict backup").path();
            if path
                .extension()
                .is_some_and(|extension| extension == "sqlite")
            {
                fs::remove_file(path).expect("remove conflict backup");
            }
        }
        let connection = rusqlite::Connection::open(&live).expect("live sqlite");
        connection
            .execute(
                "UPDATE metadata_migrations SET name=?1 WHERE migration_id=?2",
                rusqlite::params![
                    dasobjectstore_metadata::LOGICAL_IDENTITY_MIGRATION_NAME,
                    dasobjectstore_metadata::LOGICAL_IDENTITY_MIGRATION_ID
                ],
            )
            .expect("canonical migration marker");
        drop(connection);
        preserve_pre_logical_identity_metadata(&live, &backups).expect("marked migration");
        assert_eq!(
            fs::read_dir(&backups)
                .expect("marked backups")
                .filter_map(Result::ok)
                .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "sqlite"))
                .count(),
            0
        );
        fs::remove_dir_all(root).expect("cleanup");
    }
}
