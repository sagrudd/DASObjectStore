//! Atomic handoff of a portable profile catalogue into daemon-owned metadata.
//!
//! This adapter intentionally does not populate the legacy `objects` or
//! `placements` tables. Those tables encode appliance disk semantics. A
//! profile namespace and transaction id are required here so a later daemon
//! adapter can perform an explicit, conflict-checked physical handoff.

use crate::schema::{LIVE_SCHEMA_FORMAT_VERSION, LIVE_SCHEMA_SQL};
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_core::object_catalogue::PortableObjectCatalogue;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use std::collections::BTreeSet;
use std::fmt::{self, Display};
use std::path::Path;

pub const PROFILE_CATALOGUE_SCHEMA_VERSION: u16 = 1;

pub struct ProfileCatalogueCommitRequest<'a> {
    pub transaction_id: &'a str,
    pub profile_namespace: &'a str,
    pub store_id: &'a StoreId,
    pub catalogue: &'a PortableObjectCatalogue,
    pub source_retained: bool,
    /// Reconcile this namespace to the exact catalogue snapshot. Migration
    /// and incremental import callers must leave this false.
    pub exact_snapshot: bool,
    pub committed_at_utc: &'a str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileCatalogueCommitReport {
    pub object_count: usize,
    pub idempotent: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileCatalogueWithdrawalReport {
    pub objects_removed: usize,
    pub transactions_removed: usize,
}

/// Atomically withdraw one profile namespace from shared discovery metadata.
/// Private profile data is deliberately outside this transaction and retained.
pub fn withdraw_profile_catalogue(
    live_sqlite_path: impl AsRef<Path>,
    profile_namespace: &str,
    store_id: &StoreId,
    dry_run: bool,
) -> Result<ProfileCatalogueWithdrawalReport, ProfileCatalogueCommitError> {
    if profile_namespace.trim().is_empty() {
        return Err(ProfileCatalogueCommitError::BlankField("profile_namespace"));
    }
    let mut connection = Connection::open(live_sqlite_path)?;
    connection.execute_batch(LIVE_SCHEMA_SQL)?;
    let transaction = connection.transaction()?;
    let objects_removed = transaction.query_row(
        "SELECT COUNT(*) FROM profile_catalogue_objects WHERE profile_namespace = ?1 AND store_id = ?2",
        params![profile_namespace, store_id.as_str()],
        |row| row.get::<_, usize>(0),
    )?;
    let transactions_removed = transaction.query_row(
        "SELECT COUNT(*) FROM profile_catalogue_transactions WHERE profile_namespace = ?1 AND store_id = ?2",
        params![profile_namespace, store_id.as_str()],
        |row| row.get::<_, usize>(0),
    )?;
    if !dry_run {
        transaction.execute(
            "DELETE FROM profile_catalogue_objects WHERE profile_namespace = ?1 AND store_id = ?2",
            params![profile_namespace, store_id.as_str()],
        )?;
        transaction.execute(
            "DELETE FROM profile_catalogue_transactions WHERE profile_namespace = ?1 AND store_id = ?2",
            params![profile_namespace, store_id.as_str()],
        )?;
    }
    transaction.commit()?;
    Ok(ProfileCatalogueWithdrawalReport {
        objects_removed,
        transactions_removed,
    })
}

pub fn profile_catalogue_snapshot_matches(
    live_sqlite_path: impl AsRef<Path>,
    profile_namespace: &str,
    store_id: &StoreId,
    catalogue: &PortableObjectCatalogue,
) -> Result<bool, ProfileCatalogueCommitError> {
    catalogue
        .validate()
        .map_err(|error| ProfileCatalogueCommitError::InvalidCatalogue(error.to_string()))?;
    let connection = Connection::open(live_sqlite_path)?;
    let mut statement = connection.prepare(
        "SELECT object_id, object_version, object_json FROM profile_catalogue_objects
         WHERE profile_namespace = ?1 AND store_id = ?2",
    )?;
    let actual = statement
        .query_map(params![profile_namespace, store_id.as_str()], |row| {
            Ok((
                (row.get::<_, String>(0)?, row.get::<_, u64>(1)?),
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<Result<std::collections::BTreeMap<_, _>, _>>()?;
    let expected = catalogue
        .objects
        .iter()
        .map(|object| {
            serde_json::to_string(object)
                .map(|json| ((object.object_id.to_string(), object.version), json))
                .map_err(|error| ProfileCatalogueCommitError::Serialization(error.to_string()))
        })
        .collect::<Result<std::collections::BTreeMap<_, _>, _>>()?;
    Ok(actual == expected)
}

/// Validate and atomically record a portable catalogue handoff.
///
/// Replaying the same transaction and payload is idempotent. Reusing a
/// transaction id with different metadata, or changing an existing logical
/// object version, fails closed without modifying the database.
pub fn commit_profile_catalogue(
    live_sqlite_path: impl AsRef<Path>,
    request: ProfileCatalogueCommitRequest<'_>,
) -> Result<ProfileCatalogueCommitReport, ProfileCatalogueCommitError> {
    validate_request(&request)?;
    let catalogue_json = request
        .catalogue
        .encode_json()
        .map_err(|error| ProfileCatalogueCommitError::InvalidCatalogue(error.to_string()))?;

    let mut connection = Connection::open(live_sqlite_path)?;
    connection.execute_batch(LIVE_SCHEMA_SQL)?;
    let transaction = connection.transaction()?;
    ensure_store(&transaction, request.store_id)?;
    if request.catalogue.store_id != *request.store_id {
        return Err(ProfileCatalogueCommitError::StoreMismatch {
            request_store: request.store_id.to_string(),
            catalogue_store: request.catalogue.store_id.to_string(),
        });
    }
    transaction.execute(
        "INSERT INTO metadata_format_versions (artifact, major, minor, updated_at_utc)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(artifact) DO UPDATE SET
            major = excluded.major, minor = excluded.minor,
            updated_at_utc = excluded.updated_at_utc",
        params![
            LIVE_SCHEMA_FORMAT_VERSION.artifact.name(),
            LIVE_SCHEMA_FORMAT_VERSION.major,
            LIVE_SCHEMA_FORMAT_VERSION.minor,
            request.committed_at_utc,
        ],
    )?;

    if let Some(existing) = transaction
        .query_row(
            "SELECT profile_namespace, store_id, schema_version, source_retained, catalogue_json
             FROM profile_catalogue_transactions WHERE transaction_id = ?1",
            [request.transaction_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u16>(2)?,
                    row.get::<_, bool>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .optional()?
    {
        let matches = existing.0 == request.profile_namespace
            && existing.1 == request.store_id.as_str()
            && existing.2 == request.catalogue.schema_version
            && existing.3 == request.source_retained
            && existing.4 == catalogue_json;
        if matches {
            transaction.commit()?;
            return Ok(ProfileCatalogueCommitReport {
                object_count: request.catalogue.objects.len(),
                idempotent: true,
            });
        }
        return Err(ProfileCatalogueCommitError::TransactionConflict(
            request.transaction_id.to_string(),
        ));
    }

    transaction.execute(
        "INSERT INTO profile_catalogue_transactions (
            transaction_id, profile_namespace, store_id, schema_version,
            source_retained, catalogue_json, committed_at_utc
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            request.transaction_id,
            request.profile_namespace,
            request.store_id.as_str(),
            request.catalogue.schema_version,
            request.source_retained,
            catalogue_json,
            request.committed_at_utc,
        ],
    )?;

    let expected_keys = request
        .catalogue
        .objects
        .iter()
        .map(|object| (object.object_id.to_string(), object.version))
        .collect::<BTreeSet<_>>();
    let existing_keys = {
        let mut statement = transaction.prepare(
            "SELECT object_id, object_version FROM profile_catalogue_objects
             WHERE profile_namespace = ?1 AND store_id = ?2",
        )?;
        let rows = statement
            .query_map(
                params![request.profile_namespace, request.store_id.as_str()],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?)),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };

    for (object_id, object_version) in existing_keys {
        if request.exact_snapshot && !expected_keys.contains(&(object_id.clone(), object_version)) {
            transaction.execute(
                "DELETE FROM profile_catalogue_objects
                 WHERE profile_namespace = ?1 AND store_id = ?2
                   AND object_id = ?3 AND object_version = ?4",
                params![
                    request.profile_namespace,
                    request.store_id.as_str(),
                    object_id,
                    object_version,
                ],
            )?;
        }
    }

    for object in &request.catalogue.objects {
        let object_json = serde_json::to_string(object)
            .map_err(|error| ProfileCatalogueCommitError::Serialization(error.to_string()))?;
        let existing = transaction
            .query_row(
                "SELECT object_json FROM profile_catalogue_objects
                 WHERE profile_namespace = ?1 AND store_id = ?2
                   AND object_id = ?3 AND object_version = ?4",
                params![
                    request.profile_namespace,
                    request.store_id.as_str(),
                    object.object_id.as_str(),
                    object.version,
                ],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if existing.as_ref().is_some_and(|value| value != &object_json) {
            return Err(ProfileCatalogueCommitError::ObjectVersionConflict {
                object_id: object.object_id.to_string(),
                version: object.version,
            });
        }
        if existing.is_some() {
            continue;
        }
        transaction.execute(
            "INSERT INTO profile_catalogue_objects (
                profile_namespace, store_id, object_id, object_version,
                transaction_id, object_json, committed_at_utc
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                request.profile_namespace,
                request.store_id.as_str(),
                object.object_id.as_str(),
                object.version,
                request.transaction_id,
                object_json,
                request.committed_at_utc,
            ],
        )?;
    }

    transaction.commit()?;
    Ok(ProfileCatalogueCommitReport {
        object_count: request.catalogue.objects.len(),
        idempotent: false,
    })
}

fn validate_request(
    request: &ProfileCatalogueCommitRequest<'_>,
) -> Result<(), ProfileCatalogueCommitError> {
    if request.transaction_id.trim().is_empty() {
        return Err(ProfileCatalogueCommitError::BlankField("transaction_id"));
    }
    if request.profile_namespace.trim().is_empty() {
        return Err(ProfileCatalogueCommitError::BlankField("profile_namespace"));
    }
    if request.committed_at_utc.trim().is_empty() {
        return Err(ProfileCatalogueCommitError::BlankField("committed_at_utc"));
    }
    if !request.source_retained {
        return Err(ProfileCatalogueCommitError::SourceRetentionRequired);
    }
    if request.catalogue.schema_version != PROFILE_CATALOGUE_SCHEMA_VERSION {
        return Err(ProfileCatalogueCommitError::UnsupportedSchema(
            request.catalogue.schema_version,
        ));
    }
    request
        .catalogue
        .validate()
        .map_err(|error| ProfileCatalogueCommitError::InvalidCatalogue(error.to_string()))
}

fn ensure_store(
    transaction: &Transaction<'_>,
    store_id: &StoreId,
) -> Result<(), ProfileCatalogueCommitError> {
    let exists = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM stores WHERE store_id = ?1)",
        [store_id.as_str()],
        |row| row.get::<_, bool>(0),
    )?;
    if exists {
        Ok(())
    } else {
        Err(ProfileCatalogueCommitError::MissingStore(store_id.clone()))
    }
}

#[derive(Debug)]
pub enum ProfileCatalogueCommitError {
    Io(rusqlite::Error),
    MissingStore(StoreId),
    BlankField(&'static str),
    SourceRetentionRequired,
    UnsupportedSchema(u16),
    InvalidCatalogue(String),
    StoreMismatch {
        request_store: String,
        catalogue_store: String,
    },
    Serialization(String),
    TransactionConflict(String),
    ObjectVersionConflict {
        object_id: String,
        version: u64,
    },
}

impl Display for ProfileCatalogueCommitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "profile catalogue commit failed: {error}"),
            Self::MissingStore(store_id) => write!(formatter, "store {store_id} is not registered"),
            Self::BlankField(field) => write!(formatter, "{field} must not be blank"),
            Self::SourceRetentionRequired => formatter.write_str("source_retained must be true"),
            Self::UnsupportedSchema(version) => {
                write!(formatter, "unsupported catalogue schema {version}")
            }
            Self::InvalidCatalogue(error) => write!(formatter, "invalid catalogue: {error}"),
            Self::StoreMismatch {
                request_store,
                catalogue_store,
            } => write!(
                formatter,
                "request store {request_store} does not match catalogue store {catalogue_store}"
            ),
            Self::Serialization(error) => {
                write!(formatter, "catalogue serialization failed: {error}")
            }
            Self::TransactionConflict(id) => write!(
                formatter,
                "catalogue transaction {id} conflicts with existing metadata"
            ),
            Self::ObjectVersionConflict { object_id, version } => write!(
                formatter,
                "object {object_id} version {version} conflicts with existing metadata"
            ),
        }
    }
}

impl std::error::Error for ProfileCatalogueCommitError {}

impl From<rusqlite::Error> for ProfileCatalogueCommitError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Io(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dasobjectstore_core::ids::{ObjectId, PlacementId};
    use dasobjectstore_core::object_catalogue::{
        ObjectDigest, PortableLifecycleState, PortableObjectVersion, PortablePlacement,
        PortablePlacementLocation, PortableProtectionState, PortableProvenance,
    };
    use dasobjectstore_core::protection::ProtectionPolicy;
    use rusqlite::Connection;

    #[test]
    fn commits_atomically_and_replays_idempotently() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-profile-catalogue-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root");
        let db = root.join("live.sqlite");
        let connection = Connection::open(&db).expect("db");
        connection.execute_batch(LIVE_SCHEMA_SQL).expect("schema");
        connection
            .execute(
                "INSERT INTO pools VALUES ('pool-a', 'Clean', 'now', 'now')",
                [],
            )
            .expect("pool");
        connection
            .execute(
                "INSERT INTO stores VALUES ('store-a', 'pool-a', 'folder', '{}', 'now', 'now')",
                [],
            )
            .expect("store");
        drop(connection);
        let store_id = StoreId::new("store-a").expect("store id");
        let catalogue = sample_catalogue();
        let request = ProfileCatalogueCommitRequest {
            transaction_id: "tx-1",
            profile_namespace: "folder:primary",
            store_id: &store_id,
            catalogue: &catalogue,
            source_retained: true,
            exact_snapshot: false,
            committed_at_utc: "2026-07-14T00:00:00Z",
        };
        let first = commit_profile_catalogue(&db, request).expect("first commit");
        assert_eq!(first.object_count, 1);
        assert!(!first.idempotent);
        let replay = commit_profile_catalogue(
            &db,
            ProfileCatalogueCommitRequest {
                transaction_id: "tx-1",
                profile_namespace: "folder:primary",
                store_id: &store_id,
                catalogue: &catalogue,
                source_retained: true,
                exact_snapshot: false,
                committed_at_utc: "2026-07-14T00:00:00Z",
            },
        )
        .expect("replay");
        assert!(replay.idempotent);
        assert_eq!(replay.object_count, 1);
        assert!(
            profile_catalogue_snapshot_matches(&db, "folder:primary", &store_id, &catalogue,)
                .expect("matching snapshot")
        );
        let empty = PortableObjectCatalogue {
            schema_version: PROFILE_CATALOGUE_SCHEMA_VERSION,
            store_id: store_id.clone(),
            objects: Vec::new(),
        };
        assert!(
            !profile_catalogue_snapshot_matches(&db, "folder:primary", &store_id, &empty,)
                .expect("drifted snapshot")
        );
        let connection = Connection::open(&db).expect("db");
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM profile_catalogue_objects",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .expect("count"),
            1
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_non_retained_source_before_opening_database() {
        let store_id = StoreId::new("store-a").expect("store id");
        let catalogue = sample_catalogue();
        let error = commit_profile_catalogue(
            "/definitely/not/created/live.sqlite",
            ProfileCatalogueCommitRequest {
                transaction_id: "tx-1",
                profile_namespace: "folder:primary",
                store_id: &store_id,
                catalogue: &catalogue,
                source_retained: false,
                exact_snapshot: false,
                committed_at_utc: "now",
            },
        )
        .expect_err("retention guard");
        assert!(matches!(
            error,
            ProfileCatalogueCommitError::SourceRetentionRequired
        ));
    }

    #[test]
    fn replaces_namespace_rows_with_the_exact_authoritative_snapshot() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-profile-catalogue-snapshot-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root");
        let db = root.join("live.sqlite");
        let connection = Connection::open(&db).expect("db");
        connection.execute_batch(LIVE_SCHEMA_SQL).expect("schema");
        connection
            .execute(
                "INSERT INTO pools VALUES ('pool-a', 'Clean', 'now', 'now')",
                [],
            )
            .expect("pool");
        connection
            .execute(
                "INSERT INTO stores VALUES ('store-a', 'pool-a', 'folder', '{}', 'now', 'now')",
                [],
            )
            .expect("store");
        drop(connection);

        let store_id = StoreId::new("store-a").expect("store id");
        let mut first = sample_catalogue();
        let mut second_object = first.objects[0].clone();
        second_object.object_id = ObjectId::new("object-b").expect("object id");
        second_object.placements[0].placement_id =
            PlacementId::new("placement-b").expect("placement id");
        first.objects.push(second_object);
        commit_profile_catalogue(
            &db,
            ProfileCatalogueCommitRequest {
                transaction_id: "tx-full",
                profile_namespace: "folder:primary",
                store_id: &store_id,
                catalogue: &first,
                source_retained: true,
                exact_snapshot: true,
                committed_at_utc: "2026-07-16T00:00:00Z",
            },
        )
        .expect("full snapshot");
        let remaining = sample_catalogue();
        commit_profile_catalogue(
            &db,
            ProfileCatalogueCommitRequest {
                transaction_id: "tx-delete",
                profile_namespace: "folder:primary",
                store_id: &store_id,
                catalogue: &remaining,
                source_retained: true,
                exact_snapshot: true,
                committed_at_utc: "2026-07-16T00:01:00Z",
            },
        )
        .expect("replacement snapshot");
        let connection = Connection::open(&db).expect("db");
        let rows = connection
            .prepare("SELECT object_id FROM profile_catalogue_objects ORDER BY object_id")
            .expect("query")
            .query_map([], |row| row.get::<_, String>(0))
            .expect("rows")
            .collect::<Result<Vec<_>, _>>()
            .expect("object ids");
        assert_eq!(rows, vec!["object-a"]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn withdrawal_previews_then_removes_only_the_selected_profile_namespace() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-profile-withdrawal-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root");
        let db = root.join("live.sqlite");
        let connection = Connection::open(&db).expect("db");
        connection.execute_batch(LIVE_SCHEMA_SQL).expect("schema");
        connection
            .execute(
                "INSERT INTO pools VALUES ('pool-a', 'Clean', 'now', 'now')",
                [],
            )
            .expect("pool");
        connection
            .execute(
                "INSERT INTO stores VALUES ('store-a', 'pool-a', 'folder', '{}', 'now', 'now')",
                [],
            )
            .expect("store");
        drop(connection);
        let store_id = StoreId::new("store-a").expect("store id");
        let catalogue = sample_catalogue();
        for (transaction_id, namespace) in
            [("tx-retire", "folder:retire"), ("tx-keep", "folder:keep")]
        {
            commit_profile_catalogue(
                &db,
                ProfileCatalogueCommitRequest {
                    transaction_id,
                    profile_namespace: namespace,
                    store_id: &store_id,
                    catalogue: &catalogue,
                    source_retained: true,
                    exact_snapshot: true,
                    committed_at_utc: "2026-07-16T12:00:00Z",
                },
            )
            .expect("catalogue commit");
        }
        assert_eq!(
            withdraw_profile_catalogue(&db, "folder:retire", &store_id, true).expect("preview"),
            ProfileCatalogueWithdrawalReport {
                objects_removed: 1,
                transactions_removed: 1,
            }
        );
        assert_eq!(
            withdraw_profile_catalogue(&db, "folder:retire", &store_id, false).expect("withdraw"),
            ProfileCatalogueWithdrawalReport {
                objects_removed: 1,
                transactions_removed: 1,
            }
        );
        let connection = Connection::open(&db).expect("db");
        let remaining = connection
            .query_row(
                "SELECT COUNT(*) FROM profile_catalogue_objects WHERE profile_namespace = 'folder:keep'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("remaining object count");
        assert_eq!(remaining, 1);
        let _ = std::fs::remove_dir_all(root);
    }

    fn sample_catalogue() -> PortableObjectCatalogue {
        PortableObjectCatalogue {
            schema_version: PROFILE_CATALOGUE_SCHEMA_VERSION,
            store_id: StoreId::new("store-a").expect("store id"),
            objects: vec![PortableObjectVersion {
                object_id: ObjectId::new("object-a").expect("object id"),
                version: 1,
                size_bytes: 3,
                checksum: ObjectDigest {
                    algorithm: "sha256".into(),
                    value: "abc".into(),
                },
                provenance: PortableProvenance {
                    source_kind: "test".into(),
                    locator: None,
                    revision: None,
                },
                lifecycle: PortableLifecycleState::CopyVerified,
                protection_policy: ProtectionPolicy::LocalOnly,
                protection_state: PortableProtectionState::Verified,
                placements: vec![PortablePlacement {
                    placement_id: PlacementId::new("placement-a").expect("placement id"),
                    location: PortablePlacementLocation::Folder {
                        relative_path: "objects/a".into(),
                    },
                    checksum: ObjectDigest {
                        algorithm: "sha256".into(),
                        value: "abc".into(),
                    },
                    verified_at_utc: Some("now".into()),
                }],
            }],
        }
    }
}
