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
    pub(crate) fn duplicate_store_binding_count(&self) -> usize {
        self.session_bindings
            .iter()
            .enumerate()
            .map(|(index, binding)| {
                self.session_bindings[index + 1..]
                    .iter()
                    .filter(|candidate| candidate.store_id == binding.store_id)
                    .count()
            })
            .sum()
    }

    pub(crate) fn profile_association_consistent(&self, binding: &RemoteSessionBinding) -> bool {
        let associations = self
            .s3_profiles
            .iter()
            .filter(|association| association.store_id == binding.store_id)
            .collect::<Vec<_>>();
        match (binding.s3_profile.as_deref(), associations.as_slice()) {
            (None, []) => true,
            (Some(profile), [association]) => {
                association.profile == profile
                    && association.endpoint_url == binding.s3_endpoint_url
                    && association.bucket == binding.bucket
                    && association.region == binding.region
                    && association.addressing_style == binding.addressing_style
                    && association.expires_at.as_deref()
                        == Some(binding.session.expires_at.as_str())
            }
            _ => false,
        }
    }

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
            if self.session_bindings[index + 1..]
                .iter()
                .any(|candidate| candidate.store_id == binding.store_id)
            {
                return Err(RemoteConfigError::Integrity {
                    code: "ambiguous_session_state",
                    message: format!(
                        "multiple authoritative session generations exist for ObjectStore {}",
                        binding.store_id
                    ),
                    remediation: "dasobjectstore-remote config repair --dry-run --json".to_string(),
                });
            }
            if !self.profile_association_consistent(binding) {
                return Err(RemoteConfigError::Integrity {
                    code: "profile_association_mismatch",
                    message: format!(
                        "ObjectStore {} does not have exactly one matching AWS profile association",
                        binding.store_id
                    ),
                    remediation: "dasobjectstore-remote config repair --dry-run --json".to_string(),
                });
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

mod persistence;
pub use persistence::*;

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
mod tests;
