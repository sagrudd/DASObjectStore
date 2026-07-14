use crate::api::{DaemonJobAcceptedResponse, DaemonJobId, DaemonJobKind};
use dasobjectstore_core::deployment::DeploymentProfile;
use dasobjectstore_core::manifest::ObjectStoreManifest;
use dasobjectstore_core::store::CapacityPolicy;
use dasobjectstore_object_service::StoreServiceDefinition;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const PROFILE_BINDING_CONFIRMATION: &str = "confirm profile binding";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileBindingOperation {
    Create,
    /// Ensure a matching daemon-owned binding exists without adopting user
    /// files. Repeating the same request is a safe no-op.
    Provision,
    Adopt,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileBindingRequest {
    pub operation: ProfileBindingOperation,
    pub manifest: ObjectStoreManifest,
    pub capacity: CapacityPolicy,
    /// Optional daemon-owned registry definition to publish after binding.
    #[serde(default)]
    pub store_definition: Option<StoreServiceDefinition>,
    pub backend_root: PathBuf,
    #[serde(default)]
    pub ssd_staging_root: Option<PathBuf>,
    #[serde(default)]
    pub dry_run: bool,
    pub client_request_id: Option<String>,
    pub administrator_actor: Option<String>,
    pub confirmation_marker: String,
}

impl ProfileBindingRequest {
    pub fn validate(&self) -> Result<(), ProfileBindingValidationError> {
        self.manifest
            .validate()
            .map_err(|error| ProfileBindingValidationError::InvalidManifest(error.to_string()))?;
        if let Some(error) = self.capacity.validation_error() {
            return Err(ProfileBindingValidationError::InvalidCapacity(
                error.to_string(),
            ));
        }
        if let Some(definition) = &self.store_definition {
            definition.policy.validate().map_err(|error| {
                ProfileBindingValidationError::InvalidCapacity(error.to_string())
            })?;
            if definition.store_id != self.manifest.store_id {
                return Err(ProfileBindingValidationError::StoreIdMismatch);
            }
            if definition.policy.capacity != self.capacity {
                return Err(ProfileBindingValidationError::CapacityMismatch);
            }
        }
        if self.manifest.deployment_profile != DeploymentProfile::Appliance
            && self.capacity.logical_limit_bytes.is_none()
        {
            return Err(ProfileBindingValidationError::FiniteCapacityRequired);
        }
        require_absolute("backend_root", &self.backend_root)?;
        if let Some(path) = &self.ssd_staging_root {
            require_absolute("ssd_staging_root", path)?;
        }
        if self
            .client_request_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(ProfileBindingValidationError::BlankClientRequestId);
        }
        if self
            .administrator_actor
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(ProfileBindingValidationError::BlankAdministratorActor);
        }
        if self.confirmation_marker.trim() != PROFILE_BINDING_CONFIRMATION {
            return Err(ProfileBindingValidationError::ConfirmationMismatch);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileBindingResponse {
    pub accepted: DaemonJobAcceptedResponse,
    pub operation: ProfileBindingOperation,
    pub store_id: String,
    pub deployment_profile: DeploymentProfile,
    pub capacity: CapacityPolicy,
    pub store_definition_published: bool,
    pub unmanaged_path_count: usize,
    pub unsafe_path_count: usize,
    #[serde(default)]
    pub adopted_object_count: usize,
    #[serde(default)]
    pub adopted_bytes: u64,
    pub administrator_actor: Option<String>,
    /// True when `Provision` found an identical existing binding and reused it.
    #[serde(default)]
    pub reused: bool,
}

impl ProfileBindingResponse {
    pub fn accepted(
        job_id: DaemonJobId,
        accepted_at_utc: impl Into<String>,
        request: ProfileBindingRequest,
    ) -> Self {
        Self {
            accepted: DaemonJobAcceptedResponse {
                job_id,
                kind: DaemonJobKind::ProfileBinding,
                accepted_at_utc: accepted_at_utc.into(),
                dry_run: request.dry_run,
            },
            operation: request.operation,
            store_id: request.manifest.store_id.to_string(),
            deployment_profile: request.manifest.deployment_profile,
            capacity: request.capacity,
            store_definition_published: false,
            unmanaged_path_count: 0,
            unsafe_path_count: 0,
            adopted_object_count: 0,
            adopted_bytes: 0,
            administrator_actor: request.administrator_actor,
            reused: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProfileBindingValidationError {
    InvalidManifest(String),
    InvalidCapacity(String),
    FiniteCapacityRequired,
    StoreIdMismatch,
    CapacityMismatch,
    RelativePath { field: &'static str, path: PathBuf },
    BlankClientRequestId,
    BlankAdministratorActor,
    ConfirmationMismatch,
}

impl std::fmt::Display for ProfileBindingValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidManifest(message) => formatter.write_str(message),
            Self::InvalidCapacity(message) => formatter.write_str(message),
            Self::FiniteCapacityRequired => {
                formatter.write_str("bounded profile requires a finite logical capacity limit")
            }
            Self::StoreIdMismatch => {
                formatter.write_str("store definition id must match manifest id")
            }
            Self::CapacityMismatch => {
                formatter.write_str("store definition capacity must match profile binding capacity")
            }
            Self::RelativePath { field, path } => {
                write!(formatter, "{field} must be absolute: {}", path.display())
            }
            Self::BlankClientRequestId => {
                formatter.write_str("client_request_id must not be blank")
            }
            Self::BlankAdministratorActor => {
                formatter.write_str("administrator_actor must not be blank")
            }
            Self::ConfirmationMismatch => write!(
                formatter,
                "confirmation_marker must exactly match \"{PROFILE_BINDING_CONFIRMATION}\""
            ),
        }
    }
}

impl std::error::Error for ProfileBindingValidationError {}

fn require_absolute(
    field: &'static str,
    path: &PathBuf,
) -> Result<(), ProfileBindingValidationError> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(ProfileBindingValidationError::RelativePath {
            field,
            path: path.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dasobjectstore_core::deployment::HostMode;
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_core::manifest::{BackendReference, OBJECT_STORE_MANIFEST_SCHEMA_VERSION};
    use dasobjectstore_core::protection::ProtectionPolicy;

    fn request() -> ProfileBindingRequest {
        ProfileBindingRequest {
            operation: ProfileBindingOperation::Create,
            manifest: ObjectStoreManifest {
                schema_version: OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
                store_id: StoreId::new("codex").expect("store id"),
                deployment_profile: DeploymentProfile::Folder,
                host_mode: HostMode::PerUser,
                protection: ProtectionPolicy::LocalOnly,
                backend: BackendReference::Folder {
                    root_identity: "fsid:codex".to_string(),
                },
            },
            capacity: CapacityPolicy::bounded(1024, 64),
            store_definition: None,
            backend_root: PathBuf::from("/tmp/codex"),
            ssd_staging_root: None,
            dry_run: true,
            client_request_id: Some("request-1".to_string()),
            administrator_actor: Some("stephen".to_string()),
            confirmation_marker: PROFILE_BINDING_CONFIRMATION.to_string(),
        }
    }

    #[test]
    fn validates_profile_binding_without_exposing_path_in_manifest() {
        let request = request();
        request.validate().expect("valid request");
        let encoded = serde_json::to_value(request).expect("serialize");
        assert_eq!(encoded["manifest"]["backend"]["kind"], "folder");
        assert!(encoded["manifest"]["backend"].get("backend_root").is_none());
    }

    #[test]
    fn rejects_relative_backend_root() {
        let mut request = request();
        request.backend_root = PathBuf::from("relative");
        assert!(matches!(
            request.validate(),
            Err(ProfileBindingValidationError::RelativePath {
                field: "backend_root",
                ..
            })
        ));
    }

    #[test]
    fn bounded_profiles_require_finite_capacity() {
        let mut request = request();
        request.capacity = CapacityPolicy::default();
        assert!(matches!(
            request.validate(),
            Err(ProfileBindingValidationError::FiniteCapacityRequired)
        ));
    }
}
