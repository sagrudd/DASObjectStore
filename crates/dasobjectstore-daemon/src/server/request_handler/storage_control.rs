use crate::api::{
    self, DaemonApiErrorResponse, DaemonApiResponse, IngestControlRequest, IngestControlResponse,
};
use crate::auth::DaemonLocalActor;
use crate::runtime::DEFAULT_DAEMON_SERVICE_USER;

pub(super) fn ingest_control_for_actor(
    request: IngestControlRequest,
    actor: Option<&DaemonLocalActor>,
) -> Result<IngestControlResponse, (&'static str, String)> {
    if !request.dry_run {
        let Some(actor) = actor else {
            return Err((
                "administrator_authentication_required",
                "ingest control requires a preverified host service peer".to_string(),
            ));
        };
        if actor.username.as_deref() != Some(DEFAULT_DAEMON_SERVICE_USER) {
            return Err((
                "preverified_host_authority_required",
                "ingest control rejects direct root, sudo, and dasobjectstore-admin socket peers; submit through the preverified host service".to_string(),
            ));
        }
        if !request
            .verified_subject
            .as_deref()
            .is_some_and(|subject| !subject.trim().is_empty())
        {
            return Err((
                "preverified_host_subject_required",
                "ingest control requires a verified host subject".to_string(),
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

#[cfg(test)]
mod tests {
    use super::ingest_control_for_actor;
    use crate::api::{
        DaemonIngestControlAction, IngestControlRequest, INGEST_CONTROL_CONFIRMATION,
    };
    use crate::auth::DaemonLocalActor;
    use crate::runtime::DEFAULT_DAEMON_SERVICE_USER;

    fn request() -> IngestControlRequest {
        IngestControlRequest {
            action: DaemonIngestControlAction::Resume,
            reason: "authority-cutover regression".to_string(),
            dry_run: false,
            confirmation_marker: INGEST_CONTROL_CONFIRMATION.to_string(),
            verified_subject: Some("pistis:administrator".to_string()),
        }
    }

    #[test]
    fn rejects_direct_root_sudo_and_store_admin_peers_before_control_mutation() {
        let direct_peers = [
            DaemonLocalActor::new(0).with_username("root"),
            DaemonLocalActor::new(1000)
                .with_username("sudo-user")
                .with_groups(["sudo"]),
            DaemonLocalActor::new(1001)
                .with_username("store-admin")
                .with_groups(["dasobjectstore-admin"]),
        ];

        for actor in direct_peers {
            assert!(matches!(
                ingest_control_for_actor(request(), Some(&actor)),
                Err(("preverified_host_authority_required", _))
            ));
        }
    }

    #[test]
    fn rejects_service_peer_without_verified_subject() {
        let service_peer = DaemonLocalActor::new(997)
            .with_username(DEFAULT_DAEMON_SERVICE_USER)
            .with_groups(["dasobjectstore"]);
        let mut missing_subject = request();
        missing_subject.verified_subject = None;

        assert!(matches!(
            ingest_control_for_actor(missing_subject, Some(&service_peer)),
            Err(("preverified_host_subject_required", _))
        ));
    }

    #[test]
    fn accepts_preverified_host_service_peer() {
        let service_peer = DaemonLocalActor::new(997)
            .with_username(DEFAULT_DAEMON_SERVICE_USER)
            .with_groups(["dasobjectstore"]);

        assert!(ingest_control_for_actor(request(), Some(&service_peer)).is_ok());
    }
}
