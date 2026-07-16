use clap::{Args, Subcommand, ValueEnum};
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_core::store::StoreClass;
use dasobjectstore_object_service::RemoteS3AuthAuthority;
use std::path::{Path, PathBuf};
use std::str::FromStr;

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
    /// Register a daemon-owned folder or drive profile binding from a manifest.
    #[command(name = "profile-binding")]
    ProfileBinding(StoreProfileBindingArgs),
    /// Promote all catalogued objects between daemon-owned folder profiles.
    #[command(name = "profile-migrate")]
    ProfileMigrate(StoreProfileMigrationArgs),
    /// Inspect a daemon-owned profile without exposing host paths.
    #[command(name = "profile-inspection")]
    ProfileInspection(StoreProfileInspectionArgs),
    /// Browse a bounded folder profile through the daemon-owned catalogue.
    #[command(name = "profile-browser")]
    ProfileBrowser(StoreProfileBrowserArgs),
    /// Inspect one catalogue-authoritative profile object without reading payload bytes.
    #[command(name = "profile-head")]
    ProfileHead(StoreProfileHeadArgs),
    /// Verify one catalogue-authoritative profile object against its payload.
    #[command(name = "profile-verify")]
    ProfileVerify(StoreProfileHeadArgs),
    /// Inspect provider-neutral health for a bounded profile.
    #[command(name = "profile-health")]
    ProfileHealth(StoreProfileHealthArgs),
    /// Report whether a bounded profile is ready for daemon-owned use.
    #[command(name = "profile-readiness")]
    ProfileReadiness(StoreProfileReadinessArgs),
    /// Compare profile catalogue authority with backend payload enumeration.
    #[command(name = "profile-diagnostics")]
    ProfileDiagnostics(StoreProfileReadinessArgs),
    /// Render a validated per-user macOS launchd service plan without installing it.
    #[command(name = "user-service-plan")]
    UserServicePlan(StoreUserServicePlanArgs),
    /// Inspect objects and aggregate folder sizes in a store.
    #[command(alias = "objects", alias = "list-contents")]
    Contents(StoreContentsArgs),
    /// Create or update a system-managed object store.
    Create(StoreCreateArgs),
    /// Delete all objects and payload files in a store.
    Drain(StoreDrainArgs),
    /// Delete a drained object store and its registry entries.
    Delete(StoreDeleteArgs),
    /// Verify and, with explicit confirmation, rebuild live metadata from landed payloads.
    Repair(StoreRepairArgs),
    /// Check live metadata and managed payload health without mutating state.
    Verify(StoreVerifyArgs),
    /// Find checksum-identical placement rows; apply only removes duplicate metadata rows.
    Deduplicate(StoreDeduplicateArgs),
    /// Emit the built-in JSON policy defaults for a store class.
    Defaults(StoreDefaultsArgs),
    /// Show daemon-owned folder, drive, and appliance capability contracts.
    Capabilities(StoreCapabilitiesArgs),
    /// Show daemon-owned live logical and physical capacity state.
    Capacity(StoreCapacityArgs),
    /// List system-managed object stores.
    List(StoreListArgs),
    /// Inspect or update the daemon-owned store ingest landing policy.
    IngestPolicy(StoreIngestPolicyArgs),
    /// Render AWS CLI commands for remote S3-compatible uploads.
    S3Upload(StoreS3UploadArgs),
    /// Validate a JSON store policy file.
    Validate(StoreValidateArgs),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum StoreProfileBindingOperation {
    Create,
    Provision,
    Adopt,
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct StoreProfileBindingArgs {
    /// JSON file containing a versioned ObjectStore manifest.
    #[arg(long)]
    manifest: PathBuf,
    /// Daemon-visible canonical backend root for this profile.
    #[arg(long)]
    backend_root: PathBuf,
    /// Logical ObjectStore capacity in bytes; required for folder/drive profiles.
    #[arg(long)]
    capacity_limit_bytes: Option<u64>,
    /// Reserved backend bytes excluded from logical admission.
    #[arg(long, default_value_t = 0)]
    backend_reserve_bytes: u64,
    /// Optional daemon-visible SSD staging root for external ingress.
    #[arg(long)]
    ssd_staging_root: Option<PathBuf>,
    /// Explicit profile lifecycle operation.
    #[arg(long, value_enum, default_value_t = StoreProfileBindingOperation::Create)]
    operation: StoreProfileBindingOperation,
    /// Perform validation without writing the daemon registry.
    #[arg(long)]
    dry_run: bool,
    /// Required confirmation marker for the daemon mutation.
    #[arg(long, default_value = "confirm profile binding")]
    confirm: String,
    /// Emit the daemon response as JSON.
    #[arg(long)]
    json: bool,
}

impl StoreProfileBindingArgs {
    pub(crate) fn manifest(&self) -> &Path {
        &self.manifest
    }
    pub(crate) fn backend_root(&self) -> &Path {
        &self.backend_root
    }
    pub(crate) fn capacity_limit_bytes(&self) -> Option<u64> {
        self.capacity_limit_bytes
    }
    pub(crate) fn backend_reserve_bytes(&self) -> u64 {
        self.backend_reserve_bytes
    }
    pub(crate) fn ssd_staging_root(&self) -> Option<&Path> {
        self.ssd_staging_root.as_deref()
    }
    pub(crate) fn operation(&self) -> StoreProfileBindingOperation {
        self.operation
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

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct StoreProfileMigrationArgs {
    /// Stable replay-safe migration transaction identifier.
    #[arg(long)]
    migration_id: String,
    /// Source ObjectStore identifier. Source data is always retained.
    #[arg(long)]
    source_store_id: String,
    /// Destination ObjectStore identifier.
    #[arg(long)]
    destination_store_id: String,
    /// Required confirmation marker for the daemon mutation.
    #[arg(long, default_value = "confirm profile migration")]
    confirm: String,
    /// Emit the daemon response as JSON.
    #[arg(long)]
    json: bool,
}

impl StoreProfileMigrationArgs {
    pub(crate) fn migration_id(&self) -> &str {
        &self.migration_id
    }
    pub(crate) fn source_store_id(&self) -> &str {
        &self.source_store_id
    }
    pub(crate) fn destination_store_id(&self) -> &str {
        &self.destination_store_id
    }
    pub(crate) fn confirm(&self) -> &str {
        &self.confirm
    }
    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct StoreProfileInspectionArgs {
    /// Logical ObjectStore identifier.
    store_id: String,
    /// Emit the redacted inspection response as JSON.
    #[arg(long)]
    json: bool,
}

impl StoreProfileInspectionArgs {
    pub(crate) fn store_id(&self) -> &str {
        &self.store_id
    }
    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct StoreProfileBrowserArgs {
    /// Logical ObjectStore identifier.
    store_id: String,
    /// Restrict results to keys below this logical prefix.
    #[arg(long)]
    prefix: Option<String>,
    /// Restrict results to keys containing this text.
    #[arg(long)]
    search: Option<String>,
    /// Zero-based result offset.
    #[arg(long, default_value_t = 0)]
    offset: u64,
    /// Maximum entries to return (bounded by the daemon contract).
    #[arg(long, default_value_t = 100)]
    limit: u16,
    /// Emit the typed response as JSON.
    #[arg(long)]
    json: bool,
}

impl StoreProfileBrowserArgs {
    pub(crate) fn store_id(&self) -> &str {
        &self.store_id
    }
    pub(crate) fn prefix(&self) -> Option<&str> {
        self.prefix.as_deref()
    }
    pub(crate) fn search(&self) -> Option<&str> {
        self.search.as_deref()
    }
    pub(crate) fn offset(&self) -> u64 {
        self.offset
    }
    pub(crate) fn limit(&self) -> u16 {
        self.limit
    }
    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct StoreProfileHeadArgs {
    /// Logical ObjectStore identifier.
    store_id: String,
    /// Relative logical object key.
    key: String,
    /// Object version to inspect.
    #[arg(long, default_value_t = 1)]
    version: u64,
    /// Emit the typed response as JSON.
    #[arg(long)]
    json: bool,
}

impl StoreProfileHeadArgs {
    pub(crate) fn store_id(&self) -> &str {
        &self.store_id
    }
    pub(crate) fn key(&self) -> &str {
        &self.key
    }
    pub(crate) fn version(&self) -> u64 {
        self.version
    }
    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct StoreProfileHealthArgs {
    /// Logical ObjectStore identifier.
    store_id: String,
    /// Emit the typed response as JSON.
    #[arg(long)]
    json: bool,
}

impl StoreProfileHealthArgs {
    pub(crate) fn store_id(&self) -> &str {
        &self.store_id
    }
    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct StoreProfileReadinessArgs {
    /// Logical ObjectStore identifier.
    store_id: String,
    /// Emit the typed response as JSON.
    #[arg(long)]
    json: bool,
}

impl StoreProfileReadinessArgs {
    pub(crate) fn store_id(&self) -> &str {
        &self.store_id
    }

    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct StoreUserServicePlanArgs {
    /// Absolute dasobjectstored executable path.
    #[arg(long)]
    executable: PathBuf,
    /// Absolute daemon configuration path.
    #[arg(long)]
    config: PathBuf,
    /// Per-user home directory; defaults to HOME.
    #[arg(long)]
    home: Option<PathBuf>,
    /// XDG state home; defaults to HOME/.local/state.
    #[arg(long)]
    state_home: Option<PathBuf>,
    /// XDG runtime directory; omitted when unavailable.
    #[arg(long)]
    runtime_home: Option<PathBuf>,
    /// launchd label.
    #[arg(long, default_value = "org.dasobjectstore.dasobjectstored")]
    label: String,
    /// Emit a JSON envelope containing the rendered plist.
    #[arg(long)]
    json: bool,
}

impl StoreUserServicePlanArgs {
    pub(crate) fn executable(&self) -> &Path {
        &self.executable
    }
    pub(crate) fn config(&self) -> &Path {
        &self.config
    }
    pub(crate) fn home(&self) -> Option<&Path> {
        self.home.as_deref()
    }
    pub(crate) fn state_home(&self) -> Option<&Path> {
        self.state_home.as_deref()
    }
    pub(crate) fn runtime_home(&self) -> Option<&Path> {
        self.runtime_home.as_deref()
    }
    pub(crate) fn label(&self) -> &str {
        &self.label
    }
    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct StoreCapabilitiesArgs {
    /// Emit the capability catalogue as JSON.
    #[arg(long)]
    json: bool,
}

impl StoreCapabilitiesArgs {
    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct StoreCapacityArgs {
    /// ObjectStore identifier to inspect.
    store_id: StoreId,
    /// Emit the capacity status as JSON.
    #[arg(long)]
    json: bool,
}

impl StoreCapacityArgs {
    pub(crate) fn store_id(&self) -> &StoreId {
        &self.store_id
    }

    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct StoreRepairArgs {
    /// Target one ObjectStore; a retired folder profile previews or applies reactivation.
    store_id: Option<StoreId>,
    /// Apply the reconstructed metadata. Without this flag the command is read-only.
    #[arg(long)]
    apply: bool,
    /// Required with --apply: "confirm store repair".
    #[arg(long, default_value = "")]
    confirm: String,
    /// Emit the repair report as JSON.
    #[arg(long)]
    json: bool,
    /// Retrieve uncatalogued objects from this store's Garage bucket through SSD staging before repairing metadata.
    #[arg(long)]
    reconcile_s3: bool,
    /// Limit Garage reconciliation to an object-key prefix.
    #[arg(long)]
    s3_prefix: Option<String>,
}
impl StoreRepairArgs {
    pub(crate) fn store_id(&self) -> Option<&StoreId> {
        self.store_id.as_ref()
    }
    pub(crate) fn apply(&self) -> bool {
        self.apply
    }
    pub(crate) fn confirm(&self) -> &str {
        &self.confirm
    }
    pub(crate) fn json(&self) -> bool {
        self.json
    }
    pub(crate) fn reconcile_s3(&self) -> bool {
        self.reconcile_s3
    }
    pub(crate) fn s3_prefix(&self) -> Option<&str> {
        self.s3_prefix.as_deref()
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct StoreVerifyArgs {
    /// Limit the scan to one ObjectStore; omit to inspect all registered stores.
    store_id: Option<StoreId>,
    /// Hash every landed payload and compare it with metadata.
    #[arg(long)]
    hash: bool,
    /// Emit the verification report as JSON.
    #[arg(long)]
    json: bool,
}
impl StoreVerifyArgs {
    pub(crate) fn store_id(&self) -> Option<&StoreId> {
        self.store_id.as_ref()
    }
    pub(crate) fn hash(&self) -> bool {
        self.hash
    }
    pub(crate) fn json(&self) -> bool {
        self.json
    }
}

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct StoreDeduplicateArgs {
    /// Limit the scan to one ObjectStore; omit to inspect all registered stores.
    store_id: Option<StoreId>,
    /// Remove duplicate metadata rows after recording verified hashes.
    #[arg(long)]
    apply: bool,
    /// Required with --apply: "confirm store deduplicate".
    #[arg(long, default_value = "")]
    confirm: String,
    /// Emit the deduplication report as JSON.
    #[arg(long)]
    json: bool,
}
impl StoreDeduplicateArgs {
    pub(crate) fn store_id(&self) -> Option<&StoreId> {
        self.store_id.as_ref()
    }
    pub(crate) fn apply(&self) -> bool {
        self.apply
    }
    pub(crate) fn confirm(&self) -> &str {
        &self.confirm
    }
    pub(crate) fn json(&self) -> bool {
        self.json
    }
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
    /// Store identifier, optionally followed by a slash-delimited folder/file prefix.
    target: StoreContentsTarget,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StoreContentsTarget {
    store_id: StoreId,
    prefix: Option<String>,
}

impl FromStr for StoreContentsTarget {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let value = value.trim_matches('/');
        let (store, prefix) = value
            .split_once('/')
            .map_or((value, None), |(store, prefix)| {
                (store, Some(prefix.trim_matches('/').to_string()))
            });
        if store.is_empty() {
            return Err("store target must include a store identifier".to_string());
        }
        Ok(Self {
            store_id: StoreId::new(store).map_err(|error| error.to_string())?,
            prefix: prefix.filter(|prefix| !prefix.is_empty()),
        })
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
    /// Store identifier to delete, or source-preservingly retire for a profile store.
    store_id: StoreId,
    /// Preview deletion or profile retirement without changing durable state.
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
}
impl StoreDeleteArgs {
    pub(crate) fn store_id(&self) -> &StoreId {
        &self.store_id
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
        &self.target.store_id
    }
    pub(crate) fn prefix(&self) -> Option<&str> {
        self.target.prefix.as_deref()
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
    use super::{StoreCommand, StoreIngestMode, StoreProfileBindingOperation, StoreS3UploadAuth};
    use crate::cli::{Cli, Command};
    use clap::Parser;
    use dasobjectstore_core::store::StoreClass;
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

    #[test]
    fn parses_profile_capabilities_json_flag() {
        let cli = Cli::try_parse_from(["dasobjectstore", "store", "capabilities", "--json"])
            .expect("store capabilities parses");
        let Some(Command::Store(args)) = cli.command() else {
            panic!("expected store command")
        };
        let Some(StoreCommand::Capabilities(capabilities)) = args.command() else {
            panic!("expected capabilities command")
        };
        assert!(capabilities.json());
    }

    #[test]
    fn parses_store_capacity_json_flag() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "capacity",
            "generated-data",
            "--json",
        ])
        .expect("store capacity parses");
        let Some(Command::Store(args)) = cli.command() else {
            panic!("expected store command")
        };
        let Some(StoreCommand::Capacity(capacity)) = args.command() else {
            panic!("expected capacity command")
        };
        assert_eq!(capacity.store_id().as_str(), "generated-data");
        assert!(capacity.json());
    }

    #[test]
    fn parses_profile_binding_create_request() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "profile-binding",
            "--manifest",
            "/tmp/manifest.json",
            "--backend-root",
            "/tmp/store",
            "--operation",
            "adopt",
            "--dry-run",
            "--json",
        ])
        .expect("profile binding parses");
        let Some(Command::Store(args)) = cli.command() else {
            panic!("expected store command")
        };
        let Some(StoreCommand::ProfileBinding(binding)) = args.command() else {
            panic!("expected profile binding command")
        };
        assert_eq!(binding.manifest(), Path::new("/tmp/manifest.json"));
        assert_eq!(binding.backend_root(), Path::new("/tmp/store"));
        assert_eq!(binding.operation(), StoreProfileBindingOperation::Adopt);
        assert!(binding.dry_run());
        assert!(binding.json());
    }

    #[test]
    fn parses_path_free_profile_migration_request() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "profile-migrate",
            "--migration-id",
            "promotion-1",
            "--source-store-id",
            "source-store",
            "--destination-store-id",
            "destination-store",
            "--json",
        ])
        .expect("profile migration parses");
        let Some(Command::Store(args)) = cli.command() else {
            panic!("expected store command")
        };
        let Some(StoreCommand::ProfileMigrate(migration)) = args.command() else {
            panic!("expected profile migration command")
        };
        assert_eq!(migration.migration_id(), "promotion-1");
        assert_eq!(migration.source_store_id(), "source-store");
        assert_eq!(migration.destination_store_id(), "destination-store");
        assert!(migration.json());
    }

    #[test]
    fn parses_profile_binding_provision_operation() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "profile-binding",
            "--manifest",
            "/tmp/manifest.json",
            "--backend-root",
            "/tmp/store",
            "--operation",
            "provision",
        ])
        .expect("profile binding provisioning parses");
        let Some(Command::Store(args)) = cli.command() else {
            panic!("expected store command")
        };
        let Some(StoreCommand::ProfileBinding(binding)) = args.command() else {
            panic!("expected profile binding command")
        };
        assert_eq!(binding.operation(), StoreProfileBindingOperation::Provision);
    }

    #[test]
    fn parses_profile_inspection_request() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "profile-inspection",
            "generated-data",
            "--json",
        ])
        .expect("profile inspection parses");
        let Some(Command::Store(args)) = cli.command() else {
            panic!("expected store command")
        };
        let Some(StoreCommand::ProfileInspection(inspection)) = args.command() else {
            panic!("expected profile inspection command")
        };
        assert_eq!(inspection.store_id(), "generated-data");
        assert!(inspection.json());
    }

    #[test]
    fn parses_profile_browser_request() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "profile-browser",
            "generated-data",
            "--prefix",
            "reads/",
            "--search",
            "sample",
            "--offset",
            "10",
            "--limit",
            "25",
            "--json",
        ])
        .expect("profile browser parses");
        let Some(Command::Store(args)) = cli.command() else {
            panic!("expected store command")
        };
        let Some(StoreCommand::ProfileBrowser(browser)) = args.command() else {
            panic!("expected profile browser command")
        };
        assert_eq!(browser.store_id(), "generated-data");
        assert_eq!(browser.prefix(), Some("reads/"));
        assert_eq!(browser.search(), Some("sample"));
        assert_eq!(browser.offset(), 10);
        assert_eq!(browser.limit(), 25);
        assert!(browser.json());
    }

    #[test]
    fn parses_profile_head_request() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "profile-head",
            "generated-data",
            "reads/sample.fastq",
            "--version",
            "3",
            "--json",
        ])
        .expect("profile head parses");
        let Some(Command::Store(args)) = cli.command() else {
            panic!("expected store command")
        };
        let Some(StoreCommand::ProfileHead(head)) = args.command() else {
            panic!("expected profile head command")
        };
        assert_eq!(head.store_id(), "generated-data");
        assert_eq!(head.key(), "reads/sample.fastq");
        assert_eq!(head.version(), 3);
        assert!(head.json());
    }

    #[test]
    fn parses_profile_health_request() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "profile-health",
            "generated-data",
            "--json",
        ])
        .expect("profile health parses");
        let Some(Command::Store(args)) = cli.command() else {
            panic!("expected store command")
        };
        let Some(StoreCommand::ProfileHealth(health)) = args.command() else {
            panic!("expected profile health command")
        };
        assert_eq!(health.store_id(), "generated-data");
        assert!(health.json());
    }

    #[test]
    fn parses_profile_verify_request() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "profile-verify",
            "generated-data",
            "reads/sample.fastq",
            "--version",
            "2",
            "--json",
        ])
        .expect("profile verify parses");
        let Some(Command::Store(args)) = cli.command() else {
            panic!("expected store command")
        };
        let Some(StoreCommand::ProfileVerify(verify)) = args.command() else {
            panic!("expected profile verify command")
        };
        assert_eq!(verify.store_id(), "generated-data");
        assert_eq!(verify.key(), "reads/sample.fastq");
        assert_eq!(verify.version(), 2);
        assert!(verify.json());
    }

    #[test]
    fn parses_profile_readiness_request() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "profile-readiness",
            "generated-data",
            "--json",
        ])
        .expect("profile readiness parses");
        let Some(Command::Store(args)) = cli.command() else {
            panic!("expected store command")
        };
        let Some(StoreCommand::ProfileReadiness(readiness)) = args.command() else {
            panic!("expected profile readiness command")
        };
        assert_eq!(readiness.store_id(), "generated-data");
        assert!(readiness.json());
    }

    #[test]
    fn parses_profile_diagnostics_request() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "profile-diagnostics",
            "generated-data",
            "--json",
        ])
        .expect("profile diagnostics parses");
        let Some(Command::Store(args)) = cli.command() else {
            panic!("expected store command")
        };
        let Some(StoreCommand::ProfileDiagnostics(diagnostics)) = args.command() else {
            panic!("expected profile diagnostics command")
        };
        assert_eq!(diagnostics.store_id(), "generated-data");
        assert!(diagnostics.json());
    }

    #[test]
    fn parses_user_service_plan_request() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "user-service-plan",
            "--executable",
            "/Users/tester/bin/dasobjectstored",
            "--config",
            "/Users/tester/Library/Config/dasobjectstore.json",
            "--home",
            "/Users/tester",
            "--state-home",
            "/Users/tester/Library/State",
            "--runtime-home",
            "/tmp/tester-runtime",
            "--label",
            "org.example.dasobjectstored",
            "--json",
        ])
        .expect("user service plan parses");
        let Some(Command::Store(args)) = cli.command() else {
            panic!("expected store command")
        };
        let Some(StoreCommand::UserServicePlan(plan)) = args.command() else {
            panic!("expected user service plan command")
        };
        assert_eq!(
            plan.executable(),
            Path::new("/Users/tester/bin/dasobjectstored")
        );
        assert_eq!(
            plan.config(),
            Path::new("/Users/tester/Library/Config/dasobjectstore.json")
        );
        assert_eq!(plan.home(), Some(Path::new("/Users/tester")));
        assert_eq!(
            plan.state_home(),
            Some(Path::new("/Users/tester/Library/State"))
        );
        assert_eq!(plan.runtime_home(), Some(Path::new("/tmp/tester-runtime")));
        assert_eq!(plan.label(), "org.example.dasobjectstored");
        assert!(plan.json());
    }

    #[test]
    fn parses_object_store_contents_aliases() {
        for alias in ["objects", "list-contents"] {
            let cli = Cli::try_parse_from(["dasobjectstore", "store", alias, "zymo"])
                .expect("contents alias parses");
            let Some(Command::Store(args)) = cli.command() else {
                panic!("expected store command")
            };
            assert!(matches!(args.command(), Some(StoreCommand::Contents(_))));
        }
    }

    #[test]
    fn parses_store_contents_folder_target() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "contents",
            "xenognostikon/PRJEB33511",
        ])
        .expect("parse contents target");
        let Some(Command::Store(args)) = cli.command() else {
            panic!("store command");
        };
        let Some(StoreCommand::Contents(args)) = args.command() else {
            panic!("contents command");
        };
        assert_eq!(args.store_id().as_str(), "xenognostikon");
        assert_eq!(args.prefix(), Some("PRJEB33511"));
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
            "--reader-group",
            "mnemosyne-readers",
            "--writer-group",
            "mnemosyne",
            "--public",
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
                assert_eq!(create.reader_group(), Some("mnemosyne-readers"));
                assert_eq!(create.writer_group(), Some("mnemosyne"));
                assert!(create.public());
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
                assert!(delete.allow_store_delete());
                assert_eq!(delete.confirm(), "confirm store delete");
                assert!(delete.dry_run());
            }
            _ => panic!("expected delete command"),
        }
    }

    #[test]
    fn parses_store_repair_dry_run_and_apply_confirmation() {
        let cli = Cli::try_parse_from([
            "dasobjectstore",
            "store",
            "repair",
            "xenognostikon",
            "--apply",
            "--confirm",
            "confirm store repair",
            "--json",
            "--reconcile-s3",
            "--s3-prefix",
            "variants/chm13",
        ])
        .expect("store repair parses");
        let Some(Command::Store(args)) = cli.command() else {
            panic!("expected store command");
        };
        let Some(StoreCommand::Repair(args)) = args.command() else {
            panic!("expected repair command");
        };
        assert!(args.apply());
        assert_eq!(args.confirm(), "confirm store repair");
        assert!(args.json());
        assert!(args.reconcile_s3());
        assert_eq!(args.s3_prefix(), Some("variants/chm13"));
        assert_eq!(args.store_id().expect("store id").as_str(), "xenognostikon");
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
}
