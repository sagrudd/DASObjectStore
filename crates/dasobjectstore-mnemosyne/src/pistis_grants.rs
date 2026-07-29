//! Deployment-owned Pistis principal-to-ObjectStore grant resolution.

use dasobjectstore_core::store::ExportPolicy;
use dasobjectstore_daemon::{
    api::remote_easyconnect_control_operations, RemoteEasyconnectApprovalContext,
    RemoteEasyconnectAuthProvider, RemoteEasyconnectObjectStoreGrant,
};
use dasobjectstore_gui_api::{
    AuthenticatedGuiActor, PistisApprovalResolutionError, PistisEasyconnectApprovalResolver,
    VerifiedHostAuthenticatedContext,
};
use dasobjectstore_object_service::{bucket_name_for_definition, read_store_registry};
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

/// Atomic file projection owned by the DAS deployment.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PistisObjectStoreGrantRegistry {
    pub schema_version: String,
    pub revision: u64,
    pub records: Vec<PistisObjectStoreGrantRecord>,
    #[serde(default)]
    pub audit_events: Vec<PistisGrantAuditEvent>,
}

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

/// Permission-restricted, compare-and-swap policy writer used by the supported
/// administrator CLI.
#[derive(Clone, Debug)]
pub struct PistisGrantPolicyStore {
    path: PathBuf,
}

impl PistisGrantPolicyStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

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

/// Request-bound resolver that never accepts identity or grant facts from the browser.
#[derive(Clone, Debug)]
pub struct FilePistisEasyconnectApprovalResolver {
    authority_id: Uuid,
    principal_id: Uuid,
    session_id: Uuid,
    grant_registry_path: PathBuf,
    store_registry_path: PathBuf,
}

impl FilePistisEasyconnectApprovalResolver {
    pub fn new(
        authority_id: Uuid,
        principal_id: Uuid,
        session_id: Uuid,
        grant_registry_path: impl Into<PathBuf>,
        store_registry_path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            authority_id,
            principal_id,
            session_id,
            grant_registry_path: grant_registry_path.into(),
            store_registry_path: store_registry_path.into(),
        }
    }

    fn resolve_exact_grant(
        &self,
        requested_object_store: &str,
    ) -> Result<
        (
            PistisObjectStoreGrantRecord,
            RemoteEasyconnectObjectStoreGrant,
        ),
        PistisGrantResolutionError,
    > {
        if requested_object_store.trim().is_empty() {
            return Err(PistisGrantResolutionError::UnknownObjectStore);
        }
        let registry = PistisGrantPolicyStore::new(&self.grant_registry_path)
            .inspect()?
            .ok_or_else(|| {
                PistisGrantPolicyError::RegistryUnavailable(
                    "grant registry does not exist".to_owned(),
                )
            })?;
        let matches = registry
            .records
            .iter()
            .filter(|record| {
                record.active
                    && record.authority_id == self.authority_id
                    && record.principal_id == self.principal_id
                    && record.object_store_id == requested_object_store
            })
            .cloned()
            .collect::<Vec<_>>();
        let [record] = matches.as_slice() else {
            return if matches.is_empty() {
                Err(PistisGrantResolutionError::GrantNotFound)
            } else {
                Err(PistisGrantResolutionError::AmbiguousGrant)
            };
        };
        if !record.can_write {
            return Err(PistisGrantResolutionError::WriteNotGranted);
        }
        let definitions = read_store_registry(&self.store_registry_path)
            .map_err(|error| PistisGrantResolutionError::StoreRegistry(error.to_string()))?;
        let stores = definitions
            .iter()
            .filter(|definition| definition.store_id.as_str() == requested_object_store)
            .collect::<Vec<_>>();
        let [definition] = stores.as_slice() else {
            return Err(PistisGrantResolutionError::UnknownObjectStore);
        };
        if definition.policy.export_policy != ExportPolicy::S3 {
            return Err(PistisGrantResolutionError::StoreNotS3Exported);
        }
        let bucket = bucket_name_for_definition(definition)
            .map_err(|error| PistisGrantResolutionError::StoreRegistry(error.to_string()))?;
        let grant = RemoteEasyconnectObjectStoreGrant {
            object_store: requested_object_store.to_owned(),
            bucket,
            can_read: record.can_read,
            can_write: record.can_write,
            writer_group: definition.writer_group.clone(),
            object_type: definition.policy.class.name().to_owned(),
            control_operations: remote_easyconnect_control_operations(record.can_write),
            allowed_prefixes: record.allowed_prefixes.clone(),
        };
        grant
            .validate()
            .map_err(|error| PistisGrantResolutionError::InvalidGrant(error.to_string()))?;
        Ok((record.clone(), grant))
    }
}

impl PistisEasyconnectApprovalResolver for FilePistisEasyconnectApprovalResolver {
    fn resolve(
        &self,
        actor: &AuthenticatedGuiActor,
        verified: &VerifiedHostAuthenticatedContext,
        requested_object_store: &str,
    ) -> Result<RemoteEasyconnectApprovalContext, PistisApprovalResolutionError> {
        let context = verified.context();
        if actor.subject_id != self.principal_id.to_string()
            || context.subject_id != actor.subject_id
            || context.session_id != self.session_id.to_string()
            || !actor
                .roles
                .iter()
                .any(|role| matches!(role.as_str(), "storage_operator" | "storage_administrator"))
        {
            return Err(PistisApprovalResolutionError::new(
                "verified actor is not bound to the configured Pistis grant subject",
            ));
        }
        let (record, grant) = self
            .resolve_exact_grant(requested_object_store)
            .map_err(|error| PistisApprovalResolutionError::new(error.to_string()))?;
        Ok(RemoteEasyconnectApprovalContext {
            authority_id: self.authority_id.to_string(),
            principal_id: self.principal_id.to_string(),
            session_id: self.session_id.to_string(),
            auth_provider: RemoteEasyconnectAuthProvider::Pistis,
            allowed_object_stores: vec![grant],
            host_session_expires_at_utc: dasobjectstore_core::utc::format_utc_timestamp_seconds(
                context.expires_at_unix_seconds,
            ),
            correlation_id: context.correlation_id.clone(),
            audit_identity: format!(
                "pistis-grant:{}:{}",
                record.policy_revision, record.record_id
            ),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum PistisGrantResolutionError {
    RegistryUnavailable(String),
    InvalidRegistry(String),
    StoreRegistry(String),
    GrantNotFound,
    AmbiguousGrant,
    WriteNotGranted,
    UnknownObjectStore,
    StoreNotS3Exported,
    InvalidGrant(String),
}

impl From<PistisGrantPolicyError> for PistisGrantResolutionError {
    fn from(error: PistisGrantPolicyError) -> Self {
        match error {
            PistisGrantPolicyError::RegistryUnavailable(message) => {
                Self::RegistryUnavailable(message)
            }
            other => Self::InvalidRegistry(other.to_string()),
        }
    }
}

impl std::fmt::Display for PistisGrantResolutionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RegistryUnavailable(_) => {
                formatter.write_str("Pistis grant registry unavailable")
            }
            Self::InvalidRegistry(_) => formatter.write_str("Pistis grant registry is invalid"),
            Self::StoreRegistry(_) => formatter.write_str("ObjectStore registry is invalid"),
            Self::GrantNotFound => formatter.write_str("exact Pistis ObjectStore grant not found"),
            Self::AmbiguousGrant => formatter.write_str("Pistis ObjectStore grant is ambiguous"),
            Self::WriteNotGranted => formatter.write_str("Pistis ObjectStore grant is read-only"),
            Self::UnknownObjectStore => formatter.write_str("requested ObjectStore is unknown"),
            Self::StoreNotS3Exported => {
                formatter.write_str("requested ObjectStore is not S3 exported")
            }
            Self::InvalidGrant(_) => formatter.write_str("derived ObjectStore grant is invalid"),
        }
    }
}

impl std::error::Error for PistisGrantResolutionError {}

#[cfg(test)]
mod tests {
    use super::*;
    use dasobjectstore_core::{
        ids::StoreId,
        store::{StoreClass, StorePolicy},
    };
    use dasobjectstore_gui_api::{
        accept_host_authenticated_context, AuthenticatedActorAuthority, HostAuthenticatedContext,
        HostAuthenticationAuthority, HostAuthenticationContextVerifier, HOST_AUTH_AUDIENCE,
        HOST_AUTH_CONTEXT_SCHEMA_VERSION,
    };
    use dasobjectstore_object_service::StoreServiceDefinition;
    use std::fs;

    const NOW: i64 = 1_750_000_000;

    struct LiveVerifier;

    impl HostAuthenticationContextVerifier for LiveVerifier {
        fn verify_live_session(&self, _: &HostAuthenticatedContext) -> Result<(), String> {
            Ok(())
        }
    }

    fn fixture(
        records: Vec<PistisObjectStoreGrantRecord>,
    ) -> (
        FilePistisEasyconnectApprovalResolver,
        AuthenticatedGuiActor,
        VerifiedHostAuthenticatedContext,
        PathBuf,
    ) {
        let authority_id = Uuid::from_u128(1);
        let principal_id = Uuid::from_u128(2);
        let session_id = Uuid::from_u128(3);
        let root = std::env::temp_dir().join(format!("das-pistis-grants-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).expect("temporary directory");
        let grants_path = root.join("pistis-grants.json");
        let stores_path = root.join("stores.json");
        fs::write(
            &grants_path,
            serde_json::to_vec_pretty(&PistisObjectStoreGrantRegistry {
                schema_version: PISTIS_GRANT_REGISTRY_SCHEMA_VERSION.to_owned(),
                revision: 7,
                records,
                audit_events: Vec::new(),
            })
            .expect("serialize grants"),
        )
        .expect("write grants");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&grants_path, fs::Permissions::from_mode(0o600))
                .expect("private grant permissions");
        }
        fs::write(
            &stores_path,
            serde_json::to_vec_pretty(&vec![StoreServiceDefinition {
                store_id: StoreId::new("epic_collection").expect("store id"),
                policy: StorePolicy::defaults_for(StoreClass::GeneratedData),
                bucket_name: Some("dos-epic-collection".to_owned()),
                reader_group: None,
                writer_group: Some("mnemosyne".to_owned()),
                public: false,
            }])
            .expect("serialize stores"),
        )
        .expect("write stores");
        let host = HostAuthenticatedContext {
            schema_version: HOST_AUTH_CONTEXT_SCHEMA_VERSION.to_owned(),
            authority: HostAuthenticationAuthority::MonasStandalone,
            issuer: HostAuthenticationAuthority::MonasStandalone
                .issuer()
                .to_owned(),
            audience: HOST_AUTH_AUDIENCE.to_owned(),
            subject_id: principal_id.to_string(),
            session_id: session_id.to_string(),
            roles: vec!["authenticated".to_owned(), "storage_operator".to_owned()],
            issued_at_unix_seconds: NOW - 60,
            expires_at_unix_seconds: NOW + 600,
            correlation_id: "test:correlation".to_owned(),
            csrf_binding_sha256: format!("sha256:{}", "a".repeat(64)),
        };
        let verified =
            accept_host_authenticated_context(host, NOW, &LiveVerifier).expect("verified context");
        let actor = AuthenticatedGuiActor {
            subject_id: principal_id.to_string(),
            authority: AuthenticatedActorAuthority::MonasStandalone,
            roles: vec!["authenticated".to_owned(), "storage_operator".to_owned()],
            expires_at_unix_seconds: Some(NOW + 600),
            correlation_id: Some("test:correlation".to_owned()),
        };
        (
            FilePistisEasyconnectApprovalResolver::new(
                authority_id,
                principal_id,
                session_id,
                grants_path,
                stores_path,
            ),
            actor,
            verified,
            root,
        )
    }

    fn grant() -> PistisObjectStoreGrantRecord {
        PistisObjectStoreGrantRecord {
            record_id: Uuid::from_u128(4),
            authority_id: Uuid::from_u128(1),
            principal_id: Uuid::from_u128(2),
            object_store_id: "epic_collection".to_owned(),
            policy_revision: 7,
            active: true,
            can_read: true,
            can_write: true,
            allowed_prefixes: vec!["".to_owned()],
        }
    }

    #[test]
    fn resolves_one_exact_write_grant_from_deployment_registries() {
        let (resolver, actor, verified, root) = fixture(vec![grant()]);
        let context = resolver
            .resolve(&actor, &verified, "epic_collection")
            .expect("exact grant");
        assert_eq!(context.authority_id, Uuid::from_u128(1).to_string());
        assert_eq!(context.principal_id, Uuid::from_u128(2).to_string());
        assert_eq!(context.allowed_object_stores.len(), 1);
        assert_eq!(
            context.allowed_object_stores[0].bucket,
            "dos-epic-collection"
        );
        assert!(context.allowed_object_stores[0].can_write);
        assert!(context.audit_identity.starts_with("pistis-grant:7:"));
        fs::remove_dir_all(root).expect("remove temporary directory");
    }

    #[test]
    fn rejects_missing_read_only_ambiguous_and_substituted_grants() {
        for records in [
            vec![],
            {
                let mut value = grant();
                value.can_write = false;
                vec![value]
            },
            vec![grant(), grant()],
        ] {
            let (resolver, actor, verified, root) = fixture(records);
            assert!(resolver
                .resolve(&actor, &verified, "epic_collection")
                .is_err());
            fs::remove_dir_all(root).expect("remove temporary directory");
        }
        let (resolver, actor, verified, root) = fixture(vec![grant()]);
        assert!(resolver
            .resolve(&actor, &verified, "different-store")
            .is_err());
        fs::remove_dir_all(root).expect("remove temporary directory");
    }

    #[test]
    fn rejects_stale_registry_and_actor_substitution() {
        let mut stale = grant();
        stale.policy_revision = 6;
        let (resolver, actor, verified, root) = fixture(vec![stale]);
        assert!(resolver
            .resolve(&actor, &verified, "epic_collection")
            .is_err());
        fs::remove_dir_all(root).expect("remove temporary directory");

        let (resolver, mut actor, verified, root) = fixture(vec![grant()]);
        actor.subject_id = Uuid::from_u128(99).to_string();
        assert!(resolver
            .resolve(&actor, &verified, "epic_collection")
            .is_err());
        fs::remove_dir_all(root).expect("remove temporary directory");
    }

    #[test]
    fn policy_store_grants_restarts_and_revokes_exact_tuple() {
        let root = std::env::temp_dir().join(format!("das-pistis-policy-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("pistis-grants.json");
        let store = PistisGrantPolicyStore::new(&path);
        let authority_id = Uuid::from_u128(10);
        let principal_id = Uuid::from_u128(11);

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
        let reopened = PistisGrantPolicyStore::new(&path)
            .inspect()
            .unwrap()
            .unwrap();
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
        let root = std::env::temp_dir().join(format!("das-pistis-policy-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("pistis-grants.json");
        let store = PistisGrantPolicyStore::new(&path);
        let authority_id = Uuid::from_u128(10);
        let principal_id = Uuid::from_u128(11);
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
        let before = fs::read(&path).unwrap();
        assert!(matches!(
            store.revoke(0, authority_id, principal_id, "epic_collection"),
            Err(PistisGrantPolicyError::RevisionConflict { .. })
        ));
        let _lock = PolicyLock::acquire(&path.with_extension("json.lock")).unwrap();
        assert!(matches!(
            store.revoke(1, authority_id, principal_id, "epic_collection"),
            Err(PistisGrantPolicyError::ConcurrentMutation)
        ));
        assert_eq!(fs::read(&path).unwrap(), before);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn policy_upsert_cannot_create_an_ambiguous_active_tuple() {
        let root = std::env::temp_dir().join(format!("das-pistis-policy-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("pistis-grants.json");
        let store = PistisGrantPolicyStore::new(&path);
        for expected_revision in 0..2 {
            store
                .grant(
                    expected_revision,
                    Uuid::from_u128(10),
                    Uuid::from_u128(11),
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
