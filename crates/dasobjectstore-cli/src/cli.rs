use clap::{Args, Parser, Subcommand};
#[cfg(feature = "debug-commands")]
use dasobjectstore_core::ids::PoolId;
use dasobjectstore_core::store::StoreClass;
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
    Health,
    /// Manage portable storage pools.
    Pool(PoolArgs),
    /// Manage DAS member disks.
    Disk,
    /// Manage object stores and policy.
    Store(StoreArgs),
    /// Inspect SSD ingest and destage work.
    Ingest(IngestArgs),
    /// Export Mnemosyne/Synoptikon integration metadata.
    Mnemosyne,
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

#[cfg(test)]
mod tests {
    use super::{
        Cli, Command, IngestArgs, IngestCommand, PoolCommand, ProbeArgs, StoreArgs, StoreCommand,
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
            ("health", Command::Health),
            ("disk", Command::Disk),
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
            #[cfg(feature = "debug-commands")]
            _ => panic!("expected inspect command"),
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
