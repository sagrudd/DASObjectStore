//! Narrow daemon-client adapters used by standalone auth/admin routes.

use super::*;

fn daemon_unavailable(
    code: &'static str,
    message: &'static str,
) -> (StatusCode, Json<AuthRouteError>) {
    route_error(StatusCode::NOT_IMPLEMENTED, code, message)
}

pub(super) fn submit_local_group_admin_request(
    state: &StandaloneUsersGroupsRouteState,
    request: StandaloneLocalGroupAdminDaemonRequest,
) -> Result<StandaloneLocalGroupAdminResponse, (StatusCode, Json<AuthRouteError>)> {
    let client = state.local_group_admin_client.as_ref().ok_or_else(|| {
        daemon_unavailable(
            "daemon_local_group_admin_unavailable",
            "daemon local group administration contract is not available",
        )
    })?;
    let response = client
        .submit_local_group_operation(request)
        .map_err(|err| route_error(StatusCode::BAD_GATEWAY, "daemon_client_error", err.message))?;
    if !response.accepted.dry_run {
        upsert_storage_group(&state.groups_registry_path, &response.group_name).map_err(|err| {
            route_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "groups_registry_update_failed",
                format!(
                    "daemon accepted local group operation, but {} could not be updated: {err}",
                    state.groups_registry_path.display()
                ),
            )
        })?;
    }
    Ok(response)
}

pub(super) fn submit_prepare_enclosure_request(
    state: &StandaloneEnclosureAdminRouteState,
    request: StandaloneEnclosurePrepareDaemonRequest,
) -> Result<StandaloneEnclosurePrepareResponse, (StatusCode, Json<AuthRouteError>)> {
    let client = state.enclosure_admin_client.as_ref().ok_or_else(|| {
        daemon_unavailable(
            "daemon_enclosure_admin_unavailable",
            "daemon enclosure preparation contract is not available",
        )
    })?;
    client.submit_prepare_enclosure(request).map_err(|err| {
        route_error(
            StatusCode::BAD_GATEWAY,
            "daemon_enclosure_prepare_failed",
            err.message,
        )
    })
}

pub(super) fn submit_create_object_store_request(
    state: &StandaloneEnclosureAdminRouteState,
    request: DaemonCreateObjectStoreRequest,
) -> Result<StandaloneCreateObjectStoreResponse, (StatusCode, Json<AuthRouteError>)> {
    let client = state.enclosure_admin_client.as_ref().ok_or_else(|| {
        daemon_unavailable(
            "daemon_objectstore_admin_unavailable",
            "daemon ObjectStore administration contract is not available",
        )
    })?;
    client.submit_create_object_store(request).map_err(|err| {
        route_error(
            StatusCode::BAD_GATEWAY,
            "daemon_objectstore_create_failed",
            err.message,
        )
    })
}

pub(super) fn submit_update_object_store_ingest_policy_request(
    state: &StandaloneEnclosureAdminRouteState,
    request: DaemonUpdateObjectStoreIngestPolicyRequest,
) -> Result<StandaloneObjectStoreIngestPolicyResponse, (StatusCode, Json<AuthRouteError>)> {
    let client = state.enclosure_admin_client.as_ref().ok_or_else(|| {
        daemon_unavailable(
            "daemon_objectstore_admin_unavailable",
            "daemon ObjectStore administration contract is not available",
        )
    })?;
    client
        .submit_update_object_store_ingest_policy(request)
        .map_err(|err| {
            route_error(
                StatusCode::BAD_GATEWAY,
                "daemon_objectstore_ingest_policy_failed",
                err.message,
            )
        })
}

pub(super) fn submit_endpoint_inventory_upsert_request(
    state: &StandaloneEnclosureAdminRouteState,
    request: DaemonUpsertEndpointInventoryRequest,
) -> Result<StandaloneEndpointInventoryUpsertResponse, (StatusCode, Json<AuthRouteError>)> {
    let client = state.enclosure_admin_client.as_ref().ok_or_else(|| {
        daemon_unavailable(
            "daemon_endpoint_admin_unavailable",
            "daemon endpoint inventory administration contract is not available",
        )
    })?;
    client
        .submit_endpoint_inventory_upsert(request)
        .map_err(|err| {
            route_error(
                StatusCode::BAD_GATEWAY,
                "daemon_endpoint_inventory_upsert_failed",
                err.message,
            )
        })
}

pub(super) async fn submit_admin_job_status_request(
    state: &StandaloneEnclosureAdminRouteState,
    request: StandaloneAdminJobStatusDaemonRequest,
) -> Result<StandaloneAdminJobStatusResponse, (StatusCode, Json<AuthRouteError>)> {
    let client = state.enclosure_admin_client.as_ref().ok_or_else(|| {
        daemon_unavailable(
            "daemon_admin_jobs_unavailable",
            "daemon administrator job status contract is not available",
        )
    })?;
    let client = Arc::clone(client);
    let bridge = state.daemon_bridge.clone();
    bridge
        .call_message(move || client.job_status(request).map_err(|err| err.message))
        .await
        .map_err(admin_daemon_bridge_error)
}

pub(super) async fn submit_admin_job_cancel_request(
    state: &StandaloneEnclosureAdminRouteState,
    request: StandaloneAdminJobCancelDaemonRequest,
) -> Result<StandaloneAdminJobCancelResponse, (StatusCode, Json<AuthRouteError>)> {
    let client = state.enclosure_admin_client.as_ref().ok_or_else(|| {
        daemon_unavailable(
            "daemon_admin_jobs_unavailable",
            "daemon administrator job cancellation contract is not available",
        )
    })?;
    let client = Arc::clone(client);
    let bridge = state.daemon_bridge.clone();
    bridge
        .call_message(move || client.cancel_job(request).map_err(|err| err.message))
        .await
        .map_err(admin_daemon_bridge_error)
}

fn admin_daemon_bridge_error(
    error: crate::daemon_bridge::DaemonBridgeError,
) -> (StatusCode, Json<AuthRouteError>) {
    match error {
        crate::daemon_bridge::DaemonBridgeError::Client(error) => route_error(
            StatusCode::BAD_GATEWAY,
            "daemon_admin_job_failed",
            error.message,
        ),
        crate::daemon_bridge::DaemonBridgeError::Busy => route_error(
            StatusCode::TOO_MANY_REQUESTS,
            "daemon_admin_job_busy",
            "daemon control capacity is saturated; retry shortly",
        ),
        crate::daemon_bridge::DaemonBridgeError::Deadline => route_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "daemon_admin_job_timeout",
            "daemon administrator job request exceeded its deadline; retry shortly",
        ),
        crate::daemon_bridge::DaemonBridgeError::Join(message) => route_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "daemon_admin_job_unavailable",
            message,
        ),
    }
}
