//! Standalone local authentication and EasyConnect route handlers.

use super::*;

#[derive(Clone)]
pub(crate) struct StandaloneAuthRouteState {
    pub(super) auth_store: LocalAuthStore,
    pub(super) local_password_authenticator: Arc<dyn LocalPasswordAuthenticator>,
}

#[derive(Clone)]
pub(crate) struct StandaloneEasyconnectRouteState {
    pub(super) auth_store: LocalAuthStore,
    pub(super) public_base_url: String,
}

impl StandaloneAuthRouteState {
    pub(super) fn system(auth_store: LocalAuthStore) -> Self {
        Self {
            auth_store,
            local_password_authenticator: Arc::new(SystemLocalPasswordAuthenticator::default()),
        }
    }
}

impl StandaloneEasyconnectRouteState {
    pub(super) fn system(auth_store: LocalAuthStore) -> Self {
        Self {
            auth_store,
            public_base_url: crate::DEFAULT_STANDALONE_PUBLIC_BASE_URL.to_string(),
        }
    }
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

/// Exchange a registered application's signed proof for a short-lived access
/// token through the daemon authority. The proof is the request credential;
/// this route deliberately does not accept a local GUI session token.
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

    let grant = RemoteEasyconnectObjectStoreGrant {
        object_store: store.store_id.clone(),
        bucket: store.bucket.clone(),
        can_read: true,
        can_write: true,
        writer_group: store.writer_group.clone(),
        object_type: store.object_type.clone(),
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
                    approved_actor: current_user.username,
                    auth_provider: RemoteEasyconnectAuthProvider::StandaloneLocalUser,
                    allowed_object_stores: vec![grant],
                    approval_expires_at_utc: created.expires_at_utc,
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

    Ok(Json(RemoteAuthenticateResponse {
        schema_version: "dasobjectstore.remote_authenticate.v1".to_string(),
        endpoint_port: 3900,
        region: "garage".to_string(),
        addressing_style: "path".to_string(),
        object_store: request.object_store,
        bucket: store.bucket.clone(),
        session,
    }))
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

pub(super) async fn easyconnect_discovery(
    State(state): State<StandaloneEasyconnectRouteState>,
) -> Json<RemoteEasyconnectDiscoveryResponse> {
    Json(standalone_easyconnect_discovery_payload(
        &state.public_base_url,
    ))
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

pub(super) fn standalone_easyconnect_discovery_payload(
    public_base_url: &str,
) -> RemoteEasyconnectDiscoveryResponse {
    let api_base_url = format!(
        "{}/products/dasobjectstore/api",
        public_base_url.trim_end_matches('/')
    );

    RemoteEasyconnectDiscoveryResponse {
        appliance_id: "standalone-dasobjectstore".to_string(),
        product_id: "dasobjectstore".to_string(),
        display_name: "DASObjectStore standalone appliance".to_string(),
        pairing_create_url: format!("{api_base_url}/v1/remote/easyconnect/pairings"),
        pairing_exchange_url: format!("{api_base_url}/v1/remote/easyconnect/pairings/exchange"),
        session_revoke_url_template: format!(
            "{api_base_url}/v1/remote/easyconnect/sessions/{{session_id}}"
        ),
        session_renew_url_template: format!(
            "{api_base_url}/v1/remote/easyconnect/sessions/{{session_id}}/renew"
        ),
        default_session_lifetime_seconds:
            dasobjectstore_daemon::REMOTE_EASYCONNECT_DEFAULT_SESSION_LIFETIME_SECONDS,
        session_policy: RemoteEasyconnectSessionPolicy::default(),
        auth_providers: vec![RemoteEasyconnectAuthProvider::StandaloneLocalUser],
    }
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
