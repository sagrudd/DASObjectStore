//! Durable catalogue transaction for the bounded folder profile.

use dasobjectstore_core::backend::{
    BackendError, BackendObjectKey, BackendObjectRecord, ObjectCatalogueAuthority,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub const FOLDER_CATALOGUE_SCHEMA_VERSION: u32 = 1;

const MAX_BROWSER_PAGE_SIZE: usize = 1_000;

/// Read-only query for the profile-neutral folder catalogue view.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FolderCatalogueBrowserQuery {
    pub prefix: Option<String>,
    pub search: Option<String>,
    pub offset: usize,
    pub limit: usize,
}

/// Authoritative folder fields suitable for a future profile-aware browser
/// adapter. Appliance-only metadata is explicit `None`; callers must not
/// infer an HDD placement, lifecycle state, or object type from a folder row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FolderCatalogueBrowserEntry {
    pub key: BackendObjectKey,
    pub size_bytes: u64,
    pub checksum: String,
    pub location: String,
    pub object_type: Option<String>,
    pub lifecycle_state: Option<String>,
    pub placement: Option<String>,
}

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

    /// Query the private folder catalogue without walking user files or
    /// mutating metadata. Results are deterministic by object key/version and
    /// are intentionally not an ObjectBrowser response: profile-specific
    /// placement and lifecycle fields remain unknown.
    pub fn browser_entries(
        &self,
        query: &FolderCatalogueBrowserQuery,
    ) -> Result<Vec<FolderCatalogueBrowserEntry>, BackendError> {
        let limit = if query.limit == 0 {
            MAX_BROWSER_PAGE_SIZE
        } else {
            query.limit
        };
        if limit > MAX_BROWSER_PAGE_SIZE {
            return Err(BackendError::InvalidRequest(format!(
                "folder catalogue page limit exceeds {MAX_BROWSER_PAGE_SIZE}"
            )));
        }
        let prefix = query.prefix.as_deref().unwrap_or_default();
        let search = query.search.as_deref().unwrap_or_default();
        Ok(self
            .records()
            .into_iter()
            .filter(|record| record.key.object_id.starts_with(prefix))
            .filter(|record| search.is_empty() || record.key.object_id.contains(search))
            .skip(query.offset)
            .take(limit)
            .map(FolderCatalogueBrowserEntry::from)
            .collect())
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

    pub fn remove(&mut self, key: &BackendObjectKey) -> Result<(), BackendError> {
        let mut next = self.records.clone();
        next.remove(&catalogue_key(key));
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
            ".{}.tmp-{}-{}",
            self.path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("catalogue"),
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

impl ObjectCatalogueAuthority for FolderCatalogue {
    fn records(&self) -> Vec<BackendObjectRecord> {
        self.records()
    }

    fn commit_batch(&mut self, records: &[BackendObjectRecord]) -> Result<(), BackendError> {
        self.commit_records(records.iter().cloned())
    }

    fn remove_record(&mut self, key: &BackendObjectKey) -> Result<(), BackendError> {
        self.remove(key)
    }
}

impl From<BackendObjectRecord> for FolderCatalogueBrowserEntry {
    fn from(record: BackendObjectRecord) -> Self {
        Self {
            key: record.key,
            size_bytes: record.size_bytes,
            checksum: record.checksum,
            location: record.location,
            object_type: None,
            lifecycle_state: None,
            placement: None,
        }
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
    use super::{FolderCatalogue, FolderCatalogueBrowserQuery};
    use dasobjectstore_core::backend::{
        BackendObjectKey, BackendObjectRecord, ObjectCatalogueAuthority,
    };
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

    #[test]
    fn conflicting_commit_preserves_the_previous_snapshot() {
        let root = root();
        let path = root.join("catalogue.json");
        let mut catalogue = FolderCatalogue::open(&path, "codex").expect("catalogue opens");
        catalogue
            .commit_records([record()])
            .expect("record commits");
        let mut conflicting = record();
        conflicting.checksum = "sha256:different".to_string();
        assert!(catalogue.commit_records([conflicting]).is_err());

        let restarted = FolderCatalogue::open(&path, "codex").expect("catalogue reloads");
        assert_eq!(restarted.records(), vec![record()]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn malformed_schema_and_store_identity_fail_closed() {
        let root = root();
        let path = root.join("catalogue.json");
        std::fs::create_dir_all(&root).expect("root creates");
        std::fs::write(&path, b"not-json").expect("malformed catalogue writes");
        assert!(FolderCatalogue::open(&path, "codex").is_err());

        std::fs::write(
            &path,
            serde_json::json!({
                "schema_version": 99,
                "store_id": "codex",
                "records": {}
            })
            .to_string(),
        )
        .expect("future catalogue writes");
        assert!(FolderCatalogue::open(&path, "codex").is_err());

        std::fs::write(
            &path,
            serde_json::json!({
                "schema_version": 1,
                "store_id": "other",
                "records": {}
            })
            .to_string(),
        )
        .expect("wrong-store catalogue writes");
        assert!(FolderCatalogue::open(&path, "codex").is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn browser_entries_are_authoritative_and_profile_fields_remain_unknown() {
        let root = root();
        let path = root.join("catalogue.json");
        let mut catalogue = FolderCatalogue::open(&path, "codex").expect("catalogue opens");
        catalogue
            .commit_records([
                record(),
                BackendObjectRecord {
                    key: BackendObjectKey {
                        object_id: "incoming/second.dat".to_string(),
                        version: 2,
                    },
                    size_bytes: 7,
                    checksum: "sha256:efgh".to_string(),
                    location: ".dasobjectstore/objects/incoming/second.dat".to_string(),
                },
            ])
            .expect("records commit");

        let entries = catalogue
            .browser_entries(&FolderCatalogueBrowserQuery {
                prefix: Some("incoming/".to_string()),
                search: Some("second".to_string()),
                offset: 0,
                limit: 10,
            })
            .expect("browser query");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key.object_id, "incoming/second.dat");
        assert_eq!(entries[0].size_bytes, 7);
        assert_eq!(entries[0].object_type, None);
        assert_eq!(entries[0].lifecycle_state, None);
        assert_eq!(entries[0].placement, None);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn browser_page_limit_is_bounded() {
        let root = root();
        let path = root.join("catalogue.json");
        let catalogue = FolderCatalogue::open(&path, "codex").expect("catalogue opens");
        let error = catalogue
            .browser_entries(&FolderCatalogueBrowserQuery {
                limit: 1_001,
                ..FolderCatalogueBrowserQuery::default()
            })
            .expect_err("oversized browser page rejected");
        assert!(error.to_string().contains("page limit"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn folder_catalogue_implements_shared_authority_batch_contract() {
        fn accepts_authority<T: ObjectCatalogueAuthority>() {}
        accepts_authority::<FolderCatalogue>();

        let root = root();
        let path = root.join("catalogue.json");
        let mut catalogue = FolderCatalogue::open(&path, "codex").expect("catalogue opens");
        catalogue
            .commit_batch(&[record()])
            .expect("authority batch commits");
        assert_eq!(
            ObjectCatalogueAuthority::records(&catalogue),
            vec![record()]
        );
        ObjectCatalogueAuthority::remove_record(
            &mut catalogue,
            &BackendObjectKey {
                object_id: "incoming/data.txt".to_string(),
                version: 1,
            },
        )
        .expect("authority removal commits");
        assert!(catalogue.records().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }
}
