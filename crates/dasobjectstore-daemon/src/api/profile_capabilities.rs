//! Static profile capability discovery for Mnemosyne and standalone clients.
//!
//! This contract describes implementation maturity and requirements. It is not
//! a runtime health or mounted-store readiness response.

use dasobjectstore_core::backend::BackendCapabilities;
use dasobjectstore_core::deployment::{DeploymentProfile, HostMode};
use dasobjectstore_core::protection::ProtectionPolicy;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

pub const PROFILE_CAPABILITIES_SCHEMA_VERSION: &str = "dasobjectstore.profile_capabilities.v1";

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObjectStoreCapabilityDiscoveryRequest {}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileAvailability {
    Supported,
    Preview,
    Unavailable,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileRequirements {
    pub bounded_capacity_required: bool,
    pub dedicated_ssd_required: bool,
    pub stable_device_identity_required: bool,
    pub single_machine: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileServices {
    pub managed_ingress: bool,
    pub hierarchical_manifest: bool,
    pub s3: bool,
    pub web_ui: bool,
    pub object_browser: bool,
    pub reconciliation: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeploymentProfileCapabilities {
    pub profile: DeploymentProfile,
    pub availability: ProfileAvailability,
    #[serde(default)]
    pub unavailable_reason: Option<String>,
    pub host_modes: Vec<HostMode>,
    pub protection_policies: Vec<ProtectionPolicy>,
    pub backend_operations: BackendCapabilities,
    pub requirements: ProfileRequirements,
    pub services: ProfileServices,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObjectStoreCapabilityDiscoveryResponse {
    pub schema_version: String,
    pub profiles: Vec<DeploymentProfileCapabilities>,
}

impl ObjectStoreCapabilityDiscoveryResponse {
    pub fn validate(&self) -> Result<(), ObjectStoreCapabilityValidationError> {
        if self.schema_version != PROFILE_CAPABILITIES_SCHEMA_VERSION {
            return Err(ObjectStoreCapabilityValidationError::UnsupportedSchema {
                schema_version: self.schema_version.clone(),
            });
        }
        let mut profiles = Vec::new();
        for descriptor in &self.profiles {
            if profiles.contains(&descriptor.profile) {
                return Err(ObjectStoreCapabilityValidationError::DuplicateProfile(
                    descriptor.profile,
                ));
            }
            profiles.push(descriptor.profile);
            if descriptor.host_modes.is_empty() {
                return Err(ObjectStoreCapabilityValidationError::MissingHostMode(
                    descriptor.profile,
                ));
            }
            if descriptor.availability == ProfileAvailability::Unavailable
                && descriptor
                    .unavailable_reason
                    .as_deref()
                    .is_none_or(str::is_empty)
            {
                return Err(
                    ObjectStoreCapabilityValidationError::MissingUnavailableReason(
                        descriptor.profile,
                    ),
                );
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObjectStoreCapabilityValidationError {
    UnsupportedSchema { schema_version: String },
    DuplicateProfile(DeploymentProfile),
    MissingHostMode(DeploymentProfile),
    MissingUnavailableReason(DeploymentProfile),
}

impl Display for ObjectStoreCapabilityValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema { schema_version } => {
                write!(
                    formatter,
                    "unsupported profile capability schema {schema_version}"
                )
            }
            Self::DuplicateProfile(profile) => {
                write!(
                    formatter,
                    "duplicate profile descriptor: {}",
                    profile.name()
                )
            }
            Self::MissingHostMode(profile) => {
                write!(
                    formatter,
                    "profile {} has no supported host mode",
                    profile.name()
                )
            }
            Self::MissingUnavailableReason(profile) => write!(
                formatter,
                "unavailable profile {} must include a reason",
                profile.name()
            ),
        }
    }
}

impl std::error::Error for ObjectStoreCapabilityValidationError {}

#[cfg(test)]
mod tests {
    use super::*;
    use dasobjectstore_core::deployment::{DeploymentProfile, HostMode};
    use serde_json::json;

    fn descriptor(profile: DeploymentProfile) -> DeploymentProfileCapabilities {
        DeploymentProfileCapabilities {
            profile,
            availability: ProfileAvailability::Preview,
            unavailable_reason: None,
            host_modes: vec![HostMode::PerUser],
            protection_policies: vec![ProtectionPolicy::LocalOnly],
            backend_operations: BackendCapabilities::complete(),
            requirements: ProfileRequirements {
                bounded_capacity_required: profile != DeploymentProfile::Appliance,
                dedicated_ssd_required: profile == DeploymentProfile::Drive,
                stable_device_identity_required: profile == DeploymentProfile::Drive,
                single_machine: true,
            },
            services: ProfileServices {
                managed_ingress: true,
                hierarchical_manifest: true,
                s3: false,
                web_ui: true,
                object_browser: true,
                reconciliation: false,
            },
            warnings: Vec::new(),
        }
    }

    #[test]
    fn validates_catalogue_and_keeps_profiles_distinct() {
        let response = ObjectStoreCapabilityDiscoveryResponse {
            schema_version: PROFILE_CAPABILITIES_SCHEMA_VERSION.to_string(),
            profiles: vec![
                descriptor(DeploymentProfile::Folder),
                descriptor(DeploymentProfile::Drive),
                descriptor(DeploymentProfile::Appliance),
            ],
        };
        response.validate().expect("catalogue validates");
        assert!(response.profiles[1].requirements.dedicated_ssd_required);
        assert!(!response.profiles[2].requirements.bounded_capacity_required);
    }

    #[test]
    fn rejects_duplicate_profiles_and_unexplained_unavailability() {
        let mut duplicate = ObjectStoreCapabilityDiscoveryResponse {
            schema_version: PROFILE_CAPABILITIES_SCHEMA_VERSION.to_string(),
            profiles: vec![
                descriptor(DeploymentProfile::Folder),
                descriptor(DeploymentProfile::Folder),
            ],
        };
        assert_eq!(
            duplicate.validate(),
            Err(ObjectStoreCapabilityValidationError::DuplicateProfile(
                DeploymentProfile::Folder
            ))
        );
        duplicate.profiles.pop();
        duplicate.profiles[0].availability = ProfileAvailability::Unavailable;
        assert_eq!(
            duplicate.validate(),
            Err(
                ObjectStoreCapabilityValidationError::MissingUnavailableReason(
                    DeploymentProfile::Folder
                )
            )
        );
    }

    #[test]
    fn serializes_stable_schema_without_runtime_identity_or_secret_fields() {
        let response = ObjectStoreCapabilityDiscoveryResponse {
            schema_version: PROFILE_CAPABILITIES_SCHEMA_VERSION.to_string(),
            profiles: vec![descriptor(DeploymentProfile::Drive)],
        };
        let encoded = serde_json::to_value(response).expect("response serializes");
        assert_eq!(
            encoded["schema_version"],
            PROFILE_CAPABILITIES_SCHEMA_VERSION
        );
        assert_eq!(encoded["profiles"][0]["profile"], json!("drive"));
        assert!(encoded.get("mount_path").is_none());
        assert!(encoded.get("device_identity").is_none());
    }
}
