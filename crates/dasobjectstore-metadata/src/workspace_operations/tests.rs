use super::*;
use std::fs;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

const T0: &str = "2026-07-26T10:00:00Z";
const T1: &str = "2026-07-26T10:01:00Z";
const T2: &str = "2026-07-26T10:02:00Z";
const T3: &str = "2026-07-26T10:03:00Z";
const DIGEST_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

struct Fixture {
    path: PathBuf,
    workspace_id: WorkspaceId,
}

impl Fixture {
    fn new() -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "dasobjectstore-workspace-operations-{}-{unique}.sqlite3",
            std::process::id()
        ));
        let connection = Connection::open(&path).expect("open fixture");
        connection
            .execute_batch(LIVE_SCHEMA_SQL)
            .expect("create schema");
        connection
            .execute(
                "INSERT INTO pools (pool_id, state, created_at_utc, updated_at_utc)
                     VALUES ('pool-test', 'online', ?1, ?1)",
                [T0],
            )
            .expect("insert pool");
        connection
            .execute(
                "INSERT INTO compute_workspaces (
                        workspace_id, schema_version, request_id, request_digest, pool_id,
                        state, owner, project, purpose, requested_capacity_bytes,
                        reserved_capacity_bytes, quota_bytes, minimum_free_bytes_per_disk,
                        aggregation_provider, close_cleanup_policy_json, generation,
                        created_at_utc, updated_at_utc, expires_at_utc
                     ) VALUES (
                        'workspace-test', 1, 'workspace-request', ?1, 'pool-test',
                        'capacity_reserved', 'tester', 'tests', 'operation tests',
                        1024, 1024, 1024, 0, 'mergerfs', '{}', 1, ?2, ?2, ?3
                     )",
                params![DIGEST_A, T0, "2026-07-27T10:00:00Z"],
            )
            .expect("insert workspace");
        Self {
            path,
            workspace_id: WorkspaceId::new("workspace-test").expect("workspace ID"),
        }
    }

    fn request(
        &self,
        operation_id: &str,
        request_id: &str,
        disposition: WorkspaceRecoveryDisposition,
    ) -> SubmitWorkspaceOperationRequest {
        SubmitWorkspaceOperationRequest {
            live_sqlite_path: self.path.clone(),
            operation_id: operation_id.to_string(),
            workspace_id: self.workspace_id.clone(),
            kind: WorkspaceOperationKind::Provision,
            request_id: request_id.to_string(),
            request_digest: DIGEST_A.to_string(),
            initial_stage: "reserved".to_string(),
            total_bytes: Some(100),
            total_units: Some(2),
            max_attempts: 3,
            recovery_disposition: disposition,
            created_at_utc: T0.to_string(),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        let _ = fs::remove_file(self.path.with_extension("sqlite3-shm"));
        let _ = fs::remove_file(self.path.with_extension("sqlite3-wal"));
    }
}

#[test]
fn submission_is_idempotent_and_rejects_changed_request_content() {
    let fixture = Fixture::new();
    let request = fixture.request(
        "operation-one",
        "request-one",
        WorkspaceRecoveryDisposition::VerifyExternalEffect,
    );
    let created = submit_workspace_operation(&request).expect("submit");
    assert_eq!(created.state, WorkspaceOperationState::Queued);
    assert_eq!(
        submit_workspace_operation(&request).expect("replay"),
        created
    );

    let mut changed = request;
    changed.request_digest = DIGEST_B.to_string();
    assert!(matches!(
        submit_workspace_operation(&changed),
        Err(WorkspaceOperationError::RequestIdentityConflict { .. })
    ));
}

#[test]
fn lease_checkpoint_and_completion_are_fenced_and_idempotent() {
    let fixture = Fixture::new();
    let queued = submit_workspace_operation(&fixture.request(
        "operation-two",
        "request-two",
        WorkspaceRecoveryDisposition::VerifyExternalEffect,
    ))
    .expect("submit");
    let running = claim_workspace_operation(
        &fixture.path,
        &queued.operation_id,
        "worker-a",
        queued.generation,
        T1,
        T3,
    )
    .expect("claim");
    assert_eq!(running.lease_epoch, 1);
    assert!(matches!(
        renew_workspace_operation_lease(
            &fixture.path,
            &running.operation_id,
            "worker-b",
            running.generation,
            T1,
            T3
        ),
        Err(WorkspaceOperationError::LeaseOwnerMismatch { .. })
    ));

    let checkpointed = checkpoint_workspace_operation(
        &fixture.path,
        &running.operation_id,
        "worker-a",
        running.generation,
        "branch_created",
        50,
        1,
        WorkspaceRecoveryDisposition::ResumeCheckpoint,
        DIGEST_B,
        r#"{"branch_id":"branch-a"}"#,
        T2,
    )
    .expect("checkpoint");
    let replayed = checkpoint_workspace_operation(
        &fixture.path,
        &running.operation_id,
        "worker-a",
        running.generation,
        "branch_created",
        50,
        1,
        WorkspaceRecoveryDisposition::ResumeCheckpoint,
        DIGEST_B,
        r#"{"branch_id":"branch-a"}"#,
        T2,
    )
    .expect("checkpoint replay");
    assert_eq!(replayed.generation, checkpointed.generation);
    assert_eq!(
        replayed
            .latest_checkpoint
            .as_ref()
            .expect("checkpoint")
            .sequence,
        1
    );
    assert!(matches!(
        checkpoint_workspace_operation(
            &fixture.path,
            &running.operation_id,
            "worker-a",
            checkpointed.generation,
            "bad",
            49,
            1,
            WorkspaceRecoveryDisposition::ResumeCheckpoint,
            DIGEST_A,
            "{}",
            T3
        ),
        Err(WorkspaceOperationError::InvalidRequest { .. })
    ));
    assert!(matches!(
        checkpoint_workspace_operation(
            &fixture.path,
            &running.operation_id,
            "worker-a",
            checkpointed.generation,
            "bad",
            60,
            2,
            WorkspaceRecoveryDisposition::ResumeCheckpoint,
            DIGEST_A,
            r#"{"source_path":"/tmp/secret"}"#,
            T3
        ),
        Err(WorkspaceOperationError::InvalidRequest { .. })
    ));

    let finished = finish_workspace_operation(
        &fixture.path,
        &running.operation_id,
        "worker-a",
        checkpointed.generation,
        WorkspaceOperationState::Succeeded,
        Some(r#"{"published":true}"#),
        None,
        None,
        T3,
    )
    .expect("finish");
    assert_eq!(finished.state, WorkspaceOperationState::Succeeded);
    assert_eq!(
        finish_workspace_operation(
            &fixture.path,
            &running.operation_id,
            "worker-a",
            checkpointed.generation,
            WorkspaceOperationState::Succeeded,
            Some(r#"{"published":true}"#),
            None,
            None,
            T3
        )
        .expect("finish replay")
        .generation,
        finished.generation
    );
}

#[test]
fn restart_recovery_resumes_only_proven_safe_operations() {
    let fixture = Fixture::new();
    let retry = submit_workspace_operation(&fixture.request(
        "operation-retry",
        "request-retry",
        WorkspaceRecoveryDisposition::RetryIdempotent,
    ))
    .expect("submit retry");
    claim_workspace_operation(
        &fixture.path,
        &retry.operation_id,
        "worker",
        retry.generation,
        T0,
        T1,
    )
    .expect("claim retry");

    let ambiguous = submit_workspace_operation(&fixture.request(
        "operation-ambiguous",
        "request-ambiguous",
        WorkspaceRecoveryDisposition::VerifyExternalEffect,
    ))
    .expect("submit ambiguous");
    claim_workspace_operation(
        &fixture.path,
        &ambiguous.operation_id,
        "worker",
        ambiguous.generation,
        T0,
        T1,
    )
    .expect("claim ambiguous");

    let records = recover_expired_workspace_operations(&fixture.path, T2).expect("recover expired");
    assert!(records.iter().any(|record| {
        record.operation_id == "operation-retry"
            && record.action == WorkspaceOperationRecoveryAction::RetryIdempotent
    }));
    assert!(records.iter().any(|record| {
        record.operation_id == "operation-ambiguous"
            && record.action == WorkspaceOperationRecoveryAction::NeedsReview
    }));
    assert_eq!(
        read_workspace_operation(&fixture.path, "operation-retry")
            .expect("retry")
            .state,
        WorkspaceOperationState::Queued
    );
    assert_eq!(
        read_workspace_operation(&fixture.path, "operation-ambiguous")
            .expect("ambiguous")
            .state,
        WorkspaceOperationState::NeedsReview
    );
}

#[test]
fn concurrent_claim_has_one_winner() {
    let fixture = Fixture::new();
    let queued = submit_workspace_operation(&fixture.request(
        "operation-race",
        "request-race",
        WorkspaceRecoveryDisposition::RetryIdempotent,
    ))
    .expect("submit");
    let barrier = Arc::new(Barrier::new(3));
    let handles = ["worker-a", "worker-b"].map(|worker| {
        let barrier = Arc::clone(&barrier);
        let path = fixture.path.clone();
        let operation_id = queued.operation_id.clone();
        thread::spawn(move || {
            barrier.wait();
            claim_workspace_operation(&path, &operation_id, worker, queued.generation, T0, T3)
        })
    });
    barrier.wait();
    let outcomes = handles.map(|handle| handle.join().expect("claim thread"));
    assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
    assert_eq!(
        outcomes.iter().filter(|outcome| outcome.is_err()).count(),
        1
    );
}
