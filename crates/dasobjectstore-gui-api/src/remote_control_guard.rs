//! Authentication boundary for remote HTTPS control-plane routes.
//!
//! The guard reuses the rotated EasyConnect temporary-session token. It never
//! accepts the persistent Garage secret and derives all store authority from
//! the daemon-owned paired-session registry.

use axum::body::Body;
use axum::http::{header::AUTHORIZATION, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use dasobjectstore_daemon::api::{
    RemoteEasyconnectControlAuthorization, RemoteEasyconnectControlOperation,
};
use dasobjectstore_daemon::runtime::{
    remote_easyconnect_session_store_path, FileBackedRemoteEasyconnectPairedSessionStore,
    RemoteEasyconnectPairedSessionStore,
};
use dasobjectstore_daemon::{DaemonClock, DaemonRuntimeConfig, SystemDaemonClock};
use serde::Serialize;
use std::path::PathBuf;

pub const REMOTE_CONTROL_ACCESS_KEY_HEADER: &str = "x-dasobjectstore-access-key-id";

#[derive(Clone, Debug)]
pub struct RemoteControlGuardState {
    session_registry_path: PathBuf,
}

impl RemoteControlGuardState {
    pub fn packaged() -> Self {
        Self {
            session_registry_path: remote_easyconnect_session_store_path(
                DaemonRuntimeConfig::default_packaged().state_dir,
            ),
        }
    }

    #[cfg(test)]
    fn with_session_registry(path: impl Into<PathBuf>) -> Self {
        Self {
            session_registry_path: path.into(),
        }
    }

    pub fn authorize(
        &self,
        headers: &HeaderMap,
        object_store: &str,
        requested_prefix: &str,
        operation: RemoteEasyconnectControlOperation,
    ) -> Result<RemoteEasyconnectControlAuthorization, RemoteControlRejection> {
        let access_key_id = required_header(headers, REMOTE_CONTROL_ACCESS_KEY_HEADER)?;
        let token = bearer_token(headers)?;
        FileBackedRemoteEasyconnectPairedSessionStore::new(&self.session_registry_path)
            .authorize_control(
                access_key_id,
                token,
                object_store,
                requested_prefix,
                operation,
                &SystemDaemonClock.now_utc(),
            )
            .map_err(|_| RemoteControlRejection::Unauthorized)
    }
}

impl Default for RemoteControlGuardState {
    fn default() -> Self {
        Self::packaged()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemoteControlRejection {
    MissingCredentials,
    MalformedCredentials,
    Unauthorized,
}

impl IntoResponse for RemoteControlRejection {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            Self::MissingCredentials => (
                StatusCode::UNAUTHORIZED,
                "remote_control_credentials_required",
                "temporary remote control credentials are required",
            ),
            Self::MalformedCredentials => (
                StatusCode::BAD_REQUEST,
                "invalid_remote_control_credentials",
                "remote control credentials are malformed",
            ),
            Self::Unauthorized => (
                StatusCode::FORBIDDEN,
                "remote_control_not_authorized",
                "the temporary session does not authorize this operation",
            ),
        };
        let body = serde_json::to_vec(&RemoteControlErrorBody { code, message })
            .unwrap_or_else(|_| b"{\"code\":\"remote_control_error\"}".to_vec());
        Response::builder()
            .status(status)
            .header("content-type", "application/json")
            .header("cache-control", "no-store")
            .body(Body::from(body))
            .expect("static remote control response is valid")
    }
}

#[derive(Serialize)]
struct RemoteControlErrorBody {
    code: &'static str,
    message: &'static str,
}

fn required_header<'a>(
    headers: &'a HeaderMap,
    name: &'static str,
) -> Result<&'a str, RemoteControlRejection> {
    let value = headers
        .get(name)
        .ok_or(RemoteControlRejection::MissingCredentials)?;
    let value = value
        .to_str()
        .map_err(|_| RemoteControlRejection::MalformedCredentials)?;
    if value.trim().is_empty() {
        return Err(RemoteControlRejection::MalformedCredentials);
    }
    Ok(value)
}

fn bearer_token(headers: &HeaderMap) -> Result<&str, RemoteControlRejection> {
    let authorization = required_header(headers, AUTHORIZATION.as_str())?;
    let token = authorization
        .strip_prefix("Bearer ")
        .ok_or(RemoteControlRejection::MalformedCredentials)?;
    if token.trim().is_empty() || token != token.trim() {
        return Err(RemoteControlRejection::MalformedCredentials);
    }
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dasobjectstore_daemon::api::{
        RemoteEasyconnectAuthProvider, RemoteEasyconnectObjectStoreGrant,
        RemoteEasyconnectSessionCredentials,
    };
    use dasobjectstore_daemon::runtime::{
        FileBackedRemoteEasyconnectPairedSessionStore, RemoteEasyconnectPairedSessionRecord,
        RemoteEasyconnectPairedSessionStore,
    };

    #[test]
    fn guard_accepts_rotated_temporary_token_for_granted_store() {
        let root = temp_root("accept");
        let path = remote_easyconnect_session_store_path(&root);
        FileBackedRemoteEasyconnectPairedSessionStore::new(&path)
            .upsert(session())
            .expect("session stored");
        let state = RemoteControlGuardState::with_session_registry(path);
        let mut headers = HeaderMap::new();
        headers.insert(
            REMOTE_CONTROL_ACCESS_KEY_HEADER,
            "DOSTROTATED".parse().unwrap(),
        );
        headers.insert(
            AUTHORIZATION,
            "Bearer rotated-control-token".parse().unwrap(),
        );

        let actor = state
            .authorize(
                &headers,
                "epic_collection",
                "EPICv1/",
                RemoteEasyconnectControlOperation::ObjectSnapshot,
            )
            .expect("authorized");
        assert_eq!(actor.approved_actor, "stephen");
        assert_eq!(actor.allowed_prefix, "");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn guard_fails_closed_for_secret_key_and_wrong_store() {
        let root = temp_root("reject");
        let path = remote_easyconnect_session_store_path(&root);
        FileBackedRemoteEasyconnectPairedSessionStore::new(&path)
            .upsert(session())
            .expect("session stored");
        let state = RemoteControlGuardState::with_session_registry(path);
        let mut headers = HeaderMap::new();
        headers.insert(
            REMOTE_CONTROL_ACCESS_KEY_HEADER,
            "DOSTROTATED".parse().unwrap(),
        );
        headers.insert(AUTHORIZATION, "Bearer persistent-secret".parse().unwrap());
        assert_eq!(
            state.authorize(
                &headers,
                "epic_collection",
                "",
                RemoteEasyconnectControlOperation::ObjectSnapshot,
            ),
            Err(RemoteControlRejection::Unauthorized)
        );
        headers.insert(
            AUTHORIZATION,
            "Bearer rotated-control-token".parse().unwrap(),
        );
        assert_eq!(
            state.authorize(
                &headers,
                "other",
                "",
                RemoteEasyconnectControlOperation::ObjectSnapshot,
            ),
            Err(RemoteControlRejection::Unauthorized)
        );
        std::fs::remove_dir_all(root).ok();
    }

    fn session() -> RemoteEasyconnectPairedSessionRecord {
        RemoteEasyconnectPairedSessionRecord {
            pairing_id: "pairing-1".to_string(),
            session_id: "session-1".to_string(),
            authority_id: "authority-1".to_string(),
            principal_id: "stephen".to_string(),
            approved_actor: "stephen".to_string(),
            authority_session_id: "authority-session-1".to_string(),
            auth_provider: RemoteEasyconnectAuthProvider::StandaloneLocalUser,
            correlation_id: "correlation-1".to_string(),
            audit_identity: "standalone:stephen".to_string(),
            issued_at_utc: "2026-07-25T00:00:00Z".to_string(),
            expires_at_utc: "2999-07-25T08:00:00Z".to_string(),
            renew_after_utc: "2999-07-25T07:00:00Z".to_string(),
            renewal_token: "renewal-token".to_string(),
            credentials: RemoteEasyconnectSessionCredentials {
                access_key_id: "DOSTROTATED".to_string(),
                secret_access_key: "persistent-secret".to_string(),
                session_token: Some("rotated-control-token".to_string()),
            },
            object_stores: vec![RemoteEasyconnectObjectStoreGrant {
                object_store: "epic_collection".to_string(),
                bucket: "dos-epic-collection".to_string(),
                can_read: true,
                can_write: true,
                writer_group: Some("mnemosyne".to_string()),
                object_type: "generated_data".to_string(),
                control_operations:
                    dasobjectstore_daemon::api::remote_easyconnect_control_operations(true),
                allowed_prefixes: vec!["".to_string()],
            }],
            revoked_at_utc: None,
        }
    }

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "dasobjectstore-remote-control-guard-{label}-{}",
            uuid::Uuid::new_v4()
        ))
    }
}
