//! Local bounded folder backend implementation.

use dasobjectstore_core::backend::{
    BackendCapabilities, BackendError, BackendHealth, BackendObjectKey, BackendObjectRecord,
    ObjectStoreBackend,
};
use dasobjectstore_core::manifest::{BackendReference, ObjectStoreManifest};
use dasobjectstore_core::store::{CapacityPolicy, CapacityReservationLedger};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const NAMESPACE: &str = ".dasobjectstore";
const OBJECTS_DIR: &str = "objects";
const STAGING_DIR: &str = "staging";

#[derive(Debug)]
pub struct FolderBackend {
    root: PathBuf,
    objects_root: PathBuf,
    staging_root: PathBuf,
    manifest: ObjectStoreManifest,
    ledger: CapacityReservationLedger,
    staged_reservations: HashMap<PathBuf, String>,
}

impl FolderBackend {
    pub fn open(
        root: impl Into<PathBuf>,
        manifest: ObjectStoreManifest,
        capacity: CapacityPolicy,
        used_bytes: u64,
    ) -> Result<Self, BackendError> {
        manifest.validate().map_err(BackendError::Manifest)?;
        if !matches!(manifest.backend, BackendReference::Folder { .. }) {
            return Err(BackendError::InvalidRequest(
                "folder backend requires a folder manifest".to_string(),
            ));
        }
        if capacity.logical_limit_bytes.is_none() {
            return Err(BackendError::InvalidRequest(
                "folder backend requires a finite logical capacity limit".to_string(),
            ));
        }
        let root = root.into();
        if !root.is_absolute() {
            return Err(BackendError::InvalidRequest(
                "folder backend root must be absolute".to_string(),
            ));
        }
        fs::create_dir_all(&root).map_err(io_error)?;
        let root = fs::canonicalize(root).map_err(io_error)?;
        let namespace = root.join(NAMESPACE);
        let objects_root = namespace.join(OBJECTS_DIR);
        let staging_root = namespace.join(STAGING_DIR);
        ensure_directory(&namespace)?;
        ensure_directory(&objects_root)?;
        ensure_directory(&staging_root)?;
        Ok(Self {
            root,
            objects_root,
            staging_root,
            manifest,
            ledger: CapacityReservationLedger::new(capacity, used_bytes).map_err(|error| {
                BackendError::InvalidRequest(format!("capacity ledger: {error:?}"))
            })?,
            staged_reservations: HashMap::new(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn manifest(&self) -> &ObjectStoreManifest {
        &self.manifest
    }

    fn object_path(&self, key: &BackendObjectKey) -> Result<PathBuf, BackendError> {
        validate_object_key(key)?;
        Ok(self.objects_root.join(&key.object_id))
    }

    fn temporary_path(&self, reservation_id: &str) -> Result<PathBuf, BackendError> {
        if reservation_id.trim().is_empty()
            || !reservation_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(BackendError::InvalidRequest(
                "reservation ID must use ASCII letters, digits, '-' or '_'".to_string(),
            ));
        }
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
        Ok(self.staging_root.join(format!(
            "{reservation_id}-{}-{nonce}.part",
            std::process::id()
        )))
    }

    fn record_for_path(
        &self,
        key: BackendObjectKey,
        path: &Path,
        checksum: String,
    ) -> Result<BackendObjectRecord, BackendError> {
        let size_bytes = fs::metadata(path).map_err(io_error)?.len();
        let location = path
            .strip_prefix(&self.root)
            .map_err(|_| BackendError::InvalidRequest("backend path escaped root".to_string()))?
            .display()
            .to_string();
        Ok(BackendObjectRecord {
            key,
            size_bytes,
            checksum,
            location,
        })
    }
}

impl ObjectStoreBackend for FolderBackend {
    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities::complete()
    }

    fn validate_manifest(&self, manifest: &ObjectStoreManifest) -> Result<(), BackendError> {
        manifest.validate().map_err(BackendError::Manifest)
    }

    fn reserve(&mut self, reservation_id: &str, bytes: u64) -> Result<(), BackendError> {
        self.ledger
            .reserve(reservation_id.to_string(), bytes)
            .map_err(|error| {
                BackendError::InvalidRequest(format!("capacity reservation: {error:?}"))
            })
    }

    fn stage(
        &mut self,
        reservation_id: &str,
        key: &BackendObjectKey,
        source: &mut dyn Read,
    ) -> Result<BackendObjectRecord, BackendError> {
        validate_object_key(key)?;
        let temporary_path = self.temporary_path(reservation_id)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_path)
            .map_err(io_error)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        let result = (|| {
            loop {
                let read = source.read(&mut buffer).map_err(io_error)?;
                if read == 0 {
                    break;
                }
                file.write_all(&buffer[..read]).map_err(io_error)?;
                hasher.update(&buffer[..read]);
            }
            file.sync_all().map_err(io_error)?;
            let checksum = format!("sha256:{:x}", hasher.finalize());
            let record = self.record_for_path(key.clone(), &temporary_path, checksum)?;
            self.staged_reservations
                .insert(temporary_path.clone(), reservation_id.to_string());
            Ok(record)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary_path);
        }
        result
    }

    fn finalize(
        &mut self,
        staged: BackendObjectRecord,
    ) -> Result<BackendObjectRecord, BackendError> {
        let temporary_path = self.root.join(&staged.location);
        let reservation_id = self
            .staged_reservations
            .remove(&temporary_path)
            .ok_or_else(|| {
                BackendError::InvalidRequest("unknown staged folder object".to_string())
            })?;
        if !temporary_path.starts_with(&self.staging_root) {
            return Err(BackendError::InvalidRequest(
                "staged object is outside the private staging directory".to_string(),
            ));
        }
        let destination = self.object_path(&staged.key)?;
        if destination.exists() {
            return Err(BackendError::InvalidRequest(
                "object destination already exists".to_string(),
            ));
        }
        let parent = destination.parent().ok_or_else(|| {
            BackendError::InvalidRequest("object destination has no parent".to_string())
        })?;
        ensure_safe_parent(&self.objects_root, parent)?;
        fs::rename(&temporary_path, &destination).map_err(io_error)?;
        sync_directory(parent)?;
        self.ledger
            .commit(&reservation_id)
            .map_err(|error| BackendError::InvalidRequest(format!("capacity commit: {error:?}")))?;
        self.record_for_path(staged.key, &destination, staged.checksum)
    }

    fn read(&self, key: &BackendObjectKey) -> Result<Box<dyn Read + Send>, BackendError> {
        Ok(Box::new(
            File::open(self.object_path(key)?).map_err(io_error)?,
        ))
    }

    fn enumerate(&self, prefix: Option<&str>) -> Result<Vec<BackendObjectRecord>, BackendError> {
        let mut records = Vec::new();
        enumerate_files(&self.objects_root, &self.objects_root, prefix, &mut records)?;
        Ok(records)
    }

    fn verify(&self, key: &BackendObjectKey) -> Result<BackendObjectRecord, BackendError> {
        let path = self.object_path(key)?;
        let mut file = File::open(&path).map_err(io_error)?;
        let checksum = hash_reader(&mut file)?;
        self.record_for_path(key.clone(), &path, checksum)
    }

    fn health(&self) -> Result<BackendHealth, BackendError> {
        Ok(BackendHealth {
            state: if self.root.is_dir() {
                "healthy"
            } else {
                "unavailable"
            }
            .to_string(),
            message: None,
        })
    }

    fn reconcile(&mut self) -> Result<Vec<BackendObjectRecord>, BackendError> {
        self.enumerate(None)
    }

    fn remove(&mut self, key: &BackendObjectKey) -> Result<(), BackendError> {
        fs::remove_file(self.object_path(key)?).map_err(io_error)
    }
}

fn enumerate_files(
    root: &Path,
    directory: &Path,
    prefix: Option<&str>,
    records: &mut Vec<BackendObjectRecord>,
) -> Result<(), BackendError> {
    for entry in fs::read_dir(directory).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(io_error)?;
        if file_type.is_symlink() {
            return Err(BackendError::InvalidRequest(
                "folder backend encountered a symlink entry".to_string(),
            ));
        }
        if file_type.is_dir() {
            enumerate_files(root, &path, prefix, records)?;
            continue;
        }
        if !file_type.is_file() {
            return Err(BackendError::InvalidRequest(
                "folder backend encountered a non-regular file".to_string(),
            ));
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| BackendError::InvalidRequest("backend path escaped root".to_string()))?;
        let object_id = relative
            .to_string_lossy()
            .replace(std::path::MAIN_SEPARATOR, "/");
        if prefix.is_some_and(|prefix| !object_id.starts_with(prefix)) {
            continue;
        }
        let key = BackendObjectKey {
            object_id,
            version: 1,
        };
        let mut file = File::open(&path).map_err(io_error)?;
        let checksum = hash_reader(&mut file)?;
        let size_bytes = fs::metadata(&path).map_err(io_error)?.len();
        records.push(BackendObjectRecord {
            key,
            size_bytes,
            checksum,
            location: path.display().to_string(),
        });
    }
    Ok(())
}

fn hash_reader(reader: &mut dyn Read) -> Result<String, BackendError> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer).map_err(io_error)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn validate_object_key(key: &BackendObjectKey) -> Result<(), BackendError> {
    if key.object_id.trim().is_empty()
        || key.object_id.starts_with('/')
        || key.object_id.contains('\\')
        || key.object_id.split('/').any(|component| {
            component.is_empty()
                || component == "."
                || component == ".."
                || component.starts_with('.')
        })
        || key
            .object_id
            .split('/')
            .any(|component| component == NAMESPACE)
    {
        return Err(BackendError::InvalidRequest(
            "object key contains an unsafe path component".to_string(),
        ));
    }
    Ok(())
}

fn sync_directory(path: &Path) -> Result<(), BackendError> {
    File::open(path)
        .map_err(io_error)?
        .sync_all()
        .map_err(io_error)
}

fn ensure_directory(path: &Path) -> Result<(), BackendError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(BackendError::InvalidRequest(format!(
                "folder backend namespace cannot be a symlink: {}",
                path.display()
            )))
        }
        Ok(metadata) if !metadata.is_dir() => Err(BackendError::InvalidRequest(format!(
            "folder backend namespace is not a directory: {}",
            path.display()
        ))),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(io_error)
        }
        Err(error) => Err(io_error(error)),
    }
}

fn ensure_safe_parent(root: &Path, parent: &Path) -> Result<(), BackendError> {
    let relative = parent.strip_prefix(root).map_err(|_| {
        BackendError::InvalidRequest("object parent escaped backend root".to_string())
    })?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        ensure_directory(&current)?;
    }
    Ok(())
}

fn io_error(error: std::io::Error) -> BackendError {
    BackendError::Io(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::FolderBackend;
    use dasobjectstore_core::backend::{BackendObjectKey, ObjectStoreBackend};
    use dasobjectstore_core::deployment::{DeploymentProfile, HostMode};
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_core::manifest::{
        BackendReference, ObjectStoreManifest, OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
    };
    use dasobjectstore_core::protection::ProtectionPolicy;
    use dasobjectstore_core::store::CapacityPolicy;
    use std::fs;
    use std::io::{Cursor, Read};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn folder_backend_hashes_stages_fsyncs_renames_and_reads() {
        let root = unique_root();
        let mut backend =
            FolderBackend::open(&root, manifest(), CapacityPolicy::bounded(1024, 64), 0)
                .expect("folder backend opens");
        let key = BackendObjectKey {
            object_id: "sample/run/data.txt".to_string(),
            version: 1,
        };
        backend.reserve("upload-1", 11).expect("reserves capacity");
        let staged = backend
            .stage("upload-1", &key, &mut Cursor::new(b"hello world".to_vec()))
            .expect("stages object");
        assert!(staged.location.contains(".dasobjectstore/staging"));
        let finalized = backend.finalize(staged).expect("finalizes object");
        assert_eq!(
            finalized.checksum,
            "sha256:b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
        assert_eq!(
            finalized.location,
            ".dasobjectstore/objects/sample/run/data.txt"
        );
        let mut read = String::new();
        backend
            .read(&key)
            .expect("opens object")
            .read_to_string(&mut read)
            .expect("reads object");
        assert_eq!(read, "hello world");
        assert_eq!(
            backend.verify(&key).expect("verifies object").checksum,
            finalized.checksum
        );
        assert_eq!(
            backend
                .enumerate(Some("sample/"))
                .expect("enumerates")
                .len(),
            1
        );
        backend.remove(&key).expect("removes object");
        assert!(backend
            .enumerate(None)
            .expect("enumerates empty")
            .is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn folder_backend_rejects_unsafe_keys_and_capacity_overbook() {
        let root = unique_root();
        let mut backend = FolderBackend::open(&root, manifest(), CapacityPolicy::bounded(10, 2), 0)
            .expect("folder backend opens");
        assert!(backend.reserve("upload-1", 9).is_err());
        let unsafe_key = BackendObjectKey {
            object_id: "../escape".to_string(),
            version: 1,
        };
        backend.reserve("upload-2", 1).expect("reserves capacity");
        assert!(backend
            .stage("upload-2", &unsafe_key, &mut Cursor::new(b"x".to_vec()))
            .is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn folder_backend_rejects_symlink_escape() {
        let root = unique_root();
        let outside = root.with_extension("outside");
        let mut backend =
            FolderBackend::open(&root, manifest(), CapacityPolicy::bounded(1024, 1), 0)
                .expect("folder backend opens");
        fs::create_dir_all(&outside).expect("outside directory creates");
        std::os::unix::fs::symlink(&outside, root.join(".dasobjectstore/objects/escape"))
            .expect("symlink creates");
        backend
            .reserve("upload-link", 1)
            .expect("reserves capacity");
        let key = BackendObjectKey {
            object_id: "escape/file.txt".to_string(),
            version: 1,
        };
        let staged = backend
            .stage("upload-link", &key, &mut Cursor::new(b"x".to_vec()))
            .expect("stages outside destination safely");
        assert!(backend.finalize(staged).is_err());
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }

    #[test]
    fn folder_backend_rejects_legacy_unbounded_capacity() {
        let root = unique_root();
        let error = FolderBackend::open(&root, manifest(), CapacityPolicy::default(), 0)
            .expect_err("folder backend must be bounded");
        assert!(format!("{error:?}").contains("finite logical capacity"));
    }

    fn manifest() -> ObjectStoreManifest {
        ObjectStoreManifest {
            schema_version: OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
            store_id: StoreId::new("codex-folder").expect("store id"),
            deployment_profile: DeploymentProfile::Folder,
            host_mode: HostMode::PerUser,
            protection: ProtectionPolicy::LocalOnly,
            backend: BackendReference::Folder {
                root_identity: "test-root".to_string(),
            },
        }
    }

    fn unique_root() -> PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let parent = std::env::var_os("DASOBJECTSTORE_CODEX_VALIDATION_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        parent.join(format!(
            "dasobjectstore-folder-backend-{}-{now}",
            std::process::id()
        ))
    }
}
