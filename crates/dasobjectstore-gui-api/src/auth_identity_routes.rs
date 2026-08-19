//! Standalone local authentication and EasyConnect route handlers.

use super::*;

mod easyconnect_approval_page;
mod pistis_approval_route;
use dasobjectstore_daemon::{
    RemoteEasyconnectApprovalContext, RemoteEasyconnectPairingStatusRequest,
    RemoteEasyconnectPairingStatusResponse,
};
pub(super) use easyconnect_approval_page::{
    easyconnect_browser_approval, easyconnect_pairing_status,
};
pub(super) use pistis_approval_route::pistis_easyconnect_approve_pairing;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StandaloneS3ConnectionDescriptor {
    pub endpoint_url: String,
    pub region: String,
    pub addressing_style: String,
}

/// Deployment-owned S3 endpoint identity and the certificate chain used to
/// verify it before an EasyConnect approval or exchange transition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EasyconnectS3EndpointConfig {
    pub descriptor: StandaloneS3ConnectionDescriptor,
    pub tls_certificate_path: PathBuf,
}

/// Trusted in-process selection of the local daemon socket used by
/// EasyConnect routes.
///
/// Embedders may use a non-default absolute path for hermetic conformance
/// testing. The path is never accepted from an HTTP request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EasyconnectDaemonEndpoint {
    socket_path: PathBuf,
}

impl EasyconnectDaemonEndpoint {
    pub fn new(socket_path: PathBuf) -> Result<Self, &'static str> {
        if !socket_path.is_absolute() {
            return Err("EasyConnect daemon socket path must be absolute");
        }
        Ok(Self { socket_path })
    }

    pub(super) fn socket_path(&self) -> PathBuf {
        self.socket_path.clone()
    }
}

impl Default for EasyconnectDaemonEndpoint {
    fn default() -> Self {
        Self {
            socket_path: DaemonRuntimeConfig::default_packaged().socket_path,
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct EasyconnectPublicRouteState {
    pub(super) s3_endpoint: Option<EasyconnectS3EndpointConfig>,
    pub(super) public_base_url: Option<String>,
    pub(super) appliance_id: String,
    pub(super) daemon_endpoint: EasyconnectDaemonEndpoint,
}

#[cfg(not(test))]
pub(super) fn system_appliance_id() -> String {
    let identity = dasobjectstore_daemon::runtime::ensure_appliance_identity(
        &dasobjectstore_daemon::DaemonRuntimeConfig::default_packaged().state_dir,
    );
    match identity {
        Ok(identity) => identity.appliance_id,
        #[cfg(debug_assertions)]
        Err(_) => "das-appliance-development".to_string(),
        #[cfg(not(debug_assertions))]
        Err(error) => panic!("authoritative appliance identity must be readable: {error}"),
    }
}

#[cfg(test)]
pub(super) fn system_appliance_id() -> String {
    "das-appliance-test".to_string()
}

/// Exchange an application's signed proof for a daemon-owned short-lived
/// token; this route deliberately does not accept a local GUI session token.
pub(super) async fn exchange_application_access_token(
    Json(request): Json<DaemonApplicationAccessTokenExchangeRequest>,
) -> Result<Json<DaemonApplicationAccessTokenExchangeResponse>, (StatusCode, Json<AuthRouteError>)>
{
    request.validate().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_application_access_token_exchange",
            error.to_string(),
        )
    })?;
    crate::daemon_bridge::DaemonBridge::shared_packaged()
        .call_message(move || {
            let client = DaemonClient::new(UnixSocketDaemonTransport::for_bounded_bridge(
                DaemonRuntimeConfig::default_packaged().socket_path,
            ));
            client
                .exchange_application_access_token(request)
                .map_err(|error| error.to_string())
        })
        .await
        .map(Json)
        .map_err(|error| {
            admin_daemon_bridge_error_with_code(error, "application_access_token_exchange_failed")
        })
}

pub(super) async fn discover_ergasterion_capability() -> Result<
    (
        HeaderMap,
        Json<DaemonErgasterionCapabilityDiscoveryResponse>,
    ),
    (StatusCode, Json<AuthRouteError>),
> {
    let response =
        application_daemon_call(|client| client.discover_ergasterion_capability()).await?;
    Ok((no_store_headers(), Json(response)))
}

pub(super) async fn exchange_ergasterion_capability(
    Json(request): Json<DaemonErgasterionCapabilityExchangeRequest>,
) -> Result<
    (HeaderMap, Json<DaemonErgasterionCapabilityExchangeResponse>),
    (StatusCode, Json<AuthRouteError>),
> {
    request.validate().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            error.to_string(),
        )
    })?;
    let response =
        application_daemon_call(move |client| client.exchange_ergasterion_capability(request))
            .await?;
    Ok((no_store_headers(), Json(response)))
}

pub(super) async fn renew_ergasterion_capability(
    Json(request): Json<DaemonErgasterionCapabilityRenewalRequest>,
) -> Result<
    (HeaderMap, Json<DaemonErgasterionCapabilityExchangeResponse>),
    (StatusCode, Json<AuthRouteError>),
> {
    request.validate().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            error.to_string(),
        )
    })?;
    let response =
        application_daemon_call(move |client| client.renew_ergasterion_capability(request)).await?;
    Ok((no_store_headers(), Json(response)))
}

pub(super) async fn ergasterion_object_snapshot(
    headers: HeaderMap,
    Json(snapshot): Json<RemoteObjectSnapshotRequest>,
) -> Result<
    (HeaderMap, Json<DaemonErgasterionObjectSnapshotResponse>),
    (StatusCode, Json<AuthRouteError>),
> {
    let capability = application_bearer(&headers)?;
    let request = DaemonErgasterionObjectSnapshotRequest {
        capability,
        snapshot,
    };
    request.validate().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            error.to_string(),
        )
    })?;
    let response =
        application_daemon_call(move |client| client.ergasterion_object_snapshot(request)).await?;
    Ok((no_store_headers(), Json(response)))
}

pub(super) async fn ergasterion_object_group_status(
    headers: HeaderMap,
    Json(status): Json<RemoteObjectGroupStatusRequest>,
) -> Result<
    (HeaderMap, Json<DaemonErgasterionObjectGroupStatusResponse>),
    (StatusCode, Json<AuthRouteError>),
> {
    let capability = application_bearer(&headers)?;
    let request = DaemonErgasterionObjectGroupStatusRequest { capability, status };
    request.validate().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            error.to_string(),
        )
    })?;
    let response =
        application_daemon_call(move |client| client.ergasterion_object_group_status(request))
            .await?;
    Ok((no_store_headers(), Json(response)))
}

pub(super) async fn ergasterion_object_read(
    Path((store_id, version, object_key)): Path<(String, u64, String)>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, Json<AuthRouteError>)> {
    let capability = application_bearer(&headers)?;
    let store_id = store_id.parse::<StoreId>().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            error.to_string(),
        )
    })?;
    profile_download::application_provider_stream_download(
        store_id,
        object_key,
        version,
        capability,
        headers,
        DaemonRuntimeConfig::default_packaged().socket_path,
    )
    .await
}

async fn application_daemon_call<T: Send + 'static>(
    call: impl FnOnce(
            DaemonClient<UnixSocketDaemonTransport>,
        ) -> Result<T, dasobjectstore_daemon::DaemonClientError>
        + Send
        + 'static,
) -> Result<T, (StatusCode, Json<AuthRouteError>)> {
    crate::daemon_bridge::DaemonBridge::shared_packaged()
        .call_message(move || {
            call(DaemonClient::new(
                UnixSocketDaemonTransport::for_bounded_bridge(
                    DaemonRuntimeConfig::default_packaged().socket_path,
                ),
            ))
            .map_err(|error| match error {
                dasobjectstore_daemon::DaemonClientError::Api(error) => {
                    format!("__application_api__:{}:{}", error.code, error.message)
                }
                error => error.to_string(),
            })
        })
        .await
        .map_err(application_daemon_error)
}

fn application_daemon_error(
    error: crate::daemon_bridge::DaemonBridgeError,
) -> (StatusCode, Json<AuthRouteError>) {
    if let crate::daemon_bridge::DaemonBridgeError::Client(client) = &error {
        if let Some(encoded) = client.message.strip_prefix("__application_api__:") {
            if let Some((code, message)) = encoded.split_once(':') {
                let status = match code {
                    "invalid_request" => StatusCode::BAD_REQUEST,
                    "proof_invalid" | "capability_revoked" => StatusCode::UNAUTHORIZED,
                    "governed_scope_denied" => StatusCode::FORBIDDEN,
                    "replay_detected" => StatusCode::CONFLICT,
                    "authority_unavailable" | "provider_unavailable" => {
                        StatusCode::SERVICE_UNAVAILABLE
                    }
                    _ => StatusCode::BAD_GATEWAY,
                };
                return route_error(status, code, message);
            }
        }
    }
    let status = match error {
        crate::daemon_bridge::DaemonBridgeError::Busy => StatusCode::TOO_MANY_REQUESTS,
        crate::daemon_bridge::DaemonBridgeError::CircuitOpen
        | crate::daemon_bridge::DaemonBridgeError::Deadline
        | crate::daemon_bridge::DaemonBridgeError::Join(_)
        | crate::daemon_bridge::DaemonBridgeError::Client(_) => StatusCode::SERVICE_UNAVAILABLE,
    };
    route_error(
        status,
        "provider_unavailable",
        "application capability authority is temporarily unavailable",
    )
}

fn application_bearer(
    headers: &HeaderMap,
) -> Result<DaemonOpaqueApplicationCapability, (StatusCode, Json<AuthRouteError>)> {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or_else(|| {
            route_error(
                StatusCode::UNAUTHORIZED,
                "capability_revoked",
                "a bearer capability is required",
            )
        })?;
    DaemonOpaqueApplicationCapability::new(value.to_string()).map_err(|error| {
        route_error(
            StatusCode::UNAUTHORIZED,
            "capability_revoked",
            error.to_string(),
        )
    })
}

fn no_store_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers
}

pub(super) async fn issue_application_upload_capability(
    Json(request): Json<DaemonApplicationUploadCapabilityIssueRequest>,
) -> Result<Json<DaemonApplicationUploadCapabilityIssueResponse>, (StatusCode, Json<AuthRouteError>)>
{
    crate::daemon_bridge::DaemonBridge::shared_packaged()
        .call_message(move || {
            DaemonClient::new(UnixSocketDaemonTransport::for_bounded_bridge(
                DaemonRuntimeConfig::default_packaged().socket_path,
            ))
            .issue_application_upload_capability(request)
            .map_err(|error| error.to_string())
        })
        .await
        .map(Json)
        .map_err(|error| {
            admin_daemon_bridge_error_with_code(error, "application_upload_capability_issue_failed")
        })
}

pub(super) async fn complete_application_upload(
    Json(request): Json<DaemonApplicationUploadCompletionRequest>,
) -> Result<Json<DaemonApplicationUploadCompletionResponse>, (StatusCode, Json<AuthRouteError>)> {
    request.capability.validate().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_application_upload_completion",
            error.to_string(),
        )
    })?;
    crate::daemon_bridge::DaemonBridge::shared_packaged()
        .call_message(move || {
            DaemonClient::new(UnixSocketDaemonTransport::for_bounded_bridge(
                DaemonRuntimeConfig::default_packaged().socket_path,
            ))
            .complete_application_upload(request)
            .map_err(|error| error.to_string())
        })
        .await
        .map(Json)
        .map_err(|error| {
            admin_daemon_bridge_error_with_code(error, "application_upload_completion_failed")
        })
}

fn remote_auth_bridge_error(
    error: crate::daemon_bridge::DaemonBridgeError,
) -> (StatusCode, Json<AuthRouteError>) {
    match error {
        crate::daemon_bridge::DaemonBridgeError::Client(error) => {
            route_error(StatusCode::SERVICE_UNAVAILABLE, error.code, error.message)
        }
        crate::daemon_bridge::DaemonBridgeError::Busy => route_error(
            StatusCode::TOO_MANY_REQUESTS,
            "remote_session_busy",
            "daemon control capacity is saturated; retry shortly",
        ),
        crate::daemon_bridge::DaemonBridgeError::CircuitOpen => route_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "remote_session_circuit_open",
            "daemon control is temporarily degraded; retry shortly",
        ),
        crate::daemon_bridge::DaemonBridgeError::Deadline => route_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "remote_session_timeout",
            "remote session authentication exceeded its deadline; retry shortly",
        ),
        crate::daemon_bridge::DaemonBridgeError::Join(message) => route_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "remote_session_unavailable",
            message,
        ),
    }
}

pub(super) async fn easyconnect_create_pairing(
    State(state): State<EasyconnectPublicRouteState>,
    Json(request): Json<RemoteEasyconnectCreatePairingRequest>,
) -> Result<Json<RemoteEasyconnectCreatePairingResponse>, (StatusCode, Json<AuthRouteError>)> {
    request.validate().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_easyconnect_pairing",
            error.to_string(),
        )
    })?;
    validate_loopback_callback(&request.callback_url)?;
    let requested_object_store = request.requested_object_store.clone().ok_or_else(|| {
        route_error(
            StatusCode::BAD_REQUEST,
            "easyconnect_object_store_required",
            "passwordless EasyConnect requires one exact ObjectStore",
        )
    })?;
    let daemon_endpoint = state.daemon_endpoint;
    let public_base_url = state.public_base_url.ok_or_else(|| {
        route_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "easyconnect_public_origin_unavailable",
            "the appliance has not configured an authoritative public HTTPS origin",
        )
    })?;
    let mut response = crate::daemon_bridge::DaemonBridge::shared_packaged()
        .call_message(move || {
            DaemonClient::new(UnixSocketDaemonTransport::for_bounded_bridge(
                daemon_endpoint.socket_path(),
            ))
            .remote_easyconnect_create_pairing(request)
            .map_err(|error| error.to_string())
        })
        .await
        .map_err(remote_auth_bridge_error)?;
    let mut browser_url = reqwest::Url::parse(&format!(
        "{}{}",
        public_base_url.trim_end_matches('/'),
        response.browser_login_url
    ))
    .map_err(|_| {
        route_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "invalid_easyconnect_browser_route",
            "daemon returned an invalid EasyConnect browser route",
        )
    })?;
    browser_url
        .query_pairs_mut()
        .append_pair("object_store", &requested_object_store)
        .append_pair("expires_at_utc", &response.expires_at_utc);
    response.browser_login_url = browser_url.to_string();
    response.polling_url = format!(
        "{}{}",
        public_base_url.trim_end_matches('/'),
        response.polling_url
    );
    Ok(Json(response))
}

pub(super) async fn easyconnect_approve_pairing(
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<crate::VerifiedHostAuthenticatedContext>,
    Extension(approval_context): Extension<RemoteEasyconnectApprovalContext>,
    Extension(daemon_endpoint): Extension<EasyconnectDaemonEndpoint>,
    Extension(s3_endpoint): Extension<Option<EasyconnectS3EndpointConfig>>,
    Json(intent): Json<EasyconnectBrowserApprovalIntent>,
) -> Result<Json<RemoteEasyconnectApprovePairingResponse>, (StatusCode, Json<AuthRouteError>)> {
    if approval_context.auth_provider != RemoteEasyconnectAuthProvider::Pistis {
        return Err(route_error(
            StatusCode::FORBIDDEN,
            "pistis_approval_context_required",
            "EasyConnect host approval requires a credential-free Pistis approval context",
        ));
    }
    let expected_expiry = dasobjectstore_core::utc::format_utc_timestamp_seconds(
        verified.context().expires_at_unix_seconds,
    );
    if approval_context.principal_id != actor.subject_id
        || approval_context.session_id != verified.context().session_id
        || approval_context.correlation_id != verified.context().correlation_id
        || approval_context.host_session_expires_at_utc != expected_expiry
    {
        return Err(route_error(
            StatusCode::FORBIDDEN,
            "pistis_approval_context_mismatch",
            "Pistis approval context does not match the current verified host session",
        ));
    }
    let Some(grant) = approval_context
        .allowed_object_stores
        .iter()
        .find(|grant| grant.object_store == intent.object_store)
    else {
        return Err(route_error(
            StatusCode::FORBIDDEN,
            "object_store_not_authorized",
            "the Pistis approval context does not grant the requested ObjectStore",
        ));
    };
    if approval_context.allowed_object_stores.len() != 1 || !grant.can_write {
        return Err(route_error(
            StatusCode::FORBIDDEN,
            "exact_writable_object_store_grant_required",
            "EasyConnect requires exactly one writable ObjectStore grant",
        ));
    }
    let request = RemoteEasyconnectApprovePairingRequest {
        pairing_id: intent.pairing_id,
        approval_context,
    };
    request.validate().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_easyconnect_approval",
            error.to_string(),
        )
    })?;
    verify_easyconnect_s3_endpoint(s3_endpoint.as_ref()).await?;
    crate::daemon_bridge::DaemonBridge::shared_packaged()
        .call_message(move || {
            DaemonClient::new(UnixSocketDaemonTransport::for_bounded_bridge(
                daemon_endpoint.socket_path(),
            ))
            .remote_easyconnect_approve_pairing(request)
            .map_err(|error| error.to_string())
        })
        .await
        .map(Json)
        .map_err(remote_auth_bridge_error)
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

pub(super) async fn easyconnect_exchange_pairing(
    State(state): State<EasyconnectPublicRouteState>,
    Json(request): Json<RemoteEasyconnectExchangePairingRequest>,
) -> Result<Json<RemoteEasyconnectExchangeConnectionResponse>, (StatusCode, Json<AuthRouteError>)> {
    request.validate().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_easyconnect_exchange",
            error.to_string(),
        )
    })?;
    let daemon_endpoint = state.daemon_endpoint.clone();
    let s3_endpoint = state.s3_endpoint.as_ref();
    verify_easyconnect_s3_endpoint(s3_endpoint).await?;
    let s3_descriptor = s3_endpoint
        .expect("verified endpoint configuration is present")
        .descriptor
        .clone();
    let exchange = crate::daemon_bridge::DaemonBridge::shared_packaged()
        .call_message(move || {
            DaemonClient::new(UnixSocketDaemonTransport::for_bounded_bridge(
                daemon_endpoint.socket_path(),
            ))
            .remote_easyconnect_exchange_pairing(request)
            .map_err(|error| error.to_string())
        })
        .await
        .map_err(remote_auth_bridge_error)?;
    Ok(Json(RemoteEasyconnectExchangeConnectionResponse {
        schema_version: "dasobjectstore.remote_easyconnect_exchange.v1".to_string(),
        exchange,
        s3: RemoteEasyconnectS3ConnectionDescriptor {
            endpoint_url: s3_descriptor.endpoint_url,
            region: s3_descriptor.region,
            addressing_style: s3_descriptor.addressing_style,
        },
    }))
}

async fn verify_easyconnect_s3_endpoint(
    s3_endpoint: Option<&EasyconnectS3EndpointConfig>,
) -> Result<(), (StatusCode, Json<AuthRouteError>)> {
    let s3_endpoint = s3_endpoint.ok_or_else(|| {
        route_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "s3_connection_descriptor_unavailable",
            "the appliance has not configured an authoritative public S3 endpoint and TLS certificate chain",
        )
    })?;
    crate::s3_endpoint_probe::verify_public_s3_endpoint(
        &s3_endpoint.descriptor.endpoint_url,
        &s3_endpoint.tls_certificate_path,
    )
    .await
    .map(|_| ())
    .map_err(|error| {
        route_error(
            StatusCode::SERVICE_UNAVAILABLE,
            error.code(),
            error.to_string(),
        )
    })
}

fn validate_loopback_callback(
    callback_url: &str,
) -> Result<(), (StatusCode, Json<AuthRouteError>)> {
    let callback = reqwest::Url::parse(callback_url).map_err(|_| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_easyconnect_callback",
            "callback URL must be an absolute loopback HTTP URL",
        )
    })?;
    let loopback = callback.scheme() == "http"
        && callback.username().is_empty()
        && callback.password().is_none()
        && callback.query().is_none()
        && callback.fragment().is_none()
        && callback.port().is_some()
        && matches!(callback.host_str(), Some("127.0.0.1" | "::1"))
        && callback.path() == "/products/dasobjectstore/remote/easyconnect/callback";
    if !loopback {
        return Err(route_error(
            StatusCode::BAD_REQUEST,
            "invalid_easyconnect_callback",
            "callback URL must use the exact EasyConnect path on 127.0.0.1 or ::1 with an explicit port",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod governed_application_error_tests {
    use super::application_daemon_error;
    use axum::http::StatusCode;

    #[test]
    fn transport_details_are_redacted() {
        let (status, error) =
            application_daemon_error(crate::daemon_bridge::DaemonBridgeError::Join(
                "/run/private/daemon.sock failed".to_string(),
            ));

        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(error.code, "provider_unavailable");
        assert_eq!(
            error.message,
            "application capability authority is temporarily unavailable"
        );
    }
}
