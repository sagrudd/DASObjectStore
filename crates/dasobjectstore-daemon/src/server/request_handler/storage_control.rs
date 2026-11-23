use crate::api::{
    self, DaemonApiErrorResponse, DaemonApiResponse, IngestControlRequest, IngestControlResponse,
};
use crate::auth::DaemonLocalActor;
use crate::runtime::DEFAULT_DAEMON_SERVICE_USER;

fn require_control_authority(
    request: &IngestControlRequest,
    actor: Option<&DaemonLocalActor>,
) -> Result<(), DaemonApiErrorResponse> {
    if request.dry_run {
        return Ok(());
    }
    if request.action == api::DaemonIngestControlAction::HoldBackground {
        return crate::server::request_handler::require_verified_pistis_host_authority(
            actor.filter(|actor| actor.uid != 0),
            request.verified_hold_subject.as_ref(),
            "background hold",
        );
    }
    let Some(actor) = actor else {
        return Err(DaemonApiErrorResponse::new(
            "administrator_authentication_required",
            "ingest control requires a preverified host service peer",
        ));
    };
    if actor.username.as_deref() != Some(DEFAULT_DAEMON_SERVICE_USER) {
        return Err(DaemonApiErrorResponse::new(
            "preverified_host_authority_required",
            "ingest control rejects direct root, sudo, and dasobjectstore-admin socket peers; submit through the preverified host service",
        ));
    }
    if request
        .verified_subject
        .as_deref()
        .is_none_or(|subject| subject.trim().is_empty())
    {
        return Err(DaemonApiErrorResponse::new(
            "preverified_host_subject_required",
            "ingest control requires a verified host subject",
        ));
    }
    Ok(())
}

pub(super) fn ingest_control_for_actor(
    request: IngestControlRequest,
    actor: Option<&DaemonLocalActor>,
) -> Result<IngestControlResponse, DaemonApiErrorResponse> {
    require_control_authority(&request, actor)?;
    if !request.dry_run && request.action == api::DaemonIngestControlAction::HoldBackground {
        crate::background_maintenance::hold()
            .map_err(|error| DaemonApiErrorResponse::new("background_hold_unavailable", error))?;
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
        Err(error) => DaemonApiResponse::Error(error),
    }
}

#[cfg(test)]
mod tests {
    use super::{ingest_control_for_actor, require_control_authority};
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
            verified_hold_subject: None,
        }
    }

    #[test]
    fn hold_binds_the_typed_pistis_subject_to_the_packaged_monas_peer() {
        use crate::api::{
            PreverifiedHostSubject, PREVERIFIED_HOST_GUI_API_PEER_IDENTITY,
            PREVERIFIED_HOST_MONAS_PEER_IDENTITY, PREVERIFIED_HOST_SUBJECT_SCHEMA_VERSION,
        };

        let mut request = request();
        request.action = DaemonIngestControlAction::HoldBackground;
        let monas = DaemonLocalActor::new(993).with_username("mnemosyne-monas");
        assert!(matches!(
            require_control_authority(&request, Some(&monas)),
            Err(error) if error.code == "preverified_host_subject_required"
        ));

        request.verified_hold_subject = Some(PreverifiedHostSubject {
            schema_version: PREVERIFIED_HOST_SUBJECT_SCHEMA_VERSION.into(),
            peer_identity: PREVERIFIED_HOST_MONAS_PEER_IDENTITY.into(),
            subject_id: "pistis:administrator".into(),
            session_id: "fixture-session".into(),
            correlation_id: "fixture-request".into(),
        });
        assert!(require_control_authority(&request, Some(&monas)).is_ok());
        for peer in [
            DaemonLocalActor::new(0).with_username("mnemosyne-monas"),
            DaemonLocalActor::new(994).with_username(DEFAULT_DAEMON_SERVICE_USER),
            DaemonLocalActor::new(995).with_groups(["mnemosyne-pistis-das"]),
        ] {
            assert!(require_control_authority(&request, Some(&peer)).is_err());
        }

        request
            .verified_hold_subject
            .as_mut()
            .unwrap()
            .peer_identity = PREVERIFIED_HOST_GUI_API_PEER_IDENTITY.into();
        assert!(require_control_authority(&request, Some(&monas)).is_err());
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
                Err(error) if error.code == "preverified_host_authority_required"
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
            Err(error) if error.code == "preverified_host_subject_required"
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
