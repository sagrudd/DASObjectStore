use crate::format::{FormatVersion, MetadataArtifact};
use dasobjectstore_core::ids::{DiskId, PoolId};
use dasobjectstore_core::lifecycle::{DiskState, PoolState};
use serde::{Deserialize, Serialize};

pub const POOL_MANIFEST_FORMAT_VERSION: FormatVersion =
    FormatVersion::new(MetadataArtifact::PoolManifest, 0, 1);
pub const DISK_MANIFEST_FORMAT_VERSION: FormatVersion =
    FormatVersion::new(MetadataArtifact::DiskManifest, 0, 1);

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
pub struct DiskManifest {
    pub format_version: FormatVersion,
    pub pool_id: PoolId,
    pub generated_at_utc: String,
    pub disks: Vec<DiskManifestEntry>,
}

impl DiskManifest {
    pub fn new(
        pool_id: PoolId,
        generated_at_utc: impl Into<String>,
        disks: Vec<DiskManifestEntry>,
    ) -> Self {
        Self {
            format_version: DISK_MANIFEST_FORMAT_VERSION,
            pool_id,
            generated_at_utc: generated_at_utc.into(),
            disks,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiskManifestEntry {
    pub disk_id: DiskId,
    pub state: DiskState,
    pub role: DiskRole,
    pub size_bytes: Option<u64>,
    pub serial_hint: Option<String>,
    pub model_hint: Option<String>,
    pub enclosure_topology_path: Option<String>,
    pub bay_hint: Option<String>,
    pub filesystem_fingerprint: Option<String>,
    pub partition_fingerprint: Option<String>,
    pub created_at_utc: String,
    pub updated_at_utc: String,
}

impl DiskManifestEntry {
    pub fn new(
        disk_id: DiskId,
        state: DiskState,
        role: DiskRole,
        created_at_utc: impl Into<String>,
        updated_at_utc: impl Into<String>,
    ) -> Self {
        Self {
            disk_id,
            state,
            role,
            size_bytes: None,
            serial_hint: None,
            model_hint: None,
            enclosure_topology_path: None,
            bay_hint: None,
            filesystem_fingerprint: None,
            partition_fingerprint: None,
            created_at_utc: created_at_utc.into(),
            updated_at_utc: updated_at_utc.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiskRole {
    IngestSsd,
    HddCapacity,
    Replacement,
    Retired,
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
    use super::{
        ArtifactReference, DiskManifest, DiskManifestEntry, DiskRole, PoolManifest,
        DISK_MANIFEST_FORMAT_VERSION, POOL_MANIFEST_FORMAT_VERSION,
    };
    use crate::format::{FormatVersion, MetadataArtifact};
    use dasobjectstore_core::ids::{DiskId, PoolId};
    use dasobjectstore_core::lifecycle::{DiskState, PoolState};

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

    #[test]
    fn disk_manifest_uses_canonical_format_version() {
        assert_eq!(
            DISK_MANIFEST_FORMAT_VERSION.artifact,
            MetadataArtifact::DiskManifest
        );
        assert_eq!(DISK_MANIFEST_FORMAT_VERSION.major, 0);
        assert_eq!(DISK_MANIFEST_FORMAT_VERSION.minor, 1);
    }

    #[test]
    fn serializes_disk_manifest_with_composite_identity_hints() {
        let manifest = sample_disk_manifest();

        let encoded = serde_json::to_value(&manifest).expect("manifest serializes");

        assert_eq!(encoded["format_version"]["artifact"], "disk_manifest");
        assert_eq!(encoded["pool_id"], "pool-a");
        assert_eq!(encoded["disks"][0]["disk_id"], "disk-a");
        assert_eq!(encoded["disks"][0]["state"], "Healthy");
        assert_eq!(encoded["disks"][0]["role"], "hdd_capacity");
        assert_eq!(encoded["disks"][0]["serial_hint"], "WD-OLD-001");
        assert_eq!(
            encoded["disks"][0]["filesystem_fingerprint"],
            "fs:ext4:disk-a"
        );
    }

    #[test]
    fn round_trips_disk_manifest() {
        let manifest = sample_disk_manifest();

        let encoded = serde_json::to_string(&manifest).expect("manifest serializes");
        let decoded: DiskManifest = serde_json::from_str(&encoded).expect("manifest deserializes");

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

    fn sample_disk_manifest() -> DiskManifest {
        let mut disk = DiskManifestEntry::new(
            DiskId::new("disk-a").expect("disk id"),
            DiskState::Healthy,
            DiskRole::HddCapacity,
            "2026-01-01T00:00:00Z",
            "2026-01-02T00:00:00Z",
        );
        disk.size_bytes = Some(4_000_787_030_016);
        disk.serial_hint = Some("WD-OLD-001".to_string());
        disk.model_hint = Some("WDC WD40EFRX".to_string());
        disk.enclosure_topology_path = Some("usb@001/002".to_string());
        disk.bay_hint = Some("bay-1".to_string());
        disk.filesystem_fingerprint = Some("fs:ext4:disk-a".to_string());
        disk.partition_fingerprint = Some("part:gpt:disk-a".to_string());

        DiskManifest::new(
            PoolId::new("pool-a").expect("pool id"),
            "2026-01-02T00:00:00Z",
            vec![disk],
        )
    }
}
