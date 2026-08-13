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
#[path = "storage_jenkins_dossier.rs"]
mod storage_jenkins_dossier;
#[path = "storage_operations.rs"]
mod storage_operations;
#[path = "storage_profile_requests.rs"]
mod storage_profile_requests;
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
        DaemonApiRequest::JenkinsDossierEvidenceSettlement(request) => {
            Ok(storage_jenkins_dossier::settle(handler, request, actor))
        }
        DaemonApiRequest::DiskRetire(request) => {
            if let Err(error) = require_verified_pistis_host_authority(
                actor,
                request.verified_subject.as_ref(),
                "disk retirement",
            ) {
                return Ok(DaemonApiResponse::Error(error));
            }
            match handler.disk_retire_for_actor(request, actor) {
                Ok(response) => Ok(DaemonApiResponse::DiskRetire(response)),
                Err((code, message)) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    code, message,
                ))),
            }
        }
        DaemonApiRequest::DiskForceRetire(request) => {
            if let Err(error) = require_verified_pistis_host_authority(
                actor,
                request.verified_subject.as_ref(),
                "disk force-retirement",
            ) {
                return Ok(DaemonApiResponse::Error(error));
            }
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
        DaemonApiRequest::DestageRetry(request) => {
            match handler.destage_retry_for_actor(request, actor) {
                Ok(response) => Ok(DaemonApiResponse::DestageRetry(response)),
                Err((code, message)) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    code, message,
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
        DaemonApiRequest::UpdateObjectStoreIngestPolicy(request) => {
            if let Err(error) = require_preverified_host_service_peer(
                actor,
                request.administrator_actor.as_deref(),
                "object-store ingest policy updates",
            ) {
                return Ok(DaemonApiResponse::Error(error));
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
        DaemonApiRequest::UpdateObjectStoreAcknowledgementPolicy(request) => {
            if let Err(error) = require_preverified_host_service_peer(
                actor,
                request.administrator_actor.as_deref(),
                "object-store acknowledgement policy updates",
            ) {
                return Ok(DaemonApiResponse::Error(error));
            }
            let now = handler.clock.now_utc();
            let response = match handler.update_object_store_acknowledgement_policy(request, &now) {
                Ok(response) => response,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        "store_policy_update_failed",
                        error.to_string(),
                    )))
                }
            };
            handler.record_admin_job(
                daemon_job_summary_from_update_object_store_acknowledgement_policy(&response),
            )?;
            Ok(DaemonApiResponse::UpdateObjectStoreAcknowledgementPolicy(
                response,
            ))
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
            let verified_store_id = match handler.authorize_verified_object_browser_subject(
                actor,
                request.verified_subject.as_ref(),
                &request.endpoint,
                request.prefix.as_deref(),
            ) {
                Ok(store_id) => store_id,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        error.code(),
                        error.to_string(),
                    )));
                }
            };
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
            let store_id = match verified_store_id {
                Some(store_id) => store_id,
                None => {
                    let effective_actor = delegated_actor.as_ref().or(actor);
                    match handler.authorize_endpoint_read(effective_actor, &request.endpoint) {
                        Ok(store_id) => store_id,
                        Err(error) => {
                            return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                                error.code(),
                                error.to_string(),
                            )));
                        }
                    }
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
        DaemonApiRequest::RemoteObjectSnapshot(request) => {
            if let Err(error) = handler.authorize_endpoint_read(actor, &request.store_id) {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    error.code(),
                    error.to_string(),
                )));
            }
            match remote_object_snapshot(&handler.live_sqlite_path, &request) {
                Ok(response) => Ok(DaemonApiResponse::RemoteObjectSnapshot(response)),
                Err(error) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    error.code(),
                    error.to_string(),
                ))),
            }
        }
        DaemonApiRequest::RemoteObjectGroupStatus(request) => {
            if let Err(error) = handler.authorize_endpoint_read(actor, &request.store_id) {
                return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    error.code(),
                    error.to_string(),
                )));
            }
            match remote_object_group_status(&handler.live_sqlite_path, &request) {
                Ok(response) => Ok(DaemonApiResponse::RemoteObjectGroupStatus(response)),
                Err(error) => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                    error.code(),
                    error.to_string(),
                ))),
            }
        }
        DaemonApiRequest::ObjectDownload(request) => {
            let verified_store_id = match handler.authorize_verified_object_browser_subject(
                actor,
                request.verified_subject.as_ref(),
                &request.endpoint,
                Some(request.object_id.as_str()),
            ) {
                Ok(store_id) => store_id,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        error.code(),
                        error.to_string(),
                    )));
                }
            };
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
            let store_id = match verified_store_id {
                Some(store_id) => store_id,
                None => {
                    let effective_actor = delegated_actor.as_ref().or(actor);
                    match handler.authorize_object_download(effective_actor, &request) {
                        Ok(store_id) => store_id,
                        Err(error) => {
                            return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                                error.code(),
                                error.to_string(),
                            )));
                        }
                    }
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
            let verified_store_id = match handler.authorize_verified_object_browser_subject(
                actor,
                request.verified_subject.as_ref(),
                &request.endpoint,
                Some(&request.prefix),
            ) {
                Ok(store_id) => store_id,
                Err(error) => {
                    return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                        error.code(),
                        error.to_string(),
                    )));
                }
            };
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
            let store_id = match verified_store_id {
                Some(store_id) => store_id,
                None => {
                    let effective_actor = delegated_actor.as_ref().or(actor);
                    match handler.authorize_object_folder_download(effective_actor, &request) {
                        Ok(store_id) => store_id,
                        Err(error) => {
                            return Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
                                error.code(),
                                error.to_string(),
                            )));
                        }
                    }
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
        profile_request => storage_profile_requests::request(handler, profile_request, actor),
    }
}

fn require_preverified_host_service_peer(
    actor: Option<&DaemonLocalActor>,
    verified_subject: Option<&str>,
    operation: &str,
) -> Result<(), DaemonApiErrorResponse> {
    let Some(actor) = actor else {
        return Err(DaemonApiErrorResponse::new(
            "administrator_authentication_required",
            format!("{operation} require a preverified host service peer"),
        ));
    };
    if actor.username.as_deref() != Some(DEFAULT_DAEMON_SERVICE_USER) {
        return Err(DaemonApiErrorResponse::new(
            "preverified_host_authority_required",
            format!(
                "{operation} reject direct root, sudo, and dasobjectstore-admin socket peers; submit through the preverified host service"
            ),
        ));
    }
    if !verified_subject.is_some_and(|value| !value.trim().is_empty()) {
        return Err(DaemonApiErrorResponse::new(
            "preverified_host_subject_required",
            format!("{operation} require a verified host subject"),
        ));
    }
    Ok(())
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
    let Ok((backend_root, backend_manifest)) = crate::runtime::direct_s3_profile_backend(&binding)
    else {
        return;
    };
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
    let capacity = crate::runtime::direct_s3_profile_capacity(&binding, capacity);
    let Ok(backend) = FolderBackend::open(backend_root, backend_manifest, capacity, 0) else {
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
