use crate::cli::{Cli, Command, PoolCommand, PoolInspectArgs, ProbeArgs};
use dasobjectstore_metadata::{inspect_pool_metadata, PoolInspectError, PoolInspectSummary};
#[cfg(target_os = "linux")]
use dasobjectstore_platform::linux::LinuxProbeProvider;
#[cfg(target_os = "macos")]
use dasobjectstore_platform::macos::MacosProbeProvider;
use dasobjectstore_platform::{
    group_enclosures, ObservedDisk, ObservedEnclosure, ProbeError, ProbeProvider, ProbeReport,
};
use std::fmt::{self, Display};
use std::io::{self, Write};

pub(crate) fn run(cli: &Cli, writer: &mut impl Write) -> Result<(), CliError> {
    match cli.command() {
        Some(Command::Probe(args)) => run_probe(args, writer),
        Some(Command::Pool(args)) => match args.command() {
            PoolCommand::Inspect(args) => run_pool_inspect(args, writer),
        },
        _ => Ok(()),
    }
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
    MetadataInspect(PoolInspectError),
    Probe(ProbeError),
    UnsupportedProbeFormat,
}

impl Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(formatter, "failed to write command output: {err}"),
            Self::Json(err) => write!(formatter, "failed to encode JSON output: {err}"),
            Self::MetadataInspect(err) => write!(formatter, "{err}"),
            Self::Probe(err) => write!(formatter, "{err}"),
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

impl From<PoolInspectError> for CliError {
    fn from(err: PoolInspectError) -> Self {
        Self::MetadataInspect(err)
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
    use dasobjectstore_core::ids::{DiskId, PoolId};
    use dasobjectstore_core::lifecycle::{DiskState, PoolState};
    use dasobjectstore_metadata::{
        manifest::DiskRole, ArtifactReference, DiskManifest, DiskManifestEntry, FormatVersion,
        MetadataArtifact, PoolManifest, DISK_MANIFEST_FILE_NAME, PLACEMENT_LOG_FILE_NAME,
        POOL_MANIFEST_FILE_NAME,
    };
    use dasobjectstore_platform::{
        EnclosureIdentity, HostPlatform, ObservedDisk, ObservedEnclosure, ProbeReport, Transport,
    };
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
