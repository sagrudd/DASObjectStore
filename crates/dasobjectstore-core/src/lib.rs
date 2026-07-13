//! Core domain types for DASObjectStore.

pub mod application_auth;
pub mod backend;
pub mod capacity;
pub mod config;
pub mod deployment;
pub mod file_export;
pub mod health;
pub mod ids;
pub mod ingress;
pub mod lifecycle;
pub mod manifest;
pub mod migration;
pub mod object_catalogue;
pub mod object_type;
pub mod placement;
pub mod policy_template;
pub mod protection;
pub mod remote_upload;
pub mod repair;
pub mod risk;
pub mod store;
pub mod subobject_capacity;
pub mod utc;

pub use application_auth::{
    AccessTokenClaims, AccessTokenExchangeRequest, ApplicationAuthValidationError,
    ApplicationCredentialKind, ApplicationEnvironment, ApplicationIdentity,
    ApplicationKeyAlgorithm, ApplicationKeyDescriptor, ApplicationOperation, ApplicationScope,
    RenewalTokenClaims, UploadCompletionCapability, APPLICATION_AUTH_SCHEMA_VERSION,
    MAX_ACCESS_TOKEN_TTL_SECONDS, MAX_DEVELOPMENT_ACCESS_TOKEN_TTL_SECONDS,
    MAX_UPLOAD_COMPLETION_TTL_SECONDS,
};
pub use backend::{
    catalogue_logical_used_bytes, BackendCapabilities, BackendError, BackendHealth,
    BackendObjectKey, BackendObjectRecord, ObjectCatalogueAuthority, ObjectStoreBackend,
};
pub use config::{
    DEFAULT_PRODUCT_ROOT, DEFAULT_STANDALONE_BIND_ADDRESS, DEFAULT_STANDALONE_CONFIG_PATH,
    DEFAULT_STANDALONE_HTTPS_PORT,
};
pub use deployment::{DeploymentProfile, HostMode};
pub use manifest::{
    BackendReference, DriveMediaKind, ObjectStoreManifest, ObjectStoreManifestDecodeError,
    OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
};
pub use object_catalogue::{
    ObjectDigest, PortableLifecycleState, PortableObjectCatalogue,
    PortableObjectCatalogueDecodeError, PortableObjectCatalogueValidationError,
    PortableObjectVersion, PortablePlacement, PortablePlacementLocation, PortableProtectionState,
    PortableProvenance, PORTABLE_OBJECT_CATALOGUE_SCHEMA_VERSION,
};
pub use policy_template::{StoragePolicyTemplate, StoragePolicyTemplateValidationError};
pub use protection::ProtectionPolicy;
pub use store::LogicalObjectVersionCharge;
pub use subobject_capacity::{SubObjectCapacityError, SubObjectCapacityLedger};

/// Current core crate version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::VERSION;

    #[test]
    fn exposes_package_version() {
        assert_eq!(VERSION, env!("CARGO_PKG_VERSION"));
    }
}
