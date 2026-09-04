//! Platform probing boundary for macOS and Linux.

pub mod drive;
pub mod enclosure;
pub mod health;
pub mod linux;
pub mod linux_smart;
pub mod macos;
pub mod macos_health;
pub mod model;
pub mod probe;
pub mod r237_bootstrap_observer;

pub use drive::{
    validate_drive_profile, DriveProfileObservation, DriveProfileValidationError,
    ObservedDriveMedia, ValidatedDriveProfile,
};
pub use enclosure::{group_enclosures, with_enclosure_groups};
pub use model::{
    EnclosureIdentity, FilesystemHint, HostPlatform, ObservedDisk, ObservedEnclosure,
    PartitionHint, ProbeReport, ProbeWarning, Transport,
};
pub use probe::{ProbeError, ProbeProvider};
pub use r237_bootstrap_observer::{LinuxR237BootstrapObserver, R237BootstrapReadOnlyObserver};

/// Returns the platform crate version.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
