//! Local device-root discovery and validation for daemon file ingest.

use super::DaemonIngestFilesRuntimeError;
use dasobjectstore_core::ids::DiskId;
use dasobjectstore_metadata::{DiskCopyRoot, LIVE_SQLITE_FILE_NAME, METADATA_DIR_NAME};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub(super) const SSD_ROOT_ENV: &str = "DASOBJECTSTORE_SSD_ROOT";
const HDD_ROOT_ENV: &str = "DASOBJECTSTORE_HDD_ROOT";
const DEFAULT_SSD_ROOT: &str = "/srv/dasobjectstore/ssd";
const DEFAULT_HDD_ROOT: &str = "/srv/dasobjectstore/hdd";

pub(crate) fn default_ssd_root() -> PathBuf {
    std::env::var_os(SSD_ROOT_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_SSD_ROOT))
}

pub(crate) fn default_hdd_root() -> PathBuf {
    std::env::var_os(HDD_ROOT_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_HDD_ROOT))
}

pub(super) fn default_live_sqlite_path() -> PathBuf {
    default_ssd_root()
        .join(METADATA_DIR_NAME)
        .join(LIVE_SQLITE_FILE_NAME)
}

pub(super) fn validate_known_ssd_root(path: &Path) -> Result<(), DaemonIngestFilesRuntimeError> {
    let marker = read_device_marker(path).map_err(|err| {
        DaemonIngestFilesRuntimeError::CommandFailed(format!(
            "{} is not a known DASObjectStore SSD root: {err}",
            path.display()
        ))
    })?;
    if !marker.lines().any(|line| line == "role=ssd") {
        return Err(DaemonIngestFilesRuntimeError::CommandFailed(format!(
            "{} is not a DASObjectStore SSD root; expected role=ssd in .dasobjectstore/device.env",
            path.display()
        )));
    }

    Ok(())
}

pub(crate) fn discover_managed_hdd_roots(
    hdd_root: &Path,
) -> Result<Vec<DiskCopyRoot>, DaemonIngestFilesRuntimeError> {
    let mut roots = Vec::new();
    let entries = match fs::read_dir(hdd_root) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(roots),
        Err(err) => return Err(DaemonIngestFilesRuntimeError::Io(err)),
    };

    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let root_path = entry.path();
        let marker = match read_device_marker(&root_path) {
            Ok(marker) => marker,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => return Err(DaemonIngestFilesRuntimeError::Io(err)),
        };
        let Some(disk_id) = hdd_disk_id_from_marker(&marker)? else {
            continue;
        };
        roots.push(DiskCopyRoot::new(disk_id, root_path));
    }

    roots.sort_by(|left, right| left.disk_id.cmp(&right.disk_id));
    Ok(roots)
}

fn read_device_marker(path: &Path) -> Result<String, io::Error> {
    fs::read_to_string(path.join(".dasobjectstore").join("device.env"))
}

fn hdd_disk_id_from_marker(marker: &str) -> Result<Option<DiskId>, DaemonIngestFilesRuntimeError> {
    for line in marker.lines() {
        let Some(role) = line.strip_prefix("role=") else {
            continue;
        };
        let Some(disk_id) = role.strip_prefix("hdd:") else {
            return Ok(None);
        };
        return DiskId::new(disk_id)
            .map(Some)
            .map_err(|err| DaemonIngestFilesRuntimeError::CommandFailed(err.to_string()));
    }

    Ok(None)
}
