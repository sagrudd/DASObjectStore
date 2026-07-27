//! Internal validation, normalization, and persistence helpers.

use super::{
    LogicalIdentityError, LogicalPlacementClaim, LogicalVersionClaim, LogicalVersionRecord,
};
use dasobjectstore_core::ids::StoreId;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::Duration;

const IDENTITY_BUSY_TIMEOUT: Duration = Duration::from_secs(3);

pub(super) fn open(path: impl AsRef<Path>) -> Result<Connection, LogicalIdentityError> {
    let connection = Connection::open(path)?;
    connection.busy_timeout(IDENTITY_BUSY_TIMEOUT)?;
    connection.execute_batch("PRAGMA foreign_keys = ON;")?;
    Ok(connection)
}

pub(super) fn read_version_by_key(
    transaction: &Transaction<'_>,
    store_id: &StoreId,
    object_key: &str,
    object_version: u64,
) -> Result<Option<LogicalVersionRecord>, LogicalIdentityError> {
    transaction
        .query_row(
            "SELECT logical_version_id,size_bytes,content_hash_algorithm,content_hash
             FROM logical_object_versions
             WHERE store_id=?1 AND object_key=?2 AND object_version=?3",
            params![store_id.as_str(), object_key, to_i64(object_version)?],
            |row| {
                Ok(LogicalVersionRecord {
                    logical_version_id: row.get(0)?,
                    store_id: store_id.clone(),
                    object_key: object_key.to_string(),
                    object_version,
                    size_bytes: row.get(1)?,
                    content_hash_algorithm: row.get(2)?,
                    content_hash: row.get(3)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

pub(super) fn ensure_same_evidence(
    existing: &LogicalVersionRecord,
    size_bytes: u64,
    content_hash_algorithm: &str,
    content_hash: &str,
) -> Result<(), LogicalIdentityError> {
    if existing.size_bytes == size_bytes
        && existing.content_hash_algorithm == content_hash_algorithm
        && existing.content_hash == content_hash
    {
        Ok(())
    } else {
        Err(LogicalIdentityError::EvidenceConflict {
            store_id: existing.store_id.to_string(),
            object_key: existing.object_key.clone(),
            object_version: existing.object_version,
        })
    }
}

pub(crate) fn normalize_algorithm(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

pub(crate) fn normalize_hash(value: &str, algorithm: &str) -> String {
    value
        .trim()
        .strip_prefix(&format!("{algorithm}:"))
        .unwrap_or(value.trim())
        .to_ascii_lowercase()
}

pub(super) fn validate_claim(claim: &LogicalVersionClaim<'_>) -> Result<(), LogicalIdentityError> {
    validate_key_version(claim.object_key, claim.object_version)?;
    validate_nonblank("content_hash_algorithm", claim.content_hash_algorithm)?;
    validate_nonblank("content_hash", claim.content_hash)?;
    validate_nonblank("recorded_at_utc", claim.recorded_at_utc)
}

pub(super) fn validate_key_version(key: &str, version: u64) -> Result<(), LogicalIdentityError> {
    validate_nonblank("object_key", key)?;
    if key.contains('\0') {
        return Err(LogicalIdentityError::InvalidField("object_key"));
    }
    if version == 0 {
        return Err(LogicalIdentityError::ZeroVersion);
    }
    Ok(())
}

pub(super) fn validate_placement_claim(
    claim: &LogicalPlacementClaim<'_>,
) -> Result<(), LogicalIdentityError> {
    for (field, value) in [
        ("logical_version_id", claim.logical_version_id),
        ("placement_kind", claim.placement_kind),
        ("placement_namespace", claim.placement_namespace),
        ("source_placement_id", claim.source_placement_id),
        ("location", claim.location),
        ("content_hash_algorithm", claim.content_hash_algorithm),
        ("content_hash", claim.content_hash),
        ("recorded_at_utc", claim.recorded_at_utc),
    ] {
        validate_nonblank(field, value)?;
    }
    Ok(())
}

pub(super) fn validate_nonblank(
    field: &'static str,
    value: &str,
) -> Result<(), LogicalIdentityError> {
    if value.trim().is_empty() {
        Err(LogicalIdentityError::InvalidField(field))
    } else {
        Ok(())
    }
}

pub(super) fn digest_fields(fields: &[&str]) -> String {
    let mut digest = Sha256::new();
    for field in fields {
        digest.update(field.len().to_le_bytes());
        digest.update(field.as_bytes());
    }
    digest
        .finalize()
        .iter()
        .fold(String::with_capacity(64), |mut encoded, byte| {
            use std::fmt::Write as _;
            write!(&mut encoded, "{byte:02x}").expect("writing to a String cannot fail");
            encoded
        })
}

pub(super) fn to_i64(value: u64) -> Result<i64, LogicalIdentityError> {
    i64::try_from(value).map_err(|_| LogicalIdentityError::NumericOverflow)
}
