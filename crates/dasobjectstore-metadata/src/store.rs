use crate::evacuation::DiskCopyRoot;
use dasobjectstore_core::ids::{DiskId, InvalidId, ObjectId, StoreId};
use rusqlite::{Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Display};
use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreDrainRequest {
    pub live_sqlite_path: PathBuf,
    pub store_id: StoreId,
    pub disk_roots: Vec<DiskCopyRoot>,
    pub dry_run: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreDeleteRequest {
    pub live_sqlite_path: PathBuf,
    pub store_id: StoreId,
    pub disk_roots: Vec<DiskCopyRoot>,
    pub dry_run: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreDrainReport {
    pub live_sqlite_path: PathBuf,
    pub store_id: StoreId,
    pub dry_run: bool,
    pub objects_removed: usize,
    pub placements_removed: usize,
    pub ingest_jobs_removed: usize,
    pub payload_files_removed: usize,
    pub missing_payload_files: usize,
    pub affected_payloads: Vec<StorePayloadRemoval>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreDeleteReport {
    pub live_sqlite_path: PathBuf,
    pub store_id: StoreId,
    pub dry_run: bool,
    pub store_metadata_removed: bool,
    pub drain: StoreDrainReport,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StorePayloadRemoval {
    pub object_id: ObjectId,
    pub placement_id: String,
    pub disk_id: DiskId,
    pub path: PathBuf,
    pub existed: bool,
}

#[derive(Debug)]
pub enum StoreCleanupError {
    Io(std::io::Error),
    Sqlite(rusqlite::Error),
    StoreNotFound {
        store_id: StoreId,
    },
    MissingDiskRoot {
        disk_id: DiskId,
    },
    InvalidIdentifier {
        field: &'static str,
        source: InvalidId,
    },
    UnsafePlacementPath {
        path: String,
    },
}

impl Display for StoreCleanupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(
                formatter,
                "store cleanup filesystem operation failed: {err}"
            ),
            Self::Sqlite(err) => {
                write!(formatter, "store cleanup metadata operation failed: {err}")
            }
            Self::StoreNotFound { store_id } => {
                write!(
                    formatter,
                    "store {store_id} does not exist in live metadata"
                )
            }
            Self::MissingDiskRoot { disk_id } => {
                write!(
                    formatter,
                    "no managed disk root was provided for disk {disk_id}"
                )
            }
            Self::InvalidIdentifier { field, source } => {
                write!(
                    formatter,
                    "invalid store cleanup metadata {field}: {source}"
                )
            }
            Self::UnsafePlacementPath { path } => {
                write!(formatter, "unsafe placement path in metadata: {path}")
            }
        }
    }
}

impl std::error::Error for StoreCleanupError {}

impl From<std::io::Error> for StoreCleanupError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<rusqlite::Error> for StoreCleanupError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Sqlite(err)
    }
}

pub fn drain_store(request: &StoreDrainRequest) -> Result<StoreDrainReport, StoreCleanupError> {
    let mut connection = Connection::open(&request.live_sqlite_path)?;
    ensure_store_exists(&connection, &request.store_id)?;
    let payloads = read_store_payloads(&connection, &request.store_id, &request.disk_roots)?;
    let job_count = count_store_ingest_jobs(&connection, &request.store_id)?;
    let object_count = count_store_objects(&connection, &request.store_id)?;
    let placement_count = payloads.len();

    let mut report = StoreDrainReport {
        live_sqlite_path: request.live_sqlite_path.clone(),
        store_id: request.store_id.clone(),
        dry_run: request.dry_run,
        objects_removed: object_count,
        placements_removed: placement_count,
        ingest_jobs_removed: job_count,
        payload_files_removed: 0,
        missing_payload_files: 0,
        affected_payloads: Vec::new(),
    };

    for payload in payloads {
        let existed = payload.path.is_file();
        if !request.dry_run && existed {
            fs::remove_file(&payload.path)?;
        }
        if existed {
            report.payload_files_removed += 1;
        } else {
            report.missing_payload_files += 1;
        }
        report.affected_payloads.push(StorePayloadRemoval {
            object_id: payload.object_id,
            placement_id: payload.placement_id,
            disk_id: payload.disk_id,
            path: payload.path,
            existed,
        });
    }

    if !request.dry_run {
        let transaction = connection.transaction()?;
        delete_store_object_references(&transaction, &request.store_id)?;
        transaction.commit()?;
    }

    Ok(report)
}

pub fn delete_store(request: &StoreDeleteRequest) -> Result<StoreDeleteReport, StoreCleanupError> {
    let drain = drain_store(&StoreDrainRequest {
        live_sqlite_path: request.live_sqlite_path.clone(),
        store_id: request.store_id.clone(),
        disk_roots: request.disk_roots.clone(),
        dry_run: request.dry_run,
    })?;

    if !request.dry_run {
        let mut connection = Connection::open(&request.live_sqlite_path)?;
        ensure_store_exists(&connection, &request.store_id)?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM stores WHERE store_id = ?1",
            [request.store_id.as_str()],
        )?;
        transaction.commit()?;
    }

    Ok(StoreDeleteReport {
        live_sqlite_path: request.live_sqlite_path.clone(),
        store_id: request.store_id.clone(),
        dry_run: request.dry_run,
        store_metadata_removed: !request.dry_run,
        drain,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct StorePayload {
    object_id: ObjectId,
    placement_id: String,
    disk_id: DiskId,
    path: PathBuf,
}

fn ensure_store_exists(
    connection: &Connection,
    store_id: &StoreId,
) -> Result<(), StoreCleanupError> {
    let exists = connection
        .query_row(
            "SELECT 1 FROM stores WHERE store_id = ?1",
            [store_id.as_str()],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !exists {
        return Err(StoreCleanupError::StoreNotFound {
            store_id: store_id.clone(),
        });
    }
    Ok(())
}

fn read_store_payloads(
    connection: &Connection,
    store_id: &StoreId,
    disk_roots: &[DiskCopyRoot],
) -> Result<Vec<StorePayload>, StoreCleanupError> {
    let root_by_disk = disk_roots
        .iter()
        .map(|root| (root.disk_id.clone(), root.root_path.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut statement = connection.prepare(
        "SELECT objects.object_id, placements.placement_id, placements.disk_id, placements.relative_path
         FROM placements
         INNER JOIN objects ON objects.object_id = placements.object_id
         WHERE objects.store_id = ?1
         ORDER BY objects.object_id, placements.placement_id",
    )?;
    let rows = statement.query_map([store_id.as_str()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    let mut payloads = Vec::new();
    for row in rows {
        let (object_id, placement_id, disk_id, relative_path) = row?;
        let object_id =
            ObjectId::new(object_id).map_err(|source| StoreCleanupError::InvalidIdentifier {
                field: "object_id",
                source,
            })?;
        let disk_id =
            DiskId::new(disk_id).map_err(|source| StoreCleanupError::InvalidIdentifier {
                field: "disk_id",
                source,
            })?;
        validate_relative_path(&relative_path)?;
        let Some(root) = root_by_disk.get(&disk_id) else {
            return Err(StoreCleanupError::MissingDiskRoot { disk_id });
        };
        payloads.push(StorePayload {
            object_id,
            placement_id,
            disk_id,
            path: root.join(relative_path),
        });
    }

    Ok(payloads)
}

fn count_store_objects(
    connection: &Connection,
    store_id: &StoreId,
) -> Result<usize, StoreCleanupError> {
    count_usize(
        connection,
        "SELECT COUNT(*) FROM objects WHERE store_id = ?1",
        store_id,
    )
}

fn count_store_ingest_jobs(
    connection: &Connection,
    store_id: &StoreId,
) -> Result<usize, StoreCleanupError> {
    count_usize(
        connection,
        "SELECT COUNT(*) FROM ingest_jobs WHERE store_id = ?1",
        store_id,
    )
}

fn count_usize(
    connection: &Connection,
    sql: &str,
    store_id: &StoreId,
) -> Result<usize, StoreCleanupError> {
    let value = connection.query_row(sql, [store_id.as_str()], |row| row.get::<_, i64>(0))?;
    Ok(value.max(0) as usize)
}

fn delete_store_object_references(
    transaction: &Transaction<'_>,
    store_id: &StoreId,
) -> Result<(), StoreCleanupError> {
    let object_ids = read_store_object_ids(transaction, store_id)?;
    if object_ids.is_empty() {
        transaction.execute(
            "DELETE FROM ingest_jobs WHERE store_id = ?1",
            [store_id.as_str()],
        )?;
        return Ok(());
    }

    for object_id in &object_ids {
        transaction.execute(
            "UPDATE ingest_jobs SET object_id = NULL WHERE object_id = ?1",
            [object_id.as_str()],
        )?;
    }
    transaction.execute(
        "DELETE FROM ingest_jobs WHERE store_id = ?1",
        [store_id.as_str()],
    )?;
    transaction.execute(
        "DELETE FROM placements WHERE object_id IN (
            SELECT object_id FROM objects WHERE store_id = ?1
        )",
        [store_id.as_str()],
    )?;
    transaction.execute(
        "DELETE FROM objects WHERE store_id = ?1",
        [store_id.as_str()],
    )?;

    Ok(())
}

fn read_store_object_ids(
    connection: &Connection,
    store_id: &StoreId,
) -> Result<BTreeSet<ObjectId>, StoreCleanupError> {
    let mut statement = connection
        .prepare("SELECT object_id FROM objects WHERE store_id = ?1 ORDER BY object_id")?;
    let rows = statement.query_map([store_id.as_str()], |row| row.get::<_, String>(0))?;
    let mut object_ids = BTreeSet::new();
    for row in rows {
        let object_id =
            ObjectId::new(row?).map_err(|source| StoreCleanupError::InvalidIdentifier {
                field: "object_id",
                source,
            })?;
        object_ids.insert(object_id);
    }
    Ok(object_ids)
}

fn validate_relative_path(value: &str) -> Result<(), StoreCleanupError> {
    let path = Path::new(value);
    if value.trim().is_empty() || path.is_absolute() {
        return Err(StoreCleanupError::UnsafePlacementPath {
            path: value.to_string(),
        });
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(StoreCleanupError::UnsafePlacementPath {
            path: value.to_string(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{delete_store, drain_store, StoreDeleteRequest, StoreDrainRequest};
    use crate::evacuation::DiskCopyRoot;
    use crate::schema::LIVE_SCHEMA_SQL;
    use dasobjectstore_core::ids::{DiskId, StoreId};
    use rusqlite::Connection;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn drains_store_payloads_and_metadata_references() {
        let root = temp_root("store-drain");
        let live_sqlite_path = create_live_sqlite_fixture(&root);
        let disk_root = root.join("disk-a");
        let payload_path = disk_root.join("objects/aa/object-a/payload");
        fs::create_dir_all(payload_path.parent().expect("payload parent")).expect("payload dir");
        fs::write(&payload_path, b"payload").expect("payload file");

        let report = drain_store(&StoreDrainRequest {
            live_sqlite_path: live_sqlite_path.clone(),
            store_id: StoreId::new("store-a").expect("store id"),
            disk_roots: vec![DiskCopyRoot::new(
                DiskId::new("disk-a").expect("disk id"),
                disk_root,
            )],
            dry_run: false,
        })
        .expect("store drains");

        assert_eq!(report.objects_removed, 1);
        assert_eq!(report.placements_removed, 1);
        assert_eq!(report.ingest_jobs_removed, 1);
        assert_eq!(report.payload_files_removed, 1);
        assert!(!payload_path.exists());
        assert_eq!(row_count(&live_sqlite_path, "objects"), 0);
        assert_eq!(row_count(&live_sqlite_path, "placements"), 0);
        assert_eq!(row_count(&live_sqlite_path, "ingest_jobs"), 0);
        assert_eq!(row_count(&live_sqlite_path, "stores"), 1);

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn delete_store_removes_empty_store_row_after_drain() {
        let root = temp_root("store-delete");
        let live_sqlite_path = create_live_sqlite_fixture(&root);
        let disk_root = root.join("disk-a");

        let report = delete_store(&StoreDeleteRequest {
            live_sqlite_path: live_sqlite_path.clone(),
            store_id: StoreId::new("store-a").expect("store id"),
            disk_roots: vec![DiskCopyRoot::new(
                DiskId::new("disk-a").expect("disk id"),
                disk_root,
            )],
            dry_run: false,
        })
        .expect("store deletes");

        assert!(report.store_metadata_removed);
        assert_eq!(row_count(&live_sqlite_path, "stores"), 0);

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn dry_run_reports_without_mutating() {
        let root = temp_root("store-drain-dry-run");
        let live_sqlite_path = create_live_sqlite_fixture(&root);
        let disk_root = root.join("disk-a");
        let payload_path = disk_root.join("objects/aa/object-a/payload");
        fs::create_dir_all(payload_path.parent().expect("payload parent")).expect("payload dir");
        fs::write(&payload_path, b"payload").expect("payload file");

        let report = drain_store(&StoreDrainRequest {
            live_sqlite_path: live_sqlite_path.clone(),
            store_id: StoreId::new("store-a").expect("store id"),
            disk_roots: vec![DiskCopyRoot::new(
                DiskId::new("disk-a").expect("disk id"),
                disk_root,
            )],
            dry_run: true,
        })
        .expect("dry run succeeds");

        assert!(report.dry_run);
        assert!(payload_path.exists());
        assert_eq!(row_count(&live_sqlite_path, "objects"), 1);

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    fn create_live_sqlite_fixture(root: &Path) -> PathBuf {
        fs::create_dir_all(root).expect("root dir");
        let live_sqlite_path = root.join("live.sqlite");
        let connection = Connection::open(&live_sqlite_path).expect("sqlite opens");
        connection
            .execute_batch(LIVE_SCHEMA_SQL)
            .expect("schema applies");
        connection
            .execute_batch(
                "INSERT INTO pools (pool_id, state, created_at_utc, updated_at_utc)
                 VALUES ('pool-a', 'Healthy', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
                 INSERT INTO stores (store_id, pool_id, class, policy_json, created_at_utc, updated_at_utc)
                 VALUES ('store-a', 'pool-a', 'generated_data', '{}', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
                 INSERT INTO disks (disk_id, pool_id, role, state, created_at_utc, updated_at_utc)
                 VALUES ('disk-a', 'pool-a', 'hdd_capacity', 'Healthy', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
                 INSERT INTO objects (object_id, store_id, state, size_bytes, content_hash, created_at_utc, updated_at_utc)
                 VALUES ('object-a', 'store-a', 'Protected', 7, 'sha256:object-a', '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z');
                 INSERT INTO placements (placement_id, object_id, disk_id, relative_path, content_hash, verified_at_utc, created_at_utc)
                 VALUES ('placement-a', 'object-a', 'disk-a', 'objects/aa/object-a/payload', 'sha256:object-a', '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z');
                 INSERT INTO ingest_jobs (ingest_job_id, store_id, object_id, state, ingest_mode, acknowledgement_policy, staging_path, created_at_utc, updated_at_utc)
                 VALUES ('job-a', 'store-a', 'object-a', 'Complete', 'SsdFirst', 'AfterHddPlacement', '/tmp/staged', '2026-01-02T00:00:00Z', '2026-01-02T00:00:00Z');",
            )
            .expect("fixture inserts");
        live_sqlite_path
    }

    fn row_count(live_sqlite_path: &Path, table: &str) -> usize {
        let connection = Connection::open(live_sqlite_path).expect("sqlite opens");
        connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("row count") as usize
    }

    fn temp_root(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dasobjectstore-metadata-{name}-{}-{nanos}",
            std::process::id()
        ))
    }
}
