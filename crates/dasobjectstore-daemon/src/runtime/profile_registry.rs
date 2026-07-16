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
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub const PROFILE_BINDING_REGISTRY_SCHEMA: &str = "dasobjectstore.profile_binding_registry.v1";
pub const PROFILE_BINDING_REGISTRY_FILE_NAME: &str = "profile-bindings.json";
pub const PROFILE_BINDING_REGISTRY_ENV: &str = "DASOBJECTSTORE_PROFILE_BINDINGS_PATH";

// Registration is a read/modify/write transaction over one daemon-owned
// registry file. Keep concurrent authenticated requests from replacing each
// other's newly-added bindings before the atomic publication boundary.
static PROFILE_BINDING_WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub fn default_profile_binding_registry_path(state_dir: impl AsRef<Path>) -> PathBuf {
    state_dir.as_ref().join(PROFILE_BINDING_REGISTRY_FILE_NAME)
}

pub fn profile_binding_registry_path(state_dir: impl AsRef<Path>) -> PathBuf {
    std::env::var_os(PROFILE_BINDING_REGISTRY_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| default_profile_binding_registry_path(state_dir))
}

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
        match &mut self.manifest.backend {
            BackendReference::Folder { .. } => {}
            BackendReference::Drive {
                mount_path_hint: Some(hint),
                ..
            } => {
                let canonical_hint = canonical_directory("mount_path_hint", hint)?;
                if canonical_hint != self.backend_root {
                    return Err(invalid_binding(format!(
                        "drive mount_path_hint {} does not match backend root {}",
                        canonical_hint.display(),
                        self.backend_root.display()
                    )));
                }
                *hint = canonical_hint;
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

/// Read a persisted binding without requiring local roots to be mounted.
/// Capacity probes and backend opens must continue using
/// `read_profile_binding`, which canonicalizes and fails closed when a root is
/// unavailable; diagnostics can use this record-level view to report drift.
pub fn read_profile_binding_record(
    path: impl AsRef<Path>,
    store_id: &str,
) -> Result<Option<BackendProfileBinding>, DaemonServiceRuntimeError> {
    let registry = read_registry(path.as_ref())?;
    Ok(registry
        .bindings
        .into_iter()
        .find(|binding| binding.manifest.store_id.as_str() == store_id))
}

pub fn upsert_profile_binding(
    path: impl AsRef<Path>,
    binding: BackendProfileBinding,
) -> Result<(), DaemonServiceRuntimeError> {
    let _guard = PROFILE_BINDING_WRITE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| DaemonServiceRuntimeError::UnsupportedOperation {
            operation: "profile binding registry write lock poisoned".to_string(),
        })?;
    let path = path.as_ref();
    let registry = prepare_registry_with_binding(path, binding)?;
    write_registry(path, &registry)
}

/// Roll back only the exact binding inserted by a failed provisioning
/// transaction. A changed binding is never removed by stale rollback work.
pub fn remove_profile_binding_if_matches(
    path: impl AsRef<Path>,
    expected: &BackendProfileBinding,
) -> Result<(), DaemonServiceRuntimeError> {
    let _guard = PROFILE_BINDING_WRITE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| DaemonServiceRuntimeError::UnsupportedOperation {
            operation: "profile binding registry write lock poisoned".to_string(),
        })?;
    let path = path.as_ref();
    let expected = expected.clone().validate_and_canonicalize()?;
    let mut registry = read_registry(path)?;
    let Some(index) = registry
        .bindings
        .iter()
        .position(|binding| binding.manifest.store_id == expected.manifest.store_id)
    else {
        return Ok(());
    };
    let existing = registry.bindings[index]
        .clone()
        .validate_and_canonicalize()?;
    if existing != expected {
        return Err(invalid_binding(format!(
            "refusing to roll back changed binding for ObjectStore {}",
            expected.manifest.store_id
        )));
    }
    registry.bindings.remove(index);
    write_registry(path, &registry)
}

/// Restore a prior binding only while the transaction's exact replacement is
/// still current. This prevents rollback from overwriting a concurrent update.
pub fn restore_profile_binding_if_matches(
    path: impl AsRef<Path>,
    expected_current: &BackendProfileBinding,
    previous: BackendProfileBinding,
) -> Result<(), DaemonServiceRuntimeError> {
    let _guard = PROFILE_BINDING_WRITE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| DaemonServiceRuntimeError::UnsupportedOperation {
            operation: "profile binding registry write lock poisoned".to_string(),
        })?;
    let path = path.as_ref();
    let expected_current = expected_current.clone().validate_and_canonicalize()?;
    let previous = previous.validate_and_canonicalize()?;
    let mut registry = read_registry(path)?;
    let Some(existing) = registry
        .bindings
        .iter_mut()
        .find(|binding| binding.manifest.store_id == expected_current.manifest.store_id)
    else {
        return Err(invalid_binding(format!(
            "cannot restore missing binding for ObjectStore {}",
            expected_current.manifest.store_id
        )));
    };
    if existing.clone().validate_and_canonicalize()? != expected_current {
        return Err(invalid_binding(format!(
            "refusing to overwrite changed binding for ObjectStore {}",
            expected_current.manifest.store_id
        )));
    }
    *existing = previous;
    validate_binding_claims(&registry.bindings)?;
    write_registry(path, &registry)
}

/// Validate a binding against the current registry without mutating it.
///
/// Profile registration uses this preflight before initializing a capacity
/// ledger.  A claim collision therefore cannot leave durable capacity state
/// for a binding that the registry will reject.
pub fn validate_profile_binding_claim(
    path: impl AsRef<Path>,
    binding: BackendProfileBinding,
) -> Result<(), DaemonServiceRuntimeError> {
    prepare_registry_with_binding(path.as_ref(), binding).map(|_| ())
}

fn prepare_registry_with_binding(
    path: &Path,
    binding: BackendProfileBinding,
) -> Result<ProfileBindingRegistryFile, DaemonServiceRuntimeError> {
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
    validate_binding_claims(&registry.bindings)?;
    registry.bindings.sort_by(|left, right| {
        left.manifest
            .store_id
            .as_str()
            .cmp(right.manifest.store_id.as_str())
    });
    Ok(registry)
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
        validate_persisted_paths(binding)?;
        if !store_ids.insert(binding.manifest.store_id.as_str()) {
            return Err(invalid_binding(format!(
                "duplicate profile binding for store {}",
                binding.manifest.store_id
            )));
        }
    }
    validate_binding_claims(&registry.bindings)?;
    Ok(registry)
}

fn validate_persisted_paths(
    binding: &BackendProfileBinding,
) -> Result<(), DaemonServiceRuntimeError> {
    validate_persisted_path("backend_root", &binding.backend_root)?;
    if let Some(staging_root) = &binding.ssd_staging_root {
        validate_persisted_path("ssd_staging_root", staging_root)?;
        if paths_overlap(&binding.backend_root, staging_root) {
            return Err(invalid_binding(format!(
                "store {} has an SSD staging root that overlaps its backend root",
                binding.manifest.store_id
            )));
        }
    }
    if let BackendReference::Drive {
        mount_path_hint: Some(hint),
        ..
    } = &binding.manifest.backend
    {
        validate_persisted_path("mount_path_hint", hint)?;
        if hint != &binding.backend_root {
            return Err(invalid_binding(format!(
                "drive mount_path_hint does not match persisted backend root for store {}",
                binding.manifest.store_id
            )));
        }
    }
    Ok(())
}

fn validate_persisted_path(
    field: &'static str,
    path: &Path,
) -> Result<(), DaemonServiceRuntimeError> {
    if !path.is_absolute() {
        return Err(invalid_binding(format!("{field} must be absolute")));
    }
    if path == Path::new("/") {
        return Err(invalid_binding(format!(
            "{field} must not be the system root"
        )));
    }
    if path
        .components()
        .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
    {
        return Err(invalid_binding(format!(
            "{field} must be normalized without '.' or '..' components"
        )));
    }
    Ok(())
}

/// Validate the daemon-owned one-to-one claims made by local profiles.
///
/// Portable manifests identify a backend, while this registry binds that
/// identity to local paths.  Claims are therefore checked here, where the
/// daemon can prevent two stores from sharing a root, staging tree, or device
/// identity before any capacity ledger is published.
fn validate_binding_claims(
    bindings: &[BackendProfileBinding],
) -> Result<(), DaemonServiceRuntimeError> {
    for binding in bindings {
        if let Some(staging_root) = &binding.ssd_staging_root {
            if paths_overlap(&binding.backend_root, staging_root) {
                return Err(invalid_binding(format!(
                    "store {} has an SSD staging root that overlaps its backend root",
                    binding.manifest.store_id
                )));
            }
        }
    }

    for (index, left_binding) in bindings.iter().enumerate() {
        for right_binding in bindings.iter().skip(index + 1) {
            if left_binding.manifest.store_id == right_binding.manifest.store_id {
                continue;
            }
            let left_staging = left_binding.ssd_staging_root.as_deref();
            let right_staging = right_binding.ssd_staging_root.as_deref();
            if paths_overlap(&left_binding.backend_root, &right_binding.backend_root)
                || left_staging
                    .is_some_and(|staging| paths_overlap(staging, &right_binding.backend_root))
                || right_staging
                    .is_some_and(|staging| paths_overlap(&left_binding.backend_root, staging))
                || left_staging
                    .zip(right_staging)
                    .is_some_and(|(left, right)| paths_overlap(left, right))
            {
                return Err(invalid_binding(format!(
                    "stores {} and {} claim overlapping backend or staging roots",
                    left_binding.manifest.store_id, right_binding.manifest.store_id
                )));
            }

            match (
                &left_binding.manifest.backend,
                &right_binding.manifest.backend,
            ) {
                (
                    BackendReference::Folder {
                        root_identity: left_identity,
                    },
                    BackendReference::Folder {
                        root_identity: right_identity,
                    },
                ) if left_identity == right_identity => {
                    return Err(invalid_binding(format!(
                        "stores {} and {} claim the same folder identity",
                        left_binding.manifest.store_id, right_binding.manifest.store_id
                    )));
                }
                (
                    BackendReference::Drive {
                        filesystem_identity: left_filesystem,
                        device_identity: left_device,
                        ..
                    },
                    BackendReference::Drive {
                        filesystem_identity: right_filesystem,
                        device_identity: right_device,
                        ..
                    },
                ) if left_filesystem == right_filesystem
                    || left_device.is_some() && left_device == right_device =>
                {
                    let identity = if left_filesystem == right_filesystem {
                        "filesystem identity"
                    } else {
                        "device identity"
                    };
                    return Err(invalid_binding(format!(
                        "stores {} and {} claim the same drive {identity}",
                        left_binding.manifest.store_id, right_binding.manifest.store_id
                    )));
                }
                (
                    BackendReference::Appliance { pool_id: left_pool },
                    BackendReference::Appliance {
                        pool_id: right_pool,
                    },
                ) if left_pool == right_pool => {
                    return Err(invalid_binding(format!(
                        "stores {} and {} claim the same appliance pool",
                        left_binding.manifest.store_id, right_binding.manifest.store_id
                    )));
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    left == right || left.starts_with(right) || right.starts_with(left)
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
    if canonical == Path::new("/") {
        return Err(invalid_binding(format!(
            "{field} must not be the system root"
        )));
    }
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
        default_profile_binding_registry_path, read_profile_binding, read_registry,
        remove_profile_binding_if_matches, restore_profile_binding_if_matches,
        upsert_profile_binding, BackendProfileBinding,
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

    #[test]
    fn default_binding_registry_path_is_state_scoped() {
        assert_eq!(
            default_profile_binding_registry_path("/var/lib/dasobjectstore"),
            PathBuf::from("/var/lib/dasobjectstore/profile-bindings.json")
        );
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

    fn drive_binding(store_id: &str, root: &Path) -> BackendProfileBinding {
        let mut binding = folder_binding(store_id, root);
        binding.manifest.deployment_profile = DeploymentProfile::Drive;
        binding.manifest.backend = BackendReference::Drive {
            filesystem_identity: format!("fsid:{store_id}"),
            device_identity: Some(format!("device:{store_id}")),
            media: dasobjectstore_core::manifest::DriveMediaKind::Ssd,
            mount_path_hint: Some(root.to_path_buf()),
        };
        binding
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
    fn rejects_system_root_folder_binding() {
        let root = root("system-root");
        let binding = folder_binding("folder", Path::new("/"));
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

    #[test]
    fn rejects_cross_store_overlapping_backend_and_staging_roots() {
        let root = root("claims");
        let path = root.join("bindings.json");
        let first = root.join("first");
        let first_staging = root.join("staging");
        let nested = first_staging.join("nested");
        fs::create_dir_all(&first).expect("first");
        fs::create_dir_all(&first_staging).expect("staging");
        fs::create_dir_all(&nested).expect("nested");

        let mut first_binding = folder_binding("first", &first);
        first_binding.ssd_staging_root = Some(first_staging.clone());
        upsert_profile_binding(&path, first_binding).expect("first binding");

        assert!(upsert_profile_binding(&path, folder_binding("nested", &nested)).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_duplicate_drive_identity_but_allows_distinct_mounts() {
        let root = root("drive-claims");
        let path = root.join("bindings.json");
        let first = root.join("first");
        let second = root.join("second");
        fs::create_dir_all(&first).expect("first");
        fs::create_dir_all(&second).expect("second");
        let first_binding = drive_binding("first", &first);
        upsert_profile_binding(&path, first_binding).expect("first binding");

        let mut duplicate = drive_binding("second", &second);
        duplicate.manifest.backend = BackendReference::Drive {
            filesystem_identity: "fsid:first".to_string(),
            device_identity: Some("device:second".to_string()),
            media: dasobjectstore_core::manifest::DriveMediaKind::Ssd,
            mount_path_hint: Some(second.clone()),
        };
        assert!(upsert_profile_binding(&path, duplicate).is_err());

        let mut duplicate_device = drive_binding("second", &second);
        duplicate_device.manifest.backend = BackendReference::Drive {
            filesystem_identity: "fsid:second".to_string(),
            device_identity: Some("device:first".to_string()),
            media: dasobjectstore_core::manifest::DriveMediaKind::Ssd,
            mount_path_hint: Some(second.clone()),
        };
        assert!(upsert_profile_binding(&path, duplicate_device).is_err());

        let mut distinct = drive_binding("second", &second);
        distinct.manifest.backend = BackendReference::Drive {
            filesystem_identity: "fsid:second".to_string(),
            device_identity: Some("device:second".to_string()),
            media: dasobjectstore_core::manifest::DriveMediaKind::Ssd,
            mount_path_hint: Some(second),
        };
        upsert_profile_binding(&path, distinct).expect("distinct drive binding");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn permits_idempotent_same_store_replacement() {
        let root = root("replacement");
        let path = root.join("bindings.json");
        let first = root.join("first");
        let second = root.join("second");
        fs::create_dir_all(&first).expect("first");
        fs::create_dir_all(&second).expect("second");
        upsert_profile_binding(&path, folder_binding("store", &first)).expect("first binding");
        upsert_profile_binding(&path, folder_binding("store", &second)).expect("replacement");
        assert_eq!(
            read_profile_binding(&path, "store")
                .expect("read")
                .expect("binding")
                .backend_root,
            fs::canonicalize(second).expect("canonical")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rollback_requires_exact_current_binding_and_restores_previous() {
        let root = root("rollback-exact");
        let path = root.join("bindings.json");
        let first = root.join("first");
        let second = root.join("second");
        let third = root.join("third");
        fs::create_dir_all(&first).expect("first");
        fs::create_dir_all(&second).expect("second");
        fs::create_dir_all(&third).expect("third");
        let previous = folder_binding("store", &first);
        let inserted = folder_binding("store", &second);
        upsert_profile_binding(&path, previous.clone()).expect("previous binding");
        upsert_profile_binding(&path, inserted.clone()).expect("transaction binding");
        restore_profile_binding_if_matches(&path, &inserted, previous.clone())
            .expect("exact rollback restores previous");
        assert_eq!(
            read_profile_binding(&path, "store")
                .expect("read")
                .expect("binding")
                .backend_root,
            fs::canonicalize(&first).expect("canonical first")
        );

        let changed = folder_binding("store", &third);
        upsert_profile_binding(&path, changed).expect("concurrent change");
        assert!(restore_profile_binding_if_matches(&path, &inserted, previous).is_err());
        assert!(remove_profile_binding_if_matches(&path, &inserted).is_err());
        assert_eq!(
            read_profile_binding(&path, "store")
                .expect("read")
                .expect("binding")
                .backend_root,
            fs::canonicalize(third).expect("canonical third")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn concurrent_binding_upserts_preserve_each_store() {
        let root = root("concurrent-upserts");
        let path = root.join("bindings.json");
        let first_root = root.join("first");
        let second_root = root.join("second");
        fs::create_dir_all(&first_root).expect("first root");
        fs::create_dir_all(&second_root).expect("second root");

        let first_path = path.clone();
        let first = std::thread::spawn(move || {
            upsert_profile_binding(&first_path, folder_binding("first", &first_root))
                .expect("first upsert")
        });
        let second_path = path.clone();
        let second = std::thread::spawn(move || {
            upsert_profile_binding(&second_path, folder_binding("second", &second_root))
                .expect("second upsert")
        });

        first.join().expect("first thread");
        second.join().expect("second thread");

        let registry = read_registry(&path).expect("registry reads");
        let stores: Vec<_> = registry
            .bindings
            .iter()
            .map(|binding| binding.manifest.store_id.as_str())
            .collect();
        assert_eq!(stores, vec!["first", "second"]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn record_read_survives_missing_backend_while_canonical_read_fails_closed() {
        let root = root("missing-backend");
        let path = root.join("bindings.json");
        let backend = root.join("backend");
        fs::create_dir_all(&backend).expect("backend");
        upsert_profile_binding(&path, folder_binding("store", &backend)).expect("upsert");
        let persisted_root = fs::canonicalize(&backend).expect("canonical root");
        fs::remove_dir_all(&backend).expect("backend removal");

        let record = super::read_profile_binding_record(&path, "store")
            .expect("record read")
            .expect("record");
        assert_eq!(record.backend_root, persisted_root);
        assert!(super::read_profile_binding(&path, "store").is_err());
        let _ = fs::remove_dir_all(root);
    }
}
