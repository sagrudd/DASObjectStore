use super::*;

pub(crate) async fn pistis_easyconnect_approve_pairing(
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<crate::VerifiedHostAuthenticatedContext>,
    Extension(resolver): Extension<crate::SharedPistisEasyconnectApprovalResolver>,
    Extension(s3_endpoint): Extension<Option<EasyconnectS3EndpointConfig>>,
    Json(intent): Json<EasyconnectBrowserApprovalIntent>,
) -> Result<Json<RemoteEasyconnectApprovePairingResponse>, (StatusCode, Json<AuthRouteError>)> {
    let approval_context = resolver
        .resolve(&actor, &verified, &intent.object_store)
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
        Extension(s3_endpoint),
        Json(intent),
    )
    .await
}
