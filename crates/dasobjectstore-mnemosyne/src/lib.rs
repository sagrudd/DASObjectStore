//! Mnemosyne and Synoptikon adapter boundary.

pub mod boundary;
pub mod storage_definition;

pub use boundary::{
    synoptikon_object_store_boundary, ArtefactAuthority, HostMode, HostStorageBoundary,
    LocalRootPolicy, LocalRootTemplate, ObjectStorePolicy, RegistrationContract, SqlPolicy,
    SqlRequiredBackend, StateAuthority, HOST_STORAGE_BOUNDARY_SCHEMA_VERSION,
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
