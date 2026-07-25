//! HTTPS password authentication and scoped Garage connection context.

use dasobjectstore_daemon::RemoteEasyconnectDiscoveryResponse;
use dasobjectstore_daemon::RemoteEasyconnectSession;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::net::ToSocketAddrs;
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
    store_id: String,
    s3: RemoteAuthenticatedS3Descriptor,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct RemoteAuthenticatedS3Descriptor {
    endpoint_url: String,
    bucket: String,
    region: String,
    addressing_style: String,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplianceTrust {
    pub certificate_pem: Vec<u8>,
    pub tls_server_name: Option<String>,
    pub fingerprint_sha256: String,
    pub appliance_id: Option<String>,
    pub newly_enrolled: bool,
    pub legacy_fingerprint_pinned: bool,
}

pub fn prepare_appliance_trust<F>(
    host: &str,
    https_port: u16,
    explicit_ca_cert: Option<&Path>,
    explicit_tls_server_name: Option<&str>,
    expected_fingerprint: Option<&str>,
    mut confirm_unknown: F,
) -> Result<ApplianceTrust, RemoteAuthenticateError>
where
    F: FnMut(
        &crate::trust::PresentedCertificate,
        Option<&str>,
        bool,
    ) -> Result<bool, RemoteAuthenticateError>,
{
    let host = normalize_host(host)?;
    if let Some(path) = explicit_ca_cert {
        let certificate = fs::read(path)?;
        validate_public_certificate(&certificate)?;
        let leaf = crate::trust::pem_leaf_der(&certificate)
            .map_err(|error| RemoteAuthenticateError::Http(error.to_string()))?;
        let presented = crate::trust::inspect_leaf_certificate(&host, &leaf)
            .map_err(|error| RemoteAuthenticateError::Http(error.to_string()))?;
        if let Some(expected) = expected_fingerprint {
            crate::trust::expected_fingerprint_matches(expected, &presented)
                .map_err(|error| RemoteAuthenticateError::Http(error.to_string()))?;
        }
        return Ok(ApplianceTrust {
            certificate_pem: certificate,
            tls_server_name: explicit_tls_server_name.map(str::to_string),
            fingerprint_sha256: presented.fingerprint_sha256,
            appliance_id: None,
            newly_enrolled: false,
            legacy_fingerprint_pinned: false,
        });
    }

    let presented = crate::trust::probe_certificate(&host, https_port)
        .map_err(|error| RemoteAuthenticateError::Http(error.to_string()))?;
    if let Some(record) = crate::trust::load_trust(&host, https_port)
        .map_err(|error| RemoteAuthenticateError::Http(error.to_string()))?
    {
        crate::trust::verify_presented_pin(&record, &presented)
            .map_err(|error| RemoteAuthenticateError::Http(error.to_string()))?;
        if let Some(expected) = expected_fingerprint {
            crate::trust::expected_fingerprint_matches(expected, &presented)
                .map_err(|error| RemoteAuthenticateError::Http(error.to_string()))?;
        }
        return Ok(ApplianceTrust {
            certificate_pem: record.certificate_pem.into_bytes(),
            tls_server_name: Some(record.tls_server_name),
            fingerprint_sha256: record.fingerprint_sha256,
            appliance_id: Some(record.appliance_id),
            newly_enrolled: false,
            legacy_fingerprint_pinned: record.legacy_fingerprint_pinned,
        });
    }

    if let Some(expected) = expected_fingerprint {
        crate::trust::expected_fingerprint_matches(expected, &presented)
            .map_err(|error| RemoteAuthenticateError::Http(error.to_string()))?;
        let _ = confirm_unknown(&presented, None, false)?;
    } else if !confirm_unknown(&presented, None, true)? {
        return Err(RemoteAuthenticateError::Http(
            "appliance certificate was not trusted; no credentials were requested or sent"
                .to_string(),
        ));
    }
    let tls_server_name = explicit_tls_server_name
        .map(str::to_string)
        .or_else(|| presented.tls_server_name.clone())
        .ok_or_else(|| {
            RemoteAuthenticateError::Http(
                "certificate does not match the appliance address and has no supported legacy certificate name"
                    .to_string(),
            )
        })?;
    let appliance_id = discover_appliance_id(
        &host,
        https_port,
        presented.certificate_pem.as_bytes(),
        &tls_server_name,
    )
    .ok();
    let mut record =
        crate::trust::new_trust_record(&host, https_port, appliance_id.as_deref(), &presented)
            .map_err(|error| RemoteAuthenticateError::Http(error.to_string()))?;
    record.tls_server_name = tls_server_name.clone();
    crate::trust::persist_trust(&record)
        .map_err(|error| RemoteAuthenticateError::Http(error.to_string()))?;
    Ok(ApplianceTrust {
        certificate_pem: record.certificate_pem.into_bytes(),
        tls_server_name: Some(tls_server_name),
        fingerprint_sha256: record.fingerprint_sha256,
        appliance_id: Some(record.appliance_id),
        newly_enrolled: true,
        legacy_fingerprint_pinned: record.legacy_fingerprint_pinned,
    })
}

pub fn authenticate(
    host: &str,
    https_port: u16,
    ca_certificate_pem: Option<&[u8]>,
    tls_server_name: Option<&str>,
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
    let request_host = tls_server_name.unwrap_or(&host);
    if let Some(tls_server_name) = tls_server_name {
        if tls_server_name.trim().is_empty()
            || tls_server_name.contains('/')
            || tls_server_name.contains('@')
            || tls_server_name.contains(' ')
        {
            return Err(RemoteAuthenticateError::InvalidHost(
                "TLS server name must be a DNS name, not a URL or credential".to_string(),
            ));
        }
        let socket = format!("{host}:{https_port}")
            .to_socket_addrs()
            .map_err(|error| {
                RemoteAuthenticateError::Http(format!("resolve appliance host: {error}"))
            })?
            .next()
            .ok_or_else(|| {
                RemoteAuthenticateError::Http(
                    "resolve appliance host returned no address".to_string(),
                )
            })?;
        builder = builder.resolve(tls_server_name, socket);
    }
    if let Some(ca_certificate_pem) = ca_certificate_pem {
        let certificate = reqwest::Certificate::from_pem(ca_certificate_pem).map_err(|error| {
            RemoteAuthenticateError::Http(format!("read CA certificate: {error}"))
        })?;
        builder = builder.add_root_certificate(certificate);
    }
    let client = builder
        .build()
        .map_err(|error| RemoteAuthenticateError::Http(format!("build HTTPS client: {error}")))?;
    let url = format!(
        "https://{request_host}:{https_port}/products/dasobjectstore/api/v1/remote/authenticate"
    );
    let response = client
        .post(url)
        .json(&RemoteAuthenticateRequest {
            username: username.to_string(),
            password: password.to_string(),
            object_store: object_store.to_string(),
            requested_session_lifetime_seconds,
        })
        .send()
        .map_err(|error| RemoteAuthenticateError::Http(request_error_message(error)))?;
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
    validate_descriptor(&response, object_store)?;
    let s3 = response.s3;
    Ok(RemoteConnectionContext {
        schema_version: response.schema_version,
        appliance_host: host.clone(),
        endpoint_url: s3.endpoint_url,
        region: s3.region,
        addressing_style: s3.addressing_style,
        object_store: response.store_id,
        bucket: s3.bucket,
        access_key_id: s3.session.credentials.access_key_id,
        secret_access_key: s3.session.credentials.secret_access_key,
        session_token: s3.session.credentials.session_token,
        session_id: s3.session.session_id,
        issued_at_utc: s3.session.issued_at_utc,
        expires_at_utc: s3.session.expires_at_utc,
        renew_url: absolute_renew_url(&host, https_port, &s3.session.renewal.renew_url),
        renew_after_utc: s3.session.renewal.renew_after_utc,
        renewal_token: s3.session.renewal.renewal_token,
    })
}

fn discover_appliance_id(
    host: &str,
    https_port: u16,
    certificate_pem: &[u8],
    tls_server_name: &str,
) -> Result<String, RemoteAuthenticateError> {
    let socket = format!("{host}:{https_port}")
        .to_socket_addrs()
        .map_err(|error| RemoteAuthenticateError::Http(format!("resolve appliance host: {error}")))?
        .next()
        .ok_or_else(|| {
            RemoteAuthenticateError::Http("resolve appliance host returned no address".to_string())
        })?;
    let certificate = reqwest::Certificate::from_pem(certificate_pem).map_err(|error| {
        RemoteAuthenticateError::Http(format!("read enrolled appliance certificate: {error}"))
    })?;
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .resolve(tls_server_name, socket)
        .add_root_certificate(certificate)
        .build()
        .map_err(|error| {
            RemoteAuthenticateError::Http(format!("build discovery client: {error}"))
        })?;
    let url = format!(
        "https://{tls_server_name}:{https_port}/products/dasobjectstore/api/v1/remote/easyconnect/discovery"
    );
    let response = client.get(url).send().map_err(|error| {
        RemoteAuthenticateError::Http(format!("discover appliance identity: {error}"))
    })?;
    if !response.status().is_success() {
        return Err(RemoteAuthenticateError::Http(format!(
            "discover appliance identity returned HTTP {}",
            response.status()
        )));
    }
    let discovery = response
        .json::<RemoteEasyconnectDiscoveryResponse>()
        .map_err(|error| {
            RemoteAuthenticateError::Http(format!("decode appliance identity: {error}"))
        })?;
    if discovery.appliance_id.trim().is_empty() {
        return Err(RemoteAuthenticateError::Http(
            "appliance discovery returned a blank identity".to_string(),
        ));
    }
    Ok(discovery.appliance_id)
}

fn validate_descriptor(
    response: &RemoteAuthenticateResponse,
    requested_store: &str,
) -> Result<(), RemoteAuthenticateError> {
    if response.store_id != requested_store {
        return Err(RemoteAuthenticateError::Http(
            "appliance returned an S3 descriptor for a different ObjectStore".to_string(),
        ));
    }
    let s3 = &response.s3;
    let endpoint = reqwest::Url::parse(&s3.endpoint_url).map_err(|_| {
        RemoteAuthenticateError::Http("appliance returned a malformed S3 endpoint URL".to_string())
    })?;
    if !matches!(endpoint.scheme(), "http" | "https")
        || endpoint.host_str().is_none()
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
        || endpoint.path() != "/"
    {
        return Err(RemoteAuthenticateError::Http(
            "appliance S3 endpoint must be an HTTP(S) origin without credentials, path, query, or fragment".to_string(),
        ));
    }
    if s3.bucket.trim().is_empty()
        || s3.bucket.len() > 63
        || s3.region.trim().is_empty()
        || !matches!(s3.addressing_style.as_str(), "path" | "virtual")
        || s3.session.credentials.access_key_id.trim().is_empty()
        || s3.session.credentials.secret_access_key.is_empty()
        || s3
            .session
            .credentials
            .session_token
            .as_deref()
            .unwrap_or("")
            .is_empty()
        || s3.session.expires_at_utc.trim().is_empty()
    {
        return Err(RemoteAuthenticateError::Http(
            "appliance returned an incomplete or unsupported S3 connection descriptor".to_string(),
        ));
    }
    Ok(())
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

fn validate_public_certificate(certificate: &[u8]) -> Result<(), RemoteAuthenticateError> {
    let text = std::str::from_utf8(certificate).map_err(|_| {
        RemoteAuthenticateError::Http("appliance CA response is not PEM text".to_string())
    })?;
    if !text.contains("-----BEGIN CERTIFICATE-----") || text.contains("PRIVATE KEY") {
        return Err(RemoteAuthenticateError::Http(
            "appliance CA response is not a public certificate".to_string(),
        ));
    }
    reqwest::Certificate::from_pem(certificate).map_err(|error| {
        RemoteAuthenticateError::Http(format!("appliance CA certificate is invalid: {error}"))
    })?;
    Ok(())
}

fn redact(value: &str) -> String {
    let prefix = value.chars().take(4).collect::<String>();
    format!("{prefix}...redacted")
}

fn request_error_message(error: reqwest::Error) -> String {
    let debug = format!("{error:?}").to_ascii_lowercase();
    if debug.contains("certificate") || debug.contains("tls") {
        return "HTTPS authentication failed during certificate verification; pass --ca-cert with the appliance certificate and --tls-server-name matching its certificate (the packaged appliance certificate commonly uses localhost)".to_string();
    }
    format!("HTTPS authentication request failed: {error}")
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
            endpoint_url: "https://objects.example:9443".to_string(),
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
