use clap::{Args, Subcommand, ValueEnum};
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_core::store::StoreClass;
use dasobjectstore_object_service::RemoteS3AuthAuthority;
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
    /// Unix group allowed to browse and download objects from this store.
    #[arg(long)]
    reader_group: Option<String>,
    /// Unix group allowed to write objects to this store.
    #[arg(long)]
    writer_group: Option<String>,
    /// Allow all authenticated DASObjectStore users to browse and download this store.
    #[arg(long)]
    public: bool,
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
    pub(crate) fn reader_group(&self) -> Option<&str> {
        self.reader_group.as_deref()
    }
    pub(crate) fn writer_group(&self) -> Option<&str> {
        self.writer_group.as_deref()
    }
    pub(crate) fn public(&self) -> bool {
        self.public
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
