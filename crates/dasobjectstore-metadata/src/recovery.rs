//! Safe filesystem-to-live-metadata recovery for payloads left by interrupted ingest.

use crate::schema::LIVE_SCHEMA_SQL;
use crate::DiskCopyRoot;
use dasobjectstore_core::ids::StoreId;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{BufReader, Read};
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryStoreDefinition {
    pub store_id: StoreId,
    pub class: String,
    pub policy_json: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoverLiveMetadataRequest {
    pub live_sqlite_path: PathBuf,
    pub store_definitions: Vec<RecoveryStoreDefinition>,
    pub disk_roots: Vec<DiskCopyRoot>,
    pub store_id: Option<StoreId>,
    pub dry_run: bool,
    pub recorded_at_utc: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecoverLiveMetadataReport {
    pub metadata_path: PathBuf,
    pub backup_path: Option<PathBuf>,
    pub dry_run: bool,
    pub stores_scanned: usize,
    pub payload_files: u64,
    pub objects_recovered: u64,
    pub placements_recovered: u64,
    pub payload_bytes: u64,
    pub partial_duplicates_omitted: u64,
    pub hashes_verified: bool,
    pub warning: String,
}

pub fn recover_live_metadata(
    request: &RecoverLiveMetadataRequest,
) -> Result<RecoverLiveMetadataReport, RecoverLiveMetadataError> {
    let selected: HashMap<_, _> = request
        .store_definitions
        .iter()
        .filter(|definition| {
            request
                .store_id
                .as_ref()
                .is_none_or(|store_id| store_id == &definition.store_id)
        })
        .map(|definition| (definition.store_id.as_str(), definition))
        .collect();
    let mut groups: BTreeMap<String, Vec<Payload>> = BTreeMap::new();
    let mut payload_files = 0_u64;
    let mut payload_bytes = 0_u64;
    for root in &request.disk_roots {
        let objects_root = root.root_path.join("objects");
        if !objects_root.is_dir() {
            continue;
        }
        for prefix in fs::read_dir(objects_root)? {
            let prefix = prefix?.path();
            if !prefix.is_dir() {
                continue;
            }
            for object_dir in fs::read_dir(prefix)? {
                let object_dir = object_dir?.path();
                let Some(encoded) = object_dir.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                let object_id = percent_decode(encoded)?;
                let store_id = object_id.split('/').next().unwrap_or_default();
                if !selected.contains_key(store_id) {
                    continue;
                }
                let payload_path = object_dir.join("payload");
                if !payload_path.is_file() {
                    continue;
                }
                let size_bytes = payload_path.metadata()?.len();
                payload_files = payload_files.saturating_add(1);
                payload_bytes = payload_bytes.saturating_add(size_bytes);
                groups.entry(object_id.clone()).or_default().push(Payload {
                    object_id,
                    disk_id: root.disk_id.as_str().to_string(),
                    absolute_path: payload_path.clone(),
                    relative_path: payload_path
                        .strip_prefix(&root.root_path)
                        .map_err(|_| {
                            RecoverLiveMetadataError::InvalidPayloadPath(payload_path.clone())
                        })?
                        .to_string_lossy()
                        .replace(std::path::MAIN_SEPARATOR, "/"),
                    size_bytes,
                });
            }
        }
    }
    let mut objects_recovered = 0_u64;
    let mut placements_recovered = 0_u64;
    let mut partial_duplicates_omitted = 0_u64;
    for rows in groups.values() {
        if rows.is_empty() {
            continue;
        }
        let max_size = rows.iter().map(|row| row.size_bytes).max().unwrap_or(0);
        objects_recovered += 1;
        placements_recovered += rows.iter().filter(|row| row.size_bytes == max_size).count() as u64;
        partial_duplicates_omitted +=
            rows.iter().filter(|row| row.size_bytes != max_size).count() as u64;
    }
    let stores_scanned = selected.len();
    let mut report = RecoverLiveMetadataReport {
        metadata_path: request.live_sqlite_path.clone(),
        backup_path: None,
        dry_run: request.dry_run,
        stores_scanned,
        payload_files,
        objects_recovered,
        placements_recovered,
        payload_bytes,
        partial_duplicates_omitted,
        hashes_verified: false,
        warning: "recovered placements are size-selected and remain unverified; content hashes must be computed before export or protection".to_string(),
    };
    if request.dry_run {
        return Ok(report);
    }
    if request.store_id.is_some() {
        return apply_filtered_recovery(request, &selected, &groups, report);
    }

    let temporary_path = request.live_sqlite_path.with_extension("sqlite.repairing");
    let _ = fs::remove_file(&temporary_path);
    let mut connection = Connection::open(&temporary_path)?;
    connection.execute_batch(LIVE_SCHEMA_SQL)?;
    let transaction = connection.transaction()?;
    transaction.execute(
        "INSERT INTO pools VALUES ('pool-recovered', 'Degraded', ?1, ?1)",
        [&request.recorded_at_utc],
    )?;
    for root in &request.disk_roots {
        transaction.execute(
            "INSERT INTO disks (disk_id, pool_id, role, state, created_at_utc, updated_at_utc)
             VALUES (?1, 'pool-recovered', 'hdd', 'Watch', ?2, ?2)",
            params![root.disk_id.as_str(), request.recorded_at_utc],
        )?;
    }
    for definition in selected.values() {
        transaction.execute(
            "INSERT INTO stores (store_id, pool_id, class, policy_json, created_at_utc, updated_at_utc)
             VALUES (?1, 'pool-recovered', ?2, ?3, ?4, ?4)",
            params![
                definition.store_id.as_str(),
                definition.class,
                definition.policy_json,
                request.recorded_at_utc,
            ],
        )?;
    }
    let mut placement_index = 0_u64;
    for (object_id, rows) in groups {
        let Some(definition) = selected.get(object_id.split('/').next().unwrap_or_default()) else {
            continue;
        };
        let max_size = rows.iter().map(|row| row.size_bytes).max().unwrap_or(0);
        transaction.execute(
            "INSERT INTO objects (object_id, store_id, object_type, state, size_bytes, content_hash, created_at_utc, updated_at_utc)
             VALUES (?1, ?2, 'naive', 'CopyingToHdd', ?3, NULL, ?4, ?4)",
            params![object_id, definition.store_id.as_str(), i64::try_from(max_size).unwrap_or(i64::MAX), request.recorded_at_utc],
        )?;
        for row in rows.into_iter().filter(|row| row.size_bytes == max_size) {
            placement_index += 1;
            transaction.execute(
                "INSERT INTO placements (placement_id, object_id, disk_id, relative_path, content_hash, verified_at_utc, created_at_utc)
                 VALUES (?1, ?2, ?3, ?4, NULL, NULL, ?5)",
                params![
                    placement_id(placement_index, &row.object_id, &row.disk_id),
                    row.object_id,
                    row.disk_id,
                    row.relative_path,
                    request.recorded_at_utc,
                ],
            )?;
        }
    }
    transaction.commit()?;
    let integrity =
        connection.query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))?;
    if integrity != "ok" {
        let _ = fs::remove_file(&temporary_path);
        return Err(RecoverLiveMetadataError::Integrity(integrity));
    }
    drop(connection);
    let backup_path = request.live_sqlite_path.with_extension(format!(
        "sqlite.pre-repair-{}",
        request.recorded_at_utc.replace(':', "-")
    ));
    if request.live_sqlite_path.exists() {
        fs::rename(&request.live_sqlite_path, &backup_path)?;
        report.backup_path = Some(backup_path);
    }
    fs::rename(temporary_path, &request.live_sqlite_path)?;
    Ok(report)
}

fn apply_filtered_recovery(
    request: &RecoverLiveMetadataRequest,
    selected: &HashMap<&str, &RecoveryStoreDefinition>,
    groups: &BTreeMap<String, Vec<Payload>>,
    mut report: RecoverLiveMetadataReport,
) -> Result<RecoverLiveMetadataReport, RecoverLiveMetadataError> {
    let store_id = request
        .store_id
        .as_ref()
        .expect("filtered recovery requires store id");
    let definition = selected
        .get(store_id.as_str())
        .ok_or_else(|| RecoverLiveMetadataError::UnknownStore(store_id.as_str().to_string()))?;

    let mut verified = Vec::with_capacity(groups.len());
    for (object_id, rows) in groups {
        let max_size = rows.iter().map(|row| row.size_bytes).max().unwrap_or(0);
        let selected_rows = rows
            .iter()
            .filter(|row| row.size_bytes == max_size)
            .collect::<Vec<_>>();
        let mut expected_hash = None::<String>;
        for row in &selected_rows {
            let hash = sha256_file(&row.absolute_path)?;
            if expected_hash
                .as_ref()
                .is_some_and(|expected| expected != &hash)
            {
                return Err(RecoverLiveMetadataError::PlacementContentMismatch {
                    object_id: object_id.clone(),
                    size_bytes: max_size,
                });
            }
            expected_hash = Some(hash);
        }
        let content_hash = expected_hash
            .ok_or_else(|| RecoverLiveMetadataError::MissingSelectedPayload(object_id.clone()))?;
        verified.push((object_id, max_size, content_hash, selected_rows));
    }

    let mut connection = Connection::open(&request.live_sqlite_path)?;
    connection.execute_batch("PRAGMA foreign_keys = ON;")?;
    let backup_path = request.live_sqlite_path.with_extension(format!(
        "sqlite.pre-store-repair-{}-{}",
        store_id.as_str(),
        request.recorded_at_utc.replace(':', "-")
    ));
    if backup_path.exists() {
        return Err(RecoverLiveMetadataError::BackupAlreadyExists(backup_path));
    }
    connection.execute("VACUUM INTO ?1", [backup_path.to_string_lossy().as_ref()])?;
    report.backup_path = Some(backup_path);

    let transaction = connection.transaction()?;
    let registered_store = transaction
        .query_row(
            "SELECT store_id FROM stores WHERE store_id=?1",
            [store_id.as_str()],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if registered_store.is_none() {
        return Err(RecoverLiveMetadataError::UnknownStore(
            store_id.as_str().to_string(),
        ));
    }
    for (object_id, size_bytes, content_hash, rows) in verified {
        let existing_store = transaction
            .query_row(
                "SELECT store_id FROM objects WHERE object_id=?1",
                [object_id.as_str()],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if existing_store
            .as_deref()
            .is_some_and(|existing| existing != definition.store_id.as_str())
        {
            return Err(RecoverLiveMetadataError::ObjectStoreMismatch {
                object_id: object_id.clone(),
                expected_store: definition.store_id.as_str().to_string(),
                actual_store: existing_store.expect("checked existing store"),
            });
        }
        transaction.execute(
            "INSERT INTO objects (object_id,store_id,object_type,state,size_bytes,content_hash,created_at_utc,updated_at_utc)
             VALUES (?1,?2,'naive','HddCopyVerified',?3,?4,?5,?5)
             ON CONFLICT(object_id) DO UPDATE SET state='HddCopyVerified',size_bytes=excluded.size_bytes,content_hash=excluded.content_hash,updated_at_utc=excluded.updated_at_utc",
            params![object_id, definition.store_id.as_str(), i64::try_from(size_bytes).unwrap_or(i64::MAX), content_hash, request.recorded_at_utc],
        )?;
        for row in rows {
            let placement = transaction
                .query_row(
                    "SELECT placement_id FROM placements WHERE object_id=?1 AND disk_id=?2 AND relative_path=?3 LIMIT 1",
                    params![object_id, row.disk_id, row.relative_path],
                    |result| result.get::<_, String>(0),
                )
                .optional()?;
            if let Some(placement_id) = placement {
                transaction.execute(
                    "UPDATE placements SET content_hash=?1,verified_at_utc=?2 WHERE placement_id=?3",
                    params![content_hash, request.recorded_at_utc, placement_id],
                )?;
            } else {
                transaction.execute(
                    "INSERT INTO placements (placement_id,object_id,disk_id,relative_path,content_hash,verified_at_utc,created_at_utc) VALUES (?1,?2,?3,?4,?5,?6,?6)",
                    params![stable_placement_id(object_id, &row.disk_id, &row.relative_path), object_id, row.disk_id, row.relative_path, content_hash, request.recorded_at_utc],
                )?;
            }
        }
    }
    transaction.commit()?;
    report.hashes_verified = true;
    report.warning = if report.partial_duplicates_omitted == 0 {
        "store-scoped recovery verified every selected HDD placement by SHA-256 and preserved unrelated live metadata".to_string()
    } else {
        format!(
            "store-scoped recovery verified every selected HDD placement by SHA-256; {} smaller partial duplicate(s) remain excluded for explicit quarantine",
            report.partial_duplicates_omitted
        )
    };
    Ok(report)
}

fn sha256_file(path: &std::path::Path) -> Result<String, RecoverLiveMetadataError> {
    let mut reader = BufReader::with_capacity(8 * 1024 * 1024, fs::File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 8 * 1024 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn stable_placement_id(object_id: &str, disk_id: &str, relative_path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(object_id.as_bytes());
    hasher.update([0]);
    hasher.update(disk_id.as_bytes());
    hasher.update([0]);
    hasher.update(relative_path.as_bytes());
    let digest = hasher.finalize();
    let mut value = String::with_capacity(16);
    for byte in digest.iter().take(8) {
        value.push_str(&format!("{byte:02x}"));
    }
    format!("repair-{value}")
}

#[derive(Clone, Debug)]
struct Payload {
    object_id: String,
    disk_id: String,
    relative_path: String,
    absolute_path: PathBuf,
    size_bytes: u64,
}

fn placement_id(index: u64, object_id: &str, disk_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(index.to_le_bytes());
    hasher.update(object_id.as_bytes());
    hasher.update(disk_id.as_bytes());
    let mut value = String::with_capacity(16);
    for byte in hasher.finalize().iter().take(8) {
        value.push_str(&format!("{byte:02x}"));
    }
    format!("repair-{value}")
}

fn percent_decode(value: &str) -> Result<String, RecoverLiveMetadataError> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(RecoverLiveMetadataError::InvalidEncodedObject(
                    value.to_string(),
                ));
            }
            let high = hex_digit(bytes[index + 1])?;
            let low = hex_digit(bytes[index + 2])?;
            output.push((high << 4) | low);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(output)
        .map_err(|_| RecoverLiveMetadataError::InvalidEncodedObject(value.to_string()))
}

fn hex_digit(value: u8) -> Result<u8, RecoverLiveMetadataError> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(RecoverLiveMetadataError::InvalidEncodedObject(
            value.to_string(),
        )),
    }
}

#[derive(Debug)]
pub enum RecoverLiveMetadataError {
    Io(std::io::Error),
    Sqlite(rusqlite::Error),
    InvalidEncodedObject(String),
    InvalidPayloadPath(PathBuf),
    UnknownStore(String),
    MissingSelectedPayload(String),
    PlacementContentMismatch {
        object_id: String,
        size_bytes: u64,
    },
    ObjectStoreMismatch {
        object_id: String,
        expected_store: String,
        actual_store: String,
    },
    BackupAlreadyExists(PathBuf),
    Integrity(String),
}

impl std::fmt::Display for RecoverLiveMetadataError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "metadata recovery filesystem error: {error}"),
            Self::Sqlite(error) => write!(formatter, "metadata recovery SQLite error: {error}"),
            Self::InvalidEncodedObject(value) => {
                write!(formatter, "invalid encoded object path {value}")
            }
            Self::InvalidPayloadPath(path) => {
                write!(formatter, "invalid payload path {}", path.display())
            }
            Self::UnknownStore(store_id) => write!(formatter, "store-scoped recovery requires registered store {store_id}"),
            Self::MissingSelectedPayload(object_id) => write!(formatter, "store-scoped recovery found no complete payload for {object_id}"),
            Self::PlacementContentMismatch { object_id, size_bytes } => write!(formatter, "store-scoped recovery hard fail: same-size HDD placements disagree for {object_id} ({size_bytes} bytes)"),
            Self::ObjectStoreMismatch { object_id, expected_store, actual_store } => write!(formatter, "store-scoped recovery refused {object_id}: expected store {expected_store}, live metadata names {actual_store}"),
            Self::BackupAlreadyExists(path) => write!(formatter, "store-scoped recovery backup already exists: {}", path.display()),
            Self::Integrity(value) => write!(
                formatter,
                "recovered metadata integrity check failed: {value}"
            ),
        }
    }
}

impl std::error::Error for RecoverLiveMetadataError {}
impl From<std::io::Error> for RecoverLiveMetadataError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<rusqlite::Error> for RecoverLiveMetadataError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

#[cfg(test)]
mod tests {
    use super::{recover_live_metadata, RecoverLiveMetadataRequest, RecoveryStoreDefinition};
    use crate::DiskCopyRoot;
    use dasobjectstore_core::ids::{DiskId, StoreId};
    use std::fs;

    #[test]
    fn dry_run_and_apply_recover_size_selected_payloads() {
        let root =
            std::env::temp_dir().join(format!("dasobjectstore-recovery-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let disk = root.join("disk-a");
        let payload = disk.join("objects/aa/store-a%2Ffile.bin/payload");
        fs::create_dir_all(payload.parent().expect("payload parent")).expect("payload parent");
        fs::write(&payload, b"payload").expect("payload");
        let live = root.join("live.sqlite");
        let request = || RecoverLiveMetadataRequest {
            live_sqlite_path: live.clone(),
            store_definitions: vec![RecoveryStoreDefinition {
                store_id: StoreId::new("store-a").expect("store id"),
                class: "generated_data".to_string(),
                policy_json: "{}".to_string(),
            }],
            disk_roots: vec![DiskCopyRoot::new(
                DiskId::new("disk-a").expect("disk id"),
                &disk,
            )],
            store_id: None,
            dry_run: true,
            recorded_at_utc: "2026-07-10T00:00:00Z".to_string(),
        };
        let report = recover_live_metadata(&request()).expect("dry run");
        assert_eq!(report.payload_files, 1);
        assert_eq!(report.objects_recovered, 1);
        let mut apply = request();
        apply.dry_run = false;
        let report = recover_live_metadata(&apply).expect("apply");
        assert!(report.backup_path.is_none());
        assert!(live.is_file());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn filtered_apply_verifies_target_and_preserves_other_store_metadata() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-filtered-repair-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let disk = root.join("disk-a");
        let payload = disk.join("objects/aa/store-a%2Ffile.bin/payload");
        fs::create_dir_all(payload.parent().expect("payload parent")).expect("payload parent");
        fs::write(&payload, b"payload").expect("payload");
        let live = root.join("live.sqlite");
        let connection = rusqlite::Connection::open(&live).expect("live catalogue");
        connection
            .execute_batch(crate::LIVE_SCHEMA_SQL)
            .expect("schema");
        connection
            .execute_batch(
                "INSERT INTO pools VALUES ('pool-a','Healthy','now','now');
                 INSERT INTO disks (disk_id,pool_id,role,state,created_at_utc,updated_at_utc) VALUES ('disk-a','pool-a','hdd','Healthy','now','now');
                 INSERT INTO stores VALUES ('store-a','pool-a','GeneratedData','{}','now','now');
                 INSERT INTO stores VALUES ('store-b','pool-a','GeneratedData','{}','now','now');
                 INSERT INTO objects VALUES ('store-b/keep.bin','store-b','naive','HddCopyVerified',4,'keep-hash','now','now');",
            )
            .expect("fixtures");
        drop(connection);
        let request = RecoverLiveMetadataRequest {
            live_sqlite_path: live.clone(),
            store_definitions: vec![RecoveryStoreDefinition {
                store_id: StoreId::new("store-a").expect("store id"),
                class: "generated_data".to_string(),
                policy_json: "{}".to_string(),
            }],
            disk_roots: vec![DiskCopyRoot::new(
                DiskId::new("disk-a").expect("disk id"),
                &disk,
            )],
            store_id: Some(StoreId::new("store-a").expect("store id")),
            dry_run: false,
            recorded_at_utc: "2026-07-10T00:00:00Z".to_string(),
        };
        let report = recover_live_metadata(&request).expect("filtered repair");
        assert!(report.hashes_verified);
        assert!(report.backup_path.expect("backup").is_file());
        let connection = rusqlite::Connection::open(&live).expect("live catalogue");
        let recovered: (String, String) = connection
            .query_row(
                "SELECT state,content_hash FROM objects WHERE object_id='store-a/file.bin'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("recovered object");
        assert_eq!(recovered.0, "HddCopyVerified");
        assert_eq!(recovered.1.len(), 64);
        let retained: String = connection
            .query_row(
                "SELECT content_hash FROM objects WHERE object_id='store-b/keep.bin'",
                [],
                |row| row.get(0),
            )
            .expect("unrelated object");
        assert_eq!(retained, "keep-hash");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn filtered_apply_hard_fails_same_size_content_disagreement() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-filtered-mismatch-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let disk_a = root.join("disk-a");
        let disk_b = root.join("disk-b");
        for (disk, payload) in [(&disk_a, b"payload-a"), (&disk_b, b"payload-b")] {
            let path = disk.join("objects/aa/store-a%2Ffile.bin/payload");
            fs::create_dir_all(path.parent().expect("payload parent")).expect("payload parent");
            fs::write(path, payload).expect("payload");
        }
        let live = root.join("live.sqlite");
        let connection = rusqlite::Connection::open(&live).expect("live catalogue");
        connection
            .execute_batch(crate::LIVE_SCHEMA_SQL)
            .expect("schema");
        connection
            .execute_batch(
                "INSERT INTO pools VALUES ('pool-a','Healthy','now','now');
                 INSERT INTO disks (disk_id,pool_id,role,state,created_at_utc,updated_at_utc) VALUES ('disk-a','pool-a','hdd','Healthy','now','now');
                 INSERT INTO disks (disk_id,pool_id,role,state,created_at_utc,updated_at_utc) VALUES ('disk-b','pool-a','hdd','Healthy','now','now');
                 INSERT INTO stores VALUES ('store-a','pool-a','GeneratedData','{}','now','now');",
            )
            .expect("fixtures");
        drop(connection);
        let request = RecoverLiveMetadataRequest {
            live_sqlite_path: live,
            store_definitions: vec![RecoveryStoreDefinition {
                store_id: StoreId::new("store-a").expect("store id"),
                class: "generated_data".to_string(),
                policy_json: "{}".to_string(),
            }],
            disk_roots: vec![
                DiskCopyRoot::new(DiskId::new("disk-a").expect("disk id"), &disk_a),
                DiskCopyRoot::new(DiskId::new("disk-b").expect("disk id"), &disk_b),
            ],
            store_id: Some(StoreId::new("store-a").expect("store id")),
            dry_run: false,
            recorded_at_utc: "2026-07-10T00:00:00Z".to_string(),
        };
        assert!(matches!(
            recover_live_metadata(&request),
            Err(super::RecoverLiveMetadataError::PlacementContentMismatch { .. })
        ));
        let _ = fs::remove_dir_all(root);
    }
}
