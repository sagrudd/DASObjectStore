use crate::groups_registry::{
    default_groups_registry_path, read_storage_groups_for_user, upsert_storage_group,
};
use crate::{
    discover_local_user, AuthenticatedGuiActor, DashboardWarning, LocalAuthStore,
    LocalAuthStoreError, LocalPasswordAuthError, LoginResponse, LogoutResponse,
    PamLocalPasswordAuthenticator, RegisterResponse, SessionCheckResponse,
    UsersGroupsWorkspaceView,
};
use axum::{
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, Path, State},
    http::{
        header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE},
        HeaderMap, HeaderValue, StatusCode,
    },
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use dasobjectstore_daemon::runtime::LOCAL_ADMIN_CONFIRMATION_MARKER;
use dasobjectstore_daemon::{
    AssignLocalUserToLocalGroupRequest as DaemonAssignLocalUserToLocalGroupRequest,
    AssignLocalUserToLocalGroupResponse as DaemonAssignLocalUserToLocalGroupResponse,
    CreateLocalGroupRequest as DaemonCreateLocalGroupRequest,
    CreateLocalGroupResponse as DaemonCreateLocalGroupResponse,
    CreateObjectStoreRequest as DaemonCreateObjectStoreRequest,
    CreateObjectStoreResponse as DaemonCreateObjectStoreResponse, DaemonClient,
    DaemonEndpointBinding, DaemonEndpointBindingReadiness, DaemonEndpointKind,
    DaemonEndpointValidation, DaemonEndpointValidationState, DaemonJobCancelRequest,
    DaemonJobCancelResponse, DaemonJobId, DaemonJobKind, DaemonJobProgress, DaemonJobState,
    DaemonJobStatusRequest, DaemonJobStatusResponse, DaemonJobSummary, DaemonLocalAdminCommand,
    DaemonRuntimeConfig, PrepareEnclosureFilesystem as DaemonPrepareEnclosureFilesystem,
    PrepareEnclosureHddDevice as DaemonPrepareEnclosureHddDevice,
    PrepareEnclosureRequest as DaemonPrepareEnclosureRequest,
    PrepareEnclosureResponse as DaemonPrepareEnclosureResponse, RemoteEasyconnectAuthProvider,
    RemoteEasyconnectDiscoveryResponse, RemoteEasyconnectSessionPolicy, UnixSocketDaemonTransport,
    UpsertEndpointInventoryRequest as DaemonUpsertEndpointInventoryRequest,
    UpsertEndpointInventoryResponse as DaemonUpsertEndpointInventoryResponse,
    ENCLOSURE_PREPARE_CONFIRMATION, ENDPOINT_RECORD_CONFIRMATION, OBJECT_STORE_CREATE_CONFIRMATION,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
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
        GuiApiHostMode::Standalone => crate::routes::gui_api_router_without_redesign_dashboards()
            .merge(standalone_dashboard_router(auth_store.clone()))
            .merge(standalone_auth_router(auth_store.clone()))
            .merge(standalone_easyconnect_router(auth_store.clone()))
            .merge(standalone_users_groups_router(auth_store.clone()))
            .merge(standalone_enclosure_admin_router(auth_store.clone()))
            .merge(
                crate::object_browser_routes::standalone_object_browser_router(auth_store.clone()),
            )
            .merge(standalone_reporting_router(auth_store)),
        GuiApiHostMode::SynoptikonIntegrated => crate::gui_api_router(),
    }
}

pub fn standalone_auth_router(auth_store: LocalAuthStore) -> Router {
    standalone_auth_router_with_state(StandaloneAuthRouteState::system(auth_store))
}

fn standalone_auth_router_with_state(state: StandaloneAuthRouteState) -> Router {
    Router::new()
        .route("/api/register", post(register))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/session", post(session))
        .with_state(state)
}

pub fn standalone_easyconnect_router(auth_store: LocalAuthStore) -> Router {
    standalone_easyconnect_router_with_state(StandaloneEasyconnectRouteState::system(auth_store))
}

fn standalone_easyconnect_router_with_state(state: StandaloneEasyconnectRouteState) -> Router {
    Router::new()
        .route(
            "/api/v1/remote/easyconnect/discovery",
            get(easyconnect_discovery),
        )
        .route(
            "/api/v1/remote/easyconnect/auth-context",
            get(easyconnect_auth_context),
        )
        .layer(Extension(state.auth_store.clone()))
        .with_state(state)
}

fn standalone_dashboard_router(auth_store: LocalAuthStore) -> Router {
    standalone_dashboard_router_with_state(StandaloneDashboardRouteState::system(auth_store))
}

fn standalone_dashboard_router_with_state(state: StandaloneDashboardRouteState) -> Router {
    Router::new()
        .route("/api/v1/dashboard/home", get(standalone_home_dashboard))
        .route(
            "/api/v1/dashboard/enclosures",
            get(standalone_enclosures_dashboard),
        )
        .route(
            "/api/v1/dashboard/object-stores",
            get(standalone_object_stores_dashboard),
        )
        .route(
            "/api/v1/workspaces/remote-upload",
            get(standalone_remote_upload_workspace),
        )
        .layer(Extension(state.auth_store.clone()))
        .with_state(state)
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

pub fn standalone_enclosure_admin_router(auth_store: LocalAuthStore) -> Router {
    standalone_enclosure_admin_router_with_state(StandaloneEnclosureAdminRouteState::system(
        auth_store,
    ))
}

fn standalone_enclosure_admin_router_with_state(
    state: StandaloneEnclosureAdminRouteState,
) -> Router {
    Router::new()
        .route(
            "/api/v1/workspaces/enclosures/prepare",
            post(prepare_enclosure),
        )
        .route(
            "/api/v1/workspaces/object-stores/create",
            post(create_object_store),
        )
        .route(
            "/api/v1/workspaces/endpoints/upsert",
            post(upsert_endpoint_inventory),
        )
        .route(
            "/api/v1/workspaces/admin/jobs/{job_id}",
            get(admin_job_status),
        )
        .route(
            "/api/v1/workspaces/admin/jobs/{job_id}/cancel",
            post(cancel_admin_job),
        )
        .layer(Extension(state.auth_store.clone()))
        .with_state(state)
}

pub fn standalone_reporting_router(auth_store: LocalAuthStore) -> Router {
    standalone_reporting_router_with_state(StandaloneReportingRouteState::system(auth_store))
}

fn standalone_reporting_router_with_state(state: StandaloneReportingRouteState) -> Router {
    Router::new()
        .route(
            "/api/v1/workspaces/activity/reporting/performance-report",
            post(rebuild_performance_report),
        )
        .layer(DefaultBodyLimit::max(
            crate::reporting::PERFORMANCE_REPORT_UPLOAD_MAX_BYTES,
        ))
        .layer(Extension(state.auth_store.clone()))
        .with_state(state)
}

#[derive(Clone)]
struct StandaloneUsersGroupsRouteState {
    auth_store: LocalAuthStore,
    local_user_provider: Arc<dyn LocalUserAuthorityProvider>,
    local_group_admin_client: Option<Arc<dyn StandaloneLocalGroupAdminClient>>,
    groups_registry_path: PathBuf,
}

#[derive(Clone)]
struct StandaloneDashboardRouteState {
    auth_store: LocalAuthStore,
    local_user_provider: Arc<dyn LocalUserAuthorityProvider>,
}

#[derive(Clone)]
struct StandaloneEnclosureAdminRouteState {
    auth_store: LocalAuthStore,
    local_user_provider: Arc<dyn LocalUserAuthorityProvider>,
    enclosure_admin_client: Option<Arc<dyn StandaloneEnclosureAdminClient>>,
}

#[derive(Clone)]
struct StandaloneReportingRouteState {
    auth_store: LocalAuthStore,
    local_user_provider: Arc<dyn LocalUserAuthorityProvider>,
}

impl StandaloneEnclosureAdminRouteState {
    fn system(auth_store: LocalAuthStore) -> Self {
        Self {
            auth_store,
            local_user_provider: Arc::new(SystemLocalUserAuthorityProvider),
            enclosure_admin_client: Some(Arc::new(
                DaemonStandaloneEnclosureAdminClient::default_packaged(),
            )),
        }
    }
}

impl StandaloneReportingRouteState {
    fn system(auth_store: LocalAuthStore) -> Self {
        Self {
            auth_store,
            local_user_provider: Arc::new(SystemLocalUserAuthorityProvider),
        }
    }
}

impl StandaloneDashboardRouteState {
    fn system(auth_store: LocalAuthStore) -> Self {
        Self {
            auth_store,
            local_user_provider: Arc::new(SystemLocalUserAuthorityProvider),
        }
    }
}

impl StandaloneUsersGroupsRouteState {
    fn system(auth_store: LocalAuthStore) -> Self {
        Self {
            auth_store,
            local_user_provider: Arc::new(SystemLocalUserAuthorityProvider),
            local_group_admin_client: Some(Arc::new(
                DaemonStandaloneLocalGroupAdminClient::default_packaged(),
            )),
            groups_registry_path: default_groups_registry_path(),
        }
    }
}

#[derive(Clone)]
struct StandaloneAuthRouteState {
    auth_store: LocalAuthStore,
    local_password_authenticator: Arc<dyn LocalPasswordAuthenticator>,
}

#[derive(Clone)]
struct StandaloneEasyconnectRouteState {
    auth_store: LocalAuthStore,
    public_base_url: String,
}

impl StandaloneAuthRouteState {
    fn system(auth_store: LocalAuthStore) -> Self {
        Self {
            auth_store,
            local_password_authenticator: Arc::new(SystemLocalPasswordAuthenticator::default()),
        }
    }
}

impl StandaloneEasyconnectRouteState {
    fn system(auth_store: LocalAuthStore) -> Self {
        Self {
            auth_store,
            public_base_url: crate::DEFAULT_STANDALONE_PUBLIC_BASE_URL.to_string(),
        }
    }
}

trait LocalPasswordAuthenticator: Send + Sync {
    fn authenticate(&self, username: &str, password: &str) -> Result<(), LocalPasswordAuthError>;
}

#[derive(Default)]
struct SystemLocalPasswordAuthenticator {
    pam: PamLocalPasswordAuthenticator,
}

impl LocalPasswordAuthenticator for SystemLocalPasswordAuthenticator {
    fn authenticate(&self, username: &str, password: &str) -> Result<(), LocalPasswordAuthError> {
        self.pam.authenticate(username, password)
    }
}

trait LocalUserAuthorityProvider: Send + Sync {
    fn local_user(
        &self,
        username: &str,
    ) -> Result<crate::LocalUserMetadata, crate::LocalUserDiscoveryError>;
}

struct SystemLocalUserAuthorityProvider;

impl LocalUserAuthorityProvider for SystemLocalUserAuthorityProvider {
    fn local_user(
        &self,
        username: &str,
    ) -> Result<crate::LocalUserMetadata, crate::LocalUserDiscoveryError> {
        discover_local_user(username)
    }
}

trait StandaloneLocalGroupAdminClient: Send + Sync {
    fn submit_local_group_operation(
        &self,
        request: StandaloneLocalGroupAdminDaemonRequest,
    ) -> Result<StandaloneLocalGroupAdminResponse, StandaloneLocalGroupAdminClientError>;
}

trait StandaloneEnclosureAdminClient: Send + Sync {
    fn submit_prepare_enclosure(
        &self,
        request: StandaloneEnclosurePrepareDaemonRequest,
    ) -> Result<StandaloneEnclosurePrepareResponse, StandaloneEnclosureAdminClientError>;

    fn submit_create_object_store(
        &self,
        request: DaemonCreateObjectStoreRequest,
    ) -> Result<StandaloneCreateObjectStoreResponse, StandaloneEnclosureAdminClientError>;

    fn submit_endpoint_inventory_upsert(
        &self,
        request: DaemonUpsertEndpointInventoryRequest,
    ) -> Result<StandaloneEndpointInventoryUpsertResponse, StandaloneEnclosureAdminClientError>;

    fn job_status(
        &self,
        request: StandaloneAdminJobStatusDaemonRequest,
    ) -> Result<StandaloneAdminJobStatusResponse, StandaloneEnclosureAdminClientError>;

    fn cancel_job(
        &self,
        request: StandaloneAdminJobCancelDaemonRequest,
    ) -> Result<StandaloneAdminJobCancelResponse, StandaloneEnclosureAdminClientError>;
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
pub struct StandaloneEasyconnectAuthContextResponse {
    pub schema_version: String,
    pub auth_provider: RemoteEasyconnectAuthProvider,
    pub subject_id: String,
    pub session_expires_at_unix_seconds: Option<i64>,
    pub supported_auth_providers: Vec<RemoteEasyconnectAuthProvider>,
    pub future_auth_providers: Vec<RemoteEasyconnectAuthProvider>,
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PrepareEnclosureRequest {
    pub ssd_device: String,
    #[serde(default)]
    pub hdd_devices: Vec<PrepareEnclosureHddDeviceRequest>,
    pub mount_root: Option<String>,
    pub filesystem: Option<String>,
    pub owner: Option<String>,
    #[serde(default)]
    pub dry_run: bool,
    pub client_request_id: Option<String>,
    #[serde(default)]
    pub allow_format: bool,
    #[serde(default)]
    pub existing_data_acknowledged: bool,
    pub confirmation_marker: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PrepareEnclosureHddDeviceRequest {
    pub disk_id: String,
    pub device_path: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CreateObjectStoreRequest {
    pub store_id: String,
    #[serde(default)]
    pub store_class: Option<String>,
    pub required_copies: u8,
    pub bucket: Option<String>,
    #[serde(default)]
    pub reader_group: Option<String>,
    pub writer_group: String,
    #[serde(default)]
    pub ssd_root: Option<String>,
    #[serde(default)]
    pub object_type: Option<String>,
    pub enclosure_id: Option<String>,
    #[serde(default)]
    pub public: bool,
    #[serde(default)]
    pub writeable: Option<bool>,
    #[serde(default)]
    pub capacity_behavior: Option<String>,
    #[serde(default)]
    pub retention: Option<String>,
    #[serde(default)]
    pub endpoint_export_mode: Option<String>,
    #[serde(default)]
    pub dry_run: bool,
    pub client_request_id: Option<String>,
    pub confirmation_marker: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EndpointInventoryUpsertRequest {
    pub endpoint_id: String,
    pub display_name: String,
    pub kind: String,
    pub object_service_url: String,
    pub validation: EndpointValidationUpsertRequest,
    #[serde(default = "default_manager_product_id")]
    pub manager_product_id: String,
    #[serde(default)]
    pub active_bindings: Vec<EndpointBindingUpsertRequest>,
    #[serde(default)]
    pub dry_run: bool,
    pub client_request_id: Option<String>,
    pub confirmation_marker: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EndpointValidationUpsertRequest {
    pub state: String,
    pub checked_at_utc: Option<String>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EndpointBindingUpsertRequest {
    pub binding_id: String,
    pub governance_domain: String,
    pub store_id: String,
    pub readiness: String,
}

fn default_manager_product_id() -> String {
    "dasobjectstore".to_string()
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StandaloneEnclosurePrepareResponse {
    pub accepted: StandaloneEnclosurePrepareAcceptedResponse,
    pub ssd_device: String,
    pub hdd_devices: Vec<PrepareEnclosureHddDeviceRequest>,
    pub mount_root: String,
    pub filesystem: String,
    pub owner: Option<String>,
    pub administrator_actor: Option<String>,
    pub client_request_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StandaloneEnclosurePrepareAcceptedResponse {
    pub job_id: String,
    pub kind: String,
    pub accepted_at_utc: String,
    pub dry_run: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StandaloneCreateObjectStoreResponse {
    pub accepted: StandaloneCreateObjectStoreAcceptedResponse,
    pub store_id: String,
    pub store_class: String,
    pub required_copies: u8,
    pub bucket: Option<String>,
    pub reader_group: Option<String>,
    pub writer_group: String,
    pub ssd_root: String,
    pub object_type: String,
    pub enclosure_id: Option<String>,
    pub public: bool,
    pub writeable: bool,
    pub capacity_behavior: String,
    pub retention: String,
    pub endpoint_export_mode: String,
    pub administrator_actor: Option<String>,
    pub client_request_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StandaloneCreateObjectStoreAcceptedResponse {
    pub job_id: String,
    pub kind: String,
    pub accepted_at_utc: String,
    pub dry_run: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StandaloneEndpointInventoryUpsertResponse {
    pub accepted: StandaloneEndpointInventoryAcceptedResponse,
    pub endpoint_id: String,
    pub display_name: String,
    pub kind: String,
    pub validation_state: String,
    pub registry_path: String,
    pub administrator_actor: Option<String>,
    pub client_request_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StandaloneEndpointInventoryAcceptedResponse {
    pub job_id: String,
    pub kind: String,
    pub accepted_at_utc: String,
    pub dry_run: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CancelAdminJobRequest {
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StandaloneAdminJobStatusResponse {
    pub job: StandaloneAdminJobSummary,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StandaloneAdminJobSummary {
    pub job_id: String,
    pub kind: String,
    pub state: String,
    pub progress: StandaloneAdminJobProgress,
    pub percent_complete: Option<u8>,
    pub submitted_at_utc: String,
    pub updated_at_utc: String,
    pub actor: Option<String>,
    pub failure_message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StandaloneAdminJobProgress {
    pub stage: String,
    pub work_bytes_done: u64,
    pub work_bytes_total: u64,
    pub work_units_done: u64,
    pub work_units_total: u64,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StandaloneAdminJobCancelResponse {
    pub job_id: String,
    pub accepted: bool,
    pub state: String,
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
struct StandaloneEnclosurePrepareDaemonRequest {
    ssd_device: String,
    hdd_devices: Vec<PrepareEnclosureHddDeviceRequest>,
    mount_root: String,
    filesystem: DaemonPrepareEnclosureFilesystem,
    owner: Option<String>,
    dry_run: bool,
    client_request_id: Option<String>,
    administrator_actor: Option<String>,
    allow_format: bool,
    existing_data_acknowledged: bool,
    confirmation_marker: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StandaloneAdminJobStatusDaemonRequest {
    job_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StandaloneAdminJobCancelDaemonRequest {
    job_id: String,
    reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StandaloneLocalGroupAdminClientError {
    message: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StandaloneEnclosureAdminClientError {
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

struct DaemonStandaloneEnclosureAdminClient {
    client: DaemonClient<UnixSocketDaemonTransport>,
}

impl DaemonStandaloneEnclosureAdminClient {
    fn default_packaged() -> Self {
        Self {
            client: DaemonClient::new(UnixSocketDaemonTransport::new(
                DaemonRuntimeConfig::default_packaged().socket_path,
            )),
        }
    }
}

impl StandaloneEnclosureAdminClient for DaemonStandaloneEnclosureAdminClient {
    fn submit_prepare_enclosure(
        &self,
        request: StandaloneEnclosurePrepareDaemonRequest,
    ) -> Result<StandaloneEnclosurePrepareResponse, StandaloneEnclosureAdminClientError> {
        self.client
            .prepare_enclosure(DaemonPrepareEnclosureRequest {
                ssd_device: request.ssd_device.into(),
                hdd_devices: request
                    .hdd_devices
                    .into_iter()
                    .map(|device| DaemonPrepareEnclosureHddDevice {
                        disk_id: device.disk_id,
                        device_path: device.device_path.into(),
                    })
                    .collect(),
                mount_root: request.mount_root.into(),
                filesystem: request.filesystem,
                owner: request.owner,
                dry_run: request.dry_run,
                client_request_id: request.client_request_id,
                administrator_actor: request.administrator_actor,
                allow_format: request.allow_format,
                existing_data_acknowledged: request.existing_data_acknowledged,
                confirmation_marker: request.confirmation_marker,
            })
            .map(enclosure_prepare_response_from_daemon)
            .map_err(standalone_enclosure_admin_client_error)
    }

    fn submit_create_object_store(
        &self,
        request: DaemonCreateObjectStoreRequest,
    ) -> Result<StandaloneCreateObjectStoreResponse, StandaloneEnclosureAdminClientError> {
        self.client
            .create_object_store(request)
            .map(create_object_store_response_from_daemon)
            .map_err(standalone_enclosure_admin_client_error)
    }

    fn submit_endpoint_inventory_upsert(
        &self,
        request: DaemonUpsertEndpointInventoryRequest,
    ) -> Result<StandaloneEndpointInventoryUpsertResponse, StandaloneEnclosureAdminClientError>
    {
        self.client
            .upsert_endpoint_inventory(request)
            .map(endpoint_inventory_upsert_response_from_daemon)
            .map_err(standalone_enclosure_admin_client_error)
    }

    fn job_status(
        &self,
        request: StandaloneAdminJobStatusDaemonRequest,
    ) -> Result<StandaloneAdminJobStatusResponse, StandaloneEnclosureAdminClientError> {
        let job_id = DaemonJobId::new(request.job_id).map_err(|err| {
            StandaloneEnclosureAdminClientError {
                message: err.to_string(),
            }
        })?;
        self.client
            .job_status(DaemonJobStatusRequest { job_id })
            .map(admin_job_status_response_from_daemon)
            .map_err(standalone_enclosure_admin_client_error)
    }

    fn cancel_job(
        &self,
        request: StandaloneAdminJobCancelDaemonRequest,
    ) -> Result<StandaloneAdminJobCancelResponse, StandaloneEnclosureAdminClientError> {
        let job_id = DaemonJobId::new(request.job_id).map_err(|err| {
            StandaloneEnclosureAdminClientError {
                message: err.to_string(),
            }
        })?;
        self.client
            .cancel_job(DaemonJobCancelRequest {
                job_id,
                reason: request.reason,
            })
            .map(admin_job_cancel_response_from_daemon)
            .map_err(standalone_enclosure_admin_client_error)
    }
}

async fn register(
    State(state): State<StandaloneAuthRouteState>,
    Json(request): Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>, (StatusCode, Json<AuthRouteError>)> {
    state
        .auth_store
        .register_with_token(&request.username, &request.token, &request.password)
        .map(Json)
        .map_err(auth_route_error)
}

async fn login(
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

async fn logout(
    State(state): State<StandaloneAuthRouteState>,
    Json(request): Json<LogoutRequest>,
) -> Result<Json<LogoutResponse>, (StatusCode, Json<AuthRouteError>)> {
    state
        .auth_store
        .logout(&request.username, &request.session_token)
        .map(Json)
        .map_err(auth_route_error)
}

async fn session(
    State(state): State<StandaloneAuthRouteState>,
    Json(request): Json<SessionCheckRequest>,
) -> Result<Json<SessionCheckResponse>, (StatusCode, Json<AuthRouteError>)> {
    state
        .auth_store
        .verify_session(&request.username, &request.session_token)
        .map(Json)
        .map_err(auth_route_error)
}

async fn easyconnect_discovery(
    State(state): State<StandaloneEasyconnectRouteState>,
) -> Json<RemoteEasyconnectDiscoveryResponse> {
    Json(standalone_easyconnect_discovery_payload(
        &state.public_base_url,
    ))
}

async fn easyconnect_auth_context(
    actor: AuthenticatedGuiActor,
) -> Result<Json<StandaloneEasyconnectAuthContextResponse>, (StatusCode, Json<AuthRouteError>)> {
    if actor.authority != crate::AuthenticatedActorAuthority::LocalStandalone {
        return Err(route_error(
            StatusCode::FORBIDDEN,
            "standalone_local_user_required",
            "easyconnect standalone authentication requires a local standalone browser session",
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

async fn standalone_home_dashboard(
    State(_state): State<StandaloneDashboardRouteState>,
    _actor: AuthenticatedGuiActor,
) -> Result<Json<crate::dashboard::HomeDashboardView>, (StatusCode, Json<AuthRouteError>)> {
    Ok(Json(crate::home_aggregator::live_home_dashboard()))
}

async fn standalone_enclosures_dashboard(
    State(state): State<StandaloneDashboardRouteState>,
    actor: AuthenticatedGuiActor,
) -> Result<Json<crate::dashboard::EnclosuresPageView>, (StatusCode, Json<AuthRouteError>)> {
    let current_user = local_standalone_user(state.local_user_provider.as_ref(), &actor)?;
    Ok(Json(
        crate::enclosures_aggregator::live_enclosures_dashboard_for_administrator(
            current_user.sudo_administrator,
        ),
    ))
}

async fn standalone_object_stores_dashboard(
    State(state): State<StandaloneDashboardRouteState>,
    actor: AuthenticatedGuiActor,
) -> Result<Json<crate::dashboard::ObjectStoresPageView>, (StatusCode, Json<AuthRouteError>)> {
    let current_user = local_standalone_user(state.local_user_provider.as_ref(), &actor)?;
    Ok(Json(
        crate::object_stores_aggregator::live_object_stores_dashboard_for_user(
            current_user.groups,
            current_user.sudo_administrator,
        ),
    ))
}

async fn standalone_remote_upload_workspace(
    State(state): State<StandaloneDashboardRouteState>,
    actor: AuthenticatedGuiActor,
) -> Result<Json<crate::RemoteUploadWorkspaceView>, (StatusCode, Json<AuthRouteError>)> {
    let current_user = local_standalone_user(state.local_user_provider.as_ref(), &actor)?;
    Ok(Json(
        crate::remote_upload_aggregator::live_remote_upload_workspace_for_user(
            current_user.username,
            current_user.groups,
            current_user.sudo_administrator,
        ),
    ))
}

async fn users_groups_workspace(
    State(state): State<StandaloneUsersGroupsRouteState>,
    actor: AuthenticatedGuiActor,
) -> Result<Json<UsersGroupsWorkspaceView>, (StatusCode, Json<AuthRouteError>)> {
    let users = state.auth_store.list_users().map_err(auth_route_error)?;
    let (current_user, warnings) =
        match actor_local_user_for_workspace(state.local_user_provider.as_ref(), &actor) {
            Ok(user) => (Some(user), Vec::new()),
            Err(err) => (
                None,
                vec![DashboardWarning {
                    code: "local_user_discovery_failed".to_string(),
                    message: err.to_string(),
                }],
            ),
        };
    let current_user_groups = current_user
        .as_ref()
        .map(|user| user.groups.clone())
        .unwrap_or_default();
    let groups_snapshot =
        read_storage_groups_for_user(&state.groups_registry_path, &current_user_groups);
    let mut warnings = warnings;
    warnings.extend(groups_snapshot.warnings);

    Ok(Json(UsersGroupsWorkspaceView::standalone(
        current_user,
        users,
        groups_snapshot.path.display().to_string(),
        groups_snapshot.groups,
        warnings,
    )))
}

fn actor_local_user_for_workspace(
    local_user_provider: &dyn LocalUserAuthorityProvider,
    actor: &AuthenticatedGuiActor,
) -> Result<crate::LocalUserMetadata, String> {
    if actor.authority != crate::AuthenticatedActorAuthority::LocalStandalone {
        return Err("standalone local session is required to inspect local authority.".to_string());
    }
    local_user_provider
        .local_user(&actor.subject_id)
        .map_err(|err| err.to_string())
}

fn standalone_easyconnect_discovery_payload(
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

async fn create_local_group(
    State(state): State<StandaloneUsersGroupsRouteState>,
    actor: AuthenticatedGuiActor,
    Json(request): Json<CreateLocalGroupRequest>,
) -> Result<Json<StandaloneLocalGroupAdminResponse>, (StatusCode, Json<AuthRouteError>)> {
    let mut request = validate_create_local_group_request(request)?;
    let current_user = require_local_administrator(state.local_user_provider.as_ref(), &actor)?;
    request.administrator_actor = Some(current_user.username);
    submit_local_group_admin_request(&state, request).map(Json)
}

async fn assign_local_user_to_group(
    State(state): State<StandaloneUsersGroupsRouteState>,
    actor: AuthenticatedGuiActor,
    Json(request): Json<AssignLocalUserToGroupRequest>,
) -> Result<Json<StandaloneLocalGroupAdminResponse>, (StatusCode, Json<AuthRouteError>)> {
    let mut request = validate_assign_local_user_to_group_request(request)?;
    let current_user = require_local_administrator(state.local_user_provider.as_ref(), &actor)?;
    request.administrator_actor = Some(current_user.username);
    submit_local_group_admin_request(&state, request).map(Json)
}

async fn prepare_enclosure(
    State(state): State<StandaloneEnclosureAdminRouteState>,
    actor: AuthenticatedGuiActor,
    Json(request): Json<PrepareEnclosureRequest>,
) -> Result<Json<StandaloneEnclosurePrepareResponse>, (StatusCode, Json<AuthRouteError>)> {
    let mut request = validate_prepare_enclosure_request(request)?;
    let current_user = require_local_administrator(state.local_user_provider.as_ref(), &actor)?;
    request.administrator_actor = Some(current_user.username);
    submit_prepare_enclosure_request(&state, request).map(Json)
}

async fn create_object_store(
    State(state): State<StandaloneEnclosureAdminRouteState>,
    actor: AuthenticatedGuiActor,
    Json(request): Json<CreateObjectStoreRequest>,
) -> Result<Json<StandaloneCreateObjectStoreResponse>, (StatusCode, Json<AuthRouteError>)> {
    let mut request = validate_create_object_store_request(request)?;
    let current_user = require_local_administrator(state.local_user_provider.as_ref(), &actor)?;
    request.administrator_actor = Some(current_user.username);
    submit_create_object_store_request(&state, request).map(Json)
}

async fn upsert_endpoint_inventory(
    State(state): State<StandaloneEnclosureAdminRouteState>,
    actor: AuthenticatedGuiActor,
    Json(request): Json<EndpointInventoryUpsertRequest>,
) -> Result<Json<StandaloneEndpointInventoryUpsertResponse>, (StatusCode, Json<AuthRouteError>)> {
    let mut request = validate_endpoint_inventory_upsert_request(request)?;
    let current_user = require_local_administrator(state.local_user_provider.as_ref(), &actor)?;
    request.administrator_actor = Some(current_user.username);
    submit_endpoint_inventory_upsert_request(&state, request).map(Json)
}

async fn rebuild_performance_report(
    State(state): State<StandaloneReportingRouteState>,
    actor: AuthenticatedGuiActor,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, (StatusCode, Json<AuthRouteError>)> {
    let current_user = require_local_administrator(state.local_user_provider.as_ref(), &actor)?;
    let uploaded_filename = headers
        .get("x-dasobjectstore-filename")
        .and_then(|value| value.to_str().ok());
    let report = crate::reporting::rebuild_performance_report_pdf_from_upload(
        &body,
        uploaded_filename,
        &current_user.username,
    )
    .map_err(performance_report_rebuild_route_error)?;

    let mut response = Body::from(report.bytes).into_response();
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/pdf"));
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "attachment; filename=\"{}\"",
            report.filename.replace('"', "")
        ))
        .map_err(|err| {
            route_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "invalid_report_filename",
                err.to_string(),
            )
        })?,
    );
    Ok(response)
}

fn performance_report_rebuild_route_error(
    err: crate::reporting::PerformanceReportRebuildError,
) -> (StatusCode, Json<AuthRouteError>) {
    match err {
        crate::reporting::PerformanceReportRebuildError::EmptyUpload
        | crate::reporting::PerformanceReportRebuildError::TooLarge { .. }
        | crate::reporting::PerformanceReportRebuildError::InvalidJson(_)
        | crate::reporting::PerformanceReportRebuildError::UnsupportedSchema(_) => route_error(
            StatusCode::BAD_REQUEST,
            "performance_report_rebuild_failed",
            err.to_string(),
        ),
        crate::reporting::PerformanceReportRebuildError::Io(_)
        | crate::reporting::PerformanceReportRebuildError::RendererFailed(_) => route_error(
            StatusCode::BAD_GATEWAY,
            "performance_report_renderer_failed",
            err.to_string(),
        ),
    }
}

async fn admin_job_status(
    State(state): State<StandaloneEnclosureAdminRouteState>,
    actor: AuthenticatedGuiActor,
    Path(job_id): Path<String>,
) -> Result<Json<StandaloneAdminJobStatusResponse>, (StatusCode, Json<AuthRouteError>)> {
    require_local_administrator(state.local_user_provider.as_ref(), &actor)?;
    let request = StandaloneAdminJobStatusDaemonRequest {
        job_id: required_field("job_id", job_id)?,
    };
    submit_admin_job_status_request(&state, request).map(Json)
}

async fn cancel_admin_job(
    State(state): State<StandaloneEnclosureAdminRouteState>,
    actor: AuthenticatedGuiActor,
    Path(job_id): Path<String>,
    Json(request): Json<CancelAdminJobRequest>,
) -> Result<Json<StandaloneAdminJobCancelResponse>, (StatusCode, Json<AuthRouteError>)> {
    require_local_administrator(state.local_user_provider.as_ref(), &actor)?;
    let request = validate_cancel_admin_job_request(job_id, request)?;
    submit_admin_job_cancel_request(&state, request).map(Json)
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

fn validate_prepare_enclosure_request(
    request: PrepareEnclosureRequest,
) -> Result<StandaloneEnclosurePrepareDaemonRequest, (StatusCode, Json<AuthRouteError>)> {
    let ssd_device = required_field("ssd_device", request.ssd_device)?;
    let mount_root = request
        .mount_root
        .map(|value| required_field("mount_root", value))
        .transpose()?
        .unwrap_or_else(|| "/srv/dasobjectstore".to_string());
    reject_known_managed_enclosure_mount_root(&mount_root)?;
    let filesystem = parse_prepare_enclosure_filesystem(request.filesystem.as_deref())?;
    validate_client_request_id(request.client_request_id.as_deref())?;
    let owner = request
        .owner
        .map(|value| required_field("owner", value))
        .transpose()?;
    let confirmation_marker = validate_prepare_enclosure_confirmation_marker(
        request.dry_run,
        request.allow_format,
        request.existing_data_acknowledged,
        request.confirmation_marker.as_deref(),
    )?;

    let mut hdd_devices = Vec::new();
    for hdd_device in request.hdd_devices {
        hdd_devices.push(PrepareEnclosureHddDeviceRequest {
            disk_id: required_field("hdd_devices.disk_id", hdd_device.disk_id)?,
            device_path: required_field("hdd_devices.device_path", hdd_device.device_path)?,
        });
    }
    if hdd_devices.is_empty() {
        return Err(route_error(
            StatusCode::BAD_REQUEST,
            "unsupported_das",
            "at least one eligible HDD device is required before enclosure preparation can be submitted",
        ));
    }

    Ok(StandaloneEnclosurePrepareDaemonRequest {
        ssd_device,
        hdd_devices,
        mount_root,
        filesystem,
        owner,
        dry_run: request.dry_run,
        client_request_id: request.client_request_id,
        administrator_actor: None,
        allow_format: request.allow_format,
        existing_data_acknowledged: request.existing_data_acknowledged,
        confirmation_marker,
    })
}

fn reject_known_managed_enclosure_mount_root(
    mount_root: &str,
) -> Result<(), (StatusCode, Json<AuthRouteError>)> {
    let mount_root = PathBuf::from(mount_root);
    let ssd_marker = mount_root
        .join("ssd")
        .join(".dasobjectstore")
        .join("device.env");
    let hdd_root = mount_root.join("hdd");
    let hdd_marker_present = fs::read_dir(&hdd_root)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(Result::ok))
        .map(|entry| entry.path().join(".dasobjectstore").join("device.env"))
        .any(|marker| marker.exists());

    if ssd_marker.exists() || hdd_marker_present {
        return Err(route_error(
            StatusCode::CONFLICT,
            "enclosure_already_managed",
            "enclosure preparation through the Web UI is available only for unprepared DAS enclosures; this mount root is already known to DASObjectStore",
        ));
    }

    Ok(())
}

fn validate_create_object_store_request(
    request: CreateObjectStoreRequest,
) -> Result<DaemonCreateObjectStoreRequest, (StatusCode, Json<AuthRouteError>)> {
    let store_id = required_field("store_id", request.store_id)?;
    let store_class = request
        .store_class
        .map(|value| required_field("store_class", value))
        .transpose()?
        .unwrap_or_else(|| "generated_data".to_string());
    let writer_group = required_field("writer_group", request.writer_group)?;
    let reader_group = request
        .reader_group
        .map(|value| required_field("reader_group", value))
        .transpose()?;
    let enclosure_id = request
        .enclosure_id
        .map(|value| required_field("enclosure_id", value))
        .transpose()?
        .ok_or_else(|| {
            route_error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "enclosure_id is required for ObjectStore creation",
            )
        })?;
    let ssd_root = request
        .ssd_root
        .map(|value| required_field("ssd_root", value))
        .transpose()?
        .unwrap_or_else(|| "/srv/dasobjectstore/ssd".to_string());
    let object_type = request
        .object_type
        .map(|value| required_field("object_type", value))
        .transpose()?
        .unwrap_or_else(|| "naive".to_string());
    let capacity_behavior = request
        .capacity_behavior
        .map(|value| required_field("capacity_behavior", value))
        .transpose()?
        .unwrap_or_else(|| "backpressure_by_priority".to_string());
    let retention = request
        .retention
        .map(|value| required_field("retention", value))
        .transpose()?
        .unwrap_or_else(|| "retain_until_deleted".to_string());
    let endpoint_export_mode = request
        .endpoint_export_mode
        .map(|value| required_field("endpoint_export_mode", value))
        .transpose()?
        .unwrap_or_else(|| "s3_bucket".to_string());
    validate_client_request_id(request.client_request_id.as_deref())?;
    let confirmation_marker = validate_object_store_create_confirmation_marker(
        request.dry_run,
        request.confirmation_marker.as_deref(),
    )?;
    let bucket = request
        .bucket
        .map(|value| required_field("bucket", value))
        .transpose()?
        .unwrap_or_else(|| derived_object_store_bucket_name(&store_id));

    let request = DaemonCreateObjectStoreRequest {
        store_id,
        store_class,
        required_copies: request.required_copies,
        bucket: Some(bucket),
        reader_group,
        writer_group,
        ssd_root: PathBuf::from(ssd_root),
        object_type,
        enclosure_id: Some(enclosure_id),
        public: request.public,
        writeable: request.writeable.unwrap_or(true),
        capacity_behavior,
        retention,
        endpoint_export_mode,
        dry_run: request.dry_run,
        client_request_id: request.client_request_id,
        administrator_actor: None,
        confirmation_marker,
    };
    request.validate().map_err(|err| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_objectstore_policy",
            err.to_string(),
        )
    })?;
    Ok(request)
}

fn derived_object_store_bucket_name(store_id: &str) -> String {
    store_id
        .trim()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn validate_endpoint_inventory_upsert_request(
    request: EndpointInventoryUpsertRequest,
) -> Result<DaemonUpsertEndpointInventoryRequest, (StatusCode, Json<AuthRouteError>)> {
    let endpoint_id = required_field("endpoint_id", request.endpoint_id)?;
    let display_name = required_field("display_name", request.display_name)?;
    let object_service_url = required_field("object_service_url", request.object_service_url)?;
    let manager_product_id = required_field("manager_product_id", request.manager_product_id)?;
    validate_client_request_id(request.client_request_id.as_deref())?;
    let confirmation_marker = validate_endpoint_inventory_confirmation_marker(
        request.dry_run,
        request.confirmation_marker.as_deref(),
    )?;

    let mut active_bindings = Vec::new();
    for binding in request.active_bindings {
        active_bindings.push(DaemonEndpointBinding {
            binding_id: required_field("active_bindings.binding_id", binding.binding_id)?,
            governance_domain: required_field(
                "active_bindings.governance_domain",
                binding.governance_domain,
            )?,
            store_id: required_field("active_bindings.store_id", binding.store_id)?,
            readiness: parse_endpoint_binding_readiness(&binding.readiness)?,
        });
    }

    let request = DaemonUpsertEndpointInventoryRequest {
        endpoint_id,
        display_name,
        kind: parse_endpoint_kind(&request.kind)?,
        object_service_url,
        validation: DaemonEndpointValidation {
            state: parse_endpoint_validation_state(&request.validation.state)?,
            checked_at_utc: request
                .validation
                .checked_at_utc
                .map(|value| required_field("validation.checked_at_utc", value))
                .transpose()?,
            message: request
                .validation
                .message
                .map(|value| required_field("validation.message", value))
                .transpose()?,
        },
        manager_product_id,
        active_bindings,
        dry_run: request.dry_run,
        client_request_id: request.client_request_id,
        administrator_actor: None,
        confirmation_marker: Some(confirmation_marker),
    };
    request.validate().map_err(|err| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_endpoint_inventory",
            err.to_string(),
        )
    })?;
    Ok(request)
}

fn validate_cancel_admin_job_request(
    job_id: String,
    request: CancelAdminJobRequest,
) -> Result<StandaloneAdminJobCancelDaemonRequest, (StatusCode, Json<AuthRouteError>)> {
    let reason = request
        .reason
        .map(|value| required_field("reason", value))
        .transpose()?;

    Ok(StandaloneAdminJobCancelDaemonRequest {
        job_id: required_field("job_id", job_id)?,
        reason,
    })
}

fn require_local_administrator(
    local_user_provider: &dyn LocalUserAuthorityProvider,
    actor: &AuthenticatedGuiActor,
) -> Result<crate::LocalUserMetadata, (StatusCode, Json<AuthRouteError>)> {
    let current_user = local_standalone_user(local_user_provider, actor)?;

    if !current_user.sudo_administrator {
        return Err(route_error(
            StatusCode::FORBIDDEN,
            "standalone_admin_authority_missing",
            "current OS user must be a sudo-derived DASObjectStore administrator",
        ));
    }

    Ok(current_user)
}

fn local_standalone_user(
    local_user_provider: &dyn LocalUserAuthorityProvider,
    actor: &AuthenticatedGuiActor,
) -> Result<crate::LocalUserMetadata, (StatusCode, Json<AuthRouteError>)> {
    if actor.authority != crate::AuthenticatedActorAuthority::LocalStandalone {
        return Err(route_error(
            StatusCode::FORBIDDEN,
            "standalone_local_session_required",
            "standalone local group administration requires a local session",
        ));
    }

    local_user_provider
        .local_user(&actor.subject_id)
        .map_err(|err| {
            route_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "local_user_discovery_failed",
                err.to_string(),
            )
        })
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

    let response = client
        .submit_local_group_operation(request)
        .map_err(|err| route_error(StatusCode::BAD_GATEWAY, "daemon_client_error", err.message))?;

    if !response.accepted.dry_run {
        upsert_storage_group(&state.groups_registry_path, &response.group_name).map_err(|err| {
            route_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "groups_registry_update_failed",
                format!(
                    "daemon accepted local group operation, but {} could not be updated: {err}",
                    state.groups_registry_path.display()
                ),
            )
        })?;
    }

    Ok(response)
}

fn submit_prepare_enclosure_request(
    state: &StandaloneEnclosureAdminRouteState,
    request: StandaloneEnclosurePrepareDaemonRequest,
) -> Result<StandaloneEnclosurePrepareResponse, (StatusCode, Json<AuthRouteError>)> {
    let client = state.enclosure_admin_client.as_ref().ok_or_else(|| {
        route_error(
            StatusCode::NOT_IMPLEMENTED,
            "daemon_enclosure_admin_unavailable",
            "daemon enclosure preparation contract is not available",
        )
    })?;

    client.submit_prepare_enclosure(request).map_err(|err| {
        route_error(
            StatusCode::BAD_GATEWAY,
            "daemon_enclosure_prepare_failed",
            err.message,
        )
    })
}

fn submit_create_object_store_request(
    state: &StandaloneEnclosureAdminRouteState,
    request: DaemonCreateObjectStoreRequest,
) -> Result<StandaloneCreateObjectStoreResponse, (StatusCode, Json<AuthRouteError>)> {
    let client = state.enclosure_admin_client.as_ref().ok_or_else(|| {
        route_error(
            StatusCode::NOT_IMPLEMENTED,
            "daemon_objectstore_admin_unavailable",
            "daemon ObjectStore administration contract is not available",
        )
    })?;

    client.submit_create_object_store(request).map_err(|err| {
        route_error(
            StatusCode::BAD_GATEWAY,
            "daemon_objectstore_create_failed",
            err.message,
        )
    })
}

fn submit_endpoint_inventory_upsert_request(
    state: &StandaloneEnclosureAdminRouteState,
    request: DaemonUpsertEndpointInventoryRequest,
) -> Result<StandaloneEndpointInventoryUpsertResponse, (StatusCode, Json<AuthRouteError>)> {
    let client = state.enclosure_admin_client.as_ref().ok_or_else(|| {
        route_error(
            StatusCode::NOT_IMPLEMENTED,
            "daemon_endpoint_admin_unavailable",
            "daemon endpoint inventory administration contract is not available",
        )
    })?;

    client
        .submit_endpoint_inventory_upsert(request)
        .map_err(|err| {
            route_error(
                StatusCode::BAD_GATEWAY,
                "daemon_endpoint_inventory_upsert_failed",
                err.message,
            )
        })
}

fn submit_admin_job_status_request(
    state: &StandaloneEnclosureAdminRouteState,
    request: StandaloneAdminJobStatusDaemonRequest,
) -> Result<StandaloneAdminJobStatusResponse, (StatusCode, Json<AuthRouteError>)> {
    let client = state.enclosure_admin_client.as_ref().ok_or_else(|| {
        route_error(
            StatusCode::NOT_IMPLEMENTED,
            "daemon_admin_jobs_unavailable",
            "daemon administrator job status contract is not available",
        )
    })?;

    client.job_status(request).map_err(|err| {
        route_error(
            StatusCode::BAD_GATEWAY,
            "daemon_admin_job_status_failed",
            err.message,
        )
    })
}

fn submit_admin_job_cancel_request(
    state: &StandaloneEnclosureAdminRouteState,
    request: StandaloneAdminJobCancelDaemonRequest,
) -> Result<StandaloneAdminJobCancelResponse, (StatusCode, Json<AuthRouteError>)> {
    let client = state.enclosure_admin_client.as_ref().ok_or_else(|| {
        route_error(
            StatusCode::NOT_IMPLEMENTED,
            "daemon_admin_jobs_unavailable",
            "daemon administrator job cancellation contract is not available",
        )
    })?;

    client.cancel_job(request).map_err(|err| {
        route_error(
            StatusCode::BAD_GATEWAY,
            "daemon_admin_job_cancel_failed",
            err.message,
        )
    })
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

fn validate_prepare_enclosure_confirmation_marker(
    dry_run: bool,
    allow_format: bool,
    existing_data_acknowledged: bool,
    confirmation_marker: Option<&str>,
) -> Result<String, (StatusCode, Json<AuthRouteError>)> {
    let confirmation_marker = confirmation_marker
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if dry_run && confirmation_marker.is_none() {
        return Ok(ENCLOSURE_PREPARE_CONFIRMATION.to_string());
    }
    if !allow_format {
        return Err(route_error(
            StatusCode::BAD_REQUEST,
            "format_allowance_required",
            "allow_format must be true before enclosure preparation can be submitted",
        ));
    }
    if !existing_data_acknowledged {
        return Err(route_error(
            StatusCode::BAD_REQUEST,
            "existing_data_acknowledgement_required",
            "existing_data_acknowledged must be true before enclosure preparation can be submitted",
        ));
    }
    if confirmation_marker == Some(ENCLOSURE_PREPARE_CONFIRMATION) {
        return Ok(ENCLOSURE_PREPARE_CONFIRMATION.to_string());
    }

    Err(route_error(
        StatusCode::BAD_REQUEST,
        "confirmation_required",
        format!("confirmation_marker must be `{ENCLOSURE_PREPARE_CONFIRMATION}`"),
    ))
}

fn validate_object_store_create_confirmation_marker(
    dry_run: bool,
    confirmation_marker: Option<&str>,
) -> Result<String, (StatusCode, Json<AuthRouteError>)> {
    let confirmation_marker = confirmation_marker
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if dry_run && confirmation_marker.is_none() {
        return Ok(OBJECT_STORE_CREATE_CONFIRMATION.to_string());
    }
    if confirmation_marker == Some(OBJECT_STORE_CREATE_CONFIRMATION) {
        return Ok(OBJECT_STORE_CREATE_CONFIRMATION.to_string());
    }

    Err(route_error(
        StatusCode::BAD_REQUEST,
        "confirmation_required",
        format!("confirmation_marker must be `{OBJECT_STORE_CREATE_CONFIRMATION}`"),
    ))
}

fn validate_endpoint_inventory_confirmation_marker(
    dry_run: bool,
    confirmation_marker: Option<&str>,
) -> Result<String, (StatusCode, Json<AuthRouteError>)> {
    let confirmation_marker = confirmation_marker
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if dry_run && confirmation_marker.is_none() {
        return Ok(ENDPOINT_RECORD_CONFIRMATION.to_string());
    }
    if confirmation_marker == Some(ENDPOINT_RECORD_CONFIRMATION) {
        return Ok(ENDPOINT_RECORD_CONFIRMATION.to_string());
    }

    Err(route_error(
        StatusCode::BAD_REQUEST,
        "confirmation_required",
        format!("confirmation_marker must be `{ENDPOINT_RECORD_CONFIRMATION}`"),
    ))
}

fn parse_prepare_enclosure_filesystem(
    value: Option<&str>,
) -> Result<DaemonPrepareEnclosureFilesystem, (StatusCode, Json<AuthRouteError>)> {
    match value.unwrap_or("ext4").trim().to_ascii_lowercase().as_str() {
        "ext4" => Ok(DaemonPrepareEnclosureFilesystem::Ext4),
        "xfs" => Ok(DaemonPrepareEnclosureFilesystem::Xfs),
        other => Err(route_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            format!("filesystem must be ext4 or xfs: {other}"),
        )),
    }
}

fn parse_endpoint_kind(
    value: &str,
) -> Result<DaemonEndpointKind, (StatusCode, Json<AuthRouteError>)> {
    match value.trim().to_ascii_lowercase().as_str() {
        "dasobjectstore_das" => Ok(DaemonEndpointKind::DasobjectstoreDas),
        "dasobjectstore_nfs" => Ok(DaemonEndpointKind::DasobjectstoreNfs),
        "s3_compatible" => Ok(DaemonEndpointKind::S3Compatible),
        other => Err(route_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            format!(
                "kind must be dasobjectstore_das, dasobjectstore_nfs, or s3_compatible: {other}"
            ),
        )),
    }
}

fn parse_endpoint_validation_state(
    value: &str,
) -> Result<DaemonEndpointValidationState, (StatusCode, Json<AuthRouteError>)> {
    match value.trim().to_ascii_lowercase().as_str() {
        "draft" => Ok(DaemonEndpointValidationState::Draft),
        "pending_validation" => Ok(DaemonEndpointValidationState::PendingValidation),
        "validated" => Ok(DaemonEndpointValidationState::Validated),
        "degraded" => Ok(DaemonEndpointValidationState::Degraded),
        "rejected" => Ok(DaemonEndpointValidationState::Rejected),
        "unknown" => Ok(DaemonEndpointValidationState::Unknown),
        other => Err(route_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            format!(
                "validation.state must be draft, pending_validation, validated, degraded, rejected, or unknown: {other}"
            ),
        )),
    }
}

fn parse_endpoint_binding_readiness(
    value: &str,
) -> Result<DaemonEndpointBindingReadiness, (StatusCode, Json<AuthRouteError>)> {
    match value.trim().to_ascii_lowercase().as_str() {
        "ready" => Ok(DaemonEndpointBindingReadiness::Ready),
        "degraded" => Ok(DaemonEndpointBindingReadiness::Degraded),
        "blocked" => Ok(DaemonEndpointBindingReadiness::Blocked),
        other => Err(route_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            format!("active_bindings.readiness must be ready, degraded, or blocked: {other}"),
        )),
    }
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

fn enclosure_prepare_response_from_daemon(
    response: DaemonPrepareEnclosureResponse,
) -> StandaloneEnclosurePrepareResponse {
    StandaloneEnclosurePrepareResponse {
        accepted: StandaloneEnclosurePrepareAcceptedResponse {
            job_id: response.accepted.job_id.to_string(),
            kind: "enclosure_preparation".to_string(),
            accepted_at_utc: response.accepted.accepted_at_utc,
            dry_run: response.accepted.dry_run,
        },
        ssd_device: response.ssd_device.display().to_string(),
        hdd_devices: response
            .hdd_devices
            .into_iter()
            .map(|device| PrepareEnclosureHddDeviceRequest {
                disk_id: device.disk_id,
                device_path: device.device_path.display().to_string(),
            })
            .collect(),
        mount_root: response.mount_root.display().to_string(),
        filesystem: response.filesystem.to_string(),
        owner: response.owner,
        administrator_actor: response.administrator_actor,
        client_request_id: None,
    }
}

fn create_object_store_response_from_daemon(
    response: DaemonCreateObjectStoreResponse,
) -> StandaloneCreateObjectStoreResponse {
    StandaloneCreateObjectStoreResponse {
        accepted: StandaloneCreateObjectStoreAcceptedResponse {
            job_id: response.accepted.job_id.to_string(),
            kind: "object_store_creation".to_string(),
            accepted_at_utc: response.accepted.accepted_at_utc,
            dry_run: response.accepted.dry_run,
        },
        store_id: response.store_id,
        store_class: response.store_class,
        required_copies: response.required_copies,
        bucket: response.bucket,
        reader_group: response.reader_group,
        writer_group: response.writer_group,
        ssd_root: response.ssd_root.display().to_string(),
        object_type: response.object_type,
        enclosure_id: response.enclosure_id,
        public: response.public,
        writeable: response.writeable,
        capacity_behavior: response.capacity_behavior,
        retention: response.retention,
        endpoint_export_mode: response.endpoint_export_mode,
        administrator_actor: response.administrator_actor,
        client_request_id: None,
    }
}

fn endpoint_inventory_upsert_response_from_daemon(
    response: DaemonUpsertEndpointInventoryResponse,
) -> StandaloneEndpointInventoryUpsertResponse {
    StandaloneEndpointInventoryUpsertResponse {
        accepted: StandaloneEndpointInventoryAcceptedResponse {
            job_id: response.accepted.job_id.to_string(),
            kind: "endpoint_validation".to_string(),
            accepted_at_utc: response.accepted.accepted_at_utc,
            dry_run: response.accepted.dry_run,
        },
        endpoint_id: response.endpoint_id,
        display_name: response.display_name,
        kind: endpoint_kind_label(response.kind).to_string(),
        validation_state: endpoint_validation_state_label(response.validation_state).to_string(),
        registry_path: response.registry_path,
        administrator_actor: response.administrator_actor,
        client_request_id: None,
    }
}

fn endpoint_kind_label(kind: DaemonEndpointKind) -> &'static str {
    match kind {
        DaemonEndpointKind::DasobjectstoreDas => "dasobjectstore_das",
        DaemonEndpointKind::DasobjectstoreNfs => "dasobjectstore_nfs",
        DaemonEndpointKind::S3Compatible => "s3_compatible",
    }
}

fn endpoint_validation_state_label(state: DaemonEndpointValidationState) -> &'static str {
    match state {
        DaemonEndpointValidationState::Draft => "draft",
        DaemonEndpointValidationState::PendingValidation => "pending_validation",
        DaemonEndpointValidationState::Validated => "validated",
        DaemonEndpointValidationState::Degraded => "degraded",
        DaemonEndpointValidationState::Rejected => "rejected",
        DaemonEndpointValidationState::Unknown => "unknown",
    }
}

fn admin_job_status_response_from_daemon(
    response: DaemonJobStatusResponse,
) -> StandaloneAdminJobStatusResponse {
    StandaloneAdminJobStatusResponse {
        job: admin_job_summary_from_daemon(response.job),
    }
}

fn admin_job_summary_from_daemon(job: DaemonJobSummary) -> StandaloneAdminJobSummary {
    let percent_complete = job.progress.percent_complete();
    StandaloneAdminJobSummary {
        job_id: job.job_id.to_string(),
        kind: admin_job_kind_label(job.kind).to_string(),
        state: admin_job_state_label(job.state).to_string(),
        progress: admin_job_progress_from_daemon(job.progress),
        percent_complete,
        submitted_at_utc: job.submitted_at_utc,
        updated_at_utc: job.updated_at_utc,
        actor: job.actor,
        failure_message: job.failure_message,
    }
}

fn admin_job_progress_from_daemon(progress: DaemonJobProgress) -> StandaloneAdminJobProgress {
    StandaloneAdminJobProgress {
        stage: progress.stage,
        work_bytes_done: progress.work_bytes_done,
        work_bytes_total: progress.work_bytes_total,
        work_units_done: progress.work_units_done,
        work_units_total: progress.work_units_total,
        message: progress.message,
    }
}

fn admin_job_cancel_response_from_daemon(
    response: DaemonJobCancelResponse,
) -> StandaloneAdminJobCancelResponse {
    StandaloneAdminJobCancelResponse {
        job_id: response.job_id.to_string(),
        accepted: response.accepted,
        state: admin_job_state_label(response.state).to_string(),
    }
}

fn admin_job_kind_label(kind: DaemonJobKind) -> &'static str {
    match kind {
        DaemonJobKind::IngestFiles => "ingest_files",
        DaemonJobKind::DirectImport => "direct_import",
        DaemonJobKind::DiskDrain => "disk_drain",
        DaemonJobKind::DiskRetire => "disk_retire",
        DaemonJobKind::DiskReplace => "disk_replace",
        DaemonJobKind::EnclosurePreparation => "enclosure_preparation",
        DaemonJobKind::EndpointValidation => "endpoint_validation",
        DaemonJobKind::ObjectStoreCreation => "object_store_creation",
        DaemonJobKind::Repair => "repair",
        DaemonJobKind::ServiceOperation => "service_operation",
        DaemonJobKind::SystemAdministration => "system_administration",
    }
}

fn admin_job_state_label(state: DaemonJobState) -> &'static str {
    match state {
        DaemonJobState::Queued => "queued",
        DaemonJobState::Running => "running",
        DaemonJobState::Waiting => "waiting",
        DaemonJobState::Complete => "complete",
        DaemonJobState::Failed => "failed",
        DaemonJobState::Cancelled => "cancelled",
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

fn standalone_enclosure_admin_client_error(
    err: dasobjectstore_daemon::DaemonClientError,
) -> StandaloneEnclosureAdminClientError {
    StandaloneEnclosureAdminClientError {
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
        gui_api_router_for_host_mode, standalone_auth_router_with_state,
        standalone_dashboard_router_with_state, standalone_easyconnect_router_with_state,
        standalone_enclosure_admin_router_with_state, standalone_reporting_router_with_state,
        standalone_users_groups_router_with_state, AssignLocalUserToGroupRequest,
        CancelAdminJobRequest, CreateLocalGroupRequest, CreateObjectStoreRequest,
        DaemonCreateObjectStoreRequest, DaemonEndpointBinding, DaemonEndpointBindingReadiness,
        DaemonEndpointKind, DaemonEndpointValidation, DaemonEndpointValidationState,
        DaemonUpsertEndpointInventoryRequest, EndpointBindingUpsertRequest,
        EndpointInventoryUpsertRequest, EndpointValidationUpsertRequest, GuiApiHostMode,
        LocalPasswordAuthenticator, LocalUserAuthorityProvider, LoginRequest, LogoutRequest,
        PrepareEnclosureHddDeviceRequest, PrepareEnclosureRequest, RegisterRequest,
        SessionCheckRequest, StandaloneAdminJobCancelDaemonRequest,
        StandaloneAdminJobCancelResponse, StandaloneAdminJobProgress,
        StandaloneAdminJobStatusDaemonRequest, StandaloneAdminJobStatusResponse,
        StandaloneAdminJobSummary, StandaloneAuthRouteState,
        StandaloneCreateObjectStoreAcceptedResponse, StandaloneCreateObjectStoreResponse,
        StandaloneDashboardRouteState, StandaloneEasyconnectRouteState,
        StandaloneEnclosureAdminClient, StandaloneEnclosureAdminClientError,
        StandaloneEnclosureAdminRouteState, StandaloneEnclosurePrepareAcceptedResponse,
        StandaloneEnclosurePrepareDaemonRequest, StandaloneEnclosurePrepareResponse,
        StandaloneEndpointInventoryAcceptedResponse, StandaloneEndpointInventoryUpsertResponse,
        StandaloneLocalGroupAdminAcceptedResponse, StandaloneLocalGroupAdminClient,
        StandaloneLocalGroupAdminClientError, StandaloneLocalGroupAdminDaemonRequest,
        StandaloneLocalGroupAdminResponse, StandaloneLocalGroupOperation,
        StandaloneReportingRouteState, StandaloneUsersGroupsRouteState,
        ENCLOSURE_PREPARE_CONFIRMATION, ENDPOINT_RECORD_CONFIRMATION,
        LOCAL_ADMIN_CONFIRMATION_MARKER, OBJECT_STORE_CREATE_CONFIRMATION,
    };
    use crate::{
        LocalAuthStore, LocalPasswordAuthError, LocalUserDiscoveryError, LocalUserMetadata,
        LoginResponse, STANDALONE_SESSION_TOKEN_HEADER, STANDALONE_USERNAME_HEADER,
    };
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_core::store::{
        CapacityBehavior, ExportPolicy, RetentionPolicy, StoreClass, StorePolicy,
    };
    use dasobjectstore_object_service::StoreServiceDefinition;
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
        let app = test_auth_router(auth_store, vec![("admin", "secret")]);

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
        let app = test_auth_router(auth_store, vec![("admin", "secret")]);

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
        let app = test_auth_router(auth_store.clone(), vec![("admin", "secret")]);
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
        let app = test_auth_router(auth_store, vec![("admin", "secret")]);
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
    async fn login_route_accepts_os_authenticated_user_without_product_registration() {
        let root = temp_root("os-login-without-product-registration");
        let auth_store = LocalAuthStore::new(&root);
        let app = test_auth_router(auth_store.clone(), vec![("stephen", "secret")]);

        let login = post_json::<LoginResponse>(
            app.clone(),
            "/api/login",
            &LoginRequest {
                username: "stephen".to_string(),
                password: "secret".to_string(),
                session_ttl_seconds: Some(3_600),
            },
        )
        .await;
        let session = auth_store
            .verify_session("stephen", &login.session_token)
            .expect("session verifies");

        assert_eq!(login.username, "stephen");
        assert!(session.valid);

        cleanup(&root);
    }

    #[tokio::test]
    async fn standalone_easyconnect_discovery_advertises_local_user_auth() {
        let root = temp_root("easyconnect-discovery");
        let auth_store = LocalAuthStore::new(&root);
        let app = standalone_easyconnect_router_with_state(StandaloneEasyconnectRouteState {
            auth_store,
            public_base_url: "https://192.168.1.192:8448".to_string(),
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/remote/easyconnect/discovery")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("request completes");

        assert_eq!(response.status(), StatusCode::OK);
        let encoded = response_json(response).await;

        assert_eq!(
            encoded["auth_providers"],
            serde_json::json!(["standalone_local_user"])
        );
        assert_eq!(encoded["default_session_lifetime_seconds"], 28_800);
        assert_eq!(
            encoded["session_policy"]["renewal_requires_password_reauthentication"],
            false
        );
        assert_eq!(
            encoded["pairing_create_url"],
            "https://192.168.1.192:8448/products/dasobjectstore/api/v1/remote/easyconnect/pairings"
        );

        cleanup(&root);
    }

    #[tokio::test]
    async fn standalone_easyconnect_auth_context_requires_session() {
        let root = temp_root("easyconnect-auth-required");
        let auth_store = registered_auth_store(&root);
        let app = standalone_easyconnect_router_with_state(StandaloneEasyconnectRouteState {
            auth_store,
            public_base_url: "https://192.168.1.192:8448".to_string(),
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/remote/easyconnect/auth-context")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("request completes");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        cleanup(&root);
    }

    #[tokio::test]
    async fn standalone_easyconnect_auth_context_uses_authenticated_local_user() {
        let root = temp_root("easyconnect-auth-context");
        let auth_store = registered_auth_store_for_user(&root, "stephen");
        let login = auth_store
            .login("stephen", "secret")
            .expect("login succeeds");
        let app = standalone_easyconnect_router_with_state(StandaloneEasyconnectRouteState {
            auth_store,
            public_base_url: "https://192.168.1.192:8448".to_string(),
        });

        let encoded = get_json_with_session::<serde_json::Value>(
            app,
            "/api/v1/remote/easyconnect/auth-context",
            "stephen",
            &login.session_token,
        )
        .await;

        assert_eq!(
            encoded["schema_version"],
            "dasobjectstore.remote_easyconnect.auth_context.v1"
        );
        assert_eq!(encoded["auth_provider"], "standalone_local_user");
        assert_eq!(encoded["subject_id"], "stephen");
        assert_eq!(
            encoded["supported_auth_providers"],
            serde_json::json!(["standalone_local_user"])
        );
        assert_eq!(
            encoded["future_auth_providers"],
            serde_json::json!(["synoptikon", "mneion"])
        );

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
    async fn standalone_users_groups_workspace_uses_authenticated_local_username_for_authority() {
        let root = temp_root("standalone-users-groups-stephen-admin");
        let auth_store = registered_auth_store_for_user(&root, "stephen");
        let login = auth_store
            .login("stephen", "secret")
            .expect("login succeeds");
        let app =
            standalone_users_groups_router_with_state(test_users_groups_state_with_groups_path(
                auth_store,
                local_user("dasobjectstore", vec!["sudo"]),
                None,
                root_groups_path("stephen-admin"),
            ));

        let encoded = get_json_with_session::<serde_json::Value>(
            app,
            "/api/v1/workspaces/users-groups",
            "stephen",
            &login.session_token,
        )
        .await;

        assert_eq!(encoded["current_user"]["username"], "stephen");
        assert_eq!(encoded["current_user"]["sudo_administrator"], true);
        assert_eq!(
            encoded["capabilities"]["administrator_actions_enabled"],
            true
        );
        assert_eq!(encoded["operations"][0]["enabled"], true);

        cleanup(&root);
    }

    #[tokio::test]
    async fn standalone_dashboards_use_authenticated_local_username_for_admin_affordances() {
        let root = temp_root("standalone-dashboard-stephen-admin");
        let auth_store = registered_auth_store_for_user(&root, "stephen");
        let login = auth_store
            .login("stephen", "secret")
            .expect("login succeeds");
        let app = standalone_dashboard_router_with_state(StandaloneDashboardRouteState {
            auth_store,
            local_user_provider: Arc::new(FixedLocalUserProvider {
                current_user: local_user("dasobjectstore", vec!["sudo"]),
            }),
        });

        let enclosures = get_json_with_session::<serde_json::Value>(
            app.clone(),
            "/api/v1/dashboard/enclosures",
            "stephen",
            &login.session_token,
        )
        .await;
        let object_stores = get_json_with_session::<serde_json::Value>(
            app,
            "/api/v1/dashboard/object-stores",
            "stephen",
            &login.session_token,
        )
        .await;

        assert_eq!(enclosures["add_enclosure"]["administrator"], true);
        assert_ne!(enclosures["add_enclosure"]["state"], "admin_required");
        assert_eq!(object_stores["create_object_store"]["enabled"], true);

        cleanup(&root);
    }

    #[tokio::test]
    async fn standalone_users_groups_workspace_returns_managed_writer_groups() {
        let root = temp_root("standalone-users-groups-writer-policy");
        fs::create_dir_all(&root).expect("temp root");
        let groups_path = root.join("groups.json");
        fs::write(
            &groups_path,
            r#"{"groups":[{"group_name":"bioinformatics","display_name":"Bioinformatics","source":"local_os"}]}"#,
        )
        .expect("groups write");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let app =
            standalone_users_groups_router_with_state(test_users_groups_state_with_groups_path(
                auth_store,
                local_user("operator", vec!["bioinformatics"]),
                None,
                groups_path.clone(),
            ));

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

        assert_eq!(
            encoded["groups_file_path"],
            groups_path.display().to_string()
        );
        assert_eq!(encoded["writer_groups"][0]["group_name"], "bioinformatics");
        assert_eq!(encoded["writer_groups"][0]["current_user_member"], true);

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
                administrator_actor: Some("admin".to_string()),
                confirmation_marker: LOCAL_ADMIN_CONFIRMATION_MARKER.to_string(),
            }]
        );

        cleanup(&root);
    }

    #[tokio::test]
    async fn create_local_group_live_reconciles_writer_group_registry() {
        let root = temp_root("create-local-group-registry");
        let groups_path = root.join("groups.json");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let client = recording_admin_client();
        let app =
            standalone_users_groups_router_with_state(test_users_groups_state_with_groups_path(
                auth_store,
                local_user("operator", vec!["sudo"]),
                Some(client.clone()),
                groups_path.clone(),
            ));

        let response = post_json_with_session::<StandaloneLocalGroupAdminResponse>(
            app,
            "/api/v1/workspaces/users-groups/local-groups",
            "admin",
            &login.session_token,
            &CreateLocalGroupRequest {
                group_name: "mnemosyne".to_string(),
                dry_run: false,
                confirmation_marker: Some(LOCAL_ADMIN_CONFIRMATION_MARKER.to_string()),
                client_request_id: Some("request-live-group".to_string()),
            },
        )
        .await;
        let encoded: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&groups_path).expect("groups file exists"))
                .expect("groups registry decodes");

        assert_eq!(
            response.operation,
            StandaloneLocalGroupOperation::CreateGroup
        );
        assert_eq!(encoded["groups"][0]["group_name"], "mnemosyne");
        assert_eq!(encoded["groups"][0]["source"], "local_os");
        assert_eq!(
            client.requests()[0].confirmation_marker,
            LOCAL_ADMIN_CONFIRMATION_MARKER
        );

        cleanup(&root);
    }

    #[tokio::test]
    async fn assign_local_user_live_reconciles_existing_writer_group_registry() {
        let root = temp_root("assign-local-user-registry");
        let groups_path = root.join("groups.json");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let app =
            standalone_users_groups_router_with_state(test_users_groups_state_with_groups_path(
                auth_store,
                local_user("operator", vec!["sudo"]),
                Some(recording_admin_client()),
                groups_path.clone(),
            ));

        let response = post_json_with_session::<StandaloneLocalGroupAdminResponse>(
            app,
            "/api/v1/workspaces/users-groups/local-groups/members",
            "admin",
            &login.session_token,
            &AssignLocalUserToGroupRequest {
                group_name: "mnemosyne".to_string(),
                username: "stephen".to_string(),
                dry_run: false,
                confirmation_marker: Some(LOCAL_ADMIN_CONFIRMATION_MARKER.to_string()),
                client_request_id: Some("request-live-member".to_string()),
            },
        )
        .await;
        let encoded: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&groups_path).expect("groups file exists"))
                .expect("groups registry decodes");

        assert_eq!(
            response.operation,
            StandaloneLocalGroupOperation::AddUserToGroup
        );
        assert_eq!(response.username.as_deref(), Some("stephen"));
        assert_eq!(encoded["groups"][0]["group_name"], "mnemosyne");

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
                administrator_actor: Some("admin".to_string()),
                confirmation_marker: LOCAL_ADMIN_CONFIRMATION_MARKER.to_string(),
            }]
        );

        cleanup(&root);
    }

    #[tokio::test]
    async fn prepare_enclosure_requires_session() {
        let root = temp_root("prepare-enclosure-auth");
        let auth_store = registered_auth_store(&root);
        let app = standalone_enclosure_admin_router_with_state(test_enclosure_admin_state(
            auth_store,
            local_user("operator", vec!["sudo"]),
            Some(recording_enclosure_client()),
        ));

        let response = post_json_response(
            app,
            "/api/v1/workspaces/enclosures/prepare",
            &prepare_enclosure_request(),
        )
        .await;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        cleanup(&root);
    }

    #[tokio::test]
    async fn prepare_enclosure_rejects_non_admin_os_user() {
        let root = temp_root("prepare-enclosure-non-admin");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let app = standalone_enclosure_admin_router_with_state(test_enclosure_admin_state(
            auth_store,
            local_user("operator", vec!["users"]),
            Some(recording_enclosure_client()),
        ));

        let response = post_json_response_with_session(
            app,
            "/api/v1/workspaces/enclosures/prepare",
            "admin",
            &login.session_token,
            &prepare_enclosure_request(),
        )
        .await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        cleanup(&root);
    }

    #[tokio::test]
    async fn prepare_enclosure_rejects_unsupported_empty_hdd_set() {
        let root = temp_root("prepare-enclosure-no-hdd");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let app = standalone_enclosure_admin_router_with_state(test_enclosure_admin_state(
            auth_store,
            local_user("operator", vec!["sudo"]),
            Some(recording_enclosure_client()),
        ));
        let request = PrepareEnclosureRequest {
            hdd_devices: Vec::new(),
            ..prepare_enclosure_request()
        };

        let response = post_json_response_with_session(
            app,
            "/api/v1/workspaces/enclosures/prepare",
            "admin",
            &login.session_token,
            &request,
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let encoded = response_json(response).await;
        assert_eq!(encoded["code"], "unsupported_das");

        cleanup(&root);
    }

    #[tokio::test]
    async fn prepare_enclosure_rejects_already_managed_mount_root() {
        let root = temp_root("prepare-enclosure-known-root");
        let managed_root = root.join("managed");
        fs::create_dir_all(managed_root.join("ssd/.dasobjectstore")).expect("managed marker dir");
        fs::write(
            managed_root.join("ssd/.dasobjectstore/device.env"),
            "role=ssd\ndevice=/dev/disk/by-id/nvme-existing\nfilesystem=ext4\n",
        )
        .expect("managed marker");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let client = recording_enclosure_client();
        let app = standalone_enclosure_admin_router_with_state(test_enclosure_admin_state(
            auth_store,
            local_user("operator", vec!["sudo"]),
            Some(client.clone()),
        ));
        let request = PrepareEnclosureRequest {
            mount_root: Some(managed_root.display().to_string()),
            ..prepare_enclosure_request()
        };

        let response = post_json_response_with_session(
            app,
            "/api/v1/workspaces/enclosures/prepare",
            "admin",
            &login.session_token,
            &request,
        )
        .await;

        assert_eq!(response.status(), StatusCode::CONFLICT);
        let encoded = response_json(response).await;
        assert_eq!(encoded["code"], "enclosure_already_managed");
        assert!(client.requests().is_empty());

        cleanup(&root);
    }

    #[tokio::test]
    async fn prepare_enclosure_requires_format_allowance_and_confirmation() {
        let root = temp_root("prepare-enclosure-confirm");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let app = standalone_enclosure_admin_router_with_state(test_enclosure_admin_state(
            auth_store,
            local_user("operator", vec!["sudo"]),
            Some(recording_enclosure_client()),
        ));
        let request = PrepareEnclosureRequest {
            allow_format: false,
            confirmation_marker: None,
            ..prepare_enclosure_request()
        };

        let response = post_json_response_with_session(
            app,
            "/api/v1/workspaces/enclosures/prepare",
            "admin",
            &login.session_token,
            &request,
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let encoded = response_json(response).await;
        assert_eq!(encoded["code"], "format_allowance_required");

        cleanup(&root);
    }

    #[tokio::test]
    async fn prepare_enclosure_requires_existing_data_acknowledgement() {
        let root = temp_root("prepare-enclosure-existing-data");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let app = standalone_enclosure_admin_router_with_state(test_enclosure_admin_state(
            auth_store,
            local_user("operator", vec!["sudo"]),
            Some(recording_enclosure_client()),
        ));
        let request = PrepareEnclosureRequest {
            existing_data_acknowledged: false,
            ..prepare_enclosure_request()
        };

        let response = post_json_response_with_session(
            app,
            "/api/v1/workspaces/enclosures/prepare",
            "admin",
            &login.session_token,
            &request,
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let encoded = response_json(response).await;
        assert_eq!(encoded["code"], "existing_data_acknowledgement_required");

        cleanup(&root);
    }

    #[tokio::test]
    async fn prepare_enclosure_forwards_confirmed_request_to_daemon_client() {
        let root = temp_root("prepare-enclosure-forward");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let client = recording_enclosure_client();
        let app = standalone_enclosure_admin_router_with_state(test_enclosure_admin_state(
            auth_store,
            local_user("operator", vec!["wheel"]),
            Some(client.clone()),
        ));

        let response = post_json_with_session::<StandaloneEnclosurePrepareResponse>(
            app,
            "/api/v1/workspaces/enclosures/prepare",
            "admin",
            &login.session_token,
            &prepare_enclosure_request(),
        )
        .await;

        assert_eq!(response.accepted.job_id, "enclosure-prepare-job-1");
        assert_eq!(response.accepted.kind, "enclosure_preparation");
        assert_eq!(response.hdd_devices.len(), 1);
        assert_eq!(
            client.requests(),
            vec![StandaloneEnclosurePrepareDaemonRequest {
                ssd_device: "/dev/disk/by-id/nvme-ssd".to_string(),
                hdd_devices: vec![PrepareEnclosureHddDeviceRequest {
                    disk_id: "qnap-1057".to_string(),
                    device_path: "/dev/disk/by-id/usb-qnap-1057".to_string(),
                }],
                mount_root: "/srv/dasobjectstore".to_string(),
                filesystem: dasobjectstore_daemon::PrepareEnclosureFilesystem::Ext4,
                owner: Some("stephen".to_string()),
                dry_run: false,
                client_request_id: Some("prepare-1".to_string()),
                administrator_actor: Some("admin".to_string()),
                allow_format: true,
                existing_data_acknowledged: true,
                confirmation_marker: ENCLOSURE_PREPARE_CONFIRMATION.to_string(),
            }]
        );

        cleanup(&root);
    }

    #[tokio::test]
    async fn prepare_enclosure_surfaces_daemon_failure() {
        let root = temp_root("prepare-enclosure-daemon-failure");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let client = recording_enclosure_client_with_failure("daemon refused preparation");
        let app = standalone_enclosure_admin_router_with_state(test_enclosure_admin_state(
            auth_store,
            local_user("operator", vec!["sudo"]),
            Some(client),
        ));

        let response = post_json_response_with_session(
            app,
            "/api/v1/workspaces/enclosures/prepare",
            "admin",
            &login.session_token,
            &prepare_enclosure_request(),
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let encoded = response_json(response).await;
        assert_eq!(encoded["code"], "daemon_enclosure_prepare_failed");
        assert!(encoded["message"]
            .as_str()
            .expect("message")
            .contains("daemon refused preparation"));

        cleanup(&root);
    }

    #[tokio::test]
    async fn create_object_store_requires_exact_confirmation() {
        let root = temp_root("objectstore-create-confirm");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let app = standalone_enclosure_admin_router_with_state(test_enclosure_admin_state(
            auth_store,
            local_user("operator", vec!["sudo"]),
            Some(recording_enclosure_client()),
        ));
        let request = CreateObjectStoreRequest {
            confirmation_marker: Some("create it".to_string()),
            ..create_object_store_request()
        };

        let response = post_json_response_with_session(
            app,
            "/api/v1/workspaces/object-stores/create",
            "admin",
            &login.session_token,
            &request,
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let encoded = response_json(response).await;
        assert_eq!(encoded["code"], "confirmation_required");

        cleanup(&root);
    }

    #[tokio::test]
    async fn create_object_store_forwards_confirmed_request_to_daemon_client() {
        let root = temp_root("objectstore-create-forward");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let client = recording_enclosure_client();
        let app = standalone_enclosure_admin_router_with_state(test_enclosure_admin_state(
            auth_store,
            local_user("operator", vec!["wheel"]),
            Some(client.clone()),
        ));

        let response = post_json_with_session::<StandaloneCreateObjectStoreResponse>(
            app,
            "/api/v1/workspaces/object-stores/create",
            "admin",
            &login.session_token,
            &create_object_store_request(),
        )
        .await;

        assert_eq!(response.accepted.job_id, "objectstore-create-job-1");
        assert_eq!(response.accepted.kind, "object_store_creation");
        assert_eq!(response.store_id, "zymo-fecal-2025-05");
        assert_eq!(response.administrator_actor.as_deref(), Some("admin"));
        let forwarded_requests = client.create_object_store_requests();
        assert_eq!(
            forwarded_requests,
            vec![DaemonCreateObjectStoreRequest {
                store_id: "zymo-fecal-2025-05".to_string(),
                store_class: "generated_data".to_string(),
                required_copies: 2,
                bucket: Some("zymo-fecal-2025-05".to_string()),
                reader_group: None,
                writer_group: "bioinformatics".to_string(),
                ssd_root: PathBuf::from("/srv/dasobjectstore/ssd"),
                object_type: "pod5".to_string(),
                enclosure_id: Some("tl-d800c-01".to_string()),
                public: false,
                writeable: true,
                capacity_behavior: "balanced".to_string(),
                retention: "standard".to_string(),
                endpoint_export_mode: "s3_bucket".to_string(),
                dry_run: false,
                client_request_id: Some("objectstore-create-1".to_string()),
                administrator_actor: Some("admin".to_string()),
                confirmation_marker: OBJECT_STORE_CREATE_CONFIRMATION.to_string(),
            }]
        );
        assert_eq!(
            forwarded_requests[0]
                .registry_definition()
                .expect("registry definition projects"),
            StoreServiceDefinition {
                store_id: StoreId::new("zymo-fecal-2025-05").expect("store id"),
                policy: StorePolicy {
                    class: StoreClass::GeneratedData,
                    copies: 2,
                    capacity_behavior: CapacityBehavior::BackpressureByPriority,
                    retention_policy: RetentionPolicy::TombstoneThenGc,
                    export_policy: ExportPolicy::S3,
                    ..StorePolicy::defaults_for(StoreClass::GeneratedData)
                },
                bucket_name: Some("zymo-fecal-2025-05".to_string()),
                reader_group: None,
                writer_group: Some("bioinformatics".to_string()),
                public: false,
            }
        );

        cleanup(&root);
    }

    #[tokio::test]
    async fn create_object_store_derives_immutable_fields_from_minimal_request() {
        let root = temp_root("objectstore-create-derived");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let client = recording_enclosure_client();
        let app = standalone_enclosure_admin_router_with_state(test_enclosure_admin_state(
            auth_store,
            local_user("operator", vec!["sudo"]),
            Some(client.clone()),
        ));

        let response = post_json_with_session::<StandaloneCreateObjectStoreResponse>(
            app,
            "/api/v1/workspaces/object-stores/create",
            "admin",
            &login.session_token,
            &CreateObjectStoreRequest {
                store_id: "zymo-fecal-2025-05".to_string(),
                store_class: None,
                required_copies: 2,
                bucket: None,
                reader_group: None,
                writer_group: "mnemosyne".to_string(),
                ssd_root: None,
                object_type: None,
                enclosure_id: Some("qnap-tl-d800c-managed".to_string()),
                public: false,
                writeable: None,
                capacity_behavior: None,
                retention: None,
                endpoint_export_mode: None,
                dry_run: false,
                client_request_id: Some("objectstore-derived-1".to_string()),
                confirmation_marker: Some(OBJECT_STORE_CREATE_CONFIRMATION.to_string()),
            },
        )
        .await;
        let forwarded_requests = client.create_object_store_requests();

        assert_eq!(response.store_id, "zymo-fecal-2025-05");
        assert_eq!(forwarded_requests.len(), 1);
        assert_eq!(forwarded_requests[0].store_class, "generated_data");
        assert_eq!(
            forwarded_requests[0].bucket.as_deref(),
            Some("zymo-fecal-2025-05")
        );
        assert_eq!(
            forwarded_requests[0].ssd_root,
            PathBuf::from("/srv/dasobjectstore/ssd")
        );
        assert_eq!(forwarded_requests[0].object_type, "naive");
        assert_eq!(
            forwarded_requests[0].enclosure_id.as_deref(),
            Some("qnap-tl-d800c-managed")
        );
        assert!(forwarded_requests[0].writeable);
        assert_eq!(
            forwarded_requests[0].capacity_behavior,
            "backpressure_by_priority"
        );
        assert_eq!(forwarded_requests[0].retention, "retain_until_deleted");
        assert_eq!(forwarded_requests[0].endpoint_export_mode, "s3_bucket");

        cleanup(&root);
    }

    #[tokio::test]
    async fn create_object_store_rejects_invalid_domain_policy_values() {
        let root = temp_root("objectstore-create-invalid-policy");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let app = standalone_enclosure_admin_router_with_state(test_enclosure_admin_state(
            auth_store,
            local_user("operator", vec!["sudo"]),
            Some(recording_enclosure_client()),
        ));
        let request = CreateObjectStoreRequest {
            capacity_behavior: Some("fast".to_string()),
            ..create_object_store_request()
        };

        let response = post_json_response_with_session(
            app,
            "/api/v1/workspaces/object-stores/create",
            "admin",
            &login.session_token,
            &request,
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let encoded = response_json(response).await;
        assert_eq!(encoded["code"], "invalid_objectstore_policy");
        assert!(encoded["message"]
            .as_str()
            .expect("message")
            .contains("unsupported capacity_behavior"));

        cleanup(&root);
    }

    #[tokio::test]
    async fn endpoint_inventory_upsert_requires_session() {
        let root = temp_root("endpoint-upsert-auth");
        let auth_store = registered_auth_store(&root);
        let app = standalone_enclosure_admin_router_with_state(test_enclosure_admin_state(
            auth_store,
            local_user("operator", vec!["sudo"]),
            Some(recording_enclosure_client()),
        ));

        let response = post_json_response(
            app,
            "/api/v1/workspaces/endpoints/upsert",
            &endpoint_inventory_request(),
        )
        .await;

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        cleanup(&root);
    }

    #[tokio::test]
    async fn endpoint_inventory_upsert_requires_exact_confirmation() {
        let root = temp_root("endpoint-upsert-confirm");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let app = standalone_enclosure_admin_router_with_state(test_enclosure_admin_state(
            auth_store,
            local_user("operator", vec!["sudo"]),
            Some(recording_enclosure_client()),
        ));
        let request = EndpointInventoryUpsertRequest {
            confirmation_marker: Some("record it".to_string()),
            ..endpoint_inventory_request()
        };

        let response = post_json_response_with_session(
            app,
            "/api/v1/workspaces/endpoints/upsert",
            "admin",
            &login.session_token,
            &request,
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let encoded = response_json(response).await;
        assert_eq!(encoded["code"], "confirmation_required");

        cleanup(&root);
    }

    #[tokio::test]
    async fn endpoint_inventory_upsert_forwards_confirmed_request_to_daemon_client() {
        let root = temp_root("endpoint-upsert-forward");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let client = recording_enclosure_client();
        let app = standalone_enclosure_admin_router_with_state(test_enclosure_admin_state(
            auth_store,
            local_user("operator", vec!["wheel"]),
            Some(client.clone()),
        ));

        let response = post_json_with_session::<StandaloneEndpointInventoryUpsertResponse>(
            app,
            "/api/v1/workspaces/endpoints/upsert",
            "admin",
            &login.session_token,
            &endpoint_inventory_request(),
        )
        .await;

        assert_eq!(response.accepted.job_id, "endpoint-upsert-job-1");
        assert_eq!(response.accepted.kind, "endpoint_validation");
        assert_eq!(response.endpoint_id, "nas-staging");
        assert_eq!(response.kind, "dasobjectstore_nfs");
        assert_eq!(
            client.endpoint_inventory_requests(),
            vec![DaemonUpsertEndpointInventoryRequest {
                endpoint_id: "nas-staging".to_string(),
                display_name: "NAS staging".to_string(),
                kind: DaemonEndpointKind::DasobjectstoreNfs,
                object_service_url: "https://nas.example.test:9443".to_string(),
                validation: DaemonEndpointValidation {
                    state: DaemonEndpointValidationState::Validated,
                    checked_at_utc: Some("2026-07-09T00:00:00Z".to_string()),
                    message: Some("validated from Web admin workflow".to_string()),
                },
                manager_product_id: "dasobjectstore".to_string(),
                active_bindings: vec![DaemonEndpointBinding {
                    binding_id: "binding-1".to_string(),
                    governance_domain: "local".to_string(),
                    store_id: "zymo-fecal-2025-05".to_string(),
                    readiness: DaemonEndpointBindingReadiness::Ready,
                }],
                dry_run: false,
                client_request_id: Some("endpoint-upsert-1".to_string()),
                administrator_actor: Some("admin".to_string()),
                confirmation_marker: Some(ENDPOINT_RECORD_CONFIRMATION.to_string()),
            }]
        );

        cleanup(&root);
    }

    #[tokio::test]
    async fn performance_report_rebuild_requires_session() {
        let root = temp_root("performance-report-rebuild-session");
        let auth_store = registered_auth_store(&root);
        let app = standalone_reporting_router_with_state(test_reporting_state(
            auth_store,
            local_user("operator", vec!["sudo"]),
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/activity/reporting/performance-report")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .expect("request builds"),
            )
            .await
            .expect("request completes");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        cleanup(&root);
    }

    #[tokio::test]
    async fn performance_report_rebuild_rejects_wrong_schema_before_rendering() {
        let root = temp_root("performance-report-rebuild-schema");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let app = standalone_reporting_router_with_state(test_reporting_state(
            auth_store,
            local_user("operator", vec!["sudo"]),
        ));

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/v1/workspaces/activity/reporting/performance-report")
                    .header("content-type", "application/json")
                    .header(STANDALONE_USERNAME_HEADER, "admin")
                    .header(STANDALONE_SESSION_TOKEN_HEADER, login.session_token)
                    .header("x-dasobjectstore-filename", "wrong.json")
                    .body(Body::from(r#"{"schema":"wrong"}"#))
                    .expect("request builds"),
            )
            .await
            .expect("request completes");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response_json(response).await;
        assert_eq!(body["code"], "performance_report_rebuild_failed");
        assert!(body["message"]
            .as_str()
            .expect("message")
            .contains("unsupported benchmark JSON schema"));

        cleanup(&root);
    }

    #[tokio::test]
    async fn endpoint_inventory_upsert_rejects_invalid_endpoint_kind() {
        let root = temp_root("endpoint-upsert-invalid-kind");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let app = standalone_enclosure_admin_router_with_state(test_enclosure_admin_state(
            auth_store,
            local_user("operator", vec!["sudo"]),
            Some(recording_enclosure_client()),
        ));
        let request = EndpointInventoryUpsertRequest {
            kind: "nfs".to_string(),
            ..endpoint_inventory_request()
        };

        let response = post_json_response_with_session(
            app,
            "/api/v1/workspaces/endpoints/upsert",
            "admin",
            &login.session_token,
            &request,
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let encoded = response_json(response).await;
        assert_eq!(encoded["code"], "invalid_request");

        cleanup(&root);
    }

    #[tokio::test]
    async fn admin_job_status_requires_local_admin() {
        let root = temp_root("admin-job-status-non-admin");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let app = standalone_enclosure_admin_router_with_state(test_enclosure_admin_state(
            auth_store,
            local_user("operator", vec!["users"]),
            Some(recording_enclosure_client()),
        ));

        let response = get_response_with_session(
            app,
            "/api/v1/workspaces/admin/jobs/enclosure-prepare-1",
            "admin",
            &login.session_token,
        )
        .await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);

        cleanup(&root);
    }

    #[tokio::test]
    async fn admin_job_status_forwards_request_to_daemon_client() {
        let root = temp_root("admin-job-status-forward");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let client = recording_enclosure_client();
        let app = standalone_enclosure_admin_router_with_state(test_enclosure_admin_state(
            auth_store,
            local_user("operator", vec!["sudo"]),
            Some(client.clone()),
        ));

        let response = get_json_with_session::<StandaloneAdminJobStatusResponse>(
            app,
            "/api/v1/workspaces/admin/jobs/enclosure-prepare-1",
            "admin",
            &login.session_token,
        )
        .await;

        assert_eq!(response.job.job_id, "enclosure-prepare-1");
        assert_eq!(response.job.kind, "enclosure_preparation");
        assert_eq!(response.job.state, "running");
        assert_eq!(response.job.percent_complete, Some(50));
        assert_eq!(
            client.status_requests(),
            vec![StandaloneAdminJobStatusDaemonRequest {
                job_id: "enclosure-prepare-1".to_string(),
            }]
        );

        cleanup(&root);
    }

    #[tokio::test]
    async fn admin_job_cancel_rejects_blank_reason() {
        let root = temp_root("admin-job-cancel-blank-reason");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let app = standalone_enclosure_admin_router_with_state(test_enclosure_admin_state(
            auth_store,
            local_user("operator", vec!["sudo"]),
            Some(recording_enclosure_client()),
        ));

        let response = post_json_response_with_session(
            app,
            "/api/v1/workspaces/admin/jobs/enclosure-prepare-1/cancel",
            "admin",
            &login.session_token,
            &CancelAdminJobRequest {
                reason: Some(" ".to_string()),
            },
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let encoded = response_json(response).await;
        assert_eq!(encoded["code"], "invalid_request");

        cleanup(&root);
    }

    #[tokio::test]
    async fn admin_job_cancel_forwards_request_to_daemon_client() {
        let root = temp_root("admin-job-cancel-forward");
        let auth_store = registered_auth_store(&root);
        let login = auth_store.login("admin", "secret").expect("login succeeds");
        let client = recording_enclosure_client();
        let app = standalone_enclosure_admin_router_with_state(test_enclosure_admin_state(
            auth_store,
            local_user("operator", vec!["sudo"]),
            Some(client.clone()),
        ));

        let response = post_json_with_session::<StandaloneAdminJobCancelResponse>(
            app,
            "/api/v1/workspaces/admin/jobs/enclosure-prepare-1/cancel",
            "admin",
            &login.session_token,
            &CancelAdminJobRequest {
                reason: Some("operator requested cancellation".to_string()),
            },
        )
        .await;

        assert_eq!(response.job_id, "enclosure-prepare-1");
        assert!(response.accepted);
        assert_eq!(response.state, "cancelled");
        assert_eq!(
            client.cancel_requests(),
            vec![StandaloneAdminJobCancelDaemonRequest {
                job_id: "enclosure-prepare-1".to_string(),
                reason: Some("operator requested cancellation".to_string()),
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
    async fn synoptikon_integrated_host_mode_omits_standalone_easyconnect_routes() {
        let root = temp_root("integrated-easyconnect");
        let auth_store = LocalAuthStore::new(&root);
        let app = gui_api_router_for_host_mode(GuiApiHostMode::SynoptikonIntegrated, auth_store);

        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/v1/remote/easyconnect/auth-context")
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

    async fn get_json_with_session<T>(
        app: axum::Router,
        path: &str,
        username: &str,
        session_token: &str,
    ) -> T
    where
        T: DeserializeOwned,
    {
        let response = get_response_with_session(app, path, username, session_token).await;
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

    async fn get_response_with_session(
        app: axum::Router,
        path: &str,
        username: &str,
        session_token: &str,
    ) -> axum::response::Response {
        app.oneshot(
            Request::builder()
                .method("GET")
                .uri(path)
                .header(STANDALONE_USERNAME_HEADER, username)
                .header(STANDALONE_SESSION_TOKEN_HEADER, session_token)
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("request completes")
    }

    async fn response_json(response: axum::response::Response) -> serde_json::Value {
        let bytes = to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("body bytes");
        serde_json::from_slice(&bytes).expect("response decodes")
    }

    fn test_users_groups_state(
        auth_store: LocalAuthStore,
        current_user: LocalUserMetadata,
        local_group_admin_client: Option<Arc<RecordingAdminClient>>,
    ) -> StandaloneUsersGroupsRouteState {
        test_users_groups_state_with_groups_path(
            auth_store,
            current_user,
            local_group_admin_client,
            root_groups_path("missing"),
        )
    }

    fn test_users_groups_state_with_groups_path(
        auth_store: LocalAuthStore,
        current_user: LocalUserMetadata,
        local_group_admin_client: Option<Arc<RecordingAdminClient>>,
        groups_registry_path: PathBuf,
    ) -> StandaloneUsersGroupsRouteState {
        StandaloneUsersGroupsRouteState {
            auth_store,
            local_user_provider: Arc::new(FixedLocalUserProvider { current_user }),
            local_group_admin_client: local_group_admin_client
                .map(|client| client as Arc<dyn StandaloneLocalGroupAdminClient>),
            groups_registry_path,
        }
    }

    fn test_enclosure_admin_state(
        auth_store: LocalAuthStore,
        current_user: LocalUserMetadata,
        enclosure_admin_client: Option<Arc<RecordingEnclosureClient>>,
    ) -> StandaloneEnclosureAdminRouteState {
        StandaloneEnclosureAdminRouteState {
            auth_store,
            local_user_provider: Arc::new(FixedLocalUserProvider { current_user }),
            enclosure_admin_client: enclosure_admin_client
                .map(|client| client as Arc<dyn StandaloneEnclosureAdminClient>),
        }
    }

    fn test_reporting_state(
        auth_store: LocalAuthStore,
        current_user: LocalUserMetadata,
    ) -> StandaloneReportingRouteState {
        StandaloneReportingRouteState {
            auth_store,
            local_user_provider: Arc::new(FixedLocalUserProvider { current_user }),
        }
    }

    fn root_groups_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!("dasobjectstore-{label}-groups-missing.json"))
    }

    fn test_auth_router(
        auth_store: LocalAuthStore,
        accepted_credentials: Vec<(&str, &str)>,
    ) -> axum::Router {
        standalone_auth_router_with_state(StandaloneAuthRouteState {
            auth_store,
            local_password_authenticator: Arc::new(FixedPasswordAuthenticator {
                accepted_credentials: accepted_credentials
                    .into_iter()
                    .map(|(username, password)| (username.to_string(), password.to_string()))
                    .collect(),
            }),
        })
    }

    #[derive(Clone)]
    struct FixedPasswordAuthenticator {
        accepted_credentials: Vec<(String, String)>,
    }

    impl LocalPasswordAuthenticator for FixedPasswordAuthenticator {
        fn authenticate(
            &self,
            username: &str,
            password: &str,
        ) -> Result<(), LocalPasswordAuthError> {
            if username.trim().is_empty() {
                return Err(LocalPasswordAuthError::UsernameRequired);
            }
            if password.is_empty() {
                return Err(LocalPasswordAuthError::PasswordRequired);
            }
            if self
                .accepted_credentials
                .iter()
                .any(|(accepted_username, accepted_password)| {
                    accepted_username == username.trim() && accepted_password == password
                })
            {
                Ok(())
            } else {
                Err(LocalPasswordAuthError::InvalidCredentials)
            }
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
        fn local_user(&self, username: &str) -> Result<LocalUserMetadata, LocalUserDiscoveryError> {
            Ok(LocalUserMetadata::from_username_and_groups(
                username,
                self.current_user.groups.clone(),
            ))
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

    #[derive(Default)]
    struct RecordingEnclosureClient {
        requests: Mutex<Vec<StandaloneEnclosurePrepareDaemonRequest>>,
        create_object_store_requests: Mutex<Vec<DaemonCreateObjectStoreRequest>>,
        endpoint_inventory_requests: Mutex<Vec<DaemonUpsertEndpointInventoryRequest>>,
        status_requests: Mutex<Vec<StandaloneAdminJobStatusDaemonRequest>>,
        cancel_requests: Mutex<Vec<StandaloneAdminJobCancelDaemonRequest>>,
        fail_message: Option<String>,
    }

    impl RecordingEnclosureClient {
        fn requests(&self) -> Vec<StandaloneEnclosurePrepareDaemonRequest> {
            self.requests.lock().expect("requests lock").clone()
        }

        fn create_object_store_requests(&self) -> Vec<DaemonCreateObjectStoreRequest> {
            self.create_object_store_requests
                .lock()
                .expect("create object store requests lock")
                .clone()
        }

        fn endpoint_inventory_requests(&self) -> Vec<DaemonUpsertEndpointInventoryRequest> {
            self.endpoint_inventory_requests
                .lock()
                .expect("endpoint inventory requests lock")
                .clone()
        }

        fn status_requests(&self) -> Vec<StandaloneAdminJobStatusDaemonRequest> {
            self.status_requests
                .lock()
                .expect("status requests lock")
                .clone()
        }

        fn cancel_requests(&self) -> Vec<StandaloneAdminJobCancelDaemonRequest> {
            self.cancel_requests
                .lock()
                .expect("cancel requests lock")
                .clone()
        }
    }

    impl StandaloneEnclosureAdminClient for RecordingEnclosureClient {
        fn submit_prepare_enclosure(
            &self,
            request: StandaloneEnclosurePrepareDaemonRequest,
        ) -> Result<StandaloneEnclosurePrepareResponse, StandaloneEnclosureAdminClientError>
        {
            if let Some(message) = &self.fail_message {
                return Err(StandaloneEnclosureAdminClientError {
                    message: message.clone(),
                });
            }
            self.requests
                .lock()
                .expect("requests lock")
                .push(request.clone());
            Ok(StandaloneEnclosurePrepareResponse {
                accepted: StandaloneEnclosurePrepareAcceptedResponse {
                    job_id: "enclosure-prepare-job-1".to_string(),
                    kind: "enclosure_preparation".to_string(),
                    accepted_at_utc: "2026-07-08T19:50:00Z".to_string(),
                    dry_run: request.dry_run,
                },
                ssd_device: request.ssd_device,
                hdd_devices: request.hdd_devices,
                mount_root: request.mount_root,
                filesystem: request.filesystem.to_string(),
                owner: request.owner,
                administrator_actor: request.administrator_actor,
                client_request_id: request.client_request_id,
            })
        }

        fn submit_create_object_store(
            &self,
            request: DaemonCreateObjectStoreRequest,
        ) -> Result<StandaloneCreateObjectStoreResponse, StandaloneEnclosureAdminClientError>
        {
            if let Some(message) = &self.fail_message {
                return Err(StandaloneEnclosureAdminClientError {
                    message: message.clone(),
                });
            }
            self.create_object_store_requests
                .lock()
                .expect("create object store requests lock")
                .push(request.clone());
            Ok(StandaloneCreateObjectStoreResponse {
                accepted: StandaloneCreateObjectStoreAcceptedResponse {
                    job_id: "objectstore-create-job-1".to_string(),
                    kind: "object_store_creation".to_string(),
                    accepted_at_utc: "2026-07-08T21:10:00Z".to_string(),
                    dry_run: request.dry_run,
                },
                store_id: request.store_id,
                store_class: request.store_class,
                required_copies: request.required_copies,
                bucket: request.bucket,
                reader_group: request.reader_group,
                writer_group: request.writer_group,
                ssd_root: request.ssd_root.display().to_string(),
                object_type: request.object_type,
                enclosure_id: request.enclosure_id,
                public: request.public,
                writeable: request.writeable,
                capacity_behavior: request.capacity_behavior,
                retention: request.retention,
                endpoint_export_mode: request.endpoint_export_mode,
                administrator_actor: request.administrator_actor,
                client_request_id: request.client_request_id,
            })
        }

        fn submit_endpoint_inventory_upsert(
            &self,
            request: DaemonUpsertEndpointInventoryRequest,
        ) -> Result<StandaloneEndpointInventoryUpsertResponse, StandaloneEnclosureAdminClientError>
        {
            if let Some(message) = &self.fail_message {
                return Err(StandaloneEnclosureAdminClientError {
                    message: message.clone(),
                });
            }
            self.endpoint_inventory_requests
                .lock()
                .expect("endpoint inventory requests lock")
                .push(request.clone());
            Ok(StandaloneEndpointInventoryUpsertResponse {
                accepted: StandaloneEndpointInventoryAcceptedResponse {
                    job_id: "endpoint-upsert-job-1".to_string(),
                    kind: "endpoint_validation".to_string(),
                    accepted_at_utc: "2026-07-09T00:00:00Z".to_string(),
                    dry_run: request.dry_run,
                },
                endpoint_id: request.endpoint_id,
                display_name: request.display_name,
                kind: "dasobjectstore_nfs".to_string(),
                validation_state: "validated".to_string(),
                registry_path: "/opt/dasobjectstore/endpoints.json".to_string(),
                administrator_actor: request.administrator_actor,
                client_request_id: request.client_request_id,
            })
        }

        fn job_status(
            &self,
            request: StandaloneAdminJobStatusDaemonRequest,
        ) -> Result<StandaloneAdminJobStatusResponse, StandaloneEnclosureAdminClientError> {
            self.status_requests
                .lock()
                .expect("status requests lock")
                .push(request.clone());
            Ok(StandaloneAdminJobStatusResponse {
                job: StandaloneAdminJobSummary {
                    job_id: request.job_id,
                    kind: "enclosure_preparation".to_string(),
                    state: "running".to_string(),
                    progress: StandaloneAdminJobProgress {
                        stage: "formatting".to_string(),
                        work_bytes_done: 5,
                        work_bytes_total: 10,
                        work_units_done: 1,
                        work_units_total: 2,
                        message: Some("formatting selected devices".to_string()),
                    },
                    percent_complete: Some(50),
                    submitted_at_utc: "2026-07-08T20:05:00Z".to_string(),
                    updated_at_utc: "2026-07-08T20:05:10Z".to_string(),
                    actor: Some("operator".to_string()),
                    failure_message: None,
                },
            })
        }

        fn cancel_job(
            &self,
            request: StandaloneAdminJobCancelDaemonRequest,
        ) -> Result<StandaloneAdminJobCancelResponse, StandaloneEnclosureAdminClientError> {
            self.cancel_requests
                .lock()
                .expect("cancel requests lock")
                .push(request.clone());
            Ok(StandaloneAdminJobCancelResponse {
                job_id: request.job_id,
                accepted: true,
                state: "cancelled".to_string(),
            })
        }
    }

    fn recording_enclosure_client() -> Arc<RecordingEnclosureClient> {
        Arc::new(RecordingEnclosureClient::default())
    }

    fn recording_enclosure_client_with_failure(message: &str) -> Arc<RecordingEnclosureClient> {
        Arc::new(RecordingEnclosureClient {
            requests: Mutex::new(Vec::new()),
            create_object_store_requests: Mutex::new(Vec::new()),
            endpoint_inventory_requests: Mutex::new(Vec::new()),
            status_requests: Mutex::new(Vec::new()),
            cancel_requests: Mutex::new(Vec::new()),
            fail_message: Some(message.to_string()),
        })
    }

    fn prepare_enclosure_request() -> PrepareEnclosureRequest {
        PrepareEnclosureRequest {
            ssd_device: "/dev/disk/by-id/nvme-ssd".to_string(),
            hdd_devices: vec![PrepareEnclosureHddDeviceRequest {
                disk_id: "qnap-1057".to_string(),
                device_path: "/dev/disk/by-id/usb-qnap-1057".to_string(),
            }],
            mount_root: Some("/srv/dasobjectstore".to_string()),
            filesystem: Some("ext4".to_string()),
            owner: Some("stephen".to_string()),
            dry_run: false,
            client_request_id: Some("prepare-1".to_string()),
            allow_format: true,
            existing_data_acknowledged: true,
            confirmation_marker: Some(ENCLOSURE_PREPARE_CONFIRMATION.to_string()),
        }
    }

    fn create_object_store_request() -> CreateObjectStoreRequest {
        CreateObjectStoreRequest {
            store_id: "zymo-fecal-2025-05".to_string(),
            store_class: Some("generated_data".to_string()),
            required_copies: 2,
            bucket: Some("zymo-fecal-2025-05".to_string()),
            reader_group: None,
            writer_group: "bioinformatics".to_string(),
            ssd_root: Some("/srv/dasobjectstore/ssd".to_string()),
            object_type: Some("pod5".to_string()),
            enclosure_id: Some("tl-d800c-01".to_string()),
            public: false,
            writeable: Some(true),
            capacity_behavior: Some("balanced".to_string()),
            retention: Some("standard".to_string()),
            endpoint_export_mode: Some("s3_bucket".to_string()),
            dry_run: false,
            client_request_id: Some("objectstore-create-1".to_string()),
            confirmation_marker: Some(OBJECT_STORE_CREATE_CONFIRMATION.to_string()),
        }
    }

    fn endpoint_inventory_request() -> EndpointInventoryUpsertRequest {
        EndpointInventoryUpsertRequest {
            endpoint_id: "nas-staging".to_string(),
            display_name: "NAS staging".to_string(),
            kind: "dasobjectstore_nfs".to_string(),
            object_service_url: "https://nas.example.test:9443".to_string(),
            validation: EndpointValidationUpsertRequest {
                state: "validated".to_string(),
                checked_at_utc: Some("2026-07-09T00:00:00Z".to_string()),
                message: Some("validated from Web admin workflow".to_string()),
            },
            manager_product_id: "dasobjectstore".to_string(),
            active_bindings: vec![EndpointBindingUpsertRequest {
                binding_id: "binding-1".to_string(),
                governance_domain: "local".to_string(),
                store_id: "zymo-fecal-2025-05".to_string(),
                readiness: "ready".to_string(),
            }],
            dry_run: false,
            client_request_id: Some("endpoint-upsert-1".to_string()),
            confirmation_marker: Some(ENDPOINT_RECORD_CONFIRMATION.to_string()),
        }
    }

    fn registered_auth_store(root: &Path) -> LocalAuthStore {
        registered_auth_store_for_user(root, "admin")
    }

    fn registered_auth_store_for_user(root: &Path, username: &str) -> LocalAuthStore {
        let auth_store = LocalAuthStore::new(root);
        auth_store.create_user(username).expect("user created");
        let token = auth_store
            .issue_registration_token(username, Some(3_600))
            .expect("token issued");
        auth_store
            .register_with_token(username, &token, "secret")
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
