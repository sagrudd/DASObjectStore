use super::{AuthTokenResetReport, LocalAuthStore, LocalAuthStoreError};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn registration_stores_hashes_not_plaintext() {
    let root = temp_root("registration");
    let store = LocalAuthStore::new(&root);
    store.create_user("admin").expect("user created");
    let token = store
        .issue_registration_token("admin", Some(3_600))
        .expect("token issued");

    store
        .register_with_token("admin", &token, "do-not-store-this")
        .expect("registered");

    let json = fs::read_to_string(store.registry_path()).expect("registry reads");
    assert!(json.contains("password_hash"));
    assert!(json.contains("token_hash"));
    assert!(!json.contains("do-not-store-this"));
    assert!(!json.contains(&token));

    cleanup(&root);
}

#[test]
fn registration_token_is_one_time() {
    let root = temp_root("one-time");
    let store = LocalAuthStore::new(&root);
    store.create_user("admin").expect("user created");
    let token = store
        .issue_registration_token("admin", Some(3_600))
        .expect("token issued");
    store
        .register_with_token("admin", &token, "secret")
        .expect("registered");

    let err = store
        .register_with_token("admin", &token, "secret")
        .expect_err("second registration rejected");

    assert!(matches!(
        err,
        LocalAuthStoreError::UserAlreadyRegistered { .. }
    ));

    cleanup(&root);
}

#[test]
fn login_issues_verifiable_session_and_logout_revokes_it() {
    let root = temp_root("session");
    let store = registered_store(&root);

    let login = store.login("admin", "secret").expect("login succeeds");
    let session = store
        .verify_session("admin", &login.session_token)
        .expect("session verifies");
    let logout = store
        .logout("admin", &login.session_token)
        .expect("logout succeeds");
    let err = store
        .verify_session("admin", &login.session_token)
        .expect_err("session revoked");

    assert_eq!(session.username, "admin");
    assert!(logout.disconnected);
    assert!(matches!(err, LocalAuthStoreError::InvalidSessionToken));

    cleanup(&root);
}

#[test]
fn rejects_invalid_password() {
    let root = temp_root("invalid-password");
    let store = registered_store(&root);

    let err = store
        .login("admin", "wrong-password")
        .expect_err("invalid password rejected");

    assert!(matches!(err, LocalAuthStoreError::InvalidPassword));

    cleanup(&root);
}

#[test]
fn os_authenticated_user_session_is_created_without_password_registration() {
    let root = temp_root("os-auth-session");
    let store = LocalAuthStore::new(&root);

    let login = store
        .create_session_for_authenticated_local_user(" stephen ", Some(3_600))
        .expect("local OS session created");
    let session = store
        .verify_session("stephen", &login.session_token)
        .expect("session verifies");
    let users = store.list_users().expect("users list");

    assert_eq!(login.username, "stephen");
    assert!(session.valid);
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].username, "stephen");
    assert!(!users[0].registered);

    cleanup(&root);
}

#[test]
fn session_ttl_defaults_to_one_hour() {
    let root = temp_root("ttl");
    let store = registered_store(&root);
    let before = unix_now_seconds();

    let login = store.login("admin", "secret").expect("login succeeds");
    let after = unix_now_seconds();

    assert!(login.expires_at_unix_seconds >= before + 3_590);
    assert!(login.expires_at_unix_seconds <= after + 3_610);

    cleanup(&root);
}

#[test]
fn reset_all_tokens_revokes_sessions_and_unused_registration_tokens() {
    let root = temp_root("reset");
    let store = registered_store(&root);
    store.create_user("helper").expect("helper created");
    let helper_token = store
        .issue_registration_token("helper", Some(3_600))
        .expect("helper token issued");
    let login = store.login("admin", "secret").expect("login succeeds");

    let report = store.reset_all_tokens().expect("tokens reset");
    let session_err = store
        .verify_session("admin", &login.session_token)
        .expect_err("session revoked");
    let registration_err = store
        .register_with_token("helper", &helper_token, "secret")
        .expect_err("token revoked");

    assert_eq!(
        report,
        AuthTokenResetReport {
            revoked_sessions: 2,
            revoked_registration_tokens: 1,
        }
    );
    assert!(matches!(
        session_err,
        LocalAuthStoreError::InvalidSessionToken
    ));
    assert!(matches!(
        registration_err,
        LocalAuthStoreError::UsedRegistrationToken
    ));

    cleanup(&root);
}

fn registered_store(root: &Path) -> LocalAuthStore {
    let store = LocalAuthStore::new(root);
    store.create_user("admin").expect("user created");
    let token = store
        .issue_registration_token("admin", Some(3_600))
        .expect("token issued");
    store
        .register_with_token("admin", &token, "secret")
        .expect("registered");
    store
}

fn temp_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "dasobjectstore-local-auth-{label}-{}-{}",
        std::process::id(),
        unix_now_nanos()
    ))
}

fn unix_now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after unix epoch")
        .as_secs() as i64
}

fn unix_now_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after unix epoch")
        .as_nanos()
}

fn cleanup(root: &Path) {
    let _ = fs::remove_dir_all(root);
}
