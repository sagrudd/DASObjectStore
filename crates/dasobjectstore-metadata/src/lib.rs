//! Portable metadata boundary for DASObjectStore pools.

pub mod format;
pub mod initialize;
pub mod inspect;
pub mod manifest;
pub mod markers;
pub mod placement_log;
pub mod schema;
pub mod snapshot;

pub use format::{FormatVersion, MetadataArtifact};
pub use initialize::{
    initialize_pool, MetadataInitError, PoolInitOptions, PoolInitReport, LIVE_SQLITE_FILE_NAME,
    METADATA_DIR_NAME, SNAPSHOT_DIR_NAME,
};
pub use inspect::{inspect_pool_metadata, PoolInspectError, PoolInspectSummary};
pub use manifest::{
    ArtifactReference, DiskManifest, DiskManifestEntry, PoolManifest, DISK_MANIFEST_FORMAT_VERSION,
    POOL_MANIFEST_FORMAT_VERSION,
};
pub use markers::{record_pool_state_marker, PoolStateMarker, PoolStateMarkerKind};
pub use placement_log::{PlacementLogEvent, PlacementLogRecord, PLACEMENT_LOG_FORMAT_VERSION};
pub use schema::{LIVE_SCHEMA_FORMAT_VERSION, LIVE_SCHEMA_SQL};
pub use snapshot::{
    export_metadata_snapshot, import_metadata_snapshot, SnapshotExportError, SnapshotExportOptions,
    SnapshotExportReport, SnapshotImportError, SnapshotImportOptions, SnapshotImportReport,
    DISK_MANIFEST_FILE_NAME, PLACEMENT_LOG_FILE_NAME, POOL_MANIFEST_FILE_NAME,
};

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
