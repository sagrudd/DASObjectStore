#[cfg(feature = "debug-commands")]
use crate::cli::PoolMarkerArgs;
use crate::cli::{
    Cli, Command, IngestCommand, IngestQueueArgs, IngestStatusArgs, ObjectCommand,
    ObjectInspectArgs, PoolCommand, PoolInspectArgs, ProbeArgs, ServiceCommand, ServiceComposeArgs,
    ServiceRenderComposeArgs, StoreCommand, StoreDefaultsArgs, StoreValidateArgs,
};
use dasobjectstore_core::store::{StorePolicy, StorePolicyValidationErrors};
use dasobjectstore_metadata::{
    inspect_pool_metadata, measure_ssd_capacity, read_ingest_queue, read_object_inspect,
    IngestQueueReadError, ObjectInspectError, ObjectInspectSummary, PoolInspectError,
    PoolInspectSummary, SsdCapacity, SsdCapacityMeasurementError, SsdCapacityPolicy,
    SsdCapacityPolicyError, SsdPressure,
};
#[cfg(feature = "debug-commands")]
use dasobjectstore_metadata::{record_pool_state_marker_at, PoolStateMarker};
use dasobjectstore_object_service::{
    plan_store_service_layout, render_compose, ComposeRenderRequest, ComposeServiceConfig,
    ObjectServiceError, StoreServiceDefinition,
};
#[cfg(target_os = "linux")]
use dasobjectstore_platform::linux::LinuxProbeProvider;
#[cfg(target_os = "macos")]
use dasobjectstore_platform::macos::MacosProbeProvider;
use dasobjectstore_platform::{
    group_enclosures, ObservedDisk, ObservedEnclosure, ProbeError, ProbeProvider, ProbeReport,
};
use std::fmt::{self, Display};
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command as ProcessCommand;

pub(crate) fn run(cli: &Cli, writer: &mut impl Write) -> Result<(), CliError> {
    match cli.command() {
        Some(Command::Probe(args)) => run_probe(args, writer),
        Some(Command::Pool(args)) => match args.command() {
            PoolCommand::Inspect(args) => run_pool_inspect(args, writer),
            #[cfg(feature = "debug-commands")]
            PoolCommand::MarkClean(args) => run_pool_mark_clean(args, writer),
            #[cfg(feature = "debug-commands")]
            PoolCommand::MarkDirty(args) => run_pool_mark_dirty(args, writer),
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
            ObjectCommand::Inspect(args) => run_object_inspect(args, writer),
        },
        Some(Command::Service(args)) => match args.command() {
            ServiceCommand::RenderCompose(args) => run_service_render_compose(args, writer),
            ServiceCommand::Up(args) => run_service_up(args, writer),
        },
        _ => Ok(()),
    }
}

fn run_service_up(args: &ServiceComposeArgs, writer: &mut impl Write) -> Result<(), CliError> {
    let command = docker_compose_args(args.compose_file(), args.project_directory(), ["up", "-d"]);

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

    writeln!(writer, "Object service started")?;

    Ok(())
}

fn docker_compose_args<'a>(
    compose_file: &'a Path,
    project_directory: Option<&'a Path>,
    action_args: impl IntoIterator<Item = &'a str>,
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

fn run_pool_inspect(args: &PoolInspectArgs, writer: &mut impl Write) -> Result<(), CliError> {
    let summary = inspect_pool_metadata(args.metadata_path())?;
    write_pool_inspect_summary(&summary, writer)?;

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

fn write_ingest_status(
    capacity: &SsdCapacity,
    policy: &SsdCapacityPolicy,
    pressure: SsdPressure,
    writer: &mut impl Write,
) -> Result<(), io::Error> {
    writeln!(
        writer,
        "SSD ingest root: {}",
        capacity.path.to_string_lossy()
    )?;
    writeln!(writer, "Pressure: {pressure:?}")?;
    writeln!(writer, "Total bytes: {}", capacity.total_bytes)?;
    writeln!(writer, "Available bytes: {}", capacity.available_bytes)?;
    writeln!(writer, "Used bytes: {}", capacity.used_bytes())?;
    writeln!(writer, "Used percent: {}", capacity.used_percent_floor())?;
    writeln!(
        writer,
        "High watermark percent: {}",
        policy.high_watermark_percent
    )?;
    writeln!(
        writer,
        "Critical watermark percent: {}",
        policy.critical_watermark_percent
    )?;
    writeln!(writer, "Minimum free bytes: {}", policy.minimum_free_bytes)
}

fn write_pool_inspect_summary(
    summary: &PoolInspectSummary,
    writer: &mut impl Write,
) -> Result<(), io::Error> {
    writeln!(writer, "Pool: {}", summary.pool_id)?;
    writeln!(writer, "State: {:?}", summary.state)?;
    writeln!(writer, "Created: {}", summary.created_at_utc)?;
    writeln!(writer, "Updated: {}", summary.updated_at_utc)?;
    writeln!(writer, "Disks: {}", summary.disk_count)?;
    writeln!(
        writer,
        "Metadata path: {}",
        summary.metadata_path.to_string_lossy()
    )
}

fn write_object_inspect_summary(
    summary: &ObjectInspectSummary,
    writer: &mut impl Write,
) -> Result<(), io::Error> {
    writeln!(writer, "Object: {}", summary.object_id)?;
    writeln!(writer, "Store: {}", summary.store_id)?;
    writeln!(writer, "Store class: {}", summary.store_class)?;
    writeln!(writer, "State: {}", summary.state)?;
    writeln!(
        writer,
        "Size bytes: {}",
        summary
            .size_bytes
            .map(|value| value.to_string())
            .unwrap_or_else(|| "<unknown>".to_string())
    )?;
    writeln!(
        writer,
        "Content hash: {}",
        summary.content_hash.as_deref().unwrap_or("<unknown>")
    )?;
    writeln!(writer, "Placements: {}", summary.placements.len())?;
    for placement in &summary.placements {
        writeln!(
            writer,
            "- {} disk={} path={} verified_at={}",
            placement.placement_id,
            placement.disk_id,
            placement.relative_path,
            placement
                .verified_at_utc
                .as_deref()
                .unwrap_or("<unverified>")
        )?;
    }
    writeln!(
        writer,
        "Live metadata: {}",
        summary.live_sqlite_path.to_string_lossy()
    )
}

fn write_pretty_report(report: &ProbeReport, writer: &mut impl Write) -> Result<(), io::Error> {
    writeln!(writer, "Platform: {:?}", report.platform)?;
    writeln!(writer, "Disks: {}", report.disks.len())?;
    for disk in &report.disks {
        write_disk(disk, writer)?;
    }

    writeln!(writer, "Enclosures: {}", report.enclosures.len())?;
    for enclosure in &report.enclosures {
        write_enclosure(enclosure, writer)?;
    }

    if !report.warnings.is_empty() {
        writeln!(writer, "Warnings: {}", report.warnings.len())?;
        for warning in &report.warnings {
            writeln!(writer, "- {}: {}", warning.code, warning.message)?;
        }
    }

    Ok(())
}

fn write_disk(disk: &ObservedDisk, writer: &mut impl Write) -> Result<(), io::Error> {
    let device_path = disk.device_path.as_deref().unwrap_or("<unknown>");
    let size = disk
        .size_bytes
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unknown-size".to_string());
    let serial = disk.serial_hint.as_deref().unwrap_or("unknown-serial");

    writeln!(
        writer,
        "- {device_path} size={size} transport={:?} serial={serial}",
        disk.transport
    )
}

fn write_enclosure(
    enclosure: &ObservedEnclosure,
    writer: &mut impl Write,
) -> Result<(), io::Error> {
    let topology = enclosure
        .identity
        .usb_topology_path
        .as_deref()
        .unwrap_or("<unknown>");
    writeln!(
        writer,
        "- topology={topology} disks={}",
        enclosure.disk_device_paths.join(",")
    )
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
    ObjectInspect(ObjectInspectError),
    ObjectService(ObjectServiceError),
    CommandFailed(String),
    SsdCapacityMeasurement(SsdCapacityMeasurementError),
    SsdCapacityPolicy(SsdCapacityPolicyError),
    #[cfg(feature = "debug-commands")]
    MetadataMarker(String),
    Probe(ProbeError),
    StorePolicyValidation(StorePolicyValidationErrors),
    UnsupportedIngestQueueFormat,
    UnsupportedProbeFormat,
}

impl Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(formatter, "failed to access command input or output: {err}"),
            Self::Json(err) => write!(formatter, "failed to process JSON: {err}"),
            Self::IngestQueueRead(err) => write!(formatter, "{err}"),
            Self::MetadataInspect(err) => write!(formatter, "{err}"),
            Self::ObjectInspect(err) => write!(formatter, "{err}"),
            Self::ObjectService(err) => write!(formatter, "{err}"),
            Self::CommandFailed(err) => write!(formatter, "{err}"),
            Self::SsdCapacityMeasurement(err) => write!(formatter, "{err}"),
            Self::SsdCapacityPolicy(err) => write!(formatter, "{err}"),
            #[cfg(feature = "debug-commands")]
            Self::MetadataMarker(err) => write!(formatter, "failed to update pool metadata: {err}"),
            Self::Probe(err) => write!(formatter, "{err}"),
            Self::StorePolicyValidation(err) => write!(formatter, "{err}"),
            Self::UnsupportedIngestQueueFormat => {
                formatter.write_str("ingest queue requires JSON output; use `--json`")
            }
            Self::UnsupportedProbeFormat => formatter
                .write_str("probe requires exactly one output format; use `--json` or `--pretty`"),
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

impl From<ObjectInspectError> for CliError {
    fn from(err: ObjectInspectError) -> Self {
        Self::ObjectInspect(err)
    }
}

impl From<ObjectServiceError> for CliError {
    fn from(err: ObjectServiceError) -> Self {
        Self::ObjectService(err)
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
    use super::{run, write_pretty_report, CliError};
    use crate::cli::Cli;
    use clap::Parser;
    use dasobjectstore_core::ids::{DiskId, PoolId, StoreId};
    use dasobjectstore_core::lifecycle::{DiskState, PoolState};
    use dasobjectstore_core::store::{
        CapacityBehavior, StoreClass, StorePolicy, StorePolicyValidationError,
    };
    #[cfg(feature = "debug-commands")]
    use dasobjectstore_metadata::{initialize_pool, PoolInitOptions};
    use dasobjectstore_metadata::{
        manifest::DiskRole, ArtifactReference, DiskManifest, DiskManifestEntry, FormatVersion,
        MetadataArtifact, PoolManifest, DISK_MANIFEST_FILE_NAME, LIVE_SCHEMA_SQL,
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

    #[cfg(feature = "debug-commands")]
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
                    '{}',
                    '2026-01-01T00:00:00Z',
                    '2026-01-01T00:00:00Z'
                 )",
                [],
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

    fn create_live_sqlite_with_object(root: &Path) -> PathBuf {
        fs::create_dir_all(root).expect("create temp root");
        let live_sqlite_path = root.join("live.sqlite");
        let connection = Connection::open(&live_sqlite_path).expect("open live sqlite");
        connection
            .execute_batch(LIVE_SCHEMA_SQL)
            .expect("schema applies");
        insert_store(&connection);
        connection
            .execute_batch(
                "INSERT INTO disks (
                    disk_id, pool_id, role, state, created_at_utc, updated_at_utc
                 ) VALUES (
                    'disk-a', 'pool-a', 'hdd_capacity', 'Healthy',
                    '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'
                 );
                 INSERT INTO objects (
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
