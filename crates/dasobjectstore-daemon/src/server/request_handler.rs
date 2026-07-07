use crate::api::{
    DaemonApiErrorResponse, DaemonApiRequest, DaemonApiResponse, DaemonServiceLifecycleRequest,
    DaemonServiceLifecycleResponse, DaemonServiceStatusRequest, DaemonServiceStatusResponse,
};
use crate::runtime::{DaemonServiceRuntimeError, GarageServiceController, ServiceCommandRunner};
use std::fmt::{self, Display};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct DaemonRequestHandler<S, C> {
    service_orchestrator: S,
    clock: C,
}

impl<S, C> DaemonRequestHandler<S, C>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    pub fn new(service_orchestrator: S, clock: C) -> Self {
        Self {
            service_orchestrator,
            clock,
        }
    }

    pub fn handle(
        &self,
        request: DaemonApiRequest,
    ) -> Result<DaemonApiResponse, DaemonRequestHandlerError> {
        request.validate()?;

        match request {
            DaemonApiRequest::ServiceStatus(request) => self
                .service_orchestrator
                .status(request)
                .map(DaemonApiResponse::ServiceStatus)
                .map_err(DaemonRequestHandlerError::ServiceRuntime),
            DaemonApiRequest::ServiceLifecycle(request) => self
                .service_orchestrator
                .lifecycle(request, &self.clock.now_utc())
                .map(DaemonApiResponse::ServiceLifecycle)
                .map_err(DaemonRequestHandlerError::ServiceRuntime),
            request => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                "not_implemented",
                format!(
                    "{} is not wired into dasobjectstored yet",
                    request.command_name()
                ),
            ))),
        }
    }
}

pub trait DaemonServiceOrchestrator {
    fn status(
        &self,
        request: DaemonServiceStatusRequest,
    ) -> Result<DaemonServiceStatusResponse, DaemonServiceRuntimeError>;

    fn lifecycle(
        &self,
        request: DaemonServiceLifecycleRequest,
        accepted_at_utc: &str,
    ) -> Result<DaemonServiceLifecycleResponse, DaemonServiceRuntimeError>;
}

impl<R> DaemonServiceOrchestrator for GarageServiceController<R>
where
    R: ServiceCommandRunner,
{
    fn status(
        &self,
        request: DaemonServiceStatusRequest,
    ) -> Result<DaemonServiceStatusResponse, DaemonServiceRuntimeError> {
        GarageServiceController::status(self, request)
    }

    fn lifecycle(
        &self,
        request: DaemonServiceLifecycleRequest,
        accepted_at_utc: &str,
    ) -> Result<DaemonServiceLifecycleResponse, DaemonServiceRuntimeError> {
        GarageServiceController::lifecycle(self, request, accepted_at_utc)
    }
}

pub trait DaemonClock {
    fn now_utc(&self) -> String;
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SystemDaemonClock;

impl DaemonClock for SystemDaemonClock {
    fn now_utc(&self) -> String {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or_default();
        format!("unix-{seconds}")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixedDaemonClock {
    now_utc: String,
}

impl FixedDaemonClock {
    pub fn new(now_utc: impl Into<String>) -> Self {
        Self {
            now_utc: now_utc.into(),
        }
    }
}

impl DaemonClock for FixedDaemonClock {
    fn now_utc(&self) -> String {
        self.now_utc.clone()
    }
}

#[derive(Debug)]
pub enum DaemonRequestHandlerError {
    RequestValidation(crate::api::DaemonRequestValidationError),
    ServiceRuntime(DaemonServiceRuntimeError),
}

impl Display for DaemonRequestHandlerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RequestValidation(error) => Display::fmt(error, formatter),
            Self::ServiceRuntime(error) => Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for DaemonRequestHandlerError {}

impl From<crate::api::DaemonRequestValidationError> for DaemonRequestHandlerError {
    fn from(error: crate::api::DaemonRequestValidationError) -> Self {
        Self::RequestValidation(error)
    }
}

impl DaemonApiRequest {
    fn command_name(&self) -> &'static str {
        match self {
            Self::HealthSummary(_) => "health_summary",
            Self::StoreInventory(_) => "store_inventory",
            Self::SubmitIngestFiles(_) => "submit_ingest_files",
            Self::IngestJobStatus(_) => "ingest_job_status",
            Self::CancelIngestJob(_) => "cancel_ingest_job",
            Self::ServiceStatus(_) => "service_status",
            Self::ServiceLifecycle(_) => "service_lifecycle",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DaemonClock, DaemonRequestHandler, DaemonServiceOrchestrator, FixedDaemonClock,
        SystemDaemonClock,
    };
    use crate::api::{
        DaemonApiRequest, DaemonApiResponse, DaemonRequestValidationError,
        DaemonServiceLifecycleRequest, DaemonServiceLifecycleResponse, DaemonServiceOperation,
        DaemonServiceStatusRequest, DaemonServiceStatusResponse, StoreInventoryRequest,
    };
    use crate::runtime::DaemonServiceRuntimeError;
    use dasobjectstore_object_service::{ObjectServiceProviderId, ServiceState};
    use std::cell::RefCell;

    #[test]
    fn dispatches_service_status_to_orchestrator() {
        let service = FakeService::default();
        let handler =
            DaemonRequestHandler::new(service, FixedDaemonClock::new("2026-07-07T11:47:42Z"));

        let response = handler
            .handle(DaemonApiRequest::ServiceStatus(
                DaemonServiceStatusRequest {
                    include_detail: true,
                },
            ))
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::ServiceStatus(DaemonServiceStatusResponse {
                state: ServiceState::Running,
                ..
            })
        ));
        assert_eq!(
            handler
                .service_orchestrator
                .status_calls
                .borrow()
                .as_slice(),
            &[true]
        );
    }

    #[test]
    fn dispatches_service_lifecycle_with_clock_timestamp() {
        let service = FakeService::default();
        let handler =
            DaemonRequestHandler::new(service, FixedDaemonClock::new("2026-07-07T11:47:42Z"));

        let response = handler
            .handle(DaemonApiRequest::ServiceLifecycle(
                DaemonServiceLifecycleRequest {
                    operation: DaemonServiceOperation::Start,
                    provider_id: ObjectServiceProviderId::Garage,
                    dry_run: true,
                    client_request_id: Some("request-1".to_string()),
                },
            ))
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::ServiceLifecycle(DaemonServiceLifecycleResponse {
                operation: DaemonServiceOperation::Start,
                ..
            })
        ));
        assert_eq!(
            handler
                .service_orchestrator
                .lifecycle_calls
                .borrow()
                .as_slice(),
            &["2026-07-07T11:47:42Z".to_string()]
        );
    }

    #[test]
    fn validates_request_before_dispatch() {
        let service = FakeService::default();
        let handler = DaemonRequestHandler::new(service, FixedDaemonClock::new("now"));

        let error = handler
            .handle(DaemonApiRequest::ServiceLifecycle(
                DaemonServiceLifecycleRequest {
                    operation: DaemonServiceOperation::Start,
                    provider_id: ObjectServiceProviderId::Rustfs,
                    dry_run: false,
                    client_request_id: None,
                },
            ))
            .expect_err("invalid request rejected");

        assert!(matches!(
            error,
            super::DaemonRequestHandlerError::RequestValidation(
                DaemonRequestValidationError::UnsupportedServiceProvider { .. }
            )
        ));
        assert!(handler
            .service_orchestrator
            .lifecycle_calls
            .borrow()
            .is_empty());
    }

    #[test]
    fn reports_unwired_commands_as_api_errors() {
        let service = FakeService::default();
        let handler = DaemonRequestHandler::new(service, FixedDaemonClock::new("now"));

        let response = handler
            .handle(DaemonApiRequest::StoreInventory(
                StoreInventoryRequest::default(),
            ))
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::Error(error) if error.code == "not_implemented"
                && error.message.contains("store_inventory")
        ));
    }

    #[test]
    fn system_clock_returns_nonblank_timestamp() {
        assert!(!SystemDaemonClock.now_utc().trim().is_empty());
    }

    #[derive(Default)]
    struct FakeService {
        status_calls: RefCell<Vec<bool>>,
        lifecycle_calls: RefCell<Vec<String>>,
    }

    impl DaemonServiceOrchestrator for FakeService {
        fn status(
            &self,
            request: DaemonServiceStatusRequest,
        ) -> Result<DaemonServiceStatusResponse, DaemonServiceRuntimeError> {
            self.status_calls.borrow_mut().push(request.include_detail);
            Ok(DaemonServiceStatusResponse {
                provider_id: ObjectServiceProviderId::Garage,
                state: ServiceState::Running,
                endpoint: Some("http://127.0.0.1:3900".to_string()),
                message: None,
                detail: None,
            })
        }

        fn lifecycle(
            &self,
            request: DaemonServiceLifecycleRequest,
            accepted_at_utc: &str,
        ) -> Result<DaemonServiceLifecycleResponse, DaemonServiceRuntimeError> {
            self.lifecycle_calls
                .borrow_mut()
                .push(accepted_at_utc.to_string());
            Ok(DaemonServiceLifecycleResponse::accepted(
                crate::api::DaemonJobId::new("service-start-2026-07-07t11-47-42z").expect("job id"),
                accepted_at_utc,
                request.dry_run,
                request.operation,
                ObjectServiceProviderId::Garage,
            ))
        }
    }
}
