#[cfg(feature = "debug-commands")]
use crate::cli::PoolMarkerArgs;
use crate::cli::{
    Cli, Command, DiskCommand, DiskDrainArgs, DiskForceRetireArgs, DiskReplaceArgs, DiskRetireArgs,
    HealthArgs, IngestCommand, IngestQueueArgs, IngestStatusArgs, MnemosyneCommand,
    MnemosyneExportArgs, ObjectCommand, ObjectExportArgs, ObjectInspectArgs, PoolCommand,
    PoolImportArgs, PoolInspectArgs, PoolRepairArgs, ProbeArgs, ServiceCommand, ServiceComposeArgs,
    ServiceRenderComposeArgs, ServiceStatusArgs, StoreCommand, StoreDefaultsArgs,
    StoreValidateArgs,
};
mod output;

use self::output::{
    write_disk_drain_plan, write_disk_force_retirement_report, write_disk_replacement_plan,
    write_disk_retirement_report, write_health_json, write_health_summary, write_health_verbose,
    write_host_connection_status, write_ingest_status, write_object_export_report,
    write_object_inspect_summary, write_pool_import_report, write_pool_inspect_summary,
    write_pool_repair_dry_run, write_pretty_report,
};
use dasobjectstore_core::health::{HealthScore, HealthSignals};
use dasobjectstore_core::ids::DiskId;
use dasobjectstore_core::lifecycle::PoolState;
use dasobjectstore_core::risk::{ActionConfirmation, RiskPolicy};
use dasobjectstore_core::store::{StorePolicy, StorePolicyValidationErrors};
use dasobjectstore_metadata::{
    attach_clean_pool_read_only, export_settled_object, force_retire_disk,
    import_dirty_pool_read_only, inspect_pool_metadata, measure_ssd_capacity, read_disk_drain_plan,
    read_disk_replacement_plan, read_ingest_queue, read_object_inspect, request_disk_retirement,
    DiskCopyRoot, DiskDrainError, DiskRetirementError, IngestQueueReadError, ObjectExportError,
    ObjectExportRequest, ObjectInspectError, PoolInspectError, ReadOnlyAttachError,
    ReadOnlyAttachOptions, SsdCapacityMeasurementError, SsdCapacityPolicy, SsdCapacityPolicyError,
};
#[cfg(feature = "debug-commands")]
use dasobjectstore_metadata::{record_pool_state_marker_at, PoolStateMarker};
use dasobjectstore_mnemosyne::{
    export_mneion_binding_snippet, export_mneion_storage_definition, MneionBindingSnippetError,
    MneionBindingSnippetRequest, MneionStorageDefinitionError, MneionStorageDefinitionRequest,
};
use dasobjectstore_object_service::{
    plan_store_service_layout, render_compose, ComposeRenderRequest, ComposeServiceConfig,
    ObjectServiceError, StoreServiceDefinition,
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
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command as ProcessCommand;

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
            DiskCommand::Replace(args) => run_disk_replace(args, writer),
            DiskCommand::Retire(args) => run_disk_retire(args, writer),
        },
        Some(Command::Store(args)) => match args.command() {
            Some(StoreCommand::Defaults(args)) => run_store_defaults(args, writer),
            Some(StoreCommand::Validate(args)) => run_store_validate(args, writer),
            None => Ok(()),
        },
        Some(Command::Ingest(args)) => match args.command() {
            Some(IngestCommand::Status(args)) => run_ingest_status(args, writer),
            Some(IngestCommand::Queue(args)) => run_ingest_queue(args, writer),
            None => Ok(()),
        },
        Some(Command::Object(args)) => match args.command() {
            ObjectCommand::Export(args) => run_object_export(args, writer),
            ObjectCommand::Inspect(args) => run_object_inspect(args, writer),
        },
        Some(Command::Service(args)) => match args.command() {
            ServiceCommand::RenderCompose(args) => run_service_render_compose(args, writer),
            ServiceCommand::Up(args) => run_service_up(args, writer),
            ServiceCommand::Down(args) => run_service_down(args, writer),
            ServiceCommand::Status(args) => run_service_status(args, writer),
        },
        Some(Command::Mnemosyne(args)) => match args.command() {
            MnemosyneCommand::Export(args) => run_mnemosyne_export(args, writer),
        },
        _ => Ok(()),
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
    fn from_observed(disk: &ObservedDisk) -> Self {
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
        }
    }
}

fn read_current_platform_connection_status() -> Result<HostConnectionStatus, CliError> {
    let mut probe = probe_current_platform()?;
    probe.enclosures = group_enclosures(&probe.disks);

    Ok(connection_status_from_probe(&probe))
}

fn connection_status_from_probe(probe: &ProbeReport) -> HostConnectionStatus {
    let disks: Vec<DiskConnectionStatus> = probe
        .disks
        .iter()
        .map(DiskConnectionStatus::from_observed)
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

fn run_store_defaults(args: &StoreDefaultsArgs, writer: &mut impl Write) -> Result<(), CliError> {
    let policy = StorePolicy::defaults_for(args.class());

    serde_json::to_writer_pretty(&mut *writer, &policy)?;
    writer.write_all(b"\n")?;

    Ok(())
}

fn run_ingest_status(args: &IngestStatusArgs, writer: &mut impl Write) -> Result<(), CliError> {
    let policy = SsdCapacityPolicy::new(
        args.high_watermark_percent(),
        args.critical_watermark_percent(),
        args.minimum_free_bytes(),
    )?;
    let capacity = measure_ssd_capacity(args.ssd_root())?;
    let pressure = policy.evaluate(&capacity)?;

    write_ingest_status(&capacity, &policy, pressure, writer)?;

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
    let file = File::open(args.stores_file())?;
    let definitions: Vec<StoreServiceDefinition> = serde_json::from_reader(file)?;
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
    DiskRetirement(DiskRetirementError),
    ObjectExport(ObjectExportError),
    ObjectInspect(ObjectInspectError),
    ObjectService(ObjectServiceError),
    MneionBindingSnippet(MneionBindingSnippetError),
    MneionStorageDefinition(MneionStorageDefinitionError),
    CommandFailed(String),
    InvalidDiskRootMapping {
        value: String,
    },
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
            Self::DiskRetirement(err) => write!(formatter, "{err}"),
            Self::ObjectExport(err) => write!(formatter, "{err}"),
            Self::ObjectInspect(err) => write!(formatter, "{err}"),
            Self::ObjectService(err) => write!(formatter, "{err}"),
            Self::MneionBindingSnippet(err) => write!(formatter, "{err}"),
            Self::MneionStorageDefinition(err) => write!(formatter, "{err}"),
            Self::CommandFailed(err) => write!(formatter, "{err}"),
            Self::InvalidDiskRootMapping { value } => write!(
                formatter,
                "invalid disk root mapping `{value}`; expected disk-id=/mounted/disk/root"
            ),
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
        CapacityBehavior, StoreClass, StorePolicy, StorePolicyValidationError,
    };
    use dasobjectstore_metadata::{
        export_metadata_snapshot, initialize_pool, manifest::DiskRole, ArtifactReference,
        DiskManifest, DiskManifestEntry, FormatVersion, MetadataArtifact, PoolInitOptions,
        PoolManifest, SnapshotExportOptions, DISK_MANIFEST_FILE_NAME, LIVE_SCHEMA_SQL,
        PLACEMENT_LOG_FILE_NAME, POOL_MANIFEST_FILE_NAME,
    };
    use dasobjectstore_object_service::StoreServiceDefinition;
    use dasobjectstore_platform::{
        EnclosureIdentity, HostPlatform, ObservedDisk, ObservedEnclosure, ProbeReport, Transport,
    };
    use rusqlite::Connection;
    use std::fs::{self, File};
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

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
}
