use super::*;

pub(crate) async fn pistis_easyconnect_approve_pairing(
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<crate::VerifiedHostAuthenticatedContext>,
    Extension(resolver): Extension<crate::SharedPistisEasyconnectApprovalResolver>,
    Extension(daemon_endpoint): Extension<EasyconnectDaemonEndpoint>,
    Extension(s3_endpoint): Extension<Option<EasyconnectS3EndpointConfig>>,
    Json(intent): Json<EasyconnectBrowserApprovalIntent>,
) -> Result<Json<RemoteEasyconnectApprovePairingResponse>, (StatusCode, Json<AuthRouteError>)> {
    let handoff = intent.handoff.clone();
    let resolution_endpoint = daemon_endpoint.clone();
    let status = crate::daemon_bridge::DaemonBridge::shared_packaged()
        .call_message(move || {
            DaemonClient::new(UnixSocketDaemonTransport::for_bounded_bridge(
                resolution_endpoint.socket_path(),
            ))
            .remote_easyconnect_pairing_status(RemoteEasyconnectPairingStatusRequest {
                pairing_id: None,
                browser_handoff_reference: Some(handoff),
            })
            .map_err(|_| "browser handoff unavailable".to_string())
        })
        .await
        .map_err(|_| {
            route_error(
                StatusCode::GONE,
                "easyconnect_browser_handoff_unavailable",
                "this remote approval handoff is unavailable; return to the remote terminal and start a new pairing",
            )
        })?;
    let object_store = status.requested_object_store.as_deref().ok_or_else(|| {
        route_error(
            StatusCode::GONE,
            "easyconnect_browser_handoff_unavailable",
            "this remote approval handoff is unavailable; return to the remote terminal and start a new pairing",
        )
    })?;
    let approval_context = resolver
        .resolve(&actor, &verified, object_store)
        .map_err(|error| {
            route_error(
                StatusCode::FORBIDDEN,
                "object_store_not_authorized",
                error.to_string(),
            )
        })?;
    easyconnect_approve_pairing(
        actor,
        Extension(verified),
        Extension(approval_context),
        Extension(daemon_endpoint),
        Extension(s3_endpoint),
        Json(intent),
    )
    .await
}
