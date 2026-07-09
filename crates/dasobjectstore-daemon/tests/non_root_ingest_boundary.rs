use dasobjectstore_core::ids::{IngestJobId, StoreId};
use dasobjectstore_core::object_type::ObjectType;
use dasobjectstore_core::store::{StoreClass, StorePolicy};
use dasobjectstore_daemon::api::{DaemonIngestProgressEvent, DaemonServiceStatusRequest};
use dasobjectstore_daemon::runtime::{DaemonIngestFilesRuntimeError, DaemonServiceRuntimeError};
use dasobjectstore_daemon::{
    DaemonClient, DaemonClientError, DaemonIngestConflictPolicy, DaemonIngressOrigin,
    DaemonLocalActor, DaemonRequestHandler, DaemonServiceOrchestrator, FixedDaemonClock,
    InProcessDaemonTransport, SubmitIngestFilesRequest, SubmitIngestFilesResponse,
    DEFAULT_DAEMON_GROUP, DEFAULT_DAEMON_SERVICE_USER,
};
use dasobjectstore_object_service::StoreServiceDefinition;
use std::cell::Cell;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

#[test]
fn non_root_writer_submits_ingest_through_daemon_without_managed_root_write_access() {
    let root = temp_root("writer-through-daemon");
    let (store_registry, subobject_registry) =
        write_test_store_registry(&root, "zymo_fecal_2025.05", Some("mnemosyne"));
    let actor = DaemonLocalActor::new(1000)
        .with_username("stephen")
        .with_groups(["mnemosyne"]);
    let managed_root = ManagedRootOwnership::packaged_service_owned();
    let calls = Rc::new(Cell::new(0));

    assert!(
        !managed_root.direct_write_granted_to(&actor),
        "writer-group membership must not grant direct managed-root write access"
    );

    let handler = DaemonRequestHandler::new(
        FakeIngestService {
            managed_root,
            calls: Rc::clone(&calls),
        },
        FixedDaemonClock::new("2026-07-09T09:34:00Z"),
    )
    .with_registry_paths(store_registry, subobject_registry);
    let transport = InProcessDaemonTransport::new(move |request| {
        handler
            .handle_with_progress_for_actor(request, Some(&actor), |_| Ok(()))
            .map_err(|err| DaemonClientError::Transport(err.to_string()))
    });
    let client = DaemonClient::new(transport);

    let response = client
        .submit_ingest_files(ingest_request("request-1"))
        .expect("authorized non-root daemon ingest is accepted");

    assert_eq!(response.job_id.as_str(), "job-zymo");
    assert_eq!(calls.get(), 1);

    cleanup(&root);
}

#[test]
fn non_writer_is_rejected_by_daemon_before_ingest_service_runs() {
    let root = temp_root("non-writer-rejected");
    let (store_registry, subobject_registry) =
        write_test_store_registry(&root, "zymo_fecal_2025.05", Some("mnemosyne"));
    let actor = DaemonLocalActor::new(1001)
        .with_username("guest")
        .with_groups(["users"]);
    let calls = Rc::new(Cell::new(0));
    let handler = DaemonRequestHandler::new(
        FakeIngestService {
            managed_root: ManagedRootOwnership::packaged_service_owned(),
            calls: Rc::clone(&calls),
        },
        FixedDaemonClock::new("2026-07-09T09:34:00Z"),
    )
    .with_registry_paths(store_registry, subobject_registry);
    let transport = InProcessDaemonTransport::new(move |request| {
        handler
            .handle_with_progress_for_actor(request, Some(&actor), |_| Ok(()))
            .map_err(|err| DaemonClientError::Transport(err.to_string()))
    });
    let client = DaemonClient::new(transport);

    let err = client
        .submit_ingest_files(ingest_request("request-2"))
        .expect_err("non-writer daemon ingest is rejected");

    assert!(matches!(err, DaemonClientError::Api(error)
            if error.code == "permission_denied"
                && error.message.contains("membership in group mnemosyne is required")));
    assert_eq!(calls.get(), 0);

    cleanup(&root);
}

fn ingest_request(client_request_id: &str) -> SubmitIngestFilesRequest {
    SubmitIngestFilesRequest {
        endpoint: StoreId::new("zymo_fecal_2025.05").expect("store id"),
        source_path: PathBuf::from("/mnt/external/zymo"),
        object_type: ObjectType::Naive,
        copies: Some(1),
        conflict_policy: DaemonIngestConflictPolicy::Strict,
        hdd_workers: None,
        ingress_origin: DaemonIngressOrigin::LocalServer,
        dry_run: false,
        client_request_id: Some(client_request_id.to_string()),
    }
}

fn write_test_store_registry(
    root: &PathBuf,
    store_id: &str,
    writer_group: Option<&str>,
) -> (PathBuf, PathBuf) {
    fs::create_dir_all(root).expect("temp registry dir");
    let store_registry = root.join("stores.json");
    let subobject_registry = root.join("subobjects.json");
    let definitions = vec![StoreServiceDefinition {
        store_id: StoreId::new(store_id).expect("store id"),
        policy: StorePolicy::defaults_for(StoreClass::ReproducibleCache),
        bucket_name: None,
        reader_group: None,
        writer_group: writer_group.map(ToString::to_string),
        public: false,
    }];
    fs::write(
        &store_registry,
        serde_json::to_string_pretty(&definitions).expect("registry JSON"),
    )
    .expect("store registry written");
    fs::write(&subobject_registry, "[]").expect("subobject registry written");
    (store_registry, subobject_registry)
}

fn temp_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "dasobjectstore-non-root-ingest-{label}-{}",
        std::process::id()
    ))
}

fn cleanup(root: &PathBuf) {
    let _ = fs::remove_dir_all(root);
}

struct FakeIngestService {
    managed_root: ManagedRootOwnership,
    calls: Rc<Cell<usize>>,
}

impl DaemonServiceOrchestrator for FakeIngestService {
    fn status(
        &self,
        _request: DaemonServiceStatusRequest,
    ) -> Result<dasobjectstore_daemon::DaemonServiceStatusResponse, DaemonServiceRuntimeError> {
        Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: "status is not needed for this boundary test".to_string(),
        })
    }

    fn lifecycle(
        &self,
        _request: dasobjectstore_daemon::DaemonServiceLifecycleRequest,
        _accepted_at_utc: &str,
    ) -> Result<dasobjectstore_daemon::DaemonServiceLifecycleResponse, DaemonServiceRuntimeError>
    {
        Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: "lifecycle is not needed for this boundary test".to_string(),
        })
    }

    fn provision(
        &self,
        _request: dasobjectstore_daemon::DaemonServiceProvisionRequest,
        _accepted_at_utc: &str,
    ) -> Result<dasobjectstore_daemon::DaemonServiceProvisionResponse, DaemonServiceRuntimeError>
    {
        Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: "provision is not needed for this boundary test".to_string(),
        })
    }

    fn submit_ingest_files(
        &self,
        request: SubmitIngestFilesRequest,
        accepted_at_utc: &str,
        _emit_progress: &mut dyn FnMut(
            DaemonIngestProgressEvent,
        ) -> Result<(), DaemonIngestFilesRuntimeError>,
    ) -> Result<SubmitIngestFilesResponse, DaemonIngestFilesRuntimeError> {
        assert_eq!(request.endpoint.as_str(), "zymo_fecal_2025.05");
        assert_eq!(self.managed_root.owner_user, DEFAULT_DAEMON_SERVICE_USER);
        assert_eq!(self.managed_root.owner_group, DEFAULT_DAEMON_GROUP);
        self.calls.set(self.calls.get() + 1);
        Ok(SubmitIngestFilesResponse {
            job_id: IngestJobId::new("job-zymo").expect("job id"),
            accepted_at_utc: accepted_at_utc.to_string(),
            dry_run: request.dry_run,
        })
    }
}

#[derive(Clone, Copy)]
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
