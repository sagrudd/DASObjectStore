use std::fmt::{self, Display};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MetadataArtifact {
    LiveSqlite,
    PoolManifest,
    DiskManifest,
    PlacementLog,
}

impl MetadataArtifact {
    pub fn name(self) -> &'static str {
        match self {
            Self::LiveSqlite => "live_sqlite",
            Self::PoolManifest => "pool_manifest",
            Self::DiskManifest => "disk_manifest",
            Self::PlacementLog => "placement_log",
        }
    }
}

impl Display for MetadataArtifact {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FormatVersion {
    pub artifact: MetadataArtifact,
    pub major: u16,
    pub minor: u16,
}

impl FormatVersion {
    pub const fn new(artifact: MetadataArtifact, major: u16, minor: u16) -> Self {
        Self {
            artifact,
            major,
            minor,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FormatVersion, MetadataArtifact};

    #[test]
    fn artifact_names_are_stable_snake_case() {
        assert_eq!(MetadataArtifact::LiveSqlite.name(), "live_sqlite");
        assert_eq!(MetadataArtifact::PoolManifest.name(), "pool_manifest");
        assert_eq!(MetadataArtifact::DiskManifest.name(), "disk_manifest");
        assert_eq!(MetadataArtifact::PlacementLog.name(), "placement_log");
    }

    #[test]
    fn constructs_format_version() {
        let version = FormatVersion::new(MetadataArtifact::PoolManifest, 0, 1);

        assert_eq!(version.artifact, MetadataArtifact::PoolManifest);
        assert_eq!(version.major, 0);
        assert_eq!(version.minor, 1);
    }
}
