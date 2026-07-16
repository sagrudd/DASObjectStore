use dasobjectstore_core::backend::{
    BackendError, BackendObjectKey, BackendObjectRecord, ObjectCatalogueAuthority,
    ObjectStoreBackend,
};
use dasobjectstore_core::ids::{ObjectId, PlacementId, StoreId};
use dasobjectstore_core::object_catalogue::{
    ObjectDigest, PortableLifecycleState, PortableObjectCatalogue, PortableObjectVersion,
    PortablePlacement, PortablePlacementLocation, PortableProtectionState, PortableProvenance,
    PORTABLE_OBJECT_CATALOGUE_SCHEMA_VERSION,
};
use dasobjectstore_core::protection::ProtectionPolicy;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProfileCatalogueRecoveryReport {
    pub stores_scanned: u64,
    pub stores_republished: u64,
    pub stale_journals_removed: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProfileRetirementRecoveryReport {
    pub retirements_completed: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProfileReactivationRecoveryReport {
    pub reactivations_completed: u64,
}

/// Republish authoritative private catalogues and activate profiles whose
/// explicit recovery was interrupted after entering the fail-closed state.
pub fn recover_profile_reactivations(
    binding_registry_path: impl AsRef<Path>,
    store_registry_path: impl AsRef<Path>,
    live_sqlite_path: impl AsRef<Path>,
    committed_at_utc: &str,
) -> Result<ProfileReactivationRecoveryReport, BackendError> {
    let binding_registry_path = binding_registry_path.as_ref();
    let recovering = super::recovering_profile_store_ids(binding_registry_path)
        .map_err(|error| BackendError::InvalidRequest(error.to_string()))?;
    if recovering.is_empty() {
        return Ok(ProfileReactivationRecoveryReport::default());
    }
    let definitions = dasobjectstore_object_service::read_store_registry(store_registry_path)
        .map_err(|error| BackendError::InvalidRequest(error.to_string()))?;
    let mut report = ProfileReactivationRecoveryReport::default();
    for store_id in recovering {
        let binding = super::read_profile_binding_record(binding_registry_path, &store_id)
            .map_err(|error| BackendError::InvalidRequest(error.to_string()))?
            .ok_or_else(|| BackendError::NotFound(format!("profile binding {store_id}")))?
            .validate_and_canonicalize()
            .map_err(|error| BackendError::InvalidRequest(error.to_string()))?;
        if binding.manifest.deployment_profile
            != dasobjectstore_core::deployment::DeploymentProfile::Folder
        {
            return Err(BackendError::InvalidRequest(
                "only folder profile recovery is currently supported".to_string(),
            ));
        }
        let definition = definitions
            .iter()
            .find(|definition| definition.store_id == binding.manifest.store_id)
            .ok_or_else(|| BackendError::NotFound("profile capacity policy".to_string()))?;
        let backend = super::FolderBackend::open(
            &binding.backend_root,
            binding.manifest.clone(),
            definition.policy.capacity.clone(),
            0,
        )?;
        let namespace = format!("profile-s3:{}", binding.manifest.store_id.as_str());
        publish_profile_catalogue_with_metadata(
            &binding.manifest.store_id,
            &backend,
            live_sqlite_path.as_ref(),
            binding
                .backend_root
                .join(".dasobjectstore/profile-catalogue-handoffs"),
            &namespace,
            committed_at_utc,
        )?;
        super::finish_profile_binding_recovery(
            binding_registry_path,
            binding.manifest.store_id.as_str(),
        )
        .map_err(|error| BackendError::InvalidRequest(error.to_string()))?;
        report.reactivations_completed += 1;
    }
    Ok(report)
}

/// Complete crash-interrupted retirements before active catalogue publication
/// recovery runs. A retiring binding is already unavailable to data-plane
/// opens; shared visibility is withdrawn idempotently before the durable final
/// tombstone is published.
pub fn recover_profile_retirements(
    binding_registry_path: impl AsRef<Path>,
    live_sqlite_path: impl AsRef<Path>,
) -> Result<ProfileRetirementRecoveryReport, BackendError> {
    let binding_registry_path = binding_registry_path.as_ref();
    let mut report = ProfileRetirementRecoveryReport::default();
    for store_id in super::retiring_profile_store_ids(binding_registry_path)
        .map_err(|error| BackendError::InvalidRequest(error.to_string()))?
    {
        let store_id = StoreId::new(store_id)
            .map_err(|error| BackendError::InvalidRequest(error.to_string()))?;
        let namespace = format!("profile-s3:{}", store_id.as_str());
        dasobjectstore_metadata::withdraw_profile_catalogue(
            live_sqlite_path.as_ref(),
            &namespace,
            &store_id,
            false,
        )
        .map_err(|error| BackendError::InvalidRequest(error.to_string()))?;
        super::finish_profile_binding_retirement(binding_registry_path, store_id.as_str())
            .map_err(|error| BackendError::InvalidRequest(error.to_string()))?;
        report.retirements_completed += 1;
    }
    Ok(report)
}

pub fn profile_catalogue_live_sqlite_path() -> std::path::PathBuf {
    std::env::var_os("DASOBJECTSTORE_LIVE_SQLITE_PATH")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("DASOBJECTSTORE_SSD_ROOT")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::path::PathBuf::from("/srv/dasobjectstore/ssd"))
                .join(dasobjectstore_metadata::METADATA_DIR_NAME)
                .join(dasobjectstore_metadata::LIVE_SQLITE_FILE_NAME)
        })
}

/// Recover incomplete profile/shared-catalogue publications at daemon start.
/// The current private catalogue always wins over an older journal snapshot.
pub fn recover_profile_catalogue_publications(
    binding_registry_path: impl AsRef<Path>,
    store_registry_path: impl AsRef<Path>,
    live_sqlite_path: impl AsRef<Path>,
    committed_at_utc: &str,
) -> Result<ProfileCatalogueRecoveryReport, BackendError> {
    let bindings = super::read_profile_bindings(binding_registry_path)
        .map_err(|error| BackendError::InvalidRequest(error.to_string()))?;
    if bindings.is_empty() {
        return Ok(ProfileCatalogueRecoveryReport::default());
    }
    let definitions = dasobjectstore_object_service::read_store_registry(store_registry_path)
        .map_err(|error| BackendError::InvalidRequest(error.to_string()))?;
    let mut report = ProfileCatalogueRecoveryReport::default();
    for binding in bindings {
        if binding.manifest.deployment_profile
            != dasobjectstore_core::deployment::DeploymentProfile::Folder
        {
            continue;
        }
        report.stores_scanned += 1;
        let handoff_root = binding
            .backend_root
            .join(".dasobjectstore/profile-catalogue-handoffs");
        let mut incomplete = Vec::new();
        if handoff_root.is_dir() {
            for entry in fs::read_dir(&handoff_root).map_err(io_error)? {
                let entry = entry.map_err(io_error)?;
                let path = entry.path();
                let Some(transaction_id) = path.file_stem().and_then(|value| value.to_str()) else {
                    continue;
                };
                if path.extension().and_then(|value| value.to_str()) != Some("json") {
                    continue;
                }
                if read_profile_catalogue_handoff(&handoff_root, transaction_id)?
                    .is_some_and(|journal| journal.state != ProfileCatalogueHandoffState::Committed)
                {
                    incomplete.push(path);
                }
            }
        }
        if incomplete.is_empty() {
            continue;
        }
        let capacity = definitions
            .iter()
            .find(|definition| definition.store_id == binding.manifest.store_id)
            .ok_or_else(|| BackendError::NotFound("profile capacity policy is unavailable".into()))?
            .policy
            .capacity
            .clone();
        let backend = super::FolderBackend::open(
            &binding.backend_root,
            binding.manifest.clone(),
            capacity,
            0,
        )?;
        let namespace = format!("profile-s3:{}", binding.manifest.store_id.as_str());
        publish_profile_catalogue_with_metadata(
            &binding.manifest.store_id,
            &backend,
            live_sqlite_path.as_ref(),
            &handoff_root,
            &namespace,
            committed_at_utc,
        )?;
        report.stores_republished += 1;
        for path in incomplete {
            fs::remove_file(path).map_err(io_error)?;
            report.stale_journals_removed += 1;
        }
    }
    Ok(report)
}

pub trait ProfileCatalogueBackend: ObjectStoreBackend + ObjectCatalogueAuthority {}

impl<T> ProfileCatalogueBackend for T where T: ObjectStoreBackend + ObjectCatalogueAuthority {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileCatalogueHandoffRecord {
    pub transaction_id: String,
    pub profile_namespace: String,
    pub store_id: StoreId,
    pub catalogue: PortableObjectCatalogue,
    pub state: ProfileCatalogueHandoffState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileCatalogueHandoffState {
    Prepared,
    ProfileCommitted,
    Committed,
}

/// Read a durable handoff journal entry for restart reconciliation without
/// exposing its filesystem path through the daemon protocol.
pub fn read_profile_catalogue_handoff(
    handoff_root: impl AsRef<Path>,
    transaction_id: &str,
) -> Result<Option<ProfileCatalogueHandoffRecord>, BackendError> {
    validate_transaction_id(transaction_id)?;
    let path = handoff_root.as_ref().join(format!("{transaction_id}.json"));
    if !path.is_file() {
        return Ok(None);
    }
    let file = File::open(&path).map_err(io_error)?;
    let journal: HandoffJournal = serde_json::from_reader(file).map_err(|error| {
        BackendError::InvalidRequest(format!("handoff journal JSON is invalid: {error}"))
    })?;
    if journal.schema_version != HANDOFF_JOURNAL_SCHEMA_VERSION
        || journal.transaction_id != transaction_id
    {
        return Err(BackendError::InvalidRequest(
            "unsupported or mismatched profile catalogue handoff journal".to_string(),
        ));
    }
    journal.catalogue.validate().map_err(|error| {
        BackendError::InvalidRequest(format!("handoff journal catalogue is invalid: {error}"))
    })?;
    Ok(Some(ProfileCatalogueHandoffRecord {
        transaction_id: journal.transaction_id,
        profile_namespace: journal.profile_namespace,
        store_id: journal.store_id,
        catalogue: journal.catalogue,
        state: journal.state.into(),
    }))
}

/// Replay an interrupted profile-catalogue handoff after daemon restart.
/// Fully committed journals are safe no-ops; earlier states re-run payload
/// verification and the idempotent profile/SQLite commit path.
pub fn reconcile_profile_catalogue_handoff(
    store_id: &StoreId,
    transaction_id: &str,
    backend: &mut dyn ProfileCatalogueBackend,
    live_sqlite_path: impl AsRef<Path>,
    handoff_root: impl AsRef<Path>,
    committed_at_utc: &str,
) -> Result<u64, BackendError> {
    let Some(handoff) = read_profile_catalogue_handoff(&handoff_root, transaction_id)? else {
        return Err(BackendError::NotFound(format!(
            "profile catalogue handoff {transaction_id} is unavailable"
        )));
    };
    if handoff.store_id != *store_id {
        return Err(BackendError::InvalidRequest(
            "profile catalogue handoff store identity mismatch".to_string(),
        ));
    }
    if handoff.state == ProfileCatalogueHandoffState::Committed {
        return Ok(0);
    }
    import_profile_catalogue_with_metadata(
        store_id,
        &handoff.catalogue,
        backend,
        live_sqlite_path,
        handoff_root,
        transaction_id,
        &handoff.profile_namespace,
        committed_at_utc,
    )
}

/// Convert a daemon-authoritative backend catalogue into profile-neutral
/// metadata. The payload itself is never copied by this function.
pub fn export_profile_catalogue(
    store_id: &StoreId,
    authority: &dyn ObjectCatalogueAuthority,
) -> Result<PortableObjectCatalogue, BackendError> {
    let mut objects = authority.records()?;
    objects.sort_by(|left, right| {
        left.key
            .object_id
            .cmp(&right.key.object_id)
            .then(left.key.version.cmp(&right.key.version))
    });
    let objects = objects
        .into_iter()
        .map(|record| portable_object(store_id, record))
        .collect::<Result<Vec<_>, _>>()?;
    let catalogue = PortableObjectCatalogue {
        schema_version: PORTABLE_OBJECT_CATALOGUE_SCHEMA_VERSION,
        store_id: store_id.clone(),
        objects,
    };
    catalogue.validate().map_err(|error| {
        BackendError::InvalidRequest(format!("portable catalogue export: {error}"))
    })?;
    Ok(catalogue)
}

/// Publish the current profile-authoritative catalogue into shared metadata.
///
/// Normal profile ingress uses this after the backend catalogue commit and
/// before acknowledging the client.  The private journal makes the
/// profile/SQLite crash window restart-reconcilable, while the immutable
/// transaction id makes an exact transport retry idempotent.
pub fn publish_profile_catalogue_with_metadata(
    store_id: &StoreId,
    authority: &dyn ObjectCatalogueAuthority,
    live_sqlite_path: impl AsRef<Path>,
    handoff_root: impl AsRef<Path>,
    profile_namespace: &str,
    committed_at_utc: &str,
) -> Result<u64, BackendError> {
    let catalogue = export_profile_catalogue(store_id, authority)?;
    let encoded = catalogue.encode_json().map_err(|error| {
        BackendError::InvalidRequest(format!("profile catalogue snapshot is invalid: {error}"))
    })?;
    let snapshot_digest = Sha256::digest(encoded.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let transaction_id = format!("profile-s3-{}-{snapshot_digest}", store_id.as_str());
    let journal = HandoffJournal::prepare(
        handoff_root.as_ref(),
        &transaction_id,
        profile_namespace,
        store_id,
        &catalogue,
    )?;
    journal.write_state(HandoffState::ProfileCommitted)?;
    dasobjectstore_metadata::commit_profile_catalogue(
        live_sqlite_path,
        dasobjectstore_metadata::ProfileCatalogueCommitRequest {
            transaction_id: &transaction_id,
            profile_namespace,
            store_id,
            catalogue: &catalogue,
            source_retained: true,
            exact_snapshot: true,
            committed_at_utc,
        },
    )
    .map_err(|error| {
        BackendError::InvalidRequest(format!("profile catalogue metadata publication: {error}"))
    })?;
    journal.write_state(HandoffState::Committed)?;
    Ok(catalogue.objects.len() as u64)
}

/// Verify destination payloads before committing imported catalogue rows.
/// This is intentionally metadata-only: source retirement remains a separate
/// operator-confirmed migration transition.
pub fn import_profile_catalogue(
    store_id: &StoreId,
    catalogue: &PortableObjectCatalogue,
    backend: &mut dyn ProfileCatalogueBackend,
) -> Result<u64, BackendError> {
    catalogue.validate().map_err(|error| {
        BackendError::InvalidRequest(format!("portable catalogue import: {error}"))
    })?;
    if catalogue.store_id != *store_id {
        return Err(BackendError::InvalidRequest(
            "portable catalogue store identity does not match destination".to_string(),
        ));
    }
    let mut records = Vec::with_capacity(catalogue.objects.len());
    for object in &catalogue.objects {
        let Some(placement) = object.placements.first() else {
            return Err(BackendError::InvalidRequest(format!(
                "portable object {} has no placement",
                object.object_id
            )));
        };
        let key = BackendObjectKey {
            object_id: object.object_id.to_string(),
            version: object.version,
        };
        let verified = backend.verify(&key)?;
        let expected_checksum = digest_value(&object.checksum)?;
        let placement_checksum = digest_value(&placement.checksum)?;
        if verified.size_bytes != object.size_bytes
            || verified.checksum != expected_checksum
            || placement_checksum != verified.checksum
        {
            return Err(BackendError::InvalidRequest(format!(
                "portable object {}:{} does not match destination payload",
                key.object_id, key.version
            )));
        }
        records.push(verified);
    }
    backend.commit_batch(&records)?;
    Ok(records.len() as u64)
}

/// Import through both the profile authority and the daemon-owned shared
/// SQLite handoff. The profile commit remains the payload authority; the
/// SQLite adapter records the exact verified logical transaction for restart,
/// replay, and a future physical-placement adapter.
pub fn import_profile_catalogue_with_metadata(
    store_id: &StoreId,
    catalogue: &PortableObjectCatalogue,
    backend: &mut dyn ProfileCatalogueBackend,
    live_sqlite_path: impl AsRef<Path>,
    handoff_root: impl AsRef<Path>,
    transaction_id: &str,
    profile_namespace: &str,
    committed_at_utc: &str,
) -> Result<u64, BackendError> {
    let journal = HandoffJournal::prepare(
        handoff_root.as_ref(),
        transaction_id,
        profile_namespace,
        store_id,
        catalogue,
    )?;
    let imported = import_profile_catalogue(store_id, catalogue, backend)?;
    journal.write_state(HandoffState::ProfileCommitted)?;
    dasobjectstore_metadata::commit_profile_catalogue(
        live_sqlite_path,
        dasobjectstore_metadata::ProfileCatalogueCommitRequest {
            transaction_id,
            profile_namespace,
            store_id,
            catalogue,
            source_retained: true,
            exact_snapshot: false,
            committed_at_utc,
        },
    )
    .map_err(|error| {
        BackendError::InvalidRequest(format!("profile catalogue metadata handoff: {error}"))
    })?;
    journal.write_state(HandoffState::Committed)?;
    Ok(imported)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct HandoffJournal {
    schema_version: u16,
    transaction_id: String,
    profile_namespace: String,
    store_id: StoreId,
    catalogue: PortableObjectCatalogue,
    state: HandoffState,
    #[serde(skip)]
    path: std::path::PathBuf,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum HandoffState {
    Prepared,
    ProfileCommitted,
    Committed,
}

const HANDOFF_JOURNAL_SCHEMA_VERSION: u16 = 1;

impl From<HandoffState> for ProfileCatalogueHandoffState {
    fn from(state: HandoffState) -> Self {
        match state {
            HandoffState::Prepared => Self::Prepared,
            HandoffState::ProfileCommitted => Self::ProfileCommitted,
            HandoffState::Committed => Self::Committed,
        }
    }
}

impl HandoffJournal {
    fn prepare(
        root: &Path,
        transaction_id: &str,
        profile_namespace: &str,
        store_id: &StoreId,
        catalogue: &PortableObjectCatalogue,
    ) -> Result<Self, BackendError> {
        validate_transaction_id(transaction_id)?;
        fs::create_dir_all(root).map_err(io_error)?;
        let journal = Self {
            schema_version: HANDOFF_JOURNAL_SCHEMA_VERSION,
            transaction_id: transaction_id.to_string(),
            profile_namespace: profile_namespace.to_string(),
            store_id: store_id.clone(),
            catalogue: catalogue.clone(),
            state: HandoffState::Prepared,
            path: root.join(format!("{transaction_id}.json")),
        };
        journal.persist()
    }

    fn write_state(&self, state: HandoffState) -> Result<(), BackendError> {
        let mut next = self.clone();
        next.state = state;
        next.persist().map(|_| ())
    }

    fn persist(&self) -> Result<Self, BackendError> {
        let parent = self.path.parent().ok_or_else(|| {
            BackendError::InvalidRequest("handoff journal path has no parent".to_string())
        })?;
        let bytes = serde_json::to_vec_pretty(self).map_err(|error| {
            BackendError::InvalidRequest(format!("handoff journal encode failed: {error}"))
        })?;
        let temporary = parent.join(format!(
            ".handoff-{}-{}.tmp",
            self.transaction_id,
            std::process::id()
        ));
        let mut options = OpenOptions::new();
        options.create(true).truncate(true).write(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(&temporary).map_err(io_error)?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(io_error)?;
        drop(file);
        fs::rename(&temporary, &self.path).map_err(io_error)?;
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(io_error)?;
        Ok(self.clone())
    }
}

fn validate_transaction_id(transaction_id: &str) -> Result<(), BackendError> {
    if transaction_id.is_empty()
        || transaction_id.contains('/')
        || transaction_id.contains('\\')
        || transaction_id == "."
        || transaction_id == ".."
    {
        return Err(BackendError::InvalidRequest(
            "profile catalogue transaction id must be a safe filename".to_string(),
        ));
    }
    Ok(())
}

fn io_error(error: std::io::Error) -> BackendError {
    BackendError::Io(error.to_string())
}

fn portable_object(
    store_id: &StoreId,
    record: BackendObjectRecord,
) -> Result<PortableObjectVersion, BackendError> {
    let object_id = ObjectId::new(record.key.object_id.clone())
        .map_err(|error| BackendError::InvalidRequest(error.to_string()))?;
    let placement_id = PlacementId::new(format!("{}-{}", record.key.object_id, record.key.version))
        .map_err(|error| BackendError::InvalidRequest(error.to_string()))?;
    let digest = parse_digest(&record.checksum)?;
    Ok(PortableObjectVersion {
        object_id,
        version: record.key.version,
        size_bytes: record.size_bytes,
        checksum: digest.clone(),
        provenance: PortableProvenance {
            source_kind: "profile_backend".to_string(),
            locator: Some(format!("{store_id}/{}", record.key.object_id)),
            revision: Some(record.key.version.to_string()),
        },
        lifecycle: PortableLifecycleState::HashVerified,
        protection_policy: ProtectionPolicy::LocalOnly,
        protection_state: PortableProtectionState::Verified,
        placements: vec![PortablePlacement {
            placement_id,
            location: PortablePlacementLocation::Folder {
                relative_path: record.key.object_id,
            },
            checksum: digest,
            verified_at_utc: None,
        }],
    })
}

fn parse_digest(value: &str) -> Result<ObjectDigest, BackendError> {
    let Some((algorithm, digest)) = value.split_once(':') else {
        return Err(BackendError::InvalidRequest(
            "portable catalogue checksums must use algorithm:value form".to_string(),
        ));
    };
    if algorithm.trim().is_empty() || digest.trim().is_empty() {
        return Err(BackendError::InvalidRequest(
            "portable catalogue checksum must not be blank".to_string(),
        ));
    }
    Ok(ObjectDigest {
        algorithm: algorithm.to_string(),
        value: digest.to_string(),
    })
}

fn digest_value(digest: &ObjectDigest) -> Result<String, BackendError> {
    if digest.algorithm.trim().is_empty() || digest.value.trim().is_empty() {
        return Err(BackendError::InvalidRequest(
            "portable catalogue checksum must not be blank".to_string(),
        ));
    }
    Ok(format!("{}:{}", digest.algorithm, digest.value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dasobjectstore_core::backend::{BackendCapabilities, BackendHealth};
    use dasobjectstore_core::deployment::{DeploymentProfile, HostMode};
    use dasobjectstore_core::manifest::{
        BackendReference, ObjectStoreManifest, OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
    };
    use dasobjectstore_core::protection::ProtectionPolicy;
    use rusqlite::Connection;
    use std::io::{Cursor, Read};

    #[test]
    fn startup_recovery_is_a_noop_without_profile_bindings() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-profile-recovery-empty-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root");
        let report = recover_profile_catalogue_publications(
            root.join("missing-bindings.json"),
            root.join("missing-stores.json"),
            root.join("missing-live.sqlite"),
            "2026-07-16T00:00:00Z",
        )
        .expect("empty recovery");
        assert_eq!(report, ProfileCatalogueRecoveryReport::default());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn startup_recovery_completes_interrupted_profile_retirement() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-profile-retirement-recovery-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let backend_root = root.join("backend");
        std::fs::create_dir_all(&backend_root).expect("backend root");
        let registry = root.join("bindings.json");
        let store_id = StoreId::new("retiring-store").expect("store id");
        super::super::upsert_profile_binding(
            &registry,
            super::super::BackendProfileBinding {
                manifest: ObjectStoreManifest {
                    schema_version: OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
                    store_id: store_id.clone(),
                    deployment_profile: DeploymentProfile::Folder,
                    host_mode: HostMode::PerUser,
                    protection: ProtectionPolicy::LocalOnly,
                    backend: BackendReference::Folder {
                        root_identity: "fsid:retiring-store".to_string(),
                    },
                },
                backend_root,
                ssd_staging_root: None,
            },
        )
        .expect("binding");
        super::super::begin_profile_binding_retirement(
            &registry,
            store_id.as_str(),
            "2026-07-16T12:00:00Z",
        )
        .expect("begin retirement");

        let live_sqlite = root.join("live.sqlite");
        let connection = Connection::open(&live_sqlite).expect("live sqlite");
        connection
            .execute_batch(dasobjectstore_metadata::LIVE_SCHEMA_SQL)
            .expect("schema");
        connection
            .execute(
                "INSERT INTO pools VALUES ('pool-a', 'Clean', 'now', 'now')",
                [],
            )
            .expect("pool");
        connection
            .execute(
                "INSERT INTO stores VALUES ('retiring-store', 'pool-a', 'folder', '{}', 'now', 'now')",
                [],
            )
            .expect("store");
        connection
            .execute(
                "INSERT INTO profile_catalogue_transactions VALUES (
                    'retire-tx', 'profile-s3:retiring-store', 'retiring-store', 1, 1, '{}', 'now'
                )",
                [],
            )
            .expect("shared transaction");
        drop(connection);

        let report =
            recover_profile_retirements(&registry, &live_sqlite).expect("startup recovery");
        assert_eq!(report.retirements_completed, 1);
        assert!(super::super::retiring_profile_store_ids(&registry)
            .expect("retiring stores")
            .is_empty());
        assert!(
            super::super::profile_binding_retired_at(&registry, store_id.as_str())
                .expect("retired timestamp")
                .is_some()
        );
        let connection = Connection::open(live_sqlite).expect("live sqlite");
        let remaining = connection
            .query_row(
                "SELECT COUNT(*) FROM profile_catalogue_transactions",
                [],
                |row| row.get::<_, i64>(0),
            )
            .expect("remaining transactions");
        assert_eq!(remaining, 0);
        let _ = std::fs::remove_dir_all(root);
    }

    struct FakeBackend {
        record: BackendObjectRecord,
    }

    impl ObjectStoreBackend for FakeBackend {
        fn capabilities(&self) -> BackendCapabilities {
            BackendCapabilities::complete()
        }
        fn validate_manifest(&self, _: &ObjectStoreManifest) -> Result<(), BackendError> {
            Ok(())
        }
        fn reserve(&mut self, _: &str, _: u64) -> Result<(), BackendError> {
            Ok(())
        }
        fn stage(
            &mut self,
            _: &str,
            _: &BackendObjectKey,
            _: &mut dyn Read,
        ) -> Result<BackendObjectRecord, BackendError> {
            Ok(self.record.clone())
        }
        fn finalize(
            &mut self,
            staged: BackendObjectRecord,
        ) -> Result<BackendObjectRecord, BackendError> {
            Ok(staged)
        }
        fn read(&self, _: &BackendObjectKey) -> Result<Box<dyn Read + Send>, BackendError> {
            Ok(Box::new(Cursor::new(Vec::<u8>::new())))
        }
        fn enumerate(&self, _: Option<&str>) -> Result<Vec<BackendObjectRecord>, BackendError> {
            Ok(vec![self.record.clone()])
        }
        fn verify(&self, key: &BackendObjectKey) -> Result<BackendObjectRecord, BackendError> {
            if key == &self.record.key {
                Ok(self.record.clone())
            } else {
                Err(BackendError::NotFound(key.object_id.clone()))
            }
        }
        fn reconcile(&mut self) -> Result<Vec<BackendObjectRecord>, BackendError> {
            Ok(vec![self.record.clone()])
        }
        fn remove(&mut self, _: &BackendObjectKey) -> Result<(), BackendError> {
            Ok(())
        }
        fn health(&self) -> Result<BackendHealth, BackendError> {
            Ok(BackendHealth {
                state: "ready".to_string(),
                message: None,
            })
        }
    }

    impl ObjectCatalogueAuthority for FakeBackend {
        fn records(&self) -> Result<Vec<BackendObjectRecord>, BackendError> {
            Ok(vec![self.record.clone()])
        }
        fn commit_batch(&mut self, records: &[BackendObjectRecord]) -> Result<(), BackendError> {
            assert_eq!(records, &[self.record.clone()]);
            Ok(())
        }
        fn remove_record(&mut self, _: &BackendObjectKey) -> Result<(), BackendError> {
            Ok(())
        }
    }

    #[test]
    fn export_then_import_verifies_destination_and_retains_source() {
        let store_id = StoreId::new("codex").expect("store");
        let record = BackendObjectRecord {
            key: BackendObjectKey {
                object_id: "reads/a.txt".to_string(),
                version: 1,
            },
            size_bytes: 4,
            checksum: "sha256:abcd".to_string(),
            location: ".dasobjectstore/objects/reads/a.txt".to_string(),
        };
        let mut backend = FakeBackend { record };
        let catalogue = export_profile_catalogue(&store_id, &backend).expect("export");
        assert_eq!(catalogue.objects.len(), 1);
        let imported =
            import_profile_catalogue(&store_id, &catalogue, &mut backend).expect("import");
        assert_eq!(imported, 1);
        assert_eq!(catalogue.store_id, store_id);
    }

    #[test]
    fn import_records_replay_safe_metadata_transaction() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-profile-catalogue-daemon-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root");
        let db = root.join("live.sqlite");
        let connection = Connection::open(&db).expect("db");
        connection
            .execute_batch(dasobjectstore_metadata::LIVE_SCHEMA_SQL)
            .expect("schema");
        connection
            .execute(
                "INSERT INTO pools VALUES ('pool-a', 'Clean', 'now', 'now')",
                [],
            )
            .expect("pool");
        connection
            .execute(
                "INSERT INTO stores VALUES ('codex', 'pool-a', 'folder', '{}', 'now', 'now')",
                [],
            )
            .expect("store");
        drop(connection);

        let store_id = StoreId::new("codex").expect("store");
        let record = BackendObjectRecord {
            key: BackendObjectKey {
                object_id: "reads/a.txt".to_string(),
                version: 1,
            },
            size_bytes: 4,
            checksum: "sha256:abcd".to_string(),
            location: ".dasobjectstore/objects/reads/a.txt".to_string(),
        };
        let mut backend = FakeBackend { record };
        let catalogue = export_profile_catalogue(&store_id, &backend).expect("export");
        let imported = import_profile_catalogue_with_metadata(
            &store_id,
            &catalogue,
            &mut backend,
            &db,
            root.join("handoffs"),
            "tx-daemon-1",
            "folder:codex",
            "2026-07-14T00:00:00Z",
        )
        .expect("metadata handoff");
        assert_eq!(imported, 1);
        assert!(root.join("handoffs/tx-daemon-1.json").is_file());
        let handoff = read_profile_catalogue_handoff(root.join("handoffs"), "tx-daemon-1")
            .expect("read handoff")
            .expect("handoff exists");
        assert_eq!(handoff.state, ProfileCatalogueHandoffState::Committed);
        assert_eq!(handoff.catalogue, catalogue);
        let connection = Connection::open(&db).expect("db");
        assert_eq!(
            connection
                .query_row(
                    "SELECT COUNT(*) FROM profile_catalogue_objects",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .expect("count"),
            1
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn publication_failure_retains_profile_committed_restart_journal() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-profile-publication-failure-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root");
        let store_id = StoreId::new("codex").expect("store");
        let backend = FakeBackend {
            record: BackendObjectRecord {
                key: BackendObjectKey {
                    object_id: "reads/recover.txt".to_string(),
                    version: 1,
                },
                size_bytes: 4,
                checksum: "sha256:abcd".to_string(),
                location: ".dasobjectstore/objects/reads/recover.txt".to_string(),
            },
        };
        let handoffs = root.join("handoffs");
        let error = publish_profile_catalogue_with_metadata(
            &store_id,
            &backend,
            root.join("missing/live.sqlite"),
            &handoffs,
            "profile-s3:codex",
            "2026-07-16T00:00:00Z",
        )
        .expect_err("missing SQLite parent must fail");
        assert!(error.to_string().contains("metadata publication"));
        let journal_path = std::fs::read_dir(&handoffs)
            .expect("handoff directory")
            .next()
            .expect("journal entry")
            .expect("journal path")
            .path();
        let transaction_id = journal_path
            .file_stem()
            .expect("journal stem")
            .to_string_lossy();
        let journal = read_profile_catalogue_handoff(&handoffs, &transaction_id)
            .expect("journal read")
            .expect("journal retained");
        assert_eq!(
            journal.state,
            ProfileCatalogueHandoffState::ProfileCommitted
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn reconciles_a_prepared_handoff_after_restart() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-profile-catalogue-reconcile-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root");
        let db = root.join("live.sqlite");
        let connection = Connection::open(&db).expect("db");
        connection
            .execute_batch(dasobjectstore_metadata::LIVE_SCHEMA_SQL)
            .expect("schema");
        connection
            .execute(
                "INSERT INTO pools VALUES ('pool-a', 'Clean', 'now', 'now')",
                [],
            )
            .expect("pool");
        connection
            .execute(
                "INSERT INTO stores VALUES ('codex', 'pool-a', 'folder', '{}', 'now', 'now')",
                [],
            )
            .expect("store");
        drop(connection);

        let store_id = StoreId::new("codex").expect("store");
        let record = BackendObjectRecord {
            key: BackendObjectKey {
                object_id: "reads/recover.txt".to_string(),
                version: 1,
            },
            size_bytes: 4,
            checksum: "sha256:abcd".to_string(),
            location: ".dasobjectstore/objects/reads/recover.txt".to_string(),
        };
        let mut backend = FakeBackend { record };
        let catalogue = export_profile_catalogue(&store_id, &backend).expect("export");
        let handoff_root = root.join("handoffs");
        HandoffJournal::prepare(
            &handoff_root,
            "tx-recover",
            "folder:codex",
            &store_id,
            &catalogue,
        )
        .expect("prepare journal");
        let imported = reconcile_profile_catalogue_handoff(
            &store_id,
            "tx-recover",
            &mut backend,
            &db,
            &handoff_root,
            "2026-07-14T00:00:00Z",
        )
        .expect("reconcile");
        assert_eq!(imported, 1);
        assert_eq!(
            read_profile_catalogue_handoff(&handoff_root, "tx-recover")
                .expect("read")
                .expect("entry")
                .state,
            ProfileCatalogueHandoffState::Committed
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
