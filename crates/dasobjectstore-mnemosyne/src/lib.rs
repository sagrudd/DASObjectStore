//! Mnemosyne and Synoptikon adapter boundary.

/// Returns the Mnemosyne adapter crate version.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
