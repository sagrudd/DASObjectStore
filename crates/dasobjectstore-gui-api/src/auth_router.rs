//! Router composition for standalone authentication and administration.

use super::*;
use axum::{
    extract::DefaultBodyLimit,
    routing::{get, post},
    Extension, Router,
};

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

pub(crate) fn standalone_auth_router_with_state(state: StandaloneAuthRouteState) -> Router {
    Router::new()
        .route("/api/register", post(register))
        .route("/api/login", post(login))
        .route("/api/v1/remote/authenticate", post(remote_authenticate))
        .route("/api/logout", post(logout))
        .route("/api/session", post(session))
        .with_state(state)
}

pub fn standalone_easyconnect_router(auth_store: LocalAuthStore) -> Router {
    standalone_easyconnect_router_with_state(StandaloneEasyconnectRouteState::system(auth_store))
}

pub(crate) fn standalone_easyconnect_router_with_state(
    state: StandaloneEasyconnectRouteState,
) -> Router {
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

pub(crate) fn standalone_dashboard_router_with_state(
    state: StandaloneDashboardRouteState,
) -> Router {
    Router::new()
        .route("/api/v1/dashboard/home", get(standalone_home_dashboard))
        .route(
            "/api/v1/dashboard/status",
            get(standalone_cached_home_dashboard),
        )
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

pub(crate) fn standalone_users_groups_router_with_state(
    state: StandaloneUsersGroupsRouteState,
) -> Router {
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

pub(crate) fn standalone_enclosure_admin_router_with_state(
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
            "/api/v1/workspaces/object-stores/ingest-policy",
            post(update_object_store_ingest_policy),
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

pub(crate) fn standalone_reporting_router_with_state(
    state: StandaloneReportingRouteState,
) -> Router {
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
