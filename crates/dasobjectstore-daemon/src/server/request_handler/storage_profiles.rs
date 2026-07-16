use super::*;
use crate::{ProfileBrowserRequest, ProfileDiagnosticsRequest, ProfileS3ListRequest};

pub(super) fn publish_profile_s3_catalogue<S, C>(
    handler: &DaemonRequestHandler<S, C>,
    store_id: &StoreId,
    backend: &FolderBackend,
) -> Result<(), DaemonApiResponse>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    handler
        .publish_profile_s3_catalogue(store_id, backend)
        .map_err(|error| api_error("profile_s3_catalogue_publication_failed", error.to_string()))
}

pub(super) fn delete_profile_s3_object<S, C>(
    handler: &DaemonRequestHandler<S, C>,
    provider: &dyn crate::runtime::CapacityAdmissionProvider,
    store_id: &StoreId,
    backend: &mut FolderBackend,
    key: &dasobjectstore_core::backend::BackendObjectKey,
) -> Result<bool, DaemonApiResponse>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    let deleted = crate::runtime::delete_profile_object_with_capacity_provider(
        provider,
        store_id.as_str(),
        backend,
        key,
    )
    .map_err(|error| api_error("profile_s3_delete_failed", error.to_string()))?;
    publish_profile_s3_catalogue(handler, store_id, backend)?;
    Ok(deleted)
}

pub(super) fn profile_browser<S, C>(
    handler: &DaemonRequestHandler<S, C>,
    request: ProfileBrowserRequest,
    actor: Option<&DaemonLocalActor>,
) -> Result<DaemonApiResponse, DaemonRequestHandlerError>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    let delegated_actor =
        match handler.delegated_object_browser_actor(actor, request.delegated_actor.as_ref()) {
            Ok(actor) => actor,
            Err(error) => return Ok(api_error(error.code(), error.to_string())),
        };
    let effective_actor = delegated_actor.as_ref().or(actor);
    let store_id = match handler.authorize_endpoint_read(effective_actor, &request.store_id) {
        Ok(store_id) => store_id,
        Err(error) => return Ok(api_error(error.code(), error.to_string())),
    };
    let binding =
        match read_profile_binding(&handler.profile_binding_registry_path, store_id.as_str()) {
            Ok(Some(binding)) => binding,
            Ok(None) => {
                return Ok(api_error(
                    "profile_browser_unavailable",
                    "profile browser requires a registered bounded folder profile",
                ))
            }
            Err(_) => {
                return Ok(api_error(
                    "profile_browser_unavailable",
                    "profile browser could not load the registered profile",
                ))
            }
        };
    if binding.manifest.deployment_profile != DeploymentProfile::Folder {
        return Ok(api_error(
            "profile_browser_unavailable",
            "profile browser is available for bounded folder profiles only",
        ));
    }
    let catalogue_path = binding.backend_root.join(".dasobjectstore/catalogue.json");
    let catalogue = match FolderCatalogue::open_existing(&catalogue_path, store_id.as_str()) {
        Ok(catalogue) => catalogue,
        Err(_) => {
            return Ok(api_error(
                "profile_browser_unavailable",
                "profile catalogue is unavailable",
            ))
        }
    };
    let offset = match usize::try_from(request.offset) {
        Ok(offset) => offset,
        Err(_) => {
            return Ok(api_error(
                "profile_browser_unavailable",
                "profile browser offset is too large",
            ))
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
            return Ok(api_error(
                "profile_browser_unavailable",
                "profile catalogue query failed",
            ))
        }
    };
    let total_entries = match catalogue.browser_entry_count(&query) {
        Ok(total_entries) => total_entries,
        Err(_) => {
            return Ok(api_error(
                "profile_browser_unavailable",
                "profile catalogue count failed",
            ))
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

pub(super) fn profile_s3_list<S, C>(
    handler: &DaemonRequestHandler<S, C>,
    request: ProfileS3ListRequest,
    actor: Option<&DaemonLocalActor>,
) -> Result<DaemonApiResponse, DaemonRequestHandlerError>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    let store_id = match handler.authorize_endpoint_read(actor, &request.store_id) {
        Ok(store_id) => store_id,
        Err(error) => return Ok(api_error(error.code(), error.to_string())),
    };
    let binding =
        match read_profile_binding(&handler.profile_binding_registry_path, store_id.as_str()) {
            Ok(Some(binding)) => binding,
            Ok(None) => {
                return Ok(api_error(
                    "profile_s3_unavailable",
                    "profile S3 requires a registered bounded folder profile",
                ))
            }
            Err(_) => {
                return Ok(api_error(
                    "profile_s3_unavailable",
                    "profile S3 could not load the registered profile",
                ))
            }
        };
    if binding.manifest.deployment_profile != DeploymentProfile::Folder {
        return Ok(api_error(
            "profile_s3_unavailable",
            "profile S3 is available for bounded folder profiles only",
        ));
    }
    let capacity = match read_store_registry(&handler.store_registry_path) {
        Ok(definitions) => definitions
            .into_iter()
            .find(|definition| definition.store_id == store_id)
            .map(|definition| definition.policy.capacity),
        Err(_) => None,
    };
    let Some(capacity) = capacity else {
        return Ok(api_error(
            "profile_s3_unavailable",
            "profile S3 capacity policy is unavailable",
        ));
    };
    let backend = match FolderBackend::open(binding.backend_root, binding.manifest, capacity, 0) {
        Ok(backend) => backend,
        Err(error) => return Ok(api_error("profile_s3_unavailable", error.to_string())),
    };
    let offset = match usize::try_from(request.offset) {
        Ok(offset) => offset,
        Err(_) => {
            return Ok(api_error(
                "profile_s3_invalid_request",
                "profile S3 list offset is too large",
            ))
        }
    };
    let page = match list_profile_objects_page(
        &backend,
        request.prefix.as_deref(),
        offset,
        usize::from(request.limit),
    ) {
        Ok(page) => page,
        Err(error) => return Ok(api_error("profile_s3_list_failed", error.to_string())),
    };
    Ok(DaemonApiResponse::ProfileS3List(profile_s3_list_response(
        store_id, page,
    )))
}

pub(super) fn diagnostics<S, C>(
    handler: &DaemonRequestHandler<S, C>,
    request: ProfileDiagnosticsRequest,
    actor: Option<&DaemonLocalActor>,
) -> Result<DaemonApiResponse, DaemonRequestHandlerError>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    let store_id = match handler.authorize_endpoint_read(actor, &request.store_id) {
        Ok(store_id) => store_id,
        Err(error) => return Ok(api_error(error.code(), error.to_string())),
    };
    let lifecycle_state = match profile_binding_lifecycle_state(
        &handler.profile_binding_registry_path,
        store_id.as_str(),
    ) {
        Ok(state) => state,
        Err(_) => {
            return Ok(api_error(
                "profile_diagnostics_unavailable",
                "profile lifecycle state could not be inspected",
            ))
        }
    };
    let binding = match read_profile_binding_record(
        &handler.profile_binding_registry_path,
        store_id.as_str(),
    ) {
        Ok(Some(binding)) => match binding.validate_and_canonicalize() {
            Ok(binding) => binding,
            Err(error) => {
                return Ok(api_error(
                    "profile_diagnostics_unavailable",
                    error.to_string(),
                ))
            }
        },
        Ok(None) | Err(_) => {
            return Ok(api_error(
                "profile_diagnostics_unavailable",
                "profile diagnostics requires a registered bounded folder profile",
            ))
        }
    };
    if binding.manifest.deployment_profile != DeploymentProfile::Folder {
        return Ok(api_error(
            "profile_diagnostics_unavailable",
            "profile diagnostics is available for bounded folder profiles only",
        ));
    }
    let capacity = match read_store_registry(&handler.store_registry_path) {
        Ok(definitions) => definitions
            .into_iter()
            .find(|definition| definition.store_id == store_id)
            .map(|definition| definition.policy.capacity),
        Err(_) => None,
    };
    let Some(capacity) = capacity else {
        return Ok(api_error(
            "profile_diagnostics_unavailable",
            "profile diagnostics capacity policy is unavailable",
        ));
    };
    let backend = match FolderBackend::open(binding.backend_root, binding.manifest, capacity, 0) {
        Ok(backend) => backend,
        Err(error) => {
            return Ok(api_error(
                "profile_diagnostics_unavailable",
                error.to_string(),
            ))
        }
    };
    let reconciliation_path = handler
        .profile_binding_registry_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("profile-reconciliation")
        .join(store_id.as_str())
        .join(format!("{}.json", store_id));
    let last_reconciliation_at_unix_seconds = ReconciliationManifest::load(&reconciliation_path)
        .ok()
        .map(|manifest| manifest.updated_at_unix_seconds);
    let summary = match profile_diagnostics(&backend, last_reconciliation_at_unix_seconds) {
        Ok(summary) => summary,
        Err(error) => return Ok(api_error("profile_diagnostics_failed", error.to_string())),
    };
    let actionable_message = match lifecycle_state {
        ProfileBindingLifecycleState::Active => summary.actionable_message,
        ProfileBindingLifecycleState::Retiring => Some(
            "profile retirement is incomplete; restart the daemon or retry store delete"
                .to_string(),
        ),
        ProfileBindingLifecycleState::Retired => Some(
            "profile is retired; run store repair STORE to preview reactivation, then rerun with --apply"
                .to_string(),
        ),
        ProfileBindingLifecycleState::Recovering => Some(
            "profile reactivation is incomplete; restart the daemon or retry store repair STORE --apply"
                .to_string(),
        ),
    };
    Ok(DaemonApiResponse::ProfileDiagnostics(
        ProfileDiagnosticsResponse {
            schema_version: crate::api::PROFILE_DIAGNOSTICS_SCHEMA_VERSION.to_string(),
            store_id,
            profile: DeploymentProfile::Folder,
            lifecycle_state: lifecycle_state.into(),
            state: summary.state,
            catalogue_object_count: summary.catalogue_object_count,
            backend_object_count: summary.backend_object_count,
            uncatalogued_backend_object_count: summary.uncatalogued_backend_object_count,
            catalogue_missing_backend_object_count: summary.catalogue_missing_backend_object_count,
            last_reconciliation_at_unix_seconds: summary.last_reconciliation_at_unix_seconds,
            actionable_message,
        },
    ))
}

fn api_error(code: impl Into<String>, message: impl Into<String>) -> DaemonApiResponse {
    DaemonApiResponse::Error(DaemonApiErrorResponse::new(code, message))
}
