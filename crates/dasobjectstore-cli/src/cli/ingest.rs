use clap::{Args, Subcommand};
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_core::object_type::ObjectType;
use dasobjectstore_daemon::DaemonIngestConflictPolicy;
use std::path::{Path, PathBuf};

#[derive(Debug, Eq, PartialEq, Args)]
pub(crate) struct IngestArgs {
    #[command(subcommand)]
    pub(crate) command: Option<IngestCommand>,
}

impl IngestArgs {
    pub(crate) fn command(&self) -> Option<&IngestCommand> {
        self.command.as_ref()
    }
}

#[derive(Debug, Eq, PartialEq, Subcommand)]
pub(crate) enum IngestCommand {
    /// Import a directory tree from a mounted disk through SSD-first ingest.
    Files(IngestFilesArgs),
    /// Report SSD ingest capacity and pressure state.
    Status(IngestStatusArgs),
    /// Inspect live ingest queue entries for a store.
    Queue(IngestQueueArgs),
    /// Cancel active queued ingest jobs for a store.
    DrainQueue(IngestDrainQueueArgs),
    /// Request a policy-gated direct-to-HDD import from the DAS server.
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
    /// HDD settlement worker count; defaults to up to four concurrent distinct
    /// HDD target sets, bounded by the configured copy count and HDD inventory.
    #[arg(long)]
    hdd_workers: Option<usize>,
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
    pub(crate) fn hdd_workers(&self) -> Option<usize> {
        self.hdd_workers
    }
    pub(crate) fn conflict_policy(&self) -> DaemonIngestConflictPolicy {
        if self.force {
            DaemonIngestConflictPolicy::Force
        } else if self.lazy {
            DaemonIngestConflictPolicy::Lazy
        } else {
            DaemonIngestConflictPolicy::Force
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
pub(crate) struct IngestStatusArgs {
    #[arg(long)]
    ssd_root: PathBuf,
    #[arg(long, default_value_t = dasobjectstore_metadata::DEFAULT_SSD_HIGH_WATERMARK_PERCENT)]
    high_watermark_percent: u8,
    #[arg(long, default_value_t = dasobjectstore_metadata::DEFAULT_SSD_CRITICAL_WATERMARK_PERCENT)]
    critical_watermark_percent: u8,
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
    store_id: StoreId,
    #[arg(long, hide = true)]
    live_sqlite_path: Option<PathBuf>,
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
    store_id: StoreId,
    #[arg(long, hide = true)]
    live_sqlite_path: Option<PathBuf>,
    #[arg(long)]
    dry_run: bool,
    #[arg(long)]
    allow_ingest_queue_drain: bool,
    #[arg(long, default_value = "")]
    confirm: String,
    #[arg(long, default_value = "operator drained ingest queue")]
    reason: String,
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
    endpoint: StoreId,
    #[arg(long)]
    source: PathBuf,
    #[arg(long, default_value_t = ObjectType::Naive)]
    object_type: ObjectType,
    #[arg(long)]
    copies: Option<u8>,
    #[arg(long)]
    hdd_workers: Option<usize>,
    #[arg(long, conflicts_with_all = ["lazy", "force"])]
    strict: bool,
    #[arg(long, conflicts_with_all = ["strict", "force"])]
    lazy: bool,
    #[arg(long, conflicts_with_all = ["strict", "lazy"])]
    force: bool,
    #[arg(long)]
    tui: bool,
    #[arg(long)]
    dry_run: bool,
}

impl IngestDirectImportArgs {
    pub(crate) fn endpoint(&self) -> &StoreId {
        &self.endpoint
    }
    pub(crate) fn source(&self) -> &Path {
        &self.source
    }
    pub(crate) fn object_type(&self) -> ObjectType {
        self.object_type
    }
    pub(crate) fn copies(&self) -> Option<u8> {
        self.copies
    }
    pub(crate) fn hdd_workers(&self) -> Option<usize> {
        self.hdd_workers
    }
    pub(crate) fn conflict_policy(&self) -> DaemonIngestConflictPolicy {
        if self.force {
            DaemonIngestConflictPolicy::Force
        } else if self.lazy {
            DaemonIngestConflictPolicy::Lazy
        } else {
            DaemonIngestConflictPolicy::Force
        }
    }
    pub(crate) fn tui(&self) -> bool {
        self.tui
    }
    pub(crate) fn dry_run(&self) -> bool {
        self.dry_run
    }
}
