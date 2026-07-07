#[cfg(feature = "debug-commands")]
use crate::cli::PoolMarkerArgs;
use crate::cli::{
    Cli, Command, DiskCommand, DiskDrainArgs, DiskForceRetireArgs, DiskLockdownDasArgs,
    DiskPrepareDasArgs, DiskPrepareFilesystem, DiskReplaceArgs, DiskRetireArgs, HealthArgs,
    IngestCommand, IngestDirectImportArgs, IngestFilesArgs, IngestQueueArgs, IngestStatusArgs,
    MnemosyneCommand, MnemosyneExportArgs, MnemosyneValidateNasNfsEndpointArgs, ObjectCommand,
    ObjectExportArgs, ObjectInspectArgs, ObjectPutArgs, PoolCommand, PoolImportArgs,
    PoolInspectArgs, PoolRepairArgs, ProbeArgs, ServiceCommand, ServiceComposeArgs,
    ServiceRenderComposeArgs, ServiceStatusArgs, StoreAdoptArgs, StoreCommand, StoreCreateArgs,
    StoreDefaultsArgs, StoreListArgs, StoreValidateArgs, SubobjectArgs, SubobjectCommand,
    SubobjectCreateArgs, SubobjectListArgs, SubobjectSearchArgs,
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
    write_prepare_das_report, write_pretty_report, write_store_create_report,
    write_store_list_report,
};
use dasobjectstore_core::health::{HealthScore, HealthSignals};
use dasobjectstore_core::ids::{DiskId, ObjectId, StoreId};
use dasobjectstore_core::lifecycle::PoolState;
use dasobjectstore_core::risk::{
    ActionConfirmation, RiskGate, RiskGateError, RiskPolicy, RiskyOperation,
};
use dasobjectstore_core::store::{StorePolicy, StorePolicyValidationErrors};
use dasobjectstore_metadata::{
    attach_clean_pool_read_only, export_settled_object, force_retire_disk,
    import_dirty_pool_read_only, import_reproducible_object_direct_to_hdd, inspect_pool_metadata,
    measure_ssd_capacity, put_object_ssd_first, put_object_ssd_first_with_progress,
    read_disk_drain_plan, read_disk_replacement_plan, read_ingest_queue, read_object_inspect,
    request_disk_retirement, DestagePriorityPolicy, DirectHddImportError, DirectHddImportRequest,
    DiskCopyRoot, DiskDrainError, DiskRetirementError, IngestQueueReadError, ObjectExportError,
    ObjectExportRequest, ObjectInspectError, ObjectPutError, ObjectPutProgress,
    ObjectPutProgressStage, ObjectPutRequest, PoolInspectError, ReadOnlyAttachError,
    ReadOnlyAttachOptions, SsdCapacityMeasurementError, SsdCapacityPolicy, SsdCapacityPolicyError,
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
    create_subobject_definition, default_store_registry_path, default_subobject_registry_path,
    mirror_subobject_definition, plan_store_service_layout, portable_store_registry_path,
    portable_subobject_registry_path, read_store_registry, read_subobject_registry, render_compose,
    search_subobjects, upsert_store_definition, ComposeRenderRequest, ComposeServiceConfig,
    ObjectServiceError, StoreRegistryUpdateReport, StoreServiceDefinition, SubObjectDefinition,
    SubObjectParent, SubObjectRegistryUpdateReport,
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
use std::fmt::{self, Display};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::time::Instant;

pub(crate) fn run(cli: &Cli, writer: &mut impl Write) -> Result<(), CliError> {
    match cli.command() {
        Some(Command::Probe(args)) => run_probe(args, writer),
        Some(Command::Health(args)) => run_health(args, writer),
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
            Some(StoreCommand::Create(args)) => run_store_create(args, writer),
            Some(StoreCommand::Defaults(args)) => run_store_defaults(args, writer),
            Some(StoreCommand::List(args)) => run_store_list(args, writer),
            Some(StoreCommand::Validate(args)) => run_store_validate(args, writer),
            None => Cli::write_subcommand_help("store", writer).map_err(CliError::Io),
        },
        Some(Command::Ingest(args)) => match args.command() {
            Some(IngestCommand::Files(args)) => run_ingest_files(args, writer),
            Some(IngestCommand::Status(args)) => run_ingest_status(args, writer),
            Some(IngestCommand::Queue(args)) => run_ingest_queue(args, writer),
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
        None => Cli::write_help(writer).map_err(CliError::Io),
    }
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
    if args.json() == args.pretty() {
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

    let definition = StoreServiceDefinition {
        store_id: args.store_id().clone(),
        policy,
        bucket_name: args.bucket().map(ToOwned::to_owned),
    };
    let registry_path = args
        .registry_path()
        .map(Path::to_path_buf)
        .unwrap_or_else(default_store_registry_path);
    let report = upsert_store_definition(&registry_path, definition)?;
    let portable_report = upsert_portable_store_definition(args.ssd_root(), &report.definition)?;

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

fn upsert_portable_store_definition(
    ssd_root: Option<&Path>,
    definition: &StoreServiceDefinition,
) -> Result<Option<StoreRegistryUpdateReport>, CliError> {
    let Some(ssd_root) = known_ssd_root_for_optional_mirror(ssd_root)? else {
        return Ok(None);
    };
    let registry_path = portable_store_registry_path(&ssd_root);
    let report = upsert_store_definition(&registry_path, definition.clone())?;

    Ok(Some(report))
}

fn known_ssd_root_for_optional_mirror(
    ssd_root: Option<&Path>,
) -> Result<Option<PathBuf>, CliError> {
    match ssd_root {
        Some(path) => {
            validate_known_ssd_root(path)?;
            Ok(Some(path.to_path_buf()))
        }
        None => {
            let path = default_ssd_root();
            if is_known_ssd_root(&path) {
                Ok(Some(path))
            } else {
                Ok(None)
            }
        }
    }
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
    let portable_report =
        mirror_portable_subobject_definition(args.ssd_root(), &report.definition)?;

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
    definition: &SubObjectDefinition,
) -> Result<Option<SubObjectRegistryUpdateReport>, CliError> {
    let Some(ssd_root) = known_ssd_root_for_optional_mirror(ssd_root)? else {
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

fn run_ingest_files(args: &IngestFilesArgs, writer: &mut impl Write) -> Result<(), CliError> {
    let ssd_root = args
        .ssd_root()
        .map(Path::to_path_buf)
        .unwrap_or_else(default_ssd_root);
    validate_known_ssd_root(&ssd_root)?;
    let disk_roots = parse_disk_roots(args.disk_roots())?;
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
    let copies = args.copies().unwrap_or(endpoint.store.policy.copies);
    if copies == 0 || disk_roots.len() < copies as usize {
        return Err(CliError::CommandFailed(format!(
            "ingest files requires at least {copies} disk root mapping(s), got {}",
            disk_roots.len()
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
    writeln!(writer, "SSD root: {}", ssd_root.to_string_lossy())?;
    writeln!(writer, "Files: {}", files.len())?;
    writeln!(writer, "Source bytes: {total_source_bytes}")?;
    writeln!(writer, "Copies: {copies}")?;
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
            disk_roots.clone(),
            copies,
        );
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
        ObjectPutProgressStage::SsdIngest => "ssd-ingest".to_string(),
        ObjectPutProgressStage::HddCopy {
            disk_id,
            copy_number,
        } => format!("hdd-copy-{disk_id}-{copy_number}"),
    }
}

fn progress_stage_label(progress: &ObjectPutProgress) -> String {
    match &progress.stage {
        ObjectPutProgressStage::SsdIngest => "ssd-ingest".to_string(),
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
    if !args.json() {
        return Err(CliError::UnsupportedIngestQueueFormat);
    }

    let snapshot = read_ingest_queue(args.live_sqlite_path())?;
    serde_json::to_writer_pretty(&mut *writer, &snapshot)?;
    writer.write_all(b"\n")?;

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
    );
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
    let service = ComposeServiceConfig::new(
        args.provider(),
        args.service_name(),
        args.image(),
        args.api_port(),
    );
    let rendered = render_compose(&request, &service)?;

    writer.write_all(rendered.compose_yaml.as_bytes())?;

    Ok(())
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
    MetadataInspect(PoolInspectError),
    PoolImport(ReadOnlyAttachError),
    DiskDrain(DiskDrainError),
    DiskLockdown(LockdownDasError),
    DiskPrepare(PrepareDasError),
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
    UnsupportedIngestQueueFormat,
    UnsupportedPoolImportMode,
    UnsupportedPoolImportState {
        state: PoolState,
    },
    UnsupportedPoolRepairMode,
    UnsupportedProbeFormat,
    UnsupportedServiceStatusFormat,
}

impl Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(formatter, "failed to access command input or output: {err}"),
            Self::Json(err) => write!(formatter, "failed to process JSON: {err}"),
            Self::IngestQueueRead(err) => write!(formatter, "{err}"),
            Self::MetadataInspect(err) => write!(formatter, "{err}"),
            Self::PoolImport(err) => write!(formatter, "{err}"),
            Self::DiskDrain(err) => write!(formatter, "{err}"),
            Self::DiskLockdown(err) => write!(formatter, "{err}"),
            Self::DiskPrepare(err) => write!(formatter, "{err}"),
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
            Self::UnsupportedIngestQueueFormat => {
                formatter.write_str("ingest queue requires JSON output; use `--json`")
            }
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
            Self::UnsupportedProbeFormat => formatter
                .write_str("probe requires exactly one output format; use `--json` or `--pretty`"),
            Self::UnsupportedServiceStatusFormat => {
                formatter.write_str("service status requires JSON output; use `--json`")
            }
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
        connection_status_from_probe, run, write_health_json, write_health_summary,
        write_health_verbose, write_host_connection_status, write_pretty_report, CliError,
        ConnectionAssessment, DiskHealthSummary, HealthReport,
    };
    use crate::cli::Cli;
    use clap::Parser;
    use dasobjectstore_core::health::{HealthScore, HealthSignals};
    use dasobjectstore_core::ids::{DiskId, PoolId, StoreId};
    use dasobjectstore_core::lifecycle::{DiskState, PoolState};
    use dasobjectstore_core::store::{
        CapacityBehavior, IngestMode, StoreClass, StorePolicy, StorePolicyValidationError,
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
    use std::fs::{self, File};
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

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
    fn probe_without_format_returns_clear_error() {
        let cli = Cli::try_parse_from(["dasobjectstore", "probe"]).expect("probe parses");
        let mut output = Vec::new();

        let err = run(&cli, &mut output).expect_err("format is required");

        assert!(matches!(err, CliError::UnsupportedProbeFormat));
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
        assert!(output.contains("- topology=usb@001/002 disks=/dev/disk4"));
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
        let hdd_root = root.join("hdd-a");
        let registry_path = root.join("stores.json");
        let subobject_registry_path = root.join("subobjects.json");
        fs::create_dir_all(source_root.join("nested")).expect("create source");
        fs::create_dir_all(&hdd_root).expect("create hdd root");
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
            }],
        );
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "ingest",
            "files",
            "zymo_fecal_2025.05",
            "--source",
            source_root.to_str().expect("utf8 source root"),
            "--ssd-root",
            ssd_root.to_str().expect("utf8 ssd root"),
            "--disk-root",
            &format!("disk-a={}", hdd_root.to_string_lossy()),
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
        assert!(output.contains("SSD stress before file: pressure="));
        assert!(output.contains("stage=ssd-ingest"));
        assert!(output.contains("stage=hdd-copy:disk-a:1"));
        assert!(output.contains("remaining=0"));
        assert!(output.contains("File complete: nested/sample.fastq.gz"));
        assert!(output.contains("File ingest complete"));

        fs::remove_dir_all(root).expect("cleanup temp root");
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
    fn ingest_files_resolves_nested_subobject_endpoint() {
        let root = temp_root("ingest-files-subobject");
        let source_root = root.join("external");
        let ssd_root = root.join("ssd");
        let hdd_root = root.join("hdd-a");
        let registry_path = root.join("stores.json");
        let subobject_registry_path = root.join("subobjects.json");
        fs::create_dir_all(source_root.join("nested")).expect("create source");
        fs::create_dir_all(&hdd_root).expect("create hdd root");
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
            "--ssd-root",
            ssd_root.to_str().expect("utf8 ssd root"),
            "--disk-root",
            &format!("disk-a={}", hdd_root.to_string_lossy()),
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
    fn ingest_queue_requires_json_flag() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "ingest",
            "queue",
            "--live-sqlite-path",
            "/tmp/live.sqlite",
        ])
        .expect("ingest queue parses");
        let mut output = Vec::new();

        let err = run(&cli, &mut output).expect_err("json flag is required");

        assert!(matches!(err, CliError::UnsupportedIngestQueueFormat));
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

    fn create_known_ssd_marker(ssd_root: &Path) {
        let marker_dir = ssd_root.join(".dasobjectstore");
        fs::create_dir_all(&marker_dir).expect("create SSD marker directory");
        fs::write(
            marker_dir.join("device.env"),
            "role=ssd\ndevice=/dev/disk/by-id/test-ssd\nfilesystem=ext4\n",
        )
        .expect("write SSD marker");
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
