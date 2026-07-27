//! Durable metadata commits for completed object placement.

use crate::local_object_store::ObjectPutReport;
use crate::schema::LIVE_SCHEMA_SQL;
use dasobjectstore_core::ids::StoreId;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
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
    let size = i64::try_from(report.bytes_staged)
        .map_err(|_| ObjectMetadataCommitError::InvalidSize(report.bytes_staged))?;
    let existing = transaction
        .query_row(
            "SELECT store_id,object_type,size_bytes,content_hash
             FROM objects WHERE object_id=?1",
            [report.object_id.as_str()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()?;
    let prior_logical = crate::logical_identity::read_native_logical_version_in_transaction(
        &transaction,
        report.object_id.as_str(),
    )
    .map_err(|error| ObjectMetadataCommitError::LogicalIdentity(error.to_string()))?;
    let payload_changed = if let Some(existing) = &existing {
        if existing.0 != store_id.as_str() {
            return Err(ObjectMetadataCommitError::ImmutableObjectConflict(
                report.object_id.to_string(),
            ));
        }
        existing.1 != object_type
            || existing.2 != Some(size)
            || existing.3.as_deref() != Some(report.content_hash.as_str())
    } else {
        false
    };
    if payload_changed && prior_logical.is_none() {
        return Err(ObjectMetadataCommitError::LegacyIdentityRequiresBackfill(
            report.object_id.to_string(),
        ));
    }
    if let (Some(existing), Some(prior)) = (&existing, &prior_logical) {
        let prior_algorithm =
            crate::logical_identity::normalize_algorithm(&prior.content_hash_algorithm);
        let existing_hash = existing
            .3
            .as_deref()
            .map(|hash| crate::logical_identity::normalize_hash(hash, &prior_algorithm));
        if prior.store_id != *store_id
            || prior.object_key != report.object_id.as_str()
            || existing.2.and_then(|value| u64::try_from(value).ok()) != Some(prior.size_bytes)
            || existing_hash.as_deref() != Some(prior.content_hash.as_str())
        {
            return Err(ObjectMetadataCommitError::ImmutableObjectConflict(
                report.object_id.to_string(),
            ));
        }
    }
    if existing.is_some() {
        transaction.execute(
            "UPDATE objects SET object_type=?1,state='HddCopyVerified',size_bytes=?2,
                 content_hash=?3,updated_at_utc=?4 WHERE object_id=?5",
            params![
                object_type,
                size,
                report.content_hash,
                recorded_at_utc,
                report.object_id.as_str()
            ],
        )?;
    } else {
        transaction.execute(
            "INSERT INTO objects (
            object_id, store_id, object_type, state, size_bytes, content_hash,
            created_at_utc, updated_at_utc
         ) VALUES (?1, ?2, ?3, 'HddCopyVerified', ?4, ?5, ?6, ?6)",
            params![
                report.object_id.as_str(),
                store_id.as_str(),
                object_type,
                size,
                report.content_hash,
                recorded_at_utc,
            ],
        )?;
    }
    let object_version = if payload_changed {
        prior_logical
            .as_ref()
            .map(|version| version.object_version)
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(ObjectMetadataCommitError::ObjectVersionOverflow)?
    } else {
        prior_logical
            .as_ref()
            .map(|version| version.object_version)
            .unwrap_or(1)
    };
    let (logical_version, _) = crate::logical_identity::claim_logical_version_in_transaction(
        &transaction,
        &crate::logical_identity::LogicalVersionClaim {
            store_id,
            object_key: report.object_id.as_str(),
            object_version,
            size_bytes: report.bytes_staged,
            content_hash_algorithm: &report.content_hash_algorithm,
            content_hash: &report.content_hash,
            recorded_at_utc,
        },
    )
    .map_err(|error| ObjectMetadataCommitError::LogicalIdentity(error.to_string()))?;
    if let Some(prior) = &prior_logical {
        if prior.logical_version_id != logical_version.logical_version_id {
            crate::logical_identity::withdraw_logical_version_placements_in_transaction(
                &transaction,
                &prior.logical_version_id,
                "native",
                recorded_at_utc,
            )
            .and_then(|_| {
                crate::logical_identity::replace_native_object_binding_in_transaction(
                    &transaction,
                    report.object_id.as_str(),
                    &prior.logical_version_id,
                    &logical_version.logical_version_id,
                )
            })
            .map_err(|error| ObjectMetadataCommitError::LogicalIdentity(error.to_string()))?;
        }
    } else {
        crate::logical_identity::bind_native_object_in_transaction(
            &transaction,
            report.object_id.as_str(),
            &logical_version.logical_version_id,
            recorded_at_utc,
        )
        .map_err(|error| ObjectMetadataCommitError::LogicalIdentity(error.to_string()))?;
    }
    let mut desired_placement_ids = Vec::with_capacity(report.placements.len());
    let mut prepared = Vec::with_capacity(report.placements.len());
    for placement in &report.placements {
        let relative_path = relative_object_path(&placement.destination_path)?;
        let placement_id = placement_id(
            report.object_id.as_str(),
            &placement.disk_id,
            &relative_path,
        );
        ensure_disk(&transaction, &placement.disk_id)?;
        desired_placement_ids.push(placement_id.clone());
        prepared.push((placement, relative_path, placement_id));
    }
    let prior_ids = {
        let mut statement =
            transaction.prepare("SELECT placement_id FROM placements WHERE object_id=?1")?;
        let rows = statement
            .query_map([report.object_id.as_str()], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let removed_ids = prior_ids
        .iter()
        .filter(|placement_id| !desired_placement_ids.contains(placement_id))
        .map(String::as_str)
        .collect::<Vec<_>>();
    crate::logical_identity::withdraw_logical_placement_sources_in_transaction(
        &transaction,
        "native",
        &removed_ids,
        recorded_at_utc,
    )
    .map_err(|error| ObjectMetadataCommitError::LogicalIdentity(error.to_string()))?;
    transaction.execute(
        "DELETE FROM placements WHERE object_id = ?1",
        [report.object_id.as_str()],
    )?;

    for (placement, relative_path, placement_id) in prepared {
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
        let logical_source_placement_id = if object_version == 1 {
            placement_id.clone()
        } else {
            format!("{placement_id}:v{object_version}")
        };
        crate::logical_identity::claim_logical_placement_in_transaction(
            &transaction,
            &crate::logical_identity::LogicalPlacementClaim {
                logical_version_id: &logical_version.logical_version_id,
                placement_kind: "hdd",
                placement_namespace: "native",
                source_placement_id: &logical_source_placement_id,
                location: &format!("{}:{relative_path}", placement.disk_id),
                content_hash_algorithm: &report.content_hash_algorithm,
                content_hash: &placement.content_hash,
                verified_at_utc: Some(recorded_at_utc),
                recorded_at_utc,
            },
        )
        .map_err(|error| ObjectMetadataCommitError::LogicalIdentity(error.to_string()))?;
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

pub fn placement_id(object_id: &str, disk_id: &str, relative_path: &str) -> String {
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
    ObjectVersionOverflow,
    InvalidPlacementPath(std::path::PathBuf),
    ImmutableObjectConflict(String),
    LegacyIdentityRequiresBackfill(String),
    LogicalIdentity(String),
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
            Self::ObjectVersionOverflow => {
                formatter.write_str("logical object version exceeds supported range")
            }
            Self::InvalidPlacementPath(path) => write!(
                formatter,
                "object placement path does not contain an objects root: {}",
                path.display()
            ),
            Self::ImmutableObjectConflict(object_id) => {
                write!(
                    formatter,
                    "immutable object identity conflict for {object_id}"
                )
            }
            Self::LegacyIdentityRequiresBackfill(object_id) => write!(
                formatter,
                "legacy object {object_id} must be canonically adopted before replacement"
            ),
            Self::LogicalIdentity(message) => {
                write!(
                    formatter,
                    "logical object identity publication failed: {message}"
                )
            }
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
                "INSERT INTO stores VALUES ('store-b', 'pool-a', 'generated_data', '{}', 'now', 'now')",
                [],
            )
            .expect("second store");
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
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM logical_object_versions v
                     JOIN native_logical_version_bindings b
                       ON b.logical_version_id=v.logical_version_id
                     JOIN logical_placements p
                       ON p.logical_version_id=v.logical_version_id
                     WHERE b.object_id=?1 AND p.state='active'",
                    [object_id.as_str()],
                    |row| row.get::<_, i64>(0)
                )
                .expect("canonical evidence"),
            1
        );
        drop(connection);

        commit_object_put(
            &db,
            &StoreId::new("store-a").expect("store id"),
            &report,
            "later",
        )
        .expect("exact replay");
        assert!(matches!(
            commit_object_put(
                &db,
                &StoreId::new("store-b").expect("store id"),
                &report,
                "later"
            ),
            Err(super::ObjectMetadataCommitError::ImmutableObjectConflict(_))
        ));
        let connection = Connection::open(&db).expect("reopen db");
        assert_eq!(
            connection
                .query_row(
                    "SELECT size_bytes FROM objects WHERE object_id=?1",
                    [object_id.as_str()],
                    |row| row.get::<_, i64>(0)
                )
                .expect("immutable size"),
            128
        );
        drop(connection);

        let mut replacement = report.clone();
        replacement.bytes_staged = 129;
        replacement.content_hash = "hash-b".to_string();
        replacement.placements[0].bytes_written = 129;
        replacement.placements[0].content_hash = "hash-b".to_string();
        commit_object_put(
            &db,
            &StoreId::new("store-a").expect("store id"),
            &replacement,
            "replacement",
        )
        .expect("replacement creates a new immutable logical version");
        commit_object_put(
            &db,
            &StoreId::new("store-a").expect("store id"),
            &replacement,
            "replacement-replay",
        )
        .expect("replacement replay is idempotent");
        let connection = Connection::open(&db).expect("replacement db");
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM logical_object_versions
                     WHERE store_id='store-a' AND object_key=?1",
                    [object_id.as_str()],
                    |row| row.get::<_, i64>(0)
                )
                .expect("version count"),
            2
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT v.object_version FROM native_logical_version_bindings b
                     JOIN logical_object_versions v
                       ON v.logical_version_id=b.logical_version_id
                     WHERE b.object_id=?1",
                    [object_id.as_str()],
                    |row| row.get::<_, u64>(0)
                )
                .expect("current version"),
            2
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM logical_placements
                     WHERE placement_namespace='native' AND state='withdrawn'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .expect("withdrawn placement count"),
            1
        );
        drop(connection);

        let mut no_longer_placed = replacement;
        no_longer_placed.placements.clear();
        commit_object_put(
            &db,
            &StoreId::new("store-a").expect("store id"),
            &no_longer_placed,
            "latest",
        )
        .expect("placement withdrawal");
        let connection = Connection::open(&db).expect("reopen db");
        assert_eq!(
            connection
                .query_row(
                    "SELECT state FROM logical_placements
                     WHERE placement_namespace='native'",
                    [],
                    |row| row.get::<_, String>(0)
                )
                .expect("withdrawn canonical placement"),
            "withdrawn"
        );
        connection
            .execute(
                "INSERT INTO objects(
                     object_id,store_id,object_type,state,size_bytes,content_hash,
                     created_at_utc,updated_at_utc
                 ) VALUES(
                     'store-a/legacy.bin','store-a','naive','HddCopyVerified',
                     4,'old-hash','old','old'
                 )",
                [],
            )
            .expect("legacy object");
        drop(connection);
        let mut legacy_replacement = report;
        legacy_replacement.object_id =
            ObjectId::new("store-a/legacy.bin").expect("legacy object id");
        assert!(matches!(
            commit_object_put(
                &db,
                &StoreId::new("store-a").expect("store id"),
                &legacy_replacement,
                "replacement"
            ),
            Err(super::ObjectMetadataCommitError::LegacyIdentityRequiresBackfill(_))
        ));
        let connection = Connection::open(&db).expect("legacy evidence");
        assert_eq!(
            connection
                .query_row(
                    "SELECT size_bytes FROM objects WHERE object_id='store-a/legacy.bin'",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .expect("legacy size"),
            4
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
