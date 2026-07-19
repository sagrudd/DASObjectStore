use dasobjectstore_daemon::runtime::{
    application_audit_log_path, application_identity_registry_path, application_key_registry_path,
    default_ssd_root, garbage_collect_reconciliation_staging, profile_binding_registry_path,
    run_garbage_collection, run_one_durable_destage, DurableDestageOutcome,
    DurableDestageWorkerConfig, GarbageCollectDecision, GarbageCollectMode, GarbageCollectTrigger,
    GarbageCollectorConfig, LiveStatusRegistry,
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
    SystemDaemonClock, SystemServiceCommandRunner, UnixSocketDaemonServer,
    DEFAULT_CAPACITY_RESERVATION_LEASE_SECONDS,
    DEFAULT_CAPACITY_RESERVATION_MAINTENANCE_CADENCE_SECONDS, DEFAULT_DAEMON_CONFIG_PATH,
};
use dasobjectstore_object_service::DEFAULT_GARAGE_CONFIG_PATH;
use std::collections::BTreeMap;
use std::env;
use std::fs::File;
use std::path::PathBuf;
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
    let _capacity_lease_loop = spawn_capacity_lease_loop(&config, capacity_provider);
    let _garbage_collection = spawn_startup_garbage_collection(&config, live_status_registry);
    let server = UnixSocketDaemonServer::new(&config.socket_path, handler);
    println!(
        "dasobjectstored listening on {}",
        server.socket_path().display()
    );
    server.serve_forever().map_err(|err| err.to_string())
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
        .as_secs() as libc::time_t;
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
    use super::{current_utc_timestamp, garage_runtime_config, host_id, DaemonArgs};
    use dasobjectstore_daemon::DaemonRuntimeConfig;
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
}
