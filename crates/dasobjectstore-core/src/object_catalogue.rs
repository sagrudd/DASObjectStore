//! Profile-neutral, versioned object catalogue records.
//!
//! The companion catalogue deliberately does not extend the strict v1
//! `ObjectStoreManifest`. Existing manifest readers therefore keep their
//! fail-closed compatibility boundary while newer services can exchange
//! logical object versions, provenance, lifecycle, protection, and placement
//! records without encoding an appliance-only layout.

use crate::ids::{DiskId, ObjectId, PlacementId, StoreId};
use crate::protection::ProtectionPolicy;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt::{self, Display};
use std::path::{Component, Path};

pub const PORTABLE_OBJECT_CATALOGUE_SCHEMA_VERSION: u16 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PortableObjectCatalogue {
    pub schema_version: u16,
    pub store_id: StoreId,
    pub objects: Vec<PortableObjectVersion>,
}

impl PortableObjectCatalogue {
    pub fn validate(&self) -> Result<(), PortableObjectCatalogueValidationError> {
        if self.schema_version != PORTABLE_OBJECT_CATALOGUE_SCHEMA_VERSION {
            return Err(PortableObjectCatalogueValidationError::UnsupportedSchema {
                schema_version: self.schema_version,
            });
        }
        let mut keys = BTreeSet::new();
        for object in &self.objects {
            object.validate()?;
            if !keys.insert((object.object_id.clone(), object.version)) {
                return Err(
                    PortableObjectCatalogueValidationError::DuplicateObjectVersion {
                        object_id: object.object_id.to_string(),
                        version: object.version,
                    },
                );
            }
        }
        Ok(())
    }

    pub fn decode_json(input: &str) -> Result<Self, PortableObjectCatalogueDecodeError> {
        let value: serde_json::Value = serde_json::from_str(input).map_err(|error| {
            PortableObjectCatalogueDecodeError::MalformedJson(error.to_string())
        })?;
        let schema_version = value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| u16::try_from(value).ok())
            .ok_or(PortableObjectCatalogueDecodeError::MissingOrInvalidSchemaVersion)?;
        if schema_version != PORTABLE_OBJECT_CATALOGUE_SCHEMA_VERSION {
            return Err(PortableObjectCatalogueDecodeError::UnsupportedSchema {
                found: schema_version,
                supported: PORTABLE_OBJECT_CATALOGUE_SCHEMA_VERSION,
            });
        }
        let catalogue: Self = serde_json::from_value(value).map_err(|error| {
            PortableObjectCatalogueDecodeError::MalformedJson(error.to_string())
        })?;
        catalogue
            .validate()
            .map_err(PortableObjectCatalogueDecodeError::InvalidCatalogue)?;
        Ok(catalogue)
    }

    /// Encode a portable catalogue only after validating the complete
    /// profile-neutral contract. Exporters therefore cannot emit malformed
    /// paths, duplicate versions, or unsupported lifecycle/protection state.
    pub fn encode_json(&self) -> Result<String, PortableObjectCatalogueValidationError> {
        self.validate()?;
        serde_json::to_string(self).map_err(|error| {
            PortableObjectCatalogueValidationError::Serialization(error.to_string())
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PortableObjectVersion {
    pub object_id: ObjectId,
    pub version: u64,
    pub size_bytes: u64,
    pub checksum: ObjectDigest,
    pub provenance: PortableProvenance,
    pub lifecycle: PortableLifecycleState,
    pub protection_policy: ProtectionPolicy,
    pub protection_state: PortableProtectionState,
    pub placements: Vec<PortablePlacement>,
}

impl PortableObjectVersion {
    fn validate(&self) -> Result<(), PortableObjectCatalogueValidationError> {
        if self.version == 0 {
            return Err(PortableObjectCatalogueValidationError::ZeroVersion {
                object_id: self.object_id.to_string(),
            });
        }
        self.checksum.validate()?;
        self.provenance.validate()?;
        let mut placement_ids = BTreeSet::new();
        for placement in &self.placements {
            placement.validate()?;
            if !placement_ids.insert(placement.placement_id.clone()) {
                return Err(PortableObjectCatalogueValidationError::DuplicatePlacement {
                    object_id: self.object_id.to_string(),
                    version: self.version,
                    placement_id: placement.placement_id.to_string(),
                });
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectDigest {
    pub algorithm: String,
    pub value: String,
}

impl ObjectDigest {
    fn validate(&self) -> Result<(), PortableObjectCatalogueValidationError> {
        if self.algorithm.trim().is_empty() || self.value.trim().is_empty() {
            return Err(PortableObjectCatalogueValidationError::BlankDigest);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PortableProvenance {
    pub source_kind: String,
    #[serde(default)]
    pub locator: Option<String>,
    #[serde(default)]
    pub revision: Option<String>,
}

impl PortableProvenance {
    fn validate(&self) -> Result<(), PortableObjectCatalogueValidationError> {
        if self.source_kind.trim().is_empty()
            || self
                .locator
                .as_deref()
                .is_some_and(|value| value.trim().is_empty())
            || self
                .revision
                .as_deref()
                .is_some_and(|value| value.trim().is_empty())
        {
            return Err(PortableObjectCatalogueValidationError::BlankProvenance);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PortableLifecycleState {
    Received,
    HashVerified,
    PlacementPlanned,
    Copying,
    CopyVerified,
    Protected,
    EvictionEligible,
    RedownloadRequired,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PortableProtectionState {
    Unprotected,
    Verified,
    Protected,
    RedownloadRequired,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PortablePlacement {
    pub placement_id: PlacementId,
    pub location: PortablePlacementLocation,
    pub checksum: ObjectDigest,
    #[serde(default)]
    pub verified_at_utc: Option<String>,
}

impl PortablePlacement {
    fn validate(&self) -> Result<(), PortableObjectCatalogueValidationError> {
        self.checksum.validate()?;
        if self
            .verified_at_utc
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(PortableObjectCatalogueValidationError::BlankVerificationTime);
        }
        self.location.validate()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PortablePlacementLocation {
    Folder {
        relative_path: String,
    },
    Drive {
        relative_path: String,
    },
    Appliance {
        pool_id: String,
        disk_id: DiskId,
        relative_path: String,
    },
    Provider {
        provider: String,
        object_key: String,
    },
}

impl PortablePlacementLocation {
    fn validate(&self) -> Result<(), PortableObjectCatalogueValidationError> {
        match self {
            Self::Folder { relative_path } | Self::Drive { relative_path } => {
                validate_relative_path(relative_path)
            }
            Self::Appliance {
                pool_id,
                relative_path,
                ..
            } => {
                if pool_id.trim().is_empty() {
                    return Err(PortableObjectCatalogueValidationError::BlankLocation);
                }
                validate_relative_path(relative_path)
            }
            Self::Provider {
                provider,
                object_key,
            } if provider.trim().is_empty() || object_key.trim().is_empty() => {
                Err(PortableObjectCatalogueValidationError::BlankLocation)
            }
            Self::Provider { .. } => Ok(()),
        }
    }
}

fn validate_relative_path(value: &str) -> Result<(), PortableObjectCatalogueValidationError> {
    let path = Path::new(value);
    if value.trim().is_empty()
        || path.is_absolute()
        || path.components().any(|component| match component {
            Component::CurDir
            | Component::ParentDir
            | Component::RootDir
            | Component::Prefix(_) => true,
            Component::Normal(part) => part.to_string_lossy().starts_with('.'),
        })
    {
        return Err(PortableObjectCatalogueValidationError::UnsafeLocation);
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PortableObjectCatalogueValidationError {
    UnsupportedSchema {
        schema_version: u16,
    },
    DuplicateObjectVersion {
        object_id: String,
        version: u64,
    },
    DuplicatePlacement {
        object_id: String,
        version: u64,
        placement_id: String,
    },
    ZeroVersion {
        object_id: String,
    },
    BlankDigest,
    BlankProvenance,
    BlankVerificationTime,
    BlankLocation,
    UnsafeLocation,
    Serialization(String),
}

impl Display for PortableObjectCatalogueValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema { schema_version } => {
                write!(formatter, "unsupported catalogue schema {schema_version}")
            }
            Self::DuplicateObjectVersion { object_id, version } => {
                write!(formatter, "duplicate object version {object_id}:{version}")
            }
            Self::DuplicatePlacement {
                object_id,
                version,
                placement_id,
            } => write!(
                formatter,
                "duplicate placement {placement_id} for {object_id}:{version}"
            ),
            Self::ZeroVersion { object_id } => {
                write!(formatter, "object {object_id} version must be positive")
            }
            Self::BlankDigest => {
                formatter.write_str("object digests must include algorithm and value")
            }
            Self::BlankProvenance => {
                formatter.write_str("object provenance fields must not be blank")
            }
            Self::BlankVerificationTime => formatter.write_str("verified_at_utc must not be blank"),
            Self::BlankLocation => {
                formatter.write_str("placement location fields must not be blank")
            }
            Self::UnsafeLocation => {
                formatter.write_str("placement path must be a safe relative path")
            }
            Self::Serialization(message) => {
                write!(
                    formatter,
                    "portable catalogue serialization failed: {message}"
                )
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PortableObjectCatalogueDecodeError {
    MalformedJson(String),
    MissingOrInvalidSchemaVersion,
    UnsupportedSchema { found: u16, supported: u16 },
    InvalidCatalogue(PortableObjectCatalogueValidationError),
}

impl Display for PortableObjectCatalogueDecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedJson(message) => write!(formatter, "malformed portable object catalogue: {message}"),
            Self::MissingOrInvalidSchemaVersion => formatter.write_str("portable object catalogue schema_version must be an unsigned integer"),
            Self::UnsupportedSchema { found, supported } => write!(formatter, "unsupported portable object catalogue schema {found}; supported schema is {supported}"),
            Self::InvalidCatalogue(error) => write!(formatter, "invalid portable object catalogue: {error}"),
        }
    }
}

impl std::error::Error for PortableObjectCatalogueDecodeError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalogue() -> PortableObjectCatalogue {
        PortableObjectCatalogue {
            schema_version: PORTABLE_OBJECT_CATALOGUE_SCHEMA_VERSION,
            store_id: StoreId::new("codex").expect("store id"),
            objects: vec![PortableObjectVersion {
                object_id: ObjectId::new("object-a").expect("object id"),
                version: 1,
                size_bytes: 4,
                checksum: ObjectDigest {
                    algorithm: "sha256".to_string(),
                    value: "abcd".to_string(),
                },
                provenance: PortableProvenance {
                    source_kind: "web_upload".to_string(),
                    locator: Some("remote-key".to_string()),
                    revision: Some("etag-1".to_string()),
                },
                lifecycle: PortableLifecycleState::Protected,
                protection_policy: ProtectionPolicy::Reproducible,
                protection_state: PortableProtectionState::Protected,
                placements: vec![PortablePlacement {
                    placement_id: PlacementId::new("placement-a").expect("placement id"),
                    location: PortablePlacementLocation::Folder {
                        relative_path: "objects/object-a".to_string(),
                    },
                    checksum: ObjectDigest {
                        algorithm: "sha256".to_string(),
                        value: "abcd".to_string(),
                    },
                    verified_at_utc: Some("2026-07-13T04:00:00Z".to_string()),
                }],
            }],
        }
    }

    #[test]
    fn companion_catalogue_round_trips_profile_neutral_records() {
        let catalogue = catalogue();
        catalogue.validate().expect("catalogue validates");
        let encoded = catalogue.encode_json().expect("catalogue encodes");
        assert_eq!(
            PortableObjectCatalogue::decode_json(&encoded),
            Ok(catalogue)
        );
    }

    #[test]
    fn export_rejects_invalid_catalogue_before_serialization() {
        let mut catalogue = catalogue();
        catalogue.objects[0].placements[0].location = PortablePlacementLocation::Folder {
            relative_path: "../outside-store".to_string(),
        };

        assert!(matches!(
            catalogue.encode_json(),
            Err(PortableObjectCatalogueValidationError::UnsafeLocation)
        ));
    }

    #[test]
    fn strict_decode_rejects_unknown_and_future_schema() {
        let encoded = serde_json::to_value(catalogue()).expect("catalogue encodes");
        let mut unknown = encoded.clone();
        unknown["unexpected"] = serde_json::json!(true);
        assert!(matches!(
            PortableObjectCatalogue::decode_json(&unknown.to_string()),
            Err(PortableObjectCatalogueDecodeError::MalformedJson(_))
        ));

        let mut future = encoded;
        future["schema_version"] = serde_json::json!(2);
        assert_eq!(
            PortableObjectCatalogue::decode_json(&future.to_string()),
            Err(PortableObjectCatalogueDecodeError::UnsupportedSchema {
                found: 2,
                supported: PORTABLE_OBJECT_CATALOGUE_SCHEMA_VERSION,
            })
        );
    }

    #[test]
    fn validation_rejects_duplicates_and_unsafe_locations() {
        let mut duplicate = catalogue();
        duplicate.objects.push(duplicate.objects[0].clone());
        assert!(matches!(
            duplicate.validate(),
            Err(PortableObjectCatalogueValidationError::DuplicateObjectVersion { .. })
        ));

        let mut unsafe_location = catalogue();
        unsafe_location.objects[0].placements[0].location = PortablePlacementLocation::Folder {
            relative_path: "../escape".to_string(),
        };
        assert_eq!(
            unsafe_location.validate(),
            Err(PortableObjectCatalogueValidationError::UnsafeLocation)
        );
        unsafe_location.objects[0].placements[0].location = PortablePlacementLocation::Folder {
            relative_path: ".dasobjectstore/secret".to_string(),
        };
        assert_eq!(
            unsafe_location.validate(),
            Err(PortableObjectCatalogueValidationError::UnsafeLocation)
        );

        let mut drive = catalogue();
        drive.objects[0].placements[0].location = PortablePlacementLocation::Drive {
            relative_path: "objects/object-a".to_string(),
        };
        drive.validate().expect("drive location validates");
        let mut appliance = catalogue();
        appliance.objects[0].placements[0].location = PortablePlacementLocation::Appliance {
            pool_id: "pool-a".to_string(),
            disk_id: DiskId::new("disk-a").expect("disk id"),
            relative_path: "objects/object-a".to_string(),
        };
        appliance.validate().expect("appliance location validates");
        let mut provider = catalogue();
        provider.objects[0].placements[0].location = PortablePlacementLocation::Provider {
            provider: "garage".to_string(),
            object_key: "object-a".to_string(),
        };
        provider.validate().expect("provider location validates");

        for location in [
            drive.objects[0].placements[0].location.clone(),
            appliance.objects[0].placements[0].location.clone(),
            provider.objects[0].placements[0].location.clone(),
        ] {
            let encoded = serde_json::to_value(&location).expect("location encodes");
            let decoded: PortablePlacementLocation =
                serde_json::from_value(encoded).expect("location decodes");
            assert_eq!(decoded, location);
        }
    }
}
