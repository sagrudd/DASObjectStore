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

impl<S, C> DaemonRequestHandler<S, C>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    fn store_drain_for_actor(
        &self,
        request: StoreDrainRequest,
        actor: Option<&DaemonLocalActor>,
    ) -> Result<StoreDrainResponse, (&'static str, String)> {
        if !request.dry_run {
            let Some(actor) = actor else {
                return Err((
                    "administrator_authentication_required",
                    "store drain requires an authenticated local administrator".to_string(),
                ));
            };
            if !actor.is_administrator() {
                return Err((
                    "administrator_authorization_required",
                    "store drain requires root, sudo, or dasobjectstore-admin membership"
                        .to_string(),
                ));
            }
            if !request.allow_store_drain {
                return Err((
                    "store_drain_not_allowed",
                    "store drain requires policy allowance".to_string(),
                ));
            }
        }
        let store_id = StoreId::new(request.store_id.clone())
            .map_err(|error| ("invalid_store_id", error.to_string()))?;
        let disk_roots = discover_managed_hdd_roots(&self.hdd_root_path)
            .map_err(|error| ("managed_hdd_discovery_failed", error.to_string()))?;
        let report =
            dasobjectstore_metadata::drain_store(&dasobjectstore_metadata::StoreDrainRequest {
                live_sqlite_path: self.live_sqlite_path.clone(),
                store_id,
                disk_roots,
                dry_run: request.dry_run,
            })
            .map_err(|error| ("store_drain_failed", error.to_string()))?;
        Ok(StoreDrainResponse { report })
    }

    fn store_delete_for_actor(
        &self,
        request: StoreDeleteRequest,
        actor: Option<&DaemonLocalActor>,
    ) -> Result<StoreDeleteResponse, (&'static str, String)> {
        if !request.dry_run {
            let Some(actor) = actor else {
                return Err((
                    "administrator_authentication_required",
                    "store delete requires an authenticated local administrator".to_string(),
                ));
            };
            if !actor.is_administrator() {
                return Err((
                    "administrator_authorization_required",
                    "store delete requires root, sudo, or dasobjectstore-admin membership"
                        .to_string(),
                ));
            }
            if !request.allow_store_delete {
                return Err((
                    "store_delete_not_allowed",
                    "store delete requires policy allowance".to_string(),
                ));
            }
        }

        let store_id = StoreId::new(request.store_id.clone())
            .map_err(|error| ("invalid_store_id", error.to_string()))?;
        let disk_roots = discover_managed_hdd_roots(&self.hdd_root_path)
            .map_err(|error| ("managed_hdd_discovery_failed", error.to_string()))?;
        let metadata =
            dasobjectstore_metadata::delete_store(&dasobjectstore_metadata::StoreDeleteRequest {
                live_sqlite_path: self.live_sqlite_path.clone(),
                store_id: store_id.clone(),
                disk_roots,
                dry_run: request.dry_run,
            })
            .map_err(|error| ("store_delete_failed", error.to_string()))?;
        let host_registry =
            delete_store_definition_maybe(&self.store_registry_path, &store_id, request.dry_run)
                .map_err(|error| ("store_registry_delete_failed", error.to_string()))?;
        let host_subobjects = delete_subobjects_for_store_maybe(
            &self.subobject_registry_path,
            &store_id,
            request.dry_run,
        )
        .map_err(|error| ("subobject_registry_delete_failed", error.to_string()))?;

        let (portable_registry, portable_subobjects) = if known_ssd_root(&default_ssd_root()) {
            let ssd_root = default_ssd_root();
            let portable_registry_path = portable_store_registry_path(&ssd_root);
            let portable_subobject_path = portable_subobject_registry_path(&ssd_root);
            (
                Some(
                    delete_store_definition_maybe(
                        &portable_registry_path,
                        &store_id,
                        request.dry_run,
                    )
                    .map_err(|error| {
                        ("portable_store_registry_delete_failed", error.to_string())
                    })?,
                ),
                Some(
                    delete_subobjects_for_store_maybe(
                        &portable_subobject_path,
                        &store_id,
                        request.dry_run,
                    )
                    .map_err(|error| {
                        (
                            "portable_subobject_registry_delete_failed",
                            error.to_string(),
                        )
                    })?,
                ),
            )
        } else {
            (None, None)
        };

        Ok(StoreDeleteResponse {
            report: StoreDeleteCommandReport {
                metadata,
                host_registry,
                portable_registry,
                host_subobjects,
                portable_subobjects,
            },
        })
    }

    fn store_repair_for_actor(
        &self,
        request: StoreRepairRequest,
        actor: Option<&DaemonLocalActor>,
        emit_progress: &mut dyn FnMut(
            DaemonIngestProgressEvent,
        ) -> Result<(), DaemonIngestFilesRuntimeError>,
    ) -> Result<StoreRepairResponse, (&'static str, String)> {
        if !request.dry_run {
            let Some(actor) = actor else {
                return Err((
                    "administrator_authentication_required",
                    "store repair requires an authenticated local administrator".to_string(),
                ));
            };
            if !actor.is_administrator() {
                return Err((
                    "administrator_authorization_required",
                    "store repair requires root, sudo, or dasobjectstore-admin membership"
                        .to_string(),
                ));
            }
        }
        let reconciliation_job = if request.reconcile_s3 {
            let accepted_at_utc = self.clock.now_utc();
            let job = reconciliation_job_summary(
                &request,
                &accepted_at_utc,
                actor.map(DaemonLocalActor::display_name),
                crate::api::DaemonJobState::Running,
                "Garage reconciliation started",
            )
            .map_err(|error| ("store_repair_job_id_failed", error))?;
            self.record_admin_job(job.clone())
                .map_err(|error| ("store_repair_job_registry_failed", error.to_string()))?;
            Some((job.job_id.clone(), accepted_at_utc))
        } else {
            None
        };
        let result = (|| {
            let s3_reconciliation = if request.reconcile_s3 {
                emit_reconciliation_progress(
                    emit_progress,
                    &request,
                    "starting Garage download into private SSD staging",
                )
                .map_err(|error| ("store_repair_progress_failed", error.to_string()))?;
                let store_id = request
                    .store_id
                    .clone()
                    .expect("validated reconciliation store id");
                Some(
                    self.service_orchestrator
                        .reconcile_store_s3_cancellable(
                            store_id,
                            request.s3_prefix.clone(),
                            request.dry_run,
                            &self.clock.now_utc(),
                            &|| {
                                reconciliation_job
                                    .as_ref()
                                    .is_some_and(|(job_id, _)| self.is_job_cancelled(job_id))
                            },
                            emit_progress,
                        )
                        .map_err(|error| {
                            ("store_repair_s3_reconciliation_failed", error.to_string())
                        })?,
                )
            } else {
                None
            };
            if s3_reconciliation.is_some() {
                emit_reconciliation_progress(
                emit_progress,
                &request,
                "Garage download finished; SSD-to-HDD ingest and metadata registration completed",
            )
            .map_err(|error| ("store_repair_progress_failed", error.to_string()))?;
            }
            let report = if request.reconcile_s3 && !request.dry_run {
                // Reconciliation uses normal ingest, which commits verified object and
                // placement metadata atomically. A filtered live-index rebuild would
                // discard unrelated state and is intentionally unsupported.
                reconciliation_registration_report(&self.live_sqlite_path)
            } else {
                let (store_definitions, disk_roots) =
                    self.recovery_inputs("store_repair_failed")?;
                dasobjectstore_metadata::recover_live_metadata(
                    &dasobjectstore_metadata::RecoverLiveMetadataRequest {
                        live_sqlite_path: self.live_sqlite_path.clone(),
                        store_definitions,
                        disk_roots,
                        store_id: request.store_id.clone(),
                        dry_run: request.dry_run,
                        recorded_at_utc: self.clock.now_utc(),
                    },
                )
                .map_err(|error| ("store_repair_failed", error.to_string()))?
            };
            let response = StoreRepairResponse {
                report: StoreRepairReport {
                    metadata_path: report.metadata_path.display().to_string(),
                    backup_path: report.backup_path.map(|path| path.display().to_string()),
                    dry_run: report.dry_run,
                    stores_scanned: report.stores_scanned,
                    payload_files: report.payload_files,
                    objects_recovered: report.objects_recovered,
                    placements_recovered: report.placements_recovered,
                    payload_bytes: report.payload_bytes,
                    partial_duplicates_omitted: report.partial_duplicates_omitted,
                    hashes_verified: report.hashes_verified,
                    warning: report.warning,
                },
                s3_reconciliation,
            };
            Ok(response)
        })();
        let Some((job_id, accepted_at_utc)) = reconciliation_job else {
            return result;
        };
        self.clear_job_cancelled(&job_id);
        let (state, message, failure_message) = match &result {
            Ok(_) => (
                crate::api::DaemonJobState::Complete,
                "Garage reconciliation and metadata repair completed".to_string(),
                None,
            ),
            Err((_, error)) => (
                crate::api::DaemonJobState::Failed,
                format!("Garage reconciliation failed: {error}"),
                Some(error.clone()),
            ),
        };
        let mut job = reconciliation_job_summary(
            &request,
            &accepted_at_utc,
            actor.map(DaemonLocalActor::display_name),
            state,
            message,
        )
        .map_err(|error| ("store_repair_job_id_failed", error))?;
        job.job_id = job_id;
        job.updated_at_utc = self.clock.now_utc();
        job.failure_message = failure_message;
        self.record_admin_job(job)
            .map_err(|error| ("store_repair_job_registry_failed", error.to_string()))?;
        result
    }

    fn store_verify_for_actor(
        &self,
        request: StoreVerifyRequest,
    ) -> Result<StoreVerifyResponse, (&'static str, String)> {
        let disk_roots = discover_managed_hdd_roots(&self.hdd_root_path)
            .map_err(|error| ("store_verify_failed", error.to_string()))?;
        let report = dasobjectstore_metadata::verify_live_metadata(
            &dasobjectstore_metadata::VerifyLiveMetadataRequest {
                live_sqlite_path: self.live_sqlite_path.clone(),
                disk_roots,
                store_id: request.store_id.map(|id| id.as_str().to_string()),
                hash_payloads: request.hash_payloads,
            },
        )
        .map_err(|error| ("store_verify_failed", error.to_string()))?;
        Ok(StoreVerifyResponse {
            report: StoreVerifyReport {
                metadata_path: report.metadata_path.display().to_string(),
                stores_scanned: report.stores_scanned,
                objects_scanned: report.objects_scanned,
                placements_scanned: report.placements_scanned,
                payloads_checked: report.payloads_checked,
                payload_bytes_checked: report.payload_bytes_checked,
                missing_payloads: report.missing_payloads,
                orphan_payloads: report.orphan_payloads,
                size_mismatches: report.size_mismatches,
                hash_mismatches: report.hash_mismatches,
                unverified_placements: report.unverified_placements,
                duplicate_content_groups: report.duplicate_content_groups,
                duplicate_placement_rows: report.duplicate_placement_rows,
                io_errors: report.io_errors,
                healthy: report.healthy,
                findings: report.findings,
            },
        })
    }

    fn store_deduplicate_for_actor(
        &self,
        request: StoreDeduplicateRequest,
        actor: Option<&DaemonLocalActor>,
    ) -> Result<StoreDeduplicateResponse, (&'static str, String)> {
        if !request.dry_run {
            let Some(actor) = actor else {
                return Err((
                    "administrator_authentication_required",
                    "store deduplicate requires an authenticated local administrator".to_string(),
                ));
            };
            if !actor.is_administrator() {
                return Err((
                    "administrator_authorization_required",
                    "store deduplicate requires root, sudo, or dasobjectstore-admin membership"
                        .to_string(),
                ));
            }
        }
        let disk_roots = discover_managed_hdd_roots(&self.hdd_root_path)
            .map_err(|error| ("store_deduplicate_failed", error.to_string()))?;
        let report = dasobjectstore_metadata::deduplicate_live_metadata(
            &dasobjectstore_metadata::DeduplicateLiveMetadataRequest {
                live_sqlite_path: self.live_sqlite_path.clone(),
                disk_roots,
                store_id: request.store_id.map(|id| id.as_str().to_string()),
                dry_run: request.dry_run,
                recorded_at_utc: self.clock.now_utc(),
            },
        )
        .map_err(|error| ("store_deduplicate_failed", error.to_string()))?;
        Ok(StoreDeduplicateResponse {
            report: StoreDeduplicateReport {
                metadata_path: report.metadata_path.display().to_string(),
                dry_run: report.dry_run,
                payloads_hashed: report.payloads_hashed,
                hash_errors: report.hash_errors,
                duplicate_content_groups: report.duplicate_content_groups,
                duplicate_placement_rows: report.duplicate_placement_rows,
                metadata_rows_removed: report.metadata_rows_removed,
                hashes_recorded: report.hashes_recorded,
                warning: report.warning,
            },
        })
    }

    fn recovery_inputs(
        &self,
        error_code: &'static str,
    ) -> Result<
        (
            Vec<dasobjectstore_metadata::RecoveryStoreDefinition>,
            Vec<dasobjectstore_metadata::DiskCopyRoot>,
        ),
        (&'static str, String),
    > {
        let definitions =
            dasobjectstore_object_service::read_store_registry(&self.store_registry_path)
                .map_err(|error| (error_code, error.to_string()))?;
        let store_definitions = definitions
            .into_iter()
            .map(|definition| {
                let class = definition.policy.class.name().to_string();
                let policy_json = serde_json::to_string(&definition.policy)
                    .map_err(|error| (error_code, error.to_string()))?;
                Ok(dasobjectstore_metadata::RecoveryStoreDefinition {
                    store_id: definition.store_id,
                    class,
                    policy_json,
                })
            })
            .collect::<Result<Vec<_>, (&'static str, String)>>()?;
        let disk_roots = discover_managed_hdd_roots(&self.hdd_root_path)
            .map_err(|error| (error_code, error.to_string()))?;
        Ok((store_definitions, disk_roots))
    }

    fn object_put_for_actor(
        &self,
        request: ObjectPutRequest,
        actor: Option<&DaemonLocalActor>,
    ) -> Result<ObjectPutResponse, (&'static str, String)> {
        if actor.is_none() {
            return Err((
                "authentication_required",
                "object put requires an authenticated local actor".to_string(),
            ));
        }
        let object_id = dasobjectstore_core::ids::ObjectId::new(request.object_id.clone())
            .map_err(|error| ("invalid_object_id", error.to_string()))?;
        let disk_roots = parse_disk_copy_roots(&request.disk_roots)
            .map_err(|error| ("invalid_disk_root", error))?;
        let metadata_request = MetadataObjectPutRequest::new(
            object_id,
            request.source_path,
            request.ssd_root,
            disk_roots,
            request.copies,
        )
        .with_object_type(request.object_type);
        let report = put_object_ssd_first(&metadata_request)
            .map_err(|error| ("object_put_failed", error.to_string()))?;
        Ok(ObjectPutResponse { report })
    }

    fn disk_retire_for_actor(
        &self,
        request: DiskRetireRequest,
        actor: Option<&DaemonLocalActor>,
    ) -> Result<DiskRetireResponse, (&'static str, String)> {
        let Some(actor) = actor else {
            return Err((
                "administrator_authentication_required",
                "disk retirement requires an authenticated local administrator".to_string(),
            ));
        };
        if !actor.is_administrator() {
            return Err((
                "administrator_authorization_required",
                "disk retirement requires root, sudo, or dasobjectstore-admin membership"
                    .to_string(),
            ));
        }
        let disk_id = dasobjectstore_core::ids::DiskId::new(request.disk_id.clone())
            .map_err(|error| ("invalid_disk_id", error.to_string()))?;
        let report = dasobjectstore_metadata::request_disk_retirement(
            &self.live_sqlite_path,
            &disk_id,
            self.clock.now_utc(),
        )
        .map_err(|error| ("disk_retirement_failed", error.to_string()))?;
        Ok(DiskRetireResponse { report })
    }

    fn disk_force_retire_for_actor(
        &self,
        request: DiskForceRetireRequest,
        actor: Option<&DaemonLocalActor>,
    ) -> Result<DiskRetireResponse, (&'static str, String)> {
        let Some(actor) = actor else {
            return Err((
                "administrator_authentication_required",
                "disk force-retirement requires an authenticated local administrator".to_string(),
            ));
        };
        if !actor.is_administrator() {
            return Err((
                "administrator_authorization_required",
                "disk force-retirement requires root, sudo, or dasobjectstore-admin membership"
                    .to_string(),
            ));
        }
        if !request.allow_force_retire {
            return Err((
                "force_disk_retire_not_allowed",
                "disk force-retirement requires policy allowance".to_string(),
            ));
        }
        let disk_id = dasobjectstore_core::ids::DiskId::new(request.disk_id.clone())
            .map_err(|error| ("invalid_disk_id", error.to_string()))?;
        let report = dasobjectstore_metadata::force_retire_disk(
            &self.live_sqlite_path,
            &disk_id,
            self.clock.now_utc(),
            dasobjectstore_core::risk::RiskPolicy {
                allow_force_retire: true,
                ..Default::default()
            },
            &dasobjectstore_core::risk::ActionConfirmation::new(&request.confirmation_marker),
        )
        .map_err(|error| ("disk_force_retirement_failed", error.to_string()))?;
        Ok(DiskRetireResponse { report })
    }

    fn ingest_queue_drain_for_actor(
        &self,
        request: IngestQueueDrainRequest,
        actor: Option<&DaemonLocalActor>,
    ) -> Result<IngestQueueDrainResponse, (&'static str, String)> {
        if !request.dry_run {
            let Some(actor) = actor else {
                return Err((
                    "administrator_authentication_required",
                    "ingest queue drain requires an authenticated local administrator".to_string(),
                ));
            };
            if !actor.is_administrator() {
                return Err((
                    "administrator_authorization_required",
                    "ingest queue drain requires root, sudo, or dasobjectstore-admin membership"
                        .to_string(),
                ));
            }
            if !request.allow_ingest_queue_drain {
                return Err((
                    "ingest_queue_drain_not_allowed",
                    "ingest queue drain requires policy allowance".to_string(),
                ));
            }
        }
        let store_id = StoreId::new(request.store_id.clone())
            .map_err(|error| ("invalid_store_id", error.to_string()))?;
        let report = dasobjectstore_metadata::drain_ingest_queue(
            &dasobjectstore_metadata::IngestQueueDrainRequest {
                live_sqlite_path: self.live_sqlite_path.clone(),
                store_id,
                updated_at_utc: self.clock.now_utc(),
                reason: request.reason,
                dry_run: request.dry_run,
            },
        )
        .map_err(|error| ("ingest_queue_drain_failed", error.to_string()))?;
        Ok(IngestQueueDrainResponse { report })
    }

    fn store_inventory_for_actor(
        &self,
        request: StoreInventoryRequest,
        actor: Option<&DaemonLocalActor>,
    ) -> Result<StoreInventoryResponse, ObjectServiceError> {
        if let Some(session_id) = request.remote_easyconnect_session_id.as_deref() {
            return self
                .store_inventory_for_remote_easyconnect_session(&request, session_id)
                .map_err(|error| ObjectServiceError::CommandFailed(error.to_string()));
        }
        let stores = read_store_registry(&self.store_registry_path)?;
        let mut inventory = Vec::new();
        for definition in stores {
            let bucket_name = if definition.policy.export_policy == ExportPolicy::S3 {
                Some(bucket_name_for_definition(&definition)?)
            } else {
                None
            };
            let mut access_policy = DaemonStoreAccessPolicy::new(definition.store_id.clone());
            if let Some(reader_group) = &definition.reader_group {
                access_policy = access_policy.with_reader_group(reader_group.clone());
            }
            if let Some(writer_group) = &definition.writer_group {
                access_policy = access_policy.with_writer_group(writer_group.clone());
            }
            access_policy = access_policy.with_public_read(definition.public);
            let visible = match actor {
                Some(actor) => authorize_store_read(actor, &access_policy).is_ok(),
                None => definition.public,
            };
            if !visible {
                continue;
            }
            let mut policy = definition.policy.clone();
            if !request.include_policy {
                policy = dasobjectstore_core::store::StorePolicy::defaults_for(policy.class);
            }
            inventory.push(StoreInventoryItem {
                store_id: definition.store_id,
                policy,
                bucket_name,
                reader_group: definition.reader_group,
                writer_group: definition.writer_group,
                public: definition.public,
                writable: definition.policy.export_policy == ExportPolicy::S3,
            });
        }
        Ok(StoreInventoryResponse { stores: inventory })
    }

    fn store_inventory_for_remote_easyconnect_session(
        &self,
        request: &StoreInventoryRequest,
        session_id: &str,
    ) -> Result<StoreInventoryResponse, RemoteEasyconnectStoreInventoryError> {
        let session_store = FileBackedRemoteEasyconnectPairedSessionStore::new(
            &self.remote_easyconnect_session_store_path,
        );
        let session = session_store.get(session_id)?.ok_or_else(|| {
            RemoteEasyconnectPairedSessionStoreError::SessionNotFound {
                session_id: session_id.to_string(),
            }
        })?;
        let actor = DaemonLocalActor::new(0).with_username(session.approved_actor.clone());
        let stores = read_store_registry(&self.store_registry_path).map_err(|error| {
            RemoteEasyconnectPairedSessionStoreError::Json {
                path: self.store_registry_path.clone(),
                message: error.to_string(),
            }
        })?;
        let mut inventory = Vec::new();
        for definition in stores {
            let Some(grant) = session
                .object_stores
                .iter()
                .find(|grant| grant.object_store == definition.store_id.as_str())
            else {
                continue;
            };
            if request.remote_upload_writable_only {
                session_store.authorize_write(
                    session_id,
                    definition.store_id.as_str(),
                    &actor,
                    &self.clock.now_utc(),
                )?;
                if definition.writer_group.is_none() {
                    return Err(RemoteEasyconnectStoreInventoryError::MissingWriterGroup {
                        object_store: definition.store_id.to_string(),
                    });
                }
                if definition.policy.export_policy != ExportPolicy::S3 {
                    return Err(
                        RemoteEasyconnectStoreInventoryError::StoreNotRemoteWritable {
                            object_store: definition.store_id.to_string(),
                            export_policy: format!("{:?}", definition.policy.export_policy),
                        },
                    );
                }
            } else if !grant.can_read && !grant.can_write {
                continue;
            }
            let bucket_name = if definition.policy.export_policy == ExportPolicy::S3 {
                Some(bucket_name_for_definition(&definition).map_err(|error| {
                    RemoteEasyconnectPairedSessionStoreError::Json {
                        path: self.store_registry_path.clone(),
                        message: error.to_string(),
                    }
                })?)
            } else {
                None
            };
            let mut policy = definition.policy.clone();
            if !request.include_policy {
                policy = dasobjectstore_core::store::StorePolicy::defaults_for(policy.class);
            }
            inventory.push(StoreInventoryItem {
                store_id: definition.store_id,
                policy,
                bucket_name,
                reader_group: definition.reader_group,
                writer_group: definition.writer_group,
                public: definition.public,
                writable: definition.policy.export_policy == ExportPolicy::S3 && grant.can_write,
            });
        }

        Ok(StoreInventoryResponse { stores: inventory })
    }
}
