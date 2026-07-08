//! Axum API boundary for GUI-facing DASObjectStore views.

pub mod actions;
pub mod auth;
pub mod auth_guard;
pub mod auth_routes;
pub mod dashboard;
mod enclosures_aggregator;
pub mod endpoints;
mod home_aggregator;
mod object_stores_aggregator;
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
    discover_current_local_user, AuthRegistry, AuthTokenResetReport, AuthenticatedUser,
    LocalAuthStore, LocalAuthStoreError, LocalPasswordAuthError, LocalUserDiscoveryError,
    LocalUserMetadata, LoginResponse, LogoutResponse, PamLocalPasswordAuthenticator,
    RegisterResponse, RegistrationTokenRecord, SessionCheckResponse, SessionTokenRecord,
    UserSummary, SUDO_ADMIN_GROUPS,
};
pub use auth_guard::{
    AuthGuardError, AuthGuardRejection, AuthenticatedActorAuthority, AuthenticatedGuiActor,
    STANDALONE_SESSION_TOKEN_HEADER, STANDALONE_USERNAME_HEADER,
};
pub use auth_routes::{
    gui_api_router_for_host_mode, standalone_auth_router, standalone_gui_api_router,
    AssignLocalUserToGroupRequest, AuthRouteError, CreateLocalGroupRequest, GuiApiHostMode,
    LoginRequest, LogoutRequest, RegisterRequest, SessionCheckRequest,
    StandaloneLocalGroupAdminAcceptedResponse, StandaloneLocalGroupAdminResponse,
    StandaloneLocalGroupOperation,
};
pub use dashboard::{
    DashboardActionKind, DashboardActionPriority, DashboardAttentionSourceKind,
    DashboardAttentionSourceView, DashboardAttentionView, DashboardRequiredActionView,
    DashboardSeverity, DashboardWarning, DashboardWarningItemView, DestageQueueObjectView,
    DestageQueueView, DiskHealthView, HealthSignalsView, HealthStateView, IngestJobStateView,
    IngestProgressView, IngestQueueJobView, IngestQueueView, ObjectStateView, PoolAccessMode,
    PoolStateView, PoolStatusView, QueuePressureView,
};
pub use endpoints::{
    EndpointBindingReadinessView, EndpointBindingView, EndpointInventoryItemView,
    EndpointInventoryView, EndpointKindView, EndpointValidationStateView, EndpointValidationView,
    EndpointWarningSeverityView, EndpointWarningView, ENDPOINT_INVENTORY_SCHEMA_VERSION,
};
pub use routes::gui_api_router;
pub use server_config::{
    StandaloneAuthenticationAuthority, StandaloneAuthenticationConfig, StandaloneServerConfig,
    StandaloneServerConfigError, StandaloneTlsConfig, DEFAULT_STANDALONE_PUBLIC_BASE_URL,
    DEFAULT_TLS_CERTIFICATE_RELATIVE_PATH, DEFAULT_TLS_PRIVATE_KEY_RELATIVE_PATH,
};
pub use tls_assets::{
    ensure_standalone_tls_assets, load_standalone_tls_assets, StandaloneTlsAssetError,
    StandaloneTlsAssetReport, StandaloneTlsAssets,
};
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
