//! Object service orchestration boundary.

pub mod compose;
pub mod credentials;
pub mod provider;

pub use compose::{render_compose, ComposeServiceConfig};
pub use credentials::{
    generate_per_store_credentials, write_credential_reference_manifest, CredentialEntropy,
    CredentialReferenceManifest, SecretAccessKey, StoreCredentialReference, StoreCredentialRequest,
    StoreServiceCredential, SystemCredentialEntropy,
};
pub use provider::{
    ComposeRenderRequest, ObjectServiceError, ObjectServiceProvider, ObjectServiceProviderId,
    ProviderDescriptor, RenderedCompose, ServiceState, ServiceStatus, StoreBucketBinding,
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
