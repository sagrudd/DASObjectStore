//! Transactional metadata operations for background placement assurance.

use crate::object_commit::placement_id;
use dasobjectstore_core::ids::{DiskId, InvalidId, ObjectId, PlacementId, StoreId};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};
use std::path::Path;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AssurancePlacementCandidate {
    pub placement_id: PlacementId,
    pub object_id: ObjectId,
    pub store_id: StoreId,
    pub disk_id: DiskId,
    pub disk_state: String,
    pub relative_path: String,
    pub size_bytes: u64,
    pub content_hash: String,
    pub verified_at_utc: Option<String>,
    pub existing_disk_ids: Vec<DiskId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AssuranceDiskState {
    pub disk_id: DiskId,
    pub state: String,
}

pub fn list_assurance_disk_states(
    live_sqlite_path: impl AsRef<Path>,
) -> Result<Vec<AssuranceDiskState>, AssuranceMetadataError> {
    let connection = Connection::open_with_flags(
        live_sqlite_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let mut statement =
        connection.prepare("SELECT disk_id,state FROM disks ORDER BY disk_id ASC")?;
    let rows = statement.query_map([], |row| {
        Ok(AssuranceDiskState {
            disk_id: parse_id("disk_id", row.get(0)?)?,
            state: row.get(1)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub fn list_assurance_placement_candidates(
    live_sqlite_path: impl AsRef<Path>,
) -> Result<Vec<AssurancePlacementCandidate>, AssuranceMetadataError> {
    let connection = Connection::open_with_flags(
        live_sqlite_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let mut statement = connection.prepare(
        "SELECT
            p.placement_id,
            p.object_id,
            o.store_id,
            p.disk_id,
            d.state,
            p.relative_path,
            o.size_bytes,
            COALESCE(p.content_hash, o.content_hash),
            p.verified_at_utc
         FROM placements p
         JOIN objects o ON o.object_id=p.object_id
         JOIN disks d ON d.disk_id=p.disk_id
         WHERE o.size_bytes IS NOT NULL
           AND COALESCE(p.content_hash, o.content_hash) IS NOT NULL
         ORDER BY
           CASE LOWER(d.state)
             WHEN 'draining' THEN 0
             WHEN 'suspect' THEN 1
             ELSE 2
           END,
           p.verified_at_utc ASC,
           p.placement_id ASC",
    )?;
    let rows = statement.query_map([], |row| {
        let size_bytes = row.get::<_, i64>(6)?;
        if size_bytes < 0 {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                6,
                rusqlite::types::Type::Integer,
                Box::new(AssuranceMetadataError::NegativeSize(size_bytes)),
            ));
        }
        Ok(AssurancePlacementCandidate {
            placement_id: parse_id("placement_id", row.get(0)?)?,
            object_id: parse_id("object_id", row.get(1)?)?,
            store_id: parse_id("store_id", row.get(2)?)?,
            disk_id: parse_id("disk_id", row.get(3)?)?,
            disk_state: row.get(4)?,
            relative_path: row.get(5)?,
            size_bytes: size_bytes as u64,
            content_hash: row.get(7)?,
            verified_at_utc: row.get(8)?,
            existing_disk_ids: Vec::new(),
        })
    })?;
    let mut candidates = rows.collect::<Result<Vec<_>, _>>()?;
    for candidate in &mut candidates {
        let mut placements = connection
            .prepare("SELECT disk_id FROM placements WHERE object_id=?1 ORDER BY disk_id")?;
        let rows = placements.query_map([candidate.object_id.as_str()], |row| {
            parse_id("disk_id", row.get(0)?)
        })?;
        candidate.existing_disk_ids = rows.collect::<Result<Vec<_>, _>>()?;
    }
    Ok(candidates)
}

pub fn assurance_primary_work_pending(
    live_sqlite_path: impl AsRef<Path>,
) -> Result<bool, AssuranceMetadataError> {
    let connection = Connection::open_with_flags(
        live_sqlite_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let pending_destage: bool = connection.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM destage_queue
            WHERE state IN ('queued_for_hdd','hdd_copying','destage_failed')
        )",
        [],
        |row| row.get(0),
    )?;
    Ok(pending_destage)
}

/// Whether a durable SSD-to-HDD copy is actively consuming disk I/O. This is
/// intentionally narrower than [`assurance_primary_work_pending`]: queued or
/// retry-wait work must not hide unrelated host I/O from housekeeping.
pub fn assurance_destage_copying(
    live_sqlite_path: impl AsRef<Path>,
) -> Result<bool, AssuranceMetadataError> {
    let connection = Connection::open_with_flags(
        live_sqlite_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM destage_queue WHERE state='hdd_copying')",
            [],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

pub fn record_assurance_verification(
    live_sqlite_path: impl AsRef<Path>,
    placement_id: &PlacementId,
    expected_object_id: &ObjectId,
    verified_at_utc: &str,
) -> Result<(), AssuranceMetadataError> {
    let connection = Connection::open(live_sqlite_path)?;
    let changed = connection.execute(
        "UPDATE placements
         SET verified_at_utc=?1
         WHERE placement_id=?2 AND object_id=?3",
        params![
            verified_at_utc,
            placement_id.as_str(),
            expected_object_id.as_str()
        ],
    )?;
    if changed != 1 {
        return Err(AssuranceMetadataError::PlacementChanged(
            placement_id.clone(),
        ));
    }
    Ok(())
}

pub fn record_assurance_hash_failure(
    live_sqlite_path: impl AsRef<Path>,
    placement_id: &PlacementId,
    expected_object_id: &ObjectId,
    detected_at_utc: &str,
) -> Result<(), AssuranceMetadataError> {
    let mut connection = Connection::open(live_sqlite_path)?;
    let transaction = connection.transaction()?;
    let disk_id: Option<String> = transaction
        .query_row(
            "SELECT disk_id FROM placements
             WHERE placement_id=?1 AND object_id=?2",
            params![placement_id.as_str(), expected_object_id.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    let Some(disk_id) = disk_id else {
        return Err(AssuranceMetadataError::PlacementChanged(
            placement_id.clone(),
        ));
    };
    transaction.execute(
        "UPDATE placements SET verified_at_utc=NULL WHERE placement_id=?1",
        [placement_id.as_str()],
    )?;
    transaction.execute(
        "UPDATE disks
         SET state=CASE WHEN LOWER(state) IN ('healthy','watch') THEN 'Suspect' ELSE state END,
             updated_at_utc=?1
         WHERE disk_id=?2",
        params![detected_at_utc, disk_id],
    )?;
    transaction.execute(
        "UPDATE objects SET state='Degraded',updated_at_utc=?1 WHERE object_id=?2",
        params![detected_at_utc, expected_object_id.as_str()],
    )?;
    transaction.commit()?;
    Ok(())
}

pub fn commit_assurance_relocation(
    live_sqlite_path: impl AsRef<Path>,
    candidate: &AssurancePlacementCandidate,
    destination_disk_id: &DiskId,
    destination_relative_path: &str,
    verified_at_utc: &str,
) -> Result<PlacementId, AssuranceMetadataError> {
    if destination_disk_id == &candidate.disk_id
        || candidate
            .existing_disk_ids
            .iter()
            .any(|disk_id| disk_id == destination_disk_id)
    {
        return Err(AssuranceMetadataError::DuplicateObjectDisk(
            destination_disk_id.clone(),
        ));
    }
    let destination_placement_id = PlacementId::new(placement_id(
        candidate.object_id.as_str(),
        destination_disk_id.as_str(),
        destination_relative_path,
    ))
    .map_err(|source| AssuranceMetadataError::InvalidIdentifier {
        field: "placement_id",
        source,
    })?;
    let mut connection = Connection::open(live_sqlite_path)?;
    let transaction = connection.transaction()?;
    let source: Option<(String, String, String, Option<String>)> = transaction
        .query_row(
            "SELECT object_id,disk_id,relative_path,content_hash
             FROM placements WHERE placement_id=?1",
            [candidate.placement_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?;
    let expected = (
        candidate.object_id.as_str(),
        candidate.disk_id.as_str(),
        candidate.relative_path.as_str(),
        Some(candidate.content_hash.as_str()),
    );
    if source.as_ref().map(|row| {
        (
            row.0.as_str(),
            row.1.as_str(),
            row.2.as_str(),
            row.3.as_deref(),
        )
    }) != Some(expected)
    {
        return Err(AssuranceMetadataError::PlacementChanged(
            candidate.placement_id.clone(),
        ));
    }
    let destination_exists: bool = transaction.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM placements WHERE object_id=?1 AND disk_id=?2
        )",
        params![candidate.object_id.as_str(), destination_disk_id.as_str()],
        |row| row.get(0),
    )?;
    if destination_exists {
        return Err(AssuranceMetadataError::DuplicateObjectDisk(
            destination_disk_id.clone(),
        ));
    }
    transaction.execute(
        "INSERT INTO placements(
            placement_id,object_id,disk_id,relative_path,content_hash,
            verified_at_utc,created_at_utc
         ) VALUES(?1,?2,?3,?4,?5,?6,?6)",
        params![
            destination_placement_id.as_str(),
            candidate.object_id.as_str(),
            destination_disk_id.as_str(),
            destination_relative_path,
            candidate.content_hash,
            verified_at_utc
        ],
    )?;
    if transaction.execute(
        "DELETE FROM placements WHERE placement_id=?1",
        [candidate.placement_id.as_str()],
    )? != 1
    {
        return Err(AssuranceMetadataError::PlacementChanged(
            candidate.placement_id.clone(),
        ));
    }
    transaction.execute(
        "UPDATE objects SET updated_at_utc=?1 WHERE object_id=?2",
        params![verified_at_utc, candidate.object_id.as_str()],
    )?;
    transaction.commit()?;
    Ok(destination_placement_id)
}

pub fn assurance_relocation_committed(
    live_sqlite_path: impl AsRef<Path>,
    object_id: &ObjectId,
    source_placement_id: &PlacementId,
    destination_disk_id: &DiskId,
    destination_relative_path: &str,
    expected_hash: &str,
) -> Result<bool, AssuranceMetadataError> {
    let connection = Connection::open_with_flags(
        live_sqlite_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let source_exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM placements WHERE placement_id=?1)",
        [source_placement_id.as_str()],
        |row| row.get(0),
    )?;
    let destination_matches: bool = connection.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM placements
            WHERE object_id=?1 AND disk_id=?2 AND relative_path=?3
              AND content_hash=?4 AND verified_at_utc IS NOT NULL
        )",
        params![
            object_id.as_str(),
            destination_disk_id.as_str(),
            destination_relative_path,
            expected_hash
        ],
        |row| row.get(0),
    )?;
    Ok(!source_exists && destination_matches)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AssuranceDrainCompletion {
    pub disk_id: DiskId,
    pub placement_count: u64,
    pub previous_state: String,
    pub current_state: String,
    pub transitioned_to_retired: bool,
}

pub fn complete_assurance_drain_if_empty(
    live_sqlite_path: impl AsRef<Path>,
    disk_id: &DiskId,
    updated_at_utc: &str,
) -> Result<AssuranceDrainCompletion, AssuranceMetadataError> {
    let mut connection = Connection::open(live_sqlite_path)?;
    let transaction = connection.transaction()?;
    let previous_state: Option<String> = transaction
        .query_row(
            "SELECT state FROM disks WHERE disk_id=?1",
            [disk_id.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    let previous_state =
        previous_state.ok_or_else(|| AssuranceMetadataError::MissingDisk(disk_id.clone()))?;
    let placement_count: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM placements WHERE disk_id=?1",
        [disk_id.as_str()],
        |row| row.get(0),
    )?;
    let transitioned_to_retired =
        placement_count == 0 && previous_state.eq_ignore_ascii_case("draining");
    if transitioned_to_retired {
        transaction.execute(
            "UPDATE disks SET state='Retired',updated_at_utc=?1
             WHERE disk_id=?2 AND LOWER(state)='draining'",
            params![updated_at_utc, disk_id.as_str()],
        )?;
    }
    transaction.commit()?;
    Ok(AssuranceDrainCompletion {
        disk_id: disk_id.clone(),
        placement_count: placement_count as u64,
        previous_state: previous_state.clone(),
        current_state: if transitioned_to_retired {
            "Retired".to_string()
        } else {
            previous_state
        },
        transitioned_to_retired,
    })
}

fn parse_id<T>(field: &'static str, value: String) -> Result<T, rusqlite::Error>
where
    T: std::str::FromStr<Err = InvalidId>,
{
    value.parse().map_err(|source| {
        rusqlite::Error::ToSqlConversionFailure(Box::new(
            AssuranceMetadataError::InvalidIdentifier { field, source },
        ))
    })
}

#[derive(Debug)]
pub enum AssuranceMetadataError {
    Sqlite(rusqlite::Error),
    InvalidIdentifier {
        field: &'static str,
        source: InvalidId,
    },
    NegativeSize(i64),
    PlacementChanged(PlacementId),
    DuplicateObjectDisk(DiskId),
    MissingDisk(DiskId),
}

impl Display for AssuranceMetadataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => write!(formatter, "assurance metadata failed: {error}"),
            Self::InvalidIdentifier { field, source } => {
                write!(formatter, "invalid assurance {field}: {source}")
            }
            Self::NegativeSize(size) => write!(formatter, "negative assurance object size {size}"),
            Self::PlacementChanged(placement_id) => write!(
                formatter,
                "placement {placement_id} changed while assurance work was in progress"
            ),
            Self::DuplicateObjectDisk(disk_id) => write!(
                formatter,
                "object already has a placement on assurance destination {disk_id}"
            ),
            Self::MissingDisk(disk_id) => {
                write!(formatter, "assurance disk {disk_id} is missing")
            }
        }
    }
}

impl std::error::Error for AssuranceMetadataError {}

impl From<rusqlite::Error> for AssuranceMetadataError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LIVE_SCHEMA_SQL;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn relocation_atomically_replaces_only_the_selected_placement() {
        let path = fixture("relocate");
        let candidate = list_assurance_placement_candidates(&path)
            .expect("candidates")
            .remove(0);
        let retained_source_disk = if candidate.disk_id.as_str() == "disk-a" {
            "disk-b"
        } else {
            "disk-a"
        };
        let destination = DiskId::new("disk-c").expect("destination");

        let placement_id = commit_assurance_relocation(
            &path,
            &candidate,
            &destination,
            "objects/aa/object-a/payload",
            "2026-02-01T00:00:00Z",
        )
        .expect("relocation");

        let connection = Connection::open(&path).expect("open");
        let rows = connection
            .prepare(
                "SELECT placement_id,disk_id FROM placements
                 WHERE object_id='object-a' ORDER BY disk_id",
            )
            .expect("statement")
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .expect("query")
            .collect::<Result<Vec<_>, _>>()
            .expect("rows");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].1, retained_source_disk);
        assert_eq!(rows[1], (placement_id.to_string(), "disk-c".to_string()));
        assert!(!rows
            .iter()
            .any(|(_, disk)| disk == candidate.disk_id.as_str()));
        cleanup(&path);
    }

    #[test]
    fn relocation_fails_closed_when_destination_already_has_an_object_copy() {
        let path = fixture("duplicate");
        let candidate = list_assurance_placement_candidates(&path)
            .expect("candidates")
            .remove(0);
        let error = commit_assurance_relocation(
            &path,
            &candidate,
            &DiskId::new("disk-b").expect("disk"),
            "objects/aa/object-a/payload",
            "2026-02-01T00:00:00Z",
        )
        .expect_err("duplicate rejected");
        assert!(matches!(
            error,
            AssuranceMetadataError::DuplicateObjectDisk(_)
        ));
        cleanup(&path);
    }

    #[test]
    fn verification_updates_only_unchanged_placement_identity() {
        let path = fixture("verify");
        let candidate = list_assurance_placement_candidates(&path)
            .expect("candidates")
            .remove(0);
        record_assurance_verification(
            &path,
            &candidate.placement_id,
            &candidate.object_id,
            "2026-03-01T00:00:00Z",
        )
        .expect("verification");
        let verified: String = Connection::open(&path)
            .expect("open")
            .query_row(
                "SELECT verified_at_utc FROM placements WHERE placement_id=?1",
                [candidate.placement_id.as_str()],
                |row| row.get(0),
            )
            .expect("verified timestamp");
        assert_eq!(verified, "2026-03-01T00:00:00Z");
        cleanup(&path);
    }

    #[test]
    fn hash_failure_withdraws_verification_and_marks_object_and_disk_unsafe() {
        let path = fixture("hash-failure");
        let candidate = list_assurance_placement_candidates(&path)
            .expect("candidates")
            .remove(0);
        record_assurance_hash_failure(
            &path,
            &candidate.placement_id,
            &candidate.object_id,
            "2026-03-01T00:00:00Z",
        )
        .expect("record failure");
        let connection = Connection::open(&path).expect("open");
        let placement_verified: Option<String> = connection
            .query_row(
                "SELECT verified_at_utc FROM placements WHERE placement_id=?1",
                [candidate.placement_id.as_str()],
                |row| row.get(0),
            )
            .expect("placement");
        let disk_state: String = connection
            .query_row(
                "SELECT state FROM disks WHERE disk_id=?1",
                [candidate.disk_id.as_str()],
                |row| row.get(0),
            )
            .expect("disk");
        let object_state: String = connection
            .query_row(
                "SELECT state FROM objects WHERE object_id=?1",
                [candidate.object_id.as_str()],
                |row| row.get(0),
            )
            .expect("object");
        assert_eq!(placement_verified, None);
        assert_eq!(disk_state, "Suspect");
        assert_eq!(object_state, "Degraded");
        cleanup(&path);
    }

    #[test]
    fn drain_completion_retires_only_an_empty_draining_disk() {
        let path = fixture("drain-completion");
        let connection = Connection::open(&path).expect("open");
        connection
            .execute(
                "UPDATE disks SET state='Draining' WHERE disk_id='disk-a'",
                [],
            )
            .expect("draining");
        drop(connection);
        let disk = DiskId::new("disk-a").expect("disk");
        let blocked = complete_assurance_drain_if_empty(&path, &disk, "2026-03-01T00:00:00Z")
            .expect("blocked completion");
        assert!(!blocked.transitioned_to_retired);
        assert!(blocked.placement_count > 0);
        let connection = Connection::open(&path).expect("open");
        connection
            .execute("DELETE FROM placements WHERE disk_id='disk-a'", [])
            .expect("evacuated");
        drop(connection);
        let completed = complete_assurance_drain_if_empty(&path, &disk, "2026-03-01T00:01:00Z")
            .expect("completed");
        assert!(completed.transitioned_to_retired);
        assert_eq!(completed.current_state, "Retired");
        cleanup(&path);
    }

    #[test]
    fn relocation_commit_evidence_requires_destination_and_absent_source() {
        let path = fixture("relocation-evidence");
        let candidate = list_assurance_placement_candidates(&path)
            .expect("candidates")
            .remove(0);
        let destination = DiskId::new("disk-c").expect("destination");
        assert!(!assurance_relocation_committed(
            &path,
            &candidate.object_id,
            &candidate.placement_id,
            &destination,
            &candidate.relative_path,
            &candidate.content_hash,
        )
        .expect("precondition"));
        commit_assurance_relocation(
            &path,
            &candidate,
            &destination,
            &candidate.relative_path,
            "2026-03-01T00:00:00Z",
        )
        .expect("commit");
        assert!(assurance_relocation_committed(
            &path,
            &candidate.object_id,
            &candidate.placement_id,
            &destination,
            &candidate.relative_path,
            &candidate.content_hash,
        )
        .expect("committed"));
        cleanup(&path);
    }

    fn fixture(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-assurance-{label}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("root");
        let path = root.join("live.sqlite");
        let connection = Connection::open(&path).expect("open");
        connection.execute_batch(LIVE_SCHEMA_SQL).expect("schema");
        connection
            .execute(
                "INSERT INTO pools(pool_id,state,created_at_utc,updated_at_utc)
             VALUES('pool-a','Clean','now','now')",
                [],
            )
            .expect("pool");
        for disk in ["disk-a", "disk-b", "disk-c"] {
            connection
                .execute(
                    "INSERT INTO disks(
                    disk_id,pool_id,role,state,created_at_utc,updated_at_utc
                 ) VALUES(?1,'pool-a','hdd_capacity','Watch','now','now')",
                    [disk],
                )
                .expect("disk");
        }
        connection
            .execute(
                "INSERT INTO stores(
                store_id,pool_id,class,policy_json,created_at_utc,updated_at_utc
             ) VALUES('store-a','pool-a','generated_data','{}','now','now')",
                [],
            )
            .expect("store");
        connection
            .execute(
                "INSERT INTO objects(
                object_id,store_id,object_type,state,size_bytes,content_hash,
                created_at_utc,updated_at_utc
             ) VALUES(
                'object-a','store-a','naive','HddCopyVerified',128,
                'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                'now','now'
            )",
                [],
            )
            .expect("object");
        for disk in ["disk-a", "disk-b"] {
            let placement = placement_id("object-a", disk, "objects/aa/object-a/payload");
            connection
                .execute(
                    "INSERT INTO placements VALUES(
                    ?1,'object-a',?2,'objects/aa/object-a/payload',
                    'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
                    '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z'
                )",
                    params![placement, disk],
                )
                .expect("placement");
        }
        path
    }

    fn cleanup(path: &Path) {
        if let Some(root) = path.parent() {
            fs::remove_dir_all(root).expect("cleanup");
        }
    }
}
