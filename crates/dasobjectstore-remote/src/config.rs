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
}

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
    use super::{RemoteConfig, RemoteConfigOverrides};
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
    }
}
