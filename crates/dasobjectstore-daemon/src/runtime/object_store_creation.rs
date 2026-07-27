//! Durable, idempotent ObjectStore creation intents.

use crate::api::{CreateObjectStoreRequest, DaemonJobId};
use dasobjectstore_core::ids::StoreId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::{self, Display};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub const OBJECT_STORE_CREATION_INTENT_SCHEMA: &str =
    "dasobjectstore.object_store_creation_intents.v1";
pub const OBJECT_STORE_CREATION_INTENT_FILE_NAME: &str = "object-store-creation-intents.json";

static INTENT_WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectStoreCreationPhase {
    Validated,
    CapacityInitializing,
    CapacityInitialized,
    DefinitionPublished,
    Complete,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectStoreCreationIntent {
    pub client_request_id: String,
    pub request_digest: String,
    pub job_id: DaemonJobId,
    pub store_id: StoreId,
    pub administrator_actor: String,
    pub accepted_at_utc: String,
    pub phase: ObjectStoreCreationPhase,
    pub capacity_created: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ObjectStoreCreationIntentRegistry {
    schema_version: String,
    intents: Vec<ObjectStoreCreationIntent>,
}

impl Default for ObjectStoreCreationIntentRegistry {
    fn default() -> Self {
        Self {
            schema_version: OBJECT_STORE_CREATION_INTENT_SCHEMA.to_string(),
            intents: Vec::new(),
        }
    }
}

pub fn object_store_creation_intent_path(state_dir: impl AsRef<Path>) -> PathBuf {
    state_dir
        .as_ref()
        .join(OBJECT_STORE_CREATION_INTENT_FILE_NAME)
}

pub fn normalized_object_store_creation_digest(
    request: &CreateObjectStoreRequest,
) -> Result<String, ObjectStoreCreationIntentError> {
    let mut normalized = request.clone();
    // Authentication context is transport-derived and can legitimately differ
    // in display form across an exact retry. It is recorded on the intent but
    // is not client-controlled creation identity.
    normalized.administrator_actor = None;
    let encoded = serde_json::to_vec(&normalized)
        .map_err(|error| ObjectStoreCreationIntentError::Serialization(error.to_string()))?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}

pub fn creation_client_request_id(request: &CreateObjectStoreRequest, digest: &str) -> String {
    request
        .client_request_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("legacy-{digest}"))
}

pub fn creation_job_id(
    client_request_id: &str,
) -> Result<DaemonJobId, ObjectStoreCreationIntentError> {
    let digest = Sha256::digest(client_request_id.as_bytes());
    DaemonJobId::new(format!("objectstore-create-{:x}", digest))
        .map_err(|_| ObjectStoreCreationIntentError::InvalidClientRequestId)
}

pub fn begin_object_store_creation_intent(
    path: impl AsRef<Path>,
    request: &CreateObjectStoreRequest,
    administrator_actor: &str,
    accepted_at_utc: &str,
) -> Result<ObjectStoreCreationIntent, ObjectStoreCreationIntentError> {
    let digest = normalized_object_store_creation_digest(request)?;
    let client_request_id = creation_client_request_id(request, &digest);
    let _guard = intent_lock()?;
    let mut registry = read_registry(path.as_ref())?;
    if let Some(existing) = registry
        .intents
        .iter()
        .find(|intent| intent.client_request_id == client_request_id)
    {
        if existing.request_digest != digest {
            return Err(ObjectStoreCreationIntentError::RequestConflict(
                client_request_id,
            ));
        }
        return Ok(existing.clone());
    }
    let intent = ObjectStoreCreationIntent {
        client_request_id: client_request_id.clone(),
        request_digest: digest,
        job_id: creation_job_id(&client_request_id)?,
        store_id: StoreId::new(request.store_id.clone())
            .map_err(|_| ObjectStoreCreationIntentError::InvalidStoreId)?,
        administrator_actor: administrator_actor.to_string(),
        accepted_at_utc: accepted_at_utc.to_string(),
        phase: ObjectStoreCreationPhase::Validated,
        capacity_created: false,
    };
    registry.intents.push(intent.clone());
    registry
        .intents
        .sort_by(|left, right| left.client_request_id.cmp(&right.client_request_id));
    write_registry(path.as_ref(), &registry)?;
    Ok(intent)
}

pub fn advance_object_store_creation_intent(
    path: impl AsRef<Path>,
    expected: &ObjectStoreCreationIntent,
    phase: ObjectStoreCreationPhase,
    capacity_created: bool,
) -> Result<ObjectStoreCreationIntent, ObjectStoreCreationIntentError> {
    let _guard = intent_lock()?;
    let mut registry = read_registry(path.as_ref())?;
    let intent = registry
        .intents
        .iter_mut()
        .find(|intent| intent.client_request_id == expected.client_request_id)
        .ok_or(ObjectStoreCreationIntentError::IntentMissing)?;
    if intent.request_digest != expected.request_digest {
        return Err(ObjectStoreCreationIntentError::RequestConflict(
            expected.client_request_id.clone(),
        ));
    }
    intent.phase = phase;
    intent.capacity_created = capacity_created;
    let updated = intent.clone();
    write_registry(path.as_ref(), &registry)?;
    Ok(updated)
}

fn intent_lock() -> Result<std::sync::MutexGuard<'static, ()>, ObjectStoreCreationIntentError> {
    INTENT_WRITE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| ObjectStoreCreationIntentError::LockPoisoned)
}

fn read_registry(
    path: &Path,
) -> Result<ObjectStoreCreationIntentRegistry, ObjectStoreCreationIntentError> {
    let encoded = match fs::read_to_string(path) {
        Ok(encoded) => encoded,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ObjectStoreCreationIntentRegistry::default());
        }
        Err(error) => return Err(error.into()),
    };
    let registry: ObjectStoreCreationIntentRegistry = serde_json::from_str(&encoded)
        .map_err(|error| ObjectStoreCreationIntentError::Malformed(error.to_string()))?;
    if registry.schema_version != OBJECT_STORE_CREATION_INTENT_SCHEMA {
        return Err(ObjectStoreCreationIntentError::UnsupportedSchema(
            registry.schema_version,
        ));
    }
    Ok(registry)
}

fn write_registry(
    path: &Path,
    registry: &ObjectStoreCreationIntentRegistry,
) -> Result<(), ObjectStoreCreationIntentError> {
    let parent = path
        .parent()
        .ok_or(ObjectStoreCreationIntentError::MissingParent)?;
    fs::create_dir_all(parent)?;
    let encoded = serde_json::to_vec_pretty(registry)
        .map_err(|error| ObjectStoreCreationIntentError::Serialization(error.to_string()))?;
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    match fs::remove_file(&temporary) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)?;
    file.write_all(&encoded)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    fs::rename(&temporary, path)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

#[derive(Debug)]
pub enum ObjectStoreCreationIntentError {
    Io(std::io::Error),
    Malformed(String),
    UnsupportedSchema(String),
    Serialization(String),
    RequestConflict(String),
    InvalidClientRequestId,
    InvalidStoreId,
    IntentMissing,
    MissingParent,
    LockPoisoned,
}

impl Display for ObjectStoreCreationIntentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "creation intent IO failed: {error}"),
            Self::Malformed(error) => write!(formatter, "creation intent is malformed: {error}"),
            Self::UnsupportedSchema(schema) => {
                write!(formatter, "unsupported creation intent schema: {schema}")
            }
            Self::Serialization(error) => {
                write!(formatter, "creation intent serialization failed: {error}")
            }
            Self::RequestConflict(request_id) => write!(
                formatter,
                "client request id {request_id} is already bound to a different creation request"
            ),
            Self::InvalidClientRequestId => formatter.write_str("invalid client request id"),
            Self::InvalidStoreId => formatter.write_str("invalid ObjectStore id"),
            Self::IntentMissing => formatter.write_str("creation intent disappeared"),
            Self::MissingParent => formatter.write_str("creation intent path has no parent"),
            Self::LockPoisoned => formatter.write_str("creation intent lock is unavailable"),
        }
    }
}

impl std::error::Error for ObjectStoreCreationIntentError {}

impl From<std::io::Error> for ObjectStoreCreationIntentError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::OBJECT_STORE_CREATE_CONFIRMATION;

    fn request() -> CreateObjectStoreRequest {
        CreateObjectStoreRequest {
            store_id: "generated-data".to_string(),
            store_class: "generated_data".to_string(),
            required_copies: 2,
            bucket: Some("generated-data".to_string()),
            reader_group: None,
            writer_group: "mnemosyne".to_string(),
            ssd_root: PathBuf::from("/srv/dasobjectstore/ssd"),
            object_type: "naive".to_string(),
            enclosure_id: None,
            public: false,
            writeable: true,
            capacity_behavior: "backpressure_by_priority".to_string(),
            retention: "tombstone_then_gc".to_string(),
            endpoint_export_mode: "s3_bucket".to_string(),
            dry_run: false,
            client_request_id: Some("request-42".to_string()),
            administrator_actor: Some("spoofed".to_string()),
            confirmation_marker: OBJECT_STORE_CREATE_CONFIRMATION.to_string(),
        }
    }

    #[test]
    fn exact_replay_reuses_intent_and_changed_request_conflicts() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-creation-intent-{}",
            std::process::id()
        ));
        let path = root.join("intents.json");
        let first =
            begin_object_store_creation_intent(&path, &request(), "uid:0", "2026-07-27T10:00:00Z")
                .expect("first intent");
        let replay =
            begin_object_store_creation_intent(&path, &request(), "uid:0", "2026-07-27T11:00:00Z")
                .expect("replay");
        assert_eq!(first, replay);
        let mut conflicting = request();
        conflicting.required_copies = 3;
        assert!(matches!(
            begin_object_store_creation_intent(
                &path,
                &conflicting,
                "uid:0",
                "2026-07-27T12:00:00Z"
            ),
            Err(ObjectStoreCreationIntentError::RequestConflict(_))
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn spoofed_actor_does_not_change_request_identity() {
        let mut left = request();
        let mut right = request();
        left.administrator_actor = Some("spoof-a".to_string());
        right.administrator_actor = Some("spoof-b".to_string());
        assert_eq!(
            normalized_object_store_creation_digest(&left).unwrap(),
            normalized_object_store_creation_digest(&right).unwrap()
        );
    }

    #[test]
    fn nonterminal_phase_is_restart_visible_and_strictly_decoded() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-creation-phase-{}",
            std::process::id()
        ));
        let path = root.join("intents.json");
        let intent =
            begin_object_store_creation_intent(&path, &request(), "root", "2026-07-27T10:00:00Z")
                .expect("intent");
        let advanced = advance_object_store_creation_intent(
            &path,
            &intent,
            ObjectStoreCreationPhase::CapacityInitialized,
            true,
        )
        .expect("advance");
        let replay =
            begin_object_store_creation_intent(&path, &request(), "root", "later").expect("replay");
        assert_eq!(replay, advanced);
        let encoded = fs::read_to_string(&path).expect("registry");
        fs::write(
            &path,
            encoded.replacen(
                "\"schema_version\"",
                "\"unknown_field\":true,\"schema_version\"",
                1,
            ),
        )
        .expect("corrupt with unknown field");
        assert!(matches!(
            begin_object_store_creation_intent(&path, &request(), "root", "later"),
            Err(ObjectStoreCreationIntentError::Malformed(_))
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn capacity_initializing_checkpoint_survives_side_effect_crash_window() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-creation-initializing-{}",
            std::process::id()
        ));
        let path = root.join("intents.json");
        let intent =
            begin_object_store_creation_intent(&path, &request(), "root", "2026-07-27T10:00:00Z")
                .expect("intent");
        let checkpoint = advance_object_store_creation_intent(
            &path,
            &intent,
            ObjectStoreCreationPhase::CapacityInitializing,
            false,
        )
        .expect("checkpoint before capacity side effect");

        let replay =
            begin_object_store_creation_intent(&path, &request(), "root", "later").expect("replay");
        assert_eq!(replay, checkpoint);
        assert_eq!(replay.phase, ObjectStoreCreationPhase::CapacityInitializing);
        assert!(!replay.capacity_created);
        let _ = fs::remove_dir_all(root);
    }
}
