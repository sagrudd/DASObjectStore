use dasobjectstore_mnemosyne::{
    accept_synoptikon_integrated_session, StorageAuthority,
    SynoptikonIntegratedHostBoundaryContext, SynoptikonIntegratedSessionError,
    SynoptikonIntegratedSessionIssue, DASOBJECTSTORE_PRODUCT_ID, REQUEST_CONTEXT_SCHEMA_VERSION,
    SYNOPTIKON_INTEGRATED_SESSION_SCHEMA_VERSION,
};

#[test]
fn accepts_valid_synoptikon_issued_actor_session() {
    let issue = valid_issue();

    let session = accept_synoptikon_integrated_session(&issue, 1_050).expect("session accepted");

    assert_eq!(
        session.schema_version,
        SYNOPTIKON_INTEGRATED_SESSION_SCHEMA_VERSION
    );
    assert_eq!(session.request_id, "request-1");
    assert_eq!(session.accepted_at_unix_seconds, 1_050);
    assert_eq!(session.actor.tenant_id, "tenant-1");
    assert_eq!(session.actor.account_id, "account-1");
    assert_eq!(session.actor.user_id, "user-1");
    assert_eq!(session.actor.project_id, "project-1");
    assert_eq!(session.actor.entitlement_id, "entitlement-1");
    assert_eq!(session.actor.roles, ["storage_operator"]);
    assert_eq!(session.correlation_id, "corr-1");
    assert_eq!(session.storage_binding_id, "binding-1");
}

#[test]
fn rejects_expired_synoptikon_issued_actor_session() {
    let issue = valid_issue();

    let err =
        accept_synoptikon_integrated_session(&issue, 2_000).expect_err("expired issue rejected");

    assert_eq!(
        err,
        SynoptikonIntegratedSessionError::Expired {
            expires_at: 2_000,
            accepted_at: 2_000
        }
    );
}

#[test]
fn rejects_issue_with_invalid_request_id() {
    let mut issue = valid_issue();
    issue.request_id = "bad request id".to_string();

    let err = accept_synoptikon_integrated_session(&issue, 1_050).expect_err("request id rejected");

    assert!(matches!(
        err,
        SynoptikonIntegratedSessionError::InvalidRequestId { .. }
    ));
}

#[test]
fn rejects_issue_when_synoptikon_boundary_is_invalid() {
    let mut issue = valid_issue();
    issue.context.storage_authority = StorageAuthority::LocalProductState;

    let err =
        accept_synoptikon_integrated_session(&issue, 1_050).expect_err("host boundary rejected");

    assert!(matches!(
        err,
        SynoptikonIntegratedSessionError::HostBoundary(_)
    ));
}

#[test]
fn serializes_accepted_session_without_local_secret() {
    let issue = valid_issue();
    let session = accept_synoptikon_integrated_session(&issue, 1_050).expect("session accepted");

    let serialized = serde_json::to_value(session).expect("session serializes");

    assert_eq!(
        serialized["schema_version"],
        SYNOPTIKON_INTEGRATED_SESSION_SCHEMA_VERSION
    );
    assert_eq!(serialized["request_id"], "request-1");
    assert!(serialized.get("session_token").is_none());
    assert!(serialized.get("password_hash").is_none());
    assert_eq!(serialized["actor"]["user_id"], "user-1");
    assert_eq!(serialized["actor"]["roles"][0], "storage_operator");
}

fn valid_issue() -> SynoptikonIntegratedSessionIssue {
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
            correlation_id: "corr-1".to_string(),
            central_audit_enabled: true,
            storage_authority: StorageAuthority::SynoptikonStorageBinding,
            storage_binding_id: "binding-1".to_string(),
        },
    }
}
