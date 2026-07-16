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
#[path = "storage_profiles.rs"]
mod storage_profiles;
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
            let entries =
                match read_object_browser_metadata(&handler.live_sqlite_path, store_id.clone()) {
                    Ok(entries) => entries,
                    Err(error) => {
                        return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                            "object_browser_metadata_failed",
                            error.to_string(),
                        )));
                    }
                };
            let mut response = query_object_browser_metadata(&request, &entries)?;
            advertise_provider_stream_downloads(handler, &store_id, &mut response);
            Ok(DaemonApiResponse::ObjectBrowser(response))
        }
        DaemonApiRequest::ProfileBrowser(request) => {
            storage_profiles::profile_browser(handler, request, actor)
        }
        DaemonApiRequest::ProfileS3List(request) => {
            storage_profiles::profile_s3_list(handler, request, actor)
        }
        DaemonApiRequest::ProfileCatalogueExport(request) => {
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
                Ok(Some(binding))
                    if binding.manifest.deployment_profile == DeploymentProfile::Folder =>
                {
                    binding
                }
                Ok(Some(_)) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_catalogue_unavailable",
                        "portable catalogue export is available for bounded folder profiles only",
                    )));
                }
                Ok(None) | Err(_) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_catalogue_unavailable",
                        "portable catalogue export requires a registered bounded folder profile",
                    )));
                }
            };
            let capacity = match read_store_registry(&handler.store_registry_path) {
                Ok(definitions) => definitions
                    .into_iter()
                    .find(|definition| definition.store_id == store_id)
                    .map(|definition| definition.policy.capacity),
                Err(_) => None,
            };
            let Some(capacity) = capacity else {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_catalogue_unavailable",
                    "profile capacity policy is unavailable",
                )));
            };
            let backend =
                match FolderBackend::open(binding.backend_root, binding.manifest, capacity, 0) {
                    Ok(backend) => backend,
                    Err(error) => {
                        return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                            "profile_catalogue_unavailable",
                            error.to_string(),
                        )));
                    }
                };
            let catalogue = match crate::runtime::export_profile_catalogue(&store_id, &backend) {
                Ok(catalogue) => catalogue,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_catalogue_export_failed",
                        error.to_string(),
                    )));
                }
            };
            Ok(DaemonApiResponse::ProfileCatalogueExport(
                crate::api::ProfileCatalogueExportResponse {
                    schema_version: crate::api::PROFILE_CATALOGUE_SCHEMA_VERSION.to_string(),
                    store_id,
                    catalogue,
                },
            ))
        }
        DaemonApiRequest::ProfileCatalogueImport(request) => {
            let store_id = match handler.authorize_endpoint_write(actor, &request.store_id) {
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
                Ok(Some(binding))
                    if binding.manifest.deployment_profile == DeploymentProfile::Folder =>
                {
                    binding
                }
                Ok(Some(_)) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_catalogue_unavailable",
                        "portable catalogue import is available for bounded folder profiles only",
                    )));
                }
                Ok(None) | Err(_) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_catalogue_unavailable",
                        "portable catalogue import requires a registered bounded folder profile",
                    )));
                }
            };
            let capacity = match read_store_registry(&handler.store_registry_path) {
                Ok(definitions) => definitions
                    .into_iter()
                    .find(|definition| definition.store_id == store_id)
                    .map(|definition| definition.policy.capacity),
                Err(_) => None,
            };
            let Some(capacity) = capacity else {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_catalogue_unavailable",
                    "profile capacity policy is unavailable",
                )));
            };
            let handoff_root = binding
                .backend_root
                .join(".dasobjectstore/profile-catalogue-handoffs");
            let mut backend =
                match FolderBackend::open(binding.backend_root, binding.manifest, capacity, 0) {
                    Ok(backend) => backend,
                    Err(error) => {
                        return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                            "profile_catalogue_unavailable",
                            error.to_string(),
                        )));
                    }
                };
            let committed_at_utc = match SystemTime::now().duration_since(UNIX_EPOCH) {
                Ok(duration) => format_utc_timestamp_seconds(duration.as_secs() as i64),
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_catalogue_import_failed",
                        format!("clock unavailable: {error}"),
                    )));
                }
            };
            let imported_objects = match crate::runtime::import_profile_catalogue_with_metadata(
                &store_id,
                &request.catalogue,
                &mut backend,
                &handler.live_sqlite_path,
                handoff_root,
                &request.transaction_id,
                &request.profile_namespace,
                &committed_at_utc,
            ) {
                Ok(imported_objects) => imported_objects,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_catalogue_import_failed",
                        error.to_string(),
                    )));
                }
            };
            Ok(DaemonApiResponse::ProfileCatalogueImport(
                crate::api::ProfileCatalogueImportResponse {
                    schema_version: crate::api::PROFILE_CATALOGUE_SCHEMA_VERSION.to_string(),
                    store_id,
                    imported_objects,
                    source_retained: true,
                },
            ))
        }
        DaemonApiRequest::ProfileS3Delete(request) => {
            let store_id = match handler.authorize_endpoint_write(actor, &request.store_id) {
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
                        "profile S3 deletion requires a registered bounded folder profile",
                    )));
                }
            };
            if binding.manifest.deployment_profile != DeploymentProfile::Folder {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_s3_unavailable",
                    "profile S3 deletion is available for bounded folder profiles only",
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
            let mut backend =
                match FolderBackend::open(binding.backend_root, binding.manifest, capacity, 0) {
                    Ok(backend) => backend,
                    Err(error) => {
                        return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                            "profile_s3_unavailable",
                            error.to_string(),
                        )));
                    }
                };
            let Some(provider) = handler.service_orchestrator.capacity_provider() else {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_s3_delete_unavailable",
                    "profile S3 deletion requires daemon capacity admission",
                )));
            };
            let deleted = match crate::runtime::delete_profile_object_with_capacity_provider(
                provider.as_ref(),
                store_id.as_str(),
                &mut backend,
                &request.key,
            ) {
                Ok(deleted) => deleted,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_s3_delete_failed",
                        error.to_string(),
                    )));
                }
            };
            Ok(DaemonApiResponse::ProfileS3Delete(
                crate::api::ProfileS3DeleteResponse {
                    schema_version: PROFILE_S3_SCHEMA_VERSION.to_string(),
                    store_id,
                    key: request.key,
                    deleted,
                },
            ))
        }
        DaemonApiRequest::ProfileS3MultipartComplete(request) => {
            let store_id = match handler.authorize_endpoint_write(actor, &request.store_id) {
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
                        "multipart completion requires a registered bounded folder profile",
                    )));
                }
            };
            if binding.manifest.deployment_profile != DeploymentProfile::Folder {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_s3_unavailable",
                    "multipart completion is available for bounded folder profiles only",
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
            let backend_root = binding.backend_root.clone();
            let journal = match crate::runtime::MultipartPartJournal::open_for_completion(
                &backend_root,
                store_id.as_str(),
                &request.reservation_id,
                request.key.clone(),
                request.expected_size_bytes,
            ) {
                Ok(journal) => journal,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "profile_s3_multipart_unavailable",
                        error.to_string(),
                    )));
                }
            };
            let journal_parts = journal.parts().collect::<Vec<_>>();
            let requested_parts = request
                .parts
                .iter()
                .map(|part| crate::runtime::MultipartPartRecord {
                    part_number: part.part_number,
                    size_bytes: part.size_bytes,
                    checksum: part.checksum.clone(),
                })
                .collect::<Vec<_>>();
            if journal_parts != requested_parts
                || journal.staged_bytes() != request.expected_size_bytes
            {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_s3_multipart_incomplete",
                    "multipart completion does not match all verified staged parts",
                )));
            }
            let mut sources = Vec::with_capacity(request.parts.len());
            for part in &request.parts {
                let reader = match journal.open_part(part.part_number) {
                    Ok(reader) => reader,
                    Err(error) => {
                        return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                            "profile_s3_multipart_incomplete",
                            error.to_string(),
                        )));
                    }
                };
                sources.push(crate::runtime::ProfileS3MultipartPartSource {
                    part: crate::runtime::ProfileS3MultipartPart {
                        part_number: part.part_number,
                        size_bytes: part.size_bytes,
                        checksum: part.checksum.clone(),
                    },
                    reader: Box::new(reader),
                });
            }
            let mut backend =
                match FolderBackend::open(&backend_root, binding.manifest, capacity, 0) {
                    Ok(backend) => backend,
                    Err(error) => {
                        return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                            "profile_s3_unavailable",
                            error.to_string(),
                        )));
                    }
                };
            let Some(provider) = handler.service_orchestrator.capacity_provider() else {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    "profile_s3_multipart_unavailable",
                    "multipart completion requires daemon capacity admission",
                )));
            };
            let completion = crate::runtime::ProfileS3MultipartCompletion {
                reservation_id: request.reservation_id.clone(),
                key: request.key.clone(),
                expected_size_bytes: request.expected_size_bytes,
                parts: request
                    .parts
                    .iter()
                    .map(|part| crate::runtime::ProfileS3MultipartPart {
                        part_number: part.part_number,
                        size_bytes: part.size_bytes,
                        checksum: part.checksum.clone(),
                    })
                    .collect(),
            };
            let record =
                match crate::runtime::complete_profile_s3_multipart_with_admitted_capacity_provider(
                    provider.as_ref(),
                    store_id.as_str(),
                    &mut backend,
                    &completion,
                    sources,
                ) {
                    Ok(record) => record,
                    Err(error) => {
                        return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                            "profile_s3_multipart_failed",
                            error.to_string(),
                        )));
                    }
                };
            if let Err(response) = storage_profiles::publish_profile_s3_catalogue(
                handler, &store_id, &backend, &request.reservation_id,
            ) {
                return Ok(response);
            }
            let response = crate::api::ProfileS3MultipartCompletionResponse {
                schema_version: PROFILE_S3_SCHEMA_VERSION.to_string(),
                store_id,
                reservation_id: request.reservation_id,
                key: record.key,
                committed: true,
            };
            let _ = journal.remove();
            Ok(DaemonApiResponse::ProfileS3MultipartComplete(response))
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
            storage_profiles::diagnostics(handler, request, actor)
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
fn advertise_provider_stream_downloads<S, C>(
    handler: &DaemonRequestHandler<S, C>,
    store_id: &StoreId,
    response: &mut crate::api::ObjectBrowserResponse,
) where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    let Ok(Some(binding)) =
        read_profile_binding(&handler.profile_binding_registry_path, store_id.as_str())
    else {
        return;
    };
    if binding.manifest.deployment_profile != DeploymentProfile::Folder {
        return;
    }
    let Ok(definitions) = read_store_registry(&handler.store_registry_path) else {
        return;
    };
    let Some(capacity) = definitions
        .into_iter()
        .find(|definition| definition.store_id == *store_id)
        .map(|definition| definition.policy.capacity)
    else {
        return;
    };
    let Ok(backend) = FolderBackend::open(binding.backend_root, binding.manifest, capacity, 0)
    else {
        return;
    };
    let Ok(records) = backend.records() else {
        return;
    };
    for file in &mut response.files {
        if file.download_source.is_some() {
            continue;
        }
        let key = dasobjectstore_core::backend::BackendObjectKey {
            object_id: file.object_id.as_str().to_string(),
            version: 1,
        };
        if records.iter().any(|record| record.key == key) {
            file.download_source = Some(crate::api::ObjectBrowserDownloadSource::ProviderStream);
        }
    }
}
