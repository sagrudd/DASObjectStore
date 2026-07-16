use axum::{
    body::Body,
    extract::Extension,
    http::{Request, StatusCode},
    routing::get,
    Router,
};
use dasobjectstore_gui_api::{AuthenticatedGuiActor, HostAuthenticationAuthority};
use dasobjectstore_mnemosyne::{
    accept_monas_host_session, accept_synoptikon_host_session, HostSessionAdapterError,
    MonasHostSessionIssue, StorageAuthority, SynoptikonIntegratedAcceptedSession,
    SynoptikonIntegratedHostBoundaryContext, SynoptikonIntegratedSessionIssue,
    SynoptikonLiveSessionVerifier, DASOBJECTSTORE_PRODUCT_ID, REQUEST_CONTEXT_SCHEMA_VERSION,
};
use prosopikon_core::ProsopikonAuthStore;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tower::ServiceExt;

#[tokio::test]
async fn live_monas_session_drives_gui_actor_without_exposing_bearer() {
    let root = temp_root("monas-live");
    let store = registered_store(&root);
    let login = store
        .login_with_session_ttl_seconds("operator", "secret", Some(3_600))
        .expect("login succeeds");
    let now = unix_now();
    let issue = MonasHostSessionIssue {
        username: "operator".to_string(),
        session_token: login.session_token.clone(),
        correlation_id: "corr-monas-1".to_string(),
        csrf_binding_sha256: csrf_binding(),
    };

    let verified = accept_monas_host_session(&store, &issue, now).expect("session accepted");
    let context = verified.context();
    assert_eq!(
        context.authority,
        HostAuthenticationAuthority::MonasStandalone
    );
    assert_eq!(context.subject_id, "operator");
    assert_eq!(context.roles, ["authenticated"]);
    assert!(context.expires_at_unix_seconds <= now + 300);
    let serialized = serde_json::to_string(context).expect("context serializes");
    assert!(!serialized.contains(&login.session_token));
    assert!(!serialized.contains("storage_binding"));
    assert_gui_accepts(verified).await;

    store
        .logout("operator", &login.session_token)
        .expect("logout succeeds");
    let rejection = accept_monas_host_session(&store, &issue, now).expect_err("logout revokes");
    assert!(matches!(
        &rejection,
        HostSessionAdapterError::MonasSession(_)
    ));
    assert!(!rejection.to_string().contains(&login.session_token));
    cleanup(&root);
}

#[tokio::test]
async fn live_synoptikon_session_drives_gui_actor_without_storage_grant() {
    let issue = synoptikon_issue();
    let verified =
        accept_synoptikon_host_session(&issue, csrf_binding(), 1_500, &LiveSynoptikon(true))
            .expect("session accepted");
    let context = verified.context();
    assert_eq!(
        context.authority,
        HostAuthenticationAuthority::SynoptikonIntegrated
    );
    assert_eq!(context.subject_id, "user-1");
    let serialized = serde_json::to_value(context).expect("context serializes");
    assert!(serialized.get("storage_binding_id").is_none());
    assert!(serialized.get("storage_authority").is_none());
    assert_gui_accepts(verified).await;

    assert!(matches!(
        accept_synoptikon_host_session(&issue, csrf_binding(), 1_500, &LiveSynoptikon(false)),
        Err(HostSessionAdapterError::HostContext(_))
    ));
}

#[test]
fn synoptikon_adapter_rejects_invalid_boundary_and_overlong_context() {
    let mut invalid = synoptikon_issue();
    invalid.context.central_audit_enabled = false;
    assert!(matches!(
        accept_synoptikon_host_session(&invalid, csrf_binding(), 1_500, &LiveSynoptikon(true)),
        Err(HostSessionAdapterError::SynoptikonSession(_))
    ));

    let mut overlong = synoptikon_issue();
    overlong.expires_at_unix_seconds = overlong.issued_at_unix_seconds + 8 * 60 * 60 + 1;
    assert!(matches!(
        accept_synoptikon_host_session(&overlong, csrf_binding(), 1_500, &LiveSynoptikon(true)),
        Err(HostSessionAdapterError::HostContext(_))
    ));
}

async fn assert_gui_accepts(verified: dasobjectstore_gui_api::VerifiedHostAuthenticatedContext) {
    async fn protected(_actor: AuthenticatedGuiActor) -> StatusCode {
        StatusCode::OK
    }
    let app = Router::new()
        .route("/protected", get(protected))
        .layer(Extension(verified));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/protected")
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("request completes");
    assert_eq!(response.status(), StatusCode::OK);
}

struct LiveSynoptikon(bool);

impl SynoptikonLiveSessionVerifier for LiveSynoptikon {
    fn verify_live_session(
        &self,
        _session: &SynoptikonIntegratedAcceptedSession,
    ) -> Result<(), String> {
        self.0.then_some(()).ok_or_else(|| "revoked".to_string())
    }
}

fn synoptikon_issue() -> SynoptikonIntegratedSessionIssue {
    SynoptikonIntegratedSessionIssue {
        request_id: "request-1".to_string(),
        issued_at_unix_seconds: 1_000,
        expires_at_unix_seconds: 2_000,
        context: SynoptikonIntegratedHostBoundaryContext {
            request_context_schema_version: REQUEST_CONTEXT_SCHEMA_VERSION.to_string(),
            product_id: DASOBJECTSTORE_PRODUCT_ID.to_string(),
            tenant_id: "tenant-1".to_string(),
            account_id: "account-1".to_string(),
            user_id: "user-1".to_string(),
            project_id: "project-1".to_string(),
            entitlement_id: "entitlement-1".to_string(),
            roles: vec!["storage_operator".to_string()],
            correlation_id: "corr-synoptikon-1".to_string(),
            central_audit_enabled: true,
            storage_authority: StorageAuthority::SynoptikonStorageBinding,
            storage_binding_id: "binding-1".to_string(),
        },
    }
}

fn registered_store(root: &Path) -> ProsopikonAuthStore {
    let store = ProsopikonAuthStore::new(root);
    store.create_user("operator").expect("user created");
    let registration = store
        .issue_registration_token("operator", 1)
        .expect("registration issued");
    store
        .register_with_token("operator", &registration, "secret")
        .expect("registration succeeds");
    store
}

fn csrf_binding() -> String {
    format!("sha256:{}", "a".repeat(64))
}

fn temp_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "dasobjectstore-host-adapter-{label}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos()
    ))
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_secs() as i64
}

fn cleanup(root: &Path) {
    let _ = fs::remove_dir_all(root);
}
