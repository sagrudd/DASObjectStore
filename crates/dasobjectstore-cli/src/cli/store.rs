use super::{
    StoreAdoptArgs, StoreCreateArgs, StoreDefaultsArgs, StoreDeleteArgs, StoreDrainArgs,
    StoreListArgs, StoreS3UploadArgs, StoreValidateArgs,
};
use clap::{Args, Subcommand, ValueEnum};
use dasobjectstore_core::ids::StoreId;
use std::path::{Path, PathBuf};

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct StoreArgs {
    #[command(subcommand)]
    pub(crate) command: Option<StoreCommand>,
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
    /// Inspect or update the daemon-owned store ingest landing policy.
    IngestPolicy(StoreIngestPolicyArgs),
    /// Render AWS CLI commands for remote S3-compatible uploads.
    S3Upload(StoreS3UploadArgs),
    /// Validate a JSON store policy file.
    Validate(StoreValidateArgs),
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct StoreIngestPolicyArgs {
    /// Store identifier whose ingest policy should be inspected or changed.
    store_id: StoreId,
    /// Requested landing mode. Omit it to inspect the current policy.
    #[arg(long, value_enum)]
    ingest_mode: Option<StoreIngestMode>,
    /// Apply no registry change; still validate policy and confirmation.
    #[arg(long)]
    dry_run: bool,
    /// Required when selecting direct-to-HDD: "confirm direct hdd ingest".
    #[arg(long, default_value = "")]
    confirm: String,
    /// Emit the daemon response as JSON.
    #[arg(long)]
    json: bool,
}
impl StoreIngestPolicyArgs {
    pub(crate) fn store_id(&self) -> &StoreId {
        &self.store_id
    }
    pub(crate) fn ingest_mode(&self) -> Option<StoreIngestMode> {
        self.ingest_mode
    }
    pub(crate) fn dry_run(&self) -> bool {
        self.dry_run
    }
    pub(crate) fn confirm(&self) -> &str {
        &self.confirm
    }
    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum StoreIngestMode {
    SsdFirst,
    DirectToHdd,
}
impl StoreIngestMode {
    pub(crate) fn as_api_value(self) -> &'static str {
        match self {
            Self::SsdFirst => "ssd_first",
            Self::DirectToHdd => "direct_to_hdd",
        }
    }
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

#[cfg(test)]
mod tests {
    use super::{StoreCommand, StoreIngestMode};
    use crate::cli::{Cli, Command};
    use clap::Parser;
    use std::path::Path;

    #[test]
    fn parses_ingest_policy_and_contents() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "ingest-policy",
            "zymo",
            "--ingest-mode",
            "direct-to-hdd",
            "--confirm",
            "confirm direct hdd ingest",
            "--dry-run",
            "--json",
        ])
        .expect("store ingest-policy parses");
        let Some(Command::Store(args)) = cli.command() else {
            panic!("expected store command")
        };
        let Some(StoreCommand::IngestPolicy(policy)) = args.command() else {
            panic!("expected policy")
        };
        assert_eq!(policy.store_id().as_str(), "zymo");
        assert_eq!(policy.ingest_mode(), Some(StoreIngestMode::DirectToHdd));
        assert_eq!(policy.confirm(), "confirm direct hdd ingest");
        assert!(policy.dry_run());
        assert!(policy.json());

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
            panic!("expected store command")
        };
        let Some(StoreCommand::Contents(contents)) = args.command() else {
            panic!("expected contents")
        };
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
}
