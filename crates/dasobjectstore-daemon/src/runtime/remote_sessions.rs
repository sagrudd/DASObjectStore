use crate::api::{
    RemoteEasyconnectAuthProvider, RemoteEasyconnectObjectStoreGrant,
    RemoteEasyconnectSessionCredentials,
};
use crate::auth::DaemonLocalActor;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const REMOTE_EASYCONNECT_SESSION_DIR_NAME: &str = "remote-easyconnect";
pub const REMOTE_EASYCONNECT_SESSION_FILE_NAME: &str = "sessions.json";
pub const REMOTE_EASYCONNECT_SESSION_SCHEMA: &str = "dasobjectstore.remote_easyconnect.sessions.v1";

pub fn remote_easyconnect_session_store_path(state_dir: impl AsRef<Path>) -> PathBuf {
    state_dir
        .as_ref()
        .join(REMOTE_EASYCONNECT_SESSION_DIR_NAME)
        .join(REMOTE_EASYCONNECT_SESSION_FILE_NAME)
}

pub trait RemoteEasyconnectPairedSessionStore: Send + Sync {
    fn upsert(
        &self,
        session: RemoteEasyconnectPairedSessionRecord,
    ) -> Result<(), RemoteEasyconnectPairedSessionStoreError>;

    fn get(
        &self,
        session_id: &str,
    ) -> Result<
        Option<RemoteEasyconnectPairedSessionRecord>,
        RemoteEasyconnectPairedSessionStoreError,
    >;

    fn revoke(
        &self,
        session_id: &str,
        revoked_at_utc: &str,
    ) -> Result<bool, RemoteEasyconnectPairedSessionStoreError>;

    fn renew(
        &self,
        request: RemoteEasyconnectPairedSessionRenewalRequest,
    ) -> Result<RemoteEasyconnectPairedSessionRecord, RemoteEasyconnectPairedSessionStoreError>;

    fn authorize_write(
        &self,
        session_id: &str,
        object_store: &str,
        actor: &DaemonLocalActor,
        now_utc: &str,
    ) -> Result<RemoteEasyconnectObjectStoreGrant, RemoteEasyconnectPairedSessionStoreError>;
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectPairedSessionRecord {
    pub session_id: String,
    pub approved_actor: String,
    pub auth_provider: RemoteEasyconnectAuthProvider,
    pub issued_at_utc: String,
    pub expires_at_utc: String,
    pub renew_after_utc: String,
    pub renewal_token: String,
    pub credentials: RemoteEasyconnectSessionCredentials,
    pub object_stores: Vec<RemoteEasyconnectObjectStoreGrant>,
    pub revoked_at_utc: Option<String>,
}

impl RemoteEasyconnectPairedSessionRecord {
    pub fn validate(&self) -> Result<(), RemoteEasyconnectPairedSessionStoreError> {
        require_non_blank("session_id", &self.session_id)?;
        require_non_blank("approved_actor", &self.approved_actor)?;
        require_non_blank("issued_at_utc", &self.issued_at_utc)?;
        require_non_blank("expires_at_utc", &self.expires_at_utc)?;
        require_non_blank("renew_after_utc", &self.renew_after_utc)?;
        require_non_blank("renewal_token", &self.renewal_token)?;
        require_non_blank("credentials.access_key_id", &self.credentials.access_key_id)?;
        require_non_blank(
            "credentials.secret_access_key",
            &self.credentials.secret_access_key,
        )?;
        if self
            .credentials
            .session_token
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(RemoteEasyconnectPairedSessionStoreError::BlankField {
                field: "credentials.session_token",
            });
        }
        if self.object_stores.is_empty() {
            return Err(RemoteEasyconnectPairedSessionStoreError::BlankField {
                field: "object_stores",
            });
        }
        for grant in &self.object_stores {
            grant.validate().map_err(|error| {
                RemoteEasyconnectPairedSessionStoreError::InvalidGrant {
                    message: error.to_string(),
                }
            })?;
        }
        if self
            .revoked_at_utc
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(RemoteEasyconnectPairedSessionStoreError::BlankField {
                field: "revoked_at_utc",
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteEasyconnectPairedSessionRenewalRequest {
    pub session_id: String,
    pub renewal_token: String,
    pub renewed_at_utc: String,
    pub expires_at_utc: String,
    pub renew_after_utc: String,
    pub rotated_renewal_token: String,
}

#[derive(Debug)]
pub struct FileBackedRemoteEasyconnectPairedSessionStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl FileBackedRemoteEasyconnectPairedSessionStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            lock: Mutex::new(()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl RemoteEasyconnectPairedSessionStore for FileBackedRemoteEasyconnectPairedSessionStore {
    fn upsert(
        &self,
        session: RemoteEasyconnectPairedSessionRecord,
    ) -> Result<(), RemoteEasyconnectPairedSessionStoreError> {
        session.validate()?;
        let _guard = self
            .lock
            .lock()
            .expect("paired session store lock poisoned");
        let mut store = read_store(&self.path)?;
        store.upsert(session);
        write_store(&self.path, &store)
    }

    fn get(
        &self,
        session_id: &str,
    ) -> Result<
        Option<RemoteEasyconnectPairedSessionRecord>,
        RemoteEasyconnectPairedSessionStoreError,
    > {
        require_non_blank("session_id", session_id)?;
        let _guard = self
            .lock
            .lock()
            .expect("paired session store lock poisoned");
        Ok(read_store(&self.path)?.session(session_id).cloned())
    }

    fn revoke(
        &self,
        session_id: &str,
        revoked_at_utc: &str,
    ) -> Result<bool, RemoteEasyconnectPairedSessionStoreError> {
        require_non_blank("session_id", session_id)?;
        require_non_blank("revoked_at_utc", revoked_at_utc)?;
        let _guard = self
            .lock
            .lock()
            .expect("paired session store lock poisoned");
        let mut store = read_store(&self.path)?;
        let Some(session) = store.session_mut(session_id) else {
            return Ok(false);
        };
        session.revoked_at_utc = Some(revoked_at_utc.to_string());
        write_store(&self.path, &store)?;
        Ok(true)
    }

    fn renew(
        &self,
        request: RemoteEasyconnectPairedSessionRenewalRequest,
    ) -> Result<RemoteEasyconnectPairedSessionRecord, RemoteEasyconnectPairedSessionStoreError>
    {
        require_non_blank("session_id", &request.session_id)?;
        require_non_blank("renewal_token", &request.renewal_token)?;
        require_non_blank("renewed_at_utc", &request.renewed_at_utc)?;
        require_non_blank("expires_at_utc", &request.expires_at_utc)?;
        require_non_blank("renew_after_utc", &request.renew_after_utc)?;
        require_non_blank("rotated_renewal_token", &request.rotated_renewal_token)?;
        let _guard = self
            .lock
            .lock()
            .expect("paired session store lock poisoned");
        let mut store = read_store(&self.path)?;
        let Some(session) = store.session_mut(&request.session_id) else {
            return Err(RemoteEasyconnectPairedSessionStoreError::SessionNotFound {
                session_id: request.session_id,
            });
        };
        ensure_session_usable(session, &request.renewed_at_utc)?;
        if session.renewal_token != request.renewal_token {
            return Err(
                RemoteEasyconnectPairedSessionStoreError::RenewalTokenMismatch {
                    session_id: request.session_id,
                },
            );
        }
        session.issued_at_utc = request.renewed_at_utc;
        session.expires_at_utc = request.expires_at_utc;
        session.renew_after_utc = request.renew_after_utc;
        session.renewal_token = request.rotated_renewal_token;
        let renewed = session.clone();
        write_store(&self.path, &store)?;
        Ok(renewed)
    }

    fn authorize_write(
        &self,
        session_id: &str,
        object_store: &str,
        actor: &DaemonLocalActor,
        now_utc: &str,
    ) -> Result<RemoteEasyconnectObjectStoreGrant, RemoteEasyconnectPairedSessionStoreError> {
        require_non_blank("session_id", session_id)?;
        require_non_blank("object_store", object_store)?;
        require_non_blank("now_utc", now_utc)?;
        let _guard = self
            .lock
            .lock()
            .expect("paired session store lock poisoned");
        let store = read_store(&self.path)?;
        let Some(session) = store.session(session_id) else {
            return Err(RemoteEasyconnectPairedSessionStoreError::SessionNotFound {
                session_id: session_id.to_string(),
            });
        };
        ensure_session_usable(session, now_utc)?;
        let actor_name = actor.display_name();
        if session.approved_actor != actor_name {
            return Err(RemoteEasyconnectPairedSessionStoreError::ActorMismatch {
                session_id: session_id.to_string(),
                expected_actor: session.approved_actor.clone(),
                actual_actor: actor_name,
            });
        }
        let Some(grant) = session
            .object_stores
            .iter()
            .find(|grant| grant.object_store == object_store)
        else {
            return Err(
                RemoteEasyconnectPairedSessionStoreError::ObjectStoreNotGranted {
                    session_id: session_id.to_string(),
                    object_store: object_store.to_string(),
                },
            );
        };
        if !grant.can_write {
            return Err(
                RemoteEasyconnectPairedSessionStoreError::ObjectStoreNotWritable {
                    session_id: session_id.to_string(),
                    object_store: object_store.to_string(),
                    writer_group: grant.writer_group.clone(),
                },
            );
        }
        Ok(grant.clone())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct RemoteEasyconnectPairedSessionStoreFile {
    schema_version: String,
    sessions: Vec<RemoteEasyconnectPairedSessionRecord>,
}

impl Default for RemoteEasyconnectPairedSessionStoreFile {
    fn default() -> Self {
        Self {
            schema_version: REMOTE_EASYCONNECT_SESSION_SCHEMA.to_string(),
            sessions: Vec::new(),
        }
    }
}

impl RemoteEasyconnectPairedSessionStoreFile {
    fn session(&self, session_id: &str) -> Option<&RemoteEasyconnectPairedSessionRecord> {
        self.sessions
            .iter()
            .find(|session| session.session_id == session_id)
    }

    fn session_mut(
        &mut self,
        session_id: &str,
    ) -> Option<&mut RemoteEasyconnectPairedSessionRecord> {
        self.sessions
            .iter_mut()
            .find(|session| session.session_id == session_id)
    }

    fn upsert(&mut self, session: RemoteEasyconnectPairedSessionRecord) {
        if let Some(existing) = self.session_mut(&session.session_id) {
            *existing = session;
        } else {
            self.sessions.push(session);
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RemoteEasyconnectPairedSessionStoreError {
    BlankField {
        field: &'static str,
    },
    InvalidGrant {
        message: String,
    },
    Io {
        path: PathBuf,
        message: String,
    },
    Json {
        path: PathBuf,
        message: String,
    },
    SessionNotFound {
        session_id: String,
    },
    SessionRevoked {
        session_id: String,
        revoked_at_utc: String,
    },
    SessionExpired {
        session_id: String,
        expires_at_utc: String,
    },
    RenewalTokenMismatch {
        session_id: String,
    },
    ActorMismatch {
        session_id: String,
        expected_actor: String,
        actual_actor: String,
    },
    ObjectStoreNotGranted {
        session_id: String,
        object_store: String,
    },
    ObjectStoreNotWritable {
        session_id: String,
        object_store: String,
        writer_group: Option<String>,
    },
}

impl std::fmt::Display for RemoteEasyconnectPairedSessionStoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BlankField { field } => write!(formatter, "{field} must not be blank"),
            Self::InvalidGrant { message } => write!(formatter, "invalid object store grant: {message}"),
            Self::Io { path, message } => {
                write!(formatter, "{} IO failed: {message}", path.display())
            }
            Self::Json { path, message } => {
                write!(formatter, "{} JSON is invalid: {message}", path.display())
            }
            Self::SessionNotFound { session_id } => {
                write!(formatter, "paired easyconnect session {session_id} was not found")
            }
            Self::SessionRevoked {
                session_id,
                revoked_at_utc,
            } => write!(
                formatter,
                "paired easyconnect session {session_id} was revoked at {revoked_at_utc}"
            ),
            Self::SessionExpired {
                session_id,
                expires_at_utc,
            } => write!(
                formatter,
                "paired easyconnect session {session_id} expired at {expires_at_utc}"
            ),
            Self::RenewalTokenMismatch { session_id } => write!(
                formatter,
                "paired easyconnect session {session_id} renewal token did not match"
            ),
            Self::ActorMismatch {
                session_id,
                expected_actor,
                actual_actor,
            } => write!(
                formatter,
                "paired easyconnect session {session_id} belongs to {expected_actor}, not {actual_actor}"
            ),
            Self::ObjectStoreNotGranted {
                session_id,
                object_store,
            } => write!(
                formatter,
                "paired easyconnect session {session_id} does not grant ObjectStore {object_store}"
            ),
            Self::ObjectStoreNotWritable {
                session_id,
                object_store,
                writer_group,
            } => write!(
                formatter,
                "paired easyconnect session {session_id} does not allow writing ObjectStore {object_store}; writer group {:?}",
                writer_group
            ),
        }
    }
}

impl std::error::Error for RemoteEasyconnectPairedSessionStoreError {}

fn read_store(
    path: &Path,
) -> Result<RemoteEasyconnectPairedSessionStoreFile, RemoteEasyconnectPairedSessionStoreError> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
            RemoteEasyconnectPairedSessionStoreError::Json {
                path: path.to_path_buf(),
                message: error.to_string(),
            }
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(RemoteEasyconnectPairedSessionStoreFile::default())
        }
        Err(error) => Err(RemoteEasyconnectPairedSessionStoreError::Io {
            path: path.to_path_buf(),
            message: error.to_string(),
        }),
    }
}

fn write_store(
    path: &Path,
    store: &RemoteEasyconnectPairedSessionStoreFile,
) -> Result<(), RemoteEasyconnectPairedSessionStoreError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            RemoteEasyconnectPairedSessionStoreError::Io {
                path: parent.to_path_buf(),
                message: error.to_string(),
            }
        })?;
    }
    let encoded = serde_json::to_vec_pretty(store).map_err(|error| {
        RemoteEasyconnectPairedSessionStoreError::Json {
            path: path.to_path_buf(),
            message: error.to_string(),
        }
    })?;
    fs::write(path, encoded).map_err(|error| RemoteEasyconnectPairedSessionStoreError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })
}

fn ensure_session_usable(
    session: &RemoteEasyconnectPairedSessionRecord,
    now_utc: &str,
) -> Result<(), RemoteEasyconnectPairedSessionStoreError> {
    if let Some(revoked_at_utc) = &session.revoked_at_utc {
        return Err(RemoteEasyconnectPairedSessionStoreError::SessionRevoked {
            session_id: session.session_id.clone(),
            revoked_at_utc: revoked_at_utc.clone(),
        });
    }
    if session.expires_at_utc.as_str() <= now_utc {
        return Err(RemoteEasyconnectPairedSessionStoreError::SessionExpired {
            session_id: session.session_id.clone(),
            expires_at_utc: session.expires_at_utc.clone(),
        });
    }
    Ok(())
}

fn require_non_blank(
    field: &'static str,
    value: &str,
) -> Result<(), RemoteEasyconnectPairedSessionStoreError> {
    if value.trim().is_empty() {
        return Err(RemoteEasyconnectPairedSessionStoreError::BlankField { field });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        remote_easyconnect_session_store_path, FileBackedRemoteEasyconnectPairedSessionStore,
        RemoteEasyconnectPairedSessionRecord, RemoteEasyconnectPairedSessionRenewalRequest,
        RemoteEasyconnectPairedSessionStore, RemoteEasyconnectPairedSessionStoreError,
        REMOTE_EASYCONNECT_SESSION_SCHEMA,
    };
    use crate::api::{
        RemoteEasyconnectAuthProvider, RemoteEasyconnectObjectStoreGrant,
        RemoteEasyconnectSessionCredentials,
    };
    use crate::auth::DaemonLocalActor;
    use std::path::PathBuf;

    #[test]
    fn builds_store_path_under_state_dir() {
        assert_eq!(
            remote_easyconnect_session_store_path("/var/lib/dasobjectstore"),
            PathBuf::from("/var/lib/dasobjectstore/remote-easyconnect/sessions.json")
        );
    }

    #[test]
    fn persists_and_reloads_paired_session_records() {
        let root = temp_root("persist");
        let path = remote_easyconnect_session_store_path(&root);
        let store = FileBackedRemoteEasyconnectPairedSessionStore::new(&path);

        store.upsert(session("session-1")).expect("session stored");
        let reloaded = FileBackedRemoteEasyconnectPairedSessionStore::new(&path)
            .get("session-1")
            .expect("session loaded")
            .expect("session exists");

        assert_eq!(reloaded.approved_actor, "stephen");
        assert_eq!(reloaded.object_stores[0].object_store, "zymo_fecal_2025.05");
        let encoded: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).expect("store read"))
                .expect("store decodes");
        assert_eq!(encoded["schema_version"], REMOTE_EASYCONNECT_SESSION_SCHEMA);

        cleanup(&root);
    }

    #[test]
    fn revoke_blocks_later_write_authorization() {
        let root = temp_root("revoke");
        let store = FileBackedRemoteEasyconnectPairedSessionStore::new(
            remote_easyconnect_session_store_path(&root),
        );
        store.upsert(session("session-1")).expect("session stored");

        assert!(store
            .revoke("session-1", "2026-07-09T16:20:00Z")
            .expect("session revoked"));

        let err = store
            .authorize_write(
                "session-1",
                "zymo_fecal_2025.05",
                &actor("stephen", ["mnemosyne"]),
                "2026-07-09T16:21:00Z",
            )
            .expect_err("revoked session rejected");
        assert!(matches!(
            err,
            RemoteEasyconnectPairedSessionStoreError::SessionRevoked { .. }
        ));

        cleanup(&root);
    }

    #[test]
    fn renew_rotates_token_and_extends_expiry_for_active_upload_session() {
        let root = temp_root("renew");
        let store = FileBackedRemoteEasyconnectPairedSessionStore::new(
            remote_easyconnect_session_store_path(&root),
        );
        store.upsert(session("session-1")).expect("session stored");

        let renewed = store
            .renew(RemoteEasyconnectPairedSessionRenewalRequest {
                session_id: "session-1".to_string(),
                renewal_token: "renewal-token-1".to_string(),
                renewed_at_utc: "2026-07-09T20:10:00Z".to_string(),
                expires_at_utc: "2026-07-10T04:10:00Z".to_string(),
                renew_after_utc: "2026-07-10T03:10:00Z".to_string(),
                rotated_renewal_token: "renewal-token-2".to_string(),
            })
            .expect("session renewed");

        assert_eq!(renewed.expires_at_utc, "2026-07-10T04:10:00Z");
        assert_eq!(renewed.renewal_token, "renewal-token-2");
        let stale = store.renew(RemoteEasyconnectPairedSessionRenewalRequest {
            session_id: "session-1".to_string(),
            renewal_token: "renewal-token-1".to_string(),
            renewed_at_utc: "2026-07-09T20:15:00Z".to_string(),
            expires_at_utc: "2026-07-10T04:15:00Z".to_string(),
            renew_after_utc: "2026-07-10T03:15:00Z".to_string(),
            rotated_renewal_token: "renewal-token-3".to_string(),
        });
        assert!(matches!(
            stale.expect_err("stale token rejected"),
            RemoteEasyconnectPairedSessionStoreError::RenewalTokenMismatch { .. }
        ));

        cleanup(&root);
    }

    #[test]
    fn write_authorization_requires_matching_actor_grant_and_unexpired_session() {
        let root = temp_root("authorize");
        let store = FileBackedRemoteEasyconnectPairedSessionStore::new(
            remote_easyconnect_session_store_path(&root),
        );
        store.upsert(session("session-1")).expect("session stored");

        let grant = store
            .authorize_write(
                "session-1",
                "zymo_fecal_2025.05",
                &actor("stephen", ["mnemosyne"]),
                "2026-07-09T16:30:00Z",
            )
            .expect("write authorized");
        assert!(grant.can_write);

        let read_only = store.authorize_write(
            "session-1",
            "ena",
            &actor("stephen", ["mnemosyne"]),
            "2026-07-09T16:30:00Z",
        );
        assert!(matches!(
            read_only.expect_err("read-only grant rejected"),
            RemoteEasyconnectPairedSessionStoreError::ObjectStoreNotWritable { .. }
        ));

        let wrong_actor = store.authorize_write(
            "session-1",
            "zymo_fecal_2025.05",
            &actor("alex", ["mnemosyne"]),
            "2026-07-09T16:30:00Z",
        );
        assert!(matches!(
            wrong_actor.expect_err("actor mismatch rejected"),
            RemoteEasyconnectPairedSessionStoreError::ActorMismatch { .. }
        ));

        let expired = store.authorize_write(
            "session-1",
            "zymo_fecal_2025.05",
            &actor("stephen", ["mnemosyne"]),
            "2026-07-10T00:10:00Z",
        );
        assert!(matches!(
            expired.expect_err("expired session rejected"),
            RemoteEasyconnectPairedSessionStoreError::SessionExpired { .. }
        ));

        cleanup(&root);
    }

    fn session(session_id: &str) -> RemoteEasyconnectPairedSessionRecord {
        RemoteEasyconnectPairedSessionRecord {
            session_id: session_id.to_string(),
            approved_actor: "stephen".to_string(),
            auth_provider: RemoteEasyconnectAuthProvider::StandaloneLocalUser,
            issued_at_utc: "2026-07-09T16:10:00Z".to_string(),
            expires_at_utc: "2026-07-10T00:10:00Z".to_string(),
            renew_after_utc: "2026-07-09T23:10:00Z".to_string(),
            renewal_token: "renewal-token-1".to_string(),
            credentials: RemoteEasyconnectSessionCredentials {
                access_key_id: "AKIAEXAMPLE".to_string(),
                secret_access_key: "secret".to_string(),
                session_token: Some("session-token".to_string()),
            },
            object_stores: vec![
                RemoteEasyconnectObjectStoreGrant {
                    object_store: "zymo_fecal_2025.05".to_string(),
                    bucket: "dos-zymo-fecal-2025-05".to_string(),
                    can_read: true,
                    can_write: true,
                    writer_group: Some("mnemosyne".to_string()),
                    object_type: "fastq".to_string(),
                },
                RemoteEasyconnectObjectStoreGrant {
                    object_store: "ena".to_string(),
                    bucket: "dos-ena".to_string(),
                    can_read: true,
                    can_write: false,
                    writer_group: Some("ena-writers".to_string()),
                    object_type: "fastq".to_string(),
                },
            ],
            revoked_at_utc: None,
        }
    }

    fn actor(username: &str, groups: impl IntoIterator<Item = &'static str>) -> DaemonLocalActor {
        DaemonLocalActor::new(1000)
            .with_username(username)
            .with_groups(groups)
    }

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "dasobjectstore-paired-session-{label}-{}",
            std::process::id()
        ))
    }

    fn cleanup(root: &std::path::Path) {
        let _ = std::fs::remove_dir_all(root);
    }
}
