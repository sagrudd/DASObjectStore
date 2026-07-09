use crate::dashboard::{DashboardWarning, StorageGroupView};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt::{self, Display};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub(crate) const DEFAULT_GROUPS_REGISTRY_PATH: &str = "/opt/dasobjectstore/groups.json";
pub(crate) const GROUPS_REGISTRY_ENV: &str = "DASOBJECTSTORE_GROUPS_PATH";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StorageGroupsSnapshot {
    pub path: PathBuf,
    pub groups: Vec<StorageGroupView>,
    pub warnings: Vec<DashboardWarning>,
}

pub(crate) fn default_groups_registry_path() -> PathBuf {
    std::env::var_os(GROUPS_REGISTRY_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_GROUPS_REGISTRY_PATH))
}

pub(crate) fn read_storage_groups_for_user(
    path: &Path,
    current_user_groups: &[String],
) -> StorageGroupsSnapshot {
    let current_user_groups = current_user_groups.iter().collect::<BTreeSet<_>>();
    match read_storage_group_entries(path) {
        Ok(entries) => StorageGroupsSnapshot {
            path: path.to_path_buf(),
            groups: entries
                .into_iter()
                .map(|entry| entry.into_view(&current_user_groups))
                .collect(),
            warnings: Vec::new(),
        },
        Err(StorageGroupRegistryError::Missing) => StorageGroupsSnapshot {
            path: path.to_path_buf(),
            groups: Vec::new(),
            warnings: vec![DashboardWarning::new(
                "groups_registry_missing",
                format!(
                    "Storage group registry is not present at {}.",
                    path.display()
                ),
            )],
        },
        Err(StorageGroupRegistryError::Read(error)) => StorageGroupsSnapshot {
            path: path.to_path_buf(),
            groups: Vec::new(),
            warnings: vec![DashboardWarning::new(
                "groups_registry_unreadable",
                format!(
                    "Storage group registry {} could not be read: {error}.",
                    path.display()
                ),
            )],
        },
        Err(StorageGroupRegistryError::Json(error)) => StorageGroupsSnapshot {
            path: path.to_path_buf(),
            groups: Vec::new(),
            warnings: vec![DashboardWarning::new(
                "groups_registry_invalid",
                format!(
                    "Storage group registry {} is not valid JSON: {error}.",
                    path.display()
                ),
            )],
        },
    }
}

pub(crate) fn upsert_storage_group(
    path: &Path,
    group_name: &str,
) -> Result<bool, StorageGroupRegistryWriteError> {
    let group_name = group_name.trim();
    if group_name.is_empty() {
        return Err(StorageGroupRegistryWriteError::BlankGroupName);
    }

    let mut entries = match read_storage_group_entries(path) {
        Ok(entries) => entries,
        Err(StorageGroupRegistryError::Missing) => Vec::new(),
        Err(StorageGroupRegistryError::Read(error)) => {
            return Err(StorageGroupRegistryWriteError::Read(error));
        }
        Err(StorageGroupRegistryError::Json(error)) => {
            return Err(StorageGroupRegistryWriteError::Json(error));
        }
    };

    if entries
        .iter()
        .any(|entry| entry.group_name().as_deref() == Some(group_name))
    {
        return Ok(false);
    }

    entries.push(StorageGroupEntry::Object {
        group_name: group_name.to_string(),
        display_name: None,
        source: Some("local_os".to_string()),
    });

    write_storage_group_entries(path, entries)?;
    Ok(true)
}

fn read_storage_group_entries(
    path: &Path,
) -> Result<Vec<StorageGroupEntry>, StorageGroupRegistryError> {
    let data = match fs::read_to_string(path) {
        Ok(data) => data,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(StorageGroupRegistryError::Missing);
        }
        Err(error) => return Err(StorageGroupRegistryError::Read(error)),
    };
    let registry: StorageGroupRegistryFile =
        serde_json::from_str(&data).map_err(StorageGroupRegistryError::Json)?;
    Ok(registry.entries())
}

#[derive(Debug)]
enum StorageGroupRegistryError {
    Missing,
    Read(std::io::Error),
    Json(serde_json::Error),
}

#[derive(Debug)]
pub(crate) enum StorageGroupRegistryWriteError {
    BlankGroupName,
    Read(io::Error),
    Json(serde_json::Error),
    CreateDirectory(io::Error),
    Encode(serde_json::Error),
    Write(io::Error),
    Rename(io::Error),
}

impl Display for StorageGroupRegistryWriteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BlankGroupName => write!(formatter, "group name must not be blank"),
            Self::Read(error) => write!(formatter, "group registry could not be read: {error}"),
            Self::Json(error) => write!(formatter, "group registry is not valid JSON: {error}"),
            Self::CreateDirectory(error) => {
                write!(
                    formatter,
                    "group registry directory could not be created: {error}"
                )
            }
            Self::Encode(error) => {
                write!(formatter, "group registry could not be encoded: {error}")
            }
            Self::Write(error) => write!(formatter, "group registry could not be written: {error}"),
            Self::Rename(error) => write!(
                formatter,
                "group registry could not be moved into place: {error}"
            ),
        }
    }
}

impl std::error::Error for StorageGroupRegistryWriteError {}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(untagged)]
enum StorageGroupRegistryFile {
    Object { groups: Vec<StorageGroupEntry> },
    List(Vec<StorageGroupEntry>),
}

impl StorageGroupRegistryFile {
    fn entries(self) -> Vec<StorageGroupEntry> {
        match self {
            Self::Object { groups } => groups,
            Self::List(groups) => groups,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(untagged)]
enum StorageGroupEntry {
    Name(String),
    Object {
        group_name: String,
        #[serde(default)]
        #[serde(skip_serializing_if = "Option::is_none")]
        display_name: Option<String>,
        #[serde(default)]
        #[serde(skip_serializing_if = "Option::is_none")]
        source: Option<String>,
    },
}

impl StorageGroupEntry {
    fn group_name(&self) -> Option<&str> {
        match self {
            Self::Name(group_name) => Some(group_name),
            Self::Object { group_name, .. } => Some(group_name),
        }
    }

    fn into_view(self, current_user_groups: &BTreeSet<&String>) -> StorageGroupView {
        let (group_name, display_name, source) = match self {
            Self::Name(group_name) => (group_name, None, None),
            Self::Object {
                group_name,
                display_name,
                source,
            } => (group_name, display_name, source),
        };
        let display_name = display_name.unwrap_or_else(|| titleize_group_name(&group_name));

        StorageGroupView {
            current_user_member: current_user_groups.contains(&group_name),
            group_name,
            display_name,
            source: source.unwrap_or_else(|| "local_os".to_string()),
        }
    }
}

fn write_storage_group_entries(
    path: &Path,
    entries: Vec<StorageGroupEntry>,
) -> Result<(), StorageGroupRegistryWriteError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(StorageGroupRegistryWriteError::CreateDirectory)?;
    }
    let registry = StorageGroupRegistryFile::Object { groups: entries };
    let data =
        serde_json::to_string_pretty(&registry).map_err(StorageGroupRegistryWriteError::Encode)?;
    let temp_path = path.with_extension("json.tmp");
    fs::write(&temp_path, format!("{data}\n")).map_err(StorageGroupRegistryWriteError::Write)?;
    fs::rename(temp_path, path).map_err(StorageGroupRegistryWriteError::Rename)
}

fn titleize_group_name(group_name: &str) -> String {
    group_name
        .split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::{read_storage_groups_for_user, upsert_storage_group};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn reads_object_storage_group_registry_with_membership() {
        let root = temp_root("groups-object");
        let path = root.join("groups.json");
        fs::write(
            &path,
            r#"{"groups":[{"group_name":"bioinformatics","display_name":"Bioinformatics","source":"local_os"}]}"#,
        )
        .expect("groups write");

        let snapshot = read_storage_groups_for_user(&path, &["bioinformatics".to_string()]);

        assert!(snapshot.warnings.is_empty());
        assert_eq!(snapshot.groups.len(), 1);
        assert_eq!(snapshot.groups[0].group_name, "bioinformatics");
        assert_eq!(snapshot.groups[0].display_name, "Bioinformatics");
        assert!(snapshot.groups[0].current_user_member);
    }

    #[test]
    fn reads_list_storage_group_registry() {
        let root = temp_root("groups-list");
        let path = root.join("groups.json");
        fs::write(&path, r#"["sequence_writers"]"#).expect("groups write");

        let snapshot = read_storage_groups_for_user(&path, &[]);

        assert!(snapshot.warnings.is_empty());
        assert_eq!(snapshot.groups[0].group_name, "sequence_writers");
        assert_eq!(snapshot.groups[0].display_name, "Sequence Writers");
        assert!(!snapshot.groups[0].current_user_member);
    }

    #[test]
    fn missing_storage_group_registry_reports_warning() {
        let root = temp_root("groups-missing");
        let snapshot = read_storage_groups_for_user(&root.join("missing.json"), &[]);

        assert!(snapshot.groups.is_empty());
        assert!(snapshot
            .warnings
            .iter()
            .any(|warning| warning.code == "groups_registry_missing"));
    }

    #[test]
    fn upsert_storage_group_creates_registry_when_missing() {
        let root = temp_root("groups-upsert-missing");
        let path = root.join("groups.json");

        let changed = upsert_storage_group(&path, "mnemosyne").expect("upsert succeeds");
        let snapshot = read_storage_groups_for_user(&path, &["mnemosyne".to_string()]);

        assert!(changed);
        assert!(snapshot.warnings.is_empty());
        assert_eq!(snapshot.groups.len(), 1);
        assert_eq!(snapshot.groups[0].group_name, "mnemosyne");
        assert_eq!(snapshot.groups[0].source, "local_os");
        assert!(snapshot.groups[0].current_user_member);
    }

    #[test]
    fn upsert_storage_group_is_idempotent_for_existing_group() {
        let root = temp_root("groups-upsert-existing");
        let path = root.join("groups.json");
        fs::write(
            &path,
            r#"{"groups":[{"group_name":"mnemosyne","display_name":"Mnemosyne","source":"local_os"}]}"#,
        )
        .expect("groups write");

        let changed = upsert_storage_group(&path, "mnemosyne").expect("upsert succeeds");
        let snapshot = read_storage_groups_for_user(&path, &[]);

        assert!(!changed);
        assert_eq!(snapshot.groups.len(), 1);
        assert_eq!(snapshot.groups[0].display_name, "Mnemosyne");
    }

    fn temp_root(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("dos-gui-{label}-{unique}"));
        fs::create_dir_all(&root).expect("temp root");
        root
    }
}
