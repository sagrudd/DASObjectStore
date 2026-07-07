//! Managed daemon boundary for DASObjectStore.

pub mod api;
pub mod auth;
pub mod client;
pub mod runtime;
pub mod server;

pub use api::{
    CancelIngestJobRequest, CancelIngestJobResponse, DaemonApiErrorResponse, DaemonApiRequest,
    DaemonApiResponse, DaemonApiWarning, DaemonDiskHealthSummary, DaemonHealthSummaryRequest,
    DaemonHealthSummaryResponse, DaemonIngestProgressEvent, DaemonIngestStage, DaemonIngestSummary,
    DaemonJobAcceptedResponse, DaemonJobCancelRequest, DaemonJobCancelResponse, DaemonJobEvent,
    DaemonJobId, DaemonJobIdError, DaemonJobKind, DaemonJobProgress, DaemonJobState,
    DaemonJobStatusRequest, DaemonJobStatusResponse, DaemonJobSummary, DaemonJobValidationError,
    DaemonRequestValidationError, DaemonServiceLifecycleRequest, DaemonServiceLifecycleResponse,
    DaemonServiceOperation, DaemonServiceStatusDetail, DaemonServiceStatusRequest,
    DaemonServiceStatusResponse, DaemonSsdPressure, IngestJobStatusRequest,
    IngestJobStatusResponse, StoreInventoryItem, StoreInventoryRequest, StoreInventoryResponse,
    SubmitIngestFilesRequest, SubmitIngestFilesResponse,
};
pub use auth::{
    authorize_store_write, DaemonAuthorizationError, DaemonLocalActor, DaemonStoreAccessPolicy,
};
pub use client::{
    DaemonClient, DaemonClientError, DaemonClientTransport, InProcessDaemonTransport,
    UnixSocketDaemonTransport,
};
pub use runtime::{
    DaemonRuntimeConfig, DaemonRuntimeConfigError, DaemonServiceRuntimeError,
    GarageServiceController, GarageServiceRuntimeConfig, ServiceCommandOutput,
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
