//! Durable owner-side intent and settlement ledger for Synoptikon projection.

use super::DaemonServiceRuntimeError;
use dasobjectstore_core::{
    validate_synoptikon_projection_request, SynoptikonProjectionRequestV1,
    SynoptikonProjectionSettlementV1,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::ffi::CString;
use std::fs::File;
use std::io::{self, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub const SYNOPTIKON_PROJECTION_LEDGER_SCHEMA: &str =
    "dasobjectstore.synoptikon_projection_ledger.v1";
pub const SYNOPTIKON_PROJECTION_LEDGER_FILE_NAME: &str = "synoptikon-projection-ledger.json";
static LEDGER_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SynoptikonProjectionIntentRecord {
    pub intent_id: String,
    pub projection: SynoptikonProjectionRequestV1,
    pub uploaded: bool,
    pub settlement_id: Option<String>,
    pub settlement: Option<SynoptikonProjectionSettlementV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    schema_version: String,
    authority_sequence: u64,
    intents: Vec<SynoptikonProjectionIntentRecord>,
}

impl Default for Ledger {
    fn default() -> Self {
        Self {
            schema_version: SYNOPTIKON_PROJECTION_LEDGER_SCHEMA.to_owned(),
            authority_sequence: 0,
            intents: Vec::new(),
        }
    }
}

pub fn synoptikon_projection_ledger_path(state_dir: impl AsRef<Path>) -> PathBuf {
    state_dir
        .as_ref()
        .join("projection-authority")
        .join(SYNOPTIKON_PROJECTION_LEDGER_FILE_NAME)
}

pub fn prepare_synoptikon_projection_intent(
    path: impl AsRef<Path>,
    logical_name: &str,
    size_bytes: u64,
    sha256: &str,
    now: u64,
) -> Result<(SynoptikonProjectionIntentRecord, bool), DaemonServiceRuntimeError> {
    let _guard = lock()?;
    let path = path.as_ref();
    let mut ledger = read(path)?;
    if let Some(existing) = ledger.intents.iter().find(|item| {
        item.projection.object_key == logical_name
            && item.projection.source_size_bytes == size_bytes
            && item.projection.source_sha256 == sha256
    }) {
        return Ok((existing.clone(), true));
    }
    let object_version = ledger
        .intents
        .iter()
        .filter(|item| item.projection.object_key == logical_name)
        .map(|item| item.projection.object_version)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| invalid("projection object version exhausted"))?;
    let digest = canonical_sha256(&(logical_name, size_bytes, sha256, object_version))?;
    let intent_id = format!("syno-{}", &digest[..48]);
    let projection = SynoptikonProjectionRequestV1 {
        schema_version: dasobjectstore_core::SYNOPTIKON_PROJECTION_REQUEST_V1_SCHEMA.to_owned(),
        projection_id: intent_id.clone(),
        producer_product: dasobjectstore_core::SYNOPTIKON_PROJECTION_PRODUCER_PRODUCT.to_owned(),
        producer_host: dasobjectstore_core::SYNOPTIKON_PROJECTION_PRODUCER_HOST.to_owned(),
        consumer_product: dasobjectstore_core::SYNOPTIKON_PROJECTION_CONSUMER_PRODUCT.to_owned(),
        consumer_host: dasobjectstore_core::SYNOPTIKON_PROJECTION_CONSUMER_HOST.to_owned(),
        object_store_id: crate::api::SYNOPTIKON_PROJECTION_FIXED_STORE_ID.to_owned(),
        object_id: format!(
            "{}/{}",
            crate::api::SYNOPTIKON_PROJECTION_FIXED_STORE_ID,
            logical_name
        ),
        object_version,
        object_key: logical_name.to_owned(),
        generation: object_version,
        source_size_bytes: size_bytes,
        source_sha256: sha256.to_owned(),
        nonce: canonical_sha256(&(intent_id.as_str(), sha256, now))?,
        requested_at_unix_seconds: now,
        expires_at_unix_seconds: now
            .saturating_add(dasobjectstore_core::SYNOPTIKON_PROJECTION_MAX_LIFETIME_SECONDS),
    };
    validate_synoptikon_projection_request(&projection, now)
        .map_err(|error| invalid(error.to_string()))?;
    let record = SynoptikonProjectionIntentRecord {
        intent_id,
        projection,
        uploaded: false,
        settlement_id: None,
        settlement: None,
    };
    ledger.intents.push(record.clone());
    write(path, &ledger)?;
    Ok((record, false))
}

pub fn projection_intent(
    path: impl AsRef<Path>,
    intent_id: &str,
) -> Result<SynoptikonProjectionIntentRecord, DaemonServiceRuntimeError> {
    let _guard = lock()?;
    read(path.as_ref())?
        .intents
        .into_iter()
        .find(|item| item.intent_id == intent_id)
        .ok_or_else(|| invalid("projection intent is unavailable"))
}

pub fn mark_projection_uploaded(
    path: impl AsRef<Path>,
    intent_id: &str,
) -> Result<(), DaemonServiceRuntimeError> {
    let _guard = lock()?;
    let path = path.as_ref();
    let mut ledger = read(path)?;
    let record = ledger
        .intents
        .iter_mut()
        .find(|item| item.intent_id == intent_id)
        .ok_or_else(|| invalid("projection intent is unavailable"))?;
    record.uploaded = true;
    write(path, &ledger)
}

pub fn commit_projection_settlement<F>(
    path: impl AsRef<Path>,
    intent_id: &str,
    build: F,
) -> Result<(String, SynoptikonProjectionSettlementV1, bool), DaemonServiceRuntimeError>
where
    F: FnOnce(
        &SynoptikonProjectionRequestV1,
        u64,
    ) -> Result<SynoptikonProjectionSettlementV1, DaemonServiceRuntimeError>,
{
    let _guard = lock()?;
    let path = path.as_ref();
    let mut ledger = read(path)?;
    let index = ledger
        .intents
        .iter()
        .position(|item| item.intent_id == intent_id)
        .ok_or_else(|| invalid("projection intent is unavailable"))?;
    if !ledger.intents[index].uploaded {
        return Err(invalid("projection upload is not durably complete"));
    }
    if let (Some(id), Some(settlement)) = (
        ledger.intents[index].settlement_id.clone(),
        ledger.intents[index].settlement.clone(),
    ) {
        return Ok((id, settlement, true));
    }
    ledger.authority_sequence = ledger
        .authority_sequence
        .checked_add(1)
        .ok_or_else(|| invalid("authority sequence exhausted"))?;
    // Reserve the monotonic owner sequence durably before consulting live
    // readiness. A failed or interrupted readiness attempt may leave a gap,
    // but can never publish a sequence that was not first persisted.
    write(path, &ledger)?;
    let settlement = build(&ledger.intents[index].projection, ledger.authority_sequence)?;
    let settlement_id = format!("syno-set-{}", &canonical_sha256(&settlement)?[..48]);
    ledger.intents[index].settlement_id = Some(settlement_id.clone());
    ledger.intents[index].settlement = Some(settlement.clone());
    write(path, &ledger)?;
    Ok((settlement_id, settlement, false))
}

pub fn verify_projection_settlement(
    path: impl AsRef<Path>,
    settlement_id: &str,
) -> Result<SynoptikonProjectionIntentRecord, DaemonServiceRuntimeError> {
    let record = projection_intent_by_settlement(path, settlement_id)?;
    if !record.uploaded || record.settlement.is_none() {
        return Err(invalid("projection is not terminal"));
    }
    Ok(record)
}

fn projection_intent_by_settlement(
    path: impl AsRef<Path>,
    settlement_id: &str,
) -> Result<SynoptikonProjectionIntentRecord, DaemonServiceRuntimeError> {
    let _guard = lock()?;
    read(path.as_ref())?
        .intents
        .into_iter()
        .find(|item| item.settlement_id.as_deref() == Some(settlement_id))
        .ok_or_else(|| invalid("projection settlement is unavailable"))
}

fn lock() -> Result<std::sync::MutexGuard<'static, ()>, DaemonServiceRuntimeError> {
    LEDGER_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| invalid("projection ledger lock poisoned"))
}

fn read(path: &Path) -> Result<Ledger, DaemonServiceRuntimeError> {
    let parent = open_parent(path, false)?;
    let name = file_name(path)?;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if fd < 0 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::NotFound {
            return Ok(Ledger::default());
        }
        return Err(io_error(path, error));
    }
    let file = unsafe { File::from_raw_fd(fd) };
    validate_regular_file(path, &file)?;
    let ledger: Ledger = serde_json::from_reader(file)
        .map_err(|error| invalid(format!("projection ledger JSON is invalid: {error}")))?;
    if ledger.schema_version != SYNOPTIKON_PROJECTION_LEDGER_SCHEMA {
        return Err(invalid("projection ledger schema is unsupported"));
    }
    let mut identities = BTreeSet::new();
    let mut nonces = BTreeSet::new();
    for record in &ledger.intents {
        if !identities.insert((
            record.projection.projection_id.as_str(),
            record.projection.generation,
        )) || !nonces.insert(record.projection.nonce.as_str())
            || record.settlement_id.is_some() != record.settlement.is_some()
        {
            return Err(invalid("projection ledger contains conflicting records"));
        }
    }
    Ok(ledger)
}

fn write(path: &Path, ledger: &Ledger) -> Result<(), DaemonServiceRuntimeError> {
    let parent = open_parent(path, true)?;
    let name = file_name(path)?;
    let temporary_name = CString::new(format!(
        ".synoptikon-projection.tmp-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|v| v.as_nanos())
            .unwrap_or(0)
    ))
    .map_err(|_| invalid("projection temporary name is invalid"))?;
    let bytes = serde_json::to_vec_pretty(ledger)
        .map_err(|error| invalid(format!("projection ledger encode failed: {error}")))?;
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            temporary_name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_CLOEXEC | libc::O_NOFOLLOW,
            0o600,
        )
    };
    if fd < 0 {
        return Err(io_error(path, io::Error::last_os_error()));
    }
    let mut file = unsafe { File::from_raw_fd(fd) };
    if unsafe { libc::fchmod(file.as_raw_fd(), 0o600) } != 0 {
        return Err(io_error(path, io::Error::last_os_error()));
    }
    validate_regular_file(path, &file)?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| io_error(path, error))?;
    drop(file);
    if unsafe {
        libc::renameat(
            parent.as_raw_fd(),
            temporary_name.as_ptr(),
            parent.as_raw_fd(),
            name.as_ptr(),
        )
    } != 0
    {
        unsafe { libc::unlinkat(parent.as_raw_fd(), temporary_name.as_ptr(), 0) };
        return Err(io_error(path, io::Error::last_os_error()));
    }
    parent.sync_all().map_err(|error| io_error(path, error))
}

fn open_parent(path: &Path, create: bool) -> Result<File, DaemonServiceRuntimeError> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid("projection ledger has no parent"))?;
    if create && !parent.exists() {
        std::fs::DirBuilder::new()
            .recursive(false)
            .mode(0o700)
            .create(parent)
            .map_err(|error| io_error(parent, error))?;
    }
    let parent_c = CString::new(parent.as_os_str().as_bytes())
        .map_err(|_| invalid("projection ledger parent path is invalid"))?;
    let fd = unsafe {
        libc::open(
            parent_c.as_ptr(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_DIRECTORY,
        )
    };
    if fd < 0 {
        let error = io::Error::last_os_error();
        if !create && error.kind() == io::ErrorKind::NotFound {
            return Err(invalid("projection authority directory is unavailable"));
        }
        return Err(io_error(parent, error));
    }
    let directory = unsafe { File::from_raw_fd(fd) };
    let metadata = directory
        .metadata()
        .map_err(|error| io_error(parent, error))?;
    if !metadata.is_dir()
        || metadata.mode() & 0o7777 != 0o700
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.gid() != unsafe { libc::getegid() }
        || metadata.nlink() < 2
    {
        return Err(invalid(
            "projection authority directory metadata is invalid",
        ));
    }
    Ok(directory)
}

fn file_name(path: &Path) -> Result<CString, DaemonServiceRuntimeError> {
    let name = path
        .file_name()
        .ok_or_else(|| invalid("projection ledger file name is unavailable"))?;
    if name.as_bytes().contains(&b'/') {
        return Err(invalid("projection ledger file name is invalid"));
    }
    CString::new(name.as_bytes()).map_err(|_| invalid("projection ledger file name is invalid"))
}

fn validate_regular_file(path: &Path, file: &File) -> Result<(), DaemonServiceRuntimeError> {
    let metadata = file.metadata().map_err(|error| io_error(path, error))?;
    if !metadata.is_file()
        || metadata.mode() & 0o7777 != 0o600
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.gid() != unsafe { libc::getegid() }
        || metadata.nlink() != 1
    {
        return Err(invalid("projection ledger metadata is invalid"));
    }
    Ok(())
}

fn canonical_sha256(value: &impl Serialize) -> Result<String, DaemonServiceRuntimeError> {
    let bytes = serde_jcs::to_vec(value)
        .map_err(|error| invalid(format!("canonical projection encode failed: {error}")))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn invalid(message: impl Into<String>) -> DaemonServiceRuntimeError {
    DaemonServiceRuntimeError::UnsupportedOperation {
        operation: message.into(),
    }
}

fn io_error(path: &Path, error: io::Error) -> DaemonServiceRuntimeError {
    invalid(format!("projection ledger I/O {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dasobjectstore_core::SynoptikonProjectionDispositionV1;
    use std::os::unix::fs::{symlink, PermissionsExt};

    const NOW: u64 = 1_787_040_000;

    fn fixture() -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "das-synoptikon-ledger-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let authority = root.join("projection-authority");
        std::fs::create_dir_all(&authority).unwrap();
        std::fs::set_permissions(&authority, std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = authority.join(SYNOPTIKON_PROJECTION_LEDGER_FILE_NAME);
        (root, path)
    }

    fn prepare(path: &Path) -> SynoptikonProjectionIntentRecord {
        prepare_synoptikon_projection_intent(path, "demo/input.bin", 4, &"ab".repeat(32), NOW)
            .expect("prepare")
            .0
    }

    fn settlement(
        request: &SynoptikonProjectionRequestV1,
        sequence: u64,
    ) -> SynoptikonProjectionSettlementV1 {
        SynoptikonProjectionSettlementV1 {
            schema_version: dasobjectstore_core::SYNOPTIKON_PROJECTION_SETTLEMENT_V1_SCHEMA
                .to_owned(),
            projection_id: request.projection_id.clone(),
            request_sha256: "01".repeat(32),
            readiness_sha256: "02".repeat(32),
            generation: request.generation,
            source_sha256: request.source_sha256.clone(),
            object_store_id: request.object_store_id.clone(),
            object_id: request.object_id.clone(),
            object_version: request.object_version,
            nonce: request.nonce.clone(),
            authority_sequence: sequence,
            upload_completion_receipt_sha256: "03".repeat(32),
            catalogue_snapshot_sha256: "04".repeat(32),
            provider_group_status_sha256: "05".repeat(32),
            hdd_settlement_reference_sha256: "06".repeat(32),
            hdd_replica_count: 1,
            disposition: SynoptikonProjectionDispositionV1::HddSettled,
            settled_at_unix_seconds: NOW,
        }
    }

    #[test]
    fn prepare_is_owner_derived_bounded_and_restart_idempotent() {
        let (root, path) = fixture();
        let first = prepare(&path);
        assert_eq!(first.projection.object_store_id, "synoptikon-demo");
        assert_eq!(first.projection.object_key, "demo/input.bin");
        assert_eq!(first.projection.object_version, 1);
        let replay = prepare_synoptikon_projection_intent(
            &path,
            "demo/input.bin",
            4,
            &"ab".repeat(32),
            NOW + 1,
        )
        .expect("replay");
        assert!(replay.1);
        assert_eq!(replay.0, first);
        let next = prepare_synoptikon_projection_intent(
            &path,
            "demo/input.bin",
            5,
            &"cd".repeat(32),
            NOW + 1,
        )
        .expect("next generation");
        assert!(!next.1);
        assert_eq!(next.0.projection.object_version, 2);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn authority_sequence_is_persisted_before_readiness_publication() {
        let (root, path) = fixture();
        let intent = prepare(&path);
        mark_projection_uploaded(&path, &intent.intent_id).unwrap();
        let denied = commit_projection_settlement(&path, &intent.intent_id, |_, sequence| {
            assert_eq!(sequence, 1);
            Err(invalid("readiness denied"))
        });
        assert!(denied.is_err());
        let committed =
            commit_projection_settlement(&path, &intent.intent_id, |request, sequence| {
                assert_eq!(sequence, 2);
                Ok(settlement(request, sequence))
            })
            .expect("settlement");
        assert_eq!(committed.1.authority_sequence, 2);
        let replay = commit_projection_settlement(&path, &intent.intent_id, |_, _| {
            panic!("exact replay must not rebuild readiness")
        })
        .expect("replay");
        assert!(replay.2);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ledger_rejects_symlink_hardlink_mode_and_parent_substitution() {
        let (root, path) = fixture();
        prepare(&path);

        let hardlink = root.join("ledger-hardlink.json");
        std::fs::hard_link(&path, &hardlink).unwrap();
        assert!(projection_intent(&path, "missing").is_err());
        std::fs::remove_file(&hardlink).unwrap();

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o4600)).unwrap();
        assert!(projection_intent(&path, "missing").is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();

        let target = root.join("target.json");
        std::fs::rename(&path, &target).unwrap();
        symlink(&target, &path).unwrap();
        assert!(projection_intent(&path, "missing").is_err());
        std::fs::remove_file(&path).unwrap();

        let real_parent = root.join("real-parent");
        std::fs::create_dir(&real_parent).unwrap();
        std::fs::set_permissions(&real_parent, std::fs::Permissions::from_mode(0o700)).unwrap();
        let substituted_parent = root.join("substituted-parent");
        symlink(&real_parent, &substituted_parent).unwrap();
        assert!(prepare_synoptikon_projection_intent(
            substituted_parent.join(SYNOPTIKON_PROJECTION_LEDGER_FILE_NAME),
            "demo/input.bin",
            4,
            &"ab".repeat(32),
            NOW,
        )
        .is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
