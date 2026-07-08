use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
#[cfg(feature = "debug-commands")]
use dasobjectstore_core::ids::PoolId;
use dasobjectstore_core::ids::{DiskId, ObjectId, StoreId};
use dasobjectstore_core::object_type::ObjectType;
use dasobjectstore_core::store::StoreClass;
use dasobjectstore_daemon::DaemonIngestConflictPolicy;
use dasobjectstore_object_service::{ObjectServiceProviderId, RemoteS3AuthAuthority};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

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
pub(crate) struct PerformanceTestArgs {
    /// Size of each generated test file, for example 100MiB, 1GiB, or 1.1TiB.
    #[arg(long = "file_size", alias = "file-size")]
    file_size: Option<String>,
    /// Number of generated files to test.
    #[arg(long = "file_count", alias = "file-count")]
    file_count: Option<u32>,
    /// Existing folder of files to benchmark instead of generated random data.
    #[arg(long)]
    source: Option<PathBuf>,
    /// Cap an existing source-folder benchmark to a prefix such as 750GiB or 1TiB.
    #[arg(long)]
    cap: Option<String>,
    /// Maximum concurrent HDD writes to model.
    #[arg(long, default_value_t = 3)]
    max_hdd_concurrency: usize,
    /// Number of HDD copies to land for each logical file; accepted values are 1, 2, or 3.
    #[arg(long, default_value_t = 1)]
    redundancy: usize,
    /// SSD root to stress; defaults to DASOBJECTSTORE_SSD_ROOT or the packaged root.
    #[arg(long)]
    ssd_root: Option<PathBuf>,
    /// Managed HDD root containing per-disk roots.
    #[arg(long, hide = true)]
    hdd_root: Option<PathBuf>,
    /// Directory for generated source files; defaults to /tmp.
    #[arg(long, default_value = "/tmp")]
    tmp_dir: PathBuf,
    /// Final PDF report path; defaults to a timestamped PDF file in /tmp.
    #[arg(long)]
    report: Option<PathBuf>,
    /// JSON artifact path; defaults beside the PDF report.
    #[arg(long = "json-artifact")]
    json_artifact: Option<PathBuf>,
    /// Render an embedded terminal benchmark view while the run executes.
    #[arg(long)]
    tui: bool,
    /// Store the benchmark recommendation as the daemon's authoritative ingest policy.
    #[arg(long)]
    authoritative: bool,
    /// Keep temporary benchmark files for inspection instead of deleting them.
    #[arg(long)]
    keep_temp: bool,
}

impl PerformanceTestArgs {
    pub(crate) fn file_size(&self) -> Option<&str> {
        self.file_size.as_deref()
    }

    pub(crate) fn file_count(&self) -> Option<u32> {
        self.file_count
    }

    pub(crate) fn source(&self) -> Option<&Path> {
        self.source.as_deref()
    }

    pub(crate) fn cap(&self) -> Option<&str> {
        self.cap.as_deref()
    }

    pub(crate) fn max_hdd_concurrency(&self) -> usize {
        self.max_hdd_concurrency
    }

    pub(crate) fn redundancy(&self) -> usize {
        self.redundancy
    }

    pub(crate) fn ssd_root(&self) -> Option<&Path> {
        self.ssd_root.as_deref()
    }

    pub(crate) fn hdd_root(&self) -> Option<&Path> {
        self.hdd_root.as_deref()
    }

    pub(crate) fn tmp_dir(&self) -> &Path {
        &self.tmp_dir
    }

    pub(crate) fn report(&self) -> Option<&Path> {
        self.report.as_deref()
    }

    pub(crate) fn json_artifact(&self) -> Option<&Path> {
        self.json_artifact.as_deref()
    }

    pub(crate) fn tui(&self) -> bool {
        self.tui
    }

    pub(crate) fn authoritative(&self) -> bool {
        self.authoritative
    }

    pub(crate) fn keep_temp(&self) -> bool {
        self.keep_temp
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
pub(crate) struct StoreArgs {
    #[command(subcommand)]
    command: Option<StoreCommand>,
}

impl StoreArgs {
    pub(crate) fn command(&self) -> Option<&StoreCommand> {
        self.command.as_ref()
    }
}

#[derive(Debug, Eq, PartialEq, Subcommand)]
pub(crate) enum StoreCommand {
    /// Adopt portable object stores from a DAS SSD on this host.
    Adopt(StoreAdoptArgs),
    /// Inspect objects and aggregate folder sizes in a store.
    Contents(StoreContentsArgs),
    /// Create or update a system-managed object store.
    Create(StoreCreateArgs),
    /// Delete all objects and payload files in a store.
    Drain(StoreDrainArgs),
    /// Delete a drained object store and its registry entries.
    Delete(StoreDeleteArgs),
    /// Emit the built-in JSON policy defaults for a store class.
    Defaults(StoreDefaultsArgs),
    /// List system-managed object stores.
    List(StoreListArgs),
    /// Render AWS CLI commands for remote S3-compatible uploads.
    S3Upload(StoreS3UploadArgs),
    /// Validate a JSON store policy file.
    Validate(StoreValidateArgs),
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct StoreContentsArgs {
    /// Store identifier whose contents should be inspected.
    store_id: StoreId,
    /// Render aggregate folder sizes, similar to du -h -d <n>.
    #[arg(long)]
    du: bool,
    /// Render a tree of directories and object leaves.
    #[arg(long)]
    tree: bool,
    /// Maximum folder depth for --du or --tree.
    #[arg(short = 'd', long, default_value_t = 1)]
    depth: usize,
    /// Rust regex used to filter relative object paths and full object IDs.
    #[arg(long)]
    filter: Option<String>,
    /// Emit object entries as JSON.
    #[arg(long)]
    json: bool,
    /// Advanced override for the live SQLite metadata path.
    #[arg(long, hide = true)]
    live_sqlite_path: Option<PathBuf>,
}

impl StoreContentsArgs {
    pub(crate) fn store_id(&self) -> &StoreId {
        &self.store_id
    }

    pub(crate) fn du(&self) -> bool {
        self.du
    }

    pub(crate) fn tree(&self) -> bool {
        self.tree
    }

    pub(crate) fn depth(&self) -> usize {
        self.depth
    }

    pub(crate) fn filter(&self) -> Option<&str> {
        self.filter.as_deref()
    }

    pub(crate) fn json(&self) -> bool {
        self.json
    }

    pub(crate) fn live_sqlite_path(&self) -> Option<&Path> {
        self.live_sqlite_path.as_deref()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum StoreS3UploadAuth {
    Mneion,
    LocalPassword,
}

impl From<StoreS3UploadAuth> for RemoteS3AuthAuthority {
    fn from(value: StoreS3UploadAuth) -> Self {
        match value {
            StoreS3UploadAuth::Mneion => Self::Mneion,
            StoreS3UploadAuth::LocalPassword => Self::LocalPassword,
        }
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct StoreS3UploadArgs {
    /// Store identifier to expose for remote S3 uploads.
    store_id: StoreId,
    /// S3-compatible service URL reachable from the remote computer.
    #[arg(long)]
    endpoint_url: String,
    /// Explicit bucket name for remote clients that cannot read the store registry.
    #[arg(long)]
    bucket: Option<String>,
    /// AWS CLI region value to store in the generated profile.
    #[arg(long, default_value = "garage")]
    region: String,
    /// AWS CLI profile name; defaults to dasobjectstore-<store>.
    #[arg(long)]
    profile: Option<String>,
    /// Authority that manages remote S3 credential issuance.
    #[arg(long, value_enum, default_value_t = StoreS3UploadAuth::Mneion)]
    auth: StoreS3UploadAuth,
    /// Local appliance user for --auth local-password.
    #[arg(long)]
    username: Option<String>,
    /// Emit the remote upload plan as JSON.
    #[arg(long)]
    json: bool,
    /// Advanced test override for the system-managed store registry path.
    #[arg(long, hide = true)]
    registry_path: Option<PathBuf>,
}

impl StoreS3UploadArgs {
    pub(crate) fn store_id(&self) -> &StoreId {
        &self.store_id
    }

    pub(crate) fn endpoint_url(&self) -> &str {
        &self.endpoint_url
    }

    pub(crate) fn bucket(&self) -> Option<&str> {
        self.bucket.as_deref()
    }

    pub(crate) fn region(&self) -> &str {
        &self.region
    }

    pub(crate) fn profile(&self) -> Option<&str> {
        self.profile.as_deref()
    }

    pub(crate) fn auth(&self) -> StoreS3UploadAuth {
        self.auth
    }

    pub(crate) fn username(&self) -> Option<&str> {
        self.username.as_deref()
    }

    pub(crate) fn json(&self) -> bool {
        self.json
    }

    pub(crate) fn registry_path(&self) -> Option<&Path> {
        self.registry_path.as_deref()
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct StoreDrainArgs {
    /// Store identifier to drain.
    store_id: StoreId,
    /// Advanced override for the live SQLite metadata path.
    #[arg(long, hide = true)]
    live_sqlite_path: Option<PathBuf>,
    /// Advanced override for the system-managed store registry path.
    #[arg(long, hide = true)]
    registry_path: Option<PathBuf>,
    /// Managed HDD mount root.
    #[arg(long)]
    hdd_root: Option<PathBuf>,
    /// Show affected objects and payloads without deleting.
    #[arg(long)]
    dry_run: bool,
    /// Policy allowance for deleting all objects in the store.
    #[arg(long)]
    allow_store_drain: bool,
    /// Action-time confirmation phrase: "confirm store drain".
    #[arg(long, default_value = "")]
    confirm: String,
    /// Emit the drain report as JSON.
    #[arg(long)]
    json: bool,
}

impl StoreDrainArgs {
    pub(crate) fn store_id(&self) -> &StoreId {
        &self.store_id
    }

    pub(crate) fn live_sqlite_path(&self) -> Option<&Path> {
        self.live_sqlite_path.as_deref()
    }

    pub(crate) fn registry_path(&self) -> Option<&Path> {
        self.registry_path.as_deref()
    }

    pub(crate) fn hdd_root(&self) -> Option<&Path> {
        self.hdd_root.as_deref()
    }

    pub(crate) fn dry_run(&self) -> bool {
        self.dry_run
    }

    pub(crate) fn allow_store_drain(&self) -> bool {
        self.allow_store_drain
    }

    pub(crate) fn confirm(&self) -> &str {
        &self.confirm
    }

    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct StoreDeleteArgs {
    /// Store identifier to delete.
    store_id: StoreId,
    /// Path to live.sqlite for the pool.
    #[arg(long)]
    live_sqlite_path: PathBuf,
    /// Managed HDD mount root.
    #[arg(long)]
    hdd_root: Option<PathBuf>,
    /// DAS SSD root used for portable store and SubObject metadata.
    #[arg(long)]
    ssd_root: Option<PathBuf>,
    /// Show affected metadata, payloads, and registry entries without deleting.
    #[arg(long)]
    dry_run: bool,
    /// Policy allowance for deleting the store and all of its contents.
    #[arg(long)]
    allow_store_delete: bool,
    /// Action-time confirmation phrase: "confirm store delete".
    #[arg(long, default_value = "")]
    confirm: String,
    /// Emit the delete report as JSON.
    #[arg(long)]
    json: bool,
    /// Advanced test override for the system-managed store registry path.
    #[arg(long, hide = true)]
    registry_path: Option<PathBuf>,
    /// Advanced test override for the system-managed SubObject registry path.
    #[arg(long, hide = true)]
    subobject_registry_path: Option<PathBuf>,
}

impl StoreDeleteArgs {
    pub(crate) fn store_id(&self) -> &StoreId {
        &self.store_id
    }

    pub(crate) fn live_sqlite_path(&self) -> &Path {
        &self.live_sqlite_path
    }

    pub(crate) fn hdd_root(&self) -> Option<&Path> {
        self.hdd_root.as_deref()
    }

    pub(crate) fn ssd_root(&self) -> Option<&Path> {
        self.ssd_root.as_deref()
    }

    pub(crate) fn dry_run(&self) -> bool {
        self.dry_run
    }

    pub(crate) fn allow_store_delete(&self) -> bool {
        self.allow_store_delete
    }

    pub(crate) fn confirm(&self) -> &str {
        &self.confirm
    }

    pub(crate) fn json(&self) -> bool {
        self.json
    }

    pub(crate) fn registry_path(&self) -> Option<&Path> {
        self.registry_path.as_deref()
    }

    pub(crate) fn subobject_registry_path(&self) -> Option<&Path> {
        self.subobject_registry_path.as_deref()
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct StoreCreateArgs {
    /// Store identifier to create or update.
    store_id: StoreId,
    /// Store class to create.
    #[arg(long)]
    class: StoreClass,
    /// Override the default copy count for this store class.
    #[arg(long)]
    copies: Option<u8>,
    /// Explicit S3 bucket name; defaults to a stable name derived from the store ID.
    #[arg(long)]
    bucket: Option<String>,
    /// Unix group allowed to write objects to this store.
    #[arg(long)]
    writer_group: Option<String>,
    /// DAS SSD root used for portable store metadata.
    #[arg(long)]
    ssd_root: Option<PathBuf>,
    /// Emit the created or updated store definition as JSON.
    #[arg(long)]
    json: bool,
    /// Advanced test override for the system-managed store registry path.
    #[arg(long, hide = true)]
    registry_path: Option<PathBuf>,
}

impl StoreCreateArgs {
    pub(crate) fn store_id(&self) -> &StoreId {
        &self.store_id
    }

    pub(crate) fn class(&self) -> StoreClass {
        self.class
    }

    pub(crate) fn copies(&self) -> Option<u8> {
        self.copies
    }

    pub(crate) fn bucket(&self) -> Option<&str> {
        self.bucket.as_deref()
    }

    pub(crate) fn writer_group(&self) -> Option<&str> {
        self.writer_group.as_deref()
    }

    pub(crate) fn ssd_root(&self) -> Option<&Path> {
        self.ssd_root.as_deref()
    }

    pub(crate) fn json(&self) -> bool {
        self.json
    }

    pub(crate) fn registry_path(&self) -> Option<&Path> {
        self.registry_path.as_deref()
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct StoreAdoptArgs {
    /// DAS SSD root containing portable store metadata.
    #[arg(long)]
    ssd_root: Option<PathBuf>,
    /// Emit adopted store definitions as JSON.
    #[arg(long)]
    json: bool,
    /// Advanced test override for the system-managed store registry path.
    #[arg(long, hide = true)]
    registry_path: Option<PathBuf>,
}

impl StoreAdoptArgs {
    pub(crate) fn ssd_root(&self) -> Option<&Path> {
        self.ssd_root.as_deref()
    }

    pub(crate) fn json(&self) -> bool {
        self.json
    }

    pub(crate) fn registry_path(&self) -> Option<&Path> {
        self.registry_path.as_deref()
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct StoreListArgs {
    /// Emit store definitions as JSON.
    #[arg(long)]
    json: bool,
    /// List portable store metadata from the DAS SSD instead of host metadata.
    #[arg(long)]
    portable: bool,
    /// DAS SSD root used when listing portable store metadata.
    #[arg(long)]
    ssd_root: Option<PathBuf>,
    /// Advanced test override for the system-managed store registry path.
    #[arg(long, hide = true)]
    registry_path: Option<PathBuf>,
}

impl StoreListArgs {
    pub(crate) fn json(&self) -> bool {
        self.json
    }

    pub(crate) fn portable(&self) -> bool {
        self.portable
    }

    pub(crate) fn ssd_root(&self) -> Option<&Path> {
        self.ssd_root.as_deref()
    }

    pub(crate) fn registry_path(&self) -> Option<&Path> {
        self.registry_path.as_deref()
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct StoreDefaultsArgs {
    /// Store class to emit defaults for.
    #[arg(long)]
    class: StoreClass,
}

impl StoreDefaultsArgs {
    pub(crate) fn class(&self) -> StoreClass {
        self.class
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct StoreValidateArgs {
    /// Path to a JSON store policy file.
    policy_file: PathBuf,
}

impl StoreValidateArgs {
    pub(crate) fn policy_file(&self) -> &Path {
        &self.policy_file
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct IngestArgs {
    #[command(subcommand)]
    command: Option<IngestCommand>,
}

impl IngestArgs {
    pub(crate) fn command(&self) -> Option<&IngestCommand> {
        self.command.as_ref()
    }
}

#[derive(Debug, Eq, PartialEq, Subcommand)]
pub(crate) enum IngestCommand {
    /// Import a directory tree of files into a store through SSD ingest.
    Files(IngestFilesArgs),
    /// Report SSD ingest capacity and pressure state.
    Status(IngestStatusArgs),
    /// Inspect live ingest queue entries for a store.
    Queue(IngestQueueArgs),
    /// Cancel active queued ingest jobs for a store.
    DrainQueue(IngestDrainQueueArgs),
    /// Import a reproducible object directly to HDD, bypassing SSD ingest.
    DirectImport(IngestDirectImportArgs),
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct IngestFilesArgs {
    /// Store or SubObject endpoint receiving the imported files.
    endpoint: StoreId,
    /// Mounted source directory containing files to import.
    #[arg(long)]
    source: PathBuf,
    /// Logical object type assigned to imported files.
    #[arg(long, default_value_t = ObjectType::Naive)]
    object_type: ObjectType,
    /// SSD ingest root; defaults to DASOBJECTSTORE_SSD_ROOT or /srv/dasobjectstore/ssd.
    #[arg(long)]
    ssd_root: Option<PathBuf>,
    /// Advanced override for the managed HDD root.
    #[arg(long, hide = true)]
    hdd_root: Option<PathBuf>,
    /// Override the store policy copy count for this import.
    #[arg(long)]
    copies: Option<u8>,
    /// Reuse an existing object only when its recorded checksum matches the incoming file.
    #[arg(long, conflicts_with_all = ["lazy", "force"])]
    strict: bool,
    /// Reuse an existing object when the object path and file size match.
    #[arg(long, conflicts_with_all = ["strict", "force"])]
    lazy: bool,
    /// Always ingest every file as a new stored version/payload.
    #[arg(long, conflicts_with_all = ["strict", "lazy"])]
    force: bool,
    /// Render the upload context and daemon progress view while the upload runs.
    #[arg(long)]
    tui: bool,
    /// Show the planned file set without importing.
    #[arg(long)]
    dry_run: bool,
    /// Developer/test fallback that writes through the old local executor instead of the daemon.
    #[arg(long, hide = true)]
    local_direct: bool,
    /// Advanced test override for the system-managed store registry path.
    #[arg(long, hide = true)]
    registry_path: Option<PathBuf>,
    /// Advanced test override for the system-managed SubObject registry path.
    #[arg(long, hide = true)]
    subobject_registry_path: Option<PathBuf>,
}

impl IngestFilesArgs {
    pub(crate) fn endpoint(&self) -> &StoreId {
        &self.endpoint
    }

    pub(crate) fn source(&self) -> &Path {
        &self.source
    }

    pub(crate) fn object_type(&self) -> ObjectType {
        self.object_type
    }

    pub(crate) fn ssd_root(&self) -> Option<&Path> {
        self.ssd_root.as_deref()
    }

    pub(crate) fn hdd_root(&self) -> Option<&Path> {
        self.hdd_root.as_deref()
    }

    pub(crate) fn copies(&self) -> Option<u8> {
        self.copies
    }

    pub(crate) fn conflict_policy(&self) -> DaemonIngestConflictPolicy {
        if self.force {
            DaemonIngestConflictPolicy::Force
        } else if self.lazy {
            DaemonIngestConflictPolicy::Lazy
        } else {
            DaemonIngestConflictPolicy::Strict
        }
    }

    pub(crate) fn dry_run(&self) -> bool {
        self.dry_run
    }

    pub(crate) fn tui(&self) -> bool {
        self.tui
    }

    pub(crate) fn local_direct(&self) -> bool {
        self.local_direct
    }

    pub(crate) fn registry_path(&self) -> Option<&Path> {
        self.registry_path.as_deref()
    }

    pub(crate) fn subobject_registry_path(&self) -> Option<&Path> {
        self.subobject_registry_path.as_deref()
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct SubobjectArgs {
    #[command(subcommand)]
    command: SubobjectCommand,
}

impl SubobjectArgs {
    pub(crate) fn command(&self) -> &SubobjectCommand {
        &self.command
    }
}

#[derive(Debug, Eq, PartialEq, Subcommand)]
pub(crate) enum SubobjectCommand {
    /// Create a named SubObject endpoint.
    Create(SubobjectCreateArgs),
    /// List named SubObject endpoints.
    List(SubobjectListArgs),
    /// Search SubObject endpoints by name or path.
    Search(SubobjectSearchArgs),
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct SubobjectCreateArgs {
    /// Unique SubObject endpoint name.
    name: String,
    /// Parent object store for a top-level SubObject.
    #[arg(long, conflicts_with = "parent")]
    store: Option<StoreId>,
    /// Parent SubObject name for a nested SubObject.
    #[arg(long)]
    parent: Option<String>,
    /// DAS SSD root used for portable SubObject metadata.
    #[arg(long)]
    ssd_root: Option<PathBuf>,
    /// Advanced test override for the system-managed SubObject registry path.
    #[arg(long, hide = true)]
    registry_path: Option<PathBuf>,
    /// Advanced test override for the system-managed store registry path.
    #[arg(long, hide = true)]
    stores_registry_path: Option<PathBuf>,
}

impl SubobjectCreateArgs {
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn store(&self) -> Option<&StoreId> {
        self.store.as_ref()
    }

    pub(crate) fn parent(&self) -> Option<&str> {
        self.parent.as_deref()
    }

    pub(crate) fn ssd_root(&self) -> Option<&Path> {
        self.ssd_root.as_deref()
    }

    pub(crate) fn registry_path(&self) -> Option<&Path> {
        self.registry_path.as_deref()
    }

    pub(crate) fn stores_registry_path(&self) -> Option<&Path> {
        self.stores_registry_path.as_deref()
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct SubobjectListArgs {
    /// Advanced test override for the system-managed SubObject registry path.
    #[arg(long, hide = true)]
    registry_path: Option<PathBuf>,
}

impl SubobjectListArgs {
    pub(crate) fn registry_path(&self) -> Option<&Path> {
        self.registry_path.as_deref()
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct SubobjectSearchArgs {
    /// Case-insensitive name or path fragment.
    query: String,
    /// Advanced test override for the system-managed SubObject registry path.
    #[arg(long, hide = true)]
    registry_path: Option<PathBuf>,
}

impl SubobjectSearchArgs {
    pub(crate) fn query(&self) -> &str {
        &self.query
    }

    pub(crate) fn registry_path(&self) -> Option<&Path> {
        self.registry_path.as_deref()
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct IngestStatusArgs {
    /// Path to the mandatory SSD ingest root.
    #[arg(long)]
    ssd_root: PathBuf,
    /// SSD used percentage at which lower-priority writes should pause.
    #[arg(long, default_value_t = dasobjectstore_metadata::DEFAULT_SSD_HIGH_WATERMARK_PERCENT)]
    high_watermark_percent: u8,
    /// SSD used percentage at which non-critical writes should be rejected.
    #[arg(long, default_value_t = dasobjectstore_metadata::DEFAULT_SSD_CRITICAL_WATERMARK_PERCENT)]
    critical_watermark_percent: u8,
    /// Minimum free bytes to preserve on the SSD ingest filesystem.
    #[arg(long, default_value_t = 0)]
    minimum_free_bytes: u64,
}

impl IngestStatusArgs {
    pub(crate) fn ssd_root(&self) -> &Path {
        &self.ssd_root
    }

    pub(crate) fn high_watermark_percent(&self) -> u8 {
        self.high_watermark_percent
    }

    pub(crate) fn critical_watermark_percent(&self) -> u8 {
        self.critical_watermark_percent
    }

    pub(crate) fn minimum_free_bytes(&self) -> u64 {
        self.minimum_free_bytes
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct IngestQueueArgs {
    /// Store identifier whose queue entries should be listed.
    store_id: StoreId,
    /// Advanced override for the live SQLite metadata path.
    #[arg(long, hide = true)]
    live_sqlite_path: Option<PathBuf>,
    /// Emit queue entries as JSON.
    #[arg(long)]
    json: bool,
}

impl IngestQueueArgs {
    pub(crate) fn store_id(&self) -> &StoreId {
        &self.store_id
    }

    pub(crate) fn live_sqlite_path(&self) -> Option<&Path> {
        self.live_sqlite_path.as_deref()
    }

    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct IngestDrainQueueArgs {
    /// Store identifier whose active ingest jobs should be cancelled.
    store_id: StoreId,
    /// Advanced override for the live SQLite metadata path.
    #[arg(long, hide = true)]
    live_sqlite_path: Option<PathBuf>,
    /// Show active queue jobs that would be cancelled without mutating metadata.
    #[arg(long)]
    dry_run: bool,
    /// Policy allowance for cancelling active ingest queue jobs.
    #[arg(long)]
    allow_ingest_queue_drain: bool,
    /// Action-time confirmation phrase: "confirm ingest queue drain".
    #[arg(long, default_value = "")]
    confirm: String,
    /// Reason recorded on cancelled ingest jobs.
    #[arg(long, default_value = "operator drained ingest queue")]
    reason: String,
    /// Emit the drain report as JSON.
    #[arg(long)]
    json: bool,
}

impl IngestDrainQueueArgs {
    pub(crate) fn store_id(&self) -> &StoreId {
        &self.store_id
    }

    pub(crate) fn live_sqlite_path(&self) -> Option<&Path> {
        self.live_sqlite_path.as_deref()
    }

    pub(crate) fn dry_run(&self) -> bool {
        self.dry_run
    }

    pub(crate) fn allow_ingest_queue_drain(&self) -> bool {
        self.allow_ingest_queue_drain
    }

    pub(crate) fn confirm(&self) -> &str {
        &self.confirm
    }

    pub(crate) fn reason(&self) -> &str {
        &self.reason
    }

    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct IngestDirectImportArgs {
    /// Object identifier to assign to the imported object.
    object_id: ObjectId,
    /// Disk receiving this direct import copy.
    #[arg(long)]
    disk_id: DiskId,
    /// Local source file to import.
    #[arg(long)]
    source: PathBuf,
    /// Logical object type assigned to this direct import.
    #[arg(long, default_value_t = ObjectType::Naive)]
    object_type: ObjectType,
    /// Final HDD destination path to write.
    #[arg(long)]
    destination: PathBuf,
    /// Expected SHA-256 content hash for the source object.
    #[arg(long)]
    expected_sha256: String,
    /// Optional public source URL, accession, or provenance URI.
    #[arg(long)]
    source_uri: Option<String>,
    /// JSON store policy file; must be reproducible_cache with direct-to-HDD ingest.
    #[arg(long)]
    policy_file: PathBuf,
    /// Allow bypassing SSD ingest for this import.
    #[arg(long)]
    allow_direct_to_hdd_import: bool,
    /// Required confirmation phrase: confirm direct-to-hdd import.
    #[arg(long)]
    confirm: String,
    /// Emit import report as JSON.
    #[arg(long)]
    json: bool,
}

impl IngestDirectImportArgs {
    pub(crate) fn object_id(&self) -> &ObjectId {
        &self.object_id
    }

    pub(crate) fn disk_id(&self) -> &DiskId {
        &self.disk_id
    }

    pub(crate) fn source(&self) -> &Path {
        &self.source
    }

    pub(crate) fn object_type(&self) -> ObjectType {
        self.object_type
    }

    pub(crate) fn destination(&self) -> &Path {
        &self.destination
    }

    pub(crate) fn expected_sha256(&self) -> &str {
        &self.expected_sha256
    }

    pub(crate) fn source_uri(&self) -> Option<&str> {
        self.source_uri.as_deref()
    }

    pub(crate) fn policy_file(&self) -> &Path {
        &self.policy_file
    }

    pub(crate) fn allow_direct_to_hdd_import(&self) -> bool {
        self.allow_direct_to_hdd_import
    }

    pub(crate) fn confirm(&self) -> &str {
        &self.confirm
    }

    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct ObjectArgs {
    #[command(subcommand)]
    command: ObjectCommand,
}

impl ObjectArgs {
    pub(crate) fn command(&self) -> &ObjectCommand {
        &self.command
    }
}

#[derive(Debug, Eq, PartialEq, Subcommand)]
pub(crate) enum ObjectCommand {
    /// Export one settled object from a verified HDD placement.
    Export(ObjectExportArgs),
    /// Inspect one object from live metadata.
    Inspect(ObjectInspectArgs),
    /// Stage one object on SSD and settle verified HDD copies.
    Put(ObjectPutArgs),
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct ObjectExportArgs {
    /// Object identifier to export.
    object_id: ObjectId,
    /// Path to live.sqlite for the attached read-only pool.
    #[arg(long)]
    live_sqlite_path: PathBuf,
    /// Destination file to write.
    #[arg(long)]
    destination: PathBuf,
    /// Disk root mapping in the form disk-id=/mounted/disk/root.
    #[arg(long = "disk-root")]
    disk_roots: Vec<String>,
    /// Emit export report as JSON.
    #[arg(long)]
    json: bool,
}

impl ObjectExportArgs {
    pub(crate) fn object_id(&self) -> &ObjectId {
        &self.object_id
    }

    pub(crate) fn live_sqlite_path(&self) -> &Path {
        &self.live_sqlite_path
    }

    pub(crate) fn destination(&self) -> &Path {
        &self.destination
    }

    pub(crate) fn disk_roots(&self) -> &[String] {
        &self.disk_roots
    }

    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct ObjectPutArgs {
    /// Object identifier to store.
    object_id: ObjectId,
    /// Source file to import.
    #[arg(long)]
    source: PathBuf,
    /// Logical object type assigned to this file.
    #[arg(long, default_value_t = ObjectType::Naive)]
    object_type: ObjectType,
    /// SSD ingest root used for the mandatory fast landing copy.
    #[arg(long)]
    ssd_root: PathBuf,
    /// Disk root mapping in the form disk-id=/mounted/disk/root.
    #[arg(long = "disk-root")]
    disk_roots: Vec<String>,
    /// Number of verified HDD copies to settle.
    #[arg(long, default_value_t = 1)]
    copies: u8,
    /// Emit put report as JSON.
    #[arg(long)]
    json: bool,
}

impl ObjectPutArgs {
    pub(crate) fn object_id(&self) -> &ObjectId {
        &self.object_id
    }

    pub(crate) fn source(&self) -> &Path {
        &self.source
    }

    pub(crate) fn object_type(&self) -> ObjectType {
        self.object_type
    }

    pub(crate) fn ssd_root(&self) -> &Path {
        &self.ssd_root
    }

    pub(crate) fn disk_roots(&self) -> &[String] {
        &self.disk_roots
    }

    pub(crate) fn copies(&self) -> u8 {
        self.copies
    }

    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct ObjectInspectArgs {
    /// Object identifier to inspect.
    object_id: ObjectId,
    /// Path to live.sqlite for the pool.
    #[arg(long)]
    live_sqlite_path: PathBuf,
    /// Emit object metadata as JSON.
    #[arg(long)]
    json: bool,
}

impl ObjectInspectArgs {
    pub(crate) fn object_id(&self) -> &ObjectId {
        &self.object_id
    }

    pub(crate) fn live_sqlite_path(&self) -> &Path {
        &self.live_sqlite_path
    }

    pub(crate) fn json(&self) -> bool {
        self.json
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

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct ServiceArgs {
    #[command(subcommand)]
    command: ServiceCommand,
}

impl ServiceArgs {
    pub(crate) fn command(&self) -> &ServiceCommand {
        &self.command
    }
}

#[derive(Debug, Eq, PartialEq, Subcommand)]
pub(crate) enum ServiceCommand {
    /// Render Docker Compose YAML for store-aware object service access.
    RenderCompose(ServiceRenderComposeArgs),
    /// Start the rendered object service with Docker Compose.
    Up(ServiceComposeArgs),
    /// Stop the rendered object service with Docker Compose.
    Down(ServiceComposeArgs),
    /// Inspect the rendered object service with Docker Compose.
    Status(ServiceStatusArgs),
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct ServiceComposeArgs {
    /// Path to the rendered Docker Compose YAML file.
    #[arg(long)]
    compose_file: PathBuf,
    /// Optional Docker Compose project directory.
    #[arg(long)]
    project_directory: Option<PathBuf>,
    /// Print the Docker Compose command without executing it.
    #[arg(long)]
    dry_run: bool,
}

impl ServiceComposeArgs {
    pub(crate) fn compose_file(&self) -> &Path {
        &self.compose_file
    }

    pub(crate) fn project_directory(&self) -> Option<&Path> {
        self.project_directory.as_deref()
    }

    pub(crate) fn dry_run(&self) -> bool {
        self.dry_run
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct ServiceStatusArgs {
    /// Path to the rendered Docker Compose YAML file.
    #[arg(long)]
    compose_file: PathBuf,
    /// Optional Docker Compose project directory.
    #[arg(long)]
    project_directory: Option<PathBuf>,
    /// Emit Docker Compose service status as JSON.
    #[arg(long)]
    json: bool,
    /// Print the Docker Compose status command as JSON without executing it.
    #[arg(long)]
    dry_run: bool,
}

impl ServiceStatusArgs {
    pub(crate) fn compose_file(&self) -> &Path {
        &self.compose_file
    }

    pub(crate) fn project_directory(&self) -> Option<&Path> {
        self.project_directory.as_deref()
    }

    pub(crate) fn json(&self) -> bool {
        self.json
    }

    pub(crate) fn dry_run(&self) -> bool {
        self.dry_run
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct ServiceRenderComposeArgs {
    /// Advanced test override for the system-managed store registry path.
    #[arg(long = "stores-file", hide = true)]
    stores_file: Option<PathBuf>,
    /// Docker Compose project name.
    #[arg(long)]
    project_name: String,
    /// SSD metadata path to mount into the service container.
    #[arg(long)]
    ssd_metadata_path: PathBuf,
    /// HDD data path to mount into the service container.
    #[arg(long)]
    hdd_data_path: PathBuf,
    /// Object service provider to render for.
    #[arg(long)]
    provider: ObjectServiceProviderId,
    /// Compose service name.
    #[arg(long, default_value = "object-service")]
    service_name: String,
    /// Container image for the selected object service.
    #[arg(long)]
    image: String,
    /// API port to expose on 127.0.0.1.
    #[arg(long)]
    api_port: u16,
}

impl ServiceRenderComposeArgs {
    pub(crate) fn stores_file(&self) -> Option<&Path> {
        self.stores_file.as_deref()
    }

    pub(crate) fn project_name(&self) -> &str {
        &self.project_name
    }

    pub(crate) fn ssd_metadata_path(&self) -> &Path {
        &self.ssd_metadata_path
    }

    pub(crate) fn hdd_data_path(&self) -> &Path {
        &self.hdd_data_path
    }

    pub(crate) fn provider(&self) -> ObjectServiceProviderId {
        self.provider
    }

    pub(crate) fn service_name(&self) -> &str {
        &self.service_name
    }

    pub(crate) fn image(&self) -> &str {
        &self.image
    }

    pub(crate) fn api_port(&self) -> u16 {
        self.api_port
    }
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
        Cli, Command, DiskCommand, DiskPrepareFilesystem, IngestArgs, IngestCommand,
        MnemosyneCommand, ObjectCommand, PoolCommand, ProbeArgs, ServiceCommand, StatusArgs,
        StoreArgs, StoreCommand, StoreS3UploadAuth, SubobjectCommand,
    };
    use clap::Parser;
    use dasobjectstore_core::object_type::ObjectType;
    use dasobjectstore_core::store::StoreClass;
    use dasobjectstore_daemon::DaemonIngestConflictPolicy;
    use std::path::Path;

    #[test]
    fn parses_without_subcommand() {
        let cli = Cli::try_parse_from(["dasobjectstore"]).expect("root command parses");

        assert_eq!(cli.command(), None);
    }

    #[test]
    fn parses_top_level_command_skeletons() {
        let cases = [
            ("store", Command::Store(StoreArgs { command: None })),
            ("ingest", Command::Ingest(IngestArgs { command: None })),
        ];

        for (name, expected) in cases {
            let cli =
                Cli::try_parse_from(["dasobjectstore", name]).expect("subcommand should parse");

            assert_eq!(cli.command(), Some(&expected));
        }
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
        assert_eq!(args.max_hdd_concurrency(), 5);
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
            "--max-hdd-concurrency",
            "5",
        ])
        .expect("performance-test source parses");

        let Some(Command::PerformanceTest(args)) = cli.command() else {
            panic!("expected performance-test command");
        };
        assert_eq!(args.source(), Some(Path::new("/data/source-folder")));
        assert_eq!(args.cap(), Some("750GiB"));
        assert_eq!(args.file_size(), None);
        assert_eq!(args.file_count(), None);
        assert_eq!(args.max_hdd_concurrency(), 5);
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
    fn parses_service_render_compose() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "service",
            "render-compose",
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

        let Some(Command::Service(args)) = cli.command() else {
            panic!("expected service command");
        };
        match args.command() {
            ServiceCommand::RenderCompose(render) => {
                assert_eq!(render.stores_file(), None);
                assert_eq!(render.project_name(), "dasobjectstore-dev");
                assert_eq!(render.ssd_metadata_path(), Path::new("/ssd/meta"));
                assert_eq!(render.hdd_data_path(), Path::new("/hdd/data"));
                assert_eq!(render.provider().name(), "garage");
                assert_eq!(render.service_name(), "garage");
                assert_eq!(render.image(), "garage:latest");
                assert_eq!(render.api_port(), 3900);
            }
            _ => panic!("expected render-compose command"),
        }
    }

    #[test]
    fn parses_service_up_dry_run() {
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

        let Some(Command::Service(args)) = cli.command() else {
            panic!("expected service command");
        };
        match args.command() {
            ServiceCommand::Up(up) => {
                assert_eq!(up.compose_file(), Path::new("/tmp/compose.yaml"));
                assert_eq!(up.project_directory(), Some(Path::new("/tmp/project")));
                assert!(up.dry_run());
            }
            _ => panic!("expected up command"),
        }
    }

    #[test]
    fn parses_service_down_dry_run() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "service",
            "down",
            "--compose-file",
            "/tmp/compose.yaml",
            "--dry-run",
        ])
        .expect("service down parses");

        let Some(Command::Service(args)) = cli.command() else {
            panic!("expected service command");
        };
        match args.command() {
            ServiceCommand::Down(down) => {
                assert_eq!(down.compose_file(), Path::new("/tmp/compose.yaml"));
                assert_eq!(down.project_directory(), None);
                assert!(down.dry_run());
            }
            _ => panic!("expected down command"),
        }
    }

    #[test]
    fn parses_service_status_json_dry_run() {
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

        let Some(Command::Service(args)) = cli.command() else {
            panic!("expected service command");
        };
        match args.command() {
            ServiceCommand::Status(status) => {
                assert_eq!(status.compose_file(), Path::new("/tmp/compose.yaml"));
                assert_eq!(status.project_directory(), Some(Path::new("/tmp/project")));
                assert!(status.json());
                assert!(status.dry_run());
            }
            _ => panic!("expected status command"),
        }
    }

    #[test]
    fn parses_object_inspect() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "object",
            "inspect",
            "object-a",
            "--live-sqlite-path",
            "/tmp/live.sqlite",
            "--json",
        ])
        .expect("object inspect parses");

        let Some(Command::Object(args)) = cli.command() else {
            panic!("expected object command");
        };
        match args.command() {
            ObjectCommand::Export(_) | ObjectCommand::Put(_) => panic!("expected inspect command"),
            ObjectCommand::Inspect(inspect) => {
                assert_eq!(inspect.object_id().as_str(), "object-a");
                assert_eq!(inspect.live_sqlite_path(), Path::new("/tmp/live.sqlite"));
                assert!(inspect.json());
            }
        }
    }

    #[test]
    fn parses_object_export() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "object",
            "export",
            "object-a",
            "--live-sqlite-path",
            "/tmp/live.sqlite",
            "--destination",
            "/tmp/export/object-a",
            "--disk-root",
            "disk-a=/Volumes/disk-a",
            "--json",
        ])
        .expect("object export parses");

        let Some(Command::Object(args)) = cli.command() else {
            panic!("expected object command");
        };
        match args.command() {
            ObjectCommand::Export(export) => {
                assert_eq!(export.object_id().as_str(), "object-a");
                assert_eq!(export.live_sqlite_path(), Path::new("/tmp/live.sqlite"));
                assert_eq!(export.destination(), Path::new("/tmp/export/object-a"));
                assert_eq!(export.disk_roots(), &["disk-a=/Volumes/disk-a".to_string()]);
                assert!(export.json());
            }
            ObjectCommand::Inspect(_) | ObjectCommand::Put(_) => panic!("expected export command"),
        }
    }

    #[test]
    fn parses_object_put() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "object",
            "put",
            "object-a",
            "--source",
            "/tmp/input/object-a",
            "--object-type",
            "bam",
            "--ssd-root",
            "/tmp/ssd",
            "--disk-root",
            "disk-a=/Volumes/disk-a",
            "--disk-root",
            "disk-b=/Volumes/disk-b",
            "--copies",
            "2",
            "--json",
        ])
        .expect("object put parses");

        let Some(Command::Object(args)) = cli.command() else {
            panic!("expected object command");
        };
        match args.command() {
            ObjectCommand::Put(put) => {
                assert_eq!(put.object_id().as_str(), "object-a");
                assert_eq!(put.source(), Path::new("/tmp/input/object-a"));
                assert_eq!(put.object_type(), ObjectType::Bam);
                assert_eq!(put.ssd_root(), Path::new("/tmp/ssd"));
                assert_eq!(
                    put.disk_roots(),
                    &[
                        "disk-a=/Volumes/disk-a".to_string(),
                        "disk-b=/Volumes/disk-b".to_string()
                    ]
                );
                assert_eq!(put.copies(), 2);
                assert!(put.json());
            }
            ObjectCommand::Export(_) | ObjectCommand::Inspect(_) => {
                panic!("expected put command")
            }
        }
    }

    #[test]
    fn parses_ingest_status() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "ingest",
            "status",
            "--ssd-root",
            "/tmp/pool-ssd",
            "--high-watermark-percent",
            "80",
            "--critical-watermark-percent",
            "90",
            "--minimum-free-bytes",
            "1024",
        ])
        .expect("ingest status parses");

        let Some(Command::Ingest(args)) = cli.command() else {
            panic!("expected ingest command");
        };
        match args.command() {
            Some(IngestCommand::Status(status)) => {
                assert_eq!(status.ssd_root(), Path::new("/tmp/pool-ssd"));
                assert_eq!(status.high_watermark_percent(), 80);
                assert_eq!(status.critical_watermark_percent(), 90);
                assert_eq!(status.minimum_free_bytes(), 1024);
            }
            _ => panic!("expected status command"),
        }
    }

    #[test]
    fn parses_ingest_files() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "ingest",
            "files",
            "zymo_fecal_2025.05",
            "--source",
            "/mnt/external/zymo",
            "--object-type",
            "pod5",
            "--ssd-root",
            "/srv/dasobjectstore/ssd",
            "--copies",
            "1",
            "--lazy",
            "--tui",
            "--dry-run",
        ])
        .expect("ingest files parses");

        let Some(Command::Ingest(args)) = cli.command() else {
            panic!("expected ingest command");
        };
        match args.command() {
            Some(IngestCommand::Files(files)) => {
                assert_eq!(files.endpoint().as_str(), "zymo_fecal_2025.05");
                assert_eq!(files.source(), Path::new("/mnt/external/zymo"));
                assert_eq!(files.object_type(), ObjectType::Pod5);
                assert_eq!(files.ssd_root(), Some(Path::new("/srv/dasobjectstore/ssd")));
                assert_eq!(files.copies(), Some(1));
                assert_eq!(files.conflict_policy(), DaemonIngestConflictPolicy::Lazy);
                assert!(files.tui());
                assert!(files.dry_run());
            }
            _ => panic!("expected files command"),
        }
    }

    #[test]
    fn defaults_ingest_files_conflict_policy_to_strict() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "ingest",
            "files",
            "zymo_fecal_2025.05",
            "--source",
            "/mnt/external/zymo",
        ])
        .expect("ingest files parses");

        let Some(Command::Ingest(args)) = cli.command() else {
            panic!("expected ingest command");
        };
        match args.command() {
            Some(IngestCommand::Files(files)) => {
                assert_eq!(files.conflict_policy(), DaemonIngestConflictPolicy::Strict);
                assert_eq!(files.object_type(), ObjectType::Naive);
            }
            _ => panic!("expected files command"),
        }
    }

    #[test]
    fn rejects_ambiguous_ingest_files_conflict_policies() {
        let err = Cli::try_parse_from([
            "dasobjectstore",
            "ingest",
            "files",
            "zymo_fecal_2025.05",
            "--source",
            "/mnt/external/zymo",
            "--lazy",
            "--force",
        ])
        .expect_err("conflicting conflict policies rejected");

        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn parses_subobject_create_under_store() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "subobject",
            "create",
            "Xenognostikon",
            "--store",
            "ENA",
        ])
        .expect("subobject create parses");

        let Some(Command::Subobject(args)) = cli.command() else {
            panic!("expected subobject command");
        };
        match args.command() {
            SubobjectCommand::Create(create) => {
                assert_eq!(create.name(), "Xenognostikon");
                assert_eq!(create.store().expect("store").as_str(), "ENA");
                assert_eq!(create.parent(), None);
            }
            _ => panic!("expected create command"),
        }
    }

    #[test]
    fn parses_ingest_queue_json() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "ingest",
            "queue",
            "generated-data",
            "--json",
        ])
        .expect("ingest queue parses");

        let Some(Command::Ingest(args)) = cli.command() else {
            panic!("expected ingest command");
        };
        match args.command() {
            Some(IngestCommand::Queue(queue)) => {
                assert_eq!(queue.store_id().as_str(), "generated-data");
                assert_eq!(queue.live_sqlite_path(), None);
                assert!(queue.json());
            }
            _ => panic!("expected queue command"),
        }
    }

    #[test]
    fn parses_ingest_drain_queue() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "ingest",
            "drain-queue",
            "generated-data",
            "--dry-run",
            "--allow-ingest-queue-drain",
            "--confirm",
            "confirm ingest queue drain",
            "--json",
        ])
        .expect("ingest drain-queue parses");

        let Some(Command::Ingest(args)) = cli.command() else {
            panic!("expected ingest command");
        };
        match args.command() {
            Some(IngestCommand::DrainQueue(drain)) => {
                assert_eq!(drain.store_id().as_str(), "generated-data");
                assert_eq!(drain.live_sqlite_path(), None);
                assert!(drain.dry_run());
                assert!(drain.allow_ingest_queue_drain());
                assert_eq!(drain.confirm(), "confirm ingest queue drain");
                assert!(drain.json());
            }
            _ => panic!("expected drain-queue command"),
        }
    }

    #[test]
    fn parses_ingest_direct_import() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "ingest",
            "direct-import",
            "object-a",
            "--disk-id",
            "disk-a",
            "--source",
            "/tmp/downloads/reference.fa.zst",
            "--object-type",
            "ena_sra",
            "--destination",
            "/mnt/disk-a/objects/reference.fa.zst",
            "--expected-sha256",
            "abc123",
            "--source-uri",
            "https://example.invalid/reference.fa.zst",
            "--policy-file",
            "/tmp/reproducible-cache.json",
            "--allow-direct-to-hdd-import",
            "--confirm",
            "confirm direct-to-hdd import",
            "--json",
        ])
        .expect("ingest direct-import parses");

        let Some(Command::Ingest(args)) = cli.command() else {
            panic!("expected ingest command");
        };
        match args.command() {
            Some(IngestCommand::DirectImport(import)) => {
                assert_eq!(import.object_id().as_str(), "object-a");
                assert_eq!(import.disk_id().as_str(), "disk-a");
                assert_eq!(
                    import.source(),
                    Path::new("/tmp/downloads/reference.fa.zst")
                );
                assert_eq!(import.object_type(), ObjectType::EnaSra);
                assert_eq!(
                    import.destination(),
                    Path::new("/mnt/disk-a/objects/reference.fa.zst")
                );
                assert_eq!(import.expected_sha256(), "abc123");
                assert_eq!(
                    import.source_uri(),
                    Some("https://example.invalid/reference.fa.zst")
                );
                assert_eq!(
                    import.policy_file(),
                    Path::new("/tmp/reproducible-cache.json")
                );
                assert!(import.allow_direct_to_hdd_import());
                assert_eq!(import.confirm(), "confirm direct-to-hdd import");
                assert!(import.json());
            }
            _ => panic!("expected direct-import command"),
        }
    }

    #[test]
    fn parses_store_adopt() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "adopt",
            "--ssd-root",
            "/srv/dasobjectstore/ssd",
            "--json",
        ])
        .expect("store adopt parses");

        let Some(Command::Store(args)) = cli.command() else {
            panic!("expected store command");
        };
        match args.command() {
            Some(StoreCommand::Adopt(adopt)) => {
                assert_eq!(adopt.ssd_root(), Some(Path::new("/srv/dasobjectstore/ssd")));
                assert!(adopt.json());
                assert_eq!(adopt.registry_path(), None);
            }
            _ => panic!("expected adopt command"),
        }
    }

    #[test]
    fn parses_store_create() {
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
            "--writer-group",
            "mnemosyne",
            "--ssd-root",
            "/srv/dasobjectstore/ssd",
            "--json",
        ])
        .expect("store create parses");

        let Some(Command::Store(args)) = cli.command() else {
            panic!("expected store command");
        };
        match args.command() {
            Some(StoreCommand::Create(create)) => {
                assert_eq!(create.store_id().as_str(), "generated-data");
                assert_eq!(create.class(), StoreClass::GeneratedData);
                assert_eq!(create.copies(), Some(2));
                assert_eq!(create.bucket(), Some("generated-data"));
                assert_eq!(create.writer_group(), Some("mnemosyne"));
                assert_eq!(
                    create.ssd_root(),
                    Some(Path::new("/srv/dasobjectstore/ssd"))
                );
                assert!(create.json());
                assert_eq!(create.registry_path(), None);
            }
            _ => panic!("expected create command"),
        }
    }

    #[test]
    fn parses_store_drain() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "drain",
            "generated-data",
            "--hdd-root",
            "/srv/dasobjectstore/hdd",
            "--allow-store-drain",
            "--confirm",
            "confirm store drain",
            "--json",
        ])
        .expect("store drain parses");

        let Some(Command::Store(args)) = cli.command() else {
            panic!("expected store command");
        };
        match args.command() {
            Some(StoreCommand::Drain(drain)) => {
                assert_eq!(drain.store_id().as_str(), "generated-data");
                assert_eq!(drain.live_sqlite_path(), None);
                assert_eq!(drain.hdd_root(), Some(Path::new("/srv/dasobjectstore/hdd")));
                assert!(drain.allow_store_drain());
                assert_eq!(drain.confirm(), "confirm store drain");
                assert!(drain.json());
            }
            _ => panic!("expected drain command"),
        }
    }

    #[test]
    fn parses_store_delete() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "delete",
            "generated-data",
            "--live-sqlite-path",
            "/srv/dasobjectstore/ssd/.dasobjectstore/live.sqlite",
            "--hdd-root",
            "/srv/dasobjectstore/hdd",
            "--ssd-root",
            "/srv/dasobjectstore/ssd",
            "--allow-store-delete",
            "--confirm",
            "confirm store delete",
            "--dry-run",
        ])
        .expect("store delete parses");

        let Some(Command::Store(args)) = cli.command() else {
            panic!("expected store command");
        };
        match args.command() {
            Some(StoreCommand::Delete(delete)) => {
                assert_eq!(delete.store_id().as_str(), "generated-data");
                assert_eq!(
                    delete.live_sqlite_path(),
                    Path::new("/srv/dasobjectstore/ssd/.dasobjectstore/live.sqlite")
                );
                assert_eq!(
                    delete.hdd_root(),
                    Some(Path::new("/srv/dasobjectstore/hdd"))
                );
                assert_eq!(
                    delete.ssd_root(),
                    Some(Path::new("/srv/dasobjectstore/ssd"))
                );
                assert!(delete.allow_store_delete());
                assert_eq!(delete.confirm(), "confirm store delete");
                assert!(delete.dry_run());
            }
            _ => panic!("expected delete command"),
        }
    }

    #[test]
    fn parses_store_list() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "list",
            "--portable",
            "--ssd-root",
            "/srv/dasobjectstore/ssd",
            "--json",
        ])
        .expect("store list parses");

        let Some(Command::Store(args)) = cli.command() else {
            panic!("expected store command");
        };
        match args.command() {
            Some(StoreCommand::List(list)) => {
                assert!(list.json());
                assert!(list.portable());
                assert_eq!(list.ssd_root(), Some(Path::new("/srv/dasobjectstore/ssd")));
                assert_eq!(list.registry_path(), None);
            }
            _ => panic!("expected list command"),
        }
    }

    #[test]
    fn parses_store_contents_tree_with_filter() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "contents",
            "generated-data",
            "--tree",
            "--depth",
            "3",
            "--filter",
            r"\.pod5$",
            "--live-sqlite-path",
            "/srv/dasobjectstore/ssd/.dasobjectstore/live.sqlite",
        ])
        .expect("store contents parses");

        let Some(Command::Store(args)) = cli.command() else {
            panic!("expected store command");
        };
        match args.command() {
            Some(StoreCommand::Contents(contents)) => {
                assert_eq!(contents.store_id().as_str(), "generated-data");
                assert!(!contents.du());
                assert!(contents.tree());
                assert_eq!(contents.depth(), 3);
                assert_eq!(contents.filter(), Some(r"\.pod5$"));
                assert_eq!(
                    contents.live_sqlite_path(),
                    Some(Path::new(
                        "/srv/dasobjectstore/ssd/.dasobjectstore/live.sqlite"
                    ))
                );
            }
            _ => panic!("expected contents command"),
        }
    }

    #[test]
    fn parses_store_s3_upload() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "s3-upload",
            "generated-data",
            "--endpoint-url",
            "https://dos.example.test:3900",
            "--region",
            "garage",
            "--profile",
            "generated",
            "--auth",
            "local-password",
            "--username",
            "alice",
            "--json",
        ])
        .expect("store s3-upload parses");

        let Some(Command::Store(args)) = cli.command() else {
            panic!("expected store command");
        };
        match args.command() {
            Some(StoreCommand::S3Upload(upload)) => {
                assert_eq!(upload.store_id().as_str(), "generated-data");
                assert_eq!(upload.endpoint_url(), "https://dos.example.test:3900");
                assert_eq!(upload.bucket(), None);
                assert_eq!(upload.region(), "garage");
                assert_eq!(upload.profile(), Some("generated"));
                assert_eq!(upload.auth(), StoreS3UploadAuth::LocalPassword);
                assert_eq!(upload.username(), Some("alice"));
                assert!(upload.json());
                assert_eq!(upload.registry_path(), None);
            }
            _ => panic!("expected s3-upload command"),
        }
    }

    #[test]
    fn parses_store_validate_policy_file() {
        let cli = Cli::try_parse_from(["dasobjectstore", "store", "validate", "/tmp/policy.json"])
            .expect("store validate parses");

        let Some(Command::Store(args)) = cli.command() else {
            panic!("expected store command");
        };
        match args.command() {
            Some(StoreCommand::Validate(validate)) => {
                assert_eq!(validate.policy_file(), Path::new("/tmp/policy.json"));
            }
            _ => panic!("expected validate command"),
        }
    }

    #[test]
    fn parses_store_defaults_class() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "defaults",
            "--class",
            "critical_metadata",
        ])
        .expect("store defaults parses");

        let Some(Command::Store(args)) = cli.command() else {
            panic!("expected store command");
        };
        match args.command() {
            Some(StoreCommand::Defaults(defaults)) => {
                assert_eq!(defaults.class(), StoreClass::CriticalMetadata);
            }
            _ => panic!("expected defaults command"),
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
