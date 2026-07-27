use super::*;

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
}

pub fn doctor_config(path: &Path) -> Result<RemoteConfigDoctorReport, RemoteConfigError> {
    let parent = path.parent().ok_or_else(|| {
        RemoteConfigError::Invalid("remote configuration path has no parent".to_string())
    })?;
    let config = if parent.join("state.json").exists() {
        read_optional_config(path)?
    } else {
        match fs::read(path) {
            Ok(raw) => {
                let mut legacy: RemoteConfig = serde_json::from_slice(&raw)?;
                migrate_legacy_sessions(&mut legacy)?;
                Some(legacy)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        }
    }
    .unwrap_or_else(|| RemoteConfig {
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
    let mut duplicate_session_count = 0;
    for (index, binding) in config.session_bindings.iter().enumerate() {
        duplicate_session_count += config.session_bindings[index + 1..]
            .iter()
            .filter(|candidate| {
                candidate.appliance_id == binding.appliance_id
                    && candidate.store_id == binding.store_id
            })
            .count();
    }
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
            }
        })
        .collect::<Vec<_>>();
    let stale_session_count = bindings.iter().filter(|binding| binding.expired).count();
    let profile_associations_consistent = config.session_bindings.iter().all(|binding| {
        binding.s3_profile.as_ref().is_none_or(|profile| {
            config
                .s3_profiles
                .iter()
                .filter(|association| {
                    association.profile == *profile
                        && association.store_id == binding.store_id
                        && association.endpoint_url == binding.s3_endpoint_url
                        && association.bucket == binding.bucket
                        && association.expires_at.as_deref()
                            == Some(binding.session.expires_at.as_str())
                })
                .count()
                == 1
        })
    });
    let required_corrective_action =
        if duplicate_session_count > 0 || !profile_associations_consistent {
            Some("dasobjectstore-remote config repair --dry-run --json".to_string())
        } else if stale_session_count > 0 {
            Some(
            "dasobjectstore-remote authenticate HOST OBJECTSTORE --username USER --set-s3-config"
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
}

pub fn repair_config(
    path: &Path,
    apply: bool,
) -> Result<RemoteConfigRepairReport, RemoteConfigError> {
    let parent = path.parent().ok_or_else(|| {
        RemoteConfigError::Invalid("remote configuration path has no parent".to_string())
    })?;
    if !apply {
        let state_exists = parent.join("state.json").exists();
        return Ok(RemoteConfigRepairReport {
            schema_version: "dasobjectstore.remote_config_repair.v1",
            applied: false,
            backup_created: false,
            current_generation: doctor_config(path)
                .map(|report| report.current_generation)
                .unwrap_or_default(),
            action: if state_exists {
                "validate_current_generation".to_string()
            } else {
                "migrate_legacy_configuration".to_string()
            },
        });
    }
    let had_legacy = path.exists() && !parent.join("state.json").exists();
    let config = read_optional_config(path)?.ok_or_else(|| RemoteConfigError::Integrity {
        code: "configuration_migration_required",
        message: "no remote authentication state exists".to_string(),
        remediation: "dasobjectstore-remote authenticate HOST OBJECTSTORE --username USER"
            .to_string(),
    })?;
    Ok(RemoteConfigRepairReport {
        schema_version: "dasobjectstore.remote_config_repair.v1",
        applied: true,
        backup_created: had_legacy,
        current_generation: config.generation,
        action: if had_legacy {
            "legacy_configuration_migrated".to_string()
        } else {
            "current_generation_validated".to_string()
        },
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
