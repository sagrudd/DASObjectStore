//! Managed daemon boundary for DASObjectStore.

pub mod api;
pub mod auth;
pub mod client;
pub mod runtime;
pub mod server;

pub use api::{
    AssignLocalUserToLocalGroupRequest, AssignLocalUserToLocalGroupResponse,
    CancelIngestJobRequest, CancelIngestJobResponse, CreateLocalGroupRequest,
    CreateLocalGroupResponse, DaemonApiErrorResponse, DaemonApiRequest, DaemonApiResponse,
    DaemonApiWarning, DaemonDiskHealthSummary, DaemonHealthSummaryRequest,
    DaemonHealthSummaryResponse, DaemonIngestBottleneck, DaemonIngestBoundedBufferPolicy,
    DaemonIngestBufferPoolPolicySet, DaemonIngestCompletionFraction, DaemonIngestErrorRate,
    DaemonIngestHddQueueState, DaemonIngestHddTargetQueue, DaemonIngestPipelinePressure,
    DaemonIngestPipelineStage, DaemonIngestPlacementSchedulerInput, DaemonIngestPressure,
    DaemonIngestProgressEvent, DaemonIngestProgressFractions, DaemonIngestQueueDepths,
    DaemonIngestResourcePolicy, DaemonIngestSchedulingPolicy, DaemonIngestStage,
    DaemonIngestSummary, DaemonIngestSystemSafetyReserve, DaemonIngestSystemTelemetry,
    DaemonIngestTargetCapacity, DaemonIngestTargetFailureState, DaemonIngestTelemetry,
    DaemonIngestThroughputTelemetry, DaemonIngestThroughputTrend, DaemonIngestWorkerActivity,
    DaemonIngestWorkerCounts, DaemonIngestWorkerTelemetry, DaemonJobAcceptedResponse,
    DaemonJobCancelRequest, DaemonJobCancelResponse, DaemonJobEvent, DaemonJobId, DaemonJobIdError,
    DaemonJobKind, DaemonJobProgress, DaemonJobState, DaemonJobStatusRequest,
    DaemonJobStatusResponse, DaemonJobSummary, DaemonJobValidationError,
    DaemonLocalAdminAcceptedResponse, DaemonLocalAdminCommand, DaemonLocalAdminValidationError,
    DaemonRequestValidationError, DaemonServiceLifecycleRequest, DaemonServiceLifecycleResponse,
    DaemonServiceOperation, DaemonServiceProvisionRequest, DaemonServiceProvisionResponse,
    DaemonServiceStatusDetail, DaemonServiceStatusRequest, DaemonServiceStatusResponse,
    DaemonSourceReadBackpressureAction, DaemonSourceReadBackpressureDecision,
    DaemonSourceReadBackpressureInput, DaemonSourceReadBackpressurePolicy,
    DaemonSourceReadBackpressureReason, DaemonSourceReadPriority, DaemonSsdPressure,
    IngestJobStatusRequest, IngestJobStatusResponse, StoreInventoryItem, StoreInventoryRequest,
    StoreInventoryResponse, SubmitIngestFilesRequest, SubmitIngestFilesResponse,
};
pub use auth::{
    authorize_store_write, DaemonAuthorizationError, DaemonLocalActor, DaemonStoreAccessPolicy,
};
pub use client::{
    DaemonClient, DaemonClientError, DaemonClientTransport, InProcessDaemonTransport,
    UnixSocketDaemonTransport,
};
pub use runtime::{
    provision_garage_store_registry, DaemonRuntimeConfig, DaemonRuntimeConfigError,
    DaemonServiceRuntimeError, GarageProvisioningSummary, GarageServiceController,
    GarageServiceRuntimeConfig, GarageStoreRegistryProvisioningSummary, ServiceCommandOutput,
    ServiceCommandRunner, SystemServiceCommandRunner, DEFAULT_DAEMON_CONFIG_PATH,
    DEFAULT_DAEMON_GROUP, DEFAULT_DAEMON_LOG_DIR, DEFAULT_DAEMON_RUNTIME_DIR,
    DEFAULT_DAEMON_SERVICE_USER, DEFAULT_DAEMON_SOCKET_FILE_NAME, DEFAULT_DAEMON_STATE_DIR,
    LINUX_DAEMON_CONFIG_PATH, LINUX_DAEMON_LOG_DIR, LINUX_DAEMON_RUNTIME_DIR,
    LINUX_DAEMON_STATE_DIR,
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
        assert_eq!(version(), "0.0.0");
    }
}
