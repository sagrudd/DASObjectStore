use dasobjectstore_core::manifest::ObjectStoreManifest;
use dasobjectstore_core::migration::{MigrationState, StoreMigration};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::{self, Display};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const MIGRATION_PROVENANCE_SCHEMA: &str = "dasobjectstore.migration_provenance.v1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationVerificationState {
    Pending,
    Verified,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationProvenanceRecord {
    pub schema_version: String,
    pub migration_id: String,
    pub source_store_id: dasobjectstore_core::ids::StoreId,
    pub destination_store_id: dasobjectstore_core::ids::StoreId,
    pub source_manifest_sha256: String,
    pub destination_manifest_sha256: String,
    pub verification_state: MigrationVerificationState,
    pub destination_verified_at_utc: Option<String>,
    pub source_retained: bool,
    pub retirement_actor: Option<String>,
    pub retired_at_utc: Option<String>,
}

/// Create or reopen the durable provenance record before migration payload work.
/// Exact retries are idempotent; changed identities or manifests fail closed.
pub fn prepare_migration_provenance(
    root: impl AsRef<Path>,
    migration: &StoreMigration,
    source_manifest: &ObjectStoreManifest,
    destination_manifest: &ObjectStoreManifest,
) -> Result<MigrationProvenanceRecord, MigrationProvenanceError> {
    validate_migration_manifests(migration, source_manifest, destination_manifest)?;
    let record = MigrationProvenanceRecord {
        schema_version: MIGRATION_PROVENANCE_SCHEMA.to_string(),
        migration_id: migration.migration_id.clone(),
        source_store_id: migration.source_store_id.clone(),
        destination_store_id: migration.destination_store_id.clone(),
        source_manifest_sha256: manifest_digest(source_manifest)?,
        destination_manifest_sha256: manifest_digest(destination_manifest)?,
        verification_state: MigrationVerificationState::Pending,
        destination_verified_at_utc: None,
        source_retained: true,
        retirement_actor: None,
        retired_at_utc: None,
    };
    record.validate()?;
    let path = provenance_path(root.as_ref(), &migration.migration_id)?;
    if path.is_file() {
        let existing = read_record(&path)?;
        validate_immutable_identity(&existing, &record)?;
        return Ok(existing);
    }
    persist_record(&path, &record)
}

/// Persist destination verification after the destination catalogue commit.
pub fn record_migration_destination_verified(
    root: impl AsRef<Path>,
    migration: &StoreMigration,
    source_manifest: &ObjectStoreManifest,
    destination_manifest: &ObjectStoreManifest,
    verified_at_utc: &str,
) -> Result<MigrationProvenanceRecord, MigrationProvenanceError> {
    validate_nonblank(verified_at_utc, "destination verification time")?;
    if migration.state != MigrationState::RetirementPending || !migration.source_retained {
        return Err(MigrationProvenanceError::InvalidState(
            "destination verification requires a retirement-pending migration with its source retained"
                .to_string(),
        ));
    }
    let expected = prepare_or_read(
        root.as_ref(),
        migration,
        source_manifest,
        destination_manifest,
    )?;
    if expected.verification_state == MigrationVerificationState::Verified {
        if expected.destination_verified_at_utc.as_deref() == Some(verified_at_utc) {
            return Ok(expected);
        }
        return Err(MigrationProvenanceError::Conflict(
            "destination verification was already recorded at a different time".to_string(),
        ));
    }
    let mut verified = expected;
    verified.verification_state = MigrationVerificationState::Verified;
    verified.destination_verified_at_utc = Some(verified_at_utc.to_string());
    persist_record(
        &provenance_path(root.as_ref(), &migration.migration_id)?,
        &verified,
    )
}

/// Persist administrator authorization before any physical source placement
/// is removed. The source remains retained until the separate completion call.
pub fn authorize_migration_source_retirement(
    root: impl AsRef<Path>,
    migration: &StoreMigration,
    retirement_actor: &str,
    retired_at_utc: &str,
) -> Result<MigrationProvenanceRecord, MigrationProvenanceError> {
    validate_nonblank(retirement_actor, "retirement actor")?;
    validate_nonblank(retired_at_utc, "retirement time")?;
    let path = provenance_path(root.as_ref(), &migration.migration_id)?;
    let mut record = read_record(&path)?;
    validate_record_matches_migration(&record, migration)?;

    if migration.state != MigrationState::RetirementPending || !migration.source_retained {
        return Err(MigrationProvenanceError::InvalidState(
            "source retirement requires a retirement-pending migration".to_string(),
        ));
    }
    if record.verification_state != MigrationVerificationState::Verified
        || record.destination_verified_at_utc.is_none()
    {
        return Err(MigrationProvenanceError::InvalidState(
            "source retirement requires durable destination verification".to_string(),
        ));
    }
    if let (Some(actor), Some(retired_at)) = (&record.retirement_actor, &record.retired_at_utc) {
        if actor != retirement_actor || retired_at != retired_at_utc {
            return Err(MigrationProvenanceError::Conflict(
                "source retirement authorization does not match the durable record".to_string(),
            ));
        }
        return Ok(record);
    }
    record.retirement_actor = Some(retirement_actor.to_string());
    record.retired_at_utc = Some(retired_at_utc.to_string());
    persist_record(&path, &record)
}

/// Complete the logical transition only after the caller has removed the
/// source placement. Matching authorization must already be durable.
pub fn complete_migration_source_retirement(
    root: impl AsRef<Path>,
    checkpoint_path: impl AsRef<Path>,
    migration: &mut StoreMigration,
    retirement_actor: &str,
    retired_at_utc: &str,
) -> Result<MigrationProvenanceRecord, MigrationProvenanceError> {
    validate_nonblank(retirement_actor, "retirement actor")?;
    validate_nonblank(retired_at_utc, "retirement time")?;
    let path = provenance_path(root.as_ref(), &migration.migration_id)?;
    let mut record = read_record(&path)?;
    validate_record_matches_migration(&record, migration)?;

    if migration.state == MigrationState::Completed && !record.source_retained {
        if record.retirement_actor.as_deref() == Some(retirement_actor)
            && record.retired_at_utc.as_deref() == Some(retired_at_utc)
        {
            return Ok(record);
        }
        return Err(MigrationProvenanceError::Conflict(
            "source retirement was already completed by a different authorization".to_string(),
        ));
    }
    if migration.state != MigrationState::RetirementPending || !migration.source_retained {
        return Err(MigrationProvenanceError::InvalidState(
            "source retirement requires a retirement-pending migration".to_string(),
        ));
    }
    if record.retirement_actor.as_deref() != Some(retirement_actor)
        || record.retired_at_utc.as_deref() != Some(retired_at_utc)
    {
        return Err(MigrationProvenanceError::InvalidState(
            "source retirement requires matching durable administrator authorization".to_string(),
        ));
    }

    let previous = migration.clone();
    migration
        .confirm_source_retirement()
        .map_err(|error| MigrationProvenanceError::InvalidState(error.to_string()))?;
    if let Err(error) = migration.save_atomic(checkpoint_path) {
        *migration = previous;
        return Err(MigrationProvenanceError::Persistence(error.to_string()));
    }

    record.source_retained = false;
    persist_record(&path, &record)
}

/// Repair the only two valid crash windows between the checkpoint and sidecar.
/// Other disagreements fail closed for operator inspection.
pub fn reconcile_migration_provenance(
    root: impl AsRef<Path>,
    checkpoint_path: impl AsRef<Path>,
    migration: &mut StoreMigration,
) -> Result<MigrationProvenanceRecord, MigrationProvenanceError> {
    let path = provenance_path(root.as_ref(), &migration.migration_id)?;
    let mut record = read_record(&path)?;
    validate_record_matches_migration(&record, migration)?;
    let retirement_authorized =
        record.retirement_actor.is_some() && record.retired_at_utc.is_some();

    match (
        migration.state,
        migration.source_retained,
        record.source_retained,
    ) {
        (MigrationState::Completed, false, false) => Ok(record),
        (MigrationState::Completed, false, true) if retirement_authorized => {
            record.source_retained = false;
            persist_record(&path, &record)
        }
        (MigrationState::RetirementPending, true, false) if retirement_authorized => {
            migration
                .confirm_source_retirement()
                .map_err(|error| MigrationProvenanceError::InvalidState(error.to_string()))?;
            migration
                .save_atomic(checkpoint_path)
                .map_err(|error| MigrationProvenanceError::Persistence(error.to_string()))?;
            Ok(record)
        }
        (MigrationState::RetirementPending, true, true) => Ok(record),
        _ => Err(MigrationProvenanceError::InvalidState(
            "migration checkpoint and provenance sidecar disagree".to_string(),
        )),
    }
}

pub fn read_migration_provenance(
    root: impl AsRef<Path>,
    migration_id: &str,
) -> Result<Option<MigrationProvenanceRecord>, MigrationProvenanceError> {
    let path = provenance_path(root.as_ref(), migration_id)?;
    path.is_file().then(|| read_record(&path)).transpose()
}

fn prepare_or_read(
    root: &Path,
    migration: &StoreMigration,
    source_manifest: &ObjectStoreManifest,
    destination_manifest: &ObjectStoreManifest,
) -> Result<MigrationProvenanceRecord, MigrationProvenanceError> {
    let path = provenance_path(root, &migration.migration_id)?;
    if path.is_file() {
        let record = read_record(&path)?;
        validate_record_matches_migration(&record, migration)?;
        if record.source_manifest_sha256 != manifest_digest(source_manifest)?
            || record.destination_manifest_sha256 != manifest_digest(destination_manifest)?
        {
            return Err(MigrationProvenanceError::Conflict(
                "migration manifest digest does not match durable provenance".to_string(),
            ));
        }
        return Ok(record);
    }
    prepare_migration_provenance(root, migration, source_manifest, destination_manifest)
}

fn validate_migration_manifests(
    migration: &StoreMigration,
    source_manifest: &ObjectStoreManifest,
    destination_manifest: &ObjectStoreManifest,
) -> Result<(), MigrationProvenanceError> {
    source_manifest
        .validate()
        .map_err(|error| MigrationProvenanceError::InvalidState(error.to_string()))?;
    destination_manifest
        .validate()
        .map_err(|error| MigrationProvenanceError::InvalidState(error.to_string()))?;
    if migration.source_store_id != source_manifest.store_id
        || migration.destination_store_id != destination_manifest.store_id
    {
        return Err(MigrationProvenanceError::InvalidState(
            "migration store identities do not match the supplied manifests".to_string(),
        ));
    }
    Ok(())
}

fn validate_record_matches_migration(
    record: &MigrationProvenanceRecord,
    migration: &StoreMigration,
) -> Result<(), MigrationProvenanceError> {
    record.validate()?;
    if record.migration_id != migration.migration_id
        || record.source_store_id != migration.source_store_id
        || record.destination_store_id != migration.destination_store_id
    {
        return Err(MigrationProvenanceError::Conflict(
            "migration checkpoint identity does not match durable provenance".to_string(),
        ));
    }
    Ok(())
}

fn validate_immutable_identity(
    existing: &MigrationProvenanceRecord,
    expected: &MigrationProvenanceRecord,
) -> Result<(), MigrationProvenanceError> {
    if existing.migration_id != expected.migration_id
        || existing.source_store_id != expected.source_store_id
        || existing.destination_store_id != expected.destination_store_id
        || existing.source_manifest_sha256 != expected.source_manifest_sha256
        || existing.destination_manifest_sha256 != expected.destination_manifest_sha256
    {
        return Err(MigrationProvenanceError::Conflict(
            "migration provenance already exists with different immutable content".to_string(),
        ));
    }
    Ok(())
}

impl MigrationProvenanceRecord {
    fn validate(&self) -> Result<(), MigrationProvenanceError> {
        if self.schema_version != MIGRATION_PROVENANCE_SCHEMA {
            return Err(MigrationProvenanceError::UnsupportedSchema(
                self.schema_version.clone(),
            ));
        }
        validate_nonblank(&self.migration_id, "migration id")?;
        validate_digest(&self.source_manifest_sha256, "source manifest digest")?;
        validate_digest(
            &self.destination_manifest_sha256,
            "destination manifest digest",
        )?;
        if self.source_store_id == self.destination_store_id {
            return Err(MigrationProvenanceError::InvalidState(
                "migration source and destination must differ".to_string(),
            ));
        }
        match (self.verification_state, &self.destination_verified_at_utc) {
            (MigrationVerificationState::Pending, None)
            | (MigrationVerificationState::Verified, Some(_)) => {}
            _ => {
                return Err(MigrationProvenanceError::InvalidState(
                    "verification state and time are inconsistent".to_string(),
                ));
            }
        }
        if self
            .destination_verified_at_utc
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
            || self
                .retirement_actor
                .as_deref()
                .is_some_and(|value| value.trim().is_empty())
            || self
                .retired_at_utc
                .as_deref()
                .is_some_and(|value| value.trim().is_empty())
            || self.retirement_actor.is_some() != self.retired_at_utc.is_some()
            || (!self.source_retained && self.retirement_actor.is_none())
        {
            return Err(MigrationProvenanceError::InvalidState(
                "migration provenance timestamps or retirement authorization are inconsistent"
                    .to_string(),
            ));
        }
        Ok(())
    }
}

fn manifest_digest(manifest: &ObjectStoreManifest) -> Result<String, MigrationProvenanceError> {
    let encoded = serde_json::to_vec(manifest)
        .map_err(|error| MigrationProvenanceError::Persistence(error.to_string()))?;
    Ok(format!("sha256:{:x}", Sha256::digest(encoded)))
}

fn validate_digest(value: &str, field: &str) -> Result<(), MigrationProvenanceError> {
    if value.len() != 71
        || !value.starts_with("sha256:")
        || !value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(MigrationProvenanceError::InvalidState(format!(
            "{field} must be a sha256 digest"
        )));
    }
    Ok(())
}

fn validate_nonblank(value: &str, field: &str) -> Result<(), MigrationProvenanceError> {
    if value.trim().is_empty() {
        return Err(MigrationProvenanceError::InvalidState(format!(
            "{field} must not be blank"
        )));
    }
    Ok(())
}

fn provenance_path(root: &Path, migration_id: &str) -> Result<PathBuf, MigrationProvenanceError> {
    validate_nonblank(migration_id, "migration id")?;
    let digest = Sha256::digest(migration_id.as_bytes());
    Ok(root.join(format!("{:x}.json", digest)))
}

fn read_record(path: &Path) -> Result<MigrationProvenanceRecord, MigrationProvenanceError> {
    let file = File::open(path).map_err(io_error)?;
    let record: MigrationProvenanceRecord = serde_json::from_reader(file)
        .map_err(|error| MigrationProvenanceError::Malformed(error.to_string()))?;
    record.validate()?;
    Ok(record)
}

fn persist_record(
    path: &Path,
    record: &MigrationProvenanceRecord,
) -> Result<MigrationProvenanceRecord, MigrationProvenanceError> {
    record.validate()?;
    let parent = path.parent().ok_or_else(|| {
        MigrationProvenanceError::Persistence("provenance path has no parent".to_string())
    })?;
    fs::create_dir_all(parent).map_err(io_error)?;
    let bytes = serde_json::to_vec_pretty(record)
        .map_err(|error| MigrationProvenanceError::Malformed(error.to_string()))?;
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let temporary = parent.join(format!(
        ".migration-provenance-{}-{}.tmp",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let write_result = (|| {
        let mut file = options.open(&temporary).map_err(io_error)?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(io_error)?;
        drop(file);
        fs::rename(&temporary, path).map_err(io_error)?;
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(io_error)
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    write_result.map(|_| record.clone())
}

fn io_error(error: std::io::Error) -> MigrationProvenanceError {
    MigrationProvenanceError::Persistence(error.to_string())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MigrationProvenanceError {
    Persistence(String),
    Malformed(String),
    UnsupportedSchema(String),
    InvalidState(String),
    Conflict(String),
}

impl Display for MigrationProvenanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Persistence(message) => {
                write!(formatter, "migration provenance I/O failed: {message}")
            }
            Self::Malformed(message) => {
                write!(formatter, "malformed migration provenance: {message}")
            }
            Self::UnsupportedSchema(schema) => {
                write!(
                    formatter,
                    "unsupported migration provenance schema {schema}"
                )
            }
            Self::InvalidState(message) => {
                write!(formatter, "invalid migration provenance: {message}")
            }
            Self::Conflict(message) => {
                write!(formatter, "migration provenance conflict: {message}")
            }
        }
    }
}

impl std::error::Error for MigrationProvenanceError {}

#[cfg(test)]
mod tests {
    use super::*;
    use dasobjectstore_core::deployment::{DeploymentProfile, HostMode};
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_core::manifest::{
        BackendReference, ObjectStoreManifest, OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
    };
    use dasobjectstore_core::protection::ProtectionPolicy;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn manifest(store: &str, identity: &str) -> ObjectStoreManifest {
        ObjectStoreManifest {
            schema_version: OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
            store_id: StoreId::new(store).expect("store id"),
            deployment_profile: DeploymentProfile::Folder,
            host_mode: HostMode::PerUser,
            protection: ProtectionPolicy::LocalOnly,
            backend: BackendReference::Folder {
                root_identity: identity.to_string(),
            },
        }
    }

    fn fixture(name: &str) -> (PathBuf, PathBuf) {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let root = std::env::var_os("DASOBJECTSTORE_CODEX_VALIDATION_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(format!(
                "migration-provenance-{name}-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
        let checkpoint = root.join("checkpoint.json");
        (root.join("provenance"), checkpoint)
    }

    fn retirement_pending() -> (StoreMigration, ObjectStoreManifest, ObjectStoreManifest) {
        let source = manifest("source", "source-root");
        let destination = manifest("destination", "destination-root");
        let mut migration = StoreMigration::new(
            "migration-1",
            source.store_id.clone(),
            destination.store_id.clone(),
        )
        .expect("migration");
        migration.start_copy().expect("copying");
        migration
            .mark_destination_verified()
            .expect("retirement pending");
        (migration, source, destination)
    }

    #[test]
    fn persists_verified_provenance_before_retirement_completion() {
        let (root, checkpoint) = fixture("complete");
        let (mut migration, source, destination) = retirement_pending();
        prepare_migration_provenance(&root, &migration, &source, &destination)
            .expect("provenance prepares");
        let verified = record_migration_destination_verified(
            &root,
            &migration,
            &source,
            &destination,
            "2026-07-15T12:00:00Z",
        )
        .expect("verification records");
        assert_eq!(
            verified.verification_state,
            MigrationVerificationState::Verified
        );
        assert!(verified.source_retained);

        let authorized = authorize_migration_source_retirement(
            &root,
            &migration,
            "local-admin:1000",
            "2026-07-15T12:05:00Z",
        )
        .expect("retirement authorizes");
        assert!(authorized.source_retained);

        let completed = complete_migration_source_retirement(
            &root,
            &checkpoint,
            &mut migration,
            "local-admin:1000",
            "2026-07-15T12:05:00Z",
        )
        .expect("retirement records");
        assert_eq!(migration.state, MigrationState::Completed);
        assert!(!completed.source_retained);
        assert_eq!(
            StoreMigration::load(&checkpoint).expect("checkpoint reloads"),
            migration
        );
        assert_eq!(
            read_migration_provenance(&root, "migration-1")
                .expect("provenance reads")
                .expect("provenance exists"),
            completed
        );
        let _ = fs::remove_dir_all(root.parent().expect("fixture parent"));
    }

    #[test]
    fn exact_prepare_is_idempotent_but_manifest_drift_conflicts() {
        let (root, _) = fixture("conflict");
        let (migration, source, destination) = retirement_pending();
        let first = prepare_migration_provenance(&root, &migration, &source, &destination)
            .expect("first prepare");
        assert_eq!(
            prepare_migration_provenance(&root, &migration, &source, &destination)
                .expect("retry prepare"),
            first
        );
        let changed = manifest("destination", "different-root");
        let verified = record_migration_destination_verified(
            &root,
            &migration,
            &source,
            &destination,
            "2026-07-15T12:00:00Z",
        )
        .expect("verification records");
        assert_eq!(
            prepare_migration_provenance(&root, &migration, &source, &destination)
                .expect("later lifecycle prepare retry"),
            verified
        );
        assert!(matches!(
            record_migration_destination_verified(
                &root,
                &migration,
                &source,
                &changed,
                "2026-07-15T12:00:00Z"
            ),
            Err(MigrationProvenanceError::Conflict(_))
        ));
        let _ = fs::remove_dir_all(root.parent().expect("fixture parent"));
    }

    #[test]
    fn retirement_requires_durable_verification() {
        let (root, checkpoint) = fixture("unverified");
        let (mut migration, source, destination) = retirement_pending();
        prepare_migration_provenance(&root, &migration, &source, &destination)
            .expect("provenance prepares");
        assert!(matches!(
            complete_migration_source_retirement(
                &root,
                &checkpoint,
                &mut migration,
                "local-admin:1000",
                "2026-07-15T12:05:00Z"
            ),
            Err(MigrationProvenanceError::InvalidState(_))
        ));
        assert_eq!(migration.state, MigrationState::RetirementPending);
        let _ = fs::remove_dir_all(root.parent().expect("fixture parent"));
    }

    #[test]
    fn restart_reconciliation_finishes_sidecar_after_checkpoint_publication() {
        let (root, checkpoint) = fixture("restart");
        let (mut migration, source, destination) = retirement_pending();
        prepare_migration_provenance(&root, &migration, &source, &destination)
            .expect("provenance prepares");
        record_migration_destination_verified(
            &root,
            &migration,
            &source,
            &destination,
            "2026-07-15T12:00:00Z",
        )
        .expect("verification records");
        let path = provenance_path(&root, &migration.migration_id).expect("path");
        let mut interrupted = read_record(&path).expect("record");
        interrupted.retirement_actor = Some("local-admin:1000".to_string());
        interrupted.retired_at_utc = Some("2026-07-15T12:05:00Z".to_string());
        persist_record(&path, &interrupted).expect("authorization persists");
        migration
            .confirm_source_retirement()
            .expect("checkpoint transition");
        migration
            .save_atomic(&checkpoint)
            .expect("checkpoint saves");

        let reconciled = reconcile_migration_provenance(&root, &checkpoint, &mut migration)
            .expect("restart reconciles");
        assert!(!reconciled.source_retained);
        let _ = fs::remove_dir_all(root.parent().expect("fixture parent"));
    }

    #[test]
    fn strict_reader_rejects_future_schema() {
        let (root, _) = fixture("future");
        let (migration, source, destination) = retirement_pending();
        prepare_migration_provenance(&root, &migration, &source, &destination)
            .expect("provenance prepares");
        let path = provenance_path(&root, &migration.migration_id).expect("path");
        let content = fs::read_to_string(&path).expect("record reads");
        fs::write(
            &path,
            content.replace(
                MIGRATION_PROVENANCE_SCHEMA,
                "dasobjectstore.migration_provenance.v2",
            ),
        )
        .expect("future schema writes");
        assert!(matches!(
            read_migration_provenance(&root, &migration.migration_id),
            Err(MigrationProvenanceError::UnsupportedSchema(_))
        ));
        let _ = fs::remove_dir_all(root.parent().expect("fixture parent"));
    }
}
