use super::*;
use crate::api::ProfileBindingOperation;

static OBJECT_STORE_CREATION_LOCK: std::sync::OnceLock<std::sync::Mutex<()>> =
    std::sync::OnceLock::new();

pub(super) fn create_object_store_with_registry(
    request: CreateObjectStoreRequest,
    registry_path: impl AsRef<Path>,
    custody_catalog: &dasobjectstore_object_service::CustodyCatalogBinding,
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
        upsert_store_definition_with_custody_catalog(registry_path, definition, custody_catalog)?;
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
    let intent_path =
        crate::runtime::object_store_creation_intent_path(crate::runtime::DEFAULT_DAEMON_STATE_DIR);
    create_object_store_with_capacity_and_intent_path(
        controller,
        request,
        accepted_at_utc,
        intent_path,
        default_store_registry_path(),
    )
}

fn create_object_store_with_capacity_and_intent_path<R>(
    controller: &GarageServiceController<R>,
    mut request: CreateObjectStoreRequest,
    accepted_at_utc: &str,
    intent_path: impl AsRef<Path>,
    registry_path: impl AsRef<Path>,
) -> Result<CreateObjectStoreResponse, DaemonServiceRuntimeError>
where
    R: ServiceCommandRunner,
{
    let custody_catalog = controller.normal_custody_catalog_binding()?;
    request.validate().map_err(|error| {
        DaemonServiceRuntimeError::ObjectService(ObjectServiceError::InvalidConfiguration(
            error.to_string(),
        ))
    })?;
    if request.dry_run {
        return create_object_store_with_registry(
            request,
            registry_path,
            custody_catalog,
            accepted_at_utc,
        );
    }
    let _creation_guard = OBJECT_STORE_CREATION_LOCK
        .get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .map_err(|_| DaemonServiceRuntimeError::UnsupportedOperation {
            operation: "ObjectStore creation lock is unavailable".to_string(),
        })?;
    let actor = request.administrator_actor.clone().ok_or_else(|| {
        DaemonServiceRuntimeError::UnsupportedOperation {
            operation: "ObjectStore creation requires a peer-authenticated administrator"
                .to_string(),
        }
    })?;
    let mut intent = crate::runtime::begin_object_store_creation_intent(
        intent_path.as_ref(),
        &request,
        &actor,
        accepted_at_utc,
    )
    .map_err(creation_intent_error)?;
    // Always return the server-derived actor retained by the durable intent.
    request.administrator_actor = Some(intent.administrator_actor.clone());
    if intent.phase == crate::runtime::ObjectStoreCreationPhase::Complete {
        return Ok(CreateObjectStoreResponse::accepted(
            intent.job_id,
            intent.accepted_at_utc,
            request,
        ));
    }
    let definition = request.registry_definition().map_err(|error| {
        DaemonServiceRuntimeError::ObjectService(ObjectServiceError::InvalidConfiguration(
            error.to_string(),
        ))
    })?;
    let registry_path = registry_path.as_ref();
    let definition_published = if let Some(existing) =
        read_store_registry_with_custody_catalog(registry_path, custody_catalog)?
            .into_iter()
            .find(|existing| existing.store_id == definition.store_id)
    {
        if existing != definition {
            return Err(DaemonServiceRuntimeError::UnsupportedOperation {
                operation: format!(
                    "ObjectStore {} already has a conflicting definition",
                    definition.store_id
                ),
            });
        }
        true
    } else {
        false
    };
    if definition_published {
        // A matching published definition is already authoritative. Ensure its
        // capacity policy is present, but never claim ownership of or roll back
        // a ledger that predates this replay.
        controller
            .initialize_store_capacity(&definition.store_id, definition.policy.capacity.clone())?;
        intent = crate::runtime::advance_object_store_creation_intent(
            intent_path.as_ref(),
            &intent,
            crate::runtime::ObjectStoreCreationPhase::DefinitionPublished,
            false,
        )
        .map_err(creation_intent_error)?;
    } else if intent.phase == crate::runtime::ObjectStoreCreationPhase::Validated {
        // Checkpoint ownership before the external ledger write. On replay, an
        // exact-policy ledger with no published store definition is an orphan
        // owned by this creation intent, regardless of whether initialize
        // reports that this particular call created the file.
        intent = crate::runtime::advance_object_store_creation_intent(
            intent_path.as_ref(),
            &intent,
            crate::runtime::ObjectStoreCreationPhase::CapacityInitializing,
            false,
        )
        .map_err(creation_intent_error)?;
    }
    if intent.phase == crate::runtime::ObjectStoreCreationPhase::CapacityInitializing {
        controller
            .initialize_store_capacity(&definition.store_id, definition.policy.capacity.clone())?;
        intent = crate::runtime::advance_object_store_creation_intent(
            intent_path.as_ref(),
            &intent,
            crate::runtime::ObjectStoreCreationPhase::CapacityInitialized,
            true,
        )
        .map_err(creation_intent_error)?;
    }
    if intent.phase == crate::runtime::ObjectStoreCreationPhase::CapacityInitialized {
        if let Err(error) = upsert_store_definition_with_custody_catalog(
            registry_path,
            definition.clone(),
            custody_catalog,
        ) {
            if intent.capacity_created {
                controller.rollback_initialized_store_capacity(&definition.store_id)?;
                crate::runtime::advance_object_store_creation_intent(
                    intent_path.as_ref(),
                    &intent,
                    crate::runtime::ObjectStoreCreationPhase::Validated,
                    false,
                )
                .map_err(creation_intent_error)?;
            }
            return Err(error.into());
        }
        intent = crate::runtime::advance_object_store_creation_intent(
            intent_path.as_ref(),
            &intent,
            crate::runtime::ObjectStoreCreationPhase::DefinitionPublished,
            intent.capacity_created,
        )
        .map_err(creation_intent_error)?;
    }
    if intent.phase == crate::runtime::ObjectStoreCreationPhase::DefinitionPublished {
        intent = crate::runtime::advance_object_store_creation_intent(
            intent_path.as_ref(),
            &intent,
            crate::runtime::ObjectStoreCreationPhase::Complete,
            intent.capacity_created,
        )
        .map_err(creation_intent_error)?;
    }
    Ok(CreateObjectStoreResponse::accepted(
        intent.job_id,
        intent.accepted_at_utc,
        request,
    ))
}

fn creation_intent_error(
    error: crate::runtime::ObjectStoreCreationIntentError,
) -> DaemonServiceRuntimeError {
    DaemonServiceRuntimeError::UnsupportedOperation {
        operation: error.to_string(),
    }
}

#[cfg(test)]
#[path = "object_store_creation_saga_tests.rs"]
mod object_store_creation_saga_tests;

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
