use dasobjectstore_core::{
    application_auth::{
        ApplicationCredentialKind, ApplicationEnvironment, ApplicationIdentity,
        ApplicationOperation, ApplicationScope, APPLICATION_AUTH_SCHEMA_VERSION,
    },
    ids::StoreId,
    ingress::IngressOrigin,
    object_type::ObjectType,
    utc::parse_utc_timestamp_seconds,
};
use dasobjectstore_daemon::{
    api::{
        ApplicationUploadCapabilityIssueRequest, ApplicationUploadCompletionOutcome,
        ApplicationUploadCompletionRequest,
    },
    runtime::{
        upsert_application_identity, DaemonServiceRuntimeError,
        FileBackedRemoteEasyconnectPairedSessionStore, RemoteEasyconnectPairedSessionRecord,
        RemoteEasyconnectPairedSessionStore, RemoteUploadCompletionRecord,
        RemoteUploadProviderCompletion,
    },
    DaemonClient, DaemonClientError, DaemonRequestHandler, DaemonServiceLifecycleRequest,
    DaemonServiceLifecycleResponse, DaemonServiceOrchestrator, DaemonServiceProvisionRequest,
    DaemonServiceProvisionResponse, DaemonServiceStatusRequest, DaemonServiceStatusResponse,
    FixedDaemonClock, InProcessDaemonTransport, RemoteEasyconnectAuthProvider,
    RemoteEasyconnectObjectStoreGrant, RemoteEasyconnectSessionCredentials,
};
use dasobjectstore_object_service::{
    write_managed_credential_registry, ManagedCredentialRegistry, ManagedStoreCredentialRecord,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};

const NOW_UTC: &str = "2026-07-16T16:00:00Z";
const ENDPOINT: &str = "http://127.0.0.1:3900";
const STORE: &str = "codex";
const BUCKET: &str = "dos-codex";
const APPLICATION: &str = "synoptikon-uploader";
const CHECKSUM: &str = "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

#[test]
fn capability_completion_verifies_commits_replays_and_recovers_from_commit_failure() {
    let root = acceptance_root();
    fs::create_dir_all(&root).expect("acceptance root");
    let paths = AcceptancePaths::new(&root);
    seed_session(&paths.session);
    seed_identity(&paths.identity);
    seed_credentials(&paths.credentials);
    let state = Arc::new(CompletionState::default());
    let handler = configured_handler(&paths, Arc::clone(&state));
    let client = DaemonClient::new(InProcessDaemonTransport::new(move |request| {
        handler
            .handle(request)
            .map_err(|error| DaemonClientError::Transport(error.to_string()))
    }));

    let issued = client
        .issue_application_upload_capability(issue_request("upload-1", "analysis/hello.txt"))
        .expect("capability issuance succeeds");
    assert_eq!(issued.capability.application_id, APPLICATION);
    assert_eq!(issued.capability.expected_size_bytes, 5);
    assert!(
        issued.capability.expires_at_unix_seconds - issued.capability.issued_at_unix_seconds <= 900
    );
    let encoded = serde_json::to_string(&issued).expect("response serializes");
    assert!(!encoded.contains("renewal-secret"));
    assert!(!encoded.contains("managed-secret"));

    let mut forged = issued.capability.clone();
    forged.nonce.push('x');
    assert!(client
        .complete_application_upload(ApplicationUploadCompletionRequest { capability: forged })
        .is_err());
    assert_eq!(state.verifications.load(Ordering::SeqCst), 0);

    let first = client
        .complete_application_upload(ApplicationUploadCompletionRequest {
            capability: issued.capability.clone(),
        })
        .expect("first completion commits");
    assert_eq!(first.outcome, ApplicationUploadCompletionOutcome::Committed);
    assert_eq!(state.verifications.load(Ordering::SeqCst), 1);
    assert_eq!(state.commits.load(Ordering::SeqCst), 1);

    let replay = client
        .complete_application_upload(ApplicationUploadCompletionRequest {
            capability: issued.capability,
        })
        .expect("exact retry is idempotent");
    assert_eq!(
        replay.outcome,
        ApplicationUploadCompletionOutcome::AlreadyCommitted
    );
    assert_eq!(
        state.verifications.load(Ordering::SeqCst),
        1,
        "an already-committed exact retry must not repeat provider work"
    );
    assert_eq!(state.commits.load(Ordering::SeqCst), 1);

    let retryable = client
        .issue_application_upload_capability(issue_request("upload-2", "analysis/retry.txt"))
        .expect("second capability issuance succeeds");
    state.fail_next_commit.store(true, Ordering::SeqCst);
    assert!(client
        .complete_application_upload(ApplicationUploadCompletionRequest {
            capability: retryable.capability.clone(),
        })
        .is_err());
    let recovered = client
        .complete_application_upload(ApplicationUploadCompletionRequest {
            capability: retryable.capability,
        })
        .expect("catalogue failure releases replay claim for retry");
    assert_eq!(
        recovered.outcome,
        ApplicationUploadCompletionOutcome::Committed
    );
    assert_eq!(state.commits.load(Ordering::SeqCst), 3);
    assert_eq!(
        state.committed_uploads.lock().expect("uploads").as_slice(),
        &["upload-1", "upload-2"]
    );
    assert!(state.saw_managed_credentials.load(Ordering::SeqCst));

    cleanup(&root);
}

fn configured_handler(
    paths: &AcceptancePaths,
    state: Arc<CompletionState>,
) -> DaemonRequestHandler<AcceptanceOrchestrator, FixedDaemonClock> {
    DaemonRequestHandler::new(
        AcceptanceOrchestrator { state },
        FixedDaemonClock::new(NOW_UTC),
    )
    .with_remote_easyconnect_session_store_path(&paths.session)
    .with_application_identity_registry_path(&paths.identity)
    .with_application_upload_paths(&paths.capabilities, &paths.replay)
    .with_credential_registry_path(&paths.credentials)
    .with_live_sqlite_path(&paths.catalogue)
}

fn issue_request(upload_id: &str, object_key: &str) -> ApplicationUploadCapabilityIssueRequest {
    ApplicationUploadCapabilityIssueRequest {
        session_id: "session-1".to_string(),
        renewal_token: "renewal-secret".to_string(),
        application_id: APPLICATION.to_string(),
        upload_id: upload_id.to_string(),
        object_store: STORE.to_string(),
        object_id: format!("{STORE}/{object_key}"),
        object_version: 1,
        object_key: object_key.to_string(),
        expected_size_bytes: 5,
        expected_checksum: CHECKSUM.to_string(),
        audience: "dasobjectstore".to_string(),
        provider: "garage".to_string(),
        bucket: BUCKET.to_string(),
        endpoint_url: ENDPOINT.to_string(),
        requested_ttl_seconds: Some(600),
    }
}

fn seed_session(path: &Path) {
    FileBackedRemoteEasyconnectPairedSessionStore::new(path)
        .upsert(RemoteEasyconnectPairedSessionRecord {
            session_id: "session-1".to_string(),
            approved_actor: "synoptikon".to_string(),
            auth_provider: RemoteEasyconnectAuthProvider::StandaloneLocalUser,
            issued_at_utc: "2026-07-16T15:00:00Z".to_string(),
            expires_at_utc: "2026-07-16T18:00:00Z".to_string(),
            renew_after_utc: "2026-07-16T17:00:00Z".to_string(),
            renewal_token: "renewal-secret".to_string(),
            credentials: RemoteEasyconnectSessionCredentials {
                access_key_id: "session-access".to_string(),
                secret_access_key: "session-secret".to_string(),
                session_token: None,
            },
            object_stores: vec![RemoteEasyconnectObjectStoreGrant {
                object_store: STORE.to_string(),
                bucket: BUCKET.to_string(),
                can_read: true,
                can_write: true,
                writer_group: Some("synoptikon".to_string()),
                object_type: "fastq".to_string(),
            }],
            revoked_at_utc: None,
        })
        .expect("paired session");
}

fn seed_identity(path: &Path) {
    let now = parse_utc_timestamp_seconds(NOW_UTC).expect("timestamp") as u64;
    upsert_application_identity(
        path,
        ApplicationIdentity {
            schema_version: APPLICATION_AUTH_SCHEMA_VERSION.to_string(),
            application_id: APPLICATION.to_string(),
            owner: "synoptikon".to_string(),
            purpose: "remote upload completion".to_string(),
            environment: ApplicationEnvironment::Production,
            credential_kind: ApplicationCredentialKind::AsymmetricKey,
            scope: ApplicationScope {
                store_ids: vec![StoreId::new(STORE).expect("store")],
                prefixes: vec!["analysis".to_string()],
                object_types: vec![ObjectType::Fastq],
                operations: vec![ApplicationOperation::CompleteUpload],
                ingress_origin: IngressOrigin::Synoptikon,
                max_object_bytes: Some(1024),
                max_total_bytes: Some(4096),
            },
            issued_at_unix_seconds: now - 60,
            expires_at_unix_seconds: now + 3600,
            active: true,
        },
    )
    .expect("application identity");
}

fn seed_credentials(path: &Path) {
    write_managed_credential_registry(
        path,
        &ManagedCredentialRegistry {
            format_version: 1,
            updated_at_utc: NOW_UTC.to_string(),
            credentials: vec![ManagedStoreCredentialRecord {
                store_id: StoreId::new(STORE).expect("store"),
                bucket_name: BUCKET.to_string(),
                credential_reference: "secret://acceptance/codex".to_string(),
                access_key_id: "managed-access".to_string(),
                secret_access_key: "managed-secret".to_string(),
                issued_at_utc: NOW_UTC.to_string(),
                rotated_at_utc: None,
                revision: 1,
            }],
            audit: vec![],
        },
    )
    .expect("managed credentials");
}

#[derive(Default)]
struct CompletionState {
    verifications: AtomicUsize,
    commits: AtomicUsize,
    fail_next_commit: AtomicBool,
    saw_managed_credentials: AtomicBool,
    committed_uploads: Mutex<Vec<&'static str>>,
}

struct AcceptanceOrchestrator {
    state: Arc<CompletionState>,
}

impl DaemonServiceOrchestrator for AcceptanceOrchestrator {
    fn application_upload_endpoint(&self) -> Option<String> {
        Some(ENDPOINT.to_string())
    }

    fn verify_application_upload_completion(
        &self,
        record: &RemoteUploadCompletionRecord,
        completion: RemoteUploadProviderCompletion,
        environment: Vec<(String, String)>,
        _live_sqlite_path: PathBuf,
        _committed_at_utc: &str,
    ) -> Result<(), DaemonServiceRuntimeError> {
        self.state.verifications.fetch_add(1, Ordering::SeqCst);
        assert_eq!(record.source_bytes, 5);
        assert_eq!(completion.provider, "garage");
        assert_eq!(completion.expected_checksum, CHECKSUM);
        self.state.saw_managed_credentials.store(
            environment.iter().any(|entry| {
                entry
                    == &(
                        "AWS_ACCESS_KEY_ID".to_string(),
                        "managed-access".to_string(),
                    )
            }) && environment.iter().any(|entry| {
                entry
                    == &(
                        "AWS_SECRET_ACCESS_KEY".to_string(),
                        "managed-secret".to_string(),
                    )
            }),
            Ordering::SeqCst,
        );
        Ok(())
    }

    fn commit_application_upload_catalogue(
        &self,
        record: &RemoteUploadCompletionRecord,
        _completion: RemoteUploadProviderCompletion,
        _environment: Vec<(String, String)>,
        _live_sqlite_path: PathBuf,
        _committed_at_utc: &str,
    ) -> Result<(), DaemonServiceRuntimeError> {
        self.state.commits.fetch_add(1, Ordering::SeqCst);
        if self.state.fail_next_commit.swap(false, Ordering::SeqCst) {
            return Err(unsupported("injected catalogue commit failure"));
        }
        let upload = match record.job_id.as_str() {
            "upload-1" => "upload-1",
            "upload-2" => "upload-2",
            other => panic!("unexpected upload {other}"),
        };
        self.state
            .committed_uploads
            .lock()
            .expect("uploads")
            .push(upload);
        Ok(())
    }

    fn status(
        &self,
        _request: DaemonServiceStatusRequest,
    ) -> Result<DaemonServiceStatusResponse, DaemonServiceRuntimeError> {
        Err(unsupported("status"))
    }
    fn lifecycle(
        &self,
        _request: DaemonServiceLifecycleRequest,
        _accepted_at_utc: &str,
    ) -> Result<DaemonServiceLifecycleResponse, DaemonServiceRuntimeError> {
        Err(unsupported("lifecycle"))
    }
    fn provision(
        &self,
        _request: DaemonServiceProvisionRequest,
        _accepted_at_utc: &str,
    ) -> Result<DaemonServiceProvisionResponse, DaemonServiceRuntimeError> {
        Err(unsupported("provision"))
    }
}

fn unsupported(operation: &str) -> DaemonServiceRuntimeError {
    DaemonServiceRuntimeError::UnsupportedOperation {
        operation: operation.to_string(),
    }
}

struct AcceptancePaths {
    session: PathBuf,
    identity: PathBuf,
    capabilities: PathBuf,
    replay: PathBuf,
    credentials: PathBuf,
    catalogue: PathBuf,
}

impl AcceptancePaths {
    fn new(root: &Path) -> Self {
        Self {
            session: root.join("sessions.json"),
            identity: root.join("identities.json"),
            capabilities: root.join("capabilities.json"),
            replay: root.join("replay.json"),
            credentials: root.join("credentials.json"),
            catalogue: root.join("live.sqlite"),
        }
    }
}

fn acceptance_root() -> PathBuf {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let home = PathBuf::from(std::env::var_os("HOME").expect("HOME"));
    let approved = home.join(".dasobjectstore-codex-validation");
    let configured = std::env::var_os("DASOBJECTSTORE_CODEX_VALIDATION_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| approved.clone());
    assert!(configured.starts_with(&approved));
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    configured.join(format!(
        "remote-completion-mvp-{}-{now}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ))
}

fn cleanup(root: &Path) {
    let _ = fs::remove_dir_all(root);
}
