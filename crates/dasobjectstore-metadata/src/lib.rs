//! Portable metadata boundary for DASObjectStore pools.

pub mod format;
pub mod initialize;
pub mod manifest;
pub mod placement_log;
pub mod schema;

pub use format::{FormatVersion, MetadataArtifact};
pub use initialize::{
    initialize_pool, MetadataInitError, PoolInitOptions, PoolInitReport, LIVE_SQLITE_FILE_NAME,
    METADATA_DIR_NAME, SNAPSHOT_DIR_NAME,
};
pub use manifest::{
    ArtifactReference, DiskManifest, DiskManifestEntry, PoolManifest, DISK_MANIFEST_FORMAT_VERSION,
    POOL_MANIFEST_FORMAT_VERSION,
};
pub use placement_log::{PlacementLogEvent, PlacementLogRecord, PLACEMENT_LOG_FORMAT_VERSION};
pub use schema::{LIVE_SCHEMA_FORMAT_VERSION, LIVE_SCHEMA_SQL};

/// Returns the metadata crate version.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    #[test]
    fn exposes_package_version() {
        assert_eq!(super::version(), env!("CARGO_PKG_VERSION"));
    }
}
