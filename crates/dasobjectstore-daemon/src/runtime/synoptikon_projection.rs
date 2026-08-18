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
    #[serde(default)]
    pub upload_admitted: bool,
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
            && (item.settlement.is_some() || item.projection.expires_at_unix_seconds > now)
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
        upload_admitted: false,
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
    if !record.upload_admitted {
        return Err(invalid("projection upload was not durably admitted"));
    }
    record.uploaded = true;
    write(path, &ledger)
}

pub fn mark_projection_upload_admitted(
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
    if record.uploaded || record.settlement.is_some() {
        return Err(invalid("projection intent is already terminal"));
    }
    record.upload_admitted = true;
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

pub fn projection_authority_record(
    path: impl AsRef<Path>,
    authority_id: &str,
) -> Result<SynoptikonProjectionIntentRecord, DaemonServiceRuntimeError> {
    let _guard = lock()?;
    read(path.as_ref())?
        .intents
        .into_iter()
        .find(|item| {
            item.intent_id == authority_id || item.settlement_id.as_deref() == Some(authority_id)
        })
        .ok_or_else(|| invalid("projection authority is unavailable"))
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
    let mut authority_sequences = BTreeSet::new();
    for record in &ledger.intents {
        validate_synoptikon_projection_request(
            &record.projection,
            record.projection.requested_at_unix_seconds,
        )
        .map_err(|error| invalid(format!("projection ledger request is invalid: {error}")))?;
        let expected_intent = format!(
            "syno-{}",
            &canonical_sha256(&(
                record.projection.object_key.as_str(),
                record.projection.source_size_bytes,
                record.projection.source_sha256.as_str(),
                record.projection.object_version,
            ))?[..48]
        );
        let expected_nonce = canonical_sha256(&(
            expected_intent.as_str(),
            record.projection.source_sha256.as_str(),
            record.projection.requested_at_unix_seconds,
        ))?;
        if !identities.insert((
            record.projection.projection_id.as_str(),
            record.projection.generation,
        )) || !nonces.insert(record.projection.nonce.as_str())
            || record.settlement_id.is_some() != record.settlement.is_some()
            || record.intent_id != expected_intent
            || record.projection.projection_id != record.intent_id
            || record.projection.object_id
                != format!(
                    "{}/{}",
                    record.projection.object_store_id, record.projection.object_key
                )
            || record.projection.generation != record.projection.object_version
            || record.projection.nonce != expected_nonce
            || (record.uploaded && !record.upload_admitted)
        {
            return Err(invalid("projection ledger contains conflicting records"));
        }
        if let Some(settlement) = &record.settlement {
            let expected_settlement_id =
                format!("syno-set-{}", &canonical_sha256(settlement)?[..48]);
            if record.settlement_id.as_deref() != Some(expected_settlement_id.as_str())
                || settlement.schema_version
                    != dasobjectstore_core::SYNOPTIKON_PROJECTION_SETTLEMENT_V1_SCHEMA
                || settlement.projection_id != record.projection.projection_id
                || settlement.request_sha256 != canonical_sha256(&record.projection)?
                || settlement.generation != record.projection.generation
                || settlement.source_sha256 != record.projection.source_sha256
                || settlement.object_store_id != record.projection.object_store_id
                || settlement.object_id != record.projection.object_id
                || settlement.object_version != record.projection.object_version
                || settlement.nonce != record.projection.nonce
                || settlement.authority_sequence == 0
                || settlement.authority_sequence > ledger.authority_sequence
                || !authority_sequences.insert(settlement.authority_sequence)
                || !record.uploaded
                || settlement.hdd_replica_count == 0
                || settlement.disposition
                    != dasobjectstore_core::SynoptikonProjectionDispositionV1::HddSettled
            {
                return Err(invalid("projection ledger settlement is not cross-bound"));
            }
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
            request_sha256: canonical_sha256(request).unwrap(),
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

        let expired =
            prepare_synoptikon_projection_intent(&path, "expired.bin", 4, &"ef".repeat(32), NOW)
                .expect("expired intent")
                .0;
        let replacement = prepare_synoptikon_projection_intent(
            &path,
            "expired.bin",
            4,
            &"ef".repeat(32),
            expired.projection.expires_at_unix_seconds,
        )
        .expect("fresh replacement");
        assert!(!replacement.1);
        assert_eq!(replacement.0.projection.object_version, 2);
        assert_ne!(replacement.0.intent_id, expired.intent_id);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn authority_sequence_is_persisted_before_readiness_publication() {
        let (root, path) = fixture();
        let intent = prepare(&path);
        mark_projection_upload_admitted(&path, &intent.intent_id).unwrap();
        assert!(
            projection_intent(&path, &intent.intent_id)
                .expect("restart-visible admission")
                .upload_admitted
        );
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

    #[test]
    fn ledger_rejects_rewritten_identity_nonce_and_sequence_bindings() {
        for mutation in ["identity", "nonce", "sequence"] {
            let (root, path) = fixture();
            let intent = prepare(&path);
            mark_projection_upload_admitted(&path, &intent.intent_id).unwrap();
            mark_projection_uploaded(&path, &intent.intent_id).unwrap();
            commit_projection_settlement(&path, &intent.intent_id, |request, sequence| {
                Ok(settlement(request, sequence))
            })
            .unwrap();
            let mut value: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            match mutation {
                "identity" => {
                    value["intents"][0]["projection"]["object_id"] =
                        serde_json::json!("synoptikon-demo/substituted.bin")
                }
                "nonce" => {
                    value["intents"][0]["projection"]["nonce"] = serde_json::json!("99".repeat(32))
                }
                "sequence" => value["authority_sequence"] = serde_json::json!(0),
                _ => unreachable!(),
            }
            std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            assert!(
                projection_intent(&path, &intent.intent_id).is_err(),
                "{mutation}"
            );
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn exact_prepare_upload_settle_and_read_contract_uses_provider_digest_encoding() {
        use crate::api::{
            ProviderStreamChunkHeader, ProviderStreamCondition, ProviderStreamOpenRequest,
            ProviderStreamUploadOpenRequest, ProviderStreamVerifier, SynoptikonProjectionReadV1,
            SynoptikonProjectionUploadV1, PROVIDER_STREAM_SCHEMA_VERSION,
            SYNOPTIKON_PROJECTION_READ_V1_SCHEMA, SYNOPTIKON_PROJECTION_UPLOAD_V1_SCHEMA,
        };
        let (root, path) = fixture();
        let raw_sha = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        let provider_sha = format!("sha256:{raw_sha}");
        let intent = prepare_synoptikon_projection_intent(&path, "hello.txt", 5, raw_sha, NOW)
            .expect("prepare")
            .0;
        let upload = ProviderStreamUploadOpenRequest {
            schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_owned(),
            request_id: "projection-upload-1".to_owned(),
            upload_id: intent.intent_id.clone(),
            store_id: intent.projection.object_store_id.parse().unwrap(),
            object: dasobjectstore_core::backend::BackendObjectKey {
                object_id: intent.projection.object_key.clone(),
                version: intent.projection.object_version,
            },
            expected_size_bytes: 5,
            expected_sha256: provider_sha.clone(),
            chunk_size_bytes: 5,
            retained_dossier: None,
            synoptikon_projection: Some(SynoptikonProjectionUploadV1 {
                schema_version: SYNOPTIKON_PROJECTION_UPLOAD_V1_SCHEMA.to_owned(),
                intent_id: intent.intent_id.clone(),
            }),
        };
        upload.validate().expect("upload envelope");
        let mut verifier = ProviderStreamVerifier::new(upload.request_id.clone()).unwrap();
        verifier
            .push(
                &ProviderStreamChunkHeader {
                    schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_owned(),
                    request_id: upload.request_id.clone(),
                    offset: 0,
                    payload_len: 5,
                    final_chunk: false,
                    total_size: None,
                    sha256: None,
                },
                b"hello",
            )
            .unwrap();
        verifier
            .finish(
                &ProviderStreamChunkHeader {
                    schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_owned(),
                    request_id: upload.request_id.clone(),
                    offset: 5,
                    payload_len: 0,
                    final_chunk: true,
                    total_size: Some(5),
                    sha256: Some(provider_sha.clone()),
                },
                &[],
            )
            .unwrap();
        mark_projection_upload_admitted(&path, &intent.intent_id).unwrap();
        mark_projection_uploaded(&path, &intent.intent_id).unwrap();
        let (settlement_id, _, _) =
            commit_projection_settlement(&path, &intent.intent_id, |request, sequence| {
                Ok(settlement(request, sequence))
            })
            .expect("settlement");
        let read = ProviderStreamOpenRequest {
            schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_owned(),
            request_id: "projection-read-1".to_owned(),
            store_id: intent.projection.object_store_id.parse().unwrap(),
            object: dasobjectstore_core::backend::BackendObjectKey {
                object_id: intent.projection.object_key,
                version: intent.projection.object_version,
            },
            delegated_actor: None,
            verified_subject: None,
            application_capability: None,
            synoptikon_projection: Some(SynoptikonProjectionReadV1 {
                schema_version: SYNOPTIKON_PROJECTION_READ_V1_SCHEMA.to_owned(),
                settlement_id: settlement_id.clone(),
            }),
            range: None,
            condition: ProviderStreamCondition {
                if_match_sha256: Some(provider_sha),
                if_none_match_sha256: None,
            },
            chunk_size_bytes: 5,
        };
        read.validate().expect("read envelope");
        verify_projection_settlement(&path, &settlement_id).expect("terminal read authority");
        std::fs::remove_dir_all(root).unwrap();
    }
}
