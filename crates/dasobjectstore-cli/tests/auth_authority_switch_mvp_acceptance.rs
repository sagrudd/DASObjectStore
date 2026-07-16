use axum::body::Body;
use axum::http::{header::COOKIE, Request, StatusCode};
use dasobjectstore_gui_api::LocalAuthStore;
use dasobjectstore_mnemosyne::{monas_dasobjectstore_api_router, MONAS_SESSION_COOKIE};
use prosopikon_core::ProsopikonAuthStore;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use tower::ServiceExt;

const PASSWORD: &str = "generated-auth-switch-only";
const CONFIRMATION: &str = "confirm auth migration";

#[tokio::test]
async fn migrated_monas_authority_and_retained_intrinsic_rollback_both_authenticate() {
    let root = validation_root();
    let source_root = root.join("intrinsic");
    let target_root = root.join("monas");
    std::fs::create_dir_all(&root).expect("validation root created");

    let source = ProsopikonAuthStore::new(&source_root);
    source.create_user("switch-operator").expect("user created");
    let registration = source
        .issue_registration_token("switch-operator", 1)
        .expect("registration token");
    let session = source
        .register_with_token("switch-operator", &registration, PASSWORD)
        .expect("source registration");
    LocalAuthStore::new(&source_root)
        .verify_session("switch-operator", &session.session_token)
        .expect("intrinsic authority accepts source session");

    let dry_run = migration_command(&source_root, &target_root, false)
        .output()
        .expect("dry-run executable launches");
    assert!(
        dry_run.status.success(),
        "{}",
        String::from_utf8_lossy(&dry_run.stderr)
    );
    assert!(
        !target_root.exists(),
        "dry run must not create target state"
    );

    let apply = migration_command(&source_root, &target_root, true)
        .output()
        .expect("apply executable launches");
    assert!(
        apply.status.success(),
        "{}",
        String::from_utf8_lossy(&apply.stderr)
    );
    assert!(target_root
        .join("dasobjectstore-auth-migration.json")
        .is_file());

    let target = ProsopikonAuthStore::new(&target_root);
    target
        .verify_session("switch-operator", &session.session_token)
        .expect("migrated Monas registry preserves compatible session record");
    let cookie = format!(
        "{MONAS_SESSION_COOKIE}=switch-operator:{}",
        session.session_token
    );
    let response = monas_dasobjectstore_api_router(target.clone())
        .oneshot(
            Request::builder()
                .uri("/api/v1/host-session")
                .header(COOKIE, &cookie)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("Monas-composed request");
    assert_eq!(response.status(), StatusCode::OK);

    target
        .logout("switch-operator", &session.session_token)
        .expect("Monas-side logout");
    let response = monas_dasobjectstore_api_router(target)
        .oneshot(
            Request::builder()
                .uri("/api/v1/host-session")
                .header(COOKIE, &cookie)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("revoked request");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    LocalAuthStore::new(&source_root)
        .verify_session("switch-operator", &session.session_token)
        .expect("retained intrinsic source remains a valid rollback authority");
    std::fs::remove_dir_all(root).expect("generated fixture cleanup");
}

fn migration_command(source: &PathBuf, target: &PathBuf, apply: bool) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_dasobjectstore-auth-migrate"));
    command
        .arg("--source-root")
        .arg(source)
        .arg("--target-root")
        .arg(target)
        .arg("--json");
    if apply {
        command.arg("--apply").arg("--confirm").arg(CONFIRMATION);
    }
    command
}

fn validation_root() -> PathBuf {
    let home = std::env::var_os("HOME").expect("HOME is required");
    let approved = PathBuf::from(home).join(".dasobjectstore-codex-validation");
    let base = std::env::var_os("DASOBJECTSTORE_CODEX_VALIDATION_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| approved.clone());
    assert!(
        base.starts_with(&approved),
        "validation root must remain beneath {}",
        approved.display()
    );
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    base.join(format!(
        "auth-authority-switch-{}-{nonce}",
        std::process::id()
    ))
}
