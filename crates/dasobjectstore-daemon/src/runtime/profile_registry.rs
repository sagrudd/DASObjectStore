//! Daemon-owned bindings between portable manifests and validated local roots.
//!
//! Portable manifests intentionally carry identity, not authoritative host
//! paths. This registry is the daemon-only seam that resolves a profile to a
//! canonical filesystem root for capacity probes and later backend routing.

use super::DaemonServiceRuntimeError;
use dasobjectstore_core::manifest::{BackendReference, ObjectStoreManifest};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const PROFILE_BINDING_REGISTRY_SCHEMA: &str = "dasobjectstore.profile_binding_registry.v1";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BackendProfileBinding {
    pub manifest: ObjectStoreManifest,
    pub backend_root: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssd_staging_root: Option<PathBuf>,
}

impl BackendProfileBinding {
    pub fn validate_and_canonicalize(mut self) -> Result<Self, DaemonServiceRuntimeError> {
        self.manifest
            .validate()
            .map_err(|error| invalid_binding(error.to_string()))?;
        self.backend_root = canonical_directory("backend_root", &self.backend_root)?;
        if let Some(staging_root) = &self.ssd_staging_root {
            self.ssd_staging_root = Some(canonical_directory("ssd_staging_root", staging_root)?);
        }
        match &self.manifest.backend {
            BackendReference::Folder { .. } => {}
            BackendReference::Drive {
                mount_path_hint: Some(hint),
                ..
            } => {
                let hint = canonical_directory("mount_path_hint", hint)?;
                if hint != self.backend_root {
                    return Err(invalid_binding(format!(
                        "drive mount_path_hint {} does not match backend root {}",
                        hint.display(),
                        self.backend_root.display()
                    )));
                }
            }
            BackendReference::Drive {
                mount_path_hint: None,
                ..
            }
            | BackendReference::Appliance { .. } => {}
        }
        Ok(self)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ProfileBindingRegistryFile {
    schema_version: String,
    bindings: Vec<BackendProfileBinding>,
}

impl Default for ProfileBindingRegistryFile {
    fn default() -> Self {
        Self {
            schema_version: PROFILE_BINDING_REGISTRY_SCHEMA.to_string(),
            bindings: Vec::new(),
        }
    }
}

pub fn read_profile_binding(
    path: impl AsRef<Path>,
    store_id: &str,
) -> Result<Option<BackendProfileBinding>, DaemonServiceRuntimeError> {
    let registry = read_registry(path.as_ref())?;
    let binding = registry
        .bindings
        .into_iter()
        .find(|binding| binding.manifest.store_id.as_str() == store_id);
    binding
        .map(BackendProfileBinding::validate_and_canonicalize)
        .transpose()
}

pub fn upsert_profile_binding(
    path: impl AsRef<Path>,
    binding: BackendProfileBinding,
) -> Result<(), DaemonServiceRuntimeError> {
    let path = path.as_ref();
    let binding = binding.validate_and_canonicalize()?;
    let mut registry = read_registry(path)?;
    if let Some(existing) = registry
        .bindings
        .iter_mut()
        .find(|existing| existing.manifest.store_id == binding.manifest.store_id)
    {
        *existing = binding;
    } else {
        registry.bindings.push(binding);
    }
    registry.bindings.sort_by(|left, right| {
        left.manifest
            .store_id
            .as_str()
            .cmp(right.manifest.store_id.as_str())
    });
    write_registry(path, &registry)
}

fn read_registry(path: &Path) -> Result<ProfileBindingRegistryFile, DaemonServiceRuntimeError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(ProfileBindingRegistryFile::default())
        }
        Err(error) => return Err(registry_io(path, error)),
    };
    let registry: ProfileBindingRegistryFile = serde_json::from_reader(file).map_err(|error| {
        DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!(
                "invalid profile binding registry {}: {error}",
                path.display()
            ),
        }
    })?;
    if registry.schema_version != PROFILE_BINDING_REGISTRY_SCHEMA {
        return Err(invalid_binding(format!(
            "unsupported profile binding registry schema {}",
            registry.schema_version
        )));
    }
    let mut store_ids = BTreeSet::new();
    for binding in &registry.bindings {
        binding
            .manifest
            .validate()
            .map_err(|error| invalid_binding(error.to_string()))?;
        if !store_ids.insert(binding.manifest.store_id.as_str()) {
            return Err(invalid_binding(format!(
                "duplicate profile binding for store {}",
                binding.manifest.store_id
            )));
        }
    }
    Ok(registry)
}

fn write_registry(
    path: &Path,
    registry: &ProfileBindingRegistryFile,
) -> Result<(), DaemonServiceRuntimeError> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid_binding("profile binding registry has no parent"))?;
    fs::create_dir_all(parent).map_err(|error| registry_io(parent, error))?;
    let temporary = parent.join(format!(
        ".{}.tmp-{}-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("profiles"),
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    let bytes = serde_json::to_vec_pretty(registry).map_err(|error| {
        DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!("serialize profile binding registry: {error}"),
        }
    })?;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| registry_io(&temporary, error))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| registry_io(&temporary, error))?;
    drop(file);
    fs::rename(&temporary, path).map_err(|error| registry_io(path, error))?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| registry_io(parent, error))
}

fn canonical_directory(
    field: &'static str,
    path: &Path,
) -> Result<PathBuf, DaemonServiceRuntimeError> {
    if !path.is_absolute() {
        return Err(DaemonServiceRuntimeError::RelativePath {
            field,
            path: path.to_path_buf(),
        });
    }
    let canonical = fs::canonicalize(path).map_err(|error| registry_io(path, error))?;
    if !canonical.is_dir() {
        return Err(invalid_binding(format!(
            "{field} is not a directory: {}",
            canonical.display()
        )));
    }
    Ok(canonical)
}

fn invalid_binding(message: impl Into<String>) -> DaemonServiceRuntimeError {
    DaemonServiceRuntimeError::UnsupportedOperation {
        operation: format!("invalid profile binding: {}", message.into()),
    }
}

fn registry_io(path: &Path, error: io::Error) -> DaemonServiceRuntimeError {
    DaemonServiceRuntimeError::UnsupportedOperation {
        operation: format!("profile binding registry I/O {}: {error}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        read_profile_binding, read_registry, upsert_profile_binding, BackendProfileBinding,
    };
    use dasobjectstore_core::deployment::{DeploymentProfile, HostMode};
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_core::manifest::{
        BackendReference, ObjectStoreManifest, OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
    };
    use dasobjectstore_core::protection::ProtectionPolicy;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    fn root(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let root = std::env::var_os("DASOBJECTSTORE_CODEX_VALIDATION_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("dasobjectstore-codex-validation"))
            .join(format!(
                "profile-registry-{label}-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(&root).expect("fixture root");
        root
    }

    fn folder_binding(store_id: &str, root: &Path) -> BackendProfileBinding {
        BackendProfileBinding {
            manifest: ObjectStoreManifest {
                schema_version: OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
                store_id: StoreId::new(store_id).expect("store id"),
                deployment_profile: DeploymentProfile::Folder,
                host_mode: HostMode::PerUser,
                protection: ProtectionPolicy::LocalOnly,
                backend: BackendReference::Folder {
                    root_identity: format!("fsid:{store_id}"),
                },
            },
            backend_root: root.to_path_buf(),
            ssd_staging_root: None,
        }
    }

    #[test]
    fn round_trips_binding_and_canonicalizes_roots() {
        let root = root("roundtrip");
        let path = root.join("bindings.json");
        let backend = root.join("backend");
        fs::create_dir_all(&backend).expect("backend");
        upsert_profile_binding(&path, folder_binding("folder", &backend)).expect("upsert");
        let binding = read_profile_binding(&path, "folder")
            .expect("read")
            .expect("binding");
        assert_eq!(binding.manifest.store_id.as_str(), "folder");
        assert_eq!(
            binding.backend_root,
            fs::canonicalize(backend).expect("canonical")
        );
        assert!(read_profile_binding(&path, "other")
            .expect("missing read")
            .is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_drive_manifest_mount_mismatch() {
        let root = root("mismatch");
        let first = root.join("first");
        let second = root.join("second");
        fs::create_dir_all(&first).expect("first");
        fs::create_dir_all(&second).expect("second");
        let mut binding = folder_binding("drive", &first);
        binding.manifest.deployment_profile = DeploymentProfile::Drive;
        binding.manifest.backend = BackendReference::Drive {
            filesystem_identity: "fsid:drive".to_string(),
            device_identity: Some("device:drive".to_string()),
            media: dasobjectstore_core::manifest::DriveMediaKind::Ssd,
            mount_path_hint: Some(second),
        };
        assert!(upsert_profile_binding(root.join("bindings.json"), binding).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_unknown_registry_fields_and_duplicate_store_ids() {
        let root = root("strict");
        let path = root.join("bindings.json");
        let backend = root.join("backend");
        fs::create_dir_all(&backend).expect("backend");
        let binding = folder_binding("folder", &backend);
        let mut encoded = serde_json::to_value(&binding).expect("binding JSON");
        encoded["unexpected"] = serde_json::json!(true);
        fs::write(
            &path,
            serde_json::json!({
                "schema_version": super::PROFILE_BINDING_REGISTRY_SCHEMA,
                "bindings": [encoded]
            })
            .to_string(),
        )
        .expect("unknown-field registry");
        assert!(read_registry(&path).is_err());
        fs::remove_file(&path).expect("remove malformed registry");

        upsert_profile_binding(&path, binding.clone()).expect("binding upsert");
        let mut duplicate = serde_json::from_str::<serde_json::Value>(
            &fs::read_to_string(&path).expect("registry read"),
        )
        .expect("registry JSON");
        let first = duplicate["bindings"][0].clone();
        duplicate["bindings"] = serde_json::json!([first.clone(), first]);
        fs::write(&path, duplicate.to_string()).expect("duplicate registry");
        assert!(read_registry(&path).is_err());
        let _ = fs::remove_dir_all(root);
    }
}
