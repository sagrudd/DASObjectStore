//! Durable single-use replay protection for upload-completion capabilities.
//!
//! The registry stores only capability identity, application identity, nonce,
//! and expiry. It never stores bearer tokens, proofs, host paths, or object
//! contents. Consumption is an atomic read/validate/write transition so a
//! later completion attempt cannot reuse the same capability.

use super::DaemonServiceRuntimeError;
use dasobjectstore_core::application_auth::UploadCompletionCapability;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub const APPLICATION_CAPABILITY_REPLAY_SCHEMA: &str =
    "dasobjectstore.application_capability_replay.v1";
pub const APPLICATION_CAPABILITY_REPLAY_FILE_NAME: &str = "application-capability-replay.json";
pub const APPLICATION_CAPABILITY_REPLAY_ENV: &str = "DASOBJECTSTORE_APPLICATION_REPLAY_PATH";

static REPLAY_REGISTRY_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static COMPLETION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UploadCompletionCapabilityOutcome {
    Committed,
    AlreadyConsumed,
}

pub fn default_application_capability_replay_path(state_dir: impl AsRef<Path>) -> PathBuf {
    state_dir
        .as_ref()
        .join(APPLICATION_CAPABILITY_REPLAY_FILE_NAME)
}

pub fn application_capability_replay_path(state_dir: impl AsRef<Path>) -> PathBuf {
    std::env::var_os(APPLICATION_CAPABILITY_REPLAY_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| default_application_capability_replay_path(state_dir))
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ReplayRecord {
    application_id: String,
    capability_id: String,
    nonce: String,
    expires_at_unix_seconds: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ReplayRegistryFile {
    schema_version: String,
    records: Vec<ReplayRecord>,
}

impl Default for ReplayRegistryFile {
    fn default() -> Self {
        Self {
            schema_version: APPLICATION_CAPABILITY_REPLAY_SCHEMA.to_string(),
            records: Vec::new(),
        }
    }
}

/// Atomically consume a capability nonce. Returns `true` when the capability
/// was accepted and `false` when the same capability or nonce was already used.
pub fn consume_upload_completion_capability(
    path: impl AsRef<Path>,
    capability: &UploadCompletionCapability,
    now_unix_seconds: u64,
) -> Result<bool, DaemonServiceRuntimeError> {
    let _guard = REPLAY_REGISTRY_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| invalid_replay("application capability replay lock poisoned"))?;
    capability
        .validate()
        .map_err(|error| invalid_replay(error.to_string()))?;
    if now_unix_seconds >= capability.expires_at_unix_seconds {
        return Err(invalid_replay("upload-completion capability is expired"));
    }
    let path = path.as_ref();
    let mut registry = read_registry(path)?;
    registry
        .records
        .retain(|record| record.expires_at_unix_seconds > now_unix_seconds);
    let mut used = BTreeSet::new();
    for record in &registry.records {
        used.insert((
            record.application_id.as_str(),
            record.capability_id.as_str(),
        ));
        used.insert((record.application_id.as_str(), record.nonce.as_str()));
    }
    if used.contains(&(
        capability.application_id.as_str(),
        capability.capability_id.as_str(),
    )) || used.contains(&(
        capability.application_id.as_str(),
        capability.nonce.as_str(),
    )) {
        write_registry(path, &registry)?;
        return Ok(false);
    }
    registry.records.push(ReplayRecord {
        application_id: capability.application_id.clone(),
        capability_id: capability.capability_id.clone(),
        nonce: capability.nonce.clone(),
        expires_at_unix_seconds: capability.expires_at_unix_seconds,
    });
    registry.records.sort_by(|left, right| {
        left.application_id
            .cmp(&right.application_id)
            .then_with(|| left.capability_id.cmp(&right.capability_id))
    });
    write_registry(path, &registry)?;
    Ok(true)
}

/// Execute the authority-side completion ordering. Operations are serialized
/// across the commit boundary, and replay is recorded only after the durable,
/// idempotent catalogue commit. A crash can therefore cause a safe repeated
/// commit, never a false already-committed response.
pub fn complete_upload_with_capability<VerifyProvider, CommitCatalogue>(
    path: impl AsRef<Path>,
    capability: &UploadCompletionCapability,
    now_unix_seconds: u64,
    verify_provider: VerifyProvider,
    commit_catalogue: CommitCatalogue,
) -> Result<UploadCompletionCapabilityOutcome, DaemonServiceRuntimeError>
where
    VerifyProvider: FnOnce(&UploadCompletionCapability) -> Result<(), String>,
    CommitCatalogue: FnOnce(&UploadCompletionCapability) -> Result<(), String>,
{
    let _completion_guard = COMPLETION_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| invalid_replay("application upload completion lock poisoned"))?;
    if upload_completion_capability_consumed(&path, capability, now_unix_seconds)? {
        return Ok(UploadCompletionCapabilityOutcome::AlreadyConsumed);
    }
    verify_provider(capability).map_err(|message| {
        completion_operation_error(format!("provider verification failed: {message}"))
    })?;
    if let Err(message) = commit_catalogue(capability) {
        return Err(completion_operation_error(format!(
            "catalogue completion failed: {message}"
        )));
    }
    if !consume_upload_completion_capability(&path, capability, now_unix_seconds)? {
        return Ok(UploadCompletionCapabilityOutcome::AlreadyConsumed);
    }
    Ok(UploadCompletionCapabilityOutcome::Committed)
}

fn upload_completion_capability_consumed(
    path: impl AsRef<Path>,
    capability: &UploadCompletionCapability,
    now_unix_seconds: u64,
) -> Result<bool, DaemonServiceRuntimeError> {
    let _guard = REPLAY_REGISTRY_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| invalid_replay("application capability replay lock poisoned"))?;
    let path = path.as_ref();
    let mut registry = read_registry(path)?;
    registry
        .records
        .retain(|record| record.expires_at_unix_seconds > now_unix_seconds);
    let consumed = registry.records.iter().any(|record| {
        record.application_id == capability.application_id
            && (record.capability_id == capability.capability_id
                || record.nonce == capability.nonce)
    });
    if path.exists() {
        write_registry(path, &registry)?;
    }
    Ok(consumed)
}

pub fn release_upload_completion_capability(
    path: impl AsRef<Path>,
    capability: &UploadCompletionCapability,
) -> Result<bool, DaemonServiceRuntimeError> {
    let _guard = REPLAY_REGISTRY_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| invalid_replay("application capability replay lock poisoned"))?;
    let path = path.as_ref();
    let mut registry = read_registry(path)?;
    let before = registry.records.len();
    registry.records.retain(|record| {
        !(record.application_id == capability.application_id
            && record.capability_id == capability.capability_id
            && record.nonce == capability.nonce)
    });
    if registry.records.len() == before {
        return Ok(false);
    }
    write_registry(path, &registry)?;
    Ok(true)
}

fn read_registry(path: &Path) -> Result<ReplayRegistryFile, DaemonServiceRuntimeError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(ReplayRegistryFile::default())
        }
        Err(error) => return Err(registry_io(path, error)),
    };
    let registry: ReplayRegistryFile = serde_json::from_reader(file).map_err(|error| {
        invalid_replay(format!(
            "invalid application capability replay registry {}: {error}",
            path.display()
        ))
    })?;
    if registry.schema_version != APPLICATION_CAPABILITY_REPLAY_SCHEMA {
        return Err(invalid_replay(format!(
            "unsupported application capability replay schema {}",
            registry.schema_version
        )));
    }
    let mut identities = BTreeSet::new();
    for record in &registry.records {
        if record.application_id.trim().is_empty()
            || record.capability_id.trim().is_empty()
            || record.nonce.trim().is_empty()
            || !identities.insert((
                record.application_id.as_str(),
                record.capability_id.as_str(),
            ))
        {
            return Err(invalid_replay("duplicate or blank replay identity"));
        }
    }
    Ok(registry)
}

fn write_registry(
    path: &Path,
    registry: &ReplayRegistryFile,
) -> Result<(), DaemonServiceRuntimeError> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid_replay("application capability replay registry has no parent"))?;
    fs::create_dir_all(parent).map_err(|error| registry_io(parent, error))?;
    let temporary = parent.join(format!(
        ".{}.tmp-{}-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("application-capability-replay"),
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    let bytes = serde_json::to_vec_pretty(registry).map_err(|error| {
        invalid_replay(format!(
            "serialize application capability replay registry: {error}"
        ))
    })?;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| registry_io(&temporary, error))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| registry_io(&temporary, error))?;
    drop(file);
    fs::rename(&temporary, path).map_err(|error| registry_io(path, error))?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| registry_io(parent, error))
}

fn invalid_replay(message: impl Into<String>) -> DaemonServiceRuntimeError {
    DaemonServiceRuntimeError::UnsupportedOperation {
        operation: format!(
            "invalid application capability replay registry: {}",
            message.into()
        ),
    }
}

fn registry_io(path: &Path, error: io::Error) -> DaemonServiceRuntimeError {
    DaemonServiceRuntimeError::UnsupportedOperation {
        operation: format!(
            "application capability replay registry I/O {}: {error}",
            path.display()
        ),
    }
}

fn completion_operation_error(message: impl Into<String>) -> DaemonServiceRuntimeError {
    DaemonServiceRuntimeError::UnsupportedOperation {
        operation: format!("application upload completion: {}", message.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        application_capability_replay_path, complete_upload_with_capability,
        consume_upload_completion_capability, default_application_capability_replay_path,
        release_upload_completion_capability, UploadCompletionCapabilityOutcome,
        APPLICATION_CAPABILITY_REPLAY_SCHEMA,
    };
    use dasobjectstore_core::application_auth::UploadCompletionCapability;
    use dasobjectstore_core::ids::StoreId;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn root(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let root = std::env::var_os("DASOBJECTSTORE_CODEX_VALIDATION_ROOT")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|home| PathBuf::from(home).join(".dasobjectstore-codex-validation"))
            })
            .unwrap_or_else(std::env::temp_dir)
            .join(format!(
                "application-replay-{label}-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(&root).expect("fixture root");
        root
    }

    fn capability(
        id: &str,
        nonce: &str,
        issued_at_unix_seconds: u64,
        expiry: u64,
    ) -> UploadCompletionCapability {
        UploadCompletionCapability {
            schema_version: "dasobjectstore.application_auth.v1".to_string(),
            capability_id: id.to_string(),
            application_id: "synoptikon-ingest".to_string(),
            session_id: "session-1".to_string(),
            upload_id: "upload-1".to_string(),
            store_id: StoreId::new("codex").expect("store"),
            object_key: "analysis/run-1.fastq".to_string(),
            expected_size_bytes: 42,
            expected_checksum: format!("sha256:{}", "a".repeat(64)),
            audience: "dasobjectstore".to_string(),
            issued_at_unix_seconds,
            expires_at_unix_seconds: expiry,
            nonce: nonce.to_string(),
        }
    }

    #[test]
    fn replay_path_is_state_scoped_and_env_overrideable() {
        assert_eq!(
            default_application_capability_replay_path("/var/lib/dasobjectstore"),
            PathBuf::from("/var/lib/dasobjectstore/application-capability-replay.json")
        );
        let _ = application_capability_replay_path("/var/lib/dasobjectstore");
    }

    #[test]
    fn capability_is_single_use_and_expired_records_are_pruned() {
        let path = root("consume").join("replay.json");
        let first = capability("cap-1", "nonce-1", 1_000, 1_800);
        assert!(consume_upload_completion_capability(&path, &first, 1_100).expect("first"));
        assert!(!consume_upload_completion_capability(&path, &first, 1_200).expect("replay"));
        let second = capability("cap-2", "nonce-2", 1_900, 2_500);
        assert!(consume_upload_completion_capability(&path, &second, 2_000).expect("second"));
        let encoded = fs::read_to_string(&path).expect("registry bytes");
        assert!(encoded.contains(APPLICATION_CAPABILITY_REPLAY_SCHEMA));
        assert!(!encoded.contains("private_key"));
        assert!(!encoded.contains("analysis/run-1.fastq"));
    }

    #[test]
    fn concurrent_consumers_accept_only_one_capability_use() {
        let path = root("concurrent-consume").join("replay.json");
        let capability = capability("cap-concurrent", "nonce-concurrent", 1_000, 1_800);
        let left_path = path.clone();
        let left_capability = capability.clone();
        let left = std::thread::spawn(move || {
            consume_upload_completion_capability(&left_path, &left_capability, 1_100)
                .expect("left consume")
        });
        let right_path = path.clone();
        let right = std::thread::spawn(move || {
            consume_upload_completion_capability(&right_path, &capability, 1_100)
                .expect("right consume")
        });
        let outcomes = [
            left.join().expect("left joins"),
            right.join().expect("right joins"),
        ];
        assert_eq!(outcomes.iter().filter(|accepted| **accepted).count(), 1);
        assert_eq!(outcomes.iter().filter(|accepted| !**accepted).count(), 1);
    }

    #[test]
    fn completion_verifies_provider_before_consuming_and_releases_failed_catalogue() {
        let path = root("completion").join("replay.json");
        let capability = capability("cap-1", "nonce-1", 1_000, 1_800);
        let provider_failed = complete_upload_with_capability(
            &path,
            &capability,
            1_100,
            |_| Err("checksum mismatch".to_string()),
            |_| panic!("catalogue must not run"),
        );
        assert!(provider_failed.is_err());
        assert!(!path.exists());

        let catalogue_failed = complete_upload_with_capability(
            &path,
            &capability,
            1_100,
            |_| Ok(()),
            |_| Err("database unavailable".to_string()),
        );
        assert!(catalogue_failed.is_err());
        assert!(release_upload_completion_capability(&path, &capability).is_ok());

        assert_eq!(
            complete_upload_with_capability(&path, &capability, 1_100, |_| Ok(()), |_| Ok(()))
                .expect("completion"),
            UploadCompletionCapabilityOutcome::Committed
        );
        assert_eq!(
            complete_upload_with_capability(
                &path,
                &capability,
                1_100,
                |_| Ok(()),
                |_| panic!("idempotent replay must not commit")
            )
            .expect("replay"),
            UploadCompletionCapabilityOutcome::AlreadyConsumed
        );
    }
}
