//! Local bounded folder backend implementation.

use super::folder_catalogue::{
    FolderCatalogue, FolderCatalogueBrowserEntry, FolderCatalogueBrowserQuery,
};
use super::reconciliation::{
    plan_reconciliation, ReconciliationAction, ReconciliationEntryState, ReconciliationManifest,
    ReconciliationObject, ReconciliationPlan,
};
use dasobjectstore_core::backend::{
    catalogue_logical_used_bytes, BackendCapabilities, BackendError, BackendHealth,
    BackendObjectKey, BackendObjectRecord, ObjectCatalogueAuthority, ObjectStoreBackend,
};
use dasobjectstore_core::manifest::{BackendReference, ObjectStoreManifest};
use dasobjectstore_core::store::{CapacityPolicy, CapacityReservationLedger};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
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
    catalogue: FolderCatalogue,
    manifest: ObjectStoreManifest,
    ledger: CapacityReservationLedger,
    staged_reservations: HashMap<PathBuf, String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FolderInspectionReport {
    pub unmanaged_paths: Vec<String>,
    pub unsafe_paths: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FolderReconciliationPlan {
    pub inspection: FolderInspectionReport,
    pub manifest: ReconciliationManifest,
    pub plan: ReconciliationPlan,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FolderCapacitySnapshot {
    pub used_bytes: u64,
    pub reserved_bytes: u64,
    pub available_bytes: Option<u64>,
    pub logical_limit_bytes: Option<u64>,
    pub backend_reserve_bytes: u64,
}

impl FolderInspectionReport {
    pub fn is_clean(&self) -> bool {
        self.unmanaged_paths.is_empty() && self.unsafe_paths.is_empty()
    }
}

impl FolderBackend {
    /// Inspect an existing folder root without creating the private managed
    /// namespace or catalogue. This is used by read-only diagnostics when a
    /// persisted binding may refer to an unmounted or missing root.
    pub fn inspect_user_tree_at(
        root: impl AsRef<Path>,
    ) -> Result<FolderInspectionReport, BackendError> {
        let root = root.as_ref();
        let metadata = fs::symlink_metadata(root).map_err(io_error)?;
        if !metadata.is_dir() {
            return Err(BackendError::InvalidRequest(
                "folder inspection root is not a directory".to_string(),
            ));
        }
        let mut report = FolderInspectionReport::default();
        inspect_user_tree(root, root, &mut report)?;
        Ok(report)
    }

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
        let catalogue_path = namespace.join("catalogue.json");
        ensure_private_directory(&namespace)?;
        ensure_private_directory(&objects_root)?;
        ensure_private_directory(&staging_root)?;
        let catalogue = FolderCatalogue::open(catalogue_path, manifest.store_id.as_str())?;
        let catalogued_used_bytes = catalogue_logical_used_bytes(&catalogue)?;
        if used_bytes != 0 && used_bytes != catalogued_used_bytes {
            return Err(BackendError::InvalidRequest(format!(
                "folder catalogue used bytes {catalogued_used_bytes} do not match supplied accounting {used_bytes}"
            )));
        }
        let used_bytes = used_bytes.max(catalogued_used_bytes);
        Ok(Self {
            root,
            objects_root,
            staging_root,
            catalogue,
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

    pub fn catalogue_records(&self) -> Vec<BackendObjectRecord> {
        self.catalogue.records()
    }

    pub(crate) fn catalogue_authority_commit_batch(
        &mut self,
        records: &[BackendObjectRecord],
    ) -> Result<(), BackendError> {
        self.catalogue.commit_records(records.iter().cloned())
    }

    pub(crate) fn catalogue_authority_remove_record(
        &mut self,
        key: &BackendObjectKey,
    ) -> Result<(), BackendError> {
        self.catalogue.remove(key)
    }

    /// Return a guarded, profile-neutral browser projection over the durable
    /// private catalogue. This never walks payload files or user-visible data.
    pub fn browser_entries(
        &self,
        query: &FolderCatalogueBrowserQuery,
    ) -> Result<Vec<FolderCatalogueBrowserEntry>, BackendError> {
        self.catalogue.browser_entries(query)
    }

    /// Inspect user-visible hierarchy without adopting or mutating it.
    pub fn inspect_user_tree(&self) -> Result<FolderInspectionReport, BackendError> {
        let mut report = FolderInspectionReport::default();
        inspect_user_tree(&self.root, &self.root, &mut report)?;
        Ok(report)
    }

    /// Build a resumable, read-only reconciliation plan for unmanaged regular
    /// files. The plan never adopts or mutates user files; unsafe entries stay
    /// visible in the inspection report and are not made authoritative.
    pub fn plan_user_tree_reconciliation(&self) -> Result<FolderReconciliationPlan, BackendError> {
        let inspection = self.inspect_user_tree()?;
        let mut manifest = ReconciliationManifest::new(self.manifest.store_id.as_str(), None);
        let plan = self.replan_user_tree_reconciliation(&mut manifest)?;
        Ok(FolderReconciliationPlan {
            inspection,
            manifest,
            plan,
        })
    }

    /// Replan against a caller-owned checkpoint without adopting or mutating
    /// user files. The checkpoint must belong to this logical store.
    pub fn replan_user_tree_reconciliation(
        &self,
        manifest: &mut ReconciliationManifest,
    ) -> Result<ReconciliationPlan, BackendError> {
        if manifest.store_id != self.manifest.store_id.as_str() {
            return Err(BackendError::InvalidRequest(
                "reconciliation manifest belongs to a different ObjectStore".to_string(),
            ));
        }
        let inspection = self.inspect_user_tree()?;
        let objects = inspection
            .unmanaged_paths
            .iter()
            .map(|relative_path| {
                let path = self.root.join(relative_path);
                let metadata = fs::symlink_metadata(&path).map_err(io_error)?;
                if !metadata.is_file() {
                    return Err(BackendError::InvalidRequest(format!(
                        "folder entry changed during reconciliation: {relative_path}"
                    )));
                }
                Ok(ReconciliationObject {
                    key: relative_path.clone(),
                    size_bytes: Some(metadata.len()),
                    source_revision: Some(source_revision(&metadata)),
                })
            })
            .collect::<Result<Vec<_>, BackendError>>()?;
        Ok(plan_reconciliation(manifest, &objects))
    }

    /// Explicitly adopt the current read-only reconciliation plan into the
    /// private managed namespace. User files are copied through the stable
    /// source path primitive, verified, durably finalized, and only then
    /// marked complete in the caller-owned checkpoint. The caller chooses the
    /// checkpoint location and must treat the resulting records as input to
    /// its catalogue transaction; this method never mutates user files.
    pub fn adopt_user_tree_reconciliation(
        &mut self,
        checkpoint_path: &Path,
        manifest: &mut ReconciliationManifest,
        reservation_prefix: &str,
    ) -> Result<Vec<BackendObjectRecord>, BackendError> {
        if reservation_prefix.trim().is_empty()
            || !reservation_prefix
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(BackendError::InvalidRequest(
                "reconciliation reservation prefix must use ASCII letters, digits, '-' or '_'"
                    .to_string(),
            ));
        }
        let plan = self.replan_user_tree_reconciliation(manifest)?;
        let mut records = Vec::new();
        for (index, action) in plan.actions.iter().enumerate() {
            let (key, relative_path, size_bytes) = match action {
                ReconciliationAction::Download {
                    key,
                    relative_path,
                    size_bytes,
                }
                | ReconciliationAction::Resume {
                    key,
                    relative_path,
                    size_bytes,
                    ..
                } => (key, relative_path, *size_bytes),
                ReconciliationAction::SkipComplete { .. }
                | ReconciliationAction::InvalidKey { .. }
                | ReconciliationAction::Collision { .. } => continue,
            };
            let source_path = self.root.join(relative_path);
            let size_bytes = size_bytes
                .or_else(|| {
                    fs::symlink_metadata(&source_path)
                        .ok()
                        .map(|metadata| metadata.len())
                })
                .ok_or_else(|| {
                    BackendError::InvalidRequest(format!(
                        "reconciliation source size is unavailable: {relative_path}"
                    ))
                })?;
            let reservation_id = format!("{reservation_prefix}-{index}");
            manifest
                .checkpoint(
                    checkpoint_path,
                    key,
                    ReconciliationEntryState::InProgress,
                    None,
                    0,
                )
                .map_err(reconciliation_error)?;
            let object_key = BackendObjectKey {
                object_id: relative_path.clone(),
                version: 1,
            };
            let finalized = (|| {
                self.reserve(&reservation_id, size_bytes)?;
                let staged = match self.stage_path(&reservation_id, &object_key, &source_path) {
                    Ok(staged) => staged,
                    Err(error) => {
                        let _ = self.release_reservation(&reservation_id);
                        return Err(error);
                    }
                };
                match self.finalize(staged.clone()) {
                    Ok(finalized) => Ok(finalized),
                    Err(error) => {
                        self.discard_staged(&staged, &reservation_id);
                        Err(error)
                    }
                }
            })();
            let finalized = match finalized {
                Ok(finalized) => finalized,
                Err(error) => {
                    let _ = manifest.checkpoint(
                        checkpoint_path,
                        key,
                        ReconciliationEntryState::Failed,
                        Some(error.to_string()),
                        0,
                    );
                    return Err(error);
                }
            };
            self.catalogue_authority_commit_batch(&[finalized.clone()])
                .map_err(|error| {
                    let _ = manifest.checkpoint(
                        checkpoint_path,
                        key,
                        ReconciliationEntryState::Failed,
                        Some(format!("catalogue commit failed: {error}")),
                        size_bytes,
                    );
                    error
                })?;
            manifest
                .checkpoint(
                    checkpoint_path,
                    key,
                    ReconciliationEntryState::Complete,
                    None,
                    size_bytes,
                )
                .map_err(reconciliation_error)?;
            records.push(finalized);
        }
        Ok(records)
    }

    pub fn capacity(&self) -> FolderCapacitySnapshot {
        let policy = self.ledger.policy();
        FolderCapacitySnapshot {
            used_bytes: self.ledger.used_bytes(),
            reserved_bytes: self.ledger.reserved_bytes(),
            available_bytes: self.ledger.available_bytes(),
            logical_limit_bytes: policy.logical_limit_bytes,
            backend_reserve_bytes: policy.backend_reserve_bytes,
        }
    }

    /// Stage an existing regular file only when its contents remain stable for
    /// the complete read. This is the safe ingress primitive for a future
    /// explicit adoption/reconciliation workflow; it does not grant authority
    /// over the source hierarchy.
    pub fn stage_path(
        &mut self,
        reservation_id: &str,
        key: &BackendObjectKey,
        source_path: &Path,
    ) -> Result<BackendObjectRecord, BackendError> {
        reject_ambiguous_source(source_path)?;
        let mut source = File::open(source_path).map_err(io_error)?;
        let staged = self.stage(reservation_id, key, &mut source)?;
        if let Err(error) = reject_ambiguous_source(source_path) {
            self.discard_staged(&staged, reservation_id);
            return Err(error);
        }
        let mut verification_source = match File::open(source_path) {
            Ok(file) => file,
            Err(error) => {
                self.discard_staged(&staged, reservation_id);
                return Err(io_error(error));
            }
        };
        let current_checksum = match hash_reader(&mut verification_source) {
            Ok(checksum) => checksum,
            Err(error) => {
                self.discard_staged(&staged, reservation_id);
                return Err(error);
            }
        };
        if current_checksum != staged.checksum {
            self.discard_staged(&staged, reservation_id);
            return Err(BackendError::InvalidRequest(
                "source file changed during folder import".to_string(),
            ));
        }
        Ok(staged)
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
        size_bytes: u64,
    ) -> Result<BackendObjectRecord, BackendError> {
        record_for_path(&self.root, key, path, checksum, size_bytes)
    }

    fn discard_staged(&mut self, staged: &BackendObjectRecord, reservation_id: &str) {
        let temporary_path = self.root.join(&staged.location);
        self.staged_reservations.remove(&temporary_path);
        let _ = fs::remove_file(temporary_path);
        let _ = self.ledger.release(reservation_id);
    }

    pub(crate) fn release_reservation(&mut self, reservation_id: &str) -> Result<(), BackendError> {
        self.ledger
            .release(reservation_id)
            .map(|_| ())
            .map_err(|error| BackendError::InvalidRequest(format!("capacity release: {error:?}")))
    }

    pub(crate) fn abort_staged_profile_object(
        &mut self,
        reservation_id: &str,
        staged: Option<&BackendObjectRecord>,
    ) -> Result<(), BackendError> {
        if let Some(staged) = staged {
            self.discard_staged(staged, reservation_id);
            Ok(())
        } else {
            self.release_reservation(reservation_id)
        }
    }
}

impl ObjectStoreBackend for FolderBackend {
    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities::complete()
    }

    fn validate_manifest(&self, manifest: &ObjectStoreManifest) -> Result<(), BackendError> {
        manifest.validate().map_err(BackendError::Manifest)?;
        if manifest != &self.manifest
            || !matches!(manifest.backend, BackendReference::Folder { .. })
        {
            return Err(BackendError::InvalidRequest(
                "folder manifest identity does not match this backend".to_string(),
            ));
        }
        Ok(())
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
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary_path).map_err(io_error)?;
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
            let size_bytes = file.metadata().map_err(io_error)?.len();
            let reserved_bytes =
                self.ledger
                    .reservation_bytes(reservation_id)
                    .ok_or_else(|| {
                        BackendError::InvalidRequest("unknown capacity reservation".to_string())
                    })?;
            if size_bytes != reserved_bytes {
                return Err(BackendError::InvalidRequest(format!(
                    "staged object size {size_bytes} does not match reserved bytes {reserved_bytes}"
                )));
            }
            let record =
                self.record_for_path(key.clone(), &temporary_path, checksum, size_bytes)?;
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
            .get(&temporary_path)
            .cloned()
            .ok_or_else(|| {
                BackendError::InvalidRequest("unknown staged folder object".to_string())
            })?;
        if !temporary_path.starts_with(&self.staging_root) {
            return Err(BackendError::InvalidRequest(
                "staged object is outside the private staging directory".to_string(),
            ));
        }
        let (checksum, size_bytes) = hash_stable_file(&temporary_path)?;
        if checksum != staged.checksum || size_bytes != staged.size_bytes {
            return Err(BackendError::InvalidRequest(
                "staged folder object changed before finalization".to_string(),
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
        self.staged_reservations.remove(&temporary_path);
        self.ledger
            .commit(&reservation_id)
            .map_err(|error| BackendError::InvalidRequest(format!("capacity commit: {error:?}")))?;
        self.record_for_path(staged.key, &destination, staged.checksum, size_bytes)
    }

    fn read(&self, key: &BackendObjectKey) -> Result<Box<dyn Read + Send>, BackendError> {
        Ok(Box::new(
            File::open(self.object_path(key)?).map_err(io_error)?,
        ))
    }

    fn read_range(
        &self,
        key: &BackendObjectKey,
        offset: u64,
        length: u64,
    ) -> Result<Box<dyn Read + Send>, BackendError> {
        let mut file = File::open(self.object_path(key)?).map_err(io_error)?;
        let size = file.metadata().map_err(io_error)?.len();
        if offset > size {
            return Err(BackendError::InvalidRequest(
                "folder object range starts beyond object size".to_string(),
            ));
        }
        file.seek(SeekFrom::Start(offset)).map_err(io_error)?;
        Ok(Box::new(file.take(length.min(size - offset))))
    }

    fn enumerate(&self, prefix: Option<&str>) -> Result<Vec<BackendObjectRecord>, BackendError> {
        let mut records = Vec::new();
        enumerate_files(
            &self.objects_root,
            &self.root,
            &self.objects_root,
            prefix,
            &mut records,
        )?;
        Ok(records)
    }

    fn verify(&self, key: &BackendObjectKey) -> Result<BackendObjectRecord, BackendError> {
        let path = self.object_path(key)?;
        let (checksum, size_bytes) = hash_stable_file(&path)?;
        self.record_for_path(key.clone(), &path, checksum, size_bytes)
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
        let path = self.object_path(key)?;
        let (checksum, size_bytes) = hash_stable_file(&path)?;
        if size_bytes > self.ledger.used_bytes() {
            return Err(BackendError::InvalidRequest(
                "folder capacity accounting is below object size".to_string(),
            ));
        }
        let record = self.record_for_path(key.clone(), &path, checksum, size_bytes)?;
        // Remove the durable catalogue record first. If persistence fails, the
        // payload and logical accounting remain untouched. If payload removal
        // then fails, restore the record before returning so a retry remains
        // authoritative and fail-closed.
        self.catalogue.remove(key)?;
        if let Err(error) = fs::remove_file(&path).map_err(io_error) {
            let restore_error = self.catalogue.commit_records([record]);
            return Err(match restore_error {
                Ok(()) => error,
                Err(restore_error) => BackendError::InvalidRequest(format!(
                    "folder payload removal failed ({error}) and catalogue restore failed ({restore_error})"
                )),
            });
        }
        self.ledger
            .debit_used_bytes(size_bytes)
            .map_err(|error| BackendError::InvalidRequest(format!("capacity debit: {error:?}")))?;
        if let Some(parent) = path.parent() {
            sync_directory(parent)?;
        }
        Ok(())
    }
}

impl ObjectCatalogueAuthority for FolderBackend {
    fn records(&self) -> Result<Vec<BackendObjectRecord>, BackendError> {
        Ok(self.catalogue_records())
    }

    fn commit_batch(&mut self, records: &[BackendObjectRecord]) -> Result<(), BackendError> {
        self.catalogue_authority_commit_batch(records)
    }

    fn remove_record(&mut self, key: &BackendObjectKey) -> Result<(), BackendError> {
        self.catalogue_authority_remove_record(key)
    }
}

fn enumerate_files(
    object_root: &Path,
    location_root: &Path,
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
            enumerate_files(object_root, location_root, &path, prefix, records)?;
            continue;
        }
        if !file_type.is_file() {
            return Err(BackendError::InvalidRequest(
                "folder backend encountered a non-regular file".to_string(),
            ));
        }
        let relative = path
            .strip_prefix(object_root)
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
        let (checksum, size_bytes) = hash_stable_file(&path)?;
        records.push(record_for_path(
            location_root,
            key,
            &path,
            checksum,
            size_bytes,
        )?);
    }
    Ok(())
}

fn record_for_path(
    root: &Path,
    key: BackendObjectKey,
    path: &Path,
    checksum: String,
    size_bytes: u64,
) -> Result<BackendObjectRecord, BackendError> {
    let location = path
        .strip_prefix(root)
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

#[derive(Clone, Debug, Eq, PartialEq)]
struct FileSnapshot {
    len: u64,
    modified: Option<std::time::SystemTime>,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    links: u64,
}

fn snapshot(metadata: &Metadata) -> FileSnapshot {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        FileSnapshot {
            len: metadata.len(),
            modified: metadata.modified().ok(),
            device: metadata.dev(),
            inode: metadata.ino(),
            links: metadata.nlink(),
        }
    }
    #[cfg(not(unix))]
    {
        FileSnapshot {
            len: metadata.len(),
            modified: metadata.modified().ok(),
        }
    }
}

fn source_revision(metadata: &Metadata) -> String {
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| format!("{}:{}", duration.as_secs(), duration.subsec_nanos()))
        .unwrap_or_else(|| "unknown".to_string());
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        return format!(
            "unix:{}:{}:{}:{}:{}:{}",
            metadata.dev(),
            metadata.ino(),
            metadata.nlink(),
            metadata.len(),
            modified,
            metadata.mode()
        );
    }
    #[cfg(not(unix))]
    {
        format!("portable:{}:{}", metadata.len(), modified)
    }
}

fn reconciliation_error(error: super::reconciliation::ReconciliationManifestError) -> BackendError {
    BackendError::InvalidRequest(format!("reconciliation checkpoint: {error}"))
}

fn hash_stable_file(path: &Path) -> Result<(String, u64), BackendError> {
    hash_stable_file_with_hook(path, None)
}

fn hash_stable_file_with_hook(
    path: &Path,
    _after_read: Option<&dyn Fn()>,
) -> Result<(String, u64), BackendError> {
    let mut file = File::open(path).map_err(io_error)?;
    let before = file.metadata().map_err(io_error)?;
    if !before.is_file() {
        return Err(BackendError::InvalidRequest(
            "folder backend requires regular files".to_string(),
        ));
    }
    let before_snapshot = snapshot(&before);
    #[cfg(unix)]
    if before_snapshot.links != 1 {
        return Err(BackendError::InvalidRequest(
            "folder backend encountered a hard-linked object".to_string(),
        ));
    }
    let (checksum, bytes_read) = hash_reader_count(&mut file)?;
    #[cfg(test)]
    if let Some(after_read) = _after_read {
        after_read();
    }
    let after_snapshot = snapshot(&file.metadata().map_err(io_error)?);
    if before_snapshot != after_snapshot || bytes_read != before_snapshot.len {
        return Err(BackendError::InvalidRequest(
            "folder backend file changed during verification".to_string(),
        ));
    }
    let path_metadata = fs::symlink_metadata(path).map_err(io_error)?;
    if path_metadata.file_type().is_symlink()
        || !same_file(&before_snapshot, &snapshot(&path_metadata))
    {
        return Err(BackendError::InvalidRequest(
            "folder backend file was replaced during verification".to_string(),
        ));
    }
    Ok((checksum, before_snapshot.len))
}

fn same_file(left: &FileSnapshot, right: &FileSnapshot) -> bool {
    #[cfg(unix)]
    {
        left.device == right.device
            && left.inode == right.inode
            && left.links == right.links
            && left.len == right.len
            && left.modified == right.modified
    }
    #[cfg(not(unix))]
    {
        left.len == right.len && left.modified == right.modified
    }
}

fn inspect_user_tree(
    root: &Path,
    directory: &Path,
    report: &mut FolderInspectionReport,
) -> Result<(), BackendError> {
    for entry in fs::read_dir(directory).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|_| BackendError::InvalidRequest("inspection path escaped root".to_string()))?
            .display()
            .to_string();
        if relative == NAMESPACE || relative.starts_with(&format!("{NAMESPACE}/")) {
            continue;
        }
        let file_type = entry.file_type().map_err(io_error)?;
        if file_type.is_symlink() || (!file_type.is_dir() && !file_type.is_file()) {
            report.unsafe_paths.push(relative);
        } else if file_type.is_dir() {
            inspect_user_tree(root, &path, report)?;
        } else if is_ambiguous_hard_link(&path)? {
            report.unsafe_paths.push(relative);
        } else {
            report.unmanaged_paths.push(relative);
        }
    }
    Ok(())
}

fn hash_reader(reader: &mut dyn Read) -> Result<String, BackendError> {
    Ok(hash_reader_count(reader)?.0)
}

fn hash_reader_count(reader: &mut dyn Read) -> Result<(String, u64), BackendError> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut bytes_read = 0_u64;
    loop {
        let read = reader.read(&mut buffer).map_err(io_error)?;
        if read == 0 {
            break;
        }
        bytes_read = bytes_read
            .checked_add(read as u64)
            .ok_or_else(|| BackendError::InvalidRequest("file size overflow".to_string()))?;
        hasher.update(&buffer[..read]);
    }
    Ok((format!("sha256:{:x}", hasher.finalize()), bytes_read))
}

fn reject_ambiguous_source(path: &Path) -> Result<(), BackendError> {
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(BackendError::InvalidRequest(
            "folder import source must be a regular file, not a symlink or special entry"
                .to_string(),
        ));
    }
    if is_ambiguous_hard_link(path)? {
        return Err(BackendError::InvalidRequest(
            "folder import source has multiple hard links".to_string(),
        ));
    }
    Ok(())
}

fn is_ambiguous_hard_link(path: &Path) -> Result<bool, BackendError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        return Ok(fs::metadata(path).map_err(io_error)?.nlink() > 1);
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(false)
    }
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

fn ensure_private_directory(path: &Path) -> Result<(), BackendError> {
    ensure_directory(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path).map_err(io_error)?.permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(path, permissions).map_err(io_error)?;
    }
    Ok(())
}

fn ensure_safe_parent(root: &Path, parent: &Path) -> Result<(), BackendError> {
    let relative = parent.strip_prefix(root).map_err(|_| {
        BackendError::InvalidRequest("object parent escaped backend root".to_string())
    })?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component.as_os_str());
        ensure_private_directory(&current)?;
    }
    Ok(())
}

fn io_error(error: std::io::Error) -> BackendError {
    BackendError::Io(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{hash_stable_file_with_hook, FolderBackend};
    use crate::runtime::reconciliation::ReconciliationAction;
    use crate::runtime::reconciliation::{ReconciliationEntryState, ReconciliationManifest};
    use dasobjectstore_core::backend::{BackendError, BackendObjectKey, ObjectStoreBackend};
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
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn folder_backend_rejects_manifest_for_another_store() {
        let root = unique_root();
        let backend = FolderBackend::open(&root, manifest(), CapacityPolicy::bounded(1024, 1), 0)
            .expect("folder backend opens");
        let mut mismatched = manifest();
        mismatched.store_id = StoreId::new("other-folder").expect("store id");

        let error = backend
            .validate_manifest(&mismatched)
            .expect_err("foreign manifest must fail closed");
        assert_eq!(
            error,
            BackendError::InvalidRequest(
                "folder manifest identity does not match this backend".to_string()
            )
        );

        let _ = fs::remove_dir_all(root);
    }

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
        let capacity = backend.capacity();
        assert_eq!(capacity.used_bytes, 0);
        assert_eq!(capacity.reserved_bytes, 11);
        assert_eq!(capacity.available_bytes, Some(949));
        let staged = backend
            .stage("upload-1", &key, &mut Cursor::new(b"hello world".to_vec()))
            .expect("stages object");
        assert!(staged.location.contains(".dasobjectstore/staging"));
        let finalized = backend.finalize(staged).expect("finalizes object");
        assert_eq!(backend.capacity().used_bytes, 11);
        assert_eq!(
            finalized.checksum,
            "sha256:b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
        assert_eq!(
            finalized.location,
            ".dasobjectstore/objects/sample/run/data.txt"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(root.join(&finalized.location))
                    .expect("finalized metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
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
        let enumerated = backend.enumerate(None).expect("enumerates records");
        assert_eq!(
            enumerated[0].location,
            ".dasobjectstore/objects/sample/run/data.txt"
        );
        backend.remove(&key).expect("removes object");
        assert_eq!(backend.capacity().used_bytes, 0);
        assert!(backend.catalogue_records().is_empty());
        assert!(backend
            .enumerate(None)
            .expect("enumerates empty")
            .is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn folder_backend_keeps_payload_and_accounting_when_catalogue_remove_fails() {
        let root = unique_root();
        let mut backend =
            FolderBackend::open(&root, manifest(), CapacityPolicy::bounded(1024, 1), 0)
                .expect("folder backend opens");
        let key = BackendObjectKey {
            object_id: "sample/data.txt".to_string(),
            version: 1,
        };
        backend.reserve("upload-1", 4).expect("reserves capacity");
        let staged = backend
            .stage("upload-1", &key, &mut Cursor::new(b"data".to_vec()))
            .expect("stages object");
        let finalized = backend.finalize(staged).expect("finalizes object");
        backend
            .catalogue
            .commit_records([finalized.clone()])
            .expect("catalogue record commits");

        let catalogue_path = root.join(".dasobjectstore/catalogue.json");
        fs::remove_file(&catalogue_path).expect("catalogue removes");
        fs::create_dir(&catalogue_path).expect("catalogue failure fixture creates");
        assert!(backend.remove(&key).is_err(), "catalogue removal fails");
        assert!(root.join(&finalized.location).is_file());
        assert_eq!(backend.capacity().used_bytes, 4);
        assert_eq!(backend.catalogue_records(), vec![finalized]);

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

    #[test]
    fn folder_backend_rejects_staged_size_mismatch_before_commit() {
        let root = unique_root();
        let mut backend =
            FolderBackend::open(&root, manifest(), CapacityPolicy::bounded(1024, 1), 0)
                .expect("folder backend opens");
        let key = BackendObjectKey {
            object_id: "mismatch.txt".to_string(),
            version: 1,
        };
        backend.reserve("mismatch", 1).expect("reserves capacity");
        assert!(backend
            .stage("mismatch", &key, &mut Cursor::new(b"too large".to_vec()))
            .is_err());
        assert_eq!(backend.capacity().reserved_bytes, 1);
        backend
            .ledger
            .release("mismatch")
            .expect("caller can release rejected reservation");
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
    fn folder_backend_inspects_unmanaged_user_tree_without_adopting_it() {
        let root = unique_root();
        let backend = FolderBackend::open(&root, manifest(), CapacityPolicy::bounded(1024, 1), 0)
            .expect("folder backend opens");
        fs::create_dir_all(root.join("incoming/run")).expect("user hierarchy creates");
        fs::write(root.join("incoming/run/data.txt"), b"unmanaged").expect("user file writes");
        let report = backend.inspect_user_tree().expect("inspection succeeds");
        assert!(!report.is_clean());
        assert_eq!(report.unmanaged_paths, vec!["incoming/run/data.txt"]);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn folder_backend_builds_read_only_resumable_reconciliation_plan() {
        let root = unique_root();
        let backend = FolderBackend::open(&root, manifest(), CapacityPolicy::bounded(1024, 1), 0)
            .expect("folder backend opens");
        fs::create_dir_all(root.join("incoming/run")).expect("user hierarchy creates");
        fs::write(root.join("incoming/run/data.txt"), b"unmanaged").expect("user file writes");

        let reconciliation = backend
            .plan_user_tree_reconciliation()
            .expect("reconciliation plan builds");

        assert_eq!(reconciliation.manifest.store_id, "codex-folder");
        assert_eq!(reconciliation.manifest.entries.len(), 1);
        assert_eq!(
            reconciliation.manifest.entries["incoming/run/data.txt"].size_bytes,
            Some(9)
        );
        assert_eq!(
            reconciliation.plan.actions,
            vec![ReconciliationAction::Download {
                key: "incoming/run/data.txt".to_string(),
                relative_path: "incoming/run/data.txt".to_string(),
                size_bytes: Some(9),
            }]
        );

        assert!(root.join("incoming/run/data.txt").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn folder_backend_adopts_unmanaged_files_with_restart_safe_checkpoints() {
        let root = unique_root();
        let mut backend =
            FolderBackend::open(&root, manifest(), CapacityPolicy::bounded(1024, 1), 0)
                .expect("folder backend opens");
        let source = root.join("incoming/run/data.txt");
        fs::create_dir_all(source.parent().expect("source parent")).expect("parent creates");
        fs::write(&source, b"unmanaged").expect("user file writes");

        let checkpoint_path = root.with_extension("adoption").join("manifest.json");
        let mut checkpoint = backend
            .plan_user_tree_reconciliation()
            .expect("reconciliation plan builds")
            .manifest;
        checkpoint
            .save_atomic(&checkpoint_path)
            .expect("checkpoint saves");
        let mut resumed = ReconciliationManifest::load(&checkpoint_path).expect("checkpoint loads");
        let records = backend
            .adopt_user_tree_reconciliation(&checkpoint_path, &mut resumed, "adopt")
            .expect("adoption succeeds");

        assert_eq!(records.len(), 1);
        assert!(source.exists(), "adoption never mutates the user file");
        assert_eq!(backend.capacity().used_bytes, 9);
        assert_eq!(
            backend
                .enumerate(None)
                .expect("managed objects enumerate")
                .len(),
            1
        );
        assert_eq!(backend.catalogue_records(), records);
        assert!(matches!(
            resumed.entries["incoming/run/data.txt"].state,
            ReconciliationEntryState::Complete
        ));

        let resumed_plan = backend
            .replan_user_tree_reconciliation(&mut resumed)
            .expect("completed checkpoint replans");
        assert_eq!(
            resumed_plan.actions,
            vec![ReconciliationAction::SkipComplete {
                key: "incoming/run/data.txt".to_string(),
                relative_path: "incoming/run/data.txt".to_string(),
            }]
        );
        drop(backend);
        assert!(
            FolderBackend::open(&root, manifest(), CapacityPolicy::bounded(1024, 1), 1,).is_err()
        );
        let mut reopened =
            FolderBackend::open(&root, manifest(), CapacityPolicy::bounded(1024, 1), 0)
                .expect("folder backend reopens from catalogue");
        assert_eq!(reopened.capacity().used_bytes, 9);
        assert_eq!(reopened.catalogue_records(), records);
        reopened
            .remove(&records[0].key)
            .expect("removes adopted object");
        assert_eq!(reopened.capacity().used_bytes, 0);
        assert!(reopened.catalogue_records().is_empty());
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(checkpoint_path.parent().expect("checkpoint parent"));
    }

    #[test]
    fn folder_reconciliation_resume_requires_matching_source_revision() {
        let root = unique_root();
        let backend = FolderBackend::open(&root, manifest(), CapacityPolicy::bounded(1024, 1), 0)
            .expect("folder backend opens");
        let source = root.join("incoming/data.txt");
        fs::create_dir_all(source.parent().expect("source parent")).expect("parent creates");
        fs::write(&source, b"stable").expect("source writes");

        let checkpoint_root = root.with_extension("checkpoint");
        let checkpoint_path = checkpoint_root.join("manifest.json");
        let mut first = backend
            .plan_user_tree_reconciliation()
            .expect("initial plan builds")
            .manifest;
        first
            .save_atomic(&checkpoint_path)
            .expect("checkpoint saves");
        first
            .checkpoint(
                &checkpoint_path,
                "incoming/data.txt",
                ReconciliationEntryState::InProgress,
                None,
                3,
            )
            .expect("progress checkpoints");
        let mut resumed = ReconciliationManifest::load(&checkpoint_path).expect("checkpoint loads");
        let plan = backend
            .replan_user_tree_reconciliation(&mut resumed)
            .expect("unchanged plan resumes");
        assert_eq!(
            plan.actions,
            vec![ReconciliationAction::Resume {
                key: "incoming/data.txt".to_string(),
                relative_path: "incoming/data.txt".to_string(),
                size_bytes: Some(6),
                downloaded_bytes: 3,
            }]
        );

        fs::remove_file(&source).expect("source removes");
        fs::write(&source, b"stable").expect("replacement writes");
        let changed = backend
            .replan_user_tree_reconciliation(&mut resumed)
            .expect("changed plan rebuilds");
        assert!(matches!(
            changed.actions.as_slice(),
            [ReconciliationAction::Download {
                size_bytes: Some(6),
                ..
            }]
        ));
        assert_eq!(resumed.entries["incoming/data.txt"].downloaded_bytes, 0);
        assert!(source.exists());
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(checkpoint_root);
    }

    #[test]
    fn folder_reconciliation_rejects_wrong_store_checkpoint() {
        let root = unique_root();
        let backend = FolderBackend::open(&root, manifest(), CapacityPolicy::bounded(1024, 1), 0)
            .expect("folder backend opens");
        let mut manifest = ReconciliationManifest::new("other-store", None);
        let error = backend
            .replan_user_tree_reconciliation(&mut manifest)
            .expect_err("wrong store rejects");
        assert!(
            matches!(error, BackendError::InvalidRequest(message) if message.contains("different ObjectStore"))
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn folder_backend_stages_stable_source_path() {
        let root = unique_root();
        let source = root.join("source.bin");
        let mut backend =
            FolderBackend::open(&root, manifest(), CapacityPolicy::bounded(1024, 1), 0)
                .expect("folder backend opens");
        fs::write(&source, b"stable source").expect("source writes");
        let key = BackendObjectKey {
            object_id: "adopted/source.bin".to_string(),
            version: 1,
        };
        backend
            .reserve("source-import", 13)
            .expect("reserves capacity");
        let staged = backend
            .stage_path("source-import", &key, &source)
            .expect("stable source stages");
        let finalized = backend.finalize(staged).expect("finalizes source");
        assert_eq!(finalized.size_bytes, 13);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn folder_backend_rejects_changed_staged_object_and_recovers_reservation() {
        let root = unique_root();
        let mut backend =
            FolderBackend::open(&root, manifest(), CapacityPolicy::bounded(1024, 1), 0)
                .expect("folder backend opens");
        let key = BackendObjectKey {
            object_id: "changed/object.txt".to_string(),
            version: 1,
        };
        backend
            .reserve("changed-stage", 5)
            .expect("reserves capacity");
        let staged = backend
            .stage("changed-stage", &key, &mut Cursor::new(b"hello".to_vec()))
            .expect("stages object");
        let staged_path = root.join(&staged.location);
        fs::write(&staged_path, b"world").expect("tamper writes");
        assert!(backend.finalize(staged.clone()).is_err());
        assert_eq!(backend.capacity().reserved_bytes, 5);
        fs::write(&staged_path, b"hello").expect("staged object restores");
        backend
            .finalize(staged)
            .expect("recovered object finalizes");
        assert_eq!(backend.capacity().reserved_bytes, 0);
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn folder_backend_rejects_hard_linked_managed_object_verification() {
        let root = unique_root();
        let mut backend =
            FolderBackend::open(&root, manifest(), CapacityPolicy::bounded(1024, 1), 0)
                .expect("folder backend opens");
        let key = BackendObjectKey {
            object_id: "managed/object.txt".to_string(),
            version: 1,
        };
        backend
            .reserve("managed-object", 5)
            .expect("reserves capacity");
        let staged = backend
            .stage("managed-object", &key, &mut Cursor::new(b"hello".to_vec()))
            .expect("stages object");
        backend.finalize(staged).expect("finalizes object");
        let path = root.join(".dasobjectstore/objects/managed/object.txt");
        fs::hard_link(&path, root.join("managed-alias.txt")).expect("hard link creates");
        assert!(backend.verify(&key).is_err());
        assert!(backend.enumerate(None).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn folder_backend_marks_hard_linked_user_files_unsafe() {
        let root = unique_root();
        let backend = FolderBackend::open(&root, manifest(), CapacityPolicy::bounded(1024, 1), 0)
            .expect("folder backend opens");
        fs::create_dir_all(root.join("incoming")).expect("user directory creates");
        let first = root.join("incoming/first.txt");
        let second = root.join("incoming/second.txt");
        fs::write(&first, b"shared").expect("source writes");
        fs::hard_link(&first, &second).expect("hard link creates");
        let report = backend.inspect_user_tree().expect("inspection succeeds");
        assert!(report
            .unsafe_paths
            .contains(&"incoming/first.txt".to_string()));
        assert!(report
            .unsafe_paths
            .contains(&"incoming/second.txt".to_string()));
        assert!(report.unmanaged_paths.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn folder_backend_rejects_legacy_unbounded_capacity() {
        let root = unique_root();
        let error = FolderBackend::open(&root, manifest(), CapacityPolicy::default(), 0)
            .expect_err("folder backend must be bounded");
        assert!(format!("{error:?}").contains("finite logical capacity"));
    }

    #[cfg(unix)]
    #[test]
    fn folder_backend_private_namespace_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let root = unique_root();
        let _backend = FolderBackend::open(&root, manifest(), CapacityPolicy::bounded(1024, 1), 0)
            .expect("folder backend opens");
        for path in [
            root.join(".dasobjectstore"),
            root.join(".dasobjectstore/objects"),
            root.join(".dasobjectstore/staging"),
        ] {
            assert_eq!(
                fs::metadata(path)
                    .expect("namespace metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn folder_backend_deterministically_rejects_source_mutation_after_read() {
        let root = unique_root();
        fs::create_dir_all(&root).expect("test root creates");
        let source = root.join("changing.txt");
        fs::write(&source, b"before").expect("source writes");
        let mutate = || fs::write(&source, b"after mutation").expect("source mutates");
        assert!(hash_stable_file_with_hook(&source, Some(&mutate)).is_err());
        let _ = fs::remove_dir_all(root);
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
        static ROOT_COUNTER: AtomicU64 = AtomicU64::new(0);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let parent = std::env::var_os("DASOBJECTSTORE_CODEX_VALIDATION_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let counter = ROOT_COUNTER.fetch_add(1, Ordering::Relaxed);
        parent.join(format!(
            "dasobjectstore-folder-backend-{}-{now}-{counter}",
            std::process::id(),
        ))
    }
}
