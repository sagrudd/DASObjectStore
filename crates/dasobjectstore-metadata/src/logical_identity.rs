//! Additive canonical logical-object-version identity bridge.
//!
//! Legacy native, S3, and portable profile catalogues remain authoritative
//! inputs during the compatibility window. This module converges matching
//! immutable evidence into one logical version and records physical/provider
//! placements without moving or deleting payloads.

use crate::schema::{LIVE_SCHEMA_FORMAT_VERSION, LIVE_SCHEMA_SQL};
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_core::object_catalogue::{PortableObjectVersion, PortablePlacementLocation};
use rusqlite::{params, OptionalExtension, Transaction};
use std::fmt::{self, Display};
use std::path::Path;

mod support;

use support::{
    digest_fields, ensure_same_evidence, open, read_version_by_key, to_i64, validate_claim,
    validate_key_version, validate_nonblank, validate_placement_claim,
};
pub(crate) use support::{normalize_algorithm, normalize_hash};

pub const LOGICAL_IDENTITY_MIGRATION_ID: u64 = 13;
pub const LOGICAL_IDENTITY_MIGRATION_NAME: &str =
    "canonical-logical-identity-and-lifecycle-scheduler";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogicalVersionClaim<'a> {
    pub store_id: &'a StoreId,
    pub object_key: &'a str,
    pub object_version: u64,
    pub size_bytes: u64,
    pub content_hash_algorithm: &'a str,
    pub content_hash: &'a str,
    pub recorded_at_utc: &'a str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogicalPlacementClaim<'a> {
    pub logical_version_id: &'a str,
    pub placement_kind: &'a str,
    pub placement_namespace: &'a str,
    pub source_placement_id: &'a str,
    pub location: &'a str,
    pub content_hash_algorithm: &'a str,
    pub content_hash: &'a str,
    pub verified_at_utc: Option<&'a str>,
    pub recorded_at_utc: &'a str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogicalVersionRecord {
    pub logical_version_id: String,
    pub store_id: StoreId,
    pub object_key: String,
    pub object_version: u64,
    pub size_bytes: u64,
    pub content_hash_algorithm: String,
    pub content_hash: String,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LogicalIdentityBackfillReport {
    pub logical_versions: u64,
    pub placements: u64,
    pub exact_replays: u64,
    pub needs_review: u64,
    pub dry_run: bool,
}

/// Derive the stable internal ID without replacing any external object ID.
pub fn canonical_logical_version_id(
    store_id: &StoreId,
    object_key: &str,
    object_version: u64,
) -> Result<String, LogicalIdentityError> {
    validate_key_version(object_key, object_version)?;
    Ok(format!(
        "lov-{}",
        digest_fields(&[store_id.as_str(), object_key, &object_version.to_string(),])
    ))
}

/// Preserve the externally stable key when a provider upload used a distinct
/// internal object ID. Other portable catalogues already use object ID as key.
pub fn logical_profile_object_key(object: &PortableObjectVersion) -> &str {
    if object.provenance.source_kind == "remote_upload" {
        object
            .provenance
            .locator
            .as_deref()
            .unwrap_or_else(|| object.object_id.as_str())
    } else {
        object.object_id.as_str()
    }
}

/// Scope provider/profile placement identifiers to the ObjectStore because
/// portable placement IDs are not required to be globally unique.
pub fn logical_profile_placement_namespace(profile_namespace: &str, store_id: &StoreId) -> String {
    format!("{profile_namespace}:{}", store_id.as_str())
}

pub fn claim_logical_version(
    path: impl AsRef<Path>,
    claim: &LogicalVersionClaim<'_>,
) -> Result<(LogicalVersionRecord, bool), LogicalIdentityError> {
    let mut connection = open(path)?;
    let transaction = connection.transaction()?;
    transaction.execute_batch(LIVE_SCHEMA_SQL)?;
    let result = claim_logical_version_in_transaction(&transaction, claim)?;
    transaction.commit()?;
    Ok(result)
}

/// Transaction-level seam used by native and provider catalogue commits.
pub fn claim_logical_version_in_transaction(
    transaction: &Transaction<'_>,
    claim: &LogicalVersionClaim<'_>,
) -> Result<(LogicalVersionRecord, bool), LogicalIdentityError> {
    validate_claim(claim)?;
    let normalized_algorithm = normalize_algorithm(claim.content_hash_algorithm);
    let normalized_hash = normalize_hash(claim.content_hash, &normalized_algorithm);
    let logical_version_id =
        canonical_logical_version_id(claim.store_id, claim.object_key, claim.object_version)?;
    let existing = read_version_by_key(
        transaction,
        claim.store_id,
        claim.object_key,
        claim.object_version,
    )?;
    if let Some(existing) = existing {
        ensure_same_evidence(
            &existing,
            claim.size_bytes,
            &normalized_algorithm,
            &normalized_hash,
        )?;
        if existing.logical_version_id != logical_version_id {
            return Err(LogicalIdentityError::IdentityConflict {
                store_id: claim.store_id.to_string(),
                object_key: claim.object_key.to_string(),
                object_version: claim.object_version,
            });
        }
        return Ok((existing, true));
    }
    transaction.execute(
        "INSERT INTO logical_object_versions (
            logical_version_id,store_id,object_key,object_version,size_bytes,
            content_hash_algorithm,content_hash,created_at_utc,updated_at_utc
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?8)",
        params![
            logical_version_id,
            claim.store_id.as_str(),
            claim.object_key,
            to_i64(claim.object_version)?,
            to_i64(claim.size_bytes)?,
            normalized_algorithm,
            normalized_hash,
            claim.recorded_at_utc,
        ],
    )?;
    Ok((
        LogicalVersionRecord {
            logical_version_id,
            store_id: claim.store_id.clone(),
            object_key: claim.object_key.to_string(),
            object_version: claim.object_version,
            size_bytes: claim.size_bytes,
            content_hash_algorithm: normalized_algorithm,
            content_hash: normalized_hash,
        },
        false,
    ))
}

pub fn bind_native_object_in_transaction(
    transaction: &Transaction<'_>,
    object_id: &str,
    logical_version_id: &str,
    recorded_at_utc: &str,
) -> Result<bool, LogicalIdentityError> {
    validate_nonblank("object_id", object_id)?;
    validate_nonblank("logical_version_id", logical_version_id)?;
    let existing = transaction
        .query_row(
            "SELECT logical_version_id FROM native_logical_version_bindings
             WHERE object_id=?1",
            [object_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if let Some(existing) = existing {
        if existing == logical_version_id {
            return Ok(true);
        }
        return Err(LogicalIdentityError::NativeBindingConflict(
            object_id.to_string(),
        ));
    }
    transaction.execute(
        "INSERT INTO native_logical_version_bindings (
            object_id,logical_version_id,created_at_utc
         ) VALUES (?1,?2,?3)",
        params![object_id, logical_version_id, recorded_at_utc],
    )?;
    Ok(false)
}

pub fn replace_native_object_binding_in_transaction(
    transaction: &Transaction<'_>,
    object_id: &str,
    expected_logical_version_id: &str,
    replacement_logical_version_id: &str,
) -> Result<(), LogicalIdentityError> {
    validate_nonblank("object_id", object_id)?;
    validate_nonblank("expected_logical_version_id", expected_logical_version_id)?;
    validate_nonblank(
        "replacement_logical_version_id",
        replacement_logical_version_id,
    )?;
    let changed = transaction.execute(
        "UPDATE native_logical_version_bindings SET logical_version_id=?1
         WHERE object_id=?2 AND logical_version_id=?3",
        params![
            replacement_logical_version_id,
            object_id,
            expected_logical_version_id
        ],
    )?;
    if changed != 1 {
        return Err(LogicalIdentityError::NativeBindingConflict(
            object_id.to_string(),
        ));
    }
    Ok(())
}

pub fn read_native_logical_version_in_transaction(
    transaction: &Transaction<'_>,
    object_id: &str,
) -> Result<Option<LogicalVersionRecord>, LogicalIdentityError> {
    validate_nonblank("object_id", object_id)?;
    let row = transaction
        .query_row(
            "SELECT v.logical_version_id,v.store_id,v.object_key,
                    v.object_version,v.size_bytes,v.content_hash_algorithm,
                    v.content_hash
             FROM native_logical_version_bindings b
             JOIN logical_object_versions v
               ON v.logical_version_id=b.logical_version_id
            WHERE b.object_id=?1",
            [object_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, u64>(3)?,
                    row.get::<_, u64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                ))
            },
        )
        .optional()?;
    row.map(
        |(logical_version_id, store, object_key, object_version, size_bytes, algorithm, hash)| {
            Ok(LogicalVersionRecord {
                logical_version_id,
                store_id: StoreId::new(store.clone())
                    .map_err(|_| LogicalIdentityError::InvalidStoreId(store))?,
                object_key,
                object_version,
                size_bytes,
                content_hash_algorithm: algorithm,
                content_hash: hash,
            })
        },
    )
    .transpose()
}

pub fn claim_logical_placement_in_transaction(
    transaction: &Transaction<'_>,
    claim: &LogicalPlacementClaim<'_>,
) -> Result<bool, LogicalIdentityError> {
    validate_placement_claim(claim)?;
    let normalized_algorithm = normalize_algorithm(claim.content_hash_algorithm);
    let normalized_hash = normalize_hash(claim.content_hash, &normalized_algorithm);
    let version_evidence = transaction
        .query_row(
            "SELECT content_hash_algorithm,content_hash
             FROM logical_object_versions WHERE logical_version_id=?1",
            [claim.logical_version_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
        .ok_or_else(|| {
            LogicalIdentityError::UnknownLogicalVersion(claim.logical_version_id.to_string())
        })?;
    if version_evidence != (normalized_algorithm.clone(), normalized_hash.clone()) {
        return Err(LogicalIdentityError::PlacementConflict(
            claim.source_placement_id.to_string(),
        ));
    }
    let source_existing = transaction
        .query_row(
            "SELECT logical_version_id,placement_kind,location,
                    content_hash_algorithm,content_hash
             FROM logical_placements
             WHERE placement_namespace=?1 AND source_placement_id=?2",
            params![claim.placement_namespace, claim.source_placement_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .optional()?;
    if source_existing.as_ref().is_some_and(|existing| {
        existing
            != &(
                claim.logical_version_id.to_string(),
                claim.placement_kind.to_string(),
                claim.location.to_string(),
                normalized_algorithm.clone(),
                normalized_hash.clone(),
            )
    }) {
        return Err(LogicalIdentityError::PlacementConflict(
            claim.source_placement_id.to_string(),
        ));
    }
    let placement_id = format!(
        "lop-{}",
        digest_fields(&[
            claim.logical_version_id,
            claim.placement_kind,
            claim.placement_namespace,
            claim.source_placement_id,
            claim.location,
        ])
    );
    let existing = transaction
        .query_row(
            "SELECT logical_version_id,placement_kind,placement_namespace,
                    source_placement_id,location,content_hash_algorithm,
                    content_hash,verified_at_utc,state
             FROM logical_placements WHERE placement_id=?1",
            [&placement_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, String>(8)?,
                ))
            },
        )
        .optional()?;
    if let Some(existing) = existing {
        let immutable_matches = existing.0 == claim.logical_version_id
            && existing.1 == claim.placement_kind
            && existing.2 == claim.placement_namespace
            && existing.3 == claim.source_placement_id
            && existing.4 == claim.location
            && existing.5 == normalized_algorithm
            && existing.6 == normalized_hash;
        if immutable_matches && existing.8 == "active" {
            if claim.verified_at_utc.is_some() && existing.7.as_deref() != claim.verified_at_utc {
                transaction.execute(
                    "UPDATE logical_placements SET verified_at_utc=?1,updated_at_utc=?2
                     WHERE placement_id=?3",
                    params![claim.verified_at_utc, claim.recorded_at_utc, placement_id],
                )?;
            }
            return Ok(true);
        }
        if immutable_matches && existing.8 == "withdrawn" {
            transaction.execute(
                "UPDATE logical_placements
                 SET state='active',verified_at_utc=COALESCE(?1,verified_at_utc),
                     updated_at_utc=?2
                 WHERE placement_id=?3 AND state='withdrawn'",
                params![claim.verified_at_utc, claim.recorded_at_utc, placement_id],
            )?;
            return Ok(false);
        }
        return Err(LogicalIdentityError::PlacementConflict(
            claim.source_placement_id.to_string(),
        ));
    }
    transaction.execute(
        "INSERT INTO logical_placements (
            placement_id,logical_version_id,placement_kind,
            placement_namespace,source_placement_id,location,
            content_hash_algorithm,content_hash,state,verified_at_utc,
            created_at_utc,updated_at_utc
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,'active',?9,?10,?10)",
        params![
            placement_id,
            claim.logical_version_id,
            claim.placement_kind,
            claim.placement_namespace,
            claim.source_placement_id,
            claim.location,
            normalized_algorithm,
            normalized_hash,
            claim.verified_at_utc,
            claim.recorded_at_utc,
        ],
    )?;
    Ok(false)
}

pub fn withdraw_logical_placement_sources_in_transaction(
    transaction: &Transaction<'_>,
    placement_namespace: &str,
    source_placement_ids: &[&str],
    recorded_at_utc: &str,
) -> Result<u64, LogicalIdentityError> {
    validate_nonblank("placement_namespace", placement_namespace)?;
    validate_nonblank("recorded_at_utc", recorded_at_utc)?;
    let mut withdrawn = 0_u64;
    for source_id in source_placement_ids {
        validate_nonblank("source_placement_id", source_id)?;
        withdrawn += u64::try_from(transaction.execute(
            "UPDATE logical_placements SET state='withdrawn',updated_at_utc=?1
             WHERE placement_namespace=?2 AND source_placement_id=?3
               AND state='active'",
            params![recorded_at_utc, placement_namespace, source_id],
        )?)
        .map_err(|_| LogicalIdentityError::NumericOverflow)?;
    }
    Ok(withdrawn)
}

pub fn withdraw_logical_placement_namespace_in_transaction(
    transaction: &Transaction<'_>,
    placement_namespace: &str,
    recorded_at_utc: &str,
) -> Result<u64, LogicalIdentityError> {
    validate_nonblank("placement_namespace", placement_namespace)?;
    validate_nonblank("recorded_at_utc", recorded_at_utc)?;
    u64::try_from(transaction.execute(
        "UPDATE logical_placements SET state='withdrawn',updated_at_utc=?1
         WHERE placement_namespace=?2 AND state='active'",
        params![recorded_at_utc, placement_namespace],
    )?)
    .map_err(|_| LogicalIdentityError::NumericOverflow)
}

pub fn withdraw_logical_version_placements_in_transaction(
    transaction: &Transaction<'_>,
    logical_version_id: &str,
    placement_namespace: &str,
    recorded_at_utc: &str,
) -> Result<u64, LogicalIdentityError> {
    validate_nonblank("logical_version_id", logical_version_id)?;
    validate_nonblank("placement_namespace", placement_namespace)?;
    validate_nonblank("recorded_at_utc", recorded_at_utc)?;
    u64::try_from(transaction.execute(
        "UPDATE logical_placements SET state='withdrawn',updated_at_utc=?1
         WHERE logical_version_id=?2 AND placement_namespace=?3
           AND state='active'",
        params![recorded_at_utc, logical_version_id, placement_namespace],
    )?)
    .map_err(|_| LogicalIdentityError::NumericOverflow)
}

/// Inspect or apply deterministic convergence of legacy native and provider
/// evidence. Conflicts become `needs_review`; payload and source rows are never
/// modified.
pub fn backfill_logical_identities(
    path: impl AsRef<Path>,
    dry_run: bool,
    recorded_at_utc: &str,
) -> Result<LogicalIdentityBackfillReport, LogicalIdentityError> {
    validate_nonblank("recorded_at_utc", recorded_at_utc)?;
    let mut connection = open(path)?;
    let transaction = connection.transaction()?;
    transaction.execute_batch(LIVE_SCHEMA_SQL)?;
    let mut report = LogicalIdentityBackfillReport {
        dry_run,
        ..LogicalIdentityBackfillReport::default()
    };
    record_schema_upgrade(&transaction, recorded_at_utc)?;
    backfill_native(&transaction, dry_run, recorded_at_utc, &mut report)?;
    backfill_profiles(&transaction, dry_run, recorded_at_utc, &mut report)?;
    if dry_run {
        transaction.rollback()?;
    } else {
        transaction.commit()?;
    }
    Ok(report)
}

fn record_schema_upgrade(
    transaction: &Transaction<'_>,
    recorded_at_utc: &str,
) -> Result<(), LogicalIdentityError> {
    transaction.execute(
        "INSERT INTO metadata_format_versions(artifact,major,minor,updated_at_utc)
         VALUES(?1,?2,?3,?4)
         ON CONFLICT(artifact) DO UPDATE SET
           major=excluded.major,minor=excluded.minor,updated_at_utc=excluded.updated_at_utc
         WHERE metadata_format_versions.major < excluded.major
            OR (metadata_format_versions.major=excluded.major
                AND metadata_format_versions.minor < excluded.minor)",
        params![
            LIVE_SCHEMA_FORMAT_VERSION.artifact.name(),
            LIVE_SCHEMA_FORMAT_VERSION.major,
            LIVE_SCHEMA_FORMAT_VERSION.minor,
            recorded_at_utc,
        ],
    )?;
    transaction.execute(
        "INSERT OR IGNORE INTO metadata_migrations(migration_id,name,applied_at_utc)
         VALUES(?1,?2,?3)",
        params![
            LOGICAL_IDENTITY_MIGRATION_ID,
            LOGICAL_IDENTITY_MIGRATION_NAME,
            recorded_at_utc
        ],
    )?;
    let migration_name = transaction
        .query_row(
            "SELECT name FROM metadata_migrations WHERE migration_id=?1",
            [LOGICAL_IDENTITY_MIGRATION_ID],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if migration_name.as_deref() != Some(LOGICAL_IDENTITY_MIGRATION_NAME) {
        return Err(LogicalIdentityError::MigrationEvidenceConflict(
            LOGICAL_IDENTITY_MIGRATION_ID,
        ));
    }
    Ok(())
}

fn backfill_native(
    transaction: &Transaction<'_>,
    dry_run: bool,
    recorded_at_utc: &str,
    report: &mut LogicalIdentityBackfillReport,
) -> Result<(), LogicalIdentityError> {
    let mut statement = transaction.prepare(
        "SELECT o.object_id,o.store_id,COALESCE(b.object_key,o.object_id),
                COALESCE(b.object_version,1),o.size_bytes,
                COALESCE(s.content_hash_algorithm,'sha256'),o.content_hash,
                COALESCE(o.updated_at_utc,?1)
         FROM objects o
         LEFT JOIN s3_object_bindings b ON b.object_id=o.object_id
         LEFT JOIN ssd_object_placements s ON s.object_id=o.object_id
         WHERE o.size_bytes IS NOT NULL AND o.content_hash IS NOT NULL
         ORDER BY o.store_id,o.object_id",
    )?;
    let rows = statement
        .query_map([recorded_at_utc], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, u64>(3)?,
                row.get::<_, u64>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    for row in rows {
        let store_id = StoreId::new(row.1.clone())
            .map_err(|_| LogicalIdentityError::InvalidStoreId(row.1.clone()))?;
        let claim = LogicalVersionClaim {
            store_id: &store_id,
            object_key: &row.2,
            object_version: row.3,
            size_bytes: row.4,
            content_hash_algorithm: &row.5,
            content_hash: &row.6,
            recorded_at_utc,
        };
        match claim_logical_version_in_transaction(transaction, &claim) {
            Ok((version, replay)) => {
                report.logical_versions += u64::from(!replay);
                report.exact_replays += u64::from(replay);
                let placement_result = bind_native_object_in_transaction(
                    transaction,
                    &row.0,
                    &version.logical_version_id,
                    recorded_at_utc,
                )
                .and_then(|_| {
                    backfill_native_placements(
                        transaction,
                        &row.0,
                        &version,
                        recorded_at_utc,
                        report,
                    )
                });
                if let Err(error) = placement_result {
                    if !error.is_evidence_conflict() {
                        return Err(error);
                    }
                    report.needs_review += 1;
                    if !dry_run {
                        record_review(
                            transaction,
                            &claim,
                            "native",
                            &row.0,
                            &error.to_string(),
                            recorded_at_utc,
                        )?;
                    }
                }
            }
            Err(error) if error.is_evidence_conflict() => {
                report.needs_review += 1;
                if !dry_run {
                    record_review(
                        transaction,
                        &claim,
                        "native",
                        &row.0,
                        &error.to_string(),
                        recorded_at_utc,
                    )?;
                }
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn backfill_native_placements(
    transaction: &Transaction<'_>,
    object_id: &str,
    version: &LogicalVersionRecord,
    recorded_at_utc: &str,
    report: &mut LogicalIdentityBackfillReport,
) -> Result<(), LogicalIdentityError> {
    let ssd = transaction
        .query_row(
            "SELECT relative_path,content_hash_algorithm,content_hash,verified_at_utc
             FROM ssd_object_placements WHERE object_id=?1",
            [object_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?;
    if let Some(ssd) = ssd {
        let replay = claim_logical_placement_in_transaction(
            transaction,
            &LogicalPlacementClaim {
                logical_version_id: &version.logical_version_id,
                placement_kind: "ssd",
                placement_namespace: "native",
                source_placement_id: object_id,
                location: &ssd.0,
                content_hash_algorithm: &ssd.1,
                content_hash: &ssd.2,
                verified_at_utc: Some(&ssd.3),
                recorded_at_utc,
            },
        )?;
        report.placements += u64::from(!replay);
        report.exact_replays += u64::from(replay);
    }
    let mut statement = transaction.prepare(
        "SELECT p.placement_id,p.disk_id,p.relative_path,p.content_hash,
                p.verified_at_utc
         FROM placements p WHERE p.object_id=?1 ORDER BY p.placement_id",
    )?;
    let placements = statement
        .query_map([object_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    for placement in placements {
        let location = format!("{}:{}", placement.1, placement.2);
        let replay = claim_logical_placement_in_transaction(
            transaction,
            &LogicalPlacementClaim {
                logical_version_id: &version.logical_version_id,
                placement_kind: "hdd",
                placement_namespace: "native",
                source_placement_id: &placement.0,
                location: &location,
                content_hash_algorithm: &version.content_hash_algorithm,
                content_hash: &placement.3,
                verified_at_utc: placement.4.as_deref(),
                recorded_at_utc,
            },
        )?;
        report.placements += u64::from(!replay);
        report.exact_replays += u64::from(replay);
    }
    Ok(())
}

fn backfill_profiles(
    transaction: &Transaction<'_>,
    dry_run: bool,
    recorded_at_utc: &str,
    report: &mut LogicalIdentityBackfillReport,
) -> Result<(), LogicalIdentityError> {
    let mut statement = transaction.prepare(
        "SELECT profile_namespace,store_id,object_id,object_version,object_json
         FROM profile_catalogue_objects
         ORDER BY store_id,object_id,object_version,profile_namespace",
    )?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, u64>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    for row in rows {
        let store_id = StoreId::new(row.1.clone())
            .map_err(|_| LogicalIdentityError::InvalidStoreId(row.1.clone()))?;
        let object: PortableObjectVersion = serde_json::from_str(&row.4)
            .map_err(|error| LogicalIdentityError::InvalidProfileObject(error.to_string()))?;
        if object.object_id.as_str() != row.2 || object.version != row.3 {
            return Err(LogicalIdentityError::InvalidProfileObject(
                "profile row identity does not match object JSON".to_string(),
            ));
        }
        let claim = LogicalVersionClaim {
            store_id: &store_id,
            object_key: logical_profile_object_key(&object),
            object_version: row.3,
            size_bytes: object.size_bytes,
            content_hash_algorithm: &object.checksum.algorithm,
            content_hash: &object.checksum.value,
            recorded_at_utc,
        };
        match claim_logical_version_in_transaction(transaction, &claim) {
            Ok((version, replay)) => {
                report.logical_versions += u64::from(!replay);
                report.exact_replays += u64::from(replay);
                let placement_namespace = logical_profile_placement_namespace(&row.0, &store_id);
                for placement in &object.placements {
                    let (kind, location) = placement_location(&placement.location);
                    let replay = match claim_logical_placement_in_transaction(
                        transaction,
                        &LogicalPlacementClaim {
                            logical_version_id: &version.logical_version_id,
                            placement_kind: kind,
                            placement_namespace: &placement_namespace,
                            source_placement_id: placement.placement_id.as_str(),
                            location: &location,
                            content_hash_algorithm: &placement.checksum.algorithm,
                            content_hash: &placement.checksum.value,
                            verified_at_utc: placement.verified_at_utc.as_deref(),
                            recorded_at_utc,
                        },
                    ) {
                        Ok(replay) => replay,
                        Err(error) if error.is_evidence_conflict() => {
                            report.needs_review += 1;
                            if !dry_run {
                                record_review(
                                    transaction,
                                    &claim,
                                    "profile",
                                    &format!(
                                        "{}:{}:{}:{}:{}",
                                        row.0,
                                        row.1,
                                        row.2,
                                        row.3,
                                        placement.placement_id.as_str()
                                    ),
                                    &error.to_string(),
                                    recorded_at_utc,
                                )?;
                            }
                            continue;
                        }
                        Err(error) => return Err(error),
                    };
                    report.placements += u64::from(!replay);
                    report.exact_replays += u64::from(replay);
                }
            }
            Err(error) if error.is_evidence_conflict() => {
                report.needs_review += 1;
                if !dry_run {
                    record_review(
                        transaction,
                        &claim,
                        "profile",
                        &format!("{}:{}:{}:{}", row.0, row.1, row.2, row.3),
                        &error.to_string(),
                        recorded_at_utc,
                    )?;
                }
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn placement_location(location: &PortablePlacementLocation) -> (&'static str, String) {
    match location {
        PortablePlacementLocation::Folder { relative_path } => ("folder", relative_path.clone()),
        PortablePlacementLocation::Drive { relative_path } => ("drive", relative_path.clone()),
        PortablePlacementLocation::Appliance {
            pool_id,
            disk_id,
            relative_path,
        } => (
            "hdd",
            format!("{pool_id}:{}:{relative_path}", disk_id.as_str()),
        ),
        PortablePlacementLocation::Provider {
            provider,
            object_key,
        } => ("provider", format!("{provider}:{object_key}")),
    }
}

fn record_review(
    transaction: &Transaction<'_>,
    claim: &LogicalVersionClaim<'_>,
    source_kind: &str,
    source_identity: &str,
    reason: &str,
    recorded_at_utc: &str,
) -> Result<(), LogicalIdentityError> {
    let evidence_digest = digest_fields(&[
        claim.store_id.as_str(),
        claim.object_key,
        &claim.object_version.to_string(),
        &claim.size_bytes.to_string(),
        claim.content_hash_algorithm,
        claim.content_hash,
    ]);
    let review_id = format!(
        "lir-{}",
        digest_fields(&[source_kind, source_identity, &evidence_digest])
    );
    transaction.execute(
        "INSERT INTO logical_identity_reviews (
            review_id,store_id,object_key,object_version,source_kind,
            source_identity,reason,evidence_digest,state,
            created_at_utc,updated_at_utc
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,'needs_review',?9,?9)
         ON CONFLICT(source_kind,source_identity,evidence_digest) DO UPDATE SET
            reason=excluded.reason,updated_at_utc=excluded.updated_at_utc",
        params![
            review_id,
            claim.store_id.as_str(),
            claim.object_key,
            to_i64(claim.object_version)?,
            source_kind,
            source_identity,
            reason,
            evidence_digest,
            recorded_at_utc,
        ],
    )?;
    Ok(())
}

#[derive(Debug)]
pub enum LogicalIdentityError {
    Sqlite(rusqlite::Error),
    InvalidField(&'static str),
    InvalidStoreId(String),
    InvalidProfileObject(String),
    ZeroVersion,
    NumericOverflow,
    IdentityConflict {
        store_id: String,
        object_key: String,
        object_version: u64,
    },
    EvidenceConflict {
        store_id: String,
        object_key: String,
        object_version: u64,
    },
    NativeBindingConflict(String),
    UnknownLogicalVersion(String),
    PlacementConflict(String),
    MigrationEvidenceConflict(u64),
}

impl LogicalIdentityError {
    fn is_evidence_conflict(&self) -> bool {
        matches!(
            self,
            Self::IdentityConflict { .. }
                | Self::EvidenceConflict { .. }
                | Self::NativeBindingConflict(_)
                | Self::PlacementConflict(_)
        )
    }
}

impl Display for LogicalIdentityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => write!(formatter, "logical identity SQLite error: {error}"),
            Self::InvalidField(field) => write!(formatter, "{field} must not be blank"),
            Self::InvalidStoreId(store) => write!(formatter, "invalid store ID {store}"),
            Self::InvalidProfileObject(error) => {
                write!(formatter, "invalid portable profile object: {error}")
            }
            Self::ZeroVersion => formatter.write_str("object version must be greater than zero"),
            Self::NumericOverflow => formatter.write_str("logical identity numeric overflow"),
            Self::IdentityConflict {
                store_id,
                object_key,
                object_version,
            } => write!(
                formatter,
                "logical identity conflict for {store_id}/{object_key} version {object_version}"
            ),
            Self::EvidenceConflict {
                store_id,
                object_key,
                object_version,
            } => write!(
                formatter,
                "immutable evidence conflict for {store_id}/{object_key} version {object_version}"
            ),
            Self::NativeBindingConflict(object_id) => {
                write!(
                    formatter,
                    "native object {object_id} has a conflicting identity"
                )
            }
            Self::UnknownLogicalVersion(version_id) => {
                write!(formatter, "unknown logical version {version_id}")
            }
            Self::PlacementConflict(placement_id) => {
                write!(
                    formatter,
                    "placement {placement_id} has conflicting evidence"
                )
            }
            Self::MigrationEvidenceConflict(migration_id) => write!(
                formatter,
                "metadata migration evidence conflicts at migration {migration_id}"
            ),
        }
    }
}

impl std::error::Error for LogicalIdentityError {}

impl From<rusqlite::Error> for LogicalIdentityError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sqlite(value)
    }
}

#[cfg(test)]
mod tests;
