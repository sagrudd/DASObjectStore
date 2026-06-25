use crate::manifest::{DiskManifest, PoolManifest};
use crate::snapshot::{DISK_MANIFEST_FILE_NAME, POOL_MANIFEST_FILE_NAME};
use crate::{METADATA_DIR_NAME, SNAPSHOT_DIR_NAME};
use dasobjectstore_core::ids::PoolId;
use dasobjectstore_core::lifecycle::PoolState;
use std::fmt::{self, Display};
use std::fs::File;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PoolInspectSummary {
    pub metadata_path: PathBuf,
    pub pool_id: PoolId,
    pub state: PoolState,
    pub created_at_utc: String,
    pub updated_at_utc: String,
    pub disk_count: usize,
}

pub fn inspect_pool_metadata(
    path: impl AsRef<Path>,
) -> Result<PoolInspectSummary, PoolInspectError> {
    let metadata_path = resolve_metadata_snapshot_path(path.as_ref())?;
    let pool_manifest: PoolManifest = read_json(metadata_path.join(POOL_MANIFEST_FILE_NAME))?;
    let disk_manifest: DiskManifest = read_json(metadata_path.join(DISK_MANIFEST_FILE_NAME))?;

    if pool_manifest.pool_id != disk_manifest.pool_id {
        return Err(PoolInspectError::ManifestPoolMismatch {
            pool_manifest_pool_id: pool_manifest.pool_id.to_string(),
            disk_manifest_pool_id: disk_manifest.pool_id.to_string(),
        });
    }

    Ok(PoolInspectSummary {
        metadata_path,
        pool_id: pool_manifest.pool_id,
        state: pool_manifest.state,
        created_at_utc: pool_manifest.created_at_utc,
        updated_at_utc: pool_manifest.updated_at_utc,
        disk_count: disk_manifest.disks.len(),
    })
}

fn resolve_metadata_snapshot_path(path: &Path) -> Result<PathBuf, PoolInspectError> {
    if has_snapshot_manifests(path) {
        return Ok(path.to_path_buf());
    }

    let canonical_snapshot_path = path.join(METADATA_DIR_NAME).join(SNAPSHOT_DIR_NAME);
    if has_snapshot_manifests(&canonical_snapshot_path) {
        return Ok(canonical_snapshot_path);
    }

    Err(PoolInspectError::MissingMetadataSnapshot {
        path: path.to_path_buf(),
    })
}

fn has_snapshot_manifests(path: &Path) -> bool {
    path.join(POOL_MANIFEST_FILE_NAME).is_file() && path.join(DISK_MANIFEST_FILE_NAME).is_file()
}

fn read_json<T>(path: PathBuf) -> Result<T, PoolInspectError>
where
    T: serde::de::DeserializeOwned,
{
    let file = File::open(path)?;

    Ok(serde_json::from_reader(file)?)
}

#[derive(Debug)]
pub enum PoolInspectError {
    Io(std::io::Error),
    Json(serde_json::Error),
    MissingMetadataSnapshot {
        path: PathBuf,
    },
    ManifestPoolMismatch {
        pool_manifest_pool_id: String,
        disk_manifest_pool_id: String,
    },
}

impl Display for PoolInspectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(formatter, "failed to read pool metadata: {err}"),
            Self::Json(err) => write!(formatter, "failed to parse pool metadata: {err}"),
            Self::MissingMetadataSnapshot { path } => write!(
                formatter,
                "failed to find pool metadata snapshot at {} or {}/{}/{}",
                path.to_string_lossy(),
                path.to_string_lossy(),
                METADATA_DIR_NAME,
                SNAPSHOT_DIR_NAME
            ),
            Self::ManifestPoolMismatch {
                pool_manifest_pool_id,
                disk_manifest_pool_id,
            } => write!(
                formatter,
                "pool metadata manifests disagree on pool id: pool manifest has `{pool_manifest_pool_id}`, disk manifest has `{disk_manifest_pool_id}`"
            ),
        }
    }
}

impl std::error::Error for PoolInspectError {}

impl From<std::io::Error> for PoolInspectError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<serde_json::Error> for PoolInspectError {
    fn from(err: serde_json::Error) -> Self {
        Self::Json(err)
    }
}

#[cfg(test)]
mod tests {
    use super::inspect_pool_metadata;
    use crate::format::{FormatVersion, MetadataArtifact};
    use crate::manifest::{
        ArtifactReference, DiskManifest, DiskManifestEntry, DiskRole, PoolManifest,
    };
    use crate::snapshot::{
        DISK_MANIFEST_FILE_NAME, PLACEMENT_LOG_FILE_NAME, POOL_MANIFEST_FILE_NAME,
    };
    use dasobjectstore_core::ids::{DiskId, PoolId};
    use dasobjectstore_core::lifecycle::{DiskState, PoolState};
    use std::fs::{self, File};
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn inspects_manifest_metadata_directory() {
        let root = temp_root("inspect-manifest");
        write_snapshot_manifests(&root, "pool-a");

        let summary = inspect_pool_metadata(&root).expect("metadata inspects");

        assert_eq!(summary.metadata_path, root);
        assert_eq!(summary.pool_id.as_str(), "pool-a");
        assert_eq!(summary.state, PoolState::Clean);
        assert_eq!(summary.disk_count, 1);

        fs::remove_dir_all(summary.metadata_path).expect("cleanup temp root");
    }

    #[test]
    fn inspects_snapshot_under_portable_pool_root() {
        let root = temp_root("inspect-portable-root");
        let metadata_path = root.join(".dasobjectstore").join("metadata");
        write_snapshot_manifests(&metadata_path, "pool-a");

        let summary = inspect_pool_metadata(&root).expect("metadata inspects");

        assert_eq!(summary.metadata_path, metadata_path);
        assert_eq!(summary.pool_id.as_str(), "pool-a");
        assert_eq!(summary.disk_count, 1);

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    fn write_snapshot_manifests(path: &Path, pool_id: &str) {
        fs::create_dir_all(path).expect("create metadata dir");
        let pool_id = PoolId::new(pool_id).expect("pool id");
        let pool_manifest = PoolManifest::new(
            pool_id.clone(),
            PoolState::Clean,
            "2026-01-02T00:00:00Z",
            "2026-01-03T00:00:00Z",
            ArtifactReference::new(
                MetadataArtifact::DiskManifest,
                FormatVersion::new(MetadataArtifact::DiskManifest, 0, 1),
                DISK_MANIFEST_FILE_NAME,
                None,
            ),
            ArtifactReference::new(
                MetadataArtifact::PlacementLog,
                FormatVersion::new(MetadataArtifact::PlacementLog, 0, 1),
                PLACEMENT_LOG_FILE_NAME,
                None,
            ),
        );
        let disk_manifest = DiskManifest::new(
            pool_id,
            "2026-01-03T00:00:00Z",
            vec![DiskManifestEntry::new(
                DiskId::new("disk-a").expect("disk id"),
                DiskState::Healthy,
                DiskRole::HddCapacity,
                "2026-01-02T00:00:00Z",
                "2026-01-03T00:00:00Z",
            )],
        );

        write_json(&path.join(POOL_MANIFEST_FILE_NAME), &pool_manifest);
        write_json(&path.join(DISK_MANIFEST_FILE_NAME), &disk_manifest);
    }

    fn write_json(path: &Path, value: &impl serde::Serialize) {
        let file = File::create(path).expect("create json");
        serde_json::to_writer_pretty(file, value).expect("write json");
    }

    fn temp_root(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dasobjectstore-{name}-{}-{nanos}",
            std::process::id()
        ))
    }
}
