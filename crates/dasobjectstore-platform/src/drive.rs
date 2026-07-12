//! Injected validation for the dedicated SSD drive profile.
//!
//! Runtime probing supplies these observations from diskutil, lsblk, or a
//! future host provider. Validation remains deterministic and does not touch
//! devices, mounts, or system state.

use dasobjectstore_core::manifest::{
    BackendReference, DriveMediaKind, ObjectStoreManifest, ObjectStoreManifestValidationError,
};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservedDriveMedia {
    Ssd,
    Rotational,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DriveProfileObservation {
    pub device_path: PathBuf,
    pub device_identity: Option<String>,
    pub filesystem_identity: Option<String>,
    pub media: ObservedDriveMedia,
    pub mount_path: Option<PathBuf>,
    pub mounted_read_only: Option<bool>,
    pub backs_system_root: Option<bool>,
    pub size_bytes: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedDriveProfile {
    pub device_path: PathBuf,
    pub device_identity: String,
    pub filesystem_identity: String,
    pub mount_path: PathBuf,
    pub media: DriveMediaKind,
    pub size_bytes: Option<u64>,
}

pub fn validate_drive_profile(
    manifest: &ObjectStoreManifest,
    observation: &DriveProfileObservation,
) -> Result<ValidatedDriveProfile, DriveProfileValidationError> {
    manifest
        .validate()
        .map_err(DriveProfileValidationError::Manifest)?;
    let BackendReference::Drive {
        filesystem_identity,
        device_identity,
        media,
        ..
    } = &manifest.backend
    else {
        return Err(DriveProfileValidationError::WrongProfile);
    };
    if observation.device_path.as_os_str().is_empty() {
        return Err(DriveProfileValidationError::MissingDeviceIdentity);
    }
    let Some(observed_device_identity) = observation
        .device_identity
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    else {
        return Err(DriveProfileValidationError::MissingDeviceIdentity);
    };
    if device_identity.as_deref() != Some(observed_device_identity) {
        return Err(DriveProfileValidationError::DeviceIdentityMismatch);
    }
    let Some(observed_filesystem_identity) = observation
        .filesystem_identity
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    else {
        return Err(DriveProfileValidationError::MissingFilesystemIdentity);
    };
    if filesystem_identity != observed_filesystem_identity {
        return Err(DriveProfileValidationError::FilesystemIdentityMismatch);
    }
    if *media != DriveMediaKind::Ssd || observation.media != ObservedDriveMedia::Ssd {
        return Err(match observation.media {
            ObservedDriveMedia::Rotational => DriveProfileValidationError::RotationalMedia,
            ObservedDriveMedia::Unknown => DriveProfileValidationError::MediaNotConfirmedSsd,
            ObservedDriveMedia::Ssd => DriveProfileValidationError::MediaNotConfirmedSsd,
        });
    }
    let Some(mount_path) = observation.mount_path.clone() else {
        return Err(DriveProfileValidationError::NotMounted);
    };
    if !mount_path.is_absolute() {
        return Err(DriveProfileValidationError::RelativeMount);
    }
    if mount_path == Path::new("/") {
        return Err(DriveProfileValidationError::SystemRootDevice);
    }
    if observation.backs_system_root != Some(false) {
        return Err(DriveProfileValidationError::SystemRootStatusUnknown);
    }
    if observation.mounted_read_only != Some(false) {
        return Err(DriveProfileValidationError::ReadOnlyMount);
    }
    Ok(ValidatedDriveProfile {
        device_path: observation.device_path.clone(),
        device_identity: observed_device_identity.to_string(),
        filesystem_identity: observed_filesystem_identity.to_string(),
        mount_path,
        media: *media,
        size_bytes: observation.size_bytes,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DriveProfileValidationError {
    Manifest(ObjectStoreManifestValidationError),
    WrongProfile,
    MediaNotConfirmedSsd,
    RotationalMedia,
    MissingDeviceIdentity,
    DeviceIdentityMismatch,
    MissingFilesystemIdentity,
    FilesystemIdentityMismatch,
    NotMounted,
    RelativeMount,
    SystemRootDevice,
    SystemRootStatusUnknown,
    ReadOnlyMount,
}

impl Display for DriveProfileValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Manifest(error) => return write!(formatter, "invalid drive manifest: {error}"),
            Self::WrongProfile => "manifest does not describe a drive profile",
            Self::MediaNotConfirmedSsd => "drive media is not positively confirmed as SSD",
            Self::RotationalMedia => "drive media is rotational, not SSD",
            Self::MissingDeviceIdentity => "drive observation lacks a stable device identity",
            Self::DeviceIdentityMismatch => "drive device identity does not match manifest",
            Self::MissingFilesystemIdentity => "drive observation lacks a filesystem identity",
            Self::FilesystemIdentityMismatch => "drive filesystem identity does not match manifest",
            Self::NotMounted => "drive filesystem is not mounted",
            Self::RelativeMount => "drive mount path must be absolute",
            Self::SystemRootDevice => "drive cannot claim the system root",
            Self::SystemRootStatusUnknown => "drive system-root status is not confirmed safe",
            Self::ReadOnlyMount => "drive mount is read-only or its mode is unknown",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for DriveProfileValidationError {}

#[cfg(test)]
mod tests {
    use super::*;
    use dasobjectstore_core::deployment::{DeploymentProfile, HostMode};
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_core::manifest::OBJECT_STORE_MANIFEST_SCHEMA_VERSION;
    use dasobjectstore_core::protection::ProtectionPolicy;

    fn manifest() -> ObjectStoreManifest {
        ObjectStoreManifest {
            schema_version: OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
            store_id: StoreId::new("codex-drive").expect("store id"),
            deployment_profile: DeploymentProfile::Drive,
            host_mode: HostMode::System,
            protection: ProtectionPolicy::Reproducible,
            backend: BackendReference::Drive {
                filesystem_identity: "apfs:fs-1".to_string(),
                device_identity: Some("disk:device-1".to_string()),
                media: DriveMediaKind::Ssd,
                mount_path_hint: Some(PathBuf::from("/Volumes/OLD")),
            },
        }
    }

    fn observation() -> DriveProfileObservation {
        DriveProfileObservation {
            device_path: PathBuf::from("/dev/disk4"),
            device_identity: Some("disk:device-1".to_string()),
            filesystem_identity: Some("apfs:fs-1".to_string()),
            media: ObservedDriveMedia::Ssd,
            mount_path: Some(PathBuf::from("/Volumes/CODEX")),
            mounted_read_only: Some(false),
            backs_system_root: Some(false),
            size_bytes: Some(1_000),
        }
    }

    #[test]
    fn accepts_matching_ssd_and_relocated_mount_hint() {
        let drive = validate_drive_profile(&manifest(), &observation()).expect("drive validates");
        assert_eq!(drive.mount_path, PathBuf::from("/Volumes/CODEX"));
        assert_eq!(drive.media, DriveMediaKind::Ssd);
    }

    #[test]
    fn rejects_non_ssd_or_unknown_media() {
        for media in [ObservedDriveMedia::Rotational, ObservedDriveMedia::Unknown] {
            let mut candidate = observation();
            candidate.media = media;
            assert!(matches!(
                validate_drive_profile(&manifest(), &candidate),
                Err(DriveProfileValidationError::RotationalMedia)
                    | Err(DriveProfileValidationError::MediaNotConfirmedSsd)
            ));
        }
    }

    #[test]
    fn rejects_missing_or_mismatched_identities() {
        let mut missing_device = observation();
        missing_device.device_identity = None;
        assert_eq!(
            validate_drive_profile(&manifest(), &missing_device),
            Err(DriveProfileValidationError::MissingDeviceIdentity)
        );
        let mut mismatch = observation();
        mismatch.filesystem_identity = Some("apfs:other".to_string());
        assert_eq!(
            validate_drive_profile(&manifest(), &mismatch),
            Err(DriveProfileValidationError::FilesystemIdentityMismatch)
        );
    }

    #[test]
    fn rejects_unmounted_relative_root_and_read_only_observations() {
        let mut unmounted = observation();
        unmounted.mount_path = None;
        assert_eq!(
            validate_drive_profile(&manifest(), &unmounted),
            Err(DriveProfileValidationError::NotMounted)
        );
        let mut relative = observation();
        relative.mount_path = Some(PathBuf::from("Volumes/CODEX"));
        assert_eq!(
            validate_drive_profile(&manifest(), &relative),
            Err(DriveProfileValidationError::RelativeMount)
        );
        let mut root = observation();
        root.mount_path = Some(PathBuf::from("/"));
        assert_eq!(
            validate_drive_profile(&manifest(), &root),
            Err(DriveProfileValidationError::SystemRootDevice)
        );
        let mut readonly = observation();
        readonly.mounted_read_only = None;
        assert_eq!(
            validate_drive_profile(&manifest(), &readonly),
            Err(DriveProfileValidationError::ReadOnlyMount)
        );
    }

    #[test]
    fn rejects_unknown_system_root_status_and_non_drive_manifest() {
        let mut unknown = observation();
        unknown.backs_system_root = None;
        assert_eq!(
            validate_drive_profile(&manifest(), &unknown),
            Err(DriveProfileValidationError::SystemRootStatusUnknown)
        );
        let mut folder_manifest = manifest();
        folder_manifest.deployment_profile = DeploymentProfile::Folder;
        folder_manifest.backend = BackendReference::Folder {
            root_identity: "fsid:folder".to_string(),
        };
        assert_eq!(
            validate_drive_profile(&folder_manifest, &observation()),
            Err(DriveProfileValidationError::WrongProfile)
        );
    }
}
