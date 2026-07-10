use clap::{Args, Subcommand, ValueEnum};
use dasobjectstore_core::ids::DiskId;
use std::path::{Path, PathBuf};

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

#[cfg(test)]
mod tests {
    use super::{DiskCommand, DiskPrepareFilesystem};
    use crate::cli::{Cli, Command};
    use clap::Parser;
    use std::path::Path;

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
}
