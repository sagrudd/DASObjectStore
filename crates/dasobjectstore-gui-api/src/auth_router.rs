//! Router composition for standalone authentication and administration.

use super::*;
use crate::{FederatedHostSessionResponse, VerifiedHostAuthenticatedContext};
use axum::{
    extract::DefaultBodyLimit,
    middleware,
    routing::{get, post, put},
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
            let router = federated_operational_router(auth_store.clone(), None, None)
                .merge(standalone_session_auth_router(auth_store))
                .merge(easyconnect_public_pairing_router_with_config(None, None));
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
    public_base_url: Option<String>,
) -> Router {
    gui_api_router_for_host_mode_with_s3_descriptor_and_tls_certificate(
        host_mode,
        auth_store,
        include_application_auth,
        s3_descriptor,
        public_base_url,
        crate::StandaloneServerConfig::default_localhost()
            .tls
            .certificate_path,
    )
}

/// Compose the host-mode API with an authoritative S3 descriptor and the
/// certificate bundle used to verify that endpoint before remote grant
/// issuance.
pub fn gui_api_router_for_host_mode_with_s3_descriptor_and_tls_certificate(
    host_mode: GuiApiHostMode,
    auth_store: LocalAuthStore,
    include_application_auth: bool,
    s3_descriptor: Option<StandaloneS3ConnectionDescriptor>,
    public_base_url: Option<String>,
    s3_tls_certificate_path: PathBuf,
) -> Router {
    match host_mode {
        GuiApiHostMode::Standalone => {
            let s3_endpoint = s3_descriptor
                .clone()
                .map(|descriptor| EasyconnectS3EndpointConfig {
                    descriptor,
                    tls_certificate_path: s3_tls_certificate_path.clone(),
                });
            let router = federated_operational_router(
                auth_store.clone(),
                public_base_url.clone(),
                s3_endpoint.clone(),
            )
            .merge(standalone_session_auth_router_with_state(
                StandaloneAuthRouteState {
                    auth_store,
                    local_password_authenticator: Arc::new(
                        SystemLocalPasswordAuthenticator::default(),
                    ),
                    s3_descriptor: s3_descriptor.clone(),
                    s3_tls_certificate_path,
                },
            ))
            .merge(easyconnect_public_pairing_router_with_config(
                s3_endpoint,
                public_base_url,
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
        .merge(federated_operational_router(auth_store, None, None))
}

/// Product status and planning routes for a host that has already established
/// a verified actor. This composition intentionally has no local
/// authentication store and excludes every route that still depends on an OS
/// identity, local user/group state, a password, or a standalone session.
///
/// The embedding host must apply its preverified actor middleware around this
/// router. Calling it directly does not establish an authority boundary.
pub fn host_composed_gui_api_router() -> Router {
    crate::routes::gui_api_router_without_redesign_dashboards()
        .merge(preverified_host_operational_router())
        .merge(preverified_host_reporting_router())
        .merge(preverified_host_profile_catalogue_router())
        .merge(preverified_host_profile_read_router())
}

/// Host-composed operational mutations that derive authority exclusively from
/// `VerifiedHostAuthenticatedContext` and send work through the bounded
/// priority daemon bridge.
fn preverified_host_operational_router() -> Router {
    preverified_host_operational_router_with_state(PreverifiedHostAdminRouteState::packaged())
}

pub(super) fn preverified_host_operational_router_with_state(
    state: PreverifiedHostAdminRouteState,
) -> Router {
    Router::new()
        .route(
            "/api/v1/workspaces/admin/ingest-control",
            post(preverified_host_control_ingest),
        )
        .route(
            "/api/v1/workspaces/object-stores/ingest-policy",
            post(preverified_host_update_object_store_ingest_policy),
        )
        .route(
            "/api/v1/workspaces/object-stores/create",
            post(preverified_host_create_object_store),
        )
        .route(
            "/api/v1/workspaces/enclosures/prepare",
            post(preverified_host_prepare_enclosure),
        )
        .route(
            "/api/v1/workspaces/endpoints/test",
            post(preverified_host_test_endpoint_connection),
        )
        .route(
            "/api/v1/workspaces/endpoints/upsert",
            post(preverified_host_upsert_endpoint_inventory),
        )
        .route(
            "/api/v1/workspaces/admin/jobs/{job_id}",
            get(preverified_host_admin_job_status),
        )
        .route(
            "/api/v1/workspaces/admin/jobs/{job_id}/cancel",
            post(preverified_host_cancel_admin_job),
        )
        .route(
            "/api/v1/profile-s3/stores/{store_id}/objects/{*object_id}",
            put(crate::auth_routes::profile_upload::preverified_host_profile_s3_put)
                .delete(crate::auth_routes::profile_delete::preverified_host_profile_s3_delete),
        )
        .with_state(state)
}

/// Host-composed report rebuilding has no daemon mutation, but it remains a
/// bounded operational action and is therefore independently protected by the
/// same verified Pistis administrator boundary as daemon-backed mutations.
fn preverified_host_reporting_router() -> Router {
    preverified_host_reporting_router_with_state(PreverifiedHostReportingRouteState::packaged())
}

pub(super) fn preverified_host_reporting_router_with_state(
    state: PreverifiedHostReportingRouteState,
) -> Router {
    Router::new()
        .route(
            "/api/v1/workspaces/activity/reporting/performance-report",
            post(preverified_host_rebuild_performance_report),
        )
        .with_state(state)
}

/// Host-composed portable catalogue import is an administrator mutation. It
/// is separated from read-only profile routes because it commits daemon-owned
/// metadata and must use the priority daemon bridge.
fn preverified_host_profile_catalogue_router() -> Router {
    preverified_host_profile_catalogue_router_with_state(
        PreverifiedHostProfileCatalogueRouteState::packaged(),
    )
}

/// Read-only profile inspection for a verified host.  Each route additionally
/// requires an exact store scope bound to the same Pistis session; a viewer
/// role alone is intentionally insufficient.
pub(super) fn preverified_host_profile_read_router() -> Router {
    Router::new()
        .route(
            "/api/v1/profile-s3/stores/{store_id}",
            get(crate::auth_routes::preverified_host_profile_s3_list)
                .head(crate::auth_routes::preverified_host_profile_s3_head),
        )
        .route(
            "/api/v1/profile-s3/stores/{store_id}/verify",
            get(crate::auth_routes::preverified_host_profile_s3_verify),
        )
        .route(
            "/api/v1/profile-s3/stores/{store_id}/diagnostics",
            get(crate::auth_routes::preverified_host_profile_s3_diagnostics),
        )
        .route(
            "/api/v1/profile-s3/stores/{store_id}/health",
            get(crate::auth_routes::preverified_host_profile_s3_health),
        )
        .route(
            "/api/v1/profile-readiness/stores/{store_id}",
            get(crate::auth_routes::preverified_host_profile_readiness),
        )
}

pub(super) fn preverified_host_profile_catalogue_router_with_state(
    state: PreverifiedHostProfileCatalogueRouteState,
) -> Router {
    Router::new()
        .route(
            "/api/v1/profile-catalogue/stores/{store_id}/import",
            post(preverified_host_profile_catalogue_import),
        )
        .with_state(state)
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

fn federated_operational_router(
    auth_store: LocalAuthStore,
    public_base_url: Option<String>,
    s3_endpoint: Option<EasyconnectS3EndpointConfig>,
) -> Router {
    let mut easyconnect_state = StandaloneEasyconnectRouteState::system(auth_store.clone());
    if let Some(public_base_url) = public_base_url {
        easyconnect_state.public_base_url = public_base_url;
    }
    easyconnect_state.s3_endpoint = s3_endpoint;
    crate::routes::gui_api_router_without_redesign_dashboards()
        .merge(crate::remote_control_routes::remote_control_router())
        .merge(standalone_dashboard_router(auth_store.clone()))
        .merge(standalone_live_status_router(auth_store.clone()))
        .merge(standalone_easyconnect_router_with_state(easyconnect_state))
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
        .merge(easyconnect_public_pairing_router_with_config(None, None))
}

/// Public pairing creation, status, and one-time exchange without discovery.
///
/// Embeddings that have an authoritative HTTPS origin should use
/// [`easyconnect_public_router_with_config`] to expose Pistis discovery too.
/// Approval is deliberately absent and remains behind host authentication.
pub fn easyconnect_public_router() -> Router {
    easyconnect_public_pairing_router_with_config(None, None)
}

/// Public create, status, and exchange routes with a deployment-owned S3
/// descriptor. Exchange fails closed when the descriptor is absent.
pub fn easyconnect_public_router_with_s3_descriptor(
    s3_endpoint: Option<EasyconnectS3EndpointConfig>,
) -> Router {
    easyconnect_public_pairing_router_with_config(s3_endpoint, None)
}

/// Public Pistis discovery and pairing routes from deployment-owned config.
///
/// An absent or invalid public origin leaves discovery available only as an
/// explicit service-unavailable response and also prevents pairing creation.
pub fn easyconnect_public_router_with_config(
    s3_endpoint: Option<EasyconnectS3EndpointConfig>,
    public_base_url: Option<String>,
) -> Router {
    easyconnect_public_router_with_config_and_daemon(
        s3_endpoint,
        public_base_url,
        EasyconnectDaemonEndpoint::default(),
    )
}

/// Public Pistis discovery and pairing routes with an explicit trusted local
/// daemon endpoint.
///
/// The endpoint is supplied only by in-process composition and is never
/// derived from HTTP input.
pub fn easyconnect_public_router_with_config_and_daemon(
    s3_endpoint: Option<EasyconnectS3EndpointConfig>,
    public_base_url: Option<String>,
    daemon_endpoint: EasyconnectDaemonEndpoint,
) -> Router {
    let state = EasyconnectPublicRouteState {
        s3_endpoint,
        public_base_url: public_base_url.and_then(validated_easyconnect_public_base_url),
        appliance_id: super::auth_identity_routes::system_appliance_id(),
        daemon_endpoint,
    };
    easyconnect_public_pairing_router()
        .route(
            "/api/v1/remote/easyconnect/discovery",
            get(pistis_easyconnect_discovery),
        )
        .with_state(state)
}

fn easyconnect_public_pairing_router_with_config(
    s3_endpoint: Option<EasyconnectS3EndpointConfig>,
    public_base_url: Option<String>,
) -> Router {
    let state = EasyconnectPublicRouteState {
        s3_endpoint,
        public_base_url: public_base_url.and_then(validated_easyconnect_public_base_url),
        appliance_id: super::auth_identity_routes::system_appliance_id(),
        daemon_endpoint: EasyconnectDaemonEndpoint::default(),
    };
    easyconnect_public_pairing_router().with_state(state)
}

fn easyconnect_public_pairing_router() -> Router<EasyconnectPublicRouteState> {
    Router::new()
        .route(
            "/api/v1/remote/easyconnect/pairings",
            post(easyconnect_create_pairing),
        )
        .route(
            "/api/v1/remote/easyconnect/pairings/exchange",
            post(easyconnect_exchange_pairing),
        )
        .route(
            "/api/v1/remote/easyconnect/pairings/{pairing_id}",
            get(easyconnect_pairing_status),
        )
        .layer(DefaultBodyLimit::max(64 * 1024))
}

fn validated_easyconnect_public_base_url(public_base_url: String) -> Option<String> {
    let parsed = reqwest::Url::parse(&public_base_url).ok()?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || !matches!(parsed.path(), "" | "/")
    {
        return None;
    }
    Some(public_base_url.trim_end_matches('/').to_string())
}

/// Pistis approval routes that must be mounted behind a host-verified actor and
/// a credential-free [`SharedPistisEasyconnectApprovalResolver`] extension.
pub fn pistis_easyconnect_approval_router(s3_endpoint: EasyconnectS3EndpointConfig) -> Router {
    pistis_easyconnect_approval_router_with_daemon(
        s3_endpoint,
        EasyconnectDaemonEndpoint::default(),
    )
}

/// Pistis approval routes with an explicit trusted local daemon endpoint.
pub fn pistis_easyconnect_approval_router_with_daemon(
    s3_endpoint: EasyconnectS3EndpointConfig,
    daemon_endpoint: EasyconnectDaemonEndpoint,
) -> Router {
    Router::new()
        .route(
            "/api/v1/remote/easyconnect/pairings/approve",
            post(pistis_easyconnect_approve_pairing),
        )
        .route(
            "/remote/easyconnect/login",
            get(easyconnect_browser_approval),
        )
        .layer(DefaultBodyLimit::max(64 * 1024))
        .layer(Extension(daemon_endpoint))
        .layer(Extension(Some(s3_endpoint)))
}

pub(crate) fn standalone_easyconnect_router_with_state(
    state: StandaloneEasyconnectRouteState,
) -> Router {
    let s3_endpoint = state.s3_endpoint.clone();
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
        .layer(Extension(EasyconnectDaemonEndpoint::default()))
        .layer(Extension(s3_endpoint))
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
