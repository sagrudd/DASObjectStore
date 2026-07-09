//! Managed daemon boundary for DASObjectStore.

pub mod api;
pub mod auth;
pub mod client;
pub mod runtime;
pub mod server;

pub use api::{
    AssignLocalUserToLocalGroupRequest, AssignLocalUserToLocalGroupResponse,
    CancelIngestJobRequest, CancelIngestJobResponse, CreateLocalGroupRequest,
    CreateLocalGroupResponse, CreateObjectStoreRequest, CreateObjectStoreResponse,
    CreateObjectStoreValidationError, DaemonApiErrorResponse, DaemonApiRequest, DaemonApiResponse,
    DaemonApiWarning, DaemonDiskHealthSummary, DaemonEndpointBinding,
    DaemonEndpointBindingReadiness, DaemonEndpointKind, DaemonEndpointValidation,
    DaemonEndpointValidationState, DaemonHealthSummaryRequest, DaemonHealthSummaryResponse,
    DaemonIngestAdaptiveSchedulerInput, DaemonIngestAdaptiveSchedulingLimit,
    DaemonIngestAdaptiveWorkerSchedule, DaemonIngestBottleneck, DaemonIngestBoundedBufferPolicy,
    DaemonIngestBufferPoolPolicySet, DaemonIngestCompletionFraction, DaemonIngestConflictAction,
    DaemonIngestConflictDecision, DaemonIngestConflictPolicy, DaemonIngestConflictReason,
    DaemonIngestErrorRate, DaemonIngestHddQueueState, DaemonIngestHddTargetQueue,
    DaemonIngestObjectSnapshot, DaemonIngestPipelinePressure, DaemonIngestPipelineStage,
    DaemonIngestPlacementSchedulerInput, DaemonIngestPressure, DaemonIngestProgressEvent,
    DaemonIngestProgressFractions, DaemonIngestQueueDepths, DaemonIngestResourcePolicy,
    DaemonIngestSchedulingPolicy, DaemonIngestStage, DaemonIngestSummary,
    DaemonIngestSystemSafetyReserve, DaemonIngestSystemTelemetry, DaemonIngestTargetCapacity,
    DaemonIngestTargetFailureState, DaemonIngestTelemetry, DaemonIngestThroughputTelemetry,
    DaemonIngestThroughputTrend, DaemonIngestWorkerActivity, DaemonIngestWorkerCounts,
    DaemonIngestWorkerTelemetry, DaemonJobAcceptedResponse, DaemonJobCancelRequest,
    DaemonJobCancelResponse, DaemonJobEvent, DaemonJobId, DaemonJobIdError, DaemonJobKind,
    DaemonJobListRequest, DaemonJobListResponse, DaemonJobProgress, DaemonJobState,
    DaemonJobStatusRequest, DaemonJobStatusResponse, DaemonJobSummary, DaemonJobValidationError,
    DaemonLocalAdminAcceptedResponse, DaemonLocalAdminCommand, DaemonLocalAdminValidationError,
    DaemonRequestValidationError, DaemonServiceLifecycleRequest, DaemonServiceLifecycleResponse,
    DaemonServiceOperation, DaemonServiceProvisionRequest, DaemonServiceProvisionResponse,
    DaemonServiceStatusDetail, DaemonServiceStatusRequest, DaemonServiceStatusResponse,
    DaemonSourceReadBackpressureAction, DaemonSourceReadBackpressureDecision,
    DaemonSourceReadBackpressureInput, DaemonSourceReadBackpressurePolicy,
    DaemonSourceReadBackpressureReason, DaemonSourceReadPriority, DaemonSourceToSsdPriorityPolicy,
    DaemonSourceToSsdQueueUsage, DaemonSsdPressure, EndpointInventoryValidationError,
    IngestJobStatusRequest, IngestJobStatusResponse, ObjectBrowserBreadcrumb,
    ObjectBrowserChecksum, ObjectBrowserFileNode, ObjectBrowserFolderNode,
    ObjectBrowserPageRequest, ObjectBrowserPlacement, ObjectBrowserPlacementLocation,
    ObjectBrowserPlacementState, ObjectBrowserReadinessState, ObjectBrowserRequest,
    ObjectBrowserResponse, ObjectBrowserSort, PrepareEnclosureFilesystem,
    PrepareEnclosureHddDevice, PrepareEnclosureRequest, PrepareEnclosureResponse,
    PrepareEnclosureValidationError, StoreInventoryItem, StoreInventoryRequest,
    StoreInventoryResponse, SubmitIngestFilesRequest, SubmitIngestFilesResponse,
    UpsertEndpointInventoryRequest, UpsertEndpointInventoryResponse,
    ENCLOSURE_PREPARE_CONFIRMATION, ENDPOINT_RECORD_CONFIRMATION, OBJECT_BROWSER_MAX_PAGE_LIMIT,
    OBJECT_STORE_CREATE_CONFIRMATION,
};
pub use auth::{
    authorize_store_write, DaemonAuthorizationError, DaemonLocalActor, DaemonStoreAccessPolicy,
};
pub use client::{
    DaemonClient, DaemonClientError, DaemonClientTransport, InProcessDaemonTransport,
    UnixSocketDaemonTransport,
};
pub use runtime::{
    admin_job_registry_path, authoritative_performance_recommendation_path,
    default_endpoint_registry_path, provision_garage_store_registry, query_object_browser_metadata,
    read_authoritative_ingest_policy, read_object_browser_metadata,
    upsert_endpoint_inventory_record, AdminJobRegistry, AuthoritativeIngestPolicy,
    AuthoritativePerformancePolicyError, DaemonRuntimeConfig, DaemonRuntimeConfigError,
    DaemonServiceRuntimeError, EndpointRegistryUpsertSummary, FileBackedAdminJobRegistry,
    GarageProvisioningSummary, GarageServiceController, GarageServiceRuntimeConfig,
    GarageStoreRegistryProvisioningSummary, ObjectBrowserMetadataEntry,
    ObjectBrowserMetadataReadError, ObjectBrowserQueryError, ServiceCommandOutput,
    ServiceCommandRunner, SystemServiceCommandRunner, ADMIN_JOB_REGISTRY_DIR_NAME,
    ADMIN_JOB_REGISTRY_FILE_NAME, ADMIN_JOB_REGISTRY_SCHEMA, AUTHORITATIVE_PERFORMANCE_DIR_NAME,
    AUTHORITATIVE_PERFORMANCE_RECOMMENDATION_FILE_NAME, DEFAULT_DAEMON_CONFIG_PATH,
    DEFAULT_DAEMON_GROUP, DEFAULT_DAEMON_LOG_DIR, DEFAULT_DAEMON_RUNTIME_DIR,
    DEFAULT_DAEMON_SERVICE_USER, DEFAULT_DAEMON_SOCKET_FILE_NAME, DEFAULT_DAEMON_STATE_DIR,
    DEFAULT_ENDPOINT_REGISTRY_PATH, ENDPOINT_REGISTRY_ENV, ENDPOINT_REGISTRY_SCHEMA,
    LINUX_DAEMON_CONFIG_PATH, LINUX_DAEMON_LOG_DIR, LINUX_DAEMON_RUNTIME_DIR,
    LINUX_DAEMON_STATE_DIR, PERFORMANCE_RECOMMENDATION_SCHEMA,
};
pub use server::{
    DaemonApiHandler, DaemonClock, DaemonRequestHandler, DaemonRequestHandlerError,
    DaemonServiceOrchestrator, FixedDaemonClock, SystemDaemonClock, UnixSocketDaemonServer,
    UnixSocketDaemonServerError,
};

/// Returns the daemon crate version.
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
