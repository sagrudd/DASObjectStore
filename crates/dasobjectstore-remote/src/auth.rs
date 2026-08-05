use serde::{Deserialize, Serialize};
use std::fmt;
use std::process::Command;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RemoteAuthAuthority {
    #[default]
    AwsProfile,
    LocalPassword,
    Mneion,
    Pistis,
    Synoptikon,
}

impl RemoteAuthAuthority {
    /// `local-password` is retained only so persisted legacy configuration can
    /// be diagnosed and rejected without being silently reinterpreted.
    pub fn is_retired(self) -> bool {
        matches!(self, Self::LocalPassword)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::AwsProfile => "aws-profile",
            Self::LocalPassword => "local-password",
            Self::Mneion => "mneion",
            Self::Pistis => "pistis",
            Self::Synoptikon => "synoptikon",
        }
    }
}

impl std::str::FromStr for RemoteAuthAuthority {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "aws-profile" => Ok(Self::AwsProfile),
            "local-password" => Ok(Self::LocalPassword),
            "mneion" => Ok(Self::Mneion),
            "pistis" => Ok(Self::Pistis),
            "synoptikon" => Ok(Self::Synoptikon),
            _ => Err(format!(
                "unknown auth authority {value}; expected aws-profile, local-password, mneion, or synoptikon"
            )),
        }
    }
}

impl fmt::Display for RemoteAuthAuthority {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteS3Credentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_token: Option<String>,
}

impl RemoteS3Credentials {
    pub fn validate(&self) -> Result<(), RemoteAuthError> {
        if self.access_key_id.trim().is_empty() {
            return Err(RemoteAuthError::InvalidCredentials(
                "credential helper returned a blank access_key_id".to_string(),
            ));
        }
        if self.secret_access_key.trim().is_empty() {
            return Err(RemoteAuthError::InvalidCredentials(
                "credential helper returned a blank secret_access_key".to_string(),
            ));
        }
        Ok(())
    }
}

pub fn request_s3_credentials(
    helper: &str,
    authority: RemoteAuthAuthority,
    endpoint_url: &str,
    username: Option<&str>,
) -> Result<RemoteS3Credentials, RemoteAuthError> {
    if authority.is_retired() {
        return Err(RemoteAuthError::RetiredAuthority);
    }
    let output = Command::new("sh")
        .arg("-c")
        .arg(helper)
        .env("DASOBJECTSTORE_REMOTE_AUTHORITY", authority.as_str())
        .env("DASOBJECTSTORE_REMOTE_ENDPOINT_URL", endpoint_url)
        .env("DASOBJECTSTORE_REMOTE_USERNAME", username.unwrap_or(""))
        .output()?;
    if !output.status.success() {
        return Err(RemoteAuthError::HelperFailed(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    let credentials: RemoteS3Credentials = serde_json::from_slice(&output.stdout)?;
    credentials.validate()?;
    Ok(credentials)
}

#[derive(Debug)]
pub enum RemoteAuthError {
    RetiredAuthority,
    Io(std::io::Error),
    Json(serde_json::Error),
    HelperFailed(String),
    InvalidCredentials(String),
}

impl fmt::Display for RemoteAuthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RetiredAuthority => formatter.write_str(
                "local-password authority is retired; use the browser-approved Pistis EasyConnect ceremony or a site-issued credential helper without a password",
            ),
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Json(error) => write!(formatter, "{error}"),
            Self::HelperFailed(message) if message.is_empty() => {
                formatter.write_str("credential helper failed")
            }
            Self::HelperFailed(message) => write!(formatter, "credential helper failed: {message}"),
            Self::InvalidCredentials(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for RemoteAuthError {}

impl From<std::io::Error> for RemoteAuthError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for RemoteAuthError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[cfg(test)]
mod tests {
    use super::{RemoteAuthAuthority, RemoteS3Credentials};

    #[test]
    fn parses_auth_authority_names() {
        assert_eq!(
            "local-password".parse::<RemoteAuthAuthority>().unwrap(),
            RemoteAuthAuthority::LocalPassword
        );
        assert_eq!(
            "synoptikon".parse::<RemoteAuthAuthority>().unwrap(),
            RemoteAuthAuthority::Synoptikon
        );
    }

    #[test]
    fn rejects_legacy_local_password_helper_before_starting_a_shell() {
        let error = super::request_s3_credentials(
            "exit 0",
            RemoteAuthAuthority::LocalPassword,
            "https://dos.example:3900",
            Some("alice"),
        )
        .expect_err("retired authority must fail before invoking the helper");

        assert!(error
            .to_string()
            .contains("local-password authority is retired"));
    }

    #[test]
    fn rejects_blank_helper_credentials() {
        let credentials = RemoteS3Credentials {
            access_key_id: String::new(),
            secret_access_key: "secret".to_string(),
            session_token: None,
        };

        assert!(credentials.validate().is_err());
    }
}
