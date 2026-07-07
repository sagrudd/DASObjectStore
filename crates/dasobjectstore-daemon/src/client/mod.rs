//! Client boundary for callers that submit requests to `dasobjectstored`.

mod error;
mod in_process;
mod unix_socket;

pub use error::DaemonClientError;
pub use in_process::InProcessDaemonTransport;
pub use unix_socket::UnixSocketDaemonTransport;

use crate::api::{
    AssignLocalUserToLocalGroupRequest, AssignLocalUserToLocalGroupResponse,
    CancelIngestJobRequest, CancelIngestJobResponse, CreateLocalGroupRequest,
    CreateLocalGroupResponse, DaemonApiRequest, DaemonApiResponse, DaemonHealthSummaryRequest,
    DaemonHealthSummaryResponse, DaemonServiceLifecycleRequest, DaemonServiceLifecycleResponse,
    DaemonServiceProvisionRequest, DaemonServiceProvisionResponse, DaemonServiceStatusRequest,
    DaemonServiceStatusResponse, IngestJobStatusRequest, IngestJobStatusResponse,
    StoreInventoryRequest, StoreInventoryResponse, SubmitIngestFilesRequest,
    SubmitIngestFilesResponse,
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

    pub fn service_status(
        &self,
        request: DaemonServiceStatusRequest,
    ) -> Result<DaemonServiceStatusResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::ServiceStatus(request))? {
            DaemonApiResponse::ServiceStatus(response) => Ok(response),
            response => Err(unexpected("service_status", response)),
        }
    }

    pub fn service_lifecycle(
        &self,
        request: DaemonServiceLifecycleRequest,
    ) -> Result<DaemonServiceLifecycleResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::ServiceLifecycle(request))? {
            DaemonApiResponse::ServiceLifecycle(response) => Ok(response),
            response => Err(unexpected("service_lifecycle", response)),
        }
    }

    pub fn service_provision(
        &self,
        request: DaemonServiceProvisionRequest,
    ) -> Result<DaemonServiceProvisionResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::ServiceProvision(request))? {
            DaemonApiResponse::ServiceProvision(response) => Ok(response),
            response => Err(unexpected("service_provision", response)),
        }
    }

    pub fn create_local_group(
        &self,
        request: CreateLocalGroupRequest,
    ) -> Result<CreateLocalGroupResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::CreateLocalGroup(request))? {
            DaemonApiResponse::CreateLocalGroup(response) => Ok(response),
            response => Err(unexpected("create_local_group", response)),
        }
    }

    pub fn assign_local_user_to_local_group(
        &self,
        request: AssignLocalUserToLocalGroupRequest,
    ) -> Result<AssignLocalUserToLocalGroupResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::AssignLocalUserToLocalGroup(request))? {
            DaemonApiResponse::AssignLocalUserToLocalGroup(response) => Ok(response),
            response => Err(unexpected("assign_local_user_to_local_group", response)),
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
        DaemonApiResponse::ServiceStatus(_) => "service_status",
        DaemonApiResponse::ServiceLifecycle(_) => "service_lifecycle",
        DaemonApiResponse::ServiceProvision(_) => "service_provision",
        DaemonApiResponse::CreateLocalGroup(_) => "create_local_group",
        DaemonApiResponse::AssignLocalUserToLocalGroup(_) => "assign_local_user_to_local_group",
        DaemonApiResponse::IngestProgress(_) => "ingest_progress",
        DaemonApiResponse::Error(_) => "error",
    }
}

#[cfg(test)]
mod tests {
    use super::{DaemonClient, DaemonClientError, InProcessDaemonTransport};
    use crate::api::{
        AssignLocalUserToLocalGroupRequest, AssignLocalUserToLocalGroupResponse,
        CreateLocalGroupRequest, CreateLocalGroupResponse, DaemonApiRequest, DaemonApiResponse,
        DaemonIngestConflictPolicy, DaemonServiceLifecycleRequest, DaemonServiceLifecycleResponse,
        DaemonServiceOperation, DaemonServiceProvisionRequest, DaemonServiceProvisionResponse,
        DaemonServiceStatusRequest, DaemonServiceStatusResponse, StoreInventoryRequest,
        StoreInventoryResponse, SubmitIngestFilesRequest,
    };
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_object_service::{ObjectServiceProviderId, ServiceState};
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
                conflict_policy: DaemonIngestConflictPolicy::Strict,
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

    #[test]
    fn service_status_uses_typed_request_and_response() {
        let seen = RefCell::new(Vec::new());
        let transport = InProcessDaemonTransport::new(|request| {
            seen.borrow_mut().push(request);
            Ok(DaemonApiResponse::ServiceStatus(
                DaemonServiceStatusResponse {
                    provider_id: ObjectServiceProviderId::Garage,
                    state: ServiceState::Running,
                    endpoint: Some("http://127.0.0.1:3900".to_string()),
                    message: None,
                    detail: None,
                },
            ))
        });
        let client = DaemonClient::new(transport);

        let response = client
            .service_status(DaemonServiceStatusRequest {
                include_detail: true,
            })
            .expect("service status response");

        assert_eq!(response.provider_id, ObjectServiceProviderId::Garage);
        assert!(matches!(
            seen.borrow().as_slice(),
            [DaemonApiRequest::ServiceStatus(_)]
        ));
    }

    #[test]
    fn service_lifecycle_uses_typed_request_and_response() {
        let seen = RefCell::new(Vec::new());
        let transport = InProcessDaemonTransport::new(|request| {
            seen.borrow_mut().push(request);
            Ok(DaemonApiResponse::ServiceLifecycle(
                DaemonServiceLifecycleResponse::accepted(
                    crate::api::DaemonJobId::new("service-1").expect("job id"),
                    "2026-07-07T11:38:12Z",
                    true,
                    DaemonServiceOperation::Start,
                    ObjectServiceProviderId::Garage,
                ),
            ))
        });
        let client = DaemonClient::new(transport);

        let response = client
            .service_lifecycle(DaemonServiceLifecycleRequest {
                operation: DaemonServiceOperation::Start,
                provider_id: ObjectServiceProviderId::Garage,
                dry_run: true,
                client_request_id: Some("request-1".to_string()),
            })
            .expect("service lifecycle response");

        assert!(response.accepted.dry_run);
        assert!(matches!(
            seen.borrow().as_slice(),
            [DaemonApiRequest::ServiceLifecycle(_)]
        ));
    }

    #[test]
    fn service_provision_uses_typed_request_and_response() {
        let seen = RefCell::new(Vec::new());
        let transport = InProcessDaemonTransport::new(|request| {
            seen.borrow_mut().push(request);
            Ok(DaemonApiResponse::ServiceProvision(
                DaemonServiceProvisionResponse::accepted(
                    crate::api::DaemonJobId::new("service-provision-1").expect("job id"),
                    "2026-07-07T12:05:42Z",
                    true,
                    ObjectServiceProviderId::Garage,
                    "/etc/dasobjectstore/stores.json",
                    1,
                    1,
                    3,
                ),
            ))
        });
        let client = DaemonClient::new(transport);

        let response = client
            .service_provision(DaemonServiceProvisionRequest {
                provider_id: ObjectServiceProviderId::Garage,
                dry_run: true,
                client_request_id: Some("request-1".to_string()),
            })
            .expect("service provision response");

        assert!(response.accepted.dry_run);
        assert_eq!(response.commands, 3);
        assert!(matches!(
            seen.borrow().as_slice(),
            [DaemonApiRequest::ServiceProvision(_)]
        ));
    }

    #[test]
    fn create_local_group_uses_typed_request_and_response() {
        let seen = RefCell::new(Vec::new());
        let transport = InProcessDaemonTransport::new(|request| {
            seen.borrow_mut().push(request);
            Ok(DaemonApiResponse::CreateLocalGroup(
                CreateLocalGroupResponse::accepted(
                    crate::api::DaemonJobId::new("local-admin-1").expect("job id"),
                    "2026-07-07T12:10:42Z",
                    true,
                    Some("request-1".to_string()),
                    "mnemosyne",
                    Some("operator".to_string()),
                ),
            ))
        });
        let client = DaemonClient::new(transport);

        let response = client
            .create_local_group(CreateLocalGroupRequest {
                group_name: "mnemosyne".to_string(),
                dry_run: true,
                client_request_id: Some("request-1".to_string()),
                administrator_actor: Some("operator".to_string()),
                confirmation_marker: "confirm create local group".to_string(),
            })
            .expect("create group response");

        assert_eq!(response.group_name, "mnemosyne");
        assert!(matches!(
            seen.borrow().as_slice(),
            [DaemonApiRequest::CreateLocalGroup(_)]
        ));
    }

    #[test]
    fn assign_local_user_to_local_group_uses_typed_request_and_response() {
        let seen = RefCell::new(Vec::new());
        let transport = InProcessDaemonTransport::new(|request| {
            seen.borrow_mut().push(request);
            Ok(DaemonApiResponse::AssignLocalUserToLocalGroup(
                AssignLocalUserToLocalGroupResponse::accepted(
                    crate::api::DaemonJobId::new("local-admin-2").expect("job id"),
                    "2026-07-07T12:11:42Z",
                    true,
                    Some("request-2".to_string()),
                    "stephen",
                    "mnemosyne",
                    Some("operator".to_string()),
                ),
            ))
        });
        let client = DaemonClient::new(transport);

        let response = client
            .assign_local_user_to_local_group(AssignLocalUserToLocalGroupRequest {
                username: "stephen".to_string(),
                group_name: "mnemosyne".to_string(),
                dry_run: true,
                client_request_id: Some("request-2".to_string()),
                administrator_actor: Some("operator".to_string()),
                confirmation_marker: "confirm assign local user".to_string(),
            })
            .expect("assign user response");

        assert_eq!(response.username, "stephen");
        assert_eq!(response.group_name, "mnemosyne");
        assert!(matches!(
            seen.borrow().as_slice(),
            [DaemonApiRequest::AssignLocalUserToLocalGroup(_)]
        ));
    }

    #[test]
    fn rejects_invalid_local_admin_request_before_transport_send() {
        let transport = InProcessDaemonTransport::new(|_request| {
            panic!("invalid request should not reach transport")
        });
        let client = DaemonClient::new(transport);

        let err = client
            .create_local_group(CreateLocalGroupRequest {
                group_name: "Invalid Group".to_string(),
                dry_run: true,
                client_request_id: None,
                administrator_actor: None,
                confirmation_marker: "confirm create local group".to_string(),
            })
            .expect_err("invalid group name rejected");

        assert!(matches!(err, DaemonClientError::RequestValidation(_)));
    }
}
