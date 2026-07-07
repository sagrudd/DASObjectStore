use dasobjectstore_core::ids::StoreId;
use regex::Regex;
use rusqlite::{params, Connection, OpenFlags};
use serde::Serialize;
use std::fmt::{self, Display};
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreContentsRequest {
    pub live_sqlite_path: PathBuf,
    pub store_id: StoreId,
    pub filter: Option<String>,
}

impl StoreContentsRequest {
    pub fn new(live_sqlite_path: impl Into<PathBuf>, store_id: StoreId) -> Self {
        Self {
            live_sqlite_path: live_sqlite_path.into(),
            store_id,
            filter: None,
        }
    }

    pub fn with_filter(mut self, filter: impl Into<String>) -> Self {
        self.filter = Some(filter.into());
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StoreContentsSnapshot {
    pub live_sqlite_path: PathBuf,
    pub store_id: StoreId,
    pub filter: Option<String>,
    pub objects: Vec<StoreContentsObject>,
}

impl StoreContentsSnapshot {
    pub fn total_size_bytes(&self) -> u64 {
        self.objects
            .iter()
            .map(|object| object.size_bytes)
            .sum::<u64>()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StoreContentsObject {
    pub object_id: String,
    pub path: String,
    pub object_type: String,
    pub state: String,
    pub size_bytes: u64,
    pub updated_at_utc: String,
}

pub fn read_store_contents(
    request: &StoreContentsRequest,
) -> Result<StoreContentsSnapshot, StoreContentsReadError> {
    let connection =
        Connection::open_with_flags(&request.live_sqlite_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    ensure_store_exists(&connection, &request.store_id)?;
    let filter = request
        .filter
        .as_ref()
        .map(|pattern| Regex::new(pattern))
        .transpose()?;
    let mut statement = connection.prepare(
        "SELECT object_id, object_type, state, COALESCE(size_bytes, 0), updated_at_utc
         FROM objects
         WHERE store_id = ?1
         ORDER BY object_id ASC",
    )?;
    let mut rows = statement.query(params![request.store_id.as_str()])?;
    let mut objects = Vec::new();
    while let Some(row) = rows.next()? {
        let object_id = row.get::<_, String>(0)?;
        let path = relative_object_path(&request.store_id, &object_id);
        if filter
            .as_ref()
            .is_some_and(|regex| !regex.is_match(&path) && !regex.is_match(&object_id))
        {
            continue;
        }
        let size_bytes = checked_size_bytes(row.get::<_, i64>(3)?)?;
        objects.push(StoreContentsObject {
            object_id,
            path,
            object_type: row.get(1)?,
            state: row.get(2)?,
            size_bytes,
            updated_at_utc: row.get(4)?,
        });
    }

    Ok(StoreContentsSnapshot {
        live_sqlite_path: request.live_sqlite_path.clone(),
        store_id: request.store_id.clone(),
        filter: request.filter.clone(),
        objects,
    })
}

fn ensure_store_exists(
    connection: &Connection,
    store_id: &StoreId,
) -> Result<(), StoreContentsReadError> {
    let count = connection.query_row(
        "SELECT COUNT(*) FROM stores WHERE store_id = ?1",
        params![store_id.as_str()],
        |row| row.get::<_, i64>(0),
    )?;
    if count == 0 {
        return Err(StoreContentsReadError::StoreNotFound {
            store_id: store_id.clone(),
        });
    }
    Ok(())
}

fn relative_object_path(store_id: &StoreId, object_id: &str) -> String {
    let prefix = format!("{}/", store_id.as_str());
    object_id
        .strip_prefix(&prefix)
        .unwrap_or(object_id)
        .trim_matches('/')
        .to_string()
}

fn checked_size_bytes(value: i64) -> Result<u64, StoreContentsReadError> {
    value
        .try_into()
        .map_err(|_| StoreContentsReadError::NegativeByteCount { value })
}

#[derive(Debug)]
pub enum StoreContentsReadError {
    Sqlite(rusqlite::Error),
    InvalidFilter(regex::Error),
    StoreNotFound { store_id: StoreId },
    NegativeByteCount { value: i64 },
}

impl Display for StoreContentsReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(err) => write!(formatter, "failed to read store contents metadata: {err}"),
            Self::InvalidFilter(err) => write!(formatter, "invalid store contents filter: {err}"),
            Self::StoreNotFound { store_id } => {
                write!(
                    formatter,
                    "object store `{store_id}` was not found in live metadata"
                )
            }
            Self::NegativeByteCount { value } => {
                write!(
                    formatter,
                    "invalid negative object size in store contents: {value}"
                )
            }
        }
    }
}

impl std::error::Error for StoreContentsReadError {}

impl From<rusqlite::Error> for StoreContentsReadError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Sqlite(err)
    }
}

impl From<regex::Error> for StoreContentsReadError {
    fn from(err: regex::Error) -> Self {
        Self::InvalidFilter(err)
    }
}

#[cfg(test)]
mod tests {
    use super::{read_store_contents, StoreContentsRequest};
    use crate::LIVE_SCHEMA_SQL;
    use dasobjectstore_core::ids::{PoolId, StoreId};
    use dasobjectstore_core::store::{StoreClass, StorePolicy};
    use rusqlite::{params, Connection};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn reads_store_contents_relative_to_store_prefix() {
        let root = temp_root("contents-relative");
        let live_sqlite_path = create_live_sqlite(&root);
        insert_object(
            &live_sqlite_path,
            "zymo_fecal_2025.05/raw/sample.fastq.gz",
            128,
            "fastq",
        );
        insert_object(
            &live_sqlite_path,
            "zymo_fecal_2025.05/raw/nested/sample.pod5",
            256,
            "pod5",
        );

        let snapshot = read_store_contents(&StoreContentsRequest::new(
            &live_sqlite_path,
            StoreId::new("zymo_fecal_2025.05").expect("store id"),
        ))
        .expect("contents read");

        assert_eq!(snapshot.objects.len(), 2);
        assert_eq!(snapshot.objects[0].path, "raw/nested/sample.pod5");
        assert_eq!(snapshot.objects[1].path, "raw/sample.fastq.gz");
        assert_eq!(snapshot.total_size_bytes(), 384);

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn filters_store_contents_by_regex() {
        let root = temp_root("contents-filter");
        let live_sqlite_path = create_live_sqlite(&root);
        insert_object(
            &live_sqlite_path,
            "zymo_fecal_2025.05/raw/sample.fastq.gz",
            128,
            "fastq",
        );
        insert_object(
            &live_sqlite_path,
            "zymo_fecal_2025.05/raw/sample.pod5",
            256,
            "pod5",
        );

        let snapshot = read_store_contents(
            &StoreContentsRequest::new(
                &live_sqlite_path,
                StoreId::new("zymo_fecal_2025.05").expect("store id"),
            )
            .with_filter(r"\.pod5$"),
        )
        .expect("contents read");

        assert_eq!(snapshot.objects.len(), 1);
        assert_eq!(snapshot.objects[0].path, "raw/sample.pod5");

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    fn create_live_sqlite(root: &Path) -> PathBuf {
        fs::create_dir_all(root).expect("create temp root");
        let live_sqlite_path = root.join("live.sqlite");
        let connection = Connection::open(&live_sqlite_path).expect("open sqlite");
        connection
            .execute_batch(LIVE_SCHEMA_SQL)
            .expect("schema applies");
        connection
            .execute(
                "INSERT INTO pools (pool_id, state, created_at_utc, updated_at_utc)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    "pool-a",
                    "Clean",
                    "2026-01-01T00:00:00Z",
                    "2026-01-01T00:00:00Z"
                ],
            )
            .expect("pool inserts");
        let policy = StorePolicy::defaults_for(StoreClass::GeneratedData);
        connection
            .execute(
                "INSERT INTO stores (
                    store_id, pool_id, class, policy_json, created_at_utc, updated_at_utc
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    "zymo_fecal_2025.05",
                    PoolId::new("pool-a").expect("pool id").as_str(),
                    policy.class.name(),
                    serde_json::to_string(&policy).expect("policy serializes"),
                    "2026-01-01T00:00:00Z",
                    "2026-01-01T00:00:00Z"
                ],
            )
            .expect("store inserts");
        live_sqlite_path
    }

    fn insert_object(live_sqlite_path: &Path, object_id: &str, size_bytes: i64, object_type: &str) {
        let connection = Connection::open(live_sqlite_path).expect("open sqlite");
        connection
            .execute(
                "INSERT INTO objects (
                    object_id, store_id, object_type, state, size_bytes, content_hash,
                    created_at_utc, updated_at_utc
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    object_id,
                    "zymo_fecal_2025.05",
                    object_type,
                    "SsdEvictionEligible",
                    size_bytes,
                    format!("sha256:{object_id}"),
                    "2026-01-02T00:00:00Z",
                    "2026-01-03T00:00:00Z"
                ],
            )
            .expect("object inserts");
    }

    fn temp_root(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("dasobjectstore-metadata-{name}-{nanos}"))
    }
}
