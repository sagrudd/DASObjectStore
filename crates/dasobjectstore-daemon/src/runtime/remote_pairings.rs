use crate::api::{
    RemoteEasyconnectAuthProvider, RemoteEasyconnectObjectStoreGrant,
    RemoteEasyconnectSessionCredentials,
};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub const REMOTE_EASYCONNECT_PAIRING_DIR_NAME: &str = "remote-easyconnect";
pub const REMOTE_EASYCONNECT_PAIRING_FILE_NAME: &str = "pairings.json";
pub const REMOTE_EASYCONNECT_PAIRING_SCHEMA: u16 = 1;

pub fn remote_easyconnect_pairing_store_path(state_dir: impl AsRef<Path>) -> PathBuf {
    state_dir
        .as_ref()
        .join(REMOTE_EASYCONNECT_PAIRING_DIR_NAME)
        .join(REMOTE_EASYCONNECT_PAIRING_FILE_NAME)
}

pub trait RemoteEasyconnectPairingStore: Send + Sync {
    fn upsert(
        &self,
        pairing: RemoteEasyconnectPairingRecord,
    ) -> Result<(), RemoteEasyconnectPairingStoreError>;

    fn approve(
        &self,
        approval: RemoteEasyconnectPairingApproval,
    ) -> Result<RemoteEasyconnectPairingRecord, RemoteEasyconnectPairingStoreError>;

    fn exchange(
        &self,
        request: RemoteEasyconnectPairingExchange,
    ) -> Result<RemoteEasyconnectPairingRecord, RemoteEasyconnectPairingStoreError>;
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectPairingRecord {
    pub pairing_id: String,
    pub client_name: String,
    pub callback_url: String,
    pub requested_object_store: Option<String>,
    pub requested_session_lifetime_seconds: Option<u64>,
    pub client_request_id: Option<String>,
    pub created_at_utc: String,
    pub expires_at_utc: String,
    pub approval: Option<RemoteEasyconnectPairingApproval>,
    pub exchanged_at_utc: Option<String>,
}

impl RemoteEasyconnectPairingRecord {
    pub fn validate(&self) -> Result<(), RemoteEasyconnectPairingStoreError> {
        require_non_blank("pairing_id", &self.pairing_id)?;
        require_non_blank("client_name", &self.client_name)?;
        require_non_blank("callback_url", &self.callback_url)?;
        validate_optional_non_blank(
            "requested_object_store",
            self.requested_object_store.as_deref(),
        )?;
        validate_optional_non_blank("client_request_id", self.client_request_id.as_deref())?;
        require_non_blank("created_at_utc", &self.created_at_utc)?;
        require_non_blank("expires_at_utc", &self.expires_at_utc)?;
        if let Some(approval) = &self.approval {
            approval.validate()?;
            if approval.pairing_id != self.pairing_id {
                return Err(
                    RemoteEasyconnectPairingStoreError::ApprovalPairingMismatch {
                        pairing_id: self.pairing_id.clone(),
                        approval_pairing_id: approval.pairing_id.clone(),
                    },
                );
            }
        }
        validate_optional_non_blank("exchanged_at_utc", self.exchanged_at_utc.as_deref())?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectPairingApproval {
    pub pairing_id: String,
    pub approved_actor: String,
    pub auth_provider: RemoteEasyconnectAuthProvider,
    pub allowed_object_stores: Vec<RemoteEasyconnectObjectStoreGrant>,
    pub approval_expires_at_utc: String,
    pub exchange_code: String,
}

impl RemoteEasyconnectPairingApproval {
    pub fn validate(&self) -> Result<(), RemoteEasyconnectPairingStoreError> {
        require_non_blank("pairing_id", &self.pairing_id)?;
        require_non_blank("approved_actor", &self.approved_actor)?;
        require_non_blank("approval_expires_at_utc", &self.approval_expires_at_utc)?;
        require_non_blank("exchange_code", &self.exchange_code)?;
        if self.allowed_object_stores.is_empty() {
            return Err(RemoteEasyconnectPairingStoreError::BlankField {
                field: "allowed_object_stores",
            });
        }
        for grant in &self.allowed_object_stores {
            grant
                .validate()
                .map_err(|error| RemoteEasyconnectPairingStoreError::InvalidGrant {
                    pairing_id: self.pairing_id.clone(),
                    message: error.to_string(),
                })?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteEasyconnectPairingExchange {
    pub pairing_id: String,
    pub exchange_code: String,
    pub exchanged_at_utc: String,
}

#[derive(Debug)]
pub struct FileBackedRemoteEasyconnectPairingStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl FileBackedRemoteEasyconnectPairingStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            lock: Mutex::new(()),
        }
    }
}

impl RemoteEasyconnectPairingStore for FileBackedRemoteEasyconnectPairingStore {
    fn upsert(
        &self,
        pairing: RemoteEasyconnectPairingRecord,
    ) -> Result<(), RemoteEasyconnectPairingStoreError> {
        pairing.validate()?;
        let _guard = self.lock.lock().expect("pairing store lock poisoned");
        let mut store = read_store(&self.path)?;
        store.upsert(pairing);
        write_store(&self.path, &store)
    }

    fn approve(
        &self,
        approval: RemoteEasyconnectPairingApproval,
    ) -> Result<RemoteEasyconnectPairingRecord, RemoteEasyconnectPairingStoreError> {
        approval.validate()?;
        let _guard = self.lock.lock().expect("pairing store lock poisoned");
        let mut store = read_store(&self.path)?;
        let Some(pairing) = store.pairing_mut(&approval.pairing_id) else {
            return Err(RemoteEasyconnectPairingStoreError::PairingNotFound {
                pairing_id: approval.pairing_id,
            });
        };
        pairing.approval = Some(approval);
        let approved = pairing.clone();
        write_store(&self.path, &store)?;
        Ok(approved)
    }

    fn exchange(
        &self,
        request: RemoteEasyconnectPairingExchange,
    ) -> Result<RemoteEasyconnectPairingRecord, RemoteEasyconnectPairingStoreError> {
        require_non_blank("pairing_id", &request.pairing_id)?;
        require_non_blank("exchange_code", &request.exchange_code)?;
        require_non_blank("exchanged_at_utc", &request.exchanged_at_utc)?;
        let _guard = self.lock.lock().expect("pairing store lock poisoned");
        let mut store = read_store(&self.path)?;
        let Some(pairing) = store.pairing_mut(&request.pairing_id) else {
            return Err(RemoteEasyconnectPairingStoreError::PairingNotFound {
                pairing_id: request.pairing_id,
            });
        };
        ensure_pairing_usable(pairing, &request.exchanged_at_utc)?;
        let Some(approval) = &pairing.approval else {
            return Err(RemoteEasyconnectPairingStoreError::PairingNotApproved {
                pairing_id: request.pairing_id,
            });
        };
        if approval.exchange_code != request.exchange_code {
            return Err(RemoteEasyconnectPairingStoreError::ExchangeCodeMismatch {
                pairing_id: request.pairing_id,
            });
        }
        if approval.approval_expires_at_utc <= request.exchanged_at_utc {
            return Err(RemoteEasyconnectPairingStoreError::ApprovalExpired {
                pairing_id: pairing.pairing_id.clone(),
                expired_at_utc: approval.approval_expires_at_utc.clone(),
            });
        }
        pairing.exchanged_at_utc = Some(request.exchanged_at_utc);
        let exchanged = pairing.clone();
        write_store(&self.path, &store)?;
        Ok(exchanged)
    }
}

#[derive(Debug)]
pub enum RemoteEasyconnectPairingStoreError {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    Json {
        path: PathBuf,
        message: String,
    },
    BlankField {
        field: &'static str,
    },
    InvalidGrant {
        pairing_id: String,
        message: String,
    },
    ApprovalPairingMismatch {
        pairing_id: String,
        approval_pairing_id: String,
    },
    PairingNotFound {
        pairing_id: String,
    },
    PairingNotApproved {
        pairing_id: String,
    },
    PairingExpired {
        pairing_id: String,
        expired_at_utc: String,
    },
    ApprovalExpired {
        pairing_id: String,
        expired_at_utc: String,
    },
    PairingAlreadyExchanged {
        pairing_id: String,
        exchanged_at_utc: String,
    },
    ExchangeCodeMismatch {
        pairing_id: String,
    },
}

impl std::fmt::Display for RemoteEasyconnectPairingStoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(formatter, "remote easyconnect pairing store IO failed at {}: {source}", path.display())
            }
            Self::Json { path, message } => {
                write!(formatter, "remote easyconnect pairing store JSON failed at {}: {message}", path.display())
            }
            Self::BlankField { field } => write!(formatter, "{field} must not be blank"),
            Self::InvalidGrant {
                pairing_id,
                message,
            } => write!(formatter, "pairing {pairing_id} has invalid object store grant: {message}"),
            Self::ApprovalPairingMismatch {
                pairing_id,
                approval_pairing_id,
            } => write!(
                formatter,
                "pairing {pairing_id} cannot store approval for {approval_pairing_id}"
            ),
            Self::PairingNotFound { pairing_id } => {
                write!(formatter, "remote easyconnect pairing {pairing_id} was not found")
            }
            Self::PairingNotApproved { pairing_id } => {
                write!(formatter, "remote easyconnect pairing {pairing_id} has not been approved")
            }
            Self::PairingExpired {
                pairing_id,
                expired_at_utc,
            } => write!(
                formatter,
                "remote easyconnect pairing {pairing_id} expired at {expired_at_utc}"
            ),
            Self::ApprovalExpired {
                pairing_id,
                expired_at_utc,
            } => write!(
                formatter,
                "remote easyconnect pairing {pairing_id} approval expired at {expired_at_utc}"
            ),
            Self::PairingAlreadyExchanged {
                pairing_id,
                exchanged_at_utc,
            } => write!(
                formatter,
                "remote easyconnect pairing {pairing_id} was already exchanged at {exchanged_at_utc}"
            ),
            Self::ExchangeCodeMismatch { pairing_id } => write!(
                formatter,
                "remote easyconnect pairing {pairing_id} exchange code did not match"
            ),
        }
    }
}

impl std::error::Error for RemoteEasyconnectPairingStoreError {}

#[derive(Debug, Deserialize, Serialize)]
struct RemoteEasyconnectPairingStoreFile {
    schema_version: u16,
    pairings: Vec<RemoteEasyconnectPairingRecord>,
}

impl Default for RemoteEasyconnectPairingStoreFile {
    fn default() -> Self {
        Self {
            schema_version: REMOTE_EASYCONNECT_PAIRING_SCHEMA,
            pairings: Vec::new(),
        }
    }
}

impl RemoteEasyconnectPairingStoreFile {
    fn pairing_mut(&mut self, pairing_id: &str) -> Option<&mut RemoteEasyconnectPairingRecord> {
        self.pairings
            .iter_mut()
            .find(|pairing| pairing.pairing_id == pairing_id)
    }

    fn upsert(&mut self, pairing: RemoteEasyconnectPairingRecord) {
        if let Some(index) = self
            .pairings
            .iter()
            .position(|stored| stored.pairing_id == pairing.pairing_id)
        {
            self.pairings[index] = pairing;
        } else {
            self.pairings.push(pairing);
        }
    }
}

fn read_store(
    path: &Path,
) -> Result<RemoteEasyconnectPairingStoreFile, RemoteEasyconnectPairingStoreError> {
    match fs::read_to_string(path) {
        Ok(raw) => {
            serde_json::from_str(&raw).map_err(|error| RemoteEasyconnectPairingStoreError::Json {
                path: path.to_path_buf(),
                message: error.to_string(),
            })
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Ok(RemoteEasyconnectPairingStoreFile::default())
        }
        Err(source) => Err(RemoteEasyconnectPairingStoreError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn write_store(
    path: &Path,
    store: &RemoteEasyconnectPairingStoreFile,
) -> Result<(), RemoteEasyconnectPairingStoreError> {
    let parent = path
        .parent()
        .ok_or_else(|| RemoteEasyconnectPairingStoreError::Io {
            path: path.to_path_buf(),
            source: io::Error::new(io::ErrorKind::InvalidInput, "pairing store has no parent"),
        })?;
    fs::create_dir_all(parent).map_err(|source| RemoteEasyconnectPairingStoreError::Io {
        path: parent.to_path_buf(),
        source,
    })?;
    let encoded = serde_json::to_vec_pretty(store).map_err(|error| {
        RemoteEasyconnectPairingStoreError::Json {
            path: path.to_path_buf(),
            message: error.to_string(),
        }
    })?;
    let temporary = parent.join(format!(
        ".{}.tmp-{}-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("pairings"),
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|source| RemoteEasyconnectPairingStoreError::Io {
            path: temporary.clone(),
            source,
        })?;
    file.write_all(&encoded)
        .and_then(|_| file.sync_all())
        .map_err(|source| RemoteEasyconnectPairingStoreError::Io {
            path: temporary.clone(),
            source,
        })?;
    drop(file);
    fs::rename(&temporary, path).map_err(|source| RemoteEasyconnectPairingStoreError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| RemoteEasyconnectPairingStoreError::Io {
            path: parent.to_path_buf(),
            source,
        })
}

fn ensure_pairing_usable(
    pairing: &RemoteEasyconnectPairingRecord,
    now_utc: &str,
) -> Result<(), RemoteEasyconnectPairingStoreError> {
    if pairing.expires_at_utc.as_str() <= now_utc {
        return Err(RemoteEasyconnectPairingStoreError::PairingExpired {
            pairing_id: pairing.pairing_id.clone(),
            expired_at_utc: pairing.expires_at_utc.clone(),
        });
    }
    if let Some(exchanged_at_utc) = &pairing.exchanged_at_utc {
        return Err(
            RemoteEasyconnectPairingStoreError::PairingAlreadyExchanged {
                pairing_id: pairing.pairing_id.clone(),
                exchanged_at_utc: exchanged_at_utc.clone(),
            },
        );
    }
    Ok(())
}

fn require_non_blank(
    field: &'static str,
    value: &str,
) -> Result<(), RemoteEasyconnectPairingStoreError> {
    if value.trim().is_empty() {
        return Err(RemoteEasyconnectPairingStoreError::BlankField { field });
    }
    Ok(())
}

fn validate_optional_non_blank(
    field: &'static str,
    value: Option<&str>,
) -> Result<(), RemoteEasyconnectPairingStoreError> {
    if value.is_some_and(|value| value.trim().is_empty()) {
        return Err(RemoteEasyconnectPairingStoreError::BlankField { field });
    }
    Ok(())
}

pub fn session_credentials_from_store_credentials(
    credential: dasobjectstore_object_service::StoreServiceCredential,
) -> RemoteEasyconnectSessionCredentials {
    RemoteEasyconnectSessionCredentials {
        access_key_id: credential.access_key_id,
        secret_access_key: credential.secret_access_key.expose_secret().to_string(),
        session_token: None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        write_store, RemoteEasyconnectPairingStoreFile, REMOTE_EASYCONNECT_PAIRING_SCHEMA,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn root() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let root = std::env::var_os("DASOBJECTSTORE_CODEX_VALIDATION_ROOT")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|home| PathBuf::from(home).join(".dasobjectstore-codex-validation"))
            })
            .unwrap_or_else(std::env::temp_dir)
            .join(format!(
                "remote-pairings-persistence-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(&root).expect("fixture root");
        root
    }

    #[test]
    fn pairing_store_persistence_uses_atomic_final_path_without_temp_files() {
        let root = root();
        let path = root.join("nested/pairings.json");
        let store = RemoteEasyconnectPairingStoreFile {
            schema_version: REMOTE_EASYCONNECT_PAIRING_SCHEMA,
            pairings: Vec::new(),
        };
        write_store(&path, &store).expect("persist pairing store");
        assert!(path.is_file());
        let entries = fs::read_dir(path.parent().expect("parent"))
            .expect("read parent")
            .collect::<Result<Vec<_>, _>>()
            .expect("entries");
        assert_eq!(entries.len(), 1);
        fs::remove_dir_all(root).expect("cleanup");
    }
}
