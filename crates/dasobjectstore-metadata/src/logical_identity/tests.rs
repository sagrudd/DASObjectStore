use super::*;
use crate::logical_identity_migration_applied;
use crate::schema::LIVE_SCHEMA_SQL;
use dasobjectstore_core::ids::{ObjectId, PlacementId};
use dasobjectstore_core::object_catalogue::{
    ObjectDigest, PortableLifecycleState, PortablePlacement, PortableProtectionState,
    PortableProvenance,
};
use dasobjectstore_core::protection::ProtectionPolicy;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

const NOW: &str = "2026-07-27T12:00:00Z";
static NEXT_DB: AtomicU64 = AtomicU64::new(1);

fn database(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "dasobjectstore-logical-identity-{label}-{}-{}.sqlite3",
        std::process::id(),
        NEXT_DB.fetch_add(1, Ordering::Relaxed)
    ))
}

fn initialize(path: &Path, stores: &[&str]) {
    let connection = Connection::open(path).expect("database");
    connection
        .execute_batch(LIVE_SCHEMA_SQL)
        .expect("live schema");
    connection
        .execute(
            "INSERT INTO pools(pool_id,state,created_at_utc,updated_at_utc)
             VALUES('pool','Ready',?1,?1)",
            [NOW],
        )
        .expect("pool");
    for store in stores {
        connection
            .execute(
                "INSERT INTO stores(
                    store_id,pool_id,class,policy_json,created_at_utc,updated_at_utc
                 ) VALUES(?1,'pool','generated_data','{}',?2,?2)",
                params![store, NOW],
            )
            .expect("store");
    }
}

fn claim<'a>(store: &'a StoreId, key: &'a str, hash: &'a str) -> LogicalVersionClaim<'a> {
    LogicalVersionClaim {
        store_id: store,
        object_key: key,
        object_version: 1,
        size_bytes: 8,
        content_hash_algorithm: "sha256",
        content_hash: hash,
        recorded_at_utc: NOW,
    }
}

#[test]
fn same_path_across_stores_has_distinct_canonical_identity() {
    let path = database("store-scope");
    initialize(&path, &["store-a", "store-b"]);
    let store_a = StoreId::new("store-a").expect("store");
    let store_b = StoreId::new("store-b").expect("store");

    let (first, _) =
        claim_logical_version(&path, &claim(&store_a, "same/path", "aa")).expect("first identity");
    let (second, _) =
        claim_logical_version(&path, &claim(&store_b, "same/path", "aa")).expect("second identity");

    assert_ne!(first.logical_version_id, second.logical_version_id);
}

#[test]
fn exact_replay_is_idempotent_and_changed_evidence_fails_closed() {
    let path = database("replay");
    initialize(&path, &["store"]);
    let store = StoreId::new("store").expect("store");
    let (created, replay) =
        claim_logical_version(&path, &claim(&store, "key", "aa")).expect("create");
    assert!(!replay);
    let (replayed, replay) =
        claim_logical_version(&path, &claim(&store, "key", "aa")).expect("replay");
    assert!(replay);
    assert_eq!(created, replayed);
    let prefixed = LogicalVersionClaim {
        content_hash_algorithm: "SHA256",
        content_hash: "sha256:AA",
        ..claim(&store, "key", "aa")
    };
    let (_, replay) = claim_logical_version(&path, &prefixed).expect("normalized replay");
    assert!(replay);

    let error = claim_logical_version(&path, &claim(&store, "key", "bb")).expect_err("conflict");
    assert!(matches!(
        error,
        LogicalIdentityError::EvidenceConflict { .. }
    ));
    let connection = Connection::open(path).expect("database");
    let hash: String = connection
        .query_row(
            "SELECT content_hash FROM logical_object_versions",
            [],
            |row| row.get(0),
        )
        .expect("hash");
    assert_eq!(hash, "aa");
}

#[test]
fn one_version_accepts_multiple_exactly_replayable_placements() {
    let path = database("placements");
    initialize(&path, &["store"]);
    let store = StoreId::new("store").expect("store");
    let (version, _) = claim_logical_version(&path, &claim(&store, "key", "aa")).expect("identity");
    let mut connection = Connection::open(&path).expect("database");
    connection
        .execute_batch(LIVE_SCHEMA_SQL)
        .expect("live schema");
    let transaction = connection.transaction().expect("transaction");
    for (source, location) in [("placement-a", "disk-a:key"), ("placement-b", "disk-b:key")] {
        let replay = claim_logical_placement_in_transaction(
            &transaction,
            &LogicalPlacementClaim {
                logical_version_id: &version.logical_version_id,
                placement_kind: "hdd",
                placement_namespace: "native",
                source_placement_id: source,
                location,
                content_hash_algorithm: "sha256",
                content_hash: "aa",
                verified_at_utc: Some(NOW),
                recorded_at_utc: NOW,
            },
        )
        .expect("placement");
        assert!(!replay);
    }
    let replay = claim_logical_placement_in_transaction(
        &transaction,
        &LogicalPlacementClaim {
            logical_version_id: &version.logical_version_id,
            placement_kind: "hdd",
            placement_namespace: "native",
            source_placement_id: "placement-a",
            location: "disk-a:key",
            content_hash_algorithm: "sha256",
            content_hash: "aa",
            verified_at_utc: Some(NOW),
            recorded_at_utc: NOW,
        },
    )
    .expect("placement replay");
    assert!(replay);
    transaction.commit().expect("commit");
    let count: u64 = connection
        .query_row("SELECT COUNT(*) FROM logical_placements", [], |row| {
            row.get(0)
        })
        .expect("count");
    assert_eq!(count, 2);
}

#[test]
fn legacy_backfill_dry_run_rolls_back_then_apply_survives_restart() {
    let path = database("legacy");
    initialize(&path, &["store"]);
    let connection = Connection::open(&path).expect("database");
    connection
        .execute(
            "INSERT INTO disks(
                disk_id,pool_id,role,state,created_at_utc,updated_at_utc
             ) VALUES('disk','pool','hdd','Ready',?1,?1)",
            [NOW],
        )
        .expect("disk");
    connection
        .execute(
            "INSERT INTO objects(
                object_id,store_id,state,size_bytes,content_hash,
                created_at_utc,updated_at_utc
             ) VALUES('object','store','HddCopyVerified',8,'aa',?1,?1)",
            [NOW],
        )
        .expect("object");
    connection
        .execute(
            "INSERT INTO placements(
                placement_id,object_id,disk_id,relative_path,content_hash,
                verified_at_utc,created_at_utc
             ) VALUES('placement','object','disk','key','aa',?1,?1)",
            [NOW],
        )
        .expect("placement");
    connection
        .execute(
            "INSERT INTO metadata_format_versions(
                artifact,major,minor,updated_at_utc
             ) VALUES('live_sqlite',0,12,?1)",
            [NOW],
        )
        .expect("simulate legacy schema marker");
    connection
        .execute("DELETE FROM metadata_migrations WHERE migration_id=13", [])
        .expect("simulate unapplied migration");
    drop(connection);

    let preview = backfill_logical_identities(&path, true, NOW).expect("dry run");
    assert!(preview.dry_run);
    assert_eq!(preview.logical_versions, 1);
    assert_eq!(preview.placements, 1);
    let connection = Connection::open(&path).expect("database");
    let count: u64 = connection
        .query_row("SELECT COUNT(*) FROM logical_object_versions", [], |row| {
            row.get(0)
        })
        .expect("count");
    assert_eq!(count, 0);
    assert_eq!(
        connection
            .query_row(
                "SELECT minor FROM metadata_format_versions
                 WHERE artifact='live_sqlite'",
                [],
                |row| row.get::<_, u64>(0)
            )
            .expect("dry-run schema marker"),
        12
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM metadata_migrations WHERE migration_id=13",
                [],
                |row| row.get::<_, u64>(0)
            )
            .expect("dry-run migration marker"),
        0
    );
    drop(connection);

    let applied = backfill_logical_identities(&path, false, NOW).expect("apply");
    assert_eq!(applied.logical_versions, 1);
    assert_eq!(applied.placements, 1);
    let restarted = backfill_logical_identities(&path, false, NOW).expect("restart replay");
    assert_eq!(restarted.logical_versions, 0);
    assert!(restarted.exact_replays >= 2);
    let connection = Connection::open(&path).expect("database");
    assert_eq!(
        connection
            .query_row(
                "SELECT minor FROM metadata_format_versions
                 WHERE artifact='live_sqlite'",
                [],
                |row| row.get::<_, u64>(0)
            )
            .expect("applied schema marker"),
        13
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM metadata_migrations
                 WHERE migration_id=13
                   AND name='canonical-logical-identity-and-lifecycle-scheduler'",
                [],
                |row| row.get::<_, u64>(0)
            )
            .expect("applied migration marker"),
        1
    );
}

#[test]
fn conflicting_migration_evidence_rolls_back_schema_marker() {
    let path = database("migration-conflict");
    initialize(&path, &["store"]);
    let connection = Connection::open(&path).expect("database");
    connection
        .execute(
            "INSERT INTO metadata_migrations(migration_id,name,applied_at_utc)
             VALUES(13,'unrelated-migration',?1)",
            [NOW],
        )
        .expect("conflicting migration evidence");
    drop(connection);

    assert!(matches!(
        backfill_logical_identities(&path, false, NOW),
        Err(LogicalIdentityError::MigrationEvidenceConflict(13))
    ));
    let connection = Connection::open(&path).expect("database");
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM metadata_format_versions
                 WHERE artifact='live_sqlite'",
                [],
                |row| row.get::<_, u64>(0),
            )
            .expect("schema evidence"),
        0
    );
}

#[test]
fn conflicting_profile_evidence_is_retained_for_review() {
    let path = database("review");
    initialize(&path, &["store"]);
    let connection = Connection::open(&path).expect("database");
    connection
        .execute(
            "INSERT INTO objects(
                object_id,store_id,state,size_bytes,content_hash,
                created_at_utc,updated_at_utc
             ) VALUES('shared','store','HddCopyVerified',8,'aa',?1,?1)",
            [NOW],
        )
        .expect("object");
    let portable = PortableObjectVersion {
        object_id: ObjectId::new("shared").expect("object ID"),
        version: 1,
        size_bytes: 8,
        checksum: ObjectDigest {
            algorithm: "sha256".to_string(),
            value: "bb".to_string(),
        },
        provenance: PortableProvenance {
            source_kind: "provider".to_string(),
            ..PortableProvenance::default()
        },
        lifecycle: PortableLifecycleState::CopyVerified,
        protection_policy: ProtectionPolicy::ExternallyReplicated,
        protection_state: PortableProtectionState::Protected,
        placements: vec![PortablePlacement {
            placement_id: PlacementId::new("provider-placement").expect("placement"),
            location: PortablePlacementLocation::Provider {
                provider: "garage".to_string(),
                object_key: "shared".to_string(),
            },
            checksum: ObjectDigest {
                algorithm: "sha256".to_string(),
                value: "bb".to_string(),
            },
            verified_at_utc: Some(NOW.to_string()),
        }],
    };
    connection
        .execute(
            "INSERT INTO profile_catalogue_transactions(
                transaction_id,profile_namespace,store_id,schema_version,
                source_retained,catalogue_json,committed_at_utc
             ) VALUES('tx','provider:garage','store',1,1,'{}',?1)",
            [NOW],
        )
        .expect("transaction");
    connection
        .execute(
            "INSERT INTO profile_catalogue_objects(
                profile_namespace,store_id,object_id,object_version,
                transaction_id,object_json,committed_at_utc
             ) VALUES('provider:garage','store','shared',1,'tx',?1,?2)",
            params![serde_json::to_string(&portable).expect("JSON"), NOW],
        )
        .expect("profile object");
    drop(connection);

    let report = backfill_logical_identities(&path, false, NOW).expect("backfill");
    assert_eq!(report.needs_review, 1);
    let connection = Connection::open(path).expect("database");
    let review: (String, String) = connection
        .query_row(
            "SELECT state,source_identity FROM logical_identity_reviews",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("review");
    assert_eq!(review.0, "needs_review");
    assert!(review.1.contains("provider:garage"));
}

#[test]
fn database_lock_fails_without_partial_identity() {
    let path = database("locked");
    initialize(&path, &["store"]);
    let locking = Connection::open(&path).expect("locking connection");
    locking
        .execute_batch("BEGIN EXCLUSIVE;")
        .expect("exclusive lock");
    let store = StoreId::new("store").expect("store");
    let error = claim_logical_version(&path, &claim(&store, "key", "aa"))
        .expect_err("database lock must fail");
    assert!(matches!(error, LogicalIdentityError::Sqlite(_)));
    locking
        .execute_batch("ROLLBACK;")
        .expect("release exclusive lock");
    let connection = Connection::open(path).expect("database");
    let count: u64 = connection
        .query_row("SELECT COUNT(*) FROM logical_object_versions", [], |row| {
            row.get(0)
        })
        .expect("count");
    assert_eq!(count, 0);
}

#[test]
fn committed_migration_evidence_skips_repeated_appliance_backfill() {
    let path = database("migration-evidence");
    assert!(!logical_identity_migration_applied(&path).expect("new database"));
    initialize(&path, &["store"]);
    assert!(!logical_identity_migration_applied(&path).expect("unapplied schema"));

    backfill_logical_identities(&path, false, NOW).expect("committed backfill");
    assert!(logical_identity_migration_applied(&path).expect("applied migration"));

    Connection::open(&path)
        .expect("database")
        .execute(
            "UPDATE metadata_migrations SET name='conflicting-name'
             WHERE migration_id=?1",
            [LOGICAL_IDENTITY_MIGRATION_ID],
        )
        .expect("conflicting evidence");
    assert!(matches!(
        logical_identity_migration_applied(&path),
        Err(LogicalIdentityError::MigrationEvidenceConflict(
            LOGICAL_IDENTITY_MIGRATION_ID
        ))
    ));
}
