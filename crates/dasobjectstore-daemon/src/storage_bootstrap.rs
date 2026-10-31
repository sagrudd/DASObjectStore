//! Root-only, one-shot first-install storage bootstrap.
//!
//! This mode runs before normal daemon composition, which is necessary because
//! the normal daemon is intentionally gated on the completed managed-storage
//! manifest and live pool metadata produced here.

use dasobjectstore_core::ids::PoolId;
use dasobjectstore_daemon::api::{
    PrepareEnclosureFilesystem, PrepareEnclosureHddDevice, PrepareEnclosureRequest,
    ENCLOSURE_PREPARE_CONFIRMATION,
};
use dasobjectstore_metadata::{initialize_pool, PoolInitOptions};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use uuid::Uuid;

const MANIFEST: &str = "/etc/dasobjectstore/managed-storage.v1.json";
const JOURNAL: &str = "/etc/dasobjectstore/storage-bootstrap.v1.json";
const LOCK: &str = "/run/lock/dasobjectstore-storage-bootstrap.lock";
const MOUNT_ROOT: &str = "/srv/dasobjectstore";
const SSD_MOUNT: &str = "/srv/dasobjectstore/ssd";
const HDD_MOUNT: &str = "/srv/dasobjectstore/hdd/disk-hdd-01";
const MINIMUM_DEVICE_BYTES: u64 = 1024 * 1024 * 1024;
const RESUME_REFORMAT_CONFIRMATION: &str = "confirm resume reformat das";

#[derive(Serialize)]
struct ManagedStorageManifest {
    schema_version: u8,
    ssd: ManagedDevice,
    hdds: Vec<ManagedDevice>,
}

#[derive(Serialize)]
struct ManagedDevice {
    path: String,
    uuid: String,
    label: String,
    device: String,
    filesystem: &'static str,
    role: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct BootstrapDeviceIdentity {
    path: String,
    major: u32,
    minor: u32,
    size_bytes: u64,
    sysfs_path: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct BootstrapJournal {
    schema_version: u8,
    ssd: BootstrapDeviceIdentity,
    hdd: BootstrapDeviceIdentity,
}

pub(super) fn run(args: &[String]) -> Result<(), String> {
    if args == ["--help"] {
        println!("Usage: dasobjectstored bootstrap-storage --ssd-device <DEVICE> --hdd-device <DEVICE> [--resume [--confirm-resume-reformat 'confirm resume reformat das']] --allow-format --acknowledge-blank-devices --confirm 'confirm bootstrap das'");
        return Ok(());
    }
    let options = BootstrapArgs::parse(args)?;
    if unsafe { libc::geteuid() } != 0 {
        return Err("bootstrap-storage must run as root".into());
    }
    if !options.allow_format
        || !options.acknowledge_blank_devices
        || options.confirmation != "confirm bootstrap das"
    {
        return Err("bootstrap-storage requires --allow-format, --acknowledge-blank-devices, and --confirm 'confirm bootstrap das'".into());
    }
    if !options.resume && options.resume_confirmation.is_some() {
        return Err("--confirm-resume-reformat is only valid with --resume".into());
    }
    let _lock = acquire_bootstrap_lock()?;
    let existing_journal = read_journal(Path::new(JOURNAL))?;
    let (ssd, hdd, resume) = match existing_journal {
        Some(journal) => {
            if !options.resume {
                return Err("a bootstrap journal exists; use --resume with the same devices and confirmations".into());
            }
            if journal.schema_version != 1 {
                return Err("unsupported storage-bootstrap journal schema".into());
            }
            let ssd = validate_journal_device(&options.ssd_device, &journal.ssd)?;
            let hdd = validate_journal_device(&options.hdd_device, &journal.hdd)?;
            if same_block_device(&ssd, &hdd)? {
                return Err("SSD and HDD devices resolve to the same block device".into());
            }
            if configured_manifest_matches(Path::new(MANIFEST), &ssd, &hdd)? {
                ensure_fstab_entries_match(&ssd, &hdd)?;
                start_storage_services()?;
                eprintln!(
                    "DASObjectStore first-install storage recovery completed; the daemon is active"
                );
                return Ok(());
            }
            if options.resume_confirmation.as_deref() != Some(RESUME_REFORMAT_CONFIRMATION) {
                return Err(format!(
                    "this interrupted bootstrap requires --confirm-resume-reformat '{RESUME_REFORMAT_CONFIRMATION}' before it can reformat the same journaled devices"
                ));
            }
            ensure_daemon_inactive()?;
            ensure_pristine_manifest(Path::new(MANIFEST))?;
            ensure_mount_paths_are_safe()?;
            ensure_recovery_mounts_belong_to_devices(&ssd, &hdd)?;
            remove_prior_bootstrap_fstab_entries(&ssd, &hdd)?;
            (ssd, hdd, true)
        }
        None => {
            if options.resume {
                return Err("--resume was supplied but no storage-bootstrap journal exists".into());
            }
            ensure_pristine_manifest(Path::new(MANIFEST))?;
            ensure_mount_targets_unused()?;
            let ssd = validate_blank_device(&options.ssd_device)?;
            let hdd = validate_blank_device(&options.hdd_device)?;
            let ssd_identity = device_identity(&ssd)?;
            let hdd_identity = device_identity(&hdd)?;
            if ssd_identity.major == hdd_identity.major && ssd_identity.minor == hdd_identity.minor
            {
                return Err("SSD and HDD devices resolve to the same block device".into());
            }
            ensure_no_existing_fstab_entries()?;
            write_journal(
                Path::new(JOURNAL),
                &BootstrapJournal {
                    schema_version: 1,
                    ssd: ssd_identity,
                    hdd: hdd_identity,
                },
            )?;
            (ssd, hdd, false)
        }
    };

    let accepted_at = super::current_utc_timestamp();
    let request = PrepareEnclosureRequest {
        ssd_device: ssd.clone(),
        hdd_devices: vec![PrepareEnclosureHddDevice {
            disk_id: "disk-hdd-01".into(),
            device_path: hdd.clone(),
        }],
        mount_root: PathBuf::from(MOUNT_ROOT),
        filesystem: PrepareEnclosureFilesystem::Ext4,
        owner: Some("dasobjectstore".into()),
        dry_run: false,
        client_request_id: Some(format!("first-install-{}", Uuid::new_v4())),
        administrator_actor: Some("root-first-install".into()),
        allow_format: true,
        existing_data_acknowledged: true,
        confirmation_marker: ENCLOSURE_PREPARE_CONFIRMATION.into(),
    };
    let journal = read_journal(Path::new(JOURNAL))?
        .ok_or_else(|| "storage-bootstrap journal disappeared before formatting".to_string())?;
    validate_journal_device(&ssd, &journal.ssd)?;
    validate_journal_device(&hdd, &journal.hdd)?;
    let expected_ssd = journal.ssd.clone();
    let expected_hdd = journal.hdd.clone();
    let prepared = dasobjectstore_daemon::runtime::bootstrap_initial_enclosure_with_guard(
        request,
        &accepted_at,
        |device_path| {
            let actual = device_identity(device_path)?;
            let expected = if device_path == Path::new(&expected_ssd.path) {
                &expected_ssd
            } else if device_path == Path::new(&expected_hdd.path) {
                &expected_hdd
            } else {
                return Err(format!(
                    "{} is not a journaled device path",
                    device_path.display()
                ));
            };
            if identity_matches(&actual, expected) {
                Ok(())
            } else {
                Err(format!(
                    "{} no longer matches its journaled role",
                    device_path.display()
                ))
            }
        },
    )?;
    let ssd_partition = partition_path(&ssd);
    let hdd_partition = partition_path(&hdd);
    let ssd_uuid = read_blkid(&ssd_partition, "UUID")?;
    let hdd_uuid = read_blkid(&hdd_partition, "UUID")?;
    require_value(
        "SSD filesystem label",
        read_blkid(&ssd_partition, "LABEL")?,
        "DOS_SSD",
    )?;
    require_value(
        "HDD filesystem label",
        read_blkid(&hdd_partition, "LABEL")?,
        "DOS_HDD_01",
    )?;
    if resume {
        remove_prior_bootstrap_fstab_entries(&ssd, &hdd)?;
    }
    append_fstab_mounts(&ssd_uuid, &hdd_uuid)?;

    let pool_id = PoolId::new(format!("pool-{}", Uuid::new_v4()))
        .map_err(|error| format!("cannot create pool identity: {error}"))?;
    initialize_pool(&PoolInitOptions::new(SSD_MOUNT, pool_id, accepted_at))
        .map_err(|error| format!("cannot initialize the new SSD pool metadata: {error}"))?;
    run_command(
        "chown",
        &[
            "-R",
            "dasobjectstore:dasobjectstore",
            &format!("{SSD_MOUNT}/.dasobjectstore"),
        ],
    )?;

    let manifest = ManagedStorageManifest {
        schema_version: 1,
        ssd: ManagedDevice {
            path: SSD_MOUNT.into(),
            uuid: ssd_uuid,
            label: "DOS_SSD".into(),
            device: ssd.display().to_string(),
            filesystem: "ext4",
            role: "ssd".into(),
        },
        hdds: vec![ManagedDevice {
            path: HDD_MOUNT.into(),
            uuid: hdd_uuid,
            label: "DOS_HDD_01".into(),
            device: hdd.display().to_string(),
            filesystem: "ext4",
            role: "hdd:disk-hdd-01".into(),
        }],
    };
    write_manifest(Path::new(MANIFEST), &manifest)?;
    start_storage_services()?;
    eprintln!(
        "DASObjectStore first-install storage is ready and the daemon is active: SSD={} HDD={} ({})",
        ssd.display(), hdd.display(), prepared.accepted.job_id
    );
    Ok(())
}

fn same_block_device(first: &Path, second: &Path) -> Result<bool, String> {
    let first = device_identity(first)?;
    let second = device_identity(second)?;
    Ok(first.major == second.major && first.minor == second.minor)
}

fn validate_blank_device(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err(format!("device path must be absolute: {}", path.display()));
    }
    let canonical = fs::canonicalize(path)
        .map_err(|error| format!("cannot resolve {}: {error}", path.display()))?;
    let size = command_output(
        "blockdev",
        &["--getsize64", canonical.to_str().unwrap_or("")],
    )?;
    let size = size
        .trim()
        .parse::<u64>()
        .map_err(|error| format!("invalid size for {}: {error}", canonical.display()))?;
    if size < MINIMUM_DEVICE_BYTES {
        return Err(format!(
            "{} is below the bootstrap minimum of 1 GiB",
            canonical.display()
        ));
    }
    let lsblk = command_output(
        "lsblk",
        &[
            "--json",
            "--paths",
            "--output",
            "NAME,TYPE,MOUNTPOINTS",
            canonical.to_str().unwrap_or(""),
        ],
    )?;
    let value: Value = serde_json::from_str(&lsblk)
        .map_err(|error| format!("cannot parse lsblk output: {error}"))?;
    let rows = value["blockdevices"]
        .as_array()
        .ok_or_else(|| "lsblk did not return a block device list".to_string())?;
    if rows.len() != 1
        || rows[0]["type"].as_str() != Some("disk")
        || rows[0]["children"].is_array()
        || rows[0]["mountpoints"]
            .as_array()
            .is_some_and(|points| points.iter().any(|point| !point.is_null()))
    {
        return Err(format!(
            "{} is not a whole, unused disk without partitions or mountpoints",
            canonical.display()
        ));
    }
    let signatures = command_output(
        "wipefs",
        &[
            "--no-act",
            "--noheadings",
            "--output",
            "TYPE",
            canonical.to_str().unwrap_or(""),
        ],
    )?;
    if !signatures.trim().is_empty() {
        return Err(format!(
            "{} contains existing filesystem or partition signatures",
            canonical.display()
        ));
    }
    Ok(canonical)
}

fn device_identity(path: &Path) -> Result<BootstrapDeviceIdentity, String> {
    let canonical = fs::canonicalize(path)
        .map_err(|error| format!("cannot resolve {}: {error}", path.display()))?;
    let metadata = fs::metadata(&canonical)
        .map_err(|error| format!("cannot inspect {}: {error}", canonical.display()))?;
    if !metadata.file_type().is_block_device() {
        return Err(format!("{} is not a block device", canonical.display()));
    }
    let device_number = metadata.rdev() as libc::dev_t;
    let major = libc::major(device_number) as u32;
    let minor = libc::minor(device_number) as u32;
    let size_bytes = command_output(
        "blockdev",
        &["--getsize64", canonical.to_str().unwrap_or("")],
    )?
    .trim()
    .parse::<u64>()
    .map_err(|error| format!("invalid size for {}: {error}", canonical.display()))?;
    let sysfs_path = fs::canonicalize(format!("/sys/dev/block/{major}:{minor}"))
        .map_err(|error| format!("cannot identify sysfs device {major}:{minor}: {error}"))?
        .to_string_lossy()
        .into_owned();
    Ok(BootstrapDeviceIdentity {
        path: canonical.display().to_string(),
        major,
        minor,
        size_bytes,
        sysfs_path,
    })
}

fn validate_journal_device(
    path: &Path,
    expected: &BootstrapDeviceIdentity,
) -> Result<PathBuf, String> {
    let actual = device_identity(path)?;
    if !identity_matches(&actual, expected) {
        return Err(format!(
            "{} no longer identifies the journaled bootstrap device; refusing resume",
            path.display()
        ));
    }
    Ok(PathBuf::from(actual.path))
}

fn identity_matches(actual: &BootstrapDeviceIdentity, expected: &BootstrapDeviceIdentity) -> bool {
    actual.path == expected.path
        && actual.major == expected.major
        && actual.minor == expected.minor
        && actual.size_bytes == expected.size_bytes
        && actual.sysfs_path == expected.sysfs_path
}

fn acquire_bootstrap_lock() -> Result<File, String> {
    let path = Path::new(LOCK);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|error| {
            format!(
                "cannot safely open bootstrap lock {}: {error}",
                path.display()
            )
        })?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.file_type().is_file()
        || metadata.uid() != 0
        || metadata.mode() & 0o077 != 0
        || metadata.nlink() != 1
    {
        return Err(
            "bootstrap lock must be a protected root-owned regular file with one link".into(),
        );
    }
    let locked = unsafe {
        libc::flock(
            std::os::fd::AsRawFd::as_raw_fd(&file),
            libc::LOCK_EX | libc::LOCK_NB,
        )
    };
    if locked != 0 {
        return Err("another storage bootstrap operation is already running".into());
    }
    Ok(file)
}

fn read_journal(path: &Path) -> Result<Option<BootstrapJournal>, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
    };
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
    if !metadata.file_type().is_file() || metadata.uid() != 0 || metadata.mode() & 0o077 != 0 {
        return Err("storage-bootstrap journal must be a protected root-owned regular file".into());
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| format!("invalid storage-bootstrap journal: {error}"))
}

fn write_journal(path: &Path, journal: &BootstrapJournal) -> Result<(), String> {
    let mut bytes = serde_json::to_vec_pretty(journal)
        .map_err(|error| format!("cannot serialize storage-bootstrap journal: {error}"))?;
    bytes.push(b'\n');
    let temporary = path.with_extension(format!("bootstrap.pending.{}", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&temporary)
        .map_err(|error| {
            format!(
                "cannot create journal staging file {}: {error}",
                temporary.display()
            )
        })?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("cannot sync journal staging file: {error}"))?;
    fs::hard_link(&temporary, path).map_err(|error| {
        format!(
            "cannot install journal {} without replacement: {error}",
            path.display()
        )
    })?;
    fs::remove_file(&temporary)
        .map_err(|error| format!("cannot remove journal staging file: {error}"))?;
    File::open(path.parent().unwrap_or_else(|| Path::new("/")))
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("cannot sync journal directory: {error}"))
}

fn configured_manifest_matches(path: &Path, ssd: &Path, hdd: &Path) -> Result<bool, String> {
    let value: Value = serde_json::from_slice(
        &fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?,
    )
    .map_err(|error| format!("invalid {}: {error}", path.display()))?;
    if value["ssd"].is_null() {
        return Ok(false);
    }
    let keys = value
        .as_object()
        .map(|object| object.keys().map(String::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    if keys != ["hdds", "schema_version", "ssd"] {
        return Err("configured storage manifest has unexpected fields".into());
    }
    let hdds = value["hdds"]
        .as_array()
        .ok_or_else(|| "configured storage manifest has no HDD list".to_string())?;
    if value["schema_version"] != 1 || hdds.len() != 1 {
        return Err("configured storage manifest differs from the bootstrap enclosure".into());
    }
    let ssd_partition = partition_path(ssd);
    let hdd_partition = partition_path(hdd);
    let matches = value["ssd"]["path"] == SSD_MOUNT
        && value["ssd"]["device"] == ssd.display().to_string()
        && value["ssd"]["uuid"] == read_blkid(&ssd_partition, "UUID")?
        && value["ssd"]["label"] == "DOS_SSD"
        && value["ssd"]["filesystem"] == "ext4"
        && value["ssd"]["role"] == "ssd"
        && hdds[0]["path"] == HDD_MOUNT
        && hdds[0]["device"] == hdd.display().to_string()
        && hdds[0]["uuid"] == read_blkid(&hdd_partition, "UUID")?
        && hdds[0]["label"] == "DOS_HDD_01"
        && hdds[0]["filesystem"] == "ext4"
        && hdds[0]["role"] == "hdd:disk-hdd-01"
        && Path::new(SSD_MOUNT)
            .join(".dasobjectstore/live.sqlite")
            .metadata()
            .is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0);
    if !matches {
        return Err(
            "configured storage manifest or live pool does not match the bootstrap journal".into(),
        );
    }
    Ok(true)
}

fn ensure_daemon_inactive() -> Result<(), String> {
    for unit in [
        "dasobjectstore-storage-ready.service",
        "dasobjectstored.service",
    ] {
        let status = Command::new("systemctl")
            .args(["is-active", "--quiet", unit])
            .status()
            .map_err(|error| format!("cannot inspect {unit}: {error}"))?;
        if status.success() {
            return Err(format!(
                "{unit} is active; refusing destructive bootstrap resume"
            ));
        }
        if status.code() != Some(3) {
            return Err(format!("cannot establish that {unit} is inactive"));
        }
    }
    Ok(())
}

fn ensure_recovery_mounts_belong_to_devices(ssd: &Path, hdd: &Path) -> Result<(), String> {
    for (device, target) in [(ssd, Path::new(SSD_MOUNT)), (hdd, Path::new(HDD_MOUNT))] {
        let lsblk = command_output(
            "lsblk",
            &[
                "--json",
                "--paths",
                "--output",
                "NAME,TYPE,MOUNTPOINTS",
                device.to_str().unwrap_or(""),
            ],
        )?;
        let value: Value = serde_json::from_str(&lsblk)
            .map_err(|error| format!("cannot parse lsblk output: {error}"))?;
        let rows = value["blockdevices"]
            .as_array()
            .ok_or_else(|| "lsblk did not return a device list".to_string())?;
        if rows.len() != 1 || rows[0]["type"].as_str() != Some("disk") {
            return Err(format!("{} is no longer a whole disk", device.display()));
        }
        let mut stack = vec![&rows[0]];
        while let Some(row) = stack.pop() {
            if let Some(points) = row["mountpoints"].as_array() {
                if points
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|point| point != target.to_string_lossy().as_ref())
                {
                    return Err(format!(
                        "{} has a mount outside its managed root",
                        device.display()
                    ));
                }
            }
            if let Some(children) = row["children"].as_array() {
                stack.extend(children);
            }
        }
        let output = Command::new("findmnt")
            .args([
                "--json",
                "--mountpoint",
                target.to_str().unwrap_or(""),
                "--output",
                "SOURCE,TARGET",
            ])
            .output()
            .map_err(|error| {
                format!(
                    "cannot inspect recovery mount {}: {error}",
                    target.display()
                )
            })?;
        if output.status.success() {
            let value: Value = serde_json::from_slice(&output.stdout)
                .map_err(|error| format!("invalid findmnt output: {error}"))?;
            let row = value["filesystems"]
                .as_array()
                .and_then(|rows| (rows.len() == 1).then(|| &rows[0]))
                .ok_or_else(|| format!("ambiguous recovery mount {}", target.display()))?;
            let source = row["source"].as_str().ok_or("findmnt omitted source")?;
            let actual = fs::canonicalize(source)
                .map_err(|error| format!("cannot resolve mounted source {source}: {error}"))?;
            let expected_source = fs::canonicalize(partition_path(device))
                .map_err(|error| format!("cannot resolve expected recovery partition: {error}"))?;
            if actual != expected_source || row["target"] != target.to_string_lossy().as_ref() {
                return Err(format!(
                    "recovery mount {} belongs to another device",
                    target.display()
                ));
            }
        } else if output.status.code() != Some(1) {
            return Err(format!(
                "findmnt failed while inspecting {}",
                target.display()
            ));
        } else if target.exists() {
            let entries = fs::read_dir(target)
                .map_err(|error| format!("cannot inspect {}: {error}", target.display()))?;
            if entries.into_iter().next().is_some() {
                return Err(format!(
                    "unmounted recovery path {} is not empty",
                    target.display()
                ));
            }
        }
    }
    Ok(())
}

fn ensure_fstab_entries_match(ssd: &Path, hdd: &Path) -> Result<(), String> {
    let (contents, _) = read_protected_fstab()?;
    for (target, device) in [(SSD_MOUNT, ssd), (HDD_MOUNT, hdd)] {
        let uuid = read_blkid(&partition_path(device), "UUID")?;
        let entries = contents
            .lines()
            .filter(|line| !line.trim_start().starts_with('#'))
            .filter(|line| line.split_whitespace().nth(1) == Some(target))
            .collect::<Vec<_>>();
        if entries.len() != 1
            || entries[0].split_whitespace().next() != Some(&format!("UUID={uuid}"))
        {
            return Err(format!(
                "/etc/fstab does not contain the exact UUID mount for {target}"
            ));
        }
    }
    Ok(())
}

fn start_storage_services() -> Result<(), String> {
    run_command("systemctl", &["daemon-reload"])?;
    for target in [SSD_MOUNT, HDD_MOUNT] {
        let mounted = Command::new("findmnt")
            .args(["--mountpoint", target, "--noheadings"])
            .status()
            .map_err(|error| format!("cannot inspect {target}: {error}"))?;
        if mounted.success() {
            continue;
        }
        if mounted.code() != Some(1) {
            return Err(format!("findmnt failed while inspecting {target}"));
        }
        let unit = command_output("systemd-escape", &["--path", "--suffix=mount", target])?;
        run_command("systemctl", &["start", unit.trim()])?;
    }
    run_command(
        "systemctl",
        &["start", "dasobjectstore-storage-ready.service"],
    )?;
    run_command("systemctl", &["start", "dasobjectstored.service"])?;
    Ok(())
}

fn remove_prior_bootstrap_fstab_entries(ssd: &Path, hdd: &Path) -> Result<(), String> {
    let path = Path::new("/etc/fstab");
    let (contents, metadata) = read_protected_fstab()?;
    let expected = [
        (SSD_MOUNT, read_blkid(&partition_path(ssd), "UUID").ok()),
        (HDD_MOUNT, read_blkid(&partition_path(hdd), "UUID").ok()),
    ];
    let mut retained = Vec::new();
    let mut removed = 0;
    for line in contents.lines() {
        if line.trim_start().starts_with('#') {
            retained.push(line);
            continue;
        }
        let fields = line.split_whitespace().collect::<Vec<_>>();
        let target = fields.get(1).copied();
        if let Some((mount, uuid)) = expected.iter().find(|(mount, _)| Some(*mount) == target) {
            let expected_source = uuid
                .as_deref()
                .map(|uuid| format!("UUID={uuid}"))
                .ok_or_else(|| {
                    format!("/etc/fstab entry for {mount} has no matching current device UUID")
                })?;
            if fields.first().copied() != Some(expected_source.as_str()) {
                return Err(format!(
                    "/etc/fstab entry for {mount} does not match the journaled device"
                ));
            }
            removed += 1;
        } else if line.contains(SSD_MOUNT) || line.contains(HDD_MOUNT) {
            return Err("/etc/fstab contains an unrecognized managed-storage entry".into());
        } else {
            retained.push(line);
        }
    }
    if removed > 0 {
        let mut replacement = retained.join("\n").into_bytes();
        replacement.push(b'\n');
        atomic_replace_with_expected(
            path,
            &replacement,
            metadata.mode() & 0o777,
            Some((&metadata, contents.as_bytes())),
        )?;
    }
    Ok(())
}

fn partition_path(device: &Path) -> PathBuf {
    let value = device.to_string_lossy();
    if value.contains("nvme") || value.contains("mmcblk") {
        PathBuf::from(format!("{value}p1"))
    } else {
        PathBuf::from(format!("{value}1"))
    }
}

fn ensure_pristine_manifest(path: &Path) -> Result<(), String> {
    let value: Value = serde_json::from_slice(
        &fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?,
    )
    .map_err(|error| format!("invalid {}: {error}", path.display()))?;
    let keys = value
        .as_object()
        .map(|object| object.keys().map(String::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    if keys != ["hdds", "schema_version", "ssd"]
        || value["schema_version"] != 1
        || !value["ssd"].is_null()
        || value["hdds"].as_array().is_none_or(|hdds| !hdds.is_empty())
    {
        return Err("managed-storage manifest is not the untouched first-install template".into());
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
    if !metadata.file_type().is_file() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
        return Err("managed-storage manifest must be a protected root-owned regular file".into());
    }
    Ok(())
}

fn ensure_mount_targets_unused() -> Result<(), String> {
    ensure_mount_paths_are_safe()?;
    for target in [Path::new(SSD_MOUNT), Path::new(HDD_MOUNT)] {
        if target.exists() {
            let mut entries = fs::read_dir(target)
                .map_err(|error| format!("cannot inspect {}: {error}", target.display()))?;
            if entries.next().is_some() {
                return Err(format!("{} is not empty", target.display()));
            }
        }
        let mounted = Command::new("findmnt")
            .args([
                "--mountpoint",
                target.to_str().unwrap_or(""),
                "--noheadings",
            ])
            .output()
            .map_err(|error| format!("cannot inspect mountpoint: {error}"))?;
        if mounted.status.success() && !mounted.stdout.is_empty() {
            return Err(format!("{} is already mounted", target.display()));
        }
        if !mounted.status.success() && mounted.status.code() != Some(1) {
            return Err(format!(
                "findmnt failed while checking {}",
                target.display()
            ));
        }
    }
    Ok(())
}

fn ensure_mount_paths_are_safe() -> Result<(), String> {
    for target in [
        Path::new("/srv"),
        Path::new(MOUNT_ROOT),
        Path::new("/srv/dasobjectstore/hdd"),
        Path::new(SSD_MOUNT),
        Path::new(HDD_MOUNT),
    ] {
        if let Ok(metadata) = fs::symlink_metadata(target) {
            if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() {
                return Err(format!(
                    "managed mount path is not a real directory: {}",
                    target.display()
                ));
            }
        }
    }
    Ok(())
}

fn ensure_no_existing_fstab_entries() -> Result<(), String> {
    let (contents, _) = read_protected_fstab()?;
    if contents.lines().any(|line| {
        !line.trim_start().starts_with('#')
            && (line.contains(SSD_MOUNT) || line.contains(HDD_MOUNT))
    }) {
        return Err("/etc/fstab already contains a target managed-storage entry".into());
    }
    Ok(())
}

fn append_fstab_mounts(ssd_uuid: &str, hdd_uuid: &str) -> Result<(), String> {
    let path = Path::new("/etc/fstab");
    let (entries, metadata) = read_protected_fstab()?;
    for uuid in [ssd_uuid, hdd_uuid] {
        if entries.lines().any(|line| {
            !line.trim_start().starts_with('#')
                && line.split_whitespace().next() == Some(&format!("UUID={uuid}"))
        }) {
            return Err(format!("/etc/fstab already contains UUID {uuid}"));
        }
    }
    let original = entries.as_bytes().to_vec();
    let mut updated = original;
    if !updated.is_empty() && !updated.ends_with(b"\n") {
        updated.push(b'\n');
    }
    updated.extend_from_slice(
        format!(
            "UUID={ssd_uuid} {SSD_MOUNT} ext4 defaults,nofail,x-systemd.device-bound,x-systemd.device-timeout=5s 0 2\nUUID={hdd_uuid} {HDD_MOUNT} ext4 defaults,nofail,x-systemd.device-bound,x-systemd.device-timeout=5s 0 2\n"
        )
        .as_bytes(),
    );
    atomic_replace_with_expected(
        path,
        &updated,
        metadata.mode() & 0o777,
        Some((&metadata, entries.as_bytes())),
    )
}

fn read_protected_fstab() -> Result<(String, fs::Metadata), String> {
    let path = Path::new("/etc/fstab");
    let before = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect /etc/fstab: {error}"))?;
    validate_fstab_metadata(&before)?;
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|error| format!("cannot safely open /etc/fstab: {error}"))?;
    let opened = file.metadata().map_err(|error| error.to_string())?;
    validate_fstab_metadata(&opened)?;
    if !same_file_identity(&before, &opened) {
        return Err("/etc/fstab changed while it was opened".into());
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    let after = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot recheck /etc/fstab: {error}"))?;
    if !same_file_identity(&opened, &after) {
        return Err("/etc/fstab changed while it was read".into());
    }
    let contents =
        String::from_utf8(bytes).map_err(|error| format!("/etc/fstab is not UTF-8: {error}"))?;
    Ok((contents, opened))
}

fn validate_fstab_metadata(metadata: &fs::Metadata) -> Result<(), String> {
    if !metadata.file_type().is_file()
        || metadata.uid() != 0
        || metadata.mode() & 0o022 != 0
        || metadata.nlink() != 1
    {
        return Err("/etc/fstab must be a protected root-owned regular file with one link".into());
    }
    Ok(())
}

fn same_file_identity(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    left.dev() == right.dev()
        && left.ino() == right.ino()
        && left.uid() == right.uid()
        && left.gid() == right.gid()
        && left.mode() == right.mode()
        && left.nlink() == right.nlink()
}

fn write_manifest(path: &Path, manifest: &ManagedStorageManifest) -> Result<(), String> {
    let mut payload = serde_json::to_vec_pretty(manifest).map_err(|error| error.to_string())?;
    payload.push(b'\n');
    atomic_replace(path, &payload, 0o640)
}

fn atomic_replace(path: &Path, payload: &[u8], mode: u32) -> Result<(), String> {
    atomic_replace_with_expected(path, payload, mode, None)
}

fn atomic_replace_with_expected(
    path: &Path,
    payload: &[u8],
    mode: u32,
    expected: Option<(&fs::Metadata, &[u8])>,
) -> Result<(), String> {
    let temporary = path.with_extension(format!("tmp.{}", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(&temporary)
        .map_err(|error| format!("cannot create {}: {error}", temporary.display()))?;
    file.write_all(payload)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("cannot sync {}: {error}", temporary.display()))?;
    if let Some((expected, expected_contents)) = expected {
        let (current_contents, current) = read_protected_fstab()?;
        if !same_file_identity(expected, &current) {
            return Err(format!("{} changed before replacement", path.display()));
        }
        if current_contents.as_bytes() != expected_contents {
            return Err("/etc/fstab contents changed before replacement".into());
        }
    }
    fs::rename(&temporary, path)
        .map_err(|error| format!("cannot install {}: {error}", path.display()))?;
    File::open(path.parent().unwrap_or_else(|| Path::new("/")))
        .and_then(|directory| directory.sync_all())
        .map_err(|error| format!("cannot sync parent of {}: {error}", path.display()))
}

fn read_blkid(device: &Path, tag: &str) -> Result<String, String> {
    let value = command_output(
        "blkid",
        &[
            "--output",
            "value",
            "--match-tag",
            tag,
            device.to_str().unwrap_or(""),
        ],
    )?;
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(format!("{} has no {tag}", device.display()));
    }
    Ok(value)
}

fn require_value(name: &str, actual: String, expected: &str) -> Result<(), String> {
    if actual != expected {
        return Err(format!("{name} {actual:?} differs from {expected:?}"));
    }
    Ok(())
}

fn command_output(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|error| format!("cannot run {program}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "{program} failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout)
        .map_err(|error| format!("{program} returned non-UTF8: {error}"))
}

fn run_command(program: &str, args: &[&str]) -> Result<(), String> {
    command_output(program, args).map(|_| ())
}

struct BootstrapArgs {
    ssd_device: PathBuf,
    hdd_device: PathBuf,
    allow_format: bool,
    acknowledge_blank_devices: bool,
    resume: bool,
    resume_confirmation: Option<String>,
    confirmation: String,
}

impl BootstrapArgs {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut ssd_device = None;
        let mut hdd_device = None;
        let mut allow_format = false;
        let mut acknowledge_blank_devices = false;
        let mut resume = false;
        let mut resume_confirmation = None;
        let mut confirmation = None;
        let mut index = 0;
        while index < args.len() {
            let value = args[index].as_str();
            match value {
                "--ssd-device" | "--hdd-device" | "--confirm" | "--confirm-resume-reformat" => {
                    index += 1;
                    let argument = args
                        .get(index)
                        .ok_or_else(|| format!("{value} requires a value"))?
                        .clone();
                    match value {
                        "--ssd-device" => ssd_device = Some(PathBuf::from(argument)),
                        "--hdd-device" => hdd_device = Some(PathBuf::from(argument)),
                        "--confirm" => confirmation = Some(argument),
                        _ => resume_confirmation = Some(argument),
                    }
                }
                "--allow-format" => allow_format = true,
                "--acknowledge-blank-devices" => acknowledge_blank_devices = true,
                "--resume" => resume = true,
                _ => return Err(format!("unsupported bootstrap-storage argument: {value}")),
            }
            index += 1;
        }
        Ok(Self {
            ssd_device: ssd_device.ok_or("--ssd-device is required")?,
            hdd_device: hdd_device.ok_or("--hdd-device is required")?,
            allow_format,
            acknowledge_blank_devices,
            resume,
            resume_confirmation,
            confirmation: confirmation.ok_or("--confirm is required")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{partition_path, BootstrapArgs};
    use std::path::Path;

    #[test]
    fn bootstrap_requires_explicit_blank_device_acknowledgement_and_confirmation() {
        let parsed = BootstrapArgs::parse(&[
            "--ssd-device".into(),
            "/dev/vdc".into(),
            "--hdd-device".into(),
            "/dev/vdd".into(),
            "--allow-format".into(),
            "--acknowledge-blank-devices".into(),
            "--confirm".into(),
            "confirm bootstrap das".into(),
        ])
        .expect("complete guarded request parses");

        assert!(parsed.allow_format);
        assert!(parsed.acknowledge_blank_devices);
        assert!(!parsed.resume);
        assert_eq!(parsed.confirmation, "confirm bootstrap das");
        assert_eq!(parsed.ssd_device, Path::new("/dev/vdc"));
        assert_eq!(parsed.hdd_device, Path::new("/dev/vdd"));
    }

    #[test]
    fn bootstrap_rejects_unknown_options_and_missing_confirmation() {
        assert!(BootstrapArgs::parse(&["--force".into()]).is_err());
        assert!(BootstrapArgs::parse(&["--ssd-device".into()]).is_err());
    }

    #[test]
    fn recovery_requires_an_explicit_resume_flag() {
        let parsed = BootstrapArgs::parse(&[
            "--ssd-device".into(),
            "/dev/disk/by-id/ssd".into(),
            "--hdd-device".into(),
            "/dev/disk/by-id/hdd".into(),
            "--resume".into(),
            "--confirm-resume-reformat".into(),
            "confirm resume reformat das".into(),
            "--allow-format".into(),
            "--acknowledge-blank-devices".into(),
            "--confirm".into(),
            "confirm bootstrap das".into(),
        ])
        .expect("explicit recovery arguments parse");

        assert!(parsed.resume);
        assert_eq!(
            parsed.resume_confirmation.as_deref(),
            Some("confirm resume reformat das")
        );
        assert_eq!(parsed.ssd_device, Path::new("/dev/disk/by-id/ssd"));
        assert_eq!(parsed.hdd_device, Path::new("/dev/disk/by-id/hdd"));
    }

    #[test]
    fn partition_path_uses_linux_device_naming_rules() {
        assert_eq!(
            partition_path(Path::new("/dev/vdc")),
            Path::new("/dev/vdc1")
        );
        assert_eq!(
            partition_path(Path::new("/dev/nvme0n1")),
            Path::new("/dev/nvme0n1p1")
        );
    }
}
