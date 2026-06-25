use dasobjectstore_core::ids::DiskId;
use dasobjectstore_core::lifecycle::DiskState;
use dasobjectstore_core::risk::{
    ActionConfirmation, RiskGate, RiskGateError, RiskPolicy, RiskyOperation,
};
use rusqlite::{Connection, OptionalExtension};
use std::fmt::{self, Display};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiskRetirementReport {
    pub disk_id: DiskId,
    pub live_sqlite_path: PathBuf,
    pub previous_state: String,
    pub next_state: DiskState,
    pub updated_at_utc: String,
}

#[derive(Debug)]
pub enum DiskRetirementError {
    Sqlite(rusqlite::Error),
    DiskNotFound { disk_id: DiskId },
    TerminalState { disk_id: DiskId, state: String },
    RiskGate(RiskGateError),
}

impl Display for DiskRetirementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(err) => write!(formatter, "failed to update disk metadata: {err}"),
            Self::DiskNotFound { disk_id } => {
                write!(formatter, "disk {disk_id} does not exist in live metadata")
            }
            Self::TerminalState { disk_id, state } => write!(
                formatter,
                "disk {disk_id} cannot be retired from terminal state {state}"
            ),
            Self::RiskGate(err) => write!(formatter, "{err}"),
        }
    }
}

impl std::error::Error for DiskRetirementError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sqlite(err) => Some(err),
            Self::RiskGate(err) => Some(err),
            Self::DiskNotFound { .. } | Self::TerminalState { .. } => None,
        }
    }
}

impl From<rusqlite::Error> for DiskRetirementError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Sqlite(err)
    }
}

pub fn request_disk_retirement(
    live_sqlite_path: impl AsRef<Path>,
    disk_id: &DiskId,
    updated_at_utc: impl Into<String>,
) -> Result<DiskRetirementReport, DiskRetirementError> {
    let live_sqlite_path = live_sqlite_path.as_ref();
    let updated_at_utc = updated_at_utc.into();
    let connection = Connection::open(live_sqlite_path)?;
    let previous_state = read_disk_state(&connection, disk_id)?.ok_or_else(|| {
        DiskRetirementError::DiskNotFound {
            disk_id: disk_id.clone(),
        }
    })?;

    if is_terminal_state(&previous_state) {
        return Err(DiskRetirementError::TerminalState {
            disk_id: disk_id.clone(),
            state: previous_state,
        });
    }

    connection.execute(
        "UPDATE disks
         SET state = ?1,
             updated_at_utc = ?2
         WHERE disk_id = ?3",
        (
            format!("{:?}", DiskState::Draining),
            updated_at_utc.as_str(),
            disk_id.as_str(),
        ),
    )?;

    Ok(DiskRetirementReport {
        disk_id: disk_id.clone(),
        live_sqlite_path: live_sqlite_path.to_path_buf(),
        previous_state,
        next_state: DiskState::Draining,
        updated_at_utc,
    })
}

pub fn force_retire_disk(
    live_sqlite_path: impl AsRef<Path>,
    disk_id: &DiskId,
    updated_at_utc: impl Into<String>,
    risk_policy: RiskPolicy,
    confirmation: &ActionConfirmation,
) -> Result<DiskRetirementReport, DiskRetirementError> {
    RiskGate::new(risk_policy)
        .evaluate(RiskyOperation::ForceRetire, confirmation)
        .map_err(DiskRetirementError::RiskGate)?;

    let live_sqlite_path = live_sqlite_path.as_ref();
    let updated_at_utc = updated_at_utc.into();
    let connection = Connection::open(live_sqlite_path)?;
    let previous_state = read_disk_state(&connection, disk_id)?.ok_or_else(|| {
        DiskRetirementError::DiskNotFound {
            disk_id: disk_id.clone(),
        }
    })?;

    if previous_state == "Retired" {
        return Err(DiskRetirementError::TerminalState {
            disk_id: disk_id.clone(),
            state: previous_state,
        });
    }

    connection.execute(
        "UPDATE disks
         SET state = ?1,
             updated_at_utc = ?2
         WHERE disk_id = ?3",
        (
            format!("{:?}", DiskState::Retired),
            updated_at_utc.as_str(),
            disk_id.as_str(),
        ),
    )?;

    Ok(DiskRetirementReport {
        disk_id: disk_id.clone(),
        live_sqlite_path: live_sqlite_path.to_path_buf(),
        previous_state,
        next_state: DiskState::Retired,
        updated_at_utc,
    })
}

fn read_disk_state(
    connection: &Connection,
    disk_id: &DiskId,
) -> Result<Option<String>, rusqlite::Error> {
    connection
        .query_row(
            "SELECT state FROM disks WHERE disk_id = ?1",
            [disk_id.as_str()],
            |row| row.get(0),
        )
        .optional()
}

fn is_terminal_state(state: &str) -> bool {
    matches!(state, "Retired" | "Failed")
}

#[cfg(test)]
mod tests {
    use super::{force_retire_disk, request_disk_retirement, DiskRetirementError};
    use crate::schema::LIVE_SCHEMA_SQL;
    use dasobjectstore_core::ids::DiskId;
    use dasobjectstore_core::lifecycle::DiskState;
    use dasobjectstore_core::risk::{
        ActionConfirmation, RiskGateError, RiskPolicy, RiskyOperation,
    };
    use rusqlite::Connection;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn requests_disk_retirement_by_marking_disk_draining() {
        let root = temp_root("disk-retire");
        fs::create_dir_all(&root).expect("create temp root");
        let live_sqlite_path = root.join("live.sqlite");
        let connection = create_live_metadata(&live_sqlite_path);
        insert_disk(&connection, "disk-a", "Healthy");

        let report = request_disk_retirement(
            &live_sqlite_path,
            &DiskId::new("disk-a").expect("disk id"),
            "2026-01-02T00:00:00Z",
        )
        .expect("retirement requested");

        assert_eq!(report.disk_id.as_str(), "disk-a");
        assert_eq!(report.previous_state, "Healthy");
        assert_eq!(report.next_state, DiskState::Draining);
        assert_eq!(read_state(&connection, "disk-a"), "Draining");

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn rejects_missing_disk_retirement_request() {
        let root = temp_root("disk-retire-missing");
        fs::create_dir_all(&root).expect("create temp root");
        let live_sqlite_path = root.join("live.sqlite");
        let _connection = create_live_metadata(&live_sqlite_path);

        let err = request_disk_retirement(
            &live_sqlite_path,
            &DiskId::new("missing-disk").expect("disk id"),
            "2026-01-02T00:00:00Z",
        )
        .expect_err("missing disk fails");

        assert!(matches!(err, DiskRetirementError::DiskNotFound { .. }));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn rejects_retirement_from_terminal_disk_state() {
        let root = temp_root("disk-retire-terminal");
        fs::create_dir_all(&root).expect("create temp root");
        let live_sqlite_path = root.join("live.sqlite");
        let connection = create_live_metadata(&live_sqlite_path);
        insert_disk(&connection, "disk-a", "Retired");

        let err = request_disk_retirement(
            &live_sqlite_path,
            &DiskId::new("disk-a").expect("disk id"),
            "2026-01-02T00:00:00Z",
        )
        .expect_err("terminal disk state fails");

        assert!(matches!(err, DiskRetirementError::TerminalState { .. }));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn force_retire_requires_policy_allowance() {
        let root = temp_root("disk-force-retire-denied");
        fs::create_dir_all(&root).expect("create temp root");
        let live_sqlite_path = root.join("live.sqlite");
        let connection = create_live_metadata(&live_sqlite_path);
        insert_disk(&connection, "disk-a", "Healthy");

        let err = force_retire_disk(
            &live_sqlite_path,
            &DiskId::new("disk-a").expect("disk id"),
            "2026-01-02T00:00:00Z",
            RiskPolicy::default(),
            &ActionConfirmation::for_operation(RiskyOperation::ForceRetire),
        )
        .expect_err("policy denial fails");

        assert!(matches!(
            err,
            DiskRetirementError::RiskGate(RiskGateError::PolicyDoesNotAllow { .. })
        ));
        assert_eq!(read_state(&connection, "disk-a"), "Healthy");

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn force_retire_requires_matching_confirmation() {
        let root = temp_root("disk-force-retire-confirmation");
        fs::create_dir_all(&root).expect("create temp root");
        let live_sqlite_path = root.join("live.sqlite");
        let connection = create_live_metadata(&live_sqlite_path);
        insert_disk(&connection, "disk-a", "Healthy");

        let err = force_retire_disk(
            &live_sqlite_path,
            &DiskId::new("disk-a").expect("disk id"),
            "2026-01-02T00:00:00Z",
            RiskPolicy {
                allow_force_retire: true,
                ..RiskPolicy::default()
            },
            &ActionConfirmation::new("wrong confirmation"),
        )
        .expect_err("confirmation mismatch fails");

        assert!(matches!(
            err,
            DiskRetirementError::RiskGate(RiskGateError::ConfirmationMismatch { .. })
        ));
        assert_eq!(read_state(&connection, "disk-a"), "Healthy");

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn force_retire_marks_disk_retired_after_risk_gate() {
        let root = temp_root("disk-force-retire");
        fs::create_dir_all(&root).expect("create temp root");
        let live_sqlite_path = root.join("live.sqlite");
        let connection = create_live_metadata(&live_sqlite_path);
        insert_disk(&connection, "disk-a", "Healthy");

        let report = force_retire_disk(
            &live_sqlite_path,
            &DiskId::new("disk-a").expect("disk id"),
            "2026-01-02T00:00:00Z",
            RiskPolicy {
                allow_force_retire: true,
                ..RiskPolicy::default()
            },
            &ActionConfirmation::for_operation(RiskyOperation::ForceRetire),
        )
        .expect("force retire succeeds");

        assert_eq!(report.disk_id.as_str(), "disk-a");
        assert_eq!(report.previous_state, "Healthy");
        assert_eq!(report.next_state, DiskState::Retired);
        assert_eq!(read_state(&connection, "disk-a"), "Retired");

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    fn create_live_metadata(path: &PathBuf) -> Connection {
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
            .expect("insert pool");
        connection
    }

    fn insert_disk(connection: &Connection, disk_id: &str, state: &str) {
        connection
            .execute(
                "INSERT INTO disks (
                    disk_id, pool_id, role, state, created_at_utc, updated_at_utc
                 ) VALUES (?1, 'pool-a', 'hdd_capacity', ?2, ?3, ?3)",
                (disk_id, state, "2026-01-01T00:00:00Z"),
            )
            .expect("insert disk");
    }

    fn read_state(connection: &Connection, disk_id: &str) -> String {
        connection
            .query_row(
                "SELECT state FROM disks WHERE disk_id = ?1",
                [disk_id],
                |row| row.get(0),
            )
            .expect("read disk state")
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
