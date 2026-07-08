use dasobjectstore_core::ids::PoolId;
use dasobjectstore_core::lifecycle::PoolState;
use rusqlite::Connection;
use std::fmt::{self, Display};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PoolRepairActivitySnapshot {
    pub live_sqlite_path: PathBuf,
    pub events: Vec<PoolRepairActivityEvent>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PoolRepairActivityEvent {
    pub pool_id: PoolId,
    pub state: PoolState,
    pub marker_kind: Option<String>,
    pub reason: Option<String>,
    pub updated_at_utc: String,
}

pub fn read_pool_repair_activity(
    live_sqlite_path: impl AsRef<Path>,
) -> Result<PoolRepairActivitySnapshot, PoolRepairActivityReadError> {
    let live_sqlite_path = live_sqlite_path.as_ref();
    let connection = Connection::open(live_sqlite_path)?;

    if !table_exists(&connection, "pools")? {
        return Ok(PoolRepairActivitySnapshot {
            live_sqlite_path: live_sqlite_path.to_path_buf(),
            events: Vec::new(),
        });
    }

    let query = if table_exists(&connection, "pool_state_markers")? {
        "SELECT
            pools.pool_id,
            pools.state,
            pools.updated_at_utc,
            marker.marker_kind,
            marker.reason
         FROM pools
         LEFT JOIN pool_state_markers AS marker
           ON marker.marker_id = (
                SELECT marker_id
                FROM pool_state_markers
                WHERE pool_state_markers.pool_id = pools.pool_id
                  AND (
                    pool_state_markers.marker_kind = 'repair_import'
                    OR pool_state_markers.next_state IN ('Repairing', 'Degraded')
                  )
                ORDER BY recorded_at_utc DESC, marker_id DESC
                LIMIT 1
           )
         WHERE pools.state IN ('Repairing', 'Degraded')
         ORDER BY pools.updated_at_utc DESC, pools.pool_id ASC"
    } else {
        "SELECT
            pools.pool_id,
            pools.state,
            pools.updated_at_utc,
            NULL AS marker_kind,
            NULL AS reason
         FROM pools
         WHERE pools.state IN ('Repairing', 'Degraded')
         ORDER BY pools.updated_at_utc DESC, pools.pool_id ASC"
    };

    let mut statement = connection.prepare(query)?;
    let mut rows = statement.query([])?;
    let mut events = Vec::new();

    while let Some(row) = rows.next()? {
        let pool_id: String = row.get(0)?;
        let state: String = row.get(1)?;
        events.push(PoolRepairActivityEvent {
            pool_id: PoolId::new(pool_id).map_err(|err| {
                PoolRepairActivityReadError::InvalidPoolId {
                    message: err.to_string(),
                }
            })?,
            state: parse_pool_state(&state)?,
            updated_at_utc: row.get(2)?,
            marker_kind: row.get(3)?,
            reason: row.get(4)?,
        });
    }

    Ok(PoolRepairActivitySnapshot {
        live_sqlite_path: live_sqlite_path.to_path_buf(),
        events,
    })
}

fn table_exists(
    connection: &Connection,
    table_name: &str,
) -> Result<bool, PoolRepairActivityReadError> {
    let count = connection.query_row(
        "SELECT COUNT(*)
         FROM sqlite_master
         WHERE type = 'table'
           AND name = ?1",
        [table_name],
        |row| row.get::<_, i64>(0),
    )?;
    Ok(count > 0)
}

fn parse_pool_state(value: &str) -> Result<PoolState, PoolRepairActivityReadError> {
    match value {
        "New" => Ok(PoolState::New),
        "Clean" => Ok(PoolState::Clean),
        "Dirty" => Ok(PoolState::Dirty),
        "ReadOnly" => Ok(PoolState::ReadOnly),
        "Repairing" => Ok(PoolState::Repairing),
        "Degraded" => Ok(PoolState::Degraded),
        _ => Err(PoolRepairActivityReadError::InvalidPoolState {
            state: value.to_string(),
        }),
    }
}

#[derive(Debug)]
pub enum PoolRepairActivityReadError {
    Io(rusqlite::Error),
    InvalidPoolId { message: String },
    InvalidPoolState { state: String },
}

impl Display for PoolRepairActivityReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(formatter, "failed to read pool repair activity: {err}"),
            Self::InvalidPoolId { message } => {
                write!(
                    formatter,
                    "pool repair activity has invalid pool id: {message}"
                )
            }
            Self::InvalidPoolState { state } => {
                write!(
                    formatter,
                    "pool repair activity has invalid pool state `{state}`"
                )
            }
        }
    }
}

impl std::error::Error for PoolRepairActivityReadError {}

impl From<rusqlite::Error> for PoolRepairActivityReadError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Io(err)
    }
}

#[cfg(test)]
mod tests {
    use super::read_pool_repair_activity;
    use crate::schema::LIVE_SCHEMA_SQL;
    use rusqlite::Connection;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn reads_live_repairing_and_degraded_pools() {
        let root = temp_root("repair-activity");
        fs::create_dir_all(&root).expect("temp root");
        let live_sqlite_path = root.join("live.sqlite");
        let connection = Connection::open(&live_sqlite_path).expect("open sqlite");
        connection
            .execute_batch(LIVE_SCHEMA_SQL)
            .expect("schema applies");
        connection
            .execute_batch(
                "INSERT INTO pools (pool_id, state, created_at_utc, updated_at_utc)
                 VALUES
                    ('pool-clean', 'Clean', '2026-07-09T00:00:00Z', '2026-07-09T00:01:00Z'),
                    ('pool-repair', 'Repairing', '2026-07-09T00:00:00Z', '2026-07-09T00:02:00Z'),
                    ('pool-degraded', 'Degraded', '2026-07-09T00:00:00Z', '2026-07-09T00:03:00Z');
                 INSERT INTO pool_state_markers (
                    pool_id, marker_kind, previous_state, next_state, import_mode, reason,
                    recorded_at_utc
                 ) VALUES (
                    'pool-repair', 'repair_import', 'Dirty', 'Repairing', 'Repair',
                    'checksum repair', '2026-07-09T00:02:00Z'
                 );",
            )
            .expect("fixtures insert");

        let snapshot = read_pool_repair_activity(&live_sqlite_path).expect("activity reads");

        assert_eq!(snapshot.live_sqlite_path, live_sqlite_path);
        assert_eq!(snapshot.events.len(), 2);
        assert_eq!(snapshot.events[0].pool_id.as_str(), "pool-degraded");
        assert_eq!(snapshot.events[1].pool_id.as_str(), "pool-repair");
        assert_eq!(
            snapshot.events[1].marker_kind.as_deref(),
            Some("repair_import")
        );
        assert_eq!(
            snapshot.events[1].reason.as_deref(),
            Some("checksum repair")
        );

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn returns_empty_activity_for_older_sqlite_without_pool_table() {
        let root = temp_root("repair-activity-empty");
        fs::create_dir_all(&root).expect("temp root");
        let live_sqlite_path = root.join("live.sqlite");
        let _connection = Connection::open(&live_sqlite_path).expect("open sqlite");

        let snapshot = read_pool_repair_activity(&live_sqlite_path).expect("activity reads");

        assert!(snapshot.events.is_empty());

        fs::remove_dir_all(root).expect("cleanup temp root");
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
