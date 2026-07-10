//! Durable metadata commits for completed object placement.

use crate::local_object_store::ObjectPutReport;
use crate::schema::LIVE_SCHEMA_SQL;
use dasobjectstore_core::ids::StoreId;
use rusqlite::{params, Connection, Transaction};
use sha2::{Digest, Sha256};
use std::fmt::{self, Display};
use std::path::Path;

/// Record a completed, inline-hashed object copy in the live metadata index.
///
/// The payload writers are intentionally independent from SQLite. This narrow
/// commit is the hand-off that makes a successfully finalized payload visible
/// to browser, download, repair, and export consumers.
pub fn commit_object_put(
    live_sqlite_path: impl AsRef<Path>,
    store_id: &StoreId,
    report: &ObjectPutReport,
    recorded_at_utc: &str,
) -> Result<(), ObjectMetadataCommitError> {
    let mut connection = Connection::open(live_sqlite_path)?;
    connection.execute_batch(LIVE_SCHEMA_SQL)?;
    let transaction = connection.transaction()?;
    ensure_store(&transaction, store_id)?;

    let object_type = report.object_type.to_string();
    transaction.execute(
        "INSERT INTO objects (
            object_id, store_id, object_type, state, size_bytes, content_hash,
            created_at_utc, updated_at_utc
         ) VALUES (?1, ?2, ?3, 'HddCopyVerified', ?4, ?5, ?6, ?6)
         ON CONFLICT(object_id) DO UPDATE SET
            store_id = excluded.store_id,
            object_type = excluded.object_type,
            state = excluded.state,
            size_bytes = excluded.size_bytes,
            content_hash = excluded.content_hash,
            updated_at_utc = excluded.updated_at_utc",
        params![
            report.object_id.as_str(),
            store_id.as_str(),
            object_type,
            i64::try_from(report.bytes_staged)
                .map_err(|_| { ObjectMetadataCommitError::InvalidSize(report.bytes_staged) })?,
            report.content_hash,
            recorded_at_utc,
        ],
    )?;
    transaction.execute(
        "DELETE FROM placements WHERE object_id = ?1",
        [report.object_id.as_str()],
    )?;

    for placement in &report.placements {
        let relative_path = relative_object_path(&placement.destination_path)?;
        let placement_id = placement_id(
            report.object_id.as_str(),
            &placement.disk_id,
            &relative_path,
        );
        ensure_disk(&transaction, &placement.disk_id)?;
        transaction.execute(
            "INSERT INTO placements (
                placement_id, object_id, disk_id, relative_path, content_hash,
                verified_at_utc, created_at_utc
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![
                placement_id,
                report.object_id.as_str(),
                placement.disk_id,
                relative_path,
                placement.content_hash,
                recorded_at_utc,
            ],
        )?;
    }

    transaction.commit()?;
    Ok(())
}

fn ensure_store(
    transaction: &Transaction<'_>,
    store_id: &StoreId,
) -> Result<(), ObjectMetadataCommitError> {
    let exists = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM stores WHERE store_id = ?1)",
        [store_id.as_str()],
        |row| row.get::<_, bool>(0),
    )?;
    if !exists {
        return Err(ObjectMetadataCommitError::MissingStore(store_id.clone()));
    }
    Ok(())
}

fn ensure_disk(
    transaction: &Transaction<'_>,
    disk_id: &str,
) -> Result<(), ObjectMetadataCommitError> {
    let exists = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM disks WHERE disk_id = ?1)",
        [disk_id],
        |row| row.get::<_, bool>(0),
    )?;
    if !exists {
        return Err(ObjectMetadataCommitError::MissingDisk(disk_id.to_string()));
    }
    Ok(())
}

fn relative_object_path(path: &Path) -> Result<String, ObjectMetadataCommitError> {
    let mut components = path.components();
    while let Some(component) = components.next() {
        if component.as_os_str() == "objects" {
            let mut relative = component.as_os_str().to_string_lossy().into_owned();
            for component in components {
                relative.push('/');
                relative.push_str(&component.as_os_str().to_string_lossy());
            }
            return Ok(relative);
        }
    }
    Err(ObjectMetadataCommitError::InvalidPlacementPath(
        path.to_path_buf(),
    ))
}

fn placement_id(object_id: &str, disk_id: &str, relative_path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(object_id.as_bytes());
    hasher.update([0]);
    hasher.update(disk_id.as_bytes());
    hasher.update([0]);
    hasher.update(relative_path.as_bytes());
    format!("placement-{}", encode_hex(hasher.finalize()))
}

fn encode_hex(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .flat_map(|byte| [byte >> 4, byte & 0x0f])
        .map(|nibble| char::from(b"0123456789abcdef"[nibble as usize]))
        .collect()
}

#[derive(Debug)]
pub enum ObjectMetadataCommitError {
    Io(rusqlite::Error),
    MissingStore(StoreId),
    MissingDisk(String),
    InvalidSize(u64),
    InvalidPlacementPath(std::path::PathBuf),
}

impl Display for ObjectMetadataCommitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "object metadata commit failed: {error}"),
            Self::MissingStore(store_id) => {
                write!(
                    formatter,
                    "object metadata store {store_id} is not registered"
                )
            }
            Self::MissingDisk(disk_id) => {
                write!(
                    formatter,
                    "object metadata disk {disk_id} is not registered"
                )
            }
            Self::InvalidSize(size) => write!(formatter, "object size {size} exceeds SQLite range"),
            Self::InvalidPlacementPath(path) => write!(
                formatter,
                "object placement path does not contain an objects root: {}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for ObjectMetadataCommitError {}

impl From<rusqlite::Error> for ObjectMetadataCommitError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Io(error)
    }
}

#[cfg(test)]
mod tests {
    use super::commit_object_put;
    use crate::local_object_store::{ObjectPutPlacementReport, ObjectPutReport};
    use crate::schema::LIVE_SCHEMA_SQL;
    use dasobjectstore_core::ids::{ObjectId, StoreId};
    use dasobjectstore_core::object_type::ObjectType;
    use rusqlite::Connection;
    use std::path::PathBuf;

    #[test]
    fn commits_object_and_verified_placements_atomically() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-object-commit-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create root");
        let db = root.join("live.sqlite");
        let connection = Connection::open(&db).expect("open db");
        connection.execute_batch(LIVE_SCHEMA_SQL).expect("schema");
        connection
            .execute(
                "INSERT INTO pools VALUES ('pool-a', 'Clean', 'now', 'now')",
                [],
            )
            .expect("pool");
        connection
            .execute(
                "INSERT INTO stores VALUES ('store-a', 'pool-a', 'generated_data', '{}', 'now', 'now')",
                [],
            )
            .expect("store");
        connection
            .execute(
                "INSERT INTO disks VALUES ('disk-a', 'pool-a', 'hdd', 'Healthy', NULL, NULL, NULL, NULL, 'now', 'now')",
                [],
            )
            .expect("disk");
        drop(connection);

        let object_id = ObjectId::new("store-a/object.bin").expect("object id");
        let report = ObjectPutReport {
            object_id: object_id.clone(),
            object_type: ObjectType::Naive,
            source_path: PathBuf::from("/source/object.bin"),
            staged_payload_path: PathBuf::from("/ssd/staged/object.bin"),
            bytes_staged: 128,
            content_hash_algorithm: "sha256".to_string(),
            content_hash: "hash-a".to_string(),
            placements: vec![ObjectPutPlacementReport {
                disk_id: "disk-a".to_string(),
                copy_number: 1,
                destination_path: PathBuf::from(
                    "/srv/dasobjectstore/hdd/disk-a/objects/ha/store-a%2Fobject.bin/payload",
                ),
                bytes_written: 128,
                content_hash: "hash-a".to_string(),
            }],
        };

        commit_object_put(
            &db,
            &StoreId::new("store-a").expect("store id"),
            &report,
            "now",
        )
        .expect("commit");

        let connection = Connection::open(&db).expect("reopen db");
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM objects", [], |row| row
                    .get::<_, i64>(0))
                .expect("object count"),
            1
        );
        assert_eq!(
            connection
                .query_row("SELECT COUNT(*) FROM placements", [], |row| row
                    .get::<_, i64>(0))
                .expect("placement count"),
            1
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
