//! Object service orchestration boundary.

pub mod compose;
pub mod credentials;
pub mod garage;
pub mod layout;
pub mod provider;
pub mod provisioning;
pub mod registry;
pub mod subobject;

pub use compose::{render_compose, ComposeServiceConfig};
pub use credentials::{
    generate_per_store_credentials, write_credential_reference_manifest, CredentialEntropy,
    CredentialReferenceManifest, SecretAccessKey, StoreCredentialReference, StoreCredentialRequest,
    StoreServiceCredential, SystemCredentialEntropy,
};
pub use garage::{
    GarageProvider, GarageProviderConfig, DEFAULT_GARAGE_API_PORT, DEFAULT_GARAGE_CONFIG_PATH,
    DEFAULT_GARAGE_IMAGE, DEFAULT_GARAGE_SERVICE_NAME,
};
pub use layout::{plan_store_service_layout, StoreServiceDefinition, StoreServiceLayout};
pub use provider::{
    ComposeRenderRequest, ObjectServiceError, ObjectServiceProvider, ObjectServiceProviderId,
    ProviderDescriptor, RenderedCompose, ServiceState, ServiceStatus, StoreBucketBinding,
};
pub use provisioning::{
    plan_garage_provisioning, GarageProvisioningCommand, GarageProvisioningCommandKind,
    GarageProvisioningPlan,
};
pub use registry::{
    default_store_registry_path, delete_store_definition, portable_store_registry_path,
    read_store_registry, upsert_store_definition, StoreRegistryAction, StoreRegistryDeleteReport,
    StoreRegistryUpdateReport, PORTABLE_STORE_REGISTRY_RELATIVE_PATH, STORE_REGISTRY_ENV,
};
pub use subobject::{
    create_subobject_definition, default_subobject_registry_path, delete_subobjects_for_store,
    mirror_subobject_definition, portable_subobject_registry_path, read_subobject_registry,
    search_subobjects, SubObjectDefinition, SubObjectParent, SubObjectRegistryAction,
    SubObjectRegistryStoreDeleteReport, SubObjectRegistryUpdateReport,
    PORTABLE_SUBOBJECT_REGISTRY_RELATIVE_PATH,
};

/// Returns the object service crate version.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::version;

    #[test]
    fn exposes_package_version() {
        assert_eq!(version(), "0.0.0");
    }
}
