//! Object service orchestration boundary.

pub mod compose;
pub mod credentials;
pub mod custody;
pub mod custody_attestation;
pub mod custody_catalog;
pub mod garage;
pub mod inspection;
pub mod layout;
pub mod provider;
pub mod provisioning;
pub mod registry;
pub mod remote_upload;
pub mod subobject;

pub use compose::{render_compose, ComposeServiceConfig};
pub use credentials::{
    credential_reference_for_store, default_garage_credential_registry_path,
    generate_per_store_credentials, read_managed_credential_registry,
    resolve_managed_store_credentials, write_credential_reference_manifest,
    write_managed_credential_registry, CredentialEntropy, CredentialReferenceManifest,
    ManagedCredentialAuditAction, ManagedCredentialAuditEvent, ManagedCredentialRegistry,
    ManagedCredentialResolution, ManagedStoreCredentialRecord, SecretAccessKey,
    StoreCredentialReference, StoreCredentialRequest, StoreServiceCredential,
    SystemCredentialEntropy, GARAGE_CREDENTIAL_REGISTRY_ENV,
};
pub use custody::{
    append_custody_retention_extension, create_custody_ledger,
    create_custody_ledger_from_definition, custody_bucket_is_reserved, custody_object_key,
    custody_provisioning_request_sha256, custody_store_definition_sha256, inspect_custody_ledger,
    plan_custody_garage_provisioning, reject_custody_mutation, retain_custody_object_with_readback,
    verify_custody_readback_receipt, CustodyAssuranceClass, CustodyForbiddenMutation,
    CustodyFreshBucketProofV1, CustodyGarageCredential, CustodyGarageProvisionerIdentity,
    CustodyGarageProvisioningPlan, CustodyGarageProvisioningRequest, CustodyIntegrityReceiptV1,
    CustodyLedgerInspectionV1, CustodyObjectInputV1, CustodyObjectLockPolicyV1,
    CustodyObjectReader, CustodyObjectState, CustodyObjectWriter, CustodyReadbackObservationV1,
    CustodyRetentionMode, CustodyRetentionPolicyV1, CustodyStoreDefinitionV1,
    CustodyStoreProfileV1, CUSTODY_ASSURANCE_CLASS_LOCAL_TRUSTED_ADMINISTRATOR_OVERLAY,
    CUSTODY_FRESH_BUCKET_PROOF_SCHEMA_V1, CUSTODY_OBJECT_LOCK_HOLD_AUTHORITY,
    CUSTODY_OBJECT_LOCK_POLICY_ID, CUSTODY_OBJECT_LOCK_POLICY_SCHEMA_V1, CUSTODY_OVERLAY_SCHEMA_V1,
    CUSTODY_PROFILE_V1,
};
pub use custody_attestation::{
    CustodyEd25519AuthorityV1, CustodyFormalGateConsumptionV2, CustodyFormalGateExpectationV2,
    CustodyOffNucAttestationV2, CustodyOffNucJournal, CustodyOffNucObservationResult,
    CustodyOffNucPreReadRequestV1, CustodyOffNucReadAttemptV1, CustodySignedAttestationV2,
    CustodySignedPreReadRequestV1, CustodySignedRecordV1, CUSTODY_ATTESTATION_ALGORITHM_ED25519,
    CUSTODY_OFF_NUC_ATTESTATION_SCHEMA_V2, CUSTODY_OFF_NUC_PRE_READ_REQUEST_SCHEMA_V1,
    CUSTODY_SIGNED_RECORD_SCHEMA_V1,
};
pub use custody_catalog::{
    append_claimed_custody_catalog_entry, catalog_contains_bucket, catalog_contains_store,
    claim_custody_catalog_admission, create_custody_catalog_entry, custody_ledger_path_for_catalog,
    default_custody_catalog_path, default_custody_ledger_path, read_custody_catalog,
    reject_bound_catalogued_custody_definition, reject_bound_catalogued_custody_mutation,
    reject_catalogued_custody_definition, reject_catalogued_custody_mutation,
    CustodyCatalogAdmissionClaim, CustodyCatalogBinding, CustodyCatalogEntryV1,
};
pub use garage::{
    render_garage_data_directories, GarageDataDirectory, GarageProvider, GarageProviderConfig,
    DEFAULT_GARAGE_API_PORT, DEFAULT_GARAGE_CONFIG_PATH, DEFAULT_GARAGE_IMAGE,
    DEFAULT_GARAGE_SERVICE_NAME,
};
pub use inspection::{
    docker_object_service_binding, docker_object_service_container_state,
    parse_docker_published_bind_address, DEFAULT_OBJECT_SERVICE_PORT,
};
pub use layout::{
    bucket_name_for_definition, plan_store_service_layout,
    plan_store_service_layout_with_custody_catalog, StoreServiceDefinition, StoreServiceLayout,
};
pub use provider::{
    ComposeRenderRequest, ObjectServiceError, ObjectServiceProvider, ObjectServiceProviderId,
    ProviderDescriptor, RenderedCompose, ServiceState, ServiceStatus, StoreBucketBinding,
};
pub use provisioning::{
    plan_garage_provisioning, GarageProvisioningCommand, GarageProvisioningCommandKind,
    GarageProvisioningPlan,
};
pub use registry::{
    default_store_registry_path, delete_store_definition,
    delete_store_definition_with_custody_catalog, portable_store_registry_path,
    read_store_registry, read_store_registry_with_custody_catalog, upsert_store_definition,
    upsert_store_definition_with_custody_catalog, StoreRegistryAction, StoreRegistryDeleteReport,
    StoreRegistryUpdateReport, PORTABLE_STORE_REGISTRY_RELATIVE_PATH, STORE_REGISTRY_ENV,
};
pub use remote_upload::{
    plan_remote_s3_upload, RemoteS3AuthAuthority, RemoteS3UploadPlan, RemoteS3UploadPlanRequest,
};
pub use subobject::{
    create_subobject_definition, create_subobject_definition_with_capacity,
    default_subobject_registry_path, delete_subobjects_for_store, mirror_subobject_definition,
    portable_subobject_registry_path, read_subobject_registry, search_subobjects,
    SubObjectDefinition, SubObjectParent, SubObjectRegistryAction,
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
        assert_eq!(version(), env!("CARGO_PKG_VERSION"));
    }
}
