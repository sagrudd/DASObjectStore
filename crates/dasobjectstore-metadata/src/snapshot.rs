use crate::format::MetadataArtifact;
use crate::initialize::LIVE_SQLITE_FILE_NAME;
use crate::manifest::{
    ArtifactReference, DiskManifest, DiskManifestEntry, DiskRole, PoolManifest,
    DISK_MANIFEST_FORMAT_VERSION,
};
use crate::placement_log::PLACEMENT_LOG_FORMAT_VERSION;
use dasobjectstore_core::ids::{DiskId, PoolId};
use dasobjectstore_core::lifecycle::{DiskState, PoolState};
use rusqlite::Connection;
use std::fmt::{self, Display};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const POOL_MANIFEST_FILE_NAME: &str = "pool-manifest.json";
pub const DISK_MANIFEST_FILE_NAME: &str = "disk-manifest.json";
pub const PLACEMENT_LOG_FILE_NAME: &str = "placement-log.jsonl";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotExportOptions {
    pub live_sqlite_path: PathBuf,
    pub target_metadata_dirs: Vec<PathBuf>,
    pub exported_at_utc: String,
}

impl SnapshotExportOptions {
    pub fn new(
        live_sqlite_path: impl Into<PathBuf>,
        target_metadata_dirs: Vec<PathBuf>,
        exported_at_utc: impl Into<String>,
    ) -> Self {
        Self {
            live_sqlite_path: live_sqlite_path.into(),
            target_metadata_dirs,
            exported_at_utc: exported_at_utc.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SnapshotExportReport {
    pub pool_id: PoolId,
    pub exported_dirs: Vec<PathBuf>,
}

pub fn export_metadata_snapshot(
    options: &SnapshotExportOptions,
) -> Result<SnapshotExportReport, SnapshotExportError> {
    if options.target_metadata_dirs.is_empty() {
        return Err(SnapshotExportError::NoTargets);
    }

    let connection = Connection::open(&options.live_sqlite_path)?;
    let pool = load_single_pool(&connection)?;
    let disk_manifest = DiskManifest::new(
        pool.pool_id.clone(),
        options.exported_at_utc.clone(),
        load_disks(&connection)?,
    );
    let pool_manifest = PoolManifest::new(
        pool.pool_id.clone(),
        pool.state,
        pool.created_at_utc,
        pool.updated_at_utc,
        ArtifactReference::new(
            MetadataArtifact::DiskManifest,
            DISK_MANIFEST_FORMAT_VERSION,
            DISK_MANIFEST_FILE_NAME,
            None,
        ),
        ArtifactReference::new(
            MetadataArtifact::PlacementLog,
            PLACEMENT_LOG_FORMAT_VERSION,
            PLACEMENT_LOG_FILE_NAME,
            None,
        ),
    );

    for target in &options.target_metadata_dirs {
        write_snapshot_dir(
            target,
            &options.live_sqlite_path,
            &pool_manifest,
            &disk_manifest,
        )?;
    }

    Ok(SnapshotExportReport {
        pool_id: pool.pool_id,
        exported_dirs: options.target_metadata_dirs.clone(),
    })
}

fn write_snapshot_dir(
    target: &Path,
    live_sqlite_path: &Path,
    pool_manifest: &PoolManifest,
    disk_manifest: &DiskManifest,
) -> Result<(), SnapshotExportError> {
    fs::create_dir_all(target)?;
    write_json(target.join(POOL_MANIFEST_FILE_NAME), pool_manifest)?;
    write_json(target.join(DISK_MANIFEST_FILE_NAME), disk_manifest)?;
    File::create(target.join(PLACEMENT_LOG_FILE_NAME))?;
    fs::copy(live_sqlite_path, target.join(LIVE_SQLITE_FILE_NAME))?;

    Ok(())
}

fn write_json(path: PathBuf, value: &impl serde::Serialize) -> Result<(), SnapshotExportError> {
    let mut file = File::create(path)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;

    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PoolRow {
    pool_id: PoolId,
    state: PoolState,
    created_at_utc: String,
    updated_at_utc: String,
}

fn load_single_pool(connection: &Connection) -> Result<PoolRow, SnapshotExportError> {
    let rows = connection.query_row(
        "SELECT COUNT(*), COALESCE(MIN(pool_id), '')
         FROM pools",
        [],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
    )?;
    match rows {
        (1, pool_id) => load_pool(connection, &pool_id),
        (0, _) => Err(SnapshotExportError::MissingPool),
        (count, _) => Err(SnapshotExportError::MultiplePools { count }),
    }
}

fn load_pool(connection: &Connection, pool_id: &str) -> Result<PoolRow, SnapshotExportError> {
    connection
        .query_row(
            "SELECT pool_id, state, created_at_utc, updated_at_utc
             FROM pools
             WHERE pool_id = ?1",
            [pool_id],
            |row| {
                let pool_id = PoolId::new(row.get::<_, String>(0)?)
                    .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
                let state = parse_pool_state(&row.get::<_, String>(1)?)
                    .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;

                Ok(PoolRow {
                    pool_id,
                    state,
                    created_at_utc: row.get(2)?,
                    updated_at_utc: row.get(3)?,
                })
            },
        )
        .map_err(SnapshotExportError::from)
}

fn load_disks(connection: &Connection) -> Result<Vec<DiskManifestEntry>, SnapshotExportError> {
    let mut statement = connection.prepare(
        "SELECT disk_id,
                state,
                role,
                size_bytes,
                serial_hint,
                model_hint,
                enclosure_topology_path,
                created_at_utc,
                updated_at_utc
         FROM disks
         ORDER BY disk_id",
    )?;
    let rows = statement.query_map([], |row| {
        let disk_id = DiskId::new(row.get::<_, String>(0)?)
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        let state = parse_disk_state(&row.get::<_, String>(1)?)
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        let role = parse_disk_role(&row.get::<_, String>(2)?)
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        let mut disk = DiskManifestEntry::new(
            disk_id,
            state,
            role,
            row.get::<_, String>(7)?,
            row.get::<_, String>(8)?,
        );
        disk.size_bytes = row.get::<_, Option<u64>>(3)?;
        disk.serial_hint = row.get(4)?;
        disk.model_hint = row.get(5)?;
        disk.enclosure_topology_path = row.get(6)?;

        Ok(disk)
    })?;

    rows.map(|row| row.map_err(SnapshotExportError::from))
        .collect()
}

fn parse_pool_state(value: &str) -> Result<PoolState, UnknownSnapshotValue> {
    match value {
        "New" => Ok(PoolState::New),
        "Clean" => Ok(PoolState::Clean),
        "Dirty" => Ok(PoolState::Dirty),
        "ReadOnly" => Ok(PoolState::ReadOnly),
        "Repairing" => Ok(PoolState::Repairing),
        "Degraded" => Ok(PoolState::Degraded),
        _ => Err(UnknownSnapshotValue::new("pool state", value)),
    }
}

fn parse_disk_state(value: &str) -> Result<DiskState, UnknownSnapshotValue> {
    match value {
        "Candidate" => Ok(DiskState::Candidate),
        "Healthy" => Ok(DiskState::Healthy),
        "Watch" => Ok(DiskState::Watch),
        "Suspect" => Ok(DiskState::Suspect),
        "Draining" => Ok(DiskState::Draining),
        "Retired" => Ok(DiskState::Retired),
        "Failed" => Ok(DiskState::Failed),
        _ => Err(UnknownSnapshotValue::new("disk state", value)),
    }
}

fn parse_disk_role(value: &str) -> Result<DiskRole, UnknownSnapshotValue> {
    match value {
        "ingest_ssd" => Ok(DiskRole::IngestSsd),
        "hdd_capacity" => Ok(DiskRole::HddCapacity),
        "replacement" => Ok(DiskRole::Replacement),
        "retired" => Ok(DiskRole::Retired),
        _ => Err(UnknownSnapshotValue::new("disk role", value)),
    }
}

#[derive(Debug)]
pub enum SnapshotExportError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Sqlite(rusqlite::Error),
    NoTargets,
    MissingPool,
    MultiplePools { count: i64 },
}

impl Display for SnapshotExportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(
                formatter,
                "metadata snapshot filesystem export failed: {err}"
            ),
            Self::Json(err) => write!(formatter, "metadata snapshot JSON export failed: {err}"),
            Self::Sqlite(err) => write!(formatter, "metadata snapshot query failed: {err}"),
            Self::NoTargets => formatter.write_str("metadata snapshot export requires a target"),
            Self::MissingPool => formatter.write_str("live metadata has no pool row"),
            Self::MultiplePools { count } => {
                write!(formatter, "live metadata has {count} pool rows")
            }
        }
    }
}

impl std::error::Error for SnapshotExportError {}

impl From<std::io::Error> for SnapshotExportError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<serde_json::Error> for SnapshotExportError {
    fn from(err: serde_json::Error) -> Self {
        Self::Json(err)
    }
}

impl From<rusqlite::Error> for SnapshotExportError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Sqlite(err)
    }
}

#[derive(Debug)]
struct UnknownSnapshotValue {
    field: &'static str,
    value: String,
}

impl UnknownSnapshotValue {
    fn new(field: &'static str, value: &str) -> Self {
        Self {
            field,
            value: value.to_string(),
        }
    }
}

impl Display for UnknownSnapshotValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "unknown {} `{}`", self.field, self.value)
    }
}

impl std::error::Error for UnknownSnapshotValue {}

#[cfg(test)]
mod tests {
    use super::{
        export_metadata_snapshot, SnapshotExportError, SnapshotExportOptions,
        DISK_MANIFEST_FILE_NAME, LIVE_SQLITE_FILE_NAME, PLACEMENT_LOG_FILE_NAME,
        POOL_MANIFEST_FILE_NAME,
    };
    use crate::initialize::{initialize_pool, PoolInitOptions};
    use crate::manifest::{DiskManifest, PoolManifest};
    use dasobjectstore_core::ids::PoolId;
    use rusqlite::{params, Connection};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn exports_snapshot_files_to_each_target_metadata_dir() {
        let root = temp_root("snapshot-export");
        let ssd_root = root.join("ssd");
        let hdd_a = root.join("hdd-a").join("metadata");
        let hdd_b = root.join("hdd-b").join("metadata");
        let init = initialize_pool(&PoolInitOptions::new(
            &ssd_root,
            PoolId::new("pool-a").expect("pool id"),
            "2026-01-02T00:00:00Z",
        ))
        .expect("pool initializes");
        insert_disk(&init.live_sqlite_path);

        let report = export_metadata_snapshot(&SnapshotExportOptions::new(
            &init.live_sqlite_path,
            vec![hdd_a.clone(), hdd_b.clone()],
            "2026-01-03T00:00:00Z",
        ))
        .expect("snapshot exports");

        assert_eq!(report.pool_id.as_str(), "pool-a");
        assert_eq!(report.exported_dirs, vec![hdd_a.clone(), hdd_b.clone()]);
        for target in [hdd_a, hdd_b] {
            assert!(target.join(POOL_MANIFEST_FILE_NAME).is_file());
            assert!(target.join(DISK_MANIFEST_FILE_NAME).is_file());
            assert!(target.join(PLACEMENT_LOG_FILE_NAME).is_file());
            assert!(target.join(LIVE_SQLITE_FILE_NAME).is_file());

            let pool_manifest: PoolManifest = read_json(&target.join(POOL_MANIFEST_FILE_NAME));
            let disk_manifest: DiskManifest = read_json(&target.join(DISK_MANIFEST_FILE_NAME));

            assert_eq!(pool_manifest.pool_id.as_str(), "pool-a");
            assert_eq!(
                pool_manifest.disk_manifest.relative_path,
                DISK_MANIFEST_FILE_NAME
            );
            assert_eq!(disk_manifest.disks.len(), 1);
            assert_eq!(disk_manifest.disks[0].disk_id.as_str(), "disk-a");
        }

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn rejects_snapshot_export_without_targets() {
        let root = temp_root("snapshot-export-no-targets");
        let init = initialize_pool(&PoolInitOptions::new(
            root.join("ssd"),
            PoolId::new("pool-a").expect("pool id"),
            "2026-01-02T00:00:00Z",
        ))
        .expect("pool initializes");

        let err = export_metadata_snapshot(&SnapshotExportOptions::new(
            init.live_sqlite_path,
            Vec::new(),
            "2026-01-03T00:00:00Z",
        ))
        .expect_err("empty targets fail");

        assert!(matches!(err, SnapshotExportError::NoTargets));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    fn insert_disk(live_sqlite_path: &Path) {
        let connection = Connection::open(live_sqlite_path).expect("open live sqlite");
        connection
            .execute(
                "INSERT INTO disks (
                    disk_id,
                    pool_id,
                    role,
                    state,
                    size_bytes,
                    serial_hint,
                    model_hint,
                    enclosure_topology_path,
                    created_at_utc,
                    updated_at_utc
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    "disk-a",
                    "pool-a",
                    "hdd_capacity",
                    "Healthy",
                    4_000_787_030_016_u64,
                    "WD-OLD-001",
                    "WDC WD40EFRX",
                    "usb@001/002",
                    "2026-01-02T00:00:00Z",
                    "2026-01-03T00:00:00Z"
                ],
            )
            .expect("insert disk");
    }

    fn read_json<T>(path: &Path) -> T
    where
        T: serde::de::DeserializeOwned,
    {
        let file = fs::File::open(path).expect("open json");
        serde_json::from_reader(file).expect("parse json")
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
