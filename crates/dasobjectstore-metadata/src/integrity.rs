//! Read-only metadata/filesystem health checks and checksum-based cleanup.

use crate::{hash_file_sha256, DiskCopyRoot};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const MAX_FINDINGS: usize = 100;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifyLiveMetadataRequest {
    pub live_sqlite_path: PathBuf,
    pub disk_roots: Vec<DiskCopyRoot>,
    pub store_id: Option<String>,
    pub hash_payloads: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct VerifyLiveMetadataReport {
    pub metadata_path: PathBuf,
    pub stores_scanned: u64,
    pub objects_scanned: u64,
    pub placements_scanned: u64,
    pub payloads_checked: u64,
    pub payload_bytes_checked: u64,
    pub missing_payloads: u64,
    pub orphan_payloads: u64,
    pub size_mismatches: u64,
    pub hash_mismatches: u64,
    pub unverified_placements: u64,
    pub duplicate_content_groups: u64,
    pub duplicate_placement_rows: u64,
    pub io_errors: u64,
    pub healthy: bool,
    pub findings: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeduplicateLiveMetadataRequest {
    pub live_sqlite_path: PathBuf,
    pub disk_roots: Vec<DiskCopyRoot>,
    pub store_id: Option<String>,
    pub dry_run: bool,
    pub recorded_at_utc: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DeduplicateLiveMetadataReport {
    pub metadata_path: PathBuf,
    pub dry_run: bool,
    pub payloads_hashed: u64,
    pub hash_errors: u64,
    pub duplicate_content_groups: u64,
    pub duplicate_placement_rows: u64,
    pub metadata_rows_removed: u64,
    pub hashes_recorded: u64,
    pub warning: String,
}

#[derive(Clone, Debug)]
struct PlacementRecord {
    placement_id: String,
    object_id: String,
    disk_id: String,
    relative_path: String,
    object_size: Option<u64>,
    object_hash: Option<String>,
    placement_hash: Option<String>,
}

#[derive(Clone, Debug)]
struct HashedPlacement {
    record: PlacementRecord,
    hash: String,
}

pub fn verify_live_metadata(
    request: &VerifyLiveMetadataRequest,
) -> Result<VerifyLiveMetadataReport, VerifyLiveMetadataError> {
    let connection = Connection::open(&request.live_sqlite_path)?;
    connection.execute_batch("PRAGMA foreign_keys = ON;")?;
    if !table_exists(&connection, "stores")?
        || !table_exists(&connection, "objects")?
        || !table_exists(&connection, "placements")?
    {
        return Ok(VerifyLiveMetadataReport {
            metadata_path: request.live_sqlite_path.clone(),
            stores_scanned: 0,
            objects_scanned: 0,
            placements_scanned: 0,
            payloads_checked: 0,
            payload_bytes_checked: 0,
            missing_payloads: 0,
            orphan_payloads: 0,
            size_mismatches: 0,
            hash_mismatches: 0,
            unverified_placements: 0,
            duplicate_content_groups: 0,
            duplicate_placement_rows: 0,
            io_errors: 1,
            healthy: false,
            findings: vec!["live metadata schema is missing; run `store repair` first".to_string()],
        });
    }
    let stores_scanned = if let Some(store_id) = &request.store_id {
        connection.query_row(
            "SELECT COUNT(*) FROM stores WHERE store_id = ?1",
            [store_id],
            |row| row.get(0),
        )?
    } else {
        connection.query_row("SELECT COUNT(*) FROM stores", [], |row| row.get(0))?
    };
    let records = read_placements(&connection, request.store_id.as_deref())?;
    let objects_scanned = records
        .iter()
        .map(|record| record.object_id.as_str())
        .collect::<BTreeSet<_>>()
        .len() as u64;
    let placement_keys = records
        .iter()
        .map(|record| (record.object_id.clone(), record.disk_id.clone()))
        .collect::<Vec<_>>();
    let duplicate_placement_rows =
        placement_keys.len() as u64 - placement_keys.iter().collect::<HashSet<_>>().len() as u64;
    let roots = request
        .disk_roots
        .iter()
        .map(|root| (root.disk_id.as_str().to_string(), root.root_path.clone()))
        .collect::<HashMap<_, _>>();
    let mut report = VerifyLiveMetadataReport {
        metadata_path: request.live_sqlite_path.clone(),
        stores_scanned,
        objects_scanned,
        placements_scanned: records.len() as u64,
        payloads_checked: 0,
        payload_bytes_checked: 0,
        missing_payloads: 0,
        orphan_payloads: 0,
        size_mismatches: 0,
        hash_mismatches: 0,
        unverified_placements: 0,
        duplicate_content_groups: 0,
        duplicate_placement_rows,
        io_errors: 0,
        healthy: false,
        findings: Vec::new(),
    };
    let mut content_groups: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut expected_payloads = HashSet::new();
    for record in &records {
        let Some(root) = roots.get(&record.disk_id) else {
            report.io_errors += 1;
            finding(
                &mut report.findings,
                format!(
                    "placement {} references unknown disk {}",
                    record.placement_id, record.disk_id
                ),
            );
            continue;
        };
        let path = root.join(&record.relative_path);
        expected_payloads.insert((record.disk_id.clone(), record.relative_path.clone()));
        let metadata = match path.metadata() {
            Ok(metadata) if metadata.is_file() => metadata,
            Ok(_) => {
                report.missing_payloads += 1;
                finding(
                    &mut report.findings,
                    format!(
                        "placement {} payload is not a regular file: {}",
                        record.placement_id,
                        path.display()
                    ),
                );
                continue;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                report.missing_payloads += 1;
                finding(
                    &mut report.findings,
                    format!(
                        "placement {} payload is missing: {}",
                        record.placement_id,
                        path.display()
                    ),
                );
                continue;
            }
            Err(error) => {
                report.io_errors += 1;
                finding(
                    &mut report.findings,
                    format!(
                        "placement {} cannot be inspected: {} ({error})",
                        record.placement_id,
                        path.display()
                    ),
                );
                continue;
            }
        };
        report.payloads_checked += 1;
        report.payload_bytes_checked = report.payload_bytes_checked.saturating_add(metadata.len());
        if record.object_size != Some(metadata.len()) {
            report.size_mismatches += 1;
            finding(
                &mut report.findings,
                format!(
                    "placement {} size {} does not match object size {:?}",
                    record.placement_id,
                    metadata.len(),
                    record.object_size
                ),
            );
        }
        if request.hash_payloads {
            match hash_file_sha256(&path) {
                Ok(hash) => {
                    content_groups
                        .entry(hash.clone())
                        .or_default()
                        .insert(record.object_id.clone());
                    if record
                        .object_hash
                        .as_deref()
                        .is_some_and(|expected| expected != hash)
                        || record
                            .placement_hash
                            .as_deref()
                            .is_some_and(|expected| expected != hash)
                    {
                        report.hash_mismatches += 1;
                        finding(
                            &mut report.findings,
                            format!(
                                "placement {} checksum does not match metadata",
                                record.placement_id
                            ),
                        );
                    }
                }
                Err(error) => {
                    report.io_errors += 1;
                    finding(
                        &mut report.findings,
                        format!(
                            "placement {} checksum failed: {} ({error})",
                            record.placement_id,
                            path.display()
                        ),
                    );
                }
            }
        } else if record.object_hash.is_none() || record.placement_hash.is_none() {
            report.unverified_placements += 1;
        } else {
            content_groups
                .entry(record.placement_hash.clone().unwrap_or_default())
                .or_default()
                .insert(record.object_id.clone());
        }
    }
    report.duplicate_content_groups = content_groups
        .values()
        .filter(|objects| objects.len() > 1)
        .count() as u64;
    for root in &request.disk_roots {
        for payload in find_payloads(&root.root_path)? {
            let relative = payload
                .strip_prefix(&root.root_path)
                .map_err(|_| VerifyLiveMetadataError::InvalidPayloadPath(payload.clone()))?
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            if !expected_payloads.contains(&(root.disk_id.as_str().to_string(), relative.clone())) {
                report.orphan_payloads += 1;
                finding(
                    &mut report.findings,
                    format!("orphan payload is not indexed: {}", payload.display()),
                );
            }
        }
    }
    report.healthy = report.missing_payloads == 0
        && report.orphan_payloads == 0
        && report.size_mismatches == 0
        && report.hash_mismatches == 0
        && report.unverified_placements == 0
        && report.duplicate_placement_rows == 0
        && report.io_errors == 0;
    Ok(report)
}

pub fn deduplicate_live_metadata(
    request: &DeduplicateLiveMetadataRequest,
) -> Result<DeduplicateLiveMetadataReport, DeduplicateLiveMetadataError> {
    let connection = Connection::open(&request.live_sqlite_path)?;
    connection.execute_batch("PRAGMA foreign_keys = ON;")?;
    let roots = request
        .disk_roots
        .iter()
        .map(|root| (root.disk_id.as_str().to_string(), root.root_path.clone()))
        .collect::<HashMap<_, _>>();
    let records = read_placements(&connection, request.store_id.as_deref())?;
    let mut hashed = Vec::new();
    let mut hash_errors = 0_u64;
    for record in records {
        let Some(root) = roots.get(&record.disk_id) else {
            hash_errors += 1;
            continue;
        };
        let path = root.join(&record.relative_path);
        match hash_file_sha256(&path) {
            Ok(hash) => hashed.push(HashedPlacement { record, hash }),
            Err(_) => hash_errors += 1,
        }
    }
    let mut groups: BTreeMap<(String, String, String), Vec<&HashedPlacement>> = BTreeMap::new();
    let mut content_groups: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for placement in &hashed {
        groups
            .entry((
                placement.record.object_id.clone(),
                placement.record.disk_id.clone(),
                placement.hash.clone(),
            ))
            .or_default()
            .push(placement);
        content_groups
            .entry(placement.hash.clone())
            .or_default()
            .insert(placement.record.object_id.clone());
    }
    let duplicate_rows = groups
        .values()
        .map(|rows| rows.len().saturating_sub(1) as u64)
        .sum();
    let mut report = DeduplicateLiveMetadataReport {
        metadata_path: request.live_sqlite_path.clone(),
        dry_run: request.dry_run,
        payloads_hashed: hashed.len() as u64,
        hash_errors,
        duplicate_content_groups: content_groups
            .values()
            .filter(|objects| objects.len() > 1)
            .count() as u64,
        duplicate_placement_rows: duplicate_rows,
        metadata_rows_removed: 0,
        hashes_recorded: 0,
        warning: "deduplicate never deletes payload files; removed metadata rows become orphan findings for explicit operator review".to_string(),
    };
    if request.dry_run {
        return Ok(report);
    }
    let transaction = connection.unchecked_transaction()?;
    let mut object_hashes: HashMap<String, BTreeSet<String>> = HashMap::new();
    for placement in &hashed {
        transaction.execute(
            "UPDATE placements SET content_hash = ?1, verified_at_utc = ?2 WHERE placement_id = ?3",
            params![
                placement.hash,
                request.recorded_at_utc,
                placement.record.placement_id
            ],
        )?;
        report.hashes_recorded += 1;
        object_hashes
            .entry(placement.record.object_id.clone())
            .or_default()
            .insert(placement.hash.clone());
    }
    for (object_id, hashes) in object_hashes {
        if hashes.len() == 1 {
            transaction.execute(
                "UPDATE objects SET content_hash = ?1, state = 'HddCopyVerified', updated_at_utc = ?2 WHERE object_id = ?3",
                params![hashes.into_iter().next().unwrap_or_default(), request.recorded_at_utc, object_id],
            )?;
        }
    }
    for rows in groups.values().filter(|rows| rows.len() > 1) {
        for duplicate in rows.iter().skip(1) {
            transaction.execute(
                "DELETE FROM placements WHERE placement_id = ?1",
                [duplicate.record.placement_id.as_str()],
            )?;
            report.metadata_rows_removed += 1;
        }
    }
    transaction.commit()?;
    Ok(report)
}

fn read_placements(
    connection: &Connection,
    store_id: Option<&str>,
) -> Result<Vec<PlacementRecord>, rusqlite::Error> {
    let mut statement = connection.prepare(
        "SELECT p.placement_id, p.object_id, o.store_id, p.disk_id, p.relative_path,
                o.size_bytes, o.content_hash, p.content_hash
         FROM placements p JOIN objects o ON o.object_id = p.object_id
         WHERE (?1 IS NULL OR o.store_id = ?1)
         ORDER BY p.placement_id",
    )?;
    let rows = statement
        .query_map([store_id], |row| {
            Ok(PlacementRecord {
                placement_id: row.get(0)?,
                object_id: row.get(1)?,
                disk_id: row.get(3)?,
                relative_path: row.get(4)?,
                object_size: row
                    .get::<_, Option<i64>>(5)?
                    .and_then(|value| u64::try_from(value).ok()),
                object_hash: row.get(6)?,
                placement_hash: row.get(7)?,
            })
        })?
        .collect();
    rows
}

fn table_exists(connection: &Connection, table_name: &str) -> Result<bool, rusqlite::Error> {
    let count = connection.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [table_name],
        |row| row.get::<_, i64>(0),
    )?;
    Ok(count > 0)
}

fn find_payloads(root: &Path) -> Result<Vec<PathBuf>, std::io::Error> {
    let objects = root.join("objects");
    let mut payloads = Vec::new();
    collect_payloads(&objects, &mut payloads)?;
    Ok(payloads)
}

fn collect_payloads(path: &Path, payloads: &mut Vec<PathBuf>) -> Result<(), std::io::Error> {
    if !path.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let child = entry.path();
        if child.is_dir() {
            collect_payloads(&child, payloads)?;
        } else if child.file_name().and_then(|name| name.to_str()) == Some("payload") {
            payloads.push(child);
        }
    }
    Ok(())
}

fn finding(findings: &mut Vec<String>, value: String) {
    if findings.len() < MAX_FINDINGS {
        findings.push(value);
    }
}

#[derive(Debug)]
pub enum VerifyLiveMetadataError {
    Io(std::io::Error),
    Sqlite(rusqlite::Error),
    InvalidPayloadPath(PathBuf),
}

impl std::fmt::Display for VerifyLiveMetadataError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "metadata verification filesystem error: {error}"),
            Self::Sqlite(error) => write!(formatter, "metadata verification SQLite error: {error}"),
            Self::InvalidPayloadPath(path) => {
                write!(formatter, "invalid payload path {}", path.display())
            }
        }
    }
}
impl std::error::Error for VerifyLiveMetadataError {}
impl From<std::io::Error> for VerifyLiveMetadataError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<rusqlite::Error> for VerifyLiveMetadataError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

#[derive(Debug)]
pub enum DeduplicateLiveMetadataError {
    Io(std::io::Error),
    Sqlite(rusqlite::Error),
}

impl std::fmt::Display for DeduplicateLiveMetadataError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(
                formatter,
                "metadata deduplication filesystem error: {error}"
            ),
            Self::Sqlite(error) => {
                write!(formatter, "metadata deduplication SQLite error: {error}")
            }
        }
    }
}
impl std::error::Error for DeduplicateLiveMetadataError {}
impl From<std::io::Error> for DeduplicateLiveMetadataError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<rusqlite::Error> for DeduplicateLiveMetadataError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        deduplicate_live_metadata, verify_live_metadata, DeduplicateLiveMetadataRequest,
        VerifyLiveMetadataRequest,
    };
    use crate::{schema::LIVE_SCHEMA_SQL, DiskCopyRoot};
    use dasobjectstore_core::ids::DiskId;
    use rusqlite::Connection;
    use std::fs;

    #[test]
    fn verify_and_deduplicate_hash_identical_same_disk_placements() {
        let root =
            std::env::temp_dir().join(format!("dasobjectstore-integrity-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let disk = root.join("disk-a");
        let first = disk.join("objects/a/store-a%2Fobject/payload");
        let second = disk.join("objects/b/store-a%2Fobject-duplicate/payload");
        fs::create_dir_all(first.parent().expect("first parent")).expect("first parent");
        fs::create_dir_all(second.parent().expect("second parent")).expect("second parent");
        fs::write(&first, b"same").expect("first payload");
        fs::write(&second, b"same").expect("second payload");
        let live = root.join("live.sqlite");
        let connection = Connection::open(&live).expect("database");
        connection.execute_batch(LIVE_SCHEMA_SQL).expect("schema");
        connection
            .execute_batch(
                "INSERT INTO pools VALUES ('pool-a', 'Ready', 'now', 'now');
                 INSERT INTO disks VALUES ('disk-a', 'pool-a', 'hdd', 'Ready', NULL, NULL, NULL, NULL, 'now', 'now');
                 INSERT INTO stores VALUES ('store-a', 'pool-a', 'GeneratedData', '{}', 'now', 'now');
                 INSERT INTO objects VALUES ('store-a/object', 'store-a', 'naive', 'CopyingToHdd', 4, NULL, 'now', 'now');",
            )
            .expect("base rows");
        connection
            .execute(
                "INSERT INTO placements VALUES
                 ('p1', 'store-a/object', 'disk-a', 'objects/a/store-a%2Fobject/payload', NULL, NULL, 'now'),
                 ('p2', 'store-a/object', 'disk-a', 'objects/b/store-a%2Fobject-duplicate/payload', NULL, NULL, 'now')",
                [],
            )
            .expect("placements");
        drop(connection);
        let roots = vec![DiskCopyRoot::new(
            DiskId::new("disk-a").expect("disk id"),
            &disk,
        )];
        let verification = verify_live_metadata(&VerifyLiveMetadataRequest {
            live_sqlite_path: live.clone(),
            disk_roots: roots.clone(),
            store_id: None,
            hash_payloads: true,
        })
        .expect("verify");
        assert_eq!(verification.duplicate_placement_rows, 1);
        assert!(!verification.healthy);
        let dry_run = deduplicate_live_metadata(&DeduplicateLiveMetadataRequest {
            live_sqlite_path: live.clone(),
            disk_roots: roots.clone(),
            store_id: None,
            dry_run: true,
            recorded_at_utc: "now".to_string(),
        })
        .expect("deduplicate dry run");
        assert_eq!(dry_run.duplicate_placement_rows, 1);
        let applied = deduplicate_live_metadata(&DeduplicateLiveMetadataRequest {
            live_sqlite_path: live.clone(),
            disk_roots: roots,
            store_id: None,
            dry_run: false,
            recorded_at_utc: "now".to_string(),
        })
        .expect("deduplicate apply");
        assert_eq!(applied.metadata_rows_removed, 1);
        let connection = Connection::open(&live).expect("database");
        let placements: u64 = connection
            .query_row("SELECT COUNT(*) FROM placements", [], |row| row.get(0))
            .expect("placement count");
        assert_eq!(placements, 1);
        let hash: Option<String> = connection
            .query_row("SELECT content_hash FROM objects", [], |row| row.get(0))
            .expect("object hash");
        assert!(hash.is_some());
        let _ = fs::remove_dir_all(root);
    }
}
