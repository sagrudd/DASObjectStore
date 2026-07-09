#[cfg(feature = "debug-commands")]
use crate::cli::PoolMarkerArgs;
use crate::cli::{
    Cli, Command, DiskCommand, DiskDrainArgs, DiskForceRetireArgs, DiskLockdownDasArgs,
    DiskPrepareDasArgs, DiskPrepareFilesystem, DiskReplaceArgs, DiskRetireArgs, HealthArgs,
    IngestCommand, IngestDirectImportArgs, IngestDrainQueueArgs, IngestFilesArgs, IngestQueueArgs,
    IngestStatusArgs, MnemosyneCommand, MnemosyneExportArgs, MnemosyneValidateNasNfsEndpointArgs,
    ObjectCommand, ObjectExportArgs, ObjectInspectArgs, ObjectPutArgs, PerformanceFileOrder,
    PerformanceFileSelection, PerformanceReportArgs, PerformanceScenarioSelection,
    PerformanceTestArgs, PoolCommand, PoolImportArgs, PoolInspectArgs, PoolRepairArgs, ProbeArgs,
    ServiceCommand, ServiceComposeArgs, ServiceRenderComposeArgs, ServiceStatusArgs, StatusArgs,
    StoreAdoptArgs, StoreCommand, StoreContentsArgs, StoreCreateArgs, StoreDefaultsArgs,
    StoreDeleteArgs, StoreDrainArgs, StoreListArgs, StoreS3UploadArgs, StoreValidateArgs,
    SubobjectArgs, SubobjectCommand, SubobjectCreateArgs, SubobjectListArgs, SubobjectSearchArgs,
};
mod disk_lockdown;
mod disk_prepare;
mod output;

use self::disk_lockdown::{
    lockdown_das, LockdownDasError, LockdownDasRequest, LOCKDOWN_CONFIRMATION,
};
use self::disk_prepare::{
    prepare_das, PrepareDasDevice, PrepareDasError, PrepareDasRequest, PrepareDasRole,
    PrepareFilesystem,
};
use self::output::{
    write_disk_drain_plan, write_disk_force_retirement_report, write_disk_replacement_plan,
    write_disk_retirement_report, write_health_json, write_health_summary, write_health_verbose,
    write_host_connection_status, write_ingest_direct_import_report, write_ingest_status,
    write_lockdown_das_report, write_nas_nfs_endpoint_validation_report,
    write_object_export_report, write_object_inspect_summary, write_object_put_report,
    write_pool_import_report, write_pool_inspect_summary, write_pool_repair_dry_run,
    write_prepare_das_report, write_pretty_report, write_remote_s3_upload_plan,
    write_store_create_report, write_store_delete_report, write_store_drain_report,
    write_store_list_report,
};
use dasobjectstore_core::health::{HealthScore, HealthSignals};
use dasobjectstore_core::ids::{DiskId, ObjectId, StoreId};
use dasobjectstore_core::lifecycle::PoolState;
use dasobjectstore_core::placement::{
    plan_copy_count_for_store, PerformanceClass, PlacementCandidate, PlacementRequest, WriteLoad,
};
use dasobjectstore_core::risk::{
    ActionConfirmation, RiskGate, RiskGateError, RiskPolicy, RiskyOperation,
};
use dasobjectstore_core::store::{StorePolicy, StorePolicyValidationErrors};
use dasobjectstore_core::DEFAULT_STANDALONE_CONFIG_PATH;
use dasobjectstore_daemon::{
    authoritative_performance_recommendation_path, DaemonClient, DaemonClientError,
    DaemonClientTransport, DaemonIngestProgressEvent, DaemonIngestStage, DaemonRuntimeConfig,
    SubmitIngestFilesRequest, SubmitIngestFilesResponse, UnixSocketDaemonTransport,
    DEFAULT_DAEMON_STATE_DIR,
};
use dasobjectstore_gui_api::StandaloneServerConfig;
use dasobjectstore_metadata::{
    attach_clean_pool_read_only, delete_store, drain_ingest_queue, drain_store,
    export_settled_object, force_retire_disk, import_dirty_pool_read_only,
    import_reproducible_object_direct_to_hdd, inspect_pool_metadata, measure_ssd_capacity,
    put_object_ssd_first, put_object_ssd_first_with_progress, read_disk_drain_plan,
    read_disk_replacement_plan, read_ingest_queue_for_store, read_object_inspect,
    read_store_contents, request_disk_retirement, DestagePriorityPolicy, DirectHddImportError,
    DirectHddImportRequest, DiskCopyRoot, DiskDrainError, DiskRetirementError,
    IngestQueueDrainError, IngestQueueDrainReport, IngestQueueDrainRequest, IngestQueueReadError,
    IngestQueueSnapshot, ObjectExportError, ObjectExportRequest, ObjectInspectError,
    ObjectPutError, ObjectPutProgress, ObjectPutProgressStage, ObjectPutRequest, PoolInspectError,
    ReadOnlyAttachError, ReadOnlyAttachOptions, SsdCapacityMeasurementError, SsdCapacityPolicy,
    SsdCapacityPolicyError, StoreCleanupError, StoreContentsObject, StoreContentsReadError,
    StoreContentsRequest, StoreContentsSnapshot, StoreDeleteRequest, StoreDrainRequest,
    LIVE_SQLITE_FILE_NAME, METADATA_DIR_NAME,
};
#[cfg(feature = "debug-commands")]
use dasobjectstore_metadata::{record_pool_state_marker_at, PoolStateMarker};
use dasobjectstore_mnemosyne::{
    export_mneion_binding_snippet, export_mneion_storage_definition,
    validate_nas_nfs_endpoint_definition, MneionBindingSnippetError, MneionBindingSnippetRequest,
    MneionStorageDefinitionError, MneionStorageDefinitionRequest, NasNfsEndpointDefinition,
    NasNfsEndpointValidationError,
};
use dasobjectstore_object_service::{
    create_subobject_definition, credential_reference_for_store, default_store_registry_path,
    default_subobject_registry_path, delete_store_definition, delete_subobjects_for_store,
    mirror_subobject_definition, plan_remote_s3_upload, plan_store_service_layout,
    portable_store_registry_path, portable_subobject_registry_path, read_store_registry,
    read_subobject_registry, render_compose, search_subobjects, upsert_store_definition,
    ComposeRenderRequest, ComposeServiceConfig, GarageProvider, GarageProviderConfig,
    ObjectServiceError, ObjectServiceProvider, ObjectServiceProviderId, RemoteS3UploadPlanRequest,
    StoreRegistryDeleteReport, StoreRegistryUpdateReport, StoreServiceDefinition,
    SubObjectDefinition, SubObjectParent, SubObjectRegistryStoreDeleteReport,
    SubObjectRegistryUpdateReport,
};
#[cfg(target_os = "linux")]
use dasobjectstore_platform::linux::LinuxProbeProvider;
#[cfg(target_os = "linux")]
use dasobjectstore_platform::linux_smart::read_smartctl_health;
#[cfg(target_os = "macos")]
use dasobjectstore_platform::macos::MacosProbeProvider;
#[cfg(target_os = "macos")]
use dasobjectstore_platform::macos_health::read_diskutil_health;
use dasobjectstore_platform::{
    group_enclosures, health::DiskHealthReport, probe::SystemCommandRunner, HostPlatform,
    ObservedDisk, ProbeError, ProbeProvider, ProbeReport, Transport,
};
use dasobjectstore_tui::{UploadTui, UploadTuiContext};
use rand_core::{OsRng, RngCore};
use ratatui::{
    backend::TestBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Wrap},
    Terminal,
};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::fmt::{self, Display};
use std::fs::{self, File};
use std::hash::{Hash, Hasher};
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
#[cfg(unix)]
use std::os::fd::AsRawFd;
#[cfg(target_os = "linux")]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{
    collections::hash_map::DefaultHasher, collections::BTreeMap, collections::BTreeSet,
    collections::VecDeque,
};

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};

#[cfg(unix)]
static UPLOAD_CANCELLED: AtomicBool = AtomicBool::new(false);
#[cfg(test)]
thread_local! {
    static PERFORMANCE_SYNC_ALL_CALLS: RefCell<u32> = const { RefCell::new(0) };
}
const PERFORMANCE_SSD_SETTLE_QUEUE_CAPACITY: usize = 8;

struct PerformanceSsdSettleJob {
    path: PathBuf,
    file: File,
}

struct PerformanceSsdSettler {
    sender: Option<mpsc::SyncSender<PerformanceSsdSettleJob>>,
    handle: Option<thread::JoinHandle<Result<(), CliError>>>,
    completed: Arc<AtomicU32>,
}

impl PerformanceSsdSettler {
    fn start(capacity: usize) -> Self {
        let (sender, receiver) = mpsc::sync_channel::<PerformanceSsdSettleJob>(capacity);
        let completed = Arc::new(AtomicU32::new(0));
        let worker_completed = Arc::clone(&completed);
        let handle = thread::spawn(move || -> Result<(), CliError> {
            loop {
                check_performance_cancelled()?;
                let job = match receiver.recv() {
                    Ok(job) => job,
                    Err(_) => break,
                };
                performance_sync_all(&job.file).map_err(|err| {
                    CliError::CommandFailed(format!(
                        "performance-test SSD settle failed for {}: {err}",
                        job.path.display()
                    ))
                })?;
                worker_completed.fetch_add(1, Ordering::SeqCst);
            }
            Ok(())
        });
        Self {
            sender: Some(sender),
            handle: Some(handle),
            completed,
        }
    }

    fn submit(&self, path: PathBuf, file: File) -> Result<(), CliError> {
        let sender = self.sender.as_ref().ok_or_else(|| {
            CliError::CommandFailed("performance-test SSD settler is closed".to_string())
        })?;
        let mut pending = Some(PerformanceSsdSettleJob { path, file });
        loop {
            check_performance_cancelled()?;
            let job = pending.take().expect("pending SSD settle job");
            match sender.try_send(job) {
                Ok(()) => return Ok(()),
                Err(mpsc::TrySendError::Full(job)) => {
                    pending = Some(job);
                    thread::sleep(Duration::from_millis(50));
                }
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    return Err(CliError::CommandFailed(
                        "performance-test SSD settler stopped early".to_string(),
                    ));
                }
            }
        }
    }

    fn finish(mut self) -> Result<u32, CliError> {
        drop(self.sender.take());
        self.join_worker()?;
        Ok(self.completed.load(Ordering::SeqCst))
    }

    fn join_worker(&mut self) -> Result<(), CliError> {
        if let Some(handle) = self.handle.take() {
            match handle.join() {
                Ok(result) => result,
                Err(_) => Err(CliError::CommandFailed(
                    "performance-test SSD settler panicked".to_string(),
                )),
            }
        } else {
            Ok(())
        }
    }
}

impl Drop for PerformanceSsdSettler {
    fn drop(&mut self) {
        drop(self.sender.take());
        let _ = self.join_worker();
    }
}

pub(crate) fn run(cli: &Cli, writer: &mut impl Write) -> Result<(), CliError> {
    match cli.command() {
        Some(Command::Probe(args)) => run_probe(args, writer),
        Some(Command::Health(args)) => run_health(args, writer),
        Some(Command::Status(args)) => run_status(args, writer),
        Some(Command::Pool(args)) => match args.command() {
            PoolCommand::Inspect(args) => run_pool_inspect(args, writer),
            PoolCommand::Import(args) => run_pool_import(args, writer),
            PoolCommand::Repair(args) => run_pool_repair(args, writer),
            #[cfg(feature = "debug-commands")]
            PoolCommand::MarkClean(args) => run_pool_mark_clean(args, writer),
            #[cfg(feature = "debug-commands")]
            PoolCommand::MarkDirty(args) => run_pool_mark_dirty(args, writer),
        },
        Some(Command::Disk(args)) => match args.command() {
            DiskCommand::Drain(args) => run_disk_drain(args, writer),
            DiskCommand::ForceRetire(args) => run_disk_force_retire(args, writer),
            DiskCommand::LockdownDas(args) => run_disk_lockdown_das(args, writer),
            DiskCommand::PrepareDas(args) => run_disk_prepare_das(args, writer),
            DiskCommand::Replace(args) => run_disk_replace(args, writer),
            DiskCommand::Retire(args) => run_disk_retire(args, writer),
        },
        Some(Command::Store(args)) => match args.command() {
            Some(StoreCommand::Adopt(args)) => run_store_adopt(args, writer),
            Some(StoreCommand::Contents(args)) => run_store_contents(args, writer),
            Some(StoreCommand::Create(args)) => run_store_create(args, writer),
            Some(StoreCommand::Drain(args)) => run_store_drain(args, writer),
            Some(StoreCommand::Delete(args)) => run_store_delete(args, writer),
            Some(StoreCommand::Defaults(args)) => run_store_defaults(args, writer),
            Some(StoreCommand::List(args)) => run_store_list(args, writer),
            Some(StoreCommand::S3Upload(args)) => run_store_s3_upload(args, writer),
            Some(StoreCommand::Validate(args)) => run_store_validate(args, writer),
            None => Cli::write_subcommand_help("store", writer).map_err(CliError::Io),
        },
        Some(Command::Ingest(args)) => match args.command() {
            Some(IngestCommand::Files(args)) => run_ingest_files(args, writer),
            Some(IngestCommand::Status(args)) => run_ingest_status(args, writer),
            Some(IngestCommand::Queue(args)) => run_ingest_queue(args, writer),
            Some(IngestCommand::DrainQueue(args)) => run_ingest_drain_queue(args, writer),
            Some(IngestCommand::DirectImport(args)) => run_ingest_direct_import(args, writer),
            None => Cli::write_subcommand_help("ingest", writer).map_err(CliError::Io),
        },
        Some(Command::Subobject(args)) => run_subobject(args, writer),
        Some(Command::Object(args)) => match args.command() {
            ObjectCommand::Export(args) => run_object_export(args, writer),
            ObjectCommand::Inspect(args) => run_object_inspect(args, writer),
            ObjectCommand::Put(args) => run_object_put(args, writer),
        },
        Some(Command::Service(args)) => match args.command() {
            ServiceCommand::RenderCompose(args) => run_service_render_compose(args, writer),
            ServiceCommand::Up(args) => run_service_up(args, writer),
            ServiceCommand::Down(args) => run_service_down(args, writer),
            ServiceCommand::Status(args) => run_service_status(args, writer),
        },
        Some(Command::Mnemosyne(args)) => match args.command() {
            MnemosyneCommand::Export(args) => run_mnemosyne_export(args, writer),
            MnemosyneCommand::ValidateNasNfsEndpoint(args) => {
                run_mnemosyne_validate_nas_nfs_endpoint(args, writer)
            }
        },
        Some(Command::PerformanceTest(args)) => run_performance_test(args, writer),
        Some(Command::PerformanceReport(args)) => run_performance_report(args, writer),
        None => Cli::write_help(writer).map_err(CliError::Io),
    }
}

fn run_performance_report(
    args: &PerformanceReportArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let artifact = read_performance_json_artifact(args.json_artifact())?;
    let report_path = args
        .report()
        .map(Path::to_path_buf)
        .or_else(|| artifact_pdf_path(&artifact))
        .ok_or_else(|| {
            CliError::CommandFailed(
                "performance-report requires --report when the JSON artifact does not record a PDF path"
                    .to_string(),
            )
        })?;
    validate_pdf_report_path(&report_path)?;
    let markdown_path = args.tmp_dir().join(format!(
        "{}-rebuilt.md",
        report_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("dasobjectstore-performance-report")
    ));
    if let Some(parent) = markdown_path.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_performance_chart_svgs_from_json(&artifact, &report_path)?;
    let markdown = render_performance_report_from_json_artifact(&artifact, &report_path);
    fs::write(&markdown_path, markdown)?;
    write_formal_performance_pdf_report_from_artifact(&markdown_path, &report_path, &artifact)?;
    if !args.keep_markdown() {
        let _ = fs::remove_file(&markdown_path);
    }
    writeln!(writer, "Report: {}", report_path.display())?;
    writeln!(writer, "JSON: {}", args.json_artifact().display())?;
    Ok(())
}

fn run_performance_test(
    args: &PerformanceTestArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    require_admin_for_performance_test()?;
    let mut workload = plan_performance_workload(args)?;
    if args.max_hdd_concurrency() == 0 {
        return Err(CliError::CommandFailed(
            "performance-test requires --max-hdd-concurrency greater than 0".to_string(),
        ));
    }
    if !(1..=3).contains(&args.redundancy()) {
        return Err(CliError::CommandFailed(
            "performance-test --redundancy accepts only 1, 2, or 3".to_string(),
        ));
    }

    let ssd_root = args
        .ssd_root()
        .map(Path::to_path_buf)
        .unwrap_or_else(default_ssd_root);
    validate_known_ssd_root(&ssd_root)?;
    let hdd_root = args
        .hdd_root()
        .map(Path::to_path_buf)
        .unwrap_or_else(default_hdd_root);
    let disks = discover_managed_hdd_roots(&hdd_root)?;
    if disks.is_empty() {
        return Err(CliError::CommandFailed(format!(
            "performance-test found no managed HDD roots under {}",
            hdd_root.display()
        )));
    }
    if args.redundancy() > disks.len() {
        return Err(CliError::CommandFailed(format!(
            "performance-test --redundancy {} requires at least {} managed HDD roots; found {}",
            args.redundancy(),
            args.redundancy(),
            disks.len()
        )));
    }
    let scenario_plan = plan_performance_scenario_matrix(args, disks.len())?;
    if args.authoritative() && scenario_plan.max_concurrency() == 0 {
        return Err(CliError::CommandFailed(
            "performance-test --authoritative requires at least one HDD landing scenario; include ssd-stage-then-drain, ssd-overlap-drain, or direct-hdd".to_string(),
        ));
    }

    let run_id = timestamped_run_id();
    let ssd_bench_root = ssd_root
        .join(".dasobjectstore")
        .join("performance-test")
        .join(&run_id);
    fs::create_dir_all(&ssd_bench_root)?;
    let mut hdd_bench_roots = Vec::new();
    for disk in &disks {
        let root = disk
            .root_path
            .join(".dasobjectstore")
            .join("performance-test")
            .join(&run_id);
        fs::create_dir_all(&root)?;
        hdd_bench_roots.push((disk.disk_id.clone(), root));
    }
    let _temporary_objectstore = PerformanceTemporaryObjectStore::new(
        ssd_bench_root.clone(),
        hdd_bench_roots
            .iter()
            .map(|(_, root)| root.clone())
            .collect(),
        args.keep_temp(),
    );
    let report_path = args
        .report()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| args.tmp_dir().join(format!("{run_id}-report.pdf")));
    validate_pdf_report_path(&report_path)?;
    let qr_path = report_path.with_extension("qr.svg");
    let markdown_source_path = args.tmp_dir().join(format!("{run_id}-report-source.md"));
    let json_path = args
        .json_artifact()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| report_path.with_extension("json"));
    let max_concurrency = scenario_plan.max_concurrency();
    let file_orders = args.file_orders();
    let scenario_total = scenario_plan
        .scenario_total()
        .saturating_mul(file_orders.len().max(1));
    let reproduction_args = performance_test_reproduction_args(
        args,
        &ssd_root,
        &hdd_root,
        args.tmp_dir(),
        &report_path,
    );
    let reproduce_command = shell_join(&reproduction_args);
    #[cfg(unix)]
    let _interrupt_guard = UploadInterruptGuard::install();

    let _generated_source = materialize_generated_performance_workload(
        &mut workload,
        args.tmp_dir(),
        &run_id,
        writer,
        args.tui(),
        &report_path,
        &json_path,
        scenario_total,
    )?;
    let generated_at_utc = now_utc_string();
    let repository_revision = git_revision();
    let reproduction_payload = serde_json::json!({
        "schema": "dasobjectstore.performance_test.reproduction.v1",
        "brand": "Mnemosyne Biosciences",
        "product": "DASObjectStore",
        "run_id": run_id.clone(),
        "generated_at_utc": generated_at_utc.clone(),
        "repository_revision": repository_revision.clone(),
        "cli_version": dasobjectstore_core::VERSION,
        "command": reproduction_args,
        "parameters": {
            "workload_kind": workload.kind.as_str(),
            "source_path": workload.source_path.as_ref().map(|path| path.to_string_lossy().to_string()),
            "file_size": args.file_size(),
            "file_count": args.file_count(),
            "cap": args.cap(),
            "cap_bytes": workload.source_cap_bytes,
            "file_selection": workload.file_selection.as_str(),
            "file_orders": file_orders.iter().map(|order| order.as_str()).collect::<Vec<_>>(),
            "planned_file_count": workload.file_count(),
            "planned_total_bytes": workload.total_bytes(),
            "discovered_file_count": workload.discovered_file_count,
            "discovered_total_bytes": workload.discovered_total_bytes,
            "max_hdd_concurrency": args.max_hdd_concurrency(),
            "selected_scenarios": scenario_plan.scenario_names(),
            "selected_hdd_concurrency": scenario_plan.concurrency_values(),
            "redundancy": args.redundancy(),
            "ssd_root": ssd_root.to_string_lossy(),
            "hdd_root": hdd_root.to_string_lossy(),
            "tmp_dir": args.tmp_dir().to_string_lossy(),
            "keep_temp": args.keep_temp(),
            "authoritative": args.authoritative(),
        },
        "artifacts": {
            "pdf_path": report_path.to_string_lossy(),
            "qr_path": qr_path.to_string_lossy(),
            "json_path": json_path.to_string_lossy(),
        }
    })
    .to_string();
    let reproduction_payload_sha256 = sha256_hex_bytes(reproduction_payload.as_bytes());

    if !args.tui() {
        writeln!(
            writer,
            "performance-test: workload={} files={} total={} disks={} redundancy={} scenarios={} hdd_concurrency={} report={}",
            workload.kind.as_str(),
            workload.file_count(),
            format_bytes(workload.total_bytes() as f64),
            disks.len(),
            args.redundancy(),
            scenario_plan.scenario_names().join(","),
            format_concurrency_list(&scenario_plan.concurrency_values()),
            report_path.display()
        )?;
    }

    let total_started = Instant::now();

    let result = (|| -> Result<PerformanceBenchmarkResults, CliError> {
        let mut scenario_done = 0_usize;
        if args.tui() {
            render_performance_tui_snapshot(
                writer,
                &PerformanceTuiSnapshot {
                    phase: "preparing",
                    scenario: "preparing",
                    activity: "Preparing performance scenarios".to_string(),
                    objective: format!(
                        "selected scenarios: {}; HDD concurrency: {}",
                        scenario_plan.scenario_names().join(", "),
                        format_concurrency_list(&scenario_plan.concurrency_values())
                    ),
                    bounds: performance_selected_matrix_bounds(&workload, &scenario_plan),
                    scenario_done,
                    scenario_total,
                    file_done: 0,
                    current_file: None,
                    file_count: workload.file_count(),
                    processed_bytes: 0,
                    total_bytes: workload.total_bytes(),
                    hdd_concurrency: 0,
                    current_rate: None,
                    ssd_write_rate: None,
                    ssd_read_rate: None,
                    hdd_write_rate: None,
                    hdd_disk_rates: Vec::new(),
                    active_hdd_landing: Vec::new(),
                    aggregate_rate: None,
                    report_path: &report_path,
                    json_path: &json_path,
                },
            )?;
        }
        let mut ssd_only = Vec::new();
        let mut ssd_stage_then_drain = Vec::new();
        let mut ssd_pipeline = Vec::new();
        let mut direct_hdd = Vec::new();
        for &file_order in &file_orders {
            let workload = ordered_performance_workload(&workload, file_order);
            if !args.tui() {
                writeln!(
                    writer,
                    "performance-test: file order {}",
                    file_order.as_str()
                )?;
            }
            if scenario_plan.include_ssd_only {
                if !args.tui() {
                    writeln!(
                    writer,
                    "scenario ssd-only: writing all source payloads to SSD, then reading all payloads back from SSD"
                )?;
                }
                let tui_context = args.tui().then_some(PerformanceTuiContext {
                    scenario_done,
                    scenario_total,
                    report_path: &report_path,
                    json_path: &json_path,
                });
                let scenario = benchmark_ssd_only(
                    &ssd_bench_root,
                    &workload,
                    writer,
                    !args.tui(),
                    tui_context,
                )?;
                scenario_done += 1;
                if args.tui() {
                    render_performance_tui_snapshot(
                        writer,
                        &PerformanceTuiSnapshot {
                            phase: "ssd-only complete",
                            scenario: "ssd-only",
                            activity: "SSD-only scenario complete".to_string(),
                            objective: performance_scenario_objective(
                                PerformanceScenarioKind::SsdOnly,
                                0,
                            ),
                            bounds: performance_scenario_bounds(
                                &workload,
                                PerformanceScenarioKind::SsdOnly,
                                0,
                            ),
                            scenario_done,
                            scenario_total,
                            file_done: workload.file_count(),
                            current_file: None,
                            file_count: workload.file_count(),
                            processed_bytes: scenario.total_bytes,
                            total_bytes: workload.total_bytes(),
                            hdd_concurrency: 0,
                            current_rate: None,
                            ssd_write_rate: measurement_rate(
                                scenario.file_results.iter().map(|row| row.ssd_write),
                            ),
                            ssd_read_rate: measurement_rate(
                                scenario.file_results.iter().map(|row| row.ssd_read),
                            ),
                            hdd_write_rate: None,
                            hdd_disk_rates: Vec::new(),
                            active_hdd_landing: Vec::new(),
                            aggregate_rate: Some(
                                scenario.total_bytes as f64 / scenario.elapsed_seconds.max(0.001),
                            ),
                            report_path: &report_path,
                            json_path: &json_path,
                        },
                    )?;
                }
                ssd_only.push(scenario);
            }

            for &concurrency in &scenario_plan.ssd_stage_then_drain {
                if !args.tui() {
                    writeln!(
                    writer,
                    "scenario ssd-stage-then-drain: stage all selected files to SSD, then drain with {} HDD worker(s)",
                    concurrency
                )?;
                }
                let scenario = benchmark_ssd_stage_then_drain(
                    &ssd_bench_root,
                    &hdd_bench_roots,
                    &workload,
                    concurrency,
                    args.redundancy(),
                    writer,
                    !args.tui(),
                    args.tui().then_some(PerformanceTuiContext {
                        scenario_done,
                        scenario_total,
                        report_path: &report_path,
                        json_path: &json_path,
                    }),
                )?;
                scenario_done += 1;
                if args.tui() {
                    let (hdd_write_rate, _hdd_disk_rates) = performance_hdd_tui_rates(&scenario);
                    render_performance_tui_snapshot(
                    writer,
                    &PerformanceTuiSnapshot {
                        phase: "ssd-stage-then-drain complete",
                        scenario: "ssd-stage-then-drain",
                        activity: format!(
                            "Separated SSD stage then HDD drain complete with {concurrency} worker(s)"
                        ),
                        objective: performance_scenario_objective(
                            PerformanceScenarioKind::SsdStageThenDrain,
                            concurrency,
                        ),
                        bounds: performance_scenario_bounds(
                            &workload,
                            PerformanceScenarioKind::SsdStageThenDrain,
                            concurrency,
                        ),
                        scenario_done,
                        scenario_total,
                        file_done: workload.file_count(),
                        current_file: None,
                        file_count: workload.file_count(),
                        processed_bytes: scenario.total_bytes,
                        total_bytes: workload.total_bytes(),
                        hdd_concurrency: concurrency,
                        current_rate: None,
                        ssd_write_rate: measurement_rate(
                            scenario.file_results.iter().map(|row| row.ssd_write),
                        ),
                        ssd_read_rate: measurement_rate(
                            scenario.file_results.iter().map(|row| row.ssd_read),
                        ),
                        hdd_write_rate,
                        hdd_disk_rates: Vec::new(),
                        active_hdd_landing: Vec::new(),
                        aggregate_rate: Some(
                            scenario.total_bytes as f64 / scenario.elapsed_seconds.max(0.001),
                        ),
                        report_path: &report_path,
                        json_path: &json_path,
                    },
                )?;
                }
                ssd_stage_then_drain.push(scenario);
            }

            for &concurrency in &scenario_plan.ssd_pipeline {
                if !args.tui() {
                    writeln!(
                    writer,
                    "scenario ssd-overlap-drain: SSD ingest with {} overlapping FIFO HDD drain worker(s)",
                    concurrency
                )?;
                }
                let scenario = benchmark_ssd_pipeline(
                    &ssd_bench_root,
                    &hdd_bench_roots,
                    &workload,
                    concurrency,
                    args.redundancy(),
                    writer,
                    !args.tui(),
                    args.tui().then_some(PerformanceTuiContext {
                        scenario_done,
                        scenario_total,
                        report_path: &report_path,
                        json_path: &json_path,
                    }),
                )?;
                scenario_done += 1;
                if args.tui() {
                    let (hdd_write_rate, _hdd_disk_rates) = performance_hdd_tui_rates(&scenario);
                    render_performance_tui_snapshot(
                    writer,
                    &PerformanceTuiSnapshot {
                        phase: "ssd-overlap-drain complete",
                        scenario: "ssd-overlap-drain",
                        activity: format!(
                            "Overlapping SSD ingest and FIFO HDD drain complete with {concurrency} worker(s)"
                        ),
                        objective: performance_scenario_objective(
                            PerformanceScenarioKind::SsdPipeline,
                            concurrency,
                        ),
                        bounds: performance_scenario_bounds(
                            &workload,
                            PerformanceScenarioKind::SsdPipeline,
                            concurrency,
                        ),
                        scenario_done,
                        scenario_total,
                        file_done: workload.file_count(),
                        current_file: None,
                        file_count: workload.file_count(),
                        processed_bytes: scenario.total_bytes,
                        total_bytes: workload.total_bytes(),
                        hdd_concurrency: concurrency,
                        current_rate: None,
                        ssd_write_rate: measurement_rate(
                            scenario.file_results.iter().map(|row| row.ssd_write),
                        ),
                        ssd_read_rate: measurement_rate(
                            scenario.file_results.iter().map(|row| row.ssd_read),
                        ),
                        hdd_write_rate,
                        hdd_disk_rates: Vec::new(),
                        active_hdd_landing: Vec::new(),
                        aggregate_rate: Some(
                            scenario.total_bytes as f64 / scenario.elapsed_seconds.max(0.001),
                        ),
                        report_path: &report_path,
                        json_path: &json_path,
                    },
                )?;
                }
                ssd_pipeline.push(scenario);
            }

            for &concurrency in &scenario_plan.direct_hdd {
                if !args.tui() {
                    writeln!(
                        writer,
                        "scenario direct-hdd: direct source-to-HDD ingest with {} worker(s)",
                        concurrency
                    )?;
                } else {
                    render_performance_tui_snapshot(
                        writer,
                        &PerformanceTuiSnapshot {
                            phase: "direct-hdd active",
                            scenario: "direct-hdd",
                            activity: format!(
                            "Writing source payloads directly to HDD with {concurrency} worker(s)"
                        ),
                            objective: performance_scenario_objective(
                                PerformanceScenarioKind::DirectHdd,
                                concurrency,
                            ),
                            bounds: performance_scenario_bounds(
                                &workload,
                                PerformanceScenarioKind::DirectHdd,
                                concurrency,
                            ),
                            scenario_done,
                            scenario_total,
                            file_done: 0,
                            current_file: None,
                            file_count: workload.file_count(),
                            processed_bytes: 0,
                            total_bytes: workload.total_bytes(),
                            hdd_concurrency: concurrency,
                            current_rate: None,
                            ssd_write_rate: None,
                            ssd_read_rate: None,
                            hdd_write_rate: None,
                            hdd_disk_rates: Vec::new(),
                            active_hdd_landing: Vec::new(),
                            aggregate_rate: None,
                            report_path: &report_path,
                            json_path: &json_path,
                        },
                    )?;
                }
                let scenario = benchmark_direct_hdd(
                    &hdd_bench_roots,
                    &workload,
                    concurrency,
                    args.redundancy(),
                    writer,
                    !args.tui(),
                    if args.tui() {
                        Some(PerformanceTuiContext {
                            scenario_done,
                            scenario_total,
                            report_path: &report_path,
                            json_path: &json_path,
                        })
                    } else {
                        None
                    },
                )?;
                scenario_done += 1;
                if args.tui() {
                    let (hdd_write_rate, _hdd_disk_rates) = performance_hdd_tui_rates(&scenario);
                    render_performance_tui_snapshot(
                        writer,
                        &PerformanceTuiSnapshot {
                            phase: "direct-hdd complete",
                            scenario: "direct-hdd",
                            activity: format!(
                                "Direct-to-HDD scenario complete with {concurrency} worker(s)"
                            ),
                            objective: performance_scenario_objective(
                                PerformanceScenarioKind::DirectHdd,
                                concurrency,
                            ),
                            bounds: performance_scenario_bounds(
                                &workload,
                                PerformanceScenarioKind::DirectHdd,
                                concurrency,
                            ),
                            scenario_done,
                            scenario_total,
                            file_done: workload.file_count(),
                            current_file: None,
                            file_count: workload.file_count(),
                            processed_bytes: scenario.total_bytes,
                            total_bytes: workload.total_bytes(),
                            hdd_concurrency: concurrency,
                            current_rate: None,
                            ssd_write_rate: None,
                            ssd_read_rate: None,
                            hdd_write_rate,
                            hdd_disk_rates: Vec::new(),
                            active_hdd_landing: Vec::new(),
                            aggregate_rate: Some(
                                scenario.total_bytes as f64 / scenario.elapsed_seconds.max(0.001),
                            ),
                            report_path: &report_path,
                            json_path: &json_path,
                        },
                    )?;
                }
                direct_hdd.push(scenario);
            }
        }

        Ok(PerformanceBenchmarkResults {
            ssd_only,
            ssd_stage_then_drain,
            ssd_pipeline,
            direct_hdd,
        })
    })();

    let results = result?;
    let recommendation = recommend_performance_strategy(&results);

    let reproduction_qr_payload =
        format!("mnemosyne-report:DASObjectStore:{run_id}:{reproduction_payload_sha256}");
    let qr_status = write_report_qr_svg(&qr_path, &reproduction_qr_payload)?;
    let performance_report = PerformanceReport {
        run_id,
        generated_at_utc,
        repository_revision,
        file_size: workload.nominal_file_size(),
        file_count: workload.file_count(),
        workload_kind: workload.kind,
        source_path: workload.source_path.clone(),
        source_cap_bytes: workload.source_cap_bytes,
        file_selection: workload.file_selection,
        file_orders: file_orders.clone(),
        discovered_file_count: workload.discovered_file_count,
        discovered_total_bytes: workload.discovered_total_bytes,
        total_source_bytes: workload.total_bytes(),
        ssd_root,
        hdd_root,
        disk_count: disks.len(),
        max_concurrency,
        redundancy: args.redundancy(),
        elapsed_seconds: total_started.elapsed().as_secs_f64(),
        results,
        recommendation,
        authoritative: args.authoritative(),
        authoritative_path: args
            .authoritative()
            .then(|| authoritative_performance_recommendation_path(DEFAULT_DAEMON_STATE_DIR)),
        tmp_dir: args.tmp_dir().to_path_buf(),
        disks: hdd_bench_roots.clone(),
        reproduction_args,
        keep_temp: args.keep_temp(),
        json_path: json_path.clone(),
        qr_path: qr_path.clone(),
        pdf_path: report_path.clone(),
        reproduce_command,
        reproduction_payload,
        reproduction_payload_sha256,
        qr_status,
    };
    write_performance_chart_svgs(&performance_report)?;
    let report = render_performance_report(performance_report.clone());
    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(parent) = markdown_source_path.parent() {
        fs::create_dir_all(parent)?;
    }
    if let Some(parent) = json_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let performance_json = render_performance_json(&performance_report);
    fs::write(&json_path, &performance_json)?;
    if let Some(authoritative_path) = &performance_report.authoritative_path {
        if let Some(parent) = authoritative_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(authoritative_path, &performance_json)?;
    }
    fs::write(&markdown_source_path, &report)?;
    write_pdf_report(&markdown_source_path, &report_path, &performance_report)?;
    let _ = fs::remove_file(&markdown_source_path);
    writeln!(writer, "Report: {}", report_path.display())?;
    writeln!(writer, "JSON: {}", json_path.display())?;
    if let Some(authoritative_path) = &performance_report.authoritative_path {
        writeln!(
            writer,
            "Authoritative performance policy: {}",
            authoritative_path.display()
        )?;
        writeln!(
            writer,
            "Restart dasobjectstored for the authoritative policy to govern new ingest jobs"
        )?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
struct PerformanceMeasurement {
    bytes: u64,
    seconds: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PerformanceWorkloadKind {
    Generated,
    SourceFolder,
}

impl PerformanceWorkloadKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Generated => "generated",
            Self::SourceFolder => "source-folder",
        }
    }
}

#[derive(Clone, Debug)]
struct PerformancePayload {
    file_index: u32,
    relative_path: PathBuf,
    source_path: Option<PathBuf>,
    size_bytes: u64,
    modified_unix_nanos: u128,
}

#[derive(Clone, Debug)]
struct PerformanceWorkload {
    kind: PerformanceWorkloadKind,
    source_path: Option<PathBuf>,
    source_cap_bytes: Option<u64>,
    file_selection: PerformanceFileSelection,
    file_order: PerformanceFileOrder,
    discovered_file_count: u32,
    discovered_total_bytes: u64,
    payloads: Vec<PerformancePayload>,
}

impl PerformanceWorkload {
    fn file_count(&self) -> u32 {
        self.payloads.len() as u32
    }

    fn total_bytes(&self) -> u64 {
        self.payloads
            .iter()
            .map(|payload| payload.size_bytes)
            .sum::<u64>()
    }

    fn nominal_file_size(&self) -> u64 {
        match self.payloads.as_slice() {
            [] => 0,
            [payload] => payload.size_bytes,
            payloads => {
                let total = payloads
                    .iter()
                    .map(|payload| payload.size_bytes)
                    .sum::<u64>();
                total / payloads.len() as u64
            }
        }
    }
}

#[derive(Clone, Debug)]
struct PerformanceFileResult {
    file_index: u32,
    ssd_write: PerformanceMeasurement,
    ssd_read: PerformanceMeasurement,
}

#[derive(Clone, Debug)]
struct PerformanceDiskResult {
    file_index: u32,
    copy_index: usize,
    concurrency: usize,
    scenario: PerformanceScenarioKind,
    disk_id: DiskId,
    ssd_read: PerformanceMeasurement,
    write: PerformanceMeasurement,
}

#[derive(Clone, Debug)]
struct PerformanceIoSample {
    elapsed_second: u64,
    device_label: String,
    device_name: String,
    read_bytes_per_second: u64,
    write_bytes_per_second: u64,
}

#[derive(Clone, Debug)]
struct PerformanceConcurrencyResult {
    concurrency: usize,
    scenario: PerformanceScenarioKind,
    aggregate_bytes: u64,
    seconds: f64,
    slowest_seconds: f64,
    members: Vec<DiskId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PerformanceScenarioKind {
    SsdOnly,
    SsdStageThenDrain,
    SsdPipeline,
    DirectHdd,
}

impl PerformanceScenarioKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::SsdOnly => "ssd-only",
            Self::SsdStageThenDrain => "ssd-stage-then-drain",
            Self::SsdPipeline => "ssd-overlap-drain",
            Self::DirectHdd => "direct-hdd",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::SsdOnly => "SSD-only ingest",
            Self::SsdStageThenDrain => "SSD stage then HDD drain",
            Self::SsdPipeline => "SSD ingest with overlapping HDD drain",
            Self::DirectHdd => "Direct source-to-HDD ingest",
        }
    }
}

#[derive(Clone, Debug)]
struct PerformanceScenarioPlan {
    include_ssd_only: bool,
    ssd_stage_then_drain: Vec<usize>,
    ssd_pipeline: Vec<usize>,
    direct_hdd: Vec<usize>,
}

impl PerformanceScenarioPlan {
    fn scenario_total(&self) -> usize {
        usize::from(self.include_ssd_only)
            + self.ssd_stage_then_drain.len()
            + self.ssd_pipeline.len()
            + self.direct_hdd.len()
    }

    fn max_concurrency(&self) -> usize {
        self.ssd_stage_then_drain
            .iter()
            .chain(self.ssd_pipeline.iter())
            .chain(self.direct_hdd.iter())
            .copied()
            .max()
            .unwrap_or(0)
    }

    fn concurrency_values(&self) -> Vec<usize> {
        self.ssd_stage_then_drain
            .iter()
            .chain(self.ssd_pipeline.iter())
            .chain(self.direct_hdd.iter())
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    fn scenario_names(&self) -> Vec<&'static str> {
        let mut names = Vec::new();
        if self.include_ssd_only {
            names.push(PerformanceScenarioKind::SsdOnly.as_str());
        }
        if !self.ssd_stage_then_drain.is_empty() {
            names.push(PerformanceScenarioKind::SsdStageThenDrain.as_str());
        }
        if !self.ssd_pipeline.is_empty() {
            names.push(PerformanceScenarioKind::SsdPipeline.as_str());
        }
        if !self.direct_hdd.is_empty() {
            names.push(PerformanceScenarioKind::DirectHdd.as_str());
        }
        names
    }
}

fn plan_performance_scenario_matrix(
    args: &PerformanceTestArgs,
    disk_count: usize,
) -> Result<PerformanceScenarioPlan, CliError> {
    if args.max_hdd_concurrency() == 0 {
        return Err(CliError::CommandFailed(
            "performance-test requires --max-hdd-concurrency greater than 0".to_string(),
        ));
    }
    let concurrency = selected_hdd_concurrency(args, disk_count)?;
    let mut include_ssd_only = false;
    let mut ssd_stage_then_drain = Vec::new();
    let mut ssd_pipeline = Vec::new();
    let mut direct_hdd = Vec::new();

    let selections = if args.scenarios().is_empty() {
        vec![
            PerformanceScenarioSelection::SsdOnly,
            PerformanceScenarioSelection::SsdStageThenDrain,
            PerformanceScenarioSelection::SsdOverlapDrain,
            PerformanceScenarioSelection::DirectHdd,
        ]
    } else {
        args.scenarios()
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
    };

    for selection in selections {
        match selection {
            PerformanceScenarioSelection::SsdOnly => include_ssd_only = true,
            PerformanceScenarioSelection::SsdStageThenDrain => {
                ssd_stage_then_drain = concurrency.clone();
            }
            PerformanceScenarioSelection::SsdOverlapDrain => {
                ssd_pipeline = concurrency.clone();
            }
            PerformanceScenarioSelection::DirectHdd => {
                direct_hdd = concurrency.clone();
            }
        }
    }

    let plan = PerformanceScenarioPlan {
        include_ssd_only,
        ssd_stage_then_drain,
        ssd_pipeline,
        direct_hdd,
    };
    if plan.scenario_total() == 0 {
        return Err(CliError::CommandFailed(
            "performance-test selected no benchmark scenarios".to_string(),
        ));
    }
    Ok(plan)
}

fn selected_hdd_concurrency(
    args: &PerformanceTestArgs,
    disk_count: usize,
) -> Result<Vec<usize>, CliError> {
    let selected = if args.hdd_concurrency().is_empty() {
        (1..=args.max_hdd_concurrency()).collect::<Vec<_>>()
    } else {
        args.hdd_concurrency().to_vec()
    };
    let selected = selected.into_iter().collect::<BTreeSet<_>>();
    if selected.contains(&0) {
        return Err(CliError::CommandFailed(
            "performance-test --hdd-concurrency values must be greater than 0".to_string(),
        ));
    }
    if let Some(over_limit) = selected.iter().find(|value| **value > disk_count) {
        return Err(CliError::CommandFailed(format!(
            "performance-test --hdd-concurrency {over_limit} requires at least {over_limit} managed HDD roots; found {disk_count}"
        )));
    }
    Ok(selected.into_iter().collect())
}

fn format_concurrency_list(values: &[usize]) -> String {
    if values.is_empty() {
        return "none".to_string();
    }
    values
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn performance_selected_matrix_bounds(
    workload: &PerformanceWorkload,
    plan: &PerformanceScenarioPlan,
) -> String {
    format!(
        "selected {} file(s), {}; scenarios {}; HDD concurrency {}",
        workload.file_count(),
        format_bytes(workload.total_bytes() as f64),
        plan.scenario_names().join(", "),
        format_concurrency_list(&plan.concurrency_values())
    )
}

fn performance_scenario_objective(kind: PerformanceScenarioKind, concurrency: usize) -> String {
    match kind {
        PerformanceScenarioKind::SsdOnly => {
            "measure separated phases: write every selected file to SSD, then read every selected file back from SSD".to_string()
        }
        PerformanceScenarioKind::SsdStageThenDrain => format!(
            "measure separated phases: stage every selected file to SSD, then drain with {concurrency} HDD worker(s)"
        ),
        PerformanceScenarioKind::SsdPipeline => format!(
            "measure overlapping SSD ingest and FIFO HDD drain with {concurrency} worker(s)"
        ),
        PerformanceScenarioKind::DirectHdd => format!(
            "measure direct source-to-HDD landing with {concurrency} worker(s), bypassing SSD"
        ),
    }
}

fn performance_scenario_bounds(
    workload: &PerformanceWorkload,
    kind: PerformanceScenarioKind,
    concurrency: usize,
) -> String {
    let cap = workload
        .source_cap_bytes
        .map(|bytes| format!(" cap {}", format_bytes(bytes as f64)))
        .unwrap_or_else(|| " no cap".to_string());
    let file_selection = if workload.kind == PerformanceWorkloadKind::SourceFolder
        && workload.source_cap_bytes.is_some()
    {
        format!(" {} selection", workload.file_selection.as_str())
    } else {
        String::new()
    };
    let selection = if workload.kind == PerformanceWorkloadKind::SourceFolder
        && workload.source_cap_bytes.is_some()
    {
        format!(
            "selected {}/{} file(s), {}/{}{};",
            workload.file_count(),
            workload.discovered_file_count,
            format_bytes(workload.total_bytes() as f64),
            format_bytes(workload.discovered_total_bytes as f64),
            file_selection
        )
    } else {
        format!(
            "selected {} file(s), {};",
            workload.file_count(),
            format_bytes(workload.total_bytes() as f64)
        )
    };
    let file_order = format!(" file order {}", workload.file_order.as_str());
    match kind {
        PerformanceScenarioKind::SsdOnly => format!(
            "{selection}{cap};{file_order}; SSD residency grows to the measured safe SSD budget or selected total {}, whichever is smaller, before each readback batch",
            format_bytes(workload.total_bytes() as f64)
        ),
        PerformanceScenarioKind::SsdStageThenDrain => format!(
            "{selection}{cap};{file_order}; SSD residency grows to the measured safe SSD budget or selected total {}, whichever is smaller, before each HDD drain batch",
            format_bytes(workload.total_bytes() as f64)
        ),
        PerformanceScenarioKind::SsdPipeline => format!(
            "{selection}{cap};{file_order}; HDD drain starts as soon as a staged file is queued; SSD backlog is bounded by measured safe SSD capacity while drain at {concurrency} worker(s) catches up"
        ),
        PerformanceScenarioKind::DirectHdd => {
            format!("{selection}{cap};{file_order}; SSD residency is zero for this scenario")
        }
    }
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DiskIoCounters {
    read_sectors: u64,
    write_sectors: u64,
}

struct PerformanceIoSampler {
    stop_sender: Option<mpsc::Sender<()>>,
    handle: Option<thread::JoinHandle<Vec<PerformanceIoSample>>>,
}

impl PerformanceIoSampler {
    fn start(devices: Vec<(String, PathBuf)>) -> Self {
        #[cfg(target_os = "linux")]
        {
            let devices = devices
                .into_iter()
                .filter_map(|(label, path)| {
                    diskstats_device_for_path(&path).map(|device_name| (label, device_name))
                })
                .collect::<Vec<_>>();
            if devices.is_empty() {
                return Self::disabled();
            }
            let (stop_sender, stop_receiver) = mpsc::channel::<()>();
            let handle =
                thread::spawn(move || sample_disk_io_until_stopped(devices, stop_receiver));
            Self {
                stop_sender: Some(stop_sender),
                handle: Some(handle),
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = devices;
            Self::disabled()
        }
    }

    fn disabled() -> Self {
        Self {
            stop_sender: None,
            handle: None,
        }
    }

    fn stop(mut self) -> Vec<PerformanceIoSample> {
        self.stop_and_join()
    }

    fn stop_and_join(&mut self) -> Vec<PerformanceIoSample> {
        if let Some(sender) = self.stop_sender.take() {
            let _ = sender.send(());
        }
        if let Some(handle) = self.handle.take() {
            return handle.join().unwrap_or_default();
        }
        Vec::new()
    }
}

impl Drop for PerformanceIoSampler {
    fn drop(&mut self) {
        let _ = self.stop_and_join();
    }
}

fn performance_io_devices(
    ssd_root: Option<&Path>,
    hdd_roots: &[(DiskId, PathBuf)],
) -> Vec<(String, PathBuf)> {
    let mut devices = Vec::new();
    if let Some(ssd_root) = ssd_root {
        devices.push(("ssd".to_string(), ssd_root.to_path_buf()));
    }
    devices.extend(
        hdd_roots
            .iter()
            .map(|(disk_id, root)| (disk_id.as_str().to_string(), root.clone())),
    );
    devices
}

#[cfg(target_os = "linux")]
fn sample_disk_io_until_stopped(
    devices: Vec<(String, String)>,
    stop_receiver: mpsc::Receiver<()>,
) -> Vec<PerformanceIoSample> {
    let mut samples = Vec::new();
    let mut previous = read_proc_diskstats().unwrap_or_default();
    let started = Instant::now();
    let mut previous_sample_at = started;
    loop {
        match stop_receiver.recv_timeout(Duration::from_secs(1)) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let sampled_at = Instant::now();
        let interval_seconds = sampled_at
            .duration_since(previous_sample_at)
            .as_secs_f64()
            .max(0.001);
        let current = read_proc_diskstats().unwrap_or_default();
        let elapsed_second = started.elapsed().as_secs().max(1);
        for (label, device_name) in &devices {
            let Some(previous_counters) = previous.get(device_name) else {
                continue;
            };
            let Some(current_counters) = current.get(device_name) else {
                continue;
            };
            samples.push(PerformanceIoSample {
                elapsed_second,
                device_label: label.clone(),
                device_name: device_name.clone(),
                read_bytes_per_second: ((current_counters
                    .read_sectors
                    .saturating_sub(previous_counters.read_sectors)
                    .saturating_mul(DISKSTAT_SECTOR_BYTES)
                    as f64)
                    / interval_seconds)
                    .round() as u64,
                write_bytes_per_second: ((current_counters
                    .write_sectors
                    .saturating_sub(previous_counters.write_sectors)
                    .saturating_mul(DISKSTAT_SECTOR_BYTES)
                    as f64)
                    / interval_seconds)
                    .round() as u64,
            });
        }
        previous = current;
        previous_sample_at = sampled_at;
    }
    samples
}

#[cfg(target_os = "linux")]
const DISKSTAT_SECTOR_BYTES: u64 = 512;

#[cfg(target_os = "linux")]
fn read_proc_diskstats() -> io::Result<BTreeMap<String, DiskIoCounters>> {
    fs::read_to_string("/proc/diskstats").map(|contents| parse_proc_diskstats(&contents))
}

#[cfg(target_os = "linux")]
fn parse_proc_diskstats(contents: &str) -> BTreeMap<String, DiskIoCounters> {
    let mut counters = BTreeMap::new();
    for line in contents.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 10 {
            continue;
        }
        let Ok(read_sectors) = fields[5].parse::<u64>() else {
            continue;
        };
        let Ok(write_sectors) = fields[9].parse::<u64>() else {
            continue;
        };
        counters.insert(
            fields[2].to_string(),
            DiskIoCounters {
                read_sectors,
                write_sectors,
            },
        );
    }
    counters
}

#[cfg(target_os = "linux")]
fn diskstats_device_for_path(path: &Path) -> Option<String> {
    let output = ProcessCommand::new("df")
        .arg("-P")
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mount_device = stdout.lines().nth(1)?.split_whitespace().next()?.trim();
    let device_name = Path::new(mount_device)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(mount_device)
        .trim_start_matches("/dev/");
    if device_name.is_empty() {
        None
    } else {
        Some(device_name.to_string())
    }
}

#[derive(Clone, Debug)]
struct PerformanceScenarioResult {
    kind: PerformanceScenarioKind,
    file_order: PerformanceFileOrder,
    concurrency: usize,
    redundancy: usize,
    queue_capacity: usize,
    elapsed_seconds: f64,
    total_bytes: u64,
    logical_source_bytes: u64,
    physical_hdd_write_bytes: u64,
    hdd_write_operations: usize,
    hdd_drain_started_before_all_ssd_staged: bool,
    file_results: Vec<PerformanceFileResult>,
    disk_results: Vec<PerformanceDiskResult>,
    io_samples: Vec<PerformanceIoSample>,
    concurrency_result: PerformanceConcurrencyResult,
}

#[derive(Clone, Debug)]
struct PerformanceBenchmarkResults {
    ssd_only: Vec<PerformanceScenarioResult>,
    ssd_stage_then_drain: Vec<PerformanceScenarioResult>,
    ssd_pipeline: Vec<PerformanceScenarioResult>,
    direct_hdd: Vec<PerformanceScenarioResult>,
}

#[derive(Clone, Debug)]
struct PerformanceRecommendation {
    strategy: PerformanceScenarioKind,
    file_order: PerformanceFileOrder,
    hdd_concurrency: usize,
    aggregate_bytes_per_second: f64,
    reason: String,
}

#[derive(Clone, Debug)]
struct PerformanceReport {
    run_id: String,
    generated_at_utc: String,
    repository_revision: String,
    file_size: u64,
    file_count: u32,
    workload_kind: PerformanceWorkloadKind,
    source_path: Option<PathBuf>,
    source_cap_bytes: Option<u64>,
    file_selection: PerformanceFileSelection,
    file_orders: Vec<PerformanceFileOrder>,
    discovered_file_count: u32,
    discovered_total_bytes: u64,
    total_source_bytes: u64,
    ssd_root: PathBuf,
    hdd_root: PathBuf,
    disk_count: usize,
    max_concurrency: usize,
    redundancy: usize,
    elapsed_seconds: f64,
    results: PerformanceBenchmarkResults,
    recommendation: PerformanceRecommendation,
    authoritative: bool,
    authoritative_path: Option<PathBuf>,
    tmp_dir: PathBuf,
    disks: Vec<(DiskId, PathBuf)>,
    reproduction_args: Vec<String>,
    keep_temp: bool,
    json_path: PathBuf,
    qr_path: PathBuf,
    pdf_path: PathBuf,
    reproduce_command: String,
    reproduction_payload: String,
    reproduction_payload_sha256: String,
    qr_status: String,
}

fn performance_test_reproduction_args(
    args: &PerformanceTestArgs,
    ssd_root: &Path,
    hdd_root: &Path,
    tmp_dir: &Path,
    report_path: &Path,
) -> Vec<String> {
    let mut command = vec!["dasobjectstore".to_string(), "performance-test".to_string()];
    if let Some(source) = args.source() {
        command.push("--source".to_string());
        command.push(source.display().to_string());
        if let Some(cap) = args.cap() {
            command.push("--cap".to_string());
            command.push(cap.to_string());
        }
        command.push("--file_select".to_string());
        command.push(args.file_select().as_str().to_string());
    } else {
        if let Some(file_size) = args.file_size() {
            command.push("--file_size".to_string());
            command.push(file_size.to_string());
        }
        if let Some(file_count) = args.file_count() {
            command.push("--file_count".to_string());
            command.push(file_count.to_string());
        }
    }
    for file_order in args.file_orders() {
        command.push("--file_order".to_string());
        command.push(file_order.as_str().to_string());
    }
    for scenario in args.scenarios() {
        command.push("--scenario".to_string());
        command.push(performance_scenario_selection_name(*scenario).to_string());
    }
    if !args.hdd_concurrency().is_empty() {
        command.push("--hdd-concurrency".to_string());
        command.push(format_concurrency_list(args.hdd_concurrency()));
    }
    command.extend([
        "--max-hdd-concurrency".to_string(),
        args.max_hdd_concurrency().to_string(),
        "--redundancy".to_string(),
        args.redundancy().to_string(),
        "--ssd-root".to_string(),
        ssd_root.display().to_string(),
        "--hdd-root".to_string(),
        hdd_root.display().to_string(),
        "--tmp-dir".to_string(),
        tmp_dir.display().to_string(),
        "--report".to_string(),
        report_path.display().to_string(),
    ]);
    if let Some(json_artifact) = args.json_artifact() {
        command.push("--json-artifact".to_string());
        command.push(json_artifact.display().to_string());
    }
    if args.authoritative() {
        command.push("--authoritative".to_string());
    }
    if args.keep_temp() {
        command.push("--keep-temp".to_string());
    }
    command
}

fn performance_scenario_selection_name(selection: PerformanceScenarioSelection) -> &'static str {
    match selection {
        PerformanceScenarioSelection::SsdOnly => "ssd-only",
        PerformanceScenarioSelection::SsdStageThenDrain => "ssd-stage-then-drain",
        PerformanceScenarioSelection::SsdOverlapDrain => "ssd-overlap-drain",
        PerformanceScenarioSelection::DirectHdd => "direct-hdd",
    }
}

fn validate_pdf_report_path(path: &Path) -> Result<(), CliError> {
    let is_pdf = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"));
    if is_pdf {
        Ok(())
    } else {
        Err(CliError::CommandFailed(format!(
            "performance-test --report must be a PDF path ending in .pdf; got {}",
            path.display()
        )))
    }
}

fn shell_join(args: &[String]) -> String {
    args.iter()
        .map(|argument| shell_quote(argument))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(argument: &str) -> String {
    if argument
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "-_./:=+".contains(character))
    {
        return argument.to_string();
    }
    format!("'{}'", argument.replace('\'', "'\\''"))
}

fn parse_binary_size(value: &str) -> Result<u64, CliError> {
    let trimmed = value.trim();
    let number_end = trimmed
        .find(|character: char| !(character.is_ascii_digit() || character == '.'))
        .unwrap_or(trimmed.len());
    let (number, unit) = trimmed.split_at(number_end);
    if number.is_empty() {
        return Err(CliError::CommandFailed(format!(
            "invalid size '{value}'; expected e.g. 100MiB, 1GiB, 1.1TiB"
        )));
    }
    let number = number.parse::<f64>().map_err(|_| {
        CliError::CommandFailed(format!(
            "invalid size '{value}'; expected e.g. 100MiB, 1GiB, 1.1TiB"
        ))
    })?;
    let multiplier = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1_f64,
        "kib" | "ki" => 1024_f64,
        "mib" | "mi" => 1024_f64.powi(2),
        "gib" | "gi" => 1024_f64.powi(3),
        "tib" | "ti" => 1024_f64.powi(4),
        "kb" | "k" => 1000_f64,
        "mb" | "m" => 1000_f64.powi(2),
        "gb" | "g" => 1000_f64.powi(3),
        "tb" | "t" => 1000_f64.powi(4),
        _ => {
            return Err(CliError::CommandFailed(format!(
                "invalid size unit '{unit}' in '{value}'"
            )));
        }
    };
    let bytes = number * multiplier;
    if !bytes.is_finite() || bytes <= 0.0 || bytes > u64::MAX as f64 {
        return Err(CliError::CommandFailed(format!(
            "invalid size '{value}'; byte count is out of range"
        )));
    }
    Ok(bytes.round() as u64)
}

fn plan_performance_workload(args: &PerformanceTestArgs) -> Result<PerformanceWorkload, CliError> {
    let cap_bytes = args.cap().map(parse_binary_size).transpose()?;
    let primary_file_order = args
        .file_orders()
        .first()
        .copied()
        .unwrap_or(PerformanceFileOrder::SizeDesc);
    match (args.source(), args.file_size(), args.file_count()) {
        (Some(source), None, None) => {
            source_performance_workload(source, cap_bytes, args.file_select(), primary_file_order)
        }
        (None, Some(file_size), Some(file_count)) => {
            if cap_bytes.is_some() {
                return Err(CliError::CommandFailed(
                    "performance-test --cap can only be used with --source".to_string(),
                ));
            }
            if file_count == 0 {
                return Err(CliError::CommandFailed(
                    "performance-test requires --file_count greater than 0".to_string(),
                ));
            }
            let size_bytes = parse_binary_size(file_size)?;
            let payloads = (0..file_count)
                .map(|file_index| PerformancePayload {
                    file_index,
                    relative_path: PathBuf::from(format!("generated-{file_index:05}.bin")),
                    source_path: None,
                    size_bytes,
                    modified_unix_nanos: u128::from(file_index),
                })
                .collect::<Vec<_>>();
            let mut payloads = payloads;
            apply_performance_file_order(&mut payloads, primary_file_order);
            assign_performance_file_indexes(&mut payloads);
            Ok(PerformanceWorkload {
                kind: PerformanceWorkloadKind::Generated,
                source_path: None,
                source_cap_bytes: None,
                file_selection: args.file_select(),
                file_order: primary_file_order,
                discovered_file_count: file_count,
                discovered_total_bytes: size_bytes.saturating_mul(u64::from(file_count)),
                payloads,
            })
        }
        (Some(_), Some(_), _) | (Some(_), _, Some(_)) => Err(CliError::CommandFailed(
            "performance-test accepts either --source or --file_size/--file_count, not both"
                .to_string(),
        )),
        (None, _, _) => Err(CliError::CommandFailed(
            "performance-test requires either --source <DIR> or both --file_size and --file_count"
                .to_string(),
        )),
    }
}

fn source_performance_workload(
    source: &Path,
    cap_bytes: Option<u64>,
    file_selection: PerformanceFileSelection,
    file_order: PerformanceFileOrder,
) -> Result<PerformanceWorkload, CliError> {
    if !source.is_dir() {
        return Err(CliError::CommandFailed(format!(
            "performance-test source {} is not a directory",
            source.display()
        )));
    }
    let mut files = Vec::new();
    collect_performance_source_files(source, source, &mut files)?;
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    let discovered_file_count = files.len();
    let discovered_total_bytes = files.iter().map(|payload| payload.size_bytes).sum::<u64>();
    if files.is_empty() {
        return Err(CliError::CommandFailed(format!(
            "performance-test source {} contains no files",
            source.display()
        )));
    }
    if files.len() > u32::MAX as usize {
        return Err(CliError::CommandFailed(format!(
            "performance-test source {} contains more than {} files",
            source.display(),
            u32::MAX
        )));
    }
    if let Some(cap_bytes) = cap_bytes {
        files = select_performance_source_files(files, cap_bytes, file_selection, source)?;
    }
    apply_performance_file_order(&mut files, file_order);
    assign_performance_file_indexes(&mut files);
    Ok(PerformanceWorkload {
        kind: PerformanceWorkloadKind::SourceFolder,
        source_path: Some(source.to_path_buf()),
        source_cap_bytes: cap_bytes,
        file_selection,
        file_order,
        discovered_file_count: discovered_file_count as u32,
        discovered_total_bytes,
        payloads: files,
    })
}

fn select_performance_source_files(
    mut files: Vec<PerformancePayload>,
    cap_bytes: u64,
    file_selection: PerformanceFileSelection,
    source: &Path,
) -> Result<Vec<PerformancePayload>, CliError> {
    match file_selection {
        PerformanceFileSelection::Random => shuffle_performance_payloads(&mut files),
        PerformanceFileSelection::Smaller => files.sort_by(|left, right| {
            left.size_bytes
                .cmp(&right.size_bytes)
                .then_with(|| left.relative_path.cmp(&right.relative_path))
        }),
        PerformanceFileSelection::Larger => files.sort_by(|left, right| {
            right
                .size_bytes
                .cmp(&left.size_bytes)
                .then_with(|| left.relative_path.cmp(&right.relative_path))
        }),
    }

    let mut selected = Vec::new();
    let mut selected_bytes = 0_u64;
    for payload in files {
        let next_bytes = selected_bytes.saturating_add(payload.size_bytes);
        if next_bytes <= cap_bytes {
            selected_bytes = next_bytes;
            selected.push(payload);
        }
    }
    if selected.is_empty() {
        return Err(CliError::CommandFailed(format!(
            "performance-test --cap {} is smaller than every selectable source file in {}",
            format_bytes(cap_bytes as f64),
            source.display()
        )));
    }
    Ok(selected)
}

fn ordered_performance_workload(
    workload: &PerformanceWorkload,
    file_order: PerformanceFileOrder,
) -> PerformanceWorkload {
    let mut ordered = workload.clone();
    ordered.file_order = file_order;
    apply_performance_file_order(&mut ordered.payloads, file_order);
    assign_performance_file_indexes(&mut ordered.payloads);
    ordered
}

fn apply_performance_file_order(
    files: &mut [PerformancePayload],
    file_order: PerformanceFileOrder,
) {
    match file_order {
        PerformanceFileOrder::Fifo => {
            files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        }
        PerformanceFileOrder::SizeAsc => files.sort_by(|left, right| {
            left.size_bytes
                .cmp(&right.size_bytes)
                .then_with(|| left.relative_path.cmp(&right.relative_path))
        }),
        PerformanceFileOrder::SizeDesc => files.sort_by(|left, right| {
            right
                .size_bytes
                .cmp(&left.size_bytes)
                .then_with(|| left.relative_path.cmp(&right.relative_path))
        }),
        PerformanceFileOrder::TimeAsc => files.sort_by(|left, right| {
            left.modified_unix_nanos
                .cmp(&right.modified_unix_nanos)
                .then_with(|| left.relative_path.cmp(&right.relative_path))
        }),
        PerformanceFileOrder::TimeDesc => files.sort_by(|left, right| {
            right
                .modified_unix_nanos
                .cmp(&left.modified_unix_nanos)
                .then_with(|| left.relative_path.cmp(&right.relative_path))
        }),
    }
}

fn assign_performance_file_indexes(files: &mut [PerformancePayload]) {
    for (index, payload) in files.iter_mut().enumerate() {
        payload.file_index = index as u32;
    }
}

fn shuffle_performance_payloads(files: &mut [PerformancePayload]) {
    let mut rng = OsRng;
    for index in (1..files.len()).rev() {
        let swap_index = (rng.next_u64() % (index as u64 + 1)) as usize;
        files.swap(index, swap_index);
    }
}

fn collect_performance_source_files(
    root: &Path,
    current: &Path,
    files: &mut Vec<PerformancePayload>,
) -> Result<(), CliError> {
    let mut entries = fs::read_dir(current)?.collect::<Result<Vec<_>, io::Error>>()?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            collect_performance_source_files(root, &path, files)?;
        } else if metadata.is_file() {
            let relative_path = path
                .strip_prefix(root)
                .map_err(|err| CliError::CommandFailed(err.to_string()))?
                .to_path_buf();
            files.push(PerformancePayload {
                file_index: 0,
                relative_path,
                source_path: Some(path),
                size_bytes: metadata.len(),
                modified_unix_nanos: metadata_modified_unix_nanos(&metadata),
            });
        }
    }
    Ok(())
}

fn metadata_modified_unix_nanos(metadata: &fs::Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

fn require_admin_for_performance_test() -> Result<(), CliError> {
    if current_user_is_root()? {
        return Ok(());
    }

    Err(CliError::CommandFailed(
        "performance-test requires an administrative user because it performs sustained direct DAS IO; rerun with sudo".to_string(),
    ))
}

#[derive(Debug)]
struct PerformanceTemporaryObjectStore {
    ssd_root: PathBuf,
    hdd_roots: Vec<PathBuf>,
    keep: bool,
}

impl PerformanceTemporaryObjectStore {
    fn new(ssd_root: PathBuf, hdd_roots: Vec<PathBuf>, keep: bool) -> Self {
        Self {
            ssd_root,
            hdd_roots,
            keep,
        }
    }
}

impl Drop for PerformanceTemporaryObjectStore {
    fn drop(&mut self) {
        if self.keep {
            return;
        }
        let _ = fs::remove_dir_all(&self.ssd_root);
        for root in &self.hdd_roots {
            let _ = fs::remove_dir_all(root);
        }
    }
}

#[derive(Debug)]
struct PerformanceGeneratedSource {
    root: PathBuf,
}

impl PerformanceGeneratedSource {
    fn new(root: PathBuf) -> Result<Self, CliError> {
        if root.exists() {
            fs::remove_dir_all(&root)?;
        }
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }
}

impl Drop for PerformanceGeneratedSource {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn materialize_generated_performance_workload(
    workload: &mut PerformanceWorkload,
    tmp_dir: &Path,
    run_id: &str,
    writer: &mut dyn Write,
    tui: bool,
    report_path: &Path,
    json_path: &Path,
    scenario_total: usize,
) -> Result<Option<PerformanceGeneratedSource>, CliError> {
    if workload.kind != PerformanceWorkloadKind::Generated {
        return Ok(None);
    }

    let source = PerformanceGeneratedSource::new(
        tmp_dir.join(format!("dasobjectstore-performance-source-{run_id}")),
    )?;
    let total_bytes = workload.total_bytes();
    if !tui {
        writeln!(
            writer,
            "performance-test: generating {} random source file(s), {} total, under {}",
            workload.file_count(),
            format_bytes(total_bytes as f64),
            source.root.display()
        )?;
    }

    let mut completed_bytes = 0_u64;
    let payload_count = workload.file_count();
    for payload in &mut workload.payloads {
        check_performance_cancelled()?;
        let file_index = payload.file_index;
        let destination = source.root.join(&payload.relative_path);
        let mut progress = |written: u64, seconds: f64| -> Result<(), CliError> {
            if tui {
                render_performance_tui_snapshot(
                    writer,
                    &PerformanceTuiSnapshot {
                        phase: "generating source",
                        scenario: "source-prep",
                        activity: format!(
                            "Generating source file {}/{}",
                            file_index + 1,
                            payload_count
                        ),
                        objective: "create all generated random source files before benchmark upload begins".to_string(),
                        bounds: format!(
                            "generated workload; {} file(s), {} total; source files are removed after completion or cancellation",
                            payload_count,
                            format_bytes(total_bytes as f64)
                        ),
                        scenario_done: 0,
                        scenario_total,
                        file_done: file_index,
                        current_file: Some(file_index + 1),
                        file_count: payload_count,
                        processed_bytes: completed_bytes.saturating_add(written),
                        total_bytes,
                        hdd_concurrency: 0,
                        current_rate: Some(written as f64 / seconds.max(0.001)),
                        ssd_write_rate: None,
                        ssd_read_rate: None,
                        hdd_write_rate: None,
                        hdd_disk_rates: Vec::new(),
                        active_hdd_landing: Vec::new(),
                        aggregate_rate: None,
                        report_path,
                        json_path,
                    },
                )?;
            }
            Ok(())
        };
        measure_generate_random_file_with_progress(
            &destination,
            payload.size_bytes,
            file_index,
            Some(&mut progress),
            PerformanceCopySyncPolicy::SyncAll,
        )?;
        payload.source_path = Some(destination);
        completed_bytes = completed_bytes.saturating_add(payload.size_bytes);
    }

    if tui {
        render_performance_tui_snapshot(
            writer,
            &PerformanceTuiSnapshot {
                phase: "source generation complete",
                scenario: "source-prep",
                activity: "Generated source workload is ready for benchmark upload".to_string(),
                objective: "create all generated random source files before benchmark upload begins"
                    .to_string(),
                bounds: format!(
                    "generated workload; {} file(s), {} total; source files are removed after completion or cancellation",
                    payload_count,
                    format_bytes(total_bytes as f64)
                ),
                scenario_done: 0,
                scenario_total,
                file_done: payload_count,
                current_file: None,
                file_count: payload_count,
                processed_bytes: completed_bytes,
                total_bytes,
                hdd_concurrency: 0,
                current_rate: None,
                ssd_write_rate: None,
                ssd_read_rate: None,
                hdd_write_rate: None,
                hdd_disk_rates: Vec::new(),
                active_hdd_landing: Vec::new(),
                aggregate_rate: None,
                report_path,
                json_path,
            },
        )?;
    }

    Ok(Some(source))
}

#[cfg(unix)]
fn check_performance_cancelled() -> Result<(), CliError> {
    if UPLOAD_CANCELLED.load(Ordering::SeqCst) {
        Err(CliError::CommandFailed(
            "performance-test cancelled by Ctrl-C; temporary objectstore cleanup requested"
                .to_string(),
        ))
    } else {
        Ok(())
    }
}

#[cfg(not(unix))]
fn check_performance_cancelled() -> Result<(), CliError> {
    Ok(())
}

struct PerformanceTuiSnapshot<'a> {
    phase: &'a str,
    scenario: &'a str,
    activity: String,
    objective: String,
    bounds: String,
    scenario_done: usize,
    scenario_total: usize,
    file_done: u32,
    current_file: Option<u32>,
    file_count: u32,
    processed_bytes: u64,
    total_bytes: u64,
    hdd_concurrency: usize,
    current_rate: Option<f64>,
    ssd_write_rate: Option<f64>,
    ssd_read_rate: Option<f64>,
    hdd_write_rate: Option<f64>,
    hdd_disk_rates: Vec<String>,
    active_hdd_landing: Vec<String>,
    aggregate_rate: Option<f64>,
    report_path: &'a Path,
    json_path: &'a Path,
}

#[derive(Clone, Copy)]
struct PerformanceTuiContext<'a> {
    scenario_done: usize,
    scenario_total: usize,
    report_path: &'a Path,
    json_path: &'a Path,
}

fn render_performance_tui_snapshot(
    writer: &mut (impl Write + ?Sized),
    snapshot: &PerformanceTuiSnapshot<'_>,
) -> Result<(), CliError> {
    let visible_active_landing_rows = snapshot.active_hdd_landing.len().min(8);
    let hidden_active_landing_rows = snapshot
        .active_hdd_landing
        .len()
        .saturating_sub(visible_active_landing_rows);
    let landing_height = if snapshot.active_hdd_landing.is_empty() {
        5
    } else {
        (3 + visible_active_landing_rows + usize::from(hidden_active_landing_rows > 0)).clamp(6, 12)
            as u16
    };
    let mut area = performance_tui_area();
    area.height = area.height.max(landing_height.saturating_add(31));
    let backend = TestBackend::new(area.width, area.height);
    let mut terminal =
        Terminal::new(backend).expect("test backend terminal creation is infallible");
    let current_fraction = if snapshot.file_count == 0 {
        0.0
    } else {
        f64::from(snapshot.file_done.min(snapshot.file_count)) / f64::from(snapshot.file_count)
    };
    let percent = if snapshot.scenario_total == 0 {
        0
    } else {
        ((((snapshot.scenario_done as f64 + current_fraction) / snapshot.scenario_total as f64)
            * 100.0)
            .round()
            .clamp(0.0, 100.0)) as u16
    };
    terminal.draw(|frame| {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(6),
                Constraint::Length(3),
                Constraint::Length(7),
                Constraint::Length(landing_height),
                Constraint::Length(6),
                Constraint::Length(5),
                Constraint::Min(4),
            ])
            .split(frame.area());
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(vec![Span::styled(
                    "DASObjectStore Performance Test",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )]),
                Line::from(format!("Phase: {}", snapshot.phase)),
                Line::from(format!("Scenario: {}", snapshot.scenario)),
                Line::from(format!("Now: {}", snapshot.activity)),
            ])
            .block(Block::default().borders(Borders::ALL).title("Context")),
            chunks[0],
        );
        frame.render_widget(
            Gauge::default()
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Scenario Progress"),
                )
                .gauge_style(Style::default().fg(Color::Green))
                .percent(percent),
            chunks[1],
        );
        let rate = snapshot
            .aggregate_rate
            .map(|rate| format!("{}/s", format_bytes(rate)))
            .unwrap_or_else(|| "pending".to_string());
        let current_rate = snapshot
            .current_rate
            .map(|rate| format!("{}/s", format_bytes(rate)))
            .unwrap_or_else(|| "pending".to_string());
        let inferred_ssd_write_rate = snapshot.ssd_write_rate.or_else(|| {
            if snapshot.phase.contains("staging")
                || snapshot.activity.starts_with("Writing")
                || snapshot.activity.starts_with("Queued")
            {
                snapshot.current_rate
            } else {
                None
            }
        });
        let inferred_ssd_read_rate = snapshot.ssd_read_rate.or_else(|| {
            if snapshot.phase.contains("readback") || snapshot.activity.starts_with("Reading") {
                snapshot.current_rate
            } else {
                None
            }
        });
        let ssd_write_rate = inferred_ssd_write_rate
            .map(|rate| format!("{}/s", format_bytes(rate)))
            .unwrap_or_else(|| "pending".to_string());
        let ssd_read_rate = inferred_ssd_read_rate
            .map(|rate| format!("{}/s", format_bytes(rate)))
            .unwrap_or_else(|| "pending".to_string());
        let hdd_write_rate = snapshot
            .hdd_write_rate
            .map(|rate| format!("{}/s", format_bytes(rate)))
            .unwrap_or_else(|| "pending".to_string());
        let hdd_disk_rates = if snapshot.hdd_disk_rates.is_empty() {
            "pending".to_string()
        } else {
            snapshot.hdd_disk_rates.join("; ")
        };
        let active_landing_lines = if snapshot.active_hdd_landing.is_empty() {
            vec![Line::from("Active landing: idle")]
        } else {
            std::iter::once(Line::from("Active landing:"))
                .chain(
                    snapshot
                        .active_hdd_landing
                        .iter()
                        .take(visible_active_landing_rows)
                        .map(|line| Line::from(format!("  {line}"))),
                )
                .chain((hidden_active_landing_rows > 0).then(|| {
                    Line::from(format!(
                        "  ... {hidden_active_landing_rows} more active transfer(s)"
                    ))
                }))
                .collect::<Vec<_>>()
        };
        let current_file = snapshot
            .current_file
            .map(|file| file.to_string())
            .unwrap_or_else(|| "-".to_string());
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(format!(
                    "Scenarios: {}/{}",
                    snapshot.scenario_done, snapshot.scenario_total
                )),
                Line::from(format!(
                    "Current scenario files: {}/{} (active {})",
                    snapshot.file_done, snapshot.file_count, current_file
                )),
                Line::from(format!(
                    "Current scenario bytes: {}/{}",
                    format_bytes(snapshot.processed_bytes as f64),
                    format_bytes(snapshot.total_bytes as f64)
                )),
                Line::from(format!("HDD concurrency: {}", snapshot.hdd_concurrency)),
                Line::from(format!("Current operation rate: {current_rate}")),
            ])
            .block(Block::default().borders(Borders::ALL).title("Workload")),
            chunks[2],
        );
        frame.render_widget(
            Paragraph::new(active_landing_lines)
                .wrap(Wrap { trim: true })
                .block(Block::default().borders(Borders::ALL).title("HDD Landing")),
            chunks[3],
        );
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(format!("SSD write rate: {ssd_write_rate}")),
                Line::from(format!("SSD read rate: {ssd_read_rate}")),
                Line::from(format!("HDD aggregate average: {hdd_write_rate}")),
                Line::from(format!("HDD active disk writes: {hdd_disk_rates}")),
                Line::from(format!("Scenario aggregate rate: {rate}")),
            ])
            .wrap(Wrap { trim: true })
            .block(Block::default().borders(Borders::ALL).title("Rates")),
            chunks[4],
        );
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(format!("Objective: {}", snapshot.objective)),
                Line::from(format!("Bounds: {}", snapshot.bounds)),
                Line::from("Ctrl-C requests cancellation and temporary objectstore cleanup."),
                Line::from("SSD pipeline scenarios stage to SSD while HDD drain workers consume the FIFO queue."),
            ])
            .wrap(Wrap { trim: true })
            .block(Block::default().borders(Borders::ALL).title("Scenario Details")),
            chunks[5],
        );
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(format!("PDF: {}", snapshot.report_path.display())),
                Line::from(format!("JSON: {}", snapshot.json_path.display())),
            ])
            .block(Block::default().borders(Borders::ALL).title("Artifacts")),
            chunks[6],
        );
    })
    .expect("test backend drawing is infallible");
    write!(writer, "\x1b[2J\x1b[H")?;
    let buffer = terminal.backend().buffer();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            write!(writer, "{}", buffer[(x, y)].symbol())?;
        }
        if y + 1 < buffer.area.height {
            write!(writer, "\r\n")?;
        }
    }
    writer.flush()?;
    Ok(())
}

struct HddDrainTuiState<'a> {
    context: PerformanceTuiContext<'a>,
    workload: &'a PerformanceWorkload,
    kind: PerformanceScenarioKind,
    concurrency: usize,
    submitted_jobs: usize,
    total_jobs: usize,
    started_jobs: usize,
    completed_jobs: usize,
    transferred_bytes: u64,
    ssd_read_rate: Option<f64>,
    hdd_write_rate: Option<f64>,
    hdd_disk_rates: Vec<String>,
    active_hdd_landing: Vec<String>,
}

fn render_hdd_drain_tui_snapshot(
    writer: &mut impl Write,
    state: HddDrainTuiState<'_>,
) -> Result<(), CliError> {
    let active_jobs = state.started_jobs.saturating_sub(state.completed_jobs);
    let queued_jobs = state.submitted_jobs.saturating_sub(state.started_jobs);
    let pending_submission = state.total_jobs.saturating_sub(state.submitted_jobs);
    let total_bytes = state.workload.total_bytes().saturating_mul(
        state
            .total_jobs
            .checked_div(state.workload.file_count().max(1) as usize)
            .unwrap_or(1)
            .max(1) as u64,
    );
    render_performance_tui_snapshot(
        writer,
        &PerformanceTuiSnapshot {
            phase: "hdd-drain active",
            scenario: state.kind.as_str(),
            activity: format!(
                "HDD drain copy jobs: drained {}/{}, draining {}, queued {}, pending submission {}",
                state.completed_jobs,
                state.total_jobs,
                active_jobs,
                queued_jobs,
                pending_submission
            ),
            objective: performance_scenario_objective(state.kind, state.concurrency),
            bounds: performance_scenario_bounds(state.workload, state.kind, state.concurrency),
            scenario_done: state.context.scenario_done,
            scenario_total: state.context.scenario_total,
            file_done: state.completed_jobs.min(u32::MAX as usize) as u32,
            current_file: None,
            file_count: state.total_jobs.min(u32::MAX as usize) as u32,
            processed_bytes: state.transferred_bytes,
            total_bytes,
            hdd_concurrency: state.concurrency,
            current_rate: state.hdd_write_rate,
            ssd_write_rate: None,
            ssd_read_rate: state.ssd_read_rate,
            hdd_write_rate: state.hdd_write_rate,
            hdd_disk_rates: state.hdd_disk_rates,
            active_hdd_landing: state.active_hdd_landing,
            aggregate_rate: state.hdd_write_rate,
            report_path: state.context.report_path,
            json_path: state.context.json_path,
        },
    )
}

fn performance_tui_area() -> Rect {
    let env_size = std::env::var("COLUMNS")
        .ok()
        .and_then(|columns| columns.parse::<u16>().ok())
        .zip(
            std::env::var("LINES")
                .ok()
                .and_then(|lines| lines.parse::<u16>().ok()),
        );
    let (width, height) = env_size
        .or_else(performance_terminal_size_from_ioctl)
        .unwrap_or((110, 24));
    Rect::new(0, 0, width.max(80), height.max(32))
}

#[cfg(unix)]
fn performance_terminal_size_from_ioctl() -> Option<(u16, u16)> {
    let stdout = std::io::stdout();
    let mut winsize = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let result = unsafe { libc::ioctl(stdout.as_raw_fd(), libc::TIOCGWINSZ, &mut winsize) };
    if result == 0 && winsize.ws_col > 0 && winsize.ws_row > 0 {
        Some((winsize.ws_col, winsize.ws_row))
    } else {
        None
    }
}

#[cfg(not(unix))]
fn performance_terminal_size_from_ioctl() -> Option<(u16, u16)> {
    None
}

fn benchmark_ssd_only(
    ssd_bench_root: &Path,
    workload: &PerformanceWorkload,
    writer: &mut impl Write,
    log_progress: bool,
    tui_context: Option<PerformanceTuiContext<'_>>,
) -> Result<PerformanceScenarioResult, CliError> {
    let started = Instant::now();
    let io_sampler = PerformanceIoSampler::start(performance_io_devices(Some(ssd_bench_root), &[]));
    let ssd_settler = PerformanceSsdSettler::start(PERFORMANCE_SSD_SETTLE_QUEUE_CAPACITY);
    let scenario_root = ssd_bench_root.join("ssd-only");
    let residency_budget = performance_ssd_residency_budget(&scenario_root)?;
    let batches = plan_ssd_residency_batches(workload, residency_budget)?;
    let batch_count = batches.len();
    let mut file_results = Vec::<PerformanceFileResult>::new();
    let mut ssd_write_measurements = Vec::<PerformanceMeasurement>::new();
    let mut written_bytes = 0_u64;
    let mut read_bytes = 0_u64;

    for (batch_index, batch) in batches.into_iter().enumerate() {
        let batch_bytes = batch.iter().map(|payload| payload.size_bytes).sum::<u64>();
        let mut staged_payloads =
            Vec::<(PerformancePayload, PathBuf, PerformanceMeasurement)>::new();
        for payload in batch {
            check_performance_cancelled()?;
            let destination = scenario_root.join(&payload.relative_path);
            if let Some(context) = tui_context {
                render_performance_tui_snapshot(
                    writer,
                    &PerformanceTuiSnapshot {
                        phase: "ssd-only write phase",
                        scenario: "ssd-only",
                        activity: format!(
                            "Writing batch {}/{} file {}/{} to SSD: {}",
                            batch_index + 1,
                            batch_count,
                            payload.file_index + 1,
                            workload.file_count(),
                            payload.relative_path.display()
                        ),
                        objective: performance_scenario_objective(
                            PerformanceScenarioKind::SsdOnly,
                            0,
                        ),
                        bounds: performance_scenario_bounds(
                            workload,
                            PerformanceScenarioKind::SsdOnly,
                            0,
                        ),
                        scenario_done: context.scenario_done,
                        scenario_total: context.scenario_total,
                        file_done: payload.file_index,
                        current_file: Some(payload.file_index + 1),
                        file_count: workload.file_count(),
                        processed_bytes: written_bytes,
                        total_bytes: workload.total_bytes(),
                        hdd_concurrency: 0,
                        current_rate: None,
                        ssd_write_rate: measurement_rate(ssd_write_measurements.iter().copied()),
                        ssd_read_rate: None,
                        hdd_write_rate: None,
                        hdd_disk_rates: Vec::new(),
                        active_hdd_landing: Vec::new(),
                        aggregate_rate: None,
                        report_path: context.report_path,
                        json_path: context.json_path,
                    },
                )?;
            }
            let ssd_write = if let Some(context) = tui_context {
                let mut progress = |bytes: u64, seconds: f64| -> Result<(), CliError> {
                    render_performance_tui_snapshot(
                        writer,
                        &PerformanceTuiSnapshot {
                            phase: "ssd-only write phase",
                            scenario: "ssd-only",
                            activity: format!(
                                "Writing batch {}/{} file {}/{} to SSD: {} ({}/{})",
                                batch_index + 1,
                                batch_count,
                                payload.file_index + 1,
                                workload.file_count(),
                                payload.relative_path.display(),
                                format_bytes(bytes as f64),
                                format_bytes(payload.size_bytes as f64)
                            ),
                            objective: performance_scenario_objective(
                                PerformanceScenarioKind::SsdOnly,
                                0,
                            ),
                            bounds: performance_scenario_bounds(
                                workload,
                                PerformanceScenarioKind::SsdOnly,
                                0,
                            ),
                            scenario_done: context.scenario_done,
                            scenario_total: context.scenario_total,
                            file_done: payload.file_index,
                            current_file: Some(payload.file_index + 1),
                            file_count: workload.file_count(),
                            processed_bytes: written_bytes.saturating_add(bytes),
                            total_bytes: workload.total_bytes(),
                            hdd_concurrency: 0,
                            current_rate: Some(bytes as f64 / seconds.max(0.001)),
                            ssd_write_rate: measurement_rate_with_current(
                                ssd_write_measurements.iter().copied(),
                                bytes,
                                seconds,
                            ),
                            ssd_read_rate: None,
                            hdd_write_rate: None,
                            hdd_disk_rates: Vec::new(),
                            active_hdd_landing: Vec::new(),
                            aggregate_rate: None,
                            report_path: context.report_path,
                            json_path: context.json_path,
                        },
                    )
                };
                measure_ssd_stage_payload_with_progress(
                    &payload,
                    &destination,
                    payload.file_index,
                    Some(&mut progress),
                    &ssd_settler,
                )?
            } else {
                measure_ssd_stage_payload(&payload, &destination, &ssd_settler)?
            };
            written_bytes = written_bytes.saturating_add(ssd_write.bytes);
            ssd_write_measurements.push(ssd_write);
            if log_progress {
                writeln!(
                    writer,
                    "ssd-only write batch {}/{} file {}/{}: SSD write {}/s",
                    batch_index + 1,
                    batch_count,
                    payload.file_index + 1,
                    workload.file_count(),
                    format_bytes(throughput(ssd_write))
                )?;
            }
            staged_payloads.push((payload, destination, ssd_write));
        }

        if let Some(context) = tui_context {
            render_performance_tui_snapshot(
                writer,
                &PerformanceTuiSnapshot {
                    phase: "ssd-only readback phase",
                    scenario: "ssd-only",
                    activity: format!(
                        "Batch {}/{} staged {}; reading it back from SSD before the next batch",
                        batch_index + 1,
                        batch_count,
                        format_bytes(batch_bytes as f64)
                    ),
                    objective: performance_scenario_objective(PerformanceScenarioKind::SsdOnly, 0),
                    bounds: performance_scenario_bounds(
                        workload,
                        PerformanceScenarioKind::SsdOnly,
                        0,
                    ),
                    scenario_done: context.scenario_done,
                    scenario_total: context.scenario_total,
                    file_done: file_results.len() as u32,
                    current_file: None,
                    file_count: workload.file_count(),
                    processed_bytes: read_bytes,
                    total_bytes: workload.total_bytes(),
                    hdd_concurrency: 0,
                    current_rate: None,
                    ssd_write_rate: measurement_rate(ssd_write_measurements.iter().copied()),
                    ssd_read_rate: measurement_rate(file_results.iter().map(|row| row.ssd_read)),
                    hdd_write_rate: None,
                    hdd_disk_rates: Vec::new(),
                    active_hdd_landing: Vec::new(),
                    aggregate_rate: None,
                    report_path: context.report_path,
                    json_path: context.json_path,
                },
            )?;
        }
        for (payload, destination, ssd_write) in staged_payloads {
            check_performance_cancelled()?;
            let completed_reads = file_results.len() as u32;
            let ssd_read = if let Some(context) = tui_context {
                let mut progress = |bytes: u64, seconds: f64| -> Result<(), CliError> {
                    render_performance_tui_snapshot(
                        writer,
                        &PerformanceTuiSnapshot {
                            phase: "ssd-only readback phase",
                            scenario: "ssd-only",
                            activity: format!(
                                "Reading batch {}/{} file {}/{} back from SSD: {} ({})",
                                batch_index + 1,
                                batch_count,
                                payload.file_index + 1,
                                workload.file_count(),
                                payload.relative_path.display(),
                                format_bytes(bytes as f64)
                            ),
                            objective: performance_scenario_objective(
                                PerformanceScenarioKind::SsdOnly,
                                0,
                            ),
                            bounds: performance_scenario_bounds(
                                workload,
                                PerformanceScenarioKind::SsdOnly,
                                0,
                            ),
                            scenario_done: context.scenario_done,
                            scenario_total: context.scenario_total,
                            file_done: completed_reads,
                            current_file: Some(payload.file_index + 1),
                            file_count: workload.file_count(),
                            processed_bytes: read_bytes.saturating_add(bytes),
                            total_bytes: workload.total_bytes(),
                            hdd_concurrency: 0,
                            current_rate: Some(bytes as f64 / seconds.max(0.001)),
                            ssd_write_rate: measurement_rate(
                                ssd_write_measurements.iter().copied(),
                            ),
                            ssd_read_rate: measurement_rate_with_current(
                                file_results.iter().map(|row| row.ssd_read),
                                bytes,
                                seconds,
                            ),
                            hdd_write_rate: None,
                            hdd_disk_rates: Vec::new(),
                            active_hdd_landing: Vec::new(),
                            aggregate_rate: None,
                            report_path: context.report_path,
                            json_path: context.json_path,
                        },
                    )
                };
                measure_read_with_progress(&destination, Some(&mut progress))?
            } else {
                measure_read(&destination)?
            };
            let _ = fs::remove_file(&destination);
            read_bytes = read_bytes.saturating_add(ssd_read.bytes);
            file_results.push(PerformanceFileResult {
                file_index: payload.file_index,
                ssd_write,
                ssd_read,
            });
            if log_progress {
                writeln!(
                    writer,
                    "ssd-only read batch {}/{} file {}/{}: SSD read {}/s",
                    batch_index + 1,
                    batch_count,
                    payload.file_index + 1,
                    workload.file_count(),
                    format_bytes(throughput(ssd_read))
                )?;
            }
        }
    }
    ssd_settler.finish()?;
    let io_samples = io_sampler.stop();
    let elapsed_seconds = started.elapsed().as_secs_f64().max(0.001);
    let total_bytes = written_bytes;
    Ok(PerformanceScenarioResult {
        kind: PerformanceScenarioKind::SsdOnly,
        file_order: workload.file_order,
        concurrency: 0,
        redundancy: 1,
        queue_capacity: 0,
        elapsed_seconds,
        total_bytes,
        logical_source_bytes: total_bytes,
        physical_hdd_write_bytes: 0,
        hdd_write_operations: 0,
        hdd_drain_started_before_all_ssd_staged: false,
        file_results,
        disk_results: Vec::new(),
        io_samples,
        concurrency_result: PerformanceConcurrencyResult {
            concurrency: 0,
            scenario: PerformanceScenarioKind::SsdOnly,
            aggregate_bytes: total_bytes,
            seconds: elapsed_seconds,
            slowest_seconds: 0.0,
            members: Vec::new(),
        },
    })
}

fn benchmark_ssd_stage_then_drain(
    ssd_bench_root: &Path,
    hdd_bench_roots: &[(DiskId, PathBuf)],
    workload: &PerformanceWorkload,
    concurrency: usize,
    redundancy: usize,
    writer: &mut impl Write,
    log_progress: bool,
    tui_context: Option<PerformanceTuiContext<'_>>,
) -> Result<PerformanceScenarioResult, CliError> {
    let started = Instant::now();
    let io_sampler = PerformanceIoSampler::start(performance_io_devices(
        Some(ssd_bench_root),
        hdd_bench_roots,
    ));
    let ssd_settler = PerformanceSsdSettler::start(PERFORMANCE_SSD_SETTLE_QUEUE_CAPACITY);
    let scenario_root = ssd_bench_root
        .join("ssd-stage-then-drain")
        .join(format!("c{concurrency}"));
    let residency_budget = performance_ssd_residency_budget(&scenario_root)?;
    let batches = plan_ssd_residency_batches(workload, residency_budget)?;
    let batch_count = batches.len();
    let queue_capacity = hdd_queue_capacity(concurrency, redundancy);
    let scheduler = new_shared_disk_placement_scheduler(hdd_bench_roots)?;
    let mut file_results = Vec::<PerformanceFileResult>::new();
    let mut disk_results = Vec::<PerformanceDiskResult>::new();
    let mut total_bytes = 0_u64;

    for (batch_index, batch) in batches.into_iter().enumerate() {
        let mut staged_jobs = Vec::new();
        let batch_bytes = batch.iter().map(|payload| payload.size_bytes).sum::<u64>();
        for payload in batch {
            check_performance_cancelled()?;
            let ssd_path = scenario_root.join(&payload.relative_path);
            if let Some(context) = tui_context {
                render_performance_tui_snapshot(
                    writer,
                    &PerformanceTuiSnapshot {
                        phase: "ssd-stage-then-drain staging",
                        scenario: "ssd-stage-then-drain",
                        activity: format!(
                            "Staging batch {}/{} file {}/{} to SSD before batch drain: {}",
                            batch_index + 1,
                            batch_count,
                            payload.file_index + 1,
                            workload.file_count(),
                            payload.relative_path.display()
                        ),
                        objective: performance_scenario_objective(
                            PerformanceScenarioKind::SsdStageThenDrain,
                            concurrency,
                        ),
                        bounds: performance_scenario_bounds(
                            workload,
                            PerformanceScenarioKind::SsdStageThenDrain,
                            concurrency,
                        ),
                        scenario_done: context.scenario_done,
                        scenario_total: context.scenario_total,
                        file_done: payload.file_index,
                        current_file: Some(payload.file_index + 1),
                        file_count: workload.file_count(),
                        processed_bytes: total_bytes,
                        total_bytes: workload.total_bytes(),
                        hdd_concurrency: 0,
                        current_rate: None,
                        ssd_write_rate: measurement_rate(
                            file_results.iter().map(|row| row.ssd_write),
                        ),
                        ssd_read_rate: measurement_rate(
                            file_results.iter().map(|row| row.ssd_read),
                        ),
                        hdd_write_rate: None,
                        hdd_disk_rates: Vec::new(),
                        active_hdd_landing: Vec::new(),
                        aggregate_rate: None,
                        report_path: context.report_path,
                        json_path: context.json_path,
                    },
                )?;
            }
            let ssd_write = match if let Some(context) = tui_context {
                let mut progress = |bytes: u64, seconds: f64| -> Result<(), CliError> {
                    render_performance_tui_snapshot(
                        writer,
                        &PerformanceTuiSnapshot {
                            phase: "ssd-stage-then-drain staging",
                            scenario: "ssd-stage-then-drain",
                            activity: format!(
                                "Staging batch {}/{} file {}/{} to SSD before drain: {} ({}/{})",
                                batch_index + 1,
                                batch_count,
                                payload.file_index + 1,
                                workload.file_count(),
                                payload.relative_path.display(),
                                format_bytes(bytes as f64),
                                format_bytes(payload.size_bytes as f64)
                            ),
                            objective: performance_scenario_objective(
                                PerformanceScenarioKind::SsdStageThenDrain,
                                concurrency,
                            ),
                            bounds: performance_scenario_bounds(
                                workload,
                                PerformanceScenarioKind::SsdStageThenDrain,
                                concurrency,
                            ),
                            scenario_done: context.scenario_done,
                            scenario_total: context.scenario_total,
                            file_done: payload.file_index,
                            current_file: Some(payload.file_index + 1),
                            file_count: workload.file_count(),
                            processed_bytes: total_bytes.saturating_add(bytes),
                            total_bytes: workload.total_bytes(),
                            hdd_concurrency: 0,
                            current_rate: Some(bytes as f64 / seconds.max(0.001)),
                            ssd_write_rate: measurement_rate_with_current(
                                file_results.iter().map(|row| row.ssd_write),
                                bytes,
                                seconds,
                            ),
                            ssd_read_rate: measurement_rate(
                                file_results.iter().map(|row| row.ssd_read),
                            ),
                            hdd_write_rate: None,
                            hdd_disk_rates: Vec::new(),
                            active_hdd_landing: Vec::new(),
                            aggregate_rate: None,
                            report_path: context.report_path,
                            json_path: context.json_path,
                        },
                    )
                };
                measure_ssd_stage_payload_with_progress(
                    &payload,
                    &ssd_path,
                    payload.file_index,
                    Some(&mut progress),
                    &ssd_settler,
                )
            } else {
                measure_ssd_stage_payload(&payload, &ssd_path, &ssd_settler)
            } {
                Ok(measurement) => measurement,
                Err(err) => {
                    let _ = fs::remove_file(&ssd_path);
                    return Err(err);
                }
            };
            total_bytes = total_bytes.saturating_add(ssd_write.bytes);
            file_results.push(PerformanceFileResult {
                file_index: payload.file_index,
                ssd_write,
                ssd_read: zero_measurement(),
            });
            for copy_index in 0..redundancy {
                staged_jobs.push(SsdPipelineJob {
                    file_index: payload.file_index,
                    copy_index,
                    relative_path: payload.relative_path.clone(),
                    ssd_path: ssd_path.clone(),
                    size_bytes: payload.size_bytes,
                });
            }
            if log_progress {
                writeln!(
                    writer,
                    "ssd-stage-then-drain c{} batch {}/{} file {}/{}: SSD write {}/s staged for later HDD drain",
                    concurrency,
                    batch_index + 1,
                    batch_count,
                    payload.file_index + 1,
                    workload.file_count(),
                    format_bytes(throughput(ssd_write))
                )?;
            }
        }

        if let Some(context) = tui_context {
            render_performance_tui_snapshot(
                writer,
                &PerformanceTuiSnapshot {
                    phase: "ssd-stage-then-drain draining",
                    scenario: "ssd-stage-then-drain",
                    activity: format!(
                        "Batch {}/{} staged {}; draining SSD backlog with {concurrency} HDD worker(s)",
                        batch_index + 1,
                        batch_count,
                        format_bytes(batch_bytes as f64)
                    ),
                    objective: performance_scenario_objective(
                        PerformanceScenarioKind::SsdStageThenDrain,
                        concurrency,
                    ),
                    bounds: performance_scenario_bounds(
                        workload,
                        PerformanceScenarioKind::SsdStageThenDrain,
                        concurrency,
                    ),
                    scenario_done: context.scenario_done,
                    scenario_total: context.scenario_total,
                    file_done: file_results.len() as u32,
                    current_file: None,
                    file_count: workload.file_count(),
                    processed_bytes: total_bytes,
                    total_bytes: workload.total_bytes(),
                    hdd_concurrency: concurrency,
                    current_rate: None,
                    ssd_write_rate: measurement_rate(file_results.iter().map(|row| row.ssd_write)),
                    ssd_read_rate: measurement_rate(file_results.iter().map(|row| row.ssd_read)),
                    hdd_write_rate: None,
                    hdd_disk_rates: Vec::new(),
                    active_hdd_landing: Vec::new(),
                    aggregate_rate: Some(
                        total_bytes as f64 / started.elapsed().as_secs_f64().max(0.001),
                    ),
                    report_path: context.report_path,
                    json_path: context.json_path,
                },
            )?;
        }

        let (sender, receiver) = mpsc::sync_channel::<SsdPipelineJob>(queue_capacity);
        let receiver = Arc::new(Mutex::new(receiver));
        let worker_results = Arc::new(Mutex::new(Vec::<PerformanceDiskResult>::new()));
        let hdd_jobs_started = Arc::new(AtomicU32::new(0));
        let hdd_jobs_completed = Arc::new(AtomicU32::new(0));
        let hdd_bytes_transferred = Arc::new(AtomicU64::new(0));
        let live_rates = PerformanceLiveRateCounters::default();
        let active_hdd_writes = Arc::new(Mutex::new(
            BTreeMap::<ActiveHddWriteKey, ActiveHddWrite>::new(),
        ));
        let mut handles = Vec::new();
        for _ in 0..concurrency {
            let receiver = Arc::clone(&receiver);
            let scheduler = Arc::clone(&scheduler);
            let worker_results = Arc::clone(&worker_results);
            let hdd_jobs_started = Arc::clone(&hdd_jobs_started);
            let hdd_jobs_completed = Arc::clone(&hdd_jobs_completed);
            let hdd_bytes_transferred = Arc::clone(&hdd_bytes_transferred);
            let live_rates = live_rates.clone();
            let active_hdd_writes = Arc::clone(&active_hdd_writes);
            handles.push(thread::spawn(move || -> Result<(), CliError> {
                loop {
                    check_performance_cancelled()?;
                    let job = {
                        let receiver = receiver.lock().map_err(|_| {
                            CliError::CommandFailed(
                                "performance-test staged HDD queue lock poisoned".to_string(),
                            )
                        })?;
                        receiver.recv()
                    };
                    let Ok(job) = job else {
                        break;
                    };
                    hdd_jobs_started.fetch_add(1, Ordering::SeqCst);
                    let placement = reserve_performance_disk_for_file(&scheduler, job.file_index)?;
                    let destination = placement
                        .root_path
                        .join("ssd-stage-then-drain")
                        .join(format!("c{concurrency}"))
                        .join(&job.relative_path);
                    let active_key = (job.file_index, job.copy_index);
                    active_hdd_writes
                        .lock()
                        .map_err(|_| {
                            CliError::CommandFailed(
                                "performance-test active HDD write lock poisoned".to_string(),
                            )
                        })?
                        .insert(
                            active_key,
                            ActiveHddWrite {
                                file_index: job.file_index,
                                copy_index: job.copy_index,
                                relative_path: job.relative_path.clone(),
                                disk_id: placement.disk_id.clone(),
                                size_bytes: job.size_bytes,
                                bytes_written: 0,
                                started: Instant::now(),
                                phase: PerformanceCopyProgressPhase::Copying,
                            },
                        );
                    let mut last_progress_bytes = 0_u64;
                    let mut last_read_seconds = 0.0_f64;
                    let mut last_write_seconds = 0.0_f64;
                    let mut progress =
                        |copy_progress: PerformanceSplitCopyProgress| -> Result<(), CliError> {
                            let delta = copy_progress.bytes.saturating_sub(last_progress_bytes);
                            last_progress_bytes = copy_progress.bytes;
                            let delta_read_seconds =
                                (copy_progress.source_read_seconds - last_read_seconds).max(0.0);
                            last_read_seconds = copy_progress.source_read_seconds;
                            let delta_write_seconds = (copy_progress.destination_write_seconds
                                - last_write_seconds)
                                .max(0.0);
                            last_write_seconds = copy_progress.destination_write_seconds;
                            if delta > 0 {
                                hdd_bytes_transferred.fetch_add(delta, Ordering::SeqCst);
                                live_rates.add_ssd_read_interval(delta, delta_read_seconds)?;
                            }
                            live_rates.add_hdd_write_interval(
                                &placement.disk_id,
                                delta,
                                delta_write_seconds,
                            )?;
                            if let Some(active) = active_hdd_writes
                                .lock()
                                .map_err(|_| {
                                    CliError::CommandFailed(
                                        "performance-test active HDD write lock poisoned"
                                            .to_string(),
                                    )
                                })?
                                .get_mut(&active_key)
                            {
                                active.bytes_written = copy_progress.bytes;
                                active.phase = copy_progress.phase;
                            }
                            Ok(())
                        };
                    let measurement = measure_copy_with_split_progress(
                        &job.ssd_path,
                        &destination,
                        Some(&mut progress),
                    );
                    let _ = fs::remove_file(&destination);
                    let _ = active_hdd_writes
                        .lock()
                        .map(|mut active| active.remove(&active_key));
                    let measurement = match measurement {
                        Ok(measurement) => measurement,
                        Err(err) => {
                            let _ =
                                complete_performance_disk(&scheduler, &placement.disk_id, 0, 0.0);
                            return Err(err);
                        }
                    };
                    hdd_jobs_completed.fetch_add(1, Ordering::SeqCst);
                    complete_performance_disk(
                        &scheduler,
                        &placement.disk_id,
                        measurement.destination_write.bytes,
                        measurement.destination_write.seconds,
                    )?;
                    worker_results
                        .lock()
                        .map_err(|_| {
                            CliError::CommandFailed(
                                "performance-test result lock poisoned".to_string(),
                            )
                        })?
                        .push(PerformanceDiskResult {
                            file_index: job.file_index,
                            copy_index: job.copy_index,
                            concurrency,
                            scenario: PerformanceScenarioKind::SsdStageThenDrain,
                            disk_id: placement.disk_id,
                            ssd_read: measurement.source_read,
                            write: measurement.destination_write,
                        });
                }
                Ok(())
            }));
        }
        let total_hdd_jobs = staged_jobs.len();
        let mut submitted_hdd_jobs = 0_usize;
        for job in staged_jobs {
            let mut pending_job = Some(job);
            loop {
                check_performance_cancelled()?;
                let job = pending_job.take().expect("pending staged HDD job");
                match sender.try_send(job) {
                    Ok(()) => {
                        submitted_hdd_jobs += 1;
                        break;
                    }
                    Err(mpsc::TrySendError::Full(job)) => {
                        pending_job = Some(job);
                        if let Some(context) = tui_context {
                            let rate_snapshot = live_rates.snapshot()?;
                            render_hdd_drain_tui_snapshot(
                                writer,
                                HddDrainTuiState {
                                    context,
                                    workload,
                                    kind: PerformanceScenarioKind::SsdStageThenDrain,
                                    concurrency,
                                    submitted_jobs: submitted_hdd_jobs,
                                    total_jobs: total_hdd_jobs,
                                    started_jobs: hdd_jobs_started.load(Ordering::SeqCst) as usize,
                                    completed_jobs: hdd_jobs_completed.load(Ordering::SeqCst)
                                        as usize,
                                    transferred_bytes: hdd_bytes_transferred.load(Ordering::SeqCst),
                                    ssd_read_rate: rate_snapshot.ssd_read_rate,
                                    hdd_write_rate: rate_snapshot.hdd_write_rate,
                                    hdd_disk_rates: active_hdd_disk_rates(&active_hdd_writes)?,
                                    active_hdd_landing: active_hdd_landing_lines(
                                        &active_hdd_writes,
                                        workload.file_count(),
                                    )?,
                                },
                            )?;
                        }
                        thread::sleep(std::time::Duration::from_millis(250));
                    }
                    Err(mpsc::TrySendError::Disconnected(_)) => {
                        return Err(CliError::CommandFailed(
                            "performance-test staged HDD workers stopped early".to_string(),
                        ));
                    }
                }
            }
        }
        drop(sender);
        if let Some(context) = tui_context {
            while (hdd_jobs_completed.load(Ordering::SeqCst) as usize) < total_hdd_jobs {
                check_performance_cancelled()?;
                let rate_snapshot = live_rates.snapshot()?;
                render_hdd_drain_tui_snapshot(
                    writer,
                    HddDrainTuiState {
                        context,
                        workload,
                        kind: PerformanceScenarioKind::SsdStageThenDrain,
                        concurrency,
                        submitted_jobs: submitted_hdd_jobs,
                        total_jobs: total_hdd_jobs,
                        started_jobs: hdd_jobs_started.load(Ordering::SeqCst) as usize,
                        completed_jobs: hdd_jobs_completed.load(Ordering::SeqCst) as usize,
                        transferred_bytes: hdd_bytes_transferred.load(Ordering::SeqCst),
                        ssd_read_rate: rate_snapshot.ssd_read_rate,
                        hdd_write_rate: rate_snapshot.hdd_write_rate,
                        hdd_disk_rates: active_hdd_disk_rates(&active_hdd_writes)?,
                        active_hdd_landing: active_hdd_landing_lines(
                            &active_hdd_writes,
                            workload.file_count(),
                        )?,
                    },
                )?;
                if handles.iter().all(|handle| handle.is_finished()) {
                    break;
                }
                thread::sleep(std::time::Duration::from_millis(500));
            }
        }
        let mut worker_error = None;
        for handle in handles {
            match handle.join() {
                Ok(Ok(())) => {}
                Ok(Err(err)) => {
                    let _ = worker_error.get_or_insert(err);
                }
                Err(_) => {
                    let _ = worker_error.get_or_insert(CliError::CommandFailed(
                        "performance-test staged HDD worker panicked".to_string(),
                    ));
                }
            };
        }
        if let Some(err) = worker_error {
            return Err(err);
        }
        let mut batch_disk_results = Arc::try_unwrap(worker_results)
            .map_err(|_| {
                CliError::CommandFailed("performance-test result lock still shared".to_string())
            })?
            .into_inner()
            .map_err(|_| {
                CliError::CommandFailed("performance-test result lock poisoned".to_string())
            })?;
        for job in batch_disk_results.iter() {
            let ssd_path = scenario_root.join(
                workload
                    .payloads
                    .iter()
                    .find(|payload| payload.file_index == job.file_index)
                    .map(|payload| payload.relative_path.clone())
                    .unwrap_or_else(|| PathBuf::from(format!("missing-{}", job.file_index))),
            );
            let _ = fs::remove_file(&ssd_path);
        }
        disk_results.append(&mut batch_disk_results);
    }

    ssd_settler.finish()?;
    let io_samples = io_sampler.stop();
    let elapsed_seconds = started.elapsed().as_secs_f64().max(0.001);
    disk_results.sort_by(|left, right| {
        left.file_index
            .cmp(&right.file_index)
            .then_with(|| left.copy_index.cmp(&right.copy_index))
            .then_with(|| left.disk_id.cmp(&right.disk_id))
    });
    update_file_read_measurements_from_disk_results(&mut file_results, &disk_results);
    let physical_hdd_write_bytes = disk_results.iter().map(|row| row.write.bytes).sum::<u64>();
    let slowest_seconds = disk_results
        .iter()
        .map(|row| row.write.seconds)
        .fold(0.0_f64, f64::max);
    let members = disk_results
        .iter()
        .map(|row| row.disk_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if log_progress {
        writeln!(
            writer,
            "ssd-stage-then-drain c{}: aggregate landing {}/s",
            concurrency,
            format_bytes(physical_hdd_write_bytes as f64 / elapsed_seconds)
        )?;
    }
    Ok(PerformanceScenarioResult {
        kind: PerformanceScenarioKind::SsdStageThenDrain,
        file_order: workload.file_order,
        concurrency,
        redundancy,
        queue_capacity,
        elapsed_seconds,
        total_bytes: physical_hdd_write_bytes,
        logical_source_bytes: workload.total_bytes(),
        physical_hdd_write_bytes,
        hdd_write_operations: disk_results.len(),
        hdd_drain_started_before_all_ssd_staged: false,
        file_results,
        disk_results,
        io_samples,
        concurrency_result: PerformanceConcurrencyResult {
            concurrency,
            scenario: PerformanceScenarioKind::SsdStageThenDrain,
            aggregate_bytes: physical_hdd_write_bytes,
            seconds: elapsed_seconds,
            slowest_seconds,
            members,
        },
    })
}

fn benchmark_ssd_pipeline(
    ssd_bench_root: &Path,
    hdd_bench_roots: &[(DiskId, PathBuf)],
    workload: &PerformanceWorkload,
    concurrency: usize,
    redundancy: usize,
    writer: &mut impl Write,
    log_progress: bool,
    tui_context: Option<PerformanceTuiContext<'_>>,
) -> Result<PerformanceScenarioResult, CliError> {
    benchmark_ssd_pipeline_with_options(
        ssd_bench_root,
        hdd_bench_roots,
        workload,
        concurrency,
        redundancy,
        writer,
        log_progress,
        tui_context,
        SsdPipelineBenchmarkOptions::default(),
    )
}

#[derive(Clone, Copy, Debug, Default)]
struct SsdPipelineBenchmarkOptions {
    wait_for_first_hdd_start_after_first_file: bool,
}

fn benchmark_ssd_pipeline_with_options(
    ssd_bench_root: &Path,
    hdd_bench_roots: &[(DiskId, PathBuf)],
    workload: &PerformanceWorkload,
    concurrency: usize,
    redundancy: usize,
    writer: &mut impl Write,
    log_progress: bool,
    tui_context: Option<PerformanceTuiContext<'_>>,
    options: SsdPipelineBenchmarkOptions,
) -> Result<PerformanceScenarioResult, CliError> {
    let started = Instant::now();
    let io_sampler = PerformanceIoSampler::start(performance_io_devices(
        Some(ssd_bench_root),
        hdd_bench_roots,
    ));
    let ssd_settler = PerformanceSsdSettler::start(PERFORMANCE_SSD_SETTLE_QUEUE_CAPACITY);
    let scenario_root = ssd_bench_root
        .join("ssd-pipeline")
        .join(format!("c{concurrency}"));
    let residency_budget = performance_ssd_residency_budget(&scenario_root)?;
    let queue_capacity = hdd_queue_capacity(concurrency, redundancy);
    let scheduler = new_shared_disk_placement_scheduler(hdd_bench_roots)?;
    let (sender, receiver) = mpsc::sync_channel::<SsdPipelineJob>(queue_capacity);
    let receiver = Arc::new(Mutex::new(receiver));
    let worker_results = Arc::new(Mutex::new(Vec::<PerformanceDiskResult>::new()));
    let staging_complete = Arc::new(AtomicBool::new(false));
    let hdd_jobs_started = Arc::new(AtomicU32::new(0));
    let hdd_jobs_completed = Arc::new(AtomicU32::new(0));
    let hdd_bytes_transferred = Arc::new(AtomicU64::new(0));
    let live_rates = PerformanceLiveRateCounters::default();
    let resident_ssd_bytes = Arc::new(AtomicU64::new(0));
    let ssd_file_remaining_copies = Arc::new(Mutex::new(BTreeMap::<u32, usize>::new()));
    let active_hdd_writes = Arc::new(Mutex::new(
        BTreeMap::<ActiveHddWriteKey, ActiveHddWrite>::new(),
    ));
    let hdd_drain_started_before_all_ssd_staged = Arc::new(AtomicBool::new(false));
    let mut handles = Vec::new();

    for _ in 0..concurrency {
        let receiver = Arc::clone(&receiver);
        let scheduler = Arc::clone(&scheduler);
        let worker_results = Arc::clone(&worker_results);
        let staging_complete = Arc::clone(&staging_complete);
        let hdd_jobs_started = Arc::clone(&hdd_jobs_started);
        let hdd_jobs_completed = Arc::clone(&hdd_jobs_completed);
        let hdd_bytes_transferred = Arc::clone(&hdd_bytes_transferred);
        let live_rates = live_rates.clone();
        let resident_ssd_bytes = Arc::clone(&resident_ssd_bytes);
        let ssd_file_remaining_copies = Arc::clone(&ssd_file_remaining_copies);
        let active_hdd_writes = Arc::clone(&active_hdd_writes);
        let hdd_drain_started_before_all_ssd_staged =
            Arc::clone(&hdd_drain_started_before_all_ssd_staged);
        handles.push(thread::spawn(move || -> Result<(), CliError> {
            loop {
                check_performance_cancelled()?;
                let job = {
                    let receiver = receiver.lock().map_err(|_| {
                        CliError::CommandFailed(
                            "performance-test HDD queue lock poisoned".to_string(),
                        )
                    })?;
                    receiver.recv()
                };
                let Ok(job) = job else {
                    break;
                };
                hdd_jobs_started.fetch_add(1, Ordering::SeqCst);
                if !staging_complete.load(Ordering::SeqCst) {
                    hdd_drain_started_before_all_ssd_staged.store(true, Ordering::SeqCst);
                }
                let placement = reserve_performance_disk_for_file(&scheduler, job.file_index)?;
                let destination = placement
                    .root_path
                    .join("ssd-pipeline")
                    .join(format!("c{concurrency}"))
                    .join(&job.relative_path);
                let active_key = (job.file_index, job.copy_index);
                active_hdd_writes
                    .lock()
                    .map_err(|_| {
                        CliError::CommandFailed(
                            "performance-test active HDD write lock poisoned".to_string(),
                        )
                    })?
                    .insert(
                        active_key,
                        ActiveHddWrite {
                            file_index: job.file_index,
                            copy_index: job.copy_index,
                            relative_path: job.relative_path.clone(),
                            disk_id: placement.disk_id.clone(),
                            size_bytes: job.size_bytes,
                            bytes_written: 0,
                            started: Instant::now(),
                            phase: PerformanceCopyProgressPhase::Copying,
                        },
                    );
                let mut last_progress_bytes = 0_u64;
                let mut last_read_seconds = 0.0_f64;
                let mut last_write_seconds = 0.0_f64;
                let mut progress =
                    |copy_progress: PerformanceSplitCopyProgress| -> Result<(), CliError> {
                        let delta = copy_progress.bytes.saturating_sub(last_progress_bytes);
                        last_progress_bytes = copy_progress.bytes;
                        let delta_read_seconds =
                            (copy_progress.source_read_seconds - last_read_seconds).max(0.0);
                        last_read_seconds = copy_progress.source_read_seconds;
                        let delta_write_seconds =
                            (copy_progress.destination_write_seconds - last_write_seconds).max(0.0);
                        last_write_seconds = copy_progress.destination_write_seconds;
                        if delta > 0 {
                            hdd_bytes_transferred.fetch_add(delta, Ordering::SeqCst);
                            live_rates.add_ssd_read_interval(delta, delta_read_seconds)?;
                        }
                        live_rates.add_hdd_write_interval(
                            &placement.disk_id,
                            delta,
                            delta_write_seconds,
                        )?;
                        if let Some(active) = active_hdd_writes
                            .lock()
                            .map_err(|_| {
                                CliError::CommandFailed(
                                    "performance-test active HDD write lock poisoned".to_string(),
                                )
                            })?
                            .get_mut(&active_key)
                        {
                            active.bytes_written = copy_progress.bytes;
                            active.phase = copy_progress.phase;
                        }
                        Ok(())
                    };
                let measurement = measure_copy_with_split_progress(
                    &job.ssd_path,
                    &destination,
                    Some(&mut progress),
                );
                let _ = fs::remove_file(&destination);
                let _ = active_hdd_writes
                    .lock()
                    .map(|mut active| active.remove(&active_key));
                let measurement = match measurement {
                    Ok(measurement) => measurement,
                    Err(err) => {
                        let _ = complete_performance_disk(&scheduler, &placement.disk_id, 0, 0.0);
                        return Err(err);
                    }
                };
                hdd_jobs_completed.fetch_add(1, Ordering::SeqCst);
                let remove_staged_ssd_file = {
                    let mut remaining = ssd_file_remaining_copies.lock().map_err(|_| {
                        CliError::CommandFailed(
                            "performance-test SSD residency lock poisoned".to_string(),
                        )
                    })?;
                    match remaining.get_mut(&job.file_index) {
                        Some(count) if *count > 1 => {
                            *count -= 1;
                            false
                        }
                        Some(_) => {
                            remaining.remove(&job.file_index);
                            true
                        }
                        None => false,
                    }
                };
                if remove_staged_ssd_file {
                    let _ = fs::remove_file(&job.ssd_path);
                    resident_ssd_bytes
                        .fetch_sub(measurement.destination_write.bytes, Ordering::SeqCst);
                }
                complete_performance_disk(
                    &scheduler,
                    &placement.disk_id,
                    measurement.destination_write.bytes,
                    measurement.destination_write.seconds,
                )?;
                worker_results
                    .lock()
                    .map_err(|_| {
                        CliError::CommandFailed("performance-test result lock poisoned".to_string())
                    })?
                    .push(PerformanceDiskResult {
                        file_index: job.file_index,
                        copy_index: job.copy_index,
                        concurrency,
                        scenario: PerformanceScenarioKind::SsdPipeline,
                        disk_id: placement.disk_id,
                        ssd_read: measurement.source_read,
                        write: measurement.destination_write,
                    });
            }
            Ok(())
        }));
    }

    let mut file_results = Vec::<PerformanceFileResult>::new();
    let mut total_bytes = 0_u64;
    let mut producer_error = None;
    let total_hdd_jobs = workload.file_count() as usize * redundancy;
    let mut submitted_hdd_jobs = 0_usize;
    let mut pending_hdd_jobs = VecDeque::<SsdPipelineJob>::new();
    for payload in &workload.payloads {
        if let Err(err) = check_performance_cancelled() {
            producer_error = Some(err);
            break;
        }
        if let Err(err) = validate_performance_payload_fits_ssd(payload, residency_budget) {
            producer_error = Some(err);
            break;
        }
        match try_submit_pending_ssd_pipeline_jobs(
            &sender,
            &mut pending_hdd_jobs,
            &mut submitted_hdd_jobs,
        ) {
            Ok(_) => {}
            Err(err) => {
                producer_error = Some(err);
                break;
            }
        }
        while !performance_ssd_can_admit_payload(
            resident_ssd_bytes.load(Ordering::SeqCst),
            payload.size_bytes,
            residency_budget,
        ) {
            if let Err(err) = check_performance_cancelled() {
                producer_error = Some(err);
                break;
            }
            match try_submit_pending_ssd_pipeline_jobs(
                &sender,
                &mut pending_hdd_jobs,
                &mut submitted_hdd_jobs,
            ) {
                Ok(_) => {}
                Err(err) => {
                    producer_error = Some(err);
                    break;
                }
            }
            if let Some(context) = tui_context {
                let rate_snapshot = live_rates.snapshot()?;
                render_hdd_drain_tui_snapshot(
                    writer,
                    HddDrainTuiState {
                        context,
                        workload,
                        kind: PerformanceScenarioKind::SsdPipeline,
                        concurrency,
                        submitted_jobs: submitted_hdd_jobs,
                        total_jobs: total_hdd_jobs,
                        started_jobs: hdd_jobs_started.load(Ordering::SeqCst) as usize,
                        completed_jobs: hdd_jobs_completed.load(Ordering::SeqCst) as usize,
                        transferred_bytes: hdd_bytes_transferred.load(Ordering::SeqCst),
                        ssd_read_rate: rate_snapshot.ssd_read_rate,
                        hdd_write_rate: rate_snapshot.hdd_write_rate,
                        hdd_disk_rates: active_hdd_disk_rates(&active_hdd_writes)?,
                        active_hdd_landing: active_hdd_landing_lines(
                            &active_hdd_writes,
                            workload.file_count(),
                        )?,
                    },
                )?;
            }
            thread::sleep(std::time::Duration::from_millis(250));
        }
        if producer_error.is_some() {
            break;
        }
        let ssd_path = scenario_root.join(&payload.relative_path);
        if let Some(context) = tui_context {
            let rate_snapshot = live_rates.snapshot()?;
            render_performance_tui_snapshot(
                writer,
                &PerformanceTuiSnapshot {
                    phase: "ssd-pipeline active",
                    scenario: "ssd-pipeline",
                    activity: format!(
                        "Staging file {}/{} to SSD before FIFO HDD drain: {}; HDD drained {}, draining {}, queued {}",
                        payload.file_index + 1,
                        workload.file_count(),
                        payload.relative_path.display(),
                        hdd_jobs_completed.load(Ordering::SeqCst),
                        hdd_jobs_started
                            .load(Ordering::SeqCst)
                            .saturating_sub(hdd_jobs_completed.load(Ordering::SeqCst)),
                        (submitted_hdd_jobs as u32)
                            .saturating_sub(hdd_jobs_started.load(Ordering::SeqCst))
                    ),
                    objective: performance_scenario_objective(
                        PerformanceScenarioKind::SsdPipeline,
                        concurrency,
                    ),
                    bounds: performance_scenario_bounds(
                        workload,
                        PerformanceScenarioKind::SsdPipeline,
                        concurrency,
                    ),
                    scenario_done: context.scenario_done,
                    scenario_total: context.scenario_total,
                    file_done: payload.file_index,
                    current_file: Some(payload.file_index + 1),
                    file_count: workload.file_count(),
                    processed_bytes: total_bytes,
                    total_bytes: workload.total_bytes(),
                    hdd_concurrency: concurrency,
                    current_rate: None,
                    ssd_write_rate: measurement_rate(file_results.iter().map(|row| row.ssd_write)),
                    ssd_read_rate: rate_snapshot.ssd_read_rate,
                    hdd_write_rate: rate_snapshot.hdd_write_rate,
                    hdd_disk_rates: active_hdd_disk_rates(&active_hdd_writes)?,
                        active_hdd_landing: active_hdd_landing_lines(&active_hdd_writes, workload.file_count())?,
                    aggregate_rate: None,
                    report_path: context.report_path,
                    json_path: context.json_path,
                },
            )?;
        }
        let ssd_write = match if let Some(context) = tui_context {
            let mut progress = |bytes: u64, seconds: f64| -> Result<(), CliError> {
                let rate_snapshot = live_rates.snapshot()?;
                render_performance_tui_snapshot(
                    writer,
                    &PerformanceTuiSnapshot {
                        phase: "ssd-pipeline active",
                        scenario: "ssd-pipeline",
                        activity: format!(
                            "Staging file {}/{} to SSD with {} HDD drain worker(s): {} ({}/{})",
                            payload.file_index + 1,
                            workload.file_count(),
                            concurrency,
                            payload.relative_path.display(),
                            format_bytes(bytes as f64),
                            format_bytes(payload.size_bytes as f64)
                        ),
                        objective: performance_scenario_objective(
                            PerformanceScenarioKind::SsdPipeline,
                            concurrency,
                        ),
                        bounds: performance_scenario_bounds(
                            workload,
                            PerformanceScenarioKind::SsdPipeline,
                            concurrency,
                        ),
                        scenario_done: context.scenario_done,
                        scenario_total: context.scenario_total,
                        file_done: payload.file_index,
                        current_file: Some(payload.file_index + 1),
                        file_count: workload.file_count(),
                        processed_bytes: total_bytes.saturating_add(bytes),
                        total_bytes: workload.total_bytes(),
                        hdd_concurrency: concurrency,
                        current_rate: Some(bytes as f64 / seconds.max(0.001)),
                        ssd_write_rate: measurement_rate_with_current(
                            file_results.iter().map(|row| row.ssd_write),
                            bytes,
                            seconds,
                        ),
                        ssd_read_rate: measurement_rate(
                            file_results.iter().map(|row| row.ssd_read),
                        ),
                        hdd_write_rate: rate_snapshot.hdd_write_rate,
                        hdd_disk_rates: active_hdd_disk_rates(&active_hdd_writes)?,
                        active_hdd_landing: active_hdd_landing_lines(
                            &active_hdd_writes,
                            workload.file_count(),
                        )?,
                        aggregate_rate: None,
                        report_path: context.report_path,
                        json_path: context.json_path,
                    },
                )
            };
            measure_ssd_stage_payload_with_progress(
                payload,
                &ssd_path,
                payload.file_index,
                Some(&mut progress),
                &ssd_settler,
            )
        } else {
            measure_ssd_stage_payload(payload, &ssd_path, &ssd_settler)
        } {
            Ok(measurement) => measurement,
            Err(err) => {
                let _ = fs::remove_file(&ssd_path);
                producer_error = Some(err);
                break;
            }
        };
        total_bytes = total_bytes.saturating_add(ssd_write.bytes);
        resident_ssd_bytes.fetch_add(ssd_write.bytes, Ordering::SeqCst);
        match ssd_file_remaining_copies.lock() {
            Ok(mut remaining) => {
                remaining.insert(payload.file_index, redundancy);
            }
            Err(_) => {
                producer_error = Some(CliError::CommandFailed(
                    "performance-test SSD residency lock poisoned".to_string(),
                ));
                break;
            }
        }
        file_results.push(PerformanceFileResult {
            file_index: payload.file_index,
            ssd_write,
            ssd_read: zero_measurement(),
        });
        for copy_index in 0..redundancy {
            pending_hdd_jobs.push_back(SsdPipelineJob {
                file_index: payload.file_index,
                copy_index,
                relative_path: payload.relative_path.clone(),
                ssd_path: ssd_path.clone(),
                size_bytes: payload.size_bytes,
            });
        }
        match try_submit_pending_ssd_pipeline_jobs(
            &sender,
            &mut pending_hdd_jobs,
            &mut submitted_hdd_jobs,
        ) {
            Ok(_) => {}
            Err(err) => {
                producer_error = Some(err);
                break;
            }
        }
        if producer_error.is_some() {
            break;
        }
        if options.wait_for_first_hdd_start_after_first_file && payload.file_index == 0 {
            let wait_started = Instant::now();
            while hdd_jobs_started.load(Ordering::SeqCst) == 0 {
                if wait_started.elapsed().as_secs_f64() > 5.0 {
                    producer_error = Some(CliError::CommandFailed(
                        "performance-test HDD worker did not start draining first staged file"
                            .to_string(),
                    ));
                    break;
                }
                thread::sleep(std::time::Duration::from_millis(1));
            }
            if producer_error.is_some() {
                break;
            }
        }
        if let Some(context) = tui_context {
            let rate_snapshot = live_rates.snapshot()?;
            render_performance_tui_snapshot(
                writer,
                &PerformanceTuiSnapshot {
                    phase: "ssd-pipeline queued",
                    scenario: "ssd-pipeline",
                    activity: format!(
                        "Queued file {}/{} for FIFO HDD drain with {} worker(s)",
                        payload.file_index + 1,
                        workload.file_count(),
                        concurrency
                    ),
                    objective: performance_scenario_objective(
                        PerformanceScenarioKind::SsdPipeline,
                        concurrency,
                    ),
                    bounds: performance_scenario_bounds(
                        workload,
                        PerformanceScenarioKind::SsdPipeline,
                        concurrency,
                    ),
                    scenario_done: context.scenario_done,
                    scenario_total: context.scenario_total,
                    file_done: payload.file_index + 1,
                    current_file: None,
                    file_count: workload.file_count(),
                    processed_bytes: total_bytes,
                    total_bytes: workload.total_bytes(),
                    hdd_concurrency: concurrency,
                    current_rate: Some(throughput(ssd_write)),
                    ssd_write_rate: measurement_rate(file_results.iter().map(|row| row.ssd_write)),
                    ssd_read_rate: rate_snapshot.ssd_read_rate,
                    hdd_write_rate: rate_snapshot.hdd_write_rate,
                    hdd_disk_rates: active_hdd_disk_rates(&active_hdd_writes)?,
                    active_hdd_landing: active_hdd_landing_lines(
                        &active_hdd_writes,
                        workload.file_count(),
                    )?,
                    aggregate_rate: Some(
                        total_bytes as f64 / started.elapsed().as_secs_f64().max(0.001),
                    ),
                    report_path: context.report_path,
                    json_path: context.json_path,
                },
            )?;
        }
        if log_progress {
            writeln!(
                writer,
                "ssd-pipeline c{} file {}/{}: SSD write {}/s queued for HDD drain",
                concurrency,
                payload.file_index + 1,
                workload.file_count(),
                format_bytes(throughput(ssd_write))
            )?;
        }
    }
    staging_complete.store(true, Ordering::SeqCst);
    while producer_error.is_none() && !pending_hdd_jobs.is_empty() {
        if let Err(err) = check_performance_cancelled() {
            producer_error = Some(err);
            break;
        }
        match try_submit_pending_ssd_pipeline_jobs(
            &sender,
            &mut pending_hdd_jobs,
            &mut submitted_hdd_jobs,
        ) {
            Ok(true) => {}
            Ok(false) => {
                if let Some(context) = tui_context {
                    let rate_snapshot = live_rates.snapshot()?;
                    render_hdd_drain_tui_snapshot(
                        writer,
                        HddDrainTuiState {
                            context,
                            workload,
                            kind: PerformanceScenarioKind::SsdPipeline,
                            concurrency,
                            submitted_jobs: submitted_hdd_jobs,
                            total_jobs: total_hdd_jobs,
                            started_jobs: hdd_jobs_started.load(Ordering::SeqCst) as usize,
                            completed_jobs: hdd_jobs_completed.load(Ordering::SeqCst) as usize,
                            transferred_bytes: hdd_bytes_transferred.load(Ordering::SeqCst),
                            ssd_read_rate: rate_snapshot.ssd_read_rate,
                            hdd_write_rate: rate_snapshot.hdd_write_rate,
                            hdd_disk_rates: active_hdd_disk_rates(&active_hdd_writes)?,
                            active_hdd_landing: active_hdd_landing_lines(
                                &active_hdd_writes,
                                workload.file_count(),
                            )?,
                        },
                    )?;
                }
                thread::sleep(std::time::Duration::from_millis(250));
            }
            Err(err) => {
                producer_error = Some(err);
                break;
            }
        }
    }
    drop(sender);
    if let Some(context) = tui_context {
        while (hdd_jobs_completed.load(Ordering::SeqCst) as usize) < total_hdd_jobs {
            check_performance_cancelled()?;
            let rate_snapshot = live_rates.snapshot()?;
            render_hdd_drain_tui_snapshot(
                writer,
                HddDrainTuiState {
                    context,
                    workload,
                    kind: PerformanceScenarioKind::SsdPipeline,
                    concurrency,
                    submitted_jobs: submitted_hdd_jobs,
                    total_jobs: total_hdd_jobs,
                    started_jobs: hdd_jobs_started.load(Ordering::SeqCst) as usize,
                    completed_jobs: hdd_jobs_completed.load(Ordering::SeqCst) as usize,
                    transferred_bytes: hdd_bytes_transferred.load(Ordering::SeqCst),
                    ssd_read_rate: rate_snapshot.ssd_read_rate,
                    hdd_write_rate: rate_snapshot.hdd_write_rate,
                    hdd_disk_rates: active_hdd_disk_rates(&active_hdd_writes)?,
                    active_hdd_landing: active_hdd_landing_lines(
                        &active_hdd_writes,
                        workload.file_count(),
                    )?,
                },
            )?;
            if handles.iter().all(|handle| handle.is_finished()) {
                break;
            }
            thread::sleep(std::time::Duration::from_millis(500));
        }
    }
    let mut worker_error = None;
    for handle in handles {
        match handle.join() {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                let _ = worker_error.get_or_insert(err);
            }
            Err(_) => {
                let _ = worker_error.get_or_insert(CliError::CommandFailed(
                    "performance-test HDD worker panicked".to_string(),
                ));
            }
        };
    }
    if let Some(err) = producer_error.or(worker_error) {
        return Err(err);
    }
    for payload in &workload.payloads {
        let ssd_path = scenario_root.join(&payload.relative_path);
        let _ = fs::remove_file(&ssd_path);
    }
    ssd_settler.finish()?;
    let io_samples = io_sampler.stop();
    let elapsed_seconds = started.elapsed().as_secs_f64().max(0.001);
    let mut disk_results = Arc::try_unwrap(worker_results)
        .map_err(|_| {
            CliError::CommandFailed("performance-test result lock still shared".to_string())
        })?
        .into_inner()
        .map_err(|_| {
            CliError::CommandFailed("performance-test result lock poisoned".to_string())
        })?;
    disk_results.sort_by(|left, right| {
        left.file_index
            .cmp(&right.file_index)
            .then_with(|| left.copy_index.cmp(&right.copy_index))
            .then_with(|| left.disk_id.cmp(&right.disk_id))
    });
    update_file_read_measurements_from_disk_results(&mut file_results, &disk_results);
    let physical_hdd_write_bytes = disk_results.iter().map(|row| row.write.bytes).sum::<u64>();
    let slowest_seconds = disk_results
        .iter()
        .map(|row| row.write.seconds)
        .fold(0.0_f64, f64::max);
    let members = disk_results
        .iter()
        .map(|row| row.disk_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if log_progress {
        writeln!(
            writer,
            "ssd-pipeline c{}: aggregate landing {}/s",
            concurrency,
            format_bytes(physical_hdd_write_bytes as f64 / elapsed_seconds)
        )?;
    }
    Ok(PerformanceScenarioResult {
        kind: PerformanceScenarioKind::SsdPipeline,
        file_order: workload.file_order,
        concurrency,
        redundancy,
        queue_capacity,
        elapsed_seconds,
        total_bytes: physical_hdd_write_bytes,
        logical_source_bytes: workload.total_bytes(),
        physical_hdd_write_bytes,
        hdd_write_operations: disk_results.len(),
        hdd_drain_started_before_all_ssd_staged: hdd_drain_started_before_all_ssd_staged
            .load(Ordering::SeqCst),
        file_results,
        disk_results,
        io_samples,
        concurrency_result: PerformanceConcurrencyResult {
            concurrency,
            scenario: PerformanceScenarioKind::SsdPipeline,
            aggregate_bytes: physical_hdd_write_bytes,
            seconds: elapsed_seconds,
            slowest_seconds,
            members,
        },
    })
}

fn benchmark_direct_hdd(
    hdd_bench_roots: &[(DiskId, PathBuf)],
    workload: &PerformanceWorkload,
    concurrency: usize,
    redundancy: usize,
    writer: &mut impl Write,
    log_progress: bool,
    tui_context: Option<PerformanceTuiContext<'_>>,
) -> Result<PerformanceScenarioResult, CliError> {
    let started = Instant::now();
    let io_sampler = PerformanceIoSampler::start(performance_io_devices(None, hdd_bench_roots));
    let queue_capacity = hdd_queue_capacity(concurrency, redundancy);
    let scheduler = new_shared_disk_placement_scheduler(hdd_bench_roots)?;
    let (sender, receiver) = mpsc::sync_channel::<DirectHddJob>(queue_capacity);
    let receiver = Arc::new(Mutex::new(receiver));
    let worker_results = Arc::new(Mutex::new(Vec::<PerformanceDiskResult>::new()));
    let hdd_jobs_started = Arc::new(AtomicU32::new(0));
    let hdd_jobs_completed = Arc::new(AtomicU32::new(0));
    let hdd_bytes_transferred = Arc::new(AtomicU64::new(0));
    let live_rates = PerformanceLiveRateCounters::default();
    let active_hdd_writes = Arc::new(Mutex::new(
        BTreeMap::<ActiveHddWriteKey, ActiveHddWrite>::new(),
    ));
    let mut handles = Vec::new();
    for worker_index in 0..concurrency {
        let receiver = Arc::clone(&receiver);
        let scheduler = Arc::clone(&scheduler);
        let worker_results = Arc::clone(&worker_results);
        let hdd_jobs_started = Arc::clone(&hdd_jobs_started);
        let hdd_jobs_completed = Arc::clone(&hdd_jobs_completed);
        let hdd_bytes_transferred = Arc::clone(&hdd_bytes_transferred);
        let live_rates = live_rates.clone();
        let active_hdd_writes = Arc::clone(&active_hdd_writes);
        handles.push(thread::spawn(move || -> Result<(), CliError> {
            loop {
                check_performance_cancelled()?;
                let payload = {
                    let receiver = receiver.lock().map_err(|_| {
                        CliError::CommandFailed(
                            "performance-test direct HDD queue lock poisoned".to_string(),
                        )
                    })?;
                    receiver.recv()
                };
                let Ok(job) = payload else {
                    break;
                };
                hdd_jobs_started.fetch_add(1, Ordering::SeqCst);
                let placement =
                    reserve_performance_disk_for_file(&scheduler, job.payload.file_index)?;
                let destination = placement
                    .root_path
                    .join("direct-hdd")
                    .join(format!("c{concurrency}"))
                    .join(&job.payload.relative_path);
                let active_key = (job.payload.file_index, job.copy_index);
                active_hdd_writes
                    .lock()
                    .map_err(|_| {
                        CliError::CommandFailed(
                            "performance-test active HDD write lock poisoned".to_string(),
                        )
                    })?
                    .insert(
                        active_key,
                        ActiveHddWrite {
                            file_index: job.payload.file_index,
                            copy_index: job.copy_index,
                            relative_path: job.payload.relative_path.clone(),
                            disk_id: placement.disk_id.clone(),
                            size_bytes: job.payload.size_bytes,
                            bytes_written: 0,
                            started: Instant::now(),
                            phase: PerformanceCopyProgressPhase::Copying,
                        },
                    );
                let mut last_progress_bytes = 0_u64;
                let mut last_write_seconds = 0.0_f64;
                let mut progress = |bytes: u64,
                                    write_seconds: f64,
                                    phase: PerformanceCopyProgressPhase|
                 -> Result<(), CliError> {
                    let delta = bytes.saturating_sub(last_progress_bytes);
                    last_progress_bytes = bytes;
                    let delta_write_seconds = (write_seconds - last_write_seconds).max(0.0);
                    last_write_seconds = write_seconds;
                    hdd_bytes_transferred.fetch_add(delta, Ordering::SeqCst);
                    live_rates.add_hdd_write_interval(
                        &placement.disk_id,
                        delta,
                        delta_write_seconds,
                    )?;
                    if let Some(active) = active_hdd_writes
                        .lock()
                        .map_err(|_| {
                            CliError::CommandFailed(
                                "performance-test active HDD write lock poisoned".to_string(),
                            )
                        })?
                        .get_mut(&active_key)
                    {
                        active.bytes_written = bytes;
                        active.phase = phase;
                    }
                    Ok(())
                };
                let measurement = if let Some(source) = &job.payload.source_path {
                    let mut split_progress =
                        |copy_progress: PerformanceSplitCopyProgress| -> Result<(), CliError> {
                            progress(
                                copy_progress.bytes,
                                copy_progress.destination_write_seconds,
                                copy_progress.phase,
                            )
                        };
                    measure_copy_with_split_progress(
                        source,
                        &destination,
                        Some(&mut split_progress),
                    )
                    .map(|measurement| measurement.destination_write)
                } else {
                    let mut generated_progress =
                        |bytes: u64, seconds: f64| -> Result<(), CliError> {
                            progress(bytes, seconds, PerformanceCopyProgressPhase::Copying)
                        };
                    measure_land_payload_with_progress_and_sync_policy(
                        &job.payload,
                        &destination,
                        job.payload.file_index ^ worker_index as u32 ^ job.copy_index as u32,
                        Some(&mut generated_progress),
                        PerformanceCopySyncPolicy::SyncAll,
                    )
                };
                let _ = fs::remove_file(&destination);
                let _ = active_hdd_writes
                    .lock()
                    .map(|mut active| active.remove(&active_key));
                let measurement = match measurement {
                    Ok(measurement) => measurement,
                    Err(err) => {
                        let _ = complete_performance_disk(&scheduler, &placement.disk_id, 0, 0.0);
                        return Err(err);
                    }
                };
                complete_performance_disk(
                    &scheduler,
                    &placement.disk_id,
                    measurement.bytes,
                    measurement.seconds,
                )?;
                hdd_jobs_completed.fetch_add(1, Ordering::SeqCst);
                worker_results
                    .lock()
                    .map_err(|_| {
                        CliError::CommandFailed("performance-test result lock poisoned".to_string())
                    })?
                    .push(PerformanceDiskResult {
                        file_index: job.payload.file_index,
                        copy_index: job.copy_index,
                        concurrency,
                        scenario: PerformanceScenarioKind::DirectHdd,
                        disk_id: placement.disk_id,
                        ssd_read: zero_measurement(),
                        write: measurement,
                    });
            }
            Ok(())
        }));
    }
    let mut producer_error = None;
    let total_hdd_jobs = workload.file_count() as usize * redundancy;
    let mut submitted_hdd_jobs = 0_usize;
    for payload in &workload.payloads {
        if let Err(err) = check_performance_cancelled() {
            producer_error = Some(err);
            break;
        }
        for copy_index in 0..redundancy {
            let mut pending_job = Some(DirectHddJob {
                payload: payload.clone(),
                copy_index,
            });
            loop {
                check_performance_cancelled()?;
                let job = pending_job.take().expect("pending direct HDD job");
                match sender.try_send(job) {
                    Ok(()) => {
                        submitted_hdd_jobs += 1;
                        break;
                    }
                    Err(mpsc::TrySendError::Full(job)) => {
                        pending_job = Some(job);
                        if let Some(context) = tui_context {
                            let rate_snapshot = live_rates.snapshot()?;
                            render_hdd_drain_tui_snapshot(
                                writer,
                                HddDrainTuiState {
                                    context,
                                    workload,
                                    kind: PerformanceScenarioKind::DirectHdd,
                                    concurrency,
                                    submitted_jobs: submitted_hdd_jobs,
                                    total_jobs: total_hdd_jobs,
                                    started_jobs: hdd_jobs_started.load(Ordering::SeqCst) as usize,
                                    completed_jobs: hdd_jobs_completed.load(Ordering::SeqCst)
                                        as usize,
                                    transferred_bytes: hdd_bytes_transferred.load(Ordering::SeqCst),
                                    ssd_read_rate: None,
                                    hdd_write_rate: rate_snapshot.hdd_write_rate,
                                    hdd_disk_rates: active_hdd_disk_rates(&active_hdd_writes)?,
                                    active_hdd_landing: active_hdd_landing_lines(
                                        &active_hdd_writes,
                                        workload.file_count(),
                                    )?,
                                },
                            )?;
                        }
                        thread::sleep(Duration::from_millis(250));
                    }
                    Err(mpsc::TrySendError::Disconnected(_)) => {
                        producer_error = Some(CliError::CommandFailed(
                            "performance-test direct HDD workers stopped early".to_string(),
                        ));
                        break;
                    }
                }
            }
            if producer_error.is_some() {
                break;
            }
            if let Some(context) = tui_context {
                let rate_snapshot = live_rates.snapshot()?;
                render_hdd_drain_tui_snapshot(
                    writer,
                    HddDrainTuiState {
                        context,
                        workload,
                        kind: PerformanceScenarioKind::DirectHdd,
                        concurrency,
                        submitted_jobs: submitted_hdd_jobs,
                        total_jobs: total_hdd_jobs,
                        started_jobs: hdd_jobs_started.load(Ordering::SeqCst) as usize,
                        completed_jobs: hdd_jobs_completed.load(Ordering::SeqCst) as usize,
                        transferred_bytes: hdd_bytes_transferred.load(Ordering::SeqCst),
                        ssd_read_rate: None,
                        hdd_write_rate: rate_snapshot.hdd_write_rate,
                        hdd_disk_rates: active_hdd_disk_rates(&active_hdd_writes)?,
                        active_hdd_landing: active_hdd_landing_lines(
                            &active_hdd_writes,
                            workload.file_count(),
                        )?,
                    },
                )?;
            }
        }
        if producer_error.is_some() {
            break;
        }
    }
    drop(sender);
    if let Some(context) = tui_context {
        while (hdd_jobs_completed.load(Ordering::SeqCst) as usize) < total_hdd_jobs {
            check_performance_cancelled()?;
            let rate_snapshot = live_rates.snapshot()?;
            render_hdd_drain_tui_snapshot(
                writer,
                HddDrainTuiState {
                    context,
                    workload,
                    kind: PerformanceScenarioKind::DirectHdd,
                    concurrency,
                    submitted_jobs: submitted_hdd_jobs,
                    total_jobs: total_hdd_jobs,
                    started_jobs: hdd_jobs_started.load(Ordering::SeqCst) as usize,
                    completed_jobs: hdd_jobs_completed.load(Ordering::SeqCst) as usize,
                    transferred_bytes: hdd_bytes_transferred.load(Ordering::SeqCst),
                    ssd_read_rate: None,
                    hdd_write_rate: rate_snapshot.hdd_write_rate,
                    hdd_disk_rates: active_hdd_disk_rates(&active_hdd_writes)?,
                    active_hdd_landing: active_hdd_landing_lines(
                        &active_hdd_writes,
                        workload.file_count(),
                    )?,
                },
            )?;
            if handles.iter().all(|handle| handle.is_finished()) {
                break;
            }
            thread::sleep(Duration::from_millis(500));
        }
    }
    let mut worker_error = None;
    for handle in handles {
        match handle.join() {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                let _ = worker_error.get_or_insert(err);
            }
            Err(_) => {
                let _ = worker_error.get_or_insert(CliError::CommandFailed(
                    "performance-test direct HDD worker panicked".to_string(),
                ));
            }
        };
    }
    if let Some(err) = producer_error.or(worker_error) {
        return Err(err);
    }
    let io_samples = io_sampler.stop();
    let elapsed_seconds = started.elapsed().as_secs_f64().max(0.001);
    let mut disk_results = Arc::try_unwrap(worker_results)
        .map_err(|_| {
            CliError::CommandFailed("performance-test result lock still shared".to_string())
        })?
        .into_inner()
        .map_err(|_| {
            CliError::CommandFailed("performance-test result lock poisoned".to_string())
        })?;
    disk_results.sort_by(|left, right| {
        left.file_index
            .cmp(&right.file_index)
            .then_with(|| left.copy_index.cmp(&right.copy_index))
            .then_with(|| left.disk_id.cmp(&right.disk_id))
    });
    let total_bytes = disk_results.iter().map(|row| row.write.bytes).sum::<u64>();
    let slowest_seconds = disk_results
        .iter()
        .map(|row| row.write.seconds)
        .fold(0.0_f64, f64::max);
    let members = disk_results
        .iter()
        .map(|row| row.disk_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if log_progress {
        writeln!(
            writer,
            "direct-hdd c{}: aggregate landing {}/s",
            concurrency,
            format_bytes(total_bytes as f64 / elapsed_seconds)
        )?;
    }
    Ok(PerformanceScenarioResult {
        kind: PerformanceScenarioKind::DirectHdd,
        file_order: workload.file_order,
        concurrency,
        redundancy,
        queue_capacity,
        elapsed_seconds,
        total_bytes,
        logical_source_bytes: workload.total_bytes(),
        physical_hdd_write_bytes: total_bytes,
        hdd_write_operations: disk_results.len(),
        hdd_drain_started_before_all_ssd_staged: false,
        file_results: Vec::new(),
        disk_results,
        io_samples,
        concurrency_result: PerformanceConcurrencyResult {
            concurrency,
            scenario: PerformanceScenarioKind::DirectHdd,
            aggregate_bytes: total_bytes,
            seconds: elapsed_seconds,
            slowest_seconds,
            members,
        },
    })
}

#[derive(Debug)]
struct SsdPipelineJob {
    file_index: u32,
    copy_index: usize,
    relative_path: PathBuf,
    ssd_path: PathBuf,
    size_bytes: u64,
}

fn try_submit_pending_ssd_pipeline_jobs(
    sender: &mpsc::SyncSender<SsdPipelineJob>,
    pending_jobs: &mut VecDeque<SsdPipelineJob>,
    submitted_hdd_jobs: &mut usize,
) -> Result<bool, CliError> {
    let mut submitted_any = false;
    while let Some(job) = pending_jobs.pop_front() {
        match sender.try_send(job) {
            Ok(()) => {
                *submitted_hdd_jobs += 1;
                submitted_any = true;
            }
            Err(mpsc::TrySendError::Full(job)) => {
                pending_jobs.push_front(job);
                return Ok(submitted_any);
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                return Err(CliError::CommandFailed(
                    "performance-test HDD workers stopped early".to_string(),
                ));
            }
        }
    }
    Ok(submitted_any)
}

#[derive(Debug)]
struct DirectHddJob {
    payload: PerformancePayload,
    copy_index: usize,
}

type ActiveHddWriteKey = (u32, usize);
type ActiveHddWriteMap = Arc<Mutex<BTreeMap<ActiveHddWriteKey, ActiveHddWrite>>>;

#[derive(Clone, Debug, Default)]
struct PerformanceLiveRateCounters {
    inner: Arc<Mutex<PerformanceLiveRateState>>,
}

#[derive(Debug, Default)]
struct PerformanceLiveRateState {
    ssd_read: PerformanceMeasurementAccumulator,
    hdd_write: PerformanceMeasurementAccumulator,
}

#[derive(Clone, Copy, Debug, Default)]
struct PerformanceMeasurementAccumulator {
    bytes: u64,
    seconds: f64,
}

impl PerformanceLiveRateCounters {
    fn add_ssd_read_interval(&self, bytes: u64, seconds: f64) -> Result<(), CliError> {
        if bytes == 0 || seconds <= 0.0 {
            return Ok(());
        }
        let mut state = self.inner.lock().map_err(|_| {
            CliError::CommandFailed("performance-test live rate lock poisoned".to_string())
        })?;
        state.ssd_read.bytes = state.ssd_read.bytes.saturating_add(bytes);
        state.ssd_read.seconds += seconds;
        Ok(())
    }

    fn add_hdd_write_interval(
        &self,
        _disk_id: &DiskId,
        bytes: u64,
        seconds: f64,
    ) -> Result<(), CliError> {
        if seconds <= 0.0 {
            return Ok(());
        }
        let mut state = self.inner.lock().map_err(|_| {
            CliError::CommandFailed("performance-test live rate lock poisoned".to_string())
        })?;
        state.hdd_write.bytes = state.hdd_write.bytes.saturating_add(bytes);
        state.hdd_write.seconds += seconds;
        Ok(())
    }

    fn snapshot(&self) -> Result<PerformanceLiveRateSnapshot, CliError> {
        let state = self.inner.lock().map_err(|_| {
            CliError::CommandFailed("performance-test live rate lock poisoned".to_string())
        })?;
        Ok(PerformanceLiveRateSnapshot {
            ssd_read_rate: accumulated_rate(state.ssd_read),
            hdd_write_rate: accumulated_rate(state.hdd_write),
        })
    }
}

#[derive(Clone, Debug, Default)]
struct PerformanceLiveRateSnapshot {
    ssd_read_rate: Option<f64>,
    hdd_write_rate: Option<f64>,
}

fn accumulated_rate(measurement: PerformanceMeasurementAccumulator) -> Option<f64> {
    if measurement.bytes == 0 || measurement.seconds <= 0.0 {
        None
    } else {
        Some(measurement.bytes as f64 / measurement.seconds.max(0.001))
    }
}

#[derive(Clone, Debug)]
struct ActiveHddWrite {
    file_index: u32,
    copy_index: usize,
    relative_path: PathBuf,
    disk_id: DiskId,
    size_bytes: u64,
    bytes_written: u64,
    started: Instant,
    phase: PerformanceCopyProgressPhase,
}

#[derive(Clone, Debug)]
struct DiskPlacement {
    disk_id: DiskId,
    root_path: PathBuf,
}

#[derive(Clone, Debug)]
struct DiskPlacementState {
    disk_id: DiskId,
    root_path: PathBuf,
    active: usize,
    total_bytes: u64,
    available_bytes: u64,
    assigned_bytes: u64,
    completed_seconds: f64,
}

#[derive(Debug)]
struct DiskPlacementScheduler {
    disks: Vec<DiskPlacementState>,
    logical_file_disks: BTreeMap<u32, BTreeSet<DiskId>>,
}

type SharedDiskPlacementScheduler = Arc<(Mutex<DiskPlacementScheduler>, Condvar)>;

impl DiskPlacementScheduler {
    fn new(disks: &[(DiskId, PathBuf)]) -> Result<Self, CliError> {
        Ok(Self {
            disks: disks
                .iter()
                .map(|(disk_id, root_path)| {
                    fs::create_dir_all(root_path)?;
                    let capacity = measure_ssd_capacity(root_path)?;
                    Ok(DiskPlacementState {
                        disk_id: disk_id.clone(),
                        root_path: root_path.clone(),
                        active: 0,
                        total_bytes: capacity.total_bytes,
                        available_bytes: capacity.available_bytes,
                        assigned_bytes: 0,
                        completed_seconds: 0.0,
                    })
                })
                .collect::<Result<Vec<_>, CliError>>()?,
            logical_file_disks: BTreeMap::new(),
        })
    }

    fn reserve_disk_for_file(&mut self, file_index: u32) -> Option<DiskPlacement> {
        let already_assigned = self
            .logical_file_disks
            .get(&file_index)
            .cloned()
            .unwrap_or_default();
        let index = self.select_idle_disk(|disk| !already_assigned.contains(&disk.disk_id))?;
        self.reserve_disk_index(file_index, index)
    }

    fn select_idle_disk(
        &self,
        accepts_disk: impl Fn(&DiskPlacementState) -> bool,
    ) -> Option<usize> {
        self.disks
            .iter()
            .enumerate()
            .filter(|(_, disk)| disk.active == 0 && accepts_disk(disk))
            .max_by(|(_, left), (_, right)| compare_disk_free_fraction(left, right))
            .map(|(index, _)| index)
    }

    fn reserve_disk_index(&mut self, file_index: u32, index: usize) -> Option<DiskPlacement> {
        let disk = self.disks.get_mut(index)?;
        disk.active = 1;
        self.logical_file_disks
            .entry(file_index)
            .or_default()
            .insert(disk.disk_id.clone());
        Some(DiskPlacement {
            disk_id: disk.disk_id.clone(),
            root_path: disk.root_path.clone(),
        })
    }

    fn complete_disk(&mut self, disk_id: &DiskId, bytes: u64, seconds: f64) {
        if let Some(disk) = self.disks.iter_mut().find(|disk| &disk.disk_id == disk_id) {
            disk.active = disk.active.saturating_sub(1);
            disk.assigned_bytes = disk.assigned_bytes.saturating_add(bytes);
            disk.completed_seconds += seconds.max(0.0);
        }
    }
}

fn new_shared_disk_placement_scheduler(
    disks: &[(DiskId, PathBuf)],
) -> Result<SharedDiskPlacementScheduler, CliError> {
    Ok(Arc::new((
        Mutex::new(DiskPlacementScheduler::new(disks)?),
        Condvar::new(),
    )))
}

fn reserve_performance_disk_for_file(
    scheduler: &SharedDiskPlacementScheduler,
    file_index: u32,
) -> Result<DiskPlacement, CliError> {
    let (lock, condvar) = &**scheduler;
    let mut scheduler = lock.lock().map_err(|_| {
        CliError::CommandFailed("performance-test disk scheduler lock poisoned".to_string())
    })?;
    loop {
        check_performance_cancelled()?;
        if let Some(placement) = scheduler.reserve_disk_for_file(file_index) {
            return Ok(placement);
        }
        let result = condvar
            .wait_timeout(scheduler, Duration::from_millis(250))
            .map_err(|_| {
                CliError::CommandFailed("performance-test disk scheduler lock poisoned".to_string())
            })?;
        scheduler = result.0;
    }
}

fn complete_performance_disk(
    scheduler: &SharedDiskPlacementScheduler,
    disk_id: &DiskId,
    bytes: u64,
    seconds: f64,
) -> Result<(), CliError> {
    let (lock, condvar) = &**scheduler;
    let mut scheduler = lock.lock().map_err(|_| {
        CliError::CommandFailed("performance-test disk scheduler lock poisoned".to_string())
    })?;
    scheduler.complete_disk(disk_id, bytes, seconds);
    condvar.notify_one();
    Ok(())
}

fn compare_disk_free_fraction(
    left: &DiskPlacementState,
    right: &DiskPlacementState,
) -> std::cmp::Ordering {
    let left_free = left.available_bytes.saturating_sub(left.assigned_bytes);
    let right_free = right.available_bytes.saturating_sub(right.assigned_bytes);
    (u128::from(left_free) * u128::from(right.total_bytes.max(1)))
        .cmp(&(u128::from(right_free) * u128::from(left.total_bytes.max(1))))
        .then_with(|| left_free.cmp(&right_free))
        .then_with(|| right.completed_seconds.total_cmp(&left.completed_seconds))
        .then_with(|| right.disk_id.cmp(&left.disk_id))
}

fn hdd_queue_capacity(concurrency: usize, redundancy: usize) -> usize {
    concurrency
        .saturating_mul(redundancy)
        .saturating_mul(2)
        .clamp(1, 64)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PerformanceSsdResidencyBudget {
    safe_bytes: u64,
    available_bytes: u64,
}

fn performance_ssd_residency_budget(
    ssd_root: &Path,
) -> Result<PerformanceSsdResidencyBudget, CliError> {
    fs::create_dir_all(ssd_root)?;
    let capacity = measure_ssd_capacity(ssd_root)?;
    let policy = SsdCapacityPolicy::default();
    let high_watermark_used_bytes = ((u128::from(capacity.total_bytes)
        * u128::from(policy.high_watermark_percent))
        / 100) as u64;
    let high_watermark_headroom = high_watermark_used_bytes.saturating_sub(capacity.used_bytes());
    let minimum_free_headroom = capacity
        .available_bytes
        .saturating_sub(policy.minimum_free_bytes);
    let safe_bytes = high_watermark_headroom.min(minimum_free_headroom);
    Ok(PerformanceSsdResidencyBudget {
        safe_bytes,
        available_bytes: capacity.available_bytes,
    })
}

fn plan_ssd_residency_batches(
    workload: &PerformanceWorkload,
    budget: PerformanceSsdResidencyBudget,
) -> Result<Vec<Vec<PerformancePayload>>, CliError> {
    if workload.payloads.is_empty() {
        return Ok(Vec::new());
    }
    let mut batches = Vec::<Vec<PerformancePayload>>::new();
    let mut current = Vec::<PerformancePayload>::new();
    let mut current_bytes = 0_u64;

    for payload in &workload.payloads {
        let payload_bytes = payload.size_bytes;
        if payload_bytes > budget.available_bytes {
            return Err(CliError::CommandFailed(format!(
                "performance-test payload {} ({}) is larger than available SSD space ({})",
                payload.relative_path.display(),
                format_bytes(payload_bytes as f64),
                format_bytes(budget.available_bytes as f64)
            )));
        }
        let payload_fits_safe_batch = payload_bytes <= budget.safe_bytes;
        let effective_budget = if payload_fits_safe_batch {
            budget.safe_bytes
        } else {
            payload_bytes
        };
        if !current.is_empty() && current_bytes.saturating_add(payload_bytes) > effective_budget {
            batches.push(std::mem::take(&mut current));
            current_bytes = 0;
        }
        current.push(payload.clone());
        current_bytes = current_bytes.saturating_add(payload_bytes);
    }

    if !current.is_empty() {
        batches.push(current);
    }
    Ok(batches)
}

fn validate_performance_payload_fits_ssd(
    payload: &PerformancePayload,
    budget: PerformanceSsdResidencyBudget,
) -> Result<(), CliError> {
    if payload.size_bytes > budget.available_bytes {
        return Err(CliError::CommandFailed(format!(
            "performance-test payload {} ({}) is larger than available SSD space ({})",
            payload.relative_path.display(),
            format_bytes(payload.size_bytes as f64),
            format_bytes(budget.available_bytes as f64)
        )));
    }
    Ok(())
}

fn performance_ssd_can_admit_payload(
    resident_bytes: u64,
    payload_bytes: u64,
    budget: PerformanceSsdResidencyBudget,
) -> bool {
    resident_bytes.saturating_add(payload_bytes) <= budget.safe_bytes
        || (resident_bytes == 0 && payload_bytes <= budget.available_bytes)
}

fn measure_generate_random_file_with_progress(
    path: &Path,
    size_bytes: u64,
    seed: u32,
    mut progress: Option<&mut dyn FnMut(u64, f64) -> Result<(), CliError>>,
    sync_policy: PerformanceCopySyncPolicy<'_>,
) -> Result<PerformanceMeasurement, CliError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let started = Instant::now();
    let mut file = File::create(path)?;
    let mut remaining = size_bytes;
    let mut written = 0_u64;
    let progress_step = performance_progress_step(size_bytes);
    let mut next_progress = progress_step.min(size_bytes);
    let mut buffer = vec![0_u8; 1024 * 1024];
    let mut state = 0x9e37_79b9_7f4a_7c15_u64 ^ u64::from(seed);
    while remaining > 0 {
        check_performance_cancelled()?;
        fill_pseudorandom(&mut buffer, &mut state);
        let write_len = remaining.min(buffer.len() as u64) as usize;
        file.write_all(&buffer[..write_len])?;
        remaining -= write_len as u64;
        written = written.saturating_add(write_len as u64);
        if let Some(callback) = progress.as_deref_mut() {
            if written >= next_progress || written == size_bytes {
                callback(written, started.elapsed().as_secs_f64().max(0.001))?;
                next_progress = written.saturating_add(progress_step).min(size_bytes);
            }
        }
    }
    check_performance_cancelled()?;
    match sync_policy {
        PerformanceCopySyncPolicy::SyncAll => {
            performance_sync_all(&file)?;
        }
        PerformanceCopySyncPolicy::AsyncSsdSettle(settler) => {
            settler.submit(path.to_path_buf(), file)?;
        }
    }
    if let Some(callback) = progress.as_deref_mut() {
        callback(size_bytes, started.elapsed().as_secs_f64().max(0.001))?;
    }
    Ok(PerformanceMeasurement {
        bytes: size_bytes,
        seconds: started.elapsed().as_secs_f64().max(0.001),
    })
}

fn measure_ssd_stage_payload(
    payload: &PerformancePayload,
    destination: &Path,
    settler: &PerformanceSsdSettler,
) -> Result<PerformanceMeasurement, CliError> {
    measure_ssd_stage_payload_with_progress(payload, destination, payload.file_index, None, settler)
}

fn measure_ssd_stage_payload_with_progress(
    payload: &PerformancePayload,
    destination: &Path,
    seed: u32,
    progress: Option<&mut dyn FnMut(u64, f64) -> Result<(), CliError>>,
    settler: &PerformanceSsdSettler,
) -> Result<PerformanceMeasurement, CliError> {
    measure_land_payload_with_progress_and_sync_policy(
        payload,
        destination,
        seed,
        progress,
        PerformanceCopySyncPolicy::AsyncSsdSettle(settler),
    )
}

fn measure_land_payload_with_progress_and_sync_policy(
    payload: &PerformancePayload,
    destination: &Path,
    seed: u32,
    progress: Option<&mut dyn FnMut(u64, f64) -> Result<(), CliError>>,
    sync_policy: PerformanceCopySyncPolicy<'_>,
) -> Result<PerformanceMeasurement, CliError> {
    if let Some(source) = &payload.source_path {
        measure_copy_with_progress_and_sync_policy(source, destination, progress, sync_policy)
    } else {
        measure_generate_random_file_with_progress(
            destination,
            payload.size_bytes,
            seed,
            progress,
            sync_policy,
        )
    }
}

#[derive(Clone, Copy)]
enum PerformanceCopySyncPolicy<'a> {
    SyncAll,
    AsyncSsdSettle(&'a PerformanceSsdSettler),
}

fn fill_pseudorandom(buffer: &mut [u8], state: &mut u64) {
    for chunk in buffer.chunks_mut(8) {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        let bytes = state.to_le_bytes();
        chunk.copy_from_slice(&bytes[..chunk.len()]);
    }
}

#[cfg(test)]
fn measure_copy_with_progress(
    source: &Path,
    destination: &Path,
    progress: Option<&mut dyn FnMut(u64, f64) -> Result<(), CliError>>,
) -> Result<PerformanceMeasurement, CliError> {
    measure_copy_with_progress_and_sync_policy(
        source,
        destination,
        progress,
        PerformanceCopySyncPolicy::SyncAll,
    )
}

#[derive(Clone, Copy, Debug)]
struct PerformanceSplitCopyProgress {
    bytes: u64,
    source_read_seconds: f64,
    destination_write_seconds: f64,
    phase: PerformanceCopyProgressPhase,
}

#[derive(Clone, Copy, Debug)]
struct PerformanceSplitCopyMeasurement {
    source_read: PerformanceMeasurement,
    destination_write: PerformanceMeasurement,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PerformanceCopyProgressPhase {
    Copying,
    Syncing,
}

fn measure_copy_with_split_progress(
    source: &Path,
    destination: &Path,
    mut progress: Option<&mut dyn FnMut(PerformanceSplitCopyProgress) -> Result<(), CliError>>,
) -> Result<PerformanceSplitCopyMeasurement, CliError> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut reader = File::open(source)?;
    let mut writer = File::create(destination)?;
    let mut bytes = 0_u64;
    let mut source_read_seconds = 0.0_f64;
    let mut destination_write_seconds = 0.0_f64;
    let total_bytes = source
        .metadata()
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let progress_step = performance_progress_step(total_bytes);
    let mut next_progress = progress_step.min(total_bytes);
    let mut last_progress_emit = Instant::now();
    let mut buffer = vec![0_u8; 4 * 1024 * 1024];
    if let Some(callback) = progress.as_deref_mut() {
        callback(PerformanceSplitCopyProgress {
            bytes,
            source_read_seconds,
            destination_write_seconds,
            phase: PerformanceCopyProgressPhase::Copying,
        })?;
    }
    loop {
        check_performance_cancelled()?;
        let read_started = Instant::now();
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        source_read_seconds += read_started.elapsed().as_secs_f64();
        let write_started = Instant::now();
        writer.write_all(&buffer[..read])?;
        destination_write_seconds += write_started.elapsed().as_secs_f64();
        bytes = bytes.saturating_add(read as u64);
        if let Some(callback) = progress.as_deref_mut() {
            if bytes >= next_progress
                || bytes == total_bytes
                || last_progress_emit.elapsed() >= Duration::from_secs(1)
            {
                callback(PerformanceSplitCopyProgress {
                    bytes,
                    source_read_seconds,
                    destination_write_seconds,
                    phase: PerformanceCopyProgressPhase::Copying,
                })?;
                last_progress_emit = Instant::now();
                if bytes >= next_progress {
                    next_progress = bytes.saturating_add(progress_step).min(total_bytes);
                }
            }
        }
    }
    check_performance_cancelled()?;
    let sync_started = Instant::now();
    if let Some(callback) = progress.as_deref_mut() {
        callback(PerformanceSplitCopyProgress {
            bytes,
            source_read_seconds,
            destination_write_seconds,
            phase: PerformanceCopyProgressPhase::Syncing,
        })?;
        performance_sync_all_with_heartbeat(&writer, || {
            callback(PerformanceSplitCopyProgress {
                bytes,
                source_read_seconds,
                destination_write_seconds: destination_write_seconds
                    + sync_started.elapsed().as_secs_f64(),
                phase: PerformanceCopyProgressPhase::Syncing,
            })
        })?;
    } else {
        performance_sync_all(&writer)?;
    }
    destination_write_seconds += sync_started.elapsed().as_secs_f64();
    if let Some(callback) = progress.as_deref_mut() {
        callback(PerformanceSplitCopyProgress {
            bytes,
            source_read_seconds,
            destination_write_seconds,
            phase: PerformanceCopyProgressPhase::Syncing,
        })?;
    }
    Ok(PerformanceSplitCopyMeasurement {
        source_read: PerformanceMeasurement {
            bytes,
            seconds: source_read_seconds.max(0.001),
        },
        destination_write: PerformanceMeasurement {
            bytes,
            seconds: destination_write_seconds.max(0.001),
        },
    })
}

fn measure_copy_with_progress_and_sync_policy(
    source: &Path,
    destination: &Path,
    mut progress: Option<&mut dyn FnMut(u64, f64) -> Result<(), CliError>>,
    sync_policy: PerformanceCopySyncPolicy<'_>,
) -> Result<PerformanceMeasurement, CliError> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    let started = Instant::now();
    let mut reader = File::open(source)?;
    let mut writer = File::create(destination)?;
    let mut bytes = 0_u64;
    let total_bytes = source
        .metadata()
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let progress_step = performance_progress_step(total_bytes);
    let mut next_progress = progress_step.min(total_bytes);
    let mut buffer = vec![0_u8; 4 * 1024 * 1024];
    loop {
        check_performance_cancelled()?;
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        writer.write_all(&buffer[..read])?;
        bytes = bytes.saturating_add(read as u64);
        if let Some(callback) = progress.as_deref_mut() {
            if bytes >= next_progress || bytes == total_bytes {
                callback(bytes, started.elapsed().as_secs_f64().max(0.001))?;
                next_progress = bytes.saturating_add(progress_step).min(total_bytes);
            }
        }
    }
    check_performance_cancelled()?;
    match sync_policy {
        PerformanceCopySyncPolicy::SyncAll => {
            performance_sync_all(&writer)?;
        }
        PerformanceCopySyncPolicy::AsyncSsdSettle(settler) => {
            settler.submit(destination.to_path_buf(), writer)?;
        }
    }
    if let Some(callback) = progress.as_deref_mut() {
        callback(bytes, started.elapsed().as_secs_f64().max(0.001))?;
    }
    Ok(PerformanceMeasurement {
        bytes,
        seconds: started.elapsed().as_secs_f64().max(0.001),
    })
}

fn performance_sync_all(file: &File) -> io::Result<()> {
    #[cfg(test)]
    PERFORMANCE_SYNC_ALL_CALLS.with(|calls| {
        *calls.borrow_mut() += 1;
    });
    file.sync_all()
}

#[cfg(not(test))]
fn performance_sync_all_with_heartbeat(
    file: &File,
    mut heartbeat: impl FnMut() -> Result<(), CliError>,
) -> Result<(), CliError> {
    let sync_file = file.try_clone()?;
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let _ = sender.send(performance_sync_all(&sync_file));
    });

    loop {
        match receiver.recv_timeout(Duration::from_secs(1)) {
            Ok(result) => return result.map_err(CliError::from),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                heartbeat()?;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(CliError::CommandFailed(
                    "performance-test sync worker stopped before reporting completion".to_string(),
                ));
            }
        }
    }
}

#[cfg(test)]
fn performance_sync_all_with_heartbeat(
    file: &File,
    mut heartbeat: impl FnMut() -> Result<(), CliError>,
) -> Result<(), CliError> {
    heartbeat()?;
    performance_sync_all(file).map_err(CliError::from)
}

#[cfg(test)]
fn reset_performance_sync_all_calls() {
    PERFORMANCE_SYNC_ALL_CALLS.with(|calls| {
        *calls.borrow_mut() = 0;
    });
}

#[cfg(test)]
fn performance_sync_all_calls() -> u32 {
    PERFORMANCE_SYNC_ALL_CALLS.with(|calls| *calls.borrow())
}

fn measure_read(source: &Path) -> Result<PerformanceMeasurement, CliError> {
    measure_read_with_progress(source, None)
}

fn measure_read_with_progress(
    source: &Path,
    mut progress: Option<&mut dyn FnMut(u64, f64) -> Result<(), CliError>>,
) -> Result<PerformanceMeasurement, CliError> {
    let started = Instant::now();
    let mut reader = File::open(source)?;
    let mut bytes = 0_u64;
    let total_bytes = source
        .metadata()
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    let progress_step = performance_progress_step(total_bytes);
    let mut next_progress = progress_step.min(total_bytes);
    let mut buffer = vec![0_u8; 4 * 1024 * 1024];
    loop {
        check_performance_cancelled()?;
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        bytes = bytes.saturating_add(read as u64);
        if let Some(callback) = progress.as_deref_mut() {
            if bytes >= next_progress || bytes == total_bytes {
                callback(bytes, started.elapsed().as_secs_f64().max(0.001))?;
                next_progress = bytes.saturating_add(progress_step).min(total_bytes);
            }
        }
    }
    if let Some(callback) = progress.as_deref_mut() {
        callback(bytes, started.elapsed().as_secs_f64().max(0.001))?;
    }
    Ok(PerformanceMeasurement {
        bytes,
        seconds: started.elapsed().as_secs_f64().max(0.001),
    })
}

fn performance_progress_step(total_bytes: u64) -> u64 {
    const MIN_STEP: u64 = 64 * 1024 * 1024;
    const MAX_STEP: u64 = 512 * 1024 * 1024;
    if total_bytes == 0 {
        return 1;
    }
    (total_bytes / 100)
        .clamp(MIN_STEP, MAX_STEP)
        .min(total_bytes)
}

fn timestamped_run_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("dasobjectstore-performance-{nanos}-{}", std::process::id())
}

fn git_revision() -> String {
    let revision = ProcessCommand::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                String::from_utf8(output.stdout).ok()
            } else {
                None
            }
        })
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    let dirty = ProcessCommand::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                Some(!output.stdout.is_empty())
            } else {
                None
            }
        })
        .unwrap_or(false);
    if dirty && revision != "unknown" {
        format!("{revision}-dirty")
    } else {
        revision
    }
}

fn write_report_qr_svg(path: &Path, payload: &str) -> Result<String, CliError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if ProcessCommand::new("qrencode")
        .args(["-t", "SVG", "-o"])
        .arg(path)
        .arg(payload)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
    {
        return Ok("qrencode SVG".to_string());
    }
    fs::write(path, fallback_qr_svg(payload))?;
    Ok("fallback SVG; install qrencode for a scan-ready QR code".to_string())
}

fn fallback_qr_svg(payload: &str) -> String {
    let mut state = 0xcbf2_9ce4_8422_2325_u64;
    for byte in payload.as_bytes() {
        state ^= u64::from(*byte);
        state = state.wrapping_mul(0x100_0000_01b3);
    }
    let cells = 29_usize;
    let scale = 6_usize;
    let size = cells * scale;
    let mut svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 {size} {size}"><rect width="100%" height="100%" fill="white"/>"#
    );
    for y in 0..cells {
        for x in 0..cells {
            let finder = (x < 7 && (y < 7 || y >= cells - 7)) || (x >= cells - 7 && y < 7);
            let on = if finder {
                x == 0
                    || x == 6
                    || y == 0
                    || y == 6
                    || (x >= 2 && x <= 4 && y >= 2 && y <= 4)
                    || (x >= cells - 5 && x <= cells - 3 && y >= 2 && y <= 4)
                    || (x >= 2 && x <= 4 && y >= cells - 5 && y <= cells - 3)
            } else {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                ((state >> 63) & 1) == 1
            };
            if on {
                svg.push_str(&format!(
                    r#"<rect x="{}" y="{}" width="{scale}" height="{scale}" fill="black"/>"#,
                    x * scale,
                    y * scale
                ));
            }
        }
    }
    svg.push_str("</svg>\n");
    svg
}

fn write_pdf_report(
    markdown_path: &Path,
    pdf_path: &Path,
    report: &PerformanceReport,
) -> Result<(), CliError> {
    if let Some(parent) = pdf_path.parent() {
        fs::create_dir_all(parent)?;
    }
    write_formal_performance_pdf_report(markdown_path, pdf_path, report)
}

fn write_formal_performance_pdf_report(
    markdown_path: &Path,
    pdf_path: &Path,
    report: &PerformanceReport,
) -> Result<(), CliError> {
    let metadata_json = performance_report_metadata_json(report);
    let status = ProcessCommand::new("gnostikon-workflow-control")
        .arg("render-report-pdf")
        .arg("--provider")
        .arg("container")
        .arg("--input")
        .arg(markdown_path)
        .arg("--output")
        .arg(pdf_path)
        .arg("--title")
        .arg("DASObjectStore Performance Test Report")
        .arg("--title-explanation")
        .arg("Reproducible DAS performance evidence for SSD staging, drain-time SSD reads, and concurrent HDD settlement planning.")
        .arg("--metadata-json")
        .arg(&metadata_json)
        .arg("--provenance-qr-payload")
        .arg(performance_report_qr_payload(report))
        .arg("--report-template")
        .arg("dasobjectstore-performance")
        .arg("--footer-label")
        .arg("DASObjectStore performance")
        .arg("--generated-at-utc")
        .arg(&report.generated_at_utc)
        .status();
    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(CliError::CommandFailed(format!(
            "formal performance PDF rendering failed with status {status}; install/repair the gnostikon-workflow-control package, Docker/container runtime, and the Grammateus report provider, then rebuild with `dasobjectstore performance-report --json-artifact {}`",
            report.json_path.display()
        ))),
        Err(error) => Err(CliError::CommandFailed(format!(
            "formal performance PDF rendering requires the gnostikon-workflow-control package with Grammateus support plus a Docker/container runtime: {error}; rebuild later with `dasobjectstore performance-report --json-artifact {}`",
            report.json_path.display()
        ))),
    }
}

fn write_formal_performance_pdf_report_from_artifact(
    markdown_path: &Path,
    pdf_path: &Path,
    artifact: &Value,
) -> Result<(), CliError> {
    let metadata_json = performance_report_metadata_json_from_artifact(artifact);
    let generated_at =
        json_string(artifact, &["run", "generated_at_utc"]).unwrap_or_else(|| now_utc_string());
    let qr_payload = performance_report_qr_payload_from_artifact(artifact);
    let status = ProcessCommand::new("gnostikon-workflow-control")
        .arg("render-report-pdf")
        .arg("--provider")
        .arg("container")
        .arg("--input")
        .arg(markdown_path)
        .arg("--output")
        .arg(pdf_path)
        .arg("--title")
        .arg("DASObjectStore Performance Test Report")
        .arg("--title-explanation")
        .arg("Reproducible DAS performance evidence for SSD staging, drain-time SSD reads, and concurrent HDD settlement planning.")
        .arg("--metadata-json")
        .arg(metadata_json)
        .arg("--provenance-qr-payload")
        .arg(qr_payload)
        .arg("--report-template")
        .arg("dasobjectstore-performance")
        .arg("--footer-label")
        .arg("DASObjectStore performance")
        .arg("--generated-at-utc")
        .arg(generated_at)
        .status();
    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(CliError::CommandFailed(format!(
            "formal performance PDF rendering failed with status {status}; install/repair the gnostikon-workflow-control package, Docker/container runtime, and the Grammateus report provider"
        ))),
        Err(error) => Err(CliError::CommandFailed(format!(
            "formal performance PDF rendering requires the gnostikon-workflow-control package with Grammateus support plus a Docker/container runtime: {error}"
        ))),
    }
}

fn performance_report_metadata_json(report: &PerformanceReport) -> String {
    let document_id = format!("DOS-{}", report.run_id);
    serde_json::json!({
        "header": "DASObjectStore performance report",
        "rows": [
            [
                {"label": "Document ID", "value": document_id},
                {"label": "Performance test identifier", "value": "DASObjectStore-Disk-Speed"},
                {"label": "Version", "value": dasobjectstore_core::VERSION},
                {"label": "Report state", "value": "FINAL"},
            ],
            [
                {"label": "DeviceID", "value": hostname_for_report()},
                {"label": "Operator", "value": std::env::var("USER").unwrap_or_else(|_| "not recorded".to_string())},
                {"label": "Timestamp", "value": report.generated_at_utc},
                {"label": "Run ID", "value": report.run_id},
            ],
            [
                {"label": "Repository revision", "value": report.repository_revision},
                {"label": "Test status", "value": "VALID"},
                {"label": "Signature of operator", "value": "Pending operator signature"},
                {"label": "Cryptographic signature", "value": report.reproduction_payload_sha256},
            ],
        ],
    })
    .to_string()
}

fn performance_report_qr_payload(report: &PerformanceReport) -> String {
    format!(
        "mnemosyne-report:DASObjectStore:{}:{}",
        report.run_id, report.reproduction_payload_sha256
    )
}

fn read_performance_json_artifact(path: &Path) -> Result<Value, CliError> {
    let artifact = fs::read_to_string(path)?;
    let artifact = serde_json::from_str::<Value>(&artifact).map_err(|error| {
        CliError::CommandFailed(format!(
            "could not parse performance JSON artifact {}: {error}",
            path.display()
        ))
    })?;
    let schema = json_string(&artifact, &["schema"]).unwrap_or_default();
    if schema != "dasobjectstore.performance_test.recommendation.v1" {
        return Err(CliError::CommandFailed(format!(
            "unsupported performance JSON schema '{}'; expected dasobjectstore.performance_test.recommendation.v1",
            schema
        )));
    }
    Ok(artifact)
}

fn artifact_pdf_path(artifact: &Value) -> Option<PathBuf> {
    json_string(artifact, &["run", "artifacts", "pdf_path"]).map(PathBuf::from)
}

fn performance_report_metadata_json_from_artifact(artifact: &Value) -> String {
    let run_id =
        json_string(artifact, &["run", "run_id"]).unwrap_or_else(|| "not recorded".to_string());
    let generated_at = json_string(artifact, &["run", "generated_at_utc"])
        .unwrap_or_else(|| "not recorded".to_string());
    let revision = json_string(artifact, &["run", "repository_revision"])
        .unwrap_or_else(|| "not recorded".to_string());
    let version = json_string(artifact, &["run", "cli_version"])
        .unwrap_or_else(|| dasobjectstore_core::VERSION.to_string());
    let signature = performance_artifact_signature(artifact);
    serde_json::json!({
        "header": "DASObjectStore performance report",
        "rows": [
            [
                {"label": "Document ID", "value": format!("DOS-{run_id}")},
                {"label": "Performance test identifier", "value": "DASObjectStore-Disk-Speed"},
                {"label": "Version", "value": version},
                {"label": "Report state", "value": "FINAL"},
            ],
            [
                {"label": "DeviceID", "value": hostname_for_report()},
                {"label": "Operator", "value": std::env::var("USER").unwrap_or_else(|_| "not recorded".to_string())},
                {"label": "Timestamp", "value": generated_at},
                {"label": "Run ID", "value": run_id},
            ],
            [
                {"label": "Repository revision", "value": revision},
                {"label": "Test status", "value": "VALID"},
                {"label": "Signature of operator", "value": "Pending operator signature"},
                {"label": "Cryptographic signature", "value": signature},
            ],
        ],
    })
    .to_string()
}

fn performance_artifact_signature(artifact: &Value) -> String {
    let canonical = serde_json::to_vec(artifact).unwrap_or_default();
    sha256_hex_bytes(&canonical)
}

fn performance_report_qr_payload_from_artifact(artifact: &Value) -> String {
    let run_id = json_string(artifact, &["run", "run_id"]).unwrap_or_else(|| "unknown".to_string());
    let signature = performance_artifact_signature(artifact);
    format!("mnemosyne-report:DASObjectStore:{run_id}:{signature}")
}

fn render_performance_report_from_json_artifact(artifact: &Value, report_path: &Path) -> String {
    let run_id =
        json_string(artifact, &["run", "run_id"]).unwrap_or_else(|| "not recorded".to_string());
    let generated_at = json_string(artifact, &["run", "generated_at_utc"])
        .unwrap_or_else(|| "not recorded".to_string());
    let command = json_array_strings(artifact, &["run", "command"]).join(" ");
    let mut output = String::new();
    output.push_str("# DASObjectStore Performance Test Report\n\n");
    output.push_str("## Executive Summary\n\n");
    output.push_str(&format!("- Run ID: `{run_id}`\n"));
    output.push_str(&format!("- Generated at: `{generated_at}`\n"));
    output.push_str(&format!("- Report artifact: `{}`\n", report_path.display()));
    output.push_str(&format!(
        "- JSON artifact: `{}`\n",
        json_string(artifact, &["run", "artifacts", "json_path"])
            .unwrap_or_else(|| "not recorded".to_string())
    ));
    if !command.is_empty() {
        output.push_str(&format!("- Reproduction command: `{command}`\n"));
    }
    output.push('\n');

    output.push_str("## Recommendation\n\n");
    output.push_str("| Field | Value |\n| --- | --- |\n");
    output.push_str(&format!(
        "| Strategy | `{}` |\n",
        json_string(artifact, &["recommendation", "strategy"])
            .unwrap_or_else(|| "not recorded".to_string())
    ));
    output.push_str(&format!(
        "| File order | `{}` |\n",
        json_string(artifact, &["recommendation", "file_order"])
            .unwrap_or_else(|| "not recorded".to_string())
    ));
    output.push_str(&format!(
        "| HDD concurrency | {} |\n",
        json_u64(artifact, &["recommendation", "hdd_concurrency"])
            .map(|value| value.to_string())
            .unwrap_or_else(|| "not recorded".to_string())
    ));
    output.push_str(&format!(
        "| Redundancy | {} |\n",
        json_u64(artifact, &["recommendation", "redundancy"])
            .map(|value| value.to_string())
            .unwrap_or_else(|| "not recorded".to_string())
    ));
    output.push_str(&format!(
        "| Estimated aggregate | {}/s |\n\n",
        json_u64(
            artifact,
            &["recommendation", "estimated_aggregate_bytes_per_second"]
        )
        .map(|value| format_bytes(value as f64))
        .unwrap_or_else(|| "not recorded".to_string())
    ));
    if let Some(rows) = json_array(artifact, &["recommendation", "rationale"]) {
        for row in rows.iter().filter_map(Value::as_str) {
            output.push_str(&format!("- {row}\n"));
        }
        output.push('\n');
    }

    output.push_str("## Workload and Hardware\n\n");
    output.push_str("| Field | Value |\n| --- | --- |\n");
    for (label, path) in [
        ("Workload kind", &["run", "parameters", "workload_kind"][..]),
        ("Source path", &["run", "parameters", "source_path"][..]),
        (
            "File selection",
            &["run", "parameters", "file_selection"][..],
        ),
        ("SSD root", &["hardware", "roots", "ssd_root"][..]),
        ("HDD root", &["hardware", "roots", "hdd_root"][..]),
    ] {
        output.push_str(&format!(
            "| {label} | `{}` |\n",
            json_string(artifact, path).unwrap_or_else(|| "not recorded".to_string())
        ));
    }
    output.push_str(&format!(
        "| File orders | `{}` |\n",
        json_array_strings(artifact, &["run", "parameters", "file_orders"]).join("`, `")
    ));
    output.push_str(&format!(
        "| Planned files | {} |\n",
        json_u64(artifact, &["run", "parameters", "file_count"])
            .map(|value| value.to_string())
            .unwrap_or_else(|| "not recorded".to_string())
    ));
    output.push_str(&format!(
        "| Logical source bytes | {} |\n\n",
        json_u64(artifact, &["run", "parameters", "total_source_bytes"])
            .map(|value| format_bytes(value as f64))
            .unwrap_or_else(|| "not recorded".to_string())
    ));

    output.push_str("## Scenario Summary\n\n");
    output.push_str("| Scenario | File order | HDD concurrency | Redundancy | Logical source | Physical HDD writes | Operations | Aggregate landing | Overlapped SSD staging |\n");
    output.push_str("| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |\n");
    for row in performance_scenario_rows_from_json(artifact) {
        output.push_str(&format!(
            "| {} | `{}` | {} | {} | {} | {} | {} | {}/s | {} |\n",
            row.scenario,
            row.file_order,
            row.hdd_concurrency,
            row.redundancy,
            format_bytes(row.logical_source_bytes as f64),
            format_bytes(row.physical_hdd_write_bytes as f64),
            row.operations,
            format_bytes(row.aggregate_bytes_per_second as f64),
            yes_no(row.overlapped)
        ));
    }

    output.push_str("\n## Per-Disk HDD Write Rates\n\n");
    output.push_str(
        "| Scenario | File order | HDD concurrency | Disk | Assigned | Write rate | Operations |\n",
    );
    output.push_str("| --- | --- | ---: | --- | ---: | ---: | ---: |\n");
    for row in json_array(artifact, &["plot_data", "per_disk_hdd_write_rate"])
        .into_iter()
        .flatten()
    {
        output.push_str(&format!(
            "| {} | `{}` | {} | {} | {} GiB | {} MiB/s | {} |\n",
            json_string(row, &["scenario_label"]).unwrap_or_else(|| json_string(
                row,
                &["scenario"]
            )
            .unwrap_or_else(|| "unknown".to_string())),
            json_string(row, &["file_order"]).unwrap_or_else(|| "not recorded".to_string()),
            json_u64(row, &["hdd_concurrency"]).unwrap_or_default(),
            json_string(row, &["disk_id"]).unwrap_or_else(|| "unknown".to_string()),
            json_f64(row, &["assigned_gib"]).unwrap_or_default(),
            json_f64(row, &["write_mib_per_second"]).unwrap_or_default(),
            json_u64(row, &["write_operations"]).unwrap_or_default(),
        ));
    }

    output.push_str("\n## Figures\n\n");
    for artifact in performance_chart_artifacts_from_pdf_path(report_path) {
        output.push_str(&format!(
            "![{}]({})\n\n",
            artifact.title,
            artifact.path.display()
        ));
    }
    for artifact in performance_io_chart_artifacts_from_json(artifact, report_path) {
        output.push_str(&format!(
            "![{}]({})\n\n",
            artifact.title,
            artifact.path.display()
        ));
    }

    output.push_str("## Reproduction Payload\n\n");
    output.push_str("```json\n");
    output.push_str(&serde_json::to_string_pretty(artifact).unwrap_or_else(|_| "{}".to_string()));
    output.push_str("\n```\n");
    output
}

#[derive(Debug)]
struct JsonScenarioRow {
    scenario: String,
    file_order: String,
    hdd_concurrency: u64,
    redundancy: u64,
    logical_source_bytes: u64,
    physical_hdd_write_bytes: u64,
    operations: u64,
    aggregate_bytes_per_second: u64,
    overlapped: bool,
}

fn performance_scenario_rows_from_json(artifact: &Value) -> Vec<JsonScenarioRow> {
    let mut rows = Vec::new();
    for order in json_array(artifact, &["scenarios", "ssd_only", "orders"])
        .into_iter()
        .flatten()
    {
        rows.push(JsonScenarioRow {
            scenario: "SSD only".to_string(),
            file_order: json_string(order, &["file_order"])
                .unwrap_or_else(|| "not recorded".to_string()),
            hdd_concurrency: 0,
            redundancy: 0,
            logical_source_bytes: json_u64(artifact, &["scenarios", "ssd_only", "total_bytes"])
                .unwrap_or_default(),
            physical_hdd_write_bytes: 0,
            operations: 0,
            aggregate_bytes_per_second: json_u64(order, &["median_ssd_write_bytes_per_second"])
                .unwrap_or_default(),
            overlapped: false,
        });
    }
    for path in [
        &["scenarios", "ssd_stage_then_drain_pipeline", "concurrency"][..],
        &["scenarios", "ssd_hdd_pipeline", "concurrency"][..],
        &["scenarios", "direct_hdd_pipeline", "concurrency"][..],
    ] {
        for row in json_array(artifact, path).into_iter().flatten() {
            rows.push(JsonScenarioRow {
                scenario: json_string(row, &["scenario"]).unwrap_or_else(|| "unknown".to_string()),
                file_order: json_string(row, &["file_order"])
                    .unwrap_or_else(|| "not recorded".to_string()),
                hdd_concurrency: json_u64(row, &["concurrency"]).unwrap_or_default(),
                redundancy: json_u64(row, &["redundancy"]).unwrap_or_default(),
                logical_source_bytes: json_u64(row, &["logical_source_bytes"]).unwrap_or_default(),
                physical_hdd_write_bytes: json_u64(row, &["physical_hdd_write_bytes"])
                    .unwrap_or_default(),
                operations: json_u64(row, &["hdd_write_operations"]).unwrap_or_default(),
                aggregate_bytes_per_second: json_u64(row, &["aggregate_write_bytes_per_second"])
                    .unwrap_or_default(),
                overlapped: json_bool(row, &["hdd_drain_started_before_all_ssd_staged"])
                    .unwrap_or(false),
            });
        }
    }
    rows
}

fn write_performance_chart_svgs_from_json(
    artifact: &Value,
    pdf_path: &Path,
) -> Result<(), CliError> {
    let artifacts = performance_chart_artifacts_from_pdf_path(pdf_path);
    let chart_specs = [
        ("landing_rate_by_strategy", "value"),
        ("elapsed_seconds_by_strategy", "value"),
        ("hdd_write_volume_by_strategy", "value"),
        ("hdd_write_operations_by_strategy", "value"),
    ];
    for (artifact_meta, (dataset, value_key)) in artifacts.iter().take(4).zip(chart_specs) {
        if let Some(parent) = artifact_meta.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let rows = json_array(artifact, &["plot_data", dataset])
            .into_iter()
            .flatten()
            .map(|row| {
                (
                    json_plot_label(row),
                    json_f64(row, &[value_key]).unwrap_or_default(),
                )
            })
            .collect::<Vec<_>>();
        fs::write(
            &artifact_meta.path,
            render_svg_bar_chart(&artifact_meta.title, "Tested strategy", dataset, &rows),
        )?;
    }
    if let Some(artifact_meta) = artifacts.get(4) {
        let rows = json_array(artifact, &["plot_data", "per_disk_hdd_write_rate"])
            .into_iter()
            .flatten()
            .map(|row| {
                (
                    format!(
                        "{} {} c{} {}",
                        json_string(row, &["scenario"]).unwrap_or_else(|| "unknown".to_string()),
                        json_string(row, &["file_order"]).unwrap_or_else(|| "order".to_string()),
                        json_u64(row, &["hdd_concurrency"]).unwrap_or_default(),
                        json_string(row, &["disk_id"]).unwrap_or_else(|| "disk".to_string())
                    ),
                    json_f64(row, &["write_mib_per_second"]).unwrap_or_default(),
                )
            })
            .collect::<Vec<_>>();
        fs::write(
            &artifact_meta.path,
            render_svg_bar_chart(
                &artifact_meta.title,
                "Scenario and disk",
                "HDD write rate (MiB/s)",
                &rows,
            ),
        )?;
    }
    for io_artifact in performance_io_chart_artifacts_from_json(artifact, pdf_path) {
        if let Some(parent) = io_artifact.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let samples = io_samples_from_json_for_label(artifact, &io_artifact.title);
        fs::write(
            &io_artifact.path,
            render_svg_io_line_chart(&io_artifact.title, &samples),
        )?;
    }
    Ok(())
}

fn json_plot_label(row: &Value) -> String {
    format!(
        "{} {} c{} r{}",
        json_string(row, &["scenario"]).unwrap_or_else(|| "unknown".to_string()),
        json_string(row, &["file_order"]).unwrap_or_else(|| "order".to_string()),
        json_u64(row, &["hdd_concurrency"]).unwrap_or_default(),
        json_u64(row, &["redundancy"]).unwrap_or_default()
    )
}

fn performance_chart_artifacts_from_pdf_path(pdf_path: &Path) -> Vec<PerformanceChartArtifact> {
    let base = pdf_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("performance-report");
    let parent = pdf_path.parent().unwrap_or_else(|| Path::new("."));
    [
        ("Landing rate by strategy", "landing-rate"),
        ("Elapsed time by strategy", "elapsed-time"),
        ("Physical HDD write volume by strategy", "hdd-write-volume"),
        ("HDD write operations by strategy", "hdd-write-operations"),
        ("Per-disk HDD write rate", "per-disk-write-rate"),
    ]
    .into_iter()
    .map(|(title, suffix)| PerformanceChartArtifact {
        title: title.to_string(),
        path: parent.join(format!("{base}-{suffix}.svg")),
    })
    .collect()
}

fn performance_io_chart_artifacts_from_json(
    artifact: &Value,
    pdf_path: &Path,
) -> Vec<PerformanceChartArtifact> {
    let base = pdf_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("performance-report");
    let parent = pdf_path.parent().unwrap_or_else(|| Path::new("."));
    let mut labels = BTreeSet::new();
    for row in json_array(artifact, &["plot_data", "io_time_series"])
        .into_iter()
        .flatten()
    {
        labels.insert(json_plot_label(row));
    }
    labels
        .into_iter()
        .map(|label| PerformanceChartArtifact {
            title: format!("Per-second IO rates: {label}"),
            path: parent.join(format!(
                "{base}-io-{}.svg",
                label.replace(' ', "-").replace('/', "-")
            )),
        })
        .collect()
}

fn io_samples_from_json_for_label(artifact: &Value, title: &str) -> Vec<PerformanceIoSample> {
    let label = title.trim_start_matches("Per-second IO rates: ");
    json_array(artifact, &["plot_data", "io_time_series"])
        .into_iter()
        .flatten()
        .filter(|row| json_plot_label(row) == label)
        .map(|row| PerformanceIoSample {
            elapsed_second: json_u64(row, &["elapsed_second"]).unwrap_or_default(),
            device_label: json_string(row, &["device_label"])
                .unwrap_or_else(|| "device".to_string()),
            device_name: json_string(row, &["device_name"]).unwrap_or_else(|| "device".to_string()),
            read_bytes_per_second: (json_f64(row, &["read_mib_per_second"]).unwrap_or_default()
                * 1024.0
                * 1024.0)
                .round() as u64,
            write_bytes_per_second: (json_f64(row, &["write_mib_per_second"]).unwrap_or_default()
                * 1024.0
                * 1024.0)
                .round() as u64,
        })
        .collect()
}

fn json_string(value: &Value, path: &[&str]) -> Option<String> {
    let value = json_path(value, path)?;
    if value.is_null() {
        None
    } else if let Some(text) = value.as_str() {
        Some(text.to_string())
    } else {
        Some(value.to_string())
    }
}

fn json_array_strings(value: &Value, path: &[&str]) -> Vec<String> {
    json_array(value, path)
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str().map(str::to_string))
        .collect()
}

fn json_u64(value: &Value, path: &[&str]) -> Option<u64> {
    json_path(value, path)?.as_u64()
}

fn json_f64(value: &Value, path: &[&str]) -> Option<f64> {
    json_path(value, path)?.as_f64()
}

fn json_bool(value: &Value, path: &[&str]) -> Option<bool> {
    json_path(value, path)?.as_bool()
}

fn json_array<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Vec<Value>> {
    json_path(value, path)?.as_array()
}

fn json_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    Some(current)
}

fn hostname_for_report() -> String {
    ProcessCommand::new("hostname")
        .output()
        .ok()
        .and_then(|output| {
            output
                .status
                .success()
                .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "not recorded".to_string())
}

fn sha256_hex_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

#[cfg(test)]
fn render_simple_pdf(markdown: &str) -> Vec<u8> {
    let lines = markdown
        .lines()
        .map(strip_markdown_for_pdf)
        .collect::<Vec<_>>();
    let lines_per_page = 48_usize;
    let page_count = lines.len().div_ceil(lines_per_page).max(1);
    let font_id = 3 + page_count * 2;
    let mut objects = Vec::<String>::new();
    objects.push("<< /Type /Catalog /Pages 2 0 R >>".to_string());
    let kids = (0..page_count)
        .map(|index| format!("{} 0 R", 3 + index * 2))
        .collect::<Vec<_>>()
        .join(" ");
    objects.push(format!(
        "<< /Type /Pages /Kids [{kids}] /Count {page_count} >>"
    ));
    for page_index in 0..page_count {
        let page_id = 3 + page_index * 2;
        let content_id = page_id + 1;
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 {font_id} 0 R >> >> /Contents {content_id} 0 R >>"
        ));
        let page_lines = lines
            .iter()
            .skip(page_index * lines_per_page)
            .take(lines_per_page)
            .collect::<Vec<_>>();
        let mut stream = String::from("BT /F1 9 Tf 36 756 Td 0 -14 Td\n");
        for line in page_lines {
            stream.push_str(&format!("({}) Tj 0 -14 Td\n", escape_pdf_text(line)));
        }
        stream.push_str("ET");
        objects.push(format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            stream.len(),
            stream
        ));
    }
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string());

    let mut pdf = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.push_str(&format!("{} 0 obj\n{}\nendobj\n", index + 1, object));
    }
    let xref_start = pdf.len();
    pdf.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objects.len() + 1
    ));
    for offset in offsets {
        pdf.push_str(&format!("{offset:010} 00000 n \n"));
    }
    pdf.push_str(&format!(
        "trailer << /Size {} /Root 1 0 R >>\nstartxref\n{xref_start}\n%%EOF\n",
        objects.len() + 1
    ));
    pdf.into_bytes()
}

#[cfg(test)]
fn strip_markdown_for_pdf(line: &str) -> String {
    line.replace("**", "")
        .replace('`', "")
        .replace("<br>", " | ")
        .chars()
        .take(110)
        .collect()
}

#[cfg(test)]
fn escape_pdf_text(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)")
}

fn throughput(measurement: PerformanceMeasurement) -> f64 {
    measurement.bytes as f64 / measurement.seconds.max(0.001)
}

fn zero_measurement() -> PerformanceMeasurement {
    PerformanceMeasurement {
        bytes: 0,
        seconds: 0.0,
    }
}

fn update_file_read_measurements_from_disk_results(
    file_results: &mut [PerformanceFileResult],
    disk_results: &[PerformanceDiskResult],
) {
    let mut by_file = BTreeMap::<u32, PerformanceMeasurement>::new();
    for row in disk_results {
        let entry = by_file
            .entry(row.file_index)
            .or_insert_with(zero_measurement);
        entry.bytes = entry.bytes.saturating_add(row.ssd_read.bytes);
        entry.seconds += row.ssd_read.seconds.max(0.001);
    }
    for row in file_results {
        if let Some(measurement) = by_file.get(&row.file_index) {
            row.ssd_read = *measurement;
        }
    }
}

fn measurement_rate(measurements: impl Iterator<Item = PerformanceMeasurement>) -> Option<f64> {
    measurement_rate_with_current(measurements, 0, 0.0)
}

fn measurement_rate_with_current(
    measurements: impl Iterator<Item = PerformanceMeasurement>,
    current_bytes: u64,
    current_seconds: f64,
) -> Option<f64> {
    let (mut bytes, mut seconds) = measurements.fold((0_u64, 0.0_f64), |acc, measurement| {
        (
            acc.0.saturating_add(measurement.bytes),
            acc.1 + measurement.seconds.max(0.001),
        )
    });
    bytes = bytes.saturating_add(current_bytes);
    seconds += current_seconds.max(0.0);
    if bytes == 0 || seconds <= 0.0 {
        None
    } else {
        Some(bytes as f64 / seconds.max(0.001))
    }
}

fn active_hdd_disk_rates(active_writes: &ActiveHddWriteMap) -> Result<Vec<String>, CliError> {
    let now = Instant::now();
    let mut by_disk = BTreeMap::<DiskId, (u64, f64)>::new();
    for active in active_writes
        .lock()
        .map_err(|_| {
            CliError::CommandFailed("performance-test active HDD write lock poisoned".to_string())
        })?
        .values()
    {
        if active.bytes_written == 0 {
            continue;
        }
        let elapsed = now.duration_since(active.started).as_secs_f64().max(0.001);
        let entry = by_disk.entry(active.disk_id.clone()).or_insert((0, 0.0));
        entry.0 = entry.0.saturating_add(active.bytes_written);
        entry.1 = entry.1.max(elapsed);
    }
    Ok(by_disk
        .into_iter()
        .filter_map(|(disk_id, (bytes, seconds))| {
            if bytes == 0 {
                None
            } else {
                Some(format!(
                    "{} {}/s",
                    disk_id.as_str(),
                    format_bytes(bytes as f64 / seconds.max(0.001))
                ))
            }
        })
        .collect())
}

fn active_hdd_landing_lines(
    active_writes: &ActiveHddWriteMap,
    file_count: u32,
) -> Result<Vec<String>, CliError> {
    let now = Instant::now();
    let active = active_writes.lock().map_err(|_| {
        CliError::CommandFailed("performance-test active HDD write lock poisoned".to_string())
    })?;
    Ok(active
        .values()
        .map(|write| {
            format!(
                "file {}/{} copy {} -> {}: {}/{} @ {} {}",
                write.file_index + 1,
                file_count,
                write.copy_index + 1,
                write.disk_id.as_str(),
                format_bytes(write.bytes_written as f64),
                format_bytes(write.size_bytes as f64),
                active_hdd_write_rate(write, now),
                write.relative_path.display()
            )
        })
        .collect())
}

fn active_hdd_write_rate(write: &ActiveHddWrite, now: Instant) -> String {
    match (write.phase, write.bytes_written) {
        (PerformanceCopyProgressPhase::Copying, 0) => "copying".to_string(),
        (PerformanceCopyProgressPhase::Syncing, 0) => "settling".to_string(),
        (PerformanceCopyProgressPhase::Syncing, bytes) => {
            let elapsed = now.duration_since(write.started).as_secs_f64().max(0.001);
            format!("settling; avg {}/s", format_bytes(bytes as f64 / elapsed))
        }
        (PerformanceCopyProgressPhase::Copying, bytes) => {
            let elapsed = now.duration_since(write.started).as_secs_f64().max(0.001);
            format!("{}/s", format_bytes(bytes as f64 / elapsed))
        }
    }
}

fn render_performance_json(report: &PerformanceReport) -> String {
    let artifact = serde_json::json!({
        "schema": "dasobjectstore.performance_test.recommendation.v1",
        "artifact_kind": "ingress_recommendation",
        "run": {
            "run_id": report.run_id,
            "generated_at_utc": report.generated_at_utc,
            "repository_revision": report.repository_revision,
            "cli_version": dasobjectstore_core::VERSION,
            "command": report.reproduction_args,
            "parameters": {
                "workload_kind": report.workload_kind.as_str(),
                "source_path": report
                    .source_path
                    .as_ref()
                    .map(|path| path.to_string_lossy().to_string()),
                "file_size_bytes": report.file_size,
                "file_count": report.file_count,
                "total_source_bytes": report.total_source_bytes,
                "source_cap_bytes": report.source_cap_bytes,
                "file_selection": report.file_selection.as_str(),
                "file_orders": report.file_orders.iter().map(|order| order.as_str()).collect::<Vec<_>>(),
                "discovered_file_count": report.discovered_file_count,
                "discovered_total_bytes": report.discovered_total_bytes,
                "max_hdd_concurrency": report.max_concurrency,
                "selected_scenarios": report
                    .selected_scenario_names()
                    .into_iter()
                    .collect::<Vec<_>>(),
                "selected_hdd_concurrency": report.selected_hdd_concurrency(),
                "redundancy": report.redundancy,
                "keep_temp": report.keep_temp,
            },
            "artifacts": {
                "pdf_path": report.pdf_path.to_string_lossy(),
                "qr_path": report.qr_path.to_string_lossy(),
                "json_path": report.json_path.to_string_lossy(),
                "recommendation_json_path": report.json_path.to_string_lossy(),
            },
        },
        "hardware": {
            "roots": {
                "ssd_root": report.ssd_root.to_string_lossy(),
                "hdd_root": report.hdd_root.to_string_lossy(),
                "tmp_dir": report.tmp_dir.to_string_lossy(),
            },
            "disks": report.disks.iter().map(|(disk_id, root_path)| {
                serde_json::json!({
                    "disk_id": disk_id.as_str(),
                    "role": "hdd_capacity",
                    "root_path": root_path.to_string_lossy(),
                })
            }).collect::<Vec<_>>(),
        },
        "scenarios": {
            "ssd_only": ssd_only_json(&report.results.ssd_only, report.file_size, report.total_source_bytes),
            "ssd_stage_then_drain_pipeline": {
                "selected": !report.results.ssd_stage_then_drain.is_empty(),
                "description": "Source payloads are fully staged to SSD first; HDD drain begins only after all selected files have landed.",
                "concurrency": report.results.ssd_stage_then_drain.iter().map(scenario_concurrency_json).collect::<Vec<_>>(),
            },
            "ssd_hdd_pipeline": {
                "selected": !report.results.ssd_pipeline.is_empty(),
                "description": "Source payloads are staged to SSD and FIFO HDD drain begins as soon as staged files are available, overlapping SSD reads and writes.",
                "concurrency": report.results.ssd_pipeline.iter().map(scenario_concurrency_json).collect::<Vec<_>>(),
            },
            "direct_hdd_pipeline": {
                "selected": !report.results.direct_hdd.is_empty(),
                "description": "Source payloads are written directly to selected HDD members without SSD staging.",
                "concurrency": report.results.direct_hdd.iter().map(scenario_concurrency_json).collect::<Vec<_>>(),
            },
        },
        "recommendation": {
            "strategy": recommendation_strategy_name(report.recommendation.strategy),
            "file_order": report.recommendation.file_order.as_str(),
            "hdd_concurrency": report.recommendation.hdd_concurrency,
            "redundancy": report.redundancy,
            "estimated_aggregate_bytes_per_second": rate_u64(report.recommendation.aggregate_bytes_per_second),
            "ssd_read_limited": recommendation_is_ssd_read_limited(report),
            "rationale": recommendation_rationale(report),
        },
        "plot_data": {
            "schema": "dasobjectstore.performance_test.plot_data.v1",
            "landing_rate_by_strategy": landing_rate_plot_rows(report),
            "elapsed_seconds_by_strategy": elapsed_plot_rows(report),
            "hdd_write_volume_by_strategy": hdd_volume_plot_rows(report),
            "hdd_write_operations_by_strategy": hdd_operations_plot_rows(report),
            "per_disk_hdd_write_rate": per_disk_rate_plot_rows(report),
            "io_time_series": io_time_series_plot_rows(report),
        },
        "daemon_policy": {
            "authoritative": report.authoritative,
            "effective_after": "daemon_restart",
            "authoritative_path": report
                .authoritative_path
                .as_ref()
                .map(|path| path.to_string_lossy().to_string()),
            "source_routes": {
                "remote_upload": "ssd_first",
                "external_disk_ingress": "ssd_first",
                "nvme_source_ingress": recommendation_strategy_name(report.recommendation.strategy),
            },
            "ssd_hdd_settlement": {
                "strategy": "ssd_hdd_pipeline",
                "file_order": report.recommendation.file_order.as_str(),
                "hdd_concurrency": report.recommendation.hdd_concurrency,
                "redundancy": report.redundancy,
                "estimated_aggregate_bytes_per_second": rate_u64(report.recommendation.aggregate_bytes_per_second),
            },
        },
    });
    serde_json::to_string_pretty(&artifact).expect("serialize performance recommendation JSON")
}

fn ssd_only_json(
    scenarios: &[PerformanceScenarioResult],
    file_size: u64,
    total_source_bytes: u64,
) -> serde_json::Value {
    if scenarios.is_empty() {
        return serde_json::json!({
            "selected": false,
            "file_count": 0,
            "file_size_bytes": file_size,
            "total_bytes": total_source_bytes,
            "orders": [],
            "files": [],
            "io_samples": [],
        });
    }
    let first = &scenarios[0];
    serde_json::json!({
        "selected": true,
        "file_count": first.file_results.len() as u64,
        "file_size_bytes": file_size,
        "total_bytes": total_source_bytes,
        "file_order": first.file_order.as_str(),
        "median_generate_bytes_per_second": rate_u64(median_rate(
            first.file_results.iter().map(|row| throughput(row.ssd_write))
        )),
        "median_ssd_write_bytes_per_second": rate_u64(median_rate(
            first.file_results.iter().map(|row| throughput(row.ssd_write))
        )),
        "median_ssd_read_bytes_per_second": rate_u64(median_rate(
            first.file_results.iter().map(|row| throughput(row.ssd_read))
        )),
        "io_samples": scenario_io_samples_json(first),
        "files": first.file_results.iter().map(|row| {
            serde_json::json!({
                "file_index": row.file_index,
                "generated_bytes": row.ssd_write.bytes,
                "generate_bytes_per_second": rate_u64(throughput(row.ssd_write)),
                "ssd_write_bytes_per_second": rate_u64(throughput(row.ssd_write)),
                "ssd_read_bytes_per_second": rate_u64(throughput(row.ssd_read)),
            })
        }).collect::<Vec<_>>(),
        "orders": scenarios.iter().map(|scenario| {
            serde_json::json!({
                "file_order": scenario.file_order.as_str(),
                "file_count": scenario.file_results.len() as u64,
                "median_generate_bytes_per_second": rate_u64(median_rate(
                    scenario.file_results.iter().map(|row| throughput(row.ssd_write))
                )),
                "median_ssd_write_bytes_per_second": rate_u64(median_rate(
                    scenario.file_results.iter().map(|row| throughput(row.ssd_write))
                )),
                "median_ssd_read_bytes_per_second": rate_u64(median_rate(
                    scenario.file_results.iter().map(|row| throughput(row.ssd_read))
                )),
                "io_samples": scenario_io_samples_json(scenario),
                "files": scenario.file_results.iter().map(|row| {
                    serde_json::json!({
                        "file_index": row.file_index,
                        "generated_bytes": row.ssd_write.bytes,
                        "generate_bytes_per_second": rate_u64(throughput(row.ssd_write)),
                        "ssd_write_bytes_per_second": rate_u64(throughput(row.ssd_write)),
                        "ssd_read_bytes_per_second": rate_u64(throughput(row.ssd_read)),
                    })
                }).collect::<Vec<_>>(),
            })
        }).collect::<Vec<_>>(),
    })
}

fn scenario_concurrency_json(scenario: &PerformanceScenarioResult) -> serde_json::Value {
    serde_json::json!({
        "scenario": scenario.kind.as_str(),
        "file_order": scenario.file_order.as_str(),
        "concurrency": scenario.concurrency,
        "redundancy": scenario.redundancy,
        "queue_capacity": scenario.queue_capacity,
        "logical_source_bytes": scenario.logical_source_bytes,
        "physical_hdd_write_bytes": scenario.physical_hdd_write_bytes,
        "hdd_write_operations": scenario.hdd_write_operations,
        "aggregate_assigned_bytes": scenario.total_bytes,
        "aggregate_write_bytes_per_second": rate_u64(
            scenario.total_bytes as f64 / scenario.elapsed_seconds.max(0.001)
        ),
        "hdd_drain_started_before_all_ssd_staged": scenario.hdd_drain_started_before_all_ssd_staged,
        "slowest_member_seconds": scenario.concurrency_result.slowest_seconds,
        "members": scenario.concurrency_result.members.iter().map(DiskId::as_str).collect::<Vec<_>>(),
        "per_disk": per_disk_json(&scenario.disk_results),
        "io_samples": scenario_io_samples_json(scenario),
    })
}

fn scenario_io_samples_json(scenario: &PerformanceScenarioResult) -> Vec<serde_json::Value> {
    scenario
        .io_samples
        .iter()
        .map(|sample| {
            serde_json::json!({
                "elapsed_second": sample.elapsed_second,
                "device_label": sample.device_label,
                "device_name": sample.device_name,
                "read_bytes_per_second": sample.read_bytes_per_second,
                "write_bytes_per_second": sample.write_bytes_per_second,
                "read_mib_per_second": sample.read_bytes_per_second as f64 / 1024.0 / 1024.0,
                "write_mib_per_second": sample.write_bytes_per_second as f64 / 1024.0 / 1024.0,
            })
        })
        .collect()
}

fn per_disk_json(rows: &[PerformanceDiskResult]) -> Vec<serde_json::Value> {
    let mut by_disk = BTreeMap::<DiskId, (u64, f64)>::new();
    for row in rows {
        let entry = by_disk.entry(row.disk_id.clone()).or_insert((0, 0.0));
        entry.0 = entry.0.saturating_add(row.write.bytes);
        entry.1 += row.write.seconds.max(0.001);
    }
    by_disk
        .into_iter()
        .map(|(disk_id, (assigned_bytes, seconds))| {
            serde_json::json!({
                "disk_id": disk_id.as_str(),
                "assigned_bytes": assigned_bytes,
                "write_bytes_per_second": rate_u64(assigned_bytes as f64 / seconds.max(0.001)),
                "write_operations": rows.iter().filter(|row| row.disk_id == disk_id).count(),
            })
        })
        .collect()
}

fn landing_rate_plot_rows(report: &PerformanceReport) -> Vec<serde_json::Value> {
    report
        .all_scenarios()
        .into_iter()
        .map(|scenario| scenario_plot_row(report, scenario, "aggregate_mib_per_second"))
        .collect()
}

fn elapsed_plot_rows(report: &PerformanceReport) -> Vec<serde_json::Value> {
    report
        .all_scenarios()
        .into_iter()
        .map(|scenario| scenario_plot_row(report, scenario, "elapsed_seconds"))
        .collect()
}

fn hdd_volume_plot_rows(report: &PerformanceReport) -> Vec<serde_json::Value> {
    report
        .all_scenarios()
        .into_iter()
        .map(|scenario| scenario_plot_row(report, scenario, "physical_hdd_write_gib"))
        .collect()
}

fn hdd_operations_plot_rows(report: &PerformanceReport) -> Vec<serde_json::Value> {
    report
        .all_scenarios()
        .into_iter()
        .map(|scenario| scenario_plot_row(report, scenario, "hdd_write_operations"))
        .collect()
}

fn scenario_plot_row(
    report: &PerformanceReport,
    scenario: &PerformanceScenarioResult,
    metric: &str,
) -> serde_json::Value {
    let aggregate_mib_per_second =
        scenario.total_bytes as f64 / scenario.elapsed_seconds.max(0.001) / 1024.0 / 1024.0;
    let physical_hdd_write_gib =
        scenario.physical_hdd_write_bytes as f64 / 1024.0 / 1024.0 / 1024.0;
    let value = match metric {
        "aggregate_mib_per_second" => aggregate_mib_per_second,
        "elapsed_seconds" => scenario.elapsed_seconds,
        "physical_hdd_write_gib" => physical_hdd_write_gib,
        "hdd_write_operations" => scenario.hdd_write_operations as f64,
        _ => 0.0,
    };
    serde_json::json!({
        "run_id": report.run_id,
        "scenario": scenario.kind.as_str(),
        "scenario_label": scenario.kind.label(),
        "file_order": scenario.file_order.as_str(),
        "hdd_concurrency": scenario.concurrency,
        "redundancy": scenario.redundancy,
        "metric": metric,
        "value": value,
        "aggregate_mib_per_second": aggregate_mib_per_second,
        "elapsed_seconds": scenario.elapsed_seconds,
        "logical_source_gib": scenario.logical_source_bytes as f64 / 1024.0 / 1024.0 / 1024.0,
        "physical_hdd_write_gib": physical_hdd_write_gib,
        "hdd_write_operations": scenario.hdd_write_operations,
        "hdd_drain_overlapped_ssd_staging": scenario.hdd_drain_started_before_all_ssd_staged,
    })
}

fn per_disk_rate_plot_rows(report: &PerformanceReport) -> Vec<serde_json::Value> {
    let mut rows = Vec::new();
    for scenario in report.all_scenarios() {
        let mut by_disk = BTreeMap::<DiskId, (u64, f64, usize)>::new();
        for row in &scenario.disk_results {
            let entry = by_disk.entry(row.disk_id.clone()).or_insert((0, 0.0, 0));
            entry.0 = entry.0.saturating_add(row.write.bytes);
            entry.1 += row.write.seconds.max(0.001);
            entry.2 += 1;
        }
        for (disk_id, (bytes, seconds, operations)) in by_disk {
            rows.push(serde_json::json!({
                "run_id": report.run_id,
                "scenario": scenario.kind.as_str(),
                "scenario_label": scenario.kind.label(),
                "file_order": scenario.file_order.as_str(),
                "hdd_concurrency": scenario.concurrency,
                "redundancy": scenario.redundancy,
                "disk_id": disk_id.as_str(),
                "write_mib_per_second": bytes as f64 / seconds.max(0.001) / 1024.0 / 1024.0,
                "assigned_gib": bytes as f64 / 1024.0 / 1024.0 / 1024.0,
                "write_operations": operations,
            }));
        }
    }
    rows
}

fn io_time_series_plot_rows(report: &PerformanceReport) -> Vec<serde_json::Value> {
    let mut rows = Vec::new();
    for scenario in report.all_scenarios() {
        for sample in &scenario.io_samples {
            rows.push(serde_json::json!({
                "run_id": report.run_id,
                "scenario": scenario.kind.as_str(),
                "scenario_label": scenario.kind.label(),
                "file_order": scenario.file_order.as_str(),
                "hdd_concurrency": scenario.concurrency,
                "redundancy": scenario.redundancy,
                "elapsed_second": sample.elapsed_second,
                "device_label": sample.device_label,
                "device_name": sample.device_name,
                "read_mib_per_second": sample.read_bytes_per_second as f64 / 1024.0 / 1024.0,
                "write_mib_per_second": sample.write_bytes_per_second as f64 / 1024.0 / 1024.0,
            }));
        }
    }
    rows
}

fn recommendation_strategy_name(strategy: PerformanceScenarioKind) -> &'static str {
    match strategy {
        PerformanceScenarioKind::SsdOnly => "ssd_only",
        PerformanceScenarioKind::SsdStageThenDrain => "ssd_stage_then_drain_pipeline",
        PerformanceScenarioKind::SsdPipeline => "ssd_hdd_pipeline",
        PerformanceScenarioKind::DirectHdd => "direct_hdd_pipeline",
    }
}

fn recommendation_is_ssd_read_limited(report: &PerformanceReport) -> bool {
    let Some(ssd_only) = report.results.ssd_only.first() else {
        return false;
    };
    let ssd_read = median_rate(
        ssd_only
            .file_results
            .iter()
            .map(|row| throughput(row.ssd_read)),
    );
    report.recommendation.aggregate_bytes_per_second > ssd_read * 0.9
}

fn recommendation_rationale(report: &PerformanceReport) -> Vec<String> {
    let mut rationale = vec![report.recommendation.reason.clone()];
    if report.results.ssd_only.is_empty() {
        rationale.push(
            "SSD-only read/write baselines were not included in this selected benchmark matrix."
                .to_string(),
        );
    } else if recommendation_is_ssd_read_limited(report) {
        rationale.push(
            "Selected aggregate throughput is close to measured SSD read throughput; avoid raising concurrency without retesting."
                .to_string(),
        );
    } else {
        rationale.push(
            "Measured SSD read throughput leaves headroom for the selected landing strategy."
                .to_string(),
        );
    }
    rationale.push(
        "Use per-disk assigned byte and throughput rows to identify weak disks before scheduling latency-sensitive ingest."
            .to_string(),
    );
    rationale
}

fn rate_u64(rate: f64) -> u64 {
    if rate.is_finite() && rate > 0.0 {
        rate.round() as u64
    } else {
        0
    }
}

fn render_performance_report(report: PerformanceReport) -> String {
    let mut output = String::new();
    output.push_str("# DASObjectStore Performance Test Report\n\n");
    output.push_str("| Field | Value |\n");
    output.push_str("| --- | --- |\n");
    output.push_str(&format!(
        "| Brand | Mnemosyne Biosciences |\n\
| Product | DASObjectStore |\n\
| Report type | Performance test |\n\
| Report status | final |\n\
| Run ID | `{}` |\n\
| Generated at (UTC) | `{}` |\n\
| Repository revision | `{}` |\n\
| CLI version | `{}` |\n\
| Command | `{}` |\n\
| JSON artifact | `{}` |\n\
| PDF artifact | `{}` |\n\
| QR artifact | `{}` |\n\
| QR status | `{}` |\n\
| Redundancy | `{}` HDD copy/copies per logical file |\n\
| Reproduction payload SHA-256 | `{}` |\n\
| Reproduction QR payload | See `Reproduction Payload` |\n\n",
        report.run_id,
        report.generated_at_utc,
        report.repository_revision,
        dasobjectstore_core::VERSION,
        report.reproduce_command,
        report.json_path.display(),
        report.pdf_path.display(),
        report.qr_path.display(),
        report.qr_status,
        report.redundancy,
        report.reproduction_payload_sha256
    ));
    output.push_str(&format!(
        "QR code image: ![Reproduce]({})\n\n",
        report.qr_path.display()
    ));
    let source = report
        .source_path
        .as_ref()
        .map(|path| format!("; source `{}`", path.display()))
        .unwrap_or_default();
    let cap = report
        .source_cap_bytes
        .map(|bytes| format!("; cap {}", format_bytes(bytes as f64)))
        .unwrap_or_default();
    let discovered = if report.source_cap_bytes.is_some() {
        format!(
            "; discovered {} files, {} total",
            report.discovered_file_count,
            format_bytes(report.discovered_total_bytes as f64)
        )
    } else {
        String::new()
    };
    let file_orders = report
        .file_orders
        .iter()
        .map(|order| format!("`{}`", order.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    output.push_str(&format!(
        "Scenario: {} workload, {} files, {} logical source total{}{}{}; file selection `{}`; file order(s) {}. Redundancy {}; SSD root `{}`; HDD root `{}`; disks {}; selected scenarios {}; selected HDD concurrency {}.\n\n",
        report.workload_kind.as_str(),
        report.file_count,
        format_bytes(report.total_source_bytes as f64),
        source,
        cap,
        discovered,
        report.file_selection.as_str(),
        file_orders,
        report.redundancy,
        report.ssd_root.display(),
        report.hdd_root.display(),
        report.disk_count,
        report.selected_scenario_names().join(", "),
        format_concurrency_list(&report.selected_hdd_concurrency())
    ));
    output.push_str("## Summary\n\n");
    output.push_str(&format!("- Run id: `{}`\n", report.run_id));
    output.push_str(&format!(
        "- Reproduce with: `{}`\n",
        report.reproduce_command
    ));
    output.push_str("\n## Reproduction Payload\n\n```json\n");
    output.push_str(&report.reproduction_payload);
    output.push_str("\n```\n\n");
    output.push_str(&format!(
        "- Total elapsed: {:.1} s\n",
        report.elapsed_seconds
    ));
    if let Some(ssd_only) = report.results.ssd_only.first() {
        output.push_str(&format!(
            "- Median SSD write: {}/s\n",
            format_bytes(median_rate(
                ssd_only
                    .file_results
                    .iter()
                    .map(|row| throughput(row.ssd_write))
            ))
        ));
        output.push_str(&format!(
            "- Median SSD read: {}/s\n",
            format_bytes(median_rate(
                ssd_only
                    .file_results
                    .iter()
                    .map(|row| throughput(row.ssd_read))
            ))
        ));
    } else {
        output.push_str("- Median SSD write: not measured in selected scenario matrix\n");
        output.push_str("- Median SSD read: not measured in selected scenario matrix\n");
    }
    output.push_str(&format!(
        "- Recommended strategy: {} with `{}` order at {} HDD worker(s), observed aggregate {}/s\n",
        report.recommendation.strategy.as_str(),
        report.recommendation.file_order.as_str(),
        report.recommendation.hdd_concurrency,
        format_bytes(report.recommendation.aggregate_bytes_per_second)
    ));

    output.push_str("\n## Scenario Summary\n\n");
    output.push_str("| Scenario | File order | HDD concurrency | Redundancy | Logical source | Physical HDD writes | Operations | Aggregate landing | Elapsed | HDD drain overlapped SSD staging |\n");
    output.push_str("| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |\n");
    for scenario in report.all_scenarios() {
        output.push_str(&format!(
            "| {} | `{}` | {} | {} | {} | {} | {} | {}/s | {:.1} s | {} |\n",
            scenario.kind.label(),
            scenario.file_order.as_str(),
            scenario.concurrency,
            scenario.redundancy,
            format_bytes(scenario.logical_source_bytes as f64),
            format_bytes(scenario.physical_hdd_write_bytes as f64),
            scenario.hdd_write_operations,
            format_bytes(scenario.total_bytes as f64 / scenario.elapsed_seconds.max(0.001)),
            scenario.elapsed_seconds,
            yes_no(scenario.hdd_drain_started_before_all_ssd_staged)
        ));
    }

    output.push_str("\n## SSD Timings\n\n");
    output.push_str("| Scenario | File order | HDD concurrency | File | SSD write | SSD read |\n");
    output.push_str("| --- | --- | ---: | ---: | ---: | ---: |\n");
    for scenario in report.all_scenarios() {
        for row in &scenario.file_results {
            output.push_str(&format!(
                "| {} | `{}` | {} | {} | {}/s | {}/s |\n",
                scenario.kind.as_str(),
                scenario.file_order.as_str(),
                scenario.concurrency,
                row.file_index + 1,
                format_bytes(throughput(row.ssd_write)),
                format_bytes(throughput(row.ssd_read))
            ));
        }
    }

    output.push_str("\n## Per-disk Landed Files\n\n");
    output.push_str(
        "| Scenario | File order | HDD concurrency | File | Copy | Disk | Write rate |\n",
    );
    output.push_str("| --- | --- | ---: | ---: | ---: | --- | ---: |\n");
    for scenario in report.all_scenarios() {
        for row in &scenario.disk_results {
            output.push_str(&format!(
                "| {} | `{}` | {} | {} | {} | {} | {}/s |\n",
                row.scenario.as_str(),
                scenario.file_order.as_str(),
                row.concurrency,
                row.file_index + 1,
                row.copy_index + 1,
                row.disk_id,
                format_bytes(throughput(row.write))
            ));
        }
    }

    output.push_str("\n## Concurrency Results\n\n");
    output.push_str(
        "| Scenario | File order | HDD concurrency | Members | Aggregate landing | Slowest file write | HDD drain overlapped SSD staging |\n",
    );
    output.push_str("| --- | --- | ---: | --- | ---: | ---: | --- |\n");
    for scenario in report.all_scenarios() {
        let row = &scenario.concurrency_result;
        let members = row
            .members
            .iter()
            .map(DiskId::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        output.push_str(&format!(
            "| {} | `{}` | {} | {} | {}/s | {:.1} s | {} |\n",
            row.scenario.as_str(),
            scenario.file_order.as_str(),
            row.concurrency,
            members,
            format_bytes(row.aggregate_bytes as f64 / row.seconds.max(0.001)),
            row.slowest_seconds,
            yes_no(scenario.hdd_drain_started_before_all_ssd_staged)
        ));
    }

    output.push_str("\n## Quantitative Plot Data\n\n");
    output.push_str("The JSON artifact includes tidy bar-chart and IO time-series rows under `plot_data` for scientifically labelled Grammateus/floundeR plots. The report bundle also includes deterministic SVG bar charts and per-run IO line charts rendered from the same benchmark rows.\n\n");
    for artifact in performance_chart_artifacts(&report) {
        output.push_str(&format!(
            "![{}]({})\n\n",
            artifact.title,
            artifact.path.display()
        ));
    }
    for artifact in performance_io_chart_artifacts(&report) {
        output.push_str(&format!(
            "![{}]({})\n\n",
            artifact.title,
            artifact.path.display()
        ));
    }
    output.push_str("| Plot dataset | Intended quantitative question |\n");
    output.push_str("| --- | --- |\n");
    output.push_str(
        "| `landing_rate_by_strategy` | Which strategy landed the complete dataset fastest? |\n",
    );
    output.push_str("| `elapsed_seconds_by_strategy` | Which strategy completed the workload in the least wall time? |\n");
    output.push_str("| `hdd_write_volume_by_strategy` | How much physical HDD data did each strategy write after redundancy? |\n");
    output.push_str("| `hdd_write_operations_by_strategy` | How many write operations did each strategy perform? |\n");
    output.push_str("| `per_disk_hdd_write_rate` | Which disks were faster or slower under the tested configuration? |\n");
    output.push_str("| `io_time_series` | How did SSD and HDD read/write IO rates change each second during each run? |\n");

    output.push_str("\n## Recommendation\n\n");
    output.push_str(&format!(
        "- Use `{}` with `{}` file order and {} HDD worker(s) for this hardware constellation.\n",
        report.recommendation.strategy.as_str(),
        report.recommendation.file_order.as_str(),
        report.recommendation.hdd_concurrency
    ));
    output.push_str(&format!("- {}.\n", report.recommendation.reason));
    output.push_str("- Use the JSON artifact as the machine-readable placement and concurrency guidance for future ingest policy.\n");
    output
}

#[derive(Clone, Debug)]
struct PerformanceChartArtifact {
    title: String,
    path: PathBuf,
}

fn performance_chart_artifacts(report: &PerformanceReport) -> Vec<PerformanceChartArtifact> {
    let base = report
        .pdf_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("performance-report");
    let parent = report.pdf_path.parent().unwrap_or_else(|| Path::new("."));
    [
        ("Landing rate by strategy", "landing-rate"),
        ("Elapsed time by strategy", "elapsed-time"),
        ("Physical HDD write volume by strategy", "hdd-write-volume"),
        ("HDD write operations by strategy", "hdd-write-operations"),
        ("Per-disk HDD write rate", "per-disk-write-rate"),
    ]
    .into_iter()
    .map(|(title, suffix)| PerformanceChartArtifact {
        title: title.to_string(),
        path: parent.join(format!("{base}-{suffix}.svg")),
    })
    .collect()
}

fn performance_io_chart_artifacts(report: &PerformanceReport) -> Vec<PerformanceChartArtifact> {
    let base = report
        .pdf_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("performance-report");
    let parent = report.pdf_path.parent().unwrap_or_else(|| Path::new("."));
    report
        .all_scenarios()
        .into_iter()
        .filter(|scenario| !scenario.io_samples.is_empty())
        .map(|scenario| {
            let scenario_label = performance_chart_scenario_label(scenario);
            let suffix = scenario_label.replace(' ', "-");
            PerformanceChartArtifact {
                title: format!("Per-second IO rates: {scenario_label}"),
                path: parent.join(format!("{base}-io-{suffix}.svg")),
            }
        })
        .collect()
}

fn write_performance_chart_svgs(report: &PerformanceReport) -> Result<(), CliError> {
    let artifacts = performance_chart_artifacts(report);
    for artifact in &artifacts {
        if let Some(parent) = artifact.path.parent() {
            fs::create_dir_all(parent)?;
        }
    }
    let scenario_rows = report
        .all_scenarios()
        .into_iter()
        .map(|scenario| {
            (
                performance_chart_scenario_label(scenario),
                scenario.total_bytes as f64 / scenario.elapsed_seconds.max(0.001) / 1024.0 / 1024.0,
            )
        })
        .collect::<Vec<_>>();
    fs::write(
        &artifacts[0].path,
        render_svg_bar_chart(
            &artifacts[0].title,
            "Tested strategy",
            "Aggregate landing rate (MiB/s)",
            &scenario_rows,
        ),
    )?;
    let elapsed_rows = report
        .all_scenarios()
        .into_iter()
        .map(|scenario| {
            (
                performance_chart_scenario_label(scenario),
                scenario.elapsed_seconds,
            )
        })
        .collect::<Vec<_>>();
    fs::write(
        &artifacts[1].path,
        render_svg_bar_chart(
            &artifacts[1].title,
            "Tested strategy",
            "Elapsed time (s)",
            &elapsed_rows,
        ),
    )?;
    let volume_rows = report
        .all_scenarios()
        .into_iter()
        .map(|scenario| {
            (
                performance_chart_scenario_label(scenario),
                scenario.physical_hdd_write_bytes as f64 / 1024.0 / 1024.0 / 1024.0,
            )
        })
        .collect::<Vec<_>>();
    fs::write(
        &artifacts[2].path,
        render_svg_bar_chart(
            &artifacts[2].title,
            "Tested strategy",
            "Physical HDD write volume (GiB)",
            &volume_rows,
        ),
    )?;
    let operation_rows = report
        .all_scenarios()
        .into_iter()
        .map(|scenario| {
            (
                performance_chart_scenario_label(scenario),
                scenario.hdd_write_operations as f64,
            )
        })
        .collect::<Vec<_>>();
    fs::write(
        &artifacts[3].path,
        render_svg_bar_chart(
            &artifacts[3].title,
            "Tested strategy",
            "HDD write operations",
            &operation_rows,
        ),
    )?;
    let disk_rows = performance_hdd_disk_rate_chart_rows(report);
    fs::write(
        &artifacts[4].path,
        render_svg_bar_chart(
            &artifacts[4].title,
            "Scenario and disk",
            "HDD write rate (MiB/s)",
            &disk_rows,
        ),
    )?;
    for (scenario, artifact) in report
        .all_scenarios()
        .into_iter()
        .filter(|scenario| !scenario.io_samples.is_empty())
        .zip(performance_io_chart_artifacts(report).into_iter())
    {
        if let Some(parent) = artifact.path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(
            &artifact.path,
            render_svg_io_line_chart(&artifact.title, &scenario.io_samples),
        )?;
    }
    Ok(())
}

fn performance_chart_scenario_label(scenario: &PerformanceScenarioResult) -> String {
    if scenario.concurrency == 0 {
        format!(
            "{} {} r{}",
            scenario.kind.as_str(),
            scenario.file_order.as_str(),
            scenario.redundancy
        )
    } else {
        format!(
            "{} {} c{} r{}",
            scenario.kind.as_str(),
            scenario.file_order.as_str(),
            scenario.concurrency,
            scenario.redundancy
        )
    }
}

fn performance_hdd_disk_rate_chart_rows(report: &PerformanceReport) -> Vec<(String, f64)> {
    let mut rows = Vec::new();
    for scenario in report.all_scenarios() {
        let mut by_disk = BTreeMap::<DiskId, (u64, f64)>::new();
        for row in &scenario.disk_results {
            let entry = by_disk.entry(row.disk_id.clone()).or_insert((0, 0.0));
            entry.0 = entry.0.saturating_add(row.write.bytes);
            entry.1 += row.write.seconds.max(0.001);
        }
        for (disk_id, (bytes, seconds)) in by_disk {
            rows.push((
                format!(
                    "{} {} c{} {}",
                    scenario.kind.as_str(),
                    scenario.file_order.as_str(),
                    scenario.concurrency,
                    disk_id
                ),
                bytes as f64 / seconds.max(0.001) / 1024.0 / 1024.0,
            ));
        }
    }
    rows
}

fn render_svg_bar_chart(
    title: &str,
    x_label: &str,
    y_label: &str,
    rows: &[(String, f64)],
) -> String {
    let width = 960.0_f64;
    let height = 460.0_f64;
    let left = 86.0_f64;
    let right = 28.0_f64;
    let top = 58.0_f64;
    let bottom = 132.0_f64;
    let plot_width = width - left - right;
    let plot_height = height - top - bottom;
    let max_value = rows
        .iter()
        .map(|(_, value)| *value)
        .fold(0.0_f64, f64::max)
        .max(1.0);
    let slot_width = plot_width / rows.len().max(1) as f64;
    let bar_width = (slot_width * 0.68).max(6.0);
    let mut marks = String::new();
    let palette = ["#2f5d50", "#6f7f35", "#2f6f8f", "#8f5d2f", "#5f548a"];
    for (idx, (label, value)) in rows.iter().enumerate() {
        let bar_height = (value / max_value) * plot_height;
        let x = left + idx as f64 * slot_width + (slot_width - bar_width) / 2.0;
        let y = top + plot_height - bar_height;
        let color = palette[idx % palette.len()];
        marks.push_str(&format!(
            "<rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{bar_width:.1}\" height=\"{bar_height:.1}\" fill=\"{color}\"/>\n\
             <text x=\"{label_x:.1}\" y=\"{label_y:.1}\" transform=\"rotate(45 {label_x:.1} {label_y:.1})\" font-family=\"Arial, sans-serif\" font-size=\"10\" fill=\"#222\">{label}</text>\n\
             <text x=\"{value_x:.1}\" y=\"{value_y:.1}\" text-anchor=\"middle\" font-family=\"Arial, sans-serif\" font-size=\"10\" fill=\"#222\">{value:.1}</text>\n",
            label_x = x + bar_width / 2.0 - 4.0,
            label_y = top + plot_height + 18.0,
            value_x = x + bar_width / 2.0,
            value_y = (y - 6.0).max(20.0),
            label = escape_xml_text(label),
        ));
    }
    let tick_mid = max_value / 2.0;
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" role=\"img\" aria-label=\"{}\" viewBox=\"0 0 {width:.0} {height:.0}\">\n\
         <rect width=\"{width:.0}\" height=\"{height:.0}\" fill=\"#ffffff\"/>\n\
         <text x=\"24\" y=\"32\" font-family=\"Arial, sans-serif\" font-size=\"18\" font-weight=\"700\" fill=\"#111\">{}</text>\n\
         <line x1=\"{left:.1}\" y1=\"{axis_y:.1}\" x2=\"{axis_right:.1}\" y2=\"{axis_y:.1}\" stroke=\"#111\" stroke-width=\"1.2\"/>\n\
         <line x1=\"{left:.1}\" y1=\"{top:.1}\" x2=\"{left:.1}\" y2=\"{axis_y:.1}\" stroke=\"#111\" stroke-width=\"1.2\"/>\n\
         <line x1=\"{left:.1}\" y1=\"{mid_y:.1}\" x2=\"{axis_right:.1}\" y2=\"{mid_y:.1}\" stroke=\"#d9ddd2\" stroke-width=\"1\"/>\n\
         <text x=\"{tick_x:.1}\" y=\"{axis_y:.1}\" text-anchor=\"end\" font-family=\"Arial, sans-serif\" font-size=\"10\" fill=\"#555\">0</text>\n\
         <text x=\"{tick_x:.1}\" y=\"{mid_text_y:.1}\" text-anchor=\"end\" font-family=\"Arial, sans-serif\" font-size=\"10\" fill=\"#555\">{tick_mid:.1}</text>\n\
         <text x=\"{tick_x:.1}\" y=\"{top_text_y:.1}\" text-anchor=\"end\" font-family=\"Arial, sans-serif\" font-size=\"10\" fill=\"#555\">{max_value:.1}</text>\n\
         {marks}\
         <text x=\"{x_label_x:.1}\" y=\"{x_label_y:.1}\" text-anchor=\"middle\" font-family=\"Arial, sans-serif\" font-size=\"13\" fill=\"#111\">{x_axis_label}</text>\n\
         <text transform=\"translate(22 {y_label_y:.1}) rotate(-90)\" text-anchor=\"middle\" font-family=\"Arial, sans-serif\" font-size=\"13\" fill=\"#111\">{y_axis_label}</text>\n\
         </svg>\n",
        escape_xml_text(title),
        escape_xml_text(title),
        axis_y = top + plot_height,
        axis_right = width - right,
        mid_y = top + plot_height / 2.0,
        tick_x = left - 8.0,
        mid_text_y = top + plot_height / 2.0 + 4.0,
        top_text_y = top + 4.0,
        marks = marks,
        x_label_x = left + plot_width / 2.0,
        x_label_y = height - 18.0,
        x_axis_label = escape_xml_text(x_label),
        y_label_y = top + plot_height / 2.0,
        y_axis_label = escape_xml_text(y_label),
    )
}

fn render_svg_io_line_chart(title: &str, samples: &[PerformanceIoSample]) -> String {
    let width = 1120.0_f64;
    let height = 520.0_f64;
    let left = 86.0_f64;
    let right = 250.0_f64;
    let top = 62.0_f64;
    let bottom = 72.0_f64;
    let plot_width = width - left - right;
    let plot_height = height - top - bottom;
    let axis_y = top + plot_height;
    let axis_right = left + plot_width;
    if samples.is_empty() {
        return format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" role=\"img\" aria-label=\"{}\" viewBox=\"0 0 {width:.0} {height:.0}\">\n\
             <rect width=\"{width:.0}\" height=\"{height:.0}\" fill=\"#ffffff\"/>\n\
             <text x=\"24\" y=\"36\" font-family=\"Arial, sans-serif\" font-size=\"18\" font-weight=\"700\" fill=\"#111\">{}</text>\n\
             <text x=\"24\" y=\"74\" font-family=\"Arial, sans-serif\" font-size=\"13\" fill=\"#555\">No per-second IO samples were captured for this run.</text>\n\
             </svg>\n",
            escape_xml_text(title),
            escape_xml_text(title),
        );
    }
    let max_second = samples
        .iter()
        .map(|sample| sample.elapsed_second)
        .max()
        .unwrap_or(1)
        .max(1);
    let max_mib = samples
        .iter()
        .flat_map(|sample| {
            [
                sample.read_bytes_per_second as f64 / 1024.0 / 1024.0,
                sample.write_bytes_per_second as f64 / 1024.0 / 1024.0,
            ]
        })
        .fold(0.0_f64, f64::max)
        .max(1.0);
    let palette = [
        "#2f5d50", "#2f6f8f", "#8f5d2f", "#5f548a", "#6f7f35", "#9a4f68", "#49728f", "#7b6230",
    ];
    let by_device = samples.iter().fold(
        BTreeMap::<String, Vec<&PerformanceIoSample>>::new(),
        |mut by_device, sample| {
            by_device
                .entry(sample.device_label.clone())
                .or_default()
                .push(sample);
            by_device
        },
    );
    let mut marks = String::new();
    let mut legend = String::new();
    for (idx, (label, device_samples)) in by_device.iter().enumerate() {
        let color = palette[idx % palette.len()];
        let mut read_points = String::new();
        let mut write_points = String::new();
        let mut point_marks = String::new();
        for sample in device_samples {
            let x = left + (sample.elapsed_second as f64 / max_second as f64) * plot_width;
            let read_mib = sample.read_bytes_per_second as f64 / 1024.0 / 1024.0;
            let write_mib = sample.write_bytes_per_second as f64 / 1024.0 / 1024.0;
            let read_y = axis_y - (read_mib / max_mib) * plot_height;
            let write_y = axis_y - (write_mib / max_mib) * plot_height;
            read_points.push_str(&format!("{x:.1},{read_y:.1} "));
            write_points.push_str(&format!("{x:.1},{write_y:.1} "));
            point_marks.push_str(&format!(
                "<circle cx=\"{x:.1}\" cy=\"{write_y:.1}\" r=\"2.7\" fill=\"{color}\"/>\n\
                 <circle cx=\"{x:.1}\" cy=\"{read_y:.1}\" r=\"2.7\" fill=\"#ffffff\" stroke=\"{color}\" stroke-width=\"1.4\"/>\n"
            ));
        }
        marks.push_str(&format!(
            "<polyline points=\"{}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"2.0\" stroke-linejoin=\"round\" stroke-linecap=\"round\"/>\n\
             <polyline points=\"{}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"2.0\" stroke-dasharray=\"6 5\" stroke-linejoin=\"round\" stroke-linecap=\"round\"/>\n",
            escape_xml_text(write_points.trim()),
            escape_xml_text(read_points.trim()),
        ));
        marks.push_str(&point_marks);
        let legend_y = top + 30.0 + idx as f64 * 42.0;
        legend.push_str(&format!(
            "<line x1=\"{legend_x:.1}\" y1=\"{write_y:.1}\" x2=\"{legend_line_x:.1}\" y2=\"{write_y:.1}\" stroke=\"{color}\" stroke-width=\"2\"/>\n\
             <line x1=\"{legend_x:.1}\" y1=\"{read_y:.1}\" x2=\"{legend_line_x:.1}\" y2=\"{read_y:.1}\" stroke=\"{color}\" stroke-width=\"2\" stroke-dasharray=\"6 5\"/>\n\
             <text x=\"{legend_text_x:.1}\" y=\"{label_y:.1}\" font-family=\"Arial, sans-serif\" font-size=\"11\" fill=\"#222\">{label}</text>\n\
             <text x=\"{legend_text_x:.1}\" y=\"{read_label_y:.1}\" font-family=\"Arial, sans-serif\" font-size=\"10\" fill=\"#666\">solid write, dashed read</text>\n",
            legend_x = axis_right + 26.0,
            legend_line_x = axis_right + 56.0,
            legend_text_x = axis_right + 66.0,
            write_y = legend_y,
            read_y = legend_y + 12.0,
            label_y = legend_y + 4.0,
            read_label_y = legend_y + 18.0,
            label = escape_xml_text(label),
        ));
    }
    let tick_mid = max_mib / 2.0;
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" role=\"img\" aria-label=\"{}\" viewBox=\"0 0 {width:.0} {height:.0}\">\n\
         <rect width=\"{width:.0}\" height=\"{height:.0}\" fill=\"#ffffff\"/>\n\
         <text x=\"24\" y=\"34\" font-family=\"Arial, sans-serif\" font-size=\"18\" font-weight=\"700\" fill=\"#111\">{}</text>\n\
         <line x1=\"{left:.1}\" y1=\"{axis_y:.1}\" x2=\"{axis_right:.1}\" y2=\"{axis_y:.1}\" stroke=\"#111\" stroke-width=\"1.2\"/>\n\
         <line x1=\"{left:.1}\" y1=\"{top:.1}\" x2=\"{left:.1}\" y2=\"{axis_y:.1}\" stroke=\"#111\" stroke-width=\"1.2\"/>\n\
         <line x1=\"{left:.1}\" y1=\"{mid_y:.1}\" x2=\"{axis_right:.1}\" y2=\"{mid_y:.1}\" stroke=\"#d9ddd2\" stroke-width=\"1\"/>\n\
         <line x1=\"{left:.1}\" y1=\"{top:.1}\" x2=\"{axis_right:.1}\" y2=\"{top:.1}\" stroke=\"#edf0ea\" stroke-width=\"1\"/>\n\
         <text x=\"{tick_x:.1}\" y=\"{axis_y:.1}\" text-anchor=\"end\" font-family=\"Arial, sans-serif\" font-size=\"10\" fill=\"#555\">0</text>\n\
         <text x=\"{tick_x:.1}\" y=\"{mid_text_y:.1}\" text-anchor=\"end\" font-family=\"Arial, sans-serif\" font-size=\"10\" fill=\"#555\">{tick_mid:.1}</text>\n\
         <text x=\"{tick_x:.1}\" y=\"{top_text_y:.1}\" text-anchor=\"end\" font-family=\"Arial, sans-serif\" font-size=\"10\" fill=\"#555\">{max_mib:.1}</text>\n\
         <text x=\"{left:.1}\" y=\"{x_tick_y:.1}\" text-anchor=\"middle\" font-family=\"Arial, sans-serif\" font-size=\"10\" fill=\"#555\">0s</text>\n\
         <text x=\"{axis_right:.1}\" y=\"{x_tick_y:.1}\" text-anchor=\"middle\" font-family=\"Arial, sans-serif\" font-size=\"10\" fill=\"#555\">{max_second}s</text>\n\
         {marks}\
         {legend}\
         <text x=\"{x_label_x:.1}\" y=\"{x_label_y:.1}\" text-anchor=\"middle\" font-family=\"Arial, sans-serif\" font-size=\"13\" fill=\"#111\">Elapsed time (s)</text>\n\
         <text transform=\"translate(22 {y_label_y:.1}) rotate(-90)\" text-anchor=\"middle\" font-family=\"Arial, sans-serif\" font-size=\"13\" fill=\"#111\">IO rate (MiB/s)</text>\n\
         </svg>\n",
        escape_xml_text(title),
        escape_xml_text(title),
        mid_y = top + plot_height / 2.0,
        tick_x = left - 8.0,
        mid_text_y = top + plot_height / 2.0 + 4.0,
        top_text_y = top + 4.0,
        x_tick_y = axis_y + 18.0,
        marks = marks,
        legend = legend,
        x_label_x = left + plot_width / 2.0,
        x_label_y = height - 20.0,
        y_label_y = top + plot_height / 2.0,
    )
}

fn escape_xml_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn median_rate(values: impl Iterator<Item = f64>) -> f64 {
    let mut values = values.collect::<Vec<_>>();
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|left, right| left.total_cmp(right));
    values[values.len() / 2]
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn performance_hdd_tui_rates(scenario: &PerformanceScenarioResult) -> (Option<f64>, Vec<String>) {
    let mut by_disk = BTreeMap::<DiskId, (u64, f64)>::new();
    for row in &scenario.disk_results {
        let entry = by_disk.entry(row.disk_id.clone()).or_insert((0, 0.0));
        entry.0 = entry.0.saturating_add(row.write.bytes);
        entry.1 += row.write.seconds.max(0.001);
    }
    let mut aggregate = 0.0_f64;
    let mut labels = Vec::new();
    for (disk_id, (bytes, seconds)) in by_disk {
        let rate = bytes as f64 / seconds.max(0.001);
        aggregate += rate;
        labels.push(format!("{disk_id} {}/s", format_bytes(rate)));
    }
    let aggregate = if aggregate > 0.0 {
        Some(aggregate)
    } else {
        None
    };
    (aggregate, labels)
}

fn recommend_performance_strategy(
    results: &PerformanceBenchmarkResults,
) -> PerformanceRecommendation {
    let mut candidates = Vec::new();
    for scenario in results
        .ssd_pipeline
        .iter()
        .chain(results.direct_hdd.iter())
        .chain(results.ssd_stage_then_drain.iter())
    {
        candidates.push((
            scenario.kind,
            scenario.file_order,
            scenario.concurrency,
            scenario.total_bytes as f64 / scenario.elapsed_seconds.max(0.001),
        ));
    }
    if candidates.is_empty() {
        if let Some(ssd_only) = results.ssd_only.first() {
            candidates.push((
                PerformanceScenarioKind::SsdOnly,
                ssd_only.file_order,
                0,
                ssd_only.total_bytes as f64 / ssd_only.elapsed_seconds.max(0.001),
            ));
        }
    }
    let (strategy, file_order, hdd_concurrency, aggregate_bytes_per_second) = candidates
        .into_iter()
        .max_by(|left, right| left.3.total_cmp(&right.3))
        .unwrap_or((
            PerformanceScenarioKind::SsdPipeline,
            PerformanceFileOrder::SizeDesc,
            0,
            0.0,
        ));
    let ssd_only_write = results.ssd_only.first().map(|scenario| {
        median_rate(
            scenario
                .file_results
                .iter()
                .map(|row| throughput(row.ssd_write)),
        )
    });
    let ssd_only_context = ssd_only_write
        .map(|rate| format!("; SSD-only median write was {}/s", format_bytes(rate)))
        .unwrap_or_else(|| "; SSD-only was not included in this selected matrix".to_string());
    let reason = match strategy {
        PerformanceScenarioKind::SsdPipeline => format!(
            "overlapping SSD-first ingest was the highest observed landing strategy{ssd_only_context}"
        ),
        PerformanceScenarioKind::DirectHdd => format!(
            "direct-to-HDD bypass produced the highest observed aggregate rate while avoiding SSD backlog{ssd_only_context}"
        ),
        PerformanceScenarioKind::SsdStageThenDrain => format!(
            "separated SSD stage then HDD drain produced the highest observed aggregate rate among selected scenarios{ssd_only_context}"
        ),
        PerformanceScenarioKind::SsdOnly => {
            "only SSD-only ingest was measured; no HDD landing strategy was selected".to_string()
        }
    };
    PerformanceRecommendation {
        strategy,
        file_order,
        hdd_concurrency,
        aggregate_bytes_per_second,
        reason,
    }
}

impl PerformanceReport {
    fn selected_scenario_names(&self) -> Vec<&'static str> {
        let mut names = Vec::new();
        if !self.results.ssd_only.is_empty() {
            names.push(PerformanceScenarioKind::SsdOnly.as_str());
        }
        if !self.results.ssd_stage_then_drain.is_empty() {
            names.push(PerformanceScenarioKind::SsdStageThenDrain.as_str());
        }
        if !self.results.ssd_pipeline.is_empty() {
            names.push(PerformanceScenarioKind::SsdPipeline.as_str());
        }
        if !self.results.direct_hdd.is_empty() {
            names.push(PerformanceScenarioKind::DirectHdd.as_str());
        }
        names
    }

    fn selected_hdd_concurrency(&self) -> Vec<usize> {
        self.all_scenarios()
            .into_iter()
            .filter_map(|scenario| (scenario.concurrency > 0).then_some(scenario.concurrency))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    fn all_scenarios(&self) -> Vec<&PerformanceScenarioResult> {
        let mut scenarios = Vec::new();
        scenarios.extend(self.results.ssd_only.iter());
        scenarios.extend(self.results.ssd_stage_then_drain.iter());
        scenarios.extend(self.results.ssd_pipeline.iter());
        scenarios.extend(self.results.direct_hdd.iter());
        scenarios
    }
}

fn run_store_contents(args: &StoreContentsArgs, writer: &mut impl Write) -> Result<(), CliError> {
    if args.du() && args.tree() {
        return Err(CliError::UnsupportedStoreContentsFormat);
    }
    let live_sqlite_path =
        resolve_store_live_sqlite_path(args.store_id(), args.live_sqlite_path(), None)?;
    let mut request = StoreContentsRequest::new(live_sqlite_path, args.store_id().clone());
    if let Some(filter) = args.filter() {
        request = request.with_filter(filter);
    }
    let snapshot = read_store_contents(&request)?;
    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &snapshot)?;
        writer.write_all(b"\n")?;
    } else if args.tree() {
        write_store_contents_tree(&snapshot, args.depth(), writer)?;
    } else {
        write_store_contents_du(&snapshot, args.depth(), writer)?;
    }

    Ok(())
}

fn run_service_up(args: &ServiceComposeArgs, writer: &mut impl Write) -> Result<(), CliError> {
    run_docker_compose(args, ["up", "-d"], writer)?;
    if args.dry_run() {
        return Ok(());
    }
    writeln!(writer, "Object service started")?;

    Ok(())
}

fn run_service_down(args: &ServiceComposeArgs, writer: &mut impl Write) -> Result<(), CliError> {
    run_docker_compose(args, ["down"], writer)?;
    if args.dry_run() {
        return Ok(());
    }
    writeln!(writer, "Object service stopped")?;

    Ok(())
}

fn run_service_status(args: &ServiceStatusArgs, writer: &mut impl Write) -> Result<(), CliError> {
    if !args.json() {
        return Err(CliError::UnsupportedServiceStatusFormat);
    }

    let command = docker_compose_args(
        args.compose_file(),
        args.project_directory(),
        ["ps", "--format", "json"],
    );

    if args.dry_run() {
        let mut dry_run_command = vec!["docker".to_string()];
        dry_run_command.extend(command);
        serde_json::to_writer_pretty(
            &mut *writer,
            &serde_json::json!({
                "dry_run": true,
                "command": dry_run_command,
            }),
        )?;
        writer.write_all(b"\n")?;
        return Ok(());
    }

    let output = ProcessCommand::new("docker").args(&command).output()?;
    if !output.status.success() {
        return Err(CliError::CommandFailed(format!(
            "docker {} exited with status {}",
            command.join(" "),
            output.status
        )));
    }

    writer.write_all(&output.stdout)?;
    if !output.stdout.ends_with(b"\n") {
        writer.write_all(b"\n")?;
    }

    Ok(())
}

fn run_docker_compose(
    args: &ServiceComposeArgs,
    action_args: impl IntoIterator<Item = &'static str>,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let command = docker_compose_args(args.compose_file(), args.project_directory(), action_args);

    if args.dry_run() {
        writeln!(writer, "docker {}", command.join(" "))?;
        return Ok(());
    }

    let status = ProcessCommand::new("docker").args(&command).status()?;
    if !status.success() {
        return Err(CliError::CommandFailed(format!(
            "docker {} exited with status {}",
            command.join(" "),
            status
        )));
    }

    Ok(())
}

fn docker_compose_args(
    compose_file: &Path,
    project_directory: Option<&Path>,
    action_args: impl IntoIterator<Item = &'static str>,
) -> Vec<String> {
    let mut args = vec![
        "compose".to_string(),
        "-f".to_string(),
        compose_file.to_string_lossy().to_string(),
    ];

    if let Some(project_directory) = project_directory {
        args.push("--project-directory".to_string());
        args.push(project_directory.to_string_lossy().to_string());
    }

    args.extend(action_args.into_iter().map(String::from));
    args
}

fn run_probe(args: &ProbeArgs, writer: &mut impl Write) -> Result<(), CliError> {
    if args.json() && args.pretty() {
        return Err(CliError::UnsupportedProbeFormat);
    }

    let mut report = probe_current_platform()?;
    report.enclosures = group_enclosures(&report.disks);

    if args.json() {
        serde_json::to_writer(&mut *writer, &report)?;
        writer.write_all(b"\n")?;
    } else {
        write_pretty_report(&report, writer)?;
    }

    Ok(())
}

fn run_health(args: &HealthArgs, writer: &mut impl Write) -> Result<(), CliError> {
    let selected_modes = [
        args.summary(),
        args.verbose(),
        args.connections(),
        args.json(),
    ]
    .into_iter()
    .filter(|selected| *selected)
    .count();
    if selected_modes > 1 {
        return Err(CliError::UnsupportedHealthFormat);
    }

    if args.connections() {
        let report = read_current_platform_connection_status()?;
        write_host_connection_status(&report, writer)?;
    } else if args.json() {
        let report = read_current_platform_health()?;
        write_health_json(&report, writer)?;
    } else if args.verbose() {
        let report = read_current_platform_health()?;
        write_health_verbose(&report, writer)?;
    } else {
        let report = read_current_platform_health()?;
        write_health_summary(&report, writer)?;
    }

    Ok(())
}

fn run_status(args: &StatusArgs, writer: &mut impl Write) -> Result<(), CliError> {
    let report = read_runtime_status();
    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &report)?;
        writer.write_all(b"\n")?;
    } else {
        write_runtime_status(&report, writer)?;
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct RuntimeStatusReport {
    daemon: RuntimeEndpointStatus,
    web: RuntimeEndpointStatus,
    object_service: RuntimeEndpointStatus,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
struct RuntimeEndpointStatus {
    name: &'static str,
    kind: &'static str,
    configured: bool,
    active: bool,
    bind_address: Option<String>,
    port: Option<u16>,
    url: Option<String>,
    service: Option<String>,
    service_state: Option<String>,
    config_path: Option<String>,
    message: Option<String>,
}

fn read_runtime_status() -> RuntimeStatusReport {
    RuntimeStatusReport {
        daemon: daemon_runtime_status(),
        web: web_runtime_status(),
        object_service: object_service_runtime_status(),
    }
}

fn daemon_runtime_status() -> RuntimeEndpointStatus {
    let socket_path = DaemonRuntimeConfig::default().socket_path;
    let service_state = systemd_service_state("dasobjectstored.service");
    let active = socket_path.exists() || service_state.as_deref() == Some("active");
    RuntimeEndpointStatus {
        name: "daemon",
        kind: "unix_socket",
        configured: true,
        active,
        bind_address: None,
        port: None,
        url: Some(socket_path.display().to_string()),
        service: Some("dasobjectstored.service".to_string()),
        service_state,
        config_path: Some("/etc/dasobjectstore/daemon.json".to_string()),
        message: if active {
            None
        } else {
            Some("daemon socket is not available".to_string())
        },
    }
}

fn web_runtime_status() -> RuntimeEndpointStatus {
    let config_path = PathBuf::from(DEFAULT_STANDALONE_CONFIG_PATH);
    let config = fs::read_to_string(&config_path)
        .ok()
        .and_then(|contents| serde_json::from_str::<StandaloneServerConfig>(&contents).ok())
        .unwrap_or_else(StandaloneServerConfig::default);
    let socket_addr = config.socket_addr().ok();
    let active = socket_addr.is_some_and(local_tcp_listener_active);
    let service_state = systemd_service_state("dasobjectstore-server.service");
    RuntimeEndpointStatus {
        name: "web",
        kind: "https",
        configured: true,
        active,
        bind_address: Some(config.bind_address),
        port: Some(config.https_port),
        url: Some(config.public_base_url),
        service: Some("dasobjectstore-server.service".to_string()),
        service_state,
        config_path: Some(config_path.display().to_string()),
        message: if active {
            None
        } else {
            Some("web listener is not reachable locally".to_string())
        },
    }
}

fn object_service_runtime_status() -> RuntimeEndpointStatus {
    let port = 3900;
    let endpoint = format!("http://127.0.0.1:{port}");
    let active = "127.0.0.1:3900"
        .parse::<SocketAddr>()
        .is_ok_and(local_tcp_listener_active);
    RuntimeEndpointStatus {
        name: "object_service",
        kind: "s3_compatible",
        configured: true,
        active,
        bind_address: Some("127.0.0.1".to_string()),
        port: Some(port),
        url: Some(endpoint),
        service: None,
        service_state: None,
        config_path: None,
        message: if active {
            None
        } else {
            Some("S3-compatible object-service listener is not reachable locally".to_string())
        },
    }
}

fn local_tcp_listener_active(addr: SocketAddr) -> bool {
    let connect_addr = if addr.ip().is_unspecified() {
        SocketAddr::new("127.0.0.1".parse().expect("loopback IP"), addr.port())
    } else {
        addr
    };
    TcpStream::connect_timeout(&connect_addr, Duration::from_millis(200)).is_ok()
}

fn systemd_service_state(service: &str) -> Option<String> {
    let output = ProcessCommand::new("systemctl")
        .arg("is-active")
        .arg(service)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty())
}

fn write_runtime_status(
    report: &RuntimeStatusReport,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    writeln!(writer, "DASObjectStore status")?;
    write_runtime_endpoint_status(&report.daemon, writer)?;
    write_runtime_endpoint_status(&report.web, writer)?;
    write_runtime_endpoint_status(&report.object_service, writer)?;
    Ok(())
}

fn write_runtime_endpoint_status(
    endpoint: &RuntimeEndpointStatus,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    writeln!(
        writer,
        "- {}: {}{}",
        endpoint.name,
        if endpoint.active {
            "active"
        } else {
            "inactive"
        },
        endpoint
            .service_state
            .as_deref()
            .map(|state| format!(" (service {state})"))
            .unwrap_or_default()
    )?;
    if let Some(url) = &endpoint.url {
        writeln!(writer, "  url: {url}")?;
    }
    if let Some(bind_address) = &endpoint.bind_address {
        writeln!(
            writer,
            "  bind: {}:{}",
            bind_address,
            endpoint.port.unwrap_or_default()
        )?;
    }
    if let Some(config_path) = &endpoint.config_path {
        writeln!(writer, "  config: {config_path}")?;
    }
    if let Some(message) = &endpoint.message {
        writeln!(writer, "  note: {message}")?;
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct HostConnectionStatus {
    platform: HostPlatform,
    disks: Vec<DiskConnectionStatus>,
    warnings: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DiskConnectionStatus {
    device_path: Option<String>,
    model_hint: Option<String>,
    size_bytes: Option<u64>,
    transport: Transport,
    direct_attached_hint: Option<bool>,
    removable_hint: Option<bool>,
    enclosure_topology_path: Option<String>,
    assessment: ConnectionAssessment,
    warnings: Vec<String>,
    recommendation: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ConnectionAssessment {
    Good,
    Warning,
    Unknown,
}

impl ConnectionAssessment {
    fn as_str(self) -> &'static str {
        match self {
            Self::Good => "good",
            Self::Warning => "warning",
            Self::Unknown => "unknown",
        }
    }
}

impl DiskConnectionStatus {
    fn from_observed(disk: &ObservedDisk, preferred: Option<&PreferredConnectionPath>) -> Self {
        let mut warnings = Vec::new();
        let assessment = match disk.transport {
            Transport::Usb => {
                warnings.push(
                    "USB-attached DAS detected; this probe cannot verify negotiated USB link speed. Use a fast USB-C, USB 3.x, USB4, or Thunderbolt path because slow USB links will reduce ingest, destage, and object-service performance."
                        .to_string(),
                );
                ConnectionAssessment::Warning
            }
            Transport::Thunderbolt | Transport::Sata | Transport::Nvme => {
                ConnectionAssessment::Good
            }
            Transport::Unknown => {
                warnings.push(
                    "Disk transport is unknown; verify the DAS is not connected through a slow USB hub or fallback cable."
                        .to_string(),
                );
                ConnectionAssessment::Unknown
            }
        };
        let recommendation = connection_recommendation(disk, assessment, preferred);

        Self {
            device_path: disk.device_path.clone(),
            model_hint: disk.model_hint.clone(),
            size_bytes: disk.size_bytes,
            transport: disk.transport,
            direct_attached_hint: disk.direct_attached_hint,
            removable_hint: disk.removable_hint,
            enclosure_topology_path: disk.enclosure_topology_path.clone(),
            assessment,
            warnings,
            recommendation,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PreferredConnectionPath {
    device_path: Option<String>,
    transport: Transport,
    enclosure_topology_path: Option<String>,
}

fn read_current_platform_connection_status() -> Result<HostConnectionStatus, CliError> {
    let mut probe = probe_current_platform()?;
    probe.enclosures = group_enclosures(&probe.disks);

    Ok(connection_status_from_probe(&probe))
}

fn connection_status_from_probe(probe: &ProbeReport) -> HostConnectionStatus {
    let preferred = preferred_connection_path(&probe.disks);
    let disks: Vec<DiskConnectionStatus> = probe
        .disks
        .iter()
        .map(|disk| DiskConnectionStatus::from_observed(disk, preferred.as_ref()))
        .collect();
    let warnings: Vec<String> = probe
        .warnings
        .iter()
        .map(|warning| format!("{}: {}", warning.code, warning.message))
        .collect();

    HostConnectionStatus {
        platform: probe.platform.clone(),
        disks,
        warnings,
    }
}

fn preferred_connection_path(disks: &[ObservedDisk]) -> Option<PreferredConnectionPath> {
    disks
        .iter()
        .find(|disk| disk.transport == Transport::Thunderbolt)
        .map(|disk| PreferredConnectionPath {
            device_path: disk.device_path.clone(),
            transport: disk.transport,
            enclosure_topology_path: disk.enclosure_topology_path.clone(),
        })
}

fn connection_recommendation(
    disk: &ObservedDisk,
    assessment: ConnectionAssessment,
    preferred: Option<&PreferredConnectionPath>,
) -> Option<String> {
    if assessment == ConnectionAssessment::Good {
        return None;
    }

    if let Some(preferred) = preferred {
        if disk.device_path != preferred.device_path {
            return Some(format!(
                "Prefer the observed {} path used by {}{} for DAS workloads.",
                transport_label(preferred.transport),
                preferred
                    .device_path
                    .as_deref()
                    .unwrap_or("<unknown device>"),
                topology_suffix(preferred.enclosure_topology_path.as_deref())
            ));
        }
    }

    Some(
        "No faster attached DAS path is visible in this probe; move the DAS directly to a host USB-C, USB4, or Thunderbolt port and avoid hubs or fallback cables."
            .to_string(),
    )
}

fn transport_label(transport: Transport) -> &'static str {
    match transport {
        Transport::Usb => "USB",
        Transport::Thunderbolt => "Thunderbolt",
        Transport::Sata => "SATA",
        Transport::Nvme => "NVMe",
        Transport::Unknown => "unknown",
    }
}

fn topology_suffix(topology: Option<&str>) -> String {
    topology
        .map(|value| format!(" at topology {value}"))
        .unwrap_or_default()
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct HealthReport {
    platform: HostPlatform,
    disks: Vec<DiskHealthSummary>,
    warnings: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DiskHealthSummary {
    device_path: Option<String>,
    model_hint: Option<String>,
    serial_hint: Option<String>,
    size_bytes: Option<u64>,
    transport: Transport,
    smart_passed: Option<bool>,
    signals: HealthSignals,
    score: HealthScore,
    warnings: Vec<String>,
}

impl DiskHealthSummary {
    fn from_observed(
        observed: &ObservedDisk,
        health_report: Result<DiskHealthReport, ProbeError>,
    ) -> Self {
        let mut warnings = Vec::new();
        let mut health = None;

        if observed.device_path.is_none() {
            warnings.push("disk has no device path; SMART health was not queried".to_string());
        }

        match health_report {
            Ok(report) => {
                warnings.extend(report.warnings.clone());
                health = Some(report);
            }
            Err(err) => warnings.push(err.to_string()),
        }

        let signals = health
            .as_ref()
            .map(|report| report.signals)
            .unwrap_or_default();
        let score = HealthScore::from_signals(&signals);

        Self {
            device_path: health
                .as_ref()
                .and_then(|report| report.device_path.clone())
                .or_else(|| observed.device_path.clone()),
            model_hint: health
                .as_ref()
                .and_then(|report| report.model_hint.clone())
                .or_else(|| observed.model_hint.clone()),
            serial_hint: health
                .as_ref()
                .and_then(|report| report.serial_hint.clone())
                .or_else(|| observed.serial_hint.clone()),
            size_bytes: observed.size_bytes,
            transport: observed.transport,
            smart_passed: health.as_ref().and_then(|report| report.smart_passed),
            signals,
            score,
            warnings,
        }
    }
}

fn read_current_platform_health() -> Result<HealthReport, CliError> {
    let mut probe = probe_current_platform()?;
    probe.enclosures = group_enclosures(&probe.disks);

    let runner = SystemCommandRunner;
    let disks = probe
        .disks
        .iter()
        .map(|disk| {
            let health_report = disk
                .device_path
                .as_deref()
                .map(|device_path| read_disk_health_for_current_platform(&runner, device_path))
                .unwrap_or_else(|| {
                    Err(ProbeError::ParseFailed {
                        source: "health".to_string(),
                        message: "disk has no device path".to_string(),
                    })
                });
            DiskHealthSummary::from_observed(disk, health_report)
        })
        .collect();

    Ok(HealthReport {
        platform: probe.platform,
        disks,
        warnings: probe
            .warnings
            .into_iter()
            .map(|warning| format!("{}: {}", warning.code, warning.message))
            .collect(),
    })
}

#[cfg(target_os = "linux")]
fn read_disk_health_for_current_platform(
    runner: &SystemCommandRunner,
    device_path: &str,
) -> Result<DiskHealthReport, ProbeError> {
    read_smartctl_health(runner, device_path)
}

#[cfg(target_os = "macos")]
fn read_disk_health_for_current_platform(
    runner: &SystemCommandRunner,
    device_path: &str,
) -> Result<DiskHealthReport, ProbeError> {
    read_diskutil_health(runner, device_path)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn read_disk_health_for_current_platform(
    _runner: &SystemCommandRunner,
    _device_path: &str,
) -> Result<DiskHealthReport, ProbeError> {
    Err(ProbeError::UnsupportedPlatform {
        platform: std::env::consts::OS.to_string(),
    })
}

fn run_pool_inspect(args: &PoolInspectArgs, writer: &mut impl Write) -> Result<(), CliError> {
    let summary = inspect_pool_metadata(args.metadata_path())?;
    write_pool_inspect_summary(&summary, writer)?;

    Ok(())
}

fn run_pool_import(args: &PoolImportArgs, writer: &mut impl Write) -> Result<(), CliError> {
    if !args.read_only() {
        return Err(CliError::UnsupportedPoolImportMode);
    }

    let summary = inspect_pool_metadata(args.source_path())?;
    let options = ReadOnlyAttachOptions::new(
        args.source_path(),
        args.recovery_metadata_dir(),
        args.recorded_at_utc().to_string(),
    );
    let report = match summary.state {
        PoolState::Clean => attach_clean_pool_read_only(&options)?,
        PoolState::Dirty => import_dirty_pool_read_only(&options)?,
        state => return Err(CliError::UnsupportedPoolImportState { state }),
    };

    write_pool_import_report(&report, writer)?;

    Ok(())
}

fn run_pool_repair(args: &PoolRepairArgs, writer: &mut impl Write) -> Result<(), CliError> {
    if !args.dry_run() {
        return Err(CliError::UnsupportedPoolRepairMode);
    }

    let summary = inspect_pool_metadata(args.source_path())?;
    write_pool_repair_dry_run(&summary, writer)?;

    Ok(())
}

fn run_disk_retire(args: &DiskRetireArgs, writer: &mut impl Write) -> Result<(), CliError> {
    let report = request_disk_retirement(
        args.live_sqlite_path(),
        args.disk_id(),
        args.recorded_at_utc().to_string(),
    )?;
    write_disk_retirement_report(&report, writer)?;

    Ok(())
}

fn run_disk_force_retire(
    args: &DiskForceRetireArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let report = force_retire_disk(
        args.live_sqlite_path(),
        args.disk_id(),
        args.recorded_at_utc().to_string(),
        RiskPolicy {
            allow_force_retire: args.allow_force_retire(),
            ..RiskPolicy::default()
        },
        &ActionConfirmation::new(args.confirm()),
    )?;
    write_disk_force_retirement_report(&report, writer)?;

    Ok(())
}

fn run_disk_lockdown_das(
    args: &DiskLockdownDasArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    if !args.dry_run() && args.confirm() != LOCKDOWN_CONFIRMATION {
        return Err(CliError::CommandFailed(format!(
            "action confirmation mismatch; pass `{LOCKDOWN_CONFIRMATION}`"
        )));
    }

    let report = lockdown_das(&LockdownDasRequest {
        mount_root: args.mount_root().to_path_buf(),
        service_user: args.service_user().to_string(),
        service_group: args.service_group().to_string(),
        create_service_user: args.create_service_user(),
        dry_run: args.dry_run(),
    })?;
    write_lockdown_das_report(&report, writer)?;

    Ok(())
}

fn run_disk_prepare_das(
    args: &DiskPrepareDasArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    if !args.dry_run() {
        if !args.acknowledge_existing_data() {
            return Err(CliError::CommandFailed(
                "existing data acknowledgement is required; pass `--acknowledge-existing-data`"
                    .to_string(),
            ));
        }
        RiskGate::new(RiskPolicy {
            allow_prepare_das: args.allow_format(),
            ..RiskPolicy::default()
        })
        .evaluate(
            RiskyOperation::PrepareDas,
            &ActionConfirmation::new(args.confirm()),
        )?;
    }

    let request = PrepareDasRequest {
        devices: prepare_das_devices(args)?,
        mount_root: args.mount_root().to_path_buf(),
        filesystem: prepare_filesystem(args.filesystem()),
        owner: args.owner().map(ToOwned::to_owned),
        dry_run: args.dry_run(),
    };
    let report = prepare_das(&request)?;
    write_prepare_das_report(&report, writer)?;

    Ok(())
}

fn prepare_das_devices(args: &DiskPrepareDasArgs) -> Result<Vec<PrepareDasDevice>, CliError> {
    let mut devices = vec![PrepareDasDevice {
        role: PrepareDasRole::Ssd,
        device_path: args.ssd_device().to_path_buf(),
    }];
    for (index, value) in args.hdd_devices().iter().enumerate() {
        let (disk_id, device_path) =
            value
                .split_once('=')
                .ok_or_else(|| CliError::InvalidDeviceMapping {
                    value: value.clone(),
                })?;
        let disk_id = DiskId::new(disk_id).map_err(|_| CliError::InvalidDeviceMapping {
            value: value.clone(),
        })?;
        if device_path.is_empty() {
            return Err(CliError::InvalidDeviceMapping {
                value: value.clone(),
            });
        }
        devices.push(PrepareDasDevice {
            role: PrepareDasRole::Hdd {
                disk_id,
                ordinal: index + 1,
            },
            device_path: Path::new(device_path).to_path_buf(),
        });
    }

    Ok(devices)
}

fn prepare_filesystem(filesystem: DiskPrepareFilesystem) -> PrepareFilesystem {
    match filesystem {
        DiskPrepareFilesystem::Ext4 => PrepareFilesystem::Ext4,
    }
}

fn run_disk_drain(args: &DiskDrainArgs, writer: &mut impl Write) -> Result<(), CliError> {
    let plan = read_disk_drain_plan(args.live_sqlite_path(), args.disk_id())?;

    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &plan)?;
        writer.write_all(b"\n")?;
    } else {
        write_disk_drain_plan(&plan, writer)?;
    }

    Ok(())
}

fn run_disk_replace(args: &DiskReplaceArgs, writer: &mut impl Write) -> Result<(), CliError> {
    let plan = read_disk_replacement_plan(
        args.live_sqlite_path(),
        args.old_disk_id(),
        args.new_disk_id(),
    )?;

    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &plan)?;
        writer.write_all(b"\n")?;
    } else {
        write_disk_replacement_plan(&plan, writer)?;
    }

    Ok(())
}

fn run_store_validate(args: &StoreValidateArgs, writer: &mut impl Write) -> Result<(), CliError> {
    let file = File::open(args.policy_file())?;
    let policy: StorePolicy = serde_json::from_reader(file)?;

    policy.validate()?;
    writeln!(writer, "Store policy is valid: {}", policy.class.name())?;

    Ok(())
}

fn run_store_create(args: &StoreCreateArgs, writer: &mut impl Write) -> Result<(), CliError> {
    let mut policy = StorePolicy::defaults_for(args.class());
    if let Some(copies) = args.copies() {
        policy.copies = copies;
    }
    policy.validate()?;
    enforce_supported_das_for_store_create(args)?;

    let definition = StoreServiceDefinition {
        store_id: args.store_id().clone(),
        policy,
        bucket_name: args.bucket().map(ToOwned::to_owned),
        writer_group: args.writer_group().map(ToOwned::to_owned),
    };
    let registry_path = args
        .registry_path()
        .map(Path::to_path_buf)
        .unwrap_or_else(default_store_registry_path);
    let report = upsert_store_definition(&registry_path, definition)?;
    let allow_default_ssd = args.registry_path().is_none() || args.ssd_root().is_some();
    let portable_report =
        upsert_portable_store_definition(args.ssd_root(), allow_default_ssd, &report.definition)?;
    if let Some(writer_group) = &report.definition.writer_group {
        grant_store_writer_group_access(args.ssd_root(), allow_default_ssd, writer_group)?;
        grant_writer_group_registry_access(&registry_path, writer_group)?;
        grant_writer_group_registry_access(&default_subobject_registry_path(), writer_group)?;
    }

    if args.json() {
        serde_json::to_writer_pretty(
            &mut *writer,
            &serde_json::json!({
                "host": report,
                "portable": portable_report,
            }),
        )?;
        writer.write_all(b"\n")?;
    } else {
        write_store_create_report(&report, writer)?;
        match &portable_report {
            Some(report) => writeln!(
                writer,
                "Portable registry: {}",
                report.registry_path.to_string_lossy()
            )?,
            None => writeln!(writer, "Portable registry: not detected")?,
        }
    }

    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ManagedHddDevice {
    disk_id: DiskId,
    root_path: PathBuf,
    device_path: PathBuf,
}

fn enforce_supported_das_for_store_create(args: &StoreCreateArgs) -> Result<(), CliError> {
    if args.registry_path().is_some() {
        return Ok(());
    }

    let managed_hdds = managed_hdd_devices(&default_hdd_root())?;
    let mut report = probe_current_platform()?;
    report.enclosures = group_enclosures(&report.disks);
    validate_managed_hdds_on_supported_das(&managed_hdds, &report)
}

fn managed_hdd_devices(hdd_root: &Path) -> Result<Vec<ManagedHddDevice>, CliError> {
    let roots = discover_managed_hdd_roots(hdd_root)?;
    let mut devices = Vec::new();

    for root in roots {
        let marker = read_device_marker(&root.root_path)?;
        let device_path = device_path_from_marker(&marker).ok_or_else(|| {
            CliError::CommandFailed(format!(
                "managed HDD {} at {} is missing device= in .dasobjectstore/device.env",
                root.disk_id,
                root.root_path.display()
            ))
        })?;
        devices.push(ManagedHddDevice {
            disk_id: root.disk_id,
            root_path: root.root_path,
            device_path: PathBuf::from(device_path),
        });
    }

    Ok(devices)
}

fn validate_managed_hdds_on_supported_das(
    managed_hdds: &[ManagedHddDevice],
    report: &ProbeReport,
) -> Result<(), CliError> {
    if managed_hdds.is_empty() {
        return Err(CliError::CommandFailed(
            "object store creation requires at least one managed HDD on a supported, identifiable DAS enclosure; currently supported: QNAP TL-D800C".to_string(),
        ));
    }

    let supported_topology_paths = supported_das_topology_paths(report);
    if supported_topology_paths.is_empty() {
        return Err(CliError::CommandFailed(
            "object store creation requires supported, identifiable DAS enclosure mapping; no QNAP TL-D800C enclosure was detected in the current probe".to_string(),
        ));
    }

    for managed_hdd in managed_hdds {
        let Some(disk) = report
            .disks
            .iter()
            .find(|disk| probed_disk_matches_device(disk, &managed_hdd.device_path))
        else {
            return Err(CliError::CommandFailed(format!(
                "managed HDD {} at {} points to {}, but that device was not found in the current probe",
                managed_hdd.disk_id,
                managed_hdd.root_path.display(),
                managed_hdd.device_path.display()
            )));
        };

        let Some(topology_path) = disk.enclosure_topology_path.as_deref() else {
            return Err(CliError::CommandFailed(format!(
                "managed HDD {} at {} is not mapped to a supported DAS enclosure; currently supported: QNAP TL-D800C",
                managed_hdd.disk_id,
                managed_hdd.root_path.display()
            )));
        };

        if !supported_topology_paths.contains(topology_path) {
            return Err(CliError::CommandFailed(format!(
                "managed HDD {} at {} is mapped to unsupported enclosure topology {}; currently supported: QNAP TL-D800C",
                managed_hdd.disk_id,
                managed_hdd.root_path.display(),
                topology_path
            )));
        }
    }

    Ok(())
}

fn supported_das_topology_paths(report: &ProbeReport) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    for enclosure in &report.enclosures {
        if enclosure.identity.vendor_hint.as_deref() == Some("QNAP")
            && enclosure.identity.product_hint.as_deref() == Some("TL-D800C")
        {
            if let Some(topology_path) = enclosure.identity.usb_topology_path.as_deref() {
                paths.insert(format!("qnap-tl-d800c@{topology_path}"));
            }
        }
    }
    paths
}

fn probed_disk_matches_device(disk: &ObservedDisk, expected_device_path: &Path) -> bool {
    let Some(probed_path) = disk.device_path.as_deref() else {
        return false;
    };
    paths_refer_to_same_device(Path::new(probed_path), expected_device_path)
}

fn paths_refer_to_same_device(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn device_path_from_marker(marker: &str) -> Option<String> {
    marker
        .lines()
        .find_map(|line| line.strip_prefix("device=").map(ToOwned::to_owned))
}

#[derive(Debug, serde::Serialize)]
pub(super) struct StoreDeleteCommandReport {
    metadata: dasobjectstore_metadata::StoreDeleteReport,
    host_registry: StoreRegistryDeleteReport,
    portable_registry: Option<StoreRegistryDeleteReport>,
    host_subobjects: SubObjectRegistryStoreDeleteReport,
    portable_subobjects: Option<SubObjectRegistryStoreDeleteReport>,
}

fn run_store_drain(args: &StoreDrainArgs, writer: &mut impl Write) -> Result<(), CliError> {
    require_admin_for_destructive_store_action(args.dry_run())?;
    if !args.dry_run() {
        RiskGate::new(RiskPolicy {
            allow_store_drain: args.allow_store_drain(),
            ..RiskPolicy::default()
        })
        .evaluate(
            RiskyOperation::StoreDrain,
            &ActionConfirmation::new(args.confirm()),
        )?;
    }

    let hdd_root = args
        .hdd_root()
        .map(Path::to_path_buf)
        .unwrap_or_else(default_hdd_root);
    let live_sqlite_path = resolve_store_live_sqlite_path(
        args.store_id(),
        args.live_sqlite_path(),
        args.registry_path(),
    )?;
    let report = drain_store(&StoreDrainRequest {
        live_sqlite_path,
        store_id: args.store_id().clone(),
        disk_roots: discover_managed_hdd_roots(&hdd_root)?,
        dry_run: args.dry_run(),
    })?;

    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &report)?;
        writer.write_all(b"\n")?;
    } else {
        write_store_drain_report(&report, writer)?;
    }

    Ok(())
}

fn run_store_delete(args: &StoreDeleteArgs, writer: &mut impl Write) -> Result<(), CliError> {
    require_admin_for_destructive_store_action(args.dry_run())?;
    if !args.dry_run() {
        RiskGate::new(RiskPolicy {
            allow_store_delete: args.allow_store_delete(),
            ..RiskPolicy::default()
        })
        .evaluate(
            RiskyOperation::StoreDelete,
            &ActionConfirmation::new(args.confirm()),
        )?;
    }

    let hdd_root = args
        .hdd_root()
        .map(Path::to_path_buf)
        .unwrap_or_else(default_hdd_root);
    let metadata_report = delete_store(&StoreDeleteRequest {
        live_sqlite_path: args.live_sqlite_path().to_path_buf(),
        store_id: args.store_id().clone(),
        disk_roots: discover_managed_hdd_roots(&hdd_root)?,
        dry_run: args.dry_run(),
    })?;
    let host_registry_path = args
        .registry_path()
        .map(Path::to_path_buf)
        .unwrap_or_else(default_store_registry_path);
    let host_subobject_registry_path = args
        .subobject_registry_path()
        .map(Path::to_path_buf)
        .unwrap_or_else(default_subobject_registry_path);
    let host_registry_report =
        delete_store_definition_maybe(&host_registry_path, args.store_id(), args.dry_run())?;
    let host_subobject_report = delete_subobjects_for_store_maybe(
        &host_subobject_registry_path,
        args.store_id(),
        args.dry_run(),
    )?;
    let allow_default_ssd = args.registry_path().is_none() || args.ssd_root().is_some();
    let portable_registry_report = delete_portable_store_definition(
        args.ssd_root(),
        allow_default_ssd,
        args.store_id(),
        args.dry_run(),
    )?;
    let portable_subobject_report = delete_portable_subobjects(
        args.ssd_root(),
        allow_default_ssd,
        args.store_id(),
        args.dry_run(),
    )?;
    let report = StoreDeleteCommandReport {
        metadata: metadata_report,
        host_registry: host_registry_report,
        portable_registry: portable_registry_report,
        host_subobjects: host_subobject_report,
        portable_subobjects: portable_subobject_report,
    };

    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &report)?;
        writer.write_all(b"\n")?;
    } else {
        write_store_delete_report(&report, writer)?;
    }

    Ok(())
}

fn require_admin_for_destructive_store_action(dry_run: bool) -> Result<(), CliError> {
    if dry_run || current_user_is_root()? {
        return Ok(());
    }

    Err(CliError::CommandFailed(
        "destructive storage cleanup requires an administrative user; rerun with sudo".to_string(),
    ))
}

fn delete_store_definition_maybe(
    path: &Path,
    store_id: &StoreId,
    dry_run: bool,
) -> Result<StoreRegistryDeleteReport, CliError> {
    if dry_run {
        let removed = read_store_registry(path)?
            .iter()
            .any(|definition| &definition.store_id == store_id);
        return Ok(StoreRegistryDeleteReport {
            registry_path: path.to_path_buf(),
            store_id: store_id.clone(),
            removed,
        });
    }

    Ok(delete_store_definition(path, store_id)?)
}

fn delete_subobjects_for_store_maybe(
    path: &Path,
    store_id: &StoreId,
    dry_run: bool,
) -> Result<SubObjectRegistryStoreDeleteReport, CliError> {
    if dry_run {
        let mut removed_names = read_subobject_registry(path)?
            .iter()
            .filter(|definition| &definition.store_id == store_id)
            .map(|definition| definition.name.clone())
            .collect::<Vec<_>>();
        removed_names.sort();
        return Ok(SubObjectRegistryStoreDeleteReport {
            registry_path: path.to_path_buf(),
            store_id: store_id.clone(),
            removed_count: removed_names.len(),
            removed_names,
        });
    }

    Ok(delete_subobjects_for_store(path, store_id)?)
}

fn delete_portable_store_definition(
    ssd_root: Option<&Path>,
    allow_default_ssd: bool,
    store_id: &StoreId,
    dry_run: bool,
) -> Result<Option<StoreRegistryDeleteReport>, CliError> {
    let Some(ssd_root) = known_ssd_root_for_optional_mirror(ssd_root, allow_default_ssd)? else {
        return Ok(None);
    };
    let registry_path = portable_store_registry_path(&ssd_root);
    Ok(Some(delete_store_definition_maybe(
        &registry_path,
        store_id,
        dry_run,
    )?))
}

fn delete_portable_subobjects(
    ssd_root: Option<&Path>,
    allow_default_ssd: bool,
    store_id: &StoreId,
    dry_run: bool,
) -> Result<Option<SubObjectRegistryStoreDeleteReport>, CliError> {
    let Some(ssd_root) = known_ssd_root_for_optional_mirror(ssd_root, allow_default_ssd)? else {
        return Ok(None);
    };
    let registry_path = portable_subobject_registry_path(&ssd_root);
    Ok(Some(delete_subobjects_for_store_maybe(
        &registry_path,
        store_id,
        dry_run,
    )?))
}

fn run_store_adopt(args: &StoreAdoptArgs, writer: &mut impl Write) -> Result<(), CliError> {
    let ssd_root = known_ssd_root_for_adopt(args.ssd_root())?;
    let portable_registry_path = portable_store_registry_path(&ssd_root);
    let definitions = read_store_registry(&portable_registry_path)?;
    if definitions.is_empty() {
        return Err(CliError::PortableRegistry(format!(
            "portable store registry is empty at {}",
            portable_registry_path.display()
        )));
    }

    let host_registry_path = args
        .registry_path()
        .map(Path::to_path_buf)
        .unwrap_or_else(default_store_registry_path);
    let mut reports = Vec::new();
    for definition in definitions {
        reports.push(upsert_store_definition(
            &host_registry_path,
            definition.clone(),
        )?);
    }

    if args.json() {
        serde_json::to_writer_pretty(
            &mut *writer,
            &serde_json::json!({
                "ssd_root": ssd_root,
                "portable_registry_path": portable_registry_path,
                "host_registry_path": host_registry_path,
                "adopted": reports,
            }),
        )?;
        writer.write_all(b"\n")?;
    } else {
        writeln!(writer, "Portable store registry adopted")?;
        writeln!(writer, "SSD root: {}", ssd_root.to_string_lossy())?;
        writeln!(
            writer,
            "Portable registry: {}",
            portable_registry_path.to_string_lossy()
        )?;
        writeln!(
            writer,
            "Host registry: {}",
            host_registry_path.to_string_lossy()
        )?;
        writeln!(writer, "Stores adopted: {}", reports.len())?;
        for report in &reports {
            writeln!(
                writer,
                "- {} action={} class={} copies={}",
                report.definition.store_id,
                report.action.as_str(),
                report.definition.policy.class.name(),
                report.definition.policy.copies
            )?;
        }
    }

    Ok(())
}

fn run_store_list(args: &StoreListArgs, writer: &mut impl Write) -> Result<(), CliError> {
    let registry_path = if args.portable() {
        let ssd_root = known_ssd_root_for_adopt(args.ssd_root())?;
        portable_store_registry_path(ssd_root)
    } else {
        args.registry_path()
            .map(Path::to_path_buf)
            .unwrap_or_else(default_store_registry_path)
    };
    let definitions = read_store_registry(&registry_path)?;

    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &definitions)?;
        writer.write_all(b"\n")?;
    } else {
        write_store_list_report(&definitions, writer)?;
    }

    Ok(())
}

fn write_store_contents_du(
    snapshot: &StoreContentsSnapshot,
    depth: usize,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    writeln!(writer, "Store contents")?;
    writeln!(writer, "Store: {}", snapshot.store_id)?;
    writeln!(
        writer,
        "Live metadata: {}",
        snapshot.live_sqlite_path.display()
    )?;
    if let Some(filter) = &snapshot.filter {
        writeln!(writer, "Filter: {filter}")?;
    }
    writeln!(writer, "Objects: {}", snapshot.objects.len())?;
    writeln!(
        writer,
        "Total: {}",
        format_bytes(snapshot.total_size_bytes() as f64)
    )?;
    writeln!(writer, "Mode: du depth={depth}")?;
    for (path, size_bytes) in store_contents_du_entries(&snapshot.objects, depth) {
        writeln!(writer, "{}\t{path}", format_bytes(size_bytes as f64))?;
    }

    Ok(())
}

fn write_store_contents_tree(
    snapshot: &StoreContentsSnapshot,
    depth: usize,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    writeln!(writer, "Store contents")?;
    writeln!(writer, "Store: {}", snapshot.store_id)?;
    writeln!(
        writer,
        "Live metadata: {}",
        snapshot.live_sqlite_path.display()
    )?;
    if let Some(filter) = &snapshot.filter {
        writeln!(writer, "Filter: {filter}")?;
    }
    writeln!(writer, "Objects: {}", snapshot.objects.len())?;
    writeln!(
        writer,
        "Total: {}",
        format_bytes(snapshot.total_size_bytes() as f64)
    )?;
    writeln!(writer, "Mode: tree depth={depth}")?;
    let tree = StoreContentsTreeNode::from_objects(&snapshot.objects);
    writeln!(writer, ". {}", format_bytes(tree.size_bytes as f64))?;
    write_store_contents_tree_children(&tree, 1, depth, writer)?;

    Ok(())
}

fn store_contents_du_entries(objects: &[StoreContentsObject], depth: usize) -> Vec<(String, u64)> {
    let mut entries = BTreeMap::<String, u64>::new();
    entries.insert(
        ".".to_string(),
        objects.iter().map(|object| object.size_bytes).sum(),
    );
    if depth == 0 {
        return entries.into_iter().collect();
    }

    for object in objects {
        let parts = store_contents_path_parts(&object.path);
        let max_depth = depth.min(parts.len());
        for prefix_depth in 1..=max_depth {
            let prefix = parts[..prefix_depth].join("/");
            *entries.entry(prefix).or_insert(0) += object.size_bytes;
        }
    }

    entries.into_iter().collect()
}

#[derive(Default)]
struct StoreContentsTreeNode {
    size_bytes: u64,
    file_size_bytes: Option<u64>,
    children: BTreeMap<String, StoreContentsTreeNode>,
}

impl StoreContentsTreeNode {
    fn from_objects(objects: &[StoreContentsObject]) -> Self {
        let mut root = Self::default();
        for object in objects {
            root.insert(&store_contents_path_parts(&object.path), object.size_bytes);
        }
        root
    }

    fn insert(&mut self, parts: &[String], size_bytes: u64) {
        self.size_bytes = self.size_bytes.saturating_add(size_bytes);
        if let Some((head, tail)) = parts.split_first() {
            self.children
                .entry(head.clone())
                .or_default()
                .insert(tail, size_bytes);
        } else {
            self.file_size_bytes = Some(size_bytes);
        }
    }
}

fn write_store_contents_tree_children(
    node: &StoreContentsTreeNode,
    current_depth: usize,
    max_depth: usize,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    if current_depth > max_depth {
        return Ok(());
    }

    for (name, child) in &node.children {
        let indent = "  ".repeat(current_depth.saturating_sub(1));
        if child.children.is_empty() {
            writeln!(
                writer,
                "{indent}- {name} {}",
                format_bytes(child.size_bytes as f64)
            )?;
        } else {
            writeln!(
                writer,
                "{indent}- {name}/ {}",
                format_bytes(child.size_bytes as f64)
            )?;
            write_store_contents_tree_children(child, current_depth + 1, max_depth, writer)?;
        }
    }

    Ok(())
}

fn store_contents_path_parts(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|part| !part.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn run_store_s3_upload(args: &StoreS3UploadArgs, writer: &mut impl Write) -> Result<(), CliError> {
    let (bucket_name, credential_reference) = match args.bucket() {
        Some(bucket_name) => (
            bucket_name.to_string(),
            credential_reference_for_store(args.store_id()),
        ),
        None => {
            let registry_path = args
                .registry_path()
                .map(Path::to_path_buf)
                .unwrap_or_else(default_store_registry_path);
            let definitions = read_store_registry(&registry_path)?;
            let definition = definitions
                .iter()
                .find(|definition| definition.store_id == *args.store_id())
                .cloned()
                .ok_or_else(|| {
                    CliError::CommandFailed(format!(
                        "store {} was not found in {}",
                        args.store_id(),
                        registry_path.display()
                    ))
                })?;
            let layout = plan_store_service_layout(&[definition])?;
            let binding = layout.bucket_bindings.into_iter().next().ok_or_else(|| {
                CliError::CommandFailed(format!("store {} is not S3-exported", args.store_id()))
            })?;
            (binding.bucket_name, binding.credential_reference)
        }
    };
    let profile_name = args
        .profile()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("dasobjectstore-{}", args.store_id()));
    let plan = plan_remote_s3_upload(RemoteS3UploadPlanRequest {
        store_id: args.store_id().clone(),
        bucket_name,
        endpoint_url: args.endpoint_url().to_string(),
        region: args.region().to_string(),
        profile_name,
        credential_reference,
        auth_authority: args.auth().into(),
        username: args.username().map(ToOwned::to_owned),
    })?;

    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &plan)?;
        writer.write_all(b"\n")?;
    } else {
        write_remote_s3_upload_plan(&plan, writer)?;
    }

    Ok(())
}

fn upsert_portable_store_definition(
    ssd_root: Option<&Path>,
    allow_default_ssd: bool,
    definition: &StoreServiceDefinition,
) -> Result<Option<StoreRegistryUpdateReport>, CliError> {
    let Some(ssd_root) = known_ssd_root_for_optional_mirror(ssd_root, allow_default_ssd)? else {
        return Ok(None);
    };
    let registry_path = portable_store_registry_path(&ssd_root);
    let report = upsert_store_definition(&registry_path, definition.clone())?;

    Ok(Some(report))
}

fn known_ssd_root_for_optional_mirror(
    ssd_root: Option<&Path>,
    allow_default_ssd: bool,
) -> Result<Option<PathBuf>, CliError> {
    match ssd_root {
        Some(path) => {
            validate_known_ssd_root(path)?;
            Ok(Some(path.to_path_buf()))
        }
        None => {
            if !allow_default_ssd {
                return Ok(None);
            }
            let path = default_ssd_root();
            if is_known_ssd_root(&path) {
                Ok(Some(path))
            } else {
                Ok(None)
            }
        }
    }
}

fn grant_store_writer_group_access(
    ssd_root: Option<&Path>,
    allow_default_ssd: bool,
    writer_group: &str,
) -> Result<(), CliError> {
    #[cfg(target_os = "linux")]
    {
        ensure_group_exists(writer_group)?;
        let mut roots = Vec::new();
        if let Some(ssd_root) = known_ssd_root_for_optional_mirror(ssd_root, allow_default_ssd)? {
            roots.push(ssd_root);
        }
        roots.extend(
            discover_managed_hdd_roots(&default_hdd_root())?
                .into_iter()
                .map(|root| root.root_path),
        );
        for root in roots {
            grant_group_acl(&root, writer_group)?;
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = ssd_root;
        let _ = allow_default_ssd;
        let _ = writer_group;
    }

    Ok(())
}

fn grant_subobject_writer_group_registry_access(
    args: &SubobjectCreateArgs,
    definition: &SubObjectDefinition,
    registry_path: &Path,
) -> Result<(), CliError> {
    let stores_registry_path = args
        .stores_registry_path()
        .map(Path::to_path_buf)
        .unwrap_or_else(default_store_registry_path);
    let stores = read_store_registry(&stores_registry_path)?;
    let Some(store) = stores
        .iter()
        .find(|store| store.store_id == definition.store_id)
    else {
        return Ok(());
    };
    let Some(writer_group) = &store.writer_group else {
        return Ok(());
    };

    grant_writer_group_registry_access(registry_path, writer_group)
}

fn grant_writer_group_registry_access(
    registry_path: &Path,
    writer_group: &str,
) -> Result<(), CliError> {
    #[cfg(target_os = "linux")]
    {
        ensure_group_exists(writer_group)?;
        if let Some(parent) = registry_path.parent() {
            grant_group_read_dir_acl(parent, writer_group)?;
        }
        if registry_path.is_file() {
            grant_group_read_file_acl(registry_path, writer_group)?;
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = registry_path;
        let _ = writer_group;
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn ensure_group_exists(group: &str) -> Result<(), CliError> {
    let status = ProcessCommand::new("getent")
        .args(["group", group])
        .status()?;
    if status.success() {
        return Ok(());
    }

    Err(CliError::CommandFailed(format!(
        "writer group does not exist: {group}"
    )))
}

#[cfg(target_os = "linux")]
fn grant_group_acl(root: &Path, group: &str) -> Result<(), CliError> {
    let acl = format!("g:{group}:rwx");
    let default_acl = format!("d:g:{group}:rwx");
    let status = ProcessCommand::new("setfacl")
        .args(["-R", "-m", &acl, "-m", &default_acl])
        .arg(root)
        .status()?;
    if status.success() {
        return Ok(());
    }

    Err(CliError::CommandFailed(format!(
        "setfacl failed for {} with status {}",
        root.display(),
        status
    )))
}

#[cfg(target_os = "linux")]
fn grant_group_read_dir_acl(path: &Path, group: &str) -> Result<(), CliError> {
    let acl = format!("g:{group}:rx");
    let default_acl = format!("d:g:{group}:rx");
    let status = ProcessCommand::new("setfacl")
        .args(["-m", &acl, "-m", &default_acl])
        .arg(path)
        .status()?;
    if status.success() {
        return Ok(());
    }

    Err(CliError::CommandFailed(format!(
        "setfacl failed for {} with status {}",
        path.display(),
        status
    )))
}

#[cfg(target_os = "linux")]
fn grant_group_read_file_acl(path: &Path, group: &str) -> Result<(), CliError> {
    let acl = format!("g:{group}:r");
    let status = ProcessCommand::new("setfacl")
        .args(["-m", &acl])
        .arg(path)
        .status()?;
    if status.success() {
        return Ok(());
    }

    Err(CliError::CommandFailed(format!(
        "setfacl failed for {} with status {}",
        path.display(),
        status
    )))
}

fn known_ssd_root_for_adopt(ssd_root: Option<&Path>) -> Result<PathBuf, CliError> {
    let path = ssd_root
        .map(Path::to_path_buf)
        .unwrap_or_else(default_ssd_root);
    validate_known_ssd_root(&path)?;

    Ok(path)
}

fn default_ssd_root() -> PathBuf {
    std::env::var_os("DASOBJECTSTORE_SSD_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/srv/dasobjectstore/ssd"))
}

fn resolve_live_sqlite_path(override_path: Option<&Path>) -> PathBuf {
    override_path.map(Path::to_path_buf).unwrap_or_else(|| {
        default_ssd_root()
            .join(METADATA_DIR_NAME)
            .join(LIVE_SQLITE_FILE_NAME)
    })
}

fn resolve_store_live_sqlite_path(
    store_id: &StoreId,
    override_path: Option<&Path>,
    registry_path: Option<&Path>,
) -> Result<PathBuf, CliError> {
    if override_path.is_none() {
        let registry_path = registry_path
            .map(Path::to_path_buf)
            .unwrap_or_else(default_store_registry_path);
        let store_exists = read_store_registry(&registry_path)?
            .iter()
            .any(|definition| &definition.store_id == store_id);
        if !store_exists {
            return Err(CliError::CommandFailed(format!(
                "store `{store_id}` is not defined in {}",
                registry_path.display()
            )));
        }
    }

    Ok(resolve_live_sqlite_path(override_path))
}

fn now_utc_string() -> String {
    ProcessCommand::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                String::from_utf8(output.stdout).ok()
            } else {
                None
            }
        })
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            let seconds = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_secs())
                .unwrap_or_default();
            format!("unix:{seconds}")
        })
}

fn default_hdd_root() -> PathBuf {
    std::env::var_os("DASOBJECTSTORE_HDD_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/srv/dasobjectstore/hdd"))
}

fn is_known_ssd_root(path: &Path) -> bool {
    read_device_marker(path).is_ok_and(|marker| marker.lines().any(|line| line == "role=ssd"))
}

fn validate_known_ssd_root(path: &Path) -> Result<(), CliError> {
    let marker = read_device_marker(path).map_err(|err| {
        CliError::PortableRegistry(format!(
            "{} is not a known DASObjectStore SSD root: {err}",
            path.display()
        ))
    })?;
    if !marker.lines().any(|line| line == "role=ssd") {
        return Err(CliError::PortableRegistry(format!(
            "{} is not a DASObjectStore SSD root; expected role=ssd in .dasobjectstore/device.env",
            path.display()
        )));
    }

    Ok(())
}

fn read_device_marker(path: &Path) -> Result<String, std::io::Error> {
    fs::read_to_string(path.join(".dasobjectstore").join("device.env"))
}

fn discover_managed_hdd_roots(hdd_root: &Path) -> Result<Vec<DiskCopyRoot>, CliError> {
    let mut roots = Vec::new();
    let entries = match fs::read_dir(hdd_root) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(roots),
        Err(err) => return Err(CliError::Io(err)),
    };

    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let root_path = entry.path();
        let marker = match read_device_marker(&root_path) {
            Ok(marker) => marker,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => return Err(CliError::Io(err)),
        };
        let Some(disk_id) = hdd_disk_id_from_marker(&marker)? else {
            continue;
        };
        roots.push(DiskCopyRoot::new(disk_id, root_path));
    }

    roots.sort_by(|left, right| left.disk_id.cmp(&right.disk_id));
    Ok(roots)
}

fn hdd_disk_id_from_marker(marker: &str) -> Result<Option<DiskId>, CliError> {
    for line in marker.lines() {
        let Some(role) = line.strip_prefix("role=") else {
            continue;
        };
        let Some(disk_id) = role.strip_prefix("hdd:") else {
            return Ok(None);
        };
        return DiskId::new(disk_id)
            .map(Some)
            .map_err(|err| CliError::CommandFailed(format!("invalid managed HDD marker: {err}")));
    }

    Ok(None)
}

fn run_store_defaults(args: &StoreDefaultsArgs, writer: &mut impl Write) -> Result<(), CliError> {
    let policy = StorePolicy::defaults_for(args.class());

    serde_json::to_writer_pretty(&mut *writer, &policy)?;
    writer.write_all(b"\n")?;

    Ok(())
}

fn run_subobject(args: &SubobjectArgs, writer: &mut impl Write) -> Result<(), CliError> {
    match args.command() {
        SubobjectCommand::Create(args) => run_subobject_create(args, writer),
        SubobjectCommand::List(args) => run_subobject_list(args, writer),
        SubobjectCommand::Search(args) => run_subobject_search(args, writer),
    }
}

fn run_subobject_create(
    args: &SubobjectCreateArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let registry_path = args
        .registry_path()
        .map(Path::to_path_buf)
        .unwrap_or_else(default_subobject_registry_path);
    let parent = subobject_parent_from_args(args)?;
    let report = create_subobject_definition(&registry_path, args.name(), parent)?;
    let allow_default_ssd = args.registry_path().is_none() || args.ssd_root().is_some();
    let portable_report = mirror_portable_subobject_definition(
        args.ssd_root(),
        allow_default_ssd,
        &report.definition,
    )?;
    grant_subobject_writer_group_registry_access(args, &report.definition, &registry_path)?;

    write_subobject_create_report(&report, portable_report.as_ref(), writer)
}

fn subobject_parent_from_args(args: &SubobjectCreateArgs) -> Result<SubObjectParent, CliError> {
    match (args.store(), args.parent()) {
        (Some(store_id), None) => {
            let stores_registry_path = args
                .stores_registry_path()
                .map(Path::to_path_buf)
                .unwrap_or_else(default_store_registry_path);
            let store_exists = read_store_registry(&stores_registry_path)?
                .iter()
                .any(|definition| definition.store_id == *store_id);
            if !store_exists {
                return Err(CliError::CommandFailed(format!(
                    "store {} was not found in {}",
                    store_id,
                    stores_registry_path.display()
                )));
            }
            Ok(SubObjectParent::Store {
                store_id: store_id.clone(),
            })
        }
        (None, Some(name)) => Ok(SubObjectParent::SubObject {
            name: name.to_string(),
        }),
        _ => Err(CliError::CommandFailed(
            "subobject create requires exactly one of --store or --parent".to_string(),
        )),
    }
}

fn run_subobject_list(args: &SubobjectListArgs, writer: &mut impl Write) -> Result<(), CliError> {
    let registry_path = args
        .registry_path()
        .map(Path::to_path_buf)
        .unwrap_or_else(default_subobject_registry_path);
    let definitions = read_subobject_registry(&registry_path)?;

    writeln!(writer, "SubObjects: {}", definitions.len())?;
    for definition in definitions {
        write_subobject_definition_line(&definition, writer)?;
    }

    Ok(())
}

fn run_subobject_search(
    args: &SubobjectSearchArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let registry_path = args
        .registry_path()
        .map(Path::to_path_buf)
        .unwrap_or_else(default_subobject_registry_path);
    let definitions = read_subobject_registry(&registry_path)?;
    let matches = search_subobjects(&definitions, args.query());

    writeln!(writer, "SubObjects matched: {}", matches.len())?;
    for definition in matches {
        write_subobject_definition_line(definition, writer)?;
    }

    Ok(())
}

fn write_subobject_create_report(
    report: &SubObjectRegistryUpdateReport,
    portable_report: Option<&SubObjectRegistryUpdateReport>,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    writeln!(writer, "SubObject {}", report.action.as_str())?;
    writeln!(writer, "Name: {}", report.definition.name)?;
    writeln!(writer, "Store: {}", report.definition.store_id)?;
    writeln!(
        writer,
        "Parent: {}",
        subobject_parent_label(&report.definition.parent)
    )?;
    writeln!(
        writer,
        "Object prefix: {}",
        report.definition.object_prefix()
    )?;
    writeln!(
        writer,
        "Registry: {}",
        report.registry_path.to_string_lossy()
    )?;
    match portable_report {
        Some(report) => writeln!(
            writer,
            "Portable registry: {}",
            report.registry_path.to_string_lossy()
        )?,
        None => writeln!(writer, "Portable registry: not detected")?,
    }

    Ok(())
}

fn mirror_portable_subobject_definition(
    ssd_root: Option<&Path>,
    allow_default_ssd: bool,
    definition: &SubObjectDefinition,
) -> Result<Option<SubObjectRegistryUpdateReport>, CliError> {
    let Some(ssd_root) = known_ssd_root_for_optional_mirror(ssd_root, allow_default_ssd)? else {
        return Ok(None);
    };
    let registry_path = portable_subobject_registry_path(&ssd_root);
    let report = mirror_subobject_definition(&registry_path, definition.clone())?;

    Ok(Some(report))
}

fn write_subobject_definition_line(
    definition: &SubObjectDefinition,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    writeln!(
        writer,
        "- {} store={} parent={} prefix={}",
        definition.name,
        definition.store_id,
        subobject_parent_label(&definition.parent),
        definition.object_prefix()
    )?;

    Ok(())
}

fn subobject_parent_label(parent: &SubObjectParent) -> String {
    match parent {
        SubObjectParent::Store { store_id } => format!("store:{store_id}"),
        SubObjectParent::SubObject { name } => format!("subobject:{name}"),
    }
}

#[derive(Clone, Debug)]
struct FileIngestEntry {
    source_path: PathBuf,
    relative_path: PathBuf,
    object_id: ObjectId,
    size_bytes: u64,
}

#[derive(Clone, Debug)]
struct ResolvedIngestEndpoint {
    endpoint_name: String,
    endpoint_kind: &'static str,
    store: StoreServiceDefinition,
    object_prefix: String,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Debug, Eq, PartialEq)]
enum SourceAclPermission {
    Traverse,
    ReadTree,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Debug, Eq, PartialEq)]
struct SourceAclAction {
    path: PathBuf,
    permission: SourceAclPermission,
}

fn run_ingest_files(args: &IngestFilesArgs, writer: &mut impl Write) -> Result<(), CliError> {
    if args.local_direct() {
        return run_ingest_files_local_direct(args, writer);
    }

    prepare_source_access_for_packaged_daemon(args.source())?;
    let config = DaemonRuntimeConfig::default_packaged();
    let client = DaemonClient::new(UnixSocketDaemonTransport::new(config.socket_path.clone()));
    run_ingest_files_with_client(args, &client, writer)?;
    writeln!(writer, "Daemon socket: {}", config.socket_path.display())?;

    Ok(())
}

fn run_ingest_files_with_client<T>(
    args: &IngestFilesArgs,
    client: &DaemonClient<T>,
    writer: &mut impl Write,
) -> Result<(), CliError>
where
    T: DaemonClientTransport,
{
    let request = build_daemon_ingest_files_request(args);
    if args.tui() {
        return run_ingest_files_with_tui(args, client, request, writer);
    }
    let response = client.submit_ingest_files(request)?;
    write_daemon_ingest_submission(args, &response, writer)?;

    Ok(())
}

fn run_ingest_files_with_tui<T>(
    args: &IngestFilesArgs,
    client: &DaemonClient<T>,
    request: SubmitIngestFilesRequest,
    writer: &mut impl Write,
) -> Result<(), CliError>
where
    T: DaemonClientTransport,
{
    let interrupt_guard = UploadInterruptGuard::install();
    let tui = start_upload_tui(
        writer,
        UploadTuiContext {
            endpoint: args.endpoint().as_str().to_string(),
            source_path: args.source().to_path_buf(),
            object_type: args.object_type().to_string(),
            conflict_policy: args.conflict_policy().to_string(),
            dry_run: args.dry_run(),
        },
    )?;
    let tui = RefCell::new(tui);
    let response = match client.submit_ingest_files_with_progress_and_heartbeat(
        request,
        |event| {
            interrupt_guard.check_cancelled()?;
            tui.borrow_mut()
                .render_progress(event)
                .map_err(|err| DaemonClientError::Transport(err.to_string()))?;
            Ok(())
        },
        || {
            interrupt_guard.check_cancelled()?;
            tui.borrow_mut()
                .render_heartbeat()
                .map_err(|err| DaemonClientError::Transport(err.to_string()))?;
            Ok(())
        },
    ) {
        Ok(response) => response,
        Err(err) => {
            if matches!(err, DaemonClientError::Cancelled(_)) {
                let _ = tui.into_inner().cancel(&err);
                return Ok(());
            }
            let _ = tui.into_inner().fail(&err);
            return Err(err.into());
        }
    };
    tui.into_inner().finish(&response)?;

    Ok(())
}

#[cfg(not(test))]
fn start_upload_tui<W: Write>(
    writer: &mut W,
    context: UploadTuiContext,
) -> io::Result<UploadTui<'_, W>> {
    UploadTui::start(writer, context)
}

#[cfg(test)]
fn start_upload_tui<W: Write>(
    writer: &mut W,
    context: UploadTuiContext,
) -> io::Result<UploadTui<'_, W>> {
    UploadTui::start_with_fixed_viewport(writer, context, ratatui::layout::Rect::new(0, 0, 100, 28))
}

fn build_daemon_ingest_files_request(args: &IngestFilesArgs) -> SubmitIngestFilesRequest {
    SubmitIngestFilesRequest {
        endpoint: args.endpoint().clone(),
        source_path: args.source().to_path_buf(),
        object_type: args.object_type(),
        copies: args.copies(),
        conflict_policy: args.conflict_policy(),
        dry_run: args.dry_run(),
        client_request_id: None,
    }
}

#[cfg(target_os = "linux")]
fn prepare_source_access_for_packaged_daemon(source: &Path) -> Result<(), CliError> {
    const SERVICE_USER: &str = "dasobjectstore";

    let source = source.canonicalize().map_err(|err| {
        CliError::CommandFailed(format!(
            "failed to resolve ingest source {} before daemon submission: {err}",
            source.display()
        ))
    })?;
    if !source.exists() {
        return Err(CliError::CommandFailed(format!(
            "ingest source {} does not exist",
            source.display()
        )));
    }

    for action in plan_source_acl_actions(&source)? {
        match action.permission {
            SourceAclPermission::Traverse => run_setfacl(
                &[
                    "-m",
                    &format!("u:{SERVICE_USER}:--x"),
                    path_arg(&action.path).as_str(),
                ],
                &action.path,
                "grant daemon traversal",
            )?,
            SourceAclPermission::ReadTree => run_setfacl(
                &[
                    "-R",
                    "-m",
                    &format!("u:{SERVICE_USER}:rX"),
                    path_arg(&action.path).as_str(),
                ],
                &action.path,
                "grant daemon source read",
            )?,
        }
    }

    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn prepare_source_access_for_packaged_daemon(_source: &Path) -> Result<(), CliError> {
    Ok(())
}

#[cfg(target_os = "linux")]
fn plan_source_acl_actions(source: &Path) -> Result<Vec<SourceAclAction>, CliError> {
    let mut actions = acl_ancestors_requiring_execute(source)?
        .into_iter()
        .map(|path| SourceAclAction {
            path,
            permission: SourceAclPermission::Traverse,
        })
        .collect::<Vec<_>>();
    if source_requires_read_acl(source)? {
        actions.push(SourceAclAction {
            path: source.to_path_buf(),
            permission: SourceAclPermission::ReadTree,
        });
    }
    Ok(actions)
}

#[cfg(target_os = "linux")]
fn acl_ancestors_requiring_execute(source: &Path) -> Result<Vec<PathBuf>, CliError> {
    let mut ancestors = source.ancestors().skip(1).collect::<Vec<_>>();
    ancestors.reverse();
    let mut required = Vec::new();
    for ancestor in ancestors {
        if ancestor.parent().is_none() {
            continue;
        }
        let metadata = fs::metadata(ancestor).map_err(|err| {
            CliError::CommandFailed(format!(
                "failed to inspect ingest source ancestor {}: {err}",
                ancestor.display()
            ))
        })?;
        if metadata.permissions().mode() & 0o001 == 0 {
            required.push(ancestor.to_path_buf());
        }
    }
    Ok(required)
}

#[cfg(target_os = "linux")]
fn source_requires_read_acl(source: &Path) -> Result<bool, CliError> {
    let metadata = fs::metadata(source).map_err(|err| {
        CliError::CommandFailed(format!(
            "failed to inspect ingest source {}: {err}",
            source.display()
        ))
    })?;
    let required_bits = if metadata.is_dir() { 0o005 } else { 0o004 };
    Ok(metadata.permissions().mode() & required_bits != required_bits)
}

#[cfg(target_os = "linux")]
fn run_setfacl(args: &[&str], path: &Path, action: &str) -> Result<(), CliError> {
    let output = ProcessCommand::new("setfacl")
        .args(args)
        .output()
        .map_err(|err| {
            CliError::CommandFailed(format!(
                "failed to run setfacl to {action} for {}: {err}",
                path.display()
            ))
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let detail = if stderr.is_empty() {
            output.status.to_string()
        } else {
            stderr
        };
        return Err(CliError::CommandFailed(format!(
            "failed to {action} for {}: {detail}",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn path_arg(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn write_daemon_ingest_submission(
    args: &IngestFilesArgs,
    response: &SubmitIngestFilesResponse,
    writer: &mut impl Write,
) -> Result<(), io::Error> {
    writeln!(writer, "Daemon ingest job submitted")?;
    writeln!(writer, "Endpoint: {}", args.endpoint())?;
    writeln!(writer, "Source: {}", args.source().to_string_lossy())?;
    writeln!(writer, "Object type: {}", args.object_type())?;
    if let Some(copies) = args.copies() {
        writeln!(writer, "Copies override: {copies}")?;
    }
    writeln!(writer, "Conflict policy: {}", args.conflict_policy())?;
    writeln!(writer, "Dry run: {}", args.dry_run())?;
    writeln!(writer, "Job: {}", response.job_id)?;
    writeln!(writer, "Accepted at UTC: {}", response.accepted_at_utc)
}

#[cfg(unix)]
struct UploadInterruptGuard {
    previous: libc::sigaction,
}

#[cfg(unix)]
impl UploadInterruptGuard {
    fn install() -> Self {
        UPLOAD_CANCELLED.store(false, Ordering::SeqCst);
        let mut previous: libc::sigaction = unsafe { std::mem::zeroed() };
        unsafe {
            libc::sigemptyset(&mut previous.sa_mask);
            let mut handler: libc::sigaction = std::mem::zeroed();
            handler.sa_sigaction = upload_sigint_handler as *const () as usize;
            handler.sa_flags = 0;
            libc::sigemptyset(&mut handler.sa_mask);
            libc::sigaction(libc::SIGINT, &handler, &mut previous);
        }
        Self { previous }
    }

    fn check_cancelled(&self) -> Result<(), DaemonClientError> {
        if UPLOAD_CANCELLED.load(Ordering::SeqCst) {
            Err(DaemonClientError::Cancelled(
                "upload cancelled by Ctrl-C; daemon cleanup requested for the active file"
                    .to_string(),
            ))
        } else {
            Ok(())
        }
    }
}

#[cfg(unix)]
impl Drop for UploadInterruptGuard {
    fn drop(&mut self) {
        unsafe {
            libc::sigaction(libc::SIGINT, &self.previous, std::ptr::null_mut());
        }
    }
}

#[cfg(unix)]
extern "C" fn upload_sigint_handler(_: libc::c_int) {
    UPLOAD_CANCELLED.store(true, Ordering::SeqCst);
}

#[cfg(not(unix))]
struct UploadInterruptGuard;

#[cfg(not(unix))]
impl UploadInterruptGuard {
    fn install() -> Self {
        Self
    }

    fn check_cancelled(&self) -> Result<(), DaemonClientError> {
        Ok(())
    }
}

fn run_ingest_files_local_direct(
    args: &IngestFilesArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let ssd_root = args
        .ssd_root()
        .map(Path::to_path_buf)
        .unwrap_or_else(default_ssd_root);
    validate_known_ssd_root(&ssd_root)?;
    let hdd_root = args
        .hdd_root()
        .map(Path::to_path_buf)
        .unwrap_or_else(default_hdd_root);
    let registry_path = args
        .registry_path()
        .map(Path::to_path_buf)
        .unwrap_or_else(default_store_registry_path);
    let subobject_registry_path = args
        .subobject_registry_path()
        .map(Path::to_path_buf)
        .unwrap_or_else(default_subobject_registry_path);
    let endpoint =
        resolve_ingest_endpoint(args.endpoint(), &registry_path, &subobject_registry_path)?;
    authorize_store_write(&endpoint.store)?;
    let managed_disk_roots = discover_managed_hdd_roots(&hdd_root)?;
    let copies = args.copies().unwrap_or(endpoint.store.policy.copies);
    if copies == 0 || managed_disk_roots.len() < copies as usize {
        return Err(CliError::CommandFailed(format!(
            "ingest files requires at least {copies} managed HDD root(s), got {}",
            managed_disk_roots.len()
        )));
    }
    let files = collect_ingest_files(args.source(), &endpoint.object_prefix)?;
    let total_source_bytes = files.iter().map(|entry| entry.size_bytes).sum::<u64>();
    let total_work_bytes = total_source_bytes.saturating_mul(u64::from(copies) + 1);

    writeln!(writer, "File ingest plan")?;
    writeln!(writer, "Endpoint: {}", endpoint.endpoint_name)?;
    writeln!(writer, "Endpoint kind: {}", endpoint.endpoint_kind)?;
    writeln!(writer, "Store: {}", endpoint.store.store_id)?;
    writeln!(writer, "Object prefix: {}", endpoint.object_prefix)?;
    writeln!(writer, "Class: {}", endpoint.store.policy.class.name())?;
    writeln!(writer, "Source: {}", args.source().to_string_lossy())?;
    writeln!(writer, "Object type: {}", args.object_type())?;
    writeln!(writer, "SSD root: {}", ssd_root.to_string_lossy())?;
    writeln!(writer, "Managed HDD roots: {}", managed_disk_roots.len())?;
    writeln!(writer, "Files: {}", files.len())?;
    writeln!(writer, "Source bytes: {total_source_bytes}")?;
    writeln!(writer, "Copies: {copies}")?;
    writeln!(writer, "Conflict policy: {}", args.conflict_policy())?;
    writeln!(writer, "TUI: {}", args.tui())?;
    writeln!(writer, "Work bytes: {total_work_bytes}")?;

    if args.dry_run() {
        writeln!(writer, "Dry run: no files imported")?;
        for entry in &files {
            writeln!(
                writer,
                "- {} bytes={} object={}",
                entry.relative_path.to_string_lossy(),
                entry.size_bytes,
                entry.object_id
            )?;
        }
        return Ok(());
    }

    let mut completed_files = 0_usize;
    let mut completed_work_bytes = 0_u64;
    let started_at = Instant::now();
    let capacity_policy = SsdCapacityPolicy::default();

    for entry in &files {
        match read_ssd_stress(&ssd_root, &capacity_policy) {
            Ok(stress) => writeln!(writer, "SSD stress before file: {stress}")?,
            Err(err) => writeln!(writer, "SSD stress before file: unavailable ({err})")?,
        }
        writeln!(
            writer,
            "Importing {} as {}",
            entry.relative_path.to_string_lossy(),
            entry.object_id
        )?;

        let request = ObjectPutRequest::new(
            entry.object_id.clone(),
            &entry.source_path,
            &ssd_root,
            plan_disk_roots_for_entry(&managed_disk_roots, entry, &endpoint.store.policy, copies)?,
            copies,
        )
        .with_object_type(args.object_type());
        let mut stage_key = String::new();
        let mut stage_offset_bytes = 0_u64;
        let mut last_emit = Instant::now();
        let mut progress_write_error = None;
        let report = put_object_ssd_first_with_progress(&request, |progress| {
            let key = progress_stage_key(&progress);
            if key != stage_key {
                stage_key = key;
                stage_offset_bytes = 0;
                last_emit = Instant::now();
            }
            let delta = progress.bytes_written.saturating_sub(stage_offset_bytes);
            stage_offset_bytes = progress.bytes_written;
            completed_work_bytes = completed_work_bytes.saturating_add(delta);
            if last_emit.elapsed().as_secs() == 0 && progress.bytes_written < entry.size_bytes {
                return;
            }
            last_emit = Instant::now();
            if progress_write_error.is_none() {
                progress_write_error = write_file_ingest_progress(
                    writer,
                    completed_work_bytes,
                    total_work_bytes,
                    completed_files,
                    files.len(),
                    &progress,
                    started_at,
                    &ssd_root,
                    &capacity_policy,
                )
                .err();
            }
        })?;
        if let Some(err) = progress_write_error {
            return Err(CliError::Io(err));
        }

        completed_files += 1;
        writeln!(
            writer,
            "File complete: {} bytes={} hash={}:{} copies={}",
            entry.relative_path.to_string_lossy(),
            report.bytes_staged,
            report.content_hash_algorithm,
            report.content_hash,
            report.placements.len()
        )?;
    }

    writeln!(writer, "File ingest complete")?;
    writeln!(writer, "Files imported: {}", completed_files)?;
    writeln!(writer, "Source bytes imported: {total_source_bytes}")?;
    writeln!(
        writer,
        "Elapsed seconds: {:.3}",
        started_at.elapsed().as_secs_f64()
    )?;

    Ok(())
}

fn resolve_ingest_endpoint(
    endpoint: &StoreId,
    store_registry_path: &Path,
    subobject_registry_path: &Path,
) -> Result<ResolvedIngestEndpoint, CliError> {
    let stores = read_store_registry(store_registry_path)?;
    let store_match = stores
        .iter()
        .find(|definition| definition.store_id == *endpoint);
    let subobjects = read_subobject_registry(subobject_registry_path)?;
    let subobject_match = subobjects
        .iter()
        .find(|definition| definition.name == endpoint.as_str());

    if store_match.is_some() && subobject_match.is_some() {
        return Err(CliError::CommandFailed(format!(
            "ingest endpoint {} is ambiguous; both an object store and a SubObject use that name",
            endpoint
        )));
    }

    if let Some(store) = store_match {
        return Ok(ResolvedIngestEndpoint {
            endpoint_name: endpoint.as_str().to_string(),
            endpoint_kind: "object_store",
            store: store.clone(),
            object_prefix: store.store_id.as_str().to_string(),
        });
    }

    if let Some(subobject) = subobject_match {
        let store = stores
            .iter()
            .find(|definition| definition.store_id == subobject.store_id)
            .ok_or_else(|| {
                CliError::CommandFailed(format!(
                    "SubObject {} references missing store {} in {}",
                    subobject.name,
                    subobject.store_id,
                    store_registry_path.display()
                ))
            })?;
        return Ok(ResolvedIngestEndpoint {
            endpoint_name: subobject.name.clone(),
            endpoint_kind: "subobject",
            store: store.clone(),
            object_prefix: subobject.object_prefix(),
        });
    }

    Err(CliError::CommandFailed(format!(
        "ingest endpoint {} was not found in {} or {}",
        endpoint,
        store_registry_path.display(),
        subobject_registry_path.display()
    )))
}

fn authorize_store_write(store: &StoreServiceDefinition) -> Result<(), CliError> {
    let Some(writer_group) = &store.writer_group else {
        return Err(CliError::CommandFailed(format!(
            "store {} has no writer group configured; ask an administrator to set --writer-group",
            store.store_id
        )));
    };

    if current_user_is_root()? {
        return Ok(());
    }

    let groups = current_user_group_names()?;
    if groups.iter().any(|group| group == writer_group) {
        return Ok(());
    }

    Err(CliError::CommandFailed(format!(
        "current user is not allowed to write store {}; required group: {}",
        store.store_id, writer_group
    )))
}

fn current_user_is_root() -> Result<bool, CliError> {
    let output = ProcessCommand::new("id").arg("-u").output()?;
    if !output.status.success() {
        return Err(CliError::CommandFailed(format!(
            "id -u exited with status {}",
            output.status
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim() == "0")
}

fn current_user_group_names() -> Result<Vec<String>, CliError> {
    let output = ProcessCommand::new("id").arg("-Gn").output()?;
    if !output.status.success() {
        return Err(CliError::CommandFailed(format!(
            "id -Gn exited with status {}",
            output.status
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .map(ToOwned::to_owned)
        .collect())
}

fn plan_disk_roots_for_entry(
    roots: &[DiskCopyRoot],
    entry: &FileIngestEntry,
    policy: &StorePolicy,
    copies: u8,
) -> Result<Vec<DiskCopyRoot>, CliError> {
    let root_by_disk = roots
        .iter()
        .map(|root| (root.disk_id.clone(), root.clone()))
        .collect::<BTreeMap<_, _>>();
    let candidates = placement_candidates_for_entry(roots, entry)?;
    let request = if copies > 1 {
        PlacementRequest::protected(entry.size_bytes)
    } else {
        PlacementRequest::cache(entry.size_bytes)
    };
    let plan = plan_copy_count_for_store(&candidates, &request, policy, copies)
        .map_err(|err| CliError::CommandFailed(format!("copy placement failed: {err:?}")))?;
    if !plan.is_complete() {
        return Err(CliError::CommandFailed(format!(
            "copy placement for {} planned {} of {} required copy/copies",
            entry.object_id,
            plan.planned_copies.len(),
            copies
        )));
    }

    plan.planned_copies
        .into_iter()
        .map(|copy| {
            root_by_disk.get(&copy.disk_id).cloned().ok_or_else(|| {
                CliError::CommandFailed(format!(
                    "copy placement selected unknown disk {}",
                    copy.disk_id
                ))
            })
        })
        .collect()
}

fn placement_candidates_for_entry(
    roots: &[DiskCopyRoot],
    entry: &FileIngestEntry,
) -> Result<Vec<PlacementCandidate>, CliError> {
    roots
        .iter()
        .map(|root| {
            let capacity = measure_ssd_capacity(&root.root_path)?;
            Ok(PlacementCandidate::new(
                root.disk_id.clone(),
                None,
                capacity.available_bytes,
                dasobjectstore_core::lifecycle::HealthState::Healthy,
                PerformanceClass::Unknown,
                deterministic_write_load(&entry.object_id, &root.disk_id),
            ))
        })
        .collect()
}

fn deterministic_write_load(object_id: &ObjectId, disk_id: &DiskId) -> WriteLoad {
    let mut hasher = DefaultHasher::new();
    object_id.as_str().hash(&mut hasher);
    disk_id.as_str().hash(&mut hasher);
    match hasher.finish() % 4 {
        0 => WriteLoad::Idle,
        1 => WriteLoad::Light,
        2 => WriteLoad::Busy,
        _ => WriteLoad::Saturated,
    }
}

fn collect_ingest_files(
    root: &Path,
    object_prefix: &str,
) -> Result<Vec<FileIngestEntry>, CliError> {
    if !root.is_dir() {
        return Err(CliError::CommandFailed(format!(
            "ingest source must be a directory: {}",
            root.display()
        )));
    }

    let mut files = Vec::new();
    collect_ingest_files_into(root, root, object_prefix, &mut files)?;
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));

    Ok(files)
}

fn collect_ingest_files_into(
    root: &Path,
    current: &Path,
    object_prefix: &str,
    files: &mut Vec<FileIngestEntry>,
) -> Result<(), CliError> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        if is_hidden_entry_name(&entry.file_name()) {
            continue;
        }
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_ingest_files_into(root, &path, object_prefix, files)?;
        } else if file_type.is_file() {
            let metadata = entry.metadata()?;
            let relative_path = path
                .strip_prefix(root)
                .map_err(|err| CliError::CommandFailed(err.to_string()))?
                .to_path_buf();
            files.push(FileIngestEntry {
                object_id: object_id_for_ingested_file(object_prefix, &relative_path)?,
                source_path: path,
                relative_path,
                size_bytes: metadata.len(),
            });
        }
    }

    Ok(())
}

fn is_hidden_entry_name(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().starts_with('.')
}

fn object_id_for_ingested_file(
    object_prefix: &str,
    relative_path: &Path,
) -> Result<ObjectId, CliError> {
    let relative = relative_path
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    ObjectId::new(format!("{object_prefix}/{relative}"))
        .map_err(|err| CliError::CommandFailed(err.to_string()))
}

fn progress_stage_key(progress: &ObjectPutProgress) -> String {
    match &progress.stage {
        ObjectPutProgressStage::SsdIngest | ObjectPutProgressStage::SsdFlush => {
            "ssd-ingest".to_string()
        }
        ObjectPutProgressStage::HddCopy {
            disk_id,
            copy_number,
        } => format!("hdd-copy-{disk_id}-{copy_number}"),
    }
}

fn progress_stage_label(progress: &ObjectPutProgress) -> String {
    match &progress.stage {
        ObjectPutProgressStage::SsdIngest => "ssd-ingest".to_string(),
        ObjectPutProgressStage::SsdFlush => "ssd-flush".to_string(),
        ObjectPutProgressStage::HddCopy {
            disk_id,
            copy_number,
        } => format!("hdd-copy:{disk_id}:{copy_number}"),
    }
}

fn write_file_ingest_progress(
    writer: &mut impl Write,
    completed_work_bytes: u64,
    total_work_bytes: u64,
    completed_files: usize,
    total_files: usize,
    progress: &ObjectPutProgress,
    started_at: Instant,
    ssd_root: &Path,
    capacity_policy: &SsdCapacityPolicy,
) -> Result<(), io::Error> {
    let percent = if total_work_bytes == 0 {
        100.0
    } else {
        completed_work_bytes as f64 * 100.0 / total_work_bytes as f64
    };
    let elapsed = started_at.elapsed().as_secs_f64().max(0.001);
    let rate = completed_work_bytes as f64 / elapsed;
    let active_files = (completed_files + 1).min(total_files);
    let remaining_files = total_files.saturating_sub(active_files);
    let ssd_stress = match read_ssd_stress(ssd_root, capacity_policy) {
        Ok(stress) => stress,
        Err(_) => "unknown".to_string(),
    };

    writeln!(
        writer,
        "{:>12} {:>6.2}% {:>12}/s files={}/{} remaining={} stage={} stage_bytes={} ssd={}",
        completed_work_bytes,
        percent,
        format_bytes(rate),
        active_files,
        total_files,
        remaining_files,
        progress_stage_label(progress),
        progress.bytes_written,
        ssd_stress
    )
}

#[allow(dead_code)]
fn write_daemon_ingest_progress(
    writer: &mut impl Write,
    progress: &DaemonIngestProgressEvent,
    started_at: Instant,
) -> Result<(), io::Error> {
    let percent = progress
        .percent_complete()
        .map(|value| format!("{value:>3}%"))
        .unwrap_or_else(|| " n/a".to_string());
    let elapsed = started_at.elapsed().as_secs_f64().max(0.001);
    let rate = progress.work_bytes_done as f64 / elapsed;
    let total_files = progress
        .files_total
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let remaining_files = progress
        .files_total
        .map(|total| total.saturating_sub(progress.files_done).to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let ssd_pressure = progress
        .ssd_pressure
        .map(|pressure| format!("{pressure:?}"))
        .unwrap_or_else(|| "unknown".to_string());

    writeln!(
        writer,
        "{:>12} {} {:>12}/s files={}/{} remaining={} stage={} ssd={}",
        progress.work_bytes_done,
        percent,
        format_bytes(rate),
        progress.files_done,
        total_files,
        remaining_files,
        daemon_ingest_stage_label(&progress.stage),
        ssd_pressure
    )
}

#[allow(dead_code)]
fn daemon_ingest_stage_label(stage: &DaemonIngestStage) -> String {
    match stage {
        DaemonIngestStage::Queued => "queued".to_string(),
        DaemonIngestStage::SsdIngest => "ssd-ingest".to_string(),
        DaemonIngestStage::HddCopy {
            disk_id,
            copy_number,
        } => format!("hdd-copy:{disk_id}:{copy_number}"),
        DaemonIngestStage::Complete => "complete".to_string(),
        DaemonIngestStage::Failed => "failed".to_string(),
        DaemonIngestStage::Cancelled => "cancelled".to_string(),
    }
}

fn read_ssd_stress(
    ssd_root: &Path,
    capacity_policy: &SsdCapacityPolicy,
) -> Result<String, CliError> {
    let capacity = measure_ssd_capacity(ssd_root)?;
    let pressure = capacity_policy.evaluate(&capacity)?;

    Ok(format!(
        "pressure={pressure:?} used={}%",
        capacity.used_percent_floor()
    ))
}

fn format_bytes(bytes: f64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes;
    let mut unit = UNITS[0];
    for next_unit in UNITS.iter().skip(1) {
        if value < 1024.0 {
            break;
        }
        value /= 1024.0;
        unit = next_unit;
    }

    format!("{value:.1} {unit}")
}

fn run_ingest_status(args: &IngestStatusArgs, writer: &mut impl Write) -> Result<(), CliError> {
    let policy = SsdCapacityPolicy::new(
        args.high_watermark_percent(),
        args.critical_watermark_percent(),
        args.minimum_free_bytes(),
    )?;
    let capacity = measure_ssd_capacity(args.ssd_root())?;
    let pressure = policy.evaluate(&capacity)?;
    let destage_policy = DestagePriorityPolicy::default();

    write_ingest_status(&capacity, &policy, pressure, &destage_policy, writer)?;

    Ok(())
}

fn run_ingest_queue(args: &IngestQueueArgs, writer: &mut impl Write) -> Result<(), CliError> {
    let live_sqlite_path = resolve_live_sqlite_path(args.live_sqlite_path());
    let snapshot = read_ingest_queue_for_store(&live_sqlite_path, args.store_id())?;
    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &snapshot)?;
        writer.write_all(b"\n")?;
    } else {
        write_ingest_queue_summary(&snapshot, writer)?;
    }

    Ok(())
}

fn run_ingest_drain_queue(
    args: &IngestDrainQueueArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    require_admin_for_destructive_store_action(args.dry_run())?;
    if !args.dry_run() {
        RiskGate::new(RiskPolicy {
            allow_ingest_queue_drain: args.allow_ingest_queue_drain(),
            ..RiskPolicy::default()
        })
        .evaluate(
            RiskyOperation::IngestQueueDrain,
            &ActionConfirmation::new(args.confirm()),
        )?;
    }

    let request = IngestQueueDrainRequest {
        live_sqlite_path: resolve_live_sqlite_path(args.live_sqlite_path()),
        store_id: args.store_id().clone(),
        updated_at_utc: now_utc_string(),
        reason: args.reason().to_string(),
        dry_run: args.dry_run(),
    };
    let report = drain_ingest_queue(&request)?;
    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &report)?;
        writer.write_all(b"\n")?;
    } else {
        write_ingest_queue_drain_report(&report, writer)?;
    }

    Ok(())
}

fn write_ingest_queue_summary(
    snapshot: &IngestQueueSnapshot,
    writer: &mut impl Write,
) -> Result<(), io::Error> {
    writeln!(writer, "Ingest queue")?;
    writeln!(
        writer,
        "Live metadata: {}",
        snapshot.live_sqlite_path.display()
    )?;
    writeln!(writer, "Jobs: {}", snapshot.jobs.len())?;
    for job in &snapshot.jobs {
        writeln!(
            writer,
            "- {} store={} state={} object_type={} received={} expected={}",
            job.ingest_job_id,
            job.store_id,
            job.state,
            job.object_type,
            job.received_bytes,
            job.expected_size_bytes
                .map(|value| value.to_string())
                .unwrap_or_else(|| "unknown".to_string())
        )?;
    }
    Ok(())
}

fn write_ingest_queue_drain_report(
    report: &IngestQueueDrainReport,
    writer: &mut impl Write,
) -> Result<(), io::Error> {
    let action = if report.dry_run {
        "would cancel"
    } else {
        "cancelled"
    };
    writeln!(writer, "Ingest queue drain")?;
    writeln!(writer, "Store: {}", report.store_id)?;
    writeln!(
        writer,
        "Live metadata: {}",
        report.live_sqlite_path.display()
    )?;
    writeln!(writer, "Jobs {action}: {}", report.jobs_cancelled)?;
    for job_id in &report.cancelled_job_ids {
        writeln!(writer, "- {job_id}")?;
    }
    Ok(())
}

fn run_ingest_direct_import(
    args: &IngestDirectImportArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let file = File::open(args.policy_file())?;
    let policy: StorePolicy = serde_json::from_reader(file)?;
    let mut request = DirectHddImportRequest::new(
        args.object_id().clone(),
        args.disk_id().clone(),
        args.source(),
        args.destination(),
        args.expected_sha256(),
        policy,
        RiskPolicy {
            allow_direct_to_hdd_import: args.allow_direct_to_hdd_import(),
            ..RiskPolicy::default()
        },
        ActionConfirmation::new(args.confirm()),
    );
    if let Some(source_uri) = args.source_uri() {
        request = request.with_source_uri(source_uri);
    }
    request = request.with_object_type(args.object_type());

    let report = import_reproducible_object_direct_to_hdd(&request)?;
    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &report)?;
        writer.write_all(b"\n")?;
    } else {
        write_ingest_direct_import_report(&report, writer)?;
    }

    Ok(())
}

fn run_object_inspect(args: &ObjectInspectArgs, writer: &mut impl Write) -> Result<(), CliError> {
    let summary = read_object_inspect(args.live_sqlite_path(), args.object_id())?;

    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &summary)?;
        writer.write_all(b"\n")?;
    } else {
        write_object_inspect_summary(&summary, writer)?;
    }

    Ok(())
}

fn run_object_export(args: &ObjectExportArgs, writer: &mut impl Write) -> Result<(), CliError> {
    let disk_roots = parse_disk_roots(args.disk_roots())?;
    let request = ObjectExportRequest::new(
        args.live_sqlite_path(),
        args.object_id().clone(),
        args.destination(),
        disk_roots,
    );
    let report = export_settled_object(&request)?;

    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &report)?;
        writer.write_all(b"\n")?;
    } else {
        write_object_export_report(&report, writer)?;
    }

    Ok(())
}

fn run_object_put(args: &ObjectPutArgs, writer: &mut impl Write) -> Result<(), CliError> {
    let disk_roots = parse_disk_roots(args.disk_roots())?;
    let request = ObjectPutRequest::new(
        args.object_id().clone(),
        args.source(),
        args.ssd_root(),
        disk_roots,
        args.copies(),
    )
    .with_object_type(args.object_type());
    let report = put_object_ssd_first(&request)?;

    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &report)?;
        writer.write_all(b"\n")?;
    } else {
        write_object_put_report(&report, writer)?;
    }

    Ok(())
}

fn parse_disk_roots(values: &[String]) -> Result<Vec<DiskCopyRoot>, CliError> {
    values
        .iter()
        .map(|value| {
            let (disk_id, root_path) =
                value
                    .split_once('=')
                    .ok_or_else(|| CliError::InvalidDiskRootMapping {
                        value: value.clone(),
                    })?;
            let disk_id = DiskId::new(disk_id).map_err(|_| CliError::InvalidDiskRootMapping {
                value: value.clone(),
            })?;
            if root_path.is_empty() {
                return Err(CliError::InvalidDiskRootMapping {
                    value: value.clone(),
                });
            }

            Ok(DiskCopyRoot::new(disk_id, root_path))
        })
        .collect()
}

fn run_service_render_compose(
    args: &ServiceRenderComposeArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let registry_path = args
        .stores_file()
        .map(Path::to_path_buf)
        .unwrap_or_else(default_store_registry_path);
    let definitions = read_store_registry(&registry_path)?;
    let layout = plan_store_service_layout(&definitions)?;
    let request = ComposeRenderRequest {
        project_name: args.project_name().to_string(),
        ssd_metadata_path: args.ssd_metadata_path().to_string_lossy().to_string(),
        hdd_data_path: args.hdd_data_path().to_string_lossy().to_string(),
        store_bindings: layout.bucket_bindings,
    };
    let rendered = match args.provider() {
        ObjectServiceProviderId::Garage => {
            let provider = GarageProvider::new(GarageProviderConfig {
                service_name: args.service_name().to_string(),
                image: args.image().to_string(),
                api_port: args.api_port(),
                rpc_port: garage_derived_port(args.api_port(), 1)?,
                web_port: garage_derived_port(args.api_port(), 2)?,
                admin_port: garage_derived_port(args.api_port(), 3)?,
                ..GarageProviderConfig::default()
            });
            provider.render_compose(&request)?
        }
        ObjectServiceProviderId::Rustfs => {
            let service = ComposeServiceConfig::new(
                args.provider(),
                args.service_name(),
                args.image(),
                args.api_port(),
            );
            render_compose(&request, &service)?
        }
    };

    writer.write_all(rendered.compose_yaml.as_bytes())?;

    Ok(())
}

fn garage_derived_port(api_port: u16, offset: u16) -> Result<u16, ObjectServiceError> {
    api_port.checked_add(offset).ok_or_else(|| {
        ObjectServiceError::InvalidConfiguration(
            "Garage API port must leave room for RPC, web, and admin ports".to_string(),
        )
    })
}

fn run_mnemosyne_export(
    args: &MnemosyneExportArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let storage_definition =
        export_mneion_storage_definition(&MneionStorageDefinitionRequest::new(
            args.object_store_id(),
            args.display_name(),
            args.provider(),
            args.endpoint(),
        ))?;
    let mut binding_request =
        MneionBindingSnippetRequest::new(args.object_store_id(), args.governance_domain_id());
    if let Some(note) = args.note() {
        binding_request = binding_request.with_note(note);
    }
    let binding_snippet = export_mneion_binding_snippet(&binding_request)?;

    serde_json::to_writer_pretty(
        &mut *writer,
        &serde_json::json!({
            "storage_definition": storage_definition,
            "binding_snippet": binding_snippet,
        }),
    )?;
    writer.write_all(b"\n")?;

    Ok(())
}

fn run_mnemosyne_validate_nas_nfs_endpoint(
    args: &MnemosyneValidateNasNfsEndpointArgs,
    writer: &mut impl Write,
) -> Result<(), CliError> {
    let file = File::open(args.definition_file())?;
    let definition: NasNfsEndpointDefinition = serde_json::from_reader(file)?;
    let validated = validate_nas_nfs_endpoint_definition(&definition)?;

    if args.json() {
        serde_json::to_writer_pretty(&mut *writer, &validated)?;
        writer.write_all(b"\n")?;
    } else {
        write_nas_nfs_endpoint_validation_report(&validated, writer)?;
    }

    Ok(())
}

#[cfg(feature = "debug-commands")]
fn run_pool_mark_clean(args: &PoolMarkerArgs, writer: &mut impl Write) -> Result<(), CliError> {
    let marker =
        PoolStateMarker::clean_eject(args.pool_id().clone(), args.recorded_at_utc().to_string());
    record_pool_state_marker_at(args.live_sqlite_path(), &marker)
        .map_err(|err| CliError::MetadataMarker(err.to_string()))?;
    writeln!(writer, "Marked pool {} clean", args.pool_id())?;

    Ok(())
}

#[cfg(feature = "debug-commands")]
fn run_pool_mark_dirty(args: &PoolMarkerArgs, writer: &mut impl Write) -> Result<(), CliError> {
    let marker =
        PoolStateMarker::dirty_attach(args.pool_id().clone(), args.recorded_at_utc().to_string());
    record_pool_state_marker_at(args.live_sqlite_path(), &marker)
        .map_err(|err| CliError::MetadataMarker(err.to_string()))?;
    writeln!(writer, "Marked pool {} dirty", args.pool_id())?;

    Ok(())
}

#[cfg(target_os = "linux")]
fn probe_current_platform() -> Result<ProbeReport, ProbeError> {
    LinuxProbeProvider::system().probe()
}

#[cfg(target_os = "macos")]
fn probe_current_platform() -> Result<ProbeReport, ProbeError> {
    MacosProbeProvider::system().probe()
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn probe_current_platform() -> Result<ProbeReport, ProbeError> {
    Err(ProbeError::UnsupportedPlatform {
        platform: std::env::consts::OS.to_string(),
    })
}

#[derive(Debug)]
pub(crate) enum CliError {
    Io(io::Error),
    Json(serde_json::Error),
    IngestQueueRead(IngestQueueReadError),
    IngestQueueDrain(IngestQueueDrainError),
    StoreContentsRead(StoreContentsReadError),
    MetadataInspect(PoolInspectError),
    PoolImport(ReadOnlyAttachError),
    DiskDrain(DiskDrainError),
    StoreCleanup(StoreCleanupError),
    DiskLockdown(LockdownDasError),
    DiskPrepare(PrepareDasError),
    DaemonClient(DaemonClientError),
    DiskRetirement(DiskRetirementError),
    DirectHddImport(DirectHddImportError),
    ObjectExport(ObjectExportError),
    ObjectInspect(ObjectInspectError),
    ObjectPut(ObjectPutError),
    ObjectService(ObjectServiceError),
    MneionBindingSnippet(MneionBindingSnippetError),
    MneionStorageDefinition(MneionStorageDefinitionError),
    NasNfsEndpointValidation(NasNfsEndpointValidationError),
    CommandFailed(String),
    PortableRegistry(String),
    InvalidDiskRootMapping {
        value: String,
    },
    InvalidDeviceMapping {
        value: String,
    },
    RiskGate(RiskGateError),
    SsdCapacityMeasurement(SsdCapacityMeasurementError),
    SsdCapacityPolicy(SsdCapacityPolicyError),
    #[cfg(feature = "debug-commands")]
    MetadataMarker(String),
    Probe(ProbeError),
    StorePolicyValidation(StorePolicyValidationErrors),
    UnsupportedHealthFormat,
    UnsupportedPoolImportMode,
    UnsupportedPoolImportState {
        state: PoolState,
    },
    UnsupportedPoolRepairMode,
    UnsupportedProbeFormat,
    UnsupportedServiceStatusFormat,
    UnsupportedStoreContentsFormat,
}

impl Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(formatter, "failed to access command input or output: {err}"),
            Self::Json(err) => write!(formatter, "failed to process JSON: {err}"),
            Self::IngestQueueRead(err) => write!(formatter, "{err}"),
            Self::IngestQueueDrain(err) => write!(formatter, "{err}"),
            Self::StoreContentsRead(err) => write!(formatter, "{err}"),
            Self::MetadataInspect(err) => write!(formatter, "{err}"),
            Self::PoolImport(err) => write!(formatter, "{err}"),
            Self::DiskDrain(err) => write!(formatter, "{err}"),
            Self::StoreCleanup(err) => write!(formatter, "{err}"),
            Self::DiskLockdown(err) => write!(formatter, "{err}"),
            Self::DiskPrepare(err) => write!(formatter, "{err}"),
            Self::DaemonClient(err) => write!(formatter, "{err}"),
            Self::DiskRetirement(err) => write!(formatter, "{err}"),
            Self::DirectHddImport(err) => write!(formatter, "{err}"),
            Self::ObjectExport(err) => write!(formatter, "{err}"),
            Self::ObjectInspect(err) => write!(formatter, "{err}"),
            Self::ObjectPut(err) => write!(formatter, "{err}"),
            Self::ObjectService(err) => write!(formatter, "{err}"),
            Self::MneionBindingSnippet(err) => write!(formatter, "{err}"),
            Self::MneionStorageDefinition(err) => write!(formatter, "{err}"),
            Self::NasNfsEndpointValidation(err) => write!(formatter, "{err}"),
            Self::CommandFailed(err) => write!(formatter, "{err}"),
            Self::PortableRegistry(err) => write!(formatter, "{err}"),
            Self::InvalidDiskRootMapping { value } => write!(
                formatter,
                "invalid disk root mapping `{value}`; expected disk-id=/mounted/disk/root"
            ),
            Self::InvalidDeviceMapping { value } => write!(
                formatter,
                "invalid device mapping `{value}`; expected disk-id=/dev/disk/by-id/device"
            ),
            Self::RiskGate(err) => write!(formatter, "{err}"),
            Self::SsdCapacityMeasurement(err) => write!(formatter, "{err}"),
            Self::SsdCapacityPolicy(err) => write!(formatter, "{err}"),
            #[cfg(feature = "debug-commands")]
            Self::MetadataMarker(err) => write!(formatter, "failed to update pool metadata: {err}"),
            Self::Probe(err) => write!(formatter, "{err}"),
            Self::StorePolicyValidation(err) => write!(formatter, "{err}"),
            Self::UnsupportedHealthFormat => formatter.write_str(
                "health requires at most one output format; use `--summary`, `--verbose`, or `--json`",
            ),
            Self::UnsupportedPoolImportMode => {
                formatter.write_str("pool import currently requires `--read-only`")
            }
            Self::UnsupportedPoolImportState { state } => write!(
                formatter,
                "pool import --read-only supports Clean and Dirty snapshots, found {state:?}"
            ),
            Self::UnsupportedPoolRepairMode => {
                formatter.write_str("pool repair currently requires `--dry-run`")
            }
            Self::UnsupportedProbeFormat => {
                formatter.write_str("probe accepts at most one output format; use `--json` or `--pretty`")
            }
            Self::UnsupportedServiceStatusFormat => {
                formatter.write_str("service status requires JSON output; use `--json`")
            }
            Self::UnsupportedStoreContentsFormat => formatter.write_str(
                "store contents accepts at most one view format; use `--du` or `--tree`",
            ),
        }
    }
}

impl std::error::Error for CliError {}

impl From<io::Error> for CliError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<serde_json::Error> for CliError {
    fn from(err: serde_json::Error) -> Self {
        Self::Json(err)
    }
}

impl From<IngestQueueReadError> for CliError {
    fn from(err: IngestQueueReadError) -> Self {
        Self::IngestQueueRead(err)
    }
}

impl From<IngestQueueDrainError> for CliError {
    fn from(err: IngestQueueDrainError) -> Self {
        Self::IngestQueueDrain(err)
    }
}

impl From<StoreContentsReadError> for CliError {
    fn from(err: StoreContentsReadError) -> Self {
        Self::StoreContentsRead(err)
    }
}

impl From<PoolInspectError> for CliError {
    fn from(err: PoolInspectError) -> Self {
        Self::MetadataInspect(err)
    }
}

impl From<ReadOnlyAttachError> for CliError {
    fn from(err: ReadOnlyAttachError) -> Self {
        Self::PoolImport(err)
    }
}

impl From<DaemonClientError> for CliError {
    fn from(err: DaemonClientError) -> Self {
        Self::DaemonClient(err)
    }
}

impl From<DiskRetirementError> for CliError {
    fn from(err: DiskRetirementError) -> Self {
        Self::DiskRetirement(err)
    }
}

impl From<DirectHddImportError> for CliError {
    fn from(err: DirectHddImportError) -> Self {
        Self::DirectHddImport(err)
    }
}

impl From<LockdownDasError> for CliError {
    fn from(err: LockdownDasError) -> Self {
        Self::DiskLockdown(err)
    }
}

impl From<PrepareDasError> for CliError {
    fn from(err: PrepareDasError) -> Self {
        Self::DiskPrepare(err)
    }
}

impl From<RiskGateError> for CliError {
    fn from(err: RiskGateError) -> Self {
        Self::RiskGate(err)
    }
}

impl From<DiskDrainError> for CliError {
    fn from(err: DiskDrainError) -> Self {
        Self::DiskDrain(err)
    }
}

impl From<StoreCleanupError> for CliError {
    fn from(err: StoreCleanupError) -> Self {
        Self::StoreCleanup(err)
    }
}

impl From<ObjectInspectError> for CliError {
    fn from(err: ObjectInspectError) -> Self {
        Self::ObjectInspect(err)
    }
}

impl From<ObjectExportError> for CliError {
    fn from(err: ObjectExportError) -> Self {
        Self::ObjectExport(err)
    }
}

impl From<ObjectPutError> for CliError {
    fn from(err: ObjectPutError) -> Self {
        Self::ObjectPut(err)
    }
}

impl From<ObjectServiceError> for CliError {
    fn from(err: ObjectServiceError) -> Self {
        Self::ObjectService(err)
    }
}

impl From<MneionBindingSnippetError> for CliError {
    fn from(err: MneionBindingSnippetError) -> Self {
        Self::MneionBindingSnippet(err)
    }
}

impl From<MneionStorageDefinitionError> for CliError {
    fn from(err: MneionStorageDefinitionError) -> Self {
        Self::MneionStorageDefinition(err)
    }
}

impl From<NasNfsEndpointValidationError> for CliError {
    fn from(err: NasNfsEndpointValidationError) -> Self {
        Self::NasNfsEndpointValidation(err)
    }
}

impl From<SsdCapacityMeasurementError> for CliError {
    fn from(err: SsdCapacityMeasurementError) -> Self {
        Self::SsdCapacityMeasurement(err)
    }
}

impl From<SsdCapacityPolicyError> for CliError {
    fn from(err: SsdCapacityPolicyError) -> Self {
        Self::SsdCapacityPolicy(err)
    }
}

impl From<StorePolicyValidationErrors> for CliError {
    fn from(err: StorePolicyValidationErrors) -> Self {
        Self::StorePolicyValidation(err)
    }
}

impl From<ProbeError> for CliError {
    fn from(err: ProbeError) -> Self {
        Self::Probe(err)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        active_hdd_landing_lines, benchmark_direct_hdd, benchmark_ssd_only,
        benchmark_ssd_pipeline_with_options, benchmark_ssd_stage_then_drain, collect_ingest_files,
        connection_status_from_probe, current_user_group_names,
        materialize_generated_performance_workload, measure_copy_with_progress,
        measure_copy_with_split_progress, measure_ssd_stage_payload_with_progress,
        parse_binary_size, performance_report_metadata_json,
        performance_report_metadata_json_from_artifact,
        performance_report_qr_payload_from_artifact, performance_sync_all_calls,
        plan_performance_scenario_matrix, plan_ssd_residency_batches, render_performance_json,
        render_performance_report, render_performance_report_from_json_artifact,
        render_performance_tui_snapshot, render_simple_pdf, reset_performance_sync_all_calls, run,
        source_performance_workload, throughput, try_submit_pending_ssd_pipeline_jobs,
        update_file_read_measurements_from_disk_results, validate_managed_hdds_on_supported_das,
        validate_pdf_report_path, write_health_json, write_health_summary, write_health_verbose,
        write_host_connection_status, write_pretty_report, zero_measurement, ActiveHddWrite,
        ActiveHddWriteMap, CliError, ConnectionAssessment, DiskHealthSummary,
        DiskPlacementScheduler, HealthReport, ManagedHddDevice, PerformanceBenchmarkResults,
        PerformanceConcurrencyResult, PerformanceCopyProgressPhase, PerformanceDiskResult,
        PerformanceFileResult, PerformanceIoSample, PerformanceLiveRateCounters,
        PerformanceMeasurement, PerformancePayload, PerformanceRecommendation, PerformanceReport,
        PerformanceScenarioKind, PerformanceScenarioResult, PerformanceSsdResidencyBudget,
        PerformanceSsdSettler, PerformanceTuiContext, PerformanceTuiSnapshot, PerformanceWorkload,
        PerformanceWorkloadKind, SsdPipelineBenchmarkOptions, SsdPipelineJob,
        PERFORMANCE_SSD_SETTLE_QUEUE_CAPACITY,
    };
    use crate::cli::{Cli, PerformanceFileOrder, PerformanceFileSelection};
    use clap::Parser;
    use dasobjectstore_core::health::{HealthScore, HealthSignals};
    use dasobjectstore_core::ids::{DiskId, IngestJobId, PoolId, StoreId};
    use dasobjectstore_core::lifecycle::{DiskState, PoolState};
    use dasobjectstore_core::store::{
        CapacityBehavior, IngestMode, StoreClass, StorePolicy, StorePolicyValidationError,
    };
    use dasobjectstore_daemon::{
        DaemonApiRequest, DaemonApiResponse, DaemonClient, DaemonIngestConflictPolicy,
        DaemonIngestProgressEvent, DaemonIngestStage, DaemonSsdPressure, InProcessDaemonTransport,
        SubmitIngestFilesResponse,
    };
    use dasobjectstore_metadata::{
        export_metadata_snapshot, initialize_pool, manifest::DiskRole, ArtifactReference,
        DiskManifest, DiskManifestEntry, FormatVersion, MetadataArtifact, PoolInitOptions,
        PoolManifest, SnapshotExportOptions, DISK_MANIFEST_FILE_NAME, LIVE_SCHEMA_SQL,
        PLACEMENT_LOG_FILE_NAME, POOL_MANIFEST_FILE_NAME,
    };
    use dasobjectstore_mnemosyne::NAS_NFS_ENDPOINT_DEFINITION_SCHEMA_VERSION;
    use dasobjectstore_object_service::StoreServiceDefinition;
    use dasobjectstore_platform::{
        EnclosureIdentity, HostPlatform, ObservedDisk, ObservedEnclosure, ProbeReport, Transport,
    };
    use rusqlite::Connection;
    use std::collections::{BTreeMap, BTreeSet, VecDeque};
    use std::fs::{self, File};
    #[cfg(target_os = "linux")]
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::{mpsc, Arc, Mutex};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    #[test]
    fn bare_invocation_writes_top_level_help() {
        let cli = Cli::try_parse_from(["dasobjectstore"]).expect("root command parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("help writes");

        let output = String::from_utf8(output).expect("utf8 help");
        assert!(output.contains("Portable mixed-disk DAS object store"));
        assert!(output.contains("Usage: dasobjectstore"));
        assert!(output.contains("Commands:"));
        assert!(output.contains("disk"));
        assert!(output.contains("health"));
    }

    #[test]
    fn bare_store_command_writes_store_help() {
        let cli = Cli::try_parse_from(["dasobjectstore", "store"]).expect("store parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("store help writes");

        let output = String::from_utf8(output).expect("utf8 help");
        assert!(output.contains("Manage object stores and policy"));
        assert!(output.contains("Usage: dasobjectstore store [COMMAND]"));
        assert!(output.contains("adopt"));
        assert!(output.contains("create"));
        assert!(output.contains("list"));
    }

    #[test]
    fn bare_ingest_command_writes_ingest_help() {
        let cli = Cli::try_parse_from(["dasobjectstore", "ingest"]).expect("ingest parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("ingest help writes");

        let output = String::from_utf8(output).expect("utf8 help");
        assert!(output.contains("Inspect SSD ingest and destage work"));
        assert!(output.contains("Usage: dasobjectstore ingest [COMMAND]"));
        assert!(output.contains("status"));
        assert!(output.contains("queue"));
        assert!(output.contains("direct-import"));
    }

    #[test]
    fn performance_test_size_parser_accepts_binary_and_decimal_units() {
        let cases = [
            ("512", 512),
            ("1KiB", 1024),
            ("1.5MiB", 1_572_864),
            ("2GB", 2_000_000_000),
            (" 3 GiB ", 3_221_225_472),
        ];

        for (input, expected) in cases {
            assert_eq!(
                parse_binary_size(input).expect("size parses"),
                expected,
                "{input}"
            );
        }
    }

    #[test]
    fn performance_test_size_parser_rejects_invalid_sizes() {
        for input in ["", "0", "-1MiB", "1XB", "nan", "inf"] {
            let err = parse_binary_size(input).expect_err("invalid size is rejected");

            assert!(
                err.to_string().contains("invalid size")
                    || err.to_string().contains("invalid size unit"),
                "{input}: {err}"
            );
        }
    }

    #[test]
    fn performance_source_workload_collects_files_recursively_in_fifo_order() {
        let root = temp_root("performance-source-workload");
        let source = root.join("source");
        fs::create_dir_all(source.join("nested")).expect("create source fixture");
        fs::write(source.join("root.fastq.gz"), b"ACGT").expect("write root fixture");
        fs::write(source.join("nested").join("sample.pod5"), b"POD5DATA")
            .expect("write nested fixture");

        let workload = source_performance_workload(
            &source,
            None,
            PerformanceFileSelection::Random,
            PerformanceFileOrder::Fifo,
        )
        .expect("source workload is planned");

        assert_eq!(workload.kind, PerformanceWorkloadKind::SourceFolder);
        assert_eq!(workload.source_path, Some(source.clone()));
        assert_eq!(workload.source_cap_bytes, None);
        assert_eq!(workload.discovered_file_count, 2);
        assert_eq!(workload.discovered_total_bytes, 12);
        assert_eq!(workload.file_count(), 2);
        assert_eq!(workload.total_bytes(), 12);
        assert_eq!(
            workload
                .payloads
                .iter()
                .map(|payload| (
                    payload.file_index,
                    payload.relative_path.clone(),
                    payload.size_bytes
                ))
                .collect::<Vec<_>>(),
            vec![
                (0, PathBuf::from("nested/sample.pod5"), 8),
                (1, PathBuf::from("root.fastq.gz"), 4),
            ]
        );

        fs::remove_dir_all(root).expect("cleanup source fixture");
    }

    #[test]
    fn performance_source_workload_can_order_larger_files_first() {
        let root = temp_root("performance-source-workload-size-desc");
        let source = root.join("source");
        fs::create_dir_all(&source).expect("create source fixture");
        fs::write(source.join("small.fastq.gz"), b"aa").expect("write small fixture");
        fs::write(source.join("large.pod5"), b"aaaaaaaa").expect("write large fixture");
        fs::write(source.join("middle.bam"), b"aaaa").expect("write middle fixture");

        let workload = source_performance_workload(
            &source,
            None,
            PerformanceFileSelection::Random,
            PerformanceFileOrder::SizeDesc,
        )
        .expect("source workload is planned");

        assert_eq!(workload.file_order, PerformanceFileOrder::SizeDesc);
        assert_eq!(
            workload
                .payloads
                .iter()
                .map(|payload| (
                    payload.file_index,
                    payload.relative_path.clone(),
                    payload.size_bytes
                ))
                .collect::<Vec<_>>(),
            vec![
                (0, PathBuf::from("large.pod5"), 8),
                (1, PathBuf::from("middle.bam"), 4),
                (2, PathBuf::from("small.fastq.gz"), 2),
            ]
        );

        fs::remove_dir_all(root).expect("cleanup source fixture");
    }

    #[test]
    fn generated_performance_workload_materializes_all_sources_up_front_and_cleans_up() {
        let root = temp_root("performance-generated-source-workload");
        let mut workload = PerformanceWorkload {
            kind: PerformanceWorkloadKind::Generated,
            source_path: None,
            source_cap_bytes: None,
            file_selection: PerformanceFileSelection::Random,
            file_order: PerformanceFileOrder::Fifo,
            discovered_file_count: 2,
            discovered_total_bytes: 96,
            payloads: vec![
                PerformancePayload {
                    file_index: 0,
                    relative_path: PathBuf::from("generated-00000.bin"),
                    source_path: None,
                    size_bytes: 32,
                    modified_unix_nanos: 0,
                },
                PerformancePayload {
                    file_index: 1,
                    relative_path: PathBuf::from("generated-00001.bin"),
                    source_path: None,
                    size_bytes: 64,
                    modified_unix_nanos: 0,
                },
            ],
        };
        let mut output = Vec::new();
        let report_path = root.join("report.pdf");
        let json_path = root.join("report.json");

        let guard = materialize_generated_performance_workload(
            &mut workload,
            &root,
            "unit-run",
            &mut output,
            false,
            &report_path,
            &json_path,
            4,
        )
        .expect("generated workload materializes")
        .expect("generated workload returns cleanup guard");
        let source_root = root.join("dasobjectstore-performance-source-unit-run");

        assert_eq!(guard.root, source_root);
        assert!(String::from_utf8(output)
            .expect("utf8 output")
            .contains("generating 2 random source file(s)"));
        for payload in &workload.payloads {
            let source_path = payload.source_path.as_ref().expect("source path assigned");
            assert!(source_path.starts_with(&source_root));
            assert_eq!(
                fs::metadata(source_path).expect("source metadata").len(),
                payload.size_bytes
            );
        }

        drop(guard);
        assert!(
            !source_root.exists(),
            "generated source folder is removed when guard drops"
        );
        fs::remove_dir_all(root).expect("cleanup source fixture");
    }

    #[test]
    fn performance_source_workload_cap_can_select_smaller_files() {
        let root = temp_root("performance-source-workload-cap");
        let source = root.join("source");
        fs::create_dir_all(&source).expect("create source fixture");
        fs::write(source.join("a.fastq.gz"), b"aaaaaaaa").expect("write larger fixture");
        fs::write(source.join("b.fastq.gz"), b"bb").expect("write smaller fixture");
        fs::write(source.join("c.fastq.gz"), b"cccc").expect("write middle fixture");

        let workload = source_performance_workload(
            &source,
            Some(6),
            PerformanceFileSelection::Smaller,
            PerformanceFileOrder::Fifo,
        )
        .expect("capped source workload");

        assert_eq!(workload.source_cap_bytes, Some(6));
        assert_eq!(workload.file_selection, PerformanceFileSelection::Smaller);
        assert_eq!(workload.discovered_file_count, 3);
        assert_eq!(workload.discovered_total_bytes, 14);
        assert_eq!(workload.file_count(), 2);
        assert_eq!(workload.total_bytes(), 6);
        assert_eq!(
            workload
                .payloads
                .iter()
                .map(|payload| (
                    payload.file_index,
                    payload.relative_path.clone(),
                    payload.size_bytes
                ))
                .collect::<Vec<_>>(),
            vec![
                (0, PathBuf::from("b.fastq.gz"), 2),
                (1, PathBuf::from("c.fastq.gz"), 4),
            ]
        );

        fs::remove_dir_all(root).expect("cleanup source fixture");
    }

    #[test]
    fn performance_source_workload_cap_can_select_larger_files() {
        let root = temp_root("performance-source-workload-cap-larger");
        let source = root.join("source");
        fs::create_dir_all(&source).expect("create source fixture");
        fs::write(source.join("a.fastq.gz"), b"aaaaaaaa").expect("write larger fixture");
        fs::write(source.join("b.fastq.gz"), b"bb").expect("write smaller fixture");
        fs::write(source.join("c.fastq.gz"), b"cccc").expect("write middle fixture");

        let workload = source_performance_workload(
            &source,
            Some(10),
            PerformanceFileSelection::Larger,
            PerformanceFileOrder::Fifo,
        )
        .expect("capped source workload");

        assert_eq!(workload.source_cap_bytes, Some(10));
        assert_eq!(workload.file_selection, PerformanceFileSelection::Larger);
        assert_eq!(workload.discovered_file_count, 3);
        assert_eq!(workload.discovered_total_bytes, 14);
        assert_eq!(workload.file_count(), 2);
        assert_eq!(workload.total_bytes(), 10);
        assert_eq!(
            workload
                .payloads
                .iter()
                .map(|payload| (
                    payload.file_index,
                    payload.relative_path.clone(),
                    payload.size_bytes
                ))
                .collect::<Vec<_>>(),
            vec![
                (0, PathBuf::from("a.fastq.gz"), 8),
                (1, PathBuf::from("b.fastq.gz"), 2),
            ]
        );

        fs::remove_dir_all(root).expect("cleanup source fixture");
    }

    #[test]
    fn performance_source_workload_cap_rejects_empty_selection() {
        let root = temp_root("performance-source-workload-cap-empty");
        let source = root.join("source");
        fs::create_dir_all(&source).expect("create source fixture");
        fs::write(source.join("a.fastq.gz"), b"aaaa").expect("write fixture");

        let err = source_performance_workload(
            &source,
            Some(3),
            PerformanceFileSelection::Smaller,
            PerformanceFileOrder::Fifo,
        )
        .expect_err("cap smaller than every file");

        assert!(err
            .to_string()
            .contains("smaller than every selectable source file"));
        fs::remove_dir_all(root).expect("cleanup source fixture");
    }

    #[test]
    fn performance_report_path_must_be_pdf() {
        validate_pdf_report_path(Path::new("/tmp/report.pdf")).expect("pdf path accepted");

        let err =
            validate_pdf_report_path(Path::new("/tmp/report.md")).expect_err("markdown rejected");

        assert!(err.to_string().contains("must be a PDF path"));
    }

    #[test]
    fn performance_ssd_only_suppresses_progress_logs_for_tui_rendering() {
        let root = temp_root("performance-tui-log-suppression");
        let workload = PerformanceWorkload {
            kind: PerformanceWorkloadKind::Generated,
            source_path: None,
            source_cap_bytes: None,
            file_selection: PerformanceFileSelection::Random,
            file_order: PerformanceFileOrder::Fifo,
            discovered_file_count: 2,
            discovered_total_bytes: 8,
            payloads: vec![
                PerformancePayload {
                    file_index: 0,
                    relative_path: PathBuf::from("a.bin"),
                    source_path: None,
                    size_bytes: 4,
                    modified_unix_nanos: 0,
                },
                PerformancePayload {
                    file_index: 1,
                    relative_path: PathBuf::from("b.bin"),
                    source_path: None,
                    size_bytes: 4,
                    modified_unix_nanos: 0,
                },
            ],
        };
        let mut output = Vec::new();

        let report =
            benchmark_ssd_only(&root, &workload, &mut output, false, None).expect("benchmark runs");

        assert_eq!(report.total_bytes, 8);
        assert!(output.is_empty(), "TUI path must not receive line logs");
        fs::remove_dir_all(root).expect("cleanup benchmark fixture");
    }

    #[test]
    fn performance_ssd_residency_batches_follow_safe_capacity() {
        let workload = PerformanceWorkload {
            kind: PerformanceWorkloadKind::Generated,
            source_path: None,
            source_cap_bytes: None,
            file_selection: PerformanceFileSelection::Random,
            file_order: PerformanceFileOrder::Fifo,
            discovered_file_count: 4,
            discovered_total_bytes: 19,
            payloads: vec![
                PerformancePayload {
                    file_index: 0,
                    relative_path: PathBuf::from("a.bin"),
                    source_path: None,
                    size_bytes: 4,
                    modified_unix_nanos: 0,
                },
                PerformancePayload {
                    file_index: 1,
                    relative_path: PathBuf::from("b.bin"),
                    source_path: None,
                    size_bytes: 4,
                    modified_unix_nanos: 0,
                },
                PerformancePayload {
                    file_index: 2,
                    relative_path: PathBuf::from("c.bin"),
                    source_path: None,
                    size_bytes: 7,
                    modified_unix_nanos: 0,
                },
                PerformancePayload {
                    file_index: 3,
                    relative_path: PathBuf::from("d.bin"),
                    source_path: None,
                    size_bytes: 4,
                    modified_unix_nanos: 0,
                },
            ],
        };

        let batches = plan_ssd_residency_batches(
            &workload,
            PerformanceSsdResidencyBudget {
                safe_bytes: 8,
                available_bytes: 16,
            },
        )
        .expect("batches planned");

        let batch_indexes = batches
            .iter()
            .map(|batch| {
                batch
                    .iter()
                    .map(|payload| payload.file_index)
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(batch_indexes, vec![vec![0, 1], vec![2], vec![3]]);
    }

    #[test]
    fn performance_disk_scheduler_uses_idle_highest_fractional_free_disk() {
        let disk_a = DiskId::new("disk-a").expect("disk id");
        let disk_b = DiskId::new("disk-b").expect("disk id");
        let disk_c = DiskId::new("disk-c").expect("disk id");
        let mut scheduler = DiskPlacementScheduler {
            disks: vec![
                super::DiskPlacementState {
                    disk_id: disk_a.clone(),
                    root_path: PathBuf::from("/hdd/a"),
                    active: 0,
                    total_bytes: 100,
                    available_bytes: 90,
                    assigned_bytes: 0,
                    completed_seconds: 0.0,
                },
                super::DiskPlacementState {
                    disk_id: disk_b,
                    root_path: PathBuf::from("/hdd/b"),
                    active: 1,
                    total_bytes: 100,
                    available_bytes: 95,
                    assigned_bytes: 0,
                    completed_seconds: 0.0,
                },
                super::DiskPlacementState {
                    disk_id: disk_c.clone(),
                    root_path: PathBuf::from("/hdd/c"),
                    active: 0,
                    total_bytes: 200,
                    available_bytes: 100,
                    assigned_bytes: 0,
                    completed_seconds: 0.0,
                },
            ],
            logical_file_disks: BTreeMap::new(),
        };

        let first = scheduler
            .reserve_disk_for_file(0)
            .expect("first idle disk reserves");
        let second = scheduler
            .reserve_disk_for_file(1)
            .expect("second idle disk reserves");
        let third = scheduler.reserve_disk_for_file(2);

        assert_eq!(first.disk_id, disk_a);
        assert_eq!(second.disk_id, disk_c);
        assert!(
            third.is_none(),
            "scheduler must not assign a second active writer to an HDD"
        );
    }

    #[test]
    fn performance_ssd_only_writes_batch_before_readback() {
        let root = temp_root("performance-ssd-only-phase-order");
        let workload = PerformanceWorkload {
            kind: PerformanceWorkloadKind::Generated,
            source_path: None,
            source_cap_bytes: None,
            file_selection: PerformanceFileSelection::Random,
            file_order: PerformanceFileOrder::Fifo,
            discovered_file_count: 2,
            discovered_total_bytes: 8,
            payloads: vec![
                PerformancePayload {
                    file_index: 0,
                    relative_path: PathBuf::from("a.bin"),
                    source_path: None,
                    size_bytes: 4,
                    modified_unix_nanos: 0,
                },
                PerformancePayload {
                    file_index: 1,
                    relative_path: PathBuf::from("b.bin"),
                    source_path: None,
                    size_bytes: 4,
                    modified_unix_nanos: 0,
                },
            ],
        };
        let mut output = Vec::new();

        benchmark_ssd_only(&root, &workload, &mut output, true, None).expect("benchmark runs");
        let text = String::from_utf8(output).expect("utf8 output");
        let first_read = text
            .find("ssd-only read batch 1/")
            .expect("first resident batch read logged");
        if let Some(second_write) = text.find("ssd-only write batch 1/1 file 2/2") {
            assert!(
                second_write < first_read,
                "readback must not begin before the two-file resident write batch is complete"
            );
        } else {
            let first_write = text
                .find("ssd-only write batch 1/2 file 1/2")
                .expect("first one-file resident batch write logged");
            assert!(
                first_write < first_read,
                "readback must not begin before the active resident write batch starts"
            );
        }

        fs::remove_dir_all(root).expect("cleanup benchmark fixture");
    }

    #[test]
    fn performance_ssd_staging_does_not_sync_each_uploaded_file() {
        let root = temp_root("performance-ssd-stage-no-sync");
        let settler = PerformanceSsdSettler::start(PERFORMANCE_SSD_SETTLE_QUEUE_CAPACITY);
        let payload = PerformancePayload {
            file_index: 0,
            relative_path: PathBuf::from("a.bin"),
            source_path: None,
            size_bytes: 8,
            modified_unix_nanos: 0,
        };
        let ssd_destination = root.join("ssd").join("a.bin");
        reset_performance_sync_all_calls();

        let mut progress_calls = 0_u32;
        let mut progress = |_bytes: u64, _seconds: f64| -> Result<(), CliError> {
            progress_calls += 1;
            Ok(())
        };
        measure_ssd_stage_payload_with_progress(
            &payload,
            &ssd_destination,
            payload.file_index,
            Some(&mut progress),
            &settler,
        )
        .expect("SSD staging succeeds");

        assert!(
            progress_calls > 0,
            "staging should still report byte progress"
        );
        assert_eq!(
            performance_sync_all_calls(),
            0,
            "SSD staging should copy bytes linearly and leave durability settlement off the per-file upload path"
        );
        let settled_files = settler.finish().expect("SSD settlement finishes");
        assert_eq!(
            settled_files, 1,
            "SSD staging should settle the completed file on the background worker"
        );

        let source = root.join("source.bin");
        let durable_destination = root.join("hdd").join("a.bin");
        fs::write(&source, b"durable").expect("write source fixture");
        measure_copy_with_progress(&source, &durable_destination, None)
            .expect("durable HDD-style copy succeeds");
        assert_eq!(
            performance_sync_all_calls(),
            1,
            "durable final-media copies should still call sync_all"
        );
        fs::remove_dir_all(root).expect("cleanup benchmark fixture");
    }

    #[test]
    fn performance_split_copy_charges_final_sync_to_destination_only() {
        let root = temp_root("performance-split-copy-sync");
        let source = root.join("source.bin");
        let destination = root.join("hdd").join("payload.bin");
        fs::create_dir_all(&root).expect("create temp root");
        fs::write(&source, vec![7_u8; 1024 * 1024]).expect("write source fixture");
        reset_performance_sync_all_calls();

        let mut progress_events = Vec::new();
        let mut progress = |event| {
            progress_events.push(event);
            Ok(())
        };
        let measurement =
            measure_copy_with_split_progress(&source, &destination, Some(&mut progress))
                .expect("split copy succeeds");

        assert_eq!(performance_sync_all_calls(), 1);
        assert_eq!(measurement.source_read.bytes, 1024 * 1024);
        assert_eq!(measurement.destination_write.bytes, 1024 * 1024);
        assert!(
            progress_events.len() >= 2,
            "copy should emit a byte-progress event and a final post-sync event"
        );
        let before_sync = progress_events[progress_events.len() - 2];
        let after_sync = progress_events[progress_events.len() - 1];
        assert_eq!(before_sync.bytes, after_sync.bytes);
        assert_eq!(
            before_sync.source_read_seconds, after_sync.source_read_seconds,
            "final sync must not add elapsed time to the source/SSD read metric"
        );
        assert!(
            after_sync.destination_write_seconds >= before_sync.destination_write_seconds,
            "final sync should be charged to the destination/HDD metric"
        );
        assert_eq!(
            progress_events.first().expect("initial progress").phase,
            PerformanceCopyProgressPhase::Copying
        );
        assert!(
            progress_events
                .iter()
                .any(|event| event.phase == PerformanceCopyProgressPhase::Syncing),
            "copy should report the final media settling phase"
        );
        fs::remove_dir_all(root).expect("cleanup benchmark fixture");
    }

    #[test]
    fn performance_ssd_overlap_drain_starts_before_all_files_are_staged() {
        let root = temp_root("performance-overlap-drain");
        let ssd_root = root.join("ssd");
        let hdd_root = root.join("hdd").join("disk-a");
        fs::create_dir_all(&ssd_root).expect("create ssd root");
        fs::create_dir_all(&hdd_root).expect("create hdd root");
        let disk_a = DiskId::new("disk-a").expect("disk id");
        let workload = PerformanceWorkload {
            kind: PerformanceWorkloadKind::Generated,
            source_path: None,
            source_cap_bytes: None,
            file_selection: PerformanceFileSelection::Random,
            file_order: PerformanceFileOrder::Fifo,
            discovered_file_count: 2,
            discovered_total_bytes: 8,
            payloads: vec![
                PerformancePayload {
                    file_index: 0,
                    relative_path: PathBuf::from("a.bin"),
                    source_path: None,
                    size_bytes: 4,
                    modified_unix_nanos: 0,
                },
                PerformancePayload {
                    file_index: 1,
                    relative_path: PathBuf::from("b.bin"),
                    source_path: None,
                    size_bytes: 4,
                    modified_unix_nanos: 0,
                },
            ],
        };
        let mut output = Vec::new();

        let report = benchmark_ssd_pipeline_with_options(
            &ssd_root,
            &[(disk_a, hdd_root)],
            &workload,
            1,
            1,
            &mut output,
            false,
            None,
            SsdPipelineBenchmarkOptions {
                wait_for_first_hdd_start_after_first_file: true,
            },
        )
        .expect("overlap benchmark runs");

        assert!(report.hdd_drain_started_before_all_ssd_staged);
        assert_eq!(report.kind, PerformanceScenarioKind::SsdPipeline);
        fs::remove_dir_all(root).expect("cleanup benchmark fixture");
    }

    #[test]
    fn performance_stage_then_drain_reports_ssd_reads_from_drain_work() {
        let root = temp_root("performance-stage-drain-read-accounting");
        let ssd_root = root.join("ssd");
        let hdd_a = root.join("hdd").join("disk-a");
        let hdd_b = root.join("hdd").join("disk-b");
        fs::create_dir_all(&ssd_root).expect("create ssd root");
        fs::create_dir_all(&hdd_a).expect("create first hdd root");
        fs::create_dir_all(&hdd_b).expect("create second hdd root");
        let workload = PerformanceWorkload {
            kind: PerformanceWorkloadKind::Generated,
            source_path: None,
            source_cap_bytes: None,
            file_selection: PerformanceFileSelection::Random,
            file_order: PerformanceFileOrder::Fifo,
            discovered_file_count: 1,
            discovered_total_bytes: 8,
            payloads: vec![PerformancePayload {
                file_index: 0,
                relative_path: PathBuf::from("a.bin"),
                source_path: None,
                size_bytes: 8,
                modified_unix_nanos: 0,
            }],
        };
        let mut output = Vec::new();
        let disk_a = DiskId::new("disk-a").expect("disk id");
        let disk_b = DiskId::new("disk-b").expect("disk id");

        let report = benchmark_ssd_stage_then_drain(
            &ssd_root,
            &[(disk_a, hdd_a), (disk_b, hdd_b)],
            &workload,
            2,
            2,
            &mut output,
            false,
            None,
        )
        .expect("stage then drain benchmark runs");

        assert_eq!(report.hdd_write_operations, 2);
        assert_eq!(report.physical_hdd_write_bytes, 16);
        assert_eq!(report.file_results[0].ssd_write.bytes, 8);
        assert_eq!(
            report.file_results[0].ssd_read.bytes, 16,
            "SSD read bytes must come from physical drain copies, not one synthetic readback"
        );
        fs::remove_dir_all(root).expect("cleanup benchmark fixture");
    }

    #[test]
    fn performance_redundancy_lands_logical_file_on_distinct_disks() {
        let root = temp_root("performance-redundancy-distinct-disks");
        let ssd_root = root.join("ssd");
        let hdd_a = root.join("hdd").join("disk-a");
        let hdd_b = root.join("hdd").join("disk-b");
        fs::create_dir_all(&ssd_root).expect("create ssd root");
        fs::create_dir_all(&hdd_a).expect("create first hdd root");
        fs::create_dir_all(&hdd_b).expect("create second hdd root");
        let workload = PerformanceWorkload {
            kind: PerformanceWorkloadKind::Generated,
            source_path: None,
            source_cap_bytes: None,
            file_selection: PerformanceFileSelection::Random,
            file_order: PerformanceFileOrder::Fifo,
            discovered_file_count: 1,
            discovered_total_bytes: 8,
            payloads: vec![PerformancePayload {
                file_index: 0,
                relative_path: PathBuf::from("a.bin"),
                source_path: None,
                size_bytes: 8,
                modified_unix_nanos: 0,
            }],
        };
        let mut output = Vec::new();
        let disk_a = DiskId::new("disk-a").expect("disk id");
        let disk_b = DiskId::new("disk-b").expect("disk id");

        let report = benchmark_ssd_pipeline_with_options(
            &ssd_root,
            &[(disk_a.clone(), hdd_a), (disk_b.clone(), hdd_b)],
            &workload,
            2,
            2,
            &mut output,
            false,
            None,
            SsdPipelineBenchmarkOptions::default(),
        )
        .expect("redundant benchmark runs");

        let disks = report
            .disk_results
            .iter()
            .map(|row| row.disk_id.clone())
            .collect::<BTreeSet<_>>();
        assert_eq!(report.redundancy, 2);
        assert_eq!(report.queue_capacity, 8);
        assert_eq!(report.hdd_write_operations, 2);
        assert_eq!(report.logical_source_bytes, 8);
        assert_eq!(report.physical_hdd_write_bytes, 16);
        assert_eq!(
            report.file_results[0].ssd_read.bytes, 16,
            "overlap route SSD read bytes must be derived from physical drain copies"
        );
        assert_eq!(disks.len(), 2);
        assert!(disks.contains(&disk_a));
        assert!(disks.contains(&disk_b));
        fs::remove_dir_all(root).expect("cleanup benchmark fixture");
    }

    #[test]
    fn performance_direct_hdd_tui_renders_live_drain_progress() {
        let root = temp_root("performance-direct-hdd-tui-progress");
        let hdd_a = root.join("hdd").join("disk-a");
        fs::create_dir_all(&hdd_a).expect("create hdd root");
        let workload = PerformanceWorkload {
            kind: PerformanceWorkloadKind::Generated,
            source_path: None,
            source_cap_bytes: None,
            file_selection: PerformanceFileSelection::Random,
            file_order: PerformanceFileOrder::Fifo,
            discovered_file_count: 1,
            discovered_total_bytes: 8,
            payloads: vec![PerformancePayload {
                file_index: 0,
                relative_path: PathBuf::from("a.bin"),
                source_path: None,
                size_bytes: 8,
                modified_unix_nanos: 0,
            }],
        };
        let mut output = Vec::new();
        let disk_a = DiskId::new("disk-a").expect("disk id");

        let report = benchmark_direct_hdd(
            &[(disk_a, hdd_a)],
            &workload,
            1,
            1,
            &mut output,
            false,
            Some(PerformanceTuiContext {
                scenario_done: 0,
                scenario_total: 1,
                report_path: Path::new("/tmp/perf.pdf"),
                json_path: Path::new("/tmp/perf.json"),
            }),
        )
        .expect("direct hdd benchmark runs");

        let output = String::from_utf8(output).expect("utf8 output");
        assert_eq!(report.hdd_write_operations, 1);
        assert_eq!(report.physical_hdd_write_bytes, 8);
        assert!(output.contains("direct-hdd"));
        assert!(output.contains("HDD drain copy jobs"));
        assert!(output.contains("HDD Landing"));
        fs::remove_dir_all(root).expect("cleanup benchmark fixture");
    }

    #[test]
    fn performance_tui_snapshot_renders_scenario_objective_and_bounds() {
        let mut output = Vec::new();

        render_performance_tui_snapshot(
            &mut output,
            &PerformanceTuiSnapshot {
                phase: "ssd-overlap-drain active",
                scenario: "ssd-overlap-drain",
                activity: "Staging file 1/2 to SSD".to_string(),
                objective: "measure overlapping SSD ingest and FIFO HDD drain with 2 worker(s)"
                    .to_string(),
                bounds: "selected 2/10 file(s), 750.0 GiB/2.3 TiB; cap 750.0 GiB; HDD drain starts as soon as a staged file is queued; SSD backlog can grow toward selected total 750.0 GiB if drain at 2 worker(s) lags"
                    .to_string(),
                scenario_done: 1,
                scenario_total: 5,
                file_done: 0,
                current_file: Some(1),
                file_count: 2,
                processed_bytes: 0,
                total_bytes: 750_u64 * 1024 * 1024 * 1024,
                hdd_concurrency: 2,
                current_rate: Some(256.0 * 1024.0 * 1024.0),
                ssd_write_rate: Some(256.0 * 1024.0 * 1024.0),
                ssd_read_rate: Some(512.0 * 1024.0 * 1024.0),
                hdd_write_rate: Some(384.0 * 1024.0 * 1024.0),
                hdd_disk_rates: vec!["disk-a 192.0 MiB/s".to_string()],
                active_hdd_landing: vec![
                    "file 1/2 copy 1 -> disk-a: 1.0 GiB/2.0 GiB @ 128.0 MiB/s reads.fastq"
                        .to_string(),
                ],
                aggregate_rate: None,
                report_path: Path::new("/tmp/perf.pdf"),
                json_path: Path::new("/tmp/perf.json"),
            },
        )
        .expect("snapshot renders");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("Scenario Details"));
        assert!(output.contains("Objective: measure overlapping SSD ingest"));
        assert!(output.contains("SSD write rate: 256.0 MiB/s"));
        assert!(output.contains("SSD read rate: 512.0 MiB/s"));
        assert!(output.contains("HDD aggregate average: 384.0 MiB/s"));
        assert!(output.contains("HDD Landing"));
        assert!(output.contains("@ 128.0 MiB/s"));
        assert!(output.contains("reads.fastq"));
        assert!(output.contains("disk-a 192.0 MiB/s"));
    }

    #[test]
    fn performance_tui_active_hdd_landing_lines_include_per_transfer_rates() {
        let active_writes: ActiveHddWriteMap = Arc::new(Mutex::new(BTreeMap::new()));
        let started = Instant::now() - Duration::from_secs(2);
        for index in 0..5 {
            active_writes.lock().expect("active write lock").insert(
                (index, 0),
                ActiveHddWrite {
                    file_index: index,
                    copy_index: 0,
                    relative_path: PathBuf::from(format!("raw/file-{index}.pod5")),
                    disk_id: DiskId::new(format!("qnap-105{index}")).expect("disk id"),
                    size_bytes: 2 * 1024 * 1024 * 1024,
                    bytes_written: 512 * 1024 * 1024,
                    started,
                    phase: PerformanceCopyProgressPhase::Copying,
                },
            );
        }

        let lines = active_hdd_landing_lines(&active_writes, 31).expect("landing lines");
        assert_eq!(lines.len(), 5);
        assert!(lines[0].contains("@"));
        assert!(lines[0].contains("MiB/s"));
        assert!(lines[4].contains("file 5/31"));

        let mut output = Vec::new();
        render_performance_tui_snapshot(
            &mut output,
            &PerformanceTuiSnapshot {
                phase: "hdd-drain active",
                scenario: "ssd-pipeline",
                activity: "HDD drain active".to_string(),
                objective: "show active transfer visibility".to_string(),
                bounds: "five active HDD transfers should fit in the landing pane".to_string(),
                scenario_done: 1,
                scenario_total: 2,
                file_done: 0,
                current_file: None,
                file_count: 31,
                processed_bytes: 0,
                total_bytes: 10 * 1024 * 1024 * 1024,
                hdd_concurrency: 5,
                current_rate: None,
                ssd_write_rate: None,
                ssd_read_rate: None,
                hdd_write_rate: None,
                hdd_disk_rates: Vec::new(),
                active_hdd_landing: lines,
                aggregate_rate: None,
                report_path: Path::new("/tmp/perf.pdf"),
                json_path: Path::new("/tmp/perf.json"),
            },
        )
        .expect("snapshot renders");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("file 5/31"));
        assert!(output.contains("MiB/s"));
    }

    #[test]
    fn performance_tui_active_hdd_landing_lines_explain_zero_byte_and_sync_states() {
        let active_writes: ActiveHddWriteMap = Arc::new(Mutex::new(BTreeMap::new()));
        active_writes.lock().expect("active write lock").insert(
            (0, 0),
            ActiveHddWrite {
                file_index: 0,
                copy_index: 0,
                relative_path: PathBuf::from("raw/large-file.pod5"),
                disk_id: DiskId::new("qnap-1061").expect("disk id"),
                size_bytes: 21 * 1024 * 1024 * 1024,
                bytes_written: 0,
                started: Instant::now() - Duration::from_secs(30),
                phase: PerformanceCopyProgressPhase::Copying,
            },
        );
        active_writes.lock().expect("active write lock").insert(
            (1, 0),
            ActiveHddWrite {
                file_index: 1,
                copy_index: 0,
                relative_path: PathBuf::from("raw/settling-file.pod5"),
                disk_id: DiskId::new("qnap-1062").expect("disk id"),
                size_bytes: 24 * 1024 * 1024 * 1024,
                bytes_written: 24 * 1024 * 1024 * 1024,
                started: Instant::now() - Duration::from_secs(120),
                phase: PerformanceCopyProgressPhase::Syncing,
            },
        );

        let lines = active_hdd_landing_lines(&active_writes, 2).expect("landing lines");

        assert!(
            lines.iter().any(|line| line.contains("@ copying")),
            "zero-byte active writes should show that copy setup is active"
        );
        assert!(
            lines.iter().any(|line| line.contains("@ settling; avg")),
            "sync settlement should be explicit for large files"
        );
        assert!(
            !lines.iter().any(|line| line.contains("@ pending")),
            "large-file rows should not appear frozen at pending"
        );
    }

    #[test]
    fn performance_live_rates_ignore_idle_time_between_callbacks() {
        let counters = PerformanceLiveRateCounters::default();
        let disk_id = DiskId::new("disk-a").expect("disk id");

        counters
            .add_ssd_read_interval(1_000, 2.0)
            .expect("ssd read interval");
        counters
            .add_hdd_write_interval(&disk_id, 1_000, 2.0)
            .expect("hdd write interval");
        let before_idle = counters.snapshot().expect("snapshot");
        std::thread::sleep(Duration::from_millis(5));
        let after_idle = counters.snapshot().expect("snapshot");

        assert_eq!(before_idle.ssd_read_rate, after_idle.ssd_read_rate);
        assert_eq!(before_idle.hdd_write_rate, after_idle.hdd_write_rate);
    }

    #[test]
    fn performance_live_rates_charge_sync_only_time_to_hdd_not_ssd_read() {
        let counters = PerformanceLiveRateCounters::default();
        let disk_id = DiskId::new("disk-a").expect("disk id");

        counters
            .add_ssd_read_interval(1_000, 1.0)
            .expect("ssd read interval");
        counters
            .add_hdd_write_interval(&disk_id, 1_000, 1.0)
            .expect("hdd write interval");
        counters
            .add_hdd_write_interval(&disk_id, 0, 3.0)
            .expect("hdd sync interval");
        let snapshot = counters.snapshot().expect("snapshot");

        assert_eq!(snapshot.ssd_read_rate, Some(1_000.0));
        assert_eq!(snapshot.hdd_write_rate, Some(250.0));
    }

    #[test]
    fn performance_file_read_rollup_uses_ssd_read_seconds_not_hdd_sync_seconds() {
        let disk_id = DiskId::new("disk-a").expect("disk id");
        let mut file_results = vec![PerformanceFileResult {
            file_index: 0,
            ssd_write: zero_measurement(),
            ssd_read: zero_measurement(),
        }];
        let disk_results = vec![PerformanceDiskResult {
            file_index: 0,
            copy_index: 0,
            concurrency: 1,
            scenario: PerformanceScenarioKind::SsdPipeline,
            disk_id,
            ssd_read: PerformanceMeasurement {
                bytes: 1_000,
                seconds: 1.0,
            },
            write: PerformanceMeasurement {
                bytes: 1_000,
                seconds: 10.0,
            },
        }];

        update_file_read_measurements_from_disk_results(&mut file_results, &disk_results);

        assert_eq!(file_results[0].ssd_read.bytes, 1_000);
        assert_eq!(file_results[0].ssd_read.seconds, 1.0);
        assert_eq!(throughput(file_results[0].ssd_read), 1_000.0);
    }

    #[test]
    fn ssd_pipeline_pending_hdd_jobs_preserve_fifo_when_worker_channel_is_full() {
        let (sender, receiver) = mpsc::sync_channel::<SsdPipelineJob>(1);
        let mut pending = VecDeque::from([
            test_ssd_pipeline_job(0),
            test_ssd_pipeline_job(1),
            test_ssd_pipeline_job(2),
        ]);
        let mut submitted = 0_usize;

        assert!(
            try_submit_pending_ssd_pipeline_jobs(&sender, &mut pending, &mut submitted)
                .expect("first submit")
        );
        assert_eq!(submitted, 1);
        assert_eq!(pending.front().map(|job| job.file_index), Some(1));
        assert!(
            !try_submit_pending_ssd_pipeline_jobs(&sender, &mut pending, &mut submitted)
                .expect("full channel reports no progress")
        );
        assert_eq!(submitted, 1);
        assert_eq!(pending.front().map(|job| job.file_index), Some(1));

        assert_eq!(receiver.recv().expect("first job").file_index, 0);
        assert!(
            try_submit_pending_ssd_pipeline_jobs(&sender, &mut pending, &mut submitted)
                .expect("second submit")
        );
        assert_eq!(submitted, 2);
        assert_eq!(pending.front().map(|job| job.file_index), Some(2));

        assert_eq!(receiver.recv().expect("second job").file_index, 1);
        assert!(
            try_submit_pending_ssd_pipeline_jobs(&sender, &mut pending, &mut submitted)
                .expect("third submit")
        );
        assert_eq!(submitted, 3);
        assert!(pending.is_empty());
        assert_eq!(receiver.recv().expect("third job").file_index, 2);
    }

    #[test]
    fn performance_scenario_matrix_selects_requested_classes_and_concurrency() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "performance-test",
            "--source",
            "/data/source",
            "--scenario",
            "ssd-overlap-drain",
            "--scenario",
            "direct-hdd",
            "--hdd-concurrency",
            "1,3,5",
        ])
        .expect("performance-test parses");
        let Some(crate::cli::Command::PerformanceTest(args)) = cli.command() else {
            panic!("expected performance-test command");
        };

        let plan = plan_performance_scenario_matrix(args, 7).expect("matrix plans");

        assert!(!plan.include_ssd_only);
        assert!(plan.ssd_stage_then_drain.is_empty());
        assert_eq!(plan.ssd_pipeline, vec![1, 3, 5]);
        assert_eq!(plan.direct_hdd, vec![1, 3, 5]);
        assert_eq!(plan.scenario_total(), 6);
        assert_eq!(plan.max_concurrency(), 5);
    }

    #[test]
    fn performance_scenario_matrix_defaults_to_full_sweep() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "performance-test",
            "--file-size",
            "1MiB",
            "--file-count",
            "1",
            "--max-hdd-concurrency",
            "3",
        ])
        .expect("performance-test parses");
        let Some(crate::cli::Command::PerformanceTest(args)) = cli.command() else {
            panic!("expected performance-test command");
        };

        let plan = plan_performance_scenario_matrix(args, 7).expect("matrix plans");

        assert!(plan.include_ssd_only);
        assert_eq!(plan.ssd_stage_then_drain, vec![1, 2, 3]);
        assert_eq!(plan.ssd_pipeline, vec![1, 2, 3]);
        assert_eq!(plan.direct_hdd, vec![1, 2, 3]);
        assert_eq!(plan.scenario_total(), 10);
    }

    #[test]
    fn performance_scenario_matrix_rejects_unavailable_hdd_concurrency() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "performance-test",
            "--file-size",
            "1MiB",
            "--file-count",
            "1",
            "--scenario",
            "direct-hdd",
            "--hdd-concurrency",
            "4",
        ])
        .expect("performance-test parses");
        let Some(crate::cli::Command::PerformanceTest(args)) = cli.command() else {
            panic!("expected performance-test command");
        };

        let err = plan_performance_scenario_matrix(args, 3).expect_err("rejects matrix");

        assert!(err
            .to_string()
            .contains("--hdd-concurrency 4 requires at least 4 managed HDD roots"));
    }

    fn test_ssd_pipeline_job(file_index: u32) -> SsdPipelineJob {
        SsdPipelineJob {
            file_index,
            copy_index: 0,
            relative_path: PathBuf::from(format!("{file_index}.bin")),
            ssd_path: PathBuf::from(format!("/ssd/{file_index}.bin")),
            size_bytes: 1,
        }
    }

    #[test]
    fn performance_test_report_renders_summary_tables_and_recommendation() {
        let report = render_performance_report(example_performance_report());

        assert!(report.contains("# DASObjectStore Performance Test Report"));
        assert!(report.contains("| Brand | Mnemosyne Biosciences |"));
        assert!(report.contains("| JSON artifact | `/tmp/perf-test-run.json` |"));
        assert!(report.contains("| PDF artifact | `/tmp/perf-test-run.pdf` |"));
        assert!(report.contains("| QR artifact | `/tmp/perf-test-run.qr.svg` |"));
        assert!(report.contains("| QR status | `qrencode SVG` |"));
        assert!(report.contains("Reproduction payload SHA-256"));
        assert!(report.contains("Reproduction QR payload"));
        assert!(report.contains("## Reproduction Payload"));
        assert!(
            report.contains("Scenario: generated workload, 1 files, 1.0 MiB logical source total; file selection `random`; file order(s) `fifo`, `size_desc`.")
        );
        assert!(report.contains("- Run id: `perf-test-run`"));
        assert!(report.contains("- Reproduce with: `dasobjectstore performance-test"));
        assert!(report.contains(
            "- Recommended strategy: ssd-overlap-drain with `size_desc` order at 2 HDD worker(s)"
        ));
        assert!(report.contains(
            "| Scenario | File order | HDD concurrency | Redundancy | Logical source | Physical HDD writes | Operations | Aggregate landing | Elapsed | HDD drain overlapped SSD staging |"
        ));
        assert!(report
            .contains("| SSD ingest with overlapping HDD drain | `size_desc` | 2 | 2 | 1.0 MiB | 2.0 MiB | 2 | 2.0 MiB/s | 1.0 s | yes |"));
        assert!(report
            .contains("| Scenario | File order | HDD concurrency | File | SSD write | SSD read |"));
        assert!(report.contains(
            "| Scenario | File order | HDD concurrency | File | Copy | Disk | Write rate |"
        ));
        assert!(report
            .contains("| ssd-overlap-drain | `size_desc` | 2 | 1 | 2 | disk-b | 512.0 KiB/s |"));
        assert!(report.contains(
            "| Scenario | File order | HDD concurrency | Members | Aggregate landing | Slowest file write | HDD drain overlapped SSD staging |"
        ));
        assert!(report.contains(
            "| ssd-overlap-drain | `size_desc` | 2 | disk-a, disk-b | 2.0 MiB/s | 2.0 s | yes |"
        ));
        assert!(report.contains("io_time_series"));
        assert!(report.contains("Per-second IO rates: ssd-overlap-drain size_desc c2 r2"));
        assert!(report
            .contains("Use `ssd-overlap-drain` with `size_desc` file order and 2 HDD worker(s)"));
        assert!(render_simple_pdf(&report).starts_with(b"%PDF-1.4"));
    }

    #[test]
    fn performance_chart_svg_renders_labelled_bar_chart() {
        let svg = super::render_svg_bar_chart(
            "Landing rate by strategy",
            "Strategy",
            "Aggregate landing rate (MiB/s)",
            &[
                ("ssd-overlap-drain c2 r2".to_string(), 420.0),
                ("direct-hdd c2 r2".to_string(), 310.0),
            ],
        );

        assert!(svg.contains("<svg"));
        assert!(svg.contains("Landing rate by strategy"));
        assert!(svg.contains("Aggregate landing rate (MiB/s)"));
        assert!(svg.contains("ssd-overlap-drain c2 r2"));
        assert!(svg.contains("<rect"));
    }

    #[test]
    fn performance_io_line_chart_renders_per_device_read_and_write_series() {
        let svg = super::render_svg_io_line_chart(
            "Per-second IO rates: ssd-overlap-drain c2 r1",
            &[
                PerformanceIoSample {
                    elapsed_second: 1,
                    device_label: "ssd".to_string(),
                    device_name: "nvme0n1".to_string(),
                    read_bytes_per_second: 2 * 1024 * 1024,
                    write_bytes_per_second: 4 * 1024 * 1024,
                },
                PerformanceIoSample {
                    elapsed_second: 2,
                    device_label: "qnap-1057".to_string(),
                    device_name: "sda".to_string(),
                    read_bytes_per_second: 0,
                    write_bytes_per_second: 128 * 1024 * 1024,
                },
            ],
        );

        assert!(svg.contains("<svg"));
        assert!(svg.contains("<polyline"));
        assert!(svg.contains("Per-second IO rates"));
        assert!(svg.contains("qnap-1057"));
        assert!(svg.contains("solid write, dashed read"));
        assert!(svg.contains("IO rate (MiB/s)"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn diskstats_parser_extracts_read_and_write_sector_counters() {
        let counters = parse_proc_diskstats(
            "   8       0 sda 157698 0 8822930 92744 100003 0 4194304 112233 0 0 0 0 0 0 0 0 0\n",
        );

        assert_eq!(
            counters.get("sda"),
            Some(&DiskIoCounters {
                read_sectors: 8_822_930,
                write_sectors: 4_194_304,
            })
        );
    }

    #[test]
    fn performance_report_metadata_json_satisfies_standard_template() {
        let metadata = performance_report_metadata_json(&example_performance_report());
        let metadata = serde_json::from_str::<serde_json::Value>(&metadata).unwrap();
        let labels = metadata["rows"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|row| row.as_array().unwrap())
            .map(|field| field["label"].as_str().unwrap())
            .collect::<Vec<_>>();

        assert_eq!(metadata["header"], "DASObjectStore performance report");
        assert_eq!(labels[0], "Document ID");
        for required in [
            "Document ID",
            "Performance test identifier",
            "Version",
            "Report state",
            "DeviceID",
            "Operator",
            "Timestamp",
            "Run ID",
            "Repository revision",
            "Test status",
            "Signature of operator",
            "Cryptographic signature",
        ] {
            assert!(labels.contains(&required), "{required}");
        }
        assert_eq!(
            metadata["rows"][2][3]["value"],
            "623f8d191890968ec394ff02950710ecb9e5eed5a0b68c064e28e8ffa0876f58"
        );
    }

    #[test]
    fn performance_report_can_be_rebuilt_from_json_artifact() {
        let artifact = serde_json::from_str::<serde_json::Value>(&render_performance_json(
            &example_performance_report(),
        ))
        .expect("performance JSON parses");

        let metadata = serde_json::from_str::<serde_json::Value>(
            &performance_report_metadata_json_from_artifact(&artifact),
        )
        .expect("metadata parses");
        let markdown =
            render_performance_report_from_json_artifact(&artifact, Path::new("/tmp/rebuilt.pdf"));
        let qr_payload = performance_report_qr_payload_from_artifact(&artifact);

        assert_eq!(metadata["header"], "DASObjectStore performance report");
        assert_eq!(metadata["rows"][0][0]["label"], "Document ID");
        assert_eq!(metadata["rows"][0][1]["value"], "DASObjectStore-Disk-Speed");
        assert_eq!(metadata["rows"][2][2]["label"], "Signature of operator");
        assert_eq!(metadata["rows"][2][3]["label"], "Cryptographic signature");
        assert!(qr_payload.starts_with("mnemosyne-report:DASObjectStore:perf-test-run:"));
        assert!(markdown.contains("## Scenario Summary"));
        assert!(markdown.contains("## Per-Disk HDD Write Rates"));
        assert!(markdown.contains("![Landing rate by strategy]"));
        assert!(markdown.contains("```json"));
        assert!(
            markdown.contains("\"schema\": \"dasobjectstore.performance_test.recommendation.v1\"")
        );
    }

    #[test]
    fn performance_recommendation_json_contains_ingress_guidance() {
        let artifact = serde_json::from_str::<serde_json::Value>(&render_performance_json(
            &example_performance_report(),
        ))
        .expect("performance JSON parses");

        assert_eq!(
            artifact["schema"],
            "dasobjectstore.performance_test.recommendation.v1"
        );
        assert_eq!(artifact["artifact_kind"], "ingress_recommendation");
        assert!(artifact["run"]["parameters"]["source_cap_bytes"].is_null());
        assert_eq!(artifact["run"]["parameters"]["discovered_file_count"], 1);
        assert_eq!(
            artifact["run"]["parameters"]["discovered_total_bytes"],
            1_048_576
        );
        assert_eq!(artifact["recommendation"]["strategy"], "ssd_hdd_pipeline");
        assert_eq!(artifact["recommendation"]["file_order"], "size_desc");
        assert_eq!(artifact["recommendation"]["hdd_concurrency"], 2);
        assert_eq!(artifact["recommendation"]["redundancy"], 2);
        assert_eq!(artifact["run"]["parameters"]["redundancy"], 2);
        assert_eq!(
            artifact["run"]["parameters"]["file_orders"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["fifo", "size_desc"]
        );
        assert_eq!(artifact["daemon_policy"]["authoritative"], true);
        assert_eq!(
            artifact["daemon_policy"]["source_routes"]["remote_upload"],
            "ssd_first"
        );
        assert_eq!(
            artifact["daemon_policy"]["source_routes"]["external_disk_ingress"],
            "ssd_first"
        );
        assert_eq!(
            artifact["daemon_policy"]["ssd_hdd_settlement"]["hdd_concurrency"],
            2
        );
        assert_eq!(
            artifact["daemon_policy"]["ssd_hdd_settlement"]["file_order"],
            "size_desc"
        );
        assert_eq!(
            artifact["daemon_policy"]["ssd_hdd_settlement"]["redundancy"],
            2
        );
        assert_eq!(artifact["hardware"]["disks"].as_array().unwrap().len(), 2);
        assert_eq!(
            artifact["scenarios"]["ssd_hdd_pipeline"]["concurrency"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            artifact["scenarios"]["ssd_stage_then_drain_pipeline"]["concurrency"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            artifact["scenarios"]["ssd_hdd_pipeline"]["concurrency"][0]
                ["hdd_drain_started_before_all_ssd_staged"],
            true
        );
        assert_eq!(
            artifact["scenarios"]["ssd_hdd_pipeline"]["concurrency"][0]["file_order"],
            "size_desc"
        );
        assert_eq!(
            artifact["scenarios"]["ssd_only"]["orders"][0]["file_order"],
            "fifo"
        );
        assert_eq!(
            artifact["scenarios"]["ssd_hdd_pipeline"]["concurrency"][1]["hdd_write_operations"],
            2
        );
        assert_eq!(
            artifact["scenarios"]["ssd_stage_then_drain_pipeline"]["concurrency"][0]
                ["hdd_drain_started_before_all_ssd_staged"],
            false
        );
        assert_eq!(
            artifact["scenarios"]["direct_hdd_pipeline"]["concurrency"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert!(artifact["recommendation"]["rationale"]
            .as_array()
            .is_some_and(|rows| rows.len() >= 2));
        assert_eq!(
            artifact["plot_data"]["landing_rate_by_strategy"]
                .as_array()
                .unwrap()
                .len(),
            6
        );
        assert_eq!(
            artifact["plot_data"]["per_disk_hdd_write_rate"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| row["scenario"] == "ssd-overlap-drain")
                .count(),
            3
        );
        assert!(artifact["plot_data"]["io_time_series"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["device_label"] == "ssd"
                && row["scenario"] == "ssd-overlap-drain"
                && row["file_order"] == "size_desc"
                && row["write_mib_per_second"].as_f64().unwrap() > 0.0));
        assert_eq!(
            artifact["scenarios"]["ssd_hdd_pipeline"]["concurrency"][0]["io_samples"][0]
                ["device_name"],
            "nvme0n1"
        );
    }

    #[test]
    fn performance_recommendation_json_records_selected_matrix() {
        let mut report = example_performance_report();
        report.results.ssd_only.clear();
        report.results.ssd_stage_then_drain.clear();

        let artifact = serde_json::from_str::<serde_json::Value>(&render_performance_json(&report))
            .expect("performance JSON parses");

        assert_eq!(artifact["scenarios"]["ssd_only"]["selected"], false);
        assert_eq!(
            artifact["scenarios"]["ssd_stage_then_drain_pipeline"]["selected"],
            false
        );
        assert_eq!(artifact["scenarios"]["ssd_hdd_pipeline"]["selected"], true);
        assert_eq!(
            artifact["scenarios"]["direct_hdd_pipeline"]["selected"],
            true
        );
        assert_eq!(
            artifact["run"]["parameters"]["selected_scenarios"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["ssd-overlap-drain", "direct-hdd"]
        );
        assert_eq!(
            artifact["run"]["parameters"]["selected_hdd_concurrency"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_u64().unwrap())
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert!(artifact["recommendation"]["rationale"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row
                .as_str()
                .is_some_and(|text| text.contains("SSD-only read/write baselines"))));
    }

    fn example_performance_report() -> PerformanceReport {
        let disk_a = DiskId::new("disk-a").expect("disk id");
        let disk_b = DiskId::new("disk-b").expect("disk id");
        let ssd_file = PerformanceFileResult {
            file_index: 0,
            ssd_write: PerformanceMeasurement {
                bytes: 1_048_576,
                seconds: 0.5,
            },
            ssd_read: PerformanceMeasurement {
                bytes: 1_048_576,
                seconds: 0.25,
            },
        };
        let io_samples = vec![
            PerformanceIoSample {
                elapsed_second: 1,
                device_label: "ssd".to_string(),
                device_name: "nvme0n1".to_string(),
                read_bytes_per_second: 2_097_152,
                write_bytes_per_second: 4_194_304,
            },
            PerformanceIoSample {
                elapsed_second: 1,
                device_label: "disk-a".to_string(),
                device_name: "sda".to_string(),
                read_bytes_per_second: 0,
                write_bytes_per_second: 1_048_576,
            },
        ];
        let ssd_only = PerformanceScenarioResult {
            kind: PerformanceScenarioKind::SsdOnly,
            file_order: PerformanceFileOrder::Fifo,
            concurrency: 0,
            redundancy: 1,
            queue_capacity: 0,
            elapsed_seconds: 0.5,
            total_bytes: 1_048_576,
            logical_source_bytes: 1_048_576,
            physical_hdd_write_bytes: 0,
            hdd_write_operations: 0,
            hdd_drain_started_before_all_ssd_staged: false,
            file_results: vec![ssd_file.clone()],
            disk_results: Vec::new(),
            io_samples: io_samples.clone(),
            concurrency_result: PerformanceConcurrencyResult {
                concurrency: 0,
                scenario: PerformanceScenarioKind::SsdOnly,
                aggregate_bytes: 1_048_576,
                seconds: 0.5,
                slowest_seconds: 0.0,
                members: Vec::new(),
            },
        };
        let stage_then_drain_one = PerformanceScenarioResult {
            kind: PerformanceScenarioKind::SsdStageThenDrain,
            file_order: PerformanceFileOrder::SizeDesc,
            concurrency: 1,
            redundancy: 2,
            queue_capacity: 4,
            elapsed_seconds: 1.5,
            total_bytes: 2_097_152,
            logical_source_bytes: 1_048_576,
            physical_hdd_write_bytes: 2_097_152,
            hdd_write_operations: 2,
            hdd_drain_started_before_all_ssd_staged: false,
            file_results: vec![ssd_file.clone()],
            disk_results: vec![
                PerformanceDiskResult {
                    file_index: 0,
                    copy_index: 0,
                    concurrency: 1,
                    scenario: PerformanceScenarioKind::SsdStageThenDrain,
                    disk_id: disk_a.clone(),
                    ssd_read: PerformanceMeasurement {
                        bytes: 1_048_576,
                        seconds: 0.25,
                    },
                    write: PerformanceMeasurement {
                        bytes: 1_048_576,
                        seconds: 1.0,
                    },
                },
                PerformanceDiskResult {
                    file_index: 0,
                    copy_index: 1,
                    concurrency: 1,
                    scenario: PerformanceScenarioKind::SsdStageThenDrain,
                    disk_id: disk_b.clone(),
                    ssd_read: PerformanceMeasurement {
                        bytes: 1_048_576,
                        seconds: 0.25,
                    },
                    write: PerformanceMeasurement {
                        bytes: 1_048_576,
                        seconds: 1.0,
                    },
                },
            ],
            io_samples: io_samples.clone(),
            concurrency_result: PerformanceConcurrencyResult {
                concurrency: 1,
                scenario: PerformanceScenarioKind::SsdStageThenDrain,
                aggregate_bytes: 2_097_152,
                seconds: 1.5,
                slowest_seconds: 1.0,
                members: vec![disk_a.clone(), disk_b.clone()],
            },
        };
        let pipeline_one = PerformanceScenarioResult {
            kind: PerformanceScenarioKind::SsdPipeline,
            file_order: PerformanceFileOrder::SizeDesc,
            concurrency: 1,
            redundancy: 1,
            queue_capacity: 2,
            elapsed_seconds: 1.0,
            total_bytes: 1_048_576,
            logical_source_bytes: 1_048_576,
            physical_hdd_write_bytes: 1_048_576,
            hdd_write_operations: 1,
            hdd_drain_started_before_all_ssd_staged: true,
            file_results: vec![ssd_file.clone()],
            disk_results: vec![PerformanceDiskResult {
                file_index: 0,
                copy_index: 0,
                concurrency: 1,
                scenario: PerformanceScenarioKind::SsdPipeline,
                disk_id: disk_a.clone(),
                ssd_read: PerformanceMeasurement {
                    bytes: 1_048_576,
                    seconds: 0.25,
                },
                write: PerformanceMeasurement {
                    bytes: 1_048_576,
                    seconds: 1.0,
                },
            }],
            io_samples: io_samples.clone(),
            concurrency_result: PerformanceConcurrencyResult {
                concurrency: 1,
                scenario: PerformanceScenarioKind::SsdPipeline,
                aggregate_bytes: 1_048_576,
                seconds: 1.0,
                slowest_seconds: 1.0,
                members: vec![disk_a.clone()],
            },
        };
        let pipeline_two = PerformanceScenarioResult {
            kind: PerformanceScenarioKind::SsdPipeline,
            file_order: PerformanceFileOrder::SizeDesc,
            concurrency: 2,
            redundancy: 2,
            queue_capacity: 8,
            elapsed_seconds: 1.0,
            total_bytes: 2_097_152,
            logical_source_bytes: 1_048_576,
            physical_hdd_write_bytes: 2_097_152,
            hdd_write_operations: 2,
            hdd_drain_started_before_all_ssd_staged: true,
            file_results: vec![ssd_file.clone()],
            disk_results: vec![
                PerformanceDiskResult {
                    file_index: 0,
                    copy_index: 0,
                    concurrency: 2,
                    scenario: PerformanceScenarioKind::SsdPipeline,
                    disk_id: disk_a.clone(),
                    ssd_read: PerformanceMeasurement {
                        bytes: 1_048_576,
                        seconds: 0.25,
                    },
                    write: PerformanceMeasurement {
                        bytes: 1_048_576,
                        seconds: 1.0,
                    },
                },
                PerformanceDiskResult {
                    file_index: 0,
                    copy_index: 1,
                    concurrency: 2,
                    scenario: PerformanceScenarioKind::SsdPipeline,
                    disk_id: disk_b.clone(),
                    ssd_read: PerformanceMeasurement {
                        bytes: 1_048_576,
                        seconds: 0.25,
                    },
                    write: PerformanceMeasurement {
                        bytes: 1_048_576,
                        seconds: 2.0,
                    },
                },
            ],
            io_samples: io_samples.clone(),
            concurrency_result: PerformanceConcurrencyResult {
                concurrency: 2,
                scenario: PerformanceScenarioKind::SsdPipeline,
                aggregate_bytes: 2_097_152,
                seconds: 1.0,
                slowest_seconds: 2.0,
                members: vec![disk_a.clone(), disk_b.clone()],
            },
        };
        let direct_one = PerformanceScenarioResult {
            kind: PerformanceScenarioKind::DirectHdd,
            file_order: PerformanceFileOrder::SizeDesc,
            concurrency: 1,
            redundancy: 1,
            queue_capacity: 2,
            elapsed_seconds: 2.0,
            total_bytes: 1_048_576,
            logical_source_bytes: 1_048_576,
            physical_hdd_write_bytes: 1_048_576,
            hdd_write_operations: 1,
            hdd_drain_started_before_all_ssd_staged: false,
            file_results: Vec::new(),
            disk_results: vec![PerformanceDiskResult {
                file_index: 0,
                copy_index: 0,
                concurrency: 1,
                scenario: PerformanceScenarioKind::DirectHdd,
                disk_id: disk_a.clone(),
                ssd_read: zero_measurement(),
                write: PerformanceMeasurement {
                    bytes: 1_048_576,
                    seconds: 2.0,
                },
            }],
            io_samples: io_samples.clone(),
            concurrency_result: PerformanceConcurrencyResult {
                concurrency: 1,
                scenario: PerformanceScenarioKind::DirectHdd,
                aggregate_bytes: 1_048_576,
                seconds: 2.0,
                slowest_seconds: 2.0,
                members: vec![disk_a.clone()],
            },
        };
        let direct_two = PerformanceScenarioResult {
            kind: PerformanceScenarioKind::DirectHdd,
            file_order: PerformanceFileOrder::SizeDesc,
            concurrency: 2,
            redundancy: 2,
            queue_capacity: 8,
            elapsed_seconds: 2.0,
            total_bytes: 2_097_152,
            logical_source_bytes: 1_048_576,
            physical_hdd_write_bytes: 2_097_152,
            hdd_write_operations: 2,
            hdd_drain_started_before_all_ssd_staged: false,
            file_results: Vec::new(),
            disk_results: vec![
                PerformanceDiskResult {
                    file_index: 0,
                    copy_index: 0,
                    concurrency: 2,
                    scenario: PerformanceScenarioKind::DirectHdd,
                    disk_id: disk_a.clone(),
                    ssd_read: zero_measurement(),
                    write: PerformanceMeasurement {
                        bytes: 1_048_576,
                        seconds: 2.0,
                    },
                },
                PerformanceDiskResult {
                    file_index: 0,
                    copy_index: 1,
                    concurrency: 2,
                    scenario: PerformanceScenarioKind::DirectHdd,
                    disk_id: disk_b.clone(),
                    ssd_read: zero_measurement(),
                    write: PerformanceMeasurement {
                        bytes: 1_048_576,
                        seconds: 2.5,
                    },
                },
            ],
            io_samples,
            concurrency_result: PerformanceConcurrencyResult {
                concurrency: 2,
                scenario: PerformanceScenarioKind::DirectHdd,
                aggregate_bytes: 2_097_152,
                seconds: 2.0,
                slowest_seconds: 2.5,
                members: vec![disk_a.clone(), disk_b.clone()],
            },
        };
        PerformanceReport {
            run_id: "perf-test-run".to_string(),
            generated_at_utc: "2026-01-02T03:04:05Z".to_string(),
            repository_revision: "test-revision".to_string(),
            file_size: 1_048_576,
            file_count: 1,
            workload_kind: PerformanceWorkloadKind::Generated,
            source_path: None,
            source_cap_bytes: None,
            file_selection: PerformanceFileSelection::Random,
            file_orders: vec![PerformanceFileOrder::Fifo, PerformanceFileOrder::SizeDesc],
            discovered_file_count: 1,
            discovered_total_bytes: 1_048_576,
            total_source_bytes: 1_048_576,
            ssd_root: PathBuf::from("/ssd"),
            hdd_root: PathBuf::from("/hdd"),
            disk_count: 2,
            max_concurrency: 2,
            redundancy: 2,
            elapsed_seconds: 3.2,
            results: PerformanceBenchmarkResults {
                ssd_only: vec![ssd_only],
                ssd_stage_then_drain: vec![stage_then_drain_one],
                ssd_pipeline: vec![pipeline_one, pipeline_two],
                direct_hdd: vec![direct_one, direct_two],
            },
            recommendation: PerformanceRecommendation {
                strategy: PerformanceScenarioKind::SsdPipeline,
                file_order: PerformanceFileOrder::SizeDesc,
                hdd_concurrency: 2,
                aggregate_bytes_per_second: 2_097_152.0,
                reason: "SSD-first ingest remained competitive in the fixture".to_string(),
            },
            authoritative: true,
            authoritative_path: Some(PathBuf::from(
                "/var/lib/dasobjectstore/performance/authoritative-recommendation.json",
            )),
            tmp_dir: PathBuf::from("/tmp"),
            disks: vec![
                (disk_a, PathBuf::from("/hdd/disk-a")),
                (disk_b, PathBuf::from("/hdd/disk-b")),
            ],
            reproduction_args: vec![
                "dasobjectstore".to_string(),
                "performance-test".to_string(),
                "--file_size".to_string(),
                "1MiB".to_string(),
                "--file_count".to_string(),
                "1".to_string(),
            ],
            json_path: PathBuf::from("/tmp/perf-test-run.json"),
            qr_path: PathBuf::from("/tmp/perf-test-run.qr.svg"),
            pdf_path: PathBuf::from("/tmp/perf-test-run.pdf"),
            keep_temp: false,
            reproduce_command: "dasobjectstore performance-test --file_size 1MiB --file_count 1"
                .to_string(),
            reproduction_payload: r#"{"run_id":"perf-test-run"}"#.to_string(),
            reproduction_payload_sha256:
                "623f8d191890968ec394ff02950710ecb9e5eed5a0b68c064e28e8ffa0876f58".to_string(),
            qr_status: "qrencode SVG".to_string(),
        }
    }

    #[test]
    fn probe_with_multiple_formats_returns_clear_error() {
        let cli = Cli::try_parse_from(["dasobjectstore", "probe", "--json", "--pretty"])
            .expect("probe parses");
        let mut output = Vec::new();

        let err = run(&cli, &mut output).expect_err("only one format is allowed");

        assert!(matches!(err, CliError::UnsupportedProbeFormat));
    }

    #[test]
    fn health_with_multiple_formats_returns_clear_error() {
        let cli = Cli::try_parse_from(["dasobjectstore", "health", "--json", "--verbose"])
            .expect("health parses");
        let mut output = Vec::new();

        let err = run(&cli, &mut output).expect_err("only one format is allowed");

        assert!(matches!(err, CliError::UnsupportedHealthFormat));
    }

    #[test]
    fn health_connections_conflicts_with_other_health_formats() {
        let cli = Cli::try_parse_from(["dasobjectstore", "health", "--connections", "--json"])
            .expect("health parses");
        let mut output = Vec::new();

        let err = run(&cli, &mut output).expect_err("only one format is allowed");

        assert!(matches!(err, CliError::UnsupportedHealthFormat));
    }

    #[test]
    fn connection_status_warns_for_usb_transport() {
        let report = ProbeReport {
            platform: HostPlatform::Macos,
            disks: vec![ObservedDisk {
                device_path: Some("/dev/disk4".to_string()),
                size_bytes: Some(4_000_000_000_000),
                serial_hint: Some("USB-DAS-1".to_string()),
                model_hint: Some("QNAP DL800C".to_string()),
                partition_hints: Vec::new(),
                filesystem_hints: Vec::new(),
                direct_attached_hint: Some(true),
                removable_hint: Some(true),
                transport: Transport::Usb,
                enclosure_topology_path: Some("usb@001/002".to_string()),
            }],
            enclosures: Vec::new(),
            warnings: Vec::new(),
        };

        let status = connection_status_from_probe(&report);

        assert_eq!(status.disks[0].assessment, ConnectionAssessment::Warning);
        assert!(status.disks[0].warnings[0].contains("USB-attached DAS detected"));
        assert!(status.disks[0]
            .recommendation
            .as_deref()
            .expect("recommendation")
            .contains("No faster attached DAS path is visible"));
    }

    #[test]
    fn connection_status_recommends_observed_thunderbolt_path_for_usb_disk() {
        let report = ProbeReport {
            platform: HostPlatform::Macos,
            disks: vec![
                ObservedDisk {
                    device_path: Some("/dev/disk4".to_string()),
                    size_bytes: Some(4_000_000_000_000),
                    serial_hint: Some("USB-DAS-1".to_string()),
                    model_hint: Some("QNAP DL800C".to_string()),
                    partition_hints: Vec::new(),
                    filesystem_hints: Vec::new(),
                    direct_attached_hint: Some(true),
                    removable_hint: Some(true),
                    transport: Transport::Usb,
                    enclosure_topology_path: Some("usb@001/002".to_string()),
                },
                ObservedDisk {
                    device_path: Some("/dev/disk8".to_string()),
                    size_bytes: Some(4_000_000_000_000),
                    serial_hint: Some("TB-DAS-1".to_string()),
                    model_hint: Some("Thunderbolt DAS".to_string()),
                    partition_hints: Vec::new(),
                    filesystem_hints: Vec::new(),
                    direct_attached_hint: Some(true),
                    removable_hint: Some(true),
                    transport: Transport::Thunderbolt,
                    enclosure_topology_path: Some("thunderbolt@0/1".to_string()),
                },
            ],
            enclosures: Vec::new(),
            warnings: Vec::new(),
        };

        let status = connection_status_from_probe(&report);

        assert_eq!(status.disks[0].assessment, ConnectionAssessment::Warning);
        assert_eq!(status.disks[1].assessment, ConnectionAssessment::Good);
        assert_eq!(
            status.disks[0].recommendation.as_deref(),
            Some(
                "Prefer the observed Thunderbolt path used by /dev/disk8 at topology thunderbolt@0/1 for DAS workloads."
            )
        );
        assert_eq!(status.disks[1].recommendation, None);
    }

    #[test]
    fn writes_connection_status_with_performance_warning() {
        let report = ProbeReport {
            platform: HostPlatform::Linux,
            disks: vec![ObservedDisk {
                device_path: Some("/dev/sdb".to_string()),
                size_bytes: Some(1_000_000_000_000),
                serial_hint: None,
                model_hint: Some("ORICO 5 Bay".to_string()),
                partition_hints: Vec::new(),
                filesystem_hints: Vec::new(),
                direct_attached_hint: Some(true),
                removable_hint: Some(false),
                transport: Transport::Usb,
                enclosure_topology_path: Some("usb@003/004".to_string()),
            }],
            enclosures: Vec::new(),
            warnings: Vec::new(),
        };
        let status = connection_status_from_probe(&report);
        let mut output = Vec::new();

        write_host_connection_status(&status, &mut output).expect("status writes");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("transport=Usb"));
        assert!(output.contains("assessment=warning"));
        assert!(output.contains("slow USB links will reduce"));
        assert!(output.contains("Recommendation: No faster attached DAS path is visible"));
    }

    #[test]
    fn writes_connection_status_with_preferred_observed_path() {
        let report = ProbeReport {
            platform: HostPlatform::Linux,
            disks: vec![
                ObservedDisk {
                    device_path: Some("/dev/sdb".to_string()),
                    size_bytes: Some(1_000_000_000_000),
                    serial_hint: None,
                    model_hint: Some("ORICO 5 Bay".to_string()),
                    partition_hints: Vec::new(),
                    filesystem_hints: Vec::new(),
                    direct_attached_hint: Some(true),
                    removable_hint: Some(false),
                    transport: Transport::Usb,
                    enclosure_topology_path: Some("usb@003/004".to_string()),
                },
                ObservedDisk {
                    device_path: Some("/dev/sdc".to_string()),
                    size_bytes: Some(1_000_000_000_000),
                    serial_hint: None,
                    model_hint: Some("Thunderbolt DAS".to_string()),
                    partition_hints: Vec::new(),
                    filesystem_hints: Vec::new(),
                    direct_attached_hint: Some(true),
                    removable_hint: Some(false),
                    transport: Transport::Thunderbolt,
                    enclosure_topology_path: Some("thunderbolt@0/2".to_string()),
                },
            ],
            enclosures: Vec::new(),
            warnings: Vec::new(),
        };
        let status = connection_status_from_probe(&report);
        let mut output = Vec::new();

        write_host_connection_status(&status, &mut output).expect("status writes");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains(
            "Recommendation: Prefer the observed Thunderbolt path used by /dev/sdc at topology thunderbolt@0/2"
        ));
    }

    #[test]
    fn writes_pretty_probe_report() {
        let report = ProbeReport {
            platform: HostPlatform::Macos,
            disks: vec![ObservedDisk {
                device_path: Some("/dev/disk4".to_string()),
                size_bytes: Some(1_000),
                serial_hint: Some("SERIAL-1".to_string()),
                model_hint: None,
                partition_hints: Vec::new(),
                filesystem_hints: Vec::new(),
                direct_attached_hint: Some(true),
                removable_hint: Some(true),
                transport: Transport::Usb,
                enclosure_topology_path: Some("usb@001/002".to_string()),
            }],
            enclosures: vec![ObservedEnclosure {
                identity: EnclosureIdentity {
                    usb_topology_path: Some("usb@001/002".to_string()),
                    vendor_hint: None,
                    product_hint: None,
                    bridge_hint: None,
                    user_assigned_name: None,
                },
                disk_device_paths: vec!["/dev/disk4".to_string()],
            }],
            warnings: Vec::new(),
        };
        let mut output = Vec::new();

        write_pretty_report(&report, &mut output).expect("pretty output writes");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("Platform: Macos"));
        assert!(output.contains("- /dev/disk4 size=1000 transport=Usb serial=SERIAL-1"));
        assert!(output.contains(
            "- topology=usb@001/002 vendor=<unknown> product=<unknown> bridge=<unknown> disks=/dev/disk4"
        ));
    }

    #[test]
    fn writes_pretty_probe_report_with_qnap_enclosure_identity() {
        let report = ProbeReport {
            platform: HostPlatform::Linux,
            disks: Vec::new(),
            enclosures: vec![ObservedEnclosure {
                identity: EnclosureIdentity {
                    usb_topology_path: Some("pci-0000:00:14.0-usb-0:4:1.0".to_string()),
                    vendor_hint: Some("QNAP".to_string()),
                    product_hint: Some("TL-D800C".to_string()),
                    bridge_hint: Some("usb-jbod".to_string()),
                    user_assigned_name: None,
                },
                disk_device_paths: vec!["/dev/sda".to_string(), "/dev/sdb".to_string()],
            }],
            warnings: Vec::new(),
        };
        let mut output = Vec::new();

        write_pretty_report(&report, &mut output).expect("pretty output writes");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains(
            "- topology=pci-0000:00:14.0-usb-0:4:1.0 vendor=QNAP product=TL-D800C bridge=usb-jbod disks=/dev/sda,/dev/sdb"
        ));
    }

    #[test]
    fn writes_health_summary() {
        let report = health_report_fixture();
        let mut output = Vec::new();

        write_health_summary(&report, &mut output).expect("summary writes");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("Platform: Macos"));
        assert!(output.contains("Disks: 1"));
        assert!(output.contains("Watch: 1"));
        assert!(output.contains("- /dev/disk4 state=Watch score=75 smart=failing warnings=1"));
    }

    #[test]
    fn writes_health_verbose() {
        let report = health_report_fixture();
        let mut output = Vec::new();

        write_health_verbose(&report, &mut output).expect("verbose writes");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("Disk /dev/disk4"));
        assert!(output.contains("  Model: Old SATA HDD"));
        assert!(output.contains("  Smart warnings: 1"));
        assert!(output.contains("  Warning: macOS reports SMART failure"));
    }

    #[test]
    fn writes_health_json() {
        let report = health_report_fixture();
        let mut output = Vec::new();

        write_health_json(&report, &mut output).expect("json writes");

        let output: serde_json::Value =
            serde_json::from_slice(&output).expect("health output is json");
        assert_eq!(output["platform"], "Macos");
        assert_eq!(output["disk_count"], 1);
        assert_eq!(output["warning_count"], 2);
        assert_eq!(output["disks"][0]["score"]["state"], "Watch");
        assert_eq!(output["disks"][0]["signals"]["smart_warnings"], 1);
    }

    #[test]
    fn pool_inspect_writes_metadata_summary() {
        let root = temp_root("pool-inspect");
        write_snapshot_manifests(&root);
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "pool",
            "inspect",
            "--metadata-path",
            root.to_str().expect("utf8 temp path"),
        ])
        .expect("pool inspect parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("pool inspect runs");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("Pool: pool-a"));
        assert!(output.contains("State: Clean"));
        assert!(output.contains("Disks: 1"));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn pool_import_read_only_imports_clean_snapshot() {
        let root = temp_root("pool-import-clean");
        let source_root = root.join("mounted-disk");
        let recovery_root = root.join("recovered");
        create_portable_pool_snapshot(&root.join("source-ssd"), &source_root, "Clean");
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "pool",
            "import",
            "--read-only",
            "--source-path",
            source_root.to_str().expect("utf8 source path"),
            "--recovery-metadata-dir",
            recovery_root.to_str().expect("utf8 recovery path"),
            "--recorded-at-utc",
            "2026-01-04T00:00:00Z",
        ])
        .expect("pool import parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("pool import runs");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("Pool: pool-a"));
        assert!(output.contains("Mode: read-only"));
        assert_eq!(pool_state(&recovery_root.join("live.sqlite")), "ReadOnly");

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn pool_import_read_only_imports_dirty_snapshot() {
        let root = temp_root("pool-import-dirty");
        let source_root = root.join("mounted-disk");
        let recovery_root = root.join("recovered");
        create_portable_pool_snapshot(&root.join("source-ssd"), &source_root, "Dirty");
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "pool",
            "import",
            "--read-only",
            "--source-path",
            source_root.to_str().expect("utf8 source path"),
            "--recovery-metadata-dir",
            recovery_root.to_str().expect("utf8 recovery path"),
            "--recorded-at-utc",
            "2026-01-04T00:00:00Z",
        ])
        .expect("pool import parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("pool import runs");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("Pool: pool-a"));
        assert!(output.contains("Mode: read-only"));
        assert_eq!(pool_state(&recovery_root.join("live.sqlite")), "ReadOnly");

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn pool_repair_dry_run_reports_dirty_snapshot_without_writing() {
        let root = temp_root("pool-repair-dirty");
        let source_root = root.join("mounted-disk");
        create_portable_pool_snapshot(&root.join("source-ssd"), &source_root, "Dirty");
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "pool",
            "repair",
            "--source-path",
            source_root.to_str().expect("utf8 source path"),
            "--dry-run",
        ])
        .expect("pool repair parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("pool repair dry run runs");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("Pool repair dry run"));
        assert!(output.contains("Pool: pool-a"));
        assert!(output.contains("State: Dirty"));
        assert!(output.contains("Disks: 1"));
        assert!(output.contains("Planned action: read-only recovery import"));
        assert!(!root.join("recovered").exists());

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn disk_retire_marks_disk_draining() {
        let root = temp_root("disk-retire");
        fs::create_dir_all(&root).expect("create temp root");
        let live_sqlite_path = root.join("live.sqlite");
        let connection = Connection::open(&live_sqlite_path).expect("open live sqlite");
        connection
            .execute_batch(LIVE_SCHEMA_SQL)
            .expect("schema applies");
        insert_store(&connection);
        insert_disk(&connection, "disk-a", "Healthy");
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "disk",
            "retire",
            "disk-a",
            "--live-sqlite-path",
            live_sqlite_path.to_str().expect("utf8 live sqlite path"),
            "--recorded-at-utc",
            "2026-01-02T00:00:00Z",
        ])
        .expect("disk retire parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("disk retire runs");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("Disk retirement requested: disk-a"));
        assert!(output.contains("Previous state: Healthy"));
        assert!(output.contains("Next state: Draining"));
        assert_eq!(disk_state(&connection, "disk-a"), "Draining");

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn disk_force_retire_requires_policy_allowance() {
        let root = temp_root("disk-force-retire-denied");
        fs::create_dir_all(&root).expect("create temp root");
        let live_sqlite_path = root.join("live.sqlite");
        let connection = Connection::open(&live_sqlite_path).expect("open live sqlite");
        connection
            .execute_batch(LIVE_SCHEMA_SQL)
            .expect("schema applies");
        insert_store(&connection);
        insert_disk(&connection, "disk-a", "Healthy");
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "disk",
            "force-retire",
            "disk-a",
            "--live-sqlite-path",
            live_sqlite_path.to_str().expect("utf8 live sqlite path"),
            "--recorded-at-utc",
            "2026-01-02T00:00:00Z",
            "--confirm",
            "confirm force retire",
        ])
        .expect("disk force-retire parses");
        let mut output = Vec::new();

        let err = run(&cli, &mut output).expect_err("risk policy blocks force retire");

        assert!(matches!(err, CliError::DiskRetirement(_)));
        assert_eq!(disk_state(&connection, "disk-a"), "Healthy");

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn disk_force_retire_marks_disk_retired_after_risk_confirmation() {
        let root = temp_root("disk-force-retire");
        fs::create_dir_all(&root).expect("create temp root");
        let live_sqlite_path = root.join("live.sqlite");
        let connection = Connection::open(&live_sqlite_path).expect("open live sqlite");
        connection
            .execute_batch(LIVE_SCHEMA_SQL)
            .expect("schema applies");
        insert_store(&connection);
        insert_disk(&connection, "disk-a", "Healthy");
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "disk",
            "force-retire",
            "disk-a",
            "--live-sqlite-path",
            live_sqlite_path.to_str().expect("utf8 live sqlite path"),
            "--recorded-at-utc",
            "2026-01-02T00:00:00Z",
            "--allow-force-retire",
            "--confirm",
            "confirm force retire",
        ])
        .expect("disk force-retire parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("disk force-retire runs");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("Disk force-retired: disk-a"));
        assert!(output.contains("Previous state: Healthy"));
        assert!(output.contains("Next state: Retired"));
        assert_eq!(disk_state(&connection, "disk-a"), "Retired");

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn disk_drain_writes_pretty_plan() {
        let root = temp_root("disk-drain-pretty");
        let live_sqlite_path = create_live_sqlite_with_object(&root);
        let connection = Connection::open(&live_sqlite_path).expect("open live sqlite");
        insert_disk(&connection, "disk-b", "Healthy");
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "disk",
            "drain",
            "disk-a",
            "--live-sqlite-path",
            live_sqlite_path.to_str().expect("utf8 live sqlite path"),
        ])
        .expect("disk drain parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("disk drain runs");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("Disk drain plan: disk-a"));
        assert!(output.contains("Protected copy tasks: 1"));
        assert!(output.contains("- object-a store=store-a action=copy_planned destinations=disk-b"));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn disk_drain_writes_json_plan() {
        let root = temp_root("disk-drain-json");
        let live_sqlite_path = create_live_sqlite_with_object(&root);
        let connection = Connection::open(&live_sqlite_path).expect("open live sqlite");
        insert_disk(&connection, "disk-b", "Healthy");
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "disk",
            "drain",
            "disk-a",
            "--live-sqlite-path",
            live_sqlite_path.to_str().expect("utf8 live sqlite path"),
            "--json",
        ])
        .expect("disk drain parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("disk drain runs");

        let output: serde_json::Value =
            serde_json::from_slice(&output).expect("drain output is json");
        assert_eq!(output["disk_id"], "disk-a");
        assert_eq!(output["protected_copy_tasks"], 1);
        assert_eq!(output["affected_objects"][0]["action"], "copy_planned");

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn disk_replace_writes_pretty_plan() {
        let root = temp_root("disk-replace-pretty");
        let live_sqlite_path = create_live_sqlite_with_object(&root);
        let connection = Connection::open(&live_sqlite_path).expect("open live sqlite");
        insert_disk(&connection, "disk-b", "Healthy");
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "disk",
            "replace",
            "disk-a",
            "--with",
            "disk-b",
            "--live-sqlite-path",
            live_sqlite_path.to_str().expect("utf8 live sqlite path"),
        ])
        .expect("disk replace parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("disk replace runs");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("Disk replacement plan: disk-a -> disk-b"));
        assert!(output.contains("Protected copy tasks: 1"));
        assert!(output.contains("- object-a store=store-a action=copy_planned destinations=disk-b"));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn disk_replace_writes_json_plan() {
        let root = temp_root("disk-replace-json");
        let live_sqlite_path = create_live_sqlite_with_object(&root);
        let connection = Connection::open(&live_sqlite_path).expect("open live sqlite");
        insert_disk(&connection, "disk-b", "Healthy");
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "disk",
            "replace",
            "disk-a",
            "--with",
            "disk-b",
            "--live-sqlite-path",
            live_sqlite_path.to_str().expect("utf8 live sqlite path"),
            "--json",
        ])
        .expect("disk replace parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("disk replace runs");

        let output: serde_json::Value =
            serde_json::from_slice(&output).expect("replacement output is json");
        assert_eq!(output["old_disk_id"], "disk-a");
        assert_eq!(output["new_disk_id"], "disk-b");
        assert_eq!(output["protected_copy_tasks"], 1);
        assert_eq!(output["affected_objects"][0]["action"], "copy_planned");

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn store_create_writes_system_registry_definition() {
        let root = temp_root("store-create");
        fs::create_dir_all(&root).expect("create temp root");
        let registry_path = root.join("stores.json");
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "create",
            "generated-data",
            "--class",
            "generated_data",
            "--copies",
            "2",
            "--bucket",
            "generated-data",
            "--registry-path",
            registry_path.to_str().expect("utf8 registry path"),
        ])
        .expect("store create parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("store create runs");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("Store created"));
        assert!(output.contains("Store: generated-data"));
        assert!(output.contains("Bucket: generated-data"));
        assert!(output.contains("Registry: system-managed"));

        let definitions: Vec<StoreServiceDefinition> =
            serde_json::from_reader(File::open(&registry_path).expect("open registry"))
                .expect("registry json");
        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0].store_id.as_str(), "generated-data");
        assert_eq!(definitions[0].policy.class, StoreClass::GeneratedData);
        assert_eq!(definitions[0].policy.copies, 2);
        assert_eq!(
            definitions[0].bucket_name.as_deref(),
            Some("generated-data")
        );

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn store_create_mirrors_definition_to_known_portable_ssd() {
        let root = temp_root("store-create-portable");
        let host_registry_path = root.join("host").join("stores.json");
        let ssd_root = root.join("ssd");
        create_known_ssd_marker(&ssd_root);
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "create",
            "generated-data",
            "--class",
            "generated_data",
            "--bucket",
            "generated-data",
            "--ssd-root",
            ssd_root.to_str().expect("utf8 ssd root"),
            "--registry-path",
            host_registry_path
                .to_str()
                .expect("utf8 host registry path"),
        ])
        .expect("store create parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("store create runs");

        let portable_registry_path = ssd_root.join(".dasobjectstore").join("stores.json");
        assert!(portable_registry_path.is_file());
        let portable_definitions: Vec<StoreServiceDefinition> = serde_json::from_reader(
            File::open(&portable_registry_path).expect("open portable registry"),
        )
        .expect("portable registry json");
        assert_eq!(portable_definitions.len(), 1);
        assert_eq!(portable_definitions[0].store_id.as_str(), "generated-data");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("Portable registry:"));
        assert!(output.contains(".dasobjectstore/stores.json"));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn store_create_das_guard_accepts_managed_hdd_on_qnap_tl_d800c() {
        let managed_hdds = vec![ManagedHddDevice {
            disk_id: DiskId::new("disk-a").expect("disk id"),
            root_path: PathBuf::from("/srv/dasobjectstore/hdd/disk-a"),
            device_path: PathBuf::from("/dev/sda"),
        }];
        let report = ProbeReport {
            platform: HostPlatform::Linux,
            disks: vec![ObservedDisk {
                device_path: Some("/dev/sda".to_string()),
                size_bytes: Some(4_000_000_000_000),
                serial_hint: None,
                model_hint: Some("ST4000VN008".to_string()),
                partition_hints: Vec::new(),
                filesystem_hints: Vec::new(),
                direct_attached_hint: Some(true),
                removable_hint: Some(false),
                transport: Transport::Usb,
                enclosure_topology_path: Some("qnap-tl-d800c@pci-0000:00:14.0-usb-0:5".to_string()),
            }],
            enclosures: vec![ObservedEnclosure {
                identity: EnclosureIdentity {
                    usb_topology_path: Some("pci-0000:00:14.0-usb-0:5".to_string()),
                    vendor_hint: Some("QNAP".to_string()),
                    product_hint: Some("TL-D800C".to_string()),
                    bridge_hint: Some("usb-jbod".to_string()),
                    user_assigned_name: None,
                },
                disk_device_paths: vec!["/dev/sda".to_string()],
            }],
            warnings: Vec::new(),
        };

        validate_managed_hdds_on_supported_das(&managed_hdds, &report)
            .expect("supported DAS passes");
    }

    #[test]
    fn store_create_das_guard_rejects_generic_usb_hdd() {
        let managed_hdds = vec![ManagedHddDevice {
            disk_id: DiskId::new("disk-a").expect("disk id"),
            root_path: PathBuf::from("/srv/dasobjectstore/hdd/disk-a"),
            device_path: PathBuf::from("/dev/sda"),
        }];
        let report = ProbeReport {
            platform: HostPlatform::Linux,
            disks: vec![ObservedDisk {
                device_path: Some("/dev/sda".to_string()),
                size_bytes: Some(4_000_000_000_000),
                serial_hint: None,
                model_hint: Some("Generic USB Disk".to_string()),
                partition_hints: Vec::new(),
                filesystem_hints: Vec::new(),
                direct_attached_hint: Some(true),
                removable_hint: Some(false),
                transport: Transport::Usb,
                enclosure_topology_path: Some("pci-0000:00:14.0-usb-0:3:1.0".to_string()),
            }],
            enclosures: vec![ObservedEnclosure {
                identity: EnclosureIdentity {
                    usb_topology_path: Some("pci-0000:00:14.0-usb-0:3:1.0".to_string()),
                    vendor_hint: None,
                    product_hint: None,
                    bridge_hint: None,
                    user_assigned_name: None,
                },
                disk_device_paths: vec!["/dev/sda".to_string()],
            }],
            warnings: Vec::new(),
        };

        let err = validate_managed_hdds_on_supported_das(&managed_hdds, &report)
            .expect_err("generic USB is rejected");

        assert!(err
            .to_string()
            .contains("no QNAP TL-D800C enclosure was detected"));
    }

    #[test]
    fn store_create_das_guard_rejects_unmatched_managed_hdd_device() {
        let managed_hdds = vec![ManagedHddDevice {
            disk_id: DiskId::new("disk-a").expect("disk id"),
            root_path: PathBuf::from("/srv/dasobjectstore/hdd/disk-a"),
            device_path: PathBuf::from("/dev/sdz"),
        }];
        let report = ProbeReport {
            platform: HostPlatform::Linux,
            disks: vec![ObservedDisk {
                device_path: Some("/dev/sda".to_string()),
                size_bytes: Some(4_000_000_000_000),
                serial_hint: None,
                model_hint: Some("ST4000VN008".to_string()),
                partition_hints: Vec::new(),
                filesystem_hints: Vec::new(),
                direct_attached_hint: Some(true),
                removable_hint: Some(false),
                transport: Transport::Usb,
                enclosure_topology_path: Some("qnap-tl-d800c@pci-0000:00:14.0-usb-0:5".to_string()),
            }],
            enclosures: vec![ObservedEnclosure {
                identity: EnclosureIdentity {
                    usb_topology_path: Some("pci-0000:00:14.0-usb-0:5".to_string()),
                    vendor_hint: Some("QNAP".to_string()),
                    product_hint: Some("TL-D800C".to_string()),
                    bridge_hint: Some("usb-jbod".to_string()),
                    user_assigned_name: None,
                },
                disk_device_paths: vec!["/dev/sda".to_string()],
            }],
            warnings: Vec::new(),
        };

        let err = validate_managed_hdds_on_supported_das(&managed_hdds, &report)
            .expect_err("unmatched device is rejected");

        assert!(err
            .to_string()
            .contains("device was not found in the current probe"));
    }

    #[test]
    fn store_adopt_imports_portable_registry_to_host_registry() {
        let root = temp_root("store-adopt-portable");
        let host_registry_path = root.join("host").join("stores.json");
        let ssd_root = root.join("ssd");
        create_known_ssd_marker(&ssd_root);
        let portable_registry_path = ssd_root.join(".dasobjectstore").join("stores.json");
        write_store_definitions_file(
            &portable_registry_path,
            vec![StoreServiceDefinition {
                store_id: StoreId::new("public-reference").expect("store id"),
                policy: StorePolicy::defaults_for(StoreClass::ReproducibleCache),
                bucket_name: Some("public-reference".to_string()),
                writer_group: Some(test_writer_group()),
            }],
        );
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "adopt",
            "--ssd-root",
            ssd_root.to_str().expect("utf8 ssd root"),
            "--registry-path",
            host_registry_path
                .to_str()
                .expect("utf8 host registry path"),
        ])
        .expect("store adopt parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("store adopt runs");

        let host_definitions: Vec<StoreServiceDefinition> =
            serde_json::from_reader(File::open(&host_registry_path).expect("open host registry"))
                .expect("host registry json");
        assert_eq!(host_definitions.len(), 1);
        assert_eq!(host_definitions[0].store_id.as_str(), "public-reference");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("Portable store registry adopted"));
        assert!(output.contains("Stores adopted: 1"));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn store_list_reads_portable_registry_from_known_ssd() {
        let root = temp_root("store-list-portable");
        let ssd_root = root.join("ssd");
        create_known_ssd_marker(&ssd_root);
        write_store_definitions_file(
            &ssd_root.join(".dasobjectstore").join("stores.json"),
            vec![StoreServiceDefinition {
                store_id: StoreId::new("portable-generated").expect("store id"),
                policy: StorePolicy::defaults_for(StoreClass::GeneratedData),
                bucket_name: Some("portable-generated".to_string()),
                writer_group: Some(test_writer_group()),
            }],
        );
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "list",
            "--portable",
            "--ssd-root",
            ssd_root.to_str().expect("utf8 ssd root"),
        ])
        .expect("store list parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("store list runs");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("portable-generated"));
        assert!(output.contains("bucket=portable-generated"));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn store_list_reads_system_registry_definitions() {
        let root = temp_root("store-list");
        fs::create_dir_all(&root).expect("create temp root");
        let registry_path = root.join("stores.json");
        write_store_definitions_file(
            &registry_path,
            vec![StoreServiceDefinition {
                store_id: StoreId::new("generated-data").expect("store id"),
                policy: StorePolicy::defaults_for(StoreClass::GeneratedData),
                bucket_name: Some("generated-data".to_string()),
                writer_group: Some(test_writer_group()),
            }],
        );
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "list",
            "--registry-path",
            registry_path.to_str().expect("utf8 registry path"),
        ])
        .expect("store list parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("store list runs");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("Object stores: 1"));
        assert!(output.contains("generated-data"));
        assert!(output.contains("class=generated_data"));
        assert!(output.contains("copies=2"));
        assert!(output.contains("bucket=generated-data"));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn store_list_writes_json() {
        let root = temp_root("store-list-json");
        fs::create_dir_all(&root).expect("create temp root");
        let registry_path = root.join("stores.json");
        write_store_definitions_file(
            &registry_path,
            vec![StoreServiceDefinition {
                store_id: StoreId::new("public-reference").expect("store id"),
                policy: StorePolicy::defaults_for(StoreClass::ReproducibleCache),
                bucket_name: None,
                writer_group: Some(test_writer_group()),
            }],
        );
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "list",
            "--registry-path",
            registry_path.to_str().expect("utf8 registry path"),
            "--json",
        ])
        .expect("store list parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("store list runs");

        let output: serde_json::Value =
            serde_json::from_slice(&output).expect("store list output is json");
        assert_eq!(output[0]["store_id"], "public-reference");
        assert_eq!(output[0]["policy"]["class"], "ReproducibleCache");

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn store_contents_writes_du_summary_with_depth_and_filter() {
        let root = temp_root("store-contents-du");
        let live_sqlite_path = create_live_sqlite_with_store_contents(&root);
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "contents",
            "store-a",
            "--du",
            "--depth",
            "1",
            "--filter",
            r"\.(pod5|fastq\.gz)$",
            "--live-sqlite-path",
            live_sqlite_path.to_str().expect("utf8 live sqlite path"),
        ])
        .expect("store contents parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("store contents runs");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("Store contents"));
        assert!(output.contains("Store: store-a"));
        assert!(output.contains("Objects: 2"));
        assert!(output.contains("Mode: du depth=1"));
        assert!(output.contains("."));
        assert!(output.contains("\traw"));
        assert!(!output.contains("notes.txt"));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn store_contents_writes_tree() {
        let root = temp_root("store-contents-tree");
        let live_sqlite_path = create_live_sqlite_with_store_contents(&root);
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "contents",
            "store-a",
            "--tree",
            "--depth",
            "3",
            "--live-sqlite-path",
            live_sqlite_path.to_str().expect("utf8 live sqlite path"),
        ])
        .expect("store contents parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("store contents runs");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("Mode: tree depth=3"));
        assert!(output.contains("- raw/"));
        assert!(output.contains("  - PAW10254/"));
        assert!(output.contains("    - sample.pod5"));
        assert!(output.contains("- notes.txt"));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn store_contents_writes_json_snapshot() {
        let root = temp_root("store-contents-json");
        let live_sqlite_path = create_live_sqlite_with_store_contents(&root);
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "contents",
            "store-a",
            "--json",
            "--filter",
            "pod5$",
            "--live-sqlite-path",
            live_sqlite_path.to_str().expect("utf8 live sqlite path"),
        ])
        .expect("store contents parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("store contents runs");

        let output: serde_json::Value =
            serde_json::from_slice(&output).expect("store contents output is json");
        assert_eq!(output["store_id"], "store-a");
        assert_eq!(
            output["objects"].as_array().expect("objects array").len(),
            1
        );
        assert_eq!(output["objects"][0]["path"], "raw/PAW10254/sample.pod5");

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn store_s3_upload_renders_remote_aws_commands() {
        let root = temp_root("store-s3-upload");
        fs::create_dir_all(&root).expect("create temp root");
        let registry_path = root.join("stores.json");
        write_store_definitions_file(
            &registry_path,
            vec![StoreServiceDefinition {
                store_id: StoreId::new("generated-data").expect("store id"),
                policy: StorePolicy::defaults_for(StoreClass::GeneratedData),
                bucket_name: Some("dos-generated-data".to_string()),
                writer_group: Some(test_writer_group()),
            }],
        );
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "s3-upload",
            "generated-data",
            "--endpoint-url",
            "http://appliance.local:3900",
            "--registry-path",
            registry_path.to_str().expect("utf8 registry path"),
        ])
        .expect("store s3-upload parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("store s3-upload runs");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("Remote S3 upload plan"));
        assert!(output.contains("Bucket: dos-generated-data"));
        assert!(output.contains("Credential authority: mneion"));
        assert!(output.contains("aws --profile dasobjectstore-generated-data"));
        assert!(output.contains("s3api put-object"));
        assert!(output.contains("s3 cp <local-file>"));
        assert!(output.contains("s3 sync <local-directory>"));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn store_s3_upload_writes_json() {
        let root = temp_root("store-s3-upload-json");
        fs::create_dir_all(&root).expect("create temp root");
        let registry_path = root.join("stores.json");
        write_store_definitions_file(
            &registry_path,
            vec![StoreServiceDefinition {
                store_id: StoreId::new("generated-data").expect("store id"),
                policy: StorePolicy::defaults_for(StoreClass::GeneratedData),
                bucket_name: Some("dos-generated-data".to_string()),
                writer_group: Some(test_writer_group()),
            }],
        );
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "s3-upload",
            "generated-data",
            "--endpoint-url",
            "https://appliance.local:3900",
            "--auth",
            "local-password",
            "--username",
            "alice",
            "--json",
            "--registry-path",
            registry_path.to_str().expect("utf8 registry path"),
        ])
        .expect("store s3-upload parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("store s3-upload runs");

        let output: serde_json::Value =
            serde_json::from_slice(&output).expect("s3-upload output is json");
        assert_eq!(output["auth_authority"], "local_password");
        assert_eq!(output["username"], "alice");
        assert_eq!(output["bucket_name"], "dos-generated-data");
        assert!(output["aws_s3api_put_object_command"]
            .as_str()
            .expect("command string")
            .contains("s3api put-object"));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn store_s3_upload_accepts_explicit_bucket_without_registry() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "s3-upload",
            "generated-data",
            "--endpoint-url",
            "https://appliance.local:3900",
            "--bucket",
            "dos-generated-data",
            "--json",
        ])
        .expect("store s3-upload parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("store s3-upload runs");

        let output: serde_json::Value =
            serde_json::from_slice(&output).expect("s3-upload output is json");
        assert_eq!(output["store_id"], "generated-data");
        assert_eq!(output["bucket_name"], "dos-generated-data");
        assert_eq!(
            output["credential_reference"],
            "secret://dasobjectstore/stores/generated-data/s3"
        );
    }

    #[test]
    fn store_validate_accepts_valid_policy_file() {
        let root = temp_root("store-validate-valid");
        fs::create_dir_all(&root).expect("create temp root");
        let policy_file = root.join("policy.json");
        write_policy_file(
            &policy_file,
            &StorePolicy::defaults_for(StoreClass::GeneratedData),
        );
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "validate",
            policy_file.to_str().expect("utf8 policy path"),
        ])
        .expect("store validate parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("store validate runs");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("Store policy is valid: generated_data"));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn store_validate_rejects_invalid_policy_file() {
        let root = temp_root("store-validate-invalid");
        fs::create_dir_all(&root).expect("create temp root");
        let policy_file = root.join("policy.json");
        let mut policy = StorePolicy::defaults_for(StoreClass::GeneratedData);
        policy.capacity_behavior = CapacityBehavior::MarkRedownloadRequired;
        write_policy_file(&policy_file, &policy);
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "validate",
            policy_file.to_str().expect("utf8 policy path"),
        ])
        .expect("store validate parses");
        let mut output = Vec::new();

        let err = run(&cli, &mut output).expect_err("store validate should fail");

        match err {
            CliError::StorePolicyValidation(err) => {
                assert_eq!(
                    err.errors,
                    vec![
                        StorePolicyValidationError::ProtectedStoreMarksRedownloadRequired {
                            class: StoreClass::GeneratedData
                        }
                    ]
                );
            }
            err => panic!("unexpected error: {err}"),
        }

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn store_defaults_writes_policy_json() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "defaults",
            "--class",
            "critical_metadata",
        ])
        .expect("store defaults parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("store defaults runs");

        let policy: StorePolicy = serde_json::from_slice(&output).expect("policy json parses");
        assert_eq!(
            policy,
            StorePolicy::defaults_for(StoreClass::CriticalMetadata)
        );
    }

    #[test]
    fn ingest_status_writes_ssd_capacity_summary() {
        let root = temp_root("ingest-status");
        fs::create_dir_all(&root).expect("create temp root");
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "ingest",
            "status",
            "--ssd-root",
            root.to_str().expect("utf8 temp path"),
        ])
        .expect("ingest status parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("ingest status runs");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("SSD ingest root:"));
        assert!(output.contains("Pressure:"));
        assert!(output.contains("High watermark percent: 85"));
        assert!(output.contains("Critical watermark percent: 95"));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn ingest_files_reports_byte_progress_and_ssd_stress() {
        let root = temp_root("ingest-files");
        let source_root = root.join("external");
        let ssd_root = root.join("ssd");
        let hdd_base = root.join("hdd");
        let hdd_root = hdd_base.join("disk-a");
        let registry_path = root.join("stores.json");
        let subobject_registry_path = root.join("subobjects.json");
        fs::create_dir_all(source_root.join("nested")).expect("create source");
        create_known_hdd_marker(&hdd_root, "disk-a");
        create_known_ssd_marker(&ssd_root);
        fs::write(
            source_root.join("nested").join("sample.fastq.gz"),
            b"ACGT".repeat(256),
        )
        .expect("write source file");
        write_store_definitions_file(
            &registry_path,
            vec![StoreServiceDefinition {
                store_id: StoreId::new("zymo_fecal_2025.05").expect("store id"),
                policy: StorePolicy::defaults_for(StoreClass::ReproducibleCache),
                bucket_name: Some("dos-zymo-fecal-2025-05".to_string()),
                writer_group: Some(test_writer_group()),
            }],
        );
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "ingest",
            "files",
            "zymo_fecal_2025.05",
            "--source",
            source_root.to_str().expect("utf8 source root"),
            "--object-type",
            "pod5",
            "--local-direct",
            "--ssd-root",
            ssd_root.to_str().expect("utf8 ssd root"),
            "--hdd-root",
            hdd_base.to_str().expect("utf8 hdd root"),
            "--registry-path",
            registry_path.to_str().expect("utf8 registry path"),
            "--subobject-registry-path",
            subobject_registry_path
                .to_str()
                .expect("utf8 subobject registry path"),
        ])
        .expect("ingest files parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("ingest files runs");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("File ingest plan"));
        assert!(output.contains("Store: zymo_fecal_2025.05"));
        assert!(output.contains("Object type: pod5"));
        assert!(output.contains("SSD stress before file: pressure="));
        assert!(output.contains("stage=ssd-ingest"));
        assert!(output.contains("stage=hdd-copy:disk-a:1"));
        assert!(output.contains("remaining=0"));
        assert!(output.contains("File complete: nested/sample.fastq.gz"));
        assert!(output.contains("File ingest complete"));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn ingest_files_submits_daemon_request_on_normal_path() {
        let source_root = PathBuf::from("/mnt/external/zymo");
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "ingest",
            "files",
            "zymo_fecal_2025.05",
            "--source",
            source_root.to_str().expect("utf8 source"),
            "--object-type",
            "fastq",
            "--copies",
            "1",
            "--force",
            "--tui",
        ])
        .expect("ingest files parses");
        let Some(crate::cli::Command::Ingest(args)) = cli.command() else {
            panic!("expected ingest command");
        };
        let Some(crate::cli::IngestCommand::Files(files)) = args.command() else {
            panic!("expected ingest files command");
        };
        let transport = InProcessDaemonTransport::new(|request| {
            match request {
                DaemonApiRequest::SubmitIngestFiles(request) => {
                    assert_eq!(request.endpoint.as_str(), "zymo_fecal_2025.05");
                    assert_eq!(request.source_path, source_root);
                    assert_eq!(
                        request.object_type,
                        dasobjectstore_core::object_type::ObjectType::Fastq
                    );
                    assert_eq!(request.copies, Some(1));
                    assert_eq!(request.conflict_policy, DaemonIngestConflictPolicy::Force);
                    assert!(!request.dry_run);
                }
                _ => panic!("expected submit ingest files request"),
            }
            Ok(DaemonApiResponse::SubmitIngestFiles(
                SubmitIngestFilesResponse {
                    job_id: IngestJobId::new("job-zymo").expect("job id"),
                    accepted_at_utc: "2026-07-07T10:27:12Z".to_string(),
                    dry_run: false,
                },
            ))
        });
        let client = DaemonClient::new(transport);
        let mut output = Vec::new();

        super::run_ingest_files_with_client(files, &client, &mut output)
            .expect("daemon ingest submission runs");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("DASObjectStore Upload"));
        assert!(output.contains("Final response: job=job-zymo"));
        assert!(output.contains("zymo_fecal_2025.05"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn source_acl_plan_grants_private_home_traversal_and_private_source_read() {
        let root = temp_root("source-acl-private");
        let home = root.join("home").join("stephen");
        let source = home.join("zymo_fecal_2025.05");
        fs::create_dir_all(&source).expect("create private source");
        set_mode(&root, 0o755);
        set_mode(root.join("home"), 0o755);
        set_mode(&home, 0o750);
        set_mode(&source, 0o750);

        let actions = super::plan_source_acl_actions(&source).expect("source acl plan");

        assert!(actions.contains(&super::SourceAclAction {
            path: home,
            permission: super::SourceAclPermission::Traverse,
        }));
        assert!(actions.contains(&super::SourceAclAction {
            path: source.clone(),
            permission: super::SourceAclPermission::ReadTree,
        }));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn source_acl_plan_skips_recursive_acl_for_public_source_root() {
        let root = temp_root("source-acl-public");
        let mount = root.join("mnt");
        let source = mount.join("external");
        fs::create_dir_all(&source).expect("create public source");
        set_mode(&root, 0o755);
        set_mode(&mount, 0o755);
        set_mode(&source, 0o755);

        let actions = super::plan_source_acl_actions(&source).expect("source acl plan");

        assert!(!actions.contains(&super::SourceAclAction {
            path: source.clone(),
            permission: super::SourceAclPermission::ReadTree,
        }));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn daemon_ingest_progress_renderer_reports_byte_progress() {
        let progress = DaemonIngestProgressEvent {
            job_id: IngestJobId::new("job-zymo").expect("job id"),
            endpoint: StoreId::new("zymo_fecal_2025.05").expect("store id"),
            stage: DaemonIngestStage::HddCopy {
                disk_id: DiskId::new("disk-a").expect("disk id"),
                copy_number: 1,
            },
            pipeline_stage: None,
            work_bytes_done: 512,
            work_bytes_total: Some(1024),
            source_bytes_done: Some(512),
            source_bytes_total: Some(1024),
            stage_bytes_done: Some(512),
            stage_bytes_total: Some(1024),
            files_done: 2,
            files_total: Some(4),
            current_object_id: None,
            ssd_pressure: Some(DaemonSsdPressure::AcceptingWrites),
            telemetry: None,
            resource_policy: None,
            message: None,
        };
        let mut output = Vec::new();

        super::write_daemon_ingest_progress(&mut output, &progress, std::time::Instant::now())
            .expect("progress writes");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("50%"));
        assert!(output.contains("files=2/4"));
        assert!(output.contains("remaining=2"));
        assert!(output.contains("stage=hdd-copy:disk-a:1"));
        assert!(output.contains("ssd=AcceptingWrites"));
    }

    #[test]
    fn subobject_create_list_and_search_report_nested_prefixes() {
        let root = temp_root("subobject-cli");
        let stores_registry_path = root.join("stores.json");
        let subobject_registry_path = root.join("subobjects.json");
        let ssd_root = root.join("ssd");
        fs::create_dir_all(&root).expect("create temp root");
        create_known_ssd_marker(&ssd_root);
        write_store_definitions_file(
            &stores_registry_path,
            vec![StoreServiceDefinition {
                store_id: StoreId::new("ENA").expect("store id"),
                policy: StorePolicy::defaults_for(StoreClass::ReproducibleCache),
                bucket_name: Some("dos-ena".to_string()),
                writer_group: Some(test_writer_group()),
            }],
        );

        for args in [
            vec![
                "dasobjectstore",
                "subobject",
                "create",
                "Xenognostikon",
                "--store",
                "ENA",
                "--registry-path",
                subobject_registry_path
                    .to_str()
                    .expect("utf8 subobject path"),
                "--ssd-root",
                ssd_root.to_str().expect("utf8 ssd root"),
                "--stores-registry-path",
                stores_registry_path.to_str().expect("utf8 store path"),
            ],
            vec![
                "dasobjectstore",
                "subobject",
                "create",
                "Vervet",
                "--parent",
                "Xenognostikon",
                "--registry-path",
                subobject_registry_path
                    .to_str()
                    .expect("utf8 subobject path"),
                "--ssd-root",
                ssd_root.to_str().expect("utf8 ssd root"),
                "--stores-registry-path",
                stores_registry_path.to_str().expect("utf8 store path"),
            ],
        ] {
            let cli = Cli::try_parse_from(args).expect("subobject create parses");
            let mut output = Vec::new();
            run(&cli, &mut output).expect("subobject create runs");
        }
        assert!(
            ssd_root
                .join(".dasobjectstore")
                .join("subobjects.json")
                .is_file(),
            "portable SubObject registry should be mirrored to the SSD"
        );

        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "subobject",
            "search",
            "vervet",
            "--registry-path",
            subobject_registry_path
                .to_str()
                .expect("utf8 subobject path"),
        ])
        .expect("subobject search parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("subobject search runs");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("SubObjects matched: 1"));
        assert!(output.contains("Vervet"));
        assert!(output.contains("prefix=ENA/Xenognostikon/Vervet"));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn local_directory_ingest_skips_hidden_files_and_hidden_directories() {
        let root = temp_root("ingest-files-hidden");
        let source_root = root.join("external");
        fs::create_dir_all(source_root.join("nested")).expect("create source");
        fs::create_dir_all(source_root.join(".partial")).expect("create hidden source dir");
        fs::write(source_root.join("nested").join("sample.fastq.gz"), b"ACGT")
            .expect("write visible source");
        fs::write(source_root.join(".hidden.pod5.tmp"), b"temporary payload")
            .expect("write hidden source");
        fs::write(
            source_root.join(".partial").join("sample.fastq.gz"),
            b"temporary payload",
        )
        .expect("write hidden nested source");

        let files = collect_ingest_files(&source_root, "zymo_fecal_2025.05").expect("files scan");

        assert_eq!(files.len(), 1);
        assert_eq!(
            files[0].relative_path,
            PathBuf::from("nested/sample.fastq.gz")
        );

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn ingest_files_resolves_nested_subobject_endpoint() {
        let root = temp_root("ingest-files-subobject");
        let source_root = root.join("external");
        let ssd_root = root.join("ssd");
        let hdd_base = root.join("hdd");
        let hdd_root = hdd_base.join("disk-a");
        let registry_path = root.join("stores.json");
        let subobject_registry_path = root.join("subobjects.json");
        fs::create_dir_all(source_root.join("nested")).expect("create source");
        create_known_hdd_marker(&hdd_root, "disk-a");
        create_known_ssd_marker(&ssd_root);
        fs::write(
            source_root.join("nested").join("sample.fastq.gz"),
            b"ACGT".repeat(128),
        )
        .expect("write source file");
        write_store_definitions_file(
            &registry_path,
            vec![StoreServiceDefinition {
                store_id: StoreId::new("ENA").expect("store id"),
                policy: StorePolicy::defaults_for(StoreClass::ReproducibleCache),
                bucket_name: Some("dos-ena".to_string()),
                writer_group: Some(test_writer_group()),
            }],
        );
        run(
            &Cli::try_parse_from([
                "dasobjectstore",
                "subobject",
                "create",
                "Xenognostikon",
                "--store",
                "ENA",
                "--registry-path",
                subobject_registry_path
                    .to_str()
                    .expect("utf8 subobject path"),
                "--stores-registry-path",
                registry_path.to_str().expect("utf8 registry path"),
            ])
            .expect("subobject create parses"),
            &mut Vec::new(),
        )
        .expect("top-level subobject create runs");
        run(
            &Cli::try_parse_from([
                "dasobjectstore",
                "subobject",
                "create",
                "Vervet",
                "--parent",
                "Xenognostikon",
                "--registry-path",
                subobject_registry_path
                    .to_str()
                    .expect("utf8 subobject path"),
                "--stores-registry-path",
                registry_path.to_str().expect("utf8 registry path"),
            ])
            .expect("subobject create parses"),
            &mut Vec::new(),
        )
        .expect("nested subobject create runs");
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "ingest",
            "files",
            "Vervet",
            "--source",
            source_root.to_str().expect("utf8 source root"),
            "--local-direct",
            "--ssd-root",
            ssd_root.to_str().expect("utf8 ssd root"),
            "--hdd-root",
            hdd_base.to_str().expect("utf8 hdd root"),
            "--registry-path",
            registry_path.to_str().expect("utf8 registry path"),
            "--subobject-registry-path",
            subobject_registry_path
                .to_str()
                .expect("utf8 subobject path"),
        ])
        .expect("ingest files parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("ingest files runs");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("Endpoint: Vervet"));
        assert!(output.contains("Endpoint kind: subobject"));
        assert!(output.contains("Store: ENA"));
        assert!(output.contains("Object prefix: ENA/Xenognostikon/Vervet"));
        assert!(output.contains(
            "Importing nested/sample.fastq.gz as ENA/Xenognostikon/Vervet/nested/sample.fastq.gz"
        ));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn ingest_queue_writes_json_snapshot() {
        let root = temp_root("ingest-queue");
        fs::create_dir_all(&root).expect("create temp root");
        let live_sqlite_path = root.join("live.sqlite");
        let connection = Connection::open(&live_sqlite_path).expect("open live sqlite");
        connection
            .execute_batch(LIVE_SCHEMA_SQL)
            .expect("schema applies");
        insert_store(&connection);
        insert_ingest_job(&connection, "job-low", "Queued", 0, "2026-01-01T00:00:00Z");
        insert_ingest_job(
            &connection,
            "job-high",
            "Receiving",
            10,
            "2026-01-01T00:00:01Z",
        );
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "ingest",
            "queue",
            "store-a",
            "--live-sqlite-path",
            live_sqlite_path.to_str().expect("utf8 live sqlite path"),
            "--json",
        ])
        .expect("ingest queue parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("ingest queue runs");

        let output: serde_json::Value =
            serde_json::from_slice(&output).expect("queue output is json");
        assert_eq!(output["jobs"][0]["ingest_job_id"], "job-high");
        assert_eq!(output["jobs"][0]["object_type"], "naive");
        assert_eq!(output["jobs"][0]["priority"], 10);
        assert_eq!(output["jobs"][1]["ingest_job_id"], "job-low");

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn ingest_direct_import_writes_verified_reproducible_object() {
        let root = temp_root("ingest-direct-import");
        fs::create_dir_all(&root).expect("create temp root");
        let source_path = root.join("downloads").join("reference.fa.zst");
        let destination_path = root.join("hdd-a").join("objects").join("reference.fa.zst");
        let policy_file = root.join("reproducible-cache.json");
        fs::create_dir_all(source_path.parent().expect("source parent")).expect("source dir");
        fs::write(&source_path, b"public reference payload").expect("write source payload");
        write_policy_file(&policy_file, &direct_reproducible_policy());
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "ingest",
            "direct-import",
            "object-a",
            "--disk-id",
            "disk-a",
            "--source",
            source_path.to_str().expect("utf8 source path"),
            "--object-type",
            "ena_sra",
            "--destination",
            destination_path.to_str().expect("utf8 destination path"),
            "--expected-sha256",
            "c13ac914d37ad9fd216d274f2fbeb0b936ac9275e27ff7003831701ccad71def",
            "--source-uri",
            "https://example.invalid/reference.fa.zst",
            "--policy-file",
            policy_file.to_str().expect("utf8 policy path"),
            "--allow-direct-to-hdd-import",
            "--confirm",
            "confirm direct-to-hdd import",
        ])
        .expect("direct import parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("direct import runs");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("Direct-to-HDD import complete"));
        assert!(output.contains("Object: object-a"));
        assert!(output.contains("Object type: ena_sra"));
        assert!(output.contains("Warning: SSD ingest was bypassed"));
        assert_eq!(
            fs::read(&destination_path).expect("read destination"),
            b"public reference payload"
        );

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn ingest_direct_import_requires_risk_allowance() {
        let root = temp_root("ingest-direct-import-risk");
        fs::create_dir_all(&root).expect("create temp root");
        let source_path = root.join("source");
        let policy_file = root.join("policy.json");
        fs::write(&source_path, b"public reference payload").expect("write source payload");
        write_policy_file(&policy_file, &direct_reproducible_policy());
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "ingest",
            "direct-import",
            "object-a",
            "--disk-id",
            "disk-a",
            "--source",
            source_path.to_str().expect("utf8 source path"),
            "--destination",
            root.join("dest").to_str().expect("utf8 destination path"),
            "--expected-sha256",
            "c13ac914d37ad9fd216d274f2fbeb0b936ac9275e27ff7003831701ccad71def",
            "--policy-file",
            policy_file.to_str().expect("utf8 policy path"),
            "--confirm",
            "confirm direct-to-hdd import",
        ])
        .expect("direct import parses");
        let mut output = Vec::new();

        let err = run(&cli, &mut output).expect_err("risk allowance required");

        assert!(matches!(err, CliError::DirectHddImport(_)));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn ingest_queue_writes_pretty_snapshot_by_default() {
        let root = temp_root("ingest-queue-pretty");
        fs::create_dir_all(&root).expect("create temp root");
        let live_sqlite_path = root.join("live.sqlite");
        let connection = Connection::open(&live_sqlite_path).expect("open live sqlite");
        connection
            .execute_batch(LIVE_SCHEMA_SQL)
            .expect("schema applies");
        insert_store(&connection);
        insert_ingest_job(&connection, "job-low", "Queued", 0, "2026-01-01T00:00:00Z");
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "ingest",
            "queue",
            "store-a",
            "--live-sqlite-path",
            live_sqlite_path.to_str().expect("utf8 live sqlite path"),
        ])
        .expect("ingest queue parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("ingest queue runs");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("Ingest queue"));
        assert!(output.contains("Jobs: 1"));
        assert!(output.contains("job-low"));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn ingest_drain_queue_marks_active_jobs_cancelled() {
        let root = temp_root("ingest-drain-queue");
        fs::create_dir_all(&root).expect("create temp root");
        let live_sqlite_path = root.join("live.sqlite");
        let connection = Connection::open(&live_sqlite_path).expect("open live sqlite");
        connection
            .execute_batch(LIVE_SCHEMA_SQL)
            .expect("schema applies");
        insert_store(&connection);
        insert_ingest_job(
            &connection,
            "job-active",
            "Queued",
            0,
            "2026-01-01T00:00:00Z",
        );
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "ingest",
            "drain-queue",
            "store-a",
            "--live-sqlite-path",
            live_sqlite_path.to_str().expect("utf8 live sqlite path"),
            "--dry-run",
            "--allow-ingest-queue-drain",
            "--confirm",
            "confirm ingest queue drain",
        ])
        .expect("ingest drain-queue parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("ingest queue drains");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("Ingest queue drain"));
        assert!(output.contains("Jobs would cancel: 1"));
        assert_eq!(ingest_job_state(&live_sqlite_path, "job-active"), "Queued");

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn object_inspect_writes_pretty_summary() {
        let root = temp_root("object-inspect-pretty");
        let live_sqlite_path = create_live_sqlite_with_object(&root);
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "object",
            "inspect",
            "object-a",
            "--live-sqlite-path",
            live_sqlite_path.to_str().expect("utf8 live sqlite path"),
        ])
        .expect("object inspect parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("object inspect runs");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("Object: object-a"));
        assert!(output.contains("Store class: generated_data"));
        assert!(output.contains("Object type: naive"));
        assert!(output.contains("Placements: 1"));
        assert!(output.contains("- placement-a disk=disk-a path=objects/aa/object-a"));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn object_inspect_writes_json_summary() {
        let root = temp_root("object-inspect-json");
        let live_sqlite_path = create_live_sqlite_with_object(&root);
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "object",
            "inspect",
            "object-a",
            "--live-sqlite-path",
            live_sqlite_path.to_str().expect("utf8 live sqlite path"),
            "--json",
        ])
        .expect("object inspect parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("object inspect runs");

        let output: serde_json::Value =
            serde_json::from_slice(&output).expect("object output is json");
        assert_eq!(output["object_id"], "object-a");
        assert_eq!(output["store_id"], "store-a");
        assert_eq!(output["object_type"], "naive");
        assert_eq!(output["placements"][0]["disk_id"], "disk-a");

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn object_export_writes_settled_payload() {
        let root = temp_root("object-export");
        let disk_root = root.join("disk-a");
        let source_path = disk_root.join("objects").join("aa").join("object-a");
        let destination_path = root.join("exports").join("object-a");
        fs::create_dir_all(source_path.parent().expect("source parent"))
            .expect("create source parent");
        fs::write(&source_path, b"settled payload").expect("write settled payload");
        let live_sqlite_path = create_live_sqlite_with_exportable_object(&root);
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "object",
            "export",
            "object-a",
            "--live-sqlite-path",
            live_sqlite_path.to_str().expect("utf8 live sqlite path"),
            "--destination",
            destination_path.to_str().expect("utf8 destination path"),
            "--disk-root",
            &format!("disk-a={}", disk_root.to_string_lossy()),
        ])
        .expect("object export parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("object export runs");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("Object: object-a"));
        assert!(output.contains("Source disk: disk-a"));
        assert!(output.contains("Bytes written: 15"));
        assert_eq!(
            fs::read(&destination_path).expect("read exported payload"),
            b"settled payload"
        );

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn object_put_stages_and_settles_verified_copies() {
        let root = temp_root("object-put");
        let source_path = root.join("source.fastq.gz");
        let ssd_root = root.join("ssd");
        let disk_a = root.join("disk-a");
        let disk_b = root.join("disk-b");
        fs::create_dir_all(&root).expect("create temp root");
        fs::write(&source_path, b"settle this payload").expect("write source");
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "object",
            "put",
            "object-a",
            "--source",
            source_path.to_str().expect("utf8 source path"),
            "--object-type",
            "fastq",
            "--ssd-root",
            ssd_root.to_str().expect("utf8 ssd path"),
            "--disk-root",
            &format!("disk-a={}", disk_a.to_string_lossy()),
            "--disk-root",
            &format!("disk-b={}", disk_b.to_string_lossy()),
            "--copies",
            "2",
        ])
        .expect("object put parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("object put runs");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("Object put complete"));
        assert!(output.contains("Object: object-a"));
        assert!(output.contains("Object type: fastq"));
        assert!(output.contains("Settled copies: 2"));
        assert!(output.contains("disk=disk-a"));
        assert!(output.contains("disk=disk-b"));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn service_render_compose_writes_store_aware_yaml() {
        let root = temp_root("service-render-compose");
        fs::create_dir_all(&root).expect("create temp root");
        let stores_file = root.join("stores.json");
        write_store_definitions_file(
            &stores_file,
            vec![StoreServiceDefinition {
                store_id: StoreId::new("generated").expect("store id"),
                policy: StorePolicy::defaults_for(StoreClass::GeneratedData),
                bucket_name: None,
                writer_group: Some(test_writer_group()),
            }],
        );
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "service",
            "render-compose",
            "--stores-file",
            stores_file.to_str().expect("utf8 stores path"),
            "--project-name",
            "dasobjectstore-dev",
            "--ssd-metadata-path",
            "/ssd/meta",
            "--hdd-data-path",
            "/hdd/data",
            "--provider",
            "garage",
            "--service-name",
            "garage",
            "--image",
            "garage:latest",
            "--api-port",
            "3900",
        ])
        .expect("service render-compose parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("service render-compose runs");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("name: dasobjectstore-dev"));
        assert!(output.contains("image: garage:latest"));
        assert!(output.contains("DASOBJECTSTORE_PROVIDER: garage"));
        assert!(output.contains("DASOBJECTSTORE_BUCKETS: dos-generated"));
        assert!(output.contains("/etc/dasobjectstore/garage.toml:/etc/garage.toml:ro"));
        assert!(output.contains("command: [\"/garage\", \"server\", \"--single-node\""));
        assert!(
            output.contains("credential_reference: secret://dasobjectstore/stores/generated/s3")
        );

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn mnemosyne_export_writes_storage_definition_and_binding_json() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "mnemosyne",
            "export",
            "--object-store-id",
            "4f0a1ba7-9f00-422b-bf18-87567b076daa",
            "--display-name",
            "DASObjectStore Development",
            "--provider",
            "garage",
            "--endpoint",
            "http://127.0.0.1:3900",
            "--governance-domain-id",
            "22222222-2222-2222-2222-222222222222",
            "--note",
            "DASObjectStore development store",
        ])
        .expect("mnemosyne export parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("mnemosyne export runs");

        let output: serde_json::Value =
            serde_json::from_slice(&output).expect("mnemosyne output is json");
        assert_eq!(
            output["storage_definition"]["object_store_create_request"]["backend_kind"],
            "S3-Compatible"
        );
        assert_eq!(
            output["storage_definition"]["object_store_create_request"]["endpoint"],
            "http://127.0.0.1:3900"
        );
        assert_eq!(
            output["binding_snippet"]["endpoint_path"],
            "/api/v1/admin/object-stores/4f0a1ba7-9f00-422b-bf18-87567b076daa/link"
        );
        assert_eq!(
            output["binding_snippet"]["object_store_link_request"]["governance_domain_id"],
            "22222222-2222-2222-2222-222222222222"
        );
    }

    #[test]
    fn mnemosyne_validate_nas_nfs_endpoint_writes_pretty_summary() {
        let root = temp_root("mnemosyne-validate-nas-nfs");
        fs::create_dir_all(&root).expect("create temp root");
        let definition_file = root.join("nas-endpoint.json");
        fs::write(
            &definition_file,
            serde_json::to_vec_pretty(&valid_nas_nfs_endpoint_definition())
                .expect("endpoint definition serializes"),
        )
        .expect("write endpoint definition");
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "mnemosyne",
            "validate-nas-nfs-endpoint",
            "--definition-file",
            definition_file.to_str().expect("utf8 definition path"),
        ])
        .expect("NAS/NFS endpoint validation parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("NAS/NFS endpoint validation runs");

        let output = String::from_utf8(output).expect("utf8 output");
        assert!(output.contains("NAS/NFS endpoint definition is valid"));
        assert!(output.contains("Endpoint: ad255a8f-0058-4790-a640-758c573f2db1"));
        assert!(output.contains("Mneion endpoint kind: DasobjectstoreNfs"));
        assert!(output.contains("Tenant-facing contract: ObjectStyle"));
        assert!(!output.contains("/exports/bioinformatics"));
        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn mnemosyne_validate_nas_nfs_endpoint_writes_json() {
        let root = temp_root("mnemosyne-validate-nas-nfs-json");
        fs::create_dir_all(&root).expect("create temp root");
        let definition_file = root.join("nas-endpoint.json");
        fs::write(
            &definition_file,
            serde_json::to_vec_pretty(&valid_nas_nfs_endpoint_definition())
                .expect("endpoint definition serializes"),
        )
        .expect("write endpoint definition");
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "mnemosyne",
            "validate-nas-nfs-endpoint",
            "--definition-file",
            definition_file.to_str().expect("utf8 definition path"),
            "--json",
        ])
        .expect("NAS/NFS endpoint validation parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("NAS/NFS endpoint validation runs");

        let output: serde_json::Value =
            serde_json::from_slice(&output).expect("validation output is json");
        assert_eq!(
            output["definition"]["identifier"],
            "ad255a8f-0058-4790-a640-758c573f2db1"
        );
        assert_eq!(
            output["mneion_endpoint"]["endpoint_kind"],
            "dasobjectstore_nfs"
        );
        assert_eq!(
            output["mneion_endpoint"]["location"]["location_kind"],
            "nfs"
        );
        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn mnemosyne_validate_nas_nfs_endpoint_rejects_invalid_definition() {
        let root = temp_root("mnemosyne-validate-nas-nfs-invalid");
        fs::create_dir_all(&root).expect("create temp root");
        let mut definition = valid_nas_nfs_endpoint_definition();
        definition["nfs_export_path"] = serde_json::json!("relative/path");
        let definition_file = root.join("nas-endpoint.json");
        fs::write(
            &definition_file,
            serde_json::to_vec_pretty(&definition).expect("endpoint definition serializes"),
        )
        .expect("write endpoint definition");
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "mnemosyne",
            "validate-nas-nfs-endpoint",
            "--definition-file",
            definition_file.to_str().expect("utf8 definition path"),
        ])
        .expect("NAS/NFS endpoint validation parses");
        let mut output = Vec::new();

        let err = run(&cli, &mut output).expect_err("invalid endpoint is rejected");

        assert!(err
            .to_string()
            .contains("nfs_export_path must be an absolute export path"));
        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn service_up_dry_run_writes_docker_compose_command() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "service",
            "up",
            "--compose-file",
            "/tmp/compose.yaml",
            "--project-directory",
            "/tmp/project",
            "--dry-run",
        ])
        .expect("service up parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("service up dry run succeeds");

        let output = String::from_utf8(output).expect("utf8 output");
        assert_eq!(
            output,
            "docker compose -f /tmp/compose.yaml --project-directory /tmp/project up -d\n"
        );
    }

    #[test]
    fn service_down_dry_run_writes_docker_compose_command() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "service",
            "down",
            "--compose-file",
            "/tmp/compose.yaml",
            "--dry-run",
        ])
        .expect("service down parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("service down dry run succeeds");

        let output = String::from_utf8(output).expect("utf8 output");
        assert_eq!(output, "docker compose -f /tmp/compose.yaml down\n");
    }

    #[test]
    fn service_status_json_dry_run_writes_command_json() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "service",
            "status",
            "--compose-file",
            "/tmp/compose.yaml",
            "--project-directory",
            "/tmp/project",
            "--json",
            "--dry-run",
        ])
        .expect("service status parses");
        let mut output = Vec::new();

        run(&cli, &mut output).expect("service status dry run succeeds");

        let output: serde_json::Value =
            serde_json::from_slice(&output).expect("status output is json");
        assert_eq!(output["dry_run"], true);
        assert_eq!(
            output["command"],
            serde_json::json!([
                "docker",
                "compose",
                "-f",
                "/tmp/compose.yaml",
                "--project-directory",
                "/tmp/project",
                "ps",
                "--format",
                "json"
            ])
        );
    }

    #[test]
    fn service_status_requires_json_flag() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "service",
            "status",
            "--compose-file",
            "/tmp/compose.yaml",
        ])
        .expect("service status parses");
        let mut output = Vec::new();

        let err = run(&cli, &mut output).expect_err("json flag required");

        assert!(matches!(err, CliError::UnsupportedServiceStatusFormat));
    }

    #[cfg(feature = "debug-commands")]
    #[test]
    fn pool_debug_marker_commands_update_live_metadata() {
        let root = temp_root("pool-debug-markers");
        let init = initialize_pool(&PoolInitOptions::new(
            &root,
            PoolId::new("pool-a").expect("pool id"),
            "2026-01-02T00:00:00Z",
        ))
        .expect("pool initializes");
        let live_sqlite_path = init
            .live_sqlite_path
            .to_str()
            .expect("utf8 live sqlite path");

        run_marker_command("mark-dirty", live_sqlite_path, "2026-01-03T00:00:00Z")
            .expect("mark dirty runs");
        assert_eq!(pool_state(&init.live_sqlite_path), "Dirty");

        let output = run_marker_command("mark-clean", live_sqlite_path, "2026-01-04T00:00:00Z")
            .expect("mark clean runs");
        assert!(output.contains("Marked pool pool-a clean"));
        assert_eq!(pool_state(&init.live_sqlite_path), "Clean");

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[cfg(feature = "debug-commands")]
    fn run_marker_command(
        command: &str,
        live_sqlite_path: &str,
        recorded_at_utc: &str,
    ) -> Result<String, CliError> {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "pool",
            command,
            "--live-sqlite-path",
            live_sqlite_path,
            "--pool-id",
            "pool-a",
            "--recorded-at-utc",
            recorded_at_utc,
        ])
        .expect("marker command parses");
        let mut output = Vec::new();

        run(&cli, &mut output)?;

        Ok(String::from_utf8(output).expect("utf8 output"))
    }

    fn pool_state(live_sqlite_path: &Path) -> String {
        let connection = Connection::open(live_sqlite_path).expect("open live sqlite");

        connection
            .query_row(
                "SELECT state FROM pools WHERE pool_id = 'pool-a'",
                [],
                |row| row.get(0),
            )
            .expect("pool state")
    }

    fn create_portable_pool_snapshot(ssd_root: &Path, source_root: &Path, state: &str) {
        let init = initialize_pool(&PoolInitOptions::new(
            ssd_root,
            PoolId::new("pool-a").expect("pool id"),
            "2026-01-02T00:00:00Z",
        ))
        .expect("pool initializes");
        let connection = Connection::open(&init.live_sqlite_path).expect("open live sqlite");
        connection
            .execute(
                "UPDATE pools SET state = ?1, updated_at_utc = ?2 WHERE pool_id = ?3",
                (state, "2026-01-03T00:00:00Z", "pool-a"),
            )
            .expect("pool state updates");
        insert_disk(&connection, "disk-a", "Healthy");
        export_metadata_snapshot(&SnapshotExportOptions::new(
            &init.live_sqlite_path,
            vec![source_root.join(".dasobjectstore").join("metadata")],
            "2026-01-03T00:00:00Z",
        ))
        .expect("snapshot exports");
    }

    fn write_snapshot_manifests(path: &Path) {
        fs::create_dir_all(path).expect("create metadata dir");
        let pool_id = PoolId::new("pool-a").expect("pool id");
        let pool_manifest = PoolManifest::new(
            pool_id.clone(),
            PoolState::Clean,
            "2026-01-02T00:00:00Z",
            "2026-01-03T00:00:00Z",
            ArtifactReference::new(
                MetadataArtifact::DiskManifest,
                FormatVersion::new(MetadataArtifact::DiskManifest, 0, 1),
                DISK_MANIFEST_FILE_NAME,
                None,
            ),
            ArtifactReference::new(
                MetadataArtifact::PlacementLog,
                FormatVersion::new(MetadataArtifact::PlacementLog, 0, 1),
                PLACEMENT_LOG_FILE_NAME,
                None,
            ),
        );
        let disk_manifest = DiskManifest::new(
            pool_id,
            "2026-01-03T00:00:00Z",
            vec![DiskManifestEntry::new(
                DiskId::new("disk-a").expect("disk id"),
                DiskState::Healthy,
                DiskRole::HddCapacity,
                "2026-01-02T00:00:00Z",
                "2026-01-03T00:00:00Z",
            )],
        );

        let file = File::create(path.join(POOL_MANIFEST_FILE_NAME)).expect("create pool manifest");
        serde_json::to_writer_pretty(file, &pool_manifest).expect("write pool manifest");
        let file = File::create(path.join(DISK_MANIFEST_FILE_NAME)).expect("create disk manifest");
        serde_json::to_writer_pretty(file, &disk_manifest).expect("write disk manifest");
    }

    fn write_policy_file(path: &Path, policy: &StorePolicy) {
        let file = File::create(path).expect("create policy file");
        serde_json::to_writer_pretty(file, policy).expect("write policy file");
    }

    fn direct_reproducible_policy() -> StorePolicy {
        let mut policy = StorePolicy::defaults_for(StoreClass::ReproducibleCache);
        policy.ingest_mode = IngestMode::DirectToHdd;
        policy
    }

    fn write_store_definitions_file(path: &Path, definitions: Vec<StoreServiceDefinition>) {
        let file = File::create(path).expect("create store definitions file");
        serde_json::to_writer_pretty(file, &definitions).expect("write store definitions file");
    }

    fn insert_store(connection: &Connection) {
        let policy = serde_json::to_string(&StorePolicy::defaults_for(StoreClass::GeneratedData))
            .expect("policy serializes");
        connection
            .execute(
                "INSERT INTO pools (pool_id, state, created_at_utc, updated_at_utc)
                 VALUES ('pool-a', 'Clean', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )
            .expect("pool inserts");
        connection
            .execute(
                "INSERT INTO stores (
                    store_id,
                    pool_id,
                    class,
                    policy_json,
                    created_at_utc,
                    updated_at_utc
                 ) VALUES (
                    'store-a',
                    'pool-a',
                    'generated_data',
                    ?1,
                    '2026-01-01T00:00:00Z',
                    '2026-01-01T00:00:00Z'
                 )",
                [policy],
            )
            .expect("store inserts");
    }

    fn insert_ingest_job(
        connection: &Connection,
        ingest_job_id: &str,
        state: &str,
        priority: i32,
        created_at_utc: &str,
    ) {
        connection
            .execute(
                "INSERT INTO ingest_jobs (
                    ingest_job_id,
                    store_id,
                    state,
                    ingest_mode,
                    acknowledgement_policy,
                    priority,
                    staging_path,
                    received_bytes,
                    created_at_utc,
                    updated_at_utc
                 ) VALUES (?1, 'store-a', ?2, 'SsdFirst', 'AfterHddPlacement', ?3, ?4, 0, ?5, ?5)",
                (
                    ingest_job_id,
                    state,
                    priority,
                    format!("/ssd/.dasobjectstore/ingest/jobs/{ingest_job_id}"),
                    created_at_utc,
                ),
            )
            .expect("ingest job inserts");
    }

    fn ingest_job_state(live_sqlite_path: &Path, ingest_job_id: &str) -> String {
        let connection = Connection::open(live_sqlite_path).expect("open live sqlite");
        connection
            .query_row(
                "SELECT state FROM ingest_jobs WHERE ingest_job_id = ?1",
                [ingest_job_id],
                |row| row.get(0),
            )
            .expect("ingest job state")
    }

    fn insert_disk(connection: &Connection, disk_id: &str, state: &str) {
        connection
            .execute(
                "INSERT INTO disks (
                    disk_id, pool_id, role, state, created_at_utc, updated_at_utc
                 ) VALUES (?1, 'pool-a', 'hdd_capacity', ?2, ?3, ?3)",
                (disk_id, state, "2026-01-01T00:00:00Z"),
            )
            .expect("disk inserts");
    }

    fn disk_state(connection: &Connection, disk_id: &str) -> String {
        connection
            .query_row(
                "SELECT state FROM disks WHERE disk_id = ?1",
                [disk_id],
                |row| row.get(0),
            )
            .expect("disk state")
    }

    fn create_live_sqlite_with_object(root: &Path) -> PathBuf {
        fs::create_dir_all(root).expect("create temp root");
        let live_sqlite_path = root.join("live.sqlite");
        let connection = Connection::open(&live_sqlite_path).expect("open live sqlite");
        connection
            .execute_batch(LIVE_SCHEMA_SQL)
            .expect("schema applies");
        insert_store(&connection);
        insert_disk(&connection, "disk-a", "Healthy");
        connection
            .execute_batch(
                "INSERT INTO objects (
                    object_id, store_id, state, size_bytes, content_hash,
                    created_at_utc, updated_at_utc
                 ) VALUES (
                    'object-a', 'store-a', 'Protected', 128, 'sha256:object-a',
                    '2026-01-02T00:00:00Z', '2026-01-03T00:00:00Z'
                 );
                 INSERT INTO placements (
                    placement_id, object_id, disk_id, relative_path, content_hash,
                    verified_at_utc, created_at_utc
                 ) VALUES (
                    'placement-a', 'object-a', 'disk-a', 'objects/aa/object-a',
                    'sha256:object-a', '2026-01-03T00:00:00Z',
                    '2026-01-02T00:00:00Z'
                 );",
            )
            .expect("object fixture inserts");

        live_sqlite_path
    }

    fn create_live_sqlite_with_store_contents(root: &Path) -> PathBuf {
        fs::create_dir_all(root).expect("create temp root");
        let live_sqlite_path = root.join("live.sqlite");
        let connection = Connection::open(&live_sqlite_path).expect("open live sqlite");
        connection
            .execute_batch(LIVE_SCHEMA_SQL)
            .expect("schema applies");
        insert_store(&connection);
        for (object_id, size_bytes, object_type) in [
            ("store-a/raw/PAW10254/sample.pod5", 128_i64, "pod5"),
            ("store-a/raw/PAW10254/sample.fastq.gz", 64_i64, "fastq"),
            ("store-a/notes.txt", 16_i64, "naive"),
        ] {
            connection
                .execute(
                    "INSERT INTO objects (
                        object_id, store_id, object_type, state, size_bytes, content_hash,
                        created_at_utc, updated_at_utc
                     ) VALUES (?1, 'store-a', ?2, 'SsdEvictionEligible', ?3, ?4, ?5, ?5)",
                    (
                        object_id,
                        object_type,
                        size_bytes,
                        format!("sha256:{object_id}"),
                        "2026-01-02T00:00:00Z",
                    ),
                )
                .expect("object inserts");
        }

        live_sqlite_path
    }

    fn create_live_sqlite_with_exportable_object(root: &Path) -> PathBuf {
        const SETTLED_PAYLOAD_SHA256: &str =
            "ab81c35abe1f9101fb40fd79aa397af816519eb5a3fe1fe0fd923f8e5d153a67";

        fs::create_dir_all(root).expect("create temp root");
        let live_sqlite_path = root.join("live.sqlite");
        let connection = Connection::open(&live_sqlite_path).expect("open live sqlite");
        connection
            .execute_batch(LIVE_SCHEMA_SQL)
            .expect("schema applies");
        insert_store(&connection);
        insert_disk(&connection, "disk-a", "Healthy");
        connection
            .execute(
                "INSERT INTO objects (
                    object_id, store_id, state, size_bytes, content_hash,
                    created_at_utc, updated_at_utc
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    "object-a",
                    "store-a",
                    "SsdEvictionEligible",
                    15_i64,
                    SETTLED_PAYLOAD_SHA256,
                    "2026-01-02T00:00:00Z",
                    "2026-01-03T00:00:00Z"
                ],
            )
            .expect("object fixture inserts");
        connection
            .execute(
                "INSERT INTO placements (
                    placement_id, object_id, disk_id, relative_path, content_hash,
                    verified_at_utc, created_at_utc
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    "placement-a",
                    "object-a",
                    "disk-a",
                    "objects/aa/object-a",
                    SETTLED_PAYLOAD_SHA256,
                    "2026-01-03T00:00:00Z",
                    "2026-01-02T00:00:00Z"
                ],
            )
            .expect("placement fixture inserts");

        live_sqlite_path
    }

    fn health_report_fixture() -> HealthReport {
        let signals = HealthSignals {
            smart_warnings: 1,
            ..HealthSignals::default()
        };
        let score = HealthScore::from_signals(&signals);

        HealthReport {
            platform: HostPlatform::Macos,
            disks: vec![DiskHealthSummary {
                device_path: Some("/dev/disk4".to_string()),
                model_hint: Some("Old SATA HDD".to_string()),
                serial_hint: Some("WD-OLD-001".to_string()),
                size_bytes: Some(1_000),
                transport: Transport::Usb,
                smart_passed: Some(false),
                signals,
                score,
                warnings: vec!["macOS reports SMART failure".to_string()],
            }],
            warnings: vec!["probe warning".to_string()],
        }
    }

    fn temp_root(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dasobjectstore-{name}-{}-{nanos}",
            std::process::id()
        ))
    }

    #[cfg(target_os = "linux")]
    fn set_mode(path: impl AsRef<Path>, mode: u32) {
        let mut permissions = fs::metadata(path.as_ref())
            .expect("metadata for chmod fixture")
            .permissions();
        permissions.set_mode(mode);
        fs::set_permissions(path, permissions).expect("set chmod fixture mode");
    }

    fn create_known_ssd_marker(ssd_root: &Path) {
        let marker_dir = ssd_root.join(".dasobjectstore");
        fs::create_dir_all(&marker_dir).expect("create SSD marker directory");
        fs::write(
            marker_dir.join("device.env"),
            "role=ssd\ndevice=/dev/disk/by-id/test-ssd\nfilesystem=ext4\n",
        )
        .expect("write SSD marker");
    }

    fn create_known_hdd_marker(hdd_root: &Path, disk_id: &str) {
        let marker_dir = hdd_root.join(".dasobjectstore");
        fs::create_dir_all(&marker_dir).expect("create HDD marker directory");
        fs::write(
            marker_dir.join("device.env"),
            format!("role=hdd:{disk_id}\ndevice=/dev/disk/by-id/test-{disk_id}\nfilesystem=ext4\n"),
        )
        .expect("write HDD marker");
    }

    fn test_writer_group() -> String {
        current_user_group_names()
            .expect("current user groups available")
            .into_iter()
            .next()
            .unwrap_or_else(|| "staff".to_string())
    }

    fn valid_nas_nfs_endpoint_definition() -> serde_json::Value {
        serde_json::json!({
            "schema_version": NAS_NFS_ENDPOINT_DEFINITION_SCHEMA_VERSION,
            "identifier": "ad255a8f-0058-4790-a640-758c573f2db1",
            "display_name": "Shared NAS",
            "nfs_server": "nas-01.local",
            "nfs_export_path": "/exports/bioinformatics",
            "object_service_endpoint": "https://nas-gateway.local:3900",
            "credential_reference": "secret://dasobjectstore/nas/shared",
            "tls_ca_reference": "secret://dasobjectstore/ca/nas",
            "tls_server_name": "nas-gateway.local",
            "status": "pending_validation"
        })
    }
}
