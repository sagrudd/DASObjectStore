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
        DaemonApiRequest::ExchangeApplicationAccessToken(request) => {
            let now = handler.clock.now_utc();
            let exchange = request.exchange;
            let exchange_key_id = exchange.key_id.clone();
            let identity = read_application_identity(
                &handler.application_identity_registry_path,
                &exchange.application_id,
            )
            .map_err(DaemonRequestHandlerError::ServiceRuntime)?
            .ok_or_else(|| {
                DaemonRequestHandlerError::ServiceRuntime(
                    DaemonServiceRuntimeError::UnsupportedOperation {
                        operation: "application identity is not registered".to_string(),
                    },
                )
            })?;
            let key = read_application_key(
                &handler.application_key_registry_path,
                &exchange.application_id,
                &exchange.key_id,
            )
            .map_err(DaemonRequestHandlerError::ServiceRuntime)?
            .ok_or_else(|| {
                DaemonRequestHandlerError::ServiceRuntime(
                    DaemonServiceRuntimeError::UnsupportedOperation {
                        operation: "application exchange key is not registered".to_string(),
                    },
                )
            })?;
            let token_id = stable_easyconnect_id(
                "application-access-token",
                &exchange.application_id,
                &format!(
                    "{}:{}:{}",
                    exchange.key_id,
                    exchange.requested_issued_at_unix_seconds,
                    exchange.requested_expires_at_unix_seconds
                ),
            );
            let claims = exchange
                .issue_access_token(
                    &identity,
                    &key,
                    token_id,
                    &crate::application_token_verifier::RingApplicationExchangeProofVerifier,
                )
                .map_err(|error| {
                    DaemonRequestHandlerError::ServiceRuntime(
                        DaemonServiceRuntimeError::UnsupportedOperation {
                            operation: format!(
                                "application access-token exchange rejected: {error}"
                            ),
                        },
                    )
                })?;
            record_application_audit_event(
                &handler.application_audit_log_path,
                &now,
                "issue_access_token",
                &claims.application_id,
                Some(&exchange_key_id),
                None,
                "application access-token exchange",
                false,
            )
            .map_err(DaemonRequestHandlerError::ServiceRuntime)?;
            Ok(DaemonApiResponse::ExchangeApplicationAccessToken(
                ApplicationAccessTokenExchangeResponse { claims },
            ))
        }
        DaemonApiRequest::RegisterApplicationIdentity(mut request) => {
            // Identity registration mutates daemon-owned authority and is
            // therefore administrator-only. Dry-run validates policy without
            // requiring a mutation authority or persisting metadata.
            request.administrator_actor = actor.map(DaemonLocalActor::display_name);
            if !request.dry_run {
                let Some(actor) = actor else {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "administrator_authentication_required",
                        "application identity registration requires an authenticated local administrator",
                    )));
                };
                if !actor.is_administrator() {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "administrator_authorization_required",
                        "application identity registration requires root, sudo, or dasobjectstore-admin membership",
                    )));
                }
            }
            let now = handler.clock.now_utc();
            let application_id = request.identity.application_id.clone();
            let replaced = read_application_identity(
                &handler.application_identity_registry_path,
                &application_id,
            )
            .map_err(DaemonRequestHandlerError::ServiceRuntime)?
            .is_some();
            if !request.dry_run {
                upsert_application_identity(
                    &handler.application_identity_registry_path,
                    request.identity.clone(),
                )
                .map_err(DaemonRequestHandlerError::ServiceRuntime)?;
            }
            record_application_audit_event(
                &handler.application_audit_log_path,
                &now,
                "register_identity",
                &application_id,
                None,
                request.administrator_actor.as_deref(),
                "application identity registration",
                request.dry_run,
            )
            .map_err(DaemonRequestHandlerError::ServiceRuntime)?;
            let job_id_value = format!(
                "application-identity-{}",
                now.chars()
                    .map(|character| if character.is_ascii_alphanumeric() {
                        character
                    } else {
                        '-'
                    })
                    .collect::<String>()
                    .trim_matches('-')
                    .to_ascii_lowercase()
            );
            let job_id = DaemonJobId::new(job_id_value.clone()).map_err(|_| {
                DaemonRequestHandlerError::ServiceRuntime(DaemonServiceRuntimeError::InvalidJobId(
                    job_id_value,
                ))
            })?;
            let response =
                ApplicationIdentityRegistrationResponse::accepted(job_id, now, request, replaced);
            handler.record_admin_job(daemon_job_summary_from_application_identity_registration(
                &response,
            ))?;
            Ok(DaemonApiResponse::RegisterApplicationIdentity(response))
        }
        DaemonApiRequest::RegisterApplicationKey(mut request) => {
            request.administrator_actor = actor.map(DaemonLocalActor::display_name);
            if !request.dry_run {
                let Some(actor) = actor else {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "administrator_authentication_required",
                        "application key registration requires an authenticated local administrator",
                    )));
                };
                if !actor.is_administrator() {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "administrator_authorization_required",
                        "application key registration requires root, sudo, or dasobjectstore-admin membership",
                    )));
                }
            }
            let now = handler.clock.now_utc();
            let application_id = request.key.application_id.clone();
            let key_id = request.key.key_id.clone();
            let replaced = read_application_key(
                &handler.application_key_registry_path,
                &application_id,
                &key_id,
            )
            .map_err(DaemonRequestHandlerError::ServiceRuntime)?
            .is_some();
            if !request.dry_run {
                upsert_application_key(&handler.application_key_registry_path, request.key.clone())
                    .map_err(DaemonRequestHandlerError::ServiceRuntime)?;
            }
            record_application_audit_event(
                &handler.application_audit_log_path,
                &now,
                "register_key",
                &application_id,
                Some(&key_id),
                request.administrator_actor.as_deref(),
                "application key registration",
                request.dry_run,
            )
            .map_err(DaemonRequestHandlerError::ServiceRuntime)?;
            let job_id_value = format!(
                "application-key-{}",
                now.chars()
                    .map(|character| if character.is_ascii_alphanumeric() {
                        character
                    } else {
                        '-'
                    })
                    .collect::<String>()
                    .trim_matches('-')
                    .to_ascii_lowercase()
            );
            let job_id = DaemonJobId::new(job_id_value.clone()).map_err(|_| {
                DaemonRequestHandlerError::ServiceRuntime(DaemonServiceRuntimeError::InvalidJobId(
                    job_id_value,
                ))
            })?;
            let response =
                ApplicationKeyRegistrationResponse::accepted(job_id, now, request, replaced);
            handler.record_admin_job(daemon_job_summary_from_application_key_registration(
                &response,
            ))?;
            Ok(DaemonApiResponse::RegisterApplicationKey(response))
        }
        DaemonApiRequest::RevokeApplicationCredential(mut request) => {
            request.administrator_actor = actor.map(DaemonLocalActor::display_name);
            if !request.dry_run {
                let Some(actor) = actor else {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "administrator_authentication_required",
                        "application credential revocation requires an authenticated local administrator",
                    )));
                };
                if !actor.is_administrator() {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "administrator_authorization_required",
                        "application credential revocation requires root, sudo, or dasobjectstore-admin membership",
                    )));
                }
            }
            let now = handler.clock.now_utc();
            let revoked = if let Some(key_id) = request.key_id.as_deref() {
                if request.dry_run {
                    read_application_key(
                        &handler.application_key_registry_path,
                        &request.application_id,
                        key_id,
                    )
                    .map_err(DaemonRequestHandlerError::ServiceRuntime)?
                    .is_some()
                } else {
                    deactivate_application_key(
                        &handler.application_key_registry_path,
                        &request.application_id,
                        key_id,
                    )
                    .map_err(DaemonRequestHandlerError::ServiceRuntime)?
                }
            } else if request.dry_run {
                read_application_identity(
                    &handler.application_identity_registry_path,
                    &request.application_id,
                )
                .map_err(DaemonRequestHandlerError::ServiceRuntime)?
                .is_some()
            } else {
                deactivate_application_identity(
                    &handler.application_identity_registry_path,
                    &request.application_id,
                )
                .map_err(DaemonRequestHandlerError::ServiceRuntime)?
            };
            record_application_audit_event(
                &handler.application_audit_log_path,
                &now,
                "revoke_credential",
                &request.application_id,
                request.key_id.as_deref(),
                request.administrator_actor.as_deref(),
                &request.reason,
                request.dry_run,
            )
            .map_err(DaemonRequestHandlerError::ServiceRuntime)?;
            let job_id_value = format!(
                "application-revocation-{}",
                now.chars()
                    .map(|character| if character.is_ascii_alphanumeric() {
                        character
                    } else {
                        '-'
                    })
                    .collect::<String>()
                    .trim_matches('-')
                    .to_ascii_lowercase()
            );
            let job_id = DaemonJobId::new(job_id_value.clone()).map_err(|_| {
                DaemonRequestHandlerError::ServiceRuntime(DaemonServiceRuntimeError::InvalidJobId(
                    job_id_value,
                ))
            })?;
            let response =
                ApplicationCredentialRevocationResponse::accepted(job_id, now, request, revoked);
            handler.record_admin_job(daemon_job_summary_from_application_credential_revocation(
                &response,
            ))?;
            Ok(DaemonApiResponse::RevokeApplicationCredential(response))
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
                let inspection =
                    ensure_profile_backend(&request, &handler.profile_binding_registry_path)
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
                response.unmanaged_path_count = inspection.inspection.unmanaged_paths.len();
                response.unsafe_path_count = inspection.inspection.unsafe_paths.len();
                response.adopted_object_count = inspection.adopted_object_count;
                response.adopted_bytes = inspection.adopted_bytes;
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
        DaemonApiRequest::ProfileInspection(request) => {
            let Some(actor) = actor else {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_inspection_authentication_required",
                    "profile inspection requires an authenticated daemon actor",
                )));
            };
            if !actor.is_administrator()
                && handler
                    .authorize_endpoint_read(Some(actor), &request.store_id)
                    .is_err()
            {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_inspection_authorization_required",
                    "profile inspection requires administrator authority or store read access",
                )));
            }
            let binding = match read_profile_binding_record(
                &handler.profile_binding_registry_path,
                request.store_id.as_str(),
            ) {
                Ok(Some(binding)) => binding,
                Ok(None) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_binding_not_found",
                        "no persisted profile binding exists for this ObjectStore",
                    )))
                }
                Err(_) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_inspection_unavailable",
                        "the persisted profile binding could not be inspected",
                    )))
                }
            };
            let mut response = ProfileInspectionResponse {
                schema_version: crate::api::PROFILE_INSPECTION_SCHEMA_VERSION.to_string(),
                store_id: binding.manifest.store_id.clone(),
                deployment_profile: binding.manifest.deployment_profile,
                host_mode: binding.manifest.host_mode,
                protection: binding.manifest.protection,
                root_state: ProfileInspectionRootState::Available,
                unmanaged_path_count: 0,
                unsafe_path_count: 0,
                warnings: Vec::new(),
            };
            match fs::symlink_metadata(&binding.backend_root) {
                Ok(metadata) if !metadata.is_dir() => {
                    response.root_state = ProfileInspectionRootState::NotDirectory;
                }
                Ok(_) if binding.manifest.deployment_profile == DeploymentProfile::Folder => {
                    match FolderBackend::inspect_user_tree_at(&binding.backend_root) {
                        Ok(report) => {
                            response.unmanaged_path_count = report.unmanaged_paths.len();
                            response.unsafe_path_count = report.unsafe_paths.len();
                        }
                        Err(_) => {
                            response.root_state = ProfileInspectionRootState::Unreadable;
                            response.warnings.push(
                                "folder drift could not be read without changing the managed namespace"
                                    .to_string(),
                            );
                        }
                    }
                }
                Ok(_) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {
                    response.root_state = ProfileInspectionRootState::Missing;
                }
                Err(_) => {
                    response.root_state = ProfileInspectionRootState::Unreadable;
                }
            }
            response.validate().map_err(|error| {
                DaemonRequestHandlerError::ServiceRuntime(
                    DaemonServiceRuntimeError::UnsupportedOperation { operation: error },
                )
            })?;
            Ok(DaemonApiResponse::ProfileInspection(response))
        }
        DaemonApiRequest::ProfileReadiness(request) => {
            let Some(actor) = actor else {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_readiness_authentication_required",
                    "profile readiness requires an authenticated daemon actor",
                )));
            };
            if !actor.is_administrator()
                && handler
                    .authorize_endpoint_read(Some(actor), &request.store_id)
                    .is_err()
            {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_readiness_authorization_required",
                    "profile readiness requires administrator authority or store read access",
                )));
            }
            let binding = match read_profile_binding_record(
                &handler.profile_binding_registry_path,
                request.store_id.as_str(),
            ) {
                Ok(Some(binding)) => binding,
                Ok(None) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_binding_not_found",
                        "no persisted profile binding exists for this ObjectStore",
                    )))
                }
                Err(_) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_readiness_unavailable",
                        "the persisted profile binding could not be inspected",
                    )))
                }
            };
            let mut root_state = ProfileInspectionRootState::Available;
            let mut reasons = Vec::new();
            match fs::symlink_metadata(&binding.backend_root) {
                Ok(metadata) if !metadata.is_dir() => {
                    root_state = ProfileInspectionRootState::NotDirectory;
                    reasons.push("profile backend root is not a directory".to_string());
                }
                Ok(_) if binding.manifest.deployment_profile == DeploymentProfile::Folder => {
                    match FolderBackend::inspect_user_tree_at(&binding.backend_root) {
                        Ok(report) => {
                            if !report.unmanaged_paths.is_empty() {
                                reasons.push(format!(
                                    "{} unmanaged path(s) require explicit adoption",
                                    report.unmanaged_paths.len()
                                ));
                            }
                            if !report.unsafe_paths.is_empty() {
                                reasons.push(format!(
                                    "{} unsafe path(s) block profile readiness",
                                    report.unsafe_paths.len()
                                ));
                            }
                        }
                        Err(_) => {
                            root_state = ProfileInspectionRootState::Unreadable;
                            reasons.push(
                                "folder drift could not be read without changing the managed namespace"
                                    .to_string(),
                            );
                        }
                    }
                }
                Ok(_) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {
                    root_state = ProfileInspectionRootState::Missing;
                    reasons.push("profile backend root is missing".to_string());
                }
                Err(_) => {
                    root_state = ProfileInspectionRootState::Unreadable;
                    reasons.push("profile backend root is unreadable".to_string());
                }
            }
            let capacity = handler
                .service_orchestrator
                .capacity_status(crate::api::CapacityStatusRequest {
                    store_id: request.store_id.to_string(),
                })
                .ok();
            match &capacity {
                Some(status) => {
                    if status.admission_block_reason.is_some() {
                        reasons.push("capacity admission is currently blocked".to_string());
                    }
                }
                None => reasons.push("capacity status is unavailable".to_string()),
            }
            let response = ProfileReadinessResponse {
                schema_version: crate::api::PROFILE_READINESS_SCHEMA_VERSION.to_string(),
                store_id: binding.manifest.store_id.clone(),
                deployment_profile: binding.manifest.deployment_profile,
                host_mode: binding.manifest.host_mode,
                protection: binding.manifest.protection,
                root_state,
                ready: reasons.is_empty(),
                reasons,
                capacity,
            };
            response.validate().map_err(|error| {
                DaemonRequestHandlerError::ServiceRuntime(
                    DaemonServiceRuntimeError::UnsupportedOperation { operation: error },
                )
            })?;
            Ok(DaemonApiResponse::ProfileReadiness(response))
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
