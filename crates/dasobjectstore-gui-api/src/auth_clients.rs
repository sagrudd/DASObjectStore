//! Shared daemon-bridge error mapping for Pistis-hosted GUI routes.

use super::*;

pub(super) fn admin_daemon_bridge_error_with_code(
    error: crate::daemon_bridge::DaemonBridgeError,
    client_error_code: &'static str,
) -> (StatusCode, Json<AuthRouteError>) {
    match error {
        crate::daemon_bridge::DaemonBridgeError::Client(error) => {
            route_error(StatusCode::BAD_GATEWAY, client_error_code, error.message)
        }
        crate::daemon_bridge::DaemonBridgeError::Busy => route_error(
            StatusCode::TOO_MANY_REQUESTS,
            "daemon_admin_job_busy",
            "daemon control capacity is saturated; retry shortly",
        ),
        crate::daemon_bridge::DaemonBridgeError::CircuitOpen => route_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "daemon_admin_job_circuit_open",
            "daemon control is temporarily degraded; retry shortly",
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
