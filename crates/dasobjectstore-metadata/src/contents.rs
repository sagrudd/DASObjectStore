use dasobjectstore_core::{
    ids::StoreId,
    object_catalogue::{PortableObjectVersion, PortableProtectionState},
};
use regex::Regex;
use rusqlite::{params, Connection, OpenFlags};
use serde::Serialize;
use std::path::PathBuf;
use std::{
    collections::BTreeSet,
    fmt::{self, Display},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreContentsRequest {
    pub live_sqlite_path: PathBuf,
    pub store_id: StoreId,
    pub filter: Option<String>,
    pub prefix: Option<String>,
}

impl StoreContentsRequest {
    pub fn new(live_sqlite_path: impl Into<PathBuf>, store_id: StoreId) -> Self {
        Self {
            live_sqlite_path: live_sqlite_path.into(),
            store_id,
            filter: None,
            prefix: None,
        }
    }

    pub fn with_filter(mut self, filter: impl Into<String>) -> Self {
        self.filter = Some(filter.into());
        self
    }

    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into().trim_matches('/').to_string());
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StoreContentsSnapshot {
    pub live_sqlite_path: PathBuf,
    pub store_id: StoreId,
    pub filter: Option<String>,
    pub prefix: Option<String>,
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
    pub kind: String,
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
    let filter = request
        .filter
        .as_ref()
        .map(|pattern| Regex::new(pattern))
        .transpose()?;
    if !table_exists(&connection, "stores")? || !table_exists(&connection, "objects")? {
        return Ok(StoreContentsSnapshot {
            live_sqlite_path: request.live_sqlite_path.clone(),
            store_id: request.store_id.clone(),
            filter: request.filter.clone(),
            prefix: request.prefix.clone(),
            objects: Vec::new(),
        });
    }
    ensure_store_exists(&connection, &request.store_id)?;
    let mut statement = connection.prepare(
        "SELECT object_id, object_type, state, COALESCE(size_bytes, 0), updated_at_utc
         FROM objects
         WHERE store_id = ?1
         ORDER BY object_id ASC",
    )?;
    let mut rows = statement.query(params![request.store_id.as_str()])?;
    let mut objects = Vec::new();
    let mut seen_object_ids = BTreeSet::new();
    while let Some(row) = rows.next()? {
        let object_id = row.get::<_, String>(0)?;
        let path = relative_object_path(&request.store_id, &object_id);
        if request
            .prefix
            .as_ref()
            .is_some_and(|prefix| path != *prefix && !path.starts_with(&format!("{prefix}/")))
        {
            continue;
        }
        if filter
            .as_ref()
            .is_some_and(|regex| !regex.is_match(&path) && !regex.is_match(&object_id))
        {
            continue;
        }
        let size_bytes = checked_size_bytes(row.get::<_, i64>(3)?)?;
        let display_path = request.prefix.as_ref().map_or_else(
            || path.clone(),
            |prefix| {
                path.strip_prefix(prefix)
                    .unwrap_or(&path)
                    .trim_matches('/')
                    .to_string()
            },
        );
        objects.push(StoreContentsObject {
            object_id: object_id.clone(),
            path: display_path,
            kind: "file".to_string(),
            object_type: row.get(1)?,
            state: row.get(2)?,
            size_bytes,
            updated_at_utc: row.get(4)?,
        });
        seen_object_ids.insert(object_id);
    }

    if table_exists(&connection, "profile_catalogue_objects")? {
        let mut statement = connection.prepare(
            "SELECT object_id, object_version, object_json, committed_at_utc
             FROM profile_catalogue_objects
             WHERE store_id = ?1
             ORDER BY object_id ASC, object_version DESC",
        )?;
        let mut rows = statement.query(params![request.store_id.as_str()])?;
        while let Some(row) = rows.next()? {
            let row_object_id = row.get::<_, String>(0)?;
            if seen_object_ids.contains(&row_object_id) {
                continue;
            }
            let row_version = checked_object_version(row.get::<_, i64>(1)?)?;
            let object: PortableObjectVersion = serde_json::from_str(&row.get::<_, String>(2)?)
                .map_err(StoreContentsReadError::InvalidProfileObject)?;
            let object_id = object.object_id.to_string();
            if object_id != row_object_id || object.version != row_version {
                return Err(StoreContentsReadError::ProfileObjectIdentityMismatch {
                    object_id: row_object_id,
                    object_version: row_version,
                });
            }
            let path = relative_object_path(&request.store_id, &object_id);
            if request
                .prefix
                .as_ref()
                .is_some_and(|prefix| path != *prefix && !path.starts_with(&format!("{prefix}/")))
                || filter
                    .as_ref()
                    .is_some_and(|regex| !regex.is_match(&path) && !regex.is_match(&object_id))
            {
                continue;
            }
            let display_path = request.prefix.as_ref().map_or_else(
                || path.clone(),
                |prefix| {
                    path.strip_prefix(prefix)
                        .unwrap_or(&path)
                        .trim_matches('/')
                        .to_string()
                },
            );
            let state = match object.protection_state {
                PortableProtectionState::Verified | PortableProtectionState::Protected => {
                    "Protected"
                }
                PortableProtectionState::Unprotected => "Unprotected",
                PortableProtectionState::RedownloadRequired => "RedownloadRequired",
            };
            objects.push(StoreContentsObject {
                object_id: object_id.clone(),
                path: display_path,
                kind: "file".to_string(),
                object_type: "profile_object".to_string(),
                state: state.to_string(),
                size_bytes: object.size_bytes,
                updated_at_utc: row.get(3)?,
            });
            seen_object_ids.insert(object_id);
        }
    }

    objects.sort_by(|left, right| left.object_id.cmp(&right.object_id));

    Ok(StoreContentsSnapshot {
        live_sqlite_path: request.live_sqlite_path.clone(),
        store_id: request.store_id.clone(),
        filter: request.filter.clone(),
        prefix: request.prefix.clone(),
        objects,
    })
}

fn table_exists(connection: &Connection, table_name: &str) -> Result<bool, StoreContentsReadError> {
    let count = connection.query_row(
        "SELECT COUNT(*)
         FROM sqlite_master
         WHERE type = 'table'
           AND name = ?1",
        params![table_name],
        |row| row.get::<_, i64>(0),
    )?;
    Ok(count > 0)
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

fn checked_object_version(value: i64) -> Result<u64, StoreContentsReadError> {
    u64::try_from(value)
        .ok()
        .filter(|version| *version > 0)
        .ok_or(StoreContentsReadError::InvalidProfileObjectVersion { value })
}

#[derive(Debug)]
pub enum StoreContentsReadError {
    Sqlite(rusqlite::Error),
    InvalidFilter(regex::Error),
    StoreNotFound {
        store_id: StoreId,
    },
    NegativeByteCount {
        value: i64,
    },
    InvalidProfileObject(serde_json::Error),
    InvalidProfileObjectVersion {
        value: i64,
    },
    ProfileObjectIdentityMismatch {
        object_id: String,
        object_version: u64,
    },
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
            Self::InvalidProfileObject(err) => {
                write!(formatter, "invalid profile catalogue object: {err}")
            }
            Self::InvalidProfileObjectVersion { value } => {
                write!(formatter, "invalid profile catalogue object version: {value}")
            }
            Self::ProfileObjectIdentityMismatch {
                object_id,
                object_version,
            } => write!(
                formatter,
                "profile catalogue row identity does not match object `{object_id}` version {object_version}"
            ),
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
    fn includes_latest_verified_provider_catalogue_objects() {
        let root = temp_root("contents-provider-catalogue");
        let live_sqlite_path = create_live_sqlite(&root);
        insert_profile_object(&live_sqlite_path, "artists/account/image.jpg", 6, 256);
        insert_profile_object(&live_sqlite_path, "artists/account/image.jpg", 7, 512);

        let snapshot = read_store_contents(&StoreContentsRequest::new(
            &live_sqlite_path,
            StoreId::new("zymo_fecal_2025.05").expect("store id"),
        ))
        .expect("provider contents read");

        assert_eq!(snapshot.objects.len(), 1);
        assert_eq!(snapshot.objects[0].object_id, "artists/account/image.jpg");
        assert_eq!(snapshot.objects[0].state, "Protected");
        assert_eq!(snapshot.objects[0].object_type, "profile_object");
        assert_eq!(snapshot.objects[0].size_bytes, 512);

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn locally_landed_object_precedes_the_same_provider_key() {
        let root = temp_root("contents-provider-deduplication");
        let live_sqlite_path = create_live_sqlite(&root);
        let object_id = "artists/account/image.jpg";
        insert_object(&live_sqlite_path, object_id, 128, "image");
        insert_profile_object(&live_sqlite_path, object_id, 7, 512);

        let snapshot = read_store_contents(&StoreContentsRequest::new(
            &live_sqlite_path,
            StoreId::new("zymo_fecal_2025.05").expect("store id"),
        ))
        .expect("deduplicated contents read");

        assert_eq!(snapshot.objects.len(), 1);
        assert_eq!(snapshot.objects[0].object_type, "image");
        assert_eq!(snapshot.objects[0].size_bytes, 128);

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

    #[test]
    fn scopes_contents_to_a_folder_prefix_and_rebases_paths() {
        let root = temp_root("contents-prefix");
        let live_sqlite_path = create_live_sqlite(&root);
        insert_object(
            &live_sqlite_path,
            "zymo_fecal_2025.05/PRJEB33511/sample-one.pod5",
            128,
            "pod5",
        );
        insert_object(
            &live_sqlite_path,
            "zymo_fecal_2025.05/PRJNA1011899/sample-two.pod5",
            256,
            "pod5",
        );

        let snapshot = read_store_contents(
            &StoreContentsRequest::new(
                &live_sqlite_path,
                StoreId::new("zymo_fecal_2025.05").expect("store id"),
            )
            .with_prefix("PRJEB33511"),
        )
        .expect("contents read");

        assert_eq!(snapshot.objects.len(), 1);
        assert_eq!(snapshot.objects[0].path, "sample-one.pod5");
        assert_eq!(snapshot.objects[0].kind, "file");
        assert_eq!(snapshot.total_size_bytes(), 128);

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn reads_empty_contents_from_older_live_sqlite_without_contents_tables() {
        let root = temp_root("contents-old-schema");
        fs::create_dir_all(&root).expect("create temp root");
        let live_sqlite_path = root.join("live.sqlite");
        Connection::open(&live_sqlite_path).expect("open empty sqlite");

        let snapshot = read_store_contents(&StoreContentsRequest::new(
            &live_sqlite_path,
            StoreId::new("zymo_fecal_2025.05").expect("store id"),
        ))
        .expect("contents read");

        assert!(snapshot.objects.is_empty());

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

    fn insert_profile_object(
        live_sqlite_path: &Path,
        object_id: &str,
        object_version: i64,
        size_bytes: u64,
    ) {
        let connection = Connection::open(live_sqlite_path).expect("open sqlite");
        let transaction_id = format!("provider-{object_version}");
        let object_json = serde_json::json!({
            "object_id": object_id,
            "version": object_version,
            "size_bytes": size_bytes,
            "checksum": {
                "algorithm": "sha256",
                "value": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            },
            "provenance": {
                "source_kind": "remote_upload",
                "locator": object_id,
                "revision": null
            },
            "lifecycle": "hash_verified",
            "protection_policy": "externally_replicated",
            "protection_state": "verified",
            "placements": [{
                "placement_id": format!("provider-{object_version}"),
                "location": {
                    "kind": "provider",
                    "provider": "garage",
                    "object_key": format!("bucket/{object_id}")
                },
                "checksum": {
                    "algorithm": "sha256",
                    "value": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                },
                "verified_at_utc": null
            }]
        })
        .to_string();
        connection
            .execute(
                "INSERT INTO profile_catalogue_transactions (
                    transaction_id, profile_namespace, store_id, schema_version,
                    source_retained, catalogue_json, committed_at_utc
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    transaction_id,
                    "provider:garage",
                    "zymo_fecal_2025.05",
                    1,
                    1,
                    "{}",
                    "2026-01-04T00:00:00Z"
                ],
            )
            .expect("profile transaction inserts");
        connection
            .execute(
                "INSERT INTO profile_catalogue_objects (
                    profile_namespace, store_id, object_id, object_version,
                    transaction_id, object_json, committed_at_utc
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    "provider:garage",
                    "zymo_fecal_2025.05",
                    object_id,
                    object_version,
                    transaction_id,
                    object_json,
                    "2026-01-04T00:00:00Z"
                ],
            )
            .expect("profile object inserts");
    }

    fn temp_root(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!("dasobjectstore-metadata-{name}-{nanos}"))
    }
}
