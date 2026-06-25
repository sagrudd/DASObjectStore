use crate::format::{FormatVersion, MetadataArtifact};
use dasobjectstore_core::ids::PoolId;
use dasobjectstore_core::lifecycle::PoolState;
use serde::{Deserialize, Serialize};

pub const POOL_MANIFEST_FORMAT_VERSION: FormatVersion =
    FormatVersion::new(MetadataArtifact::PoolManifest, 0, 1);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PoolManifest {
    pub format_version: FormatVersion,
    pub pool_id: PoolId,
    pub state: PoolState,
    pub created_at_utc: String,
    pub updated_at_utc: String,
    pub disk_manifest: ArtifactReference,
    pub placement_log: ArtifactReference,
}

impl PoolManifest {
    pub fn new(
        pool_id: PoolId,
        state: PoolState,
        created_at_utc: impl Into<String>,
        updated_at_utc: impl Into<String>,
        disk_manifest: ArtifactReference,
        placement_log: ArtifactReference,
    ) -> Self {
        Self {
            format_version: POOL_MANIFEST_FORMAT_VERSION,
            pool_id,
            state,
            created_at_utc: created_at_utc.into(),
            updated_at_utc: updated_at_utc.into(),
            disk_manifest,
            placement_log,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ArtifactReference {
    pub artifact: MetadataArtifact,
    pub format_version: FormatVersion,
    pub relative_path: String,
    pub checksum: Option<String>,
}

impl ArtifactReference {
    pub fn new(
        artifact: MetadataArtifact,
        format_version: FormatVersion,
        relative_path: impl Into<String>,
        checksum: Option<String>,
    ) -> Self {
        Self {
            artifact,
            format_version,
            relative_path: relative_path.into(),
            checksum,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ArtifactReference, PoolManifest, POOL_MANIFEST_FORMAT_VERSION};
    use crate::format::{FormatVersion, MetadataArtifact};
    use dasobjectstore_core::ids::PoolId;
    use dasobjectstore_core::lifecycle::PoolState;

    #[test]
    fn pool_manifest_uses_canonical_format_version() {
        assert_eq!(
            POOL_MANIFEST_FORMAT_VERSION.artifact,
            MetadataArtifact::PoolManifest
        );
        assert_eq!(POOL_MANIFEST_FORMAT_VERSION.major, 0);
        assert_eq!(POOL_MANIFEST_FORMAT_VERSION.minor, 1);
    }

    #[test]
    fn serializes_pool_manifest_with_version_and_artifact_references() {
        let manifest = sample_manifest();

        let encoded = serde_json::to_value(&manifest).expect("manifest serializes");

        assert_eq!(encoded["format_version"]["artifact"], "pool_manifest");
        assert_eq!(encoded["format_version"]["major"], 0);
        assert_eq!(encoded["pool_id"], "pool-a");
        assert_eq!(encoded["state"], "Clean");
        assert_eq!(encoded["disk_manifest"]["artifact"], "disk_manifest");
        assert_eq!(
            encoded["placement_log"]["relative_path"],
            "metadata/placement-log.jsonl"
        );
    }

    #[test]
    fn round_trips_pool_manifest() {
        let manifest = sample_manifest();

        let encoded = serde_json::to_string(&manifest).expect("manifest serializes");
        let decoded: PoolManifest = serde_json::from_str(&encoded).expect("manifest deserializes");

        assert_eq!(decoded, manifest);
    }

    fn sample_manifest() -> PoolManifest {
        PoolManifest::new(
            PoolId::new("pool-a").expect("pool id"),
            PoolState::Clean,
            "2026-01-01T00:00:00Z",
            "2026-01-02T00:00:00Z",
            ArtifactReference::new(
                MetadataArtifact::DiskManifest,
                FormatVersion::new(MetadataArtifact::DiskManifest, 0, 1),
                "metadata/disk-manifest.json",
                Some("sha256:disk-manifest".to_string()),
            ),
            ArtifactReference::new(
                MetadataArtifact::PlacementLog,
                FormatVersion::new(MetadataArtifact::PlacementLog, 0, 1),
                "metadata/placement-log.jsonl",
                None,
            ),
        )
    }
}
