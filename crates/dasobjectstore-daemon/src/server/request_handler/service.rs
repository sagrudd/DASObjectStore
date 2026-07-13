use super::*;

/// Handles daemon service, provisioning, local administration, and job requests.
pub(super) fn request<S, C>(
    handler: &DaemonRequestHandler<S, C>,
    request: DaemonApiRequest,
    actor: Option<&DaemonLocalActor>,
) -> Result<DaemonApiResponse, DaemonRequestHandlerError>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    match request {
        DaemonApiRequest::ProfileCapabilities(request) => {
            Ok(DaemonApiResponse::ProfileCapabilities(
                crate::api::discover_profile_capabilities(&request),
            ))
        }
        DaemonApiRequest::ServiceStatus(request) => handler
            .service_orchestrator
            .status(request)
            .map(DaemonApiResponse::ServiceStatus)
            .map_err(DaemonRequestHandlerError::ServiceRuntime),
        DaemonApiRequest::ServiceLifecycle(request) => {
            let now = handler.clock.now_utc();
            let response = handler
                .service_orchestrator
                .lifecycle(request, &now)
                .map_err(DaemonRequestHandlerError::ServiceRuntime)?;
            handler.record_admin_job(daemon_job_summary_from_service_lifecycle(&response))?;
            Ok(DaemonApiResponse::ServiceLifecycle(response))
        }
        DaemonApiRequest::ServiceProvision(request) => {
            let now = handler.clock.now_utc();
            let response = handler
                .service_orchestrator
                .provision(request, &now)
                .map_err(DaemonRequestHandlerError::ServiceRuntime)?;
            handler.record_admin_job(daemon_job_summary_from_service_provision(&response))?;
            Ok(DaemonApiResponse::ServiceProvision(response))
        }
        DaemonApiRequest::PrepareEnclosure(request) => {
            let now = handler.clock.now_utc();
            let response = handler
                .service_orchestrator
                .prepare_enclosure(request, &now)
                .map_err(DaemonRequestHandlerError::ServiceRuntime)?;
            handler.record_admin_job(daemon_job_summary_from_prepare_enclosure(&response))?;
            Ok(DaemonApiResponse::PrepareEnclosure(response))
        }
        DaemonApiRequest::CreateObjectStore(request) => {
            let now = handler.clock.now_utc();
            let response = handler
                .service_orchestrator
                .create_object_store(request, &now)
                .map_err(DaemonRequestHandlerError::ServiceRuntime)?;
            handler.record_admin_job(daemon_job_summary_from_create_object_store(&response))?;
            Ok(DaemonApiResponse::CreateObjectStore(response))
        }
        DaemonApiRequest::RegisterProfileBinding(mut request) => {
            // Profile creation/adoption mutates daemon-owned storage state and
            // therefore requires a peer-authenticated local administrator.  Do
            // not trust the actor name carried in the request: it is only
            // confirmation metadata and may be spoofed by a client.  Dry-run
            // validation remains available without authentication, but never
            // echoes an untrusted request actor in its response/job metadata.
            request.administrator_actor = actor.map(DaemonLocalActor::display_name);
            if !request.dry_run {
                let Some(actor) = actor else {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "administrator_authentication_required",
                        "profile binding requires an authenticated local administrator",
                    )));
                };
                if !actor.is_administrator() {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "administrator_authorization_required",
                        "profile binding requires root, sudo, or dasobjectstore-admin membership",
                    )));
                }
            }
            let now = handler.clock.now_utc();
            let store_definition = request.store_definition.clone();
            let inspection = if !request.dry_run {
                request.validate().map_err(|error| {
                    DaemonRequestHandlerError::ServiceRuntime(
                        DaemonServiceRuntimeError::ObjectService(
                            ObjectServiceError::InvalidConfiguration(error.to_string()),
                        ),
                    )
                })?;
                validate_profile_binding_claim(
                    &handler.profile_binding_registry_path,
                    BackendProfileBinding {
                        manifest: request.manifest.clone(),
                        backend_root: request.backend_root.clone(),
                        ssd_staging_root: request.ssd_staging_root.clone(),
                    },
                )
                .map_err(DaemonRequestHandlerError::ServiceRuntime)?;
                let inspection = ensure_profile_backend(&request)
                    .map_err(DaemonRequestHandlerError::ServiceRuntime)?;
                handler
                    .service_orchestrator
                    .initialize_profile_capacity(
                        &request.manifest.store_id,
                        request.capacity.clone(),
                    )
                    .map_err(DaemonRequestHandlerError::ServiceRuntime)?;
                inspection
            } else {
                None
            };
            let mut response =
                register_profile_binding(request, &handler.profile_binding_registry_path, &now)
                    .map_err(DaemonRequestHandlerError::ServiceRuntime)?;
            if let Some(inspection) = inspection {
                response.unmanaged_path_count = inspection.unmanaged_paths.len();
                response.unsafe_path_count = inspection.unsafe_paths.len();
            }
            if let Some(definition) = store_definition {
                if !response.accepted.dry_run {
                    upsert_store_definition(&handler.store_registry_path, definition).map_err(
                        |error| {
                            DaemonRequestHandlerError::ServiceRuntime(
                                DaemonServiceRuntimeError::ObjectService(error),
                            )
                        },
                    )?;
                }
                response.store_definition_published = !response.accepted.dry_run;
            }
            handler.record_admin_job(daemon_job_summary_from_profile_binding(&response))?;
            Ok(DaemonApiResponse::RegisterProfileBinding(response))
        }
        DaemonApiRequest::UpsertEndpointInventory(request) => {
            let now = handler.clock.now_utc();
            let response = handler
                .service_orchestrator
                .upsert_endpoint_inventory(request, &now)
                .map_err(DaemonRequestHandlerError::ServiceRuntime)?;
            handler.record_admin_job(daemon_job_summary_from_endpoint_inventory(&response))?;
            Ok(DaemonApiResponse::UpsertEndpointInventory(response))
        }
        DaemonApiRequest::CreateLocalGroup(request) => {
            let now = handler.clock.now_utc();
            let response = handler
                .service_orchestrator
                .create_local_group(request, &now)
                .map_err(DaemonRequestHandlerError::LocalAdminRuntime)?;
            handler.record_admin_job(daemon_job_summary_from_local_admin(
                &response.accepted,
                response.administrator_actor.clone(),
            ))?;
            Ok(DaemonApiResponse::CreateLocalGroup(response))
        }
        DaemonApiRequest::AssignLocalUserToLocalGroup(request) => {
            let now = handler.clock.now_utc();
            let response = handler
                .service_orchestrator
                .assign_local_user_to_local_group(request, &now)
                .map_err(DaemonRequestHandlerError::LocalAdminRuntime)?;
            handler.record_admin_job(daemon_job_summary_from_local_admin(
                &response.accepted,
                response.administrator_actor.clone(),
            ))?;
            Ok(DaemonApiResponse::AssignLocalUserToLocalGroup(response))
        }
        DaemonApiRequest::JobList(request) => handler
            .admin_job_list(request)
            .map(DaemonApiResponse::JobList)
            .map_err(DaemonRequestHandlerError::ServiceRuntime),
        DaemonApiRequest::JobStatus(request) => handler
            .admin_job_status(request)
            .map(DaemonApiResponse::JobStatus)
            .map_err(DaemonRequestHandlerError::ServiceRuntime),
        DaemonApiRequest::CancelJob(request) => handler
            .cancel_admin_job(request, &handler.clock.now_utc())
            .map(DaemonApiResponse::CancelJob)
            .map_err(DaemonRequestHandlerError::ServiceRuntime),
        _ => unreachable!("service dispatcher received a non-service request"),
    }
}
