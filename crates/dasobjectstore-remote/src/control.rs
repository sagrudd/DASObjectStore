//! Authenticated, pinned-CA HTTPS control-plane client.
//!
//! Object bytes remain on the S3 data plane. This module only exposes
//! store-scoped operations that S3 cannot represent.

use crate::authenticate::appliance_trust_path;
use crate::config::{
    RemoteConfig, RemoteObjectStoreGrant, RemotePairedAppliance, RemoteSessionCredentials,
    RemoteSessionRenewalMetadata, RemoteUploadSession,
};
use dasobjectstore_daemon::{
    RemoteEasyconnectRenewSessionRequest, RemoteEasyconnectRenewSessionResponse,
};
use reqwest::blocking::{Client, Response};
use reqwest::{Method, StatusCode};
use serde::Serialize;
use serde_json::Value;
use std::fmt;
use std::fs;
use std::time::Duration;

const CONTROL_ROOT: &str = "/products/dasobjectstore/api/v1/remote";
const ACCESS_KEY_HEADER: &str = "x-dasobjectstore-access-key-id";
const OBJECT_STORE_HEADER: &str = "x-dasobjectstore-object-store";

/// Rotate a due temporary session before a long-running control operation.
///
/// The login password is not retained. Renewal uses the one-time rotating
/// renewal token and replaces both S3 and HTTPS session material atomically in
/// the caller's in-memory config.
pub fn renew_store_session_if_due(
    config: &mut RemoteConfig,
    store: &str,
) -> Result<bool, RemoteControlError> {
    let appliance_index = config
        .paired_appliances
        .iter()
        .position(|appliance| {
            appliance
                .object_stores
                .iter()
                .any(|grant| grant.object_store == store)
        })
        .ok_or_else(|| {
            RemoteControlError::Authorization(format!(
                "ObjectStore {store} is not present in this session's grants"
            ))
        })?;
    let appliance = &config.paired_appliances[appliance_index];
    let session = appliance.session.as_ref().ok_or_else(|| {
        RemoteControlError::Authentication("paired appliance has no active session".to_string())
    })?;
    let renewal = match &session.renewal {
        Some(renewal) => renewal,
        None => return Ok(false),
    };
    let now = unix_now()?;
    let renew_after =
        dasobjectstore_core::utc::parse_canonical_utc_timestamp_seconds(&renewal.renew_after)
            .ok_or_else(|| {
                RemoteControlError::Authentication(
                    "session has an invalid renewal timestamp; authenticate again".to_string(),
                )
            })?;
    if now < renew_after {
        return Ok(false);
    }
    let renewal_token = renewal.renewal_token.as_deref().ok_or_else(|| {
        RemoteControlError::Authentication(
            "session is due for renewal but has no renewal token; authenticate again".to_string(),
        )
    })?;
    let transport = pinned_client(appliance)?;
    let renew_url = rewrite_url_for_transport(&renewal.renew_url, &transport.base_url)?;
    let response = transport
        .client
        .post(renew_url)
        .json(&RemoteEasyconnectRenewSessionRequest {
            session_id: session.session_id.clone(),
            renewal_token: renewal_token.to_string(),
            requested_lifetime_seconds: None,
        })
        .send()
        .map_err(RemoteControlError::Transport)?;
    let status = response.status();
    if !status.is_success() {
        return parse_response(response).map(|_| false);
    }
    let renewed = response
        .json::<RemoteEasyconnectRenewSessionResponse>()
        .map_err(RemoteControlError::Transport)?
        .session;
    config.paired_appliances[appliance_index].session = Some(RemoteUploadSession {
        session_id: renewed.session_id,
        issued_at: renewed.issued_at_utc.clone(),
        expires_at: renewed.expires_at_utc,
        credentials: RemoteSessionCredentials {
            access_key_id: renewed.credentials.access_key_id,
            secret_access_key: renewed.credentials.secret_access_key,
            session_token: renewed.credentials.session_token,
        },
        renewal: Some(RemoteSessionRenewalMetadata {
            renew_url: renewal.renew_url.clone(),
            renew_after: renewed.renewal.renew_after_utc,
            renewal_token: Some(renewed.renewal.renewal_token),
            last_renewed_at: Some(renewed.issued_at_utc),
        }),
    });
    Ok(true)
}

#[derive(Clone, Debug)]
pub struct RemoteControlClient {
    client: Client,
    base_url: String,
    access_key_id: String,
    control_token: String,
    object_store: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReconcileS3Request<'a> {
    pub key: &'a str,
    pub expected_bytes: u64,
    pub expected_sha256: &'a str,
    pub idempotency_key: &'a str,
    pub ack_policy: &'a str,
}

impl RemoteControlClient {
    pub fn for_store(
        config: &RemoteConfig,
        store: &str,
        write_required: bool,
    ) -> Result<(Self, RemoteObjectStoreGrant), RemoteControlError> {
        let (appliance, grant) = find_grant(config, store, write_required)?;
        let session = appliance.session.as_ref().ok_or_else(|| {
            RemoteControlError::Authentication(
                "paired appliance has no active session; authenticate again".to_string(),
            )
        })?;
        let token = session
            .credentials
            .session_token
            .as_deref()
            .ok_or_else(|| {
                RemoteControlError::Authentication(
                    "session has no temporary session token; authenticate again".to_string(),
                )
            })?;
        reject_expired_session(session)?;
        let transport = pinned_client(appliance)?;
        Ok((
            Self {
                client: transport.client,
                base_url: transport.base_url,
                access_key_id: session.credentials.access_key_id.clone(),
                control_token: token.to_string(),
                object_store: store.to_string(),
            },
            grant.clone(),
        ))
    }

    pub fn readiness(&self, store: &str) -> Result<Value, RemoteControlError> {
        self.send_json(
            Method::GET,
            &format!("{CONTROL_ROOT}/stores/{}/readiness", encode_segment(store)?),
            &[],
            Option::<&()>::None,
        )
    }

    pub fn snapshot(
        &self,
        store: &str,
        prefix: &str,
        cursor: Option<&str>,
        limit: u32,
    ) -> Result<Value, RemoteControlError> {
        let mut query = vec![("prefix", prefix.to_string()), ("limit", limit.to_string())];
        if let Some(cursor) = cursor {
            query.push(("cursor", cursor.to_string()));
        }
        self.send_json(
            Method::GET,
            &format!(
                "{CONTROL_ROOT}/stores/{}/objects/snapshot",
                encode_segment(store)?
            ),
            &query,
            Option::<&()>::None,
        )
    }

    pub fn group_status(&self, store: &str, key: &str) -> Result<Value, RemoteControlError> {
        self.send_json(
            Method::GET,
            &format!(
                "{CONTROL_ROOT}/stores/{}/objects/group-status",
                encode_segment(store)?
            ),
            &[("key", key.to_string())],
            Option::<&()>::None,
        )
    }

    pub fn reconcile_s3(
        &self,
        store: &str,
        request: &ReconcileS3Request<'_>,
    ) -> Result<Value, RemoteControlError> {
        self.send_json(
            Method::POST,
            &format!(
                "{CONTROL_ROOT}/stores/{}/objects/reconcile-s3",
                encode_segment(store)?
            ),
            &[],
            Some(request),
        )
    }

    pub fn operation_status(&self, operation_id: &str) -> Result<Value, RemoteControlError> {
        self.send_json(
            Method::GET,
            &format!(
                "{CONTROL_ROOT}/operations/{}",
                encode_segment(operation_id)?
            ),
            &[],
            Option::<&()>::None,
        )
    }

    fn send_json<T: Serialize + ?Sized>(
        &self,
        method: Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<&T>,
    ) -> Result<Value, RemoteControlError> {
        let mut request = self
            .client
            .request(method, format!("{}{}", self.base_url, path))
            .bearer_auth(&self.control_token)
            .header(ACCESS_KEY_HEADER, &self.access_key_id)
            .header(OBJECT_STORE_HEADER, &self.object_store)
            .query(query);
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().map_err(RemoteControlError::Transport)?;
        parse_response(response)
    }
}

fn find_grant<'a>(
    config: &'a RemoteConfig,
    store: &str,
    write_required: bool,
) -> Result<(&'a RemotePairedAppliance, &'a RemoteObjectStoreGrant), RemoteControlError> {
    config
        .paired_appliances
        .iter()
        .find_map(|appliance| {
            appliance.object_stores.iter().find_map(|grant| {
                (grant.object_store == store
                    && grant.can_read
                    && (!write_required || grant.can_write))
                    .then_some((appliance, grant))
            })
        })
        .ok_or_else(|| {
            RemoteControlError::Authorization(format!(
                "ObjectStore {store} is not present in this session's {} grants",
                if write_required {
                    "writable"
                } else {
                    "readable"
                }
            ))
        })
}

struct PinnedControlTransport {
    client: Client,
    base_url: String,
}

fn pinned_client(
    appliance: &RemotePairedAppliance,
) -> Result<PinnedControlTransport, RemoteControlError> {
    let url = reqwest::Url::parse(&appliance.appliance_base_url)
        .map_err(|_| RemoteControlError::Configuration("invalid appliance base URL".to_string()))?;
    if url.scheme() != "https" {
        return Err(RemoteControlError::Configuration(
            "remote control requires an HTTPS appliance URL".to_string(),
        ));
    }
    let host = url.host_str().ok_or_else(|| {
        RemoteControlError::Configuration("appliance URL has no host".to_string())
    })?;
    let port = url.port_or_known_default().unwrap_or(8448);
    let trust_path = appliance_trust_path(host, port)?;
    let certificate = reqwest::Certificate::from_pem(&fs::read(&trust_path)?).map_err(|_| {
        RemoteControlError::Configuration(format!(
            "pinned appliance certificate at {} is invalid; authenticate again",
            trust_path.display()
        ))
    })?;
    let mut builder = Client::builder().add_root_certificate(certificate);
    let base_url = if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        let socket = std::net::SocketAddr::new(ip, port);
        builder = builder.resolve("localhost", socket);
        format!("https://localhost:{port}")
    } else {
        appliance
            .appliance_base_url
            .trim_end_matches('/')
            .to_string()
    };
    let client = builder
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(RemoteControlError::Transport)?;
    Ok(PinnedControlTransport { client, base_url })
}

fn rewrite_url_for_transport(
    absolute_url: &str,
    transport_base: &str,
) -> Result<String, RemoteControlError> {
    let url = reqwest::Url::parse(absolute_url).map_err(|_| {
        RemoteControlError::Configuration("invalid session renewal URL".to_string())
    })?;
    let mut rewritten = format!("{transport_base}{}", url.path());
    if let Some(query) = url.query() {
        rewritten.push('?');
        rewritten.push_str(query);
    }
    Ok(rewritten)
}

fn reject_expired_session(session: &RemoteUploadSession) -> Result<(), RemoteControlError> {
    let expiry =
        dasobjectstore_core::utc::parse_canonical_utc_timestamp_seconds(&session.expires_at)
            .ok_or_else(|| {
                RemoteControlError::Authentication(
                    "session has an invalid expiry; authenticate again".to_string(),
                )
            })?;
    let now = unix_now()?;
    if expiry <= now {
        return Err(RemoteControlError::Authentication(
            "temporary session has expired; renew or authenticate again".to_string(),
        ));
    }
    Ok(())
}

fn unix_now() -> Result<i64, RemoteControlError> {
    Ok(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| RemoteControlError::Configuration(error.to_string()))?
        .as_secs() as i64)
}

fn encode_segment(value: &str) -> Result<String, RemoteControlError> {
    if value.is_empty() || value == "." || value == ".." {
        return Err(RemoteControlError::Configuration(
            "remote identifier must not be blank or relative".to_string(),
        ));
    }
    Ok(value
        .bytes()
        .flat_map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') {
                vec![byte as char]
            } else {
                format!("%{byte:02X}").chars().collect()
            }
        })
        .collect())
}

fn parse_response(response: Response) -> Result<Value, RemoteControlError> {
    let status = response.status();
    let retry_after = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let body = response.json::<Value>().unwrap_or_else(|_| {
        serde_json::json!({"code": "invalid_control_response", "message": "appliance returned a non-JSON response"})
    });
    if status.is_success() {
        return Ok(body);
    }
    let code = body
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("remote_control_error")
        .to_string();
    let message = body
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("the appliance rejected the remote control request")
        .to_string();
    Err(RemoteControlError::Server {
        status,
        code,
        message,
        retry_after,
    })
}

#[derive(Debug)]
pub enum RemoteControlError {
    Io(std::io::Error),
    Authenticate(crate::authenticate::RemoteAuthenticateError),
    Configuration(String),
    Authentication(String),
    Authorization(String),
    Transport(reqwest::Error),
    Server {
        status: StatusCode,
        code: String,
        message: String,
        retry_after: Option<String>,
    },
}

impl fmt::Display for RemoteControlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Authenticate(error) => write!(formatter, "{error}"),
            Self::Configuration(message)
            | Self::Authentication(message)
            | Self::Authorization(message) => formatter.write_str(message),
            Self::Transport(error) => {
                write!(formatter, "remote HTTPS control request failed: {error}")
            }
            Self::Server {
                status,
                code,
                message,
                retry_after,
            } => {
                write!(
                    formatter,
                    "remote control rejected ({status}, {code}): {message}"
                )?;
                if let Some(retry_after) = retry_after {
                    write!(formatter, "; retry after {retry_after}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for RemoteControlError {}

impl From<std::io::Error> for RemoteControlError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<crate::authenticate::RemoteAuthenticateError> for RemoteControlError {
    fn from(error: crate::authenticate::RemoteAuthenticateError) -> Self {
        Self::Authenticate(error)
    }
}

#[cfg(test)]
mod tests {
    use super::encode_segment;

    #[test]
    fn encodes_route_segments_without_permitting_relative_identifiers() {
        assert_eq!(
            encode_segment("epic_collection").unwrap(),
            "epic_collection"
        );
        assert_eq!(encode_segment("zymo.2025").unwrap(), "zymo.2025");
        assert_eq!(encode_segment("operation/one").unwrap(), "operation%2Fone");
        assert!(encode_segment("..").is_err());
    }
}
