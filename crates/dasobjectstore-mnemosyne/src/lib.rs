//! Mnemosyne and Synoptikon adapter boundary.

pub mod binding;
pub mod boundary;
pub mod host_mode;
pub mod product_ui;
pub mod storage_definition;
mod validation;

pub use binding::{
    export_mneion_binding_snippet, MneionBindingSnippetError, MneionBindingSnippetExport,
    MneionBindingSnippetRequest, MneionObjectStoreLinkRequest,
    INTERNAL_MNEION_GOVERNANCE_DOMAIN_ID, MNEION_OBJECT_STORE_ADMIN_ENDPOINT,
};
pub use boundary::{
    synoptikon_object_store_boundary, ArtefactAuthority, HostMode, HostStorageBoundary,
    LocalRootPolicy, LocalRootTemplate, ObjectStorePolicy, RegistrationContract, SqlPolicy,
    SqlRequiredBackend, StateAuthority, HOST_STORAGE_BOUNDARY_SCHEMA_VERSION,
};
pub use host_mode::{
    host_mode_profile, standalone_host_mode_profile, synoptikon_integrated_host_mode_profile,
    AuditAuthority, AuthenticationAuthority, HostModeProfile, HostModeProfileError,
    ProductHostMode, StorageAuthority, DASOBJECTSTORE_PRODUCT_ROOT,
    DASOBJECTSTORE_STANDALONE_HTTPS_PORT,
};
pub use product_ui::{
    bootstrap_path_for_web_mount, export_product_ui_bootstrap,
    export_synoptikon_product_ui_bootstrap, operations_navigation, ProductUiBootstrapError,
    ProductUiBootstrapMetadata, ProductUiCorrelationMode, ProductUiCorrelationPolicy,
    ProductUiHostCapability, ProductUiNavigationItem, ProductUiVisibility,
    ProductUiVisibilityState, CORRELATION_ID_HEADER, DASOBJECTSTORE_API_MOUNT,
    DASOBJECTSTORE_PRODUCT_ID, DASOBJECTSTORE_PRODUCT_NAME, DASOBJECTSTORE_WEB_MOUNT,
    PRODUCT_UI_BOOTSTRAP_SCHEMA_VERSION, PRODUCT_UI_BOOTSTRAP_WELL_KNOWN_SUFFIX,
};
pub use storage_definition::{
    export_mneion_storage_definition, MneionObjectStoreCreateRequest, MneionStorageDefinitionError,
    MneionStorageDefinitionExport, MneionStorageDefinitionRequest, MNEION_S3_BACKEND_KIND,
};

/// Returns the Mnemosyne adapter crate version.
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
