//! Router composition for standalone authentication and administration.

use super::*;
use crate::{FederatedHostSessionResponse, VerifiedHostAuthenticatedContext};
use axum::{
    extract::DefaultBodyLimit,
    middleware,
    routing::{get, post},
    Extension, Router,
};
use dasobjectstore_daemon::api::{
    APPLICATION_ACCESS_TOKEN_EXCHANGE_ROUTE, APPLICATION_UPLOAD_COMPLETION_CAPABILITY_ROUTE,
    APPLICATION_UPLOAD_COMPLETION_ROUTE, ERGASTERION_CAPABILITY_DISCOVERY_ROUTE,
    ERGASTERION_CAPABILITY_EXCHANGE_ROUTE, ERGASTERION_CAPABILITY_RENEWAL_ROUTE,
    ERGASTERION_OBJECT_GROUP_STATUS_ROUTE, ERGASTERION_OBJECT_READ_ROUTE,
    ERGASTERION_OBJECT_SNAPSHOT_ROUTE, PROFILE_S3_MULTIPART_PART_ROUTE,
};

pub fn standalone_gui_api_router(auth_store: LocalAuthStore) -> Router {
    gui_api_router_for_host_mode(GuiApiHostMode::Standalone, auth_store)
}

pub fn gui_api_router_for_host_mode(
    host_mode: GuiApiHostMode,
    auth_store: LocalAuthStore,
) -> Router {
    gui_api_router_for_host_mode_with_application_auth(host_mode, auth_store, true)
}

pub fn gui_api_router_for_host_mode_with_application_auth(
    host_mode: GuiApiHostMode,
    auth_store: LocalAuthStore,
    include_application_auth: bool,
) -> Router {
    match host_mode {
        GuiApiHostMode::Standalone => {
            let router = federated_operational_router(auth_store.clone())
                .merge(standalone_session_auth_router(auth_store))
                .merge(easyconnect_public_router());
            if include_application_auth {
                router.merge(standalone_application_auth_router())
            } else {
                router
            }
        }
        GuiApiHostMode::SynoptikonIntegrated => crate::gui_api_router(),
    }
}

pub fn gui_api_router_for_host_mode_with_s3_descriptor(
    host_mode: GuiApiHostMode,
    auth_store: LocalAuthStore,
    include_application_auth: bool,
    s3_descriptor: Option<StandaloneS3ConnectionDescriptor>,
) -> Router {
    match host_mode {
        GuiApiHostMode::Standalone => {
            let router = federated_operational_router(auth_store.clone())
                .merge(standalone_session_auth_router_with_state(
                    StandaloneAuthRouteState {
                        auth_store,
                        local_password_authenticator: Arc::new(
                            SystemLocalPasswordAuthenticator::default(),
                        ),
                        s3_descriptor: s3_descriptor.clone(),
                    },
                ))
                .merge(easyconnect_public_router_with_state(
                    EasyconnectPublicRouteState { s3_descriptor },
                ));
            if include_application_auth {
                router.merge(standalone_application_auth_router())
            } else {
                router
            }
        }
        GuiApiHostMode::SynoptikonIntegrated => crate::gui_api_router(),
    }
}

/// Product routes for a host that supplies a verified actor. Login/session
/// issuance is intentionally omitted so Monas or Synoptikon remains the sole
/// browser authentication authority.
pub fn federated_gui_api_router(auth_store: LocalAuthStore) -> Router {
    Router::new()
        .route("/api/v1/host-session", get(federated_host_session))
        .merge(federated_operational_router(auth_store))
}

async fn federated_host_session(
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<VerifiedHostAuthenticatedContext>,
) -> Json<FederatedHostSessionResponse> {
    Json(FederatedHostSessionResponse::from_host_actor(
        actor,
        verified.context().csrf_binding_sha256.clone(),
    ))
}

fn federated_operational_router(auth_store: LocalAuthStore) -> Router {
    crate::routes::gui_api_router_without_redesign_dashboards()
        .merge(crate::remote_control_routes::remote_control_router())
        .merge(standalone_dashboard_router(auth_store.clone()))
        .merge(standalone_live_status_router(auth_store.clone()))
        .merge(standalone_easyconnect_router_with_state(
            StandaloneEasyconnectRouteState::system(auth_store.clone()),
        ))
        .merge(standalone_users_groups_router(auth_store.clone()))
        .merge(standalone_enclosure_admin_router(auth_store.clone()))
        .merge(crate::object_browser_routes::standalone_object_browser_router(auth_store.clone()))
        .merge(standalone_reporting_router(auth_store))
}

fn standalone_live_status_router(auth_store: LocalAuthStore) -> Router {
    standalone_live_status_router_with_bridge(
        auth_store,
        crate::daemon_bridge::DaemonBridge::shared_packaged(),
    )
}

pub(crate) fn standalone_live_status_router_with_bridge(
    auth_store: LocalAuthStore,
    daemon_bridge: Arc<crate::daemon_bridge::DaemonBridge>,
) -> Router {
    Router::new()
        .route(
            "/api/v1/workspaces/live-status",
            get(standalone_live_status_workspace),
        )
        .layer(Extension(auth_store))
        .with_state(daemon_bridge)
}

pub fn standalone_auth_router(auth_store: LocalAuthStore) -> Router {
    standalone_session_auth_router(auth_store).merge(standalone_application_auth_router())
}

fn standalone_session_auth_router(auth_store: LocalAuthStore) -> Router {
    standalone_session_auth_router_with_state(StandaloneAuthRouteState::system(auth_store))
}

#[cfg(test)]
pub(crate) fn standalone_auth_router_with_state(state: StandaloneAuthRouteState) -> Router {
    standalone_session_auth_router_with_state(state).merge(standalone_application_auth_router())
}

fn standalone_session_auth_router_with_state(state: StandaloneAuthRouteState) -> Router {
    Router::new()
        .route("/api/register", post(register))
        .route("/api/login", post(login))
        .route("/api/v1/remote/authenticate", post(remote_authenticate))
        .route(
            "/api/v1/remote/easyconnect/sessions/{session_id}/renew",
            post(remote_easyconnect_renew),
        )
        .route("/api/logout", post(logout))
        .route("/api/session", post(session))
        .with_state(state)
}

fn standalone_application_auth_router() -> Router {
    Router::new()
        .route(
            ERGASTERION_CAPABILITY_DISCOVERY_ROUTE,
            get(discover_ergasterion_capability),
        )
        .route(
            ERGASTERION_CAPABILITY_EXCHANGE_ROUTE,
            post(exchange_ergasterion_capability),
        )
        .route(
            ERGASTERION_CAPABILITY_RENEWAL_ROUTE,
            post(renew_ergasterion_capability),
        )
        .route(
            ERGASTERION_OBJECT_SNAPSHOT_ROUTE,
            post(ergasterion_object_snapshot),
        )
        .route(
            ERGASTERION_OBJECT_GROUP_STATUS_ROUTE,
            post(ergasterion_object_group_status),
        )
        .route(ERGASTERION_OBJECT_READ_ROUTE, get(ergasterion_object_read))
        .route(
            APPLICATION_ACCESS_TOKEN_EXCHANGE_ROUTE,
            post(exchange_application_access_token),
        )
        .route(
            APPLICATION_UPLOAD_COMPLETION_CAPABILITY_ROUTE,
            post(issue_application_upload_capability),
        )
        .route(
            APPLICATION_UPLOAD_COMPLETION_ROUTE,
            post(complete_application_upload),
        )
        .layer(DefaultBodyLimit::max(64 * 1024))
        .layer(middleware::map_response(application_no_store))
}

async fn application_no_store(mut response: axum::response::Response) -> axum::response::Response {
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response
}

pub fn standalone_easyconnect_router(auth_store: LocalAuthStore) -> Router {
    standalone_easyconnect_router_with_state(StandaloneEasyconnectRouteState::system(auth_store))
        .merge(easyconnect_public_router())
}

/// Pairing creation and one-time exchange routes that do not require a browser
/// session. Approval is deliberately absent and remains behind host auth.
pub fn easyconnect_public_router() -> Router {
    easyconnect_public_router_with_state(EasyconnectPublicRouteState::default())
}

fn easyconnect_public_router_with_state(state: EasyconnectPublicRouteState) -> Router {
    Router::new()
        .route(
            "/api/v1/remote/easyconnect/pairings",
            post(easyconnect_create_pairing),
        )
        .route(
            "/api/v1/remote/easyconnect/pairings/exchange",
            post(easyconnect_exchange_pairing),
        )
        .layer(DefaultBodyLimit::max(64 * 1024))
        .with_state(state)
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
        .route(
            "/api/v1/remote/easyconnect/pairings/approve",
            post(easyconnect_approve_pairing),
        )
        .route(
            "/remote/easyconnect/login",
            get(easyconnect_browser_approval),
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
            "/api/v1/dashboard/object-stores/{store_id}/capacity",
            get(standalone_store_capacity),
        )
        .route(
            "/api/v1/profile-s3/stores/{store_id}/objects",
            get(standalone_profile_s3_list).head(standalone_profile_s3_head),
        )
        .route(
            "/api/v1/profile-s3/stores/{store_id}/objects/{*object_id}",
            get(standalone_profile_s3_get)
                .put(standalone_profile_s3_put)
                .delete(standalone_profile_s3_delete),
        )
        .route(
            "/api/v1/profile-s3/stores/{store_id}/multipart/{reservation_id}/complete",
            post(standalone_profile_s3_multipart_complete),
        )
        .route(
            "/api/v1/profile-s3/stores/{store_id}/multipart/{reservation_id}/status",
            get(standalone_profile_s3_multipart_status),
        )
        .route(
            PROFILE_S3_MULTIPART_PART_ROUTE,
            post(standalone_profile_s3_multipart_part),
        )
        .route(
            "/api/v1/profile-s3/stores/{store_id}/verify",
            get(standalone_profile_s3_verify),
        )
        .route(
            "/api/v1/profile-s3/stores/{store_id}/diagnostics",
            get(standalone_profile_s3_diagnostics),
        )
        .route(
            "/api/v1/profile-s3/stores/{store_id}/health",
            get(standalone_profile_s3_health),
        )
        .route(
            "/api/v1/profile-readiness/stores/{store_id}",
            get(standalone_profile_readiness),
        )
        .route(
            "/api/v1/profile-capabilities",
            get(standalone_profile_capabilities),
        )
        .route(
            "/api/v1/profile-catalogue/stores/{store_id}",
            get(standalone_profile_catalogue_export),
        )
        .route(
            "/api/v1/profile-catalogue/stores/{store_id}/import",
            post(standalone_profile_catalogue_import),
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
            "/api/v1/workspaces/admin/ingest-control",
            post(control_ingest),
        )
        .route(
            "/api/v1/workspaces/endpoints/upsert",
            post(upsert_endpoint_inventory),
        )
        .route(
            "/api/v1/workspaces/endpoints/test",
            post(test_endpoint_connection),
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
