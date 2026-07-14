//! Authenticated HTTP adapters for daemon-owned portable profile catalogues.
//!
//! The Web process only translates the logical store identity and catalogue
//! document. The daemon remains authoritative for profile paths, payload
//! verification, catalogue commit, and source-retention semantics.

use super::{
    admin_daemon_bridge_error_with_code, route_error, AuthRouteError, AuthenticatedGuiActor,
};
use axum::{extract::Path, http::StatusCode, Json};
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_daemon::{
    DaemonClient, DaemonRuntimeConfig, ProfileCatalogueExportRequest,
    ProfileCatalogueExportResponse, ProfileCatalogueImportRequest, ProfileCatalogueImportResponse,
    UnixSocketDaemonTransport,
};

pub(super) async fn standalone_profile_catalogue_export(
    Path(store_id): Path<String>,
    _actor: AuthenticatedGuiActor,
) -> Result<Json<ProfileCatalogueExportResponse>, (StatusCode, Json<AuthRouteError>)> {
    let store_id = store_id.parse::<StoreId>().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "profile_catalogue_invalid_store_id",
            error.to_string(),
        )
    })?;
    let request = ProfileCatalogueExportRequest { store_id };
    request.validate().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "profile_catalogue_invalid_request",
            error.to_string(),
        )
    })?;
    crate::daemon_bridge::DaemonBridge::shared_packaged()
        .call_message(move || {
            let client = DaemonClient::new(UnixSocketDaemonTransport::for_bounded_bridge(
                DaemonRuntimeConfig::default_packaged().socket_path,
            ));
            client
                .profile_catalogue_export(request)
                .map_err(|error| error.to_string())
        })
        .await
        .map(Json)
        .map_err(|error| {
            admin_daemon_bridge_error_with_code(error, "profile_catalogue_export_failed")
        })
}

pub(super) async fn standalone_profile_catalogue_import(
    Path(store_id): Path<String>,
    _actor: AuthenticatedGuiActor,
    Json(mut request): Json<ProfileCatalogueImportRequest>,
) -> Result<Json<ProfileCatalogueImportResponse>, (StatusCode, Json<AuthRouteError>)> {
    let store_id = store_id.parse::<StoreId>().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "profile_catalogue_invalid_store_id",
            error.to_string(),
        )
    })?;
    request.store_id = store_id;
    request.validate().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "profile_catalogue_invalid_request",
            error.to_string(),
        )
    })?;
    crate::daemon_bridge::DaemonBridge::shared_packaged()
        .call_message(move || {
            let client = DaemonClient::new(UnixSocketDaemonTransport::for_bounded_bridge(
                DaemonRuntimeConfig::default_packaged().socket_path,
            ));
            client
                .profile_catalogue_import(request)
                .map_err(|error| error.to_string())
        })
        .await
        .map(Json)
        .map_err(|error| {
            admin_daemon_bridge_error_with_code(error, "profile_catalogue_import_failed")
        })
}
