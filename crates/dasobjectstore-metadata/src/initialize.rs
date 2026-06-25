use crate::format::FormatVersion;
use crate::ingest::IngestStagingLayout;
use crate::manifest::{DISK_MANIFEST_FORMAT_VERSION, POOL_MANIFEST_FORMAT_VERSION};
use crate::placement_log::PLACEMENT_LOG_FORMAT_VERSION;
use crate::schema::{LIVE_SCHEMA_FORMAT_VERSION, LIVE_SCHEMA_SQL};
use dasobjectstore_core::ids::PoolId;
use dasobjectstore_core::lifecycle::PoolState;
use rusqlite::{params, Connection};
use std::fmt::{self, Display};
use std::fs;
use std::path::{Path, PathBuf};

pub const METADATA_DIR_NAME: &str = ".dasobjectstore";
pub const LIVE_SQLITE_FILE_NAME: &str = "live.sqlite";
pub const SNAPSHOT_DIR_NAME: &str = "metadata";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PoolInitOptions {
    pub ssd_root: PathBuf,
    pub pool_id: PoolId,
    pub created_at_utc: String,
}

impl PoolInitOptions {
    pub fn new(
        ssd_root: impl Into<PathBuf>,
        pool_id: PoolId,
        created_at_utc: impl Into<String>,
    ) -> Self {
        Self {
            ssd_root: ssd_root.into(),
            pool_id,
            created_at_utc: created_at_utc.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PoolInitReport {
    pub metadata_root: PathBuf,
    pub snapshot_dir: PathBuf,
    pub live_sqlite_path: PathBuf,
    pub pool_id: PoolId,
}

pub fn initialize_pool(options: &PoolInitOptions) -> Result<PoolInitReport, MetadataInitError> {
    fs::create_dir_all(&options.ssd_root)?;

    let metadata_root = options.ssd_root.join(METADATA_DIR_NAME);
    let snapshot_dir = metadata_root.join(SNAPSHOT_DIR_NAME);
    fs::create_dir_all(&snapshot_dir)?;
    let ingest_layout = IngestStagingLayout::for_ssd_root(&options.ssd_root);
    ingest_layout.create_base_directories()?;

    let live_sqlite_path = metadata_root.join(LIVE_SQLITE_FILE_NAME);
    if live_sqlite_path.exists() {
        return Err(MetadataInitError::LiveMetadataAlreadyExists {
            path: live_sqlite_path,
        });
    }

    let connection = Connection::open(&live_sqlite_path)?;
    apply_initial_schema(&connection, options)?;

    Ok(PoolInitReport {
        metadata_root,
        snapshot_dir,
        live_sqlite_path,
        pool_id: options.pool_id.clone(),
    })
}

fn apply_initial_schema(
    connection: &Connection,
    options: &PoolInitOptions,
) -> Result<(), rusqlite::Error> {
    connection.execute_batch(LIVE_SCHEMA_SQL)?;
    seed_format_versions(connection, &options.created_at_utc)?;
    connection.execute(
        "INSERT INTO pools (pool_id, state, created_at_utc, updated_at_utc)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            options.pool_id.as_str(),
            format!("{:?}", PoolState::New),
            options.created_at_utc,
            options.created_at_utc
        ],
    )?;

    Ok(())
}

fn seed_format_versions(
    connection: &Connection,
    updated_at_utc: &str,
) -> Result<(), rusqlite::Error> {
    for version in [
        LIVE_SCHEMA_FORMAT_VERSION,
        POOL_MANIFEST_FORMAT_VERSION,
        DISK_MANIFEST_FORMAT_VERSION,
        PLACEMENT_LOG_FORMAT_VERSION,
    ] {
        insert_format_version(connection, version, updated_at_utc)?;
    }

    Ok(())
}

fn insert_format_version(
    connection: &Connection,
    version: FormatVersion,
    updated_at_utc: &str,
) -> Result<(), rusqlite::Error> {
    connection.execute(
        "INSERT INTO metadata_format_versions (artifact, major, minor, updated_at_utc)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            version.artifact.name(),
            version.major,
            version.minor,
            updated_at_utc
        ],
    )?;

    Ok(())
}

#[derive(Debug)]
pub enum MetadataInitError {
    Io(std::io::Error),
    Sqlite(rusqlite::Error),
    LiveMetadataAlreadyExists { path: PathBuf },
}

impl Display for MetadataInitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(
                formatter,
                "metadata filesystem initialization failed: {err}"
            ),
            Self::Sqlite(err) => write!(formatter, "live metadata initialization failed: {err}"),
            Self::LiveMetadataAlreadyExists { path } => {
                write!(
                    formatter,
                    "live metadata already exists at {}",
                    display_path(path)
                )
            }
        }
    }
}

impl std::error::Error for MetadataInitError {}

impl From<std::io::Error> for MetadataInitError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<rusqlite::Error> for MetadataInitError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Sqlite(err)
    }
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::{initialize_pool, MetadataInitError, PoolInitOptions};
    use dasobjectstore_core::ids::PoolId;
    use rusqlite::Connection;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn initializes_live_metadata_on_ssd_path() {
        let root = temp_root("initializes-live-metadata");
        let options = PoolInitOptions::new(
            &root,
            PoolId::new("pool-a").expect("pool id"),
            "2026-01-02T00:00:00Z",
        );

        let report = initialize_pool(&options).expect("pool initializes");

        assert_eq!(report.pool_id.as_str(), "pool-a");
        assert!(report.metadata_root.is_dir());
        assert!(report.snapshot_dir.is_dir());
        assert!(root
            .join(".dasobjectstore")
            .join("ingest")
            .join("jobs")
            .is_dir());
        assert!(report.live_sqlite_path.is_file());

        let connection = Connection::open(&report.live_sqlite_path).expect("open live sqlite");
        assert_eq!(pool_row_count(&connection), 1);
        assert_eq!(format_version_artifacts(&connection).len(), 4);

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn refuses_to_replace_existing_live_metadata() {
        let root = temp_root("refuses-existing-live-metadata");
        let options = PoolInitOptions::new(
            &root,
            PoolId::new("pool-a").expect("pool id"),
            "2026-01-02T00:00:00Z",
        );
        initialize_pool(&options).expect("first init succeeds");

        let err = initialize_pool(&options).expect_err("second init fails");

        assert!(matches!(
            err,
            MetadataInitError::LiveMetadataAlreadyExists { .. }
        ));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    fn pool_row_count(connection: &Connection) -> i64 {
        connection
            .query_row("SELECT COUNT(*) FROM pools", [], |row| row.get(0))
            .expect("count pools")
    }

    fn format_version_artifacts(connection: &Connection) -> Vec<String> {
        let mut statement = connection
            .prepare(
                "SELECT artifact
                 FROM metadata_format_versions
                 ORDER BY artifact",
            )
            .expect("prepare format version query");
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .expect("query format versions");

        rows.map(|row| row.expect("artifact")).collect()
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
