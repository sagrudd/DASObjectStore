use super::*;
use crate::api::ProfileBindingOperation;

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

/// Validate an idempotent provisioning request against the persisted binding.
///
/// Provisioning may only reuse an identical binding. It must never silently
/// replace a manifest, backend root, or staging root under the same store id;
/// callers that need that transition must use an explicit create/adopt flow.
pub(super) fn validate_profile_provision_claim(
    registry_path: impl AsRef<Path>,
    binding: BackendProfileBinding,
) -> Result<bool, DaemonServiceRuntimeError> {
    let desired = binding.validate_and_canonicalize()?;
    let existing = read_profile_binding_record(registry_path, desired.manifest.store_id.as_str())?;
    let Some(existing) = existing else {
        return Ok(false);
    };
    let existing = existing.validate_and_canonicalize()?;
    if existing != desired {
        return Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!(
                "profile provisioning conflicts with existing binding for ObjectStore {}",
                desired.manifest.store_id
            ),
        });
    }
    Ok(true)
}

/// Create only the final folder component needed by an idempotent provision.
///
/// Claim validation canonicalizes roots to prevent aliasing and symlink
/// escapes, but a first provision has no root to canonicalize yet. The daemon
/// may create that one explicit leaf after validating its existing parent; it
/// never creates a missing parent tree or performs this behavior for drive or
/// appliance profiles.
pub(super) fn prepare_profile_provision_root(
    request: &ProfileBindingRequest,
) -> Result<bool, DaemonServiceRuntimeError> {
    if request.operation != ProfileBindingOperation::Provision
        || request.manifest.deployment_profile != DeploymentProfile::Folder
        || request.backend_root.exists()
    {
        return Ok(false);
    }
    let parent = request.backend_root.parent().ok_or_else(|| {
        DaemonServiceRuntimeError::UnsupportedOperation {
            operation: "profile provision backend root has no parent".to_string(),
        }
    })?;
    let canonical_parent = fs::canonicalize(parent).map_err(|error| {
        DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!(
                "profile provision requires an existing backend parent {}: {error}",
                parent.display()
            ),
        }
    })?;
    if !canonical_parent.is_dir() {
        return Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!(
                "profile provision backend parent is not a directory: {}",
                canonical_parent.display()
            ),
        });
    }
    fs::create_dir(&request.backend_root).map_err(|error| {
        DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!(
                "create profile provision backend root {}: {error}",
                request.backend_root.display()
            ),
        }
    })?;
    Ok(true)
}

pub(super) fn rollback_empty_profile_provision_root(request: &ProfileBindingRequest) {
    let _ = fs::remove_dir(&request.backend_root);
}

/// Create the daemon-private folder namespace for a new bounded folder store.
///
/// Create the private namespace and, for explicit adoption, execute the
/// daemon-owned restart-safe reconciliation checkpoint before publishing the
/// binding.
pub(super) fn ensure_profile_backend(
    request: &ProfileBindingRequest,
    profile_registry_path: &Path,
) -> Result<Option<ProfileBackendPreparation>, DaemonServiceRuntimeError> {
    if request.manifest.deployment_profile != DeploymentProfile::Folder {
        return Ok(None);
    }
    let mut backend = FolderBackend::open(
        request.backend_root.clone(),
        request.manifest.clone(),
        request.capacity.clone(),
        0,
    )
    .map_err(|error| DaemonServiceRuntimeError::UnsupportedOperation {
        operation: format!("open profile backend: {error}"),
    })?;
    let inspection = backend.inspect_user_tree().map_err(|error| {
        DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!("inspect profile backend: {error}"),
        }
    })?;
    let mut preparation = ProfileBackendPreparation {
        inspection,
        adopted_object_count: 0,
        adopted_bytes: 0,
    };
    if request.operation == ProfileBindingOperation::Adopt {
        let checkpoint_root = profile_registry_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("profile-reconciliation")
            .join(request.manifest.store_id.as_str());
        let checkpoint_path = checkpoint_root.join(format!("{}.json", request.manifest.store_id));
        let mut manifest = if checkpoint_path.exists() {
            ReconciliationManifest::load(&checkpoint_path).map_err(|error| {
                DaemonServiceRuntimeError::UnsupportedOperation {
                    operation: format!("load profile reconciliation checkpoint: {error}"),
                }
            })?
        } else {
            ReconciliationManifest::new(request.manifest.store_id.as_str(), None)
        };
        let records = backend
            .adopt_user_tree_reconciliation(
                &checkpoint_path,
                &mut manifest,
                &format!("profile-adopt-{}", request.manifest.store_id),
            )
            .map_err(|error| DaemonServiceRuntimeError::UnsupportedOperation {
                operation: format!("adopt profile backend: {error}"),
            })?;
        preparation.adopted_object_count = records.len();
        preparation.adopted_bytes = records.iter().map(|record| record.size_bytes).sum();
        preparation.inspection = backend.inspect_user_tree().map_err(|error| {
            DaemonServiceRuntimeError::UnsupportedOperation {
                operation: format!("inspect adopted profile backend: {error}"),
            }
        })?;
    }
    Ok(Some(preparation))
}

pub(super) struct ProfileBackendPreparation {
    pub inspection: FolderInspectionReport,
    pub adopted_object_count: usize,
    pub adopted_bytes: u64,
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
