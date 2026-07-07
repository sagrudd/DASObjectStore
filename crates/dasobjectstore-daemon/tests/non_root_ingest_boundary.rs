use dasobjectstore_core::ids::{IngestJobId, StoreId};
use dasobjectstore_daemon::{
    authorize_store_write, DaemonApiRequest, DaemonApiResponse, DaemonClient, DaemonClientError,
    DaemonLocalActor, DaemonStoreAccessPolicy, InProcessDaemonTransport, SubmitIngestFilesRequest,
    SubmitIngestFilesResponse, DEFAULT_DAEMON_GROUP, DEFAULT_DAEMON_SERVICE_USER,
};
use std::path::PathBuf;

#[test]
fn non_root_writer_submits_ingest_without_managed_root_write_access() {
    let actor = DaemonLocalActor::new(1000)
        .with_username("stephen")
        .with_groups(["mnemosyne"]);
    let store_policy = zymo_policy();
    let managed_root = ManagedRootOwnership::packaged_service_owned();

    assert!(
        !managed_root.direct_write_granted_to(&actor),
        "writer-group membership must not grant direct managed-root write access"
    );

    let transport = InProcessDaemonTransport::new(move |request| {
        handle_submit_ingest_for_actor(request, &actor, &store_policy, &managed_root)
    });
    let client = DaemonClient::new(transport);

    let response = client
        .submit_ingest_files(SubmitIngestFilesRequest {
            endpoint: StoreId::new("zymo_fecal_2025.05").expect("store id"),
            source_path: PathBuf::from("/mnt/external/zymo"),
            copies: Some(1),
            dry_run: false,
            client_request_id: Some("request-1".to_string()),
        })
        .expect("authorized non-root daemon ingest is accepted");

    assert_eq!(response.job_id.as_str(), "job-zymo");
}

#[test]
fn non_writer_is_rejected_even_when_daemon_owns_managed_root() {
    let actor = DaemonLocalActor::new(1001)
        .with_username("guest")
        .with_groups(["users"]);
    let store_policy = zymo_policy();
    let managed_root = ManagedRootOwnership::packaged_service_owned();
    let transport = InProcessDaemonTransport::new(move |request| {
        handle_submit_ingest_for_actor(request, &actor, &store_policy, &managed_root)
    });
    let client = DaemonClient::new(transport);

    let err = client
        .submit_ingest_files(SubmitIngestFilesRequest {
            endpoint: StoreId::new("zymo_fecal_2025.05").expect("store id"),
            source_path: PathBuf::from("/mnt/external/zymo"),
            copies: Some(1),
            dry_run: false,
            client_request_id: Some("request-2".to_string()),
        })
        .expect_err("non-writer daemon ingest is rejected");

    assert!(
        matches!(err, DaemonClientError::Transport(message) if message.contains("membership in group mnemosyne is required"))
    );
}

fn handle_submit_ingest_for_actor(
    request: DaemonApiRequest,
    actor: &DaemonLocalActor,
    store_policy: &DaemonStoreAccessPolicy,
    managed_root: &ManagedRootOwnership,
) -> Result<DaemonApiResponse, DaemonClientError> {
    let DaemonApiRequest::SubmitIngestFiles(request) = request else {
        return Err(DaemonClientError::Transport(
            "expected submit ingest request".to_string(),
        ));
    };

    assert_eq!(request.endpoint.as_str(), "zymo_fecal_2025.05");
    assert_eq!(managed_root.owner_user, DEFAULT_DAEMON_SERVICE_USER);
    assert_eq!(managed_root.owner_group, DEFAULT_DAEMON_GROUP);
    assert!(
        !managed_root.direct_write_granted_to(actor),
        "daemon request acceptance must not depend on direct managed-root write access"
    );

    authorize_store_write(actor, store_policy)
        .map_err(|err| DaemonClientError::Transport(err.to_string()))?;

    Ok(DaemonApiResponse::SubmitIngestFiles(
        SubmitIngestFilesResponse {
            job_id: IngestJobId::new("job-zymo").expect("job id"),
            accepted_at_utc: "2026-07-07T10:45:12Z".to_string(),
            dry_run: request.dry_run,
        },
    ))
}

fn zymo_policy() -> DaemonStoreAccessPolicy {
    DaemonStoreAccessPolicy::new(StoreId::new("zymo_fecal_2025.05").expect("store id"))
        .with_writer_group("mnemosyne")
        .with_admin_group("dasobjectstore-admin")
}

struct ManagedRootOwnership {
    owner_user: &'static str,
    owner_group: &'static str,
    mode: u32,
}

impl ManagedRootOwnership {
    fn packaged_service_owned() -> Self {
        Self {
            owner_user: DEFAULT_DAEMON_SERVICE_USER,
            owner_group: DEFAULT_DAEMON_GROUP,
            mode: 0o750,
        }
    }

    fn direct_write_granted_to(&self, actor: &DaemonLocalActor) -> bool {
        actor.has_group(self.owner_group) && self.mode & 0o020 != 0
    }
}
