//! Portable metadata boundary for DASObjectStore pools.

pub mod format;
pub mod manifest;
pub mod schema;

pub use format::{FormatVersion, MetadataArtifact};
pub use manifest::{ArtifactReference, PoolManifest, POOL_MANIFEST_FORMAT_VERSION};
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
