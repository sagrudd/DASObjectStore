use crate::api::{ObjectDownloadRequest, ObjectDownloadResponse};
use crate::runtime::{discover_managed_hdd_roots, DaemonIngestFilesRuntimeError};
use dasobjectstore_core::ids::{DiskId, ObjectId, StoreId};
use dasobjectstore_metadata::{read_object_inspect, DiskCopyRoot, ObjectInspectError};
use std::fmt::{self, Display};
use std::fs;
use std::path::{Component, Path, PathBuf};

pub(crate) fn resolve_object_download_with_hdd_root(
    live_sqlite_path: &Path,
    hdd_root: &Path,
    store_id: &StoreId,
    request: &ObjectDownloadRequest,
) -> Result<ObjectDownloadResponse, ObjectDownloadResolveError> {
    let summary = read_object_inspect(live_sqlite_path, &request.object_id)?;
    if summary.store_id != *store_id {
        return Err(ObjectDownloadResolveError::ObjectNotInStore {
            object_id: request.object_id.clone(),
            store_id: store_id.clone(),
        });
    }

    let disk_roots = discover_managed_hdd_roots(hdd_root)?;
    let (disk_id, source_path) = verified_source_path(&summary.placements, &disk_roots)
        .ok_or_else(|| ObjectDownloadResolveError::NoVerifiedManagedPlacement {
            object_id: request.object_id.clone(),
        })?;
    let metadata = fs::metadata(&source_path)?;
    if !metadata.is_file() {
        return Err(ObjectDownloadResolveError::SourceNotFile { path: source_path });
    }

    Ok(ObjectDownloadResponse {
        endpoint: request.endpoint.clone(),
        store_id: store_id.clone(),
        object_id: request.object_id.clone(),
        file_name: download_file_name(&request.object_id),
        source_disk_id: disk_id,
        source_path,
        size_bytes: metadata.len(),
    })
}

fn verified_source_path(
    placements: &[dasobjectstore_metadata::ObjectPlacementSummary],
    disk_roots: &[DiskCopyRoot],
) -> Option<(DiskId, PathBuf)> {
    placements
        .iter()
        .filter(|placement| placement.verified_at_utc.is_some())
        .filter_map(|placement| {
            safe_relative_path(&placement.relative_path).and_then(|relative_path| {
                disk_roots
                    .iter()
                    .find(|root| root.disk_id == placement.disk_id)
                    .map(|root| {
                        (
                            placement.disk_id.clone(),
                            root.root_path.join(relative_path),
                        )
                    })
            })
        })
        .next()
}

fn safe_relative_path(value: &str) -> Option<PathBuf> {
    let path = Path::new(value);
    if path.is_absolute() {
        return None;
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return None;
    }
    Some(path.to_path_buf())
}

fn download_file_name(object_id: &ObjectId) -> String {
    object_id
        .as_str()
        .rsplit('/')
        .find(|value| !value.is_empty())
        .unwrap_or(object_id.as_str())
        .to_string()
}

#[derive(Debug)]
pub(crate) enum ObjectDownloadResolveError {
    Inspect(ObjectInspectError),
    HddDiscovery(DaemonIngestFilesRuntimeError),
    Io(std::io::Error),
    ObjectNotInStore {
        object_id: ObjectId,
        store_id: StoreId,
    },
    NoVerifiedManagedPlacement {
        object_id: ObjectId,
    },
    SourceNotFile {
        path: PathBuf,
    },
}

impl ObjectDownloadResolveError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::Inspect(ObjectInspectError::ObjectNotFound(_))
            | Self::ObjectNotInStore { .. } => "object_download_not_found",
            Self::NoVerifiedManagedPlacement { .. } | Self::SourceNotFile { .. } => {
                "object_download_unavailable"
            }
            Self::HddDiscovery(_) | Self::Io(_) | Self::Inspect(_) => "object_download_failed",
        }
    }
}

impl Display for ObjectDownloadResolveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inspect(error) => Display::fmt(error, formatter),
            Self::HddDiscovery(error) => Display::fmt(error, formatter),
            Self::Io(error) => write!(formatter, "object download IO failed: {error}"),
            Self::ObjectNotInStore {
                object_id,
                store_id,
            } => write!(
                formatter,
                "object `{object_id}` was not found in ObjectStore {store_id}"
            ),
            Self::NoVerifiedManagedPlacement { object_id } => write!(
                formatter,
                "object `{object_id}` has no verified placement on a managed HDD root"
            ),
            Self::SourceNotFile { path } => {
                write!(
                    formatter,
                    "object download source is not a file: {}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for ObjectDownloadResolveError {}

impl From<ObjectInspectError> for ObjectDownloadResolveError {
    fn from(error: ObjectInspectError) -> Self {
        Self::Inspect(error)
    }
}

impl From<DaemonIngestFilesRuntimeError> for ObjectDownloadResolveError {
    fn from(error: DaemonIngestFilesRuntimeError) -> Self {
        Self::HddDiscovery(error)
    }
}

impl From<std::io::Error> for ObjectDownloadResolveError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_object_download_with_hdd_root;
    use crate::api::ObjectDownloadRequest;
    use crate::runtime::object_download::ObjectDownloadResolveError;
    use dasobjectstore_core::ids::{ObjectId, StoreId};
    use dasobjectstore_metadata::LIVE_SCHEMA_SQL;
    use rusqlite::{params, Connection};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn resolves_verified_object_from_managed_hdd_root() {
        let root = temp_root("download-resolve");
        let live_sqlite_path = root.join("live.sqlite");
        let hdd_root = root.join("hdd");
        let disk_root = hdd_root.join("disk-a");
        write_hdd_marker(&disk_root, "disk-a");
        let source_path = disk_root
            .join("objects")
            .join("aa")
            .join("object-a")
            .join("payload");
        fs::create_dir_all(source_path.parent().expect("source parent")).expect("source parent");
        fs::write(&source_path, b"download payload").expect("write source");
        let connection = fixture_connection(&live_sqlite_path);
        insert_object_fixture(&connection, "ena/raw/metadata.tsv", "sha256:payload");

        let response = resolve_object_download_with_hdd_root(
            &live_sqlite_path,
            &hdd_root,
            &store_id(),
            &ObjectDownloadRequest {
                endpoint: store_id(),
                object_id: object_id("ena/raw/metadata.tsv"),
            },
        )
        .expect("download resolves");

        assert_eq!(response.file_name, "metadata.tsv");
        assert_eq!(response.source_disk_id.as_str(), "disk-a");
        assert_eq!(response.source_path, source_path);
        assert_eq!(response.size_bytes, b"download payload".len() as u64);

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn rejects_object_without_verified_managed_hdd_placement() {
        let root = temp_root("download-unverified");
        let live_sqlite_path = root.join("live.sqlite");
        let hdd_root = root.join("hdd");
        fs::create_dir_all(&hdd_root).expect("hdd root");
        let connection = fixture_connection(&live_sqlite_path);
        insert_unverified_object_fixture(&connection, "ena/raw/metadata.tsv");

        let err = resolve_object_download_with_hdd_root(
            &live_sqlite_path,
            &hdd_root,
            &store_id(),
            &ObjectDownloadRequest {
                endpoint: store_id(),
                object_id: object_id("ena/raw/metadata.tsv"),
            },
        )
        .expect_err("unverified object is rejected");

        assert!(matches!(
            err,
            ObjectDownloadResolveError::NoVerifiedManagedPlacement { .. }
        ));

        fs::remove_dir_all(root).expect("cleanup");
    }

    fn fixture_connection(path: &std::path::Path) -> Connection {
        let connection = Connection::open(path).expect("open sqlite");
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
                    "2026-07-09T10:18:21Z",
                    "2026-07-09T10:18:21Z",
                ],
            )
            .expect("insert pool");
        connection
            .execute(
                "INSERT INTO stores
                    (store_id, pool_id, class, policy_json, created_at_utc, updated_at_utc)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    "ena",
                    "pool-a",
                    "generated_data",
                    "{}",
                    "2026-07-09T10:18:21Z",
                    "2026-07-09T10:18:21Z",
                ],
            )
            .expect("insert store");
        connection
            .execute(
                "INSERT INTO disks
                    (disk_id, pool_id, role, state, size_bytes, serial_hint, model_hint,
                     enclosure_topology_path, created_at_utc, updated_at_utc)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, NULL, ?6, ?7)",
                params![
                    "disk-a",
                    "pool-a",
                    "hdd",
                    "Healthy",
                    4_000_000_000_i64,
                    "2026-07-09T10:18:21Z",
                    "2026-07-09T10:18:21Z",
                ],
            )
            .expect("insert disk");
        connection
    }

    fn insert_object_fixture(connection: &Connection, object_id: &str, content_hash: &str) {
        connection
            .execute(
                "INSERT INTO objects
                    (object_id, store_id, object_type, state, size_bytes, content_hash, created_at_utc, updated_at_utc)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    object_id,
                    "ena",
                    "Naive",
                    "Protected",
                    16_i64,
                    content_hash,
                    "2026-07-09T10:18:21Z",
                    "2026-07-09T10:18:21Z",
                ],
            )
            .expect("insert object");
        connection
            .execute(
                "INSERT INTO placements
                    (placement_id, object_id, disk_id, relative_path, content_hash, verified_at_utc, created_at_utc)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    "placement-a",
                    object_id,
                    "disk-a",
                    "objects/aa/object-a/payload",
                    content_hash,
                    "2026-07-09T10:18:22Z",
                    "2026-07-09T10:18:21Z",
                ],
            )
            .expect("insert placement");
    }

    fn insert_unverified_object_fixture(connection: &Connection, object_id: &str) {
        connection
            .execute(
                "INSERT INTO objects
                    (object_id, store_id, object_type, state, size_bytes, content_hash, created_at_utc, updated_at_utc)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    object_id,
                    "ena",
                    "Naive",
                    "Protected",
                    16_i64,
                    "hash",
                    "2026-07-09T10:18:21Z",
                    "2026-07-09T10:18:21Z",
                ],
            )
            .expect("insert object");
    }

    fn write_hdd_marker(root: &std::path::Path, disk_id: &str) {
        let marker_dir = root.join(".dasobjectstore");
        fs::create_dir_all(&marker_dir).expect("marker dir");
        fs::write(
            marker_dir.join("device.env"),
            format!("role=hdd:{disk_id}\n"),
        )
        .expect("marker");
    }

    fn store_id() -> StoreId {
        StoreId::new("ena").expect("store id")
    }

    fn object_id(value: &str) -> ObjectId {
        ObjectId::new(value).expect("object id")
    }

    fn temp_root(name: &str) -> std::path::PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!("dasobjectstore-daemon-{name}-{suffix}"))
    }
}
