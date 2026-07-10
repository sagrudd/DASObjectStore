use clap::{Args, Subcommand};
#[cfg(feature = "debug-commands")]
use dasobjectstore_core::ids::PoolId;
use std::path::{Path, PathBuf};

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

#[cfg(test)]
mod tests {
    use super::PoolCommand;
    use crate::cli::{Cli, Command};
    use clap::Parser;
    use std::path::Path;

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
            PoolCommand::Import(_) | PoolCommand::Repair(_) => {
                panic!("expected inspect command")
            }
            #[cfg(feature = "debug-commands")]
            PoolCommand::MarkClean(_) | PoolCommand::MarkDirty(_) => {
                panic!("expected inspect command")
            }
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
            "/srv/dasobjectstore/hdd/pool-a",
            "--recovery-metadata-dir",
            "/srv/dasobjectstore/ssd/.dasobjectstore",
            "--recorded-at-utc",
            "2026-07-07T12:00:00Z",
        ])
        .expect("pool import parses");
        let Some(Command::Pool(args)) = cli.command() else {
            panic!("expected pool command");
        };
        match args.command() {
            PoolCommand::Import(import) => {
                assert!(import.read_only());
                assert_eq!(
                    import.source_path(),
                    Path::new("/srv/dasobjectstore/hdd/pool-a")
                );
                assert_eq!(
                    import.recovery_metadata_dir(),
                    Path::new("/srv/dasobjectstore/ssd/.dasobjectstore")
                );
                assert_eq!(import.recorded_at_utc(), "2026-07-07T12:00:00Z");
            }
            PoolCommand::Inspect(_) | PoolCommand::Repair(_) => {
                panic!("expected import command")
            }
            #[cfg(feature = "debug-commands")]
            PoolCommand::MarkClean(_) | PoolCommand::MarkDirty(_) => {
                panic!("expected import command")
            }
        }
    }

    #[test]
    fn parses_pool_repair_dry_run() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "pool",
            "repair",
            "--source-path",
            "/srv/dasobjectstore/hdd/pool-a",
            "--dry-run",
        ])
        .expect("pool repair parses");
        let Some(Command::Pool(args)) = cli.command() else {
            panic!("expected pool command");
        };
        match args.command() {
            PoolCommand::Repair(repair) => {
                assert_eq!(
                    repair.source_path(),
                    Path::new("/srv/dasobjectstore/hdd/pool-a")
                );
                assert!(repair.dry_run());
            }
            PoolCommand::Inspect(_) | PoolCommand::Import(_) => {
                panic!("expected repair command")
            }
            #[cfg(feature = "debug-commands")]
            PoolCommand::MarkClean(_) | PoolCommand::MarkDirty(_) => {
                panic!("expected repair command")
            }
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
            "2026-07-07T12:00:00Z",
        ])
        .expect("pool mark-clean parses");
        let Some(Command::Pool(args)) = cli.command() else {
            panic!("expected pool command");
        };
        match args.command() {
            PoolCommand::MarkClean(mark) => {
                assert_eq!(mark.live_sqlite_path(), Path::new("/tmp/live.sqlite"));
                assert_eq!(mark.pool_id().as_str(), "pool-a");
                assert_eq!(mark.recorded_at_utc(), "2026-07-07T12:00:00Z");
            }
            PoolCommand::Inspect(_)
            | PoolCommand::Import(_)
            | PoolCommand::Repair(_)
            | PoolCommand::MarkDirty(_) => panic!("expected mark-clean command"),
        }
    }
}
