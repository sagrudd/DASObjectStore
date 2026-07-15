//! Mnemosyne and Synoptikon adapter boundary.

pub mod binding;
pub mod boundary;
pub mod flounder_telemetry;
pub mod host_boundary;
pub mod host_mode;
pub mod integrated_session;
pub mod monas_host_boundary;
pub mod nas_nfs_endpoint;
pub mod nas_nfs_runtime;
pub mod policy_templates;
pub mod product_ui;
pub mod prosopikon;
pub mod storage_definition;
mod validation;

pub use binding::{
    export_mneion_binding_snippet, export_mneion_managed_storage_binding,
    MneionBindingSnippetError, MneionBindingSnippetExport, MneionBindingSnippetRequest,
    MneionManagedBindingReadiness, MneionManagedStorageBindingContract,
    MneionManagedStorageBindingExport, MneionManagedStorageBindingRequest,
    MneionObjectStoreLinkRequest, INTERNAL_MNEION_GOVERNANCE_DOMAIN_ID,
    MNEION_OBJECT_STORE_ADMIN_ENDPOINT,
};
pub use boundary::{
    synoptikon_object_store_boundary, ArtefactAuthority, HostMode, HostStorageBoundary,
    LocalRootPolicy, LocalRootTemplate, ObjectStorePolicy, RegistrationContract, SqlPolicy,
    SqlRequiredBackend, StateAuthority, HOST_STORAGE_BOUNDARY_SCHEMA_VERSION,
};
pub use flounder_telemetry::{
    FlounderApplianceTelemetryContract, FlounderTelemetryAudience, FlounderTelemetryAxis,
    FlounderTelemetryBand, FlounderTelemetryChart, FlounderTelemetryChartContract,
    FlounderTelemetryChartLayout, FlounderTelemetryDevice, FlounderTelemetryGapLabel,
    FlounderTelemetryMissingInterval, FlounderTelemetryMissingReason, FlounderTelemetryPoint,
    FlounderTelemetryPointQuality, FlounderTelemetryProducer, FlounderTelemetryRenderPlan,
    FlounderTelemetryRenderSegment, FlounderTelemetrySeries, FlounderTelemetrySeriesRole,
    FlounderTelemetrySmallMultiple, FlounderTelemetryUnit, FlounderTelemetryWindow,
    FLOUNDER_APPLIANCE_TELEMETRY_SCHEMA_VERSION, FLOUNDER_TELEMETRY_CHART_CONTRACT_SCHEMA_VERSION,
};
pub use host_boundary::{
    validate_synoptikon_integrated_host_boundary, SynoptikonIntegratedHostBoundary,
    SynoptikonIntegratedHostBoundaryContext, SynoptikonIntegratedHostBoundaryError,
    REQUEST_CONTEXT_SCHEMA_VERSION,
};
pub use host_mode::{
    host_mode_profile, standalone_host_mode_profile, synoptikon_integrated_host_mode_profile,
    AuditAuthority, AuthenticationAuthority, HostModeProfile, HostModeProfileError,
    ProductHostMode, StorageAuthority, DASOBJECTSTORE_PRODUCT_ROOT,
    DASOBJECTSTORE_STANDALONE_HTTPS_PORT,
};
pub use integrated_session::{
    accept_synoptikon_integrated_session, SynoptikonIntegratedAcceptedSession,
    SynoptikonIntegratedActor, SynoptikonIntegratedSessionError, SynoptikonIntegratedSessionIssue,
    SYNOPTIKON_INTEGRATED_SESSION_SCHEMA_VERSION,
};
pub use monas_host_boundary::{
    validate_monas_standalone_host_boundary, MonasStandaloneHostBoundary,
    MonasStandaloneHostBoundaryContext, MonasStandaloneHostBoundaryError,
};
pub use nas_nfs_endpoint::{
    validate_nas_nfs_endpoint_definition, NasNfsEndpointDefinition, NasNfsEndpointValidationError,
    NasNfsEndpointValidationStatus, ValidatedNasNfsEndpointDefinition,
    NAS_NFS_ENDPOINT_DEFINITION_SCHEMA_VERSION,
};
pub use nas_nfs_runtime::{
    plan_nas_nfs_runtime_validation, NasNfsMountMode, NasNfsMountProbePlan, NasNfsMountScope,
    NasNfsObjectServiceProbePlan, NasNfsRuntimeProbeStep, NasNfsRuntimeValidationPlan,
    NasNfsRuntimeValidationPlanError, NasNfsTenantContractBoundary,
    NAS_NFS_RUNTIME_VALIDATION_PLAN_SCHEMA_VERSION,
};
pub use policy_templates::{
    ProductPolicyAdapterKind, ProductPolicyTemplateAdapter, ProductPolicyTemplateAdapterError,
    ProductPolicyTemplateEnvelope, PRODUCT_POLICY_TEMPLATE_SCHEMA_VERSION,
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
pub use prosopikon::{
    dasobjectstore_prosopikon_profile, dasobjectstore_prosopikon_relationships,
    dasobjectstore_prosopikon_snapshot,
};
pub use storage_definition::{
    export_mneion_das_storage_definition, export_mneion_nfs_storage_definition,
    export_mneion_storage_definition, MneionDasObjectStoreEndpoint,
    MneionDasObjectStoreEndpointKind, MneionDasObjectStoreEndpointLocation,
    MneionEndpointObjectContract, MneionManagedStorageDefinitionExport,
    MneionObjectStoreCreateRequest, MneionStorageDefinitionError, MneionStorageDefinitionExport,
    MneionStorageDefinitionRequest, MNEION_DASOBJECTSTORE_DAS_BACKEND_KIND,
    MNEION_DASOBJECTSTORE_NFS_BACKEND_KIND, MNEION_S3_BACKEND_KIND,
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
        assert_eq!(version(), env!("CARGO_PKG_VERSION"));
    }
}
