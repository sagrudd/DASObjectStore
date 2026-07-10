//! HTTPS password authentication and scoped Garage connection context.

use dasobjectstore_daemon::RemoteEasyconnectSession;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::path::Path;
use std::time::Duration;

pub const DEFAULT_APPLIANCE_HTTPS_PORT: u16 = 8448;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct RemoteAuthenticateRequest {
    username: String,
    password: String,
    object_store: String,
    requested_session_lifetime_seconds: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct RemoteAuthenticateResponse {
    schema_version: String,
    endpoint_port: u16,
    region: String,
    addressing_style: String,
    object_store: String,
    bucket: String,
    session: RemoteEasyconnectSession,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteConnectionContext {
    pub schema_version: String,
    pub appliance_host: String,
    pub endpoint_url: String,
    pub region: String,
    pub addressing_style: String,
    pub object_store: String,
    pub bucket: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
    pub session_id: String,
    pub issued_at_utc: String,
    pub expires_at_utc: String,
    pub renew_url: String,
    pub renew_after_utc: String,
    pub renewal_token: String,
}

impl RemoteConnectionContext {
    pub fn redacted(&self) -> RedactedRemoteConnectionContext {
        RedactedRemoteConnectionContext {
            schema_version: self.schema_version.clone(),
            appliance_host: self.appliance_host.clone(),
            endpoint_url: self.endpoint_url.clone(),
            region: self.region.clone(),
            addressing_style: self.addressing_style.clone(),
            object_store: self.object_store.clone(),
            bucket: self.bucket.clone(),
            access_key_id: redact(&self.access_key_id),
            session_id: redact(&self.session_id),
            issued_at_utc: self.issued_at_utc.clone(),
            expires_at_utc: self.expires_at_utc.clone(),
            renew_url: self.renew_url.clone(),
            renew_after_utc: self.renew_after_utc.clone(),
            credentials: "<redacted>".to_string(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RedactedRemoteConnectionContext {
    pub schema_version: String,
    pub appliance_host: String,
    pub endpoint_url: String,
    pub region: String,
    pub addressing_style: String,
    pub object_store: String,
    pub bucket: String,
    pub access_key_id: String,
    pub session_id: String,
    pub issued_at_utc: String,
    pub expires_at_utc: String,
    pub renew_url: String,
    pub renew_after_utc: String,
    pub credentials: String,
}

#[derive(Debug)]
pub enum RemoteAuthenticateError {
    InvalidHost(String),
    Io(std::io::Error),
    Http(String),
    Server { status: u16, message: String },
    Json(serde_json::Error),
}

impl fmt::Display for RemoteAuthenticateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidHost(message) | Self::Http(message) => formatter.write_str(message),
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Server { status, message } => {
                write!(
                    formatter,
                    "appliance authentication failed ({status}): {message}"
                )
            }
            Self::Json(error) => write!(
                formatter,
                "invalid appliance authentication response: {error}"
            ),
        }
    }
}

impl std::error::Error for RemoteAuthenticateError {}

impl From<std::io::Error> for RemoteAuthenticateError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for RemoteAuthenticateError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

pub fn authenticate(
    host: &str,
    https_port: u16,
    ca_cert: Option<&Path>,
    username: &str,
    password: &str,
    object_store: &str,
    requested_session_lifetime_seconds: Option<u64>,
) -> Result<RemoteConnectionContext, RemoteAuthenticateError> {
    let host = normalize_host(host)?;
    if https_port == 0 {
        return Err(RemoteAuthenticateError::InvalidHost(
            "HTTPS port must be greater than zero".to_string(),
        ));
    }
    if username.trim().is_empty() || password.is_empty() || object_store.trim().is_empty() {
        return Err(RemoteAuthenticateError::InvalidHost(
            "username, password, and object store must not be blank".to_string(),
        ));
    }
    if requested_session_lifetime_seconds.is_some_and(|seconds| !(60..=86_400).contains(&seconds)) {
        return Err(RemoteAuthenticateError::InvalidHost(
            "session lifetime must be between 60 and 86400 seconds".to_string(),
        ));
    }

    let mut builder = Client::builder().timeout(Duration::from_secs(20));
    if let Some(ca_cert) = ca_cert {
        let certificate = reqwest::Certificate::from_pem(&fs::read(ca_cert)?).map_err(|error| {
            RemoteAuthenticateError::Http(format!("read CA certificate: {error}"))
        })?;
        builder = builder.add_root_certificate(certificate);
    }
    let client = builder
        .build()
        .map_err(|error| RemoteAuthenticateError::Http(format!("build HTTPS client: {error}")))?;
    let url =
        format!("https://{host}:{https_port}/products/dasobjectstore/api/v1/remote/authenticate");
    let response = client
        .post(url)
        .json(&RemoteAuthenticateRequest {
            username: username.to_string(),
            password: password.to_string(),
            object_store: object_store.to_string(),
            requested_session_lifetime_seconds,
        })
        .send()
        .map_err(|error| {
            RemoteAuthenticateError::Http(format!("HTTPS authentication request failed: {error}"))
        })?;
    let status = response.status();
    if !status.is_success() {
        let message = response
            .json::<serde_json::Value>()
            .ok()
            .and_then(|body| {
                body.get("message")
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "the appliance rejected the authentication request".to_string());
        return Err(RemoteAuthenticateError::Server {
            status: status.as_u16(),
            message,
        });
    }
    let response = response
        .json::<RemoteAuthenticateResponse>()
        .map_err(|error| {
            RemoteAuthenticateError::Http(format!(
                "invalid appliance authentication response: {error}"
            ))
        })?;
    Ok(RemoteConnectionContext {
        schema_version: response.schema_version,
        appliance_host: host.clone(),
        endpoint_url: format!("http://{host}:{}", response.endpoint_port),
        region: response.region,
        addressing_style: response.addressing_style,
        object_store: response.object_store,
        bucket: response.bucket,
        access_key_id: response.session.credentials.access_key_id,
        secret_access_key: response.session.credentials.secret_access_key,
        session_token: response.session.credentials.session_token,
        session_id: response.session.session_id,
        issued_at_utc: response.session.issued_at_utc,
        expires_at_utc: response.session.expires_at_utc,
        renew_url: absolute_renew_url(&host, https_port, &response.session.renewal.renew_url),
        renew_after_utc: response.session.renewal.renew_after_utc,
        renewal_token: response.session.renewal.renewal_token,
    })
}

fn normalize_host(value: &str) -> Result<String, RemoteAuthenticateError> {
    let host = value
        .trim()
        .strip_prefix("https://")
        .or_else(|| value.trim().strip_prefix("http://"))
        .unwrap_or(value.trim())
        .trim_end_matches('/');
    if host.is_empty() || host.contains('/') || host.contains('@') || host.contains(' ') {
        return Err(RemoteAuthenticateError::InvalidHost(
            "host must be a hostname or IP address, not a URL path or credential".to_string(),
        ));
    }
    Ok(host.to_string())
}

fn redact(value: &str) -> String {
    let prefix = value.chars().take(4).collect::<String>();
    format!("{prefix}...redacted")
}

fn absolute_renew_url(host: &str, https_port: u16, path: &str) -> String {
    if path.starts_with("http://") || path.starts_with("https://") {
        return path.to_string();
    }
    format!("https://{host}:{https_port}/products/dasobjectstore{path}")
}

#[cfg(test)]
mod tests {
    use super::{normalize_host, RemoteConnectionContext};

    #[test]
    fn normalizes_safe_hosts_and_rejects_paths() {
        assert_eq!(
            normalize_host("https://192.168.1.192/").unwrap(),
            "192.168.1.192"
        );
        assert!(normalize_host("192.168.1.192/path").is_err());
        assert!(normalize_host("user@host").is_err());
    }

    #[test]
    fn redacted_context_does_not_expose_secret_values() {
        let context = RemoteConnectionContext {
            schema_version: "v1".to_string(),
            appliance_host: "host".to_string(),
            endpoint_url: "http://host:3900".to_string(),
            region: "garage".to_string(),
            addressing_style: "path".to_string(),
            object_store: "store".to_string(),
            bucket: "dos-store".to_string(),
            access_key_id: "ACCESS123".to_string(),
            secret_access_key: "SECRET123".to_string(),
            session_token: Some("TOKEN123".to_string()),
            session_id: "SESSION123".to_string(),
            issued_at_utc: "2026-01-01T00:00:00Z".to_string(),
            expires_at_utc: "2026-01-01T08:00:00Z".to_string(),
            renew_url: "/renew".to_string(),
            renew_after_utc: "2026-01-01T07:00:00Z".to_string(),
            renewal_token: "RENEW123".to_string(),
        };
        let redacted = serde_json::to_string(&context.redacted()).unwrap();
        assert!(!redacted.contains("SECRET123"));
        assert!(!redacted.contains("TOKEN123"));
        assert!(!redacted.contains("RENEW123"));
    }
}
