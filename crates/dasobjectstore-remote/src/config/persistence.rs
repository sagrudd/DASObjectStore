use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub fn default_config_path() -> Result<PathBuf, RemoteConfigError> {
    if let Ok(path) = env::var("DASOBJECTSTORE_REMOTE_CONFIG") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }
    let home = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .ok_or(RemoteConfigError::MissingHome)?;
    Ok(PathBuf::from(home)
        .join(".config")
        .join("dasobjectstore")
        .join("remote.json"))
}

pub fn read_optional_config(path: &Path) -> Result<Option<RemoteConfig>, RemoteConfigError> {
    let parent = path.parent().ok_or_else(|| {
        RemoteConfigError::Invalid("remote configuration path has no parent".to_string())
    })?;
    let state_path = parent.join("state.json");
    if state_path.exists() {
        let state: RemoteStatePointer = serde_json::from_slice(&fs::read(&state_path)?)?;
        if state.schema_version != REMOTE_STATE_SCHEMA_VERSION
            || state.current_generation.trim().is_empty()
        {
            return Err(RemoteConfigError::Integrity {
                code: "configuration_transaction_incomplete",
                message: "the remote-state generation pointer is invalid".to_string(),
                remediation: "dasobjectstore-remote config repair --dry-run --json".to_string(),
            });
        }
        let generation_path = parent
            .join("generations")
            .join(&state.current_generation)
            .join("remote.json");
        let config: RemoteConfig =
            serde_json::from_slice(&fs::read(&generation_path).map_err(|error| {
                if error.kind() == io::ErrorKind::NotFound {
                    RemoteConfigError::Integrity {
                        code: "configuration_transaction_incomplete",
                        message: format!(
                            "committed generation {} is incomplete",
                            state.current_generation
                        ),
                        remediation: "dasobjectstore-remote config repair --dry-run --json"
                            .to_string(),
                    }
                } else {
                    error.into()
                }
            })?)?;
        config.validate_session_integrity()?;
        return Ok(Some(config));
    }
    match fs::read_to_string(path) {
        Ok(raw) => {
            let mut config: RemoteConfig = serde_json::from_str(&raw)?;
            migrate_legacy_sessions(&mut config)?;
            archive_legacy_config(parent, raw.as_bytes())?;
            write_config(path, &config)?;
            read_optional_config(path)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn archive_legacy_config(parent: &Path, raw: &[u8]) -> Result<(), RemoteConfigError> {
    let diagnostics = parent.join("diagnostics");
    fs::create_dir_all(&diagnostics)?;
    let archive = diagnostics.join(format!(
        "legacy-remote-{}-{}.json",
        std::process::id(),
        GENERATION_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    write_private_file(&archive, raw)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RemoteConfigDoctorReport {
    pub schema_version: &'static str,
    pub configuration_schema_version: String,
    pub current_generation: u64,
    pub bindings: Vec<RemoteConfigBindingReport>,
    pub duplicate_session_count: usize,
    pub stale_session_count: usize,
    pub s3_control_generation_consistent: bool,
    pub profile_associations_consistent: bool,
    pub required_corrective_action: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RemoteConfigBindingReport {
    pub appliance_id: String,
    pub store_id: String,
    pub credential_expires_at: String,
    pub expired: bool,
    pub renewal_available: bool,
    pub s3_profile: Option<String>,
    pub certificate_binding_recorded: bool,
    pub trust_identity_matches: bool,
}

pub fn doctor_config(path: &Path) -> Result<RemoteConfigDoctorReport, RemoteConfigError> {
    let config = read_config_for_diagnostics(path)?.unwrap_or_else(|| RemoteConfig {
        schema_version: REMOTE_CONFIG_SCHEMA_VERSION.to_string(),
        generation: 0,
        endpoint_url: String::new(),
        region: DEFAULT_REGION.to_string(),
        profile: DEFAULT_PROFILE.to_string(),
        auth_authority: RemoteAuthAuthority::AwsProfile,
        username: None,
        credential_helper: None,
        default_appliance_id: None,
        paired_appliances: Vec::new(),
        s3_profiles: Vec::new(),
        session_bindings: Vec::new(),
    });
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| RemoteConfigError::Invalid(error.to_string()))?
        .as_secs() as i64;
    let duplicate_session_count = config.duplicate_store_binding_count();
    let bindings = config
        .session_bindings
        .iter()
        .map(|binding| {
            let expired = dasobjectstore_core::utc::parse_canonical_utc_timestamp_seconds(
                &binding.session.expires_at,
            )
            .is_none_or(|expiry| expiry <= now);
            RemoteConfigBindingReport {
                appliance_id: binding.appliance_id.clone(),
                store_id: binding.store_id.clone(),
                credential_expires_at: binding.session.expires_at.clone(),
                expired,
                renewal_available: binding
                    .session
                    .renewal
                    .as_ref()
                    .and_then(|renewal| renewal.renewal_token.as_ref())
                    .is_some(),
                s3_profile: binding.s3_profile.clone(),
                certificate_binding_recorded: !binding.trust_fingerprint_sha256.trim().is_empty()
                    && binding.trust_fingerprint_sha256 != "legacy-trust-unavailable",
                trust_identity_matches: enrolled_identity_for_binding(binding)
                    .is_some_and(|identity| identity == binding.appliance_id),
            }
        })
        .collect::<Vec<_>>();
    let stale_session_count = bindings.iter().filter(|binding| binding.expired).count();
    let profile_associations_consistent = config
        .session_bindings
        .iter()
        .all(|binding| config.profile_association_consistent(binding));
    let required_corrective_action =
        if duplicate_session_count > 0 || !profile_associations_consistent {
            Some("dasobjectstore-remote config repair --dry-run --json".to_string())
        } else if stale_session_count > 0 {
            Some(
                "dasobjectstore-remote login HOST OBJECTSTORE --username USER --set-s3-config"
                    .to_string(),
            )
        } else {
            None
        };
    Ok(RemoteConfigDoctorReport {
        schema_version: "dasobjectstore.remote_config_doctor.v1",
        configuration_schema_version: config.schema_version,
        current_generation: config.generation,
        bindings,
        duplicate_session_count,
        stale_session_count,
        s3_control_generation_consistent: profile_associations_consistent,
        profile_associations_consistent,
        required_corrective_action,
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RemoteConfigRepairReport {
    pub schema_version: &'static str,
    pub applied: bool,
    pub backup_created: bool,
    pub current_generation: u64,
    pub action: String,
    pub retain_bindings: Vec<RemoteConfigRepairBinding>,
    pub retire_bindings: Vec<RemoteConfigRepairBinding>,
    pub archived_generation: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RemoteConfigRepairBinding {
    pub appliance_id: String,
    pub store_id: String,
    pub s3_profile: Option<String>,
    pub credential_expires_at: String,
}

#[derive(Debug)]
struct SessionReconciliationPlan {
    config: RemoteConfig,
    retain_bindings: Vec<RemoteConfigRepairBinding>,
    retire_bindings: Vec<RemoteConfigRepairBinding>,
}

fn read_config_for_diagnostics(path: &Path) -> Result<Option<RemoteConfig>, RemoteConfigError> {
    let parent = path.parent().ok_or_else(|| {
        RemoteConfigError::Invalid("remote configuration path has no parent".to_string())
    })?;
    if let Some(state) = read_state_pointer(parent)? {
        let generation_path = parent
            .join("generations")
            .join(state.current_generation)
            .join("remote.json");
        return Ok(Some(serde_json::from_slice(&fs::read(generation_path)?)?));
    }
    match fs::read(path) {
        Ok(raw) => {
            let mut config: RemoteConfig = serde_json::from_slice(&raw)?;
            config.schema_version = REMOTE_CONFIG_SCHEMA_VERSION.to_string();
            if config.session_bindings.is_empty() {
                migrate_legacy_sessions(&mut config)?;
            }
            Ok(Some(config))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn read_state_pointer(parent: &Path) -> Result<Option<RemoteStatePointer>, RemoteConfigError> {
    let path = parent.join("state.json");
    match fs::read(path) {
        Ok(raw) => {
            let state: RemoteStatePointer = serde_json::from_slice(&raw)?;
            if state.schema_version != REMOTE_STATE_SCHEMA_VERSION
                || state.current_generation.trim().is_empty()
            {
                return Err(RemoteConfigError::Integrity {
                    code: "configuration_transaction_incomplete",
                    message: "the remote-state generation pointer is invalid".to_string(),
                    remediation: "dasobjectstore-remote config repair --dry-run --json".to_string(),
                });
            }
            Ok(Some(state))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn enrolled_identity_for_binding(binding: &RemoteSessionBinding) -> Option<String> {
    let url = reqwest::Url::parse(&binding.control_base_url).ok()?;
    crate::trust::load_trust(url.host_str()?, url.port_or_known_default().unwrap_or(8448))
        .ok()
        .flatten()
        .map(|record| record.appliance_id)
}

fn binding_report(binding: &RemoteSessionBinding) -> RemoteConfigRepairBinding {
    RemoteConfigRepairBinding {
        appliance_id: binding.appliance_id.clone(),
        store_id: binding.store_id.clone(),
        s3_profile: binding.s3_profile.clone(),
        credential_expires_at: binding.session.expires_at.clone(),
    }
}

fn session_binding_is_complete(binding: &RemoteSessionBinding) -> bool {
    !binding.appliance_id.trim().is_empty()
        && !binding.store_id.trim().is_empty()
        && !binding.control_base_url.trim().is_empty()
        && !binding.s3_endpoint_url.trim().is_empty()
        && !binding.bucket.trim().is_empty()
        && !binding.session.session_id.trim().is_empty()
        && !binding.session.credentials.access_key_id.trim().is_empty()
        && !binding
            .session
            .credentials
            .secret_access_key
            .trim()
            .is_empty()
}

fn reconcile_session_bindings_with(
    config: &RemoteConfig,
    enrolled_identity: impl Fn(&RemoteSessionBinding) -> Option<String>,
) -> SessionReconciliationPlan {
    let mut stores = BTreeMap::<&str, Vec<usize>>::new();
    for (index, binding) in config.session_bindings.iter().enumerate() {
        stores.entry(&binding.store_id).or_default().push(index);
    }
    let mut retained = BTreeSet::new();
    let mut retired = BTreeSet::new();
    let mut replacement_default = None;
    for indices in stores.values() {
        if indices.len() == 1 {
            retained.insert(indices[0]);
            continue;
        }
        let enrolled = indices
            .iter()
            .filter_map(|index| enrolled_identity(&config.session_bindings[*index]))
            .collect::<BTreeSet<_>>();
        let selected = (enrolled.len() == 1)
            .then(|| {
                indices
                    .iter()
                    .copied()
                    .filter(|index| {
                        let binding = &config.session_bindings[*index];
                        session_binding_is_complete(binding)
                            && enrolled.contains(binding.appliance_id.as_str())
                    })
                    .max_by(|left, right| {
                        let left = &config.session_bindings[*left].session;
                        let right = &config.session_bindings[*right].session;
                        left.expires_at
                            .cmp(&right.expires_at)
                            .then_with(|| left.issued_at.cmp(&right.issued_at))
                            .then_with(|| left.session_id.cmp(&right.session_id))
                    })
            })
            .flatten();
        if let Some(selected) = selected {
            retained.insert(selected);
            replacement_default = Some(config.session_bindings[selected].appliance_id.clone());
        } else if enrolled.len() == 1 {
            replacement_default = enrolled.into_iter().next();
        }
        for index in indices {
            if Some(*index) != selected {
                retired.insert(*index);
            }
        }
    }

    let mut repaired = config.clone();
    repaired.session_bindings = config
        .session_bindings
        .iter()
        .enumerate()
        .filter(|(index, _)| retained.contains(index))
        .map(|(_, binding)| binding.clone())
        .collect();
    let bound_stores = config
        .session_bindings
        .iter()
        .map(|binding| binding.store_id.as_str())
        .collect::<BTreeSet<_>>();
    repaired
        .s3_profiles
        .retain(|association| !bound_stores.contains(association.store_id.as_str()));
    repaired.s3_profiles.extend(
        repaired
            .session_bindings
            .iter()
            .filter_map(profile_association_from_binding),
    );
    if repaired
        .default_appliance_id
        .as_ref()
        .is_some_and(|current| {
            retired
                .iter()
                .any(|index| config.session_bindings[*index].appliance_id == *current)
        })
    {
        repaired.default_appliance_id = replacement_default;
    }
    SessionReconciliationPlan {
        retain_bindings: retained
            .into_iter()
            .map(|index| binding_report(&config.session_bindings[index]))
            .collect(),
        retire_bindings: retired
            .into_iter()
            .map(|index| binding_report(&config.session_bindings[index]))
            .collect(),
        config: repaired,
    }
}

fn profile_association_from_binding(
    binding: &RemoteSessionBinding,
) -> Option<crate::aws_profile::AwsProfileAssociation> {
    let profile = binding.s3_profile.as_ref()?;
    let appliance_host = reqwest::Url::parse(&binding.control_base_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .unwrap_or_else(|| binding.control_base_url.clone());
    Some(crate::aws_profile::AwsProfileAssociation {
        profile: profile.clone(),
        appliance_host,
        store_id: binding.store_id.clone(),
        endpoint_url: binding.s3_endpoint_url.clone(),
        bucket: binding.bucket.clone(),
        region: binding.region.clone(),
        addressing_style: binding.addressing_style.clone(),
        ca_bundle_path: None,
        temporary_credentials: binding.session.credentials.session_token.is_some(),
        expires_at: Some(binding.session.expires_at.clone()),
    })
}

pub fn repair_config(
    path: &Path,
    apply: bool,
) -> Result<RemoteConfigRepairReport, RemoteConfigError> {
    repair_config_with(path, apply, enrolled_identity_for_binding)
}

fn repair_config_with(
    path: &Path,
    apply: bool,
    enrolled_identity: impl Fn(&RemoteSessionBinding) -> Option<String>,
) -> Result<RemoteConfigRepairReport, RemoteConfigError> {
    let parent = path.parent().ok_or_else(|| {
        RemoteConfigError::Invalid("remote configuration path has no parent".to_string())
    })?;
    let had_legacy = path.exists() && !parent.join("state.json").exists();
    let lock = apply
        .then(|| acquire_config_transaction(path))
        .transpose()?;
    let config =
        read_config_for_diagnostics(path)?.ok_or_else(|| RemoteConfigError::Integrity {
            code: "configuration_migration_required",
            message: "no remote authentication state exists".to_string(),
            remediation: "dasobjectstore-remote login HOST OBJECTSTORE --username USER".to_string(),
        })?;
    let plan = reconcile_session_bindings_with(&config, enrolled_identity);
    let state = read_state_pointer(parent)?;
    let changed = plan.config != config;
    let write_required = changed || had_legacy;
    if apply && had_legacy {
        archive_legacy_config(parent, &fs::read(path)?)?;
    }
    let next_generation = if apply && write_required {
        write_config_locked(
            path,
            &plan.config,
            lock.as_ref().expect("apply repair holds transaction lock"),
        )?;
        config.generation.saturating_add(1).max(1)
    } else {
        config.generation
    };
    Ok(RemoteConfigRepairReport {
        schema_version: "dasobjectstore.remote_config_repair.v2",
        applied: apply && write_required,
        backup_created: apply && had_legacy,
        current_generation: next_generation,
        action: if !apply && had_legacy && changed {
            "migrate_and_reconcile_legacy_configuration".to_string()
        } else if !apply && had_legacy {
            "migrate_legacy_configuration".to_string()
        } else if !apply && changed {
            "reconcile_authoritative_store_bindings".to_string()
        } else if !apply {
            "validate_current_generation".to_string()
        } else if had_legacy && changed {
            "legacy_configuration_migrated_and_reconciled".to_string()
        } else if had_legacy {
            "legacy_configuration_migrated".to_string()
        } else if changed {
            "authoritative_store_bindings_reconciled".to_string()
        } else {
            "current_generation_validated".to_string()
        },
        retain_bindings: plan.retain_bindings,
        retire_bindings: plan.retire_bindings,
        archived_generation: (apply && write_required)
            .then(|| state.map(|pointer| pointer.current_generation))
            .flatten(),
    })
}

pub fn write_config(path: &Path, config: &RemoteConfig) -> Result<(), RemoteConfigError> {
    let lock = acquire_config_transaction(path)?;
    write_config_locked(path, config, &lock)
}

pub struct ConfigTransactionLock {
    _file: fs::File,
}

pub fn acquire_config_transaction(path: &Path) -> Result<ConfigTransactionLock, RemoteConfigError> {
    let parent = path.parent().ok_or_else(|| {
        RemoteConfigError::Invalid("remote configuration path has no parent".to_string())
    })?;
    fs::create_dir_all(parent)?;
    let lock_path = parent.join(".remote.json.lock");
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)?;
    lock.lock()?;
    Ok(ConfigTransactionLock { _file: lock })
}

pub fn write_config_locked(
    path: &Path,
    config: &RemoteConfig,
    _lock: &ConfigTransactionLock,
) -> Result<(), RemoteConfigError> {
    let parent = path.parent().ok_or_else(|| {
        RemoteConfigError::Invalid("remote configuration path has no parent".to_string())
    })?;
    let mut committed = config.clone();
    committed.schema_version = REMOTE_CONFIG_SCHEMA_VERSION.to_string();
    committed.generation = committed.generation.saturating_add(1).max(1);
    committed.validate_session_integrity()?;
    let generation_id = format!(
        "generation-{}-{}-{}",
        committed.generation,
        std::process::id(),
        GENERATION_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    );
    let generations = parent.join("generations");
    fs::create_dir_all(&generations)?;
    let generation_temporary = generations.join(format!(".{generation_id}.tmp"));
    fs::create_dir(&generation_temporary)?;
    let raw = serde_json::to_vec_pretty(&committed)?;
    write_private_file(&generation_temporary.join("remote.json"), &raw)?;
    let association = RemoteGenerationAssociation::from_config(&generation_id, &committed);
    write_private_file(
        &generation_temporary.join("association.json"),
        &serde_json::to_vec_pretty(&association)?,
    )?;
    let generation_path = generations.join(&generation_id);
    fs::rename(&generation_temporary, &generation_path)?;
    let state = RemoteStatePointer {
        schema_version: REMOTE_STATE_SCHEMA_VERSION.to_string(),
        current_generation: generation_id,
        config_generation: committed.generation,
    };
    atomic_private_write(
        &parent.join("state.json"),
        &serde_json::to_vec_pretty(&state)?,
    )?;
    // Maintain a compatibility mirror for older clients. Current clients never
    // select from it once the generation pointer exists.
    atomic_private_write(path, &raw)?;
    Ok(())
}

fn atomic_private_write(path: &Path, raw: &[u8]) -> Result<(), RemoteConfigError> {
    let parent = path.parent().ok_or_else(|| {
        RemoteConfigError::Invalid("remote configuration path has no parent".to_string())
    })?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(
        ".remote-{}-{}.tmp",
        std::process::id(),
        GENERATION_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut options = fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    file.write_all(raw)?;
    file.sync_all()?;
    fs::rename(&temporary, path)?;
    restrict_config_permissions(path)?;
    if let Ok(directory) = fs::File::open(parent) {
        let _ = directory.sync_all();
    }
    Ok(())
}

fn write_private_file(path: &Path, raw: &[u8]) -> Result<(), RemoteConfigError> {
    let mut options = fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(raw)?;
    file.sync_all()?;
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct RemoteStatePointer {
    schema_version: String,
    current_generation: String,
    config_generation: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct RemoteGenerationAssociation {
    schema_version: String,
    generation_id: String,
    config_generation: u64,
    bindings: Vec<RemoteGenerationBindingSummary>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct RemoteGenerationBindingSummary {
    appliance_id: String,
    store_id: String,
    s3_profile: Option<String>,
    expires_at: String,
}

impl RemoteGenerationAssociation {
    fn from_config(generation_id: &str, config: &RemoteConfig) -> Self {
        Self {
            schema_version: "dasobjectstore.remote_generation_association.v1".to_string(),
            generation_id: generation_id.to_string(),
            config_generation: config.generation,
            bindings: config
                .session_bindings
                .iter()
                .map(|binding| RemoteGenerationBindingSummary {
                    appliance_id: binding.appliance_id.clone(),
                    store_id: binding.store_id.clone(),
                    s3_profile: binding.s3_profile.clone(),
                    expires_at: binding.session.expires_at.clone(),
                })
                .collect(),
        }
    }
}

fn migrate_legacy_sessions(config: &mut RemoteConfig) -> Result<(), RemoteConfigError> {
    config.schema_version = REMOTE_CONFIG_SCHEMA_VERSION.to_string();
    if !config.session_bindings.is_empty() {
        return config.validate_session_integrity();
    }
    for appliance in &config.paired_appliances {
        let Some(session) = &appliance.session else {
            continue;
        };
        let trust = reqwest::Url::parse(&appliance.appliance_base_url)
            .ok()
            .and_then(|url| {
                crate::trust::load_trust(
                    url.host_str()?,
                    url.port_or_known_default().unwrap_or(8448),
                )
                .ok()
                .flatten()
            });
        for grant in &appliance.object_stores {
            let profile = config
                .s3_profiles
                .iter()
                .find(|association| association.store_id == grant.object_store)
                .map(|association| association.profile.clone());
            config.session_bindings.push(RemoteSessionBinding {
                appliance_id: appliance.appliance_id.clone(),
                store_id: grant.object_store.clone(),
                control_base_url: appliance.appliance_base_url.clone(),
                s3_endpoint_url: config
                    .s3_profiles
                    .iter()
                    .find(|association| association.store_id == grant.object_store)
                    .map(|association| association.endpoint_url.clone())
                    .unwrap_or_else(|| config.endpoint_url.clone()),
                bucket: grant.bucket.clone(),
                region: config.region.clone(),
                addressing_style: config
                    .s3_profiles
                    .iter()
                    .find(|association| association.store_id == grant.object_store)
                    .map(|association| association.addressing_style.clone())
                    .unwrap_or_else(|| "path".to_string()),
                s3_profile: profile,
                tls_trust: appliance.tls_trust,
                site_trust_bundle_path: None,
                trust_fingerprint_sha256: trust
                    .as_ref()
                    .map(|record| record.fingerprint_sha256.clone())
                    .unwrap_or_else(|| "legacy-trust-unavailable".to_string()),
                trust_spki_sha256: trust
                    .as_ref()
                    .map(|record| record.spki_sha256.clone())
                    .unwrap_or_else(|| "legacy-trust-unavailable".to_string()),
                session: session.clone(),
            });
        }
    }
    config.validate_session_integrity()
}

#[cfg(unix)]
fn restrict_config_permissions(path: &Path) -> Result<(), RemoteConfigError> {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_config_permissions(_path: &Path) -> Result<(), RemoteConfigError> {
    Ok(())
}

pub(super) fn default_region() -> String {
    DEFAULT_REGION.to_string()
}

pub(super) fn default_profile() -> String {
    DEFAULT_PROFILE.to_string()
}

pub(super) fn redact_identifier(value: &str) -> String {
    let trimmed = value.trim();
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() <= 8 {
        return REDACTED_SECRET.to_string();
    }
    let prefix = chars.iter().take(4).collect::<String>();
    let suffix = chars
        .iter()
        .skip(chars.len().saturating_sub(4))
        .collect::<String>();
    format!("{prefix}...{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::RemoteAuthAuthority;
    use crate::config::{
        RemoteSessionCredentials, RemoteSessionRenewalMetadata, RemoteUploadSession,
    };
    use uuid::Uuid;

    const OLD_APPLIANCE: &str = "standalone-dasobjectstore@2cb548dc079ab9f55d918bcc";
    const NEW_APPLIANCE: &str = "das-appliance-8eab8d3d-86a0-4f2e-8147-b3df0dc0cc54";

    #[test]
    fn repair_retires_replaced_appliance_binding_and_commits_new_generation() {
        let root =
            std::env::temp_dir().join(format!("das-remote-config-repair-{}", Uuid::new_v4()));
        let path = root.join("remote.json");
        let generation_id = "generation-7-existing";
        let generation = root.join("generations").join(generation_id);
        fs::create_dir_all(&generation).expect("generation directory");
        let config = replacement_config();
        write_private_file(
            &generation.join("remote.json"),
            &serde_json::to_vec_pretty(&config).expect("config JSON"),
        )
        .expect("generation config");
        write_private_file(
            &generation.join("association.json"),
            br#"{"schema_version":"fixture"}"#,
        )
        .expect("generation association");
        atomic_private_write(
            &root.join("state.json"),
            &serde_json::to_vec_pretty(&RemoteStatePointer {
                schema_version: REMOTE_STATE_SCHEMA_VERSION.to_string(),
                current_generation: generation_id.to_string(),
                config_generation: config.generation,
            })
            .expect("state JSON"),
        )
        .expect("state");
        atomic_private_write(
            &path,
            &serde_json::to_vec_pretty(&config).expect("mirror JSON"),
        )
        .expect("mirror");

        assert!(matches!(
            read_optional_config(&path),
            Err(RemoteConfigError::Integrity {
                code: "ambiguous_session_state",
                ..
            })
        ));
        let doctor = doctor_config(&path).expect("doctor inspects invalid generation");
        assert_eq!(doctor.duplicate_session_count, 1);
        assert_eq!(
            doctor.required_corrective_action.as_deref(),
            Some("dasobjectstore-remote config repair --dry-run --json")
        );
        let dry_run =
            repair_config_with(&path, false, |_| Some(NEW_APPLIANCE.to_string())).expect("dry-run");
        assert_eq!(dry_run.action, "reconcile_authoritative_store_bindings");
        assert_eq!(dry_run.retain_bindings.len(), 1);
        assert_eq!(dry_run.retain_bindings[0].appliance_id, NEW_APPLIANCE);
        assert_eq!(dry_run.retire_bindings.len(), 1);
        assert_eq!(dry_run.retire_bindings[0].appliance_id, OLD_APPLIANCE);
        assert!(!serde_json::to_string(&dry_run)
            .expect("report JSON")
            .contains("super-secret"));

        let applied =
            repair_config_with(&path, true, |_| Some(NEW_APPLIANCE.to_string())).expect("apply");
        assert!(applied.applied);
        assert_eq!(applied.action, "authoritative_store_bindings_reconciled");
        assert_eq!(applied.archived_generation.as_deref(), Some(generation_id));
        assert!(generation.exists(), "retired generation remains archived");

        let repaired = read_optional_config(&path)
            .expect("read repaired")
            .expect("config");
        assert_eq!(repaired.session_bindings.len(), 1);
        assert_eq!(repaired.session_bindings[0].appliance_id, NEW_APPLIANCE);
        assert_eq!(repaired.s3_profiles.len(), 1);
        assert_eq!(repaired.s3_profiles[0].store_id, "epic_collection");
        assert_eq!(
            repaired.s3_profiles[0].profile,
            "dasobjectstore-epic_collection"
        );
        repaired
            .validate_session_integrity()
            .expect("one binding and profile association");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn repair_retires_all_duplicate_bindings_when_none_matches_enrolled_trust() {
        let config = replacement_config();
        let plan = reconcile_session_bindings_with(&config, |_| {
            Some("das-appliance-unrelated".to_string())
        });
        assert!(plan.config.session_bindings.is_empty());
        assert!(plan.config.s3_profiles.is_empty());
        assert_eq!(plan.retire_bindings.len(), 2);
        assert!(plan.retain_bindings.is_empty());
    }

    fn replacement_config() -> RemoteConfig {
        let old = binding(OLD_APPLIANCE, "OLDSESSION", "2099-01-01T00:00:00Z");
        let new = binding(NEW_APPLIANCE, "NEWSESSION", "2099-01-02T00:00:00Z");
        RemoteConfig {
            schema_version: REMOTE_CONFIG_SCHEMA_VERSION.to_string(),
            generation: 7,
            endpoint_url: "http://192.168.1.192:3900".to_string(),
            region: "garage".to_string(),
            profile: "dasobjectstore-epic_collection".to_string(),
            auth_authority: RemoteAuthAuthority::LocalPassword,
            username: Some("stephen".to_string()),
            credential_helper: None,
            default_appliance_id: Some(OLD_APPLIANCE.to_string()),
            paired_appliances: Vec::new(),
            s3_profiles: vec![
                profile_association_from_binding(&old).expect("old profile"),
                profile_association_from_binding(&new).expect("new profile"),
            ],
            session_bindings: vec![old, new],
        }
    }

    fn binding(appliance_id: &str, session_id: &str, expires_at: &str) -> RemoteSessionBinding {
        RemoteSessionBinding {
            appliance_id: appliance_id.to_string(),
            store_id: "epic_collection".to_string(),
            control_base_url: "https://192.168.1.192:8448".to_string(),
            s3_endpoint_url: "http://192.168.1.192:3900".to_string(),
            bucket: "dos-epic-collection".to_string(),
            region: "garage".to_string(),
            addressing_style: "path".to_string(),
            s3_profile: Some("dasobjectstore-epic_collection".to_string()),
            tls_trust: crate::config::RemoteTlsTrust::EnrolledCertificate,
            site_trust_bundle_path: None,
            trust_fingerprint_sha256: "AA:BB".to_string(),
            trust_spki_sha256: "CC:DD".to_string(),
            session: RemoteUploadSession {
                session_id: session_id.to_string(),
                issued_at: "2098-01-01T00:00:00Z".to_string(),
                expires_at: expires_at.to_string(),
                credentials: RemoteSessionCredentials {
                    access_key_id: format!("{session_id}-access"),
                    secret_access_key: "super-secret".to_string(),
                    session_token: Some("temporary-token".to_string()),
                },
                renewal: Some(RemoteSessionRenewalMetadata {
                    renew_url: "https://192.168.1.192:8448/renew".to_string(),
                    renew_after: "2098-12-31T00:00:00Z".to_string(),
                    renewal_token: Some("renewal-secret".to_string()),
                    last_renewed_at: None,
                }),
            },
        }
    }
}
