//! Transport-neutral daemon API contracts.

mod appliance_telemetry;
mod application_identity;
mod application_mtls;
mod application_token;
mod application_upload;
mod capacity;
mod disk_lockdown;
mod disk_mutation;
mod enclosure;
mod endpoint;
mod health;
mod ingest;
#[path = "ingest/control.rs"]
pub(crate) mod ingest_control;
mod ingest_mutation;
mod jobs;
mod live_status;
mod local_admin;
mod object_browser;
mod object_mutation;
mod object_store;
mod profile_binding;
mod profile_browser;
mod profile_capabilities;
mod profile_catalogue;
mod profile_diagnostics;
mod profile_inspection;
mod profile_migration;
mod profile_readiness;
mod profile_s3;
mod provider_stream;
mod remote_easyconnect;
mod request_validation;
mod service;
mod storage_mutation;
mod store_deduplicate;
mod store_policy;
mod store_repair;
mod store_verify;
mod stores;

pub use appliance_telemetry::{
    query_appliance_telemetry, ApplianceTelemetryCapacityPoint, ApplianceTelemetryCapacitySummary,
    ApplianceTelemetryCurrentSummary, ApplianceTelemetryDiskCapacitySummary,
    ApplianceTelemetryDiskIoPoint, ApplianceTelemetryDiskIoSeries, ApplianceTelemetryDiskIoSummary,
    ApplianceTelemetryMissingInterval, ApplianceTelemetryPercentPoint, ApplianceTelemetryRequest,
    ApplianceTelemetryResponse, ApplianceTelemetrySeries, ApplianceTelemetrySessionPoint,
    ApplianceTelemetrySessionSummary, ApplianceTelemetryState, ApplianceTelemetryWindow,
    ApplianceTelemetryWindowAvailability,
};
pub use application_identity::{
    ApplicationCredentialEnrollmentRecord, ApplicationCredentialRevocationRequest,
    ApplicationCredentialRevocationResponse, ApplicationCredentialRevocationValidationError,
    ApplicationIdentityRegistrationRequest, ApplicationIdentityRegistrationResponse,
    ApplicationIdentityRegistrationValidationError, ApplicationKeyRegistrationRequest,
    ApplicationKeyRegistrationResponse, ApplicationKeyRegistrationValidationError,
    ApplicationRegistrationRecord, APPLICATION_CREDENTIAL_REVOCATION_CONFIRMATION,
    APPLICATION_IDENTITY_REGISTRATION_CONFIRMATION,
};
pub use application_mtls::{
    ApplicationMtlsAuthorizationContext, ApplicationMtlsAuthorizationRequest,
    ApplicationMtlsAuthorizationResponse,
};
pub use application_token::{
    ApplicationAccessTokenExchangeRequest, ApplicationAccessTokenExchangeResponse,
    APPLICATION_ACCESS_TOKEN_EXCHANGE_ROUTE,
};
pub use application_upload::{
    ApplicationUploadCapabilityIssueRequest, ApplicationUploadCapabilityIssueResponse,
    ApplicationUploadCompletionOutcome, ApplicationUploadCompletionRequest,
    ApplicationUploadCompletionResponse, APPLICATION_UPLOAD_COMPLETION_CAPABILITY_ROUTE,
    APPLICATION_UPLOAD_COMPLETION_ROUTE,
};
pub use capacity::{
    CapacityAdmissionDecision, CapacityAdmissionRejectionReason, CapacityAdmissionRequest,
    CapacityAdmissionReservationError, CapacityAdmissionResponse, CapacityAdmissionValidationError,
    CapacityStatusRequest, CapacityStatusResponse,
};
pub use disk_lockdown::{
    DiskLockdownRequest, DiskLockdownResponse, DiskLockdownValidationError,
    DISK_LOCKDOWN_CONFIRMATION,
};
pub use disk_mutation::{
    DiskForceRetireRequest, DiskRetireRequest, DiskRetireResponse, DiskRetireValidationError,
    FORCE_DISK_RETIRE_CONFIRMATION,
};
pub use enclosure::{
    PrepareEnclosureFilesystem, PrepareEnclosureHddDevice, PrepareEnclosureRequest,
    PrepareEnclosureResponse, PrepareEnclosureValidationError, ENCLOSURE_PREPARE_CONFIRMATION,
};
pub use endpoint::{
    DaemonEndpointBinding, DaemonEndpointBindingReadiness, DaemonEndpointKind,
    DaemonEndpointValidation, DaemonEndpointValidationState, EndpointInventoryValidationError,
    UpsertEndpointInventoryRequest, UpsertEndpointInventoryResponse, ENDPOINT_RECORD_CONFIRMATION,
};
pub use health::{
    DaemonApiWarning, DaemonDiskHealthSummary, DaemonHealthSummaryRequest,
    DaemonHealthSummaryResponse, DaemonIngestSummary, DaemonSsdPressure,
};
pub use ingest::DaemonIngestHddTransferPhase;
pub use ingest::{
    decide_ingest_admission, CancelIngestJobRequest, CancelIngestJobResponse,
    DaemonIngestAcknowledgementState, DaemonIngestAdaptiveSchedulerInput,
    DaemonIngestAdaptiveSchedulingLimit, DaemonIngestAdaptiveWorkerSchedule,
    DaemonIngestAdmissionAction, DaemonIngestAdmissionDecision, DaemonIngestAdmissionInput,
    DaemonIngestAdmissionReason, DaemonIngestBottleneck, DaemonIngestBoundedBufferPolicy,
    DaemonIngestBufferPoolPolicySet, DaemonIngestCompletionFraction, DaemonIngestConflictAction,
    DaemonIngestConflictDecision, DaemonIngestConflictPolicy, DaemonIngestConflictReason,
    DaemonIngestErrorRate, DaemonIngestHddActiveTransfer, DaemonIngestHddQueueState,
    DaemonIngestHddTargetQueue, DaemonIngestObjectCompletion, DaemonIngestObjectSnapshot,
    DaemonIngestPipelinePressure, DaemonIngestPipelineStage, DaemonIngestPlacementSchedulerInput,
    DaemonIngestPressure, DaemonIngestProgressEvent, DaemonIngestProgressFractions,
    DaemonIngestQueueDepths, DaemonIngestResourceBudget, DaemonIngestResourceGate,
    DaemonIngestResourceLease, DaemonIngestResourcePolicy, DaemonIngestResourceReservation,
    DaemonIngestResourceReservationError, DaemonIngestSchedulingPolicy, DaemonIngestStage,
    DaemonIngestSystemSafetyReserve, DaemonIngestSystemTelemetry, DaemonIngestTargetCapacity,
    DaemonIngestTargetFailureState, DaemonIngestTelemetry, DaemonIngestThroughputTelemetry,
    DaemonIngestThroughputTrend, DaemonIngestWorkerActivity, DaemonIngestWorkerCounts,
    DaemonIngestWorkerTelemetry, DaemonIngressLandingMode, DaemonIngressOrigin,
    DaemonRequestValidationError, DaemonSourceReadBackpressureAction,
    DaemonSourceReadBackpressureDecision, DaemonSourceReadBackpressureInput,
    DaemonSourceReadBackpressurePolicy, DaemonSourceReadBackpressureReason,
    DaemonSourceReadPriority, DaemonSourceToSsdPriorityPolicy, DaemonSourceToSsdQueueUsage,
    IngestJobStatusRequest, IngestJobStatusResponse, SubmitIngestFilesRequest,
    SubmitIngestFilesResponse,
};
pub use ingest_control::{
    DaemonIngestControlAction, DaemonIngestControlState, IngestControlRequest,
    IngestControlResponse, IngestControlValidationError, INGEST_CONTROL_CONFIRMATION,
};
pub use ingest_mutation::{
    IngestQueueDrainRequest, IngestQueueDrainResponse, IngestQueueDrainValidationError,
    INGEST_QUEUE_DRAIN_CONFIRMATION,
};
pub use jobs::{
    DaemonJobAcceptedResponse, DaemonJobCancelRequest, DaemonJobCancelResponse, DaemonJobEvent,
    DaemonJobId, DaemonJobIdError, DaemonJobKind, DaemonJobListRequest, DaemonJobListResponse,
    DaemonJobProgress, DaemonJobState, DaemonJobStatusRequest, DaemonJobStatusResponse,
    DaemonJobSummary, DaemonJobValidationError,
};
pub use live_status::{
    LiveStatusActor, LiveStatusAggregate, LiveStatusConnectionOrigin, LiveStatusGarbageCollection,
    LiveStatusGarbageCollectionRetained, LiveStatusIngest, LiveStatusRequest, LiveStatusResponse,
    LIVE_STATUS_SCHEMA_VERSION,
};
pub use local_admin::{
    AssignLocalUserToLocalGroupRequest, AssignLocalUserToLocalGroupResponse,
    CreateLocalGroupRequest, CreateLocalGroupResponse, DaemonLocalAdminAcceptedResponse,
    DaemonLocalAdminCommand, DaemonLocalAdminValidationError,
};
pub use object_browser::{
    ObjectBrowserBreadcrumb, ObjectBrowserChecksum, ObjectBrowserDelegatedActor,
    ObjectBrowserDownloadSource, ObjectBrowserFileNode, ObjectBrowserFolderNode,
    ObjectBrowserPageRequest, ObjectBrowserPlacement, ObjectBrowserPlacementLocation,
    ObjectBrowserPlacementState, ObjectBrowserReadinessState, ObjectBrowserRequest,
    ObjectBrowserResponse, ObjectBrowserSort, ObjectDownloadRequest, ObjectDownloadResponse,
    ObjectFolderArchiveEntry, ObjectFolderDownloadRequest, ObjectFolderDownloadResponse,
    OBJECT_BROWSER_MAX_PAGE_LIMIT,
};
pub use object_mutation::{ObjectPutRequest, ObjectPutResponse, ObjectPutValidationError};
pub use object_store::{
    CreateObjectStoreRequest, CreateObjectStoreResponse, CreateObjectStoreValidationError,
    OBJECT_STORE_CREATE_CONFIRMATION,
};
pub use profile_binding::{
    ProfileBindingOperation, ProfileBindingRequest, ProfileBindingResponse,
    ProfileBindingValidationError, PROFILE_BINDING_CONFIRMATION,
};
pub use profile_browser::{
    ProfileBrowserEntry, ProfileBrowserRequest, ProfileBrowserResponse,
    PROFILE_BROWSER_MAX_PAGE_LIMIT, PROFILE_BROWSER_SCHEMA_VERSION,
};
pub use profile_capabilities::{
    discover_profile_capabilities, ObjectStoreCapabilityDiscoveryRequest,
    ObjectStoreCapabilityDiscoveryResponse, ObjectStoreCapabilityValidationError,
};
pub use profile_catalogue::{
    ProfileCatalogueExportRequest, ProfileCatalogueExportResponse, ProfileCatalogueImportRequest,
    ProfileCatalogueImportResponse, PROFILE_CATALOGUE_SCHEMA_VERSION,
};
pub use profile_diagnostics::{
    ProfileDiagnosticsRequest, ProfileDiagnosticsResponse, ProfileDiagnosticsState,
    PROFILE_DIAGNOSTICS_SCHEMA_VERSION,
};
pub use profile_inspection::{
    ProfileInspectionRequest, ProfileInspectionResponse, ProfileInspectionRootState,
    PROFILE_INSPECTION_SCHEMA_VERSION,
};
pub use profile_migration::{
    ProfileMigrationRequest, ProfileMigrationResponse, ProfileMigrationValidationError,
    PROFILE_MIGRATION_CONFIRMATION,
};
pub use profile_readiness::{
    ProfileLifecycleState, ProfileReadinessRequest, ProfileReadinessResponse,
    PROFILE_READINESS_ROUTE, PROFILE_READINESS_SCHEMA_VERSION,
};
pub use profile_s3::{
    ProfileS3DeleteRequest, ProfileS3DeleteResponse, ProfileS3HeadRequest, ProfileS3HeadResponse,
    ProfileS3HealthRequest, ProfileS3HealthResponse, ProfileS3ListRequest, ProfileS3ListResponse,
    ProfileS3MultipartAbortRequest, ProfileS3MultipartAbortResponse,
    ProfileS3MultipartCompletionRequest, ProfileS3MultipartCompletionResponse,
    ProfileS3MultipartPartRequest, ProfileS3ObjectView, ProfileS3VerifyRequest,
    ProfileS3VerifyResponse, PROFILE_S3_DELETE_ROUTE, PROFILE_S3_HEALTH_ROUTE, PROFILE_S3_MAX_KEYS,
    PROFILE_S3_MAX_MULTIPART_PARTS, PROFILE_S3_MULTIPART_COMPLETE_ROUTE,
    PROFILE_S3_MULTIPART_PART_ROUTE, PROFILE_S3_OBJECTS_ROUTE, PROFILE_S3_OBJECT_ROUTE,
    PROFILE_S3_ROUTE_PREFIX, PROFILE_S3_SCHEMA_VERSION,
};
pub use provider_stream::{
    read_provider_stream_frame, write_provider_stream_frame, ProviderStreamCancellation,
    ProviderStreamChunkHeader, ProviderStreamCondition, ProviderStreamFrameError,
    ProviderStreamMultipartPartUploadOpenRequest, ProviderStreamMultipartPartUploadResponse,
    ProviderStreamOpenRequest, ProviderStreamRange, ProviderStreamUploadOpenRequest,
    ProviderStreamUploadResponse, ProviderStreamValidationError, ProviderStreamVerificationError,
    ProviderStreamVerifier, PROVIDER_STREAM_MAX_CHUNK_BYTES, PROVIDER_STREAM_MAX_HEADER_BYTES,
    PROVIDER_STREAM_SCHEMA_VERSION,
};
pub use remote_easyconnect::{
    decide_remote_easyconnect_upload_admission, plan_remote_easyconnect_upload_handoff,
    remote_easyconnect_object_store_grants_for_actor,
    remote_easyconnect_renew_after_offset_seconds,
    resolve_remote_easyconnect_session_lifetime_seconds, RemoteEasyconnectApprovePairingRequest,
    RemoteEasyconnectApprovePairingResponse, RemoteEasyconnectAuthProvider,
    RemoteEasyconnectAwsCliEnvironmentVariable, RemoteEasyconnectCreatePairingRequest,
    RemoteEasyconnectCreatePairingResponse, RemoteEasyconnectDiscoveryRequest,
    RemoteEasyconnectDiscoveryResponse, RemoteEasyconnectExchangePairingRequest,
    RemoteEasyconnectExchangePairingResponse, RemoteEasyconnectObjectStoreAccessPolicy,
    RemoteEasyconnectObjectStoreGrant, RemoteEasyconnectRenewSessionRequest,
    RemoteEasyconnectRenewSessionResponse, RemoteEasyconnectRevokeSessionRequest,
    RemoteEasyconnectRevokeSessionResponse, RemoteEasyconnectSession,
    RemoteEasyconnectSessionCredentials, RemoteEasyconnectSessionPolicy,
    RemoteEasyconnectSessionRenewal, RemoteEasyconnectSubmitAwsCliUploadRequest,
    RemoteEasyconnectSubmitAwsCliUploadResponse, RemoteEasyconnectUploadAdmissionDecision,
    RemoteEasyconnectUploadAdmissionRequest, RemoteEasyconnectUploadBackpressureReason,
    RemoteEasyconnectUploadCompletion, RemoteEasyconnectUploadHandoffFailure,
    RemoteEasyconnectUploadHandoffMode, RemoteEasyconnectUploadHandoffRequest,
    RemoteEasyconnectUploadHandoffResponse, RemoteEasyconnectUploadHandoffState,
    RemoteEasyconnectUploadProgressTelemetry, RemoteEasyconnectUploadSelectionEntry,
    RemoteEasyconnectValidationError, REMOTE_EASYCONNECT_DEFAULT_SESSION_LIFETIME_SECONDS,
    REMOTE_EASYCONNECT_DISCOVERY_ROUTE, REMOTE_EASYCONNECT_LOCAL_AGENT_HANDOFF_ROUTE,
    REMOTE_EASYCONNECT_MAX_SESSION_LIFETIME_SECONDS,
    REMOTE_EASYCONNECT_MIN_SESSION_LIFETIME_SECONDS, REMOTE_EASYCONNECT_PAIRINGS_ROUTE,
    REMOTE_EASYCONNECT_PAIRING_APPROVAL_ROUTE_TEMPLATE, REMOTE_EASYCONNECT_PAIRING_EXCHANGE_ROUTE,
    REMOTE_EASYCONNECT_RENEWAL_LEAD_SECONDS, REMOTE_EASYCONNECT_SESSIONS_ROUTE,
    REMOTE_EASYCONNECT_SESSION_RENEW_ROUTE_TEMPLATE, REMOTE_EASYCONNECT_SESSION_ROUTE_TEMPLATE,
};
pub use service::{
    DaemonServiceLifecycleRequest, DaemonServiceLifecycleResponse, DaemonServiceOperation,
    DaemonServiceProvisionRequest, DaemonServiceProvisionResponse, DaemonServiceStatusDetail,
    DaemonServiceStatusRequest, DaemonServiceStatusResponse,
};
pub use storage_mutation::{
    ProfileRetirementReport, StoreDeleteCommandReport, StoreDeleteRequest, StoreDeleteResponse,
    StoreDeleteValidationError, StoreDrainRequest, StoreDrainResponse, StoreDrainValidationError,
    STORE_DELETE_CONFIRMATION, STORE_DRAIN_CONFIRMATION,
};
pub use store_deduplicate::{
    StoreDeduplicateReport, StoreDeduplicateRequest, StoreDeduplicateResponse,
    StoreDeduplicateValidationError, STORE_DEDUPLICATE_CONFIRMATION,
};
pub use store_policy::{
    UpdateObjectStoreAcknowledgementPolicyRequest, UpdateObjectStoreAcknowledgementPolicyResponse,
    UpdateObjectStoreAcknowledgementPolicyValidationError, UpdateObjectStoreIngestPolicyRequest,
    UpdateObjectStoreIngestPolicyResponse, UpdateObjectStoreIngestPolicyValidationError,
    ACKNOWLEDGEMENT_POLICY_CONFIRMATION, DIRECT_TO_HDD_POLICY_CONFIRMATION,
};
pub use store_repair::{
    CompletedSnapshotOutcome, StoreRepairReport, StoreRepairRequest, StoreRepairResponse,
    StoreRepairS3Reconciliation, StoreRepairValidationError, STORE_REPAIR_CONFIRMATION,
};
pub use store_verify::{StoreVerifyReport, StoreVerifyRequest, StoreVerifyResponse};
pub use stores::{StoreInventoryItem, StoreInventoryRequest, StoreInventoryResponse};

use request_validation::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "command", content = "payload")]
pub enum DaemonApiRequest {
    LiveStatus(LiveStatusRequest),
    HealthSummary(DaemonHealthSummaryRequest),
    DiskRetire(DiskRetireRequest),
    DiskForceRetire(DiskForceRetireRequest),
    DiskLockdown(DiskLockdownRequest),
    StoreInventory(StoreInventoryRequest),
    StoreDrain(StoreDrainRequest),
    StoreDelete(StoreDeleteRequest),
    StoreVerify(StoreVerifyRequest),
    StoreDeduplicate(StoreDeduplicateRequest),
    StoreRepair(StoreRepairRequest),
    ObjectPut(ObjectPutRequest),
    IngestQueueDrain(IngestQueueDrainRequest),
    IngestControl(IngestControlRequest),
    SubmitIngestFiles(SubmitIngestFilesRequest),
    IngestJobStatus(IngestJobStatusRequest),
    CancelIngestJob(CancelIngestJobRequest),
    JobList(DaemonJobListRequest),
    JobStatus(DaemonJobStatusRequest),
    CancelJob(DaemonJobCancelRequest),
    ServiceStatus(DaemonServiceStatusRequest),
    ApplianceTelemetry(ApplianceTelemetryRequest),
    ServiceLifecycle(DaemonServiceLifecycleRequest),
    ServiceProvision(DaemonServiceProvisionRequest),
    RegisterApplicationIdentity(ApplicationIdentityRegistrationRequest),
    RegisterApplicationKey(ApplicationKeyRegistrationRequest),
    RevokeApplicationCredential(ApplicationCredentialRevocationRequest),
    AuthorizeApplicationMtls(ApplicationMtlsAuthorizationRequest),
    ExchangeApplicationAccessToken(ApplicationAccessTokenExchangeRequest),
    IssueApplicationUploadCapability(ApplicationUploadCapabilityIssueRequest),
    CompleteApplicationUpload(ApplicationUploadCompletionRequest),
    PrepareEnclosure(PrepareEnclosureRequest),
    CreateObjectStore(CreateObjectStoreRequest),
    RegisterProfileBinding(ProfileBindingRequest),
    ProfileMigration(ProfileMigrationRequest),
    ProfileBrowser(ProfileBrowserRequest),
    ProfileCatalogueExport(ProfileCatalogueExportRequest),
    ProfileCatalogueImport(ProfileCatalogueImportRequest),
    ProfileS3List(ProfileS3ListRequest),
    ProfileS3Delete(ProfileS3DeleteRequest),
    ProfileS3MultipartComplete(ProfileS3MultipartCompletionRequest),
    ProfileS3MultipartAbort(ProfileS3MultipartAbortRequest),
    ProfileS3Head(ProfileS3HeadRequest),
    ProfileS3Verify(ProfileS3VerifyRequest),
    ProfileS3Health(ProfileS3HealthRequest),
    ProfileDiagnostics(ProfileDiagnosticsRequest),
    ProfileInspection(ProfileInspectionRequest),
    ProfileReadiness(ProfileReadinessRequest),
    ProfileCapabilities(ObjectStoreCapabilityDiscoveryRequest),
    CapacityAdmission(CapacityAdmissionRequest),
    CapacityStatus(CapacityStatusRequest),
    UpdateObjectStoreIngestPolicy(UpdateObjectStoreIngestPolicyRequest),
    UpdateObjectStoreAcknowledgementPolicy(UpdateObjectStoreAcknowledgementPolicyRequest),
    ObjectBrowser(ObjectBrowserRequest),
    ObjectDownload(ObjectDownloadRequest),
    ObjectFolderDownload(ObjectFolderDownloadRequest),
    UpsertEndpointInventory(UpsertEndpointInventoryRequest),
    CreateLocalGroup(CreateLocalGroupRequest),
    AssignLocalUserToLocalGroup(AssignLocalUserToLocalGroupRequest),
    RemoteEasyconnectDiscovery(RemoteEasyconnectDiscoveryRequest),
    RemoteEasyconnectCreatePairing(RemoteEasyconnectCreatePairingRequest),
    RemoteEasyconnectApprovePairing(RemoteEasyconnectApprovePairingRequest),
    RemoteEasyconnectExchangePairing(RemoteEasyconnectExchangePairingRequest),
    RemoteEasyconnectRevokeSession(RemoteEasyconnectRevokeSessionRequest),
    RemoteEasyconnectRenewSession(RemoteEasyconnectRenewSessionRequest),
    RemoteEasyconnectUploadAdmission(RemoteEasyconnectUploadAdmissionRequest),
    RemoteEasyconnectSubmitAwsCliUpload(RemoteEasyconnectSubmitAwsCliUploadRequest),
}

impl DaemonApiRequest {
    pub(crate) fn command_name(&self) -> &'static str {
        match self {
            Self::LiveStatus(_) => "live_status",
            Self::HealthSummary(_) => "health_summary",
            Self::DiskRetire(_) => "disk_retire",
            Self::DiskForceRetire(_) => "disk_force_retire",
            Self::DiskLockdown(_) => "disk_lockdown",
            Self::StoreInventory(_) => "store_inventory",
            Self::StoreDrain(_) => "store_drain",
            Self::StoreDelete(_) => "store_delete",
            Self::StoreVerify(_) => "store_verify",
            Self::StoreDeduplicate(_) => "store_deduplicate",
            Self::StoreRepair(_) => "store_repair",
            Self::ObjectPut(_) => "object_put",
            Self::IngestQueueDrain(_) => "ingest_queue_drain",
            Self::IngestControl(_) => "ingest_control",
            Self::SubmitIngestFiles(_) => "submit_ingest_files",
            Self::IngestJobStatus(_) => "ingest_job_status",
            Self::CancelIngestJob(_) => "cancel_ingest_job",
            Self::JobList(_) => "job_list",
            Self::JobStatus(_) => "job_status",
            Self::CancelJob(_) => "cancel_job",
            Self::ServiceStatus(_) => "service_status",
            Self::ApplianceTelemetry(_) => "appliance_telemetry",
            Self::ServiceLifecycle(_) => "service_lifecycle",
            Self::ServiceProvision(_) => "service_provision",
            Self::RegisterApplicationIdentity(_) => "register_application_identity",
            Self::RegisterApplicationKey(_) => "register_application_key",
            Self::RevokeApplicationCredential(_) => "revoke_application_credential",
            Self::AuthorizeApplicationMtls(_) => "authorize_application_mtls",
            Self::ExchangeApplicationAccessToken(_) => "exchange_application_access_token",
            Self::IssueApplicationUploadCapability(_) => "issue_application_upload_capability",
            Self::CompleteApplicationUpload(_) => "complete_application_upload",
            Self::PrepareEnclosure(_) => "prepare_enclosure",
            Self::CreateObjectStore(_) => "create_object_store",
            Self::RegisterProfileBinding(_) => "register_profile_binding",
            Self::ProfileMigration(_) => "profile_migration",
            Self::ProfileBrowser(_) => "profile_browser",
            Self::ProfileCatalogueExport(_) => "profile_catalogue_export",
            Self::ProfileCatalogueImport(_) => "profile_catalogue_import",
            Self::ProfileS3List(_) => "profile_s3_list",
            Self::ProfileS3Delete(_) => "profile_s3_delete",
            Self::ProfileS3MultipartComplete(_) => "profile_s3_multipart_complete",
            Self::ProfileS3MultipartAbort(_) => "profile_s3_multipart_abort",
            Self::ProfileS3Head(_) => "profile_s3_head",
            Self::ProfileS3Verify(_) => "profile_s3_verify",
            Self::ProfileS3Health(_) => "profile_s3_health",
            Self::ProfileDiagnostics(_) => "profile_diagnostics",
            Self::ProfileInspection(_) => "profile_inspection",
            Self::ProfileReadiness(_) => "profile_readiness",
            Self::ProfileCapabilities(_) => "profile_capabilities",
            Self::CapacityAdmission(_) => "capacity_admission",
            Self::CapacityStatus(_) => "capacity_status",
            Self::UpdateObjectStoreIngestPolicy(_) => "update_object_store_ingest_policy",
            Self::UpdateObjectStoreAcknowledgementPolicy(_) => {
                "update_object_store_acknowledgement_policy"
            }
            Self::ObjectBrowser(_) => "object_browser",
            Self::ObjectDownload(_) => "object_download",
            Self::ObjectFolderDownload(_) => "object_folder_download",
            Self::UpsertEndpointInventory(_) => "upsert_endpoint_inventory",
            Self::CreateLocalGroup(_) => "create_local_group",
            Self::AssignLocalUserToLocalGroup(_) => "assign_local_user_to_local_group",
            Self::RemoteEasyconnectDiscovery(_) => "remote_easyconnect_discovery",
            Self::RemoteEasyconnectCreatePairing(_) => "remote_easyconnect_create_pairing",
            Self::RemoteEasyconnectApprovePairing(_) => "remote_easyconnect_approve_pairing",
            Self::RemoteEasyconnectExchangePairing(_) => "remote_easyconnect_exchange_pairing",
            Self::RemoteEasyconnectRevokeSession(_) => "remote_easyconnect_revoke_session",
            Self::RemoteEasyconnectRenewSession(_) => "remote_easyconnect_renew_session",
            Self::RemoteEasyconnectUploadAdmission(_) => "remote_easyconnect_upload_admission",
            Self::RemoteEasyconnectSubmitAwsCliUpload(_) => {
                "remote_easyconnect_submit_aws_cli_upload"
            }
        }
    }

    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        match self {
            Self::DiskRetire(request) => request.validate().map_err(disk_retire_validation_error),
            Self::DiskForceRetire(request) => {
                request.validate().map_err(disk_retire_validation_error)
            }
            Self::DiskLockdown(request) => {
                request.validate().map_err(disk_lockdown_validation_error)
            }
            Self::SubmitIngestFiles(request) => request.validate(),
            Self::IngestControl(request) => {
                request.validate().map_err(ingest_control_validation_error)
            }
            Self::StoreDrain(request) => request.validate().map_err(store_drain_validation_error),
            Self::StoreDelete(request) => request.validate().map_err(store_delete_validation_error),
            Self::StoreRepair(request) => {
                request
                    .validate()
                    .map_err(|_| DaemonRequestValidationError::ConfirmationMismatch {
                        expected: STORE_REPAIR_CONFIRMATION,
                    })
            }
            Self::StoreDeduplicate(request) => {
                request
                    .validate()
                    .map_err(|_| DaemonRequestValidationError::ConfirmationMismatch {
                        expected: STORE_DEDUPLICATE_CONFIRMATION,
                    })
            }
            Self::ObjectPut(request) => request.validate().map_err(object_put_validation_error),
            Self::IngestQueueDrain(request) => request
                .validate()
                .map_err(ingest_queue_drain_validation_error),
            Self::CancelIngestJob(request) => request.validate(),
            Self::CancelJob(request) => request.validate().map_err(generic_job_validation_error),
            Self::ServiceLifecycle(request) => request.validate(),
            Self::ServiceProvision(request) => request.validate(),
            Self::RegisterApplicationIdentity(request) => request
                .validate()
                .map_err(application_identity_registration_validation_error),
            Self::RegisterApplicationKey(request) => request
                .validate()
                .map_err(application_key_registration_validation_error),
            Self::RevokeApplicationCredential(request) => request
                .validate()
                .map_err(application_credential_revocation_validation_error),
            Self::AuthorizeApplicationMtls(request) => request.validate(),
            Self::ExchangeApplicationAccessToken(request) => request.validate(),
            Self::IssueApplicationUploadCapability(request) => request
                .validate()
                .map_err(|message| DaemonRequestValidationError::InvalidPolicy { message }),
            Self::CompleteApplicationUpload(request) => request
                .validate()
                .map_err(|message| DaemonRequestValidationError::InvalidPolicy { message }),
            Self::PrepareEnclosure(request) => request
                .validate()
                .map_err(prepare_enclosure_validation_error),
            Self::CreateObjectStore(request) => request
                .validate()
                .map_err(create_object_store_validation_error),
            Self::RegisterProfileBinding(request) => {
                request.validate().map_err(profile_binding_validation_error)
            }
            Self::ProfileMigration(request) => {
                request
                    .validate()
                    .map_err(|error| DaemonRequestValidationError::InvalidPolicy {
                        message: error.to_string(),
                    })
            }
            Self::ProfileBrowser(request) => request.validate(),
            Self::ProfileCatalogueExport(request) => request.validate(),
            Self::ProfileCatalogueImport(request) => request.validate(),
            Self::ProfileS3List(request) => request.validate(),
            Self::ProfileS3Delete(request) => request.validate(),
            Self::ProfileS3MultipartComplete(request) => request.validate(),
            Self::ProfileS3MultipartAbort(request) => request.validate(),
            Self::ProfileS3Head(request) => request.validate(),
            Self::ProfileS3Verify(request) => request.validate(),
            Self::ProfileS3Health(request) => request.validate(),
            Self::ProfileDiagnostics(request) => request.validate(),
            Self::ProfileInspection(_) => Ok(()),
            Self::ProfileReadiness(request) => request
                .validate()
                .map_err(|message| DaemonRequestValidationError::InvalidPolicy { message }),
            Self::ProfileCapabilities(_) => Ok(()),
            Self::CapacityAdmission(request) => request
                .validate()
                .map(|_| ())
                .map_err(capacity_admission_validation_error),
            Self::CapacityStatus(request) => request
                .validate()
                .map(|_| ())
                .map_err(capacity_admission_validation_error),
            Self::UpdateObjectStoreIngestPolicy(request) => request
                .validate()
                .map(|_| ())
                .map_err(update_object_store_ingest_policy_validation_error),
            Self::UpdateObjectStoreAcknowledgementPolicy(request) => request
                .validate()
                .map(|_| ())
                .map_err(update_object_store_acknowledgement_policy_validation_error),
            Self::ObjectBrowser(request) => request.validate(),
            Self::ObjectDownload(request) => request.validate(),
            Self::ObjectFolderDownload(request) => request.validate(),
            Self::UpsertEndpointInventory(request) => request
                .validate()
                .map_err(endpoint_inventory_validation_error),
            Self::CreateLocalGroup(request) => {
                request.validate().map_err(local_admin_validation_error)
            }
            Self::AssignLocalUserToLocalGroup(request) => {
                request.validate().map_err(local_admin_validation_error)
            }
            Self::RemoteEasyconnectCreatePairing(request) => request
                .validate()
                .map_err(remote_easyconnect_validation_error),
            Self::RemoteEasyconnectApprovePairing(request) => request
                .validate()
                .map_err(remote_easyconnect_validation_error),
            Self::RemoteEasyconnectExchangePairing(request) => request
                .validate()
                .map_err(remote_easyconnect_validation_error),
            Self::RemoteEasyconnectRevokeSession(request) => request
                .validate()
                .map_err(remote_easyconnect_validation_error),
            Self::RemoteEasyconnectRenewSession(request) => request
                .validate()
                .map_err(remote_easyconnect_validation_error),
            Self::RemoteEasyconnectSubmitAwsCliUpload(request) => request
                .validate()
                .map_err(remote_easyconnect_validation_error),
            Self::HealthSummary(_)
            | Self::LiveStatus(_)
            | Self::StoreInventory(_)
            | Self::StoreVerify(_)
            | Self::IngestJobStatus(_)
            | Self::JobList(_)
            | Self::JobStatus(_)
            | Self::ServiceStatus(_)
            | Self::ApplianceTelemetry(_)
            | Self::RemoteEasyconnectDiscovery(_)
            | Self::RemoteEasyconnectUploadAdmission(_) => Ok(()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "payload")]
pub enum DaemonApiResponse {
    LiveStatus(LiveStatusResponse),
    HealthSummary(DaemonHealthSummaryResponse),
    DiskRetire(DiskRetireResponse),
    DiskForceRetire(DiskRetireResponse),
    DiskLockdown(DiskLockdownResponse),
    ProviderStreamUpload(ProviderStreamUploadResponse),
    ProviderStreamMultipartPartUpload(ProviderStreamMultipartPartUploadResponse),
    StoreInventory(StoreInventoryResponse),
    StoreDrain(StoreDrainResponse),
    StoreDelete(StoreDeleteResponse),
    StoreVerify(StoreVerifyResponse),
    StoreDeduplicate(StoreDeduplicateResponse),
    StoreRepair(StoreRepairResponse),
    ObjectPut(ObjectPutResponse),
    IngestQueueDrain(IngestQueueDrainResponse),
    IngestControl(IngestControlResponse),
    SubmitIngestFiles(SubmitIngestFilesResponse),
    IngestJobStatus(IngestJobStatusResponse),
    CancelIngestJob(CancelIngestJobResponse),
    JobList(DaemonJobListResponse),
    JobStatus(DaemonJobStatusResponse),
    CancelJob(DaemonJobCancelResponse),
    ServiceStatus(DaemonServiceStatusResponse),
    ApplianceTelemetry(ApplianceTelemetryResponse),
    ServiceLifecycle(DaemonServiceLifecycleResponse),
    ServiceProvision(DaemonServiceProvisionResponse),
    RegisterApplicationIdentity(ApplicationIdentityRegistrationResponse),
    RegisterApplicationKey(ApplicationKeyRegistrationResponse),
    RevokeApplicationCredential(ApplicationCredentialRevocationResponse),
    AuthorizeApplicationMtls(ApplicationMtlsAuthorizationResponse),
    ExchangeApplicationAccessToken(ApplicationAccessTokenExchangeResponse),
    PrepareEnclosure(PrepareEnclosureResponse),
    CreateObjectStore(CreateObjectStoreResponse),
    RegisterProfileBinding(ProfileBindingResponse),
    ProfileMigration(ProfileMigrationResponse),
    ProfileBrowser(ProfileBrowserResponse),
    ProfileCatalogueExport(ProfileCatalogueExportResponse),
    ProfileCatalogueImport(ProfileCatalogueImportResponse),
    ProfileS3List(ProfileS3ListResponse),
    ProfileS3Delete(ProfileS3DeleteResponse),
    ProfileS3MultipartComplete(ProfileS3MultipartCompletionResponse),
    ProfileS3MultipartAbort(ProfileS3MultipartAbortResponse),
    ProfileS3Head(ProfileS3HeadResponse),
    ProfileS3Verify(ProfileS3VerifyResponse),
    ProfileS3Health(ProfileS3HealthResponse),
    ProfileDiagnostics(ProfileDiagnosticsResponse),
    ProfileInspection(ProfileInspectionResponse),
    ProfileReadiness(ProfileReadinessResponse),
    ProfileCapabilities(ObjectStoreCapabilityDiscoveryResponse),
    CapacityAdmission(CapacityAdmissionResponse),
    CapacityStatus(CapacityStatusResponse),
    UpdateObjectStoreIngestPolicy(UpdateObjectStoreIngestPolicyResponse),
    UpdateObjectStoreAcknowledgementPolicy(UpdateObjectStoreAcknowledgementPolicyResponse),
    ObjectBrowser(ObjectBrowserResponse),
    ObjectDownload(ObjectDownloadResponse),
    ObjectFolderDownload(ObjectFolderDownloadResponse),
    UpsertEndpointInventory(UpsertEndpointInventoryResponse),
    CreateLocalGroup(CreateLocalGroupResponse),
    AssignLocalUserToLocalGroup(AssignLocalUserToLocalGroupResponse),
    RemoteEasyconnectDiscovery(RemoteEasyconnectDiscoveryResponse),
    RemoteEasyconnectCreatePairing(RemoteEasyconnectCreatePairingResponse),
    RemoteEasyconnectApprovePairing(RemoteEasyconnectApprovePairingResponse),
    RemoteEasyconnectExchangePairing(RemoteEasyconnectExchangePairingResponse),
    RemoteEasyconnectRevokeSession(RemoteEasyconnectRevokeSessionResponse),
    RemoteEasyconnectRenewSession(RemoteEasyconnectRenewSessionResponse),
    RemoteEasyconnectUploadAdmission(RemoteEasyconnectUploadAdmissionDecision),
    RemoteEasyconnectSubmitAwsCliUpload(RemoteEasyconnectSubmitAwsCliUploadResponse),
    ApplicationUploadCapabilityIssued(ApplicationUploadCapabilityIssueResponse),
    ApplicationUploadCompleted(ApplicationUploadCompletionResponse),
    IngestProgress(DaemonIngestProgressEvent),
    Error(DaemonApiErrorResponse),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonApiErrorResponse {
    pub code: String,
    pub message: String,
}

impl DaemonApiErrorResponse {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ApplianceTelemetryRequest, ApplianceTelemetryWindow, ApplicationAccessTokenExchangeRequest,
        AssignLocalUserToLocalGroupRequest, CapacityAdmissionRequest, CreateLocalGroupRequest,
        CreateObjectStoreRequest, DaemonApiRequest, DaemonEndpointKind, DaemonEndpointValidation,
        DaemonEndpointValidationState, DaemonIngestConflictPolicy, DaemonIngestControlAction,
        DaemonIngressOrigin, DaemonJobCancelRequest, DaemonJobId, DaemonJobListRequest,
        DaemonJobStatusRequest, DaemonServiceLifecycleRequest, DaemonServiceOperation,
        DaemonServiceProvisionRequest, DaemonServiceStatusRequest, DaemonSsdPressure,
        IngestControlRequest, ObjectBrowserPageRequest, ObjectBrowserRequest, ObjectBrowserSort,
        ObjectDownloadRequest, ObjectFolderDownloadRequest, PrepareEnclosureFilesystem,
        PrepareEnclosureHddDevice, PrepareEnclosureRequest, ProfileBrowserRequest,
        ProfileS3ListRequest, RemoteEasyconnectAuthProvider,
        RemoteEasyconnectAwsCliEnvironmentVariable, RemoteEasyconnectCreatePairingRequest,
        RemoteEasyconnectExchangePairingRequest, RemoteEasyconnectObjectStoreGrant,
        RemoteEasyconnectRenewSessionRequest, RemoteEasyconnectRevokeSessionRequest,
        RemoteEasyconnectSubmitAwsCliUploadRequest, RemoteEasyconnectUploadAdmissionRequest,
        StoreInventoryRequest, SubmitIngestFilesRequest, UpsertEndpointInventoryRequest,
        ENCLOSURE_PREPARE_CONFIRMATION, ENDPOINT_RECORD_CONFIRMATION, INGEST_CONTROL_CONFIRMATION,
        OBJECT_STORE_CREATE_CONFIRMATION, REMOTE_EASYCONNECT_PAIRING_EXCHANGE_ROUTE,
    };
    use crate::api::DiskLockdownRequest;
    use dasobjectstore_core::ids::{ObjectId, StoreId};
    use dasobjectstore_core::remote_upload::RemoteUploadBackpressurePolicy;
    use dasobjectstore_object_service::ObjectServiceProviderId;

    #[test]
    fn serializes_request_with_stable_command_name() {
        let request = DaemonApiRequest::StoreInventory(StoreInventoryRequest::default());

        let encoded = serde_json::to_value(request).expect("request serializes");

        assert_eq!(encoded["command"], "store_inventory");
    }

    #[test]
    fn disk_lockdown_command_uses_stable_shape() {
        let request = DaemonApiRequest::DiskLockdown(DiskLockdownRequest {
            mount_root: "/srv/das".into(),
            service_user: "dasobjectstore".to_string(),
            service_group: "dasobjectstore".to_string(),
            create_service_user: true,
            dry_run: true,
            confirmation_marker: String::new(),
        });

        request.validate().expect("dry-run request validates");
        let encoded = serde_json::to_value(request).expect("request serializes");
        assert_eq!(encoded["command"], "disk_lockdown");
        assert_eq!(encoded["payload"]["mount_root"], "/srv/das");
    }

    #[test]
    fn capacity_admission_command_uses_stable_shape() {
        let request = DaemonApiRequest::CapacityAdmission(CapacityAdmissionRequest {
            store_id: "codex".to_string(),
            requested_bytes: 4096,
            copy_count: 2,
            ingress_origin: DaemonIngressOrigin::RemoteS3,
            client_request_id: Some("request-1".to_string()),
        });

        request.validate().expect("capacity request validates");
        let encoded = serde_json::to_value(request).expect("request serializes");

        assert_eq!(encoded["command"], "capacity_admission");
        assert_eq!(encoded["payload"]["store_id"], "codex");
        assert_eq!(encoded["payload"]["ingress_origin"], "remote_s3");
    }

    #[test]
    fn ingest_control_command_uses_stable_shape() {
        let request = DaemonApiRequest::IngestControl(IngestControlRequest {
            action: DaemonIngestControlAction::Pause,
            reason: "protect Web availability".to_string(),
            dry_run: false,
            confirmation_marker: INGEST_CONTROL_CONFIRMATION.to_string(),
        });
        request.validate().expect("control request validates");
        let encoded = serde_json::to_value(request).expect("request serializes");
        assert_eq!(encoded["command"], "ingest_control");
        assert_eq!(encoded["payload"]["action"], "pause");
    }

    #[test]
    fn delegates_submit_ingest_validation() {
        let request = DaemonApiRequest::SubmitIngestFiles(SubmitIngestFilesRequest {
            endpoint: StoreId::new("zymo").expect("store id"),
            source_path: "relative".into(),
            object_type: dasobjectstore_core::object_type::ObjectType::Naive,
            copies: None,
            hdd_workers: None,
            ingress_origin: DaemonIngressOrigin::LocalServer,
            conflict_policy: DaemonIngestConflictPolicy::Strict,
            dry_run: false,
            client_request_id: None,
        });

        assert!(request.validate().is_err());
    }

    #[test]
    fn service_commands_use_stable_command_names() {
        let status = DaemonApiRequest::ServiceStatus(DaemonServiceStatusRequest {
            include_detail: true,
        });
        let lifecycle = DaemonApiRequest::ServiceLifecycle(DaemonServiceLifecycleRequest {
            operation: DaemonServiceOperation::Start,
            provider_id: ObjectServiceProviderId::Garage,
            dry_run: true,
            client_request_id: None,
        });
        let provision = DaemonApiRequest::ServiceProvision(DaemonServiceProvisionRequest {
            provider_id: ObjectServiceProviderId::Garage,
            dry_run: true,
            rotate_credentials: false,
            client_request_id: None,
        });

        let status = serde_json::to_value(status).expect("status request serializes");
        let lifecycle = serde_json::to_value(lifecycle).expect("lifecycle request serializes");
        let provision = serde_json::to_value(provision).expect("provision request serializes");

        assert_eq!(status["command"], "service_status");
        assert_eq!(lifecycle["command"], "service_lifecycle");
        assert_eq!(lifecycle["payload"]["operation"], "start");
        assert_eq!(provision["command"], "service_provision");
    }

    #[test]
    fn appliance_telemetry_command_uses_stable_command_name() {
        let request = DaemonApiRequest::ApplianceTelemetry(ApplianceTelemetryRequest {
            window: ApplianceTelemetryWindow::TenDays,
        });

        request.validate().expect("request validates");
        let encoded = serde_json::to_value(request).expect("request serializes");

        assert_eq!(encoded["command"], "appliance_telemetry");
        assert_eq!(encoded["payload"]["window"], "ten_days");
    }

    #[test]
    fn generic_job_commands_use_stable_command_names() {
        let job_id = DaemonJobId::new("enclosure-prepare-1").expect("job id");
        let list = DaemonApiRequest::JobList(DaemonJobListRequest { limit: Some(25) });
        let status = DaemonApiRequest::JobStatus(DaemonJobStatusRequest {
            job_id: job_id.clone(),
        });
        let cancel = DaemonApiRequest::CancelJob(DaemonJobCancelRequest {
            job_id,
            reason: Some("operator requested cancellation".to_string()),
        });

        let list = serde_json::to_value(list).expect("list request serializes");
        let status = serde_json::to_value(status).expect("status request serializes");
        let cancel = serde_json::to_value(cancel).expect("cancel request serializes");

        assert_eq!(list["command"], "job_list");
        assert_eq!(list["payload"]["limit"], 25);
        assert_eq!(status["command"], "job_status");
        assert_eq!(cancel["command"], "cancel_job");
        assert_eq!(
            cancel["payload"]["reason"],
            "operator requested cancellation"
        );
    }

    #[test]
    fn local_admin_commands_use_stable_command_names() {
        let create = DaemonApiRequest::CreateLocalGroup(CreateLocalGroupRequest {
            group_name: "mnemosyne".to_string(),
            dry_run: true,
            client_request_id: Some("request-1".to_string()),
            administrator_actor: Some("operator".to_string()),
            confirmation_marker: "confirm create local group".to_string(),
        });
        let assign =
            DaemonApiRequest::AssignLocalUserToLocalGroup(AssignLocalUserToLocalGroupRequest {
                username: "stephen".to_string(),
                group_name: "mnemosyne".to_string(),
                dry_run: true,
                client_request_id: Some("request-2".to_string()),
                administrator_actor: Some("operator".to_string()),
                confirmation_marker: "confirm assign local user".to_string(),
            });

        let create = serde_json::to_value(create).expect("create request serializes");
        let assign = serde_json::to_value(assign).expect("assignment request serializes");

        assert_eq!(create["command"], "create_local_group");
        assert_eq!(assign["command"], "assign_local_user_to_local_group");
    }

    #[test]
    fn prepare_enclosure_command_uses_stable_command_name() {
        let request = DaemonApiRequest::PrepareEnclosure(PrepareEnclosureRequest {
            ssd_device: "/dev/disk/by-id/nvme-ssd".into(),
            hdd_devices: vec![PrepareEnclosureHddDevice {
                disk_id: "qnap-1057".to_string(),
                device_path: "/dev/disk/by-id/usb-qnap-1057".into(),
            }],
            mount_root: "/srv/dasobjectstore".into(),
            filesystem: PrepareEnclosureFilesystem::Ext4,
            owner: Some("stephen".to_string()),
            dry_run: false,
            client_request_id: Some("request-1".to_string()),
            administrator_actor: Some("operator".to_string()),
            allow_format: true,
            existing_data_acknowledged: true,
            confirmation_marker: ENCLOSURE_PREPARE_CONFIRMATION.to_string(),
        });

        let encoded = serde_json::to_value(request).expect("request serializes");

        assert_eq!(encoded["command"], "prepare_enclosure");
        assert_eq!(encoded["payload"]["filesystem"], "ext4");
        assert_eq!(encoded["payload"]["hdd_devices"][0]["disk_id"], "qnap-1057");
    }

    #[test]
    fn create_object_store_command_uses_stable_command_name() {
        let request = DaemonApiRequest::CreateObjectStore(CreateObjectStoreRequest {
            store_id: "generated-data".to_string(),
            store_class: "generated_data".to_string(),
            required_copies: 2,
            bucket: Some("generated-data".to_string()),
            reader_group: None,
            writer_group: "bioinformatics".to_string(),
            ssd_root: "/srv/dasobjectstore/ssd".into(),
            object_type: "pod5".to_string(),
            enclosure_id: Some("qnap-tl-d800c-01".to_string()),
            public: false,
            writeable: true,
            capacity_behavior: "balanced".to_string(),
            retention: "standard".to_string(),
            endpoint_export_mode: "s3_bucket".to_string(),
            dry_run: false,
            client_request_id: Some("request-1".to_string()),
            administrator_actor: Some("stephen".to_string()),
            confirmation_marker: OBJECT_STORE_CREATE_CONFIRMATION.to_string(),
        });

        let encoded = serde_json::to_value(request).expect("request serializes");

        assert_eq!(encoded["command"], "create_object_store");
        assert_eq!(encoded["payload"]["store_id"], "generated-data");
        assert_eq!(encoded["payload"]["required_copies"], 2);
    }

    #[test]
    fn object_browser_command_uses_stable_command_name() {
        let request = DaemonApiRequest::ObjectBrowser(ObjectBrowserRequest {
            endpoint: StoreId::new("ena").expect("store id"),
            prefix: Some("ENA/Xenognostikon".to_string()),
            search: None,
            sort: ObjectBrowserSort::NameAsc,
            page: ObjectBrowserPageRequest::default(),
            include_placement: true,
            delegated_actor: None,
        });

        request.validate().expect("request validates");
        let encoded = serde_json::to_value(request).expect("request serializes");

        assert_eq!(encoded["command"], "object_browser");
        assert_eq!(encoded["payload"]["prefix"], "ENA/Xenognostikon");
        assert_eq!(encoded["payload"]["page"]["limit"], 100);
    }

    #[test]
    fn profile_browser_command_uses_stable_command_name() {
        let request = DaemonApiRequest::ProfileBrowser(ProfileBrowserRequest {
            store_id: StoreId::new("codex").expect("store id"),
            prefix: Some("reads".to_string()),
            search: None,
            offset: 0,
            limit: 100,
            delegated_actor: None,
        });
        request.validate().expect("request validates");
        let encoded = serde_json::to_value(request).expect("request serializes");
        assert_eq!(encoded["command"], "profile_browser");
        assert_eq!(encoded["payload"]["limit"], 100);
    }

    #[test]
    fn profile_s3_list_command_uses_stable_command_name() {
        let request = DaemonApiRequest::ProfileS3List(ProfileS3ListRequest {
            store_id: StoreId::new("codex").expect("store id"),
            prefix: Some("reads".to_string()),
            offset: 0,
            limit: 100,
        });
        request.validate().expect("request validates");
        let encoded = serde_json::to_value(request).expect("request serializes");
        assert_eq!(encoded["command"], "profile_s3_list");
        assert_eq!(encoded["payload"]["limit"], 100);
    }

    #[test]
    fn application_access_token_exchange_uses_stable_command_name() {
        let request = DaemonApiRequest::ExchangeApplicationAccessToken(
            ApplicationAccessTokenExchangeRequest {
                exchange: dasobjectstore_core::application_auth::AccessTokenExchangeRequest {
                    schema_version:
                        dasobjectstore_core::application_auth::APPLICATION_AUTH_SCHEMA_VERSION
                            .to_string(),
                    application_id: "synoptikon".to_string(),
                    key_id: "key-1".to_string(),
                    audience: "dasobjectstore".to_string(),
                    requested_issued_at_unix_seconds: 10,
                    requested_expires_at_unix_seconds: 20,
                    scope: dasobjectstore_core::application_auth::ApplicationScope {
                        store_ids: vec![StoreId::new("codex").expect("store id")],
                        prefixes: Vec::new(),
                        object_types: Vec::new(),
                        operations: vec![
                            dasobjectstore_core::application_auth::ApplicationOperation::Read,
                        ],
                        ingress_origin: dasobjectstore_core::ingress::IngressOrigin::Synoptikon,
                        max_object_bytes: Some(10),
                        max_total_bytes: Some(100),
                    },
                    correlation_id: None,
                    governed_binding: None,
                    proof: "proof".to_string(),
                },
            },
        );
        request.validate().expect("exchange request validates");
        let encoded = serde_json::to_value(request).expect("request serializes");
        assert_eq!(encoded["command"], "exchange_application_access_token");
        assert_eq!(
            encoded["payload"]["exchange"]["application_id"],
            "synoptikon"
        );
    }

    #[test]
    fn object_download_command_uses_stable_command_name() {
        let request = DaemonApiRequest::ObjectDownload(ObjectDownloadRequest {
            endpoint: StoreId::new("ena").expect("store id"),
            object_id: ObjectId::new("ena/raw/metadata.tsv").expect("object id"),
            delegated_actor: None,
        });

        request.validate().expect("request validates");
        let encoded = serde_json::to_value(request).expect("request serializes");

        assert_eq!(encoded["command"], "object_download");
        assert_eq!(encoded["payload"]["object_id"], "ena/raw/metadata.tsv");
    }

    #[test]
    fn object_folder_download_command_uses_stable_command_name() {
        let request = DaemonApiRequest::ObjectFolderDownload(ObjectFolderDownloadRequest {
            endpoint: StoreId::new("ena").expect("store id"),
            prefix: "ena/raw".to_string(),
            delegated_actor: None,
        });

        request.validate().expect("request validates");
        let encoded = serde_json::to_value(request).expect("request serializes");

        assert_eq!(encoded["command"], "object_folder_download");
        assert_eq!(encoded["payload"]["prefix"], "ena/raw");
    }

    #[test]
    fn endpoint_inventory_upsert_command_uses_stable_command_name() {
        let request = DaemonApiRequest::UpsertEndpointInventory(UpsertEndpointInventoryRequest {
            endpoint_id: "nas-staging".to_string(),
            display_name: "NAS staging".to_string(),
            kind: DaemonEndpointKind::DasobjectstoreNfs,
            object_service_url: "https://nas.example.test:9443".to_string(),
            validation: DaemonEndpointValidation {
                state: DaemonEndpointValidationState::Validated,
                checked_at_utc: Some("2026-07-09T00:00:00Z".to_string()),
                message: None,
            },
            manager_product_id: "dasobjectstore".to_string(),
            active_bindings: Vec::new(),
            dry_run: false,
            client_request_id: Some("request-1".to_string()),
            administrator_actor: Some("stephen".to_string()),
            confirmation_marker: Some(ENDPOINT_RECORD_CONFIRMATION.to_string()),
        });

        let encoded = serde_json::to_value(request).expect("request serializes");

        assert_eq!(encoded["command"], "upsert_endpoint_inventory");
        assert_eq!(encoded["payload"]["endpoint_id"], "nas-staging");
        assert_eq!(encoded["payload"]["kind"], "dasobjectstore_nfs");
    }

    #[test]
    fn remote_easyconnect_commands_use_stable_command_names() {
        let discovery = DaemonApiRequest::RemoteEasyconnectDiscovery(Default::default());
        let create =
            DaemonApiRequest::RemoteEasyconnectCreatePairing(create_easyconnect_pairing_request());
        let exchange = DaemonApiRequest::RemoteEasyconnectExchangePairing(
            RemoteEasyconnectExchangePairingRequest {
                pairing_id: "pair-1".to_string(),
                exchange_code: "exchange-code".to_string(),
                client_request_id: Some("request-2".to_string()),
            },
        );
        let revoke = DaemonApiRequest::RemoteEasyconnectRevokeSession(
            RemoteEasyconnectRevokeSessionRequest {
                session_id: "session-1".to_string(),
                reason: Some("operator requested revocation".to_string()),
            },
        );
        let renew =
            DaemonApiRequest::RemoteEasyconnectRenewSession(RemoteEasyconnectRenewSessionRequest {
                session_id: "session-1".to_string(),
                renewal_token: "renewal-token".to_string(),
                requested_lifetime_seconds: Some(28_800),
            });
        let admission = DaemonApiRequest::RemoteEasyconnectUploadAdmission(
            RemoteEasyconnectUploadAdmissionRequest {
                policy: RemoteUploadBackpressurePolicy::default(),
                ssd_pressure: DaemonSsdPressure::AcceptingWrites,
                active_s3_transfers: 0,
                ssd_stage_queue_depth: 0,
                hdd_landing_queue_depth: 0,
                verification_queue_depth: 0,
            },
        );
        let submit_upload = DaemonApiRequest::RemoteEasyconnectSubmitAwsCliUpload(
            RemoteEasyconnectSubmitAwsCliUploadRequest {
                job_id: "remote-upload-job-1".to_string(),
                object_store: "zymo_fecal_2025.05".to_string(),
                source_bytes: 42,
                policy: RemoteUploadBackpressurePolicy::default(),
                ssd_pressure: DaemonSsdPressure::AcceptingWrites,
                program: "aws".to_string(),
                args: vec!["s3".to_string(), "cp".to_string()],
                display_args: vec!["s3".to_string(), "cp".to_string()],
                environment: vec![RemoteEasyconnectAwsCliEnvironmentVariable {
                    name: "AWS_ACCESS_KEY_ID".to_string(),
                    value: "AKIAEXAMPLE".to_string(),
                }],
                progress_telemetry: None,
                progress_message: None,
                completion: None,
            },
        );

        let discovery = serde_json::to_value(discovery).expect("discovery serializes");
        let create = serde_json::to_value(create).expect("create serializes");
        let exchange = serde_json::to_value(exchange).expect("exchange serializes");
        let revoke = serde_json::to_value(revoke).expect("revoke serializes");
        let renew = serde_json::to_value(renew).expect("renew serializes");
        let admission = serde_json::to_value(admission).expect("admission serializes");
        let submit_upload = serde_json::to_value(submit_upload).expect("submit serializes");

        assert_eq!(discovery["command"], "remote_easyconnect_discovery");
        assert_eq!(create["command"], "remote_easyconnect_create_pairing");
        assert_eq!(
            create["payload"]["callback_url"],
            "http://127.0.0.1:49321/callback"
        );
        assert_eq!(exchange["command"], "remote_easyconnect_exchange_pairing");
        assert_eq!(revoke["command"], "remote_easyconnect_revoke_session");
        assert_eq!(renew["command"], "remote_easyconnect_renew_session");
        assert_eq!(admission["command"], "remote_easyconnect_upload_admission");
        assert_eq!(admission["payload"]["ssd_pressure"], "accepting_writes");
        assert_eq!(
            submit_upload["command"],
            "remote_easyconnect_submit_aws_cli_upload"
        );
        assert_eq!(
            REMOTE_EASYCONNECT_PAIRING_EXCHANGE_ROUTE,
            "/api/v1/remote/easyconnect/pairings/exchange"
        );
    }

    #[test]
    fn remote_easyconnect_validation_rejects_bad_pairing_contract() {
        let request = DaemonApiRequest::RemoteEasyconnectCreatePairing(
            RemoteEasyconnectCreatePairingRequest {
                client_name: "macbook".to_string(),
                callback_url: "127.0.0.1:49321/callback".to_string(),
                requested_object_store: None,
                requested_session_lifetime_seconds: Some(1),
                client_request_id: None,
            },
        );

        let err = request.validate().expect_err("invalid callback rejected");

        assert!(matches!(
            err,
            crate::api::DaemonRequestValidationError::UnsupportedFieldValue {
                field: "callback_url",
                ..
            }
        ));
    }

    #[test]
    fn remote_easyconnect_session_commands_validate_revocation_and_renewal_contracts() {
        let revoke = DaemonApiRequest::RemoteEasyconnectRevokeSession(
            RemoteEasyconnectRevokeSessionRequest {
                session_id: " ".to_string(),
                reason: Some("operator requested revocation".to_string()),
            },
        );
        let renew =
            DaemonApiRequest::RemoteEasyconnectRenewSession(RemoteEasyconnectRenewSessionRequest {
                session_id: "session-1".to_string(),
                renewal_token: "renewal-token".to_string(),
                requested_lifetime_seconds: Some(59),
            });

        assert!(matches!(
            revoke.validate().expect_err("blank session id rejected"),
            crate::api::DaemonRequestValidationError::BlankField {
                field: "session_id"
            }
        ));
        assert!(matches!(
            renew.validate().expect_err("short renewal lifetime rejected"),
            crate::api::DaemonRequestValidationError::UnsupportedFieldValue {
                field: "requested_session_lifetime_seconds",
                value
            } if value == "59"
        ));
    }

    #[test]
    fn remote_easyconnect_approval_validates_object_store_grants() {
        let request = DaemonApiRequest::RemoteEasyconnectApprovePairing(
            crate::api::RemoteEasyconnectApprovePairingRequest {
                pairing_id: "pair-1".to_string(),
                approved_actor: "stephen".to_string(),
                auth_provider: RemoteEasyconnectAuthProvider::StandaloneLocalUser,
                allowed_object_stores: vec![RemoteEasyconnectObjectStoreGrant {
                    object_store: "zymo_fecal_2025.05".to_string(),
                    bucket: "dos-zymo-fecal-2025-05".to_string(),
                    can_read: true,
                    can_write: true,
                    writer_group: Some("mnemosyne".to_string()),
                    object_type: "fastq".to_string(),
                }],
                approval_expires_at_utc: "2026-07-09T12:10:00Z".to_string(),
            },
        );

        request.validate().expect("approval request validates");
        let encoded = serde_json::to_value(request).expect("approval serializes");

        assert_eq!(encoded["command"], "remote_easyconnect_approve_pairing");
        assert_eq!(encoded["payload"]["auth_provider"], "standalone_local_user");
    }

    fn create_easyconnect_pairing_request() -> RemoteEasyconnectCreatePairingRequest {
        RemoteEasyconnectCreatePairingRequest {
            client_name: "macbook".to_string(),
            callback_url: "http://127.0.0.1:49321/callback".to_string(),
            requested_object_store: Some("zymo_fecal_2025.05".to_string()),
            requested_session_lifetime_seconds: Some(28_800),
            client_request_id: Some("request-1".to_string()),
        }
    }

    #[test]
    fn delegates_service_lifecycle_validation() {
        let request = DaemonApiRequest::ServiceLifecycle(DaemonServiceLifecycleRequest {
            operation: DaemonServiceOperation::Start,
            provider_id: ObjectServiceProviderId::Rustfs,
            dry_run: false,
            client_request_id: None,
        });

        assert!(request.validate().is_err());
    }

    #[test]
    fn delegates_service_provision_validation() {
        let request = DaemonApiRequest::ServiceProvision(DaemonServiceProvisionRequest {
            provider_id: ObjectServiceProviderId::Rustfs,
            dry_run: false,
            rotate_credentials: false,
            client_request_id: None,
        });

        assert!(request.validate().is_err());
    }

    #[test]
    fn delegates_local_admin_validation() {
        let request = DaemonApiRequest::CreateLocalGroup(CreateLocalGroupRequest {
            group_name: "Invalid Group".to_string(),
            dry_run: true,
            client_request_id: None,
            administrator_actor: None,
            confirmation_marker: "confirm create local group".to_string(),
        });

        assert!(request.validate().is_err());
    }

    #[test]
    fn delegates_prepare_enclosure_validation() {
        let request = DaemonApiRequest::PrepareEnclosure(PrepareEnclosureRequest {
            ssd_device: "relative".into(),
            hdd_devices: Vec::new(),
            mount_root: "/srv/dasobjectstore".into(),
            filesystem: PrepareEnclosureFilesystem::Ext4,
            owner: None,
            dry_run: false,
            client_request_id: None,
            administrator_actor: None,
            allow_format: false,
            existing_data_acknowledged: false,
            confirmation_marker: "wrong".to_string(),
        });

        assert!(request.validate().is_err());
    }
}
