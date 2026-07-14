//! Atomic persistence for daemon-owned capacity reservation ledgers.

use dasobjectstore_core::store::{
    CapacityLedgerError, CapacityReservationLedger, CapacityReservationLedgerSnapshot,
    CapacityReservationLedgerSnapshotV2,
};
use std::fmt::{self, Display};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug)]
pub enum CapacityLedgerPersistenceError {
    Io { path: PathBuf, message: String },
    Serialize { path: PathBuf, message: String },
    Deserialize { path: PathBuf, message: String },
    Ledger(CapacityLedgerError),
}

impl Display for CapacityLedgerPersistenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, message } => write!(
                formatter,
                "capacity ledger I/O {}: {message}",
                path.display()
            ),
            Self::Serialize { path, message } => write!(
                formatter,
                "capacity ledger serialization {}: {message}",
                path.display()
            ),
            Self::Deserialize { path, message } => write!(
                formatter,
                "capacity ledger deserialization {}: {message}",
                path.display()
            ),
            Self::Ledger(error) => write!(formatter, "capacity ledger validation: {error:?}"),
        }
    }
}

impl std::error::Error for CapacityLedgerPersistenceError {}

pub fn save_capacity_ledger(
    path: impl AsRef<Path>,
    ledger: &CapacityReservationLedger,
) -> Result<(), CapacityLedgerPersistenceError> {
    let path = path.as_ref();
    let parent = path
        .parent()
        .ok_or_else(|| CapacityLedgerPersistenceError::Io {
            path: path.to_path_buf(),
            message: "capacity ledger path has no parent".to_string(),
        })?;
    fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
    let snapshot = ledger.snapshot_with_expiry();
    let bytes = serde_json::to_vec_pretty(&snapshot).map_err(|error| {
        CapacityLedgerPersistenceError::Serialize {
            path: path.to_path_buf(),
            message: error.to_string(),
        }
    })?;
    let temporary = parent.join(format!(
        ".{}.tmp-{}-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("ledger"),
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| io_error(&temporary, error))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| io_error(&temporary, error))?;
    drop(file);
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(io_error(path, error));
    }
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| io_error(parent, error))
}

pub fn load_capacity_ledger(
    path: impl AsRef<Path>,
) -> Result<CapacityReservationLedger, CapacityLedgerPersistenceError> {
    let path = path.as_ref();
    let file = File::open(path).map_err(|error| io_error(path, error))?;
    let value: serde_json::Value = serde_json::from_reader(file).map_err(|error| {
        CapacityLedgerPersistenceError::Deserialize {
            path: path.to_path_buf(),
            message: error.to_string(),
        }
    })?;
    let schema_version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .and_then(|version| u32::try_from(version).ok())
        .ok_or_else(|| CapacityLedgerPersistenceError::Deserialize {
            path: path.to_path_buf(),
            message: "capacity ledger schema_version is missing or invalid".to_string(),
        })?;
    if schema_version == dasobjectstore_core::store::CAPACITY_LEDGER_EXPIRY_SNAPSHOT_SCHEMA_VERSION
    {
        let snapshot: CapacityReservationLedgerSnapshotV2 =
            serde_json::from_value(value).map_err(|error| {
                CapacityLedgerPersistenceError::Deserialize {
                    path: path.to_path_buf(),
                    message: error.to_string(),
                }
            })?;
        return CapacityReservationLedger::from_snapshot_with_expiry(snapshot)
            .map_err(CapacityLedgerPersistenceError::Ledger);
    }
    let snapshot: CapacityReservationLedgerSnapshot =
        serde_json::from_value(value).map_err(|error| {
            CapacityLedgerPersistenceError::Deserialize {
                path: path.to_path_buf(),
                message: error.to_string(),
            }
        })?;
    CapacityReservationLedger::from_snapshot(snapshot)
        .map_err(CapacityLedgerPersistenceError::Ledger)
}

fn io_error(path: &Path, error: std::io::Error) -> CapacityLedgerPersistenceError {
    CapacityLedgerPersistenceError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{load_capacity_ledger, save_capacity_ledger, CapacityLedgerPersistenceError};
    use dasobjectstore_core::store::{
        CapacityPolicy, CapacityReservationLedger, CapacityReservationLedgerSnapshot,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn root(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let parent = std::env::var_os("DASOBJECTSTORE_CODEX_VALIDATION_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("dasobjectstore-codex-validation"));
        parent.join(format!(
            "capacity-ledger-{label}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn persists_and_restores_ledger_atomically() {
        let root = root("roundtrip");
        let path = root.join("state/ledger.json");
        let mut ledger = CapacityReservationLedger::new(CapacityPolicy::bounded(1_000, 10), 20)
            .expect("policy valid");
        ledger.reserve("upload-1", 100).expect("reservation fits");

        save_capacity_ledger(&path, &ledger).expect("ledger saves");
        let restored = load_capacity_ledger(&path).expect("ledger loads");

        assert_eq!(restored.used_bytes(), 20);
        assert_eq!(restored.reserved_bytes(), 100);
        assert_eq!(restored.reservation_bytes("upload-1"), Some(100));
        assert!(!root.join("state/.ledger.json.tmp").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn repeated_checkpoint_replacement_leaves_no_temporary_artifacts() {
        let root = root("repeated");
        let path = root.join("state/ledger.json");
        let ledger = CapacityReservationLedger::new(CapacityPolicy::bounded(1_000, 10), 0)
            .expect("policy valid");

        save_capacity_ledger(&path, &ledger).expect("first save");
        save_capacity_ledger(&path, &ledger).expect("second save");

        let entries = fs::read_dir(path.parent().expect("ledger parent"))
            .expect("state directory")
            .map(|entry| entry.expect("directory entry").file_name())
            .collect::<Vec<_>>();
        assert_eq!(entries, vec![std::ffi::OsString::from("ledger.json")]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_corrupt_snapshot_without_mutating_previous_state() {
        let root = root("corrupt");
        let path = root.join("ledger.json");
        fs::create_dir_all(&root).expect("root creates");
        fs::write(&path, b"not-json").expect("corrupt state writes");

        let error = load_capacity_ledger(&path).expect_err("corrupt state rejects");
        assert!(matches!(
            error,
            CapacityLedgerPersistenceError::Deserialize { .. }
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn loads_schema_v1_snapshot_without_expiring_unknown_age_reservations() {
        let root = root("legacy");
        let path = root.join("ledger.json");
        fs::create_dir_all(&root).expect("root creates");
        let snapshot = CapacityReservationLedgerSnapshot {
            schema_version: dasobjectstore_core::store::CAPACITY_LEDGER_SNAPSHOT_SCHEMA_VERSION,
            policy: CapacityPolicy::bounded(1_000, 0),
            used_bytes: 10,
            reservations: [("legacy".to_string(), 20)].into_iter().collect(),
        };
        fs::write(
            &path,
            serde_json::to_vec(&snapshot).expect("legacy snapshot serializes"),
        )
        .expect("legacy snapshot writes");

        let restored = load_capacity_ledger(&path).expect("legacy snapshot loads");
        assert_eq!(restored.reservation_bytes("legacy"), Some(20));
        assert_eq!(restored.reserved_bytes(), 20);
        let _ = fs::remove_dir_all(root);
    }
}
