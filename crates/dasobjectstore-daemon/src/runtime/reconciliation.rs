//! Durable, provider-independent reconciliation manifest and resume planning.
//!
//! Provider adapters only need to enumerate object keys and drive the planned
//! actions.  The manifest is the restart authority for work already admitted
//! to the SSD staging pipeline; provider listings never become metadata
//! authority.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Display};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const RECONCILIATION_MANIFEST_SCHEMA: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReconciliationObject {
    pub key: String,
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub source_revision: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReconciliationManifest {
    pub schema_version: u32,
    pub store_id: String,
    pub prefix: Option<String>,
    pub updated_at_unix_seconds: u64,
    pub entries: BTreeMap<String, ReconciliationManifestEntry>,
}

impl ReconciliationManifest {
    pub fn new(store_id: impl Into<String>, prefix: Option<String>) -> Self {
        Self {
            schema_version: RECONCILIATION_MANIFEST_SCHEMA,
            store_id: store_id.into(),
            prefix,
            updated_at_unix_seconds: now_unix_seconds(),
            entries: BTreeMap::new(),
        }
    }

    pub fn load(path: &Path) -> Result<Self, ReconciliationManifestError> {
        let file = File::open(path).map_err(|error| ReconciliationManifestError::Io {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
        let manifest: Self = serde_json::from_reader(file).map_err(|error| {
            ReconciliationManifestError::InvalidJson {
                path: path.to_path_buf(),
                message: error.to_string(),
            }
        })?;
        if manifest.schema_version != RECONCILIATION_MANIFEST_SCHEMA {
            return Err(ReconciliationManifestError::UnsupportedSchema {
                path: path.to_path_buf(),
                schema_version: manifest.schema_version,
            });
        }
        Ok(manifest)
    }

    pub fn save_atomic(&mut self, path: &Path) -> Result<(), ReconciliationManifestError> {
        let parent = path
            .parent()
            .ok_or_else(|| ReconciliationManifestError::Io {
                path: path.to_path_buf(),
                message: "manifest path has no parent directory".to_string(),
            })?;
        fs::create_dir_all(parent).map_err(|error| ReconciliationManifestError::Io {
            path: parent.to_path_buf(),
            message: error.to_string(),
        })?;
        self.updated_at_unix_seconds = now_unix_seconds();
        let bytes = serde_json::to_vec_pretty(self).map_err(|error| {
            ReconciliationManifestError::Serialize {
                path: path.to_path_buf(),
                message: error.to_string(),
            }
        })?;
        let temporary_path = parent.join(format!(
            ".{}.tmp-{}-{}",
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("manifest"),
            std::process::id(),
            now_unix_nanos()
        ));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary_path)
            .map_err(|error| ReconciliationManifestError::Io {
                path: temporary_path.clone(),
                message: error.to_string(),
            })?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| ReconciliationManifestError::Io {
                path: temporary_path.clone(),
                message: error.to_string(),
            })?;
        drop(file);
        fs::rename(&temporary_path, path).map_err(|error| ReconciliationManifestError::Io {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| ReconciliationManifestError::Io {
                path: parent.to_path_buf(),
                message: error.to_string(),
            })
    }

    pub fn checkpoint(
        &mut self,
        path: &Path,
        key: &str,
        state: ReconciliationEntryState,
        message: Option<String>,
        downloaded_bytes: u64,
    ) -> Result<(), ReconciliationManifestError> {
        let entry =
            self.entries
                .get_mut(key)
                .ok_or_else(|| ReconciliationManifestError::UnknownKey {
                    key: key.to_string(),
                })?;
        entry.state = state;
        entry.message = message;
        entry.downloaded_bytes = downloaded_bytes;
        self.save_atomic(path)
    }
}

/// Find the newest incomplete checkpoint for one provider reconciliation
/// scope. Each job gets its own staging directory, but restart/retry must be
/// able to rediscover an interrupted job instead of silently starting a fresh
/// transfer. The daemon-owned root is scanned only one level deep and
/// symlinked entries are ignored to keep discovery inside the managed root.
pub fn discover_incomplete_reconciliation_manifest(
    reconciliation_root: &Path,
    store_id: &str,
    prefix: Option<&str>,
) -> Result<Option<PathBuf>, ReconciliationManifestError> {
    let entries = match fs::read_dir(reconciliation_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(ReconciliationManifestError::Io {
                path: reconciliation_root.to_path_buf(),
                message: error.to_string(),
            });
        }
    };
    let mut candidates = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| ReconciliationManifestError::Io {
            path: reconciliation_root.to_path_buf(),
            message: error.to_string(),
        })?;
        let entry_type = entry
            .file_type()
            .map_err(|error| ReconciliationManifestError::Io {
                path: entry.path(),
                message: error.to_string(),
            })?;
        if !entry_type.is_dir() || entry_type.is_symlink() {
            continue;
        }
        let manifest_path = entry
            .path()
            .join(".dasobjectstore")
            .join("reconciliation-manifest.json");
        let manifest_type = match fs::symlink_metadata(&manifest_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(ReconciliationManifestError::Io {
                    path: manifest_path,
                    message: error.to_string(),
                });
            }
        };
        if !manifest_type.is_file() || manifest_type.file_type().is_symlink() {
            continue;
        }
        let manifest = ReconciliationManifest::load(&manifest_path)?;
        if manifest.store_id != store_id || manifest.prefix.as_deref() != prefix {
            continue;
        }
        if manifest
            .entries
            .values()
            .any(|entry| !matches!(entry.state, ReconciliationEntryState::Complete))
        {
            candidates.push((manifest.updated_at_unix_seconds, manifest_path));
        }
    }
    Ok(candidates
        .into_iter()
        .max_by(|left, right| left.cmp(right))
        .map(|(_, path)| path))
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReconciliationManifestEntry {
    pub source_key: String,
    pub relative_path: Option<String>,
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub source_revision: Option<String>,
    pub state: ReconciliationEntryState,
    pub downloaded_bytes: u64,
    pub message: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReconciliationEntryState {
    Pending,
    InProgress,
    Complete,
    Failed,
    InvalidKey,
    Collision,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReconciliationPlan {
    pub actions: Vec<ReconciliationAction>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReconciliationAction {
    Download {
        key: String,
        relative_path: String,
        size_bytes: Option<u64>,
    },
    Resume {
        key: String,
        relative_path: String,
        size_bytes: Option<u64>,
        downloaded_bytes: u64,
    },
    SkipComplete {
        key: String,
        relative_path: String,
    },
    InvalidKey {
        key: String,
        reason: String,
    },
    Collision {
        key: String,
        relative_path: String,
        conflicting_keys: Vec<String>,
    },
}

pub fn plan_reconciliation(
    manifest: &mut ReconciliationManifest,
    objects: &[ReconciliationObject],
) -> ReconciliationPlan {
    let mut normalized: BTreeMap<String, Vec<&ReconciliationObject>> = BTreeMap::new();
    let mut actions = Vec::new();
    for object in objects {
        match normalize_key(&object.key) {
            Ok(relative_path) => normalized.entry(relative_path).or_default().push(object),
            Err(reason) => {
                manifest.entries.insert(
                    object.key.clone(),
                    ReconciliationManifestEntry {
                        source_key: object.key.clone(),
                        relative_path: None,
                        size_bytes: object.size_bytes,
                        source_revision: object.source_revision.clone(),
                        state: ReconciliationEntryState::InvalidKey,
                        downloaded_bytes: 0,
                        message: Some(reason.clone()),
                    },
                );
                actions.push(ReconciliationAction::InvalidKey {
                    key: object.key.clone(),
                    reason,
                });
            }
        }
    }

    for (relative_path, objects) in normalized {
        if objects.len() > 1 {
            let conflicting_keys: Vec<String> =
                objects.iter().map(|object| object.key.clone()).collect();
            for object in objects {
                manifest.entries.insert(
                    object.key.clone(),
                    ReconciliationManifestEntry {
                        source_key: object.key.clone(),
                        relative_path: Some(relative_path.clone()),
                        size_bytes: object.size_bytes,
                        source_revision: object.source_revision.clone(),
                        state: ReconciliationEntryState::Collision,
                        downloaded_bytes: 0,
                        message: Some("multiple provider keys normalize to one path".to_string()),
                    },
                );
                actions.push(ReconciliationAction::Collision {
                    key: object.key.clone(),
                    relative_path: relative_path.clone(),
                    conflicting_keys: conflicting_keys.clone(),
                });
            }
            continue;
        }

        let object = objects[0];
        let previous = manifest.entries.get(&object.key).cloned();
        let revision_matches = previous
            .as_ref()
            .and_then(|entry| entry.source_revision.as_ref())
            .zip(object.source_revision.as_ref())
            .is_some_and(|(previous, current)| previous == current);
        let (state, downloaded_bytes, message) = match previous {
            Some(entry)
                if entry.state == ReconciliationEntryState::Complete && revision_matches =>
            {
                actions.push(ReconciliationAction::SkipComplete {
                    key: object.key.clone(),
                    relative_path: relative_path.clone(),
                });
                (
                    ReconciliationEntryState::Complete,
                    entry.downloaded_bytes,
                    None,
                )
            }
            Some(entry)
                if entry.state == ReconciliationEntryState::InProgress && revision_matches =>
            {
                actions.push(ReconciliationAction::Resume {
                    key: object.key.clone(),
                    relative_path: relative_path.clone(),
                    size_bytes: object.size_bytes,
                    downloaded_bytes: entry.downloaded_bytes,
                });
                (
                    ReconciliationEntryState::InProgress,
                    entry.downloaded_bytes,
                    None,
                )
            }
            Some(entry)
                if matches!(
                    entry.state,
                    ReconciliationEntryState::Complete | ReconciliationEntryState::InProgress
                ) =>
            {
                actions.push(ReconciliationAction::Download {
                    key: object.key.clone(),
                    relative_path: relative_path.clone(),
                    size_bytes: object.size_bytes,
                });
                (
                    ReconciliationEntryState::Pending,
                    0,
                    Some("source revision changed; restarting safely".to_string()),
                )
            }
            _ => {
                actions.push(ReconciliationAction::Download {
                    key: object.key.clone(),
                    relative_path: relative_path.clone(),
                    size_bytes: object.size_bytes,
                });
                (ReconciliationEntryState::Pending, 0, None)
            }
        };
        manifest.entries.insert(
            object.key.clone(),
            ReconciliationManifestEntry {
                source_key: object.key.clone(),
                relative_path: Some(relative_path),
                size_bytes: object.size_bytes,
                source_revision: object.source_revision.clone(),
                state,
                downloaded_bytes,
                message,
            },
        );
    }
    manifest
        .entries
        .retain(|key, _| objects.iter().any(|object| object.key == *key));
    ReconciliationPlan { actions }
}

pub fn normalize_key(key: &str) -> Result<String, String> {
    if key.is_empty() {
        return Err("provider key is empty".to_string());
    }
    if key.contains('\0') || key.contains('\\') {
        return Err("provider key contains a forbidden character".to_string());
    }
    let path = Path::new(key);
    if path.is_absolute() {
        return Err("provider key must be relative".to_string());
    }
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => {
                let value = value.to_string_lossy();
                if value.is_empty() {
                    return Err("provider key contains an empty path component".to_string());
                }
                components.push(value.to_string());
            }
            Component::CurDir | Component::RootDir | Component::Prefix(_) => {
                return Err("provider key contains an unsafe path component".to_string());
            }
            Component::ParentDir => {
                return Err("provider key contains a parent path component".to_string());
            }
        }
    }
    if components.is_empty() {
        return Err("provider key has no usable path components".to_string());
    }
    let mut seen = BTreeSet::new();
    if components.iter().any(|component| !seen.insert(component)) {
        return Err("provider key repeats a path component".to_string());
    }
    Ok(components.join("/"))
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn now_unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
}

#[derive(Debug)]
pub enum ReconciliationManifestError {
    Io { path: PathBuf, message: String },
    InvalidJson { path: PathBuf, message: String },
    Serialize { path: PathBuf, message: String },
    UnsupportedSchema { path: PathBuf, schema_version: u32 },
    UnknownKey { key: String },
}

impl Display for ReconciliationManifestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, message } => {
                write!(formatter, "manifest I/O at {}: {message}", path.display())
            }
            Self::InvalidJson { path, message } => {
                write!(
                    formatter,
                    "invalid manifest JSON at {}: {message}",
                    path.display()
                )
            }
            Self::Serialize { path, message } => {
                write!(
                    formatter,
                    "manifest serialization at {}: {message}",
                    path.display()
                )
            }
            Self::UnsupportedSchema {
                path,
                schema_version,
            } => write!(
                formatter,
                "unsupported reconciliation manifest schema {schema_version} at {}",
                path.display()
            ),
            Self::UnknownKey { key } => write!(
                formatter,
                "manifest checkpoint references unknown key {key}"
            ),
        }
    }
}

impl std::error::Error for ReconciliationManifestError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn validation_root(label: &str) -> PathBuf {
        let root = std::env::var_os("DASOBJECTSTORE_CODEX_VALIDATION_ROOT")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|home| PathBuf::from(home).join(".dasobjectstore-codex-validation"))
            })
            .unwrap_or_else(std::env::temp_dir)
            .join(format!("reconciliation-{label}-{}", std::process::id()));
        fs::create_dir_all(&root).expect("validation root");
        root
    }

    fn temp_manifest_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "dasobjectstore-reconciliation-{name}-{}.json",
            std::process::id()
        ))
    }

    #[test]
    fn normalizes_safe_hierarchical_keys_and_rejects_escapes() {
        assert_eq!(
            normalize_key("run-42/data/file.bin").unwrap(),
            "run-42/data/file.bin"
        );
        assert!(normalize_key("../escape").is_err());
        assert!(normalize_key("/absolute").is_err());
        assert!(normalize_key("run\\\\file").is_err());
    }

    #[test]
    fn planner_reports_malformed_and_colliding_keys_without_download_actions() {
        let mut manifest = ReconciliationManifest::new("store-1", None);
        let plan = plan_reconciliation(
            &mut manifest,
            &[
                ReconciliationObject {
                    key: "../escape".to_string(),
                    size_bytes: Some(1),
                    source_revision: None,
                },
                ReconciliationObject {
                    key: "a//b".to_string(),
                    size_bytes: Some(2),
                    source_revision: None,
                },
                ReconciliationObject {
                    key: "a/b".to_string(),
                    size_bytes: Some(3),
                    source_revision: None,
                },
            ],
        );
        assert!(plan
            .actions
            .iter()
            .any(|action| matches!(action, ReconciliationAction::InvalidKey { .. })));
        assert_eq!(
            plan.actions
                .iter()
                .filter(|action| matches!(action, ReconciliationAction::Collision { .. }))
                .count(),
            2
        );
        assert!(manifest
            .entries
            .values()
            .all(|entry| !matches!(entry.state, ReconciliationEntryState::Pending)));
    }

    #[test]
    fn atomic_manifest_checkpoint_survives_reload_and_plans_resume() {
        let path = temp_manifest_path("resume");
        let _ = fs::remove_file(&path);
        let mut manifest = ReconciliationManifest::new("store-1", Some("run-42".to_string()));
        let objects = vec![ReconciliationObject {
            key: "run-42/data.bin".to_string(),
            size_bytes: Some(100),
            source_revision: Some("revision-1".to_string()),
        }];
        let first = plan_reconciliation(&mut manifest, &objects);
        assert!(matches!(
            first.actions[0],
            ReconciliationAction::Download { .. }
        ));
        manifest.save_atomic(&path).unwrap();
        manifest
            .checkpoint(
                &path,
                "run-42/data.bin",
                ReconciliationEntryState::InProgress,
                Some("download interrupted".to_string()),
                40,
            )
            .unwrap();
        let mut reloaded = ReconciliationManifest::load(&path).unwrap();
        let resumed = plan_reconciliation(&mut reloaded, &objects);
        assert!(matches!(
            resumed.actions[0],
            ReconciliationAction::Resume {
                downloaded_bytes: 40,
                ..
            }
        ));
        reloaded
            .checkpoint(
                &path,
                "run-42/data.bin",
                ReconciliationEntryState::Complete,
                None,
                100,
            )
            .unwrap();
        let mut complete = ReconciliationManifest::load(&path).unwrap();
        assert!(matches!(
            plan_reconciliation(&mut complete, &objects).actions[0],
            ReconciliationAction::SkipComplete { .. }
        ));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn discovers_newest_incomplete_manifest_for_store_and_prefix() {
        let root = validation_root("discovery");
        let _ = fs::remove_dir_all(&root);
        let older = root.join("older/.dasobjectstore/reconciliation-manifest.json");
        let newer = root.join("newer/.dasobjectstore/reconciliation-manifest.json");
        let wrong = root.join("wrong/.dasobjectstore/reconciliation-manifest.json");
        for path in [&older, &newer, &wrong] {
            fs::create_dir_all(path.parent().unwrap()).expect("manifest parent");
        }
        let mut older_manifest = ReconciliationManifest::new("store-1", Some("reads".into()));
        older_manifest.entries.insert(
            "older.bin".into(),
            ReconciliationManifestEntry {
                source_key: "older.bin".into(),
                relative_path: Some("older.bin".into()),
                size_bytes: Some(1),
                source_revision: Some("revision-1".into()),
                state: ReconciliationEntryState::InProgress,
                downloaded_bytes: 0,
                message: None,
            },
        );
        older_manifest.updated_at_unix_seconds = 10;
        fs::write(
            &older,
            serde_json::to_vec(&older_manifest).expect("older JSON"),
        )
        .expect("older manifest");
        let mut newer_manifest = ReconciliationManifest::new("store-1", Some("reads".into()));
        newer_manifest.entries.insert(
            "newer.bin".into(),
            ReconciliationManifestEntry {
                source_key: "newer.bin".into(),
                relative_path: Some("newer.bin".into()),
                size_bytes: Some(1),
                source_revision: Some("revision-2".into()),
                state: ReconciliationEntryState::Failed,
                downloaded_bytes: 0,
                message: Some("interrupted".into()),
            },
        );
        newer_manifest.updated_at_unix_seconds = 20;
        fs::write(
            &newer,
            serde_json::to_vec(&newer_manifest).expect("newer JSON"),
        )
        .expect("newer manifest");
        let mut wrong_manifest = ReconciliationManifest::new("other", Some("reads".into()));
        wrong_manifest.updated_at_unix_seconds = 30;
        fs::write(
            &wrong,
            serde_json::to_vec(&wrong_manifest).expect("wrong JSON"),
        )
        .expect("wrong manifest");

        let discovered =
            discover_incomplete_reconciliation_manifest(&root, "store-1", Some("reads"))
                .expect("discovery")
                .expect("checkpoint");
        assert_eq!(discovered, newer);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn complete_manifests_are_not_selected_for_restart() {
        let root = validation_root("complete");
        let _ = fs::remove_dir_all(&root);
        let path = root.join("complete/.dasobjectstore/reconciliation-manifest.json");
        fs::create_dir_all(path.parent().unwrap()).expect("manifest parent");
        let mut manifest = ReconciliationManifest::new("store-1", None);
        manifest.entries.insert(
            "reads.bin".into(),
            ReconciliationManifestEntry {
                source_key: "reads.bin".into(),
                relative_path: Some("reads.bin".into()),
                size_bytes: Some(1),
                source_revision: Some("revision-1".into()),
                state: ReconciliationEntryState::Complete,
                downloaded_bytes: 1,
                message: None,
            },
        );
        manifest.save_atomic(&path).expect("manifest");

        assert_eq!(
            discover_incomplete_reconciliation_manifest(&root, "store-1", None).expect("discovery"),
            None
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn legacy_entries_without_source_revision_never_skip_or_resume() {
        let mut manifest = ReconciliationManifest::new("store-legacy", None);
        manifest.entries.insert(
            "data.bin".to_string(),
            ReconciliationManifestEntry {
                source_key: "data.bin".to_string(),
                relative_path: Some("data.bin".to_string()),
                size_bytes: Some(4),
                source_revision: None,
                state: ReconciliationEntryState::Complete,
                downloaded_bytes: 4,
                message: None,
            },
        );
        let plan = plan_reconciliation(
            &mut manifest,
            &[ReconciliationObject {
                key: "data.bin".to_string(),
                size_bytes: Some(4),
                source_revision: None,
            }],
        );
        assert!(matches!(
            plan.actions.as_slice(),
            [ReconciliationAction::Download { .. }]
        ));
        assert_eq!(
            manifest.entries["data.bin"].state,
            ReconciliationEntryState::Pending
        );
    }
}
