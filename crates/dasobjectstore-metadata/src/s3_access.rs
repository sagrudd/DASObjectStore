//! Authoritative S3 key bindings over the native live catalogue.

use crate::schema::LIVE_SCHEMA_SQL;
use dasobjectstore_core::ids::{ObjectId, StoreId};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use std::fmt::{self, Display};
use std::path::Path;
use std::time::Duration;

const BUSY_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct S3ObjectBinding {
    pub store_id: StoreId,
    pub object_key: String,
    pub object_version: u64,
    pub object_id: ObjectId,
    pub size_bytes: u64,
    pub checksum: String,
    pub object_state: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct S3BindingBackfillReport {
    pub bindings_created: u64,
    pub bindings_existing: u64,
    pub objects_retained_unmapped: u64,
}

// Keep the complete authoritative binding explicit at this transaction
// boundary; callers must not be able to omit identity or integrity fields.
#[allow(clippy::too_many_arguments)]
pub(crate) fn bind_s3_object_in_transaction(
    tx: &Transaction<'_>,
    store_id: &StoreId,
    object_key: &str,
    object_version: u64,
    object_id: &ObjectId,
    size_bytes: u64,
    content_hash_algorithm: &str,
    content_hash: &str,
    committed_at_utc: &str,
) -> Result<(), S3AccessError> {
    validate_key(object_key, object_version)?;
    let size = i64::try_from(size_bytes).map_err(|_| S3AccessError::SizeOverflow(size_bytes))?;
    tx.execute(
        "INSERT INTO s3_object_bindings (
            store_id,object_key,object_version,object_id,size_bytes,
            content_hash_algorithm,content_hash,created_at_utc,updated_at_utc
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?8)
         ON CONFLICT(store_id,object_key,object_version) DO NOTHING",
        params![
            store_id.as_str(),
            object_key,
            object_version,
            object_id.as_str(),
            size,
            content_hash_algorithm,
            content_hash,
            committed_at_utc
        ],
    )?;
    let existing: (String, i64, String, String) = tx.query_row(
        "SELECT object_id,size_bytes,content_hash_algorithm,content_hash
         FROM s3_object_bindings
         WHERE store_id=?1 AND object_key=?2 AND object_version=?3",
        params![store_id.as_str(), object_key, object_version],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    if existing
        != (
            object_id.as_str().to_string(),
            size,
            content_hash_algorithm.to_string(),
            content_hash.to_string(),
        )
    {
        return Err(S3AccessError::BindingConflict {
            store_id: store_id.clone(),
            object_key: object_key.to_string(),
            object_version,
        });
    }
    Ok(())
}

pub fn read_s3_object_binding(
    path: impl AsRef<Path>,
    store_id: &StoreId,
    object_key: &str,
    object_version: u64,
) -> Result<Option<S3ObjectBinding>, S3AccessError> {
    validate_key(object_key, object_version)?;
    let connection = open(path)?;
    connection
        .query_row(
            "SELECT b.object_id,b.size_bytes,b.content_hash_algorithm,b.content_hash,o.state
             FROM s3_object_bindings b
             JOIN objects o ON o.object_id=b.object_id AND o.store_id=b.store_id
             WHERE b.store_id=?1 AND b.object_key=?2 AND b.object_version=?3",
            params![store_id.as_str(), object_key, object_version],
            |row| binding_from_row(store_id, object_key, object_version, row),
        )
        .optional()
        .map_err(Into::into)
}

pub fn list_s3_object_bindings(
    path: impl AsRef<Path>,
    store_id: &StoreId,
    prefix: Option<&str>,
    offset: u64,
    limit: u16,
) -> Result<(Vec<S3ObjectBinding>, Option<u64>), S3AccessError> {
    if limit == 0 {
        return Err(S3AccessError::InvalidLimit);
    }
    if let Some(prefix) = prefix {
        validate_prefix(prefix)?;
    }
    let connection = open(path)?;
    let offset_i64 = i64::try_from(offset).map_err(|_| S3AccessError::OffsetOverflow(offset))?;
    let fetch = i64::from(limit) + 1;
    let prefix = prefix.unwrap_or_default();
    let mut statement = connection.prepare(
        "SELECT b.object_key,b.object_version,b.object_id,b.size_bytes,
                b.content_hash_algorithm,b.content_hash,o.state
         FROM s3_object_bindings b
         JOIN objects o ON o.object_id=b.object_id AND o.store_id=b.store_id
         WHERE b.store_id=?1
           AND (?2='' OR substr(b.object_key,1,length(?2))=?2)
         ORDER BY b.object_key,b.object_version
         LIMIT ?3 OFFSET ?4",
    )?;
    let rows = statement.query_map(
        params![store_id.as_str(), prefix, fetch, offset_i64],
        |row| {
            let key: String = row.get(0)?;
            let version: u64 = row.get(1)?;
            binding_from_columns(store_id, key, version, row, 2)
        },
    )?;
    let mut objects = rows.collect::<Result<Vec<_>, _>>()?;
    let truncated = objects.len() > usize::from(limit);
    objects.truncate(usize::from(limit));
    let next = truncated.then(|| offset.saturating_add(objects.len() as u64));
    Ok((objects, next))
}

pub fn store_has_s3_object_bindings(
    path: impl AsRef<Path>,
    store_id: &StoreId,
) -> Result<bool, S3AccessError> {
    let connection = open(path)?;
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM s3_object_bindings WHERE store_id=?1)",
            [store_id.as_str()],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

pub fn backfill_s3_object_bindings(
    path: impl AsRef<Path>,
    committed_at_utc: &str,
) -> Result<S3BindingBackfillReport, S3AccessError> {
    let mut connection = open(path)?;
    let tx = connection.transaction()?;
    let candidates = {
        let mut statement = tx.prepare(
            "SELECT o.store_id,o.object_id,o.size_bytes,o.content_hash
             FROM objects o
             LEFT JOIN s3_object_bindings b ON b.object_id=o.object_id
             WHERE b.object_id IS NULL
               AND NOT EXISTS (
                   SELECT 1 FROM profile_catalogue_objects p
                   WHERE p.store_id=o.store_id AND p.object_id=o.object_id
               )
             ORDER BY o.store_id,o.object_id",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let mut report = S3BindingBackfillReport::default();
    for (store, object, size, hash) in candidates {
        let prefix = format!("{store}/");
        let Some(key) = object.strip_prefix(&prefix) else {
            report.objects_retained_unmapped += 1;
            continue;
        };
        let (Some(size), Some(hash)) = (size, hash) else {
            report.objects_retained_unmapped += 1;
            continue;
        };
        let Ok(store_id) = StoreId::new(store) else {
            report.objects_retained_unmapped += 1;
            continue;
        };
        let Ok(object_id) = ObjectId::new(object.clone()) else {
            report.objects_retained_unmapped += 1;
            continue;
        };
        if validate_key(key, 1).is_err() {
            report.objects_retained_unmapped += 1;
            continue;
        }
        match bind_s3_object_in_transaction(
            &tx,
            &store_id,
            key,
            1,
            &object_id,
            u64::try_from(size).map_err(|_| S3AccessError::InvalidStoredSize(size))?,
            "sha256",
            &hash,
            committed_at_utc,
        ) {
            Ok(()) => report.bindings_created += 1,
            Err(S3AccessError::BindingConflict { .. }) => {
                report.objects_retained_unmapped += 1;
            }
            Err(error) => return Err(error),
        }
    }
    let profile_candidates = {
        let mut statement = tx.prepare(
            "SELECT DISTINCT o.store_id,o.object_id,p.object_version,o.size_bytes,o.content_hash
             FROM objects o
             JOIN profile_catalogue_objects p
               ON p.store_id=o.store_id AND p.object_id=o.object_id
             LEFT JOIN s3_object_bindings b ON b.object_id=o.object_id
             WHERE b.object_id IS NULL
             ORDER BY o.store_id,o.object_id,p.object_version",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u64>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for (store, object, version, size, hash) in profile_candidates {
        let (Some(size), Some(hash)) = (size, hash) else {
            report.objects_retained_unmapped += 1;
            continue;
        };
        let (Ok(store_id), Ok(object_id)) = (StoreId::new(store), ObjectId::new(object.clone()))
        else {
            report.objects_retained_unmapped += 1;
            continue;
        };
        if bind_s3_object_in_transaction(
            &tx,
            &store_id,
            &object,
            version,
            &object_id,
            u64::try_from(size).map_err(|_| S3AccessError::InvalidStoredSize(size))?,
            "sha256",
            &hash,
            committed_at_utc,
        )
        .is_ok()
        {
            report.bindings_created += 1;
        } else {
            report.objects_retained_unmapped += 1;
        }
    }
    tx.commit()?;
    Ok(report)
}

fn open(path: impl AsRef<Path>) -> Result<Connection, S3AccessError> {
    let connection = Connection::open(path)?;
    connection.busy_timeout(BUSY_TIMEOUT)?;
    connection.execute_batch(LIVE_SCHEMA_SQL)?;
    Ok(connection)
}

fn binding_from_row(
    store_id: &StoreId,
    object_key: &str,
    object_version: u64,
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<S3ObjectBinding> {
    binding_from_columns(store_id, object_key.to_string(), object_version, row, 0)
}

fn binding_from_columns(
    store_id: &StoreId,
    object_key: String,
    object_version: u64,
    row: &rusqlite::Row<'_>,
    start: usize,
) -> rusqlite::Result<S3ObjectBinding> {
    let object_id: String = row.get(start)?;
    let size: i64 = row.get(start + 1)?;
    let algorithm: String = row.get(start + 2)?;
    let hash: String = row.get(start + 3)?;
    let state: String = row.get(start + 4)?;
    Ok(S3ObjectBinding {
        store_id: store_id.clone(),
        object_key,
        object_version,
        object_id: ObjectId::new(object_id).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                start,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?,
        size_bytes: u64::try_from(size)
            .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(start + 1, size))?,
        checksum: format!("{algorithm}:{hash}"),
        object_state: state,
    })
}

fn validate_key(key: &str, version: u64) -> Result<(), S3AccessError> {
    validate_path(key, false)?;
    if version == 0 {
        return Err(S3AccessError::InvalidVersion);
    }
    Ok(())
}

fn validate_prefix(value: &str) -> Result<(), S3AccessError> {
    validate_path(value, true)
}

fn validate_path(value: &str, allow_trailing_slash: bool) -> Result<(), S3AccessError> {
    if value.is_empty()
        || value.starts_with('/')
        || (!allow_trailing_slash && value.ends_with('/'))
        || value.contains('\\')
        || value.contains('\0')
        || value
            .strip_suffix('/')
            .unwrap_or(value)
            .split('/')
            .any(|segment| segment.is_empty() || segment == "..")
    {
        return Err(S3AccessError::InvalidKey(value.to_string()));
    }
    Ok(())
}

#[derive(Debug)]
pub enum S3AccessError {
    Sqlite(rusqlite::Error),
    InvalidKey(String),
    InvalidVersion,
    InvalidLimit,
    SizeOverflow(u64),
    InvalidStoredSize(i64),
    OffsetOverflow(u64),
    BindingConflict {
        store_id: StoreId,
        object_key: String,
        object_version: u64,
    },
}

impl Display for S3AccessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => Display::fmt(error, formatter),
            Self::InvalidKey(key) => write!(formatter, "invalid S3 object key `{key}`"),
            Self::InvalidVersion => write!(formatter, "S3 object version must be non-zero"),
            Self::InvalidLimit => write!(formatter, "S3 list limit must be non-zero"),
            Self::SizeOverflow(size) => write!(formatter, "object size {size} exceeds SQLite"),
            Self::InvalidStoredSize(size) => write!(formatter, "invalid stored size {size}"),
            Self::OffsetOverflow(offset) => {
                write!(formatter, "S3 list offset {offset} is too large")
            }
            Self::BindingConflict {
                store_id,
                object_key,
                object_version,
            } => write!(
                formatter,
                "S3 identity conflict for {store_id}/{object_key} version {object_version}"
            ),
        }
    }
}

impl std::error::Error for S3AccessError {}

impl From<rusqlite::Error> for S3AccessError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{commit_verified_ssd_and_enqueue, VerifiedSsdCommitRequest, LIVE_SCHEMA_SQL};

    fn prepared() -> (std::path::PathBuf, std::path::PathBuf, StoreId) {
        let root = std::env::var_os("DASOBJECTSTORE_CODEX_VALIDATION_ROOT")
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| {
                    std::path::PathBuf::from(home).join(".dasobjectstore-codex-validation")
                })
            })
            .unwrap_or_else(std::env::temp_dir)
            .join(format!(
                "metadata-s3-access-{}-{}",
                std::process::id(),
                std::thread::current().name().unwrap_or("test")
            ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("validation root");
        let path = root.join("live.sqlite");
        let connection = Connection::open(&path).expect("open");
        connection.execute_batch(LIVE_SCHEMA_SQL).expect("schema");
        connection.execute("INSERT INTO pools(pool_id,state,created_at_utc,updated_at_utc) VALUES('pool-a','Clean','now','now')", []).expect("pool");
        connection.execute("INSERT INTO stores(store_id,pool_id,class,policy_json,created_at_utc,updated_at_utc) VALUES('alleleanchor_mvp','pool-a','GeneratedData','{}','now','now')", []).expect("store");
        drop(connection);
        (
            root,
            path,
            StoreId::new("alleleanchor_mvp").expect("store id"),
        )
    }

    fn commit(path: &Path, store: &StoreId, object_id: &str, key: &str, hash: &str) {
        let object_id = ObjectId::new(object_id).expect("object id");
        commit_verified_ssd_and_enqueue(
            path,
            VerifiedSsdCommitRequest {
                destage_job_id: &format!("destage-{key}"),
                store_id: store,
                object_id: &object_id,
                object_type: "naive",
                relative_path: &format!(".dasobjectstore/ingest/{key}"),
                size_bytes: 7,
                content_hash_algorithm: "sha256",
                content_hash: hash,
                acknowledgement_policy: "after_ssd_ingest",
                required_copy_count: 1,
                max_attempts: 8,
                priority: 0,
                committed_at_utc: "2026-07-24T00:00:00Z",
                ingest_job_id: None,
                ingress_origin: None,
                s3_key: Some(key),
                s3_version: 1,
            },
        )
        .expect("commit");
    }

    #[test]
    fn native_acknowledgement_atomically_publishes_s3_identity() {
        let (_root, path, store) = prepared();
        commit(
            &path,
            &store,
            "alleleanchor_mvp/results/a.vcf",
            "results/a.vcf",
            "abc123",
        );

        let binding = read_s3_object_binding(&path, &store, "results/a.vcf", 1)
            .expect("read")
            .expect("binding");
        assert_eq!(binding.object_id.as_str(), "alleleanchor_mvp/results/a.vcf");
        assert_eq!(binding.checksum, "sha256:abc123");
    }

    #[test]
    fn immutable_s3_identity_conflict_rolls_back_new_object() {
        let (_root, path, store) = prepared();
        commit(
            &path,
            &store,
            "alleleanchor_mvp/results/a.vcf",
            "results/a.vcf",
            "abc123",
        );
        let second = ObjectId::new("alleleanchor_mvp/other/a.vcf").expect("object id");
        let error = commit_verified_ssd_and_enqueue(
            &path,
            VerifiedSsdCommitRequest {
                destage_job_id: "destage-conflict",
                store_id: &store,
                object_id: &second,
                object_type: "naive",
                relative_path: ".dasobjectstore/ingest/conflict",
                size_bytes: 7,
                content_hash_algorithm: "sha256",
                content_hash: "different",
                acknowledgement_policy: "after_ssd_ingest",
                required_copy_count: 1,
                max_attempts: 8,
                priority: 0,
                committed_at_utc: "2026-07-24T00:00:01Z",
                ingest_job_id: None,
                ingress_origin: None,
                s3_key: Some("results/a.vcf"),
                s3_version: 1,
            },
        )
        .expect_err("conflict");
        assert!(error.to_string().contains("S3 identity conflict"));
        let count: u64 = Connection::open(path)
            .expect("open")
            .query_row(
                "SELECT COUNT(*) FROM objects WHERE object_id=?1",
                [second.as_str()],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(count, 0);
    }

    #[test]
    fn backfill_maps_only_unambiguous_store_prefixed_objects() {
        let (_root, path, store) = prepared();
        let connection = Connection::open(&path).expect("open");
        connection.execute("INSERT INTO objects(object_id,store_id,object_type,state,size_bytes,content_hash,created_at_utc,updated_at_utc) VALUES('alleleanchor_mvp/legacy/a.vcf','alleleanchor_mvp','naive','HddCopyVerified',9,'def456','now','now')", []).expect("object");
        connection.execute("INSERT INTO objects(object_id,store_id,object_type,state,size_bytes,content_hash,created_at_utc,updated_at_utc) VALUES('ambiguous.vcf','alleleanchor_mvp','naive','HddCopyVerified',9,'ghi789','now','now')", []).expect("ambiguous");
        drop(connection);

        let report = backfill_s3_object_bindings(&path, "2026-07-24T00:00:00Z").expect("backfill");
        assert_eq!(report.bindings_created, 1);
        assert_eq!(report.objects_retained_unmapped, 1);
        assert!(read_s3_object_binding(&path, &store, "legacy/a.vcf", 1)
            .expect("read")
            .is_some());
    }
}
