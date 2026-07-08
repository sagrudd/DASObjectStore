use crate::api::{
    AssignLocalUserToLocalGroupRequest, AssignLocalUserToLocalGroupResponse,
    CreateLocalGroupRequest, CreateLocalGroupResponse, CreateObjectStoreRequest,
    CreateObjectStoreResponse, DaemonApiErrorResponse, DaemonApiRequest, DaemonApiResponse,
    DaemonIngestProgressEvent, DaemonJobCancelRequest, DaemonJobCancelResponse, DaemonJobKind,
    DaemonJobProgress, DaemonJobState, DaemonJobStatusRequest, DaemonJobStatusResponse,
    DaemonJobSummary, DaemonLocalAdminAcceptedResponse, DaemonServiceLifecycleRequest,
    DaemonServiceLifecycleResponse, DaemonServiceProvisionRequest, DaemonServiceProvisionResponse,
    DaemonServiceStatusRequest, DaemonServiceStatusResponse, PrepareEnclosureRequest,
    PrepareEnclosureResponse, SubmitIngestFilesRequest, SubmitIngestFilesResponse,
};
use crate::runtime::{
    provision_garage_store_registry, submit_ingest_files_to_local_store_with_progress,
    AdminJobRegistry, DaemonIngestFilesRuntimeError, DaemonServiceRuntimeError,
    GarageServiceController, LocalAdminRuntimeError, LocalGroupAdminController,
    LocalGroupAdministrationOperation, LocalGroupAdministrationRequest, ServiceCommandRunner,
    SystemLocalAdminCommandRunner,
};
use dasobjectstore_object_service::{default_store_registry_path, ObjectServiceProviderId};
use std::fmt::{self, Display};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct DaemonRequestHandler<S, C> {
    service_orchestrator: S,
    clock: C,
    admin_job_registry: Option<Arc<dyn AdminJobRegistry>>,
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
            admin_job_registry: None,
        }
    }

    pub fn new_with_admin_job_registry(
        service_orchestrator: S,
        clock: C,
        admin_job_registry: Arc<dyn AdminJobRegistry>,
    ) -> Self {
        Self {
            service_orchestrator,
            clock,
            admin_job_registry: Some(admin_job_registry),
        }
    }

    pub fn handle(
        &self,
        request: DaemonApiRequest,
    ) -> Result<DaemonApiResponse, DaemonRequestHandlerError> {
        self.handle_with_progress(request, |_| Ok(()))
    }

    pub fn handle_with_progress(
        &self,
        request: DaemonApiRequest,
        mut emit_progress: impl FnMut(
            DaemonIngestProgressEvent,
        ) -> Result<(), DaemonIngestFilesRuntimeError>,
    ) -> Result<DaemonApiResponse, DaemonRequestHandlerError> {
        request.validate()?;

        match request {
            DaemonApiRequest::ServiceStatus(request) => self
                .service_orchestrator
                .status(request)
                .map(DaemonApiResponse::ServiceStatus)
                .map_err(DaemonRequestHandlerError::ServiceRuntime),
            DaemonApiRequest::ServiceLifecycle(request) => {
                let now = self.clock.now_utc();
                let response = self
                    .service_orchestrator
                    .lifecycle(request, &now)
                    .map_err(DaemonRequestHandlerError::ServiceRuntime)?;
                self.record_admin_job(daemon_job_summary_from_service_lifecycle(&response))?;
                Ok(DaemonApiResponse::ServiceLifecycle(response))
            }
            DaemonApiRequest::ServiceProvision(request) => {
                let now = self.clock.now_utc();
                let response = self
                    .service_orchestrator
                    .provision(request, &now)
                    .map_err(DaemonRequestHandlerError::ServiceRuntime)?;
                self.record_admin_job(daemon_job_summary_from_service_provision(&response))?;
                Ok(DaemonApiResponse::ServiceProvision(response))
            }
            DaemonApiRequest::PrepareEnclosure(request) => {
                let now = self.clock.now_utc();
                let response = self
                    .service_orchestrator
                    .prepare_enclosure(request, &now)
                    .map_err(DaemonRequestHandlerError::ServiceRuntime)?;
                self.record_admin_job(daemon_job_summary_from_prepare_enclosure(&response))?;
                Ok(DaemonApiResponse::PrepareEnclosure(response))
            }
            DaemonApiRequest::CreateObjectStore(request) => {
                let now = self.clock.now_utc();
                let response = self
                    .service_orchestrator
                    .create_object_store(request, &now)
                    .map_err(DaemonRequestHandlerError::ServiceRuntime)?;
                self.record_admin_job(daemon_job_summary_from_create_object_store(&response))?;
                Ok(DaemonApiResponse::CreateObjectStore(response))
            }
            DaemonApiRequest::CreateLocalGroup(request) => {
                let now = self.clock.now_utc();
                let response = self
                    .service_orchestrator
                    .create_local_group(request, &now)
                    .map_err(DaemonRequestHandlerError::LocalAdminRuntime)?;
                self.record_admin_job(daemon_job_summary_from_local_admin(
                    &response.accepted,
                    response.administrator_actor.clone(),
                ))?;
                Ok(DaemonApiResponse::CreateLocalGroup(response))
            }
            DaemonApiRequest::AssignLocalUserToLocalGroup(request) => {
                let now = self.clock.now_utc();
                let response = self
                    .service_orchestrator
                    .assign_local_user_to_local_group(request, &now)
                    .map_err(DaemonRequestHandlerError::LocalAdminRuntime)?;
                self.record_admin_job(daemon_job_summary_from_local_admin(
                    &response.accepted,
                    response.administrator_actor.clone(),
                ))?;
                Ok(DaemonApiResponse::AssignLocalUserToLocalGroup(response))
            }
            DaemonApiRequest::JobStatus(request) => self
                .admin_job_status(request)
                .map(DaemonApiResponse::JobStatus)
                .map_err(DaemonRequestHandlerError::ServiceRuntime),
            DaemonApiRequest::CancelJob(request) => self
                .cancel_admin_job(request, &self.clock.now_utc())
                .map(DaemonApiResponse::CancelJob)
                .map_err(DaemonRequestHandlerError::ServiceRuntime),
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

    fn record_admin_job(&self, job: DaemonJobSummary) -> Result<(), DaemonRequestHandlerError> {
        if let Some(registry) = &self.admin_job_registry {
            registry
                .record(job)
                .map_err(DaemonRequestHandlerError::ServiceRuntime)?;
        }
        Ok(())
    }

    fn admin_job_status(
        &self,
        request: DaemonJobStatusRequest,
    ) -> Result<DaemonJobStatusResponse, DaemonServiceRuntimeError> {
        if let Some(registry) = &self.admin_job_registry {
            return registry.status(request);
        }
        self.service_orchestrator.job_status(request)
    }

    fn cancel_admin_job(
        &self,
        request: DaemonJobCancelRequest,
        accepted_at_utc: &str,
    ) -> Result<DaemonJobCancelResponse, DaemonServiceRuntimeError> {
        if let Some(registry) = &self.admin_job_registry {
            return registry.cancel(request, accepted_at_utc);
        }
        self.service_orchestrator
            .cancel_job(request, accepted_at_utc)
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

    fn prepare_enclosure(
        &self,
        _request: PrepareEnclosureRequest,
        _accepted_at_utc: &str,
    ) -> Result<PrepareEnclosureResponse, DaemonServiceRuntimeError> {
        Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: "prepare_enclosure requires an enclosure preparation orchestrator"
                .to_string(),
        })
    }

    fn create_object_store(
        &self,
        _request: CreateObjectStoreRequest,
        _accepted_at_utc: &str,
    ) -> Result<CreateObjectStoreResponse, DaemonServiceRuntimeError> {
        Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: "create_object_store requires an ObjectStore administration orchestrator"
                .to_string(),
        })
    }

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

    fn job_status(
        &self,
        _request: DaemonJobStatusRequest,
    ) -> Result<DaemonJobStatusResponse, DaemonServiceRuntimeError> {
        Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: "job_status requires a daemon job orchestrator".to_string(),
        })
    }

    fn cancel_job(
        &self,
        _request: DaemonJobCancelRequest,
        _accepted_at_utc: &str,
    ) -> Result<DaemonJobCancelResponse, DaemonServiceRuntimeError> {
        Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: "cancel_job requires a daemon job orchestrator".to_string(),
        })
    }

    fn submit_ingest_files(
        &self,
        _request: SubmitIngestFilesRequest,
        _accepted_at_utc: &str,
        _emit_progress: &mut dyn FnMut(
            DaemonIngestProgressEvent,
        ) -> Result<(), DaemonIngestFilesRuntimeError>,
    ) -> Result<SubmitIngestFilesResponse, DaemonIngestFilesRuntimeError> {
        Err(DaemonIngestFilesRuntimeError::CommandFailed(
            "submit_ingest_files requires a file ingest orchestrator".to_string(),
        ))
    }
}

fn daemon_job_summary_from_service_lifecycle(
    response: &DaemonServiceLifecycleResponse,
) -> DaemonJobSummary {
    daemon_job_summary_from_accepted(
        response.accepted.job_id.clone(),
        response.accepted.kind.clone(),
        response.accepted.accepted_at_utc.clone(),
        response.accepted.dry_run,
        None,
        format!("service {:?} completed", response.operation),
    )
}

fn daemon_job_summary_from_service_provision(
    response: &DaemonServiceProvisionResponse,
) -> DaemonJobSummary {
    daemon_job_summary_from_accepted(
        response.accepted.job_id.clone(),
        response.accepted.kind.clone(),
        response.accepted.accepted_at_utc.clone(),
        response.accepted.dry_run,
        None,
        format!(
            "provisioned {} store(s), {} bucket(s), {} command(s)",
            response.stores, response.buckets, response.commands
        ),
    )
}

fn daemon_job_summary_from_prepare_enclosure(
    response: &PrepareEnclosureResponse,
) -> DaemonJobSummary {
    daemon_job_summary_from_accepted(
        response.accepted.job_id.clone(),
        response.accepted.kind.clone(),
        response.accepted.accepted_at_utc.clone(),
        response.accepted.dry_run,
        response.administrator_actor.clone(),
        format!(
            "prepared {} landing device and {} HDD device(s)",
            response.ssd_device.display(),
            response.hdd_devices.len()
        ),
    )
}

fn daemon_job_summary_from_create_object_store(
    response: &CreateObjectStoreResponse,
) -> DaemonJobSummary {
    daemon_job_summary_from_accepted(
        response.accepted.job_id.clone(),
        DaemonJobKind::ObjectStoreCreation,
        response.accepted.accepted_at_utc.clone(),
        response.accepted.dry_run,
        response.administrator_actor.clone(),
        format!(
            "ObjectStore {} creation accepted for writer group {}",
            response.store_id, response.writer_group
        ),
    )
}

fn daemon_job_summary_from_local_admin(
    accepted: &DaemonLocalAdminAcceptedResponse,
    actor: Option<String>,
) -> DaemonJobSummary {
    daemon_job_summary_from_accepted(
        accepted.job_id.clone(),
        DaemonJobKind::SystemAdministration,
        accepted.accepted_at_utc.clone(),
        accepted.dry_run,
        actor,
        format!(
            "local administrator command {:?} completed",
            accepted.command
        ),
    )
}

fn daemon_job_summary_from_accepted(
    job_id: crate::api::DaemonJobId,
    kind: DaemonJobKind,
    accepted_at_utc: String,
    dry_run: bool,
    actor: Option<String>,
    message: String,
) -> DaemonJobSummary {
    let message = if dry_run {
        format!("dry run: {message}")
    } else {
        message
    };
    DaemonJobSummary {
        job_id,
        kind,
        state: DaemonJobState::Complete,
        progress: DaemonJobProgress {
            stage: "complete".to_string(),
            work_bytes_done: 1,
            work_bytes_total: 1,
            work_units_done: 1,
            work_units_total: 1,
            message: Some(message),
        },
        submitted_at_utc: accepted_at_utc.clone(),
        updated_at_utc: accepted_at_utc,
        actor,
        failure_message: None,
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

    fn create_object_store(
        &self,
        request: CreateObjectStoreRequest,
        accepted_at_utc: &str,
    ) -> Result<CreateObjectStoreResponse, DaemonServiceRuntimeError> {
        let job_id_value = format!(
            "objectstore-create-{}",
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
        Ok(CreateObjectStoreResponse::accepted(
            job_id,
            accepted_at_utc,
            request,
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
        emit_progress: &mut dyn FnMut(
            DaemonIngestProgressEvent,
        ) -> Result<(), DaemonIngestFilesRuntimeError>,
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
            Self::JobStatus(_) => "job_status",
            Self::CancelJob(_) => "cancel_job",
            Self::ServiceStatus(_) => "service_status",
            Self::ServiceLifecycle(_) => "service_lifecycle",
            Self::ServiceProvision(_) => "service_provision",
            Self::PrepareEnclosure(_) => "prepare_enclosure",
            Self::CreateObjectStore(_) => "create_object_store",
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
        CreateLocalGroupRequest, CreateLocalGroupResponse, CreateObjectStoreRequest,
        CreateObjectStoreResponse, DaemonApiRequest, DaemonApiResponse, DaemonJobCancelRequest,
        DaemonJobCancelResponse, DaemonJobId, DaemonJobKind, DaemonJobProgress, DaemonJobState,
        DaemonJobStatusRequest, DaemonJobStatusResponse, DaemonJobSummary,
        DaemonRequestValidationError, DaemonServiceLifecycleRequest,
        DaemonServiceLifecycleResponse, DaemonServiceOperation, DaemonServiceProvisionRequest,
        DaemonServiceProvisionResponse, DaemonServiceStatusRequest, DaemonServiceStatusResponse,
        PrepareEnclosureFilesystem, PrepareEnclosureHddDevice, PrepareEnclosureRequest,
        PrepareEnclosureResponse, StoreInventoryRequest, SubmitIngestFilesRequest,
        SubmitIngestFilesResponse, ENCLOSURE_PREPARE_CONFIRMATION,
        OBJECT_STORE_CREATE_CONFIRMATION,
    };
    use crate::runtime::{
        admin_job_registry_path, DaemonIngestFilesRuntimeError, DaemonServiceRuntimeError,
        FileBackedAdminJobRegistry, LocalAdminRuntimeError, LocalGroupAdministrationOperation,
    };
    use dasobjectstore_core::ids::{IngestJobId, StoreId};
    use dasobjectstore_core::object_type::ObjectType;
    use dasobjectstore_object_service::{ObjectServiceProviderId, ServiceState};
    use std::cell::RefCell;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;

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
                |event| {
                    progress_events.push(event);
                    Ok(())
                },
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
    fn dispatches_prepare_enclosure_with_clock_timestamp() {
        let service = FakeService::default();
        let handler =
            DaemonRequestHandler::new(service, FixedDaemonClock::new("2026-07-08T19:40:00Z"));

        let response = handler
            .handle(DaemonApiRequest::PrepareEnclosure(
                PrepareEnclosureRequest {
                    ssd_device: "/dev/disk/by-id/nvme-ssd".into(),
                    hdd_devices: vec![PrepareEnclosureHddDevice {
                        disk_id: "qnap-1057".to_string(),
                        device_path: "/dev/disk/by-id/usb-qnap-1057".into(),
                    }],
                    mount_root: "/srv/dasobjectstore".into(),
                    filesystem: PrepareEnclosureFilesystem::Ext4,
                    owner: Some("stephen".to_string()),
                    dry_run: true,
                    client_request_id: Some("request-prepare-1".to_string()),
                    administrator_actor: Some("operator".to_string()),
                    allow_format: true,
                    existing_data_acknowledged: true,
                    confirmation_marker: ENCLOSURE_PREPARE_CONFIRMATION.to_string(),
                },
            ))
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::PrepareEnclosure(PrepareEnclosureResponse {
                accepted,
                ..
            }) if accepted.job_id.as_str() == "enclosure-prepare-2026-07-08t19-40-00z"
                && accepted.dry_run
        ));
        assert_eq!(
            handler
                .service_orchestrator
                .prepare_enclosure_calls
                .borrow()
                .as_slice(),
            &[(
                "2026-07-08T19:40:00Z".to_string(),
                "/dev/disk/by-id/nvme-ssd".to_string(),
                true,
            )]
        );
    }

    #[test]
    fn dispatches_create_object_store_with_clock_timestamp() {
        let service = FakeService::default();
        let handler =
            DaemonRequestHandler::new(service, FixedDaemonClock::new("2026-07-08T20:45:00Z"));

        let response = handler
            .handle(DaemonApiRequest::CreateObjectStore(
                create_object_store_request(),
            ))
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::CreateObjectStore(CreateObjectStoreResponse {
                accepted,
                store_id,
                ..
            }) if accepted.job_id.as_str() == "objectstore-create-2026-07-08t20-45-00z"
                && accepted.dry_run
                && store_id == "generated-data"
        ));
        assert_eq!(
            handler
                .service_orchestrator
                .create_object_store_calls
                .borrow()
                .as_slice(),
            &[(
                "2026-07-08T20:45:00Z".to_string(),
                "generated-data".to_string(),
                true,
            )]
        );
    }

    #[test]
    fn records_accepted_prepare_enclosure_job_in_registry() {
        let root = temp_root("record-prepare");
        let registry = Arc::new(FileBackedAdminJobRegistry::new(admin_job_registry_path(
            &root,
        )));
        let service = FakeService::default();
        let handler = DaemonRequestHandler::new_with_admin_job_registry(
            service,
            FixedDaemonClock::new("2026-07-08T19:40:00Z"),
            registry,
        );

        handler
            .handle(DaemonApiRequest::PrepareEnclosure(
                prepare_enclosure_request(),
            ))
            .expect("prepare request handled");
        let response = handler
            .handle(DaemonApiRequest::JobStatus(DaemonJobStatusRequest {
                job_id: DaemonJobId::new("enclosure-prepare-2026-07-08t19-40-00z").expect("job id"),
            }))
            .expect("status request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::JobStatus(DaemonJobStatusResponse {
                job: DaemonJobSummary {
                    kind: DaemonJobKind::EnclosurePreparation,
                    state: DaemonJobState::Complete,
                    ..
                }
            })
        ));

        cleanup(&root);
    }

    #[test]
    fn records_accepted_create_object_store_job_in_registry() {
        let root = temp_root("record-create-objectstore");
        let registry = Arc::new(FileBackedAdminJobRegistry::new(admin_job_registry_path(
            &root,
        )));
        let service = FakeService::default();
        let handler = DaemonRequestHandler::new_with_admin_job_registry(
            service,
            FixedDaemonClock::new("2026-07-08T20:45:00Z"),
            registry,
        );

        handler
            .handle(DaemonApiRequest::CreateObjectStore(
                create_object_store_request(),
            ))
            .expect("create objectstore request handled");
        let response = handler
            .handle(DaemonApiRequest::JobStatus(DaemonJobStatusRequest {
                job_id: DaemonJobId::new("objectstore-create-2026-07-08t20-45-00z")
                    .expect("job id"),
            }))
            .expect("status request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::JobStatus(DaemonJobStatusResponse {
                job: DaemonJobSummary {
                    kind: DaemonJobKind::ObjectStoreCreation,
                    state: DaemonJobState::Complete,
                    ..
                }
            })
        ));

        cleanup(&root);
    }

    #[test]
    fn registry_cancel_reports_completed_prepare_job_not_cancelled() {
        let root = temp_root("cancel-complete");
        let registry = Arc::new(FileBackedAdminJobRegistry::new(admin_job_registry_path(
            &root,
        )));
        let service = FakeService::default();
        let handler = DaemonRequestHandler::new_with_admin_job_registry(
            service,
            FixedDaemonClock::new("2026-07-08T19:40:00Z"),
            registry,
        );

        handler
            .handle(DaemonApiRequest::PrepareEnclosure(
                prepare_enclosure_request(),
            ))
            .expect("prepare request handled");
        let response = handler
            .handle(DaemonApiRequest::CancelJob(DaemonJobCancelRequest {
                job_id: DaemonJobId::new("enclosure-prepare-2026-07-08t19-40-00z").expect("job id"),
                reason: Some("operator requested cancellation".to_string()),
            }))
            .expect("cancel request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::CancelJob(DaemonJobCancelResponse {
                accepted: false,
                state: DaemonJobState::Complete,
                ..
            })
        ));

        cleanup(&root);
    }

    #[test]
    fn dispatches_job_status_to_orchestrator() {
        let service = FakeService::default();
        let handler =
            DaemonRequestHandler::new(service, FixedDaemonClock::new("2026-07-08T20:05:00Z"));

        let response = handler
            .handle(DaemonApiRequest::JobStatus(DaemonJobStatusRequest {
                job_id: DaemonJobId::new("enclosure-prepare-1").expect("job id"),
            }))
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::JobStatus(DaemonJobStatusResponse {
                job: DaemonJobSummary {
                    kind: DaemonJobKind::EnclosurePreparation,
                    state: DaemonJobState::Running,
                    ..
                }
            })
        ));
        assert_eq!(
            handler
                .service_orchestrator
                .job_status_calls
                .borrow()
                .as_slice(),
            &["enclosure-prepare-1".to_string()]
        );
    }

    #[test]
    fn dispatches_cancel_job_with_clock_timestamp() {
        let service = FakeService::default();
        let handler =
            DaemonRequestHandler::new(service, FixedDaemonClock::new("2026-07-08T20:06:00Z"));

        let response = handler
            .handle(DaemonApiRequest::CancelJob(DaemonJobCancelRequest {
                job_id: DaemonJobId::new("enclosure-prepare-1").expect("job id"),
                reason: Some("operator requested cancellation".to_string()),
            }))
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::CancelJob(DaemonJobCancelResponse {
                accepted: true,
                state: DaemonJobState::Cancelled,
                ..
            })
        ));
        assert_eq!(
            handler
                .service_orchestrator
                .cancel_job_calls
                .borrow()
                .as_slice(),
            &[(
                "2026-07-08T20:06:00Z".to_string(),
                "enclosure-prepare-1".to_string(),
            )]
        );
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

    fn prepare_enclosure_request() -> PrepareEnclosureRequest {
        PrepareEnclosureRequest {
            ssd_device: "/dev/disk/by-id/nvme-ssd".into(),
            hdd_devices: vec![PrepareEnclosureHddDevice {
                disk_id: "qnap-1057".to_string(),
                device_path: "/dev/disk/by-id/usb-qnap-1057".into(),
            }],
            mount_root: "/srv/dasobjectstore".into(),
            filesystem: PrepareEnclosureFilesystem::Ext4,
            owner: Some("stephen".to_string()),
            dry_run: true,
            client_request_id: Some("request-prepare-1".to_string()),
            administrator_actor: Some("operator".to_string()),
            allow_format: true,
            existing_data_acknowledged: true,
            confirmation_marker: ENCLOSURE_PREPARE_CONFIRMATION.to_string(),
        }
    }

    fn create_object_store_request() -> CreateObjectStoreRequest {
        CreateObjectStoreRequest {
            store_id: "generated-data".to_string(),
            store_class: "generated_data".to_string(),
            required_copies: 2,
            bucket: Some("generated-data".to_string()),
            writer_group: "bioinformatics".to_string(),
            ssd_root: "/srv/dasobjectstore/ssd".into(),
            object_type: "pod5".to_string(),
            enclosure_id: Some("qnap-tl-d800c-01".to_string()),
            public: false,
            writeable: true,
            capacity_behavior: "balanced".to_string(),
            retention: "standard".to_string(),
            endpoint_export_mode: "s3_bucket".to_string(),
            dry_run: true,
            client_request_id: Some("request-store-1".to_string()),
            administrator_actor: Some("operator".to_string()),
            confirmation_marker: OBJECT_STORE_CREATE_CONFIRMATION.to_string(),
        }
    }

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "dasobjectstore-request-handler-{label}-{}",
            std::process::id()
        ))
    }

    fn cleanup(root: &PathBuf) {
        let _ = fs::remove_dir_all(root);
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
        prepare_enclosure_calls: RefCell<Vec<(String, String, bool)>>,
        create_object_store_calls: RefCell<Vec<(String, String, bool)>>,
        job_status_calls: RefCell<Vec<String>>,
        cancel_job_calls: RefCell<Vec<(String, String)>>,
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
            emit_progress: &mut dyn FnMut(
                crate::api::DaemonIngestProgressEvent,
            ) -> Result<(), DaemonIngestFilesRuntimeError>,
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
                source_bytes_done: Some(0),
                source_bytes_total: Some(0),
                stage_bytes_done: Some(0),
                stage_bytes_total: Some(0),
                files_done: 0,
                files_total: Some(0),
                current_object_id: None,
                ssd_pressure: None,
                telemetry: None,
                resource_policy: None,
                message: Some("queued".to_string()),
            })?;
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

        fn prepare_enclosure(
            &self,
            request: PrepareEnclosureRequest,
            accepted_at_utc: &str,
        ) -> Result<PrepareEnclosureResponse, DaemonServiceRuntimeError> {
            self.prepare_enclosure_calls.borrow_mut().push((
                accepted_at_utc.to_string(),
                request.ssd_device.display().to_string(),
                request.dry_run,
            ));
            Ok(PrepareEnclosureResponse::accepted(
                DaemonJobId::new("enclosure-prepare-2026-07-08t19-40-00z").expect("job id"),
                accepted_at_utc,
                request.dry_run,
                request.ssd_device,
                request.hdd_devices,
                request.mount_root,
                request.filesystem,
                request.owner,
                request.administrator_actor,
            ))
        }

        fn create_object_store(
            &self,
            request: CreateObjectStoreRequest,
            accepted_at_utc: &str,
        ) -> Result<CreateObjectStoreResponse, DaemonServiceRuntimeError> {
            self.create_object_store_calls.borrow_mut().push((
                accepted_at_utc.to_string(),
                request.store_id.clone(),
                request.dry_run,
            ));
            Ok(CreateObjectStoreResponse::accepted(
                DaemonJobId::new("objectstore-create-2026-07-08t20-45-00z").expect("job id"),
                accepted_at_utc,
                request,
            ))
        }

        fn job_status(
            &self,
            request: DaemonJobStatusRequest,
        ) -> Result<DaemonJobStatusResponse, DaemonServiceRuntimeError> {
            self.job_status_calls
                .borrow_mut()
                .push(request.job_id.to_string());
            Ok(DaemonJobStatusResponse {
                job: DaemonJobSummary {
                    job_id: request.job_id,
                    kind: DaemonJobKind::EnclosurePreparation,
                    state: DaemonJobState::Running,
                    progress: DaemonJobProgress {
                        stage: "formatting".to_string(),
                        work_bytes_done: 5,
                        work_bytes_total: 10,
                        work_units_done: 1,
                        work_units_total: 2,
                        message: Some("formatting selected devices".to_string()),
                    },
                    submitted_at_utc: "2026-07-08T20:05:00Z".to_string(),
                    updated_at_utc: "2026-07-08T20:05:10Z".to_string(),
                    actor: Some("operator".to_string()),
                    failure_message: None,
                },
            })
        }

        fn cancel_job(
            &self,
            request: DaemonJobCancelRequest,
            accepted_at_utc: &str,
        ) -> Result<DaemonJobCancelResponse, DaemonServiceRuntimeError> {
            self.cancel_job_calls
                .borrow_mut()
                .push((accepted_at_utc.to_string(), request.job_id.to_string()));
            Ok(DaemonJobCancelResponse {
                job_id: request.job_id,
                accepted: true,
                state: DaemonJobState::Cancelled,
            })
        }
    }
}
