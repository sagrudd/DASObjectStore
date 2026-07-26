use crate::BrokerError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ManagedDiskRoot {
    pub root: PathBuf,
    pub workspace_directory: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BrokerConfig {
    pub schema_version: u32,
    pub disks: BTreeMap<String, ManagedDiskRoot>,
}

impl BrokerConfig {
    pub fn load_root_owned(path: &Path) -> Result<Self, BrokerError> {
        let metadata =
            fs::symlink_metadata(path).map_err(|error| BrokerError::Io("stat config", error))?;
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            return Err(BrokerError::UnsafeConfig(
                "configuration must be a regular non-symlink file".to_string(),
            ));
        }
        if metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
            return Err(BrokerError::UnsafeConfig(
                "configuration must be root-owned and not group/world writable".to_string(),
            ));
        }
        let value = fs::read(path).map_err(|error| BrokerError::Io("read broker config", error))?;
        let config: Self = serde_json::from_slice(&value)
            .map_err(|error| BrokerError::Protocol(error.to_string()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), BrokerError> {
        if self.schema_version != 1 {
            return Err(BrokerError::UnsafeConfig(
                "unsupported broker configuration schema".to_string(),
            ));
        }
        for (disk_id, disk) in &self.disks {
            validate_identity("disk_id", disk_id)?;
            validate_identity("workspace_directory", &disk.workspace_directory)?;
            if !disk.root.is_absolute() {
                return Err(BrokerError::UnsafeConfig(format!(
                    "managed root for {disk_id} is not absolute"
                )));
            }
            let metadata = fs::symlink_metadata(&disk.root)
                .map_err(|error| BrokerError::Io("stat managed root", error))?;
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(BrokerError::UnsafeConfig(format!(
                    "managed root for {disk_id} is not a real directory"
                )));
            }
        }
        Ok(())
    }
}

pub(crate) fn validate_identity(field: &'static str, value: &str) -> Result<(), BrokerError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        || value == "."
        || value == ".."
    {
        return Err(BrokerError::InvalidRequest(format!(
            "{field} must be a conservative path-free identity"
        )));
    }
    Ok(())
}
