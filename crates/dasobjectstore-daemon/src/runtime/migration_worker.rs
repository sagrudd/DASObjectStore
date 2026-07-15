//! Local folder-to-folder migration execution.

use super::drive_backend::DriveBackend;
use super::folder_backend::FolderBackend;
use super::migration_provenance::{
    prepare_migration_provenance, record_migration_destination_verified, MigrationProvenanceError,
};
use super::profile_catalogue::{export_profile_catalogue, import_profile_catalogue_with_metadata};
use dasobjectstore_core::backend::{
    BackendError, BackendObjectKey, BackendObjectRecord, ObjectCatalogueAuthority,
    ObjectStoreBackend,
};
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_core::manifest::ObjectStoreManifest;
use dasobjectstore_core::migration::{MigrationTransitionError, StoreMigration};
use std::fmt::{self, Display};
use std::path::{Path, PathBuf};

/// Daemon-owned destination catalogue handoff required by production
/// whole-store migration. The migration id is reused as the replay-safe
/// transaction id; source retention remains mandatory in the SQLite adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MigrationCatalogueHandoff {
    pub live_sqlite_path: PathBuf,
    pub handoff_root: PathBuf,
    pub profile_namespace: String,
    pub committed_at_utc: String,
}

impl MigrationCatalogueHandoff {
    fn validate(&self) -> Result<(), FolderMigrationError> {
        if self.live_sqlite_path.as_os_str().is_empty() {
            return Err(FolderMigrationError::InvalidRequest(
                "migration shared catalogue path must not be empty".to_string(),
            ));
        }
        if self.handoff_root.as_os_str().is_empty() {
            return Err(FolderMigrationError::InvalidRequest(
                "migration catalogue handoff root must not be empty".to_string(),
            ));
        }
        if self.profile_namespace.trim().is_empty() {
            return Err(FolderMigrationError::InvalidRequest(
                "migration profile namespace must not be empty".to_string(),
            ));
        }
        if self.committed_at_utc.trim().is_empty() {
            return Err(FolderMigrationError::InvalidRequest(
                "migration catalogue commit timestamp must not be empty".to_string(),
            ));
        }
        Ok(())
    }
}

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

/// Copy one object and record its destination verification. Whole-store
/// promotion must use [`migrate_folder_store_with_provenance`] so every source
/// catalogue record is verified before the migration becomes retirement-ready.
pub fn copy_folder_object_with_provenance(
    migration: &mut StoreMigration,
    source: &mut FolderBackend,
    destination: &mut FolderBackend,
    key: &BackendObjectKey,
    reservation_id: &str,
    provenance_root: impl AsRef<Path>,
    verified_at_utc: &str,
) -> Result<BackendObjectRecord, FolderMigrationError> {
    let source_manifest = source.manifest().clone();
    let destination_manifest = destination.manifest().clone();
    prepare_migration_provenance(
        provenance_root.as_ref(),
        migration,
        &source_manifest,
        &destination_manifest,
    )
    .map_err(FolderMigrationError::Provenance)?;
    let copied = copy_object(migration, source, destination, key, reservation_id)?;
    record_migration_destination_verified(
        provenance_root,
        migration,
        &source_manifest,
        &destination_manifest,
        verified_at_utc,
    )
    .map_err(FolderMigrationError::Provenance)?;
    Ok(copied)
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

pub fn copy_folder_to_drive_with_provenance(
    migration: &mut StoreMigration,
    source: &mut FolderBackend,
    destination: &mut DriveBackend,
    key: &BackendObjectKey,
    reservation_id: &str,
    provenance_root: impl AsRef<Path>,
    verified_at_utc: &str,
) -> Result<BackendObjectRecord, FolderMigrationError> {
    let source_manifest = source.manifest().clone();
    let destination_manifest = destination.manifest().clone();
    prepare_migration_provenance(
        provenance_root.as_ref(),
        migration,
        &source_manifest,
        &destination_manifest,
    )
    .map_err(FolderMigrationError::Provenance)?;
    let copied = copy_object(migration, source, destination, key, reservation_id)?;
    record_migration_destination_verified(
        provenance_root,
        migration,
        &source_manifest,
        &destination_manifest,
        verified_at_utc,
    )
    .map_err(FolderMigrationError::Provenance)?;
    Ok(copied)
}

pub fn migrate_folder_store_with_provenance(
    migration: &mut StoreMigration,
    source: &mut FolderBackend,
    destination: &mut FolderBackend,
    checkpoint_path: impl AsRef<Path>,
    provenance_root: impl AsRef<Path>,
    reservation_prefix: &str,
    verified_at_utc: &str,
) -> Result<u64, FolderMigrationError> {
    let source_manifest = source.manifest().clone();
    let destination_manifest = destination.manifest().clone();
    migrate_store(
        migration,
        source,
        destination,
        &source_manifest,
        &destination_manifest,
        checkpoint_path.as_ref(),
        provenance_root.as_ref(),
        reservation_prefix,
        verified_at_utc,
    )
}

pub fn migrate_folder_store_to_drive_with_provenance(
    migration: &mut StoreMigration,
    source: &mut FolderBackend,
    destination: &mut DriveBackend,
    checkpoint_path: impl AsRef<Path>,
    provenance_root: impl AsRef<Path>,
    reservation_prefix: &str,
    verified_at_utc: &str,
) -> Result<u64, FolderMigrationError> {
    let source_manifest = source.manifest().clone();
    let destination_manifest = destination.manifest().clone();
    migrate_store(
        migration,
        source,
        destination,
        &source_manifest,
        &destination_manifest,
        checkpoint_path.as_ref(),
        provenance_root.as_ref(),
        reservation_prefix,
        verified_at_utc,
    )
}

/// Migrate a complete folder store and bridge its verified destination
/// catalogue into daemon-owned shared metadata before reporting success.
/// Retrying after either catalogue commit is safe because both the handoff
/// journal and SQLite transaction use the stable migration id.
pub fn migrate_folder_store_with_catalogue_handoff(
    migration: &mut StoreMigration,
    source: &mut FolderBackend,
    destination: &mut FolderBackend,
    checkpoint_path: impl AsRef<Path>,
    provenance_root: impl AsRef<Path>,
    reservation_prefix: &str,
    verified_at_utc: &str,
    handoff: &MigrationCatalogueHandoff,
) -> Result<u64, FolderMigrationError> {
    handoff.validate()?;
    let copied = migrate_folder_store_with_provenance(
        migration,
        source,
        destination,
        checkpoint_path,
        provenance_root,
        reservation_prefix,
        verified_at_utc,
    )?;
    commit_migration_catalogue(migration, destination, handoff)?;
    Ok(copied)
}

/// Drive-profile counterpart to
/// [`migrate_folder_store_with_catalogue_handoff`]. The guarded drive remains
/// the payload authority and must pass its runtime identity checks during the
/// export/import verification cycle.
pub fn migrate_folder_store_to_drive_with_catalogue_handoff(
    migration: &mut StoreMigration,
    source: &mut FolderBackend,
    destination: &mut DriveBackend,
    checkpoint_path: impl AsRef<Path>,
    provenance_root: impl AsRef<Path>,
    reservation_prefix: &str,
    verified_at_utc: &str,
    handoff: &MigrationCatalogueHandoff,
) -> Result<u64, FolderMigrationError> {
    handoff.validate()?;
    let copied = migrate_folder_store_to_drive_with_provenance(
        migration,
        source,
        destination,
        checkpoint_path,
        provenance_root,
        reservation_prefix,
        verified_at_utc,
    )?;
    commit_migration_catalogue(migration, destination, handoff)?;
    Ok(copied)
}

fn commit_migration_catalogue(
    migration: &StoreMigration,
    destination: &mut dyn super::profile_catalogue::ProfileCatalogueBackend,
    handoff: &MigrationCatalogueHandoff,
) -> Result<(), FolderMigrationError> {
    let catalogue = export_profile_catalogue(&migration.destination_store_id, destination)
        .map_err(|error| FolderMigrationError::Backend {
            operation: "export verified migration catalogue",
            error,
        })?;
    import_profile_catalogue_with_metadata(
        &migration.destination_store_id,
        &catalogue,
        destination,
        &handoff.live_sqlite_path,
        &handoff.handoff_root,
        &migration.migration_id,
        &handoff.profile_namespace,
        &handoff.committed_at_utc,
    )
    .map_err(|error| FolderMigrationError::Backend {
        operation: "commit migration shared catalogue",
        error,
    })?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn migrate_store(
    migration: &mut StoreMigration,
    source: &mut dyn MigrationBackend,
    destination: &mut dyn MigrationBackend,
    source_manifest: &ObjectStoreManifest,
    destination_manifest: &ObjectStoreManifest,
    checkpoint_path: &Path,
    provenance_root: &Path,
    reservation_prefix: &str,
    verified_at_utc: &str,
) -> Result<u64, FolderMigrationError> {
    if reservation_prefix.trim().is_empty() {
        return Err(FolderMigrationError::InvalidRequest(
            "migration reservation prefix must not be blank".to_string(),
        ));
    }
    prepare_migration_provenance(
        provenance_root,
        migration,
        source_manifest,
        destination_manifest,
    )
    .map_err(FolderMigrationError::Provenance)?;
    match migration.state {
        dasobjectstore_core::migration::MigrationState::Planned => {
            migration
                .start_copy()
                .map_err(FolderMigrationError::Transition)?;
            persist_migration(migration, checkpoint_path)?;
        }
        dasobjectstore_core::migration::MigrationState::Copying => {}
        dasobjectstore_core::migration::MigrationState::RetirementPending => {
            return finish_verified_migration(
                migration,
                source,
                destination,
                source_manifest,
                destination_manifest,
                checkpoint_path,
                provenance_root,
                verified_at_utc,
            )
        }
        state => {
            return Err(FolderMigrationError::InvalidRequest(format!(
                "cannot execute migration while it is {state:?}"
            )))
        }
    }

    let mut records = source
        .records()
        .map_err(|error| FolderMigrationError::Backend {
            operation: "enumerate source catalogue",
            error,
        })?;
    records.sort_by(|left, right| {
        left.key
            .object_id
            .cmp(&right.key.object_id)
            .then(left.key.version.cmp(&right.key.version))
    });
    for (index, record) in records.iter().enumerate() {
        let reservation_id = format!("{reservation_prefix}-{index}");
        if let Err(error) = copy_object_payload(source, destination, &record.key, &reservation_id) {
            let _ = migration.fail();
            let _ = persist_migration(migration, checkpoint_path);
            return Err(error);
        }
    }
    migration
        .mark_destination_verified()
        .map_err(FolderMigrationError::Transition)?;
    persist_migration(migration, checkpoint_path)?;
    record_migration_destination_verified(
        provenance_root,
        migration,
        source_manifest,
        destination_manifest,
        verified_at_utc,
    )
    .map_err(FolderMigrationError::Provenance)?;
    Ok(records.len() as u64)
}

#[allow(clippy::too_many_arguments)]
fn finish_verified_migration(
    migration: &mut StoreMigration,
    source: &mut dyn MigrationBackend,
    destination: &mut dyn MigrationBackend,
    source_manifest: &ObjectStoreManifest,
    destination_manifest: &ObjectStoreManifest,
    _checkpoint_path: &Path,
    provenance_root: &Path,
    verified_at_utc: &str,
) -> Result<u64, FolderMigrationError> {
    let records = source
        .records()
        .map_err(|error| FolderMigrationError::Backend {
            operation: "enumerate source catalogue",
            error,
        })?;
    for record in &records {
        let verified =
            destination
                .verify(&record.key)
                .map_err(|error| FolderMigrationError::Backend {
                    operation: "verify resumed destination",
                    error,
                })?;
        if verified.size_bytes != record.size_bytes || verified.checksum != record.checksum {
            return Err(FolderMigrationError::VerificationMismatch);
        }
    }
    record_migration_destination_verified(
        provenance_root,
        migration,
        source_manifest,
        destination_manifest,
        verified_at_utc,
    )
    .map_err(FolderMigrationError::Provenance)?;
    Ok(records.len() as u64)
}

fn persist_migration(
    migration: &StoreMigration,
    checkpoint_path: &Path,
) -> Result<(), FolderMigrationError> {
    migration
        .save_atomic(checkpoint_path)
        .map_err(|error| FolderMigrationError::Persistence(error.to_string()))
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
    migration
        .start_copy()
        .map_err(FolderMigrationError::Transition)?;
    let finalized = match copy_object_payload(source, destination, key, reservation_id) {
        Ok(finalized) => finalized,
        Err(error) => {
            let _ = migration.fail();
            return Err(error);
        }
    };
    migration
        .mark_destination_verified()
        .map_err(FolderMigrationError::Transition)?;
    Ok(finalized)
}

fn copy_object_payload(
    source: &mut dyn MigrationBackend,
    destination: &mut dyn MigrationBackend,
    key: &BackendObjectKey,
    reservation_id: &str,
) -> Result<BackendObjectRecord, FolderMigrationError> {
    let source_record = source
        .verify(key)
        .map_err(|error| FolderMigrationError::Backend {
            operation: "verify source",
            error,
        })?;
    let existing = destination
        .records()
        .map_err(|error| FolderMigrationError::Backend {
            operation: "inspect destination catalogue",
            error,
        })?
        .into_iter()
        .find(|record| record.key == *key);
    if let Some(catalogued) = existing {
        let existing = destination
            .verify(key)
            .map_err(|error| FolderMigrationError::Backend {
                operation: "verify existing destination",
                error,
            })?;
        if existing.size_bytes == source_record.size_bytes
            && existing.checksum == source_record.checksum
            && catalogued.size_bytes == source_record.size_bytes
            && catalogued.checksum == source_record.checksum
        {
            destination
                .commit_batch(&[existing.clone()])
                .map_err(|error| FolderMigrationError::Backend {
                    operation: "commit existing destination catalogue",
                    error,
                })?;
            return Ok(existing);
        }
        return Err(FolderMigrationError::VerificationMismatch);
    }
    destination
        .reserve(reservation_id, source_record.size_bytes)
        .map_err(|error| FolderMigrationError::Backend {
            operation: "reserve destination",
            error,
        })?;
    let mut reader = match source.read(key) {
        Ok(reader) => reader,
        Err(error) => {
            let _ = destination.release_reservation(reservation_id);
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
        return Err(FolderMigrationError::VerificationMismatch);
    }
    if let Err(error) = destination.commit_batch(&[verified.clone()]) {
        return Err(FolderMigrationError::Backend {
            operation: "commit destination catalogue",
            error,
        });
    }
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
    Provenance(MigrationProvenanceError),
    Persistence(String),
    InvalidRequest(String),
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
            Self::Provenance(error) => write!(formatter, "migration provenance failed: {error}"),
            Self::Persistence(error) => write!(formatter, "migration checkpoint failed: {error}"),
            Self::InvalidRequest(error) => formatter.write_str(error),
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
    use super::{
        copy_folder_object, copy_folder_object_with_provenance, copy_folder_to_drive,
        migrate_folder_store_with_catalogue_handoff, migrate_folder_store_with_provenance,
        FolderMigrationError, MigrationCatalogueHandoff,
    };
    use crate::runtime::folder_backend::FolderBackend;
    use crate::runtime::{read_migration_provenance, MigrationVerificationState};
    use dasobjectstore_core::backend::{
        BackendObjectKey, ObjectCatalogueAuthority, ObjectStoreBackend,
    };
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

    fn initialize_shared_catalogue(path: &std::path::Path, store_id: &str) {
        let connection = rusqlite::Connection::open(path).expect("open shared catalogue");
        connection
            .execute_batch(dasobjectstore_metadata::LIVE_SCHEMA_SQL)
            .expect("initialize schema");
        connection
            .execute(
                "INSERT INTO pools VALUES ('profile-pool', 'Clean', 'now', 'now')",
                [],
            )
            .expect("insert pool");
        connection
            .execute(
                "INSERT INTO stores VALUES (?1, 'profile-pool', 'folder', '{}', 'now', 'now')",
                [store_id],
            )
            .expect("insert destination store");
    }

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
    fn production_copy_persists_verified_manifest_bound_provenance() {
        let source_root = root("migration-provenance-source");
        let destination_root = root("migration-provenance-destination");
        let provenance_root = root("migration-provenance-journal");
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
            object_id: "run/provenance.txt".to_string(),
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
            "migration-provenance",
            source.manifest().store_id.clone(),
            destination.manifest().store_id.clone(),
        )
        .expect("migration creates");

        copy_folder_object_with_provenance(
            &mut migration,
            &mut source,
            &mut destination,
            &key,
            "destination-upload",
            &provenance_root,
            "2026-07-15T12:00:00Z",
        )
        .expect("object copies with provenance");

        let provenance = read_migration_provenance(&provenance_root, "migration-provenance")
            .expect("provenance reads")
            .expect("provenance exists");
        assert_eq!(
            provenance.verification_state,
            MigrationVerificationState::Verified
        );
        assert_eq!(
            provenance.destination_verified_at_utc.as_deref(),
            Some("2026-07-15T12:00:00Z")
        );
        assert!(provenance.source_retained);
        assert_eq!(migration.state, MigrationState::RetirementPending);
        let _ = fs::remove_dir_all(source_root);
        let _ = fs::remove_dir_all(destination_root);
        let _ = fs::remove_dir_all(provenance_root);
    }

    #[test]
    fn whole_store_migration_checkpoints_and_verifies_every_catalogue_record() {
        let source_root = root("migration-store-source");
        let destination_root = root("migration-store-destination");
        let state_root = root("migration-store-state");
        let checkpoint = state_root.join("checkpoint.json");
        let provenance_root = state_root.join("provenance");
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
        for (index, payload) in [b"first".as_slice(), b"second".as_slice()]
            .into_iter()
            .enumerate()
        {
            let reservation = format!("source-{index}");
            let key = BackendObjectKey {
                object_id: format!("run/{index}.txt"),
                version: 1,
            };
            source
                .reserve(&reservation, payload.len() as u64)
                .expect("source reserves");
            let staged = source
                .stage(&reservation, &key, &mut Cursor::new(payload.to_vec()))
                .expect("source stages");
            let finalized = source.finalize(staged).expect("source finalizes");
            source
                .commit_batch(&[finalized])
                .expect("source catalogue commits");
        }
        let mut migration = StoreMigration::new(
            "migration-store",
            source.manifest().store_id.clone(),
            destination.manifest().store_id.clone(),
        )
        .expect("migration creates");

        let copied = migrate_folder_store_with_provenance(
            &mut migration,
            &mut source,
            &mut destination,
            &checkpoint,
            &provenance_root,
            "migration-store-reservation",
            "2026-07-15T12:00:00Z",
        )
        .expect("store migrates");

        assert_eq!(copied, 2);
        assert_eq!(destination.records().expect("destination records").len(), 2);
        assert_eq!(migration.state, MigrationState::RetirementPending);
        assert_eq!(
            StoreMigration::load(&checkpoint)
                .expect("checkpoint reloads")
                .state,
            MigrationState::RetirementPending
        );
        let provenance = read_migration_provenance(&provenance_root, "migration-store")
            .expect("provenance reads")
            .expect("provenance exists");
        assert_eq!(
            provenance.verification_state,
            MigrationVerificationState::Verified
        );
        assert!(provenance.source_retained);
        let _ = fs::remove_dir_all(source_root);
        let _ = fs::remove_dir_all(destination_root);
        let _ = fs::remove_dir_all(state_root);
    }

    #[test]
    fn whole_store_migration_commits_replay_safe_shared_catalogue() {
        let source_root = root("migration-handoff-source");
        let destination_root = root("migration-handoff-destination");
        let state_root = root("migration-handoff-state");
        fs::create_dir_all(&state_root).expect("state root");
        let live_sqlite_path = state_root.join("live.sqlite");
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
            object_id: "run/shared-catalogue.txt".to_string(),
            version: 1,
        };
        source.reserve("source-upload", 11).expect("reserve");
        let staged = source
            .stage(
                "source-upload",
                &key,
                &mut Cursor::new(b"hello world".to_vec()),
            )
            .expect("stage");
        let finalized = source.finalize(staged).expect("finalize");
        source.commit_batch(&[finalized]).expect("source catalogue");
        let mut migration = StoreMigration::new(
            "migration-shared-catalogue",
            source.manifest().store_id.clone(),
            destination.manifest().store_id.clone(),
        )
        .expect("migration");
        let checkpoint = state_root.join("checkpoint.json");
        let provenance_root = state_root.join("provenance");
        let handoff = MigrationCatalogueHandoff {
            live_sqlite_path: live_sqlite_path.clone(),
            handoff_root: state_root.join("catalogue-handoffs"),
            profile_namespace: "folder:destination-store".to_string(),
            committed_at_utc: "2026-07-15T15:00:00Z".to_string(),
        };

        let first = migrate_folder_store_with_catalogue_handoff(
            &mut migration,
            &mut source,
            &mut destination,
            &checkpoint,
            &provenance_root,
            "migration-shared-reservation",
            "2026-07-15T15:00:00Z",
            &handoff,
        );
        assert!(matches!(first, Err(FolderMigrationError::Backend { .. })));
        assert_eq!(migration.state, MigrationState::RetirementPending);
        assert!(migration.source_retained);
        assert_eq!(destination.records().expect("destination records").len(), 1);

        initialize_shared_catalogue(&live_sqlite_path, "destination-store");

        for _ in 0..2 {
            let copied = migrate_folder_store_with_catalogue_handoff(
                &mut migration,
                &mut source,
                &mut destination,
                &checkpoint,
                &provenance_root,
                "migration-shared-reservation",
                "2026-07-15T15:00:00Z",
                &handoff,
            )
            .expect("migration handoff is replay safe");
            assert_eq!(copied, 1);
        }
        assert_eq!(migration.state, MigrationState::RetirementPending);
        assert!(migration.source_retained);
        assert_eq!(source.verify(&key).expect("source retained").size_bytes, 11);
        let connection = rusqlite::Connection::open(&live_sqlite_path).expect("reopen SQLite");
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM profile_catalogue_transactions WHERE transaction_id = 'migration-shared-catalogue' AND source_retained = 1",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .expect("transaction count"),
            1
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM profile_catalogue_objects",
                    [],
                    |row| { row.get::<_, i64>(0) }
                )
                .expect("object count"),
            1
        );
        let _ = fs::remove_dir_all(source_root);
        let _ = fs::remove_dir_all(destination_root);
        let _ = fs::remove_dir_all(state_root);
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
        assert_eq!(
            destination
                .catalogue_records()
                .expect("drive catalogue")
                .len(),
            1
        );
        let reopened = DriveBackend::open(
            &destination_root,
            drive_manifest("drive-store"),
            CapacityPolicy::bounded(1_000, 0),
            0,
            Arc::new(TestDriveGuard(AtomicBool::new(true))),
        )
        .expect("drive reopens from authoritative catalogue");
        assert_eq!(reopened.capacity().used_bytes, 11);
        assert_eq!(
            reopened
                .catalogue_records()
                .expect("reopened drive catalogue")
                .len(),
            1
        );
        let _ = fs::remove_dir_all(source_root);
        let _ = fs::remove_dir_all(destination_root);
    }
}
