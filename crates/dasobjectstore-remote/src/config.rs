use crate::auth::RemoteAuthAuthority;
use crate::aws_profile::AwsProfileAssociation;
use serde::{Deserialize, Serialize};
use std::env;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const DEFAULT_REGION: &str = "garage";
pub const DEFAULT_PROFILE: &str = "dasobjectstore";
pub const REMOTE_CONFIG_SCHEMA_VERSION: &str = "dasobjectstore.remote_config.v2";
pub const REMOTE_STATE_SCHEMA_VERSION: &str = "dasobjectstore.remote_state.v1";
static GENERATION_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteConfig {
    #[serde(default = "default_config_schema_version")]
    pub schema_version: String,
    #[serde(default)]
    pub generation: u64,
    pub endpoint_url: String,
    #[serde(default = "default_region")]
    pub region: String,
    #[serde(default = "default_profile")]
    pub profile: String,
    #[serde(default)]
    pub auth_authority: RemoteAuthAuthority,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_helper: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_appliance_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paired_appliances: Vec<RemotePairedAppliance>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub s3_profiles: Vec<AwsProfileAssociation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub session_bindings: Vec<RemoteSessionBinding>,
}

impl RemoteConfig {
    pub fn merged_with(&self, overrides: RemoteConfigOverrides<'_>) -> Self {
        Self {
            schema_version: self.schema_version.clone(),
            generation: self.generation,
            endpoint_url: overrides
                .endpoint_url
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| self.endpoint_url.clone()),
            region: overrides
                .region
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| self.region.clone()),
            profile: overrides
                .profile
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| self.profile.clone()),
            auth_authority: overrides.auth_authority.unwrap_or(self.auth_authority),
            username: overrides
                .username
                .map(ToOwned::to_owned)
                .or_else(|| self.username.clone()),
            credential_helper: overrides
                .credential_helper
                .map(ToOwned::to_owned)
                .or_else(|| self.credential_helper.clone()),
            default_appliance_id: self.default_appliance_id.clone(),
            paired_appliances: self.paired_appliances.clone(),
            s3_profiles: self.s3_profiles.clone(),
            session_bindings: self.session_bindings.clone(),
        }
    }

    pub fn validate_for_command(&self) -> Result<(), RemoteConfigError> {
        if self.endpoint_url.trim().is_empty() {
            return Err(RemoteConfigError::Invalid(
                "endpoint URL is required; pass --endpoint-url or run config set".to_string(),
            ));
        }
        if !self.endpoint_url.starts_with("http://") && !self.endpoint_url.starts_with("https://") {
            return Err(RemoteConfigError::Invalid(
                "endpoint URL must start with http:// or https://".to_string(),
            ));
        }
        if self.region.trim().is_empty() {
            return Err(RemoteConfigError::Invalid(
                "region must not be blank".to_string(),
            ));
        }
        if self.profile.trim().is_empty() {
            return Err(RemoteConfigError::Invalid(
                "AWS profile must not be blank".to_string(),
            ));
        }
        if self.auth_authority == RemoteAuthAuthority::LocalPassword
            && self.username.as_deref().unwrap_or("").trim().is_empty()
        {
            return Err(RemoteConfigError::Invalid(
                "local-password authentication requires --username or configured username"
                    .to_string(),
            ));
        }
        Ok(())
    }

    pub fn redacted(&self) -> RedactedRemoteConfig {
        RedactedRemoteConfig {
            schema_version: self.schema_version.clone(),
            generation: self.generation,
            endpoint_url: self.endpoint_url.clone(),
            region: self.region.clone(),
            profile: self.profile.clone(),
            auth_authority: self.auth_authority,
            username: self.username.clone(),
            credential_helper_configured: self.credential_helper.is_some(),
            default_appliance_id: self.default_appliance_id.clone(),
            paired_appliances: self
                .paired_appliances
                .iter()
                .map(RemotePairedAppliance::redacted)
                .collect(),
            s3_profiles: self.s3_profiles.clone(),
            session_bindings: self
                .session_bindings
                .iter()
                .map(RemoteSessionBinding::redacted)
                .collect(),
        }
    }
}

impl RemoteSessionBinding {
    fn redacted(&self) -> RedactedRemoteSessionBinding {
        RedactedRemoteSessionBinding {
            appliance_id: self.appliance_id.clone(),
            store_id: self.store_id.clone(),
            control_base_url: self.control_base_url.clone(),
            s3_endpoint_url: self.s3_endpoint_url.clone(),
            bucket: self.bucket.clone(),
            region: self.region.clone(),
            addressing_style: self.addressing_style.clone(),
            s3_profile: self.s3_profile.clone(),
            trust_fingerprint_sha256: self.trust_fingerprint_sha256.clone(),
            trust_spki_sha256: self.trust_spki_sha256.clone(),
            session: self.session.redacted(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteSessionBinding {
    pub appliance_id: String,
    pub store_id: String,
    pub control_base_url: String,
    pub s3_endpoint_url: String,
    pub bucket: String,
    pub region: String,
    pub addressing_style: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub s3_profile: Option<String>,
    pub trust_fingerprint_sha256: String,
    pub trust_spki_sha256: String,
    pub session: RemoteUploadSession,
}

impl RemoteConfig {
    pub fn session_binding(
        &self,
        store_id: &str,
    ) -> Result<&RemoteSessionBinding, RemoteConfigError> {
        let matches = self
            .session_bindings
            .iter()
            .filter(|binding| binding.store_id == store_id)
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [binding] => Ok(binding),
            [] => Err(RemoteConfigError::Integrity {
                code: "configuration_migration_required",
                message: format!(
                    "no authoritative session generation exists for ObjectStore {store_id}"
                ),
                remediation: format!(
                    "dasobjectstore-remote authenticate HOST {store_id} --username USER"
                ),
            }),
            _ => Err(RemoteConfigError::Integrity {
                code: "ambiguous_session_state",
                message: format!(
                    "multiple authoritative session generations exist for ObjectStore {store_id}"
                ),
                remediation: "dasobjectstore-remote config repair --dry-run --json".to_string(),
            }),
        }
    }

    pub fn validate_session_integrity(&self) -> Result<(), RemoteConfigError> {
        if self.schema_version != REMOTE_CONFIG_SCHEMA_VERSION {
            return Err(RemoteConfigError::Integrity {
                code: "configuration_migration_required",
                message: format!(
                    "remote configuration schema {} requires migration",
                    self.schema_version
                ),
                remediation: "dasobjectstore-remote config repair --dry-run --json".to_string(),
            });
        }
        for (index, binding) in self.session_bindings.iter().enumerate() {
            if binding.appliance_id.trim().is_empty()
                || binding.store_id.trim().is_empty()
                || binding.control_base_url.trim().is_empty()
                || binding.s3_endpoint_url.trim().is_empty()
            {
                return Err(RemoteConfigError::Integrity {
                    code: "configuration_migration_required",
                    message: "an authoritative session binding is incomplete".to_string(),
                    remediation: "dasobjectstore-remote config repair --dry-run --json".to_string(),
                });
            }
            if self.session_bindings[index + 1..].iter().any(|candidate| {
                candidate.appliance_id == binding.appliance_id
                    && candidate.store_id == binding.store_id
            }) {
                return Err(RemoteConfigError::Integrity {
                    code: "ambiguous_session_state",
                    message: format!(
                        "duplicate session binding for appliance {} and ObjectStore {}",
                        binding.appliance_id, binding.store_id
                    ),
                    remediation: "dasobjectstore-remote config repair --dry-run --json".to_string(),
                });
            }
            if let Some(profile) = &binding.s3_profile {
                let associations = self
                    .s3_profiles
                    .iter()
                    .filter(|association| {
                        association.profile == *profile && association.store_id == binding.store_id
                    })
                    .count();
                if associations != 1 {
                    return Err(RemoteConfigError::Integrity {
                        code: "profile_association_mismatch",
                        message: format!(
                            "AWS profile {profile} is not uniquely associated with ObjectStore {}",
                            binding.store_id
                        ),
                        remediation: "dasobjectstore-remote config repair --dry-run --json"
                            .to_string(),
                    });
                }
            }
        }
        Ok(())
    }
}

fn default_config_schema_version() -> String {
    REMOTE_CONFIG_SCHEMA_VERSION.to_string()
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemotePairedAppliance {
    pub appliance_id: String,
    pub display_name: String,
    pub appliance_base_url: String,
    pub discovery_url: String,
    #[serde(default)]
    pub auth_authority: RemoteAuthAuthority,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paired_actor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_object_store: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<RemoteUploadSession>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub object_stores: Vec<RemoteObjectStoreGrant>,
}

impl RemotePairedAppliance {
    pub fn redacted(&self) -> RedactedRemotePairedAppliance {
        RedactedRemotePairedAppliance {
            appliance_id: self.appliance_id.clone(),
            display_name: self.display_name.clone(),
            appliance_base_url: self.appliance_base_url.clone(),
            discovery_url: self.discovery_url.clone(),
            auth_authority: self.auth_authority,
            paired_actor: self.paired_actor.clone(),
            default_object_store: self.default_object_store.clone(),
            session: self.session.as_ref().map(RemoteUploadSession::redacted),
            object_stores: self.object_stores.clone(),
        }
    }

    pub fn writable_object_store(&self, object_store: &str) -> Option<&RemoteObjectStoreGrant> {
        self.object_stores
            .iter()
            .find(|grant| grant.object_store == object_store && grant.can_write)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteObjectStoreGrant {
    pub object_store: String,
    pub bucket: String,
    pub can_read: bool,
    pub can_write: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub writer_group: Option<String>,
    pub object_type: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteUploadSession {
    pub session_id: String,
    pub issued_at: String,
    pub expires_at: String,
    pub credentials: RemoteSessionCredentials,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub renewal: Option<RemoteSessionRenewalMetadata>,
}

impl RemoteUploadSession {
    pub fn redacted(&self) -> RedactedRemoteUploadSession {
        RedactedRemoteUploadSession {
            session_id: self.redacted_session_id(),
            issued_at: self.issued_at.clone(),
            expires_at: self.expires_at.clone(),
            credentials: self.credentials.redacted(),
            renewal: self
                .renewal
                .as_ref()
                .map(RemoteSessionRenewalMetadata::redacted),
        }
    }

    pub fn redacted_session_id(&self) -> String {
        redact_identifier(&self.session_id)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteSessionCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_token: Option<String>,
}

impl RemoteSessionCredentials {
    pub fn redacted(&self) -> RedactedRemoteSessionCredentials {
        RedactedRemoteSessionCredentials {
            access_key_id: redact_identifier(&self.access_key_id),
            secret_access_key: REDACTED_SECRET.to_string(),
            session_token: self
                .session_token
                .as_ref()
                .map(|_| REDACTED_SECRET.to_string()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteSessionRenewalMetadata {
    pub renew_url: String,
    pub renew_after: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub renewal_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_renewed_at: Option<String>,
}

impl RemoteSessionRenewalMetadata {
    pub fn redacted(&self) -> RedactedRemoteSessionRenewalMetadata {
        RedactedRemoteSessionRenewalMetadata {
            renew_url: self.renew_url.clone(),
            renew_after: self.renew_after.clone(),
            renewal_token: self
                .renewal_token
                .as_ref()
                .map(|_| REDACTED_SECRET.to_string()),
            last_renewed_at: self.last_renewed_at.clone(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RedactedRemoteConfig {
    pub schema_version: String,
    pub generation: u64,
    pub endpoint_url: String,
    pub region: String,
    pub profile: String,
    pub auth_authority: RemoteAuthAuthority,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    pub credential_helper_configured: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_appliance_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paired_appliances: Vec<RedactedRemotePairedAppliance>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub s3_profiles: Vec<AwsProfileAssociation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub session_bindings: Vec<RedactedRemoteSessionBinding>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RedactedRemoteSessionBinding {
    pub appliance_id: String,
    pub store_id: String,
    pub control_base_url: String,
    pub s3_endpoint_url: String,
    pub bucket: String,
    pub region: String,
    pub addressing_style: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub s3_profile: Option<String>,
    pub trust_fingerprint_sha256: String,
    pub trust_spki_sha256: String,
    pub session: RedactedRemoteUploadSession,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RedactedRemotePairedAppliance {
    pub appliance_id: String,
    pub display_name: String,
    pub appliance_base_url: String,
    pub discovery_url: String,
    pub auth_authority: RemoteAuthAuthority,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paired_actor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_object_store: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<RedactedRemoteUploadSession>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub object_stores: Vec<RemoteObjectStoreGrant>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RedactedRemoteUploadSession {
    pub session_id: String,
    pub issued_at: String,
    pub expires_at: String,
    pub credentials: RedactedRemoteSessionCredentials,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub renewal: Option<RedactedRemoteSessionRenewalMetadata>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RedactedRemoteSessionRenewalMetadata {
    pub renew_url: String,
    pub renew_after: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub renewal_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_renewed_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RedactedRemoteSessionCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_token: Option<String>,
}

pub const REDACTED_SECRET: &str = "<redacted>";

#[derive(Clone, Copy, Debug, Default)]
pub struct RemoteConfigOverrides<'a> {
    pub endpoint_url: Option<&'a str>,
    pub region: Option<&'a str>,
    pub profile: Option<&'a str>,
    pub auth_authority: Option<RemoteAuthAuthority>,
    pub username: Option<&'a str>,
    pub credential_helper: Option<&'a str>,
}

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
    let required_corrective_action = if duplicate_session_count > 0 {
        Some("dasobjectstore-remote config repair --dry-run --json".to_string())
    } else if !profile_associations_consistent {
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
    file.write_all(&raw)?;
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

fn default_region() -> String {
    DEFAULT_REGION.to_string()
}

fn default_profile() -> String {
    DEFAULT_PROFILE.to_string()
}

fn redact_identifier(value: &str) -> String {
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

#[derive(Debug)]
pub enum RemoteConfigError {
    Io(io::Error),
    Json(serde_json::Error),
    MissingHome,
    Invalid(String),
    Integrity {
        code: &'static str,
        message: String,
        remediation: String,
    },
}

impl fmt::Display for RemoteConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Json(error) => write!(formatter, "{error}"),
            Self::MissingHome => write!(
                formatter,
                "cannot resolve remote config path because HOME is not set"
            ),
            Self::Invalid(message) => formatter.write_str(message),
            Self::Integrity {
                code,
                message,
                remediation,
            } => write!(formatter, "{code}: {message}; run `{remediation}`"),
        }
    }
}

impl std::error::Error for RemoteConfigError {}

impl From<io::Error> for RemoteConfigError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for RemoteConfigError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        RemoteConfig, RemoteConfigOverrides, RemoteObjectStoreGrant, RemotePairedAppliance,
        RemoteSessionCredentials, RemoteSessionRenewalMetadata, RemoteUploadSession,
        REDACTED_SECRET, REMOTE_CONFIG_SCHEMA_VERSION,
    };
    use crate::auth::RemoteAuthAuthority;

    #[test]
    fn overrides_config_without_losing_unset_values() {
        let config = RemoteConfig {
            schema_version: REMOTE_CONFIG_SCHEMA_VERSION.to_string(),
            generation: 1,
            endpoint_url: "http://old:3900".to_string(),
            region: "garage".to_string(),
            profile: "old".to_string(),
            auth_authority: RemoteAuthAuthority::Mneion,
            username: Some("alice".to_string()),
            credential_helper: Some("helper".to_string()),
            default_appliance_id: Some("appliance-1".to_string()),
            paired_appliances: vec![RemotePairedAppliance {
                appliance_id: "appliance-1".to_string(),
                display_name: "Lab DAS".to_string(),
                appliance_base_url: "https://192.168.1.192:8448".to_string(),
                discovery_url:
                    "https://192.168.1.192:8448/products/dasobjectstore/api/v1/remote/easyconnect/discovery"
                        .to_string(),
                auth_authority: RemoteAuthAuthority::LocalPassword,
                paired_actor: Some("alice".to_string()),
                default_object_store: Some("generated-data".to_string()),
                session: None,
                object_stores: Vec::new(),
            }],
            s3_profiles: Vec::new(),
            session_bindings: Vec::new(),
        };

        let merged = config.merged_with(RemoteConfigOverrides {
            endpoint_url: Some("https://new:3900"),
            profile: Some("new"),
            ..RemoteConfigOverrides::default()
        });

        assert_eq!(merged.endpoint_url, "https://new:3900");
        assert_eq!(merged.region, "garage");
        assert_eq!(merged.profile, "new");
        assert_eq!(merged.username.as_deref(), Some("alice"));
        assert_eq!(merged.credential_helper.as_deref(), Some("helper"));
        assert_eq!(merged.default_appliance_id.as_deref(), Some("appliance-1"));
        assert_eq!(merged.paired_appliances.len(), 1);
    }

    #[test]
    fn reads_legacy_config_without_pairing_fields() {
        let raw = r#"{
          "endpoint_url": "http://192.168.1.192:3900",
          "region": "garage",
          "profile": "dasobjectstore"
        }"#;

        let config: RemoteConfig = serde_json::from_str(raw).expect("legacy config parses");

        assert_eq!(config.endpoint_url, "http://192.168.1.192:3900");
        assert!(config.default_appliance_id.is_none());
        assert!(config.paired_appliances.is_empty());
    }

    #[test]
    fn redacts_session_credentials_for_display() {
        let config = RemoteConfig {
            schema_version: REMOTE_CONFIG_SCHEMA_VERSION.to_string(),
            generation: 1,
            endpoint_url: "https://192.168.1.192:3900".to_string(),
            region: "garage".to_string(),
            profile: "dasobjectstore".to_string(),
            auth_authority: RemoteAuthAuthority::LocalPassword,
            username: Some("stephen".to_string()),
            credential_helper: Some("helper".to_string()),
            default_appliance_id: Some("appliance-1".to_string()),
            paired_appliances: vec![RemotePairedAppliance {
                appliance_id: "appliance-1".to_string(),
                display_name: "QNAP TL-D800C".to_string(),
                appliance_base_url: "https://192.168.1.192:8448".to_string(),
                discovery_url:
                    "https://192.168.1.192:8448/products/dasobjectstore/api/v1/remote/easyconnect/discovery"
                        .to_string(),
                auth_authority: RemoteAuthAuthority::LocalPassword,
                paired_actor: Some("stephen".to_string()),
                default_object_store: Some("zymo_fecal_2025.05".to_string()),
                object_stores: vec![RemoteObjectStoreGrant {
                    object_store: "zymo_fecal_2025.05".to_string(),
                    bucket: "dos-zymo-fecal-2025-05".to_string(),
                    can_read: true,
                    can_write: true,
                    writer_group: Some("mnemosyne".to_string()),
                    object_type: "metagenomics".to_string(),
                }],
                session: Some(RemoteUploadSession {
                    session_id: "SESSIONREFERENCE7890".to_string(),
                    issued_at: "2026-07-09T11:30:00Z".to_string(),
                    expires_at: "2026-07-09T19:30:00Z".to_string(),
                    credentials: RemoteSessionCredentials {
                        access_key_id: "DOSREMOTEACCESSKEY1234".to_string(),
                        secret_access_key: "super-secret".to_string(),
                        session_token: Some("temporary-token".to_string()),
                    },
                    renewal: Some(RemoteSessionRenewalMetadata {
                        renew_url: "https://192.168.1.192:8448/api/renew".to_string(),
                        renew_after: "2026-07-09T18:30:00Z".to_string(),
                        renewal_token: Some("renewal-token-secret".to_string()),
                        last_renewed_at: None,
                    }),
                }),
            }],
            s3_profiles: Vec::new(),
            session_bindings: Vec::new(),
        };

        let redacted = config.redacted();
        let rendered = serde_json::to_string(&redacted).expect("redacted config serializes");

        assert!(rendered.contains("DOSR...1234"));
        assert!(rendered.contains("SESS...7890"));
        assert!(rendered.contains(REDACTED_SECRET));
        assert!(rendered.contains("zymo_fecal_2025.05"));
        assert!(rendered.contains("dos-zymo-fecal-2025-05"));
        assert!(!rendered.contains("SESSIONREFERENCE7890"));
        assert!(!rendered.contains("super-secret"));
        assert!(!rendered.contains("temporary-token"));
        assert!(!rendered.contains("renewal-token-secret"));
    }
}
