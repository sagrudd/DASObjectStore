//! Durable catalogue transaction for the bounded folder profile.

use dasobjectstore_core::backend::{BackendError, BackendObjectKey, BackendObjectRecord};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

pub const FOLDER_CATALOGUE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct FolderCatalogueSnapshot {
    schema_version: u32,
    store_id: String,
    records: BTreeMap<String, BackendObjectRecord>,
}

#[derive(Debug)]
pub struct FolderCatalogue {
    path: PathBuf,
    store_id: String,
    records: BTreeMap<String, BackendObjectRecord>,
}

impl FolderCatalogue {
    pub fn open(
        path: impl Into<PathBuf>,
        store_id: impl Into<String>,
    ) -> Result<Self, BackendError> {
        let path = path.into();
        let store_id = store_id.into();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
        if !path.exists() {
            return Ok(Self {
                path,
                store_id,
                records: BTreeMap::new(),
            });
        }
        let file = File::open(&path).map_err(io_error)?;
        let snapshot: FolderCatalogueSnapshot = serde_json::from_reader(file).map_err(|error| {
            BackendError::InvalidRequest(format!("folder catalogue JSON is invalid: {error}"))
        })?;
        if snapshot.schema_version != FOLDER_CATALOGUE_SCHEMA_VERSION {
            return Err(BackendError::InvalidRequest(format!(
                "unsupported folder catalogue schema {}",
                snapshot.schema_version
            )));
        }
        if snapshot.store_id != store_id {
            return Err(BackendError::InvalidRequest(
                "folder catalogue belongs to a different ObjectStore".to_string(),
            ));
        }
        Ok(Self {
            path,
            store_id,
            records: snapshot.records,
        })
    }

    pub fn records(&self) -> Vec<BackendObjectRecord> {
        self.records.values().cloned().collect()
    }

    pub fn commit_records(
        &mut self,
        records: impl IntoIterator<Item = BackendObjectRecord>,
    ) -> Result<(), BackendError> {
        let mut next = self.records.clone();
        for record in records {
            let key = catalogue_key(&record.key);
            if let Some(existing) = next.get(&key) {
                if existing != &record {
                    return Err(BackendError::InvalidRequest(format!(
                        "folder catalogue entry conflicts for {key}"
                    )));
                }
            } else {
                next.insert(key, record);
            }
        }
        self.persist(&next)?;
        self.records = next;
        Ok(())
    }

    fn persist(&self, records: &BTreeMap<String, BackendObjectRecord>) -> Result<(), BackendError> {
        let parent = self.path.parent().ok_or_else(|| {
            BackendError::InvalidRequest("folder catalogue path has no parent".to_string())
        })?;
        let snapshot = FolderCatalogueSnapshot {
            schema_version: FOLDER_CATALOGUE_SCHEMA_VERSION,
            store_id: self.store_id.clone(),
            records: records.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&snapshot).map_err(|error| {
            BackendError::InvalidRequest(format!("folder catalogue encode failed: {error}"))
        })?;
        let temporary = parent.join(format!(
            ".{}.tmp-{}",
            self.path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("catalogue"),
            std::process::id()
        ));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(io_error)?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(io_error)?;
        drop(file);
        if let Err(error) = fs::rename(&temporary, &self.path) {
            let _ = fs::remove_file(&temporary);
            return Err(io_error(error));
        }
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(io_error)
    }
}

fn catalogue_key(key: &BackendObjectKey) -> String {
    format!("{}@{}", key.object_id, key.version)
}

fn io_error(error: std::io::Error) -> BackendError {
    BackendError::Io(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::FolderCatalogue;
    use dasobjectstore_core::backend::{BackendObjectKey, BackendObjectRecord};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn root() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let root = std::env::var_os("DASOBJECTSTORE_CODEX_VALIDATION_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("dasobjectstore-codex-validation"));
        root.join(format!(
            "folder-catalogue-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn record() -> BackendObjectRecord {
        BackendObjectRecord {
            key: BackendObjectKey {
                object_id: "incoming/data.txt".to_string(),
                version: 1,
            },
            size_bytes: 4,
            checksum: "sha256:abcd".to_string(),
            location: ".dasobjectstore/objects/incoming/data.txt".to_string(),
        }
    }

    #[test]
    fn catalogue_commit_is_idempotent_and_survives_restart() {
        let root = root();
        let path = root.join("catalogue.json");
        let mut catalogue = FolderCatalogue::open(&path, "codex").expect("catalogue opens");
        catalogue
            .commit_records([record()])
            .expect("record commits");
        catalogue
            .commit_records([record()])
            .expect("duplicate commit is idempotent");
        assert_eq!(catalogue.records().len(), 1);

        let restarted = FolderCatalogue::open(&path, "codex").expect("catalogue reloads");
        assert_eq!(restarted.records(), vec![record()]);
        let _ = std::fs::remove_dir_all(root);
    }
}
