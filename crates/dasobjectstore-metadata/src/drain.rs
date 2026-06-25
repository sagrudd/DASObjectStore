use dasobjectstore_core::ids::{DiskId, InvalidId, ObjectId, StoreId};
use dasobjectstore_core::lifecycle::HealthState;
use dasobjectstore_core::placement::{PerformanceClass, PlacementCandidate, WriteLoad};
use dasobjectstore_core::protection::VerifiedCopy;
use dasobjectstore_core::repair::{
    plan_protected_store_evacuation, plan_reproducible_cache_evacuation, ProtectedObjectCopies,
};
use dasobjectstore_core::store::StorePolicy;
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use std::fmt::{self, Display};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DiskDrainPlanSummary {
    pub disk_id: DiskId,
    pub live_sqlite_path: PathBuf,
    pub protected_copy_tasks: usize,
    pub protected_blocked_objects: usize,
    pub cache_copy_tasks: usize,
    pub cache_redownload_required_objects: usize,
    pub affected_objects: Vec<DiskDrainObjectSummary>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DiskReplacementPlanSummary {
    pub old_disk_id: DiskId,
    pub new_disk_id: DiskId,
    pub live_sqlite_path: PathBuf,
    pub protected_copy_tasks: usize,
    pub protected_blocked_objects: usize,
    pub cache_copy_tasks: usize,
    pub cache_redownload_required_objects: usize,
    pub affected_objects: Vec<DiskDrainObjectSummary>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DiskDrainObjectSummary {
    pub object_id: ObjectId,
    pub store_id: StoreId,
    pub action: DiskDrainAction,
    pub destination_disk_ids: Vec<DiskId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiskDrainAction {
    CopyPlanned,
    Blocked,
    RedownloadRequired,
}

#[derive(Debug)]
pub enum DiskDrainError {
    Sqlite(rusqlite::Error),
    Json(serde_json::Error),
    DiskNotFound {
        disk_id: DiskId,
    },
    InvalidIdentifier {
        field: &'static str,
        source: InvalidId,
    },
    NegativeByteCount {
        field: &'static str,
        value: i64,
    },
    SameDiskReplacement {
        disk_id: DiskId,
    },
}

impl Display for DiskDrainError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(err) => write!(formatter, "failed to read drain metadata: {err}"),
            Self::Json(err) => write!(formatter, "failed to parse store policy JSON: {err}"),
            Self::DiskNotFound { disk_id } => {
                write!(formatter, "disk {disk_id} does not exist in live metadata")
            }
            Self::InvalidIdentifier { field, source } => {
                write!(formatter, "invalid drain metadata {field}: {source}")
            }
            Self::NegativeByteCount { field, value } => {
                write!(
                    formatter,
                    "invalid negative drain metadata {field}: {value}"
                )
            }
            Self::SameDiskReplacement { disk_id } => {
                write!(formatter, "cannot replace disk {disk_id} with itself")
            }
        }
    }
}

impl std::error::Error for DiskDrainError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sqlite(err) => Some(err),
            Self::Json(err) => Some(err),
            Self::DiskNotFound { .. }
            | Self::InvalidIdentifier { .. }
            | Self::NegativeByteCount { .. }
            | Self::SameDiskReplacement { .. } => None,
        }
    }
}

impl From<rusqlite::Error> for DiskDrainError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Sqlite(err)
    }
}

impl From<serde_json::Error> for DiskDrainError {
    fn from(err: serde_json::Error) -> Self {
        Self::Json(err)
    }
}

pub fn read_disk_drain_plan(
    live_sqlite_path: impl AsRef<Path>,
    disk_id: &DiskId,
) -> Result<DiskDrainPlanSummary, DiskDrainError> {
    let live_sqlite_path = live_sqlite_path.as_ref();
    let connection = Connection::open(live_sqlite_path)?;
    ensure_disk_exists(&connection, disk_id)?;

    let objects = read_source_disk_objects(&connection, disk_id)?;
    let candidates = read_placement_candidates(&connection, disk_id)?;
    build_disk_drain_plan_summary(live_sqlite_path, disk_id, &objects, &candidates)
}

pub fn read_disk_replacement_plan(
    live_sqlite_path: impl AsRef<Path>,
    old_disk_id: &DiskId,
    new_disk_id: &DiskId,
) -> Result<DiskReplacementPlanSummary, DiskDrainError> {
    if old_disk_id == new_disk_id {
        return Err(DiskDrainError::SameDiskReplacement {
            disk_id: old_disk_id.clone(),
        });
    }

    let live_sqlite_path = live_sqlite_path.as_ref();
    let connection = Connection::open(live_sqlite_path)?;
    ensure_disk_exists(&connection, old_disk_id)?;
    ensure_disk_exists(&connection, new_disk_id)?;

    let objects = read_source_disk_objects(&connection, old_disk_id)?;
    let candidates = read_placement_candidates(&connection, old_disk_id)?
        .into_iter()
        .filter(|candidate| &candidate.disk_id == new_disk_id)
        .collect::<Vec<_>>();
    let drain_plan =
        build_disk_drain_plan_summary(live_sqlite_path, old_disk_id, &objects, &candidates)?;

    Ok(DiskReplacementPlanSummary {
        old_disk_id: old_disk_id.clone(),
        new_disk_id: new_disk_id.clone(),
        live_sqlite_path: drain_plan.live_sqlite_path,
        protected_copy_tasks: drain_plan.protected_copy_tasks,
        protected_blocked_objects: drain_plan.protected_blocked_objects,
        cache_copy_tasks: drain_plan.cache_copy_tasks,
        cache_redownload_required_objects: drain_plan.cache_redownload_required_objects,
        affected_objects: drain_plan.affected_objects,
    })
}

fn build_disk_drain_plan_summary(
    live_sqlite_path: &Path,
    disk_id: &DiskId,
    objects: &[ProtectedObjectCopies],
    candidates: &[PlacementCandidate],
) -> Result<DiskDrainPlanSummary, DiskDrainError> {
    let protected_plan = plan_protected_store_evacuation(disk_id, objects, candidates)
        .expect("store policies are validated before drain planning");
    let cache_plan = plan_reproducible_cache_evacuation(disk_id, objects, candidates)
        .expect("store policies are validated before drain planning");

    let mut affected_objects = Vec::new();
    for task in &protected_plan.tasks {
        affected_objects.push(DiskDrainObjectSummary {
            object_id: task.object_id.clone(),
            store_id: task.store_id.clone(),
            action: DiskDrainAction::CopyPlanned,
            destination_disk_ids: task
                .replacement_plan
                .planned_copies
                .iter()
                .map(|copy| copy.disk_id.clone())
                .collect(),
        });
    }
    for blocked in &protected_plan.blocked_objects {
        affected_objects.push(DiskDrainObjectSummary {
            object_id: blocked.object_id.clone(),
            store_id: blocked.store_id.clone(),
            action: DiskDrainAction::Blocked,
            destination_disk_ids: Vec::new(),
        });
    }
    for task in &cache_plan.tasks {
        affected_objects.push(DiskDrainObjectSummary {
            object_id: task.object_id.clone(),
            store_id: task.store_id.clone(),
            action: DiskDrainAction::CopyPlanned,
            destination_disk_ids: task
                .replacement_plan
                .planned_copies
                .iter()
                .map(|copy| copy.disk_id.clone())
                .collect(),
        });
    }
    for redownload in &cache_plan.redownload_required {
        affected_objects.push(DiskDrainObjectSummary {
            object_id: redownload.object_id.clone(),
            store_id: redownload.store_id.clone(),
            action: DiskDrainAction::RedownloadRequired,
            destination_disk_ids: Vec::new(),
        });
    }
    affected_objects.sort_by(|left, right| left.object_id.cmp(&right.object_id));

    Ok(DiskDrainPlanSummary {
        disk_id: disk_id.clone(),
        live_sqlite_path: live_sqlite_path.to_path_buf(),
        protected_copy_tasks: protected_plan.tasks.len(),
        protected_blocked_objects: protected_plan.blocked_objects.len(),
        cache_copy_tasks: cache_plan.tasks.len(),
        cache_redownload_required_objects: cache_plan.redownload_required.len(),
        affected_objects,
    })
}

fn ensure_disk_exists(connection: &Connection, disk_id: &DiskId) -> Result<(), DiskDrainError> {
    let exists = connection
        .query_row(
            "SELECT 1 FROM disks WHERE disk_id = ?1",
            [disk_id.as_str()],
            |_| Ok(()),
        )
        .optional()?
        .is_some();

    if exists {
        Ok(())
    } else {
        Err(DiskDrainError::DiskNotFound {
            disk_id: disk_id.clone(),
        })
    }
}

fn read_source_disk_objects(
    connection: &Connection,
    disk_id: &DiskId,
) -> Result<Vec<ProtectedObjectCopies>, DiskDrainError> {
    let mut statement = connection.prepare(
        "SELECT DISTINCT
            objects.object_id,
            objects.store_id,
            objects.size_bytes,
            stores.policy_json
         FROM placements
         INNER JOIN objects ON objects.object_id = placements.object_id
         INNER JOIN stores ON stores.store_id = objects.store_id
         WHERE placements.disk_id = ?1
           AND placements.verified_at_utc IS NOT NULL
         ORDER BY objects.object_id",
    )?;
    let rows = statement.query_map([disk_id.as_str()], |row| {
        let object_id = parse_id("object_id", row.get::<_, String>(0)?)?;
        let store_id = parse_id("store_id", row.get::<_, String>(1)?)?;
        let object_size_bytes =
            optional_u64("size_bytes", row.get::<_, Option<i64>>(2)?)?.unwrap_or_default();
        let policy_json: String = row.get(3)?;
        Ok((object_id, store_id, object_size_bytes, policy_json))
    })?;

    let mut objects = Vec::new();
    for row in rows {
        let (object_id, store_id, object_size_bytes, policy_json) = row?;
        let policy: StorePolicy = serde_json::from_str(&policy_json)?;
        let verified_copies = read_verified_copies(connection, &object_id)?;
        objects.push(ProtectedObjectCopies::new(
            object_id,
            store_id,
            object_size_bytes,
            policy,
            verified_copies,
        ));
    }

    Ok(objects)
}

fn read_verified_copies(
    connection: &Connection,
    object_id: &ObjectId,
) -> Result<Vec<VerifiedCopy>, DiskDrainError> {
    let mut statement = connection.prepare(
        "SELECT disk_id
         FROM placements
         WHERE object_id = ?1
           AND verified_at_utc IS NOT NULL
         ORDER BY placement_id",
    )?;
    let rows = statement.query_map([object_id.as_str()], |row| {
        let disk_id = parse_id("disk_id", row.get::<_, String>(0)?)?;
        Ok(disk_id)
    })?;

    let mut copies = Vec::new();
    for (index, row) in rows.enumerate() {
        copies.push(VerifiedCopy::new(row?, index as u8 + 1));
    }

    Ok(copies)
}

fn read_placement_candidates(
    connection: &Connection,
    source_disk_id: &DiskId,
) -> Result<Vec<PlacementCandidate>, DiskDrainError> {
    let mut statement = connection.prepare(
        "SELECT disk_id, state, size_bytes, enclosure_topology_path
         FROM disks
         WHERE disk_id != ?1
         ORDER BY disk_id",
    )?;
    let rows = statement.query_map([source_disk_id.as_str()], |row| {
        let disk_id = parse_id("disk_id", row.get::<_, String>(0)?)?;
        let state: String = row.get(1)?;
        let available_bytes =
            optional_u64("size_bytes", row.get::<_, Option<i64>>(2)?)?.unwrap_or(u64::MAX / 2);
        let enclosure_id = row
            .get::<_, Option<String>>(3)?
            .map(|value| parse_id("enclosure_topology_path", value))
            .transpose()?;

        Ok(PlacementCandidate::new(
            disk_id,
            enclosure_id,
            available_bytes,
            health_state_from_disk_state(&state),
            PerformanceClass::Unknown,
            WriteLoad::Idle,
        ))
    })?;

    let mut candidates = Vec::new();
    for row in rows {
        candidates.push(row?);
    }

    Ok(candidates)
}

fn health_state_from_disk_state(state: &str) -> HealthState {
    match state {
        "Healthy" => HealthState::Healthy,
        "Watch" => HealthState::Watch,
        "Suspect" => HealthState::Suspect,
        "Draining" => HealthState::Draining,
        "Retired" => HealthState::Retired,
        "Failed" => HealthState::Failed,
        _ => HealthState::Suspect,
    }
}

fn parse_id<T>(field: &'static str, value: String) -> Result<T, rusqlite::Error>
where
    T: std::str::FromStr<Err = InvalidId>,
{
    value.parse().map_err(|source| {
        rusqlite::Error::ToSqlConversionFailure(Box::new(DiskDrainError::InvalidIdentifier {
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
                    DiskDrainError::NegativeByteCount { field, value },
                ))
            })
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::{
        read_disk_drain_plan, read_disk_replacement_plan, DiskDrainAction, DiskDrainError,
    };
    use crate::schema::LIVE_SCHEMA_SQL;
    use dasobjectstore_core::ids::DiskId;
    use dasobjectstore_core::store::{StoreClass, StorePolicy};
    use rusqlite::Connection;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn reads_disk_drain_plan_for_protected_and_cache_objects() {
        let root = temp_root("disk-drain");
        fs::create_dir_all(&root).expect("create temp root");
        let live_sqlite_path = root.join("live.sqlite");
        let connection = fixture_connection(&live_sqlite_path);
        insert_store(
            &connection,
            "generated",
            StorePolicy::defaults_for(StoreClass::GeneratedData),
        );
        insert_store(
            &connection,
            "cache",
            StorePolicy::defaults_for(StoreClass::ReproducibleCache),
        );
        insert_disk(&connection, "disk-a", "Healthy", 1_000);
        insert_disk(&connection, "disk-b", "Healthy", 1_000);
        insert_disk(&connection, "disk-c", "Healthy", 1_000);
        insert_object(&connection, "object-protected", "generated", 100);
        insert_object(&connection, "object-cache", "cache", 100);
        insert_placement(&connection, "placement-a", "object-protected", "disk-a");
        insert_placement(&connection, "placement-b", "object-protected", "disk-b");
        insert_placement(&connection, "placement-c", "object-cache", "disk-a");

        let plan =
            read_disk_drain_plan(&live_sqlite_path, &DiskId::new("disk-a").expect("disk id"))
                .expect("drain plan");

        assert_eq!(plan.disk_id.as_str(), "disk-a");
        assert_eq!(plan.protected_copy_tasks, 1);
        assert_eq!(plan.protected_blocked_objects, 0);
        assert_eq!(plan.cache_copy_tasks, 1);
        assert_eq!(plan.cache_redownload_required_objects, 0);
        assert_eq!(plan.affected_objects.len(), 2);
        assert!(plan
            .affected_objects
            .iter()
            .all(|object| object.action == DiskDrainAction::CopyPlanned));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn marks_cache_object_redownload_required_when_no_candidate_exists() {
        let root = temp_root("disk-drain-cache-blocked");
        fs::create_dir_all(&root).expect("create temp root");
        let live_sqlite_path = root.join("live.sqlite");
        let connection = fixture_connection(&live_sqlite_path);
        insert_store(
            &connection,
            "cache",
            StorePolicy::defaults_for(StoreClass::ReproducibleCache),
        );
        insert_disk(&connection, "disk-a", "Healthy", 1_000);
        insert_object(&connection, "object-cache", "cache", 100);
        insert_placement(&connection, "placement-a", "object-cache", "disk-a");

        let plan =
            read_disk_drain_plan(&live_sqlite_path, &DiskId::new("disk-a").expect("disk id"))
                .expect("drain plan");

        assert_eq!(plan.cache_copy_tasks, 0);
        assert_eq!(plan.cache_redownload_required_objects, 1);
        assert_eq!(
            plan.affected_objects[0].action,
            DiskDrainAction::RedownloadRequired
        );

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn plans_protected_evacuation_from_suspect_disk() {
        let root = temp_root("disk-drain-suspect");
        fs::create_dir_all(&root).expect("create temp root");
        let live_sqlite_path = root.join("live.sqlite");
        let connection = fixture_connection(&live_sqlite_path);
        insert_store(
            &connection,
            "generated",
            StorePolicy::defaults_for(StoreClass::GeneratedData),
        );
        insert_disk(&connection, "disk-a", "Suspect", 1_000);
        insert_disk(&connection, "disk-b", "Healthy", 1_000);
        insert_disk(&connection, "disk-c", "Healthy", 1_000);
        insert_object(&connection, "object-protected", "generated", 100);
        insert_placement(&connection, "placement-a", "object-protected", "disk-a");
        insert_placement(&connection, "placement-b", "object-protected", "disk-b");

        let plan =
            read_disk_drain_plan(&live_sqlite_path, &DiskId::new("disk-a").expect("disk id"))
                .expect("drain plan");

        assert_eq!(plan.disk_id.as_str(), "disk-a");
        assert_eq!(plan.protected_copy_tasks, 1);
        assert_eq!(plan.protected_blocked_objects, 0);
        assert_eq!(plan.affected_objects.len(), 1);
        assert_eq!(
            plan.affected_objects[0].action,
            DiskDrainAction::CopyPlanned
        );
        assert_eq!(
            plan.affected_objects[0].destination_disk_ids[0].as_str(),
            "disk-c"
        );

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn rejects_missing_source_disk() {
        let root = temp_root("disk-drain-missing");
        fs::create_dir_all(&root).expect("create temp root");
        let live_sqlite_path = root.join("live.sqlite");
        let _connection = fixture_connection(&live_sqlite_path);

        let err = read_disk_drain_plan(&live_sqlite_path, &DiskId::new("disk-a").expect("disk id"))
            .expect_err("missing disk fails");

        assert!(matches!(err, DiskDrainError::DiskNotFound { .. }));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn reads_disk_replacement_plan_for_named_destination() {
        let root = temp_root("disk-replace");
        fs::create_dir_all(&root).expect("create temp root");
        let live_sqlite_path = root.join("live.sqlite");
        let connection = fixture_connection(&live_sqlite_path);
        insert_store(
            &connection,
            "generated",
            StorePolicy::defaults_for(StoreClass::GeneratedData),
        );
        insert_disk(&connection, "disk-a", "Healthy", 1_000);
        insert_disk(&connection, "disk-b", "Healthy", 1_000);
        insert_disk(&connection, "disk-c", "Healthy", 1_000);
        insert_object(&connection, "object-protected", "generated", 100);
        insert_placement(&connection, "placement-a", "object-protected", "disk-a");

        let plan = read_disk_replacement_plan(
            &live_sqlite_path,
            &DiskId::new("disk-a").expect("old disk id"),
            &DiskId::new("disk-b").expect("new disk id"),
        )
        .expect("replacement plan");

        assert_eq!(plan.old_disk_id.as_str(), "disk-a");
        assert_eq!(plan.new_disk_id.as_str(), "disk-b");
        assert_eq!(plan.protected_copy_tasks, 1);
        assert_eq!(plan.protected_blocked_objects, 0);
        assert_eq!(plan.affected_objects.len(), 1);
        assert_eq!(
            plan.affected_objects[0].destination_disk_ids[0].as_str(),
            "disk-b"
        );

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn replacement_plan_blocks_when_named_destination_cannot_take_copy() {
        let root = temp_root("disk-replace-blocked");
        fs::create_dir_all(&root).expect("create temp root");
        let live_sqlite_path = root.join("live.sqlite");
        let connection = fixture_connection(&live_sqlite_path);
        insert_store(
            &connection,
            "generated",
            StorePolicy::defaults_for(StoreClass::GeneratedData),
        );
        insert_disk(&connection, "disk-a", "Healthy", 1_000);
        insert_disk(&connection, "disk-b", "Failed", 1_000);
        insert_disk(&connection, "disk-c", "Healthy", 1_000);
        insert_object(&connection, "object-protected", "generated", 100);
        insert_placement(&connection, "placement-a", "object-protected", "disk-a");

        let plan = read_disk_replacement_plan(
            &live_sqlite_path,
            &DiskId::new("disk-a").expect("old disk id"),
            &DiskId::new("disk-b").expect("new disk id"),
        )
        .expect("replacement plan");

        assert_eq!(plan.protected_copy_tasks, 0);
        assert_eq!(plan.protected_blocked_objects, 1);
        assert_eq!(plan.affected_objects[0].action, DiskDrainAction::Blocked);
        assert!(plan.affected_objects[0].destination_disk_ids.is_empty());

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn rejects_replacing_disk_with_itself() {
        let root = temp_root("disk-replace-same");
        fs::create_dir_all(&root).expect("create temp root");
        let live_sqlite_path = root.join("live.sqlite");
        let _connection = fixture_connection(&live_sqlite_path);

        let err = read_disk_replacement_plan(
            &live_sqlite_path,
            &DiskId::new("disk-a").expect("old disk id"),
            &DiskId::new("disk-a").expect("new disk id"),
        )
        .expect_err("same disk replacement fails");

        assert!(matches!(err, DiskDrainError::SameDiskReplacement { .. }));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    fn fixture_connection(path: &PathBuf) -> Connection {
        let connection = Connection::open(path).expect("open sqlite");
        connection
            .execute_batch(LIVE_SCHEMA_SQL)
            .expect("schema applies");
        connection
            .execute(
                "INSERT INTO pools (pool_id, state, created_at_utc, updated_at_utc)
                 VALUES ('pool-a', 'Clean', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )
            .expect("pool inserts");
        connection
    }

    fn insert_store(connection: &Connection, store_id: &str, policy: StorePolicy) {
        connection
            .execute(
                "INSERT INTO stores (
                    store_id, pool_id, class, policy_json, created_at_utc, updated_at_utc
                 ) VALUES (?1, 'pool-a', ?2, ?3, ?4, ?4)",
                (
                    store_id,
                    policy.class.name(),
                    serde_json::to_string(&policy).expect("policy serializes"),
                    "2026-01-01T00:00:00Z",
                ),
            )
            .expect("store inserts");
    }

    fn insert_disk(connection: &Connection, disk_id: &str, state: &str, size_bytes: i64) {
        connection
            .execute(
                "INSERT INTO disks (
                    disk_id, pool_id, role, state, size_bytes, created_at_utc, updated_at_utc
                 ) VALUES (?1, 'pool-a', 'hdd_capacity', ?2, ?3, ?4, ?4)",
                (disk_id, state, size_bytes, "2026-01-01T00:00:00Z"),
            )
            .expect("disk inserts");
    }

    fn insert_object(connection: &Connection, object_id: &str, store_id: &str, size_bytes: i64) {
        connection
            .execute(
                "INSERT INTO objects (
                    object_id, store_id, state, size_bytes, content_hash,
                    created_at_utc, updated_at_utc
                 ) VALUES (?1, ?2, 'Protected', ?3, 'sha256:test', ?4, ?4)",
                (object_id, store_id, size_bytes, "2026-01-01T00:00:00Z"),
            )
            .expect("object inserts");
    }

    fn insert_placement(
        connection: &Connection,
        placement_id: &str,
        object_id: &str,
        disk_id: &str,
    ) {
        connection
            .execute(
                "INSERT INTO placements (
                    placement_id, object_id, disk_id, relative_path, content_hash,
                    verified_at_utc, created_at_utc
                 ) VALUES (?1, ?2, ?3, 'objects/test', 'sha256:test', ?4, ?4)",
                (placement_id, object_id, disk_id, "2026-01-01T00:00:00Z"),
            )
            .expect("placement inserts");
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
