//! System-managed store service definition registry.

use crate::credentials::credential_reference_for_store;
use crate::layout::{bucket_name_for_definition, StoreServiceDefinition};
use crate::provider::ObjectServiceError;
use dasobjectstore_core::store::ExportPolicy;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

pub const STORE_REGISTRY_ENV: &str = "DASOBJECTSTORE_STORE_REGISTRY_PATH";

#[cfg(target_os = "macos")]
const DEFAULT_STORE_REGISTRY_PATH: &str = "/usr/local/etc/dasobjectstore/stores.json";
#[cfg(not(target_os = "macos"))]
const DEFAULT_STORE_REGISTRY_PATH: &str = "/var/lib/dasobjectstore/stores.json";

pub const PORTABLE_STORE_REGISTRY_RELATIVE_PATH: &str = ".dasobjectstore/stores.json";

#[cfg(unix)]
const REGISTRY_DIR_MODE: u32 = 0o750;
#[cfg(unix)]
const REGISTRY_FILE_MODE: u32 = 0o640;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreRegistryUpdateReport {
    pub registry_path: PathBuf,
    pub action: StoreRegistryAction,
    pub definition: StoreServiceDefinition,
    pub bucket_name: Option<String>,
    pub credential_reference: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreRegistryDeleteReport {
    pub registry_path: PathBuf,
    pub store_id: dasobjectstore_core::ids::StoreId,
    pub removed: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreRegistryAction {
    Created,
    Updated,
}

impl StoreRegistryAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Updated => "updated",
        }
    }
}

pub fn default_store_registry_path() -> PathBuf {
    std::env::var_os(STORE_REGISTRY_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_STORE_REGISTRY_PATH))
}

pub fn portable_store_registry_path(ssd_root: impl AsRef<Path>) -> PathBuf {
    ssd_root
        .as_ref()
        .join(PORTABLE_STORE_REGISTRY_RELATIVE_PATH)
}

pub fn read_store_registry(
    path: impl AsRef<Path>,
) -> Result<Vec<StoreServiceDefinition>, ObjectServiceError> {
    let path = path.as_ref();
    match File::open(path) {
        Ok(file) => serde_json::from_reader(file).map_err(|error| {
            ObjectServiceError::InvalidConfiguration(format!(
                "read store registry {}: {error}",
                path.display()
            ))
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(ObjectServiceError::CommandFailed(format!(
            "open store registry {}: {error}",
            path.display()
        ))),
    }
}

pub fn upsert_store_definition(
    path: impl AsRef<Path>,
    definition: StoreServiceDefinition,
) -> Result<StoreRegistryUpdateReport, ObjectServiceError> {
    definition
        .policy
        .validate()
        .map_err(|error| ObjectServiceError::InvalidConfiguration(error.to_string()))?;

    let path = path.as_ref();
    let mut definitions = read_store_registry(path)?;
    let existing = definitions
        .iter()
        .position(|stored| stored.store_id == definition.store_id);
    let action = if let Some(index) = existing {
        definitions[index] = definition.clone();
        StoreRegistryAction::Updated
    } else {
        definitions.push(definition.clone());
        StoreRegistryAction::Created
    };

    validate_store_registry(&definitions)?;
    write_store_registry(path, &definitions)?;

    let (bucket_name, credential_reference) = if definition.policy.export_policy == ExportPolicy::S3
    {
        (
            Some(bucket_name_for_definition(&definition)?),
            Some(credential_reference_for_store(&definition.store_id)),
        )
    } else {
        (None, None)
    };

    Ok(StoreRegistryUpdateReport {
        registry_path: path.to_path_buf(),
        action,
        definition,
        bucket_name,
        credential_reference,
    })
}

pub fn delete_store_definition(
    path: impl AsRef<Path>,
    store_id: &dasobjectstore_core::ids::StoreId,
) -> Result<StoreRegistryDeleteReport, ObjectServiceError> {
    let path = path.as_ref();
    let mut definitions = read_store_registry(path)?;
    let original_len = definitions.len();
    definitions.retain(|definition| &definition.store_id != store_id);
    let removed = definitions.len() != original_len;
    if removed {
        validate_store_registry(&definitions)?;
        write_store_registry(path, &definitions)?;
    }

    Ok(StoreRegistryDeleteReport {
        registry_path: path.to_path_buf(),
        store_id: store_id.clone(),
        removed,
    })
}

fn validate_store_registry(
    definitions: &[StoreServiceDefinition],
) -> Result<(), ObjectServiceError> {
    let mut store_ids = BTreeSet::new();
    let mut bucket_names = BTreeSet::new();

    for definition in definitions {
        definition
            .policy
            .validate()
            .map_err(|error| ObjectServiceError::InvalidConfiguration(error.to_string()))?;

        if !store_ids.insert(definition.store_id.as_str()) {
            return Err(ObjectServiceError::InvalidConfiguration(format!(
                "duplicate store definition: {}",
                definition.store_id
            )));
        }

        if definition.policy.export_policy == ExportPolicy::S3 {
            let bucket_name = bucket_name_for_definition(definition)?;
            if !bucket_names.insert(bucket_name.clone()) {
                return Err(ObjectServiceError::InvalidConfiguration(format!(
                    "duplicate bucket name: {bucket_name}"
                )));
            }
        }
    }

    Ok(())
}

fn write_store_registry(
    path: &Path,
    definitions: &[StoreServiceDefinition],
) -> Result<(), ObjectServiceError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            ObjectServiceError::CommandFailed(format!(
                "create store registry directory {}: {error}",
                parent.display()
            ))
        })?;
        restrict_dir(parent)?;
    }

    let file = create_private_file(path).map_err(|error| {
        ObjectServiceError::CommandFailed(format!(
            "create store registry {}: {error}",
            path.display()
        ))
    })?;
    serde_json::to_writer_pretty(file, definitions).map_err(|error| {
        ObjectServiceError::CommandFailed(format!(
            "write store registry {}: {error}",
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
                    "restrict store registry directory {}: {error}",
                    path.display()
                ))
            },
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        delete_store_definition, read_store_registry, upsert_store_definition, StoreRegistryAction,
    };
    use crate::layout::StoreServiceDefinition;
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_core::store::{StoreClass, StorePolicy};
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn linux_default_reads_the_daemon_owned_mutable_registry() {
        assert_eq!(
            super::DEFAULT_STORE_REGISTRY_PATH,
            "/var/lib/dasobjectstore/stores.json"
        );
    }

    #[test]
    fn creates_system_managed_store_registry() {
        let root = temp_root("store-registry-create");
        let registry_path = root.join("stores.json");
        let report = upsert_store_definition(
            &registry_path,
            definition("generated-data", StoreClass::GeneratedData, None),
        )
        .expect("store created");

        assert_eq!(report.action, StoreRegistryAction::Created);
        assert_eq!(report.bucket_name.as_deref(), Some("dos-generated-data"));
        assert_eq!(
            report.credential_reference.as_deref(),
            Some("secret://dasobjectstore/stores/generated-data/s3")
        );

        let definitions = read_store_registry(&registry_path).expect("registry reads");
        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0].store_id.as_str(), "generated-data");

        #[cfg(unix)]
        {
            assert_eq!(
                fs::metadata(&root)
                    .expect("registry dir metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o750
            );
            assert_eq!(
                fs::metadata(&registry_path)
                    .expect("registry metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o640
            );
        }

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn updates_existing_store_definition() {
        let root = temp_root("store-registry-update");
        let registry_path = root.join("stores.json");
        upsert_store_definition(
            &registry_path,
            definition("generated-data", StoreClass::GeneratedData, None),
        )
        .expect("store created");

        let report = upsert_store_definition(
            &registry_path,
            definition(
                "generated-data",
                StoreClass::CriticalMetadata,
                Some("critical-generated-data".to_string()),
            ),
        )
        .expect("store updated");

        assert_eq!(report.action, StoreRegistryAction::Updated);
        assert_eq!(
            report.bucket_name.as_deref(),
            Some("critical-generated-data")
        );
        let definitions = read_store_registry(&registry_path).expect("registry reads");
        assert_eq!(definitions.len(), 1);
        assert_eq!(definitions[0].policy.class, StoreClass::CriticalMetadata);

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn rejects_duplicate_bucket_names() {
        let root = temp_root("store-registry-duplicate-bucket");
        let registry_path = root.join("stores.json");
        upsert_store_definition(
            &registry_path,
            definition(
                "store-a",
                StoreClass::GeneratedData,
                Some("shared".to_string()),
            ),
        )
        .expect("store a created");

        let err = upsert_store_definition(
            &registry_path,
            definition(
                "store-b",
                StoreClass::GeneratedData,
                Some("shared".to_string()),
            ),
        )
        .expect_err("duplicate bucket rejected");

        assert!(err.to_string().contains("duplicate bucket name"));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn deletes_store_definition() {
        let root = temp_root("store-registry-delete");
        let registry_path = root.join("stores.json");
        upsert_store_definition(
            &registry_path,
            definition("generated-data", StoreClass::GeneratedData, None),
        )
        .expect("store created");

        let report = delete_store_definition(
            &registry_path,
            &StoreId::new("generated-data").expect("store id"),
        )
        .expect("store deleted");

        assert!(report.removed);
        assert!(read_store_registry(&registry_path)
            .expect("registry reads")
            .is_empty());

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    fn definition(
        store_id: &str,
        class: StoreClass,
        bucket_name: Option<String>,
    ) -> StoreServiceDefinition {
        StoreServiceDefinition {
            store_id: StoreId::new(store_id).expect("store id"),
            policy: StorePolicy::defaults_for(class),
            bucket_name,
            reader_group: None,
            writer_group: None,
            public: false,
        }
    }

    fn temp_root(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dasobjectstore-object-service-{name}-{}-{nanos}",
            std::process::id()
        ))
    }
}
