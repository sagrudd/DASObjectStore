//! Product-owned storage-policy templates.
//!
//! A template is the narrow contract a host product uses to request an
//! ObjectStore.  It carries policy, rather than provisioning instructions or
//! filesystem paths.  Adapters own product defaults and may add their own
//! validation, while this module enforces invariants that protect the shared
//! storage boundary.

use crate::deployment::{DeploymentProfile, HostMode};
use crate::ingress::{IngressLandingMode, IngressOrigin};
use crate::protection::ProtectionPolicy;
use crate::store::{CapacityPolicy, CapacityPolicyValidationError};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

/// A version-independent request for one product-owned ObjectStore policy.
///
/// `template_id` is stable within `owner_product`; neither field is a path,
/// credential, or provisioning command.  New profile creation requires a
/// finite logical limit even though legacy `StorePolicy` values may remain
/// unbounded until an explicit compatibility migration.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StoragePolicyTemplate {
    pub template_id: String,
    pub owner_product: String,
    pub profile: DeploymentProfile,
    pub host_mode: HostMode,
    pub protection: ProtectionPolicy,
    pub capacity: CapacityPolicy,
    pub copies: u8,
    pub ingress_origin: IngressOrigin,
}

impl StoragePolicyTemplate {
    pub fn validate(&self) -> Result<(), StoragePolicyTemplateValidationError> {
        validate_slug("template_id", &self.template_id)?;
        validate_slug("owner_product", &self.owner_product)?;

        if let Some(error) = self.capacity.validation_error() {
            return Err(StoragePolicyTemplateValidationError::InvalidCapacity(error));
        }
        if self.capacity.logical_limit_bytes.is_none() {
            return Err(
                StoragePolicyTemplateValidationError::LogicalCapacityRequired {
                    profile: self.profile,
                },
            );
        }
        if !(1..=3).contains(&self.copies) {
            return Err(StoragePolicyTemplateValidationError::InvalidCopyCount {
                copies: self.copies,
            });
        }

        let maximum_local_copies = match self.profile {
            DeploymentProfile::Folder | DeploymentProfile::Drive => 1,
            DeploymentProfile::Appliance => 3,
        };
        if self.copies > maximum_local_copies {
            return Err(StoragePolicyTemplateValidationError::TooManyLocalCopies {
                profile: self.profile,
                copies: self.copies,
                maximum: maximum_local_copies,
            });
        }

        Ok(())
    }

    /// Whether this template's typed ingress source requires SSD-first
    /// staging.  The result is informational for adapters; it does not grant
    /// a caller permission to bypass daemon admission.
    pub fn requires_ssd_staging(&self) -> bool {
        self.ingress_origin.requires_ssd_staging()
    }

    pub fn landing_mode(&self) -> IngressLandingMode {
        self.ingress_origin.landing_mode()
    }
}

fn validate_slug(
    field: &'static str,
    value: &str,
) -> Result<(), StoragePolicyTemplateValidationError> {
    if value.is_empty() {
        return Err(StoragePolicyTemplateValidationError::EmptyField { field });
    }
    let bytes = value.as_bytes();
    if !bytes[0].is_ascii_lowercase() && !bytes[0].is_ascii_digit() {
        return Err(StoragePolicyTemplateValidationError::UnsafeField { field });
    }
    if !bytes[bytes.len() - 1].is_ascii_lowercase() && !bytes[bytes.len() - 1].is_ascii_digit() {
        return Err(StoragePolicyTemplateValidationError::UnsafeField { field });
    }
    if !bytes
        .iter()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(byte))
    {
        return Err(StoragePolicyTemplateValidationError::UnsafeField { field });
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StoragePolicyTemplateValidationError {
    EmptyField {
        field: &'static str,
    },
    UnsafeField {
        field: &'static str,
    },
    InvalidCapacity(CapacityPolicyValidationError),
    LogicalCapacityRequired {
        profile: DeploymentProfile,
    },
    InvalidCopyCount {
        copies: u8,
    },
    TooManyLocalCopies {
        profile: DeploymentProfile,
        copies: u8,
        maximum: u8,
    },
}

impl Display for StoragePolicyTemplateValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyField { field } => write!(formatter, "{field} must not be empty"),
            Self::UnsafeField { field } => write!(
                formatter,
                "{field} must be a lowercase ASCII slug containing only letters, digits, '.', '_' or '-'"
            ),
            Self::InvalidCapacity(error) => write!(formatter, "invalid capacity policy: {error}"),
            Self::LogicalCapacityRequired { profile } => write!(
                formatter,
                "profile {profile} requires a finite logical capacity limit"
            ),
            Self::InvalidCopyCount { copies } => {
                write!(formatter, "copy count must be between 1 and 3, got {copies}")
            }
            Self::TooManyLocalCopies {
                profile,
                copies,
                maximum,
            } => write!(
                formatter,
                "profile {profile} cannot request {copies} local copies; maximum is {maximum}"
            ),
        }
    }
}

impl std::error::Error for StoragePolicyTemplateValidationError {}

#[cfg(test)]
mod tests {
    use super::{StoragePolicyTemplate, StoragePolicyTemplateValidationError};
    use crate::deployment::{DeploymentProfile, HostMode};
    use crate::ingress::{IngressLandingMode, IngressOrigin};
    use crate::protection::ProtectionPolicy;
    use crate::store::CapacityPolicy;

    fn template(profile: DeploymentProfile, copies: u8) -> StoragePolicyTemplate {
        StoragePolicyTemplate {
            template_id: "analysis-default".to_string(),
            owner_product: "synoptikon".to_string(),
            profile,
            host_mode: HostMode::Integrated,
            protection: ProtectionPolicy::Reproducible,
            capacity: CapacityPolicy::bounded(10_000, 1_000),
            copies,
            ingress_origin: IngressOrigin::WebUpload,
        }
    }

    #[test]
    fn validates_bounded_profiles_and_exposes_typed_ingress() {
        let template = template(DeploymentProfile::Folder, 1);
        template.validate().expect("bounded folder is valid");
        assert!(template.requires_ssd_staging());
        assert_eq!(template.landing_mode(), IngressLandingMode::SsdFirst);
    }

    #[test]
    fn appliance_allows_three_local_copies_but_profile_templates_are_bounded() {
        let appliance = template(DeploymentProfile::Appliance, 3);
        appliance
            .validate()
            .expect("three appliance copies are valid");

        let unbounded = StoragePolicyTemplate {
            capacity: CapacityPolicy::default(),
            ..template(DeploymentProfile::Appliance, 1)
        };
        assert!(matches!(
            unbounded.validate(),
            Err(
                StoragePolicyTemplateValidationError::LogicalCapacityRequired {
                    profile: DeploymentProfile::Appliance
                }
            )
        ));
    }

    #[test]
    fn folder_and_drive_cannot_claim_multiple_local_copies() {
        for profile in [DeploymentProfile::Folder, DeploymentProfile::Drive] {
            assert!(matches!(
                template(profile, 2).validate(),
                Err(StoragePolicyTemplateValidationError::TooManyLocalCopies {
                    profile: found,
                    copies: 2,
                    maximum: 1
                }) if found == profile
            ));
        }
    }

    #[test]
    fn rejects_unsafe_ids_capacity_and_copy_counts() {
        let mut value = template(DeploymentProfile::Drive, 1);
        value.template_id = "../store".to_string();
        assert!(matches!(
            value.validate(),
            Err(StoragePolicyTemplateValidationError::UnsafeField {
                field: "template_id"
            })
        ));

        let mut value = template(DeploymentProfile::Drive, 1);
        value.capacity = CapacityPolicy::bounded(1_000, 1_000);
        assert!(matches!(
            value.validate(),
            Err(StoragePolicyTemplateValidationError::InvalidCapacity(_))
        ));

        let value = template(DeploymentProfile::Drive, 0);
        assert!(matches!(
            value.validate(),
            Err(StoragePolicyTemplateValidationError::InvalidCopyCount { copies: 0 })
        ));
    }

    #[test]
    fn serializes_a_stable_product_owned_shape() {
        let value = template(DeploymentProfile::Drive, 1);
        let json = serde_json::to_value(value).expect("template serializes");
        assert_eq!(json["profile"], "drive");
        assert_eq!(json["host_mode"], "integrated");
        assert_eq!(json["protection"], "reproducible");
        assert_eq!(json["ingress_origin"], "web_upload");
        assert_eq!(json["capacity"]["logical_limit_bytes"], 10_000);
    }
}
