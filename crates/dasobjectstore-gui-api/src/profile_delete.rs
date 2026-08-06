//! Authenticated HTTP adapter for daemon-owned profile-object deletion.
//!
//! DELETE remains catalogue-authoritative and idempotent. The Web process only
//! translates logical identity; the daemon owns authorization, backend removal,
//! and logical-capacity reconciliation.

use super::{
    admin_daemon_bridge_error_with_code, require_preverified_host_operator, route_error,
    AuthRouteError, AuthenticatedGuiActor, VerifiedHostAuthenticatedContext,
};
use axum::{
    extract::{Extension, Path, Query},
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
pub(crate) struct ProfileDeleteQuery {
    pub version: Option<u64>,
}

pub(super) async fn standalone_profile_s3_delete(
    Path((store_id, object_id)): Path<(String, String)>,
    Query(query): Query<ProfileDeleteQuery>,
    _actor: AuthenticatedGuiActor,
) -> Result<Json<ProfileS3DeleteResponse>, (StatusCode, Json<AuthRouteError>)> {
    delete_profile_s3_object(store_id, object_id, query).await
}

/// Delete a profile object for an actor that Monas or Synoptikon has already
/// verified with Pistis.
///
/// The host-composed route deliberately derives its authority from the
/// matching verified subject and closed DAS `storage_operator` role only. It
/// has no local session, password, PAM, POSIX user/group, or sudo dependency;
/// deletion remains daemon-owned and uses the bounded daemon bridge.
pub(crate) async fn preverified_host_profile_s3_delete(
    Path((store_id, object_id)): Path<(String, String)>,
    Query(query): Query<ProfileDeleteQuery>,
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<VerifiedHostAuthenticatedContext>,
) -> Result<Json<ProfileS3DeleteResponse>, (StatusCode, Json<AuthRouteError>)> {
    require_preverified_host_operator(&actor, &verified)?;
    delete_profile_s3_object(store_id, object_id, query).await
}

async fn delete_profile_s3_object(
    store_id: String,
    object_id: String,
    query: ProfileDeleteQuery,
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
