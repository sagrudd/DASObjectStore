use super::storage_helpers::{
    delete_store_definition_maybe, delete_subobjects_for_store_maybe, known_ssd_root,
    parse_disk_copy_roots,
};
use super::storage_reconciliation::{
    emit_reconciliation_progress, reconciliation_job_summary, reconciliation_registration_report,
};
use super::*;
#[path = "storage_control.rs"]
mod storage_control;
#[path = "storage_operations.rs"]
mod storage_operations;
pub(super) fn request<S, C>(
    handler: &DaemonRequestHandler<S, C>,
    request: DaemonApiRequest,
    actor: Option<&DaemonLocalActor>,
    emit_progress: &mut impl FnMut(
        DaemonIngestProgressEvent,
    ) -> Result<(), DaemonIngestFilesRuntimeError>,
) -> Result<DaemonApiResponse, DaemonRequestHandlerError>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    match request {
        DaemonApiRequest::DiskRetire(request) => {
            match handler.disk_retire_for_actor(request, actor) {
                Ok(response) => Ok(DaemonApiResponse::DiskRetire(response)),
                Err((code, message)) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    code, message,
                ))),
            }
        }
        DaemonApiRequest::DiskForceRetire(request) => {
            match handler.disk_force_retire_for_actor(request, actor) {
                Ok(response) => Ok(DaemonApiResponse::DiskForceRetire(response)),
                Err((code, message)) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    code, message,
                ))),
            }
        }
        DaemonApiRequest::StoreInventory(request) => {
            match handler.store_inventory_for_actor(request, actor) {
                Ok(response) => Ok(DaemonApiResponse::StoreInventory(response)),
                Err(error) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "store_inventory_failed",
                    error.to_string(),
                ))),
            }
        }
        DaemonApiRequest::CapacityAdmission(request) => {
            let store_id = match StoreId::new(request.store_id.clone()) {
                Ok(store_id) => store_id,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "invalid_store_id",
                        error.to_string(),
                    )));
                }
            };
            if let Err(error) = handler.authorize_endpoint_read(actor, &store_id) {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    error.code(),
                    error.to_string(),
                )));
            }
            match handler.service_orchestrator.capacity_admission(request) {
                Ok(response) => Ok(DaemonApiResponse::CapacityAdmission(response)),
                Err(error) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "capacity_admission_unavailable",
                    error.to_string(),
                ))),
            }
        }
        DaemonApiRequest::CapacityStatus(request) => {
            let store_id = match StoreId::new(request.store_id.clone()) {
                Ok(store_id) => store_id,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "invalid_store_id",
                        error.to_string(),
                    )));
                }
            };
            if let Err(error) = handler.authorize_endpoint_read(actor, &store_id) {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    error.code(),
                    error.to_string(),
                )));
            }
            match handler.service_orchestrator.capacity_status(request) {
                Ok(response) => Ok(DaemonApiResponse::CapacityStatus(response)),
                Err(error) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "capacity_status_unavailable",
                    error.to_string(),
                ))),
            }
        }
        DaemonApiRequest::StoreDrain(request) => {
            match handler.store_drain_for_actor(request, actor) {
                Ok(response) => Ok(DaemonApiResponse::StoreDrain(response)),
                Err((code, message)) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    code, message,
                ))),
            }
        }
        DaemonApiRequest::StoreDelete(request) => {
            match handler.store_delete_for_actor(request, actor) {
                Ok(response) => Ok(DaemonApiResponse::StoreDelete(response)),
                Err((code, message)) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    code, message,
                ))),
            }
        }
        DaemonApiRequest::StoreRepair(request) => {
            match handler.store_repair_for_actor(request, actor, emit_progress) {
                Ok(response) => Ok(DaemonApiResponse::StoreRepair(response)),
                Err((code, message)) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    code, message,
                ))),
            }
        }
        DaemonApiRequest::StoreVerify(request) => match handler.store_verify_for_actor(request) {
            Ok(response) => Ok(DaemonApiResponse::StoreVerify(response)),
            Err((code, message)) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                code, message,
            ))),
        },
        DaemonApiRequest::StoreDeduplicate(request) => {
            match handler.store_deduplicate_for_actor(request, actor) {
                Ok(response) => Ok(DaemonApiResponse::StoreDeduplicate(response)),
                Err((code, message)) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    code, message,
                ))),
            }
        }
        DaemonApiRequest::ObjectPut(request) => {
            match handler.object_put_for_actor(request, actor) {
                Ok(response) => Ok(DaemonApiResponse::ObjectPut(response)),
                Err((code, message)) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    code, message,
                ))),
            }
        }
        DaemonApiRequest::IngestControl(request) => Ok(storage_control::response(request, actor)),
        DaemonApiRequest::IngestQueueDrain(request) => {
            match handler.ingest_queue_drain_for_actor(request, actor) {
                Ok(response) => Ok(DaemonApiResponse::IngestQueueDrain(response)),
                Err((code, message)) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    code, message,
                ))),
            }
        }
        DaemonApiRequest::UpdateObjectStoreIngestPolicy(mut request) => {
            let Some(actor) = actor else {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "administrator_authentication_required",
                    "object-store ingest policy updates require an authenticated local administrator",
                )));
            };
            let trusted_web_peer = actor.username.as_deref() == Some(DEFAULT_DAEMON_SERVICE_USER)
                && request
                    .administrator_actor
                    .as_deref()
                    .is_some_and(|value| !value.trim().is_empty());
            if !actor.is_administrator() && !trusted_web_peer {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "administrator_authorization_required",
                    "object-store ingest policy updates require root, sudo, dasobjectstore-admin membership, or the trusted authenticated Web service peer",
                )));
            }
            if actor.is_administrator() {
                request.administrator_actor = Some(actor.display_name());
            }
            let now = handler.clock.now_utc();
            let response = match handler.update_object_store_ingest_policy(request, &now) {
                Ok(response) => response,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "store_policy_update_failed",
                        error.to_string(),
                    )))
                }
            };
            handler.record_admin_job(daemon_job_summary_from_update_object_store_ingest_policy(
                &response,
            ))?;
            Ok(DaemonApiResponse::UpdateObjectStoreIngestPolicy(response))
        }
        DaemonApiRequest::ApplianceTelemetry(request) => {
            match handler.appliance_telemetry_for_actor(request, actor) {
                Ok(response) => Ok(DaemonApiResponse::ApplianceTelemetry(response)),
                Err(error) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    error.code(),
                    error.to_string(),
                ))),
            }
        }
        DaemonApiRequest::SubmitIngestFiles(request) => {
            if let Some(actor) = actor {
                if let Err(error) = handler.authorize_ingest_files(actor, &request) {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        error.code(),
                        error.to_string(),
                    )));
                }
            }
            match handler.service_orchestrator.submit_ingest_files(
                request,
                &handler.clock.now_utc(),
                emit_progress,
            ) {
                Ok(response) => Ok(DaemonApiResponse::SubmitIngestFiles(response)),
                Err(error) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "ingest_files_failed",
                    error.to_string(),
                ))),
            }
        }
        DaemonApiRequest::ObjectBrowser(request) => {
            let delegated_actor = match handler
                .delegated_object_browser_actor(actor, request.delegated_actor.as_ref())
            {
                Ok(actor) => actor,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        error.code(),
                        error.to_string(),
                    )));
                }
            };
            let effective_actor = delegated_actor.as_ref().or(actor);
            let store_id = match handler.authorize_endpoint_read(effective_actor, &request.endpoint)
            {
                Ok(store_id) => store_id,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        error.code(),
                        error.to_string(),
                    )));
                }
            };
            let entries = match read_object_browser_metadata(&handler.live_sqlite_path, store_id) {
                Ok(entries) => entries,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "object_browser_metadata_failed",
                        error.to_string(),
                    )));
                }
            };
            query_object_browser_metadata(&request, &entries)
                .map(DaemonApiResponse::ObjectBrowser)
                .map_err(Into::into)
        }
        DaemonApiRequest::ProfileBrowser(request) => {
            let delegated_actor = match handler
                .delegated_object_browser_actor(actor, request.delegated_actor.as_ref())
            {
                Ok(actor) => actor,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        error.code(),
                        error.to_string(),
                    )));
                }
            };
            let effective_actor = delegated_actor.as_ref().or(actor);
            let store_id = match handler.authorize_endpoint_read(effective_actor, &request.store_id)
            {
                Ok(store_id) => store_id,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        error.code(),
                        error.to_string(),
                    )));
                }
            };
            let binding = match read_profile_binding(
                &handler.profile_binding_registry_path,
                store_id.as_str(),
            ) {
                Ok(Some(binding)) => binding,
                Ok(None) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_browser_unavailable",
                        "profile browser requires a registered bounded folder profile",
                    )));
                }
                Err(_) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_browser_unavailable",
                        "profile browser could not load the registered profile",
                    )));
                }
            };
            if binding.manifest.deployment_profile != DeploymentProfile::Folder {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_browser_unavailable",
                    "profile browser is available for bounded folder profiles only",
                )));
            }
            let catalogue_path = binding.backend_root.join(".dasobjectstore/catalogue.json");
            let catalogue = match FolderCatalogue::open_existing(&catalogue_path, store_id.as_str())
            {
                Ok(catalogue) => catalogue,
                Err(_) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_browser_unavailable",
                        "profile catalogue is unavailable",
                    )));
                }
            };
            let offset = match usize::try_from(request.offset) {
                Ok(offset) => offset,
                Err(_) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_browser_unavailable",
                        "profile browser offset is too large",
                    )));
                }
            };
            let query = FolderCatalogueBrowserQuery {
                prefix: request.prefix.clone(),
                search: request.search.clone(),
                offset,
                limit: usize::from(request.limit),
            };
            let entries = match catalogue.browser_entries(&query) {
                Ok(entries) => entries
                    .into_iter()
                    .map(|entry| ProfileBrowserEntry {
                        key: entry.key,
                        size_bytes: entry.size_bytes,
                        checksum: entry.checksum,
                    })
                    .collect(),
                Err(_) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_browser_unavailable",
                        "profile catalogue query failed",
                    )));
                }
            };
            let total_entries = match catalogue.browser_entry_count(&query) {
                Ok(total_entries) => total_entries,
                Err(_) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_browser_unavailable",
                        "profile catalogue count failed",
                    )));
                }
            };
            let next_offset = request
                .offset
                .checked_add(u64::from(request.limit))
                .filter(|next| *next < total_entries);
            Ok(DaemonApiResponse::ProfileBrowser(ProfileBrowserResponse {
                schema_version: PROFILE_BROWSER_SCHEMA_VERSION.to_string(),
                store_id,
                profile: DeploymentProfile::Folder,
                entries,
                next_offset,
                total_entries,
            }))
        }
        DaemonApiRequest::ProfileS3List(request) => {
            let store_id = match handler.authorize_endpoint_read(actor, &request.store_id) {
                Ok(store_id) => store_id,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        error.code(),
                        error.to_string(),
                    )));
                }
            };
            let binding = match read_profile_binding(
                &handler.profile_binding_registry_path,
                store_id.as_str(),
            ) {
                Ok(Some(binding)) => binding,
                Ok(None) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_s3_unavailable",
                        "profile S3 requires a registered bounded folder profile",
                    )));
                }
                Err(_) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_s3_unavailable",
                        "profile S3 could not load the registered profile",
                    )));
                }
            };
            if binding.manifest.deployment_profile != DeploymentProfile::Folder {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_s3_unavailable",
                    "profile S3 is available for bounded folder profiles only",
                )));
            }
            let capacity = match read_store_registry(&handler.store_registry_path) {
                Ok(definitions) => definitions
                    .into_iter()
                    .find(|definition| definition.store_id == store_id)
                    .map(|definition| definition.policy.capacity),
                Err(_) => None,
            };
            let Some(capacity) = capacity else {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_s3_unavailable",
                    "profile S3 capacity policy is unavailable",
                )));
            };
            let backend =
                match FolderBackend::open(binding.backend_root, binding.manifest, capacity, 0) {
                    Ok(backend) => backend,
                    Err(error) => {
                        return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                            "profile_s3_unavailable",
                            error.to_string(),
                        )));
                    }
                };
            let offset = match usize::try_from(request.offset) {
                Ok(offset) => offset,
                Err(_) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_s3_invalid_request",
                        "profile S3 list offset is too large",
                    )));
                }
            };
            let page = match list_profile_objects_page(
                &backend,
                request.prefix.as_deref(),
                offset,
                usize::from(request.limit),
            ) {
                Ok(page) => page,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_s3_list_failed",
                        error.to_string(),
                    )));
                }
            };
            Ok(DaemonApiResponse::ProfileS3List(profile_s3_list_response(
                store_id, page,
            )))
        }
        DaemonApiRequest::ProfileS3Head(request) => {
            let store_id = match handler.authorize_endpoint_read(actor, &request.store_id) {
                Ok(store_id) => store_id,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        error.code(),
                        error.to_string(),
                    )));
                }
            };
            let binding = match read_profile_binding(
                &handler.profile_binding_registry_path,
                store_id.as_str(),
            ) {
                Ok(Some(binding)) => binding,
                Ok(None) | Err(_) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_s3_unavailable",
                        "profile S3 requires a registered bounded folder profile",
                    )));
                }
            };
            if binding.manifest.deployment_profile != DeploymentProfile::Folder {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_s3_unavailable",
                    "profile S3 is available for bounded folder profiles only",
                )));
            }
            let capacity = match read_store_registry(&handler.store_registry_path) {
                Ok(definitions) => definitions
                    .into_iter()
                    .find(|definition| definition.store_id == store_id)
                    .map(|definition| definition.policy.capacity),
                Err(_) => None,
            };
            let Some(capacity) = capacity else {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_s3_unavailable",
                    "profile S3 capacity policy is unavailable",
                )));
            };
            let backend =
                match FolderBackend::open(binding.backend_root, binding.manifest, capacity, 0) {
                    Ok(backend) => backend,
                    Err(error) => {
                        return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                            "profile_s3_unavailable",
                            error.to_string(),
                        )));
                    }
                };
            let object = match head_profile_object(&backend, &request.key) {
                Ok(object) => object,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_s3_head_failed",
                        error.to_string(),
                    )));
                }
            };
            Ok(DaemonApiResponse::ProfileS3Head(ProfileS3HeadResponse {
                schema_version: PROFILE_S3_SCHEMA_VERSION.to_string(),
                store_id,
                object: ProfileS3ObjectView {
                    key: object.key,
                    size_bytes: object.size_bytes,
                    checksum: object.checksum,
                },
            }))
        }
        DaemonApiRequest::ProfileS3Verify(request) => {
            let store_id = match handler.authorize_endpoint_read(actor, &request.store_id) {
                Ok(store_id) => store_id,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        error.code(),
                        error.to_string(),
                    )));
                }
            };
            let binding = match read_profile_binding(
                &handler.profile_binding_registry_path,
                store_id.as_str(),
            ) {
                Ok(Some(binding)) => binding,
                Ok(None) | Err(_) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_s3_unavailable",
                        "profile S3 requires a registered bounded folder profile",
                    )));
                }
            };
            if binding.manifest.deployment_profile != DeploymentProfile::Folder {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_s3_unavailable",
                    "profile S3 is available for bounded folder profiles only",
                )));
            }
            let capacity = match read_store_registry(&handler.store_registry_path) {
                Ok(definitions) => definitions
                    .into_iter()
                    .find(|definition| definition.store_id == store_id)
                    .map(|definition| definition.policy.capacity),
                Err(_) => None,
            };
            let Some(capacity) = capacity else {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_s3_unavailable",
                    "profile S3 capacity policy is unavailable",
                )));
            };
            let backend =
                match FolderBackend::open(binding.backend_root, binding.manifest, capacity, 0) {
                    Ok(backend) => backend,
                    Err(error) => {
                        return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                            "profile_s3_unavailable",
                            error.to_string(),
                        )));
                    }
                };
            let object = match verify_profile_object(&backend, &request.key) {
                Ok(object) => object,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_s3_verify_failed",
                        error.to_string(),
                    )));
                }
            };
            Ok(DaemonApiResponse::ProfileS3Verify(
                ProfileS3VerifyResponse {
                    schema_version: PROFILE_S3_SCHEMA_VERSION.to_string(),
                    store_id,
                    object: ProfileS3ObjectView {
                        key: object.key,
                        size_bytes: object.size_bytes,
                        checksum: object.checksum,
                    },
                    verified: true,
                },
            ))
        }
        DaemonApiRequest::ProfileS3Health(request) => {
            let store_id = match handler.authorize_endpoint_read(actor, &request.store_id) {
                Ok(store_id) => store_id,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        error.code(),
                        error.to_string(),
                    )));
                }
            };
            let binding = match read_profile_binding(
                &handler.profile_binding_registry_path,
                store_id.as_str(),
            ) {
                Ok(Some(binding)) => binding,
                Ok(None) | Err(_) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_s3_unavailable",
                        "profile S3 requires a registered bounded folder profile",
                    )));
                }
            };
            if binding.manifest.deployment_profile != DeploymentProfile::Folder {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_s3_unavailable",
                    "profile S3 is available for bounded folder profiles only",
                )));
            }
            let capacity = match read_store_registry(&handler.store_registry_path) {
                Ok(definitions) => definitions
                    .into_iter()
                    .find(|definition| definition.store_id == store_id)
                    .map(|definition| definition.policy.capacity),
                Err(_) => None,
            };
            let Some(capacity) = capacity else {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_s3_unavailable",
                    "profile S3 capacity policy is unavailable",
                )));
            };
            let backend =
                match FolderBackend::open(binding.backend_root, binding.manifest, capacity, 0) {
                    Ok(backend) => backend,
                    Err(error) => {
                        return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                            "profile_s3_unavailable",
                            error.to_string(),
                        )));
                    }
                };
            let health = match profile_health(&backend) {
                Ok(health) => health,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_s3_health_failed",
                        error.to_string(),
                    )));
                }
            };
            Ok(DaemonApiResponse::ProfileS3Health(
                ProfileS3HealthResponse {
                    schema_version: PROFILE_S3_SCHEMA_VERSION.to_string(),
                    store_id,
                    health,
                },
            ))
        }
        DaemonApiRequest::ProfileDiagnostics(request) => {
            let store_id = match handler.authorize_endpoint_read(actor, &request.store_id) {
                Ok(store_id) => store_id,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        error.code(),
                        error.to_string(),
                    )));
                }
            };
            let binding = match read_profile_binding(
                &handler.profile_binding_registry_path,
                store_id.as_str(),
            ) {
                Ok(Some(binding)) => binding,
                Ok(None) | Err(_) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_diagnostics_unavailable",
                        "profile diagnostics requires a registered bounded folder profile",
                    )));
                }
            };
            if binding.manifest.deployment_profile != DeploymentProfile::Folder {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_diagnostics_unavailable",
                    "profile diagnostics is available for bounded folder profiles only",
                )));
            }
            let capacity = match read_store_registry(&handler.store_registry_path) {
                Ok(definitions) => definitions
                    .into_iter()
                    .find(|definition| definition.store_id == store_id)
                    .map(|definition| definition.policy.capacity),
                Err(_) => None,
            };
            let Some(capacity) = capacity else {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_diagnostics_unavailable",
                    "profile diagnostics capacity policy is unavailable",
                )));
            };
            let backend =
                match FolderBackend::open(binding.backend_root, binding.manifest, capacity, 0) {
                    Ok(backend) => backend,
                    Err(error) => {
                        return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                            "profile_diagnostics_unavailable",
                            error.to_string(),
                        )));
                    }
                };
            let reconciliation_path = handler
                .profile_binding_registry_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("profile-reconciliation")
                .join(store_id.as_str())
                .join(format!("{}.json", store_id));
            let last_reconciliation_at_unix_seconds =
                ReconciliationManifest::load(&reconciliation_path)
                    .ok()
                    .map(|manifest| manifest.updated_at_unix_seconds);
            let summary = match profile_diagnostics(&backend, last_reconciliation_at_unix_seconds) {
                Ok(summary) => summary,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_diagnostics_failed",
                        error.to_string(),
                    )));
                }
            };
            Ok(DaemonApiResponse::ProfileDiagnostics(
                ProfileDiagnosticsResponse {
                    schema_version: crate::api::PROFILE_DIAGNOSTICS_SCHEMA_VERSION.to_string(),
                    store_id,
                    profile: DeploymentProfile::Folder,
                    state: summary.state,
                    catalogue_object_count: summary.catalogue_object_count,
                    backend_object_count: summary.backend_object_count,
                    uncatalogued_backend_object_count: summary.uncatalogued_backend_object_count,
                    catalogue_missing_backend_object_count: summary
                        .catalogue_missing_backend_object_count,
                    last_reconciliation_at_unix_seconds: summary
                        .last_reconciliation_at_unix_seconds,
                    actionable_message: summary.actionable_message,
                },
            ))
        }
        DaemonApiRequest::ObjectDownload(request) => {
            let delegated_actor = match handler
                .delegated_object_browser_actor(actor, request.delegated_actor.as_ref())
            {
                Ok(actor) => actor,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        error.code(),
                        error.to_string(),
                    )));
                }
            };
            let effective_actor = delegated_actor.as_ref().or(actor);
            let store_id = match handler.authorize_object_download(effective_actor, &request) {
                Ok(store_id) => store_id,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        error.code(),
                        error.to_string(),
                    )));
                }
            };
            match resolve_object_download_with_hdd_root(
                &handler.live_sqlite_path,
                &handler.hdd_root_path,
                &store_id,
                &request,
            ) {
                Ok(response) => Ok(DaemonApiResponse::ObjectDownload(response)),
                Err(error) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    error.code(),
                    error.to_string(),
                ))),
            }
        }
        DaemonApiRequest::ObjectFolderDownload(request) => {
            let delegated_actor = match handler
                .delegated_object_browser_actor(actor, request.delegated_actor.as_ref())
            {
                Ok(actor) => actor,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        error.code(),
                        error.to_string(),
                    )));
                }
            };
            let effective_actor = delegated_actor.as_ref().or(actor);
            let store_id = match handler.authorize_object_folder_download(effective_actor, &request)
            {
                Ok(store_id) => store_id,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        error.code(),
                        error.to_string(),
                    )));
                }
            };
            match resolve_object_folder_download_with_hdd_root(
                &handler.live_sqlite_path,
                &handler.hdd_root_path,
                &store_id,
                &request,
            ) {
                Ok(response) => Ok(DaemonApiResponse::ObjectFolderDownload(response)),
                Err(error) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    error.code(),
                    error.to_string(),
                ))),
            }
        }
        _ => unreachable!("storage dispatcher received an unrelated request"),
    }
}
