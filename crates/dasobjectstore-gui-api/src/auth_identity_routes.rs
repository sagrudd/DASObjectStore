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

#[derive(Clone)]
pub(crate) struct StandaloneAuthRouteState {
    pub(super) auth_store: LocalAuthStore,
    pub(super) local_password_authenticator: Arc<dyn LocalPasswordAuthenticator>,
    pub(super) s3_descriptor: Option<StandaloneS3ConnectionDescriptor>,
    pub(super) s3_tls_certificate_path: PathBuf,
}

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

#[derive(Clone)]
pub(crate) struct StandaloneEasyconnectRouteState {
    pub(super) auth_store: LocalAuthStore,
    pub(super) public_base_url: String,
    pub(super) appliance_id: String,
    pub(super) s3_endpoint: Option<EasyconnectS3EndpointConfig>,
}

#[derive(Clone, Default)]
pub(crate) struct EasyconnectPublicRouteState {
    pub(super) s3_endpoint: Option<EasyconnectS3EndpointConfig>,
    pub(super) public_base_url: Option<String>,
    pub(super) appliance_id: String,
}

impl StandaloneAuthRouteState {
    pub(super) fn system(auth_store: LocalAuthStore) -> Self {
        Self {
            auth_store,
            local_password_authenticator: Arc::new(SystemLocalPasswordAuthenticator::default()),
            s3_descriptor: None,
            s3_tls_certificate_path: crate::StandaloneServerConfig::default_localhost()
                .tls
                .certificate_path,
        }
    }
}

impl StandaloneEasyconnectRouteState {
    pub(super) fn system(auth_store: LocalAuthStore) -> Self {
        Self {
            auth_store,
            public_base_url: crate::DEFAULT_STANDALONE_PUBLIC_BASE_URL.to_string(),
            appliance_id: system_appliance_id(),
            s3_endpoint: None,
        }
    }
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

pub(super) trait LocalPasswordAuthenticator: Send + Sync {
    fn authenticate(&self, username: &str, password: &str) -> Result<(), LocalPasswordAuthError>;
}

#[derive(Default)]
pub(super) struct SystemLocalPasswordAuthenticator {
    pam: PamLocalPasswordAuthenticator,
}

impl LocalPasswordAuthenticator for SystemLocalPasswordAuthenticator {
    fn authenticate(&self, username: &str, password: &str) -> Result<(), LocalPasswordAuthError> {
        self.pam.authenticate(username, password)
    }
}

pub(super) async fn register(
    State(state): State<StandaloneAuthRouteState>,
    Json(request): Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>, (StatusCode, Json<AuthRouteError>)> {
    state
        .auth_store
        .register_with_token(&request.username, &request.token, &request.password)
        .map(Json)
        .map_err(auth_route_error)
}

pub(super) async fn login(
    State(state): State<StandaloneAuthRouteState>,
    Json(request): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, (StatusCode, Json<AuthRouteError>)> {
    state
        .local_password_authenticator
        .authenticate(&request.username, &request.password)
        .map_err(local_password_auth_route_error)?;
    state
        .auth_store
        .create_session_for_authenticated_local_user(&request.username, request.session_ttl_seconds)
        .map(Json)
        .map_err(auth_route_error)
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

/// Authenticate a remote user and issue one daemon-owned, store-scoped S3
/// session. The password is used only for this request and never crosses the
/// daemon boundary or gets persisted in the remote-client configuration.
pub(super) async fn remote_authenticate(
    State(state): State<StandaloneAuthRouteState>,
    Json(request): Json<RemoteAuthenticateRequest>,
) -> Result<Json<RemoteAuthenticateResponse>, (StatusCode, Json<AuthRouteError>)> {
    validate_remote_authenticate_request(&request)?;
    state
        .local_password_authenticator
        .authenticate(&request.username, &request.password)
        .map_err(local_password_auth_route_error)?;

    let current_user = discover_local_user(&request.username).map_err(|error| {
        route_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "local_user_discovery_failed",
            error.to_string(),
        )
    })?;
    let workspace = crate::remote_upload_aggregator::live_remote_upload_workspace_for_user(
        current_user.username.clone(),
        current_user.groups.clone(),
        current_user.sudo_administrator,
    );
    let store = workspace
        .stores
        .iter()
        .find(|store| store.store_id == request.object_store)
        .ok_or_else(|| {
            route_error(
                StatusCode::FORBIDDEN,
                "object_store_not_authorized",
                "the authenticated user has no remote access to the requested ObjectStore",
            )
        })?;
    if !store.upload_allowed {
        return Err(route_error(
            StatusCode::FORBIDDEN,
            "object_store_write_authorization_required",
            "remote S3 sessions currently require a writable ObjectStore grant",
        ));
    }
    let s3_descriptor = state.s3_descriptor.as_ref().ok_or_else(|| {
        route_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "s3_connection_descriptor_unavailable",
            "the appliance has not configured an authoritative public S3 endpoint, region, and addressing style",
        )
    })?;
    let verified_endpoint = crate::s3_endpoint_probe::verify_public_s3_endpoint(
        &s3_descriptor.endpoint_url,
        &state.s3_tls_certificate_path,
    )
    .await
    .map_err(|error| {
        route_error(
            StatusCode::SERVICE_UNAVAILABLE,
            error.code(),
            error.to_string(),
        )
    })?;

    let grant = RemoteEasyconnectObjectStoreGrant {
        object_store: store.store_id.clone(),
        bucket: store.bucket.clone(),
        can_read: true,
        can_write: true,
        writer_group: store.writer_group.clone(),
        object_type: store.object_type.clone(),
        control_operations: dasobjectstore_daemon::api::remote_easyconnect_control_operations(true),
        allowed_prefixes: vec![
            dasobjectstore_daemon::api::REMOTE_EASYCONNECT_DEFAULT_CONTROL_PREFIX.to_string(),
        ],
    };
    let requested_object_store = request.object_store.clone();
    let requested_lifetime = request.requested_session_lifetime_seconds;
    let session = crate::daemon_bridge::DaemonBridge::shared_packaged()
        .call_message(move || {
            let client = DaemonClient::new(UnixSocketDaemonTransport::for_bounded_bridge(
                DaemonRuntimeConfig::default_packaged().socket_path,
            ));
            let created = client
                .remote_easyconnect_create_pairing(RemoteEasyconnectCreatePairingRequest {
                    client_name: "dasobjectstore-remote authenticate".to_string(),
                    callback_url: "https://127.0.0.1/api/v1/remote/authenticate/callback"
                        .to_string(),
                    requested_object_store: Some(requested_object_store),
                    requested_session_lifetime_seconds: requested_lifetime,
                    client_request_id: None,
                })
                .map_err(|error| error.to_string())?;
            let approved = client
                .remote_easyconnect_approve_pairing(RemoteEasyconnectApprovePairingRequest {
                    pairing_id: created.pairing_id.clone(),
                    approval_context: RemoteEasyconnectApprovalContext {
                        authority_id: "dasobjectstore-local-os".to_string(),
                        principal_id: current_user.username.clone(),
                        session_id: format!("local-password:{}", created.pairing_id),
                        auth_provider: RemoteEasyconnectAuthProvider::StandaloneLocalUser,
                        allowed_object_stores: vec![grant],
                        host_session_expires_at_utc: created.expires_at_utc,
                        correlation_id: format!("remote-authenticate:{}", created.pairing_id),
                        audit_identity: format!("local-os:{}", current_user.username),
                    },
                })
                .map_err(|error| error.to_string())?;
            let exchanged = client
                .remote_easyconnect_exchange_pairing(RemoteEasyconnectExchangePairingRequest {
                    pairing_id: approved.pairing_id,
                    exchange_code: approved.exchange_code,
                    client_request_id: None,
                })
                .map_err(|error| error.to_string())?;
            Ok(exchanged.session)
        })
        .await
        .map_err(remote_auth_bridge_error)?;

    let endpoint_port = verified_endpoint.port;
    Ok(Json(RemoteAuthenticateResponse {
        schema_version: "dasobjectstore.remote_authenticate.v4".to_string(),
        appliance_id: system_appliance_id(),
        store_id: request.object_store.clone(),
        s3: RemoteAuthenticatedS3Descriptor {
            schema_version: "dasobjectstore.authenticated_s3_endpoint.v1".to_string(),
            endpoint_url: s3_descriptor.endpoint_url.clone(),
            scheme: verified_endpoint.scheme.clone(),
            host: verified_endpoint.host,
            port: verified_endpoint.port,
            region: s3_descriptor.region.clone(),
            addressing_style: s3_descriptor.addressing_style.clone(),
            bucket: store.bucket.clone(),
            tls: RemoteAuthenticatedS3TlsRequirements {
                required: verified_endpoint.scheme == "https",
                trust_mode: if verified_endpoint.scheme == "https" {
                    "appliance_ca".to_string()
                } else {
                    "plaintext".to_string()
                },
                ca_certificate_url: None,
            },
            credential_expires_at: session.expires_at_utc.clone(),
            capabilities: vec![
                "list_objects_v2".to_string(),
                "head_object".to_string(),
                "put_object".to_string(),
                "multipart_upload".to_string(),
                "path_style_addressing".to_string(),
            ],
            endpoint_protocol_verified: true,
            session: session.clone(),
        },
        endpoint_port,
        region: s3_descriptor.region.clone(),
        addressing_style: s3_descriptor.addressing_style.clone(),
        object_store: request.object_store,
        bucket: store.bucket.clone(),
        session,
    }))
}

pub(super) async fn remote_easyconnect_renew(
    Path(session_id): Path<String>,
    Json(request): Json<RemoteEasyconnectRenewSessionRequest>,
) -> Result<Json<RemoteEasyconnectRenewSessionResponse>, (StatusCode, Json<AuthRouteError>)> {
    if session_id != request.session_id {
        return Err(route_error(
            StatusCode::BAD_REQUEST,
            "remote_session_identity_mismatch",
            "session identity in the route and renewal request must match",
        ));
    }
    request.validate().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_remote_session_renewal",
            error.to_string(),
        )
    })?;
    crate::daemon_bridge::DaemonBridge::shared_packaged()
        .call_message(move || {
            DaemonClient::new(UnixSocketDaemonTransport::for_bounded_bridge(
                DaemonRuntimeConfig::default_packaged().socket_path,
            ))
            .remote_easyconnect_renew_session(request)
            .map_err(|error| error.to_string())
        })
        .await
        .map(Json)
        .map_err(remote_auth_bridge_error)
}

fn validate_remote_authenticate_request(
    request: &RemoteAuthenticateRequest,
) -> Result<(), (StatusCode, Json<AuthRouteError>)> {
    for (field, value) in [
        ("username", request.username.as_str()),
        ("password", request.password.as_str()),
        ("object_store", request.object_store.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(route_error(
                StatusCode::BAD_REQUEST,
                "invalid_remote_authenticate_request",
                format!("{field} must not be blank"),
            ));
        }
    }
    if request
        .requested_session_lifetime_seconds
        .is_some_and(|seconds| !(60..=86_400).contains(&seconds))
    {
        return Err(route_error(
            StatusCode::BAD_REQUEST,
            "invalid_remote_authenticate_request",
            "requested session lifetime must be between 60 and 86400 seconds",
        ));
    }
    Ok(())
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

pub(super) async fn logout(
    State(state): State<StandaloneAuthRouteState>,
    Json(request): Json<LogoutRequest>,
) -> Result<Json<LogoutResponse>, (StatusCode, Json<AuthRouteError>)> {
    state
        .auth_store
        .logout(&request.username, &request.session_token)
        .map(Json)
        .map_err(auth_route_error)
}

pub(super) async fn session(
    State(state): State<StandaloneAuthRouteState>,
    Json(request): Json<SessionCheckRequest>,
) -> Result<Json<SessionCheckResponse>, (StatusCode, Json<AuthRouteError>)> {
    state
        .auth_store
        .verify_session(&request.username, &request.session_token)
        .map(Json)
        .map_err(auth_route_error)
}

pub(super) async fn easyconnect_auth_context(
    actor: AuthenticatedGuiActor,
) -> Result<Json<StandaloneEasyconnectAuthContextResponse>, (StatusCode, Json<AuthRouteError>)> {
    if !actor.authority.uses_local_os_policy() {
        return Err(route_error(
            StatusCode::FORBIDDEN,
            "local_os_policy_identity_required",
            "easyconnect standalone authentication requires an appliance-local or Monas-authenticated OS identity",
        ));
    }

    Ok(Json(StandaloneEasyconnectAuthContextResponse {
        schema_version: "dasobjectstore.remote_easyconnect.auth_context.v1".to_string(),
        auth_provider: RemoteEasyconnectAuthProvider::StandaloneLocalUser,
        subject_id: actor.subject_id,
        session_expires_at_unix_seconds: actor.expires_at_unix_seconds,
        supported_auth_providers: vec![RemoteEasyconnectAuthProvider::StandaloneLocalUser],
        future_auth_providers: vec![
            RemoteEasyconnectAuthProvider::Synoptikon,
            RemoteEasyconnectAuthProvider::Mneion,
        ],
    }))
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
                DaemonRuntimeConfig::default_packaged().socket_path,
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
                DaemonRuntimeConfig::default_packaged().socket_path,
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
    let s3_endpoint = state.s3_endpoint.as_ref();
    verify_easyconnect_s3_endpoint(s3_endpoint).await?;
    let s3_descriptor = s3_endpoint
        .expect("verified endpoint configuration is present")
        .descriptor
        .clone();
    let exchange = crate::daemon_bridge::DaemonBridge::shared_packaged()
        .call_message(move || {
            DaemonClient::new(UnixSocketDaemonTransport::for_bounded_bridge(
                DaemonRuntimeConfig::default_packaged().socket_path,
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

fn local_password_auth_route_error(
    err: LocalPasswordAuthError,
) -> (StatusCode, Json<AuthRouteError>) {
    match err {
        LocalPasswordAuthError::UsernameRequired | LocalPasswordAuthError::PasswordRequired => {
            route_error(StatusCode::BAD_REQUEST, "invalid_request", err.to_string())
        }
        LocalPasswordAuthError::InvalidCredentials => route_error(
            StatusCode::UNAUTHORIZED,
            "invalid_credentials",
            err.to_string(),
        ),
        LocalPasswordAuthError::BackendUnavailable { .. } => route_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "local_auth_unavailable",
            err.to_string(),
        ),
    }
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
