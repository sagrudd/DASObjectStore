//! Portable metadata boundary for DASObjectStore pools.

pub mod format;

pub use format::{FormatVersion, MetadataArtifact};

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
