//! Portable metadata boundary for DASObjectStore pools.

/// Returns the metadata crate version.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
