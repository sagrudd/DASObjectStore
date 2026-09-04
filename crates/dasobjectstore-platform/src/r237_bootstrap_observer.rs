//! Local-root, read-only observations for the fixed r237 bootstrap event.
//!
//! There is no target argument and no remote transport. The observer reads
//! only local Linux state through no-follow/no-atime descriptors or bounded
//! read-only commands. It never creates a marker, starts a service, opens a
//! daemon socket, calls Garage/S3/Docker, or executes a provisioning command.

use crate::linux_smart::{parse_smartctl_json, smartctl_health_args, SMARTCTL_COMMAND};
use crate::probe::{CommandRunner, ProbeError};
use dasobjectstore_core::{
    R237BootstrapLocalObservationV1, R237HddObservationV1, R237ObservationCheckV1,
    R237ObservationStatusV1, R237ObservedMediaV1, R237_BOOTSTRAP_LOCAL_OBSERVATION_V1_SCHEMA,
    R237_BUCKET_NAME, R237_NUC_HOST, R237_STORE_ID, R237_WRITER_GROUP,
};
use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use std::collections::BTreeSet;
#[cfg(target_os = "linux")]
use std::ffi::CString;
#[cfg(target_os = "linux")]
use std::fs::File;
#[cfg(target_os = "linux")]
use std::io::Read;
#[cfg(target_os = "linux")]
use std::os::fd::FromRawFd;
#[cfg(target_os = "linux")]
use std::path::{Component, Path};
#[cfg(target_os = "linux")]
use std::process::Command;

const MACHINE_ID_PATH: &str = "/etc/machine-id";
const APPLIANCE_IDENTITY_PATH: &str = "/var/lib/dasobjectstore/appliance-identity.json";
const STORE_REGISTRY_PATH: &str = "/var/lib/dasobjectstore/stores.json";
const NSSWITCH_PATH: &str = "/etc/nsswitch.conf";
const GROUP_PATH: &str = "/etc/group";
const PASSWD_PATH: &str = "/etc/passwd";
const MOUNTINFO_PATH: &str = "/proc/self/mountinfo";
const MARKER_ROOT: &str = "/var/lib/mnemosyne-r237-custody-marker";
const TRUSTED_LSBLK_PATH: &str = "/usr/bin/lsblk";
const TRUSTED_SMARTCTL_PATH: &str = "/usr/sbin/smartctl";
#[cfg(target_os = "linux")]
const TRUSTED_COMMAND_PATH: &str = "/usr/sbin:/usr/bin";
const LSBLK_ARGS: [&str; 6] = [
    "--json",
    "--bytes",
    "--paths",
    "--output",
    "PATH,TYPE,PKNAME,WWN,SERIAL,ROTA,RO,MOUNTPOINTS",
    "--tree",
];

/// Narrow observation seam. A test fake can expose only read-only facts; the
/// assessment crate has no operation capable of mutating the observed system.
pub trait R237BootstrapReadOnlyObserver {
    fn observe(&self) -> R237BootstrapLocalObservationV1;
}

/// The only command runner used by the production observer. It accepts only
/// fixed absolute paths, verifies their root-owned non-writable regular-file
/// identity without following symlinks, and clears inherited environment.
/// Tests may supply a narrower `CommandRunner` fake through `new`.
#[derive(Debug, Default)]
pub struct TrustedR237CommandRunner;

impl CommandRunner for TrustedR237CommandRunner {
    fn run(&self, command: &str, args: &[&str]) -> Result<String, ProbeError> {
        #[cfg(target_os = "linux")]
        {
            if !trusted_observer_command(command) {
                return Err(ProbeError::CommandFailed {
                    command: command.to_owned(),
                    message: "untrusted or unavailable fixed observer executable".to_owned(),
                });
            }
            let output = Command::new(command)
                .env_clear()
                .env("PATH", TRUSTED_COMMAND_PATH)
                .env("LC_ALL", "C")
                .args(args)
                .output()
                .map_err(|error| ProbeError::CommandFailed {
                    command: command.to_owned(),
                    message: error.to_string(),
                })?;
            if !output.status.success() {
                return Err(ProbeError::CommandFailed {
                    command: command.to_owned(),
                    message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
                });
            }
            return String::from_utf8(output.stdout).map_err(|error| ProbeError::ParseFailed {
                source: command.to_owned(),
                message: error.to_string(),
            });
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (command, args);
            Err(ProbeError::UnsupportedPlatform {
                platform: std::env::consts::OS.to_owned(),
            })
        }
    }
}

#[cfg(target_os = "linux")]
trait R237LocalReadOnlyAccess {
    fn is_root(&self) -> bool;
    fn local_ipv4_addresses(&self) -> Option<BTreeSet<String>>;
    fn read_regular_noatime(&self, path: &str) -> SafeRead;
    fn marker_root_state(&self, path: &str) -> Option<R237ObservationStatusV1>;
    fn statvfs_available_bytes(&self, path: &str) -> Option<u64>;
}

#[cfg(target_os = "linux")]
struct SystemR237LocalReadOnlyAccess;

#[cfg(target_os = "linux")]
impl R237LocalReadOnlyAccess for SystemR237LocalReadOnlyAccess {
    fn is_root(&self) -> bool {
        (unsafe { libc::geteuid() }) == 0
    }

    fn local_ipv4_addresses(&self) -> Option<BTreeSet<String>> {
        local_ipv4_addresses()
    }

    fn read_regular_noatime(&self, path: &str) -> SafeRead {
        read_regular_noatime(path)
    }

    fn marker_root_state(&self, path: &str) -> Option<R237ObservationStatusV1> {
        marker_root_state_at(path)
    }

    fn statvfs_available_bytes(&self, path: &str) -> Option<u64> {
        statvfs_available_bytes(path)
    }
}

#[derive(Debug, Default)]
pub struct LinuxR237BootstrapObserver<R = TrustedR237CommandRunner> {
    runner: R,
}

impl LinuxR237BootstrapObserver<TrustedR237CommandRunner> {
    pub fn system() -> Self {
        Self {
            runner: TrustedR237CommandRunner,
        }
    }
}

impl<R> LinuxR237BootstrapObserver<R> {
    pub fn new(runner: R) -> Self {
        Self { runner }
    }
}

impl<R> R237BootstrapReadOnlyObserver for LinuxR237BootstrapObserver<R>
where
    R: CommandRunner,
{
    fn observe(&self) -> R237BootstrapLocalObservationV1 {
        #[cfg(target_os = "linux")]
        {
            return collect_r237_local_observation(&self.runner, &SystemR237LocalReadOnlyAccess);
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = &self.runner;
            unavailable_observation()
        }
    }
}

#[cfg(target_os = "linux")]
fn collect_r237_local_observation<R: CommandRunner, A: R237LocalReadOnlyAccess>(
    runner: &R,
    access: &A,
) -> R237BootstrapLocalObservationV1 {
    if !access.is_root() {
        return unavailable_observation();
    }
    let target_ip = observe_target_ip(access);
    let machine_identity = observe_machine_identity(access);
    let appliance_identity = observe_appliance_identity(access);
    let store_registry_namespace = observe_store_registry_namespace(access);
    let marker_root = observe_marker_root(access);
    let writer_group = observe_writer_group(access);
    let hdd_members = observe_hdd_members(runner, access);
    R237BootstrapLocalObservationV1 {
        schema_version: R237_BOOTSTRAP_LOCAL_OBSERVATION_V1_SCHEMA.to_owned(),
        target_ip,
        machine_identity,
        appliance_identity,
        // No local-only source can compare this appliance to an independently
        // retained NUC identity baseline.
        clone_detection: unavailable_check(),
        store_registry_namespace,
        marker_root,
        writer_group,
        hdd_members,
        // Deliberately do not scrape Garage, Docker, S3, or a control endpoint.
        garage_bucket_inventory: unavailable_check(),
        exact_physical_placement: unavailable_check(),
    }
}

fn unavailable_observation() -> R237BootstrapLocalObservationV1 {
    R237BootstrapLocalObservationV1 {
        schema_version: R237_BOOTSTRAP_LOCAL_OBSERVATION_V1_SCHEMA.to_owned(),
        target_ip: unavailable_check(),
        machine_identity: unavailable_check(),
        appliance_identity: unavailable_check(),
        clone_detection: unavailable_check(),
        store_registry_namespace: unavailable_check(),
        marker_root: unavailable_check(),
        writer_group: unavailable_check(),
        hdd_members: Vec::new(),
        garage_bucket_inventory: unavailable_check(),
        exact_physical_placement: unavailable_check(),
    }
}

fn unavailable_check() -> R237ObservationCheckV1 {
    R237ObservationCheckV1 {
        status: R237ObservationStatusV1::Unavailable,
        evidence_sha256: None,
    }
}

fn check(status: R237ObservationStatusV1, bytes: &[u8]) -> R237ObservationCheckV1 {
    R237ObservationCheckV1 {
        status,
        evidence_sha256: Some(sha256(bytes)),
    }
}

#[cfg(target_os = "linux")]
fn observe_target_ip(access: &impl R237LocalReadOnlyAccess) -> R237ObservationCheckV1 {
    match access.local_ipv4_addresses() {
        Some(addresses) => {
            let evidence = addresses
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join("\n");
            check(
                if addresses.contains(R237_NUC_HOST) {
                    R237ObservationStatusV1::Verified
                } else {
                    R237ObservationStatusV1::Conflicted
                },
                evidence.as_bytes(),
            )
        }
        None => unavailable_check(),
    }
}

#[cfg(target_os = "linux")]
fn observe_machine_identity(access: &impl R237LocalReadOnlyAccess) -> R237ObservationCheckV1 {
    match access.read_regular_noatime(MACHINE_ID_PATH) {
        SafeRead::Bytes(bytes) if valid_machine_id(&bytes) => {
            check(R237ObservationStatusV1::Verified, &bytes)
        }
        SafeRead::Bytes(_) => check(R237ObservationStatusV1::Invalid, b"machine-id-invalid"),
        _ => unavailable_check(),
    }
}

#[cfg(target_os = "linux")]
fn observe_appliance_identity(access: &impl R237LocalReadOnlyAccess) -> R237ObservationCheckV1 {
    match access.read_regular_noatime(APPLIANCE_IDENTITY_PATH) {
        SafeRead::Bytes(bytes) => match serde_json::from_slice::<ApplianceIdentity>(&bytes) {
            Ok(identity)
                if identity.schema_version == "dasobjectstore.appliance_identity.v1"
                    && identity.appliance_id.starts_with("das-appliance-")
                    && identity.appliance_id.len() <= 128
                    && identity
                        .appliance_id
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-') =>
            {
                check(
                    R237ObservationStatusV1::Verified,
                    identity.appliance_id.as_bytes(),
                )
            }
            _ => check(
                R237ObservationStatusV1::Invalid,
                b"appliance-identity-invalid",
            ),
        },
        _ => unavailable_check(),
    }
}

#[cfg(target_os = "linux")]
fn observe_store_registry_namespace(
    access: &impl R237LocalReadOnlyAccess,
) -> R237ObservationCheckV1 {
    match access.read_regular_noatime(STORE_REGISTRY_PATH) {
        SafeRead::Absent => check(R237ObservationStatusV1::Absent, b"registry-absent"),
        SafeRead::Bytes(bytes) => match registry_namespace_state(&bytes) {
            Some(R237ObservationStatusV1::Absent) => check(R237ObservationStatusV1::Absent, &bytes),
            Some(status) => check(status, &bytes),
            None => check(R237ObservationStatusV1::Invalid, b"registry-invalid"),
        },
        SafeRead::Unavailable => unavailable_check(),
    }
}

#[cfg(target_os = "linux")]
fn observe_marker_root(access: &impl R237LocalReadOnlyAccess) -> R237ObservationCheckV1 {
    match access.marker_root_state(MARKER_ROOT) {
        Some(R237ObservationStatusV1::Absent) => {
            check(R237ObservationStatusV1::Absent, b"marker-absent")
        }
        Some(status) => check(status, b"marker-not-absent"),
        None => unavailable_check(),
    }
}

#[cfg(target_os = "linux")]
fn observe_writer_group(access: &impl R237LocalReadOnlyAccess) -> R237ObservationCheckV1 {
    let SafeRead::Bytes(nsswitch) = access.read_regular_noatime(NSSWITCH_PATH) else {
        return unavailable_check();
    };
    if !nss_group_is_local_files_only(&nsswitch) {
        return check(R237ObservationStatusV1::Unavailable, b"nonlocal-nss");
    }
    let (SafeRead::Bytes(groups), SafeRead::Bytes(passwd)) = (
        access.read_regular_noatime(GROUP_PATH),
        access.read_regular_noatime(PASSWD_PATH),
    ) else {
        return unavailable_check();
    };
    match local_group_state(&groups, &passwd, R237_WRITER_GROUP) {
        Some(R237ObservationStatusV1::Absent) => {
            check(R237ObservationStatusV1::Absent, b"group-absent")
        }
        Some(status) => check(status, b"group-present-or-invalid"),
        None => check(R237ObservationStatusV1::Invalid, b"group-file-invalid"),
    }
}

#[cfg(target_os = "linux")]
fn observe_hdd_members<R: CommandRunner, A: R237LocalReadOnlyAccess>(
    runner: &R,
    access: &A,
) -> Vec<R237HddObservationV1> {
    let Ok(lsblk) = runner.run(TRUSTED_LSBLK_PATH, &LSBLK_ARGS) else {
        return Vec::new();
    };
    let mountinfo = match access.read_regular_noatime(MOUNTINFO_PATH) {
        SafeRead::Bytes(bytes) => String::from_utf8(bytes).ok(),
        _ => None,
    };
    let Some(mountinfo) = mountinfo else {
        return Vec::new();
    };
    let Some(disks) = parse_lsblk_disks(&lsblk) else {
        return Vec::new();
    };
    disks
        .into_iter()
        .map(|disk| observed_hdd_from_disk(runner, access, &mountinfo, disk))
        .collect()
}

#[cfg(target_os = "linux")]
fn observed_hdd_from_disk<R: CommandRunner, A: R237LocalReadOnlyAccess>(
    runner: &R,
    access: &A,
    mountinfo: &str,
    disk: LinuxDisk,
) -> R237HddObservationV1 {
    let physical_member_sha256 = sha256(format!("{}\u{0}{}", disk.wwn, disk.serial).as_bytes());
    let mount_point = disk
        .mountpoints
        .iter()
        .find(|mount| mountinfo_contains_writable_mapping(mountinfo, mount));
    let available_bytes = mount_point
        .and_then(|mount| access.statvfs_available_bytes(&mount.mount_point))
        .unwrap_or(0);
    let smart_args = smartctl_health_args(&disk.path);
    let smart_arg_refs: Vec<_> = smart_args.iter().map(String::as_str).collect();
    let smart = match runner.run(TRUSTED_SMARTCTL_PATH, &smart_arg_refs) {
        Ok(output) => match parse_smartctl_json(&output) {
            Ok(health)
                if health.smart_passed == Some(true) && health.signals.smart_warnings == 0 =>
            {
                R237ObservationStatusV1::Verified
            }
            Ok(_) => R237ObservationStatusV1::Invalid,
            Err(_) => R237ObservationStatusV1::Unavailable,
        },
        Err(_) => R237ObservationStatusV1::Unavailable,
    };
    R237HddObservationV1 {
        physical_member_sha256,
        media: if disk.rotational {
            R237ObservedMediaV1::Hdd
        } else {
            R237ObservedMediaV1::Ssd
        },
        mounted: mount_point.is_some(),
        writable: !disk.read_only && mount_point.is_some_and(|mount| !mount.read_only),
        mount_mapping_verified: mount_point.is_some(),
        available_bytes,
        smart,
    }
}

#[cfg(target_os = "linux")]
fn local_ipv4_addresses() -> Option<BTreeSet<String>> {
    let mut addresses = BTreeSet::new();
    let mut raw = std::ptr::null_mut();
    if unsafe { libc::getifaddrs(&mut raw) } != 0 {
        return None;
    }
    let mut current = raw;
    while !current.is_null() {
        let entry = unsafe { &*current };
        if !entry.ifa_addr.is_null()
            && unsafe { (*entry.ifa_addr).sa_family as i32 } == libc::AF_INET
        {
            let address = unsafe { *(entry.ifa_addr as *const libc::sockaddr_in) }
                .sin_addr
                .s_addr;
            addresses.insert(format_ipv4_network_order(address));
        }
        current = entry.ifa_next;
    }
    unsafe { libc::freeifaddrs(raw) };
    Some(addresses)
}

#[cfg(target_os = "linux")]
fn format_ipv4_network_order(address: libc::in_addr_t) -> String {
    // `s_addr` is received from a sockaddr as network-order bytes; a native
    // integer conversion would reverse IPv4 octets on little-endian Linux.
    let octets = address.to_ne_bytes();
    format!("{}.{}.{}.{}", octets[0], octets[1], octets[2], octets[3])
}

#[cfg(target_os = "linux")]
enum SafeRead {
    Absent,
    Bytes(Vec<u8>),
    Unavailable,
}

#[cfg(target_os = "linux")]
fn trusted_observer_command(path: &str) -> bool {
    matches!(path, TRUSTED_LSBLK_PATH | TRUSTED_SMARTCTL_PATH)
        && trusted_root_owned_executable(path)
}

/// Verify the final path entry through a no-follow descriptor. A root observer
/// never resolves a program through PATH and refuses a symlink, non-regular
/// file, non-root owner, missing execute bit, or group/world writable binary.
#[cfg(target_os = "linux")]
fn trusted_root_owned_executable(path: &str) -> bool {
    let Ok((parent, leaf)) = open_absolute_parent(path) else {
        return false;
    };
    let Ok(leaf) = CString::new(leaf) else {
        unsafe { libc::close(parent) };
        return false;
    };
    let fd = unsafe {
        libc::openat(
            parent,
            leaf.as_ptr(),
            libc::O_RDONLY
                | libc::O_CLOEXEC
                | libc::O_NOFOLLOW
                | libc::O_NOATIME
                | libc::O_NONBLOCK,
        )
    };
    unsafe { libc::close(parent) };
    if fd < 0 {
        return false;
    }
    let mut stat = std::mem::MaybeUninit::<libc::stat>::zeroed();
    let trusted = unsafe { libc::fstat(fd, stat.as_mut_ptr()) == 0 } && {
        let stat = unsafe { stat.assume_init() };
        stat.st_mode & libc::S_IFMT == libc::S_IFREG
            && stat.st_uid == 0
            && stat.st_mode & 0o022 == 0
            && stat.st_mode & 0o111 != 0
    };
    unsafe { libc::close(fd) };
    trusted
}

/// Safe path-walk: each parent is opened with `openat` and `O_NOFOLLOW`, then
/// the final regular file is opened with `O_NOATIME | O_NOFOLLOW`. There is no
/// ordinary-read fallback, so a symlink, FIFO, device, directory, replacement,
/// or missing protected ancestor produces no trusted observation.
#[cfg(target_os = "linux")]
fn read_regular_noatime(path: &str) -> SafeRead {
    let Ok((parent, leaf)) = open_absolute_parent(path) else {
        return SafeRead::Unavailable;
    };
    let Ok(leaf) = CString::new(leaf) else {
        unsafe { libc::close(parent) };
        return SafeRead::Unavailable;
    };
    let fd = unsafe {
        libc::openat(
            parent,
            leaf.as_ptr(),
            libc::O_RDONLY
                | libc::O_CLOEXEC
                | libc::O_NOFOLLOW
                | libc::O_NOATIME
                | libc::O_NONBLOCK,
        )
    };
    if fd < 0 {
        unsafe { libc::close(parent) };
        return if std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound {
            SafeRead::Absent
        } else {
            SafeRead::Unavailable
        };
    }
    let mut stat = std::mem::MaybeUninit::<libc::stat>::zeroed();
    if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } != 0 {
        unsafe { libc::close(fd) };
        unsafe { libc::close(parent) };
        return SafeRead::Unavailable;
    }
    let stat = unsafe { stat.assume_init() };
    if stat.st_mode & libc::S_IFMT != libc::S_IFREG {
        unsafe { libc::close(fd) };
        unsafe { libc::close(parent) };
        return SafeRead::Unavailable;
    }
    if !entry_matches_open_file(parent, &leaf, &stat) {
        unsafe { libc::close(fd) };
        unsafe { libc::close(parent) };
        return SafeRead::Unavailable;
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    let mut bytes = Vec::new();
    let read_result = file.read_to_end(&mut bytes);
    drop(file);
    let stable = entry_matches_open_file(parent, &leaf, &stat);
    unsafe { libc::close(parent) };
    if read_result.is_err() || bytes.len() > 1024 * 1024 || !stable {
        return SafeRead::Unavailable;
    }
    SafeRead::Bytes(bytes)
}

#[cfg(target_os = "linux")]
fn entry_matches_open_file(parent: libc::c_int, leaf: &CString, opened: &libc::stat) -> bool {
    let mut entry = std::mem::MaybeUninit::<libc::stat>::zeroed();
    (unsafe {
        libc::fstatat(
            parent,
            leaf.as_ptr(),
            entry.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        ) == 0
    }) && {
        let entry = unsafe { entry.assume_init() };
        entry.st_mode & libc::S_IFMT == libc::S_IFREG
            && entry.st_dev == opened.st_dev
            && entry.st_ino == opened.st_ino
    }
}

#[cfg(target_os = "linux")]
fn open_absolute_parent(path: &str) -> Result<(libc::c_int, String), ()> {
    let path = Path::new(path);
    if !path.is_absolute() {
        return Err(());
    }
    let parts: Vec<String> = path
        .components()
        .filter_map(|part| match part {
            Component::Normal(value) => value.to_str().map(ToOwned::to_owned),
            Component::RootDir => None,
            _ => Some(String::new()),
        })
        .collect();
    if parts.is_empty() || parts.iter().any(String::is_empty) {
        return Err(());
    }
    let root = CString::new("/").map_err(|_| ())?;
    let mut fd = unsafe {
        libc::open(
            root.as_ptr(),
            libc::O_RDONLY
                | libc::O_DIRECTORY
                | libc::O_CLOEXEC
                | libc::O_NOFOLLOW
                | libc::O_NOATIME,
        )
    };
    if fd < 0 {
        return Err(());
    }
    for component in &parts[..parts.len() - 1] {
        let component = match CString::new(component.as_str()) {
            Ok(component) => component,
            Err(_) => {
                unsafe { libc::close(fd) };
                return Err(());
            }
        };
        let next = unsafe {
            libc::openat(
                fd,
                component.as_ptr(),
                libc::O_RDONLY
                    | libc::O_DIRECTORY
                    | libc::O_CLOEXEC
                    | libc::O_NOFOLLOW
                    | libc::O_NOATIME,
            )
        };
        unsafe { libc::close(fd) };
        if next < 0 {
            return Err(());
        }
        fd = next;
    }
    Ok((fd, parts.last().expect("non-empty path parts").clone()))
}

#[cfg(target_os = "linux")]
fn marker_root_state_at(path: &str) -> Option<R237ObservationStatusV1> {
    let (parent, leaf) = open_absolute_parent(path).ok()?;
    let leaf = match CString::new(leaf) {
        Ok(leaf) => leaf,
        Err(_) => {
            unsafe { libc::close(parent) };
            return None;
        }
    };
    let mut stat = std::mem::MaybeUninit::<libc::stat>::zeroed();
    let result = unsafe {
        libc::fstatat(
            parent,
            leaf.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    unsafe { libc::close(parent) };
    if result != 0 {
        return if std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound {
            Some(R237ObservationStatusV1::Absent)
        } else {
            None
        };
    }
    let stat = unsafe { stat.assume_init() };
    if stat.st_mode & libc::S_IFMT == libc::S_IFLNK {
        Some(R237ObservationStatusV1::Conflicted)
    } else {
        Some(R237ObservationStatusV1::Present)
    }
}

#[cfg(target_os = "linux")]
fn statvfs_available_bytes(path: &str) -> Option<u64> {
    let path = CString::new(path).ok()?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
    if unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) } != 0 {
        return None;
    }
    let stat = unsafe { stat.assume_init() };
    (stat.f_bavail as u64).checked_mul(stat.f_frsize as u64)
}

#[cfg(target_os = "linux")]
fn mountinfo_contains_writable_mapping(mountinfo: &str, mount: &LinuxMount) -> bool {
    mountinfo.lines().any(|line| {
        let Some((prefix, suffix)) = line.split_once(" - ") else {
            return false;
        };
        let prefix_fields: Vec<_> = prefix.split_whitespace().collect();
        let suffix_fields: Vec<_> = suffix.split_whitespace().collect();
        prefix_fields
            .get(4)
            .is_some_and(|observed| *observed == mount.mount_point)
            && suffix_fields
                .get(1)
                .is_some_and(|source| *source == mount.source_path)
            && !prefix_fields
                .get(5)
                .is_some_and(|options| options.split(',').any(|option| option == "ro"))
            && !suffix_fields
                .get(2)
                .is_some_and(|options| options.split(',').any(|option| option == "ro"))
    })
}

#[cfg(target_os = "linux")]
fn valid_machine_id(bytes: &[u8]) -> bool {
    let value = std::str::from_utf8(bytes)
        .ok()
        .map(str::trim)
        .unwrap_or_default();
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(target_os = "linux")]
fn nss_group_is_local_files_only(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes).ok().and_then(|text| {
        text.lines().find_map(|line| {
            let line = line.split('#').next()?.trim();
            let (database, sources) = line.split_once(':')?;
            (database.trim() == "group").then_some(sources.split_whitespace().collect::<Vec<_>>())
        })
    }) == Some(vec!["files"])
}

#[cfg(target_os = "linux")]
fn local_group_state(
    groups: &[u8],
    passwd: &[u8],
    wanted_group: &str,
) -> Option<R237ObservationStatusV1> {
    let groups = std::str::from_utf8(groups).ok()?;
    let passwd = std::str::from_utf8(passwd).ok()?;
    let mut found_gid = None;
    let mut members = BTreeSet::new();
    for line in groups.lines().filter(|line| !line.trim().is_empty()) {
        let fields: Vec<_> = line.split(':').collect();
        if fields.len() != 4 {
            return None;
        }
        if fields[0] == wanted_group {
            if found_gid.is_some() {
                return None;
            }
            found_gid = fields[2].parse::<u32>().ok();
            members.extend(
                fields[3]
                    .split(',')
                    .filter(|value| !value.is_empty())
                    .map(ToOwned::to_owned),
            );
        }
    }
    let Some(gid) = found_gid else {
        return Some(R237ObservationStatusV1::Absent);
    };
    let mut human = !members.is_empty();
    for line in passwd.lines().filter(|line| !line.trim().is_empty()) {
        let fields: Vec<_> = line.split(':').collect();
        if fields.len() != 7 {
            return None;
        }
        let uid = fields[2].parse::<u32>().ok()?;
        let primary_gid = fields[3].parse::<u32>().ok()?;
        if primary_gid == gid && uid >= 1000 && uid != 65_534 {
            human = true;
        }
    }
    Some(if human {
        R237ObservationStatusV1::Conflicted
    } else {
        R237ObservationStatusV1::Present
    })
}

#[cfg(target_os = "linux")]
fn registry_namespace_state(bytes: &[u8]) -> Option<R237ObservationStatusV1> {
    let entries: Vec<serde_json::Value> = serde_json::from_slice(bytes).ok()?;
    let mut store_ids = BTreeSet::new();
    let mut bucket_names = BTreeSet::new();
    for entry in entries {
        let entry = entry.as_object()?;
        let store_id = entry.get("store_id")?.as_str()?;
        let bucket = entry
            .get("bucket_name")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| default_bucket_name(store_id));
        if !store_ids.insert(store_id.to_owned()) || !bucket_names.insert(bucket) {
            return Some(R237ObservationStatusV1::Conflicted);
        }
    }
    if store_ids.contains(R237_STORE_ID) || bucket_names.contains(R237_BUCKET_NAME) {
        Some(R237ObservationStatusV1::Present)
    } else {
        Some(R237ObservationStatusV1::Absent)
    }
}

#[cfg(target_os = "linux")]
fn default_bucket_name(store_id: &str) -> String {
    let mut output = String::from("dos-");
    let mut previous_hyphen = false;
    for character in store_id.chars().flat_map(char::to_lowercase) {
        let next = if character.is_ascii_alphanumeric() {
            character
        } else {
            '-'
        };
        if next != '-' || (!previous_hyphen && !output.ends_with('-')) {
            output.push(next);
        }
        previous_hyphen = next == '-';
    }
    output.truncate(63);
    output.trim_end_matches('-').to_owned()
}

#[cfg(target_os = "linux")]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplianceIdentity {
    schema_version: String,
    appliance_id: String,
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct LinuxDisk {
    path: String,
    wwn: String,
    serial: String,
    rotational: bool,
    read_only: bool,
    mountpoints: Vec<LinuxMount>,
}

#[cfg(target_os = "linux")]
#[derive(Debug, Eq, PartialEq)]
struct LinuxMount {
    source_path: String,
    mount_point: String,
    read_only: bool,
}

#[cfg(target_os = "linux")]
fn parse_lsblk_disks(input: &str) -> Option<Vec<LinuxDisk>> {
    let root: serde_json::Value = serde_json::from_str(input).ok()?;
    let devices = root.get("blockdevices")?.as_array()?;
    let mut disks = Vec::new();
    for device in devices {
        collect_lsblk_disks(device, &mut disks)?;
    }
    Some(disks)
}

#[cfg(target_os = "linux")]
fn collect_lsblk_disks(value: &serde_json::Value, disks: &mut Vec<LinuxDisk>) -> Option<()> {
    let object = value.as_object()?;
    if object.get("type")?.as_str()? == "disk" {
        let path = object.get("path")?.as_str()?.to_owned();
        if !safe_device_path(&path)
            || object
                .get("pkname")
                .and_then(serde_json::Value::as_str)
                .is_some()
        {
            return None;
        }
        let wwn = object.get("wwn")?.as_str()?.to_owned();
        let serial = object.get("serial")?.as_str()?.to_owned();
        let rotational = json_flag(object.get("rota")?)?;
        let read_only = json_flag(object.get("ro")?)?;
        let mut mountpoints = Vec::new();
        collect_mountpoints(value, &mut mountpoints)?;
        disks.push(LinuxDisk {
            path,
            wwn,
            serial,
            rotational,
            read_only,
            mountpoints,
        });
    }
    if let Some(children) = object.get("children").and_then(serde_json::Value::as_array) {
        for child in children {
            collect_lsblk_disks(child, disks)?;
        }
    }
    Some(())
}

#[cfg(target_os = "linux")]
fn collect_mountpoints(value: &serde_json::Value, mountpoints: &mut Vec<LinuxMount>) -> Option<()> {
    let object = value.as_object()?;
    let source_path = object.get("path")?.as_str()?.to_owned();
    let read_only = json_flag(object.get("ro")?)?;
    if let Some(values) = object
        .get("mountpoints")
        .and_then(serde_json::Value::as_array)
    {
        for mount_point in values
            .iter()
            .filter_map(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
        {
            mountpoints.push(LinuxMount {
                source_path: source_path.clone(),
                mount_point: mount_point.to_owned(),
                read_only,
            });
        }
    }
    if let Some(children) = object.get("children").and_then(serde_json::Value::as_array) {
        for child in children {
            collect_mountpoints(child, mountpoints)?;
        }
    }
    Some(())
}

#[cfg(target_os = "linux")]
fn json_flag(value: &serde_json::Value) -> Option<bool> {
    value
        .as_bool()
        .or_else(|| value.as_u64().map(|value| value != 0))
}

#[cfg(target_os = "linux")]
fn safe_device_path(path: &str) -> bool {
    path.starts_with("/dev/")
        && Path::new(path)
            .components()
            .all(|component| matches!(component, Component::RootDir | Component::Normal(_)))
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::ProbeError;
    use dasobjectstore_core::R237_REQUIRED_FREE_BYTES_PER_SELECTED_HDD;
    #[cfg(target_os = "linux")]
    use std::fs;
    #[cfg(target_os = "linux")]
    use std::os::unix::fs::{symlink, MetadataExt};
    #[cfg(target_os = "linux")]
    use std::path::PathBuf;

    #[test]
    fn standalone_observer_has_no_target_or_mutation_surface() {
        let observation = unavailable_observation();
        assert_eq!(
            observation.target_ip.status,
            R237ObservationStatusV1::Unavailable
        );
        assert_eq!(
            R237_REQUIRED_FREE_BYTES_PER_SELECTED_HDD,
            40 * 1024 * 1024 * 1024
        );
        assert_eq!(TRUSTED_LSBLK_PATH, "/usr/bin/lsblk");
        assert_eq!(TRUSTED_SMARTCTL_PATH, "/usr/sbin/smartctl");
        assert_eq!(SMARTCTL_COMMAND, "smartctl");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parser_tracks_partition_source_and_rejects_parent_aliases() {
        let fixture = r#"{"blockdevices":[{"path":"/dev/sda","type":"disk","wwn":"w1","serial":"s1","rota":1,"ro":0,"mountpoints":[null],"children":[{"path":"/dev/sda1","type":"part","ro":0,"mountpoints":["/data"]}]},{"path":"/dev/nvme0n1","type":"disk","wwn":"w2","serial":"s2","rota":0,"ro":0,"mountpoints":["/"]}]}"#;
        let disks = parse_lsblk_disks(fixture).expect("fixture");
        assert_eq!(disks.len(), 2);
        assert!(disks[0].rotational);
        assert_eq!(
            disks[0].mountpoints,
            [LinuxMount {
                source_path: "/dev/sda1".to_owned(),
                mount_point: "/data".to_owned(),
                read_only: false,
            }]
        );
        assert!(!disks[1].rotational);
        assert!(parse_lsblk_disks(
            r#"{"blockdevices":[{"path":"/dev/sdb","type":"disk","pkname":"dm-0","wwn":"w","serial":"s","rota":1,"ro":0,"mountpoints":[]}]}"#
        )
        .is_none());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn mount_mapping_requires_the_exact_writable_partition_source() {
        let mount = LinuxMount {
            source_path: "/dev/sda1".to_owned(),
            mount_point: "/data".to_owned(),
            read_only: false,
        };
        assert!(mountinfo_contains_writable_mapping(
            "45 1 8:1 / /data rw,relatime - ext4 /dev/sda1 rw",
            &mount
        ));
        assert!(!mountinfo_contains_writable_mapping(
            "45 1 8:2 / /data rw,relatime - ext4 /dev/sdb1 rw",
            &mount
        ));
        assert!(!mountinfo_contains_writable_mapping(
            "45 1 8:1 / /data ro,relatime - ext4 /dev/sda1 ro",
            &mount
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn local_nss_requires_files_only_and_rejects_existing_human_group() {
        assert!(nss_group_is_local_files_only(
            b"passwd: files\ngroup: files\n"
        ));
        assert!(!nss_group_is_local_files_only(b"group: files sss\n"));
        assert_eq!(
            local_group_state(
                b"mnemosyne-r237-custody:x:2000:stephen\n",
                b"stephen:x:1000:2000::/:/bin/sh\n",
                R237_WRITER_GROUP
            ),
            Some(R237ObservationStatusV1::Conflicted)
        );
        assert_eq!(
            local_group_state(
                b"other:x:2:\n",
                b"root:x:0:0::/:/bin/sh\n",
                R237_WRITER_GROUP
            ),
            Some(R237ObservationStatusV1::Absent)
        );
        assert_eq!(
            local_group_state(
                b"mnemosyne-r237-custody:x:2000:\n",
                b"root:x:0:0::/:/bin/sh\n",
                R237_WRITER_GROUP
            ),
            Some(R237ObservationStatusV1::Present)
        );
        assert_eq!(
            local_group_state(
                b"mnemosyne-r237-custody:x:2000:\nmnemosyne-r237-custody:x:2001:\n",
                b"root:x:0:0::/:/bin/sh\n",
                R237_WRITER_GROUP
            ),
            None
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn registry_unknown_duplicate_and_target_collisions_fail_closed() {
        assert_eq!(
            registry_namespace_state(br#"[]"#),
            Some(R237ObservationStatusV1::Absent)
        );
        assert_eq!(
            registry_namespace_state(
                br#"[{"store_id":"r237_s4_bootstrap_custody","bucket_name":null}]"#
            ),
            Some(R237ObservationStatusV1::Present)
        );
        assert_eq!(
            registry_namespace_state(br#"[{"store_id":"a"},{"store_id":"a"}]"#),
            Some(R237ObservationStatusV1::Conflicted)
        );
        assert_eq!(
            registry_namespace_state(
                br#"[{"store_id":"another_store","bucket_name":"dos-r237-s4-bootstrap-custody"}]"#
            ),
            Some(R237ObservationStatusV1::Present),
            "the target bucket is a conflict even under a different store"
        );
        assert_eq!(registry_namespace_state(b"not-json"), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn bounded_smart_probe_is_the_only_disk_command_and_unavailable_denies_member() {
        let disk = LinuxDisk {
            path: "/dev/sda".to_owned(),
            wwn: "wwn-1".to_owned(),
            serial: "serial-1".to_owned(),
            rotational: true,
            read_only: false,
            mountpoints: vec![LinuxMount {
                source_path: "/dev/sda1".to_owned(),
                mount_point: "/definitely-not-a-mounted-fixture".to_owned(),
                read_only: false,
            }],
        };
        let observed = observed_hdd_from_disk(
            &UnavailableSmartRunner,
            &RecordingAccess::default(),
            "45 1 8:1 / /definitely-not-a-mounted-fixture rw,relatime - ext4 /dev/sda1 rw",
            disk,
        );
        assert_eq!(observed.media, R237ObservedMediaV1::Hdd);
        assert_eq!(observed.smart, R237ObservationStatusV1::Unavailable);
        assert_eq!(
            observed.available_bytes,
            R237_REQUIRED_FREE_BYTES_PER_SELECTED_HDD
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn machine_id_and_safe_path_validation_fail_closed() {
        assert!(valid_machine_id(b"0123456789abcdef0123456789abcdef\n"));
        assert!(!valid_machine_id(b"0123"));
        assert!(safe_device_path("/dev/disk/by-id/wwn-1"));
        assert!(!safe_device_path("relative-device"));
        assert!(!safe_device_path("/dev/../tmp/device"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn marker_root_checks_missing_present_and_symlink_states_without_following() {
        let root = test_directory("marker");
        let marker = root.join("marker");
        assert_eq!(
            marker_root_state_at(marker.to_str().expect("path")),
            Some(R237ObservationStatusV1::Absent)
        );
        fs::write(&marker, b"already-present").expect("fixture marker");
        assert_eq!(
            marker_root_state_at(marker.to_str().expect("path")),
            Some(R237ObservationStatusV1::Present)
        );
        fs::remove_file(&marker).expect("remove fixture marker");
        symlink("elsewhere", &marker).expect("fixture marker symlink");
        assert_eq!(
            marker_root_state_at(marker.to_str().expect("path")),
            Some(R237ObservationStatusV1::Conflicted)
        );
        assert_eq!(
            marker_root_state_at(root.join("missing-parent/marker").to_str().expect("path")),
            None
        );
        let ancestor_link = root.join("linked-parent");
        symlink(&root, &ancestor_link).expect("fixture ancestor symlink");
        assert_eq!(
            marker_root_state_at(ancestor_link.join("marker").to_str().expect("path")),
            None
        );
        fs::remove_dir_all(root).expect("remove fixture directory");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn noatime_nofollow_reader_rejects_nonregular_and_preserves_regular_atime() {
        let root = test_directory("safe-read");
        let regular = root.join("regular");
        fs::write(&regular, b"read-only fixture").expect("fixture file");
        let before = fs::metadata(&regular).expect("before metadata");
        assert!(matches!(
            read_regular_noatime(regular.to_str().expect("path")),
            SafeRead::Bytes(_)
        ));
        let after = fs::metadata(&regular).expect("after metadata");
        assert_eq!(
            before.atime(),
            after.atime(),
            "safe read must not update atime"
        );
        assert_eq!(
            before.atime_nsec(),
            after.atime_nsec(),
            "safe read must not update atime"
        );

        let directory = root.join("directory");
        fs::create_dir(&directory).expect("fixture directory");
        assert!(matches!(
            read_regular_noatime(directory.to_str().expect("path")),
            SafeRead::Unavailable
        ));
        let link = root.join("link");
        symlink(&regular, &link).expect("fixture symlink");
        assert!(matches!(
            read_regular_noatime(link.to_str().expect("path")),
            SafeRead::Unavailable
        ));
        let fifo = root.join("fifo");
        let fifo_c = CString::new(fifo.to_str().expect("path")).expect("cstring");
        assert_eq!(unsafe { libc::mkfifo(fifo_c.as_ptr(), 0o600) }, 0);
        assert!(matches!(
            read_regular_noatime(fifo.to_str().expect("path")),
            SafeRead::Unavailable
        ));
        assert!(matches!(
            read_regular_noatime("/dev/null"),
            SafeRead::Unavailable
        ));
        assert!(matches!(
            read_regular_noatime(root.join("absent").to_str().expect("path")),
            SafeRead::Absent
        ));
        fs::remove_dir_all(root).expect("remove fixture directory");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn observer_source_has_no_mutating_or_transport_syscall_surface() {
        let source = include_str!("r237_bootstrap_observer.rs");
        for forbidden in [
            "O_CREAT",
            "O_TRUNC",
            "O_WRONLY",
            "rename(",
            "unlink(",
            "mkdir(",
            "chmod(",
            "chown(",
            "mount(",
            "TcpStream",
            "UnixStream",
            "systemctl",
        ] {
            assert!(
                !source.contains(forbidden),
                "forbidden observer operation: {forbidden}"
            );
        }
        assert!(source.contains("libc::O_NOATIME"));
        assert!(source.contains("libc::O_NOFOLLOW"));
        assert!(source.contains("libc::fstatat"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn trusted_commands_are_absolute_whitelisted_and_fail_closed() {
        assert!(!trusted_observer_command("lsblk"));
        assert!(!trusted_observer_command("/tmp/lsblk"));
        assert!(!trusted_observer_command("/usr/bin/smartctl"));
        assert!(
            TrustedR237CommandRunner.run("lsblk", &[]).is_err(),
            "ambient PATH name must not be accepted"
        );
        assert!(
            TrustedR237CommandRunner.run("/tmp/lsblk", &[]).is_err(),
            "untrusted executable must not be accepted"
        );
        assert!(!trusted_root_owned_executable(
            "/definitely-missing-r237-observer-tool"
        ));
        let root = test_directory("untrusted-tool");
        let ordinary = root.join("tool");
        fs::write(&ordinary, b"not an approved executable").expect("fixture tool");
        assert!(!trusted_root_owned_executable(
            ordinary.to_str().expect("path")
        ));
        let link = root.join("tool-link");
        symlink(&ordinary, &link).expect("fixture link");
        assert!(!trusted_root_owned_executable(link.to_str().expect("path")));
        fs::remove_dir_all(root).expect("remove fixture directory");
    }

    #[cfg(target_os = "linux")]
    fn test_directory(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "dasobjectstore-r237-observer-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::create_dir(&path).expect("fixture directory");
        path
    }

    #[cfg(target_os = "linux")]
    #[derive(Default)]
    struct RecordingAccess {
        reads: std::cell::RefCell<Vec<String>>,
        writes: std::cell::Cell<usize>,
    }

    #[cfg(target_os = "linux")]
    impl R237LocalReadOnlyAccess for RecordingAccess {
        fn is_root(&self) -> bool {
            self.reads.borrow_mut().push("geteuid".to_owned());
            true
        }

        fn local_ipv4_addresses(&self) -> Option<BTreeSet<String>> {
            self.reads.borrow_mut().push("getifaddrs".to_owned());
            Some(BTreeSet::from([R237_NUC_HOST.to_owned()]))
        }

        fn read_regular_noatime(&self, path: &str) -> SafeRead {
            self.reads.borrow_mut().push(path.to_owned());
            match path {
                MACHINE_ID_PATH => SafeRead::Bytes(b"0123456789abcdef0123456789abcdef\n".to_vec()),
                APPLIANCE_IDENTITY_PATH => SafeRead::Bytes(
                    br#"{"schema_version":"dasobjectstore.appliance_identity.v1","appliance_id":"das-appliance-test"}"#.to_vec(),
                ),
                STORE_REGISTRY_PATH => SafeRead::Bytes(b"[]".to_vec()),
                NSSWITCH_PATH => SafeRead::Bytes(b"group: files\n".to_vec()),
                GROUP_PATH => SafeRead::Bytes(b"other:x:2:\n".to_vec()),
                PASSWD_PATH => SafeRead::Bytes(b"root:x:0:0::/:/bin/sh\n".to_vec()),
                MOUNTINFO_PATH => SafeRead::Bytes(
                    b"45 1 8:1 / /fixture rw,relatime - ext4 /dev/sda1 rw\n".to_vec(),
                ),
                _ => SafeRead::Unavailable,
            }
        }

        fn marker_root_state(&self, path: &str) -> Option<R237ObservationStatusV1> {
            self.reads.borrow_mut().push(path.to_owned());
            Some(R237ObservationStatusV1::Absent)
        }

        fn statvfs_available_bytes(&self, path: &str) -> Option<u64> {
            self.reads.borrow_mut().push(path.to_owned());
            Some(R237_REQUIRED_FREE_BYTES_PER_SELECTED_HDD)
        }
    }

    #[cfg(target_os = "linux")]
    #[derive(Default)]
    struct RecordingRunner {
        calls: std::cell::RefCell<Vec<(String, Vec<String>)>>,
    }

    #[cfg(target_os = "linux")]
    impl CommandRunner for RecordingRunner {
        fn run(&self, command: &str, args: &[&str]) -> Result<String, ProbeError> {
            self.calls.borrow_mut().push((
                command.to_owned(),
                args.iter().map(|arg| (*arg).to_owned()).collect(),
            ));
            assert!(matches!(
                command,
                TRUSTED_LSBLK_PATH | TRUSTED_SMARTCTL_PATH
            ));
            assert!(!args
                .iter()
                .any(|arg| arg.contains("--create") || arg.contains("--write")));
            match command {
                TRUSTED_LSBLK_PATH => Ok(
                    r#"{"blockdevices":[{"path":"/dev/sda","type":"disk","wwn":"wwn-a","serial":"serial-a","rota":1,"ro":0,"mountpoints":[null],"children":[{"path":"/dev/sda1","type":"part","ro":0,"mountpoints":["/fixture"]}]}]}"#.to_owned(),
                ),
                TRUSTED_SMARTCTL_PATH => Ok(r#"{"smart_status":{"passed":true}}"#.to_owned()),
                _ => unreachable!("whitelisted above"),
            }
        }
    }

    #[cfg(target_os = "linux")]
    struct UnavailableSmartRunner;

    #[cfg(target_os = "linux")]
    impl CommandRunner for UnavailableSmartRunner {
        fn run(&self, command: &str, args: &[&str]) -> Result<String, ProbeError> {
            assert_eq!(command, TRUSTED_SMARTCTL_PATH);
            assert_eq!(args.last().copied(), Some("/dev/sda"));
            Err(ProbeError::CommandFailed {
                command: command.to_owned(),
                message: "fixture unavailable".to_owned(),
            })
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recording_observer_uses_only_the_fixed_read_set_and_no_effect_paths() {
        let access = RecordingAccess::default();
        let runner = RecordingRunner::default();
        let observation = collect_r237_local_observation(&runner, &access);
        assert_eq!(
            observation.garage_bucket_inventory.status,
            R237ObservationStatusV1::Unavailable
        );
        assert_eq!(access.writes.get(), 0, "read-only access has no write path");
        let expected_reads = [
            "geteuid",
            "getifaddrs",
            MACHINE_ID_PATH,
            APPLIANCE_IDENTITY_PATH,
            STORE_REGISTRY_PATH,
            MARKER_ROOT,
            NSSWITCH_PATH,
            GROUP_PATH,
            PASSWD_PATH,
            MOUNTINFO_PATH,
            "/fixture",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
        assert_eq!(access.reads.into_inner(), expected_reads);
        let calls = runner.calls.into_inner();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].0, TRUSTED_LSBLK_PATH);
        assert_eq!(calls[1].0, TRUSTED_SMARTCTL_PATH);
        assert_eq!(calls[1].1.last().map(String::as_str), Some("/dev/sda"));
        assert!(calls.iter().all(|(path, _)| {
            matches!(path.as_str(), TRUSTED_LSBLK_PATH | TRUSTED_SMARTCTL_PATH)
        }));
    }
}
