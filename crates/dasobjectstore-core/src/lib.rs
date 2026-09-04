//! Core domain types for DASObjectStore.

pub mod application_auth;
pub mod application_auth_v2;
pub mod backend;
pub mod bootstrap_plan;
pub mod capacity;
pub mod config;
pub mod demo_prerequisite;
pub mod deployment;
pub mod enclosure_registry;
pub mod file_export;
pub mod health;
pub mod ids;
pub mod ingress;
pub mod jenkins_dossier_evidence;
pub mod jenkins_dossier_readback;
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
pub mod retained_dossier_prerequisite;
pub mod risk;
pub mod s6_dossier_custody;
pub mod store;
pub mod subobject_capacity;
pub mod synoptikon_projection;
pub mod synthetic_scoped_readback;
pub mod utc;
pub mod workspace;

pub use application_auth::{
    AccessTokenClaims, AccessTokenExchangeRequest, ApplicationAuthValidationError,
    ApplicationCredentialKind, ApplicationEnvironment, ApplicationExchangeProofVerifier,
    ApplicationIdentity, ApplicationKeyAlgorithm, ApplicationKeyDescriptor, ApplicationOperation,
    ApplicationScope, RenewalTokenClaims, UploadCompletionCapability,
    APPLICATION_AUTH_SCHEMA_VERSION, MAX_ACCESS_TOKEN_TTL_SECONDS,
    MAX_DEVELOPMENT_ACCESS_TOKEN_TTL_SECONDS, MAX_UPLOAD_COMPLETION_TTL_SECONDS,
};
pub use backend::{
    catalogue_logical_used_bytes, BackendCapabilities, BackendError, BackendHealth,
    BackendObjectKey, BackendObjectRecord, BackendOperation, ObjectCatalogueAuthority,
    ObjectStoreBackend,
};
pub use bootstrap_plan::{
    assess_r237_bootstrap_local_observation, canonical_r237_bootstrap_observer_report,
    R237BootstrapLocalObservationV1, R237BootstrapObserverDenialV1,
    R237BootstrapObserverDispositionV1, R237BootstrapObserverReportBodyV1,
    R237BootstrapObserverReportV1, R237HddObservationV1, R237ObservationCheckV1,
    R237ObservationStatusV1, R237ObservedMediaV1, R237ReviewedBootstrapTupleV1,
    R237_BOOTSTRAP_LOCAL_OBSERVATION_V1_SCHEMA, R237_BOOTSTRAP_OBSERVER_REPORT_V1_SCHEMA,
    R237_BUCKET_NAME, R237_CANONICAL_PROGRAMME_MAIN_REVISION, R237_NUC_HOST,
    R237_REQUIRED_FREE_BYTES_PER_SELECTED_HDD, R237_STORE_ID, R237_TRANSACTION_DOCUMENT_SHA256,
    R237_TRANSACTION_DOCUMENT_SOURCE_REVISION, R237_WRITER_GROUP,
};
pub use config::{
    DEFAULT_PRODUCT_ROOT, DEFAULT_STANDALONE_BIND_ADDRESS, DEFAULT_STANDALONE_CONFIG_PATH,
    DEFAULT_STANDALONE_HTTPS_PORT,
};
pub use dasobjectstore_reference::{
    AuthorityScopeV1, DigestV1, EvidenceRefV1, ObjectRefV1, ReferenceDecodeError,
    ReferenceValidationError,
};
pub use demo_prerequisite::{
    ArtifactPrerequisiteError, MonasOikodomeArtifactPrerequisiteV1,
    VerifiedMonasOikodomeArtifactV1, MONAS_OIKODOME_ARTIFACT_PREREQUISITE_V1_SCHEMA,
};
pub use deployment::{DeploymentProfile, HostMode};
pub use enclosure_registry::{
    PhysicalBay, PhysicalEnclosure, PhysicalEnclosureRegistry,
    PhysicalEnclosureRegistryValidationError, PHYSICAL_ENCLOSURE_REGISTRY_SCHEMA_VERSION,
};
pub use jenkins_dossier_evidence::{
    JenkinsDossierEvidenceProjectionError, JenkinsDossierEvidenceProjectionV1,
    JENKINS_DOSSIER_EVIDENCE_KIND, JENKINS_DOSSIER_EVIDENCE_PROJECTION_V1_SCHEMA,
};
pub use jenkins_dossier_readback::{
    verify_jenkins_dossier_readback, JenkinsDossierReadbackError, JenkinsDossierReadbackV1,
};
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
pub use retained_dossier_prerequisite::{
    JenkinsRetainedDossierDasPrerequisiteV1, RetainedDossierPrerequisiteError,
    VerifiedRetainedDossierDasReadbackV1, JENKINS_RETAINED_DOSSIER_DAS_PREREQUISITE_V1_SCHEMA,
};
pub use s6_dossier_custody::{
    inspect_s6_dossier_custody, preflight_s6_dossier_custody, retain_s6_dossier_corpus,
    verify_s6_dossier_corpus, verify_s6_dossier_readback_receipt, S6DossierAcceptedAuthorityV1,
    S6DossierCorpusManifestV1, S6DossierCreateOutcomeV1, S6DossierCustodyBindingV1,
    S6DossierCustodyError, S6DossierCustodyPreflightV1, S6DossierFallbackReasonV1,
    S6DossierFixedPeerGrantV1, S6DossierMemberRefV1, S6DossierMemberV1, S6DossierPeerChannelV1,
    S6DossierRawAttachmentV1, S6DossierReadbackReceiptV1, S6DossierReaderPortV1,
    S6DossierRetentionResultV1, S6DossierSubjectV1, S6DossierWriterPortV1,
    S6_DOSSIER_ACCEPTED_AUTHORITY_V1_SCHEMA, S6_DOSSIER_CORPUS_V1_SCHEMA,
    S6_DOSSIER_CUSTODY_BINDING_V1_SCHEMA, S6_DOSSIER_FIXED_PEER_GRANT_V1_SCHEMA,
    S6_DOSSIER_MANIFEST_V1_SCHEMA, S6_DOSSIER_OBJECT_PREFIX, S6_DOSSIER_PROFILE_ID,
    S6_DOSSIER_READBACK_RECEIPT_V1_SCHEMA, S6_DOSSIER_SUBJECT_V1_SCHEMA,
};
pub use store::LogicalObjectVersionCharge;
pub use subobject_capacity::{
    ExpiredSubObjectCapacityReservation, SubObjectCapacityError, SubObjectCapacityLedger,
    SubObjectCapacityLedgerSnapshot, SubObjectCapacityReservationScope,
    SUBOBJECT_CAPACITY_SNAPSHOT_SCHEMA_VERSION,
};
pub use synoptikon_projection::{
    authenticate_das_owned_synoptikon_projection_readiness, settle_synoptikon_projection,
    synoptikon_tls_leaf_der_sha256, validate_synoptikon_projection_request,
    verify_das_owned_synoptikon_projection_readiness, DasAuthenticatedProjectionReadinessV1,
    DasCatalogueMappingEvidenceV1, DasCatalogueObjectEvidenceV1, DasHddReplicaEvidenceV1,
    DasMappingExclusionSettlementV1, DasProviderGroupStatusEvidenceV1,
    DasUploadCompletionEvidenceV1, SynoptikonProjectionDispositionV1, SynoptikonProjectionError,
    SynoptikonProjectionReadinessV1, SynoptikonProjectionRequestV1,
    SynoptikonProjectionSettlementOutcomeV1, SynoptikonProjectionSettlementV1,
    VerifiedSynoptikonProjectionReadinessV1, SYNOPTIKON_PROJECTION_CONSUMER_HOST,
    SYNOPTIKON_PROJECTION_CONSUMER_PRODUCT, SYNOPTIKON_PROJECTION_ENDPOINT,
    SYNOPTIKON_PROJECTION_MAX_HDD_REPLICAS, SYNOPTIKON_PROJECTION_MAX_LIFETIME_SECONDS,
    SYNOPTIKON_PROJECTION_MAX_READINESS_AGE_SECONDS, SYNOPTIKON_PROJECTION_OWNER_KEY_PATH,
    SYNOPTIKON_PROJECTION_PRODUCER_HOST, SYNOPTIKON_PROJECTION_PRODUCER_PRODUCT,
    SYNOPTIKON_PROJECTION_READINESS_V1_SCHEMA, SYNOPTIKON_PROJECTION_REQUEST_V1_SCHEMA,
    SYNOPTIKON_PROJECTION_SETTLEMENT_V1_SCHEMA, SYNOPTIKON_PROJECTION_TLS_CERTIFICATE_PATH,
    SYNOPTIKON_PROJECTION_TLS_EXPECTATION_PATH,
};
pub use synthetic_scoped_readback::{
    verify_synthetic_scoped_readback, MonasScopedReadCapabilityV1,
    MonasScopedReadCapabilityVerifier, SyntheticReadbackError, SyntheticReadbackInputV1,
    SyntheticReadbackSettlementV1, DAS_SYNTHETIC_READBACK_SETTLEMENT_V1_SCHEMA,
    MONAS_SCOPED_READ_CAPABILITY_V1_SCHEMA, SYNTHETIC_SEVEN_DAY_RETENTION_CLASS,
};
pub use workspace::{
    plan_workspace_capacity, ComputeClientIdentity, ComputeWorkspace, ComputeWorkspaceState,
    WorkspaceBranch, WorkspaceCapacityCandidate, WorkspaceCapacityPlan, WorkspaceCapacityPlanError,
    WorkspaceCheckpoint, WorkspaceMaterialization, WorkspacePromotedOutput,
    WorkspaceTransitionError, COMPUTE_WORKSPACE_SCHEMA_VERSION,
};

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
