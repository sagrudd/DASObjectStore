use crate::api::{
    RemoteEasyconnectApprovalContext, RemoteEasyconnectPairingState,
    RemoteEasyconnectPairingStatusResponse, RemoteEasyconnectSessionCredentials,
};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(unix)]
use std::{os::unix::fs::OpenOptionsExt, os::unix::fs::PermissionsExt};

pub const REMOTE_EASYCONNECT_PAIRING_DIR_NAME: &str = "remote-easyconnect";
pub const REMOTE_EASYCONNECT_PAIRING_FILE_NAME: &str = "pairings.json";
pub const REMOTE_EASYCONNECT_PAIRING_SCHEMA: u16 = 1;
pub const REMOTE_EASYCONNECT_PAIRING_TTL_SECONDS: u64 = 5 * 60;
pub const REMOTE_EASYCONNECT_APPROVAL_TTL_SECONDS: u64 = 2 * 60;
/// Maximum number of pending or approved pairing capabilities retained at once.
pub const REMOTE_EASYCONNECT_MAX_LIVE_PAIRINGS: usize = 1_024;

pub fn remote_easyconnect_pairing_store_path(state_dir: impl AsRef<Path>) -> PathBuf {
    state_dir
        .as_ref()
        .join(REMOTE_EASYCONNECT_PAIRING_DIR_NAME)
        .join(REMOTE_EASYCONNECT_PAIRING_FILE_NAME)
}

pub trait RemoteEasyconnectPairingStore: Send + Sync {
    fn create(
        &self,
        pairing: RemoteEasyconnectPairingRecord,
    ) -> Result<(), RemoteEasyconnectPairingStoreError>;

    fn approve(
        &self,
        approval: RemoteEasyconnectPairingApproval,
        approved_at_utc: &str,
    ) -> Result<RemoteEasyconnectPairingRecord, RemoteEasyconnectPairingStoreError>;

    fn status(
        &self,
        pairing_id: &str,
        now_utc: &str,
    ) -> Result<RemoteEasyconnectPairingStatusResponse, RemoteEasyconnectPairingStoreError>;

    fn status_by_browser_handoff(
        &self,
        browser_handoff_reference: &str,
        now_utc: &str,
    ) -> Result<RemoteEasyconnectPairingStatusResponse, RemoteEasyconnectPairingStoreError>;

    fn prepare_exchange(
        &self,
        request: &RemoteEasyconnectPairingExchange,
    ) -> Result<RemoteEasyconnectPairingRecord, RemoteEasyconnectPairingStoreError>;

    fn commit_exchange(
        &self,
        request: RemoteEasyconnectPairingExchange,
    ) -> Result<RemoteEasyconnectPairingRecord, RemoteEasyconnectPairingStoreError>;
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteEasyconnectPairingRecord {
    pub pairing_id: String,
    /// Random, opaque, one-purpose reference permitted in a browser URL.
    /// Empty only on expired pre-handoff records retained for client polling.
    #[serde(default)]
    pub browser_handoff_reference: String,
    pub client_name: String,
    #[serde(default)]
    pub callback_url: Option<String>,
    #[serde(default)]
    pub completion_mode: crate::api::RemoteEasyconnectCompletionMode,
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
        match (self.completion_mode, self.callback_url.as_deref()) {
            (crate::api::RemoteEasyconnectCompletionMode::Callback, Some(callback_url)) => {
                require_non_blank("callback_url", callback_url)?;
            }
            (crate::api::RemoteEasyconnectCompletionMode::Callback, None) => {
                return Err(RemoteEasyconnectPairingStoreError::InvalidRecord {
                    pairing_id: self.pairing_id.clone(),
                    message: "callback completion requires a callback URL".to_string(),
                });
            }
            (crate::api::RemoteEasyconnectCompletionMode::Polling, None) => {}
            (crate::api::RemoteEasyconnectCompletionMode::Polling, Some(_)) => {
                return Err(RemoteEasyconnectPairingStoreError::InvalidRecord {
                    pairing_id: self.pairing_id.clone(),
                    message: "polling completion must not retain a callback URL".to_string(),
                });
            }
        }
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
    pub context: RemoteEasyconnectApprovalContext,
    pub approval_expires_at_utc: String,
    pub exchange_code: String,
}

impl RemoteEasyconnectPairingApproval {
    pub fn validate(&self) -> Result<(), RemoteEasyconnectPairingStoreError> {
        require_non_blank("pairing_id", &self.pairing_id)?;
        require_non_blank("approval_expires_at_utc", &self.approval_expires_at_utc)?;
        require_non_blank("exchange_code", &self.exchange_code)?;
        self.context.validate().map_err(|error| {
            RemoteEasyconnectPairingStoreError::InvalidApprovalContext {
                pairing_id: self.pairing_id.clone(),
                message: error.to_string(),
            }
        })?;
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
    max_live_pairings: usize,
}

impl FileBackedRemoteEasyconnectPairingStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            lock: Mutex::new(()),
            max_live_pairings: REMOTE_EASYCONNECT_MAX_LIVE_PAIRINGS,
        }
    }

    #[cfg(test)]
    fn with_max_live_pairings(path: impl Into<PathBuf>, max_live_pairings: usize) -> Self {
        Self {
            path: path.into(),
            lock: Mutex::new(()),
            max_live_pairings,
        }
    }
}

fn pairing_status(
    pairing: &RemoteEasyconnectPairingRecord,
    now_utc: &str,
) -> RemoteEasyconnectPairingStatusResponse {
    let approval_expired = pairing
        .approval
        .as_ref()
        .is_some_and(|approval| approval.approval_expires_at_utc.as_str() <= now_utc);
    let state = if pairing.exchanged_at_utc.is_some() {
        RemoteEasyconnectPairingState::Exchanged
    } else if pairing.expires_at_utc.as_str() <= now_utc || approval_expired {
        RemoteEasyconnectPairingState::Expired
    } else if pairing.approval.is_some() {
        RemoteEasyconnectPairingState::Approved
    } else {
        RemoteEasyconnectPairingState::Pending
    };
    RemoteEasyconnectPairingStatusResponse {
        pairing_id: pairing.pairing_id.clone(),
        state,
        expires_at_utc: pairing.expires_at_utc.clone(),
        requested_object_store: pairing.requested_object_store.clone(),
        completion_mode: pairing.completion_mode,
        exchange_code: if state == RemoteEasyconnectPairingState::Approved {
            pairing
                .approval
                .as_ref()
                .map(|approval| approval.exchange_code.clone())
        } else {
            None
        },
    }
}

impl RemoteEasyconnectPairingStore for FileBackedRemoteEasyconnectPairingStore {
    fn create(
        &self,
        pairing: RemoteEasyconnectPairingRecord,
    ) -> Result<(), RemoteEasyconnectPairingStoreError> {
        pairing.validate()?;
        let _guard = self.lock.lock().expect("pairing store lock poisoned");
        let mut store = read_store(&self.path)?;
        store.create(pairing, self.max_live_pairings)?;
        write_store(&self.path, &store)
    }

    fn approve(
        &self,
        mut approval: RemoteEasyconnectPairingApproval,
        approved_at_utc: &str,
    ) -> Result<RemoteEasyconnectPairingRecord, RemoteEasyconnectPairingStoreError> {
        approval.validate()?;
        require_non_blank("approved_at_utc", approved_at_utc)?;
        let _guard = self.lock.lock().expect("pairing store lock poisoned");
        let mut store = read_store(&self.path)?;
        let Some(pairing) = store.pairing_mut(&approval.pairing_id) else {
            return Err(RemoteEasyconnectPairingStoreError::PairingNotFound {
                pairing_id: approval.pairing_id,
            });
        };
        ensure_pairing_usable(pairing, approved_at_utc)?;
        if approval.context.host_session_expires_at_utc.as_str() <= approved_at_utc {
            return Err(RemoteEasyconnectPairingStoreError::ApprovalExpired {
                pairing_id: pairing.pairing_id.clone(),
                expired_at_utc: approval.context.host_session_expires_at_utc.clone(),
            });
        }
        approval.approval_expires_at_utc = approval
            .approval_expires_at_utc
            .min(pairing.expires_at_utc.clone())
            .min(approval.context.host_session_expires_at_utc.clone());
        if let Some(requested) = pairing.requested_object_store.as_deref() {
            if approval.context.allowed_object_stores.len() != 1
                || approval.context.allowed_object_stores[0].object_store != requested
            {
                return Err(
                    RemoteEasyconnectPairingStoreError::RequestedObjectStoreMismatch {
                        pairing_id: pairing.pairing_id.clone(),
                        requested: requested.to_string(),
                    },
                );
            }
        }
        pairing.approval = Some(approval);
        let approved = pairing.clone();
        write_store(&self.path, &store)?;
        Ok(approved)
    }

    fn status(
        &self,
        pairing_id: &str,
        now_utc: &str,
    ) -> Result<RemoteEasyconnectPairingStatusResponse, RemoteEasyconnectPairingStoreError> {
        require_non_blank("pairing_id", pairing_id)?;
        require_non_blank("now_utc", now_utc)?;
        let _guard = self.lock.lock().expect("pairing store lock poisoned");
        let store = read_store(&self.path)?;
        let Some(pairing) = store
            .pairings
            .iter()
            .find(|pairing| pairing.pairing_id == pairing_id)
        else {
            return Err(RemoteEasyconnectPairingStoreError::PairingNotFound {
                pairing_id: pairing_id.to_string(),
            });
        };
        Ok(pairing_status(pairing, now_utc))
    }

    fn status_by_browser_handoff(
        &self,
        browser_handoff_reference: &str,
        now_utc: &str,
    ) -> Result<RemoteEasyconnectPairingStatusResponse, RemoteEasyconnectPairingStoreError> {
        require_non_blank("browser_handoff_reference", browser_handoff_reference)?;
        require_non_blank("now_utc", now_utc)?;
        let _guard = self.lock.lock().expect("pairing store lock poisoned");
        let store = read_store(&self.path)?;
        let pairing = store
            .pairings
            .iter()
            .find(|pairing| pairing.browser_handoff_reference == browser_handoff_reference)
            .ok_or(RemoteEasyconnectPairingStoreError::BrowserHandoffUnavailable)?;
        let status = pairing_status(pairing, now_utc);
        if status.state != RemoteEasyconnectPairingState::Pending {
            return Err(RemoteEasyconnectPairingStoreError::BrowserHandoffUnavailable);
        }
        Ok(status)
    }

    fn prepare_exchange(
        &self,
        request: &RemoteEasyconnectPairingExchange,
    ) -> Result<RemoteEasyconnectPairingRecord, RemoteEasyconnectPairingStoreError> {
        validate_exchange_request(request)?;
        let _guard = self.lock.lock().expect("pairing store lock poisoned");
        let store = read_store(&self.path)?;
        let Some(pairing) = store
            .pairings
            .iter()
            .find(|pairing| pairing.pairing_id == request.pairing_id)
        else {
            return Err(RemoteEasyconnectPairingStoreError::PairingNotFound {
                pairing_id: request.pairing_id.clone(),
            });
        };
        validate_exchange(pairing, request)?;
        Ok(pairing.clone())
    }

    fn commit_exchange(
        &self,
        request: RemoteEasyconnectPairingExchange,
    ) -> Result<RemoteEasyconnectPairingRecord, RemoteEasyconnectPairingStoreError> {
        validate_exchange_request(&request)?;
        let _guard = self.lock.lock().expect("pairing store lock poisoned");
        let mut store = read_store(&self.path)?;
        let Some(pairing) = store.pairing_mut(&request.pairing_id) else {
            return Err(RemoteEasyconnectPairingStoreError::PairingNotFound {
                pairing_id: request.pairing_id,
            });
        };
        validate_exchange(pairing, &request)?;
        pairing.exchanged_at_utc = Some(request.exchanged_at_utc);
        let exchanged = pairing.clone();
        write_store(&self.path, &store)?;
        Ok(exchanged)
    }
}

fn validate_exchange_request(
    request: &RemoteEasyconnectPairingExchange,
) -> Result<(), RemoteEasyconnectPairingStoreError> {
    require_non_blank("pairing_id", &request.pairing_id)?;
    require_non_blank("exchange_code", &request.exchange_code)?;
    require_non_blank("exchanged_at_utc", &request.exchanged_at_utc)
}

fn validate_exchange(
    pairing: &RemoteEasyconnectPairingRecord,
    request: &RemoteEasyconnectPairingExchange,
) -> Result<(), RemoteEasyconnectPairingStoreError> {
    ensure_pairing_usable(pairing, &request.exchanged_at_utc)?;
    let Some(approval) = &pairing.approval else {
        return Err(RemoteEasyconnectPairingStoreError::PairingNotApproved {
            pairing_id: request.pairing_id.clone(),
        });
    };
    if approval.exchange_code != request.exchange_code {
        return Err(RemoteEasyconnectPairingStoreError::ExchangeCodeMismatch {
            pairing_id: request.pairing_id.clone(),
        });
    }
    if approval.approval_expires_at_utc <= request.exchanged_at_utc {
        return Err(RemoteEasyconnectPairingStoreError::ApprovalExpired {
            pairing_id: pairing.pairing_id.clone(),
            expired_at_utc: approval.approval_expires_at_utc.clone(),
        });
    }
    Ok(())
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
    InvalidRecord {
        pairing_id: String,
        message: String,
    },
    InvalidApprovalContext {
        pairing_id: String,
        message: String,
    },
    PairingAlreadyExists {
        pairing_id: String,
    },
    CapacityExceeded {
        max_live_pairings: usize,
    },
    ApprovalPairingMismatch {
        pairing_id: String,
        approval_pairing_id: String,
    },
    PairingNotFound {
        pairing_id: String,
    },
    /// Deliberately contains no handoff reference: browser-visible failures
    /// must not disclose opaque URL capabilities.
    BrowserHandoffUnavailable,
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
    RequestedObjectStoreMismatch {
        pairing_id: String,
        requested: String,
    },
}

impl std::fmt::Display for RemoteEasyconnectPairingStoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(
                    formatter,
                    "remote easyconnect pairing store IO failed at {}: {source}",
                    path.display()
                )
            }
            Self::Json { path, message } => {
                write!(
                    formatter,
                    "remote easyconnect pairing store JSON failed at {}: {message}",
                    path.display()
                )
            }
            Self::BlankField { field } => write!(formatter, "{field} must not be blank"),
            Self::InvalidGrant {
                pairing_id,
                message,
            } => write!(
                formatter,
                "pairing {pairing_id} has invalid object store grant: {message}"
            ),
            Self::InvalidRecord {
                pairing_id,
                message,
            } => write!(formatter, "pairing {pairing_id} is invalid: {message}"),
            Self::InvalidApprovalContext {
                pairing_id,
                message,
            } => write!(
                formatter,
                "pairing {pairing_id} has invalid approval context: {message}"
            ),
            Self::PairingAlreadyExists { pairing_id } => {
                write!(formatter, "remote easyconnect pairing {pairing_id} already exists")
            }
            Self::CapacityExceeded { max_live_pairings } => write!(
                formatter,
                "remote easyconnect pairing store admits at most {max_live_pairings} live pairings"
            ),
            Self::ApprovalPairingMismatch {
                pairing_id,
                approval_pairing_id,
            } => write!(
                formatter,
                "pairing {pairing_id} cannot store approval for {approval_pairing_id}"
            ),
            Self::PairingNotFound { pairing_id } => {
                write!(
                    formatter,
                    "remote easyconnect pairing {pairing_id} was not found"
                )
            }
            Self::BrowserHandoffUnavailable => {
                formatter.write_str("remote easyconnect browser handoff is unavailable")
            }
            Self::PairingNotApproved { pairing_id } => {
                write!(
                    formatter,
                    "remote easyconnect pairing {pairing_id} has not been approved"
                )
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
            Self::RequestedObjectStoreMismatch {
                pairing_id,
                requested,
            } => write!(
                formatter,
                "remote easyconnect pairing {pairing_id} may approve only requested ObjectStore {requested}"
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

    fn create(
        &mut self,
        pairing: RemoteEasyconnectPairingRecord,
        max_live_pairings: usize,
    ) -> Result<(), RemoteEasyconnectPairingStoreError> {
        if self
            .pairings
            .iter()
            .any(|stored| stored.pairing_id == pairing.pairing_id)
        {
            return Err(RemoteEasyconnectPairingStoreError::PairingAlreadyExists {
                pairing_id: pairing.pairing_id,
            });
        }
        self.prune_terminal(&pairing.created_at_utc);
        if let Some(request_id) = pairing.client_request_id.as_deref() {
            self.pairings.retain(|stored| {
                stored.client_request_id.as_deref() != Some(request_id)
                    || stored.approval.is_some()
                    || stored.exchanged_at_utc.is_some()
            });
        }
        if self.pairings.len() >= max_live_pairings {
            return Err(RemoteEasyconnectPairingStoreError::CapacityExceeded { max_live_pairings });
        }
        self.pairings.push(pairing);
        Ok(())
    }

    fn prune_terminal(&mut self, now_utc: &str) {
        self.pairings.retain(|pairing| {
            pairing.exchanged_at_utc.is_none()
                && pairing.expires_at_utc.as_str() > now_utc
                && pairing
                    .approval
                    .as_ref()
                    .is_none_or(|approval| approval.approval_expires_at_utc.as_str() > now_utc)
        });
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
    #[cfg(unix)]
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).map_err(|source| {
        RemoteEasyconnectPairingStoreError::Io {
            path: parent.to_path_buf(),
            source,
        }
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
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file =
        options
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
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|source| {
        RemoteEasyconnectPairingStoreError::Io {
            path: path.to_path_buf(),
            source,
        }
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
        read_store, write_store, FileBackedRemoteEasyconnectPairingStore,
        RemoteEasyconnectPairingApproval, RemoteEasyconnectPairingRecord,
        RemoteEasyconnectPairingStore, RemoteEasyconnectPairingStoreError,
        RemoteEasyconnectPairingStoreFile, REMOTE_EASYCONNECT_PAIRING_SCHEMA,
    };
    use crate::api::{
        remote_easyconnect_control_operations, RemoteEasyconnectApprovalContext,
        RemoteEasyconnectAuthProvider, RemoteEasyconnectObjectStoreGrant,
        REMOTE_EASYCONNECT_DEFAULT_CONTROL_PREFIX,
    };
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
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

    #[test]
    fn approval_cannot_substitute_the_requested_object_store() {
        let root = root();
        let store = FileBackedRemoteEasyconnectPairingStore::new(root.join("pairings.json"));
        store
            .create(RemoteEasyconnectPairingRecord {
                pairing_id: "pairing-1".to_string(),
                browser_handoff_reference: "handoff-test-1".to_string(),
                client_name: "remote CLI".to_string(),
                callback_url: Some(
                    "http://127.0.0.1:49152/products/dasobjectstore/remote/easyconnect/callback"
                        .to_string(),
                ),
                completion_mode: crate::api::RemoteEasyconnectCompletionMode::Callback,
                requested_object_store: Some("requested-store".to_string()),
                requested_session_lifetime_seconds: None,
                client_request_id: Some("request-1".to_string()),
                created_at_utc: "2026-07-28T10:00:00Z".to_string(),
                expires_at_utc: "2026-07-28T10:05:00Z".to_string(),
                approval: None,
                exchanged_at_utc: None,
            })
            .expect("pairing is stored");
        let error = store
            .approve(
                RemoteEasyconnectPairingApproval {
                    pairing_id: "pairing-1".to_string(),
                    context: RemoteEasyconnectApprovalContext {
                        authority_id: "authority-1".to_string(),
                        principal_id: "principal-1".to_string(),
                        session_id: "session-1".to_string(),
                        auth_provider: RemoteEasyconnectAuthProvider::Pistis,
                        allowed_object_stores: vec![RemoteEasyconnectObjectStoreGrant {
                            object_store: "different-store".to_string(),
                            bucket: "bucket".to_string(),
                            can_read: true,
                            can_write: true,
                            writer_group: Some("writers".to_string()),
                            object_type: "store_scoped_session".to_string(),
                            control_operations: remote_easyconnect_control_operations(true),
                            allowed_prefixes: vec![
                                REMOTE_EASYCONNECT_DEFAULT_CONTROL_PREFIX.to_string()
                            ],
                        }],
                        host_session_expires_at_utc: "2026-07-28T10:04:00Z".to_string(),
                        correlation_id: "correlation-1".to_string(),
                        audit_identity: "pistis:principal-1".to_string(),
                    },
                    approval_expires_at_utc: "2026-07-28T10:04:00Z".to_string(),
                    exchange_code: "exchange-secret".to_string(),
                },
                "2026-07-28T10:01:00Z",
            )
            .expect_err("store substitution must fail");
        assert!(matches!(
            error,
            RemoteEasyconnectPairingStoreError::RequestedObjectStoreMismatch { .. }
        ));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn collision_safe_create_never_overwrites_an_existing_pairing() {
        let root = root();
        let store = FileBackedRemoteEasyconnectPairingStore::new(root.join("pairings.json"));
        let pairing = pairing("pairing-random-capability");
        store
            .create(pairing.clone())
            .expect("first create succeeds");
        assert!(matches!(
            store.create(pairing).expect_err("collision must fail"),
            RemoteEasyconnectPairingStoreError::PairingAlreadyExists { .. }
        ));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn capacity_rejects_new_live_pairings_after_restart() {
        let root = root();
        let path = root.join("pairings.json");
        FileBackedRemoteEasyconnectPairingStore::with_max_live_pairings(&path, 2)
            .create(pairing("pairing-1"))
            .expect("first pairing");
        FileBackedRemoteEasyconnectPairingStore::with_max_live_pairings(&path, 2)
            .create(pairing("pairing-2"))
            .expect("second pairing after restart");

        let error = FileBackedRemoteEasyconnectPairingStore::with_max_live_pairings(&path, 2)
            .create(pairing("pairing-3"))
            .expect_err("third live pairing must be rejected");
        assert!(matches!(
            error,
            RemoteEasyconnectPairingStoreError::CapacityExceeded {
                max_live_pairings: 2
            }
        ));
        assert_eq!(read_store(&path).expect("store").pairings.len(), 2);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn admission_prunes_terminal_and_expired_rows_after_restart() {
        let root = root();
        let path = root.join("pairings.json");
        let mut expired = pairing("expired");
        expired.expires_at_utc = "2026-07-28T09:59:59Z".to_string();
        let mut exchanged = pairing("exchanged");
        exchanged.exchanged_at_utc = Some("2026-07-28T10:01:00Z".to_string());
        let mut approval_expired = pairing("approval-expired");
        approval_expired.approval = Some(approval("approval-expired", "requested-store"));
        approval_expired
            .approval
            .as_mut()
            .expect("approval")
            .approval_expires_at_utc = "2026-07-28T09:59:59Z".to_string();
        write_store(
            &path,
            &RemoteEasyconnectPairingStoreFile {
                schema_version: REMOTE_EASYCONNECT_PAIRING_SCHEMA,
                pairings: vec![pairing("still-live"), expired, exchanged, approval_expired],
            },
        )
        .expect("seed store");

        FileBackedRemoteEasyconnectPairingStore::with_max_live_pairings(&path, 2)
            .create(pairing("new-live"))
            .expect("terminal rows release admission capacity");
        let stored = read_store(&path).expect("reloaded store");
        let ids = stored
            .pairings
            .iter()
            .map(|pairing| pairing.pairing_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, ["still-live", "new-live"]);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn fresh_request_mint_supersedes_only_the_prior_pending_pairing() {
        let root = root();
        let path = root.join("pairings.json");
        let store = FileBackedRemoteEasyconnectPairingStore::new(&path);
        let mut first = pairing("pairing-old");
        first.client_request_id = Some("login:stephen@example.com:allele-anchor".to_string());
        store.create(first).expect("first pending mint");
        let mut replacement = pairing("pairing-new");
        replacement.client_request_id = Some("login:stephen@example.com:allele-anchor".to_string());
        store.create(replacement).expect("replacement mint");

        let stored = read_store(&path).expect("stored pairings");
        assert_eq!(stored.pairings.len(), 1);
        assert_eq!(stored.pairings[0].pairing_id, "pairing-new");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[cfg(unix)]
    #[test]
    fn persistence_repairs_pairing_directory_and_file_modes() {
        let root = root();
        let path = root.join("nested/pairings.json");
        let store = FileBackedRemoteEasyconnectPairingStore::new(&path);
        store.create(pairing("pairing-1")).expect("first pairing");
        fs::set_permissions(
            path.parent().expect("parent"),
            fs::Permissions::from_mode(0o755),
        )
        .expect("relax parent mode");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("relax file mode");

        store.create(pairing("pairing-2")).expect("rewrite store");
        assert_eq!(
            fs::metadata(path.parent().expect("parent"))
                .expect("parent metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&path)
                .expect("file metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn prepare_exchange_does_not_consume_before_the_session_commit() {
        let root = root();
        let store = FileBackedRemoteEasyconnectPairingStore::new(root.join("pairings.json"));
        store.create(pairing("pairing-atomic")).expect("created");
        store
            .approve(
                approval("pairing-atomic", "requested-store"),
                "2026-07-28T10:01:00Z",
            )
            .expect("approved");
        let exchange = super::RemoteEasyconnectPairingExchange {
            pairing_id: "pairing-atomic".to_string(),
            exchange_code: "exchange-secret".to_string(),
            exchanged_at_utc: "2026-07-28T10:02:00Z".to_string(),
        };
        store
            .prepare_exchange(&exchange)
            .expect("preparation validates");
        assert_eq!(
            store
                .status("pairing-atomic", "2026-07-28T10:02:00Z")
                .expect("status")
                .state,
            crate::RemoteEasyconnectPairingState::Approved
        );
        store.commit_exchange(exchange).expect("commit consumes");
        assert_eq!(
            store
                .status("pairing-atomic", "2026-07-28T10:02:01Z")
                .expect("status")
                .state,
            crate::RemoteEasyconnectPairingState::Exchanged
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    fn pairing(pairing_id: &str) -> RemoteEasyconnectPairingRecord {
        RemoteEasyconnectPairingRecord {
            pairing_id: pairing_id.to_string(),
            browser_handoff_reference: format!("handoff-{pairing_id}"),
            client_name: "remote CLI".to_string(),
            callback_url: Some(
                "http://127.0.0.1:49152/products/dasobjectstore/remote/easyconnect/callback"
                    .to_string(),
            ),
            completion_mode: crate::api::RemoteEasyconnectCompletionMode::Callback,
            requested_object_store: Some("requested-store".to_string()),
            requested_session_lifetime_seconds: None,
            client_request_id: Some(format!("request-{pairing_id}")),
            created_at_utc: "2099-07-28T10:00:00Z".to_string(),
            expires_at_utc: "2099-07-28T10:05:00Z".to_string(),
            approval: None,
            exchanged_at_utc: None,
        }
    }

    fn approval(pairing_id: &str, object_store: &str) -> RemoteEasyconnectPairingApproval {
        RemoteEasyconnectPairingApproval {
            pairing_id: pairing_id.to_string(),
            context: RemoteEasyconnectApprovalContext {
                authority_id: "authority-1".to_string(),
                principal_id: "principal-1".to_string(),
                session_id: "session-1".to_string(),
                auth_provider: RemoteEasyconnectAuthProvider::Pistis,
                allowed_object_stores: vec![RemoteEasyconnectObjectStoreGrant {
                    object_store: object_store.to_string(),
                    bucket: "bucket".to_string(),
                    can_read: true,
                    can_write: true,
                    writer_group: Some("writers".to_string()),
                    object_type: "store_scoped_session".to_string(),
                    control_operations: remote_easyconnect_control_operations(true),
                    allowed_prefixes: vec![REMOTE_EASYCONNECT_DEFAULT_CONTROL_PREFIX.to_string()],
                }],
                host_session_expires_at_utc: "2099-07-28T10:04:00Z".to_string(),
                correlation_id: "correlation-1".to_string(),
                audit_identity: "pistis:principal-1".to_string(),
            },
            approval_expires_at_utc: "2099-07-28T10:03:00Z".to_string(),
            exchange_code: "exchange-secret".to_string(),
        }
    }

    #[test]
    fn browser_handoff_is_opaque_pending_and_cannot_be_replayed() {
        let root = root();
        let store = FileBackedRemoteEasyconnectPairingStore::new(root.join("pairings.json"));
        store
            .create(pairing("handoff-pending"))
            .expect("pairing stored");

        let status = store
            .status_by_browser_handoff("handoff-handoff-pending", "2099-07-28T10:01:00Z")
            .expect("opaque handoff resolves while pending");
        assert_eq!(status.pairing_id, "handoff-pending");
        assert_eq!(
            status.requested_object_store.as_deref(),
            Some("requested-store")
        );
        assert!(matches!(
            store.status_by_browser_handoff("handoff-tampered", "2099-07-28T10:01:00Z"),
            Err(RemoteEasyconnectPairingStoreError::BrowserHandoffUnavailable)
        ));
        store
            .approve(
                approval("handoff-pending", "requested-store"),
                "2099-07-28T10:02:00Z",
            )
            .expect("approval stored");
        assert!(matches!(
            store.status_by_browser_handoff("handoff-handoff-pending", "2099-07-28T10:02:00Z"),
            Err(RemoteEasyconnectPairingStoreError::BrowserHandoffUnavailable)
        ));
        assert!(matches!(
            store.status_by_browser_handoff("handoff-handoff-pending", "2099-07-28T10:06:00Z"),
            Err(RemoteEasyconnectPairingStoreError::BrowserHandoffUnavailable)
        ));

        fs::remove_dir_all(root).expect("cleanup");
    }
}
