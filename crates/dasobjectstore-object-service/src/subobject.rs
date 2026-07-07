//! Named SubObject endpoint registry.

use crate::provider::ObjectServiceError;
use crate::registry::STORE_REGISTRY_ENV;
use dasobjectstore_core::ids::StoreId;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

#[cfg(target_os = "macos")]
const DEFAULT_SUBOBJECT_REGISTRY_PATH: &str = "/usr/local/etc/dasobjectstore/subobjects.json";
#[cfg(not(target_os = "macos"))]
const DEFAULT_SUBOBJECT_REGISTRY_PATH: &str = "/etc/dasobjectstore/subobjects.json";

pub const PORTABLE_SUBOBJECT_REGISTRY_RELATIVE_PATH: &str = ".dasobjectstore/subobjects.json";

#[cfg(unix)]
const REGISTRY_DIR_MODE: u32 = 0o750;
#[cfg(unix)]
const REGISTRY_FILE_MODE: u32 = 0o640;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SubObjectDefinition {
    pub name: String,
    pub store_id: StoreId,
    pub parent: SubObjectParent,
    pub path: Vec<String>,
}

impl SubObjectDefinition {
    pub fn object_prefix(&self) -> String {
        let mut parts = Vec::with_capacity(self.path.len() + 1);
        parts.push(self.store_id.as_str().to_string());
        parts.extend(self.path.iter().cloned());
        parts.join("/")
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SubObjectParent {
    Store { store_id: StoreId },
    SubObject { name: String },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SubObjectRegistryUpdateReport {
    pub registry_path: PathBuf,
    pub action: SubObjectRegistryAction,
    pub definition: SubObjectDefinition,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SubObjectRegistryAction {
    Created,
    Updated,
}

impl SubObjectRegistryAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Updated => "updated",
        }
    }
}

pub fn default_subobject_registry_path() -> PathBuf {
    std::env::var_os("DASOBJECTSTORE_SUBOBJECT_REGISTRY_PATH")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os(STORE_REGISTRY_ENV).map(|store_path| {
                let store_path = PathBuf::from(store_path);
                store_path
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join("subobjects.json")
            })
        })
        .unwrap_or_else(|| PathBuf::from(DEFAULT_SUBOBJECT_REGISTRY_PATH))
}

pub fn portable_subobject_registry_path(ssd_root: impl AsRef<Path>) -> PathBuf {
    ssd_root
        .as_ref()
        .join(PORTABLE_SUBOBJECT_REGISTRY_RELATIVE_PATH)
}

pub fn read_subobject_registry(
    path: impl AsRef<Path>,
) -> Result<Vec<SubObjectDefinition>, ObjectServiceError> {
    let path = path.as_ref();
    match File::open(path) {
        Ok(file) => serde_json::from_reader(file).map_err(|error| {
            ObjectServiceError::InvalidConfiguration(format!(
                "read SubObject registry {}: {error}",
                path.display()
            ))
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(ObjectServiceError::CommandFailed(format!(
            "open SubObject registry {}: {error}",
            path.display()
        ))),
    }
}

pub fn create_subobject_definition(
    path: impl AsRef<Path>,
    name: impl Into<String>,
    parent: SubObjectParent,
) -> Result<SubObjectRegistryUpdateReport, ObjectServiceError> {
    let name = normalize_name(name.into())?;
    let path = path.as_ref();
    let mut definitions = read_subobject_registry(path)?;
    if definitions.iter().any(|definition| definition.name == name) {
        return Err(ObjectServiceError::InvalidConfiguration(format!(
            "SubObject name already exists: {name}"
        )));
    }

    let (store_id, mut object_path) = match &parent {
        SubObjectParent::Store { store_id } => (store_id.clone(), Vec::new()),
        SubObjectParent::SubObject { name } => {
            let parent = definitions
                .iter()
                .find(|definition| definition.name == *name)
                .ok_or_else(|| {
                    ObjectServiceError::InvalidConfiguration(format!(
                        "parent SubObject does not exist: {name}"
                    ))
                })?;
            (parent.store_id.clone(), parent.path.clone())
        }
    };
    object_path.push(name.clone());

    let definition = SubObjectDefinition {
        name,
        store_id,
        parent,
        path: object_path,
    };
    definitions.push(definition.clone());
    definitions.sort_by(|left, right| left.name.cmp(&right.name));
    write_subobject_registry(path, &definitions)?;

    Ok(SubObjectRegistryUpdateReport {
        registry_path: path.to_path_buf(),
        action: SubObjectRegistryAction::Created,
        definition,
    })
}

pub fn mirror_subobject_definition(
    path: impl AsRef<Path>,
    definition: SubObjectDefinition,
) -> Result<SubObjectRegistryUpdateReport, ObjectServiceError> {
    let path = path.as_ref();
    let mut definitions = read_subobject_registry(path)?;
    let action = if let Some(existing) = definitions
        .iter_mut()
        .find(|existing| existing.name == definition.name)
    {
        if existing != &definition {
            return Err(ObjectServiceError::InvalidConfiguration(format!(
                "SubObject name already exists with different metadata: {}",
                definition.name
            )));
        }
        SubObjectRegistryAction::Updated
    } else {
        definitions.push(definition.clone());
        SubObjectRegistryAction::Created
    };
    definitions.sort_by(|left, right| left.name.cmp(&right.name));
    write_subobject_registry(path, &definitions)?;

    Ok(SubObjectRegistryUpdateReport {
        registry_path: path.to_path_buf(),
        action,
        definition,
    })
}

pub fn search_subobjects<'a>(
    definitions: &'a [SubObjectDefinition],
    query: &str,
) -> Vec<&'a SubObjectDefinition> {
    let query = query.to_lowercase();
    definitions
        .iter()
        .filter(|definition| {
            definition.name.to_lowercase().contains(&query)
                || definition.object_prefix().to_lowercase().contains(&query)
        })
        .collect()
}

fn normalize_name(name: String) -> Result<String, ObjectServiceError> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.chars().any(char::is_whitespace)
    {
        return Err(ObjectServiceError::InvalidConfiguration(format!(
            "SubObject name must be non-blank and must not contain whitespace or path separators: {trimmed}"
        )));
    }

    Ok(trimmed.to_string())
}

fn write_subobject_registry(
    path: &Path,
    definitions: &[SubObjectDefinition],
) -> Result<(), ObjectServiceError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            ObjectServiceError::CommandFailed(format!(
                "create SubObject registry directory {}: {error}",
                parent.display()
            ))
        })?;
        restrict_dir(parent)?;
    }

    let file = create_private_file(path).map_err(|error| {
        ObjectServiceError::CommandFailed(format!(
            "create SubObject registry {}: {error}",
            path.display()
        ))
    })?;
    serde_json::to_writer_pretty(file, definitions).map_err(|error| {
        ObjectServiceError::CommandFailed(format!(
            "write SubObject registry {}: {error}",
            path.display()
        ))
    })?;

    Ok(())
}

fn create_private_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);

    #[cfg(unix)]
    options.mode(REGISTRY_FILE_MODE);

    options.open(path)
}

fn restrict_dir(path: &Path) -> Result<(), ObjectServiceError> {
    #[cfg(unix)]
    {
        fs::set_permissions(path, fs::Permissions::from_mode(REGISTRY_DIR_MODE)).map_err(
            |error| {
                ObjectServiceError::CommandFailed(format!(
                    "restrict SubObject registry directory {}: {error}",
                    path.display()
                ))
            },
        )?;
    }

    Ok(())
}
