//! Portable, versioned ObjectStore identity and backend-location contract.
//!
//! This contract deliberately keeps appliance pool/disk placement separate
//! from folder and drive identities. Existing metadata is not reinterpreted;
//! callers must explicitly write a manifest when adopting a profile.

use crate::deployment::{DeploymentProfile, HostMode};
use crate::ids::StoreId;
use crate::protection::ProtectionPolicy;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};
use std::path::Path;
use std::path::PathBuf;

pub const OBJECT_STORE_MANIFEST_SCHEMA_VERSION: u16 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectStoreManifest {
    pub schema_version: u16,
    pub store_id: StoreId,
    pub deployment_profile: DeploymentProfile,
    pub host_mode: HostMode,
    pub protection: ProtectionPolicy,
    pub backend: BackendReference,
}

impl ObjectStoreManifest {
    pub fn validate(&self) -> Result<(), ObjectStoreManifestValidationError> {
        if self.schema_version != OBJECT_STORE_MANIFEST_SCHEMA_VERSION {
            return Err(ObjectStoreManifestValidationError::UnsupportedSchema {
                schema_version: self.schema_version,
            });
        }
        self.backend.validate_for_profile(self.deployment_profile)
    }

    /// Decode a persisted manifest using the compatibility boundary. Schema is
    /// inspected before profile/backend fields so future versions fail with a
    /// typed error even when they introduce unknown enum variants.
    pub fn decode_json(input: &str) -> Result<Self, ObjectStoreManifestDecodeError> {
        let value: serde_json::Value = serde_json::from_str(input)
            .map_err(|error| ObjectStoreManifestDecodeError::MalformedJson(error.to_string()))?;
        let schema_version = value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u16::try_from(value).ok())
            .ok_or(ObjectStoreManifestDecodeError::MissingOrInvalidSchemaVersion)?;
        if schema_version != OBJECT_STORE_MANIFEST_SCHEMA_VERSION {
            return Err(ObjectStoreManifestDecodeError::UnsupportedSchema {
                found: schema_version,
                supported: OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
            });
        }
        let manifest: Self = serde_json::from_value(value)
            .map_err(|error| ObjectStoreManifestDecodeError::MalformedJson(error.to_string()))?;
        manifest
            .validate()
            .map_err(ObjectStoreManifestDecodeError::InvalidManifest)?;
        Ok(manifest)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BackendReference {
    Folder {
        /// Canonical root identity, not an untrusted user-facing path.
        root_identity: String,
    },
    Drive {
        filesystem_identity: String,
        device_identity: Option<String>,
        media: DriveMediaKind,
        #[serde(default)]
        mount_path_hint: Option<PathBuf>,
    },
    Appliance {
        pool_id: String,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DriveMediaKind {
    Ssd,
}

impl BackendReference {
    fn validate_for_profile(
        &self,
        profile: DeploymentProfile,
    ) -> Result<(), ObjectStoreManifestValidationError> {
        let expected = match self {
            Self::Folder { root_identity } => {
                if root_identity.trim().is_empty() {
                    return Err(ObjectStoreManifestValidationError::BlankBackendIdentity);
                }
                DeploymentProfile::Folder
            }
            Self::Drive {
                filesystem_identity,
                device_identity,
                media,
                mount_path_hint,
            } => {
                if filesystem_identity.trim().is_empty()
                    || device_identity
                        .as_deref()
                        .is_some_and(|identity| identity.trim().is_empty())
                {
                    return Err(ObjectStoreManifestValidationError::BlankBackendIdentity);
                }
                if device_identity.is_none() {
                    return Err(ObjectStoreManifestValidationError::MissingDeviceIdentity);
                }
                if mount_path_hint
                    .as_ref()
                    .is_some_and(|path| !path.is_absolute())
                {
                    return Err(ObjectStoreManifestValidationError::RelativeMountHint);
                }
                if mount_path_hint.as_deref() == Some(Path::new("/")) {
                    return Err(ObjectStoreManifestValidationError::SystemRootMount);
                }
                if *media != DriveMediaKind::Ssd {
                    return Err(ObjectStoreManifestValidationError::DriveMustBeSsd);
                }
                DeploymentProfile::Drive
            }
            Self::Appliance { pool_id } => {
                if pool_id.trim().is_empty() {
                    return Err(ObjectStoreManifestValidationError::BlankBackendIdentity);
                }
                DeploymentProfile::Appliance
            }
        };
        if profile != expected {
            return Err(ObjectStoreManifestValidationError::ProfileBackendMismatch {
                profile,
                backend: expected,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObjectStoreManifestValidationError {
    UnsupportedSchema {
        schema_version: u16,
    },
    BlankBackendIdentity,
    MissingDeviceIdentity,
    RelativeMountHint,
    SystemRootMount,
    DriveMustBeSsd,
    ProfileBackendMismatch {
        profile: DeploymentProfile,
        backend: DeploymentProfile,
    },
}

impl Display for ObjectStoreManifestValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema { schema_version } => {
                write!(
                    formatter,
                    "unsupported ObjectStore manifest schema {schema_version}"
                )
            }
            Self::BlankBackendIdentity => formatter.write_str("backend identity must not be blank"),
            Self::MissingDeviceIdentity => {
                formatter.write_str("drive backend requires a stable device identity")
            }
            Self::RelativeMountHint => {
                formatter.write_str("drive mount_path_hint must be absolute")
            }
            Self::SystemRootMount => {
                formatter.write_str("drive backend cannot target the system root")
            }
            Self::DriveMustBeSsd => {
                formatter.write_str("drive backend must declare non-rotational SSD media")
            }
            Self::ProfileBackendMismatch { profile, backend } => write!(
                formatter,
                "deployment profile {} does not match backend reference {}",
                profile.name(),
                backend.name()
            ),
        }
    }
}

impl std::error::Error for ObjectStoreManifestValidationError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObjectStoreManifestDecodeError {
    MalformedJson(String),
    MissingOrInvalidSchemaVersion,
    UnsupportedSchema { found: u16, supported: u16 },
    InvalidManifest(ObjectStoreManifestValidationError),
}

impl Display for ObjectStoreManifestDecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedJson(message) => {
                write!(formatter, "malformed ObjectStore manifest: {message}")
            }
            Self::MissingOrInvalidSchemaVersion => formatter
                .write_str("ObjectStore manifest schema_version must be an unsigned integer"),
            Self::UnsupportedSchema { found, supported } => write!(
                formatter,
                "unsupported ObjectStore manifest schema {found}; supported schema is {supported}"
            ),
            Self::InvalidManifest(error) => {
                write!(formatter, "invalid ObjectStore manifest: {error}")
            }
        }
    }
}

impl std::error::Error for ObjectStoreManifestDecodeError {}

#[cfg(test)]
mod tests {
    use super::{
        BackendReference, DriveMediaKind, ObjectStoreManifest, ObjectStoreManifestDecodeError,
        OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
    };
    use crate::deployment::{DeploymentProfile, HostMode};
    use crate::ids::StoreId;
    use crate::protection::ProtectionPolicy;
    use std::path::PathBuf;

    #[test]
    fn folder_manifest_uses_root_identity_and_stable_wire_shape() {
        let manifest = ObjectStoreManifest {
            schema_version: OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
            store_id: StoreId::new("codex").expect("store id"),
            deployment_profile: DeploymentProfile::Folder,
            host_mode: HostMode::PerUser,
            protection: ProtectionPolicy::LocalOnly,
            backend: BackendReference::Folder {
                root_identity: "fsid:codex-root".to_string(),
            },
        };
        manifest.validate().expect("manifest validates");
        let encoded = serde_json::to_value(manifest).expect("manifest serializes");
        assert_eq!(encoded["deployment_profile"], "folder");
        assert_eq!(encoded["host_mode"], "per_user");
        assert_eq!(encoded["backend"]["kind"], "folder");
        assert_eq!(encoded["backend"]["root_identity"], "fsid:codex-root");
    }

    #[test]
    fn drive_manifest_requires_absolute_mount_hint_and_matching_profile() {
        let mut manifest = ObjectStoreManifest {
            schema_version: OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
            store_id: StoreId::new("codex-drive").expect("store id"),
            deployment_profile: DeploymentProfile::Drive,
            host_mode: HostMode::System,
            protection: ProtectionPolicy::Reproducible,
            backend: BackendReference::Drive {
                filesystem_identity: "apfs:123".to_string(),
                device_identity: Some("nvme:456".to_string()),
                media: DriveMediaKind::Ssd,
                mount_path_hint: Some(PathBuf::from("/Volumes/CODEX")),
            },
        };
        manifest.validate().expect("drive manifest validates");
        if let BackendReference::Drive {
            mount_path_hint, ..
        } = &mut manifest.backend
        {
            *mount_path_hint = Some(PathBuf::from("relative"));
        }
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn appliance_manifest_does_not_accept_folder_backend() {
        let manifest = ObjectStoreManifest {
            schema_version: OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
            store_id: StoreId::new("codex-appliance").expect("store id"),
            deployment_profile: DeploymentProfile::Appliance,
            host_mode: HostMode::Integrated,
            protection: ProtectionPolicy::ApplianceProtected,
            backend: BackendReference::Folder {
                root_identity: "fsid:legacy".to_string(),
            },
        };
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn drive_manifest_requires_explicit_ssd_media() {
        let manifest = ObjectStoreManifest {
            schema_version: OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
            store_id: StoreId::new("codex-drive-media").expect("store id"),
            deployment_profile: DeploymentProfile::Drive,
            host_mode: HostMode::System,
            protection: ProtectionPolicy::Reproducible,
            backend: BackendReference::Drive {
                filesystem_identity: "apfs:123".to_string(),
                device_identity: Some("disk:456".to_string()),
                media: DriveMediaKind::Ssd,
                mount_path_hint: Some(PathBuf::from("/Volumes/CODEX")),
            },
        };
        manifest.validate().expect("SSD drive manifest validates");
    }

    #[test]
    fn drive_manifest_rejects_missing_device_identity_and_system_root() {
        let mut manifest = ObjectStoreManifest {
            schema_version: OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
            store_id: StoreId::new("codex-drive-safety").expect("store id"),
            deployment_profile: DeploymentProfile::Drive,
            host_mode: HostMode::System,
            protection: ProtectionPolicy::Reproducible,
            backend: BackendReference::Drive {
                filesystem_identity: "apfs:123".to_string(),
                device_identity: None,
                media: DriveMediaKind::Ssd,
                mount_path_hint: Some(PathBuf::from("/Volumes/CODEX")),
            },
        };
        assert!(manifest.validate().is_err());
        if let BackendReference::Drive {
            device_identity,
            mount_path_hint,
            ..
        } = &mut manifest.backend
        {
            *device_identity = Some("disk:456".to_string());
            *mount_path_hint = Some(PathBuf::from("/"));
        }
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn manifest_validation_rejects_unknown_schema_without_migration() {
        let mut manifest = ObjectStoreManifest {
            schema_version: OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
            store_id: StoreId::new("codex-schema").expect("store id"),
            deployment_profile: DeploymentProfile::Folder,
            host_mode: HostMode::PerUser,
            protection: ProtectionPolicy::LocalOnly,
            backend: BackendReference::Folder {
                root_identity: "fsid:schema".to_string(),
            },
        };
        manifest.schema_version = OBJECT_STORE_MANIFEST_SCHEMA_VERSION + 1;
        assert!(matches!(
            manifest.validate(),
            Err(super::ObjectStoreManifestValidationError::UnsupportedSchema {
                schema_version
            }) if schema_version == OBJECT_STORE_MANIFEST_SCHEMA_VERSION + 1
        ));
    }

    #[test]
    fn decode_json_enforces_strict_v1_compatibility_boundary() {
        let manifest = ObjectStoreManifest {
            schema_version: OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
            store_id: StoreId::new("codex-decode").expect("store id"),
            deployment_profile: DeploymentProfile::Folder,
            host_mode: HostMode::PerUser,
            protection: ProtectionPolicy::LocalOnly,
            backend: BackendReference::Folder {
                root_identity: "fsid:decode".to_string(),
            },
        };
        let encoded = serde_json::to_string(&manifest).expect("manifest serializes");
        assert_eq!(ObjectStoreManifest::decode_json(&encoded), Ok(manifest));

        let unknown = format!(
            "{encoded_trimmed},\"future_field\":true}}",
            encoded_trimmed = encoded.trim_end_matches('}')
        );
        assert!(matches!(
            ObjectStoreManifest::decode_json(&unknown),
            Err(ObjectStoreManifestDecodeError::MalformedJson(_))
        ));

        let future = encoded.replace("\"schema_version\":1", "\"schema_version\":2");
        assert_eq!(
            ObjectStoreManifest::decode_json(&future),
            Err(ObjectStoreManifestDecodeError::UnsupportedSchema {
                found: 2,
                supported: OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
            })
        );
        assert!(matches!(
            ObjectStoreManifest::decode_json("{\"schema_version\":\"one\"}"),
            Err(ObjectStoreManifestDecodeError::MissingOrInvalidSchemaVersion)
        ));
    }
}
