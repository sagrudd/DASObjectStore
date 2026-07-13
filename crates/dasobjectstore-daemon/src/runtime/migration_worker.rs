//! Local folder-to-folder migration execution.

use super::drive_backend::DriveBackend;
use super::folder_backend::FolderBackend;
use dasobjectstore_core::backend::{
    BackendError, BackendObjectKey, BackendObjectRecord, ObjectCatalogueAuthority,
    ObjectStoreBackend,
};
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_core::migration::{MigrationTransitionError, StoreMigration};
use std::fmt::{self, Display};

/// Copy one verified object between bounded folder profiles. The source is
/// never retired here; the migration remains retirement-pending until an
/// explicit operator confirmation.
pub fn copy_folder_object(
    migration: &mut StoreMigration,
    source: &mut FolderBackend,
    destination: &mut FolderBackend,
    key: &BackendObjectKey,
    reservation_id: &str,
) -> Result<BackendObjectRecord, FolderMigrationError> {
    copy_object(migration, source, destination, key, reservation_id)
}

pub fn copy_folder_to_drive(
    migration: &mut StoreMigration,
    source: &mut FolderBackend,
    destination: &mut DriveBackend,
    key: &BackendObjectKey,
    reservation_id: &str,
) -> Result<BackendObjectRecord, FolderMigrationError> {
    copy_object(migration, source, destination, key, reservation_id)
}

fn copy_object(
    migration: &mut StoreMigration,
    source: &mut dyn MigrationBackend,
    destination: &mut dyn MigrationBackend,
    key: &BackendObjectKey,
    reservation_id: &str,
) -> Result<BackendObjectRecord, FolderMigrationError> {
    if migration.source_store_id != *source.store_id()
        || migration.destination_store_id != *destination.store_id()
    {
        return Err(FolderMigrationError::StoreIdentityMismatch);
    }
    let source_record = source
        .verify(key)
        .map_err(|error| FolderMigrationError::Backend {
            operation: "verify source",
            error,
        })?;
    migration
        .start_copy()
        .map_err(FolderMigrationError::Transition)?;

    if let Err(error) = destination.reserve(reservation_id, source_record.size_bytes) {
        let _ = migration.fail();
        return Err(FolderMigrationError::Backend {
            operation: "reserve destination",
            error,
        });
    }
    let mut reader = match source.read(key) {
        Ok(reader) => reader,
        Err(error) => {
            let _ = destination.release_reservation(reservation_id);
            let _ = migration.fail();
            return Err(FolderMigrationError::Backend {
                operation: "read source",
                error,
            });
        }
    };
    let staged = match destination.stage(reservation_id, key, &mut *reader) {
        Ok(staged) => staged,
        Err(error) => {
            let _ = destination.release_reservation(reservation_id);
            let _ = migration.fail();
            return Err(FolderMigrationError::Backend {
                operation: "stage destination",
                error,
            });
        }
    };
    let finalized = match destination.finalize(staged) {
        Ok(finalized) => finalized,
        Err(error) => {
            // A failed finalization may leave a durable staged file and its
            // reservation for a later retry. Do not delete it implicitly.
            let _ = migration.fail();
            return Err(FolderMigrationError::Backend {
                operation: "finalize destination",
                error,
            });
        }
    };
    let verified = destination
        .verify(key)
        .map_err(|error| FolderMigrationError::Backend {
            operation: "verify destination",
            error,
        })?;
    if verified.size_bytes != source_record.size_bytes
        || verified.checksum != source_record.checksum
    {
        let _ = migration.fail();
        return Err(FolderMigrationError::VerificationMismatch);
    }
    if let Err(error) = destination.commit_batch(&[verified.clone()]) {
        let _ = migration.fail();
        return Err(FolderMigrationError::Backend {
            operation: "commit destination catalogue",
            error,
        });
    }
    migration
        .mark_destination_verified()
        .map_err(FolderMigrationError::Transition)?;
    Ok(finalized)
}

pub trait MigrationBackend: ObjectStoreBackend + ObjectCatalogueAuthority {
    fn store_id(&self) -> &StoreId;
    fn release_reservation(&mut self, reservation_id: &str) -> Result<(), BackendError>;
}

impl MigrationBackend for FolderBackend {
    fn store_id(&self) -> &StoreId {
        &self.manifest().store_id
    }

    fn release_reservation(&mut self, reservation_id: &str) -> Result<(), BackendError> {
        FolderBackend::release_reservation(self, reservation_id)
    }
}

impl MigrationBackend for DriveBackend {
    fn store_id(&self) -> &StoreId {
        &self.manifest().store_id
    }

    fn release_reservation(&mut self, reservation_id: &str) -> Result<(), BackendError> {
        DriveBackend::release_reservation(self, reservation_id)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FolderMigrationError {
    StoreIdentityMismatch,
    Backend {
        operation: &'static str,
        error: BackendError,
    },
    Transition(MigrationTransitionError),
    VerificationMismatch,
}

impl Display for FolderMigrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StoreIdentityMismatch => {
                formatter.write_str("migration store identities do not match folder manifests")
            }
            Self::Backend { operation, error } => write!(formatter, "{operation} failed: {error}"),
            Self::Transition(error) => write!(formatter, "migration transition failed: {error}"),
            Self::VerificationMismatch => {
                formatter.write_str("destination checksum or size differs from source")
            }
        }
    }
}

impl std::error::Error for FolderMigrationError {}

#[cfg(test)]
mod tests {
    use super::super::drive_backend::{DriveBackend, DriveRuntimeGuard};
    use super::{copy_folder_object, copy_folder_to_drive, FolderMigrationError};
    use crate::runtime::folder_backend::FolderBackend;
    use dasobjectstore_core::backend::{BackendObjectKey, ObjectStoreBackend};
    use dasobjectstore_core::deployment::{DeploymentProfile, HostMode};
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_core::manifest::{
        BackendReference, ObjectStoreManifest, OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
    };
    use dasobjectstore_core::migration::{MigrationState, StoreMigration};
    use dasobjectstore_core::protection::ProtectionPolicy;
    use dasobjectstore_core::store::CapacityPolicy;
    use std::fs;
    use std::io::Cursor;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn manifest(store_id: &str) -> ObjectStoreManifest {
        ObjectStoreManifest {
            schema_version: OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
            store_id: StoreId::new(store_id).expect("store ID"),
            deployment_profile: DeploymentProfile::Folder,
            host_mode: HostMode::PerUser,
            protection: ProtectionPolicy::LocalOnly,
            backend: BackendReference::Folder {
                root_identity: format!("{store_id}-root"),
            },
        }
    }

    fn drive_manifest(store_id: &str) -> ObjectStoreManifest {
        ObjectStoreManifest {
            schema_version: OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
            store_id: StoreId::new(store_id).expect("store ID"),
            deployment_profile: DeploymentProfile::Drive,
            host_mode: HostMode::System,
            protection: ProtectionPolicy::LocalOnly,
            backend: BackendReference::Drive {
                filesystem_identity: format!("{store_id}-filesystem"),
                device_identity: Some(format!("{store_id}-device")),
                media: dasobjectstore_core::manifest::DriveMediaKind::Ssd,
                mount_path_hint: Some(PathBuf::from("/Volumes/CODEX")),
            },
        }
    }

    #[derive(Debug)]
    struct TestDriveGuard(AtomicBool);

    impl DriveRuntimeGuard for TestDriveGuard {
        fn validate(&self) -> Result<(), String> {
            self.0
                .load(Ordering::SeqCst)
                .then_some(())
                .ok_or_else(|| "drive unavailable".to_string())
        }
    }

    fn root(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let parent = std::env::var_os("DASOBJECTSTORE_CODEX_VALIDATION_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        parent.join(format!(
            "dasobjectstore-{label}-{}-{now}-{counter}",
            std::process::id()
        ))
    }

    #[test]
    fn copies_and_verifies_object_without_retiring_source() {
        let source_root = root("migration-source");
        let destination_root = root("migration-destination");
        let mut source = FolderBackend::open(
            &source_root,
            manifest("source-store"),
            CapacityPolicy::bounded(1_000, 0),
            0,
        )
        .expect("source opens");
        let mut destination = FolderBackend::open(
            &destination_root,
            manifest("destination-store"),
            CapacityPolicy::bounded(1_000, 0),
            0,
        )
        .expect("destination opens");
        let key = BackendObjectKey {
            object_id: "run/result.txt".to_string(),
            version: 1,
        };
        source
            .reserve("source-upload", 11)
            .expect("source reserves");
        let staged = source
            .stage(
                "source-upload",
                &key,
                &mut Cursor::new(b"hello world".to_vec()),
            )
            .expect("source stages");
        source.finalize(staged).expect("source finalizes");
        let mut migration = StoreMigration::new(
            "migration-1",
            source.manifest().store_id.clone(),
            destination.manifest().store_id.clone(),
        )
        .expect("migration creates");

        let copied = copy_folder_object(
            &mut migration,
            &mut source,
            &mut destination,
            &key,
            "destination-upload",
        )
        .expect("object copies");
        assert_eq!(
            copied,
            destination.verify(&key).expect("destination verifies")
        );
        assert_eq!(migration.state, MigrationState::RetirementPending);
        assert!(migration.source_retained);
        assert_eq!(source.verify(&key).expect("source remains").size_bytes, 11);
        assert_eq!(destination.capacity().used_bytes, 11);
        assert_eq!(destination.catalogue_records().len(), 1);
        let reopened = FolderBackend::open(
            &destination_root,
            manifest("destination-store"),
            CapacityPolicy::bounded(1_000, 0),
            0,
        )
        .expect("destination reopens from authoritative catalogue");
        assert_eq!(reopened.catalogue_records().len(), 1);
        assert_eq!(reopened.capacity().used_bytes, 11);
        let _ = fs::remove_dir_all(source_root);
        let _ = fs::remove_dir_all(destination_root);
    }

    #[test]
    fn rejects_manifest_identity_mismatch_without_mutating_migration() {
        let source_root = root("migration-source-mismatch");
        let destination_root = root("migration-destination-mismatch");
        let mut source = FolderBackend::open(
            &source_root,
            manifest("source-store"),
            CapacityPolicy::bounded(1_000, 0),
            0,
        )
        .expect("source opens");
        let mut destination = FolderBackend::open(
            &destination_root,
            manifest("destination-store"),
            CapacityPolicy::bounded(1_000, 0),
            0,
        )
        .expect("destination opens");
        let mut migration = StoreMigration::new(
            "migration-mismatch",
            StoreId::new("wrong-source").expect("store ID"),
            destination.manifest().store_id.clone(),
        )
        .expect("migration creates");
        let error = copy_folder_object(
            &mut migration,
            &mut source,
            &mut destination,
            &BackendObjectKey {
                object_id: "missing".to_string(),
                version: 1,
            },
            "destination-upload",
        )
        .expect_err("identity mismatch rejects");
        assert_eq!(error, FolderMigrationError::StoreIdentityMismatch);
        assert_eq!(migration.state, MigrationState::Planned);
        let _ = fs::remove_dir_all(source_root);
        let _ = fs::remove_dir_all(destination_root);
    }

    #[test]
    fn copies_folder_object_to_guarded_drive_backend() {
        let source_root = root("migration-drive-source");
        let destination_root = root("migration-drive-destination");
        let mut source = FolderBackend::open(
            &source_root,
            manifest("source-store"),
            CapacityPolicy::bounded(1_000, 0),
            0,
        )
        .expect("source opens");
        let guard: Arc<dyn DriveRuntimeGuard> = Arc::new(TestDriveGuard(AtomicBool::new(true)));
        let mut destination = DriveBackend::open(
            &destination_root,
            drive_manifest("drive-store"),
            CapacityPolicy::bounded(1_000, 0),
            0,
            guard,
        )
        .expect("drive opens");
        let key = BackendObjectKey {
            object_id: "run/result.txt".to_string(),
            version: 1,
        };
        source
            .reserve("source-upload", 11)
            .expect("source reserves");
        let staged = source
            .stage(
                "source-upload",
                &key,
                &mut Cursor::new(b"hello world".to_vec()),
            )
            .expect("source stages");
        source.finalize(staged).expect("source finalizes");
        let mut migration = StoreMigration::new(
            "migration-drive",
            source.manifest().store_id.clone(),
            destination.manifest().store_id.clone(),
        )
        .expect("migration creates");
        copy_folder_to_drive(
            &mut migration,
            &mut source,
            &mut destination,
            &key,
            "drive-upload",
        )
        .expect("folder object copies to drive");
        assert_eq!(migration.state, MigrationState::RetirementPending);
        assert_eq!(destination.capacity().used_bytes, 11);
        let _ = fs::remove_dir_all(source_root);
        let _ = fs::remove_dir_all(destination_root);
    }
}
