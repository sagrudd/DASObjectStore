use dasobjectstore_core::ids::PoolId;
use dasobjectstore_core::lifecycle::{ImportMode, PoolState};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolStateMarkerKind {
    CleanEject,
    DirtyAttach,
    ReadOnlyImport,
    RepairImport,
    ForceReadWriteImport,
}

impl PoolStateMarkerKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::CleanEject => "clean_eject",
            Self::DirtyAttach => "dirty_attach",
            Self::ReadOnlyImport => "read_only_import",
            Self::RepairImport => "repair_import",
            Self::ForceReadWriteImport => "force_read_write_import",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PoolStateMarker {
    pub pool_id: PoolId,
    pub marker_kind: PoolStateMarkerKind,
    pub previous_state: Option<PoolState>,
    pub next_state: PoolState,
    pub import_mode: Option<ImportMode>,
    pub reason: Option<String>,
    pub recorded_at_utc: String,
}

impl PoolStateMarker {
    pub fn clean_eject(pool_id: PoolId, recorded_at_utc: impl Into<String>) -> Self {
        Self::new(
            pool_id,
            PoolStateMarkerKind::CleanEject,
            Some(PoolState::Dirty),
            PoolState::Clean,
            None,
            None,
            recorded_at_utc,
        )
    }

    pub fn dirty_attach(pool_id: PoolId, recorded_at_utc: impl Into<String>) -> Self {
        Self::new(
            pool_id,
            PoolStateMarkerKind::DirtyAttach,
            Some(PoolState::Clean),
            PoolState::Dirty,
            None,
            None,
            recorded_at_utc,
        )
    }

    pub fn read_only_import(
        pool_id: PoolId,
        reason: impl Into<String>,
        recorded_at_utc: impl Into<String>,
    ) -> Self {
        Self::new(
            pool_id,
            PoolStateMarkerKind::ReadOnlyImport,
            Some(PoolState::Dirty),
            PoolState::ReadOnly,
            Some(ImportMode::ReadOnly),
            Some(reason.into()),
            recorded_at_utc,
        )
    }

    pub fn read_only_clean_attach(
        pool_id: PoolId,
        reason: impl Into<String>,
        recorded_at_utc: impl Into<String>,
    ) -> Self {
        Self::new(
            pool_id,
            PoolStateMarkerKind::ReadOnlyImport,
            Some(PoolState::Clean),
            PoolState::ReadOnly,
            Some(ImportMode::ReadOnly),
            Some(reason.into()),
            recorded_at_utc,
        )
    }

    pub fn repair_import(
        pool_id: PoolId,
        reason: impl Into<String>,
        recorded_at_utc: impl Into<String>,
    ) -> Self {
        Self::new(
            pool_id,
            PoolStateMarkerKind::RepairImport,
            Some(PoolState::Dirty),
            PoolState::Repairing,
            Some(ImportMode::Repair),
            Some(reason.into()),
            recorded_at_utc,
        )
    }

    pub fn force_read_write_import(
        pool_id: PoolId,
        reason: impl Into<String>,
        recorded_at_utc: impl Into<String>,
    ) -> Self {
        Self::new(
            pool_id,
            PoolStateMarkerKind::ForceReadWriteImport,
            Some(PoolState::Dirty),
            PoolState::Dirty,
            Some(ImportMode::ForceReadWrite),
            Some(reason.into()),
            recorded_at_utc,
        )
    }

    fn new(
        pool_id: PoolId,
        marker_kind: PoolStateMarkerKind,
        previous_state: Option<PoolState>,
        next_state: PoolState,
        import_mode: Option<ImportMode>,
        reason: Option<String>,
        recorded_at_utc: impl Into<String>,
    ) -> Self {
        Self {
            pool_id,
            marker_kind,
            previous_state,
            next_state,
            import_mode,
            reason,
            recorded_at_utc: recorded_at_utc.into(),
        }
    }
}

pub fn record_pool_state_marker(
    connection: &Connection,
    marker: &PoolStateMarker,
) -> Result<(), rusqlite::Error> {
    connection.execute(
        "INSERT INTO pool_state_markers (
            pool_id,
            marker_kind,
            previous_state,
            next_state,
            import_mode,
            reason,
            recorded_at_utc
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            marker.pool_id.as_str(),
            marker.marker_kind.name(),
            marker.previous_state.map(state_name),
            state_name(marker.next_state),
            marker.import_mode.map(import_mode_name),
            marker.reason,
            marker.recorded_at_utc
        ],
    )?;
    connection.execute(
        "UPDATE pools
         SET state = ?1,
             updated_at_utc = ?2
         WHERE pool_id = ?3",
        params![
            state_name(marker.next_state),
            marker.recorded_at_utc,
            marker.pool_id.as_str()
        ],
    )?;

    Ok(())
}

pub fn record_pool_state_marker_at(
    live_sqlite_path: impl AsRef<Path>,
    marker: &PoolStateMarker,
) -> Result<(), rusqlite::Error> {
    let connection = Connection::open(live_sqlite_path)?;

    record_pool_state_marker(&connection, marker)
}

fn state_name(state: PoolState) -> &'static str {
    match state {
        PoolState::New => "New",
        PoolState::Clean => "Clean",
        PoolState::Dirty => "Dirty",
        PoolState::ReadOnly => "ReadOnly",
        PoolState::Repairing => "Repairing",
        PoolState::Degraded => "Degraded",
    }
}

fn import_mode_name(import_mode: ImportMode) -> &'static str {
    match import_mode {
        ImportMode::ReadWrite => "ReadWrite",
        ImportMode::ReadOnly => "ReadOnly",
        ImportMode::Repair => "Repair",
        ImportMode::ForceReadWrite => "ForceReadWrite",
    }
}

#[cfg(test)]
mod tests {
    use super::{record_pool_state_marker, PoolStateMarker, PoolStateMarkerKind};
    use crate::initialize::{initialize_pool, PoolInitOptions};
    use dasobjectstore_core::ids::PoolId;
    use rusqlite::Connection;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn records_dirty_attach_marker_and_updates_pool_state() {
        let root = temp_root("dirty-attach-marker");
        let init = initialize_pool(&PoolInitOptions::new(
            &root,
            PoolId::new("pool-a").expect("pool id"),
            "2026-01-02T00:00:00Z",
        ))
        .expect("pool initializes");
        let connection = Connection::open(&init.live_sqlite_path).expect("open live sqlite");
        let marker = PoolStateMarker::dirty_attach(
            PoolId::new("pool-a").expect("pool id"),
            "2026-01-03T00:00:00Z",
        );

        record_pool_state_marker(&connection, &marker).expect("marker records");

        assert_eq!(pool_state(&connection), "Dirty");
        assert_eq!(marker_kind(&connection), "dirty_attach");

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn defines_import_marker_modes() {
        let pool_id = PoolId::new("pool-a").expect("pool id");

        let read_only =
            PoolStateMarker::read_only_import(pool_id.clone(), "unclean detach", "2026-01-03");
        let read_only_clean_attach = PoolStateMarker::read_only_clean_attach(
            pool_id.clone(),
            "portable clean attach",
            "2026-01-03",
        );
        let repair =
            PoolStateMarker::repair_import(pool_id.clone(), "checksum repair", "2026-01-03");
        let force =
            PoolStateMarker::force_read_write_import(pool_id, "operator override", "2026-01-03");

        assert_eq!(read_only.marker_kind, PoolStateMarkerKind::ReadOnlyImport);
        assert_eq!(
            read_only_clean_attach.marker_kind,
            PoolStateMarkerKind::ReadOnlyImport
        );
        assert_eq!(
            read_only_clean_attach.previous_state,
            Some(dasobjectstore_core::lifecycle::PoolState::Clean)
        );
        assert_eq!(repair.marker_kind, PoolStateMarkerKind::RepairImport);
        assert_eq!(force.marker_kind, PoolStateMarkerKind::ForceReadWriteImport);
    }

    fn pool_state(connection: &Connection) -> String {
        connection
            .query_row(
                "SELECT state FROM pools WHERE pool_id = 'pool-a'",
                [],
                |row| row.get(0),
            )
            .expect("pool state")
    }

    fn marker_kind(connection: &Connection) -> String {
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
