//! Dedicated SSD drive backend built on the bounded folder implementation.
//!
//! The drive profile retains its manifest and identity at the public boundary
//! while reusing the folder backend's hierarchy and durable file engine. An
//! injected runtime guard is checked before filesystem operations so an
//! unmounted or identity-drifted drive fails closed instead of writing through
//! a stale mount directory.

use super::folder_backend::{
    FolderBackend, FolderCapacitySnapshot, FolderInspectionReport, FolderReconciliationPlan,
};
use super::folder_catalogue::{FolderCatalogueBrowserEntry, FolderCatalogueBrowserQuery};
use super::reconciliation::{ReconciliationManifest, ReconciliationPlan};
use dasobjectstore_core::backend::{
    BackendCapabilities, BackendError, BackendHealth, BackendObjectKey, BackendObjectRecord,
    ObjectCatalogueAuthority, ObjectStoreBackend,
};
use dasobjectstore_core::deployment::DeploymentProfile;
use dasobjectstore_core::manifest::{BackendReference, ObjectStoreManifest};
use dasobjectstore_core::store::CapacityPolicy;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub trait DriveRuntimeGuard: Send + Sync + std::fmt::Debug {
    fn validate(&self) -> Result<(), String>;
}

#[derive(Debug)]
pub struct DriveBackend {
    root: PathBuf,
    manifest: ObjectStoreManifest,
    folder: FolderBackend,
    guard: Arc<dyn DriveRuntimeGuard>,
}

impl DriveBackend {
    pub fn open(
        root: impl Into<PathBuf>,
        manifest: ObjectStoreManifest,
        capacity: CapacityPolicy,
        used_bytes: u64,
        guard: Arc<dyn DriveRuntimeGuard>,
    ) -> Result<Self, BackendError> {
        manifest.validate().map_err(BackendError::Manifest)?;
        let BackendReference::Drive {
            filesystem_identity,
            ..
        } = &manifest.backend
        else {
            return Err(BackendError::InvalidRequest(
                "drive backend requires a drive manifest".to_string(),
            ));
        };
        guard.validate().map_err(BackendError::InvalidRequest)?;
        let root = root.into();
        let folder_manifest = ObjectStoreManifest {
            schema_version: manifest.schema_version,
            store_id: manifest.store_id.clone(),
            deployment_profile: DeploymentProfile::Folder,
            host_mode: manifest.host_mode,
            protection: manifest.protection,
            backend: BackendReference::Folder {
                root_identity: filesystem_identity.clone(),
            },
        };
        let folder = FolderBackend::open(&root, folder_manifest, capacity, used_bytes)?;
        Ok(Self {
            root: folder.root().to_path_buf(),
            manifest,
            folder,
            guard,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn manifest(&self) -> &ObjectStoreManifest {
        &self.manifest
    }

    pub fn capacity(&self) -> FolderCapacitySnapshot {
        self.folder.capacity()
    }

    /// Return capacity only while the drive identity guard is valid. The
    /// unguarded accessor remains available for internal post-operation
    /// accounting, but callers presenting live drive state should use this
    /// fail-closed view.
    pub fn guarded_capacity(&self) -> Result<FolderCapacitySnapshot, BackendError> {
        self.guard()?;
        Ok(self.folder.capacity())
    }

    /// Return only authoritative catalogue records while the drive identity
    /// guard is valid; private payload enumeration is intentionally excluded.
    pub fn catalogue_records(&self) -> Result<Vec<BackendObjectRecord>, BackendError> {
        self.guard()?;
        Ok(self.folder.catalogue_records())
    }

    pub fn inspect_user_tree(&self) -> Result<FolderInspectionReport, BackendError> {
        self.guard()?;
        self.folder.inspect_user_tree()
    }

    /// Build a read-only, restart-safe reconciliation plan only while the
    /// drive identity guard is valid. The returned manifest is caller-owned;
    /// no source files or managed catalogue state are changed.
    pub fn plan_user_tree_reconciliation(&self) -> Result<FolderReconciliationPlan, BackendError> {
        self.guard()?;
        self.folder.plan_user_tree_reconciliation()
    }

    /// Replan against a caller-owned checkpoint after validating the mounted
    /// drive identity. This lets daemon orchestration resume without exposing
    /// the folder engine or bypassing the drive guard.
    pub fn replan_user_tree_reconciliation(
        &self,
        manifest: &mut ReconciliationManifest,
    ) -> Result<ReconciliationPlan, BackendError> {
        self.guard()?;
        self.folder.replan_user_tree_reconciliation(manifest)
    }

    /// Explicitly adopt user-visible files through the guarded drive profile.
    /// The folder engine performs durable staging/checkpointing; callers must
    /// commit returned records through the drive catalogue authority before
    /// treating the adoption as complete.
    pub fn adopt_user_tree_reconciliation(
        &mut self,
        checkpoint_path: &Path,
        manifest: &mut ReconciliationManifest,
        reservation_prefix: &str,
    ) -> Result<Vec<BackendObjectRecord>, BackendError> {
        self.guard()?;
        self.folder
            .adopt_user_tree_reconciliation(checkpoint_path, manifest, reservation_prefix)
    }

    /// Return the folder-compatible browser projection only while the drive
    /// identity guard is valid. The projection does not invent placement or
    /// lifecycle metadata for this single-device failure domain.
    pub fn browser_entries(
        &self,
        query: &FolderCatalogueBrowserQuery,
    ) -> Result<Vec<FolderCatalogueBrowserEntry>, BackendError> {
        self.guard()?;
        self.folder.browser_entries(query)
    }

    pub(crate) fn release_reservation(&mut self, reservation_id: &str) -> Result<(), BackendError> {
        self.folder.release_reservation(reservation_id)
    }

    fn guard(&self) -> Result<(), BackendError> {
        self.guard.validate().map_err(BackendError::InvalidRequest)
    }
}

impl ObjectStoreBackend for DriveBackend {
    fn capabilities(&self) -> BackendCapabilities {
        self.folder.capabilities()
    }

    fn validate_manifest(&self, manifest: &ObjectStoreManifest) -> Result<(), BackendError> {
        manifest.validate().map_err(BackendError::Manifest)?;
        if manifest != &self.manifest || !matches!(manifest.backend, BackendReference::Drive { .. })
        {
            return Err(BackendError::InvalidRequest(
                "drive manifest identity does not match this backend".to_string(),
            ));
        }
        Ok(())
    }

    fn reserve(&mut self, reservation_id: &str, bytes: u64) -> Result<(), BackendError> {
        self.folder.reserve(reservation_id, bytes)
    }

    fn stage(
        &mut self,
        reservation_id: &str,
        key: &BackendObjectKey,
        source: &mut dyn Read,
    ) -> Result<BackendObjectRecord, BackendError> {
        self.guard()?;
        self.folder.stage(reservation_id, key, source)
    }

    fn finalize(
        &mut self,
        staged: BackendObjectRecord,
    ) -> Result<BackendObjectRecord, BackendError> {
        self.guard()?;
        self.folder.finalize(staged)
    }

    fn read(&self, key: &BackendObjectKey) -> Result<Box<dyn Read + Send>, BackendError> {
        self.guard()?;
        self.folder.read(key)
    }

    fn enumerate(&self, prefix: Option<&str>) -> Result<Vec<BackendObjectRecord>, BackendError> {
        self.guard()?;
        self.folder.enumerate(prefix)
    }

    fn verify(&self, key: &BackendObjectKey) -> Result<BackendObjectRecord, BackendError> {
        self.guard()?;
        self.folder.verify(key)
    }

    fn health(&self) -> Result<BackendHealth, BackendError> {
        self.guard()?;
        self.folder.health()
    }

    fn reconcile(&mut self) -> Result<Vec<BackendObjectRecord>, BackendError> {
        self.guard()?;
        self.folder.reconcile()
    }

    fn remove(&mut self, key: &BackendObjectKey) -> Result<(), BackendError> {
        self.guard()?;
        self.folder.remove(key)
    }
}

impl ObjectCatalogueAuthority for DriveBackend {
    fn records(&self) -> Result<Vec<BackendObjectRecord>, BackendError> {
        self.guard()?;
        Ok(self.folder.catalogue_records())
    }

    fn commit_batch(&mut self, records: &[BackendObjectRecord]) -> Result<(), BackendError> {
        self.guard()?;
        self.folder.catalogue_authority_commit_batch(records)
    }

    fn remove_record(&mut self, key: &BackendObjectKey) -> Result<(), BackendError> {
        self.guard()?;
        self.folder.catalogue_authority_remove_record(key)
    }
}

#[cfg(test)]
mod tests {
    use super::{DriveBackend, DriveRuntimeGuard};
    use crate::runtime::folder_catalogue::FolderCatalogueBrowserQuery;
    use crate::runtime::reconciliation::ReconciliationManifest;
    use dasobjectstore_core::backend::{
        BackendObjectKey, ObjectCatalogueAuthority, ObjectStoreBackend,
    };
    use dasobjectstore_core::deployment::{DeploymentProfile, HostMode};
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_core::manifest::{
        BackendReference, DriveMediaKind, ObjectStoreManifest, OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
    };
    use dasobjectstore_core::protection::ProtectionPolicy;
    use dasobjectstore_core::store::CapacityPolicy;
    use std::fs;
    use std::io::{Cursor, Read};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Debug)]
    struct FakeGuard(AtomicBool);

    impl DriveRuntimeGuard for FakeGuard {
        fn validate(&self) -> Result<(), String> {
            self.0
                .load(Ordering::SeqCst)
                .then_some(())
                .ok_or_else(|| "drive is unavailable".to_string())
        }
    }

    #[test]
    fn drive_backend_reuses_folder_engine_and_retains_drive_manifest() {
        let root = unique_root();
        let guard: Arc<dyn DriveRuntimeGuard> = Arc::new(FakeGuard(AtomicBool::new(true)));
        let mut backend = DriveBackend::open(
            &root,
            manifest(),
            CapacityPolicy::bounded(1024, 1),
            0,
            guard,
        )
        .expect("drive backend opens");
        assert_eq!(
            backend.manifest().deployment_profile,
            DeploymentProfile::Drive
        );
        let key = BackendObjectKey {
            object_id: "nested/run.txt".to_string(),
            version: 1,
        };
        backend
            .reserve("drive-upload", 5)
            .expect("reserves capacity");
        let staged = backend
            .stage("drive-upload", &key, &mut Cursor::new(b"hello".to_vec()))
            .expect("stages object");
        let finalized = backend.finalize(staged).expect("finalizes object");
        assert_eq!(finalized.location, ".dasobjectstore/objects/nested/run.txt");
        assert_eq!(backend.capacity().used_bytes, 5);
        let mut content = String::new();
        backend
            .read(&key)
            .expect("opens object")
            .read_to_string(&mut content)
            .expect("reads object");
        assert_eq!(content, "hello");
        backend.remove(&key).expect("removes object");
        assert_eq!(backend.capacity().used_bytes, 0);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn drive_backend_fails_closed_when_guard_reports_unavailable() {
        let root = unique_root();
        let guard_state = Arc::new(FakeGuard(AtomicBool::new(true)));
        let guard: Arc<dyn DriveRuntimeGuard> = guard_state.clone();
        let mut backend = DriveBackend::open(
            &root,
            manifest(),
            CapacityPolicy::bounded(1024, 1),
            0,
            Arc::clone(&guard),
        )
        .expect("drive backend opens");
        guard_state.0.store(false, Ordering::SeqCst);
        backend
            .reserve("blocked-upload", 1)
            .expect("reservation is memory-only");
        let key = BackendObjectKey {
            object_id: "blocked.txt".to_string(),
            version: 1,
        };
        assert!(backend
            .stage("blocked-upload", &key, &mut Cursor::new(b"x".to_vec()))
            .is_err());
        assert_eq!(
            fs::read_dir(root.join(".dasobjectstore/staging"))
                .expect("staging directory exists")
                .count(),
            0
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn drive_browser_projection_requires_a_valid_guard() {
        let root = unique_root();
        let guard_state = Arc::new(FakeGuard(AtomicBool::new(true)));
        let guard: Arc<dyn DriveRuntimeGuard> = guard_state.clone();
        let mut backend = DriveBackend::open(
            &root,
            manifest(),
            CapacityPolicy::bounded(1024, 1),
            0,
            guard,
        )
        .expect("drive backend opens");
        let key = BackendObjectKey {
            object_id: "nested/run.txt".to_string(),
            version: 1,
        };
        backend.reserve("drive-browser", 5).expect("reserves");
        let staged = backend
            .stage("drive-browser", &key, &mut Cursor::new(b"hello".to_vec()))
            .expect("stages");
        backend.finalize(staged).expect("finalizes");
        let entries = backend
            .browser_entries(&FolderCatalogueBrowserQuery {
                prefix: Some("nested/".to_string()),
                limit: 10,
                ..FolderCatalogueBrowserQuery::default()
            })
            .expect("browser projection");
        // Finalization alone does not make an object authoritative in the
        // profile catalogue; the shared adoption/catalogue transaction does.
        // In particular, the projection must not fall back to payload walks.
        assert!(entries.is_empty());

        guard_state.0.store(false, Ordering::SeqCst);
        assert!(backend.guarded_capacity().is_err());
        assert!(backend.catalogue_records().is_err());
        assert!(backend
            .browser_entries(&FolderCatalogueBrowserQuery::default())
            .is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn drive_reconciliation_adoption_is_guarded_and_restart_safe() {
        let root = unique_root();
        let guard_state = Arc::new(FakeGuard(AtomicBool::new(true)));
        let guard: Arc<dyn DriveRuntimeGuard> = guard_state.clone();
        let mut backend = DriveBackend::open(
            &root,
            manifest(),
            CapacityPolicy::bounded(1024, 1),
            0,
            guard,
        )
        .expect("drive backend opens");
        let source = root.join("incoming/run/data.txt");
        fs::create_dir_all(source.parent().expect("source parent")).expect("parent creates");
        fs::write(&source, b"drive source").expect("source writes");

        let checkpoint_path = root.with_extension("drive-adoption").join("manifest.json");
        let mut checkpoint = backend
            .plan_user_tree_reconciliation()
            .expect("drive plan builds")
            .manifest;
        checkpoint
            .save_atomic(&checkpoint_path)
            .expect("checkpoint saves");
        let mut resumed = ReconciliationManifest::load(&checkpoint_path).expect("checkpoint loads");
        let records = backend
            .adopt_user_tree_reconciliation(&checkpoint_path, &mut resumed, "drive-adopt")
            .expect("drive adoption succeeds");

        assert_eq!(records.len(), 1);
        assert!(source.exists(), "adoption preserves the source file");
        assert_eq!(
            backend.catalogue_records().expect("records guarded").len(),
            1
        );
        assert_eq!(backend.capacity().used_bytes, 12);
        assert!(resumed.entries["incoming/run/data.txt"]
            .state
            .eq(&crate::runtime::reconciliation::ReconciliationEntryState::Complete));

        guard_state.0.store(false, Ordering::SeqCst);
        assert!(backend.plan_user_tree_reconciliation().is_err());
        assert!(backend
            .adopt_user_tree_reconciliation(&checkpoint_path, &mut resumed, "drive-adopt")
            .is_err());
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(checkpoint_path.parent().expect("checkpoint parent"));
    }

    #[test]
    fn drive_catalogue_authority_is_guarded_and_profile_neutral() {
        let root = unique_root();
        let guard_state = Arc::new(FakeGuard(AtomicBool::new(true)));
        let guard: Arc<dyn DriveRuntimeGuard> = guard_state.clone();
        let mut backend = DriveBackend::open(
            &root,
            manifest(),
            CapacityPolicy::bounded(1024, 1),
            0,
            guard,
        )
        .expect("drive backend opens");
        let record = dasobjectstore_core::backend::BackendObjectRecord {
            key: BackendObjectKey {
                object_id: "catalogue/data.txt".to_string(),
                version: 1,
            },
            size_bytes: 4,
            checksum: "sha256:data".to_string(),
            location: ".dasobjectstore/objects/catalogue/data.txt".to_string(),
        };
        ObjectCatalogueAuthority::commit_batch(&mut backend, &[record.clone()])
            .expect("guarded authority commit");
        assert_eq!(
            ObjectCatalogueAuthority::records(&backend).expect("guarded authority records"),
            vec![record.clone()]
        );
        guard_state.0.store(false, Ordering::SeqCst);
        assert!(ObjectCatalogueAuthority::records(&backend).is_err());
        assert!(ObjectCatalogueAuthority::remove_record(&mut backend, &record.key).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn drive_backend_rejects_folder_manifest_and_unbounded_capacity() {
        let root = unique_root();
        let guard: Arc<dyn DriveRuntimeGuard> = Arc::new(FakeGuard(AtomicBool::new(true)));
        let mut folder_manifest = manifest();
        folder_manifest.deployment_profile = DeploymentProfile::Folder;
        folder_manifest.backend = BackendReference::Folder {
            root_identity: "folder-root".to_string(),
        };
        assert!(DriveBackend::open(
            &root,
            folder_manifest,
            CapacityPolicy::bounded(10, 1),
            0,
            Arc::clone(&guard),
        )
        .is_err());
        assert!(
            DriveBackend::open(&root, manifest(), CapacityPolicy::default(), 0, guard,).is_err()
        );
        let _ = fs::remove_dir_all(root);
    }

    fn manifest() -> ObjectStoreManifest {
        ObjectStoreManifest {
            schema_version: OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
            store_id: StoreId::new("codex-drive").expect("store id"),
            deployment_profile: DeploymentProfile::Drive,
            host_mode: HostMode::System,
            protection: ProtectionPolicy::Reproducible,
            backend: BackendReference::Drive {
                filesystem_identity: "apfs:drive".to_string(),
                device_identity: Some("disk:drive".to_string()),
                media: DriveMediaKind::Ssd,
                mount_path_hint: Some(PathBuf::from("/Volumes/CODEX")),
            },
        }
    }

    fn unique_root() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let parent = std::env::var_os("DASOBJECTSTORE_CODEX_VALIDATION_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        parent.join(format!(
            "dasobjectstore-drive-backend-{}-{now}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
        ))
    }
}
