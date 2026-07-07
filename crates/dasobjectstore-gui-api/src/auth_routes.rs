use crate::{
    discover_current_local_user, AuthenticatedGuiActor, DashboardWarning, LocalAuthStore,
    LocalAuthStoreError, LoginResponse, LogoutResponse, RegisterResponse, SessionCheckResponse,
    UsersGroupsWorkspaceView,
};
use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Extension, Json, Router,
};
use dasobjectstore_daemon::runtime::LOCAL_ADMIN_CONFIRMATION_MARKER;
use dasobjectstore_daemon::{
    AssignLocalUserToLocalGroupRequest as DaemonAssignLocalUserToLocalGroupRequest,
    AssignLocalUserToLocalGroupResponse as DaemonAssignLocalUserToLocalGroupResponse,
    CreateLocalGroupRequest as DaemonCreateLocalGroupRequest,
    CreateLocalGroupResponse as DaemonCreateLocalGroupResponse, DaemonClient,
    DaemonLocalAdminCommand, DaemonRuntimeConfig, UnixSocketDaemonTransport,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuiApiHostMode {
    Standalone,
    SynoptikonIntegrated,
}

pub fn standalone_gui_api_router(auth_store: LocalAuthStore) -> Router {
    gui_api_router_for_host_mode(GuiApiHostMode::Standalone, auth_store)
}

pub fn gui_api_router_for_host_mode(
    host_mode: GuiApiHostMode,
    auth_store: LocalAuthStore,
) -> Router {
    match host_mode {
        GuiApiHostMode::Standalone => crate::gui_api_router()
            .merge(standalone_auth_router(auth_store.clone()))
            .merge(standalone_users_groups_router(auth_store)),
        GuiApiHostMode::SynoptikonIntegrated => crate::gui_api_router(),
    }
}

pub fn standalone_auth_router(auth_store: LocalAuthStore) -> Router {
    Router::new()
        .route("/api/register", post(register))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/session", post(session))
        .with_state(auth_store)
}

pub fn standalone_users_groups_router(auth_store: LocalAuthStore) -> Router {
    standalone_users_groups_router_with_state(StandaloneUsersGroupsRouteState::system(auth_store))
}

fn standalone_users_groups_router_with_state(state: StandaloneUsersGroupsRouteState) -> Router {
    Router::new()
        .route(
            "/api/v1/workspaces/users-groups",
            get(users_groups_workspace),
        )
        .route(
            "/api/v1/workspaces/users-groups/local-groups",
            post(create_local_group),
        )
        .route(
            "/api/v1/workspaces/users-groups/local-groups/members",
            post(assign_local_user_to_group),
        )
        .layer(Extension(state.auth_store.clone()))
        .with_state(state)
}

#[derive(Clone)]
struct StandaloneUsersGroupsRouteState {
    auth_store: LocalAuthStore,
    local_user_provider: Arc<dyn LocalUserAuthorityProvider>,
    local_group_admin_client: Option<Arc<dyn StandaloneLocalGroupAdminClient>>,
}

impl StandaloneUsersGroupsRouteState {
    fn system(auth_store: LocalAuthStore) -> Self {
        Self {
            auth_store,
            local_user_provider: Arc::new(SystemLocalUserAuthorityProvider),
            local_group_admin_client: Some(Arc::new(
                DaemonStandaloneLocalGroupAdminClient::default_packaged(),
            )),
        }
    }
}

trait LocalUserAuthorityProvider: Send + Sync {
    fn current_local_user(
        &self,
    ) -> Result<crate::LocalUserMetadata, crate::LocalUserDiscoveryError>;
}

struct SystemLocalUserAuthorityProvider;

impl LocalUserAuthorityProvider for SystemLocalUserAuthorityProvider {
    fn current_local_user(
        &self,
    ) -> Result<crate::LocalUserMetadata, crate::LocalUserDiscoveryError> {
        discover_current_local_user()
    }
}

trait StandaloneLocalGroupAdminClient: Send + Sync {
    fn submit_local_group_operation(
        &self,
        request: StandaloneLocalGroupAdminDaemonRequest,
    ) -> Result<StandaloneLocalGroupAdminResponse, StandaloneLocalGroupAdminClientError>;
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RegisterRequest {
    pub username: String,
    pub token: String,
    pub password: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    pub session_ttl_seconds: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LogoutRequest {
    pub username: String,
    pub session_token: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionCheckRequest {
    pub username: String,
    pub session_token: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthRouteError {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CreateLocalGroupRequest {
    #[serde(alias = "group")]
    pub group_name: String,
    #[serde(default)]
    pub dry_run: bool,
    pub confirmation_marker: Option<String>,
    pub client_request_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AssignLocalUserToGroupRequest {
    #[serde(alias = "group")]
    pub group_name: String,
    #[serde(alias = "user")]
    pub username: String,
    #[serde(default)]
    pub dry_run: bool,
    pub confirmation_marker: Option<String>,
    pub client_request_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StandaloneLocalGroupOperation {
    CreateGroup,
    AddUserToGroup,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StandaloneLocalGroupAdminResponse {
    pub accepted: StandaloneLocalGroupAdminAcceptedResponse,
    pub operation: StandaloneLocalGroupOperation,
    pub group_name: String,
    pub username: Option<String>,
    pub client_request_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StandaloneLocalGroupAdminAcceptedResponse {
    pub job_id: String,
    pub kind: String,
    pub accepted_at_utc: String,
    pub dry_run: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StandaloneLocalGroupAdminDaemonRequest {
    operation: StandaloneLocalGroupOperation,
    group_name: String,
    username: Option<String>,
    dry_run: bool,
    client_request_id: Option<String>,
    administrator_actor: Option<String>,
    confirmation_marker: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StandaloneLocalGroupAdminClientError {
    message: String,
}

struct DaemonStandaloneLocalGroupAdminClient {
    client: DaemonClient<UnixSocketDaemonTransport>,
}

impl DaemonStandaloneLocalGroupAdminClient {
    fn default_packaged() -> Self {
        Self {
            client: DaemonClient::new(UnixSocketDaemonTransport::new(
                DaemonRuntimeConfig::default_packaged().socket_path,
            )),
        }
    }
}

impl StandaloneLocalGroupAdminClient for DaemonStandaloneLocalGroupAdminClient {
    fn submit_local_group_operation(
        &self,
        request: StandaloneLocalGroupAdminDaemonRequest,
    ) -> Result<StandaloneLocalGroupAdminResponse, StandaloneLocalGroupAdminClientError> {
        match request.operation {
            StandaloneLocalGroupOperation::CreateGroup => self
                .client
                .create_local_group(DaemonCreateLocalGroupRequest {
                    group_name: request.group_name,
                    dry_run: request.dry_run,
                    client_request_id: request.client_request_id,
                    administrator_actor: request.administrator_actor,
                    confirmation_marker: request.confirmation_marker,
                })
                .map(create_local_group_response_from_daemon)
                .map_err(standalone_admin_client_error),
            StandaloneLocalGroupOperation::AddUserToGroup => self
                .client
                .assign_local_user_to_local_group(DaemonAssignLocalUserToLocalGroupRequest {
                    username: request.username.ok_or_else(|| {
                        StandaloneLocalGroupAdminClientError {
                            message: "username is required".to_string(),
                        }
                    })?,
                    group_name: request.group_name,
                    dry_run: request.dry_run,
                    client_request_id: request.client_request_id,
                    administrator_actor: request.administrator_actor,
                    confirmation_marker: request.confirmation_marker,
                })
                .map(assign_local_user_to_group_response_from_daemon)
                .map_err(standalone_admin_client_error),
        }
    }
}

async fn register(
    State(auth_store): State<LocalAuthStore>,
    Json(request): Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>, (StatusCode, Json<AuthRouteError>)> {
    auth_store
        .register_with_token(&request.username, &request.token, &request.password)
        .map(Json)
        .map_err(auth_route_error)
}

async fn login(
    State(auth_store): State<LocalAuthStore>,
    Json(request): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, (StatusCode, Json<AuthRouteError>)> {
    auth_store
        .login_with_session_ttl_seconds(
            &request.username,
            &request.password,
            request.session_ttl_seconds,
        )
        .map(Json)
        .map_err(auth_route_error)
}

async fn logout(
    State(auth_store): State<LocalAuthStore>,
    Json(request): Json<LogoutRequest>,
) -> Result<Json<LogoutResponse>, (StatusCode, Json<AuthRouteError>)> {
    auth_store
        .logout(&request.username, &request.session_token)
        .map(Json)
        .map_err(auth_route_error)
}

async fn session(
    State(auth_store): State<LocalAuthStore>,
    Json(request): Json<SessionCheckRequest>,
) -> Result<Json<SessionCheckResponse>, (StatusCode, Json<AuthRouteError>)> {
    auth_store
        .verify_session(&request.username, &request.session_token)
        .map(Json)
        .map_err(auth_route_error)
}

async fn users_groups_workspace(
    State(state): State<StandaloneUsersGroupsRouteState>,
    _actor: AuthenticatedGuiActor,
) -> Result<Json<UsersGroupsWorkspaceView>, (StatusCode, Json<AuthRouteError>)> {
    let users = state.auth_store.list_users().map_err(auth_route_error)?;
    let (current_user, warnings) = match state.local_user_provider.current_local_user() {
        Ok(user) => (Some(user), Vec::new()),
        Err(err) => (
            None,
            vec![DashboardWarning {
                code: "local_user_discovery_failed".to_string(),
                message: err.to_string(),
            }],
        ),
    };

    Ok(Json(UsersGroupsWorkspaceView::standalone(
        current_user,
        users,
        warnings,
    )))
}

async fn create_local_group(
    State(state): State<StandaloneUsersGroupsRouteState>,
    actor: AuthenticatedGuiActor,
    Json(request): Json<CreateLocalGroupRequest>,
) -> Result<Json<StandaloneLocalGroupAdminResponse>, (StatusCode, Json<AuthRouteError>)> {
    let mut request = validate_create_local_group_request(request)?;
    let current_user = require_local_administrator(&state, &actor)?;
    request.administrator_actor = Some(current_user.username);
    submit_local_group_admin_request(&state, request).map(Json)
}

async fn assign_local_user_to_group(
    State(state): State<StandaloneUsersGroupsRouteState>,
    actor: AuthenticatedGuiActor,
    Json(request): Json<AssignLocalUserToGroupRequest>,
) -> Result<Json<StandaloneLocalGroupAdminResponse>, (StatusCode, Json<AuthRouteError>)> {
    let mut request = validate_assign_local_user_to_group_request(request)?;
    let current_user = require_local_administrator(&state, &actor)?;
    request.administrator_actor = Some(current_user.username);
    submit_local_group_admin_request(&state, request).map(Json)
}

fn validate_create_local_group_request(
    request: CreateLocalGroupRequest,
) -> Result<StandaloneLocalGroupAdminDaemonRequest, (StatusCode, Json<AuthRouteError>)> {
    let group_name = required_field("group_name", request.group_name)?;
    validate_client_request_id(request.client_request_id.as_deref())?;
    let confirmation_marker =
        validate_confirmation_marker(request.dry_run, request.confirmation_marker.as_deref())?;

    Ok(StandaloneLocalGroupAdminDaemonRequest {
        operation: StandaloneLocalGroupOperation::CreateGroup,
        group_name,
        username: None,
        dry_run: request.dry_run,
        client_request_id: request.client_request_id,
        administrator_actor: None,
        confirmation_marker,
    })
}

fn validate_assign_local_user_to_group_request(
    request: AssignLocalUserToGroupRequest,
) -> Result<StandaloneLocalGroupAdminDaemonRequest, (StatusCode, Json<AuthRouteError>)> {
    let group_name = required_field("group_name", request.group_name)?;
    let username = required_field("username", request.username)?;
    validate_client_request_id(request.client_request_id.as_deref())?;
    let confirmation_marker =
        validate_confirmation_marker(request.dry_run, request.confirmation_marker.as_deref())?;

    Ok(StandaloneLocalGroupAdminDaemonRequest {
        operation: StandaloneLocalGroupOperation::AddUserToGroup,
        group_name,
        username: Some(username),
        dry_run: request.dry_run,
        client_request_id: request.client_request_id,
        administrator_actor: None,
        confirmation_marker,
    })
}

fn require_local_administrator(
    state: &StandaloneUsersGroupsRouteState,
    actor: &AuthenticatedGuiActor,
) -> Result<crate::LocalUserMetadata, (StatusCode, Json<AuthRouteError>)> {
    if actor.authority != crate::AuthenticatedActorAuthority::LocalStandalone {
        return Err(route_error(
            StatusCode::FORBIDDEN,
            "standalone_local_session_required",
            "standalone local group administration requires a local session",
        ));
    }

    let current_user = state
        .local_user_provider
        .current_local_user()
        .map_err(|err| {
            route_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "local_user_discovery_failed",
                err.to_string(),
            )
        })?;

    if !current_user.sudo_administrator {
        return Err(route_error(
            StatusCode::FORBIDDEN,
            "standalone_admin_authority_missing",
            "current OS user must be a sudo-derived DASObjectStore administrator",
        ));
    }

    Ok(current_user)
}

fn submit_local_group_admin_request(
    state: &StandaloneUsersGroupsRouteState,
    request: StandaloneLocalGroupAdminDaemonRequest,
) -> Result<StandaloneLocalGroupAdminResponse, (StatusCode, Json<AuthRouteError>)> {
    let client = state.local_group_admin_client.as_ref().ok_or_else(|| {
        route_error(
            StatusCode::NOT_IMPLEMENTED,
            "daemon_local_group_admin_unavailable",
            "daemon local group administration contract is not available",
        )
    })?;

    client
        .submit_local_group_operation(request)
        .map_err(|err| route_error(StatusCode::BAD_GATEWAY, "daemon_client_error", err.message))
}

fn required_field(
    field: &'static str,
    value: String,
) -> Result<String, (StatusCode, Json<AuthRouteError>)> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(route_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            format!("{field} must not be blank"),
        ));
    }
    Ok(value)
}

fn validate_client_request_id(
    client_request_id: Option<&str>,
) -> Result<(), (StatusCode, Json<AuthRouteError>)> {
    if client_request_id.is_some_and(|value| value.trim().is_empty()) {
        return Err(route_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "client_request_id must not be blank",
        ));
    }
    Ok(())
}

fn validate_confirmation_marker(
    dry_run: bool,
    confirmation_marker: Option<&str>,
) -> Result<String, (StatusCode, Json<AuthRouteError>)> {
    let confirmation_marker = confirmation_marker
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if dry_run {
        return Ok(confirmation_marker
            .unwrap_or(LOCAL_ADMIN_CONFIRMATION_MARKER)
            .to_string());
    }

    if confirmation_marker == Some(LOCAL_ADMIN_CONFIRMATION_MARKER) {
        return Ok(LOCAL_ADMIN_CONFIRMATION_MARKER.to_string());
    }

    Err(route_error(
        StatusCode::BAD_REQUEST,
        "confirmation_required",
        format!("confirmation_marker must be `{LOCAL_ADMIN_CONFIRMATION_MARKER}`"),
    ))
}

fn create_local_group_response_from_daemon(
    response: DaemonCreateLocalGroupResponse,
) -> StandaloneLocalGroupAdminResponse {
    let client_request_id = response.accepted.client_request_id.clone();
    StandaloneLocalGroupAdminResponse {
        accepted: standalone_accepted_response_from_daemon(response.accepted),
        operation: StandaloneLocalGroupOperation::CreateGroup,
        group_name: response.group_name,
        username: None,
        client_request_id,
    }
}

fn assign_local_user_to_group_response_from_daemon(
    response: DaemonAssignLocalUserToLocalGroupResponse,
) -> StandaloneLocalGroupAdminResponse {
    let client_request_id = response.accepted.client_request_id.clone();
    StandaloneLocalGroupAdminResponse {
        accepted: standalone_accepted_response_from_daemon(response.accepted),
        operation: StandaloneLocalGroupOperation::AddUserToGroup,
        group_name: response.group_name,
        username: Some(response.username),
        client_request_id,
    }
}

fn standalone_accepted_response_from_daemon(
    accepted: dasobjectstore_daemon::DaemonLocalAdminAcceptedResponse,
) -> StandaloneLocalGroupAdminAcceptedResponse {
    StandaloneLocalGroupAdminAcceptedResponse {
        job_id: accepted.job_id.to_string(),
        kind: standalone_accepted_kind(accepted.command).to_string(),
        accepted_at_utc: accepted.accepted_at_utc,
        dry_run: accepted.dry_run,
    }
}

fn standalone_accepted_kind(command: DaemonLocalAdminCommand) -> &'static str {
    match command {
        DaemonLocalAdminCommand::CreateLocalGroup
        | DaemonLocalAdminCommand::AssignLocalUserToLocalGroup => "system_administration",
    }
}

fn standalone_admin_client_error(
    err: dasobjectstore_daemon::DaemonClientError,
) -> StandaloneLocalGroupAdminClientError {
    StandaloneLocalGroupAdminClientError {
        message: err.to_string(),
    }
}

fn auth_route_error(err: LocalAuthStoreError) -> (StatusCode, Json<AuthRouteError>) {
    let status = match err {
        LocalAuthStoreError::UserNameRequired | LocalAuthStoreError::PasswordRequired => {
            StatusCode::BAD_REQUEST
        }
        LocalAuthStoreError::UserAlreadyExists { .. }
        | LocalAuthStoreError::UserAlreadyRegistered { .. } => StatusCode::CONFLICT,
        LocalAuthStoreError::UserNotFound { .. }
        | LocalAuthStoreError::UserNotRegistered { .. }
        | LocalAuthStoreError::InvalidRegistrationToken
        | LocalAuthStoreError::UsedRegistrationToken
        | LocalAuthStoreError::ExpiredRegistrationToken
        | LocalAuthStoreError::InvalidSessionToken
        | LocalAuthStoreError::ExpiredSessionToken
        | LocalAuthStoreError::InvalidPassword => StatusCode::UNAUTHORIZED,
        LocalAuthStoreError::Io { .. }
        | LocalAuthStoreError::Json(_)
        | LocalAuthStoreError::PasswordHash => StatusCode::INTERNAL_SERVER_ERROR,
    };

    (
        status,
        Json(AuthRouteError {
            code: status.as_u16().to_string(),
            message: err.to_string(),
        }),
    )
}

fn route_error(
    status: StatusCode,
    code: impl Into<String>,
    message: impl Into<String>,
) -> (StatusCode, Json<AuthRouteError>) {
    (
        status,
        Json(AuthRouteError {
            code: code.into(),
            message: message.into(),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        gui_api_router_for_host_mode, standalone_auth_router,
        standalone_users_groups_router_with_state, AssignLocalUserToGroupRequest,
        CreateLocalGroupRequest, GuiApiHostMode, LocalUserAuthorityProvider, LoginRequest,
        LogoutRequest, RegisterRequest, SessionCheckRequest,
        StandaloneLocalGroupAdminAcceptedResponse, StandaloneLocalGroupAdminClient,
        StandaloneLocalGroupAdminClientError, StandaloneLocalGroupAdminDaemonRequest,
        StandaloneLocalGroupAdminResponse, StandaloneLocalGroupOperation,
        StandaloneUsersGroupsRouteState, LOCAL_ADMIN_CONFIRMATION_MARKER,
    };
    use crate::{
        LocalAuthStore, LocalUserDiscoveryError, LocalUserMetadata, LoginResponse,
        STANDALONE_SESSION_TOKEN_HEADER, STANDALONE_USERNAME_HEADER,
    };
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use serde::de::DeserializeOwned;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};
    use tower::ServiceExt;

    #[tokio::test]
    async fn standalone_auth_routes_complete_login_session_logout_flow() {
        let root = temp_root("flow");
        let auth_store = LocalAuthStore::new(&root);
        auth_store.create_user("admin").expect("user created");
        let registration_token = auth_store
            .issue_registration_token("admin", Some(3_600))
            .expect("registration token issued");
        let app = standalone_auth_router(auth_store);

        let register = post_json::<crate::RegisterResponse>(
            app.clone(),
            "/api/register",
            &RegisterRequest {
                username: "admin".to_string(),
                token: registration_token,
                password: "secret".to_string(),
            },
        )
        .await;
        assert_eq!(register.username, "admin");

        let login = post_json::<LoginResponse>(
            app.clone(),
            "/api/login",
            &LoginRequest {
                username: "admin".to_string(),
                password: "secret".to_string(),
                session_ttl_seconds: Some(3_600),
            },
        )
        .await;

        let session = post_json::<crate::SessionCheckResponse>(
            app.clone(),
            "/api/session",
            &SessionCheckRequest {
                username: "admin".to_string(),
                session_token: login.session_token.clone(),
            },
        )
        .await;
        assert!(session.valid);

        let logout = post_json::<crate::LogoutResponse>(
            app.clone(),
            "/api/logout",
            &LogoutRequest {
                username: "admin".to_string(),
                session_token: login.session_token,
            },
        )
        .await;
        assert!(logout.disconnected);

        cleanup(&root);
    }

    #[tokio::test]
    async fn login_route_rejects_invalid_password() {
        let root = temp_root("invalid-password-route");
        let auth_store = LocalAuthStore::new(&root);
        auth_store.create_user("admin").expect("user created");
        let token = auth_store
            .issue_registration_token("admin", Some(3_600))
            .expect("token issued");
        auth_store
            .register_with_token("admin", &token, "secret")
            .expect("registered");
        let app = standalone_auth_router(auth_store);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/login")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&LoginRequest {
                            username: "admin".to_string(),
                            password: "wrong".to_string(),
                            session_ttl_seconds: None,
                        })
                        .expect("request encodes"),
                    ))
                    .expect("request builds"),
            )
            .await
            .expect("request completes");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        cleanup(&root);
    }

    #[tokio::test]
    async fn session_route_rejects_expired_session() {
        let root = temp_root("expired-session-route");
        let auth_store = registered_auth_store(&root);
        let app = standalone_auth_router(auth_store.clone());
        let login = post_json::<LoginResponse>(
            app.clone(),
            "/api/login",
            &LoginRequest {
                username: "admin".to_string(),
                password: "secret".to_string(),
                session_ttl_seconds: Some(3_600),
            },
        )
        .await;
        expire_user_sessions(&auth_store, "admin");

        let response = post_json_response(
            app,
            "/api/session",
            &SessionCheckRequest {
                username: "admin".to_string(),
                session_token: login.session_token,
            },
        )
        .await;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        cleanup(&root);
    }

    #[tokio::test]
    async fn session_route_rejects_logged_out_session() {
        let root = temp_root("logged-out-session-route");
        let auth_store = registered_auth_store(&root);
        let app = standalone_auth_router(auth_store);
        let login = post_json::<LoginResponse>(
            app.clone(),
            "/api/login",
            &LoginRequest {
                username: "admin".to_string(),
                password: "secret".to_string(),
                session_ttl_seconds: Some(3_600),
            },
        )
        .await;

        let logout = post_json::<crate::LogoutResponse>(
            app.clone(),
            "/api/logout",
            &LogoutRequest {
                username: "admin".to_string(),
                session_token: login.session_token.clone(),
            },
        )
        .await;
        assert!(logout.disconnected);

        let response = post_json_response(
            app,
            "/api/session",
            &SessionCheckRequest {
                username: "admin".to_string(),
                session_token: login.session_token,
            },
        )
        .await;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        cleanup(&root);
    }

    #[tokio::test]
    async fn standalone_host_mode_mounts_local_auth_routes() {
        let root = temp_root("standalone-host-mode");
        let auth_store = LocalAuthStore::new(&root);
        let app = gui_api_router_for_host_mode(GuiApiHostMode::Standalone, auth_store);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/login")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("request completes");

        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);

        cleanup(&root);
    }

    #[tokio::test]
    async fn standalone_users_groups_workspace_requires_session() {
        let root = temp_root("standalone-users-groups-auth");
        let auth_store = registered_auth_store(&root);
        let app = gui_api_router_for_host_mode(GuiApiHostMode::Standalone, auth_store);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/workspaces/users-groups")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("request completes");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        cleanup(&root);
    }

    #[tokio::test]
    async fn standalone_users_groups_workspace_rejects_invalid_session() {
        let root = temp_root("standalone-users-groups-invalid-session");
        let auth_store = registered_auth_store(&root);
        let app = gui_api_router_for_host_mode(GuiApiHostMode::Standalone, auth_store);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/workspaces/users-groups")
                    .header(STANDALONE_USERNAME_HEADER, "admin")
                    .header(STANDALONE_SESSION_TOKEN_HEADER, "invalid")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("request completes");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        cleanup(&root);
    }

    #[tokio::test]
    async fn standalone_users_groups_workspace_returns_authority_payload() {
        let root = temp_root("standalone-users-groups");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let app = gui_api_router_for_host_mode(GuiApiHostMode::Standalone, auth_store);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/workspaces/users-groups")
                    .header(STANDALONE_USERNAME_HEADER, "admin")
                    .header(STANDALONE_SESSION_TOKEN_HEADER, login.session_token)
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("request completes");

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("body bytes");
        let encoded: serde_json::Value = serde_json::from_slice(&bytes).expect("response decodes");

        assert_eq!(encoded["host_mode"], "standalone");
        assert_eq!(encoded["users"][0]["username"], "admin");
        assert_eq!(encoded["users"][0]["registered"], true);
        assert_eq!(
            encoded["capabilities"]["product_local_user_registration"],
            true
        );
        assert_eq!(encoded["operations"][0]["kind"], "create_local_group");
        assert_eq!(
            encoded["operations"][1]["kind"],
            "assign_local_user_to_group"
        );
        assert!(encoded["current_user"].is_object() || encoded["warnings"].is_array());

        cleanup(&root);
    }

    #[tokio::test]
    async fn create_local_group_requires_session() {
        let root = temp_root("create-local-group-auth");
        let auth_store = registered_auth_store(&root);
        let app = standalone_users_groups_router_with_state(test_users_groups_state(
            auth_store,
            local_user("operator", vec!["sudo"]),
            Some(recording_admin_client()),
        ));

        let response = post_json_response(
            app,
            "/api/v1/workspaces/users-groups/local-groups",
            &CreateLocalGroupRequest {
                group_name: "mnemosyne-writers".to_string(),
                dry_run: true,
                confirmation_marker: None,
                client_request_id: Some("request-1".to_string()),
            },
        )
        .await;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        cleanup(&root);
    }

    #[tokio::test]
    async fn create_local_group_rejects_non_admin_os_user() {
        let root = temp_root("create-local-group-non-admin");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let app = standalone_users_groups_router_with_state(test_users_groups_state(
            auth_store,
            local_user("operator", vec!["users"]),
            Some(recording_admin_client()),
        ));

        let response = post_json_response_with_session(
            app,
            "/api/v1/workspaces/users-groups/local-groups",
            "admin",
            &login.session_token,
            &CreateLocalGroupRequest {
                group_name: "mnemosyne-writers".to_string(),
                dry_run: true,
                confirmation_marker: None,
                client_request_id: Some("request-1".to_string()),
            },
        )
        .await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        cleanup(&root);
    }

    #[tokio::test]
    async fn create_local_group_forwards_dry_run_to_admin_client() {
        let root = temp_root("create-local-group-forward");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let client = recording_admin_client();
        let app = standalone_users_groups_router_with_state(test_users_groups_state(
            auth_store,
            local_user("operator", vec!["sudo"]),
            Some(client.clone()),
        ));

        let response = post_json_with_session::<StandaloneLocalGroupAdminResponse>(
            app,
            "/api/v1/workspaces/users-groups/local-groups",
            "admin",
            &login.session_token,
            &CreateLocalGroupRequest {
                group_name: " mnemosyne-writers ".to_string(),
                dry_run: true,
                confirmation_marker: None,
                client_request_id: Some("request-1".to_string()),
            },
        )
        .await;

        assert_eq!(
            response.operation,
            StandaloneLocalGroupOperation::CreateGroup
        );
        assert_eq!(response.group_name, "mnemosyne-writers");
        assert!(response.accepted.dry_run);
        assert_eq!(
            client.requests(),
            vec![StandaloneLocalGroupAdminDaemonRequest {
                operation: StandaloneLocalGroupOperation::CreateGroup,
                group_name: "mnemosyne-writers".to_string(),
                username: None,
                dry_run: true,
                client_request_id: Some("request-1".to_string()),
                administrator_actor: Some("operator".to_string()),
                confirmation_marker: LOCAL_ADMIN_CONFIRMATION_MARKER.to_string(),
            }]
        );

        cleanup(&root);
    }

    #[tokio::test]
    async fn assign_local_user_to_group_forwards_dry_run_to_admin_client() {
        let root = temp_root("assign-local-user-forward");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let client = recording_admin_client();
        let app = standalone_users_groups_router_with_state(test_users_groups_state(
            auth_store,
            local_user("operator", vec!["wheel"]),
            Some(client.clone()),
        ));

        let response = post_json_with_session::<StandaloneLocalGroupAdminResponse>(
            app,
            "/api/v1/workspaces/users-groups/local-groups/members",
            "admin",
            &login.session_token,
            &AssignLocalUserToGroupRequest {
                group_name: "mnemosyne-writers".to_string(),
                username: "stephen".to_string(),
                dry_run: true,
                confirmation_marker: None,
                client_request_id: Some("request-2".to_string()),
            },
        )
        .await;

        assert_eq!(
            response.operation,
            StandaloneLocalGroupOperation::AddUserToGroup
        );
        assert_eq!(response.username.as_deref(), Some("stephen"));
        assert!(response.accepted.dry_run);
        assert_eq!(
            client.requests(),
            vec![StandaloneLocalGroupAdminDaemonRequest {
                operation: StandaloneLocalGroupOperation::AddUserToGroup,
                group_name: "mnemosyne-writers".to_string(),
                username: Some("stephen".to_string()),
                dry_run: true,
                client_request_id: Some("request-2".to_string()),
                administrator_actor: Some("operator".to_string()),
                confirmation_marker: LOCAL_ADMIN_CONFIRMATION_MARKER.to_string(),
            }]
        );

        cleanup(&root);
    }

    #[tokio::test]
    async fn synoptikon_integrated_host_mode_omits_local_auth_routes() {
        let root = temp_root("integrated-host-mode");
        let auth_store = LocalAuthStore::new(&root);
        let app = gui_api_router_for_host_mode(GuiApiHostMode::SynoptikonIntegrated, auth_store);

        for path in ["/api/register", "/api/login", "/api/logout", "/api/session"] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(path)
                        .body(Body::empty())
                        .expect("request builds"),
                )
                .await
                .expect("request completes");

            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        }

        cleanup(&root);
    }

    #[tokio::test]
    async fn synoptikon_integrated_host_mode_omits_users_groups_workspace() {
        let root = temp_root("integrated-users-groups");
        let auth_store = LocalAuthStore::new(&root);
        let app = gui_api_router_for_host_mode(GuiApiHostMode::SynoptikonIntegrated, auth_store);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/workspaces/users-groups")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("request completes");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        cleanup(&root);
    }

    #[tokio::test]
    async fn synoptikon_integrated_host_mode_keeps_base_api_routes() {
        let root = temp_root("integrated-base-api");
        let auth_store = LocalAuthStore::new(&root);
        let app = gui_api_router_for_host_mode(GuiApiHostMode::SynoptikonIntegrated, auth_store);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/health")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("request completes");

        assert_eq!(response.status(), StatusCode::OK);

        cleanup(&root);
    }

    async fn post_json<T>(app: axum::Router, path: &str, body: &impl serde::Serialize) -> T
    where
        T: DeserializeOwned,
    {
        let response = post_json_response(app, path, body).await;
        assert_eq!(response.status(), StatusCode::OK);

        let bytes = to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("body bytes");
        serde_json::from_slice(&bytes).expect("response decodes")
    }

    async fn post_json_with_session<T>(
        app: axum::Router,
        path: &str,
        username: &str,
        session_token: &str,
        body: &impl serde::Serialize,
    ) -> T
    where
        T: DeserializeOwned,
    {
        let response =
            post_json_response_with_session(app, path, username, session_token, body).await;
        assert_eq!(response.status(), StatusCode::OK);

        let bytes = to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("body bytes");
        serde_json::from_slice(&bytes).expect("response decodes")
    }

    async fn post_json_response(
        app: axum::Router,
        path: &str,
        body: &impl serde::Serialize,
    ) -> axum::response::Response {
        app.oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(body).expect("request encodes"),
                ))
                .expect("request builds"),
        )
        .await
        .expect("request completes")
    }

    async fn post_json_response_with_session(
        app: axum::Router,
        path: &str,
        username: &str,
        session_token: &str,
        body: &impl serde::Serialize,
    ) -> axum::response::Response {
        app.oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header("content-type", "application/json")
                .header(STANDALONE_USERNAME_HEADER, username)
                .header(STANDALONE_SESSION_TOKEN_HEADER, session_token)
                .body(Body::from(
                    serde_json::to_vec(body).expect("request encodes"),
                ))
                .expect("request builds"),
        )
        .await
        .expect("request completes")
    }

    fn test_users_groups_state(
        auth_store: LocalAuthStore,
        current_user: LocalUserMetadata,
        local_group_admin_client: Option<Arc<RecordingAdminClient>>,
    ) -> StandaloneUsersGroupsRouteState {
        StandaloneUsersGroupsRouteState {
            auth_store,
            local_user_provider: Arc::new(FixedLocalUserProvider { current_user }),
            local_group_admin_client: local_group_admin_client
                .map(|client| client as Arc<dyn StandaloneLocalGroupAdminClient>),
        }
    }

    fn local_user(username: &str, groups: Vec<&str>) -> LocalUserMetadata {
        LocalUserMetadata::from_username_and_groups(
            username,
            groups.into_iter().map(str::to_string).collect(),
        )
    }

    #[derive(Clone)]
    struct FixedLocalUserProvider {
        current_user: LocalUserMetadata,
    }

    impl LocalUserAuthorityProvider for FixedLocalUserProvider {
        fn current_local_user(&self) -> Result<LocalUserMetadata, LocalUserDiscoveryError> {
            Ok(self.current_user.clone())
        }
    }

    #[derive(Default)]
    struct RecordingAdminClient {
        requests: Mutex<Vec<StandaloneLocalGroupAdminDaemonRequest>>,
    }

    impl RecordingAdminClient {
        fn requests(&self) -> Vec<StandaloneLocalGroupAdminDaemonRequest> {
            self.requests.lock().expect("requests lock").clone()
        }
    }

    impl StandaloneLocalGroupAdminClient for RecordingAdminClient {
        fn submit_local_group_operation(
            &self,
            request: StandaloneLocalGroupAdminDaemonRequest,
        ) -> Result<StandaloneLocalGroupAdminResponse, StandaloneLocalGroupAdminClientError>
        {
            self.requests
                .lock()
                .expect("requests lock")
                .push(request.clone());
            Ok(StandaloneLocalGroupAdminResponse {
                accepted: StandaloneLocalGroupAdminAcceptedResponse {
                    job_id: "local-group-job-1".to_string(),
                    kind: "system_administration".to_string(),
                    accepted_at_utc: "2026-07-07T12:00:00Z".to_string(),
                    dry_run: request.dry_run,
                },
                operation: request.operation,
                group_name: request.group_name,
                username: request.username,
                client_request_id: request.client_request_id,
            })
        }
    }

    fn recording_admin_client() -> Arc<RecordingAdminClient> {
        Arc::new(RecordingAdminClient::default())
    }

    fn registered_auth_store(root: &Path) -> LocalAuthStore {
        let auth_store = LocalAuthStore::new(root);
        auth_store.create_user("admin").expect("user created");
        let token = auth_store
            .issue_registration_token("admin", Some(3_600))
            .expect("token issued");
        auth_store
            .register_with_token("admin", &token, "secret")
            .expect("registered");
        auth_store
    }

    fn expire_user_sessions(auth_store: &LocalAuthStore, username: &str) {
        let mut registry = auth_store.load_registry().expect("registry loads");
        let user = registry
            .users
            .iter_mut()
            .find(|user| user.username == username)
            .expect("user exists");
        for session in &mut user.sessions {
            session.expires_at_unix_seconds = 0;
        }
        let data = serde_json::to_string_pretty(&registry).expect("registry encodes");
        fs::write(auth_store.registry_path(), format!("{data}\n")).expect("registry writes");
    }

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "dasobjectstore-auth-routes-{label}-{}-{}",
            std::process::id(),
            unix_now_nanos()
        ))
    }

    fn unix_now_nanos() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time after unix epoch")
            .as_nanos()
    }

    fn cleanup(root: &Path) {
        let _ = fs::remove_dir_all(root);
    }
}
