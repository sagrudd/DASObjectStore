//! Client boundary for callers that submit requests to `dasobjectstored`.

mod error;
mod in_process;
mod unix_socket;

pub use error::DaemonClientError;
pub use in_process::InProcessDaemonTransport;
pub use unix_socket::UnixSocketDaemonTransport;

use crate::api::{
    CancelIngestJobRequest, CancelIngestJobResponse, DaemonApiRequest, DaemonApiResponse,
    DaemonHealthSummaryRequest, DaemonHealthSummaryResponse, IngestJobStatusRequest,
    IngestJobStatusResponse, StoreInventoryRequest, StoreInventoryResponse,
    SubmitIngestFilesRequest, SubmitIngestFilesResponse,
};

pub trait DaemonClientTransport {
    fn send(&self, request: DaemonApiRequest) -> Result<DaemonApiResponse, DaemonClientError>;
}

pub struct DaemonClient<T> {
    transport: T,
}

impl<T> DaemonClient<T>
where
    T: DaemonClientTransport,
{
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    pub fn send(&self, request: DaemonApiRequest) -> Result<DaemonApiResponse, DaemonClientError> {
        request.validate()?;
        self.transport.send(request)
    }

    pub fn health_summary(
        &self,
        request: DaemonHealthSummaryRequest,
    ) -> Result<DaemonHealthSummaryResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::HealthSummary(request))? {
            DaemonApiResponse::HealthSummary(response) => Ok(response),
            response => Err(unexpected("health_summary", response)),
        }
    }

    pub fn store_inventory(
        &self,
        request: StoreInventoryRequest,
    ) -> Result<StoreInventoryResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::StoreInventory(request))? {
            DaemonApiResponse::StoreInventory(response) => Ok(response),
            response => Err(unexpected("store_inventory", response)),
        }
    }

    pub fn submit_ingest_files(
        &self,
        request: SubmitIngestFilesRequest,
    ) -> Result<SubmitIngestFilesResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::SubmitIngestFiles(request))? {
            DaemonApiResponse::SubmitIngestFiles(response) => Ok(response),
            response => Err(unexpected("submit_ingest_files", response)),
        }
    }

    pub fn ingest_job_status(
        &self,
        request: IngestJobStatusRequest,
    ) -> Result<IngestJobStatusResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::IngestJobStatus(request))? {
            DaemonApiResponse::IngestJobStatus(response) => Ok(response),
            response => Err(unexpected("ingest_job_status", response)),
        }
    }

    pub fn cancel_ingest_job(
        &self,
        request: CancelIngestJobRequest,
    ) -> Result<CancelIngestJobResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::CancelIngestJob(request))? {
            DaemonApiResponse::CancelIngestJob(response) => Ok(response),
            response => Err(unexpected("cancel_ingest_job", response)),
        }
    }
}

fn unexpected(expected: &'static str, response: DaemonApiResponse) -> DaemonClientError {
    DaemonClientError::UnexpectedResponse {
        expected,
        actual: response_name(&response),
    }
}

fn response_name(response: &DaemonApiResponse) -> &'static str {
    match response {
        DaemonApiResponse::HealthSummary(_) => "health_summary",
        DaemonApiResponse::StoreInventory(_) => "store_inventory",
        DaemonApiResponse::SubmitIngestFiles(_) => "submit_ingest_files",
        DaemonApiResponse::IngestJobStatus(_) => "ingest_job_status",
        DaemonApiResponse::CancelIngestJob(_) => "cancel_ingest_job",
        DaemonApiResponse::IngestProgress(_) => "ingest_progress",
        DaemonApiResponse::Error(_) => "error",
    }
}

#[cfg(test)]
mod tests {
    use super::{DaemonClient, DaemonClientError, InProcessDaemonTransport};
    use crate::api::{
        DaemonApiRequest, DaemonApiResponse, StoreInventoryRequest, StoreInventoryResponse,
        SubmitIngestFilesRequest,
    };
    use dasobjectstore_core::ids::StoreId;
    use std::cell::RefCell;

    #[test]
    fn in_process_transport_round_trips_typed_response() {
        let seen = RefCell::new(Vec::new());
        let transport = InProcessDaemonTransport::new(|request| {
            seen.borrow_mut().push(request);
            Ok(DaemonApiResponse::StoreInventory(StoreInventoryResponse {
                stores: Vec::new(),
            }))
        });
        let client = DaemonClient::new(transport);

        let response = client
            .store_inventory(StoreInventoryRequest::default())
            .expect("store inventory response");

        assert!(response.stores.is_empty());
        assert!(matches!(
            seen.borrow().as_slice(),
            [DaemonApiRequest::StoreInventory(_)]
        ));
    }

    #[test]
    fn validates_request_before_transport_send() {
        let transport = InProcessDaemonTransport::new(|_request| {
            panic!("invalid request should not reach transport")
        });
        let client = DaemonClient::new(transport);

        let err = client
            .submit_ingest_files(SubmitIngestFilesRequest {
                endpoint: StoreId::new("zymo").expect("store id"),
                source_path: "relative".into(),
                copies: None,
                dry_run: false,
                client_request_id: None,
            })
            .expect_err("relative path is rejected");

        assert!(matches!(err, DaemonClientError::RequestValidation(_)));
    }

    #[test]
    fn rejects_unexpected_response_kind() {
        let transport = InProcessDaemonTransport::new(|_request| {
            Ok(DaemonApiResponse::StoreInventory(StoreInventoryResponse {
                stores: Vec::new(),
            }))
        });
        let client = DaemonClient::new(transport);

        let err = client
            .health_summary(Default::default())
            .expect_err("wrong response kind rejected");

        assert!(matches!(
            err,
            DaemonClientError::UnexpectedResponse {
                expected: "health_summary",
                actual: "store_inventory",
            }
        ));
    }
}
