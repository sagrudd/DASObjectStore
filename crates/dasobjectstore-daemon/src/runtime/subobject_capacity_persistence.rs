//! Atomic persistence for daemon-owned SubObject capacity ledgers.

use dasobjectstore_core::subobject_capacity::{
    SubObjectCapacityError, SubObjectCapacityLedger, SubObjectCapacityLedgerSnapshot,
};
use std::fmt::{self, Display};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug)]
pub enum SubObjectCapacityLedgerPersistenceError {
    Io { path: PathBuf, message: String },
    Serialize { path: PathBuf, message: String },
    Deserialize { path: PathBuf, message: String },
    Ledger(SubObjectCapacityError),
}

impl Display for SubObjectCapacityLedgerPersistenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, message } => write!(
                formatter,
                "SubObject capacity ledger I/O {}: {message}",
                path.display()
            ),
            Self::Serialize { path, message } => write!(
                formatter,
                "SubObject capacity ledger serialization {}: {message}",
                path.display()
            ),
            Self::Deserialize { path, message } => write!(
                formatter,
                "SubObject capacity ledger deserialization {}: {message}",
                path.display()
            ),
            Self::Ledger(error) => {
                write!(formatter, "SubObject capacity ledger validation: {error}")
            }
        }
    }
}

impl std::error::Error for SubObjectCapacityLedgerPersistenceError {}

pub fn save_subobject_capacity_ledger(
    path: impl AsRef<Path>,
    ledger: &SubObjectCapacityLedger,
) -> Result<(), SubObjectCapacityLedgerPersistenceError> {
    let path = path.as_ref();
    let parent = path
        .parent()
        .ok_or_else(|| io_error(path, "capacity ledger path has no parent"))?;
    fs::create_dir_all(parent).map_err(|error| io_error(parent, error.to_string()))?;
    let bytes = serde_json::to_vec_pretty(&ledger.snapshot()).map_err(|error| {
        SubObjectCapacityLedgerPersistenceError::Serialize {
            path: path.to_path_buf(),
            message: error.to_string(),
        }
    })?;
    let temporary = parent.join(format!(
        ".{}.tmp-{}-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("subobject-ledger"),
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
        .map_err(|error| io_error(&temporary, error.to_string()))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| io_error(&temporary, error.to_string()))?;
    drop(file);
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(io_error(path, error.to_string()));
    }
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| io_error(parent, error.to_string()))
}

pub fn load_subobject_capacity_ledger(
    path: impl AsRef<Path>,
) -> Result<SubObjectCapacityLedger, SubObjectCapacityLedgerPersistenceError> {
    let path = path.as_ref();
    let file = File::open(path).map_err(|error| io_error(path, error.to_string()))?;
    let snapshot: SubObjectCapacityLedgerSnapshot =
        serde_json::from_reader(file).map_err(|error| {
            SubObjectCapacityLedgerPersistenceError::Deserialize {
                path: path.to_path_buf(),
                message: error.to_string(),
            }
        })?;
    SubObjectCapacityLedger::from_snapshot(snapshot)
        .map_err(SubObjectCapacityLedgerPersistenceError::Ledger)
}

fn io_error(path: &Path, message: impl Into<String>) -> SubObjectCapacityLedgerPersistenceError {
    SubObjectCapacityLedgerPersistenceError::Io {
        path: path.to_path_buf(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        load_subobject_capacity_ledger, save_subobject_capacity_ledger,
        SubObjectCapacityLedgerPersistenceError,
    };
    use dasobjectstore_core::store::CapacityPolicy;
    use dasobjectstore_core::subobject_capacity::SubObjectCapacityLedger;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn root(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let parent = std::env::var_os("DASOBJECTSTORE_CODEX_VALIDATION_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("dasobjectstore-codex-validation"));
        parent.join(format!(
            "subobject-capacity-{label}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn ledger() -> SubObjectCapacityLedger {
        let mut ledger = SubObjectCapacityLedger::new(CapacityPolicy::bounded(1_000, 10), 100)
            .expect("parent policy is valid");
        ledger
            .add_child("child-a", CapacityPolicy::bounded(200, 0), 20)
            .expect("child policy is valid");
        ledger
    }

    #[test]
    fn persists_and_restores_parent_child_links_atomically() {
        let root = root("roundtrip");
        let path = root.join("state/ledger.json");
        let mut ledger = ledger();
        ledger
            .reserve("child-a", "upload-1", 50)
            .expect("reservation fits");

        save_subobject_capacity_ledger(&path, &ledger).expect("ledger saves");
        let restored = load_subobject_capacity_ledger(&path).expect("ledger loads");

        assert_eq!(restored.parent().used_bytes(), 100);
        assert_eq!(restored.parent().reserved_bytes(), 50);
        assert_eq!(restored.child("child-a").unwrap().reserved_bytes(), 50);
        assert_eq!(
            restored
                .child("child-a")
                .unwrap()
                .reservation_bytes("upload-1"),
            Some(50)
        );
        let entries = fs::read_dir(path.parent().expect("ledger parent"))
            .expect("state directory")
            .map(|entry| entry.expect("directory entry").file_name())
            .collect::<Vec<_>>();
        assert_eq!(entries, vec![std::ffi::OsString::from("ledger.json")]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_corrupt_or_inconsistent_snapshots_without_a_usable_ledger() {
        let root = root("invalid");
        let path = root.join("ledger.json");
        fs::create_dir_all(&root).expect("root creates");
        fs::write(&path, b"not-json").expect("corrupt state writes");
        assert!(matches!(
            load_subobject_capacity_ledger(&path),
            Err(SubObjectCapacityLedgerPersistenceError::Deserialize { .. })
        ));

        let mut snapshot = ledger().snapshot();
        snapshot
            .parent
            .reservations
            .insert("unlinked".to_string(), 1);
        fs::write(
            &path,
            serde_json::to_vec(&snapshot).expect("snapshot serializes"),
        )
        .expect("invalid snapshot writes");
        assert!(matches!(
            load_subobject_capacity_ledger(&path),
            Err(SubObjectCapacityLedgerPersistenceError::Ledger(_))
        ));
        let _ = fs::remove_dir_all(root);
    }
}
