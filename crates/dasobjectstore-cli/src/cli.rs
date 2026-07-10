use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use dasobjectstore_core::ids::DiskId;
#[cfg(feature = "debug-commands")]
use dasobjectstore_core::ids::PoolId;
use dasobjectstore_object_service::ObjectServiceProviderId;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

mod ingest;
mod object;
mod performance;
mod service;
mod store;
mod subobject;

pub(crate) use ingest::{
    IngestArgs, IngestCommand, IngestDirectImportArgs, IngestDrainQueueArgs, IngestFilesArgs,
    IngestQueueArgs, IngestStatusArgs,
};
pub(crate) use object::{
    ObjectArgs, ObjectCommand, ObjectExportArgs, ObjectInspectArgs, ObjectPutArgs,
};
pub(crate) use service::{
    ServiceArgs, ServiceCommand, ServiceComposeArgs, ServiceProvisionArgs,
    ServiceRenderComposeArgs, ServiceStatusArgs,
};
pub(crate) use store::{
    StoreAdoptArgs, StoreArgs, StoreCommand, StoreContentsArgs, StoreCreateArgs, StoreDefaultsArgs,
    StoreDeleteArgs, StoreDrainArgs, StoreIngestPolicyArgs, StoreListArgs, StoreS3UploadArgs,
    StoreValidateArgs,
};
pub(crate) use subobject::{
    SubobjectArgs, SubobjectCommand, SubobjectCreateArgs, SubobjectListArgs, SubobjectSearchArgs,
};

pub(crate) use performance::{
    PerformanceFileOrder, PerformanceFileSelection, PerformanceReportArgs,
    PerformanceScenarioSelection, PerformanceTestArgs,
};

/// Portable mixed-disk DAS object store.
#[derive(Debug, Parser)]
#[command(name = "dasobjectstore", version = dasobjectstore_core::VERSION)]
pub(crate) struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

impl Cli {
    pub(crate) fn command(&self) -> Option<&Command> {
        self.command.as_ref()
    }

    pub(crate) fn write_help(writer: &mut impl Write) -> io::Result<()> {
        let mut command = <Self as CommandFactory>::command();
        command.write_help(writer)?;
        writeln!(writer)
    }

    pub(crate) fn write_subcommand_help(name: &str, writer: &mut impl Write) -> io::Result<()> {
        let mut command = <Self as CommandFactory>::command();
        let help_result = command.try_get_matches_from_mut(["dasobjectstore", name, "--help"]);

        match help_result {
            Ok(_) => Ok(()),
            Err(err) => write!(writer, "{err}"),
        }
    }
}

#[derive(Debug, Eq, PartialEq, Subcommand)]
pub(crate) enum Command {
    /// Inspect candidate DAS disks and enclosures.
    Probe(ProbeArgs),
    /// Report pool, disk, and service health.
    Health(HealthArgs),
    /// Report daemon, Web UI, and object-service listener status.
    Status(StatusArgs),
    /// Manage portable storage pools.
    Pool(PoolArgs),
    /// Manage DAS member disks.
    Disk(DiskArgs),
    /// Manage object stores and policy.
    Store(StoreArgs),
    /// Inspect SSD ingest and destage work.
    Ingest(IngestArgs),
    /// Manage named SubObject endpoints.
    Subobject(SubobjectArgs),
    /// Inspect object metadata.
    Object(ObjectArgs),
    /// Render and manage the S3-compatible object service.
    Service(ServiceArgs),
    /// Export Mnemosyne/Synoptikon integration metadata.
    Mnemosyne(MnemosyneArgs),
    /// Benchmark SSD and HDD ingest settlement performance.
    PerformanceTest(PerformanceTestArgs),
    /// Rebuild a formal performance PDF report from an existing JSON artifact.
    PerformanceReport(PerformanceReportArgs),
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct StatusArgs {
    /// Emit status as JSON.
    #[arg(long)]
    json: bool,
}

impl StatusArgs {
    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct HealthArgs {
    /// Emit one-line pool and disk health summary.
    #[arg(long)]
    summary: bool,
    /// Emit per-disk health details.
    #[arg(long)]
    verbose: bool,
    /// Emit host connection and DAS transport warnings.
    #[arg(long)]
    connections: bool,
    /// Emit health report as JSON.
    #[arg(long)]
    json: bool,
}

impl HealthArgs {
    pub(crate) fn summary(&self) -> bool {
        self.summary
    }

    pub(crate) fn verbose(&self) -> bool {
        self.verbose
    }

    pub(crate) fn connections(&self) -> bool {
        self.connections
    }

    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct PoolArgs {
    #[command(subcommand)]
    command: PoolCommand,
}

impl PoolArgs {
    pub(crate) fn command(&self) -> &PoolCommand {
        &self.command
    }
}

#[derive(Debug, Eq, PartialEq, Subcommand)]
pub(crate) enum PoolCommand {
    /// Inspect portable pool metadata from a snapshot directory.
    Inspect(PoolInspectArgs),
    /// Import a portable pool snapshot for local read-only use.
    Import(PoolImportArgs),
    /// Preview pool repair actions without modifying metadata.
    Repair(PoolRepairArgs),
    /// Mark a pool clean in live metadata for developer testing.
    #[cfg(feature = "debug-commands")]
    MarkClean(PoolMarkerArgs),
    /// Mark a pool dirty in live metadata for developer testing.
    #[cfg(feature = "debug-commands")]
    MarkDirty(PoolMarkerArgs),
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct PoolInspectArgs {
    /// Path to an HDD metadata snapshot directory.
    #[arg(long)]
    metadata_path: PathBuf,
}

impl PoolInspectArgs {
    pub(crate) fn metadata_path(&self) -> &Path {
        &self.metadata_path
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct PoolImportArgs {
    /// Import the pool in read-only mode.
    #[arg(long)]
    read_only: bool,
    /// Mounted pool root or metadata snapshot directory to import from.
    #[arg(long)]
    source_path: PathBuf,
    /// Local metadata directory where recovered live.sqlite will be written.
    #[arg(long)]
    recovery_metadata_dir: PathBuf,
    /// Timestamp to record in the read-only import marker.
    #[arg(long)]
    recorded_at_utc: String,
}

impl PoolImportArgs {
    pub(crate) fn read_only(&self) -> bool {
        self.read_only
    }

    pub(crate) fn source_path(&self) -> &Path {
        &self.source_path
    }

    pub(crate) fn recovery_metadata_dir(&self) -> &Path {
        &self.recovery_metadata_dir
    }

    pub(crate) fn recorded_at_utc(&self) -> &str {
        &self.recorded_at_utc
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct PoolRepairArgs {
    /// Mounted pool root or metadata snapshot directory to inspect.
    #[arg(long)]
    source_path: PathBuf,
    /// Preview repair actions without writing recovered metadata.
    #[arg(long)]
    dry_run: bool,
}

impl PoolRepairArgs {
    pub(crate) fn source_path(&self) -> &Path {
        &self.source_path
    }

    pub(crate) fn dry_run(&self) -> bool {
        self.dry_run
    }
}

#[cfg(feature = "debug-commands")]
#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct PoolMarkerArgs {
    /// Path to live.sqlite for the pool under test.
    #[arg(long)]
    live_sqlite_path: PathBuf,
    /// Pool identifier to mark.
    #[arg(long)]
    pool_id: PoolId,
    /// Timestamp to record in metadata.
    #[arg(long)]
    recorded_at_utc: String,
}

#[cfg(feature = "debug-commands")]
impl PoolMarkerArgs {
    pub(crate) fn live_sqlite_path(&self) -> &Path {
        &self.live_sqlite_path
    }

    pub(crate) fn pool_id(&self) -> &PoolId {
        &self.pool_id
    }

    pub(crate) fn recorded_at_utc(&self) -> &str {
        &self.recorded_at_utc
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct DiskArgs {
    #[command(subcommand)]
    command: DiskCommand,
}

impl DiskArgs {
    pub(crate) fn command(&self) -> &DiskCommand {
        &self.command
    }
}

#[derive(Debug, Eq, PartialEq, Subcommand)]
pub(crate) enum DiskCommand {
    /// Plan drain work for a disk without copying or deleting data.
    Drain(DiskDrainArgs),
    /// Force a disk into retired state after explicit risk confirmation.
    ForceRetire(DiskForceRetireArgs),
    /// Lock mounted DAS roots so only the service account can access them.
    LockdownDas(DiskLockdownDasArgs),
    /// Repartition, format, and mount DAS devices for DASObjectStore.
    PrepareDas(DiskPrepareDasArgs),
    /// Plan replacement work from an old disk onto a named new disk.
    Replace(DiskReplaceArgs),
    /// Request retirement by moving a disk into draining state.
    Retire(DiskRetireArgs),
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct DiskDrainArgs {
    /// Disk identifier to drain.
    disk_id: DiskId,
    /// Path to live.sqlite for the pool.
    #[arg(long)]
    live_sqlite_path: PathBuf,
    /// Emit drain plan as JSON.
    #[arg(long)]
    json: bool,
}

impl DiskDrainArgs {
    pub(crate) fn disk_id(&self) -> &DiskId {
        &self.disk_id
    }

    pub(crate) fn live_sqlite_path(&self) -> &Path {
        &self.live_sqlite_path
    }

    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct DiskForceRetireArgs {
    /// Disk identifier to force retire.
    disk_id: DiskId,
    /// Path to live.sqlite for the pool.
    #[arg(long)]
    live_sqlite_path: PathBuf,
    /// Timestamp to record in metadata.
    #[arg(long)]
    recorded_at_utc: String,
    /// Policy allowance for force retire.
    #[arg(long)]
    allow_force_retire: bool,
    /// Action-time confirmation phrase: "confirm force retire".
    #[arg(long)]
    confirm: String,
}

impl DiskForceRetireArgs {
    pub(crate) fn disk_id(&self) -> &DiskId {
        &self.disk_id
    }

    pub(crate) fn live_sqlite_path(&self) -> &Path {
        &self.live_sqlite_path
    }

    pub(crate) fn recorded_at_utc(&self) -> &str {
        &self.recorded_at_utc
    }

    pub(crate) fn allow_force_retire(&self) -> bool {
        self.allow_force_retire
    }

    pub(crate) fn confirm(&self) -> &str {
        &self.confirm
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct DiskLockdownDasArgs {
    /// Root containing prepared SSD and HDD mountpoints.
    #[arg(long, default_value = "/srv/dasobjectstore")]
    mount_root: PathBuf,
    /// Dedicated account that owns object-store media.
    #[arg(long, default_value = "dasobjectstore")]
    service_user: String,
    /// Dedicated group that owns object-store media.
    #[arg(long, default_value = "dasobjectstore")]
    service_group: String,
    /// Create the service user and group if absent.
    #[arg(long)]
    create_service_user: bool,
    /// Show the managed lockdown plan without changing permissions.
    #[arg(long)]
    dry_run: bool,
    /// Action-time confirmation phrase: "confirm lockdown das".
    #[arg(long, default_value = "")]
    confirm: String,
}

impl DiskLockdownDasArgs {
    pub(crate) fn mount_root(&self) -> &Path {
        &self.mount_root
    }

    pub(crate) fn service_user(&self) -> &str {
        &self.service_user
    }

    pub(crate) fn service_group(&self) -> &str {
        &self.service_group
    }

    pub(crate) fn create_service_user(&self) -> bool {
        self.create_service_user
    }

    pub(crate) fn dry_run(&self) -> bool {
        self.dry_run
    }

    pub(crate) fn confirm(&self) -> &str {
        &self.confirm
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct DiskPrepareDasArgs {
    /// Stable by-id path for the mandatory SSD ingest device.
    #[arg(long)]
    ssd_device: PathBuf,
    /// HDD mapping in the form disk-id=/dev/disk/by-id/usb-...
    #[arg(long = "hdd-device")]
    hdd_devices: Vec<String>,
    /// Root under which prepared devices are mounted.
    #[arg(long, default_value = "/srv/dasobjectstore")]
    mount_root: PathBuf,
    /// Filesystem to create on each prepared device.
    #[arg(long, value_enum, default_value_t = DiskPrepareFilesystem::Ext4)]
    filesystem: DiskPrepareFilesystem,
    /// Owner account for mounted roots after preparation.
    #[arg(long)]
    owner: Option<String>,
    /// Show the managed command plan without changing devices.
    #[arg(long)]
    dry_run: bool,
    /// Policy allowance for destructive DAS preparation.
    #[arg(long)]
    allow_format: bool,
    /// Acknowledge that existing data on selected devices may be destroyed.
    #[arg(long)]
    acknowledge_existing_data: bool,
    /// Action-time confirmation phrase: "confirm prepare das".
    #[arg(long, default_value = "")]
    confirm: String,
}

impl DiskPrepareDasArgs {
    pub(crate) fn ssd_device(&self) -> &Path {
        &self.ssd_device
    }

    pub(crate) fn hdd_devices(&self) -> &[String] {
        &self.hdd_devices
    }

    pub(crate) fn mount_root(&self) -> &Path {
        &self.mount_root
    }

    pub(crate) fn filesystem(&self) -> DiskPrepareFilesystem {
        self.filesystem
    }

    pub(crate) fn owner(&self) -> Option<&str> {
        self.owner.as_deref()
    }

    pub(crate) fn dry_run(&self) -> bool {
        self.dry_run
    }

    pub(crate) fn allow_format(&self) -> bool {
        self.allow_format
    }

    pub(crate) fn acknowledge_existing_data(&self) -> bool {
        self.acknowledge_existing_data
    }

    pub(crate) fn confirm(&self) -> &str {
        &self.confirm
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum DiskPrepareFilesystem {
    Ext4,
}

impl std::fmt::Display for DiskPrepareFilesystem {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ext4 => formatter.write_str("ext4"),
        }
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct DiskReplaceArgs {
    /// Disk identifier to replace.
    old_disk_id: DiskId,
    /// New disk identifier to receive replacement copies.
    #[arg(long = "with")]
    new_disk_id: DiskId,
    /// Path to live.sqlite for the pool.
    #[arg(long)]
    live_sqlite_path: PathBuf,
    /// Emit replacement plan as JSON.
    #[arg(long)]
    json: bool,
}

impl DiskReplaceArgs {
    pub(crate) fn old_disk_id(&self) -> &DiskId {
        &self.old_disk_id
    }

    pub(crate) fn new_disk_id(&self) -> &DiskId {
        &self.new_disk_id
    }

    pub(crate) fn live_sqlite_path(&self) -> &Path {
        &self.live_sqlite_path
    }

    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct DiskRetireArgs {
    /// Disk identifier to retire.
    disk_id: DiskId,
    /// Path to live.sqlite for the pool.
    #[arg(long)]
    live_sqlite_path: PathBuf,
    /// Timestamp to record in metadata.
    #[arg(long)]
    recorded_at_utc: String,
}

impl DiskRetireArgs {
    pub(crate) fn disk_id(&self) -> &DiskId {
        &self.disk_id
    }

    pub(crate) fn live_sqlite_path(&self) -> &Path {
        &self.live_sqlite_path
    }

    pub(crate) fn recorded_at_utc(&self) -> &str {
        &self.recorded_at_utc
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct ProbeArgs {
    /// Emit probe results as JSON.
    #[arg(long)]
    json: bool,
    /// Emit probe results as human-readable text.
    #[arg(long)]
    pretty: bool,
}

impl ProbeArgs {
    pub(crate) fn json(&self) -> bool {
        self.json
    }

    pub(crate) fn pretty(&self) -> bool {
        self.pretty
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct MnemosyneArgs {
    #[command(subcommand)]
    command: MnemosyneCommand,
}

impl MnemosyneArgs {
    pub(crate) fn command(&self) -> &MnemosyneCommand {
        &self.command
    }
}

#[derive(Debug, Eq, PartialEq, Subcommand)]
pub(crate) enum MnemosyneCommand {
    /// Export Mneion-compatible storage definition and binding JSON.
    Export(MnemosyneExportArgs),
    /// Validate a DASObjectStore-managed NAS/NFS endpoint definition.
    ValidateNasNfsEndpoint(MnemosyneValidateNasNfsEndpointArgs),
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct MnemosyneExportArgs {
    /// Mneion object-store UUID to create or update.
    #[arg(long)]
    object_store_id: String,
    /// Mneion object-store display name.
    #[arg(long)]
    display_name: String,
    /// DASObjectStore object-service provider backing the endpoint.
    #[arg(long)]
    provider: ObjectServiceProviderId,
    /// S3-compatible HTTP endpoint exposed to Mneion/Limen.
    #[arg(long)]
    endpoint: String,
    /// Mneion governance-domain UUID to bind to the object store.
    #[arg(long)]
    governance_domain_id: String,
    /// Optional operator note to include in the Mneion link request.
    #[arg(long)]
    note: Option<String>,
}

impl MnemosyneExportArgs {
    pub(crate) fn object_store_id(&self) -> &str {
        &self.object_store_id
    }

    pub(crate) fn display_name(&self) -> &str {
        &self.display_name
    }

    pub(crate) fn provider(&self) -> ObjectServiceProviderId {
        self.provider
    }

    pub(crate) fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub(crate) fn governance_domain_id(&self) -> &str {
        &self.governance_domain_id
    }

    pub(crate) fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct MnemosyneValidateNasNfsEndpointArgs {
    /// Path to a JSON NAS/NFS endpoint definition file.
    #[arg(long)]
    definition_file: PathBuf,
    /// Emit validated endpoint details as JSON.
    #[arg(long)]
    json: bool,
}

impl MnemosyneValidateNasNfsEndpointArgs {
    pub(crate) fn definition_file(&self) -> &Path {
        &self.definition_file
    }

    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Cli, Command, DiskCommand, DiskPrepareFilesystem, MnemosyneCommand, PerformanceFileOrder,
        PerformanceFileSelection, PerformanceScenarioSelection, PoolCommand, ProbeArgs, StatusArgs,
    };
    use clap::Parser;
    use std::path::Path;

    #[test]
    fn parses_without_subcommand() {
        let cli = Cli::try_parse_from(["dasobjectstore"]).expect("root command parses");

        assert_eq!(cli.command(), None);
    }

    #[test]
    fn parses_top_level_command_skeletons() {
        let store = Cli::try_parse_from(["dasobjectstore", "store"])
            .expect("store subcommand should parse");
        let Some(Command::Store(args)) = store.command() else {
            panic!("expected store command");
        };
        assert!(args.command().is_none());

        let ingest = Cli::try_parse_from(["dasobjectstore", "ingest"])
            .expect("ingest subcommand should parse");
        let Some(Command::Ingest(args)) = ingest.command() else {
            panic!("expected ingest command");
        };
        assert!(args.command().is_none());
    }

    #[test]
    fn parses_top_level_status_json() {
        let cli = Cli::try_parse_from(["dasobjectstore", "status", "--json"])
            .expect("status command parses");

        assert_eq!(
            cli.command(),
            Some(&Command::Status(StatusArgs { json: true }))
        );
    }

    #[test]
    fn parses_performance_test_options() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "performance-test",
            "--file-size",
            "1GiB",
            "--file-count",
            "2",
            "--max-hdd-concurrency",
            "5",
            "--scenario",
            "ssd-overlap-drain",
            "--scenario",
            "direct-hdd",
            "--hdd-concurrency",
            "1,3,5",
            "--ssd-root",
            "/srv/dasobjectstore/ssd",
            "--hdd-root",
            "/srv/dasobjectstore/hdd",
            "--tmp-dir",
            "/tmp/dos-perf",
            "--report",
            "/tmp/dos-perf/report.pdf",
            "--json-artifact",
            "/tmp/dos-perf/report.json",
            "--tui",
            "--authoritative",
            "--keep-temp",
        ])
        .expect("performance-test parses");

        let Some(Command::PerformanceTest(args)) = cli.command() else {
            panic!("expected performance-test command");
        };
        assert_eq!(args.file_size(), Some("1GiB"));
        assert_eq!(args.file_count(), Some(2));
        assert_eq!(args.source(), None);
        assert_eq!(args.cap(), None);
        assert_eq!(args.file_orders(), vec![PerformanceFileOrder::SizeDesc]);
        assert_eq!(args.max_hdd_concurrency(), 5);
        assert_eq!(
            args.scenarios(),
            &[
                PerformanceScenarioSelection::SsdOverlapDrain,
                PerformanceScenarioSelection::DirectHdd
            ]
        );
        assert_eq!(args.hdd_concurrency(), &[1, 3, 5]);
        assert_eq!(args.ssd_root(), Some(Path::new("/srv/dasobjectstore/ssd")));
        assert_eq!(args.hdd_root(), Some(Path::new("/srv/dasobjectstore/hdd")));
        assert_eq!(args.tmp_dir(), Path::new("/tmp/dos-perf"));
        assert_eq!(args.report(), Some(Path::new("/tmp/dos-perf/report.pdf")));
        assert_eq!(
            args.json_artifact(),
            Some(Path::new("/tmp/dos-perf/report.json"))
        );
        assert!(args.tui());
        assert!(args.authoritative());
        assert!(args.keep_temp());
    }

    #[test]
    fn parses_performance_test_source_folder_options() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "performance-test",
            "--source",
            "/data/source-folder",
            "--cap",
            "750GiB",
            "--file_select",
            "larger",
            "--file_order",
            "fifo,size_desc",
            "--max-hdd-concurrency",
            "5",
        ])
        .expect("performance-test source parses");

        let Some(Command::PerformanceTest(args)) = cli.command() else {
            panic!("expected performance-test command");
        };
        assert_eq!(args.source(), Some(Path::new("/data/source-folder")));
        assert_eq!(args.cap(), Some("750GiB"));
        assert_eq!(args.file_select(), PerformanceFileSelection::Larger);
        assert_eq!(
            args.file_orders(),
            vec![PerformanceFileOrder::Fifo, PerformanceFileOrder::SizeDesc]
        );
        assert_eq!(args.file_size(), None);
        assert_eq!(args.file_count(), None);
        assert_eq!(args.max_hdd_concurrency(), 5);
    }

    #[test]
    fn parses_performance_report_rebuild_options() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "performance-report",
            "--json-artifact",
            "/var/lib/dasobjectstore/reports/performance.json",
            "--report",
            "/var/lib/dasobjectstore/reports/performance.pdf",
            "--tmp-dir",
            "/var/tmp",
            "--keep-markdown",
        ])
        .expect("performance-report parses");

        let Some(Command::PerformanceReport(args)) = cli.command() else {
            panic!("expected performance-report command");
        };
        assert_eq!(
            args.json_artifact(),
            Path::new("/var/lib/dasobjectstore/reports/performance.json")
        );
        assert_eq!(
            args.report(),
            Some(Path::new("/var/lib/dasobjectstore/reports/performance.pdf"))
        );
        assert_eq!(args.tmp_dir(), Path::new("/var/tmp"));
        assert!(args.keep_markdown());
    }

    #[test]
    fn parses_mnemosyne_export() {
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

        let Some(Command::Mnemosyne(args)) = cli.command() else {
            panic!("expected mnemosyne command");
        };
        match args.command() {
            MnemosyneCommand::Export(export) => {
                assert_eq!(
                    export.object_store_id(),
                    "4f0a1ba7-9f00-422b-bf18-87567b076daa"
                );
                assert_eq!(export.display_name(), "DASObjectStore Development");
                assert_eq!(export.provider().name(), "garage");
                assert_eq!(export.endpoint(), "http://127.0.0.1:3900");
                assert_eq!(
                    export.governance_domain_id(),
                    "22222222-2222-2222-2222-222222222222"
                );
                assert_eq!(export.note(), Some("DASObjectStore development store"));
            }
            MnemosyneCommand::ValidateNasNfsEndpoint(_) => panic!("expected export command"),
        }
    }

    #[test]
    fn parses_mnemosyne_validate_nas_nfs_endpoint() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "mnemosyne",
            "validate-nas-nfs-endpoint",
            "--definition-file",
            "/tmp/nas-endpoint.json",
            "--json",
        ])
        .expect("mnemosyne NAS/NFS endpoint validation parses");

        let Some(Command::Mnemosyne(args)) = cli.command() else {
            panic!("expected mnemosyne command");
        };
        match args.command() {
            MnemosyneCommand::ValidateNasNfsEndpoint(validate) => {
                assert_eq!(
                    validate.definition_file(),
                    Path::new("/tmp/nas-endpoint.json")
                );
                assert!(validate.json());
            }
            MnemosyneCommand::Export(_) => panic!("expected NAS/NFS endpoint validation command"),
        }
    }

    #[test]
    fn parses_health_summary_default() {
        let cli = Cli::try_parse_from(["dasobjectstore", "health"]).expect("health parses");

        let Some(Command::Health(args)) = cli.command() else {
            panic!("expected health command");
        };
        assert!(!args.summary());
        assert!(!args.verbose());
        assert!(!args.connections());
        assert!(!args.json());
    }

    #[test]
    fn parses_health_output_flags() {
        let cases = [
            ("--summary", true, false, false, false),
            ("--verbose", false, true, false, false),
            ("--connections", false, false, true, false),
            ("--json", false, false, false, true),
        ];

        for (flag, summary, verbose, connections, json) in cases {
            let cli =
                Cli::try_parse_from(["dasobjectstore", "health", flag]).expect("health parses");

            let Some(Command::Health(args)) = cli.command() else {
                panic!("expected health command");
            };
            assert_eq!(args.summary(), summary);
            assert_eq!(args.verbose(), verbose);
            assert_eq!(args.connections(), connections);
            assert_eq!(args.json(), json);
        }
    }

    #[test]
    fn parses_disk_drain() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "disk",
            "drain",
            "disk-a",
            "--live-sqlite-path",
            "/tmp/live.sqlite",
            "--json",
        ])
        .expect("disk drain parses");

        let Some(Command::Disk(args)) = cli.command() else {
            panic!("expected disk command");
        };
        match args.command() {
            DiskCommand::Drain(drain) => {
                assert_eq!(drain.disk_id().as_str(), "disk-a");
                assert_eq!(drain.live_sqlite_path(), Path::new("/tmp/live.sqlite"));
                assert!(drain.json());
            }
            _ => panic!("expected drain command"),
        }
    }

    #[test]
    fn parses_disk_retire() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "disk",
            "retire",
            "disk-a",
            "--live-sqlite-path",
            "/tmp/live.sqlite",
            "--recorded-at-utc",
            "2026-01-02T00:00:00Z",
        ])
        .expect("disk retire parses");

        let Some(Command::Disk(args)) = cli.command() else {
            panic!("expected disk command");
        };
        match args.command() {
            DiskCommand::Drain(_) => panic!("expected retire command"),
            DiskCommand::ForceRetire(_) => panic!("expected retire command"),
            DiskCommand::LockdownDas(_) | DiskCommand::PrepareDas(_) => {
                panic!("expected retire command")
            }
            DiskCommand::Replace(_) => panic!("expected retire command"),
            DiskCommand::Retire(retire) => {
                assert_eq!(retire.disk_id().as_str(), "disk-a");
                assert_eq!(retire.live_sqlite_path(), Path::new("/tmp/live.sqlite"));
                assert_eq!(retire.recorded_at_utc(), "2026-01-02T00:00:00Z");
            }
        }
    }

    #[test]
    fn parses_disk_force_retire() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "disk",
            "force-retire",
            "disk-a",
            "--live-sqlite-path",
            "/tmp/live.sqlite",
            "--recorded-at-utc",
            "2026-01-02T00:00:00Z",
            "--allow-force-retire",
            "--confirm",
            "confirm force retire",
        ])
        .expect("disk force-retire parses");

        let Some(Command::Disk(args)) = cli.command() else {
            panic!("expected disk command");
        };
        match args.command() {
            DiskCommand::ForceRetire(force_retire) => {
                assert_eq!(force_retire.disk_id().as_str(), "disk-a");
                assert_eq!(
                    force_retire.live_sqlite_path(),
                    Path::new("/tmp/live.sqlite")
                );
                assert_eq!(force_retire.recorded_at_utc(), "2026-01-02T00:00:00Z");
                assert!(force_retire.allow_force_retire());
                assert_eq!(force_retire.confirm(), "confirm force retire");
            }
            _ => panic!("expected force-retire command"),
        }
    }

    #[test]
    fn parses_disk_lockdown_das() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "disk",
            "lockdown-das",
            "--mount-root",
            "/srv/dasobjectstore",
            "--service-user",
            "dasobjectstore",
            "--service-group",
            "dasobjectstore",
            "--create-service-user",
            "--dry-run",
            "--confirm",
            "confirm lockdown das",
        ])
        .expect("disk lockdown-das parses");

        let Some(Command::Disk(args)) = cli.command() else {
            panic!("expected disk command");
        };
        match args.command() {
            DiskCommand::LockdownDas(lockdown) => {
                assert_eq!(lockdown.mount_root(), Path::new("/srv/dasobjectstore"));
                assert_eq!(lockdown.service_user(), "dasobjectstore");
                assert_eq!(lockdown.service_group(), "dasobjectstore");
                assert!(lockdown.create_service_user());
                assert!(lockdown.dry_run());
                assert_eq!(lockdown.confirm(), "confirm lockdown das");
            }
            _ => panic!("expected lockdown-das command"),
        }
    }

    #[test]
    fn parses_disk_prepare_das() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "disk",
            "prepare-das",
            "--ssd-device",
            "/dev/disk/by-id/usb-Samsung_QNAP_0000081064-0:0",
            "--hdd-device",
            "qnap-1057=/dev/disk/by-id/usb-ST4000VN_QNAP_0000081057-0:0",
            "--hdd-device",
            "qnap-1058=/dev/disk/by-id/usb-ST4000VN_QNAP_0000081058-0:0",
            "--mount-root",
            "/srv/dasobjectstore",
            "--filesystem",
            "ext4",
            "--owner",
            "stephen",
            "--dry-run",
            "--allow-format",
            "--acknowledge-existing-data",
            "--confirm",
            "confirm prepare das",
        ])
        .expect("disk prepare-das parses");

        let Some(Command::Disk(args)) = cli.command() else {
            panic!("expected disk command");
        };
        match args.command() {
            DiskCommand::PrepareDas(prepare) => {
                assert_eq!(
                    prepare.ssd_device(),
                    Path::new("/dev/disk/by-id/usb-Samsung_QNAP_0000081064-0:0")
                );
                assert_eq!(
                    prepare.hdd_devices(),
                    &[
                        "qnap-1057=/dev/disk/by-id/usb-ST4000VN_QNAP_0000081057-0:0".to_string(),
                        "qnap-1058=/dev/disk/by-id/usb-ST4000VN_QNAP_0000081058-0:0".to_string()
                    ]
                );
                assert_eq!(prepare.mount_root(), Path::new("/srv/dasobjectstore"));
                assert_eq!(prepare.filesystem(), DiskPrepareFilesystem::Ext4);
                assert_eq!(prepare.owner(), Some("stephen"));
                assert!(prepare.dry_run());
                assert!(prepare.allow_format());
                assert!(prepare.acknowledge_existing_data());
                assert_eq!(prepare.confirm(), "confirm prepare das");
            }
            _ => panic!("expected prepare-das command"),
        }
    }

    #[test]
    fn parses_disk_replace() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "disk",
            "replace",
            "disk-a",
            "--with",
            "disk-b",
            "--live-sqlite-path",
            "/tmp/live.sqlite",
            "--json",
        ])
        .expect("disk replace parses");

        let Some(Command::Disk(args)) = cli.command() else {
            panic!("expected disk command");
        };
        match args.command() {
            DiskCommand::Replace(replace) => {
                assert_eq!(replace.old_disk_id().as_str(), "disk-a");
                assert_eq!(replace.new_disk_id().as_str(), "disk-b");
                assert_eq!(replace.live_sqlite_path(), Path::new("/tmp/live.sqlite"));
                assert!(replace.json());
            }
            _ => panic!("expected replace command"),
        }
    }

    #[test]
    fn parses_pool_inspect_metadata_path() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "pool",
            "inspect",
            "--metadata-path",
            "/tmp/metadata",
        ])
        .expect("pool inspect parses");

        let Some(Command::Pool(args)) = cli.command() else {
            panic!("expected pool command");
        };
        match args.command() {
            PoolCommand::Inspect(inspect) => {
                assert_eq!(inspect.metadata_path(), Path::new("/tmp/metadata"));
            }
            PoolCommand::Import(_) => panic!("expected inspect command"),
            PoolCommand::Repair(_) => panic!("expected inspect command"),
            #[cfg(feature = "debug-commands")]
            _ => panic!("expected inspect command"),
        }
    }

    #[test]
    fn parses_pool_import_read_only() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "pool",
            "import",
            "--read-only",
            "--source-path",
            "/Volumes/pool-disk",
            "--recovery-metadata-dir",
            "/tmp/recovered",
            "--recorded-at-utc",
            "2026-01-04T00:00:00Z",
        ])
        .expect("pool import parses");

        let Some(Command::Pool(args)) = cli.command() else {
            panic!("expected pool command");
        };
        match args.command() {
            PoolCommand::Import(import) => {
                assert!(import.read_only());
                assert_eq!(import.source_path(), Path::new("/Volumes/pool-disk"));
                assert_eq!(import.recovery_metadata_dir(), Path::new("/tmp/recovered"));
                assert_eq!(import.recorded_at_utc(), "2026-01-04T00:00:00Z");
            }
            _ => panic!("expected import command"),
        }
    }

    #[test]
    fn parses_pool_repair_dry_run() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "pool",
            "repair",
            "--source-path",
            "/Volumes/pool-disk",
            "--dry-run",
        ])
        .expect("pool repair parses");

        let Some(Command::Pool(args)) = cli.command() else {
            panic!("expected pool command");
        };
        match args.command() {
            PoolCommand::Repair(repair) => {
                assert_eq!(repair.source_path(), Path::new("/Volumes/pool-disk"));
                assert!(repair.dry_run());
            }
            _ => panic!("expected repair command"),
        }
    }

    #[cfg(feature = "debug-commands")]
    #[test]
    fn parses_pool_mark_clean_debug_command() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "pool",
            "mark-clean",
            "--live-sqlite-path",
            "/tmp/live.sqlite",
            "--pool-id",
            "pool-a",
            "--recorded-at-utc",
            "2026-01-03T00:00:00Z",
        ])
        .expect("pool mark-clean parses");

        let Some(Command::Pool(args)) = cli.command() else {
            panic!("expected pool command");
        };
        match args.command() {
            PoolCommand::MarkClean(marker) => {
                assert_eq!(marker.live_sqlite_path(), Path::new("/tmp/live.sqlite"));
                assert_eq!(marker.pool_id().as_str(), "pool-a");
                assert_eq!(marker.recorded_at_utc(), "2026-01-03T00:00:00Z");
            }
            _ => panic!("expected mark-clean"),
        }
    }

    #[test]
    fn parses_probe_json_flag() {
        let cli = Cli::try_parse_from(["dasobjectstore", "probe", "--json"]).expect("probe parses");

        assert_eq!(
            cli.command(),
            Some(&Command::Probe(ProbeArgs {
                json: true,
                pretty: false
            }))
        );
    }

    #[test]
    fn parses_probe_pretty_flag() {
        let cli =
            Cli::try_parse_from(["dasobjectstore", "probe", "--pretty"]).expect("probe parses");

        assert_eq!(
            cli.command(),
            Some(&Command::Probe(ProbeArgs {
                json: false,
                pretty: true
            }))
        );
    }
}
