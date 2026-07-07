//! Transport-neutral daemon API contracts.

mod health;
mod ingest;
mod jobs;
mod service;
mod stores;

pub use health::{
    DaemonApiWarning, DaemonDiskHealthSummary, DaemonHealthSummaryRequest,
    DaemonHealthSummaryResponse, DaemonIngestSummary, DaemonSsdPressure,
};
pub use ingest::{
    CancelIngestJobRequest, CancelIngestJobResponse, DaemonIngestBottleneck,
    DaemonIngestCompletionFraction, DaemonIngestPipelinePressure, DaemonIngestPipelineStage,
    DaemonIngestPressure, DaemonIngestProgressEvent, DaemonIngestProgressFractions,
    DaemonIngestQueueDepths, DaemonIngestResourcePolicy, DaemonIngestStage,
    DaemonIngestSystemSafetyReserve, DaemonIngestSystemTelemetry, DaemonIngestTelemetry,
    DaemonIngestThroughputTelemetry, DaemonIngestThroughputTrend, DaemonIngestWorkerActivity,
    DaemonIngestWorkerCounts, DaemonIngestWorkerTelemetry, DaemonRequestValidationError,
    IngestJobStatusRequest, IngestJobStatusResponse, SubmitIngestFilesRequest,
    SubmitIngestFilesResponse,
};
pub use jobs::{
    DaemonJobAcceptedResponse, DaemonJobCancelRequest, DaemonJobCancelResponse, DaemonJobEvent,
    DaemonJobId, DaemonJobIdError, DaemonJobKind, DaemonJobProgress, DaemonJobState,
    DaemonJobStatusRequest, DaemonJobStatusResponse, DaemonJobSummary, DaemonJobValidationError,
};
pub use service::{
    DaemonServiceLifecycleRequest, DaemonServiceLifecycleResponse, DaemonServiceOperation,
    DaemonServiceProvisionRequest, DaemonServiceProvisionResponse, DaemonServiceStatusDetail,
    DaemonServiceStatusRequest, DaemonServiceStatusResponse,
};
pub use stores::{StoreInventoryItem, StoreInventoryRequest, StoreInventoryResponse};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "command", content = "payload")]
pub enum DaemonApiRequest {
    HealthSummary(DaemonHealthSummaryRequest),
    StoreInventory(StoreInventoryRequest),
    SubmitIngestFiles(SubmitIngestFilesRequest),
    IngestJobStatus(IngestJobStatusRequest),
    CancelIngestJob(CancelIngestJobRequest),
    ServiceStatus(DaemonServiceStatusRequest),
    ServiceLifecycle(DaemonServiceLifecycleRequest),
    ServiceProvision(DaemonServiceProvisionRequest),
}

impl DaemonApiRequest {
    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        match self {
            Self::SubmitIngestFiles(request) => request.validate(),
            Self::CancelIngestJob(request) => request.validate(),
            Self::ServiceLifecycle(request) => request.validate(),
            Self::ServiceProvision(request) => request.validate(),
            Self::HealthSummary(_)
            | Self::StoreInventory(_)
            | Self::IngestJobStatus(_)
            | Self::ServiceStatus(_) => Ok(()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "payload")]
pub enum DaemonApiResponse {
    HealthSummary(DaemonHealthSummaryResponse),
    StoreInventory(StoreInventoryResponse),
    SubmitIngestFiles(SubmitIngestFilesResponse),
    IngestJobStatus(IngestJobStatusResponse),
    CancelIngestJob(CancelIngestJobResponse),
    ServiceStatus(DaemonServiceStatusResponse),
    ServiceLifecycle(DaemonServiceLifecycleResponse),
    ServiceProvision(DaemonServiceProvisionResponse),
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
        DaemonApiRequest, DaemonServiceLifecycleRequest, DaemonServiceOperation,
        DaemonServiceProvisionRequest, DaemonServiceStatusRequest, StoreInventoryRequest,
        SubmitIngestFilesRequest,
    };
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_object_service::ObjectServiceProviderId;

    #[test]
    fn serializes_request_with_stable_command_name() {
        let request = DaemonApiRequest::StoreInventory(StoreInventoryRequest::default());

        let encoded = serde_json::to_value(request).expect("request serializes");

        assert_eq!(encoded["command"], "store_inventory");
    }

    #[test]
    fn delegates_submit_ingest_validation() {
        let request = DaemonApiRequest::SubmitIngestFiles(SubmitIngestFilesRequest {
            endpoint: StoreId::new("zymo").expect("store id"),
            source_path: "relative".into(),
            copies: None,
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
            client_request_id: None,
        });

        assert!(request.validate().is_err());
    }
}
