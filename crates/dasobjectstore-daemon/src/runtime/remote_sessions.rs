use crate::api::{
    RemoteEasyconnectAuthProvider, RemoteEasyconnectControlAuthorization,
    RemoteEasyconnectControlOperation, RemoteEasyconnectObjectStoreGrant,
    RemoteEasyconnectSessionCredentials,
};
use crate::auth::DaemonLocalActor;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(unix)]
use std::{os::unix::fs::OpenOptionsExt, os::unix::fs::PermissionsExt};

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

    fn create_for_pairing(
        &self,
        session: RemoteEasyconnectPairedSessionRecord,
    ) -> Result<(), RemoteEasyconnectPairedSessionStoreError>;

    fn get_by_pairing_id(
        &self,
        pairing_id: &str,
    ) -> Result<
        Option<RemoteEasyconnectPairedSessionRecord>,
        RemoteEasyconnectPairedSessionStoreError,
    >;

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

    fn authorize_completion(
        &self,
        session_id: &str,
        renewal_token: &str,
        object_store: &str,
        now_utc: &str,
    ) -> Result<RemoteEasyconnectObjectStoreGrant, RemoteEasyconnectPairedSessionStoreError>;

    fn active_s3_credentials(
        &self,
        now_utc: &str,
    ) -> Result<Vec<RemoteEasyconnectS3Credential>, RemoteEasyconnectPairedSessionStoreError>;

    fn authorize_control(
        &self,
        access_key_id: &str,
        session_token: &str,
        object_store: &str,
        requested_prefix: &str,
        operation: RemoteEasyconnectControlOperation,
        now_utc: &str,
    ) -> Result<RemoteEasyconnectControlAuthorization, RemoteEasyconnectPairedSessionStoreError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteEasyconnectS3Credential {
    pub session_id: String,
    pub store_id: String,
    pub bucket: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: String,
    pub can_read: bool,
    pub can_write: bool,
    pub expires_at_utc: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectPairedSessionRecord {
    pub pairing_id: String,
    pub session_id: String,
    pub authority_id: String,
    pub principal_id: String,
    /// Compatibility projection of `principal_id` for existing control audit
    /// responses. New code must keep the values identical.
    pub approved_actor: String,
    pub authority_session_id: String,
    pub auth_provider: RemoteEasyconnectAuthProvider,
    pub correlation_id: String,
    pub audit_identity: String,
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
        require_non_blank("pairing_id", &self.pairing_id)?;
        require_non_blank("session_id", &self.session_id)?;
        require_non_blank("authority_id", &self.authority_id)?;
        require_non_blank("principal_id", &self.principal_id)?;
        require_non_blank("approved_actor", &self.approved_actor)?;
        if self.principal_id != self.approved_actor {
            return Err(RemoteEasyconnectPairedSessionStoreError::PrincipalMismatch);
        }
        require_non_blank("authority_session_id", &self.authority_session_id)?;
        require_non_blank("correlation_id", &self.correlation_id)?;
        require_non_blank("audit_identity", &self.audit_identity)?;
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
    pub rotated_credentials: RemoteEasyconnectSessionCredentials,
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

    fn create_for_pairing(
        &self,
        session: RemoteEasyconnectPairedSessionRecord,
    ) -> Result<(), RemoteEasyconnectPairedSessionStoreError> {
        session.validate()?;
        let _guard = self
            .lock
            .lock()
            .expect("paired session store lock poisoned");
        let mut store = read_store(&self.path)?;
        store.create(session)?;
        write_store(&self.path, &store)
    }

    fn get_by_pairing_id(
        &self,
        pairing_id: &str,
    ) -> Result<
        Option<RemoteEasyconnectPairedSessionRecord>,
        RemoteEasyconnectPairedSessionStoreError,
    > {
        require_non_blank("pairing_id", pairing_id)?;
        let _guard = self
            .lock
            .lock()
            .expect("paired session store lock poisoned");
        Ok(read_store(&self.path)?
            .sessions
            .iter()
            .find(|session| session.pairing_id == pairing_id)
            .cloned())
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
        require_non_blank(
            "rotated_credentials.access_key_id",
            &request.rotated_credentials.access_key_id,
        )?;
        require_non_blank(
            "rotated_credentials.secret_access_key",
            &request.rotated_credentials.secret_access_key,
        )?;
        require_non_blank(
            "rotated_credentials.session_token",
            request
                .rotated_credentials
                .session_token
                .as_deref()
                .unwrap_or_default(),
        )?;
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
        if request.renewed_at_utc.as_str() < session.renew_after_utc.as_str() {
            return Err(
                RemoteEasyconnectPairedSessionStoreError::SessionNotRenewable {
                    session_id: request.session_id,
                    renew_after_utc: session.renew_after_utc.clone(),
                },
            );
        }
        session.issued_at_utc = request.renewed_at_utc;
        session.expires_at_utc = request.expires_at_utc;
        session.renew_after_utc = request.renew_after_utc;
        session.renewal_token = request.rotated_renewal_token;
        session.credentials = request.rotated_credentials;
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

    fn authorize_completion(
        &self,
        session_id: &str,
        renewal_token: &str,
        object_store: &str,
        now_utc: &str,
    ) -> Result<RemoteEasyconnectObjectStoreGrant, RemoteEasyconnectPairedSessionStoreError> {
        require_non_blank("session_id", session_id)?;
        require_non_blank("renewal_token", renewal_token)?;
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
        if session.renewal_token != renewal_token {
            return Err(
                RemoteEasyconnectPairedSessionStoreError::RenewalTokenMismatch {
                    session_id: session_id.to_string(),
                },
            );
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

    fn active_s3_credentials(
        &self,
        now_utc: &str,
    ) -> Result<Vec<RemoteEasyconnectS3Credential>, RemoteEasyconnectPairedSessionStoreError> {
        require_non_blank("now_utc", now_utc)?;
        let _guard = self
            .lock
            .lock()
            .expect("paired session store lock poisoned");
        let store = read_store(&self.path)?;
        let mut credentials = Vec::new();
        for session in &store.sessions {
            if session.revoked_at_utc.is_some() || session.expires_at_utc.as_str() <= now_utc {
                continue;
            }
            let Some(session_token) = session.credentials.session_token.clone() else {
                continue;
            };
            for grant in &session.object_stores {
                credentials.push(RemoteEasyconnectS3Credential {
                    session_id: session.session_id.clone(),
                    store_id: grant.object_store.clone(),
                    bucket: grant.bucket.clone(),
                    access_key_id: session.credentials.access_key_id.clone(),
                    secret_access_key: session.credentials.secret_access_key.clone(),
                    session_token: session_token.clone(),
                    can_read: grant.can_read,
                    can_write: grant.can_write,
                    expires_at_utc: session.expires_at_utc.clone(),
                });
            }
        }
        Ok(credentials)
    }

    fn authorize_control(
        &self,
        access_key_id: &str,
        session_token: &str,
        object_store: &str,
        requested_prefix: &str,
        operation: RemoteEasyconnectControlOperation,
        now_utc: &str,
    ) -> Result<RemoteEasyconnectControlAuthorization, RemoteEasyconnectPairedSessionStoreError>
    {
        require_non_blank("access_key_id", access_key_id)?;
        require_non_blank("session_token", session_token)?;
        require_non_blank("object_store", object_store)?;
        require_non_blank("now_utc", now_utc)?;
        validate_control_prefix(requested_prefix)?;
        let _guard = self
            .lock
            .lock()
            .expect("paired session store lock poisoned");
        let store = read_store(&self.path)?;
        let Some(session) = store.sessions.iter().find(|session| {
            secure_eq(&session.credentials.access_key_id, access_key_id)
                && session
                    .credentials
                    .session_token
                    .as_deref()
                    .is_some_and(|expected| secure_eq(expected, session_token))
        }) else {
            return Err(RemoteEasyconnectPairedSessionStoreError::ControlTokenInvalid);
        };
        ensure_session_usable(session, now_utc)?;
        let Some(grant) = session
            .object_stores
            .iter()
            .find(|grant| grant.object_store == object_store)
        else {
            return Err(
                RemoteEasyconnectPairedSessionStoreError::ObjectStoreNotGranted {
                    session_id: session.session_id.clone(),
                    object_store: object_store.to_string(),
                },
            );
        };
        let access_permitted = if operation.requires_write() {
            grant.can_write
        } else {
            grant.can_read
        };
        let operation_permitted = grant.control_operations.contains(&operation);
        let allowed_prefix = grant
            .allowed_prefixes
            .iter()
            .filter(|prefix| requested_prefix.starts_with(prefix.as_str()))
            .max_by_key(|prefix| prefix.len());
        if !access_permitted || !operation_permitted || allowed_prefix.is_none() {
            return Err(
                RemoteEasyconnectPairedSessionStoreError::ControlOperationNotGranted {
                    session_id: session.session_id.clone(),
                    object_store: object_store.to_string(),
                    operation,
                },
            );
        }
        Ok(RemoteEasyconnectControlAuthorization {
            session_id: session.session_id.clone(),
            approved_actor: session.approved_actor.clone(),
            object_store: object_store.to_string(),
            bucket: grant.bucket.clone(),
            can_read: grant.can_read,
            can_write: grant.can_write,
            allowed_prefix: allowed_prefix.cloned().unwrap_or_default(),
            operation,
            expires_at_utc: session.expires_at_utc.clone(),
        })
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

    fn create(
        &mut self,
        session: RemoteEasyconnectPairedSessionRecord,
    ) -> Result<(), RemoteEasyconnectPairedSessionStoreError> {
        if self.sessions.iter().any(|stored| {
            stored.session_id == session.session_id || stored.pairing_id == session.pairing_id
        }) {
            return Err(
                RemoteEasyconnectPairedSessionStoreError::SessionAlreadyExists {
                    session_id: session.session_id,
                },
            );
        }
        self.sessions.push(session);
        Ok(())
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
    SessionAlreadyExists {
        session_id: String,
    },
    PrincipalMismatch,
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
    SessionNotRenewable {
        session_id: String,
        renew_after_utc: String,
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
    ControlTokenInvalid,
    InvalidControlPrefix,
    ControlOperationNotGranted {
        session_id: String,
        object_store: String,
        operation: RemoteEasyconnectControlOperation,
    },
}

impl std::fmt::Display for RemoteEasyconnectPairedSessionStoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BlankField { field } => write!(formatter, "{field} must not be blank"),
            Self::InvalidGrant { message } => {
                write!(formatter, "invalid object store grant: {message}")
            }
            Self::Io { path, message } => {
                write!(formatter, "{} IO failed: {message}", path.display())
            }
            Self::Json { path, message } => {
                write!(formatter, "{} JSON is invalid: {message}", path.display())
            }
            Self::SessionNotFound { session_id } => {
                write!(
                    formatter,
                    "paired easyconnect session {session_id} was not found"
                )
            }
            Self::SessionAlreadyExists { session_id } => {
                write!(formatter, "paired easyconnect session {session_id} already exists")
            }
            Self::PrincipalMismatch => {
                formatter.write_str("paired session principal projections must match")
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
            Self::SessionNotRenewable {
                session_id,
                renew_after_utc,
            } => write!(
                formatter,
                "paired easyconnect session {session_id} is not renewable before {renew_after_utc}"
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
            Self::ControlTokenInvalid => {
                write!(formatter, "remote control token is invalid")
            }
            Self::InvalidControlPrefix => {
                write!(formatter, "remote control prefix is invalid")
            }
            Self::ControlOperationNotGranted {
                session_id,
                object_store,
                operation,
            } => write!(
                formatter,
                "paired easyconnect session {session_id} does not allow {operation:?} for ObjectStore {object_store}"
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
    let parent = path
        .parent()
        .ok_or_else(|| RemoteEasyconnectPairedSessionStoreError::Io {
            path: path.to_path_buf(),
            message: "paired session store has no parent".to_string(),
        })?;
    fs::create_dir_all(parent).map_err(|error| RemoteEasyconnectPairedSessionStoreError::Io {
        path: parent.to_path_buf(),
        message: error.to_string(),
    })?;
    #[cfg(unix)]
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).map_err(|error| {
        RemoteEasyconnectPairedSessionStoreError::Io {
            path: parent.to_path_buf(),
            message: error.to_string(),
        }
    })?;
    let encoded = serde_json::to_vec_pretty(store).map_err(|error| {
        RemoteEasyconnectPairedSessionStoreError::Json {
            path: path.to_path_buf(),
            message: error.to_string(),
        }
    })?;
    let temporary = parent.join(format!(
        ".{}.tmp-{}-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("sessions"),
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file =
        options
            .open(&temporary)
            .map_err(|error| RemoteEasyconnectPairedSessionStoreError::Io {
                path: temporary.clone(),
                message: error.to_string(),
            })?;
    use std::io::Write;
    file.write_all(&encoded)
        .and_then(|_| file.sync_all())
        .map_err(|error| RemoteEasyconnectPairedSessionStoreError::Io {
            path: temporary.clone(),
            message: error.to_string(),
        })?;
    drop(file);
    fs::rename(&temporary, path).map_err(|error| RemoteEasyconnectPairedSessionStoreError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| {
        RemoteEasyconnectPairedSessionStoreError::Io {
            path: path.to_path_buf(),
            message: error.to_string(),
        }
    })?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| RemoteEasyconnectPairedSessionStoreError::Io {
            path: parent.to_path_buf(),
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

fn validate_control_prefix(prefix: &str) -> Result<(), RemoteEasyconnectPairedSessionStoreError> {
    if prefix.starts_with('/')
        || prefix.contains('\\')
        || prefix.chars().any(char::is_control)
        || prefix.split('/').any(|component| component == "..")
    {
        return Err(RemoteEasyconnectPairedSessionStoreError::InvalidControlPrefix);
    }
    Ok(())
}

fn secure_eq(expected: &str, presented: &str) -> bool {
    let expected = expected.as_bytes();
    let presented = presented.as_bytes();
    let mut difference = expected.len() ^ presented.len();
    let length = expected.len().max(presented.len());
    for index in 0..length {
        difference |= usize::from(
            expected.get(index).copied().unwrap_or_default()
                ^ presented.get(index).copied().unwrap_or_default(),
        );
    }
    difference == 0
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
        RemoteEasyconnectAuthProvider, RemoteEasyconnectControlOperation,
        RemoteEasyconnectObjectStoreGrant, RemoteEasyconnectSessionCredentials,
    };
    use crate::auth::DaemonLocalActor;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
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
    fn session_persistence_leaves_only_final_file_after_atomic_write() {
        let root = temp_root("atomic-persist");
        let path = remote_easyconnect_session_store_path(&root);
        let store = FileBackedRemoteEasyconnectPairedSessionStore::new(&path);

        store.upsert(session("session-1")).expect("session stored");
        let entries = std::fs::read_dir(path.parent().expect("parent"))
            .expect("read parent")
            .collect::<Result<Vec<_>, _>>()
            .expect("entries");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].file_name(), "sessions.json");

        cleanup(&root);
    }

    #[test]
    fn gateway_credentials_include_only_active_store_scoped_sessions() {
        let root = temp_root("gateway-credentials");
        let path = remote_easyconnect_session_store_path(&root);
        let store = FileBackedRemoteEasyconnectPairedSessionStore::new(&path);
        store.upsert(session("session-1")).expect("session stored");

        let credentials = store
            .active_s3_credentials("2026-07-09T17:00:00Z")
            .expect("credentials resolved");
        assert_eq!(credentials.len(), 2);
        assert!(credentials.iter().all(|credential| {
            credential.access_key_id == "AKIAEXAMPLE"
                && credential.session_token == "session-token"
                && credential.expires_at_utc == "2026-07-10T00:10:00Z"
        }));
        assert!(store
            .active_s3_credentials("2026-07-10T00:10:00Z")
            .expect("expired credentials filtered")
            .is_empty());

        #[cfg(unix)]
        {
            assert_eq!(
                std::fs::metadata(path.parent().expect("parent"))
                    .expect("parent metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
            assert_eq!(
                std::fs::metadata(&path)
                    .expect("file metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
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
                renewed_at_utc: "2026-07-09T23:10:00Z".to_string(),
                expires_at_utc: "2026-07-10T04:10:00Z".to_string(),
                renew_after_utc: "2026-07-10T03:10:00Z".to_string(),
                rotated_renewal_token: "renewal-token-2".to_string(),
                rotated_credentials: RemoteEasyconnectSessionCredentials {
                    access_key_id: "DOSTROTATED".to_string(),
                    secret_access_key: "rotated-secret".to_string(),
                    session_token: Some("rotated-session-token".to_string()),
                },
            })
            .expect("session renewed");

        assert_eq!(renewed.expires_at_utc, "2026-07-10T04:10:00Z");
        assert_eq!(renewed.renewal_token, "renewal-token-2");
        let stale = store.renew(RemoteEasyconnectPairedSessionRenewalRequest {
            session_id: "session-1".to_string(),
            renewal_token: "renewal-token-1".to_string(),
            renewed_at_utc: "2026-07-09T23:15:00Z".to_string(),
            expires_at_utc: "2026-07-10T04:15:00Z".to_string(),
            renew_after_utc: "2026-07-10T03:15:00Z".to_string(),
            rotated_renewal_token: "renewal-token-3".to_string(),
            rotated_credentials: RemoteEasyconnectSessionCredentials {
                access_key_id: "DOSTROTATED2".to_string(),
                secret_access_key: "rotated-secret-2".to_string(),
                session_token: Some("rotated-session-token-2".to_string()),
            },
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

    #[test]
    fn completion_authorization_requires_the_paired_session_secret_and_write_grant() {
        let root = temp_root("completion-authorize");
        let store = FileBackedRemoteEasyconnectPairedSessionStore::new(
            remote_easyconnect_session_store_path(&root),
        );
        store.upsert(session("session-1")).expect("session stored");

        let grant = store
            .authorize_completion(
                "session-1",
                "renewal-token-1",
                "zymo_fecal_2025.05",
                "2026-07-09T16:30:00Z",
            )
            .expect("completion authorized");
        assert!(grant.can_write);

        let wrong_secret = store.authorize_completion(
            "session-1",
            "wrong-token",
            "zymo_fecal_2025.05",
            "2026-07-09T16:30:00Z",
        );
        assert!(matches!(
            wrong_secret.expect_err("wrong completion secret rejected"),
            RemoteEasyconnectPairedSessionStoreError::RenewalTokenMismatch { .. }
        ));

        let read_only = store.authorize_completion(
            "session-1",
            "renewal-token-1",
            "ena",
            "2026-07-09T16:30:00Z",
        );
        assert!(matches!(
            read_only.expect_err("read-only completion rejected"),
            RemoteEasyconnectPairedSessionStoreError::ObjectStoreNotWritable { .. }
        ));

        cleanup(&root);
    }

    #[test]
    fn control_authorization_uses_rotating_session_token_and_store_capabilities() {
        let root = temp_root("control-authorize");
        let store = FileBackedRemoteEasyconnectPairedSessionStore::new(
            remote_easyconnect_session_store_path(&root),
        );
        store.upsert(session("session-1")).expect("session stored");

        let authorization = store
            .authorize_control(
                "AKIAEXAMPLE",
                "session-token",
                "zymo_fecal_2025.05",
                "raw/PAU59949/",
                RemoteEasyconnectControlOperation::ObjectSnapshot,
                "2026-07-09T16:30:00Z",
            )
            .expect("read authorized");
        assert_eq!(authorization.approved_actor, "stephen");
        assert_eq!(authorization.allowed_prefix, "");

        assert!(matches!(
            store
                .authorize_control(
                    "AKIAEXAMPLE",
                    "session-token",
                    "ena",
                    "",
                    RemoteEasyconnectControlOperation::ReconcileS3,
                    "2026-07-09T16:30:00Z",
                )
                .expect_err("write operation denied"),
            RemoteEasyconnectPairedSessionStoreError::ControlOperationNotGranted { .. }
        ));
        assert!(matches!(
            store
                .authorize_control(
                    "AKIAEXAMPLE",
                    "secret",
                    "zymo_fecal_2025.05",
                    "",
                    RemoteEasyconnectControlOperation::ObjectSnapshot,
                    "2026-07-09T16:30:00Z",
                )
                .expect_err("persistent secret is not a bearer token"),
            RemoteEasyconnectPairedSessionStoreError::ControlTokenInvalid
        ));
        cleanup(&root);
    }

    #[test]
    fn control_authorization_rejects_path_like_prefixes() {
        let root = temp_root("control-prefix");
        let store = FileBackedRemoteEasyconnectPairedSessionStore::new(
            remote_easyconnect_session_store_path(&root),
        );
        store.upsert(session("session-1")).expect("session stored");
        let result = store.authorize_control(
            "AKIAEXAMPLE",
            "session-token",
            "zymo_fecal_2025.05",
            "../other-store",
            RemoteEasyconnectControlOperation::ObjectSnapshot,
            "2026-07-09T16:30:00Z",
        );
        assert!(matches!(
            result.expect_err("unsafe prefix rejected"),
            RemoteEasyconnectPairedSessionStoreError::InvalidControlPrefix
        ));
        cleanup(&root);
    }

    #[test]
    fn legacy_session_without_control_scope_fails_closed() {
        let root = temp_root("legacy-control");
        let path = remote_easyconnect_session_store_path(&root);
        let mut legacy = session("session-1");
        legacy.object_stores[0].control_operations.clear();
        legacy.object_stores[0].allowed_prefixes.clear();
        FileBackedRemoteEasyconnectPairedSessionStore::new(&path)
            .upsert(legacy)
            .expect("legacy-compatible session stored");

        let result = FileBackedRemoteEasyconnectPairedSessionStore::new(&path).authorize_control(
            "AKIAEXAMPLE",
            "session-token",
            "zymo_fecal_2025.05",
            "",
            RemoteEasyconnectControlOperation::StoreReadiness,
            "2026-07-09T16:30:00Z",
        );
        assert!(matches!(
            result.expect_err("missing control scope denied"),
            RemoteEasyconnectPairedSessionStoreError::ControlOperationNotGranted { .. }
        ));
        cleanup(&root);
    }

    #[test]
    fn control_authorization_enforces_the_most_specific_granted_prefix() {
        let root = temp_root("scoped-control-prefix");
        let path = remote_easyconnect_session_store_path(&root);
        let mut scoped = session("session-1");
        scoped.object_stores[0].allowed_prefixes = vec!["EPICv1/".to_string()];
        FileBackedRemoteEasyconnectPairedSessionStore::new(&path)
            .upsert(scoped)
            .expect("session stored");
        let store = FileBackedRemoteEasyconnectPairedSessionStore::new(&path);

        let allowed = store
            .authorize_control(
                "AKIAEXAMPLE",
                "session-token",
                "zymo_fecal_2025.05",
                "EPICv1/GSE171074",
                RemoteEasyconnectControlOperation::ObjectGroupStatus,
                "2026-07-09T16:30:00Z",
            )
            .expect("prefix authorized");
        assert_eq!(allowed.allowed_prefix, "EPICv1/");
        assert!(store
            .authorize_control(
                "AKIAEXAMPLE",
                "session-token",
                "zymo_fecal_2025.05",
                "other/",
                RemoteEasyconnectControlOperation::ObjectGroupStatus,
                "2026-07-09T16:30:00Z",
            )
            .is_err());
        cleanup(&root);
    }

    fn session(session_id: &str) -> RemoteEasyconnectPairedSessionRecord {
        RemoteEasyconnectPairedSessionRecord {
            pairing_id: format!("pairing-{session_id}"),
            session_id: session_id.to_string(),
            authority_id: "authority-1".to_string(),
            principal_id: "stephen".to_string(),
            approved_actor: "stephen".to_string(),
            authority_session_id: "authority-session-1".to_string(),
            auth_provider: RemoteEasyconnectAuthProvider::StandaloneLocalUser,
            correlation_id: "correlation-1".to_string(),
            audit_identity: "local-os:stephen".to_string(),
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
                    control_operations: crate::api::remote_easyconnect_control_operations(true),
                    allowed_prefixes: vec!["".to_string()],
                },
                RemoteEasyconnectObjectStoreGrant {
                    object_store: "ena".to_string(),
                    bucket: "dos-ena".to_string(),
                    can_read: true,
                    can_write: false,
                    writer_group: Some("ena-writers".to_string()),
                    object_type: "fastq".to_string(),
                    control_operations: crate::api::remote_easyconnect_control_operations(false),
                    allowed_prefixes: vec!["".to_string()],
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
