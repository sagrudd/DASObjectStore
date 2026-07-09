use crate::api::{
    ObjectDownloadRequest, ObjectDownloadResponse, ObjectFolderArchiveEntry,
    ObjectFolderDownloadRequest, ObjectFolderDownloadResponse,
};
use crate::runtime::object_browser::{
    read_object_browser_metadata, ObjectBrowserMetadataReadError,
};
use crate::runtime::{discover_managed_hdd_roots, DaemonIngestFilesRuntimeError};
use dasobjectstore_core::ids::{DiskId, ObjectId, StoreId};
use dasobjectstore_metadata::{read_object_inspect, DiskCopyRoot, ObjectInspectError};
use std::collections::BTreeSet;
use std::fmt::{self, Display};
use std::fs;
use std::path::{Component, Path, PathBuf};

pub(crate) fn resolve_object_download_with_hdd_root(
    live_sqlite_path: &Path,
    hdd_root: &Path,
    store_id: &StoreId,
    request: &ObjectDownloadRequest,
) -> Result<ObjectDownloadResponse, ObjectDownloadResolveError> {
    resolve_one_object_download_with_roots(
        live_sqlite_path,
        &discover_managed_hdd_roots(hdd_root)?,
        store_id,
        request,
    )
}

pub(crate) fn resolve_object_folder_download_with_hdd_root(
    live_sqlite_path: &Path,
    hdd_root: &Path,
    store_id: &StoreId,
    request: &ObjectFolderDownloadRequest,
) -> Result<ObjectFolderDownloadResponse, ObjectFolderDownloadResolveError> {
    let prefix = normalize_folder_prefix(&request.prefix, store_id)?;
    let folder_prefix = format!("{prefix}/");
    let disk_roots = discover_managed_hdd_roots(hdd_root)?;
    let mut seen_archive_paths = BTreeSet::new();
    let mut total_source_bytes = 0_u64;
    let mut entries = Vec::new();

    for metadata in read_object_browser_metadata(live_sqlite_path, store_id.clone())? {
        let object_path = normalize_object_path(&metadata.path);
        let Some(archive_path) = object_path.strip_prefix(&folder_prefix) else {
            continue;
        };
        if archive_path.is_empty() {
            continue;
        }
        safe_archive_path(archive_path)?;
        if !seen_archive_paths.insert(archive_path.to_string()) {
            return Err(ObjectFolderDownloadResolveError::DuplicateArchivePath {
                archive_path: archive_path.to_string(),
            });
        }

        let download = resolve_one_object_download_with_roots(
            live_sqlite_path,
            &disk_roots,
            store_id,
            &ObjectDownloadRequest {
                endpoint: request.endpoint.clone(),
                object_id: metadata.object_id,
            },
        )?;
        total_source_bytes = total_source_bytes.saturating_add(download.size_bytes);
        entries.push(ObjectFolderArchiveEntry {
            object_id: download.object_id,
            archive_path: archive_path.to_string(),
            source_disk_id: download.source_disk_id,
            source_path: download.source_path,
            size_bytes: download.size_bytes,
        });
    }

    if entries.is_empty() {
        return Err(ObjectFolderDownloadResolveError::NoObjects {
            prefix: prefix.clone(),
            store_id: store_id.clone(),
        });
    }

    Ok(ObjectFolderDownloadResponse {
        endpoint: request.endpoint.clone(),
        store_id: store_id.clone(),
        prefix: prefix.clone(),
        archive_name: folder_archive_name(&prefix),
        total_files: entries.len() as u64,
        total_source_bytes,
        entries,
    })
}

fn resolve_one_object_download_with_roots(
    live_sqlite_path: &Path,
    disk_roots: &[DiskCopyRoot],
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

    let (disk_id, source_path) =
        verified_source_path(&summary.placements, disk_roots).ok_or_else(|| {
            ObjectDownloadResolveError::NoVerifiedManagedPlacement {
                object_id: request.object_id.clone(),
            }
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

fn normalize_folder_prefix(
    prefix: &str,
    store_id: &StoreId,
) -> Result<String, ObjectFolderDownloadResolveError> {
    let mut prefix = normalize_object_path(prefix);
    let store_prefix = format!("{}/", store_id.as_str());
    if let Some(relative_prefix) = prefix.strip_prefix(&store_prefix) {
        prefix = relative_prefix.to_string();
    }
    if prefix.is_empty() {
        return Err(ObjectFolderDownloadResolveError::BlankPrefix);
    }
    safe_archive_path(&prefix)?;
    Ok(prefix)
}

fn normalize_object_path(path: &str) -> String {
    path.trim()
        .trim_start_matches('/')
        .trim_end_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

fn safe_archive_path(value: &str) -> Result<(), ObjectFolderDownloadResolveError> {
    let path = Path::new(value);
    if path.is_absolute() {
        return Err(ObjectFolderDownloadResolveError::UnsafeArchivePath {
            archive_path: value.to_string(),
        });
    }
    if value.split('/').any(|segment| segment == "..") {
        return Err(ObjectFolderDownloadResolveError::UnsafeArchivePath {
            archive_path: value.to_string(),
        });
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(ObjectFolderDownloadResolveError::UnsafeArchivePath {
            archive_path: value.to_string(),
        });
    }
    Ok(())
}

fn folder_archive_name(prefix: &str) -> String {
    let leaf = prefix
        .rsplit('/')
        .find(|segment| !segment.trim().is_empty())
        .unwrap_or("folder");
    let safe_leaf = leaf
        .chars()
        .filter_map(|character| match character {
            '"' | '\'' | '\\' | '/' | '\r' | '\n' => Some('_'),
            character if character.is_control() => None,
            character => Some(character),
        })
        .collect::<String>();
    let safe_leaf = if safe_leaf.trim().is_empty() {
        "folder"
    } else {
        safe_leaf.trim()
    };
    format!("{safe_leaf}.tar.gz")
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

#[derive(Debug)]
pub(crate) enum ObjectFolderDownloadResolveError {
    Metadata(ObjectBrowserMetadataReadError),
    Object(ObjectDownloadResolveError),
    BlankPrefix,
    NoObjects { prefix: String, store_id: StoreId },
    UnsafeArchivePath { archive_path: String },
    DuplicateArchivePath { archive_path: String },
}

impl ObjectFolderDownloadResolveError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::NoObjects { .. } => "object_folder_download_not_found",
            Self::Object(ObjectDownloadResolveError::NoVerifiedManagedPlacement { .. })
            | Self::Object(ObjectDownloadResolveError::SourceNotFile { .. }) => {
                "object_folder_download_unavailable"
            }
            Self::Object(error) => error.code(),
            Self::Metadata(_)
            | Self::BlankPrefix
            | Self::UnsafeArchivePath { .. }
            | Self::DuplicateArchivePath { .. } => "object_folder_download_failed",
        }
    }
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

impl Display for ObjectFolderDownloadResolveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Metadata(error) => Display::fmt(error, formatter),
            Self::Object(error) => Display::fmt(error, formatter),
            Self::BlankPrefix => write!(formatter, "folder download prefix must not be blank"),
            Self::NoObjects { prefix, store_id } => {
                write!(
                    formatter,
                    "ObjectStore {store_id} has no downloadable objects under `{prefix}`"
                )
            }
            Self::UnsafeArchivePath { archive_path } => {
                write!(
                    formatter,
                    "folder download archive path is unsafe: {archive_path}"
                )
            }
            Self::DuplicateArchivePath { archive_path } => {
                write!(
                    formatter,
                    "folder download archive path is duplicated: {archive_path}"
                )
            }
        }
    }
}

impl std::error::Error for ObjectFolderDownloadResolveError {}

impl From<ObjectBrowserMetadataReadError> for ObjectFolderDownloadResolveError {
    fn from(error: ObjectBrowserMetadataReadError) -> Self {
        Self::Metadata(error)
    }
}

impl From<ObjectDownloadResolveError> for ObjectFolderDownloadResolveError {
    fn from(error: ObjectDownloadResolveError) -> Self {
        Self::Object(error)
    }
}

impl From<DaemonIngestFilesRuntimeError> for ObjectFolderDownloadResolveError {
    fn from(error: DaemonIngestFilesRuntimeError) -> Self {
        Self::Object(ObjectDownloadResolveError::HddDiscovery(error))
    }
}

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
    use super::{
        resolve_object_download_with_hdd_root, resolve_object_folder_download_with_hdd_root,
    };
    use crate::api::{ObjectDownloadRequest, ObjectFolderDownloadRequest};
    use crate::runtime::object_download::{
        ObjectDownloadResolveError, ObjectFolderDownloadResolveError,
    };
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
        insert_object_fixture(
            &connection,
            "ena/raw/metadata.tsv",
            "objects/aa/object-a/payload",
            "sha256:payload",
        );

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
    fn selects_verified_copy_from_managed_hdd_root() {
        let root = temp_root("download-selects-managed-copy");
        let live_sqlite_path = root.join("live.sqlite");
        let hdd_root = root.join("hdd");
        let disk_root = hdd_root.join("disk-b");
        write_hdd_marker(&disk_root, "disk-b");
        let selected_source = write_source(
            &disk_root,
            "objects/bb/object-b/payload",
            b"selected payload",
        );
        let connection = fixture_connection(&live_sqlite_path);
        insert_disk_fixture(&connection, "disk-b");
        insert_object_row_fixture(&connection, "ena/raw/metadata.tsv", "sha256:payload");
        insert_placement_fixture(
            &connection,
            "placement-unmanaged",
            "ena/raw/metadata.tsv",
            "disk-a",
            "objects/aa/object-a/payload",
            Some("2026-07-09T10:18:22Z"),
        );
        insert_placement_fixture(
            &connection,
            "placement-unverified",
            "ena/raw/metadata.tsv",
            "disk-b",
            "objects/bb/object-b/unverified",
            None,
        );
        insert_placement_fixture(
            &connection,
            "placement-selected",
            "ena/raw/metadata.tsv",
            "disk-b",
            "objects/bb/object-b/payload",
            Some("2026-07-09T10:18:23Z"),
        );

        let response = resolve_object_download_with_hdd_root(
            &live_sqlite_path,
            &hdd_root,
            &store_id(),
            &ObjectDownloadRequest {
                endpoint: store_id(),
                object_id: object_id("ena/raw/metadata.tsv"),
            },
        )
        .expect("download resolves from managed verified copy");

        assert_eq!(response.source_disk_id.as_str(), "disk-b");
        assert_eq!(response.source_path, selected_source);
        assert_eq!(response.size_bytes, b"selected payload".len() as u64);

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

    #[test]
    fn resolves_folder_archive_entries_from_verified_managed_hdd_root() {
        let root = temp_root("folder-download-resolve");
        let live_sqlite_path = root.join("live.sqlite");
        let hdd_root = root.join("hdd");
        let disk_root = hdd_root.join("disk-a");
        write_hdd_marker(&disk_root, "disk-a");
        let first_source = write_source(
            &disk_root,
            "objects/aa/object-a/payload",
            b"metadata payload",
        );
        let second_source = write_source(&disk_root, "objects/bb/object-b/payload", b"reads");
        let connection = fixture_connection(&live_sqlite_path);
        insert_object_fixture(
            &connection,
            "ena/raw/Xeno/metadata.tsv",
            "objects/aa/object-a/payload",
            "sha256:first",
        );
        insert_object_fixture(
            &connection,
            "ena/raw/Xeno/sample.fastq.gz",
            "objects/bb/object-b/payload",
            "sha256:second",
        );
        insert_object_fixture(
            &connection,
            "ena/raw/Other/outside.tsv",
            "objects/cc/object-c/payload",
            "sha256:outside",
        );

        let response = resolve_object_folder_download_with_hdd_root(
            &live_sqlite_path,
            &hdd_root,
            &store_id(),
            &ObjectFolderDownloadRequest {
                endpoint: store_id(),
                prefix: "/ena/raw/Xeno/".to_string(),
            },
        )
        .expect("folder download resolves");

        assert_eq!(response.prefix, "raw/Xeno");
        assert_eq!(response.archive_name, "Xeno.tar.gz");
        assert_eq!(response.total_files, 2);
        assert_eq!(
            response.total_source_bytes,
            b"metadata payload".len() as u64 + b"reads".len() as u64
        );
        assert_eq!(response.entries[0].archive_path, "metadata.tsv");
        assert_eq!(response.entries[0].source_path, first_source);
        assert_eq!(response.entries[1].archive_path, "sample.fastq.gz");
        assert_eq!(response.entries[1].source_path, second_source);

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn rejects_folder_archive_when_any_entry_lacks_verified_hdd_copy() {
        let root = temp_root("folder-download-unverified");
        let live_sqlite_path = root.join("live.sqlite");
        let hdd_root = root.join("hdd");
        let disk_root = hdd_root.join("disk-a");
        write_hdd_marker(&disk_root, "disk-a");
        write_source(&disk_root, "objects/aa/object-a/payload", b"metadata");
        let connection = fixture_connection(&live_sqlite_path);
        insert_object_fixture(
            &connection,
            "ena/raw/Xeno/metadata.tsv",
            "objects/aa/object-a/payload",
            "sha256:first",
        );
        insert_unverified_object_fixture(&connection, "ena/raw/Xeno/missing.fastq.gz");

        let err = resolve_object_folder_download_with_hdd_root(
            &live_sqlite_path,
            &hdd_root,
            &store_id(),
            &ObjectFolderDownloadRequest {
                endpoint: store_id(),
                prefix: "ena/raw/Xeno".to_string(),
            },
        )
        .expect_err("folder download rejects unverified object");

        assert!(matches!(
            err,
            ObjectFolderDownloadResolveError::Object(
                ObjectDownloadResolveError::NoVerifiedManagedPlacement { .. }
            )
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

    fn insert_disk_fixture(connection: &Connection, disk_id: &str) {
        connection
            .execute(
                "INSERT INTO disks
                    (disk_id, pool_id, role, state, size_bytes, serial_hint, model_hint,
                     enclosure_topology_path, created_at_utc, updated_at_utc)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL, NULL, NULL, ?6, ?7)",
                params![
                    disk_id,
                    "pool-a",
                    "hdd",
                    "Healthy",
                    4_000_000_000_i64,
                    "2026-07-09T10:18:21Z",
                    "2026-07-09T10:18:21Z",
                ],
            )
            .expect("insert disk");
    }

    fn insert_object_fixture(
        connection: &Connection,
        object_id: &str,
        relative_path: &str,
        content_hash: &str,
    ) {
        insert_object_row_fixture(connection, object_id, content_hash);
        insert_placement_fixture(
            connection,
            &format!("placement-{object_id}"),
            object_id,
            "disk-a",
            relative_path,
            Some("2026-07-09T10:18:22Z"),
        );
    }

    fn insert_object_row_fixture(connection: &Connection, object_id: &str, content_hash: &str) {
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
    }

    fn insert_placement_fixture(
        connection: &Connection,
        placement_id: &str,
        object_id: &str,
        disk_id: &str,
        relative_path: &str,
        verified_at_utc: Option<&str>,
    ) {
        connection
            .execute(
                "INSERT INTO placements
                    (placement_id, object_id, disk_id, relative_path, content_hash, verified_at_utc, created_at_utc)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    placement_id,
                    object_id,
                    disk_id,
                    relative_path,
                    "sha256:payload",
                    verified_at_utc,
                    "2026-07-09T10:18:21Z",
                ],
            )
            .expect("insert placement");
    }

    fn write_source(
        disk_root: &std::path::Path,
        relative_path: &str,
        bytes: &[u8],
    ) -> std::path::PathBuf {
        let source_path = disk_root.join(relative_path);
        fs::create_dir_all(source_path.parent().expect("source parent")).expect("source parent");
        fs::write(&source_path, bytes).expect("write source");
        source_path
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
