use dasobjectstore_core::ids::{DiskId, InvalidId, ObjectId, PlacementId, StoreId};
use dasobjectstore_core::object_type::{ObjectType, ObjectTypeParseError};
use rusqlite::types::Type;
use rusqlite::{Connection, OpenFlags, OptionalExtension, Row};
use serde::Serialize;
use std::collections::HashMap;
use std::fmt::{self, Display};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ObjectInspectSummary {
    pub live_sqlite_path: PathBuf,
    pub object_id: ObjectId,
    pub store_id: StoreId,
    pub store_class: String,
    pub object_type: ObjectType,
    pub state: String,
    pub size_bytes: Option<u64>,
    pub content_hash: Option<String>,
    pub created_at_utc: String,
    pub updated_at_utc: String,
    pub placements: Vec<ObjectPlacementSummary>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ObjectPlacementSummary {
    pub placement_id: PlacementId,
    pub disk_id: DiskId,
    pub relative_path: String,
    pub content_hash: Option<String>,
    pub verified_at_utc: Option<String>,
}

pub fn read_object_inspect(
    live_sqlite_path: impl AsRef<Path>,
    object_id: &ObjectId,
) -> Result<ObjectInspectSummary, ObjectInspectError> {
    let live_sqlite_path = live_sqlite_path.as_ref();
    let connection = Connection::open(live_sqlite_path)?;
    let mut summary = read_object_row(&connection, live_sqlite_path, object_id)?;
    summary.placements = read_object_placements(&connection, object_id)?;

    Ok(summary)
}

/// Read every object and placement for one store using a single SQLite
/// connection and a bounded pair of queries.
pub fn read_store_object_inspects(
    live_sqlite_path: impl AsRef<Path>,
    store_id: &StoreId,
) -> Result<Vec<ObjectInspectSummary>, ObjectInspectError> {
    let live_sqlite_path = live_sqlite_path.as_ref();
    let connection =
        Connection::open_with_flags(live_sqlite_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut statement = connection.prepare(
        "SELECT
            objects.object_id,
            objects.store_id,
            stores.class,
            objects.object_type,
            objects.state,
            objects.size_bytes,
            objects.content_hash,
            objects.created_at_utc,
            objects.updated_at_utc
         FROM objects
         INNER JOIN stores ON stores.store_id = objects.store_id
         WHERE objects.store_id = ?1
         ORDER BY objects.object_id ASC",
    )?;
    let rows = statement.query_map([store_id.as_str()], |row| {
        object_summary_from_row(row, live_sqlite_path)
    })?;
    let mut summaries = Vec::new();
    for row in rows {
        summaries.push(row?);
    }

    let summary_indexes = summaries
        .iter()
        .enumerate()
        .map(|(index, summary)| (summary.object_id.as_str().to_string(), index))
        .collect::<HashMap<_, _>>();
    let mut placement_statement = connection.prepare(
        "SELECT
            placements.object_id,
            placements.placement_id,
            placements.disk_id,
            placements.relative_path,
            placements.content_hash,
            placements.verified_at_utc
         FROM placements
         INNER JOIN objects ON objects.object_id = placements.object_id
         WHERE objects.store_id = ?1
         ORDER BY placements.object_id ASC,
                  placements.verified_at_utc IS NULL,
                  placements.verified_at_utc ASC,
                  placements.placement_id ASC",
    )?;
    let placements = placement_statement.query_map([store_id.as_str()], |row| {
        let object_id = row.get::<_, String>(0)?;
        let placement_id = parse_id("placement_id", row.get::<_, String>(1)?)?;
        let disk_id = parse_id("disk_id", row.get::<_, String>(2)?)?;
        Ok((
            object_id,
            ObjectPlacementSummary {
                placement_id,
                disk_id,
                relative_path: row.get(3)?,
                content_hash: row.get(4)?,
                verified_at_utc: row.get(5)?,
            },
        ))
    })?;
    for row in placements {
        let (object_id, placement) = row?;
        if let Some(index) = summary_indexes.get(&object_id) {
            summaries[*index].placements.push(placement);
        }
    }

    Ok(summaries)
}

#[derive(Debug)]
pub enum ObjectInspectError {
    Sqlite(rusqlite::Error),
    ObjectNotFound(ObjectId),
    InvalidIdentifier {
        field: &'static str,
        source: InvalidId,
    },
    InvalidObjectType {
        value: String,
        source: ObjectTypeParseError,
    },
    NegativeByteCount {
        field: &'static str,
        value: i64,
    },
}

impl Display for ObjectInspectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(err) => write!(formatter, "failed to read object metadata: {err}"),
            Self::ObjectNotFound(object_id) => {
                write!(formatter, "object `{object_id}` was not found")
            }
            Self::InvalidIdentifier { field, source } => {
                write!(formatter, "invalid object metadata {field}: {source}")
            }
            Self::InvalidObjectType { value, source } => {
                write!(
                    formatter,
                    "invalid object metadata object_type `{value}`: {source}"
                )
            }
            Self::NegativeByteCount { field, value } => {
                write!(
                    formatter,
                    "invalid negative object metadata {field}: {value}"
                )
            }
        }
    }
}

impl std::error::Error for ObjectInspectError {}

impl From<rusqlite::Error> for ObjectInspectError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Sqlite(err)
    }
}

fn read_object_row(
    connection: &Connection,
    live_sqlite_path: &Path,
    object_id: &ObjectId,
) -> Result<ObjectInspectSummary, ObjectInspectError> {
    connection
        .query_row(
            "SELECT
                objects.object_id,
                objects.store_id,
                stores.class,
                objects.object_type,
                objects.state,
                objects.size_bytes,
                objects.content_hash,
                objects.created_at_utc,
                objects.updated_at_utc
             FROM objects
             INNER JOIN stores ON stores.store_id = objects.store_id
             WHERE objects.object_id = ?1",
            [object_id.as_str()],
            |row| object_summary_from_row(row, live_sqlite_path),
        )
        .optional()?
        .ok_or_else(|| ObjectInspectError::ObjectNotFound(object_id.clone()))
}

fn object_summary_from_row(
    row: &Row<'_>,
    live_sqlite_path: &Path,
) -> Result<ObjectInspectSummary, rusqlite::Error> {
    let object_id = parse_id("object_id", row.get::<_, String>(0)?)?;
    let store_id = parse_id("store_id", row.get::<_, String>(1)?)?;
    let object_type = parse_object_type(row.get::<_, String>(3)?)?;
    let size_bytes = optional_u64("size_bytes", row.get::<_, Option<i64>>(5)?)?;

    Ok(ObjectInspectSummary {
        live_sqlite_path: live_sqlite_path.to_path_buf(),
        object_id,
        store_id,
        store_class: row.get(2)?,
        object_type,
        state: row.get(4)?,
        size_bytes,
        content_hash: row.get(6)?,
        created_at_utc: row.get(7)?,
        updated_at_utc: row.get(8)?,
        placements: Vec::new(),
    })
}

fn parse_object_type(value: String) -> Result<ObjectType, rusqlite::Error> {
    value.parse().map_err(|source| {
        rusqlite::Error::FromSqlConversionFailure(
            3,
            Type::Text,
            Box::new(ObjectInspectError::InvalidObjectType { value, source }),
        )
    })
}

fn read_object_placements(
    connection: &Connection,
    object_id: &ObjectId,
) -> Result<Vec<ObjectPlacementSummary>, ObjectInspectError> {
    let mut statement = connection.prepare(
        "SELECT
            placement_id,
            disk_id,
            relative_path,
            content_hash,
            verified_at_utc
         FROM placements
         WHERE object_id = ?1
         ORDER BY verified_at_utc IS NULL, verified_at_utc ASC, placement_id ASC",
    )?;
    let rows = statement.query_map([object_id.as_str()], |row| {
        let placement_id = parse_id("placement_id", row.get::<_, String>(0)?)?;
        let disk_id = parse_id("disk_id", row.get::<_, String>(1)?)?;

        Ok(ObjectPlacementSummary {
            placement_id,
            disk_id,
            relative_path: row.get(2)?,
            content_hash: row.get(3)?,
            verified_at_utc: row.get(4)?,
        })
    })?;

    let mut placements = Vec::new();
    for row in rows {
        placements.push(row?);
    }

    Ok(placements)
}

fn parse_id<T>(field: &'static str, value: String) -> Result<T, rusqlite::Error>
where
    T: std::str::FromStr<Err = InvalidId>,
{
    value.parse().map_err(|source| {
        rusqlite::Error::ToSqlConversionFailure(Box::new(ObjectInspectError::InvalidIdentifier {
            field,
            source,
        }))
    })
}

fn optional_u64(field: &'static str, value: Option<i64>) -> Result<Option<u64>, rusqlite::Error> {
    value
        .map(|value| {
            u64::try_from(value).map_err(|_| {
                rusqlite::Error::ToSqlConversionFailure(Box::new(
                    ObjectInspectError::NegativeByteCount { field, value },
                ))
            })
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::{read_object_inspect, read_store_object_inspects, ObjectInspectError};
    use crate::LIVE_SCHEMA_SQL;
    use dasobjectstore_core::ids::{ObjectId, StoreId};
    use dasobjectstore_core::object_type::ObjectType;
    use rusqlite::Connection;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn reads_object_summary_with_verified_placements() {
        let root = temp_root("object-inspect");
        fs::create_dir_all(&root).expect("create root");
        let live_sqlite_path = root.join("live.sqlite");
        let connection = fixture_connection(&live_sqlite_path);
        insert_object_fixture(&connection);

        let summary = read_object_inspect(&live_sqlite_path, &object_id()).expect("object inspect");

        assert_eq!(summary.object_id.as_str(), "object-a");
        assert_eq!(summary.store_id.as_str(), "store-a");
        assert_eq!(summary.store_class, "generated_data");
        assert_eq!(summary.object_type, ObjectType::Bam);
        assert_eq!(summary.state, "Protected");
        assert_eq!(summary.size_bytes, Some(128));
        assert_eq!(summary.content_hash.as_deref(), Some("sha256:object-a"));
        assert_eq!(summary.placements.len(), 2);
        assert_eq!(summary.placements[0].disk_id.as_str(), "disk-a");
        assert_eq!(summary.placements[1].disk_id.as_str(), "disk-b");

        fs::remove_dir_all(root).expect("cleanup root");
    }

    #[test]
    fn reports_missing_object() {
        let root = temp_root("object-inspect-missing");
        fs::create_dir_all(&root).expect("create root");
        let live_sqlite_path = root.join("live.sqlite");
        let _connection = fixture_connection(&live_sqlite_path);

        let err = read_object_inspect(&live_sqlite_path, &object_id()).expect_err("missing object");

        assert!(matches!(err, ObjectInspectError::ObjectNotFound(_)));

        fs::remove_dir_all(root).expect("cleanup root");
    }

    #[test]
    fn reads_store_objects_and_placements_in_bulk() {
        let root = temp_root("store-object-inspects");
        fs::create_dir_all(&root).expect("create root");
        let live_sqlite_path = root.join("live.sqlite");
        let connection = fixture_connection(&live_sqlite_path);
        insert_object_fixture(&connection);

        let summaries = read_store_object_inspects(
            &live_sqlite_path,
            &StoreId::new("store-a").expect("store id"),
        )
        .expect("store object inspects");

        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].object_id.as_str(), "object-a");
        assert_eq!(summaries[0].placements.len(), 2);
        assert_eq!(summaries[0].placements[0].disk_id.as_str(), "disk-a");
        assert_eq!(summaries[0].placements[1].disk_id.as_str(), "disk-b");

        fs::remove_dir_all(root).expect("cleanup root");
    }

    fn fixture_connection(path: &PathBuf) -> Connection {
        let connection = Connection::open(path).expect("open sqlite");
        connection
            .execute_batch(LIVE_SCHEMA_SQL)
            .expect("schema applies");
        connection
    }

    fn insert_object_fixture(connection: &Connection) {
        connection
            .execute_batch(
                "INSERT INTO pools (pool_id, state, created_at_utc, updated_at_utc)
                 VALUES ('pool-a', 'Clean', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
                 INSERT INTO stores (
                    store_id, pool_id, class, policy_json, created_at_utc, updated_at_utc
                 ) VALUES (
                    'store-a', 'pool-a', 'generated_data', '{}',
                    '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'
                 );
                 INSERT INTO disks (
                    disk_id, pool_id, role, state, created_at_utc, updated_at_utc
                 ) VALUES
                    ('disk-a', 'pool-a', 'hdd_capacity', 'Healthy',
                     '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'),
                    ('disk-b', 'pool-a', 'hdd_capacity', 'Healthy',
                     '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
                 INSERT INTO objects (
                    object_id, store_id, object_type, state, size_bytes, content_hash,
                    created_at_utc, updated_at_utc
                 ) VALUES (
                    'object-a', 'store-a', 'bam', 'Protected', 128, 'sha256:object-a',
                    '2026-01-02T00:00:00Z', '2026-01-03T00:00:00Z'
                 );
                 INSERT INTO placements (
                    placement_id, object_id, disk_id, relative_path, content_hash,
                    verified_at_utc, created_at_utc
                 ) VALUES
                    ('placement-a', 'object-a', 'disk-a', 'objects/aa/object-a',
                     'sha256:object-a', '2026-01-03T00:00:00Z',
                     '2026-01-02T00:00:00Z'),
                    ('placement-b', 'object-a', 'disk-b', 'objects/bb/object-a',
                     'sha256:object-a', '2026-01-03T00:01:00Z',
                     '2026-01-02T00:00:00Z');",
            )
            .expect("fixture inserts");
    }

    fn object_id() -> ObjectId {
        ObjectId::new("object-a").expect("object id")
    }

    fn temp_root(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dasobjectstore-metadata-{name}-{}-{nanos}",
            std::process::id()
        ))
    }
}
