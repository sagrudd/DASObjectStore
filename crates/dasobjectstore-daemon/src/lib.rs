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
    DaemonApiWarning, DaemonDiskHealthSummary, DaemonHealthSummaryRequest,
    DaemonHealthSummaryResponse, DaemonIngestAdaptiveSchedulerInput,
    DaemonIngestAdaptiveSchedulingLimit, DaemonIngestAdaptiveWorkerSchedule,
    DaemonIngestBottleneck, DaemonIngestBoundedBufferPolicy, DaemonIngestBufferPoolPolicySet,
    DaemonIngestCompletionFraction, DaemonIngestConflictAction, DaemonIngestConflictDecision,
    DaemonIngestConflictPolicy, DaemonIngestConflictReason, DaemonIngestErrorRate,
    DaemonIngestHddQueueState, DaemonIngestHddTargetQueue, DaemonIngestObjectSnapshot,
    DaemonIngestPipelinePressure, DaemonIngestPipelineStage, DaemonIngestPlacementSchedulerInput,
    DaemonIngestPressure, DaemonIngestProgressEvent, DaemonIngestProgressFractions,
    DaemonIngestQueueDepths, DaemonIngestResourcePolicy, DaemonIngestSchedulingPolicy,
    DaemonIngestStage, DaemonIngestSummary, DaemonIngestSystemSafetyReserve,
    DaemonIngestSystemTelemetry, DaemonIngestTargetCapacity, DaemonIngestTargetFailureState,
    DaemonIngestTelemetry, DaemonIngestThroughputTelemetry, DaemonIngestThroughputTrend,
    DaemonIngestWorkerActivity, DaemonIngestWorkerCounts, DaemonIngestWorkerTelemetry,
    DaemonJobAcceptedResponse, DaemonJobCancelRequest, DaemonJobCancelResponse, DaemonJobEvent,
    DaemonJobId, DaemonJobIdError, DaemonJobKind, DaemonJobProgress, DaemonJobState,
    DaemonJobStatusRequest, DaemonJobStatusResponse, DaemonJobSummary, DaemonJobValidationError,
    DaemonLocalAdminAcceptedResponse, DaemonLocalAdminCommand, DaemonLocalAdminValidationError,
    DaemonRequestValidationError, DaemonServiceLifecycleRequest, DaemonServiceLifecycleResponse,
    DaemonServiceOperation, DaemonServiceProvisionRequest, DaemonServiceProvisionResponse,
    DaemonServiceStatusDetail, DaemonServiceStatusRequest, DaemonServiceStatusResponse,
    DaemonSourceReadBackpressureAction, DaemonSourceReadBackpressureDecision,
    DaemonSourceReadBackpressureInput, DaemonSourceReadBackpressurePolicy,
    DaemonSourceReadBackpressureReason, DaemonSourceReadPriority, DaemonSourceToSsdPriorityPolicy,
    DaemonSourceToSsdQueueUsage, DaemonSsdPressure, IngestJobStatusRequest,
    IngestJobStatusResponse, PrepareEnclosureFilesystem, PrepareEnclosureHddDevice,
    PrepareEnclosureRequest, PrepareEnclosureResponse, PrepareEnclosureValidationError,
    StoreInventoryItem, StoreInventoryRequest, StoreInventoryResponse, SubmitIngestFilesRequest,
    SubmitIngestFilesResponse, ENCLOSURE_PREPARE_CONFIRMATION, OBJECT_STORE_CREATE_CONFIRMATION,
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
    provision_garage_store_registry, read_authoritative_ingest_policy, AdminJobRegistry,
    AuthoritativeIngestPolicy, AuthoritativePerformancePolicyError, DaemonRuntimeConfig,
    DaemonRuntimeConfigError, DaemonServiceRuntimeError, FileBackedAdminJobRegistry,
    GarageProvisioningSummary, GarageServiceController, GarageServiceRuntimeConfig,
    GarageStoreRegistryProvisioningSummary, ServiceCommandOutput, ServiceCommandRunner,
    SystemServiceCommandRunner, ADMIN_JOB_REGISTRY_DIR_NAME, ADMIN_JOB_REGISTRY_FILE_NAME,
    ADMIN_JOB_REGISTRY_SCHEMA, AUTHORITATIVE_PERFORMANCE_DIR_NAME,
    AUTHORITATIVE_PERFORMANCE_RECOMMENDATION_FILE_NAME, DEFAULT_DAEMON_CONFIG_PATH,
    DEFAULT_DAEMON_GROUP, DEFAULT_DAEMON_LOG_DIR, DEFAULT_DAEMON_RUNTIME_DIR,
    DEFAULT_DAEMON_SERVICE_USER, DEFAULT_DAEMON_SOCKET_FILE_NAME, DEFAULT_DAEMON_STATE_DIR,
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
