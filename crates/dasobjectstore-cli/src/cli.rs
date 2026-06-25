use clap::{Args, Parser, Subcommand};
#[cfg(feature = "debug-commands")]
use dasobjectstore_core::ids::PoolId;
use dasobjectstore_core::ids::{DiskId, ObjectId};
use dasobjectstore_core::store::StoreClass;
use dasobjectstore_object_service::ObjectServiceProviderId;
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
}

#[derive(Debug, Eq, PartialEq, Subcommand)]
pub(crate) enum Command {
    /// Inspect candidate DAS disks and enclosures.
    Probe(ProbeArgs),
    /// Report pool, disk, and service health.
    Health(HealthArgs),
    /// Manage portable storage pools.
    Pool(PoolArgs),
    /// Manage DAS member disks.
    Disk(DiskArgs),
    /// Manage object stores and policy.
    Store(StoreArgs),
    /// Inspect SSD ingest and destage work.
    Ingest(IngestArgs),
    /// Inspect object metadata.
    Object(ObjectArgs),
    /// Render and manage the S3-compatible object service.
    Service(ServiceArgs),
    /// Export Mnemosyne/Synoptikon integration metadata.
    Mnemosyne,
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct HealthArgs {
    /// Emit one-line pool and disk health summary.
    #[arg(long)]
    summary: bool,
    /// Emit per-disk health details.
    #[arg(long)]
    verbose: bool,
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
    /// Emit the built-in JSON policy defaults for a store class.
    Defaults(StoreDefaultsArgs),
    /// Validate a JSON store policy file.
    Validate(StoreValidateArgs),
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
    /// Report SSD ingest capacity and pressure state.
    Status(IngestStatusArgs),
    /// Emit live ingest queue entries as JSON.
    Queue(IngestQueueArgs),
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
    /// Path to live.sqlite for the pool.
    #[arg(long)]
    live_sqlite_path: PathBuf,
    /// Emit queue entries as JSON.
    #[arg(long)]
    json: bool,
}

impl IngestQueueArgs {
    pub(crate) fn live_sqlite_path(&self) -> &Path {
        &self.live_sqlite_path
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
    /// Path to a JSON array of store service definitions.
    #[arg(long)]
    stores_file: PathBuf,
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
    pub(crate) fn stores_file(&self) -> &Path {
        &self.stores_file
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

#[cfg(test)]
mod tests {
    use super::{
        Cli, Command, DiskCommand, IngestArgs, IngestCommand, ObjectCommand, PoolCommand,
        ProbeArgs, ServiceCommand, StoreArgs, StoreCommand,
    };
    use clap::Parser;
    use dasobjectstore_core::store::StoreClass;
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
            ("mnemosyne", Command::Mnemosyne),
        ];

        for (name, expected) in cases {
            let cli =
                Cli::try_parse_from(["dasobjectstore", name]).expect("subcommand should parse");

            assert_eq!(cli.command(), Some(&expected));
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
        assert!(!args.json());
    }

    #[test]
    fn parses_health_output_flags() {
        let cases = [
            ("--summary", true, false, false),
            ("--verbose", false, true, false),
            ("--json", false, false, true),
        ];

        for (flag, summary, verbose, json) in cases {
            let cli =
                Cli::try_parse_from(["dasobjectstore", "health", flag]).expect("health parses");

            let Some(Command::Health(args)) = cli.command() else {
                panic!("expected health command");
            };
            assert_eq!(args.summary(), summary);
            assert_eq!(args.verbose(), verbose);
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
            "--stores-file",
            "/tmp/stores.json",
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
                assert_eq!(render.stores_file(), Path::new("/tmp/stores.json"));
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
            ObjectCommand::Export(_) => panic!("expected inspect command"),
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
            ObjectCommand::Inspect(_) => panic!("expected export command"),
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
    fn parses_ingest_queue_json() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "ingest",
            "queue",
            "--live-sqlite-path",
            "/tmp/live.sqlite",
            "--json",
        ])
        .expect("ingest queue parses");

        let Some(Command::Ingest(args)) = cli.command() else {
            panic!("expected ingest command");
        };
        match args.command() {
            Some(IngestCommand::Queue(queue)) => {
                assert_eq!(queue.live_sqlite_path(), Path::new("/tmp/live.sqlite"));
                assert!(queue.json());
            }
            _ => panic!("expected queue command"),
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
