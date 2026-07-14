//! Versioned, path-free physical enclosure and bay identity contract.
//!
//! The registry records authoritative hardware associations without probing
//! devices or carrying mount paths. Telemetry and placement adapters may use
//! it to enrich samples after a deployment-specific daemon has validated the
//! live topology.

use crate::ids::{DiskId, EnclosureId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt::{self, Display};

pub const PHYSICAL_ENCLOSURE_REGISTRY_SCHEMA_VERSION: u16 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicalEnclosureRegistry {
    pub schema_version: u16,
    pub enclosures: Vec<PhysicalEnclosure>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicalEnclosure {
    pub enclosure_id: EnclosureId,
    #[serde(default)]
    pub label: Option<String>,
    pub bays: Vec<PhysicalBay>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicalBay {
    pub disk_id: DiskId,
    pub bay_label: String,
}

impl PhysicalEnclosureRegistry {
    pub fn validate(&self) -> Result<(), PhysicalEnclosureRegistryValidationError> {
        if self.schema_version != PHYSICAL_ENCLOSURE_REGISTRY_SCHEMA_VERSION {
            return Err(
                PhysicalEnclosureRegistryValidationError::UnsupportedSchema {
                    schema_version: self.schema_version,
                },
            );
        }
        let mut enclosure_ids = BTreeSet::new();
        let mut disk_ids = BTreeSet::new();
        for enclosure in &self.enclosures {
            if !enclosure_ids.insert(enclosure.enclosure_id.clone()) {
                return Err(
                    PhysicalEnclosureRegistryValidationError::DuplicateEnclosure {
                        enclosure_id: enclosure.enclosure_id.to_string(),
                    },
                );
            }
            validate_optional_text(enclosure.label.as_deref(), "enclosure label")?;
            let mut bay_labels = BTreeSet::new();
            for bay in &enclosure.bays {
                validate_text(&bay.bay_label, "bay label")?;
                if !bay_labels.insert(bay.bay_label.clone()) {
                    return Err(PhysicalEnclosureRegistryValidationError::DuplicateBay {
                        enclosure_id: enclosure.enclosure_id.to_string(),
                        bay_label: bay.bay_label.clone(),
                    });
                }
                if !disk_ids.insert(bay.disk_id.clone()) {
                    return Err(PhysicalEnclosureRegistryValidationError::DuplicateDisk {
                        disk_id: bay.disk_id.to_string(),
                    });
                }
            }
        }
        Ok(())
    }

    pub fn find_disk(&self, disk_id: &DiskId) -> Option<(&PhysicalEnclosure, &PhysicalBay)> {
        self.enclosures.iter().find_map(|enclosure| {
            enclosure
                .bays
                .iter()
                .find(|bay| &bay.disk_id == disk_id)
                .map(|bay| (enclosure, bay))
        })
    }
}

fn validate_optional_text(
    value: Option<&str>,
    field: &'static str,
) -> Result<(), PhysicalEnclosureRegistryValidationError> {
    if let Some(value) = value {
        validate_text(value, field)?;
    }
    Ok(())
}

fn validate_text(
    value: &str,
    field: &'static str,
) -> Result<(), PhysicalEnclosureRegistryValidationError> {
    if value.trim().is_empty()
        || value.len() > 128
        || value.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(PhysicalEnclosureRegistryValidationError::InvalidText { field });
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PhysicalEnclosureRegistryValidationError {
    UnsupportedSchema {
        schema_version: u16,
    },
    DuplicateEnclosure {
        enclosure_id: String,
    },
    DuplicateDisk {
        disk_id: String,
    },
    DuplicateBay {
        enclosure_id: String,
        bay_label: String,
    },
    InvalidText {
        field: &'static str,
    },
}

impl Display for PhysicalEnclosureRegistryValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema { schema_version } => {
                write!(
                    formatter,
                    "unsupported physical enclosure registry schema {schema_version}"
                )
            }
            Self::DuplicateEnclosure { enclosure_id } => {
                write!(formatter, "duplicate physical enclosure {enclosure_id}")
            }
            Self::DuplicateDisk { disk_id } => {
                write!(formatter, "disk {disk_id} is assigned to multiple bays")
            }
            Self::DuplicateBay {
                enclosure_id,
                bay_label,
            } => write!(
                formatter,
                "duplicate bay {bay_label} in enclosure {enclosure_id}"
            ),
            Self::InvalidText { field } => {
                write!(formatter, "{field} must be bounded and non-blank")
            }
        }
    }
}

impl std::error::Error for PhysicalEnclosureRegistryValidationError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> PhysicalEnclosureRegistry {
        PhysicalEnclosureRegistry {
            schema_version: PHYSICAL_ENCLOSURE_REGISTRY_SCHEMA_VERSION,
            enclosures: vec![PhysicalEnclosure {
                enclosure_id: EnclosureId::new("enclosure-1").expect("enclosure"),
                label: Some("primary".to_string()),
                bays: vec![PhysicalBay {
                    disk_id: DiskId::new("disk-1").expect("disk"),
                    bay_label: "1".to_string(),
                }],
            }],
        }
    }

    #[test]
    fn registry_round_trips_and_resolves_disk_identity() {
        let registry = registry();
        registry.validate().expect("registry validates");
        let encoded = serde_json::to_string(&registry).expect("encode");
        let decoded: PhysicalEnclosureRegistry = serde_json::from_str(&encoded).expect("decode");
        assert_eq!(decoded, registry);
        let disk = DiskId::new("disk-1").expect("disk");
        assert_eq!(
            registry
                .find_disk(&disk)
                .map(|(_, bay)| bay.bay_label.as_str()),
            Some("1")
        );
    }

    #[test]
    fn registry_rejects_duplicate_disk_and_unknown_fields() {
        let mut duplicate = registry();
        duplicate.enclosures[0].bays.push(PhysicalBay {
            disk_id: DiskId::new("disk-1").expect("disk"),
            bay_label: "2".to_string(),
        });
        assert!(matches!(
            duplicate.validate(),
            Err(PhysicalEnclosureRegistryValidationError::DuplicateDisk { .. })
        ));

        let mut value = serde_json::to_value(registry()).expect("encode");
        value["unexpected"] = serde_json::json!(true);
        assert!(serde_json::from_value::<PhysicalEnclosureRegistry>(value).is_err());
    }
}
