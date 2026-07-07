use crate::api::{
    AssignLocalUserToLocalGroupRequest, AssignLocalUserToLocalGroupResponse,
    CreateLocalGroupRequest, CreateLocalGroupResponse, DaemonApiErrorResponse, DaemonApiRequest,
    DaemonApiResponse, DaemonIngestProgressEvent, DaemonServiceLifecycleRequest,
    DaemonServiceLifecycleResponse, DaemonServiceProvisionRequest, DaemonServiceProvisionResponse,
    DaemonServiceStatusRequest, DaemonServiceStatusResponse, SubmitIngestFilesRequest,
    SubmitIngestFilesResponse,
};
use crate::runtime::{
    provision_garage_store_registry, submit_ingest_files_to_local_store_with_progress,
    DaemonIngestFilesRuntimeError, DaemonServiceRuntimeError, GarageServiceController,
    LocalAdminRuntimeError, LocalGroupAdminController, LocalGroupAdministrationOperation,
    LocalGroupAdministrationRequest, ServiceCommandRunner, SystemLocalAdminCommandRunner,
};
use dasobjectstore_object_service::{default_store_registry_path, ObjectServiceProviderId};
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
        self.handle_with_progress(request, |_| {})
    }

    pub fn handle_with_progress(
        &self,
        request: DaemonApiRequest,
        mut emit_progress: impl FnMut(DaemonIngestProgressEvent),
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
            DaemonApiRequest::ServiceProvision(request) => self
                .service_orchestrator
                .provision(request, &self.clock.now_utc())
                .map(DaemonApiResponse::ServiceProvision)
                .map_err(DaemonRequestHandlerError::ServiceRuntime),
            DaemonApiRequest::CreateLocalGroup(request) => self
                .service_orchestrator
                .create_local_group(request, &self.clock.now_utc())
                .map(DaemonApiResponse::CreateLocalGroup)
                .map_err(DaemonRequestHandlerError::LocalAdminRuntime),
            DaemonApiRequest::AssignLocalUserToLocalGroup(request) => self
                .service_orchestrator
                .assign_local_user_to_local_group(request, &self.clock.now_utc())
                .map(DaemonApiResponse::AssignLocalUserToLocalGroup)
                .map_err(DaemonRequestHandlerError::LocalAdminRuntime),
            DaemonApiRequest::SubmitIngestFiles(request) => {
                match self.service_orchestrator.submit_ingest_files(
                    request,
                    &self.clock.now_utc(),
                    &mut emit_progress,
                ) {
                    Ok(response) => Ok(DaemonApiResponse::SubmitIngestFiles(response)),
                    Err(error) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "ingest_files_failed",
                        error.to_string(),
                    ))),
                }
            }
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

    fn provision(
        &self,
        request: DaemonServiceProvisionRequest,
        accepted_at_utc: &str,
    ) -> Result<DaemonServiceProvisionResponse, DaemonServiceRuntimeError>;

    fn create_local_group(
        &self,
        _request: CreateLocalGroupRequest,
        _accepted_at_utc: &str,
    ) -> Result<CreateLocalGroupResponse, LocalAdminRuntimeError> {
        Err(LocalAdminRuntimeError::UnsupportedOperation {
            operation: "create_local_group requires a local admin orchestrator".to_string(),
        })
    }

    fn assign_local_user_to_local_group(
        &self,
        _request: AssignLocalUserToLocalGroupRequest,
        _accepted_at_utc: &str,
    ) -> Result<AssignLocalUserToLocalGroupResponse, LocalAdminRuntimeError> {
        Err(LocalAdminRuntimeError::UnsupportedOperation {
            operation: "assign_local_user_to_local_group requires a local admin orchestrator"
                .to_string(),
        })
    }

    fn submit_ingest_files(
        &self,
        _request: SubmitIngestFilesRequest,
        _accepted_at_utc: &str,
        _emit_progress: &mut dyn FnMut(DaemonIngestProgressEvent),
    ) -> Result<SubmitIngestFilesResponse, DaemonIngestFilesRuntimeError> {
        Err(DaemonIngestFilesRuntimeError::CommandFailed(
            "submit_ingest_files requires a file ingest orchestrator".to_string(),
        ))
    }
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

    fn provision(
        &self,
        request: DaemonServiceProvisionRequest,
        accepted_at_utc: &str,
    ) -> Result<DaemonServiceProvisionResponse, DaemonServiceRuntimeError> {
        request.validate()?;
        let summary =
            provision_garage_store_registry(self, default_store_registry_path(), request.dry_run)?;
        let job_id_value = format!(
            "service-provision-{}",
            accepted_at_utc
                .chars()
                .map(|character| if character.is_ascii_alphanumeric() {
                    character
                } else {
                    '-'
                })
                .collect::<String>()
                .trim_matches('-')
                .to_ascii_lowercase()
        );
        let job_id = crate::api::DaemonJobId::new(job_id_value.clone())
            .map_err(|_| DaemonServiceRuntimeError::InvalidJobId(job_id_value))?;
        Ok(DaemonServiceProvisionResponse::accepted(
            job_id,
            accepted_at_utc,
            request.dry_run,
            ObjectServiceProviderId::Garage,
            summary.registry_path.to_string_lossy().to_string(),
            summary.stores,
            summary.buckets,
            summary.commands,
        ))
    }

    fn create_local_group(
        &self,
        request: CreateLocalGroupRequest,
        accepted_at_utc: &str,
    ) -> Result<CreateLocalGroupResponse, LocalAdminRuntimeError> {
        let administrator_actor = request.administrator_actor.clone();
        let response = LocalGroupAdminController::new(SystemLocalAdminCommandRunner).execute(
            LocalGroupAdministrationRequest {
                operation: LocalGroupAdministrationOperation::CreateGroup,
                group_name: request.group_name,
                username: None,
                dry_run: request.dry_run,
                client_request_id: request.client_request_id,
                administrator_confirmation: Some(request.confirmation_marker),
            },
            accepted_at_utc,
        )?;

        Ok(CreateLocalGroupResponse {
            accepted: response.accepted,
            group_name: response.group_name,
            administrator_actor,
        })
    }

    fn assign_local_user_to_local_group(
        &self,
        request: AssignLocalUserToLocalGroupRequest,
        accepted_at_utc: &str,
    ) -> Result<AssignLocalUserToLocalGroupResponse, LocalAdminRuntimeError> {
        let administrator_actor = request.administrator_actor.clone();
        let response = LocalGroupAdminController::new(SystemLocalAdminCommandRunner).execute(
            LocalGroupAdministrationRequest {
                operation: LocalGroupAdministrationOperation::AssignUserToGroup,
                group_name: request.group_name,
                username: Some(request.username),
                dry_run: request.dry_run,
                client_request_id: request.client_request_id,
                administrator_confirmation: Some(request.confirmation_marker),
            },
            accepted_at_utc,
        )?;

        let username = response
            .username
            .ok_or(LocalAdminRuntimeError::MissingField { field: "username" })?;

        Ok(AssignLocalUserToLocalGroupResponse {
            accepted: response.accepted,
            username,
            group_name: response.group_name,
            administrator_actor,
        })
    }

    fn submit_ingest_files(
        &self,
        request: SubmitIngestFilesRequest,
        accepted_at_utc: &str,
        emit_progress: &mut dyn FnMut(DaemonIngestProgressEvent),
    ) -> Result<SubmitIngestFilesResponse, DaemonIngestFilesRuntimeError> {
        submit_ingest_files_to_local_store_with_progress(request, accepted_at_utc, emit_progress)
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
    LocalAdminRuntime(LocalAdminRuntimeError),
    IngestRuntime(DaemonIngestFilesRuntimeError),
}

impl Display for DaemonRequestHandlerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RequestValidation(error) => Display::fmt(error, formatter),
            Self::ServiceRuntime(error) => Display::fmt(error, formatter),
            Self::LocalAdminRuntime(error) => Display::fmt(error, formatter),
            Self::IngestRuntime(error) => Display::fmt(error, formatter),
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
            Self::ServiceProvision(_) => "service_provision",
            Self::CreateLocalGroup(_) => "create_local_group",
            Self::AssignLocalUserToLocalGroup(_) => "assign_local_user_to_local_group",
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
        AssignLocalUserToLocalGroupRequest, AssignLocalUserToLocalGroupResponse,
        CreateLocalGroupRequest, CreateLocalGroupResponse, DaemonApiRequest, DaemonApiResponse,
        DaemonJobId, DaemonRequestValidationError, DaemonServiceLifecycleRequest,
        DaemonServiceLifecycleResponse, DaemonServiceOperation, DaemonServiceProvisionRequest,
        DaemonServiceProvisionResponse, DaemonServiceStatusRequest, DaemonServiceStatusResponse,
        StoreInventoryRequest, SubmitIngestFilesRequest, SubmitIngestFilesResponse,
    };
    use crate::runtime::{
        DaemonIngestFilesRuntimeError, DaemonServiceRuntimeError, LocalAdminRuntimeError,
        LocalGroupAdministrationOperation,
    };
    use dasobjectstore_core::ids::{IngestJobId, StoreId};
    use dasobjectstore_core::object_type::ObjectType;
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
    fn dispatches_service_provision_with_clock_timestamp() {
        let service = FakeService::default();
        let handler =
            DaemonRequestHandler::new(service, FixedDaemonClock::new("2026-07-07T12:05:42Z"));

        let response = handler
            .handle(DaemonApiRequest::ServiceProvision(
                DaemonServiceProvisionRequest {
                    provider_id: ObjectServiceProviderId::Garage,
                    dry_run: true,
                    client_request_id: Some("request-1".to_string()),
                },
            ))
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::ServiceProvision(DaemonServiceProvisionResponse {
                buckets: 1,
                commands: 3,
                ..
            })
        ));
        assert_eq!(
            handler
                .service_orchestrator
                .provision_calls
                .borrow()
                .as_slice(),
            &["2026-07-07T12:05:42Z".to_string()]
        );
    }

    #[test]
    fn dispatches_create_local_group_with_clock_timestamp() {
        let service = FakeService::default();
        let handler =
            DaemonRequestHandler::new(service, FixedDaemonClock::new("2026-07-07T12:25:00Z"));

        let response = handler
            .handle(DaemonApiRequest::CreateLocalGroup(
                CreateLocalGroupRequest {
                    group_name: "daswriters".to_string(),
                    dry_run: true,
                    client_request_id: None,
                    administrator_actor: Some("operator".to_string()),
                    confirmation_marker: "confirm local group administration".to_string(),
                },
            ))
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::CreateLocalGroup(CreateLocalGroupResponse {
                group_name,
                ..
            }) if group_name == "daswriters"
        ));
        assert_eq!(
            handler
                .service_orchestrator
                .local_group_calls
                .borrow()
                .as_slice(),
            &[(
                "2026-07-07T12:25:00Z".to_string(),
                LocalGroupAdministrationOperation::CreateGroup,
                "daswriters".to_string(),
                None,
                true,
            )]
        );
    }

    #[test]
    fn dispatches_assign_local_user_to_local_group_with_clock_timestamp() {
        let service = FakeService::default();
        let handler =
            DaemonRequestHandler::new(service, FixedDaemonClock::new("2026-07-07T12:28:00Z"));

        let response = handler
            .handle(DaemonApiRequest::AssignLocalUserToLocalGroup(
                AssignLocalUserToLocalGroupRequest {
                    username: "stephen".to_string(),
                    group_name: "daswriters".to_string(),
                    dry_run: true,
                    client_request_id: Some("request-2".to_string()),
                    administrator_actor: Some("operator".to_string()),
                    confirmation_marker: "confirm local group administration".to_string(),
                },
            ))
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::AssignLocalUserToLocalGroup(
                AssignLocalUserToLocalGroupResponse {
                    username,
                    group_name,
                    ..
                }
            ) if username == "stephen" && group_name == "daswriters"
        ));
        assert_eq!(
            handler
                .service_orchestrator
                .local_group_calls
                .borrow()
                .as_slice(),
            &[(
                "2026-07-07T12:28:00Z".to_string(),
                LocalGroupAdministrationOperation::AssignUserToGroup,
                "daswriters".to_string(),
                Some("stephen".to_string()),
                true,
            )]
        );
    }

    #[test]
    fn dispatches_submit_ingest_files_with_clock_timestamp() {
        let service = FakeService::default();
        let handler =
            DaemonRequestHandler::new(service, FixedDaemonClock::new("2026-07-07T12:35:00Z"));
        let mut progress_events = Vec::new();

        let response = handler
            .handle_with_progress(
                DaemonApiRequest::SubmitIngestFiles(SubmitIngestFilesRequest {
                    endpoint: StoreId::new("zymo_fecal_2025.05").expect("store id"),
                    source_path: "/mnt/external/zymo".into(),
                    object_type: ObjectType::Fastq,
                    copies: Some(1),
                    conflict_policy: crate::api::DaemonIngestConflictPolicy::Strict,
                    dry_run: true,
                    client_request_id: Some("request-3".to_string()),
                }),
                |event| progress_events.push(event),
            )
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::SubmitIngestFiles(SubmitIngestFilesResponse {
                job_id,
                dry_run: true,
                ..
            }) if job_id.as_str() == "ingest-files-2026-07-07t12-35-00z"
        ));
        assert_eq!(
            handler
                .service_orchestrator
                .ingest_calls
                .borrow()
                .as_slice(),
            &[(
                "2026-07-07T12:35:00Z".to_string(),
                "zymo_fecal_2025.05".to_string(),
                true,
            )]
        );
        assert_eq!(progress_events.len(), 1);
        assert_eq!(progress_events[0].message.as_deref(), Some("queued"));
    }

    #[test]
    fn reports_submit_ingest_runtime_failures_as_api_errors() {
        let service = FakeService {
            ingest_error: Some("source is unreadable".to_string()),
            ..FakeService::default()
        };
        let handler = DaemonRequestHandler::new(service, FixedDaemonClock::new("now"));

        let response = handler
            .handle(DaemonApiRequest::SubmitIngestFiles(
                SubmitIngestFilesRequest {
                    endpoint: StoreId::new("zymo_fecal_2025.05").expect("store id"),
                    source_path: "/mnt/external/zymo".into(),
                    object_type: ObjectType::Naive,
                    copies: None,
                    conflict_policy: crate::api::DaemonIngestConflictPolicy::Strict,
                    dry_run: false,
                    client_request_id: None,
                },
            ))
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::Error(error) if error.code == "ingest_files_failed"
                && error.message == "source is unreadable"
        ));
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
        provision_calls: RefCell<Vec<String>>,
        local_group_calls: RefCell<
            Vec<(
                String,
                LocalGroupAdministrationOperation,
                String,
                Option<String>,
                bool,
            )>,
        >,
        ingest_calls: RefCell<Vec<(String, String, bool)>>,
        ingest_error: Option<String>,
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

        fn provision(
            &self,
            request: DaemonServiceProvisionRequest,
            accepted_at_utc: &str,
        ) -> Result<DaemonServiceProvisionResponse, DaemonServiceRuntimeError> {
            self.provision_calls
                .borrow_mut()
                .push(accepted_at_utc.to_string());
            Ok(DaemonServiceProvisionResponse::accepted(
                crate::api::DaemonJobId::new("service-provision-2026-07-07t12-05-42z")
                    .expect("job id"),
                accepted_at_utc,
                request.dry_run,
                ObjectServiceProviderId::Garage,
                "/etc/dasobjectstore/stores.json",
                1,
                1,
                3,
            ))
        }

        fn create_local_group(
            &self,
            request: CreateLocalGroupRequest,
            accepted_at_utc: &str,
        ) -> Result<CreateLocalGroupResponse, LocalAdminRuntimeError> {
            self.local_group_calls.borrow_mut().push((
                accepted_at_utc.to_string(),
                LocalGroupAdministrationOperation::CreateGroup,
                request.group_name.clone(),
                None,
                request.dry_run,
            ));
            Ok(CreateLocalGroupResponse::accepted(
                DaemonJobId::new("local-group-create-group-2026-07-07t12-25-00z").expect("job id"),
                accepted_at_utc,
                request.dry_run,
                request.client_request_id,
                request.group_name,
                request.administrator_actor,
            ))
        }

        fn assign_local_user_to_local_group(
            &self,
            request: AssignLocalUserToLocalGroupRequest,
            accepted_at_utc: &str,
        ) -> Result<AssignLocalUserToLocalGroupResponse, LocalAdminRuntimeError> {
            self.local_group_calls.borrow_mut().push((
                accepted_at_utc.to_string(),
                LocalGroupAdministrationOperation::AssignUserToGroup,
                request.group_name.clone(),
                Some(request.username.clone()),
                request.dry_run,
            ));
            Ok(AssignLocalUserToLocalGroupResponse::accepted(
                DaemonJobId::new("local-group-assign-user-to-group-2026-07-07t12-28-00z")
                    .expect("job id"),
                accepted_at_utc,
                request.dry_run,
                request.client_request_id,
                request.username,
                request.group_name,
                request.administrator_actor,
            ))
        }

        fn submit_ingest_files(
            &self,
            request: SubmitIngestFilesRequest,
            accepted_at_utc: &str,
            emit_progress: &mut dyn FnMut(crate::api::DaemonIngestProgressEvent),
        ) -> Result<SubmitIngestFilesResponse, DaemonIngestFilesRuntimeError> {
            if let Some(message) = &self.ingest_error {
                return Err(DaemonIngestFilesRuntimeError::CommandFailed(
                    message.clone(),
                ));
            }
            emit_progress(crate::api::DaemonIngestProgressEvent {
                job_id: IngestJobId::new("ingest-files-2026-07-07t12-35-00z").expect("job id"),
                endpoint: request.endpoint.clone(),
                stage: crate::api::DaemonIngestStage::Queued,
                pipeline_stage: Some(crate::api::DaemonIngestPipelineStage::Scan),
                work_bytes_done: 0,
                work_bytes_total: Some(0),
                files_done: 0,
                files_total: Some(0),
                current_object_id: None,
                ssd_pressure: None,
                telemetry: None,
                resource_policy: None,
                message: Some("queued".to_string()),
            });
            self.ingest_calls.borrow_mut().push((
                accepted_at_utc.to_string(),
                request.endpoint.as_str().to_string(),
                request.dry_run,
            ));
            Ok(SubmitIngestFilesResponse {
                job_id: IngestJobId::new("ingest-files-2026-07-07t12-35-00z").expect("job id"),
                accepted_at_utc: accepted_at_utc.to_string(),
                dry_run: request.dry_run,
            })
        }
    }
}
