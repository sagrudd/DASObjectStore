//! Object service orchestration boundary.

/// Returns the object service crate version.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
