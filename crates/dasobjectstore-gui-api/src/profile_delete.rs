//! Authenticated HTTP adapter for daemon-owned profile-object deletion.
//!
//! DELETE remains catalogue-authoritative and idempotent. The Web process only
//! translates logical identity; the daemon owns authorization, backend removal,
//! and logical-capacity reconciliation.

use super::{
    admin_daemon_bridge_error_with_code, route_error, AuthRouteError, AuthenticatedGuiActor,
};
use axum::{
    extract::{Path, Query},
    http::StatusCode,
    Json,
};
use dasobjectstore_core::{backend::BackendObjectKey, ids::StoreId};
use dasobjectstore_daemon::{
    DaemonClient, DaemonRuntimeConfig, ProfileS3DeleteRequest, ProfileS3DeleteResponse,
    UnixSocketDaemonTransport,
};
use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize)]
pub(super) struct ProfileDeleteQuery {
    pub version: Option<u64>,
}

pub(super) async fn standalone_profile_s3_delete(
    Path((store_id, object_id)): Path<(String, String)>,
    Query(query): Query<ProfileDeleteQuery>,
    _actor: AuthenticatedGuiActor,
) -> Result<Json<ProfileS3DeleteResponse>, (StatusCode, Json<AuthRouteError>)> {
    let store_id = store_id.parse::<StoreId>().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "profile_s3_invalid_store_id",
            error.to_string(),
        )
    })?;
    let request = ProfileS3DeleteRequest {
        store_id,
        key: BackendObjectKey {
            object_id,
            version: query.version.unwrap_or(1),
        },
    };
    request.validate().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "profile_s3_invalid_delete",
            error.to_string(),
        )
    })?;
    crate::daemon_bridge::DaemonBridge::shared_packaged()
        .call_message(move || {
            let client = DaemonClient::new(UnixSocketDaemonTransport::for_bounded_bridge(
                DaemonRuntimeConfig::default_packaged().socket_path,
            ));
            client
                .profile_s3_delete(request)
                .map_err(|error| error.to_string())
        })
        .await
        .map(Json)
        .map_err(|error| admin_daemon_bridge_error_with_code(error, "profile_s3_delete_failed"))
}
