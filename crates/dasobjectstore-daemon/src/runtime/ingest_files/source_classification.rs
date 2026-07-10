use crate::api::DaemonIngressOrigin;
#[cfg(target_os = "linux")]
use std::fs;
use std::path::Path;
#[cfg(any(target_os = "linux", test))]
use std::path::PathBuf;

#[cfg(target_os = "linux")]
const REMOTE_OR_EXTERNAL_FILESYSTEMS: &[&str] = &[
    "9p", "cifs", "davfs", "fuse", "fuseblk", "nfs", "nfs4", "smb3", "sshfs",
];

pub(super) fn verified_ingress_origin_with_source_verifier(
    requested_origin: DaemonIngressOrigin,
    source_path: &Path,
    source_is_server_local: fn(&Path) -> bool,
) -> DaemonIngressOrigin {
    verified_ingress_origin_with_source_local(requested_origin, source_is_server_local(source_path))
}

/// Returns operator-facing source topology details for the ingest preflight.
///
/// The daemon remains fail-closed when these details cannot be resolved. The
/// summary is deliberately a string because it is rendered in the existing
/// progress message and older clients can continue to deserialize the event.
pub(super) fn source_topology_details(source_path: &Path) -> String {
    #[cfg(target_os = "linux")]
    {
        let Ok(source_path) = source_path.canonicalize() else {
            return "mount_point=unknown filesystem=unknown backing_device=unknown major_minor=unknown verification=unavailable"
                .to_string();
        };
        let Ok(mountinfo) = fs::read_to_string("/proc/self/mountinfo") else {
            return "mount_point=unknown filesystem=unknown backing_device=unknown major_minor=unknown verification=unavailable"
                .to_string();
        };
        let Some(mount) = matching_mount(&source_path, &mountinfo) else {
            return "mount_point=unknown filesystem=unknown backing_device=unknown major_minor=unknown verification=unavailable"
                .to_string();
        };
        let backing_device = if mount.source == "-" || mount.source.is_empty() {
            "unknown"
        } else {
            mount.source.as_str()
        };
        return format!(
            "mount_point={} filesystem={} backing_device={} major_minor={} verification={}",
            mount.mount_point.display(),
            mount.filesystem_type,
            backing_device,
            mount.major_minor,
            if source_mount_is_server_local(&mount) {
                "verified-server-local"
            } else {
                "external-or-unverified"
            }
        );
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = source_path;
        "mount_point=unknown filesystem=unknown backing_device=unknown major_minor=unknown verification=unavailable".to_string()
    }
}

pub(super) fn source_is_server_local(source_path: &Path) -> bool {
    source_is_server_local_impl(source_path)
}

fn verified_ingress_origin_with_source_local(
    requested_origin: DaemonIngressOrigin,
    source_is_server_local: bool,
) -> DaemonIngressOrigin {
    match requested_origin {
        DaemonIngressOrigin::RemoteS3
        | DaemonIngressOrigin::WebUpload
        | DaemonIngressOrigin::Synoptikon
        | DaemonIngressOrigin::Mneion => requested_origin,
        DaemonIngressOrigin::LocalServer
        | DaemonIngressOrigin::LocalServerSsdFirst
        | DaemonIngressOrigin::LocalServerDirectImport => {
            if source_is_server_local {
                requested_origin
            } else {
                DaemonIngressOrigin::UsbMountedDisk
            }
        }
        DaemonIngressOrigin::UsbMountedDisk => {
            if source_is_server_local {
                DaemonIngressOrigin::LocalServer
            } else {
                DaemonIngressOrigin::UsbMountedDisk
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn source_is_server_local_impl(source_path: &Path) -> bool {
    let Ok(source_path) = source_path.canonicalize() else {
        return false;
    };
    let Ok(mountinfo) = fs::read_to_string("/proc/self/mountinfo") else {
        return false;
    };
    let Some(mount) = matching_mount(&source_path, &mountinfo) else {
        return false;
    };
    source_mount_is_server_local(&mount)
}

#[cfg(target_os = "linux")]
fn source_mount_is_server_local(mount: &MountInfoEntry) -> bool {
    !REMOTE_OR_EXTERNAL_FILESYSTEMS.contains(&mount.filesystem_type.as_str())
        && block_device_is_server_local(&mount.major_minor)
}

#[cfg(not(target_os = "linux"))]
fn source_is_server_local_impl(_source_path: &Path) -> bool {
    false
}

#[cfg(target_os = "linux")]
fn block_device_is_server_local(major_minor: &str) -> bool {
    let sysfs_device = Path::new("/sys/dev/block").join(major_minor);
    let Ok(sysfs_device) = sysfs_device.canonicalize() else {
        return false;
    };
    let path = sysfs_device.to_string_lossy();
    if path.contains("/usb") || path.contains("/virtual/") {
        return false;
    }
    sysfs_device
        .ancestors()
        .take_while(|path| path.starts_with("/sys"))
        .all(|path| !sysfs_removable(path))
}

#[cfg(target_os = "linux")]
fn sysfs_removable(path: &Path) -> bool {
    fs::read_to_string(path.join("removable"))
        .map(|value| value.trim() == "1")
        .unwrap_or(false)
}

#[cfg(any(target_os = "linux", test))]
#[derive(Debug, Eq, PartialEq)]
struct MountInfoEntry {
    mount_point: PathBuf,
    major_minor: String,
    filesystem_type: String,
    source: String,
}

#[cfg(any(target_os = "linux", test))]
fn matching_mount(source_path: &Path, mountinfo: &str) -> Option<MountInfoEntry> {
    mountinfo
        .lines()
        .filter_map(parse_mountinfo_entry)
        .filter(|entry| source_path.starts_with(&entry.mount_point))
        .max_by_key(|entry| entry.mount_point.as_os_str().len())
}

#[cfg(any(target_os = "linux", test))]
fn parse_mountinfo_entry(line: &str) -> Option<MountInfoEntry> {
    let (before_separator, after_separator) = line.split_once(" - ")?;
    let fields = before_separator.split_whitespace().collect::<Vec<_>>();
    let filesystem_fields = after_separator.split_whitespace().collect::<Vec<_>>();
    Some(MountInfoEntry {
        major_minor: fields.get(2)?.to_string(),
        mount_point: PathBuf::from(unescape_mountinfo_path(fields.get(4)?)),
        filesystem_type: filesystem_fields.first()?.to_string(),
        source: unescape_mountinfo_path(filesystem_fields.get(1).copied().unwrap_or("-")),
    })
}

#[cfg(any(target_os = "linux", test))]
fn unescape_mountinfo_path(value: &str) -> String {
    value
        .replace("\\134", "\\")
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
}

#[cfg(test)]
mod tests {
    use super::{
        matching_mount, parse_mountinfo_entry, source_topology_details, unescape_mountinfo_path,
        verified_ingress_origin_with_source_local,
    };
    use crate::api::DaemonIngressOrigin;
    use std::path::Path;

    const MOUNTINFO: &str = "25 0 259:2 / / rw,relatime - ext4 /dev/nvme0n1p2 rw\n\
36 25 0:32 / /mnt/external rw,relatime - fuseblk /dev/fuse rw\n\
42 25 0:49 / /mnt/nfs rw,relatime - nfs4 server:/share rw\n";

    #[test]
    fn selects_the_most_specific_source_mount() {
        let mount = matching_mount(Path::new("/mnt/nfs/run/file.fastq"), MOUNTINFO)
            .expect("matching mount");
        assert_eq!(mount.filesystem_type, "nfs4");
        assert_eq!(mount.major_minor, "0:49");
        assert_eq!(mount.source, "server:/share");
    }

    #[test]
    fn parses_mountinfo_entries_with_escaped_paths() {
        let entry =
            parse_mountinfo_entry("36 25 8:1 / /mnt/my\\040disk rw,relatime - ext4 /dev/sdb1 rw")
                .expect("entry parses");
        assert_eq!(entry.mount_point, Path::new("/mnt/my disk"));
        assert_eq!(entry.filesystem_type, "ext4");
        assert_eq!(entry.source, "/dev/sdb1");
        assert_eq!(unescape_mountinfo_path("a\\134b"), "a\\b");
    }

    #[test]
    fn unresolved_source_reports_explicit_unknown_topology_details() {
        let details = source_topology_details(Path::new(
            "/dasobjectstore/nonexistent/source-that-cannot-be-mounted",
        ));
        assert!(details.contains("mount_point=unknown"));
        assert!(details.contains("filesystem=unknown"));
        assert!(details.contains("backing_device=unknown"));
        assert!(details.contains("major_minor=unknown"));
        assert!(details.contains("verification=unavailable"));
    }

    #[test]
    fn local_hints_fail_closed_when_the_daemon_cannot_verify_the_source() {
        assert_eq!(
            verified_ingress_origin_with_source_local(DaemonIngressOrigin::LocalServer, false),
            DaemonIngressOrigin::UsbMountedDisk
        );
        assert_eq!(
            verified_ingress_origin_with_source_local(
                DaemonIngressOrigin::LocalServerDirectImport,
                false
            ),
            DaemonIngressOrigin::UsbMountedDisk
        );
        assert_eq!(
            verified_ingress_origin_with_source_local(DaemonIngressOrigin::UsbMountedDisk, true),
            DaemonIngressOrigin::LocalServer
        );
    }

    #[test]
    fn remote_api_origins_cannot_be_promoted_by_a_local_path() {
        assert_eq!(
            verified_ingress_origin_with_source_local(DaemonIngressOrigin::RemoteS3, true),
            DaemonIngressOrigin::RemoteS3
        );
        assert_eq!(
            verified_ingress_origin_with_source_local(DaemonIngressOrigin::WebUpload, true),
            DaemonIngressOrigin::WebUpload
        );
    }
}
