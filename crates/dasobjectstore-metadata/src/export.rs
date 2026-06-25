use crate::copy::{verify_hdd_copy_hash, HddCopyError};
use crate::evacuation::DiskCopyRoot;
use crate::object::{read_object_inspect, ObjectInspectError, ObjectPlacementSummary};
use dasobjectstore_core::ids::{DiskId, ObjectId};
use serde::Serialize;
use std::fmt::{self, Display};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectExportRequest {
    pub live_sqlite_path: PathBuf,
    pub object_id: ObjectId,
    pub destination_path: PathBuf,
    pub disk_roots: Vec<DiskCopyRoot>,
}

impl ObjectExportRequest {
    pub fn new(
        live_sqlite_path: impl Into<PathBuf>,
        object_id: ObjectId,
        destination_path: impl Into<PathBuf>,
        disk_roots: Vec<DiskCopyRoot>,
    ) -> Self {
        Self {
            live_sqlite_path: live_sqlite_path.into(),
            object_id,
            destination_path: destination_path.into(),
            disk_roots,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ObjectExportReport {
    pub object_id: ObjectId,
    pub source_disk_id: DiskId,
    pub source_path: PathBuf,
    pub destination_path: PathBuf,
    pub bytes_written: u64,
    pub content_hash: String,
}

pub fn export_settled_object(
    request: &ObjectExportRequest,
) -> Result<ObjectExportReport, ObjectExportError> {
    let summary = read_object_inspect(&request.live_sqlite_path, &request.object_id)?;
    let first_verified = first_verified_placement(&summary.placements).ok_or_else(|| {
        ObjectExportError::NoVerifiedPlacement {
            object_id: request.object_id.clone(),
        }
    })?;
    let (placement, disk_root) =
        verified_placement_with_root(&summary.placements, &request.disk_roots).ok_or_else(
            || ObjectExportError::MissingDiskRoot {
                disk_id: first_verified.disk_id.clone(),
            },
        )?;
    let expected_hash = placement
        .content_hash
        .as_deref()
        .or(summary.content_hash.as_deref())
        .ok_or_else(|| ObjectExportError::MissingContentHash {
            object_id: request.object_id.clone(),
            disk_id: placement.disk_id.clone(),
        })?;
    let source_path = disk_root.join(&placement.relative_path);
    if let Some(parent) = request.destination_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes_written = fs::copy(&source_path, &request.destination_path)?;
    let content_hash = verify_hdd_copy_hash(&request.destination_path, expected_hash)?;

    Ok(ObjectExportReport {
        object_id: request.object_id.clone(),
        source_disk_id: placement.disk_id.clone(),
        source_path,
        destination_path: request.destination_path.clone(),
        bytes_written,
        content_hash,
    })
}

fn first_verified_placement(
    placements: &[ObjectPlacementSummary],
) -> Option<&ObjectPlacementSummary> {
    placements
        .iter()
        .find(|placement| placement.verified_at_utc.is_some())
}

fn verified_placement_with_root<'a>(
    placements: &'a [ObjectPlacementSummary],
    disk_roots: &'a [DiskCopyRoot],
) -> Option<(&'a ObjectPlacementSummary, &'a Path)> {
    placements
        .iter()
        .filter(|placement| placement.verified_at_utc.is_some())
        .find_map(|placement| {
            disk_root_for(disk_roots, &placement.disk_id).map(|disk_root| (placement, disk_root))
        })
}

fn disk_root_for<'a>(disk_roots: &'a [DiskCopyRoot], disk_id: &DiskId) -> Option<&'a Path> {
    disk_roots
        .iter()
        .find(|root| root.disk_id == *disk_id)
        .map(|root| root.root_path.as_path())
}

#[derive(Debug)]
pub enum ObjectExportError {
    Inspect(ObjectInspectError),
    Io(std::io::Error),
    Hash(HddCopyError),
    MissingDiskRoot {
        disk_id: DiskId,
    },
    MissingContentHash {
        object_id: ObjectId,
        disk_id: DiskId,
    },
    NoVerifiedPlacement {
        object_id: ObjectId,
    },
}

impl Display for ObjectExportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inspect(err) => write!(formatter, "{err}"),
            Self::Io(err) => write!(formatter, "object export IO failed: {err}"),
            Self::Hash(err) => write!(formatter, "{err}"),
            Self::MissingDiskRoot { disk_id } => {
                write!(formatter, "object export needs a root path for disk {disk_id}")
            }
            Self::MissingContentHash { object_id, disk_id } => write!(
                formatter,
                "object {object_id} placement on disk {disk_id} has no content hash to verify export"
            ),
            Self::NoVerifiedPlacement { object_id } => {
                write!(formatter, "object {object_id} has no verified settled placement to export")
            }
        }
    }
}

impl std::error::Error for ObjectExportError {}

impl From<ObjectInspectError> for ObjectExportError {
    fn from(err: ObjectInspectError) -> Self {
        Self::Inspect(err)
    }
}

impl From<std::io::Error> for ObjectExportError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<HddCopyError> for ObjectExportError {
    fn from(err: HddCopyError) -> Self {
        Self::Hash(err)
    }
}

#[cfg(test)]
mod tests {
    use super::{export_settled_object, ObjectExportError, ObjectExportRequest};
    use crate::evacuation::DiskCopyRoot;
    use crate::hash::hash_file_sha256;
    use crate::LIVE_SCHEMA_SQL;
    use dasobjectstore_core::ids::{DiskId, ObjectId};
    use rusqlite::{params, Connection};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn exports_verified_settled_object_from_disk_root() {
        let root = temp_root("object-export");
        let live_sqlite_path = root.join("live.sqlite");
        let disk_root = root.join("disk-a");
        let source_path = disk_root.join("objects").join("aa").join("object-a");
        let destination_path = root.join("exports").join("object-a");
        fs::create_dir_all(source_path.parent().expect("source parent")).expect("source parent");
        fs::write(&source_path, b"settled payload").expect("write payload");
        let content_hash = hash_file_sha256(&source_path).expect("hash payload");
        let connection = fixture_connection(&live_sqlite_path);
        insert_object_fixture(&connection, &content_hash);

        let report = export_settled_object(&ObjectExportRequest::new(
            &live_sqlite_path,
            object_id(),
            &destination_path,
            vec![DiskCopyRoot::new(disk_id(), &disk_root)],
        ))
        .expect("object exports");

        assert_eq!(report.object_id.as_str(), "object-a");
        assert_eq!(report.source_disk_id.as_str(), "disk-a");
        assert_eq!(report.source_path, source_path);
        assert_eq!(report.destination_path, destination_path);
        assert_eq!(report.bytes_written, b"settled payload".len() as u64);
        assert_eq!(report.content_hash, content_hash);
        assert_eq!(
            fs::read(report.destination_path).expect("read export"),
            b"settled payload"
        );

        fs::remove_dir_all(root).expect("cleanup root");
    }

    #[test]
    fn rejects_object_without_verified_placement() {
        let root = temp_root("object-export-unverified");
        fs::create_dir_all(&root).expect("create root");
        let live_sqlite_path = root.join("live.sqlite");
        let connection = fixture_connection(&live_sqlite_path);
        insert_unverified_object_fixture(&connection);

        let err = export_settled_object(&ObjectExportRequest::new(
            &live_sqlite_path,
            object_id(),
            root.join("exports").join("object-a"),
            vec![DiskCopyRoot::new(disk_id(), root.join("disk-a"))],
        ))
        .expect_err("unverified object is rejected");

        assert!(matches!(err, ObjectExportError::NoVerifiedPlacement { .. }));

        fs::remove_dir_all(root).expect("cleanup root");
    }

    fn fixture_connection(path: &PathBuf) -> Connection {
        let connection = Connection::open(path).expect("open sqlite");
        connection
            .execute_batch(LIVE_SCHEMA_SQL)
            .expect("schema applies");
        connection
    }

    fn insert_object_fixture(connection: &Connection, content_hash: &str) {
        insert_base_fixture(connection, content_hash);
        connection
            .execute(
                "INSERT INTO placements (
                    placement_id, object_id, disk_id, relative_path, content_hash,
                    verified_at_utc, created_at_utc
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    "placement-a",
                    "object-a",
                    "disk-a",
                    "objects/aa/object-a",
                    content_hash,
                    "2026-01-03T00:00:00Z",
                    "2026-01-02T00:00:00Z"
                ],
            )
            .expect("placement inserts");
    }

    fn insert_unverified_object_fixture(connection: &Connection) {
        insert_base_fixture(connection, "not-used");
        connection
            .execute(
                "INSERT INTO placements (
                    placement_id, object_id, disk_id, relative_path, content_hash,
                    verified_at_utc, created_at_utc
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    "placement-a",
                    "object-a",
                    "disk-a",
                    "objects/aa/object-a",
                    "not-used",
                    Option::<String>::None,
                    "2026-01-02T00:00:00Z"
                ],
            )
            .expect("placement inserts");
    }

    fn insert_base_fixture(connection: &Connection, content_hash: &str) {
        connection
            .execute_batch(
                "INSERT INTO pools (pool_id, state, created_at_utc, updated_at_utc)
                 VALUES ('pool-a', 'ReadOnly', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
                 INSERT INTO stores (
                    store_id, pool_id, class, policy_json, created_at_utc, updated_at_utc
                 ) VALUES (
                    'store-a', 'pool-a', 'generated_data', '{}',
                    '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'
                 );
                 INSERT INTO disks (
                    disk_id, pool_id, role, state, created_at_utc, updated_at_utc
                 ) VALUES (
                    'disk-a', 'pool-a', 'hdd_capacity', 'Healthy',
                    '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'
                 );",
            )
            .expect("base fixture inserts");
        connection
            .execute(
                "INSERT INTO objects (
                    object_id, store_id, state, size_bytes, content_hash,
                    created_at_utc, updated_at_utc
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    "object-a",
                    "store-a",
                    "SsdEvictionEligible",
                    15_i64,
                    content_hash,
                    "2026-01-02T00:00:00Z",
                    "2026-01-03T00:00:00Z"
                ],
            )
            .expect("object inserts");
    }

    fn object_id() -> ObjectId {
        ObjectId::new("object-a").expect("object id")
    }

    fn disk_id() -> DiskId {
        DiskId::new("disk-a").expect("disk id")
    }

    fn temp_root(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dasobjectstore-{name}-{}-{nanos}",
            std::process::id()
        ))
    }
}
