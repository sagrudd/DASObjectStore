//! Object service orchestration boundary.

pub mod compose;
pub mod credentials;
pub mod layout;
pub mod provider;
pub mod registry;

pub use compose::{render_compose, ComposeServiceConfig};
pub use credentials::{
    generate_per_store_credentials, write_credential_reference_manifest, CredentialEntropy,
    CredentialReferenceManifest, SecretAccessKey, StoreCredentialReference, StoreCredentialRequest,
    StoreServiceCredential, SystemCredentialEntropy,
};
pub use layout::{plan_store_service_layout, StoreServiceDefinition, StoreServiceLayout};
pub use provider::{
    ComposeRenderRequest, ObjectServiceError, ObjectServiceProvider, ObjectServiceProviderId,
    ProviderDescriptor, RenderedCompose, ServiceState, ServiceStatus, StoreBucketBinding,
};
pub use registry::{
    default_store_registry_path, portable_store_registry_path, read_store_registry,
    upsert_store_definition, StoreRegistryAction, StoreRegistryUpdateReport,
    PORTABLE_STORE_REGISTRY_RELATIVE_PATH, STORE_REGISTRY_ENV,
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
