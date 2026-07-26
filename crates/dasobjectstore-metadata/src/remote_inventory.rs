//! Stable, bounded inventory reads for authenticated remote workflows.

use dasobjectstore_core::ids::{ObjectId, StoreId};
use rusqlite::{params, Connection, OpenFlags};
use std::fmt::{self, Display};
use std::path::Path;
use std::time::Duration;

const BUSY_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteObjectInventoryRecord {
    pub object_key: String,
    pub object_version: u64,
    pub object_id: ObjectId,
    pub size_bytes: u64,
    pub content_hash_algorithm: String,
    pub content_hash: String,
    pub lifecycle_state: String,
    pub updated_at_utc: String,
    pub active_ssd_copy: bool,
    pub hdd_copy_count: u64,
    pub verified_hdd_copy_count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteObjectInventoryPage {
    pub snapshot_high_water: u64,
    pub total_objects: u64,
    pub objects: Vec<RemoteObjectInventoryRecord>,
    pub next_key: Option<String>,
    pub next_version: Option<u64>,
}

/// Reads one keyset-paginated page from a fixed insertion high-water mark.
///
/// The high-water mark prevents newly published bindings from moving objects
/// between pages. Callers retain it in their opaque cursor. The read-only
/// connection and one aggregate query keep large inventories bounded without
/// holding a write transaction.
pub fn read_remote_object_inventory_page(
    path: impl AsRef<Path>,
    store_id: &StoreId,
    prefix: &str,
    snapshot_high_water: Option<u64>,
    after: Option<(&str, u64)>,
    limit: u32,
) -> Result<RemoteObjectInventoryPage, RemoteObjectInventoryError> {
    if limit == 0 {
        return Err(RemoteObjectInventoryError::InvalidLimit);
    }
    if prefix.contains('\0') {
        return Err(RemoteObjectInventoryError::InvalidPrefix);
    }
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    connection.busy_timeout(BUSY_TIMEOUT)?;
    let discovered_high_water = connection.query_row(
        "SELECT COALESCE(MAX(rowid),0) FROM s3_object_bindings WHERE store_id=?1",
        [store_id.as_str()],
        |row| checked_u64("snapshot_high_water", row.get::<_, i64>(0)?),
    )?;
    let high_water = snapshot_high_water.unwrap_or(discovered_high_water);
    let high_water_i64 = checked_i64("snapshot_high_water", high_water)?;
    let prefix_pattern = format!("{}%", escape_like(prefix));
    let total_objects = connection.query_row(
        "SELECT COUNT(*) FROM s3_object_bindings
         WHERE store_id=?1 AND rowid<=?2 AND object_key LIKE ?3 ESCAPE '\\'",
        params![store_id.as_str(), high_water_i64, prefix_pattern],
        |row| checked_u64("total_objects", row.get::<_, i64>(0)?),
    )?;
    let (after_key, after_version) = after.unwrap_or(("", 0));
    let fetch = i64::from(limit) + 1;
    let mut statement = connection.prepare(
        "SELECT b.object_key,b.object_version,b.object_id,b.size_bytes,
                b.content_hash_algorithm,b.content_hash,o.state,b.updated_at_utc,
                EXISTS(
                    SELECT 1 FROM ssd_object_placements s
                    WHERE s.object_id=b.object_id AND s.evicted_at_utc IS NULL
                ),
                (SELECT COUNT(*) FROM placements p WHERE p.object_id=b.object_id),
                (SELECT COUNT(*) FROM placements p
                    WHERE p.object_id=b.object_id AND p.verified_at_utc IS NOT NULL)
         FROM s3_object_bindings b
         JOIN objects o ON o.object_id=b.object_id AND o.store_id=b.store_id
         WHERE b.store_id=?1
           AND b.rowid<=?2
           AND b.object_key LIKE ?3 ESCAPE '\\'
           AND (b.object_key>?4 OR (b.object_key=?4 AND b.object_version>?5))
         ORDER BY b.object_key ASC,b.object_version ASC
         LIMIT ?6",
    )?;
    let rows = statement.query_map(
        params![
            store_id.as_str(),
            high_water_i64,
            prefix_pattern,
            after_key,
            checked_i64("after_version", after_version)?,
            fetch,
        ],
        |row| {
            Ok(RemoteObjectInventoryRecord {
                object_key: row.get(0)?,
                object_version: checked_u64("object_version", row.get::<_, i64>(1)?)?,
                object_id: ObjectId::new(row.get::<_, String>(2)?).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        2,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?,
                size_bytes: checked_u64("size_bytes", row.get::<_, i64>(3)?)?,
                content_hash_algorithm: row.get(4)?,
                content_hash: row.get(5)?,
                lifecycle_state: row.get(6)?,
                updated_at_utc: row.get(7)?,
                active_ssd_copy: row.get(8)?,
                hdd_copy_count: checked_u64("hdd_copy_count", row.get::<_, i64>(9)?)?,
                verified_hdd_copy_count: checked_u64(
                    "verified_hdd_copy_count",
                    row.get::<_, i64>(10)?,
                )?,
            })
        },
    )?;
    let mut objects = rows.collect::<Result<Vec<_>, _>>()?;
    let truncated = objects.len() > limit as usize;
    objects.truncate(limit as usize);
    let (next_key, next_version) = if truncated {
        let last = objects.last().expect("truncated page is non-empty");
        (Some(last.object_key.clone()), Some(last.object_version))
    } else {
        (None, None)
    };
    Ok(RemoteObjectInventoryPage {
        snapshot_high_water: high_water,
        total_objects,
        objects,
        next_key,
        next_version,
    })
}

fn checked_u64(field: &'static str, value: i64) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Integer,
            Box::new(RemoteObjectInventoryError::NegativeValue { field, value }),
        )
    })
}

fn checked_i64(field: &'static str, value: u64) -> Result<i64, RemoteObjectInventoryError> {
    i64::try_from(value).map_err(|_| RemoteObjectInventoryError::ValueOverflow { field, value })
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

#[derive(Debug)]
pub enum RemoteObjectInventoryError {
    Sqlite(rusqlite::Error),
    InvalidLimit,
    InvalidPrefix,
    NegativeValue { field: &'static str, value: i64 },
    ValueOverflow { field: &'static str, value: u64 },
}

impl Display for RemoteObjectInventoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => write!(formatter, "remote inventory query failed: {error}"),
            Self::InvalidLimit => write!(formatter, "remote inventory limit must be positive"),
            Self::InvalidPrefix => write!(formatter, "remote inventory prefix contains NUL"),
            Self::NegativeValue { field, value } => {
                write!(formatter, "remote inventory {field} is negative: {value}")
            }
            Self::ValueOverflow { field, value } => {
                write!(
                    formatter,
                    "remote inventory {field} exceeds SQLite range: {value}"
                )
            }
        }
    }
}

impl std::error::Error for RemoteObjectInventoryError {}

impl RemoteObjectInventoryError {
    /// Stable daemon/API classification for failures at the authoritative
    /// catalogue boundary. Callers must not infer retryability from prose.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Sqlite(rusqlite::Error::SqliteFailure(error, _)) => {
                use rusqlite::ffi::ErrorCode;
                match error.code {
                    ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked => "catalogue_locked",
                    ErrorCode::PermissionDenied | ErrorCode::ReadOnly => {
                        "catalogue_permission_denied"
                    }
                    ErrorCode::CannotOpen | ErrorCode::NotADatabase => "catalogue_unavailable",
                    _ => "catalogue_query_failed",
                }
            }
            Self::Sqlite(_) => "catalogue_query_failed",
            Self::InvalidLimit | Self::InvalidPrefix => "invalid_remote_control_request",
            Self::NegativeValue { .. } | Self::ValueOverflow { .. } => {
                "catalogue_invariant_violation"
            }
        }
    }
}

impl From<rusqlite::Error> for RemoteObjectInventoryError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

#[cfg(test)]
mod tests {
    use super::{read_remote_object_inventory_page, RemoteObjectInventoryError};
    use crate::schema::LIVE_SCHEMA_SQL;
    use dasobjectstore_core::ids::StoreId;
    use rusqlite::{params, Connection};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn classifies_catalogue_failures_without_parsing_messages() {
        let locked = RemoteObjectInventoryError::Sqlite(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            None,
        ));
        let denied = RemoteObjectInventoryError::Sqlite(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_PERM),
            None,
        ));
        let unavailable = RemoteObjectInventoryError::Sqlite(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CANTOPEN),
            None,
        ));

        assert_eq!(locked.code(), "catalogue_locked");
        assert_eq!(denied.code(), "catalogue_permission_denied");
        assert_eq!(unavailable.code(), "catalogue_unavailable");
    }

    #[test]
    fn reads_twenty_thousand_objects_in_one_bounded_page() {
        let path = temp_path("twenty-thousand");
        seed_objects(&path, 20_000);
        let store_id = StoreId::new("epic_collection").expect("store");

        let page =
            read_remote_object_inventory_page(&path, &store_id, "EPICv1/", None, None, 20_000)
                .expect("inventory");

        assert_eq!(page.objects.len(), 20_000);
        assert_eq!(page.total_objects, 20_000);
        assert!(page.next_key.is_none());
        assert_eq!(
            page.objects.first().expect("first").object_key,
            "EPICv1/object-00000"
        );
        assert_eq!(
            page.objects.last().expect("last").object_key,
            "EPICv1/object-19999"
        );
        fs::remove_file(path).expect("cleanup");
    }

    #[test]
    fn cursor_high_water_excludes_concurrent_publications() {
        let path = temp_path("stable-page");
        seed_objects(&path, 3);
        let store_id = StoreId::new("epic_collection").expect("store");
        let first = read_remote_object_inventory_page(&path, &store_id, "EPICv1/", None, None, 2)
            .expect("first page");
        insert_object(&Connection::open(&path).expect("open"), 99);
        let second = read_remote_object_inventory_page(
            &path,
            &store_id,
            "EPICv1/",
            Some(first.snapshot_high_water),
            Some((
                first.next_key.as_deref().expect("next key"),
                first.next_version.expect("next version"),
            )),
            2,
        )
        .expect("second page");

        assert_eq!(first.total_objects, 3);
        assert_eq!(second.total_objects, 3);
        assert_eq!(second.objects.len(), 1);
        assert_eq!(second.objects[0].object_key, "EPICv1/object-00002");
        fs::remove_file(path).expect("cleanup");
    }

    fn seed_objects(path: &PathBuf, count: usize) {
        let mut connection = Connection::open(path).expect("open");
        connection.execute_batch(LIVE_SCHEMA_SQL).expect("schema");
        connection
            .execute(
                "INSERT INTO pools(pool_id,state,created_at_utc,updated_at_utc)
                 VALUES('pool-a','clean','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')",
                [],
            )
            .expect("pool");
        connection
            .execute(
                "INSERT INTO stores(store_id,pool_id,class,policy_json,created_at_utc,updated_at_utc)
                 VALUES('epic_collection','pool-a','generated_data','{}',
                 '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')",
                [],
            )
            .expect("store");
        let transaction = connection.transaction().expect("transaction");
        for index in 0..count {
            insert_object(&transaction, index);
        }
        transaction.commit().expect("commit");
    }

    fn insert_object(connection: &Connection, index: usize) {
        let object_id = format!("object-{index:05}");
        let key = format!("EPICv1/object-{index:05}");
        connection
            .execute(
                "INSERT INTO objects(
                    object_id,store_id,object_type,state,size_bytes,content_hash,
                    created_at_utc,updated_at_utc
                 ) VALUES(?1,'epic_collection','naive','copying_to_hdd',42,?2,
                    '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')",
                params![object_id, "a".repeat(64)],
            )
            .expect("object");
        connection
            .execute(
                "INSERT INTO s3_object_bindings(
                    store_id,object_key,object_version,object_id,size_bytes,
                    content_hash_algorithm,content_hash,created_at_utc,updated_at_utc
                 ) VALUES('epic_collection',?1,1,?2,42,'sha256',?3,
                    '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')",
                params![key, object_id, "a".repeat(64)],
            )
            .expect("binding");
    }

    fn temp_path(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dasobjectstore-remote-inventory-{label}-{}-{nonce}.sqlite",
            std::process::id()
        ))
    }
}
