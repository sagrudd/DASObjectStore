//! Platform probing boundary for macOS and Linux.

pub mod model;
pub mod probe;

pub use model::{
    EnclosureIdentity, FilesystemHint, HostPlatform, ObservedDisk, ObservedEnclosure,
    PartitionHint, ProbeReport, ProbeWarning, Transport,
};
pub use probe::{ProbeError, ProbeProvider};

/// Returns the platform crate version.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
