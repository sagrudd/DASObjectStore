//! Product-policy to daemon provisioning bridge.
//!
//! Products own policy values and deployment roots. This adapter validates
//! those explicit decisions and submits the same idempotent daemon job used by
//! CLI and Web clients; it never derives paths, credentials, or provider
//! configuration.

use crate::{ProductPolicyTemplateAdapterError, ProductPolicyTemplateEnvelope};
use dasobjectstore_core::{manifest::ObjectStoreManifest, store::StorePolicy};
use dasobjectstore_daemon::{
    api::{
        ProfileBindingOperation, ProfileBindingRequest, ProfileBindingResponse,
        ProfileBindingValidationError, PROFILE_BINDING_CONFIRMATION,
    },
    client::{DaemonClient, DaemonClientError, DaemonClientTransport},
};
use dasobjectstore_object_service::StoreServiceDefinition;
use serde::{Deserialize, Serialize};
use std::{fmt, path::PathBuf};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProductProfileProvisioningPlan {
    pub policy_template: ProductPolicyTemplateEnvelope,
    pub manifest: ObjectStoreManifest,
    pub store_policy: StorePolicy,
    pub bucket_name: Option<String>,
    pub reader_group: Option<String>,
    pub writer_group: Option<String>,
    #[serde(default)]
    pub public: bool,
    pub backend_root: PathBuf,
    #[serde(default)]
    pub ssd_staging_root: Option<PathBuf>,
    pub client_request_id: String,
    pub administrator_actor: String,
    #[serde(default)]
    pub dry_run: bool,
}

impl ProductProfileProvisioningPlan {
    pub fn daemon_request(&self) -> Result<ProfileBindingRequest, ProductProfileProvisioningError> {
        self.policy_template.validate()?;
        self.manifest
            .validate()
            .map_err(|error| ProductProfileProvisioningError::InvalidManifest(error.to_string()))?;
        self.store_policy.validate().map_err(|error| {
            ProductProfileProvisioningError::InvalidStorePolicy(error.to_string())
        })?;

        let template = &self.policy_template.template;
        if self.manifest.deployment_profile != template.profile {
            return Err(ProductProfileProvisioningError::ProfileMismatch);
        }
        if self.manifest.host_mode != template.host_mode {
            return Err(ProductProfileProvisioningError::HostModeMismatch);
        }
        if self.manifest.protection != template.protection {
            return Err(ProductProfileProvisioningError::ProtectionMismatch);
        }
        if self.store_policy.capacity != template.capacity {
            return Err(ProductProfileProvisioningError::CapacityMismatch);
        }
        if self.store_policy.copies != template.copies {
            return Err(ProductProfileProvisioningError::CopyCountMismatch);
        }

        let request = ProfileBindingRequest {
            operation: ProfileBindingOperation::Provision,
            manifest: self.manifest.clone(),
            capacity: template.capacity.clone(),
            store_definition: Some(StoreServiceDefinition {
                store_id: self.manifest.store_id.clone(),
                policy: self.store_policy.clone(),
                bucket_name: self.bucket_name.clone(),
                reader_group: self.reader_group.clone(),
                writer_group: self.writer_group.clone(),
                public: self.public,
            }),
            backend_root: self.backend_root.clone(),
            ssd_staging_root: self.ssd_staging_root.clone(),
            dry_run: self.dry_run,
            client_request_id: Some(self.client_request_id.clone()),
            administrator_actor: Some(self.administrator_actor.clone()),
            confirmation_marker: PROFILE_BINDING_CONFIRMATION.to_string(),
        };
        request.validate()?;
        Ok(request)
    }
}

pub fn provision_product_profile<T: DaemonClientTransport>(
    client: &DaemonClient<T>,
    plan: &ProductProfileProvisioningPlan,
) -> Result<ProfileBindingResponse, ProductProfileProvisioningError> {
    Ok(client.register_profile_binding(plan.daemon_request()?)?)
}

#[derive(Debug)]
pub enum ProductProfileProvisioningError {
    InvalidTemplate(ProductPolicyTemplateAdapterError),
    InvalidManifest(String),
    InvalidStorePolicy(String),
    ProfileMismatch,
    HostModeMismatch,
    ProtectionMismatch,
    CapacityMismatch,
    CopyCountMismatch,
    InvalidDaemonRequest(ProfileBindingValidationError),
    Daemon(DaemonClientError),
}

impl From<ProductPolicyTemplateAdapterError> for ProductProfileProvisioningError {
    fn from(value: ProductPolicyTemplateAdapterError) -> Self {
        Self::InvalidTemplate(value)
    }
}

impl From<ProfileBindingValidationError> for ProductProfileProvisioningError {
    fn from(value: ProfileBindingValidationError) -> Self {
        Self::InvalidDaemonRequest(value)
    }
}

impl From<DaemonClientError> for ProductProfileProvisioningError {
    fn from(value: DaemonClientError) -> Self {
        Self::Daemon(value)
    }
}

impl fmt::Display for ProductProfileProvisioningError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTemplate(error) => write!(f, "invalid product policy template: {error}"),
            Self::InvalidManifest(error) => write!(f, "invalid object store manifest: {error}"),
            Self::InvalidStorePolicy(error) => write!(f, "invalid store policy: {error}"),
            Self::ProfileMismatch => {
                f.write_str("manifest profile does not match product policy template")
            }
            Self::HostModeMismatch => {
                f.write_str("manifest host mode does not match product policy template")
            }
            Self::ProtectionMismatch => {
                f.write_str("manifest protection does not match product policy template")
            }
            Self::CapacityMismatch => {
                f.write_str("store policy capacity does not match product policy template")
            }
            Self::CopyCountMismatch => {
                f.write_str("store policy copy count does not match product policy template")
            }
            Self::InvalidDaemonRequest(error) => {
                write!(f, "invalid daemon provisioning request: {error}")
            }
            Self::Daemon(error) => write!(f, "daemon provisioning failed: {error}"),
        }
    }
}

impl std::error::Error for ProductProfileProvisioningError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProductPolicyAdapterKind, ProductPolicyTemplateAdapter};
    use dasobjectstore_core::{
        deployment::{DeploymentProfile, HostMode},
        ids::StoreId,
        ingress::IngressOrigin,
        manifest::{BackendReference, OBJECT_STORE_MANIFEST_SCHEMA_VERSION},
        protection::ProtectionPolicy,
        store::{CapacityPolicy, StoreClass},
        StoragePolicyTemplate,
    };
    use dasobjectstore_daemon::api::{DaemonApiRequest, DaemonApiResponse, DaemonJobId};
    use std::cell::Cell;

    fn plan() -> ProductProfileProvisioningPlan {
        let capacity = CapacityPolicy::bounded(10_000, 100);
        let template =
            ProductPolicyTemplateAdapter::for_product(ProductPolicyAdapterKind::Synoptikon)
                .adapt(StoragePolicyTemplate {
                    template_id: "bounded-project".to_string(),
                    owner_product: "synoptikon".to_string(),
                    profile: DeploymentProfile::Folder,
                    host_mode: HostMode::Integrated,
                    protection: ProtectionPolicy::Reproducible,
                    capacity: capacity.clone(),
                    copies: 1,
                    ingress_origin: IngressOrigin::WebUpload,
                })
                .expect("valid product template");
        let mut store_policy = StorePolicy::defaults_for(StoreClass::ReproducibleCache);
        store_policy.capacity = capacity;
        ProductProfileProvisioningPlan {
            policy_template: template,
            manifest: ObjectStoreManifest {
                schema_version: OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
                store_id: StoreId::new("synoptikon-project").expect("store id"),
                deployment_profile: DeploymentProfile::Folder,
                host_mode: HostMode::Integrated,
                protection: ProtectionPolicy::Reproducible,
                backend: BackendReference::Folder {
                    root_identity: "fsid:synoptikon-project".to_string(),
                },
            },
            store_policy,
            bucket_name: Some("synoptikon-project".to_string()),
            reader_group: Some("synoptikon-readers".to_string()),
            writer_group: Some("synoptikon-writers".to_string()),
            public: false,
            backend_root: PathBuf::from("/srv/dasobjectstore/synoptikon-project"),
            ssd_staging_root: Some(PathBuf::from("/srv/dasobjectstore/staging")),
            client_request_id: "synoptikon-install-1".to_string(),
            administrator_actor: "package-installer".to_string(),
            dry_run: false,
        }
    }

    #[test]
    fn converts_explicit_product_policy_to_idempotent_daemon_job() {
        let request = plan().daemon_request().expect("request is valid");
        assert_eq!(request.operation, ProfileBindingOperation::Provision);
        assert_eq!(request.manifest.store_id.as_str(), "synoptikon-project");
        assert_eq!(
            request.store_definition.expect("definition").writer_group,
            Some("synoptikon-writers".to_string())
        );
        assert_eq!(
            request.backend_root,
            PathBuf::from("/srv/dasobjectstore/synoptikon-project")
        );
    }

    #[test]
    fn rejects_policy_drift_before_daemon_submission() {
        let mut mismatched = plan();
        mismatched.store_policy.copies = 2;
        assert!(matches!(
            mismatched.daemon_request(),
            Err(ProductProfileProvisioningError::CopyCountMismatch)
        ));

        let mut future_schema = plan();
        future_schema.policy_template.schema_version =
            "dasobjectstore.product_policy_template.v2".to_string();
        assert!(matches!(
            future_schema.daemon_request(),
            Err(ProductProfileProvisioningError::InvalidTemplate(
                ProductPolicyTemplateAdapterError::UnsupportedSchema { .. }
            ))
        ));
    }

    #[test]
    fn submits_through_the_shared_daemon_client_boundary() {
        struct RecordingTransport(Cell<usize>);

        impl DaemonClientTransport for RecordingTransport {
            fn send(
                &self,
                request: DaemonApiRequest,
            ) -> Result<DaemonApiResponse, DaemonClientError> {
                let DaemonApiRequest::RegisterProfileBinding(request) = request else {
                    panic!("unexpected daemon request")
                };
                assert_eq!(request.operation, ProfileBindingOperation::Provision);
                self.0.set(self.0.get() + 1);
                Ok(DaemonApiResponse::RegisterProfileBinding(
                    ProfileBindingResponse::accepted(
                        DaemonJobId::new("product-profile-1").expect("job id"),
                        "2026-07-15T12:00:00Z",
                        request,
                    ),
                ))
            }
        }

        let client = DaemonClient::new(RecordingTransport(Cell::new(0)));
        let response = provision_product_profile(&client, &plan()).expect("daemon accepts request");
        assert_eq!(response.accepted.job_id.as_str(), "product-profile-1");
        assert_eq!(response.operation, ProfileBindingOperation::Provision);
    }

    #[test]
    fn serialized_plan_is_strict_and_contains_no_credentials() {
        let value = serde_json::to_value(plan()).expect("plan serializes");
        assert!(value.get("credentials").is_none());
        assert!(value.get("provider_endpoint").is_none());
        let mut object = value.as_object().expect("object").clone();
        object.insert("unexpected".to_string(), serde_json::json!(true));
        assert!(serde_json::from_value::<ProductProfileProvisioningPlan>(
            serde_json::Value::Object(object)
        )
        .is_err());
    }
}
