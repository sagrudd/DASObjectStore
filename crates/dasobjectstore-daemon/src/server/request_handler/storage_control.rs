use crate::api::{
    self, DaemonApiErrorResponse, DaemonApiResponse, IngestControlRequest, IngestControlResponse,
};
use crate::auth::DaemonLocalActor;

pub(super) fn ingest_control_for_actor(
    request: IngestControlRequest,
    actor: Option<&DaemonLocalActor>,
) -> Result<IngestControlResponse, (&'static str, String)> {
    if !request.dry_run {
        let Some(actor) = actor else {
            return Err((
                "administrator_authentication_required",
                "ingest control requires an authenticated local administrator".to_string(),
            ));
        };
        if !actor.is_administrator() {
            return Err((
                "administrator_authorization_required",
                "ingest control requires root, sudo, or dasobjectstore-admin membership"
                    .to_string(),
            ));
        }
    }
    Ok(api::ingest_control::apply(
        request.action,
        request.reason,
        request.dry_run,
    ))
}

pub(super) fn response(
    request: IngestControlRequest,
    actor: Option<&DaemonLocalActor>,
) -> DaemonApiResponse {
    match ingest_control_for_actor(request, actor) {
        Ok(response) => DaemonApiResponse::IngestControl(response),
        Err((code, message)) => {
            DaemonApiResponse::Error(DaemonApiErrorResponse::new(code, message))
        }
    }
}
