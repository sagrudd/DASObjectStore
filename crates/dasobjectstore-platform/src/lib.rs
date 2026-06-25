//! Platform probing boundary for macOS and Linux.

/// Returns the platform crate version.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
