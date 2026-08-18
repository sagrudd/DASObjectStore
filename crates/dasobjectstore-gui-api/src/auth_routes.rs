use crate::groups_registry::{
    default_groups_registry_path, read_storage_groups_for_user, upsert_storage_group,
};
use crate::{
    AuthenticatedGuiActor, DasCapability, DasRolePolicy, DashboardWarning, LocalAuthStore,
    LocalAuthStoreError, UsersGroupsWorkspaceView, VerifiedHostAuthenticatedContext,
    VerifiedHostObjectPrefixScope, VerifiedHostStoreScope,
};

#[path = "auth_admin_clients.rs"]
mod auth_admin_clients;
#[path = "auth_clients.rs"]
mod auth_clients;
#[path = "auth_identity_routes.rs"]
mod auth_identity_routes;
#[path = "auth_parsing.rs"]
mod auth_parsing;
#[path = "auth_reporting.rs"]
mod auth_reporting;
#[path = "auth_router.rs"]
mod auth_router;
#[path = "auth_validation.rs"]
mod auth_validation;
#[path = "auth_contracts.rs"]
mod contracts;
#[path = "easyconnect_discovery.rs"]
mod easyconnect_discovery;
#[path = "profile_catalogue.rs"]
mod profile_catalogue;
#[path = "profile_delete.rs"]
pub(crate) mod profile_delete;
#[path = "profile_download.rs"]
mod profile_download;
#[path = "profile_multipart.rs"]
pub(crate) mod profile_multipart;
#[path = "profile_upload.rs"]
pub(crate) mod profile_upload;
use auth_admin_clients::*;
use auth_clients::*;
use auth_identity_routes::*;
pub use auth_identity_routes::{
    EasyconnectDaemonEndpoint, EasyconnectS3EndpointConfig, StandaloneS3ConnectionDescriptor,
};
use auth_parsing::*;
use auth_reporting::*;
pub use auth_router::{
    easyconnect_public_router, easyconnect_public_router_with_config,
    easyconnect_public_router_with_config_and_daemon, easyconnect_public_router_with_s3_descriptor,
    federated_gui_api_router, gui_api_router_for_host_mode,
    gui_api_router_for_host_mode_with_application_auth,
    gui_api_router_for_host_mode_with_s3_descriptor,
    gui_api_router_for_host_mode_with_s3_descriptor_and_tls_certificate,
    host_composed_gui_api_router, pistis_easyconnect_approval_router,
    pistis_easyconnect_approval_router_with_daemon, standalone_auth_router,
    standalone_easyconnect_router, standalone_gui_api_router,
};
use auth_validation::*;
use axum::{
    body::{Body, Bytes},
    extract::{Extension, Path, Query, State},
    http::{
        header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE},
        HeaderMap, HeaderValue, StatusCode,
    },
    response::{Html, IntoResponse, Response},
    Json,
};
use contracts::*;
pub use contracts::*;
use dasobjectstore_core::backend::BackendObjectKey;
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_daemon::api::{
    ApplicationUploadCapabilityIssueRequest as DaemonApplicationUploadCapabilityIssueRequest,
    ApplicationUploadCapabilityIssueResponse as DaemonApplicationUploadCapabilityIssueResponse,
    ApplicationUploadCompletionRequest as DaemonApplicationUploadCompletionRequest,
    ApplicationUploadCompletionResponse as DaemonApplicationUploadCompletionResponse,
    ErgasterionCapabilityDiscoveryResponse as DaemonErgasterionCapabilityDiscoveryResponse,
    ErgasterionCapabilityExchangeRequest as DaemonErgasterionCapabilityExchangeRequest,
    ErgasterionCapabilityExchangeResponse as DaemonErgasterionCapabilityExchangeResponse,
    ErgasterionCapabilityRenewalRequest as DaemonErgasterionCapabilityRenewalRequest,
    ErgasterionObjectGroupStatusRequest as DaemonErgasterionObjectGroupStatusRequest,
    ErgasterionObjectGroupStatusResponse as DaemonErgasterionObjectGroupStatusResponse,
    ErgasterionObjectSnapshotRequest as DaemonErgasterionObjectSnapshotRequest,
    ErgasterionObjectSnapshotResponse as DaemonErgasterionObjectSnapshotResponse,
    OpaqueApplicationCapability as DaemonOpaqueApplicationCapability,
    RemoteObjectGroupStatusRequest, RemoteObjectSnapshotRequest,
};
use dasobjectstore_daemon::runtime::LOCAL_ADMIN_CONFIRMATION_MARKER;
use dasobjectstore_daemon::{
    ApplicationAccessTokenExchangeRequest as DaemonApplicationAccessTokenExchangeRequest,
    ApplicationAccessTokenExchangeResponse as DaemonApplicationAccessTokenExchangeResponse,
    AssignLocalUserToLocalGroupRequest as DaemonAssignLocalUserToLocalGroupRequest,
    AssignLocalUserToLocalGroupResponse as DaemonAssignLocalUserToLocalGroupResponse,
    CapacityStatusRequest as DaemonCapacityStatusRequest,
    CapacityStatusResponse as DaemonCapacityStatusResponse,
    CreateLocalGroupRequest as DaemonCreateLocalGroupRequest,
    CreateLocalGroupResponse as DaemonCreateLocalGroupResponse,
    CreateObjectStoreRequest as DaemonCreateObjectStoreRequest,
    CreateObjectStoreResponse as DaemonCreateObjectStoreResponse, DaemonClient,
    DaemonEndpointBinding, DaemonEndpointKind, DaemonEndpointValidation,
    DaemonEndpointValidationState, DaemonIngestControlAction, DaemonIngestControlState,
    DaemonJobCancelRequest, DaemonJobCancelResponse, DaemonJobId, DaemonJobKind, DaemonJobProgress,
    DaemonJobState, DaemonJobStatusRequest, DaemonJobStatusResponse, DaemonJobSummary,
    DaemonLocalAdminCommand, DaemonRuntimeConfig,
    IngestControlRequest as DaemonIngestControlRequest,
    IngestControlResponse as DaemonIngestControlResponse,
    ObjectStoreCapabilityDiscoveryRequest as DaemonProfileCapabilitiesRequest,
    ObjectStoreCapabilityDiscoveryResponse as DaemonProfileCapabilitiesResponse,
    PrepareEnclosureFilesystem as DaemonPrepareEnclosureFilesystem,
    PrepareEnclosureHddDevice as DaemonPrepareEnclosureHddDevice,
    PrepareEnclosureRequest as DaemonPrepareEnclosureRequest,
    PrepareEnclosureResponse as DaemonPrepareEnclosureResponse,
    ProfileDiagnosticsRequest as DaemonProfileDiagnosticsRequest,
    ProfileDiagnosticsResponse as DaemonProfileDiagnosticsResponse,
    ProfileReadinessRequest as DaemonProfileReadinessRequest,
    ProfileReadinessResponse as DaemonProfileReadinessResponse,
    ProfileS3HeadRequest as DaemonProfileS3HeadRequest,
    ProfileS3HeadResponse as DaemonProfileS3HeadResponse,
    ProfileS3HealthRequest as DaemonProfileS3HealthRequest,
    ProfileS3HealthResponse as DaemonProfileS3HealthResponse,
    ProfileS3ListRequest as DaemonProfileS3ListRequest,
    ProfileS3ListResponse as DaemonProfileS3ListResponse,
    ProfileS3VerifyRequest as DaemonProfileS3VerifyRequest,
    ProfileS3VerifyResponse as DaemonProfileS3VerifyResponse,
    RemoteEasyconnectApprovePairingRequest, RemoteEasyconnectApprovePairingResponse,
    RemoteEasyconnectAuthProvider, RemoteEasyconnectCreatePairingRequest,
    RemoteEasyconnectCreatePairingResponse, RemoteEasyconnectDiscoveryResponse,
    RemoteEasyconnectExchangeConnectionResponse, RemoteEasyconnectExchangePairingRequest,
    RemoteEasyconnectObjectStoreGrant, RemoteEasyconnectRenewSessionRequest,
    RemoteEasyconnectRenewSessionResponse, RemoteEasyconnectS3ConnectionDescriptor,
    RemoteEasyconnectSessionPolicy, UnixSocketDaemonTransport,
    UpdateObjectStoreIngestPolicyRequest as DaemonUpdateObjectStoreIngestPolicyRequest,
    UpdateObjectStoreIngestPolicyResponse as DaemonUpdateObjectStoreIngestPolicyResponse,
    UpsertEndpointInventoryRequest as DaemonUpsertEndpointInventoryRequest,
    UpsertEndpointInventoryResponse as DaemonUpsertEndpointInventoryResponse,
    ENCLOSURE_PREPARE_CONFIRMATION, ENDPOINT_RECORD_CONFIRMATION, OBJECT_STORE_CREATE_CONFIRMATION,
};
use easyconnect_discovery::*;
use profile_catalogue::{
    preverified_host_profile_catalogue_import, standalone_profile_catalogue_export,
    standalone_profile_catalogue_import,
};
use profile_delete::standalone_profile_s3_delete;
use profile_download::standalone_profile_s3_get;
pub(crate) use profile_download::{provider_stream_download, synoptikon_provider_stream_download};
use profile_multipart::{
    standalone_profile_s3_multipart_complete, standalone_profile_s3_multipart_part,
    standalone_profile_s3_multipart_status,
};
use profile_upload::standalone_profile_s3_put;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Semaphore;

const MAX_PERFORMANCE_REPORT_WORKERS: usize = 2;

#[derive(Clone, Debug, Default, Deserialize)]
pub(super) struct ProfileS3ListQuery {
    prefix: Option<String>,
    offset: Option<u64>,
    limit: Option<u16>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(super) struct ProfileS3HeadQuery {
    key: Option<String>,
    version: Option<u64>,
}

#[derive(Clone, Debug, Deserialize)]
struct EasyconnectBrowserApprovalQuery {
    pairing_id: String,
    object_store: String,
    expires_at_utc: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct EasyconnectBrowserApprovalIntent {
    pairing_id: String,
    object_store: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuiApiHostMode {
    Standalone,
    SynoptikonIntegrated,
}

#[derive(Clone)]
pub(crate) struct StandaloneUsersGroupsRouteState {
    auth_store: LocalAuthStore,
    local_user_provider: Arc<dyn LocalUserAuthorityProvider>,
    local_group_admin_client: Option<Arc<dyn StandaloneLocalGroupAdminClient>>,
    groups_registry_path: PathBuf,
    daemon_bridge: Arc<crate::daemon_bridge::DaemonBridge>,
}

#[derive(Clone)]
pub(crate) struct StandaloneDashboardRouteState {
    auth_store: LocalAuthStore,
    local_user_provider: Arc<dyn LocalUserAuthorityProvider>,
}

#[derive(Clone)]
pub(crate) struct StandaloneEnclosureAdminRouteState {
    auth_store: LocalAuthStore,
    local_user_provider: Arc<dyn LocalUserAuthorityProvider>,
    enclosure_admin_client: Option<Arc<dyn StandaloneEnclosureAdminClient>>,
    daemon_bridge: Arc<crate::daemon_bridge::DaemonBridge>,
    priority_daemon_bridge: Arc<crate::daemon_bridge::DaemonBridge>,
}

/// Host-composed administrator mutation state.
///
/// This deliberately carries only the bounded daemon client and priority
/// bridge required by the migrated routes.  It has no local session store,
/// user lookup, password, PAM, group, or sudo authority.
#[derive(Clone)]
pub(crate) struct PreverifiedHostAdminRouteState {
    enclosure_admin_client: Arc<dyn StandaloneEnclosureAdminClient>,
    priority_daemon_bridge: Arc<crate::daemon_bridge::DaemonBridge>,
}

/// Host-composed portable catalogue import state.
///
/// A catalogue import commits daemon-owned metadata and is therefore a
/// priority operational mutation. The state deliberately contains no local
/// authentication, OS-user, group, sudo, PAM, or password dependency.
#[derive(Clone)]
pub(crate) struct PreverifiedHostProfileCatalogueRouteState {
    priority_daemon_bridge: Arc<crate::daemon_bridge::DaemonBridge>,
}

/// Host-composed report rebuilding state.
///
/// Report rendering does not issue a daemon mutation, so it carries only the
/// bounded worker capacity. Human authority is supplied exclusively by the
/// verified Pistis context at the route boundary.
#[derive(Clone)]
pub(crate) struct PreverifiedHostReportingRouteState {
    performance_report_workers: Arc<Semaphore>,
}

#[derive(Clone)]
pub(crate) struct StandaloneReportingRouteState {
    auth_store: LocalAuthStore,
    local_user_provider: Arc<dyn LocalUserAuthorityProvider>,
    performance_report_workers: Arc<Semaphore>,
}

impl StandaloneEnclosureAdminRouteState {
    fn system(auth_store: LocalAuthStore) -> Self {
        Self {
            auth_store,
            local_user_provider: Arc::new(SystemLocalUserAuthorityProvider),
            enclosure_admin_client: Some(Arc::new(
                DaemonStandaloneEnclosureAdminClient::default_packaged(),
            )),
            daemon_bridge: crate::daemon_bridge::DaemonBridge::shared_packaged(),
            priority_daemon_bridge: crate::daemon_bridge::DaemonBridge::shared_priority_packaged(),
        }
    }
}

impl PreverifiedHostAdminRouteState {
    pub(crate) fn packaged() -> Self {
        Self {
            enclosure_admin_client: Arc::new(
                DaemonStandaloneEnclosureAdminClient::default_packaged(),
            ),
            priority_daemon_bridge: crate::daemon_bridge::DaemonBridge::shared_priority_packaged(),
        }
    }
}

impl PreverifiedHostProfileCatalogueRouteState {
    pub(crate) fn packaged() -> Self {
        Self {
            priority_daemon_bridge: crate::daemon_bridge::DaemonBridge::shared_priority_packaged(),
        }
    }
}

impl PreverifiedHostReportingRouteState {
    pub(crate) fn packaged() -> Self {
        Self {
            performance_report_workers: Arc::new(Semaphore::new(MAX_PERFORMANCE_REPORT_WORKERS)),
        }
    }
}

impl StandaloneReportingRouteState {
    fn system(auth_store: LocalAuthStore) -> Self {
        Self {
            auth_store,
            local_user_provider: Arc::new(SystemLocalUserAuthorityProvider),
            performance_report_workers: Arc::new(Semaphore::new(MAX_PERFORMANCE_REPORT_WORKERS)),
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
            daemon_bridge: crate::daemon_bridge::DaemonBridge::shared_packaged(),
        }
    }
}

async fn standalone_home_dashboard(
    State(_state): State<StandaloneDashboardRouteState>,
    Query(query): Query<crate::routes::HomeDashboardQuery>,
    _actor: AuthenticatedGuiActor,
) -> Result<Json<crate::dashboard::HomeDashboardView>, (StatusCode, Json<AuthRouteError>)> {
    Ok(Json(
        crate::home_aggregator::live_home_dashboard_for_window(query.selected_window()),
    ))
}

/// Render live Home telemetry only for a matching host actor already verified
/// by Pistis. This host-composed route carries no standalone route state and
/// therefore cannot inspect a local session, password, PAM, POSIX user/group,
/// or sudo-derived identity.
async fn preverified_host_home_dashboard(
    Query(query): Query<crate::routes::HomeDashboardQuery>,
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<VerifiedHostAuthenticatedContext>,
) -> Result<Json<crate::dashboard::HomeDashboardView>, (StatusCode, Json<AuthRouteError>)> {
    require_preverified_host_viewer(&actor, &verified)?;
    Ok(Json(
        crate::home_aggregator::live_home_dashboard_for_window(query.selected_window()),
    ))
}

async fn standalone_live_status_workspace(
    State(daemon_bridge): State<Arc<crate::daemon_bridge::DaemonBridge>>,
    _actor: AuthenticatedGuiActor,
) -> Json<crate::LiveStatusWorkspaceView> {
    Json(crate::live_status::live_status_workspace(daemon_bridge).await)
}

async fn standalone_cached_home_dashboard(
    State(_state): State<StandaloneDashboardRouteState>,
    _actor: AuthenticatedGuiActor,
) -> Result<Json<crate::home_aggregator::CachedHomeDashboardView>, (StatusCode, Json<AuthRouteError>)>
{
    cached_home_dashboard_response()
}

fn cached_home_dashboard_response(
) -> Result<Json<crate::home_aggregator::CachedHomeDashboardView>, (StatusCode, Json<AuthRouteError>)>
{
    crate::home_aggregator::cached_home_dashboard()
        .map(Json)
        .map_err(|message| {
            route_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "dashboard_unavailable",
                message,
            )
        })
}

/// Read the last Home telemetry snapshot only for a matching verified Pistis
/// viewer. Cache availability remains a service condition, not an identity
/// fallback.
async fn preverified_host_cached_home_dashboard(
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<VerifiedHostAuthenticatedContext>,
) -> Result<Json<crate::home_aggregator::CachedHomeDashboardView>, (StatusCode, Json<AuthRouteError>)>
{
    require_preverified_host_viewer(&actor, &verified)?;
    cached_home_dashboard_response()
}

/// Read-only profile routes for a host that has already established Pistis
/// authority.  These wrappers retain the existing daemon adapters but reject
/// before they are invoked unless the actor has both the verified viewer role
/// and an exact store scope bound to that same host session.
pub(super) async fn preverified_host_profile_s3_list(
    Path(store_id): Path<String>,
    Query(query): Query<ProfileS3ListQuery>,
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<VerifiedHostAuthenticatedContext>,
    scope: Option<Extension<VerifiedHostStoreScope>>,
) -> Result<Json<DaemonProfileS3ListResponse>, (StatusCode, Json<AuthRouteError>)> {
    require_preverified_host_viewer_for_store(
        &actor,
        &verified,
        scope.as_ref().map(|value| &value.0),
        &store_id,
    )?;
    standalone_profile_s3_list(Path(store_id), Query(query), actor).await
}

pub(super) async fn preverified_host_profile_s3_head(
    Path(store_id): Path<String>,
    Query(query): Query<ProfileS3HeadQuery>,
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<VerifiedHostAuthenticatedContext>,
    scope: Option<Extension<VerifiedHostStoreScope>>,
) -> Result<Json<DaemonProfileS3HeadResponse>, (StatusCode, Json<AuthRouteError>)> {
    require_preverified_host_viewer_for_store(
        &actor,
        &verified,
        scope.as_ref().map(|value| &value.0),
        &store_id,
    )?;
    standalone_profile_s3_head(Path(store_id), Query(query), actor).await
}

pub(super) async fn preverified_host_profile_s3_verify(
    Path(store_id): Path<String>,
    Query(query): Query<ProfileS3HeadQuery>,
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<VerifiedHostAuthenticatedContext>,
    scope: Option<Extension<VerifiedHostStoreScope>>,
) -> Result<Json<DaemonProfileS3VerifyResponse>, (StatusCode, Json<AuthRouteError>)> {
    require_preverified_host_viewer_for_store(
        &actor,
        &verified,
        scope.as_ref().map(|value| &value.0),
        &store_id,
    )?;
    standalone_profile_s3_verify(Path(store_id), Query(query), actor).await
}

pub(super) async fn preverified_host_profile_s3_health(
    Path(store_id): Path<String>,
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<VerifiedHostAuthenticatedContext>,
    scope: Option<Extension<VerifiedHostStoreScope>>,
) -> Result<Json<DaemonProfileS3HealthResponse>, (StatusCode, Json<AuthRouteError>)> {
    require_preverified_host_viewer_for_store(
        &actor,
        &verified,
        scope.as_ref().map(|value| &value.0),
        &store_id,
    )?;
    standalone_profile_s3_health(Path(store_id), actor).await
}

pub(super) async fn preverified_host_profile_readiness(
    Path(store_id): Path<String>,
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<VerifiedHostAuthenticatedContext>,
    scope: Option<Extension<VerifiedHostStoreScope>>,
) -> Result<Json<DaemonProfileReadinessResponse>, (StatusCode, Json<AuthRouteError>)> {
    require_preverified_host_viewer_for_store(
        &actor,
        &verified,
        scope.as_ref().map(|value| &value.0),
        &store_id,
    )?;
    standalone_profile_readiness(Path(store_id), actor).await
}

pub(super) async fn preverified_host_profile_s3_diagnostics(
    Path(store_id): Path<String>,
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<VerifiedHostAuthenticatedContext>,
    scope: Option<Extension<VerifiedHostStoreScope>>,
) -> Result<Json<DaemonProfileDiagnosticsResponse>, (StatusCode, Json<AuthRouteError>)> {
    require_preverified_host_viewer_for_store(
        &actor,
        &verified,
        scope.as_ref().map(|value| &value.0),
        &store_id,
    )?;
    standalone_profile_s3_diagnostics(Path(store_id), actor).await
}

async fn standalone_store_capacity(
    Path(store_id): Path<String>,
    _actor: AuthenticatedGuiActor,
) -> Result<Json<DaemonCapacityStatusResponse>, (StatusCode, Json<AuthRouteError>)> {
    dashboard_store_capacity(store_id).await
}

/// Read daemon capacity for a matching verified Pistis viewer. The daemon
/// bridge remains the sole appliance authority; this route carries neither a
/// local authentication state nor an appliance-local identity assertion.
async fn preverified_host_store_capacity(
    Path(store_id): Path<String>,
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<VerifiedHostAuthenticatedContext>,
) -> Result<Json<DaemonCapacityStatusResponse>, (StatusCode, Json<AuthRouteError>)> {
    require_preverified_host_viewer(&actor, &verified)?;
    dashboard_store_capacity(store_id).await
}

async fn dashboard_store_capacity(
    store_id: String,
) -> Result<Json<DaemonCapacityStatusResponse>, (StatusCode, Json<AuthRouteError>)> {
    let bridge = crate::daemon_bridge::DaemonBridge::shared_packaged();
    bridge
        .call_message(move || {
            let client = DaemonClient::new(UnixSocketDaemonTransport::for_bounded_bridge(
                DaemonRuntimeConfig::default_packaged().socket_path,
            ));
            client
                .capacity_status(DaemonCapacityStatusRequest { store_id })
                .map_err(|error| error.to_string())
        })
        .await
        .map(Json)
        .map_err(|error| admin_daemon_bridge_error_with_code(error, "capacity_status_failed"))
}

async fn standalone_profile_s3_list(
    Path(store_id): Path<String>,
    Query(query): Query<ProfileS3ListQuery>,
    _actor: AuthenticatedGuiActor,
) -> Result<Json<DaemonProfileS3ListResponse>, (StatusCode, Json<AuthRouteError>)> {
    let store_id = StoreId::new(store_id).map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "profile_s3_invalid_store_id",
            error.to_string(),
        )
    })?;
    let request = DaemonProfileS3ListRequest {
        store_id,
        prefix: query.prefix,
        offset: query.offset.unwrap_or_default(),
        limit: query.limit.unwrap_or(100),
    };
    crate::daemon_bridge::DaemonBridge::shared_packaged()
        .call_message(move || {
            let client = DaemonClient::new(UnixSocketDaemonTransport::for_bounded_bridge(
                DaemonRuntimeConfig::default_packaged().socket_path,
            ));
            client
                .profile_s3_list(request)
                .map_err(|error| error.to_string())
        })
        .await
        .map(Json)
        .map_err(|error| admin_daemon_bridge_error_with_code(error, "profile_s3_list_failed"))
}

async fn standalone_profile_s3_head(
    Path(store_id): Path<String>,
    Query(query): Query<ProfileS3HeadQuery>,
    _actor: AuthenticatedGuiActor,
) -> Result<Json<DaemonProfileS3HeadResponse>, (StatusCode, Json<AuthRouteError>)> {
    let store_id = StoreId::new(store_id).map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "profile_s3_invalid_store_id",
            error.to_string(),
        )
    })?;
    let object_id = query.key.ok_or_else(|| {
        route_error(
            StatusCode::BAD_REQUEST,
            "profile_s3_invalid_key",
            "profile S3 HEAD requires a key query parameter",
        )
    })?;
    let request = DaemonProfileS3HeadRequest {
        store_id,
        key: BackendObjectKey {
            object_id,
            version: query.version.unwrap_or(1),
        },
    };
    crate::daemon_bridge::DaemonBridge::shared_packaged()
        .call_message(move || {
            let client = DaemonClient::new(UnixSocketDaemonTransport::for_bounded_bridge(
                DaemonRuntimeConfig::default_packaged().socket_path,
            ));
            client
                .profile_s3_head(request)
                .map_err(|error| error.to_string())
        })
        .await
        .map(Json)
        .map_err(|error| admin_daemon_bridge_error_with_code(error, "profile_s3_head_failed"))
}

async fn standalone_profile_s3_verify(
    Path(store_id): Path<String>,
    Query(query): Query<ProfileS3HeadQuery>,
    _actor: AuthenticatedGuiActor,
) -> Result<Json<DaemonProfileS3VerifyResponse>, (StatusCode, Json<AuthRouteError>)> {
    let store_id = StoreId::new(store_id).map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "profile_s3_invalid_store_id",
            error.to_string(),
        )
    })?;
    let object_id = query.key.ok_or_else(|| {
        route_error(
            StatusCode::BAD_REQUEST,
            "profile_s3_invalid_key",
            "profile S3 verification requires a key query parameter",
        )
    })?;
    let request = DaemonProfileS3VerifyRequest {
        store_id,
        key: BackendObjectKey {
            object_id,
            version: query.version.unwrap_or(1),
        },
    };
    crate::daemon_bridge::DaemonBridge::shared_packaged()
        .call_message(move || {
            let client = DaemonClient::new(UnixSocketDaemonTransport::for_bounded_bridge(
                DaemonRuntimeConfig::default_packaged().socket_path,
            ));
            client
                .profile_s3_verify(request)
                .map_err(|error| error.to_string())
        })
        .await
        .map(Json)
        .map_err(|error| admin_daemon_bridge_error_with_code(error, "profile_s3_verify_failed"))
}

async fn standalone_profile_s3_health(
    Path(store_id): Path<String>,
    _actor: AuthenticatedGuiActor,
) -> Result<Json<DaemonProfileS3HealthResponse>, (StatusCode, Json<AuthRouteError>)> {
    let store_id = StoreId::new(store_id).map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "profile_s3_invalid_store_id",
            error.to_string(),
        )
    })?;
    crate::daemon_bridge::DaemonBridge::shared_packaged()
        .call_message(move || {
            let client = DaemonClient::new(UnixSocketDaemonTransport::for_bounded_bridge(
                DaemonRuntimeConfig::default_packaged().socket_path,
            ));
            client
                .profile_s3_health(DaemonProfileS3HealthRequest { store_id })
                .map_err(|error| error.to_string())
        })
        .await
        .map(Json)
        .map_err(|error| admin_daemon_bridge_error_with_code(error, "profile_s3_health_failed"))
}

async fn standalone_profile_readiness(
    Path(store_id): Path<String>,
    _actor: AuthenticatedGuiActor,
) -> Result<Json<DaemonProfileReadinessResponse>, (StatusCode, Json<AuthRouteError>)> {
    let store_id = StoreId::new(store_id).map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "profile_readiness_invalid_store_id",
            error.to_string(),
        )
    })?;
    crate::daemon_bridge::DaemonBridge::shared_packaged()
        .call_message(move || {
            let client = DaemonClient::new(UnixSocketDaemonTransport::for_bounded_bridge(
                DaemonRuntimeConfig::default_packaged().socket_path,
            ));
            client
                .profile_readiness(DaemonProfileReadinessRequest { store_id })
                .map_err(|error| error.to_string())
        })
        .await
        .map(Json)
        .map_err(|error| admin_daemon_bridge_error_with_code(error, "profile_readiness_failed"))
}

async fn standalone_profile_capabilities(
    _actor: AuthenticatedGuiActor,
) -> Result<Json<DaemonProfileCapabilitiesResponse>, (StatusCode, Json<AuthRouteError>)> {
    crate::daemon_bridge::DaemonBridge::shared_packaged()
        .call_message(move || {
            let client = DaemonClient::new(UnixSocketDaemonTransport::for_bounded_bridge(
                DaemonRuntimeConfig::default_packaged().socket_path,
            ));
            client
                .profile_capabilities(DaemonProfileCapabilitiesRequest::default())
                .map_err(|error| error.to_string())
        })
        .await
        .map(Json)
        .map_err(|error| admin_daemon_bridge_error_with_code(error, "profile_capabilities_failed"))
}

async fn standalone_profile_s3_diagnostics(
    Path(store_id): Path<String>,
    _actor: AuthenticatedGuiActor,
) -> Result<Json<DaemonProfileDiagnosticsResponse>, (StatusCode, Json<AuthRouteError>)> {
    let store_id = StoreId::new(store_id).map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "profile_s3_invalid_store_id",
            error.to_string(),
        )
    })?;
    crate::daemon_bridge::DaemonBridge::shared_packaged()
        .call_message(move || {
            let client = DaemonClient::new(UnixSocketDaemonTransport::for_bounded_bridge(
                DaemonRuntimeConfig::default_packaged().socket_path,
            ));
            client
                .profile_diagnostics(DaemonProfileDiagnosticsRequest { store_id })
                .map_err(|error| error.to_string())
        })
        .await
        .map(Json)
        .map_err(|error| {
            admin_daemon_bridge_error_with_code(error, "profile_s3_diagnostics_failed")
        })
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

/// Render the enclosure dashboard for a host actor that Monas or Synoptikon
/// has already verified with Pistis.
///
/// This host-composed route has no local authentication store, local-user
/// provider, POSIX group, or sudo lookup. A matching verified DAS viewer role
/// admits dashboard visibility, while the closed administrator role alone
/// controls the existing administrator affordance in the response.
async fn preverified_host_enclosures_dashboard(
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<VerifiedHostAuthenticatedContext>,
) -> Result<Json<crate::dashboard::EnclosuresPageView>, (StatusCode, Json<AuthRouteError>)> {
    require_preverified_host_viewer(&actor, &verified)?;
    let administrator = DasRolePolicy::from_verified(&verified).permits(DasCapability::Administer);
    Ok(Json(
        crate::enclosures_aggregator::live_enclosures_dashboard_for_administrator(administrator),
    ))
}

/// Render the ObjectStores dashboard for a host actor already verified by
/// Pistis.  This route intentionally does not inspect a local user, POSIX
/// group, sudo state, or local session.  A closed verified DAS viewer role
/// admits visibility; the administrator role controls only the create-store
/// affordance. Legacy writer-group membership is deliberately not inferred.
async fn preverified_host_object_stores_dashboard(
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<VerifiedHostAuthenticatedContext>,
) -> Result<Json<crate::dashboard::ObjectStoresPageView>, (StatusCode, Json<AuthRouteError>)> {
    require_preverified_host_viewer(&actor, &verified)?;
    let administrator = DasRolePolicy::from_verified(&verified).permits(DasCapability::Administer);
    Ok(Json(
        crate::object_stores_aggregator::live_object_stores_dashboard_for_verified_pistis(
            administrator,
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

    let mut view = UsersGroupsWorkspaceView::standalone(
        current_user,
        users,
        groups_snapshot.path.display().to_string(),
        groups_snapshot.groups,
        warnings,
    );
    let mut qualification_warnings = Vec::new();
    for user in &mut view.users {
        match state.local_user_provider.local_user(&user.username) {
            Ok(authority) => {
                user.qualification_state = "qualified".to_string();
                user.groups = authority.groups;
                user.sudo_administrator = authority.sudo_administrator;
            }
            Err(error) => {
                user.qualification_state = if user.registered {
                    "registered".to_string()
                } else {
                    "unqualified".to_string()
                };
                qualification_warnings.push(DashboardWarning {
                    code: "local_user_qualification_unavailable".to_string(),
                    message: format!(
                        "Local authority for {} could not be qualified: {}",
                        user.username, error
                    ),
                });
            }
        }
    }
    view.warnings.extend(qualification_warnings);

    Ok(Json(view))
}

fn actor_local_user_for_workspace(
    local_user_provider: &dyn LocalUserAuthorityProvider,
    actor: &AuthenticatedGuiActor,
) -> Result<crate::LocalUserMetadata, String> {
    if !actor.authority.uses_local_os_policy() {
        return Err(
            "an appliance-local or Monas standalone OS identity is required to inspect local authority."
                .to_string(),
        );
    }
    local_user_provider
        .local_user(&actor.subject_id)
        .map_err(|err| err.to_string())
}

async fn create_local_group(
    State(state): State<StandaloneUsersGroupsRouteState>,
    actor: AuthenticatedGuiActor,
    Json(request): Json<CreateLocalGroupRequest>,
) -> Result<Json<StandaloneLocalGroupAdminResponse>, (StatusCode, Json<AuthRouteError>)> {
    let mut request = validate_create_local_group_request(request)?;
    let current_user = require_local_administrator(state.local_user_provider.as_ref(), &actor)?;
    request.administrator_actor = Some(current_user.username);
    submit_local_group_admin_request(&state, request)
        .await
        .map(Json)
}

async fn assign_local_user_to_group(
    State(state): State<StandaloneUsersGroupsRouteState>,
    actor: AuthenticatedGuiActor,
    Json(request): Json<AssignLocalUserToGroupRequest>,
) -> Result<Json<StandaloneLocalGroupAdminResponse>, (StatusCode, Json<AuthRouteError>)> {
    let mut request = validate_assign_local_user_to_group_request(request)?;
    let current_user = require_local_administrator(state.local_user_provider.as_ref(), &actor)?;
    request.administrator_actor = Some(current_user.username);
    submit_local_group_admin_request(&state, request)
        .await
        .map(Json)
}

async fn prepare_enclosure(
    State(state): State<StandaloneEnclosureAdminRouteState>,
    actor: AuthenticatedGuiActor,
    Json(request): Json<PrepareEnclosureRequest>,
) -> Result<Json<StandaloneEnclosurePrepareResponse>, (StatusCode, Json<AuthRouteError>)> {
    let mut request = validate_prepare_enclosure_request(request)?;
    let current_user = require_local_administrator(state.local_user_provider.as_ref(), &actor)?;
    request.administrator_actor = Some(current_user.username);
    submit_prepare_enclosure_request(&state, request)
        .await
        .map(Json)
}

async fn create_object_store(
    State(state): State<StandaloneEnclosureAdminRouteState>,
    actor: AuthenticatedGuiActor,
    Json(request): Json<CreateObjectStoreRequest>,
) -> Result<Json<StandaloneCreateObjectStoreResponse>, (StatusCode, Json<AuthRouteError>)> {
    let mut request = validate_create_object_store_request(request)?;
    let current_user = require_local_administrator(state.local_user_provider.as_ref(), &actor)?;
    request.administrator_actor = Some(current_user.username);
    submit_create_object_store_request(&state, request)
        .await
        .map(Json)
}

async fn update_object_store_ingest_policy(
    State(state): State<StandaloneEnclosureAdminRouteState>,
    actor: AuthenticatedGuiActor,
    Json(request): Json<ObjectStoreIngestPolicyRequest>,
) -> Result<Json<StandaloneObjectStoreIngestPolicyResponse>, (StatusCode, Json<AuthRouteError>)> {
    let mut request = validate_object_store_ingest_policy_request(request)?;
    let current_user = require_local_administrator(state.local_user_provider.as_ref(), &actor)?;
    request.administrator_actor = Some(current_user.username);
    submit_update_object_store_ingest_policy_request(&state, request)
        .await
        .map(Json)
}

async fn control_ingest(
    State(state): State<StandaloneEnclosureAdminRouteState>,
    actor: AuthenticatedGuiActor,
    Json(request): Json<IngestControlRequest>,
) -> Result<Json<IngestControlResponse>, (StatusCode, Json<AuthRouteError>)> {
    require_local_administrator(state.local_user_provider.as_ref(), &actor)?;
    let request = validate_ingest_control_request(request)?;
    submit_ingest_control_request(&state, request)
        .await
        .map(Json)
}

/// Execute an ingest-control mutation for a host actor that Monas or
/// Synoptikon has already verified with Pistis.
///
/// This route deliberately has no `LocalAuthStore`, local-user provider,
/// password, PAM, POSIX group, or sudo dependency.  A matching verified
/// context and the closed DAS administrator role are both required before the
/// request can reach the priority daemon bridge.
async fn preverified_host_control_ingest(
    State(state): State<PreverifiedHostAdminRouteState>,
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<VerifiedHostAuthenticatedContext>,
    Json(request): Json<IngestControlRequest>,
) -> Result<Json<IngestControlResponse>, (StatusCode, Json<AuthRouteError>)> {
    require_preverified_host_administrator(&actor, &verified)?;
    let mut request = validate_ingest_control_request(request)?;
    request.verified_subject = Some(actor.subject_id);
    submit_preverified_host_ingest_control_request(&state, request)
        .await
        .map(Json)
}

async fn preverified_host_store_drain(
    State(state): State<PreverifiedHostAdminRouteState>,
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<VerifiedHostAuthenticatedContext>,
    Json(request): Json<HostStoreDrainRequest>,
) -> Result<Json<HostStoreDrainResponse>, (StatusCode, Json<AuthRouteError>)> {
    require_preverified_host_administrator(&actor, &verified)?;
    let mut request = validate_host_store_drain_request(request)?;
    request.verified_subject = Some(actor.subject_id);
    submit_preverified_host_store_drain_request(&state, request)
        .await
        .map(Json)
}

async fn preverified_host_store_delete(
    State(state): State<PreverifiedHostAdminRouteState>,
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<VerifiedHostAuthenticatedContext>,
    Json(request): Json<HostStoreDeleteRequest>,
) -> Result<Json<HostStoreDeleteResponse>, (StatusCode, Json<AuthRouteError>)> {
    require_preverified_host_administrator(&actor, &verified)?;
    let mut request = validate_host_store_delete_request(request)?;
    request.verified_subject = Some(actor.subject_id);
    submit_preverified_host_store_delete_request(&state, request)
        .await
        .map(Json)
}

async fn preverified_host_store_repair(
    State(state): State<PreverifiedHostAdminRouteState>,
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<VerifiedHostAuthenticatedContext>,
    Json(request): Json<HostStoreRepairRequest>,
) -> Result<Json<HostStoreRepairResponse>, (StatusCode, Json<AuthRouteError>)> {
    require_preverified_host_administrator(&actor, &verified)?;
    let mut request = validate_host_store_repair_request(request)?;
    request.verified_subject = Some(actor.subject_id);
    submit_preverified_host_store_repair_request(&state, request)
        .await
        .map(Json)
}

async fn preverified_host_store_deduplicate(
    State(state): State<PreverifiedHostAdminRouteState>,
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<VerifiedHostAuthenticatedContext>,
    Json(request): Json<HostStoreDeduplicateRequest>,
) -> Result<Json<HostStoreDeduplicateResponse>, (StatusCode, Json<AuthRouteError>)> {
    require_preverified_host_administrator(&actor, &verified)?;
    let mut request = validate_host_store_deduplicate_request(request)?;
    request.verified_subject = Some(actor.subject_id);
    submit_preverified_host_store_deduplicate_request(&state, request)
        .await
        .map(Json)
}

async fn preverified_host_ingest_queue_drain(
    State(state): State<PreverifiedHostAdminRouteState>,
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<VerifiedHostAuthenticatedContext>,
    Json(request): Json<HostIngestQueueDrainRequest>,
) -> Result<Json<HostIngestQueueDrainResponse>, (StatusCode, Json<AuthRouteError>)> {
    require_preverified_host_administrator(&actor, &verified)?;
    let mut request = validate_host_ingest_queue_drain_request(request)?;
    request.verified_subject = Some(actor.subject_id);
    submit_preverified_host_ingest_queue_drain_request(&state, request)
        .await
        .map(Json)
}

async fn preverified_host_update_object_store_ingest_policy(
    State(state): State<PreverifiedHostAdminRouteState>,
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<VerifiedHostAuthenticatedContext>,
    Json(request): Json<ObjectStoreIngestPolicyRequest>,
) -> Result<Json<StandaloneObjectStoreIngestPolicyResponse>, (StatusCode, Json<AuthRouteError>)> {
    require_preverified_host_administrator(&actor, &verified)?;
    let mut request = validate_object_store_ingest_policy_request(request)?;
    request.administrator_actor = Some(actor.subject_id);
    submit_preverified_host_ingest_policy_request(&state, request)
        .await
        .map(Json)
}

/// Create an ObjectStore for a host actor already verified by Pistis.
///
/// Creation retains the existing explicit policy validation and confirmation
/// marker, but authority is derived only from the matching verified host
/// subject and closed DAS administrator role. No local session, password,
/// PAM, POSIX group, or sudo lookup participates in this route.
async fn preverified_host_create_object_store(
    State(state): State<PreverifiedHostAdminRouteState>,
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<VerifiedHostAuthenticatedContext>,
    Json(request): Json<CreateObjectStoreRequest>,
) -> Result<Json<StandaloneCreateObjectStoreResponse>, (StatusCode, Json<AuthRouteError>)> {
    require_preverified_host_administrator(&actor, &verified)?;
    let mut request = validate_create_object_store_request(request)?;
    request.administrator_actor = Some(actor.subject_id);
    submit_preverified_host_create_object_store_request(&state, request)
        .await
        .map(Json)
}

/// Prepare an enclosure for a host actor already verified by Pistis.
///
/// This destructive operation retains the existing format and data-loss
/// acknowledgement validation. Its authority is exclusively the matching
/// verified host subject and closed DAS administrator role; no local session,
/// password, PAM, POSIX group, or sudo-derived identity is consulted.
async fn preverified_host_prepare_enclosure(
    State(state): State<PreverifiedHostAdminRouteState>,
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<VerifiedHostAuthenticatedContext>,
    Json(request): Json<PrepareEnclosureRequest>,
) -> Result<Json<StandaloneEnclosurePrepareResponse>, (StatusCode, Json<AuthRouteError>)> {
    require_preverified_host_administrator(&actor, &verified)?;
    let mut request = validate_prepare_enclosure_request(request)?;
    request.administrator_actor = Some(actor.subject_id);
    submit_preverified_host_prepare_enclosure_request(&state, request)
        .await
        .map(Json)
}

/// Test a registered endpoint for a host actor already verified by Pistis.
///
/// This state carries only the daemon client and priority bridge. Authority
/// derives exclusively from the matching verified host subject and closed DAS
/// role policy, never a local session, password, PAM result, or OS identity.
async fn preverified_host_test_endpoint_connection(
    State(state): State<PreverifiedHostAdminRouteState>,
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<VerifiedHostAuthenticatedContext>,
    Json(request): Json<EndpointConnectionTestRequest>,
) -> Result<Json<StandaloneEndpointConnectionTestResponse>, (StatusCode, Json<AuthRouteError>)> {
    require_preverified_host_administrator(&actor, &verified)?;
    let request = dasobjectstore_daemon::TestEndpointConnectionRequest {
        endpoint_id: request.endpoint_id,
    };
    request.validate().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_endpoint_connection_test",
            error.to_string(),
        )
    })?;
    submit_preverified_host_endpoint_connection_test_request(&state, request)
        .await
        .map(Json)
}

/// Record a validated endpoint inventory entry for a host actor already
/// verified by Pistis.
///
/// The actor subject is recorded only after the verified-context and closed
/// DAS administrator-role checks succeed.  Like the other host-composed
/// mutations, this route has no local session, password, PAM, POSIX group, or
/// sudo-derived authority.
async fn preverified_host_upsert_endpoint_inventory(
    State(state): State<PreverifiedHostAdminRouteState>,
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<VerifiedHostAuthenticatedContext>,
    Json(request): Json<EndpointInventoryUpsertRequest>,
) -> Result<Json<StandaloneEndpointInventoryUpsertResponse>, (StatusCode, Json<AuthRouteError>)> {
    require_preverified_host_administrator(&actor, &verified)?;
    let mut request = validate_endpoint_inventory_upsert_request(request)?;
    request.administrator_actor = Some(actor.subject_id);
    submit_preverified_host_endpoint_inventory_upsert_request(&state, request)
        .await
        .map(Json)
}

/// Read an administrator job for a host actor already verified by Pistis.
///
/// Job state is operational data: it is never inferred from a local session,
/// password, PAM result, POSIX group, or sudo policy.  The matching verified
/// subject and closed DAS administrator role are required before the daemon
/// bridge is invoked.
async fn preverified_host_admin_job_status(
    State(state): State<PreverifiedHostAdminRouteState>,
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<VerifiedHostAuthenticatedContext>,
    Path(job_id): Path<String>,
) -> Result<Json<StandaloneAdminJobStatusResponse>, (StatusCode, Json<AuthRouteError>)> {
    require_preverified_host_administrator(&actor, &verified)?;
    let request = StandaloneAdminJobStatusDaemonRequest {
        job_id: required_field("job_id", job_id)?,
    };
    submit_preverified_host_admin_job_status_request(&state, request)
        .await
        .map(Json)
}

/// Cancel an administrator job for a host actor already verified by Pistis.
///
/// Cancellation remains a priority daemon operation, but its authority is
/// exclusively the verified Pistis administrator context.
async fn preverified_host_cancel_admin_job(
    State(state): State<PreverifiedHostAdminRouteState>,
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<VerifiedHostAuthenticatedContext>,
    Path(job_id): Path<String>,
    Json(request): Json<CancelAdminJobRequest>,
) -> Result<Json<StandaloneAdminJobCancelResponse>, (StatusCode, Json<AuthRouteError>)> {
    require_preverified_host_administrator(&actor, &verified)?;
    let request = validate_cancel_admin_job_request(job_id, request)?;
    submit_preverified_host_admin_job_cancel_request(&state, request)
        .await
        .map(Json)
}

pub(crate) fn require_preverified_host_operator(
    actor: &AuthenticatedGuiActor,
    verified: &VerifiedHostAuthenticatedContext,
) -> Result<(), (StatusCode, Json<AuthRouteError>)> {
    require_preverified_host_capability(actor, verified, DasCapability::Operate, "operator")
}

/// Require the closed DAS viewer role and an exact ObjectStore scope that is
/// bound to the same verified Pistis session.  A broad host role, local
/// browser session, cookie, OS user, or guessed store identifier is never a
/// substitute for the scope extension supplied by the embedding host.
pub(crate) fn require_preverified_host_viewer_for_store(
    actor: &AuthenticatedGuiActor,
    verified: &VerifiedHostAuthenticatedContext,
    scope: Option<&VerifiedHostStoreScope>,
    store_id: &str,
) -> Result<(), (StatusCode, Json<AuthRouteError>)> {
    require_preverified_host_capability(actor, verified, DasCapability::View, "viewer")?;
    let Some(scope) = scope else {
        return Err(route_error(
            StatusCode::FORBIDDEN,
            "host_store_scope_required",
            "an exact verified ObjectStore scope is required for this operation",
        ));
    };
    if !scope.permits(verified, store_id) {
        return Err(route_error(
            StatusCode::FORBIDDEN,
            "host_store_scope_denied",
            "the verified host session is not authorised for this ObjectStore",
        ));
    }
    Ok(())
}

/// Require an operator role plus the exact in-process ObjectStore/prefix
/// grant that was bound to the same verified Pistis session.  Multipart
/// writes never inherit local browser, OS, provider, PAM, or sudo authority.
pub(crate) fn require_preverified_host_operator_for_object_prefix(
    actor: &AuthenticatedGuiActor,
    verified: &VerifiedHostAuthenticatedContext,
    scope: Option<&VerifiedHostObjectPrefixScope>,
    store_id: &str,
    object_id: &str,
) -> Result<(), (StatusCode, Json<AuthRouteError>)> {
    require_preverified_host_operator(actor, verified)?;
    let Some(scope) = scope else {
        return Err(route_error(
            StatusCode::FORBIDDEN,
            "host_object_prefix_scope_required",
            "an exact verified ObjectStore prefix scope is required for multipart operations",
        ));
    };
    if !scope.permits(verified, store_id, object_id) {
        return Err(route_error(
            StatusCode::FORBIDDEN,
            "host_object_prefix_scope_denied",
            "the verified host session is not authorised for this ObjectStore object prefix",
        ));
    }
    Ok(())
}

/// Require a viewer role plus the exact in-process ObjectStore/prefix grant
/// bound to this verified Pistis session. Provider-stream reads use this
/// narrower rule rather than inheriting a standalone S3 or POSIX identity.
pub(crate) fn require_preverified_host_viewer_for_object_prefix(
    actor: &AuthenticatedGuiActor,
    verified: &VerifiedHostAuthenticatedContext,
    scope: Option<&VerifiedHostObjectPrefixScope>,
    store_id: &str,
    object_id: &str,
) -> Result<(), (StatusCode, Json<AuthRouteError>)> {
    require_preverified_host_capability(actor, verified, DasCapability::View, "viewer")?;
    let Some(scope) = scope else {
        return Err(route_error(
            StatusCode::FORBIDDEN,
            "host_object_prefix_scope_required",
            "an exact verified ObjectStore prefix scope is required for provider-stream reads",
        ));
    };
    if !scope.permits(verified, store_id, object_id) {
        return Err(route_error(
            StatusCode::FORBIDDEN,
            "host_object_prefix_scope_denied",
            "the verified host session is not authorised for this ObjectStore object prefix",
        ));
    }
    Ok(())
}

/// Require the closed DAS viewer role for host-wide read-only views. This
/// admits only a matching verified Pistis context; local browser sessions,
/// passwords, PAM, POSIX users/groups, and sudo policy never participate.
pub(crate) fn require_preverified_host_viewer(
    actor: &AuthenticatedGuiActor,
    verified: &VerifiedHostAuthenticatedContext,
) -> Result<(), (StatusCode, Json<AuthRouteError>)> {
    require_preverified_host_capability(actor, verified, DasCapability::View, "viewer")
}

pub(super) fn require_preverified_host_administrator(
    actor: &AuthenticatedGuiActor,
    verified: &VerifiedHostAuthenticatedContext,
) -> Result<(), (StatusCode, Json<AuthRouteError>)> {
    require_preverified_host_capability(actor, verified, DasCapability::Administer, "administrator")
}

fn require_preverified_host_capability(
    actor: &AuthenticatedGuiActor,
    verified: &VerifiedHostAuthenticatedContext,
    capability: DasCapability,
    role_name: &str,
) -> Result<(), (StatusCode, Json<AuthRouteError>)> {
    if actor.subject_id != verified.context().subject_id {
        return Err(route_error(
            StatusCode::FORBIDDEN,
            "host_actor_subject_mismatch",
            "the supplied host actor does not match the verified Pistis context",
        ));
    }
    if !DasRolePolicy::from_verified(verified).permits(capability) {
        return Err(route_error(
            StatusCode::FORBIDDEN,
            match capability {
                DasCapability::View => "host_viewer_role_required",
                DasCapability::Operate => "host_operator_role_required",
                DasCapability::Administer => "host_administrator_role_required",
            },
            format!("a verified DAS storage_{role_name} role is required for this operation"),
        ));
    }
    Ok(())
}

async fn submit_preverified_host_ingest_control_request(
    state: &PreverifiedHostAdminRouteState,
    request: StandaloneIngestControlDaemonRequest,
) -> Result<IngestControlResponse, (StatusCode, Json<AuthRouteError>)> {
    let client = Arc::clone(&state.enclosure_admin_client);
    state
        .priority_daemon_bridge
        .clone()
        .call_message(move || {
            client
                .submit_ingest_control(request)
                .map_err(|error| error.message)
        })
        .await
        .map_err(|error| admin_daemon_bridge_error_with_code(error, "ingest_control_failed"))
}

async fn submit_preverified_host_store_drain_request(
    state: &PreverifiedHostAdminRouteState,
    request: dasobjectstore_daemon::StoreDrainRequest,
) -> Result<HostStoreDrainResponse, (StatusCode, Json<AuthRouteError>)> {
    let client = Arc::clone(&state.enclosure_admin_client);
    state
        .priority_daemon_bridge
        .clone()
        .call_message(move || client.store_drain(request).map_err(|error| error.message))
        .await
        .map_err(|error| admin_daemon_bridge_error_with_code(error, "store_drain_failed"))
}

async fn submit_preverified_host_store_delete_request(
    state: &PreverifiedHostAdminRouteState,
    request: dasobjectstore_daemon::StoreDeleteRequest,
) -> Result<HostStoreDeleteResponse, (StatusCode, Json<AuthRouteError>)> {
    let client = Arc::clone(&state.enclosure_admin_client);
    state
        .priority_daemon_bridge
        .clone()
        .call_message(move || client.store_delete(request).map_err(|error| error.message))
        .await
        .map_err(|error| admin_daemon_bridge_error_with_code(error, "store_delete_failed"))
}

async fn submit_preverified_host_store_repair_request(
    state: &PreverifiedHostAdminRouteState,
    request: dasobjectstore_daemon::StoreRepairRequest,
) -> Result<HostStoreRepairResponse, (StatusCode, Json<AuthRouteError>)> {
    let client = Arc::clone(&state.enclosure_admin_client);
    state
        .priority_daemon_bridge
        .clone()
        .call_message(move || client.store_repair(request).map_err(|error| error.message))
        .await
        .map_err(|error| admin_daemon_bridge_error_with_code(error, "store_repair_failed"))
}

async fn submit_preverified_host_store_deduplicate_request(
    state: &PreverifiedHostAdminRouteState,
    request: dasobjectstore_daemon::StoreDeduplicateRequest,
) -> Result<HostStoreDeduplicateResponse, (StatusCode, Json<AuthRouteError>)> {
    let client = Arc::clone(&state.enclosure_admin_client);
    state
        .priority_daemon_bridge
        .clone()
        .call_message(move || {
            client
                .store_deduplicate(request)
                .map_err(|error| error.message)
        })
        .await
        .map_err(|error| admin_daemon_bridge_error_with_code(error, "store_deduplicate_failed"))
}

async fn submit_preverified_host_ingest_queue_drain_request(
    state: &PreverifiedHostAdminRouteState,
    request: dasobjectstore_daemon::IngestQueueDrainRequest,
) -> Result<HostIngestQueueDrainResponse, (StatusCode, Json<AuthRouteError>)> {
    let client = Arc::clone(&state.enclosure_admin_client);
    state
        .priority_daemon_bridge
        .clone()
        .call_message(move || {
            client
                .ingest_queue_drain(request)
                .map_err(|error| error.message)
        })
        .await
        .map_err(|error| admin_daemon_bridge_error_with_code(error, "ingest_queue_drain_failed"))
}

async fn submit_preverified_host_ingest_policy_request(
    state: &PreverifiedHostAdminRouteState,
    request: DaemonUpdateObjectStoreIngestPolicyRequest,
) -> Result<StandaloneObjectStoreIngestPolicyResponse, (StatusCode, Json<AuthRouteError>)> {
    let client = Arc::clone(&state.enclosure_admin_client);
    state
        .priority_daemon_bridge
        .clone()
        .call_message(move || {
            client
                .submit_update_object_store_ingest_policy(request)
                .map_err(|error| error.message)
        })
        .await
        .map_err(|error| admin_daemon_bridge_error_with_code(error, "ingest_policy_update_failed"))
}

async fn submit_preverified_host_create_object_store_request(
    state: &PreverifiedHostAdminRouteState,
    request: DaemonCreateObjectStoreRequest,
) -> Result<StandaloneCreateObjectStoreResponse, (StatusCode, Json<AuthRouteError>)> {
    let client = Arc::clone(&state.enclosure_admin_client);
    state
        .priority_daemon_bridge
        .clone()
        .call_message(move || {
            client
                .submit_create_object_store(request)
                .map_err(|error| error.message)
        })
        .await
        .map_err(|error| admin_daemon_bridge_error_with_code(error, "objectstore_create_failed"))
}

async fn submit_preverified_host_prepare_enclosure_request(
    state: &PreverifiedHostAdminRouteState,
    request: StandaloneEnclosurePrepareDaemonRequest,
) -> Result<StandaloneEnclosurePrepareResponse, (StatusCode, Json<AuthRouteError>)> {
    let client = Arc::clone(&state.enclosure_admin_client);
    state
        .priority_daemon_bridge
        .clone()
        .call_message(move || {
            client
                .submit_prepare_enclosure(request)
                .map_err(|error| error.message)
        })
        .await
        .map_err(|error| {
            admin_daemon_bridge_error_with_code(error, "daemon_enclosure_prepare_failed")
        })
}

async fn submit_preverified_host_endpoint_connection_test_request(
    state: &PreverifiedHostAdminRouteState,
    request: dasobjectstore_daemon::TestEndpointConnectionRequest,
) -> Result<StandaloneEndpointConnectionTestResponse, (StatusCode, Json<AuthRouteError>)> {
    let client = Arc::clone(&state.enclosure_admin_client);
    state
        .priority_daemon_bridge
        .clone()
        .call_message(move || {
            client
                .test_endpoint_connection(request)
                .map_err(|error| error.message)
        })
        .await
        .map_err(|error| {
            admin_daemon_bridge_error_with_code(error, "endpoint_connection_test_failed")
        })
}

async fn submit_preverified_host_endpoint_inventory_upsert_request(
    state: &PreverifiedHostAdminRouteState,
    request: DaemonUpsertEndpointInventoryRequest,
) -> Result<StandaloneEndpointInventoryUpsertResponse, (StatusCode, Json<AuthRouteError>)> {
    let client = Arc::clone(&state.enclosure_admin_client);
    state
        .priority_daemon_bridge
        .clone()
        .call_message(move || {
            client
                .submit_endpoint_inventory_upsert(request)
                .map_err(|error| error.message)
        })
        .await
        .map_err(|error| {
            admin_daemon_bridge_error_with_code(error, "endpoint_inventory_upsert_failed")
        })
}

async fn submit_preverified_host_admin_job_status_request(
    state: &PreverifiedHostAdminRouteState,
    request: StandaloneAdminJobStatusDaemonRequest,
) -> Result<StandaloneAdminJobStatusResponse, (StatusCode, Json<AuthRouteError>)> {
    let client = Arc::clone(&state.enclosure_admin_client);
    state
        .priority_daemon_bridge
        .clone()
        .call_message(move || client.job_status(request).map_err(|error| error.message))
        .await
        .map_err(|error| admin_daemon_bridge_error_with_code(error, "admin_job_status_failed"))
}

async fn submit_preverified_host_admin_job_cancel_request(
    state: &PreverifiedHostAdminRouteState,
    request: StandaloneAdminJobCancelDaemonRequest,
) -> Result<StandaloneAdminJobCancelResponse, (StatusCode, Json<AuthRouteError>)> {
    let client = Arc::clone(&state.enclosure_admin_client);
    state
        .priority_daemon_bridge
        .clone()
        .call_message(move || client.cancel_job(request).map_err(|error| error.message))
        .await
        .map_err(|error| admin_daemon_bridge_error_with_code(error, "admin_job_cancel_failed"))
}

async fn upsert_endpoint_inventory(
    State(state): State<StandaloneEnclosureAdminRouteState>,
    actor: AuthenticatedGuiActor,
    Json(request): Json<EndpointInventoryUpsertRequest>,
) -> Result<Json<StandaloneEndpointInventoryUpsertResponse>, (StatusCode, Json<AuthRouteError>)> {
    let mut request = validate_endpoint_inventory_upsert_request(request)?;
    let current_user = require_local_administrator(state.local_user_provider.as_ref(), &actor)?;
    request.administrator_actor = Some(current_user.username);
    submit_endpoint_inventory_upsert_request(&state, request)
        .await
        .map(Json)
}

async fn test_endpoint_connection(
    State(state): State<StandaloneEnclosureAdminRouteState>,
    actor: AuthenticatedGuiActor,
    Json(request): Json<EndpointConnectionTestRequest>,
) -> Result<Json<StandaloneEndpointConnectionTestResponse>, (StatusCode, Json<AuthRouteError>)> {
    require_local_administrator(state.local_user_provider.as_ref(), &actor)?;
    let request = dasobjectstore_daemon::TestEndpointConnectionRequest {
        endpoint_id: request.endpoint_id,
    };
    request.validate().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "invalid_endpoint_connection_test",
            error.to_string(),
        )
    })?;
    submit_endpoint_connection_test_request(&state, request)
        .await
        .map(Json)
}

async fn rebuild_performance_report(
    State(state): State<StandaloneReportingRouteState>,
    actor: AuthenticatedGuiActor,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, (StatusCode, Json<AuthRouteError>)> {
    let current_user = require_local_administrator(state.local_user_provider.as_ref(), &actor)?;
    rebuild_performance_report_for_actor(
        state.performance_report_workers,
        current_user.username,
        headers,
        body,
    )
    .await
}

/// Rebuild a report for an already verified Pistis host administrator.
///
/// The report renderer is local compute rather than a storage daemon mutation,
/// but access is still a human operational action. No local session, password,
/// PAM, POSIX group, or sudo-derived authority is consulted.
async fn preverified_host_rebuild_performance_report(
    State(state): State<PreverifiedHostReportingRouteState>,
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<VerifiedHostAuthenticatedContext>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, (StatusCode, Json<AuthRouteError>)> {
    require_preverified_host_administrator(&actor, &verified)?;
    rebuild_performance_report_for_actor(
        state.performance_report_workers,
        actor.subject_id,
        headers,
        body,
    )
    .await
}

async fn rebuild_performance_report_for_actor(
    performance_report_workers: Arc<Semaphore>,
    operator: String,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, (StatusCode, Json<AuthRouteError>)> {
    let uploaded_filename = headers
        .get("x-dasobjectstore-filename")
        .and_then(|value| value.to_str().ok());
    let uploaded_filename = uploaded_filename.map(str::to_owned);
    let permit = performance_report_workers
        .try_acquire_owned()
        .map_err(|_| {
            route_error(
                StatusCode::TOO_MANY_REQUESTS,
                "performance_report_busy",
                "performance report capacity is saturated; retry shortly",
            )
        })?;
    let report = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        crate::reporting::rebuild_performance_report_pdf_from_upload(
            &body,
            uploaded_filename.as_deref(),
            &operator,
        )
    })
    .await
    .map_err(|error| {
        route_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "performance_report_worker_failed",
            error.to_string(),
        )
    })?
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
    submit_admin_job_status_request(&state, request)
        .await
        .map(Json)
}

async fn cancel_admin_job(
    State(state): State<StandaloneEnclosureAdminRouteState>,
    actor: AuthenticatedGuiActor,
    Path(job_id): Path<String>,
    Json(request): Json<CancelAdminJobRequest>,
) -> Result<Json<StandaloneAdminJobCancelResponse>, (StatusCode, Json<AuthRouteError>)> {
    require_local_administrator(state.local_user_provider.as_ref(), &actor)?;
    let request = validate_cancel_admin_job_request(job_id, request)?;
    submit_admin_job_cancel_request(&state, request)
        .await
        .map(Json)
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
    if !actor.authority.uses_local_os_policy() {
        return Err(route_error(
            StatusCode::FORBIDDEN,
            "local_os_policy_identity_required",
            "standalone storage policy requires an appliance-local or Monas-authenticated OS identity",
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
        | LocalAuthStoreError::ProsopikonStore(_)
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

pub(super) fn route_error(
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
