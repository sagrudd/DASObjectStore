//! Daemon-owned Pistis principal-to-ObjectStore grant policy persistence.

use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Version of the deployment-owned federated grant registry.
pub const PISTIS_GRANT_REGISTRY_SCHEMA_VERSION: &str = "dasobjectstore.pistis-grant-registry.v1";

/// One immutable-authority grant for one exact ObjectStore.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PistisObjectStoreGrantRecord {
    pub record_id: Uuid,
    pub authority_id: Uuid,
    pub principal_id: Uuid,
    pub object_store_id: String,
    pub policy_revision: u64,
    pub active: bool,
    pub can_read: bool,
    pub can_write: bool,
    #[serde(default)]
    pub allowed_prefixes: Vec<String>,
}

/// Atomic file projection owned by the DAS daemon.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PistisObjectStoreGrantRegistry {
    pub schema_version: String,
    pub revision: u64,
    pub records: Vec<PistisObjectStoreGrantRecord>,
    #[serde(default)]
    pub audit_events: Vec<PistisGrantAuditEvent>,
}

/// One compatibility-preserving v1 policy event.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PistisGrantAuditEvent {
    pub event_id: Uuid,
    pub policy_revision: u64,
    pub operation: PistisGrantAuditOperation,
    pub authority_id: Uuid,
    pub principal_id: Uuid,
    pub object_store_id: String,
    pub record_id: Uuid,
}

/// Supported v1 policy mutations.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PistisGrantAuditOperation {
    Grant,
    Revoke,
}

impl PistisObjectStoreGrantRegistry {
    /// Read and structurally validate a registry.
    pub fn read(path: impl AsRef<Path>) -> Result<Self, PistisGrantPolicyError> {
        let path = path.as_ref();
        let file = File::open(path)
            .map_err(|error| PistisGrantPolicyError::RegistryUnavailable(error.to_string()))?;
        let registry: Self = serde_json::from_reader(file)
            .map_err(|error| PistisGrantPolicyError::InvalidRegistry(error.to_string()))?;
        registry.validate()?;
        Ok(registry)
    }

    /// Validate the closed v1 policy projection.
    pub fn validate(&self) -> Result<(), PistisGrantPolicyError> {
        if self.schema_version != PISTIS_GRANT_REGISTRY_SCHEMA_VERSION || self.revision == 0 {
            return Err(PistisGrantPolicyError::InvalidRegistry(
                "unsupported schema version or zero revision".to_owned(),
            ));
        }
        for record in &self.records {
            if record.object_store_id.trim().is_empty()
                || (!record.can_read && !record.can_write)
                || record.policy_revision != self.revision
            {
                return Err(PistisGrantPolicyError::InvalidRegistry(
                    "record has an invalid identifier, access set, or stale policy revision"
                        .to_owned(),
                ));
            }
        }
        let mut event_ids = std::collections::BTreeSet::new();
        if self.audit_events.iter().any(|event| {
            event.policy_revision == 0
                || event.policy_revision > self.revision
                || event.object_store_id.trim().is_empty()
                || !event_ids.insert(event.event_id)
        }) {
            return Err(PistisGrantPolicyError::InvalidRegistry(
                "grant audit history is invalid".to_owned(),
            ));
        }
        Ok(())
    }
}

/// Permission-restricted, compare-and-swap policy projection.
///
/// Live mutation remains daemon-internal. Callers may use [`Self::inspect`] to
/// resolve policy, while the daemon's reviewed administration boundary will
/// invoke the mutation methods.
#[derive(Clone, Debug)]
pub struct PistisGrantPolicyStore {
    path: PathBuf,
}

impl PistisGrantPolicyStore {
    /// Bind the store to one daemon-configured registry path.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Inspect the current policy without creating it.
    pub fn inspect(
        &self,
    ) -> Result<Option<PistisObjectStoreGrantRegistry>, PistisGrantPolicyError> {
        match fs::symlink_metadata(&self.path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_file() {
                    return Err(PistisGrantPolicyError::UnsafePath);
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if metadata.permissions().mode() & 0o077 != 0 {
                        return Err(PistisGrantPolicyError::UnsafePath);
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(PistisGrantPolicyError::RegistryUnavailable(
                    error.to_string(),
                ));
            }
        }
        match PistisObjectStoreGrantRegistry::read(&self.path) {
            Ok(registry) => Ok(Some(registry)),
            Err(PistisGrantPolicyError::RegistryUnavailable(_)) if !self.path.exists() => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Grant or replace the exact immutable authority/principal/store tuple.
    pub fn grant(
        &self,
        expected_revision: u64,
        authority_id: Uuid,
        principal_id: Uuid,
        object_store_id: String,
        can_read: bool,
        can_write: bool,
        allowed_prefixes: Vec<String>,
    ) -> Result<PistisObjectStoreGrantRegistry, PistisGrantPolicyError> {
        if object_store_id.trim() != object_store_id
            || object_store_id.is_empty()
            || (!can_read && !can_write)
        {
            return Err(PistisGrantPolicyError::InvalidMutation(
                "grant requires an exact ObjectStore ID and at least one access mode".to_owned(),
            ));
        }
        self.mutate(expected_revision, |registry, revision| {
            let matching = registry
                .records
                .iter()
                .enumerate()
                .filter(|(_, record)| {
                    record.authority_id == authority_id
                        && record.principal_id == principal_id
                        && record.object_store_id == object_store_id
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            if matching.len() > 1 {
                return Err(PistisGrantPolicyError::AmbiguousGrant);
            }
            let record = PistisObjectStoreGrantRecord {
                record_id: Uuid::new_v4(),
                authority_id,
                principal_id,
                object_store_id,
                policy_revision: revision,
                active: true,
                can_read,
                can_write,
                allowed_prefixes,
            };
            if let Some(index) = matching.first() {
                registry.records[*index] = record.clone();
            } else {
                registry.records.push(record.clone());
            }
            registry.audit_events.push(PistisGrantAuditEvent {
                event_id: Uuid::new_v4(),
                policy_revision: revision,
                operation: PistisGrantAuditOperation::Grant,
                authority_id,
                principal_id,
                object_store_id: record.object_store_id.clone(),
                record_id: record.record_id,
            });
            Ok(())
        })
    }

    /// Revoke one active exact immutable authority/principal/store tuple.
    pub fn revoke(
        &self,
        expected_revision: u64,
        authority_id: Uuid,
        principal_id: Uuid,
        object_store_id: &str,
    ) -> Result<PistisObjectStoreGrantRegistry, PistisGrantPolicyError> {
        self.mutate(expected_revision, |registry, revision| {
            let matching = registry
                .records
                .iter()
                .enumerate()
                .filter(|record| {
                    record.1.authority_id == authority_id
                        && record.1.principal_id == principal_id
                        && record.1.object_store_id == object_store_id
                        && record.1.active
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>();
            let [index] = matching.as_slice() else {
                return if matching.is_empty() {
                    Err(PistisGrantPolicyError::GrantNotFound)
                } else {
                    Err(PistisGrantPolicyError::AmbiguousGrant)
                };
            };
            registry.records[*index].active = false;
            let record_id = registry.records[*index].record_id;
            registry.audit_events.push(PistisGrantAuditEvent {
                event_id: Uuid::new_v4(),
                policy_revision: revision,
                operation: PistisGrantAuditOperation::Revoke,
                authority_id,
                principal_id,
                object_store_id: object_store_id.to_owned(),
                record_id,
            });
            Ok(())
        })
    }

    fn mutate(
        &self,
        expected_revision: u64,
        update: impl FnOnce(
            &mut PistisObjectStoreGrantRegistry,
            u64,
        ) -> Result<(), PistisGrantPolicyError>,
    ) -> Result<PistisObjectStoreGrantRegistry, PistisGrantPolicyError> {
        let parent = self.path.parent().ok_or_else(|| {
            PistisGrantPolicyError::InvalidMutation(
                "grant registry must have a parent directory".to_owned(),
            )
        })?;
        let metadata = fs::symlink_metadata(parent)
            .map_err(|error| PistisGrantPolicyError::RegistryUnavailable(error.to_string()))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(PistisGrantPolicyError::UnsafePath);
        }
        let _lock = PolicyLock::acquire(&self.path.with_extension("json.lock"))?;
        let mut registry = match self.inspect()? {
            Some(registry) => registry,
            None if expected_revision == 0 => PistisObjectStoreGrantRegistry {
                schema_version: PISTIS_GRANT_REGISTRY_SCHEMA_VERSION.to_owned(),
                revision: 0,
                records: Vec::new(),
                audit_events: Vec::new(),
            },
            None => {
                return Err(PistisGrantPolicyError::RevisionConflict {
                    expected: expected_revision,
                    current: 0,
                })
            }
        };
        if registry.revision != expected_revision {
            return Err(PistisGrantPolicyError::RevisionConflict {
                expected: expected_revision,
                current: registry.revision,
            });
        }
        let revision = expected_revision
            .checked_add(1)
            .ok_or(PistisGrantPolicyError::RevisionOverflow)?;
        update(&mut registry, revision)?;
        registry.revision = revision;
        for record in &mut registry.records {
            record.policy_revision = revision;
        }
        registry.validate()?;
        atomic_write_private(
            &self.path,
            &serde_json::to_vec_pretty(&registry)
                .map_err(|error| PistisGrantPolicyError::InvalidRegistry(error.to_string()))?,
        )?;
        Ok(registry)
    }
}

struct PolicyLock {
    path: PathBuf,
}

impl PolicyLock {
    fn acquire(path: &Path) -> Result<Self, PistisGrantPolicyError> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                PistisGrantPolicyError::ConcurrentMutation
            } else {
                PistisGrantPolicyError::RegistryUnavailable(error.to_string())
            }
        })?;
        Ok(Self {
            path: path.to_path_buf(),
        })
    }
}

impl Drop for PolicyLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn atomic_write_private(path: &Path, bytes: &[u8]) -> Result<(), PistisGrantPolicyError> {
    let temporary = path.with_extension(format!("json.{}.tmp", Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|error| PistisGrantPolicyError::RegistryUnavailable(error.to_string()))?;
    let result = (|| {
        file.write_all(bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        File::open(path.parent().expect("validated parent"))?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|error| PistisGrantPolicyError::RegistryUnavailable(error.to_string()))
}

/// A closed policy persistence or validation failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PistisGrantPolicyError {
    RegistryUnavailable(String),
    InvalidRegistry(String),
    InvalidMutation(String),
    RevisionConflict { expected: u64, current: u64 },
    RevisionOverflow,
    ConcurrentMutation,
    AmbiguousGrant,
    GrantNotFound,
    UnsafePath,
}

impl std::fmt::Display for PistisGrantPolicyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RegistryUnavailable(_) => {
                formatter.write_str("Pistis grant registry unavailable")
            }
            Self::InvalidRegistry(_) => formatter.write_str("Pistis grant registry is invalid"),
            Self::InvalidMutation(message) => {
                write!(formatter, "invalid Pistis grant mutation: {message}")
            }
            Self::RevisionConflict { expected, current } => {
                write!(
                    formatter,
                    "Pistis grant revision conflict: expected {expected}, current {current}"
                )
            }
            Self::RevisionOverflow => formatter.write_str("Pistis grant revision overflow"),
            Self::ConcurrentMutation => {
                formatter.write_str("another Pistis grant mutation is active")
            }
            Self::AmbiguousGrant => formatter.write_str("Pistis ObjectStore grant is ambiguous"),
            Self::GrantNotFound => formatter.write_str("exact Pistis ObjectStore grant not found"),
            Self::UnsafePath => formatter.write_str("Pistis grant registry path is unsafe"),
        }
    }
}

impl std::error::Error for PistisGrantPolicyError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (PathBuf, PistisGrantPolicyStore, Uuid, Uuid) {
        let root = std::env::temp_dir().join(format!("das-pistis-policy-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("pistis-grants.json");
        let store = PistisGrantPolicyStore::new(&path);
        (root, store, Uuid::from_u128(10), Uuid::from_u128(11))
    }

    #[test]
    fn grants_restarts_and_revokes_exact_tuple() {
        let (root, store, authority_id, principal_id) = fixture();
        let granted = store
            .grant(
                0,
                authority_id,
                principal_id,
                "epic_collection".to_owned(),
                true,
                true,
                vec!["project/".to_owned()],
            )
            .unwrap();
        assert_eq!(granted.revision, 1);
        assert!(granted.records[0].active);
        let reopened = store.inspect().unwrap().unwrap();
        assert_eq!(reopened, granted);

        let revoked = store
            .revoke(1, authority_id, principal_id, "epic_collection")
            .unwrap();
        assert_eq!(revoked.revision, 2);
        assert!(!revoked.records[0].active);
        assert_eq!(revoked.records[0].policy_revision, 2);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stale_revision_and_active_writer_leave_prior_policy_unchanged() {
        let (root, store, authority_id, principal_id) = fixture();
        store
            .grant(
                0,
                authority_id,
                principal_id,
                "epic_collection".to_owned(),
                true,
                true,
                Vec::new(),
            )
            .unwrap();
        let before = fs::read(&store.path).unwrap();
        assert!(matches!(
            store.revoke(0, authority_id, principal_id, "epic_collection"),
            Err(PistisGrantPolicyError::RevisionConflict { .. })
        ));
        let _lock = PolicyLock::acquire(&store.path.with_extension("json.lock")).unwrap();
        assert!(matches!(
            store.revoke(1, authority_id, principal_id, "epic_collection"),
            Err(PistisGrantPolicyError::ConcurrentMutation)
        ));
        assert_eq!(fs::read(&store.path).unwrap(), before);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn upsert_cannot_create_an_ambiguous_active_tuple() {
        let (root, store, authority_id, principal_id) = fixture();
        for expected_revision in 0..2 {
            store
                .grant(
                    expected_revision,
                    authority_id,
                    principal_id,
                    "epic_collection".to_owned(),
                    true,
                    true,
                    Vec::new(),
                )
                .unwrap();
        }
        let registry = store.inspect().unwrap().unwrap();
        assert_eq!(registry.records.len(), 1);
        assert_eq!(registry.revision, 2);
        fs::remove_dir_all(root).unwrap();
    }
}
