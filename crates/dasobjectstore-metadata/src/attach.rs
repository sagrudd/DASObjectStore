use crate::inspect::{inspect_pool_metadata, PoolInspectError};
use crate::markers::{record_pool_state_marker_at, PoolStateMarker};
use crate::snapshot::{import_metadata_snapshot, SnapshotImportError, SnapshotImportOptions};
use dasobjectstore_core::ids::PoolId;
use dasobjectstore_core::lifecycle::PoolState;
use std::fmt::{self, Display};
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadOnlyAttachOptions {
    pub source_path: PathBuf,
    pub recovery_metadata_dir: PathBuf,
    pub recorded_at_utc: String,
}

impl ReadOnlyAttachOptions {
    pub fn new(
        source_path: impl Into<PathBuf>,
        recovery_metadata_dir: impl Into<PathBuf>,
        recorded_at_utc: impl Into<String>,
    ) -> Self {
        Self {
            source_path: source_path.into(),
            recovery_metadata_dir: recovery_metadata_dir.into(),
            recorded_at_utc: recorded_at_utc.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadOnlyAttachReport {
    pub pool_id: PoolId,
    pub recovered_live_sqlite_path: PathBuf,
    pub recovered_disk_count: usize,
}

pub fn attach_clean_pool_read_only(
    options: &ReadOnlyAttachOptions,
) -> Result<ReadOnlyAttachReport, ReadOnlyAttachError> {
    let summary = inspect_pool_metadata(&options.source_path)?;
    if summary.state != PoolState::Clean {
        return Err(ReadOnlyAttachError::PoolNotClean {
            state: summary.state,
        });
    }

    let import = import_metadata_snapshot(&SnapshotImportOptions::new(
        summary.metadata_path,
        &options.recovery_metadata_dir,
    ))?;
    let marker = PoolStateMarker::read_only_clean_attach(
        import.pool_id.clone(),
        "clean portable read-only attach",
        options.recorded_at_utc.clone(),
    );
    record_pool_state_marker_at(&import.recovered_live_sqlite_path, &marker)?;

    Ok(ReadOnlyAttachReport {
        pool_id: import.pool_id,
        recovered_live_sqlite_path: import.recovered_live_sqlite_path,
        recovered_disk_count: import.recovered_disk_count,
    })
}

#[derive(Debug)]
pub enum ReadOnlyAttachError {
    Inspect(PoolInspectError),
    Import(SnapshotImportError),
    Sqlite(rusqlite::Error),
    PoolNotClean { state: PoolState },
}

impl Display for ReadOnlyAttachError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inspect(err) => write!(formatter, "read-only attach inspection failed: {err}"),
            Self::Import(err) => write!(formatter, "read-only attach import failed: {err}"),
            Self::Sqlite(err) => write!(formatter, "read-only attach marker failed: {err}"),
            Self::PoolNotClean { state } => write!(
                formatter,
                "clean read-only attach requires a clean pool snapshot, found {state:?}"
            ),
        }
    }
}

impl std::error::Error for ReadOnlyAttachError {}

impl From<PoolInspectError> for ReadOnlyAttachError {
    fn from(err: PoolInspectError) -> Self {
        Self::Inspect(err)
    }
}

impl From<SnapshotImportError> for ReadOnlyAttachError {
    fn from(err: SnapshotImportError) -> Self {
        Self::Import(err)
    }
}

impl From<rusqlite::Error> for ReadOnlyAttachError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Sqlite(err)
    }
}

#[cfg(test)]
mod tests {
    use super::{attach_clean_pool_read_only, ReadOnlyAttachError, ReadOnlyAttachOptions};
    use crate::initialize::{initialize_pool, PoolInitOptions};
    use crate::snapshot::{export_metadata_snapshot, SnapshotExportOptions};
    use dasobjectstore_core::ids::PoolId;
    use rusqlite::{params, Connection};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn attaches_clean_pool_snapshot_as_read_only() {
        let root = temp_root("clean-read-only-attach");
        let ssd_root = root.join("ssd");
        let source_root = root.join("mounted-disk");
        let recovery_root = root.join("recovered");
        let snapshot_dir = source_root.join(".dasobjectstore").join("metadata");
        let init = initialize_pool(&PoolInitOptions::new(
            &ssd_root,
            PoolId::new("pool-a").expect("pool id"),
            "2026-01-02T00:00:00Z",
        ))
        .expect("pool initializes");
        mark_pool_state(&init.live_sqlite_path, "Clean");
        insert_disk(&init.live_sqlite_path);
        export_metadata_snapshot(&SnapshotExportOptions::new(
            &init.live_sqlite_path,
            vec![snapshot_dir],
            "2026-01-03T00:00:00Z",
        ))
        .expect("snapshot exports");

        let report = attach_clean_pool_read_only(&ReadOnlyAttachOptions::new(
            &source_root,
            &recovery_root,
            "2026-01-04T00:00:00Z",
        ))
        .expect("pool attaches read-only");

        assert_eq!(report.pool_id.as_str(), "pool-a");
        assert_eq!(report.recovered_disk_count, 1);
        assert_eq!(pool_state(&report.recovered_live_sqlite_path), "ReadOnly");
        assert_eq!(
            marker_kind(&report.recovered_live_sqlite_path),
            "read_only_import"
        );

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn rejects_dirty_pool_snapshot_for_clean_attach() {
        let root = temp_root("dirty-read-only-attach");
        let ssd_root = root.join("ssd");
        let source_root = root.join("mounted-disk");
        let recovery_root = root.join("recovered");
        let snapshot_dir = source_root.join(".dasobjectstore").join("metadata");
        let init = initialize_pool(&PoolInitOptions::new(
            &ssd_root,
            PoolId::new("pool-a").expect("pool id"),
            "2026-01-02T00:00:00Z",
        ))
        .expect("pool initializes");
        mark_pool_state(&init.live_sqlite_path, "Dirty");
        export_metadata_snapshot(&SnapshotExportOptions::new(
            &init.live_sqlite_path,
            vec![snapshot_dir],
            "2026-01-03T00:00:00Z",
        ))
        .expect("snapshot exports");

        let err = attach_clean_pool_read_only(&ReadOnlyAttachOptions::new(
            &source_root,
            &recovery_root,
            "2026-01-04T00:00:00Z",
        ))
        .expect_err("dirty pool is rejected");

        assert!(matches!(err, ReadOnlyAttachError::PoolNotClean { .. }));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    fn mark_pool_state(live_sqlite_path: &PathBuf, state: &str) {
        let connection = Connection::open(live_sqlite_path).expect("open live sqlite");
        connection
            .execute(
                "UPDATE pools SET state = ?1, updated_at_utc = ?2 WHERE pool_id = ?3",
                params![state, "2026-01-03T00:00:00Z", "pool-a"],
            )
            .expect("pool state updates");
    }

    fn insert_disk(live_sqlite_path: &PathBuf) {
        let connection = Connection::open(live_sqlite_path).expect("open live sqlite");
        connection
            .execute(
                "INSERT INTO disks (
                    disk_id,
                    pool_id,
                    role,
                    state,
                    size_bytes,
                    created_at_utc,
                    updated_at_utc
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    "disk-a",
                    "pool-a",
                    "hdd_capacity",
                    "Healthy",
                    1000_i64,
                    "2026-01-02T00:00:00Z",
                    "2026-01-03T00:00:00Z"
                ],
            )
            .expect("disk inserts");
    }
    fn pool_state(live_sqlite_path: &PathBuf) -> String {
        let connection = Connection::open(live_sqlite_path).expect("open live sqlite");
        connection
            .query_row(
                "SELECT state FROM pools WHERE pool_id = 'pool-a'",
                [],
                |row| row.get(0),
            )
            .expect("pool state")
    }

    fn marker_kind(live_sqlite_path: &PathBuf) -> String {
        let connection = Connection::open(live_sqlite_path).expect("open live sqlite");
        connection
            .query_row(
                "SELECT marker_kind FROM pool_state_markers WHERE pool_id = 'pool-a'",
                [],
                |row| row.get(0),
            )
            .expect("marker kind")
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
