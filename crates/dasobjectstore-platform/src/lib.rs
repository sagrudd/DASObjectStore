//! Platform probing boundary for macOS and Linux.

pub mod enclosure;
pub mod linux;
pub mod macos;
pub mod model;
pub mod probe;

pub use enclosure::{group_enclosures, with_enclosure_groups};
pub use model::{
    EnclosureIdentity, FilesystemHint, HostPlatform, ObservedDisk, ObservedEnclosure,
    PartitionHint, ProbeReport, ProbeWarning, Transport,
};
pub use probe::{ProbeError, ProbeProvider};

/// Returns the platform crate version.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
