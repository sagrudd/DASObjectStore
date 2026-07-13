use super::*;

pub(super) fn create_object_store_with_registry(
    request: CreateObjectStoreRequest,
    registry_path: impl AsRef<Path>,
    accepted_at_utc: &str,
) -> Result<CreateObjectStoreResponse, DaemonServiceRuntimeError> {
    request.validate().map_err(|error| {
        DaemonServiceRuntimeError::ObjectService(ObjectServiceError::InvalidConfiguration(
            error.to_string(),
        ))
    })?;
    let definition = request.registry_definition().map_err(|error| {
        DaemonServiceRuntimeError::ObjectService(ObjectServiceError::InvalidConfiguration(
            error.to_string(),
        ))
    })?;
    if !request.dry_run {
        upsert_store_definition(registry_path, definition)?;
    }
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

pub(super) fn create_object_store_with_capacity<R>(
    controller: &GarageServiceController<R>,
    request: CreateObjectStoreRequest,
    accepted_at_utc: &str,
) -> Result<CreateObjectStoreResponse, DaemonServiceRuntimeError>
where
    R: ServiceCommandRunner,
{
    if !request.dry_run {
        controller.initialize_store_capacity_from_request(&request)?;
    }
    create_object_store_with_registry(request, default_store_registry_path(), accepted_at_utc)
}

pub(super) fn register_profile_binding(
    request: ProfileBindingRequest,
    registry_path: impl AsRef<Path>,
    accepted_at_utc: &str,
) -> Result<ProfileBindingResponse, DaemonServiceRuntimeError> {
    request.validate().map_err(|error| {
        DaemonServiceRuntimeError::ObjectService(ObjectServiceError::InvalidConfiguration(
            error.to_string(),
        ))
    })?;
    if !request.dry_run {
        upsert_profile_binding(
            registry_path,
            BackendProfileBinding {
                manifest: request.manifest.clone(),
                backend_root: request.backend_root.clone(),
                ssd_staging_root: request.ssd_staging_root.clone(),
            },
        )?;
    }
    let job_id_value = format!(
        "profile-binding-{}",
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
    Ok(ProfileBindingResponse::accepted(
        job_id,
        accepted_at_utc,
        request,
    ))
}

/// Create the daemon-private folder namespace for a new bounded folder store.
///
/// Adoption deliberately remains binding-only here: it needs a caller-owned
/// reconciliation checkpoint and action-time confirmation before any user
/// files can be copied into the managed namespace.
pub(super) fn ensure_profile_backend(
    request: &ProfileBindingRequest,
) -> Result<(), DaemonServiceRuntimeError> {
    if request.operation != ProfileBindingOperation::Create
        || request.manifest.deployment_profile != DeploymentProfile::Folder
    {
        return Ok(());
    }
    FolderBackend::open(
        request.backend_root.clone(),
        request.manifest.clone(),
        request.capacity.clone(),
        0,
    )
    .map(|_| ())
    .map_err(|error| DaemonServiceRuntimeError::UnsupportedOperation {
        operation: format!("open profile backend: {error}"),
    })
}

pub(super) fn resolve_authorization_store_id(
    endpoint: &StoreId,
    store_registry_path: &Path,
    subobject_registry_path: &Path,
) -> Result<StoreId, IngestAuthorizationFailure> {
    let stores = read_store_registry(store_registry_path)?;
    let store_match = stores
        .iter()
        .find(|definition| definition.store_id == *endpoint)
        .map(|definition| definition.store_id.clone());
    let subobjects = read_subobject_registry(subobject_registry_path)?;
    let subobject_match = subobjects
        .iter()
        .find(|definition| definition.name == endpoint.as_str());

    match (store_match, subobject_match) {
        (Some(_), Some(_)) => Err(IngestAuthorizationFailure::AmbiguousEndpoint {
            endpoint: endpoint.clone(),
        }),
        (Some(store_id), None) => Ok(store_id),
        (None, Some(subobject)) => Ok(subobject.store_id.clone()),
        (None, None) => Err(IngestAuthorizationFailure::UnknownEndpoint {
            endpoint: endpoint.clone(),
            store_registry_path: store_registry_path.to_path_buf(),
            subobject_registry_path: subobject_registry_path.to_path_buf(),
        }),
    }
}

pub(super) fn stable_easyconnect_id(prefix: &str, subject: &str, timestamp: &str) -> String {
    let mut suffix = String::new();
    for character in subject.chars().chain(timestamp.chars()) {
        if character.is_ascii_alphanumeric() {
            suffix.push(character.to_ascii_lowercase());
        } else if !suffix.ends_with('-') {
            suffix.push('-');
        }
    }
    let suffix = suffix.trim_matches('-');
    if suffix.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix}-{suffix}")
    }
}

pub(super) fn rotated_easyconnect_renewal_token(session_id: &str, renewed_at_utc: &str) -> String {
    let suffix = renewed_at_utc
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    format!("renewal-{session_id}-{suffix}")
}
