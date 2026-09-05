//! Daemon-owned orchestration for locally bound whole-store migrations.
//!
//! Clients name stores and a migration transaction only. Backend paths,
//! checkpoints, provenance, and the shared-catalogue handoff remain daemon
//! state and never cross the transport boundary.

use super::{
    migrate_folder_store_with_catalogue_handoff, read_profile_binding, FolderBackend,
    MigrationCatalogueHandoff,
};
use dasobjectstore_core::deployment::DeploymentProfile;
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_core::migration::{MigrationState, StoreMigration};
use dasobjectstore_object_service::read_store_registry;
use std::fmt::{self, Display};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

static PROFILE_MIGRATION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegisteredProfileMigrationReport {
    pub verified_object_count: u64,
    pub destination_used_bytes: u64,
    pub state: MigrationState,
    pub source_retained: bool,
}

#[derive(Debug)]
pub enum RegisteredProfileMigrationError {
    Binding(String),
    Backend(String),
    Checkpoint(String),
    Migration(String),
    StoreRegistry(String),
    UnsupportedProfile,
}

impl Display for RegisteredProfileMigrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Binding(message) => write!(formatter, "profile binding: {message}"),
            Self::Backend(message) => write!(formatter, "profile backend: {message}"),
            Self::Checkpoint(message) => write!(formatter, "migration checkpoint: {message}"),
            Self::Migration(message) => write!(formatter, "profile migration: {message}"),
            Self::StoreRegistry(message) => write!(formatter, "store registry: {message}"),
            Self::UnsupportedProfile => formatter.write_str(
                "daemon profile migration currently requires folder source and destination bindings",
            ),
        }
    }
}

impl std::error::Error for RegisteredProfileMigrationError {}

#[allow(clippy::too_many_arguments)]
pub fn migrate_registered_folder_store(
    migration_id: &str,
    source_store_id: &StoreId,
    destination_store_id: &StoreId,
    profile_binding_registry_path: &Path,
    store_registry_path: &Path,
    live_sqlite_path: &Path,
    migration_state_root: &Path,
    verified_at_utc: &str,
) -> Result<RegisteredProfileMigrationReport, RegisteredProfileMigrationError> {
    let _guard = PROFILE_MIGRATION_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| {
            RegisteredProfileMigrationError::Migration(
                "profile migration transaction lock poisoned".to_string(),
            )
        })?;
    let source_binding =
        read_profile_binding(profile_binding_registry_path, source_store_id.as_str())
            .map_err(|error| RegisteredProfileMigrationError::Binding(error.to_string()))?
            .ok_or_else(|| {
                RegisteredProfileMigrationError::Binding(format!(
                    "source store {source_store_id} is not registered"
                ))
            })?;
    let destination_binding =
        read_profile_binding(profile_binding_registry_path, destination_store_id.as_str())
            .map_err(|error| RegisteredProfileMigrationError::Binding(error.to_string()))?
            .ok_or_else(|| {
                RegisteredProfileMigrationError::Binding(format!(
                    "destination store {destination_store_id} is not registered"
                ))
            })?;
    if source_binding.manifest.deployment_profile != DeploymentProfile::Folder
        || destination_binding.manifest.deployment_profile != DeploymentProfile::Folder
    {
        return Err(RegisteredProfileMigrationError::UnsupportedProfile);
    }

    let definitions = read_store_registry(store_registry_path)
        .map_err(|error| RegisteredProfileMigrationError::StoreRegistry(error.to_string()))?;
    let source_capacity = definitions
        .iter()
        .find(|definition| definition.store_id == *source_store_id)
        .map(|definition| definition.policy.capacity.clone())
        .ok_or_else(|| {
            RegisteredProfileMigrationError::StoreRegistry(format!(
                "source store {source_store_id} has no capacity policy"
            ))
        })?;
    let destination_capacity = definitions
        .iter()
        .find(|definition| definition.store_id == *destination_store_id)
        .map(|definition| definition.policy.capacity.clone())
        .ok_or_else(|| {
            RegisteredProfileMigrationError::StoreRegistry(format!(
                "destination store {destination_store_id} has no capacity policy"
            ))
        })?;

    let transaction_root = migration_state_root.join(migration_id);
    let checkpoint_path = transaction_root.join("checkpoint.json");
    let mut migration = load_or_create_migration(
        &checkpoint_path,
        migration_id,
        source_store_id,
        destination_store_id,
    )?;
    let mut source = FolderBackend::open(
        source_binding.backend_root,
        source_binding.manifest,
        source_capacity,
        0,
    )
    .map_err(|error| RegisteredProfileMigrationError::Backend(error.to_string()))?;
    let mut destination = FolderBackend::open(
        destination_binding.backend_root,
        destination_binding.manifest,
        destination_capacity,
        0,
    )
    .map_err(|error| RegisteredProfileMigrationError::Backend(error.to_string()))?;
    let handoff = MigrationCatalogueHandoff {
        live_sqlite_path: live_sqlite_path.to_path_buf(),
        handoff_root: migration_state_root.join("catalogue-handoffs"),
        profile_namespace: format!("folder:{destination_store_id}"),
        committed_at_utc: verified_at_utc.to_string(),
    };
    let verified_object_count = migrate_folder_store_with_catalogue_handoff(
        &mut migration,
        &mut source,
        &mut destination,
        &checkpoint_path,
        transaction_root.join("provenance"),
        &format!("migration-{migration_id}"),
        verified_at_utc,
        &handoff,
    )
    .map_err(|error| RegisteredProfileMigrationError::Migration(error.to_string()))?;

    Ok(RegisteredProfileMigrationReport {
        verified_object_count,
        destination_used_bytes: destination.capacity().used_bytes,
        state: migration.state,
        source_retained: migration.source_retained,
    })
}

fn load_or_create_migration(
    checkpoint_path: &Path,
    migration_id: &str,
    source_store_id: &StoreId,
    destination_store_id: &StoreId,
) -> Result<StoreMigration, RegisteredProfileMigrationError> {
    if !checkpoint_path.exists() {
        return StoreMigration::new(
            migration_id,
            source_store_id.clone(),
            destination_store_id.clone(),
        )
        .map_err(|error| RegisteredProfileMigrationError::Migration(error.to_string()));
    }
    match StoreMigration::load(checkpoint_path) {
        Ok(migration) => {
            if migration.migration_id != migration_id
                || migration.source_store_id != *source_store_id
                || migration.destination_store_id != *destination_store_id
            {
                return Err(RegisteredProfileMigrationError::Checkpoint(
                    "persisted transaction identity does not match the request".to_string(),
                ));
            }
            Ok(migration)
        }
        Err(error) => Err(RegisteredProfileMigrationError::Checkpoint(
            error.to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{upsert_profile_binding, BackendProfileBinding};
    use dasobjectstore_core::backend::{
        BackendObjectKey, ObjectCatalogueAuthority, ObjectStoreBackend,
    };
    use dasobjectstore_core::deployment::{DeploymentProfile, HostMode};
    use dasobjectstore_core::manifest::{
        BackendReference, ObjectStoreManifest, OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
    };
    use dasobjectstore_core::protection::ProtectionPolicy;
    use dasobjectstore_core::store::{CapacityPolicy, StoreClass, StorePolicy};
    use dasobjectstore_object_service::StoreServiceDefinition;
    use std::io::Cursor;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn root() -> std::path::PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir()
            .join("dasobjectstore-codex-validation")
            .join(format!(
                "registered-profile-migration-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ))
    }

    fn manifest(id: &str, identity: &str) -> ObjectStoreManifest {
        ObjectStoreManifest {
            schema_version: OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
            store_id: StoreId::new(id).expect("store id"),
            deployment_profile: DeploymentProfile::Folder,
            host_mode: HostMode::PerUser,
            protection: ProtectionPolicy::LocalOnly,
            backend: BackendReference::Folder {
                root_identity: identity.to_string(),
            },
        }
    }

    #[test]
    fn registered_migration_is_path_private_and_replay_safe() {
        let root = root();
        let source_root = root.join("source");
        let destination_root = root.join("destination");
        std::fs::create_dir_all(&source_root).expect("source root");
        std::fs::create_dir_all(&destination_root).expect("destination root");
        let source_manifest = manifest("source-store", "source-fs");
        let destination_manifest = manifest("destination-store", "destination-fs");
        let capacity = CapacityPolicy::bounded(1_000, 0);
        let mut source =
            FolderBackend::open(&source_root, source_manifest.clone(), capacity.clone(), 0)
                .expect("source backend");
        let key = BackendObjectKey {
            object_id: "runs/result.txt".to_string(),
            version: 1,
        };
        source.reserve("seed", 7).expect("reserve");
        let staged = source
            .stage("seed", &key, &mut Cursor::new(b"result\n".to_vec()))
            .expect("stage");
        let finalized = source.finalize(staged).expect("finalize");
        source.commit_batch(&[finalized]).expect("catalogue");

        let binding_registry = root.join("profile-bindings.json");
        upsert_profile_binding(
            &binding_registry,
            BackendProfileBinding {
                manifest: source_manifest,
                backend_root: source_root,
                ssd_staging_root: None,
            },
        )
        .expect("source binding");
        upsert_profile_binding(
            &binding_registry,
            BackendProfileBinding {
                manifest: destination_manifest,
                backend_root: destination_root.clone(),
                ssd_staging_root: None,
            },
        )
        .expect("destination binding");
        let store_registry = root.join("stores.json");
        let definitions = ["source-store", "destination-store"].map(|id| StoreServiceDefinition {
            store_id: StoreId::new(id).expect("store id"),
            policy: StorePolicy {
                capacity: capacity.clone(),
                ..StorePolicy::defaults_for(StoreClass::GeneratedData)
            },
            bucket_name: None,
            reader_group: None,
            writer_group: None,
            public: false,
        });
        std::fs::write(
            &store_registry,
            serde_json::to_vec(&definitions).expect("registry JSON"),
        )
        .expect("store registry");
        let live_sqlite = root.join("live.sqlite");
        let connection = rusqlite::Connection::open(&live_sqlite).expect("SQLite");
        connection
            .execute_batch(dasobjectstore_metadata::LIVE_SCHEMA_SQL)
            .expect("schema");
        connection
            .execute(
                "INSERT INTO pools VALUES ('profiles', 'Clean', 'now', 'now')",
                [],
            )
            .expect("pool");
        connection
            .execute(
                "INSERT INTO stores VALUES ('destination-store', 'profiles', 'folder', '{}', 'now', 'now')",
                [],
            )
            .expect("destination store");
        drop(connection);

        let source_id = StoreId::new("source-store").expect("source id");
        let destination_id = StoreId::new("destination-store").expect("destination id");
        for _ in 0..2 {
            let report = migrate_registered_folder_store(
                "promotion-1",
                &source_id,
                &destination_id,
                &binding_registry,
                &store_registry,
                &live_sqlite,
                &root.join("migration-state"),
                "2026-07-15T18:00:00Z",
            )
            .expect("migration succeeds and replays");
            assert_eq!(report.verified_object_count, 1);
            assert_eq!(report.destination_used_bytes, 7);
            assert_eq!(report.state, MigrationState::RetirementPending);
            assert!(report.source_retained);
        }
        let destination = FolderBackend::open(
            destination_root,
            manifest("destination-store", "destination-fs"),
            capacity,
            0,
        )
        .expect("destination reopens");
        assert_eq!(destination.verify(&key).expect("verified").size_bytes, 7);
        let connection = rusqlite::Connection::open(live_sqlite).expect("SQLite reopen");
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM profile_catalogue_transactions WHERE transaction_id = 'promotion-1'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .expect("transaction count"),
            1
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
