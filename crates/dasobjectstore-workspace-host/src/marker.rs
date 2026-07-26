use crate::{BranchPlan, BrokerError};
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

pub const MARKER_SCHEMA_VERSION: u32 = 1;
pub const MARKER_FILE: &str = ".dasobjectstore-workspace.json";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BranchMarker {
    pub schema_version: u32,
    pub workspace_id: String,
    pub disk_id: String,
    pub branch_id: String,
    pub project_id: u32,
    pub quota_bytes: u64,
}

impl BranchMarker {
    pub fn expected(workspace_id: &str, branch: &BranchPlan) -> Self {
        Self {
            schema_version: MARKER_SCHEMA_VERSION,
            workspace_id: workspace_id.to_string(),
            disk_id: branch.disk_id.clone(),
            branch_id: branch.branch_id.clone(),
            project_id: branch.project_id,
            quota_bytes: branch.quota_bytes,
        }
    }

    pub fn read(directory: &Path) -> Result<Option<Self>, BrokerError> {
        let path = directory.join(MARKER_FILE);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(BrokerError::Io("stat branch marker", error)),
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 16 * 1024 {
            return Err(BrokerError::UnsafeEntry(
                "branch marker is not a bounded regular file".to_string(),
            ));
        }
        let value =
            fs::read(&path).map_err(|error| BrokerError::Io("read branch marker", error))?;
        serde_json::from_slice(&value)
            .map(Some)
            .map_err(|error| BrokerError::Protocol(error.to_string()))
    }

    pub fn create_exclusive(&self, directory: &Path) -> Result<(), BrokerError> {
        let marker_path = directory.join(MARKER_FILE);
        let temporary_path = directory.join(format!(".{MARKER_FILE}.new"));
        let value =
            serde_json::to_vec(self).map_err(|error| BrokerError::Protocol(error.to_string()))?;
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary_path)
            .map_err(|error| BrokerError::Io("create temporary branch marker", error))?;
        file.write_all(&value)
            .and_then(|_| file.sync_all())
            .map_err(|error| BrokerError::Io("persist branch marker", error))?;
        fs::rename(&temporary_path, &marker_path)
            .map_err(|error| BrokerError::Io("publish branch marker", error))?;
        sync_directory(directory)
    }
}

pub(crate) fn sync_directory(path: &Path) -> Result<(), BrokerError> {
    let directory =
        fs::File::open(path).map_err(|error| BrokerError::Io("open directory for sync", error))?;
    directory
        .sync_all()
        .map_err(|error| BrokerError::Io("sync directory", error))
}
