//! Axum API boundary for GUI-facing DASObjectStore views.

pub mod actions;
mod activity_aggregator;
pub mod auth;
pub mod auth_guard;
pub mod auth_routes;
mod daemon_bridge;
pub mod dashboard;
mod enclosures_aggregator;
pub mod endpoints;
mod endpoints_aggregator;
mod endpoints_registry;
mod groups_registry;
mod home_aggregator;
pub mod host_auth;
mod mtls_listener;
mod object_browser_routes;
mod object_stores_aggregator;
mod remote_upload_aggregator;
mod reporting;
pub mod routes;
pub mod server_config;
pub mod tls_assets;
pub mod view;
pub mod workspaces;

pub use actions::{
    action_catalog, plan_action, GuiActionCatalog, GuiActionDescriptor, GuiActionExecution,
    GuiActionKind, GuiActionPlan, GuiActionPlanError, GuiActionPlanRequest, GuiActionSafety,
};
pub use auth::{
    discover_current_local_user, discover_local_user, AuthRegistry, AuthTokenResetReport,
    AuthenticatedUser, LocalAuthStore, LocalAuthStoreError, LocalPasswordAuthError,
    LocalUserDiscoveryError, LocalUserMetadata, LoginResponse, LogoutResponse,
    PamLocalPasswordAuthenticator, RegisterResponse, RegistrationTokenRecord, SessionCheckResponse,
    SessionTokenRecord, UserSummary, SUDO_ADMIN_GROUPS,
};
#[cfg(target_os = "linux")]
pub use auth::{
    DEFAULT_DASOBJECTSTORE_LOCAL_AUTH_HELPER_PATH, DEFAULT_PROSOPIKON_LOCAL_AUTH_HELPER_PATH,
    PROSOPIKON_LOCAL_AUTH_HELPER_BYPASS_ENV, PROSOPIKON_LOCAL_AUTH_HELPER_ENV,
};
pub use auth_guard::{
    AuthGuardError, AuthGuardRejection, AuthenticatedActorAuthority, AuthenticatedGuiActor,
    FederatedHostSessionResponse, STANDALONE_SESSION_TOKEN_HEADER, STANDALONE_USERNAME_HEADER,
};
pub use auth_routes::{
    federated_gui_api_router, gui_api_router_for_host_mode,
    gui_api_router_for_host_mode_with_application_auth, standalone_auth_router,
    standalone_gui_api_router, AssignLocalUserToGroupRequest, AuthRouteError,
    CreateLocalGroupRequest, GuiApiHostMode, LoginRequest, LogoutRequest, RegisterRequest,
    SessionCheckRequest, StandaloneEasyconnectAuthContextResponse,
    StandaloneLocalGroupAdminAcceptedResponse, StandaloneLocalGroupAdminResponse,
    StandaloneLocalGroupOperation,
};
pub use dashboard::{
    DashboardActionKind, DashboardActionPriority, DashboardAttentionSourceKind,
    DashboardAttentionSourceView, DashboardAttentionView, DashboardRequiredActionView,
    DashboardSeverity, DashboardWarning, DashboardWarningItemView, DestageQueueObjectView,
    DestageQueueView, DiskHealthView, HealthSignalsView, HealthStateView, IngestJobStateView,
    IngestProgressView, IngestQueueJobView, IngestQueueView, ObjectStateView, PoolAccessMode,
    PoolStateView, PoolStatusView, QueuePressureView, StorageGroupView, WriterPolicyReadinessView,
};
pub use endpoints::{
    EndpointBindingReadinessView, EndpointBindingView, EndpointInventoryItemView,
    EndpointInventoryView, EndpointKindView, EndpointValidationStateView, EndpointValidationView,
    EndpointWarningSeverityView, EndpointWarningView, ENDPOINT_INVENTORY_SCHEMA_VERSION,
};
pub use host_auth::{
    accept_host_authenticated_context, HostAuthContextError, HostAuthenticatedContext,
    HostAuthenticationAuthority, HostAuthenticationContextVerifier,
    VerifiedHostAuthenticatedContext, HOST_AUTH_AUDIENCE, HOST_AUTH_CONTEXT_SCHEMA_VERSION,
    MAX_HOST_AUTH_CONTEXT_TTL_SECONDS,
};
pub use mtls_listener::{
    application_mtls_router, build_application_mtls_listener, MtlsApplicationConnectInfo,
    MtlsApplicationListener, MtlsListenerError,
};
pub use remote_upload_aggregator::{
    RemoteUploadActorView, RemoteUploadObjectStoreView, RemoteUploadWorkspaceView,
};
pub use routes::gui_api_router;
pub use server_config::{
    StandaloneAuthenticationAuthority, StandaloneAuthenticationConfig, StandaloneMutualTlsConfig,
    StandaloneServerConfig, StandaloneServerConfigError, StandaloneTlsConfig,
    DEFAULT_MTLS_HTTPS_PORT, DEFAULT_STANDALONE_PUBLIC_BASE_URL,
    DEFAULT_TLS_CERTIFICATE_RELATIVE_PATH, DEFAULT_TLS_PRIVATE_KEY_RELATIVE_PATH,
};
pub use tls_assets::{
    ensure_standalone_tls_assets, load_standalone_tls_assets, StandaloneTlsAssetError,
    StandaloneTlsAssetReport, StandaloneTlsAssets,
};
pub use view::{api_liveness, ApiLiveness, ApiLivenessStatus};
pub use workspaces::{
    workspace_navigation, ActivityTaskKindView, ActivityTaskStateView, ActivityTaskView,
    ActivityWorkspaceView, DisksWorkspaceView, EndpointsWorkspaceView, LocalGroupMembershipView,
    LocalGroupOperationKindView, LocalGroupOperationView, LocalUserAuthorityView,
    ObjectInventoryFiltersView, ObjectInventoryRowView, ObjectsWorkspaceView,
    OperationsWorkspaceKindView, OperationsWorkspacesView, OverviewWorkspaceView,
    StandaloneUserAccountView, StorePolicySummaryView, StoresWorkspaceView,
    UsersGroupsCapabilitiesView, UsersGroupsHostModeView, UsersGroupsWorkspaceView,
    WorkspaceNavigationItemView, OPERATIONS_WORKSPACES_SCHEMA_VERSION,
};

/// Returns the GUI API crate version.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::version;

    #[test]
    fn exposes_package_version() {
        assert_eq!(version(), env!("CARGO_PKG_VERSION"));
    }
}
