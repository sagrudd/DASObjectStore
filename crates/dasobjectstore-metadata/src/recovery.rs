//! Safe filesystem-to-live-metadata recovery for payloads left by interrupted ingest.

use crate::schema::LIVE_SCHEMA_SQL;
use crate::DiskCopyRoot;
use dasobjectstore_core::ids::StoreId;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};
use std::fs;
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
    if request.store_id.is_some() && !request.dry_run {
        return Err(RecoverLiveMetadataError::FilteredApplyNotSupported);
    }
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

#[derive(Clone, Debug)]
struct Payload {
    object_id: String,
    disk_id: String,
    relative_path: String,
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
    FilteredApplyNotSupported,
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
            Self::FilteredApplyNotSupported => write!(
                formatter,
                "filtered repair apply is not supported; use dry-run for a single store or apply the complete registered store set"
            ),
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
    fn filtered_apply_is_rejected_to_preserve_other_store_metadata() {
        let request = RecoverLiveMetadataRequest {
            live_sqlite_path: std::env::temp_dir().join("dasobjectstore-filtered-repair.sqlite"),
            store_definitions: vec![],
            disk_roots: vec![],
            store_id: Some(StoreId::new("store-a").expect("store id")),
            dry_run: false,
            recorded_at_utc: "2026-07-10T00:00:00Z".to_string(),
        };
        assert!(matches!(
            recover_live_metadata(&request),
            Err(super::RecoverLiveMetadataError::FilteredApplyNotSupported)
        ));
    }
}
