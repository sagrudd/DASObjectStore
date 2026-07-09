use crate::auth::RemoteAuthAuthority;
use serde::{Deserialize, Serialize};
use std::env;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const DEFAULT_REGION: &str = "garage";
pub const DEFAULT_PROFILE: &str = "dasobjectstore";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteConfig {
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
}

impl RemoteConfig {
    pub fn merged_with(&self, overrides: RemoteConfigOverrides<'_>) -> Self {
        Self {
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
        }
    }
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
    match fs::read_to_string(path) {
        Ok(raw) => Ok(Some(serde_json::from_str(&raw)?)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub fn write_config(path: &Path, config: &RemoteConfig) -> Result<(), RemoteConfigError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_vec_pretty(config)?;
    fs::write(path, raw)?;
    restrict_config_permissions(path)?;
    Ok(())
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
        REDACTED_SECRET,
    };
    use crate::auth::RemoteAuthAuthority;

    #[test]
    fn overrides_config_without_losing_unset_values() {
        let config = RemoteConfig {
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
