//! Authenticated HTTP adapter for daemon-owned multipart completion.
//!
//! Multipart parts are staged through the daemon's provider stream boundary.
//! This route only submits the path-free completion manifest; the daemon
//! reopens its durable journal, verifies the staged parts, and commits the
//! catalogue record.

use super::{
    admin_daemon_bridge_error_with_code, route_error, AuthRouteError, AuthenticatedGuiActor,
};
use axum::{
    extract::{Json, Path},
    http::StatusCode,
};
use dasobjectstore_core::{backend::BackendObjectKey, ids::StoreId};
use dasobjectstore_daemon::api::{
    ProfileS3MultipartCompletionRequest, ProfileS3MultipartCompletionResponse,
    ProfileS3MultipartPartRequest,
};
use dasobjectstore_daemon::{DaemonClient, DaemonRuntimeConfig, UnixSocketDaemonTransport};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub(super) struct ProfileS3MultipartCompleteBody {
    pub key: BackendObjectKey,
    pub expected_size_bytes: u64,
    pub parts: Vec<ProfileS3MultipartPartRequest>,
}

pub(super) async fn standalone_profile_s3_multipart_complete(
    Path((store_id, reservation_id)): Path<(String, String)>,
    _actor: AuthenticatedGuiActor,
    Json(body): Json<ProfileS3MultipartCompleteBody>,
) -> Result<
    axum::Json<ProfileS3MultipartCompletionResponse>,
    (StatusCode, axum::Json<AuthRouteError>),
> {
    let store_id = store_id.parse::<StoreId>().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "profile_s3_invalid_store_id",
            error.to_string(),
        )
    })?;
    if reservation_id.trim().is_empty() {
        return Err(route_error(
            StatusCode::BAD_REQUEST,
            "profile_s3_invalid_reservation",
            "multipart completion requires a reservation id",
        ));
    }

    let request = ProfileS3MultipartCompletionRequest {
        store_id,
        reservation_id,
        key: body.key,
        expected_size_bytes: body.expected_size_bytes,
        parts: body.parts,
    };
    request.validate().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "profile_s3_invalid_multipart_completion",
            error.to_string(),
        )
    })?;

    crate::daemon_bridge::DaemonBridge::shared_packaged()
        .call_message(move || {
            let client = DaemonClient::new(UnixSocketDaemonTransport::for_bounded_bridge(
                DaemonRuntimeConfig::default_packaged().socket_path,
            ));
            client
                .profile_s3_multipart_complete(request)
                .map_err(|error| error.to_string())
        })
        .await
        .map(axum::Json)
        .map_err(|error| {
            admin_daemon_bridge_error_with_code(error, "profile_s3_multipart_complete_failed")
        })
}
