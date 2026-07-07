//! Transport-neutral daemon API contracts.

mod health;
mod ingest;
mod stores;

pub use health::{
    DaemonApiWarning, DaemonDiskHealthSummary, DaemonHealthSummaryRequest,
    DaemonHealthSummaryResponse, DaemonIngestSummary, DaemonSsdPressure,
};
pub use ingest::{
    CancelIngestJobRequest, CancelIngestJobResponse, DaemonIngestProgressEvent, DaemonIngestStage,
    DaemonRequestValidationError, IngestJobStatusRequest, IngestJobStatusResponse,
    SubmitIngestFilesRequest, SubmitIngestFilesResponse,
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
}

impl DaemonApiRequest {
    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        match self {
            Self::SubmitIngestFiles(request) => request.validate(),
            Self::CancelIngestJob(request) => request.validate(),
            Self::HealthSummary(_) | Self::StoreInventory(_) | Self::IngestJobStatus(_) => Ok(()),
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
    use super::{DaemonApiRequest, StoreInventoryRequest, SubmitIngestFilesRequest};
    use dasobjectstore_core::ids::StoreId;

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
}
