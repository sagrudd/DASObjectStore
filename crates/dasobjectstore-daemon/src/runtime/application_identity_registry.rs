//! Daemon-owned persistence for application service principals.
//!
//! The registry stores validated public identity metadata and policy only. It
//! never stores private keys, bearer tokens, host paths, or provider secrets;
//! those belong in a daemon-owned credential/key store in a later exchange
//! slice.

use super::DaemonServiceRuntimeError;
use dasobjectstore_core::application_auth::ApplicationIdentity;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub const APPLICATION_IDENTITY_REGISTRY_SCHEMA: &str =
    "dasobjectstore.application_identity_registry.v1";
pub const APPLICATION_IDENTITY_REGISTRY_FILE_NAME: &str = "application-identities.json";
pub const APPLICATION_IDENTITY_REGISTRY_ENV: &str = "DASOBJECTSTORE_APPLICATION_IDENTITIES_PATH";

static APPLICATION_IDENTITY_REGISTRY_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn registry_lock() -> &'static Mutex<()> {
    APPLICATION_IDENTITY_REGISTRY_LOCK.get_or_init(|| Mutex::new(()))
}

pub fn default_application_identity_registry_path(state_dir: impl AsRef<Path>) -> PathBuf {
    state_dir
        .as_ref()
        .join(APPLICATION_IDENTITY_REGISTRY_FILE_NAME)
}

pub fn application_identity_registry_path(state_dir: impl AsRef<Path>) -> PathBuf {
    std::env::var_os(APPLICATION_IDENTITY_REGISTRY_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| default_application_identity_registry_path(state_dir))
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ApplicationIdentityRegistryFile {
    schema_version: String,
    identities: Vec<ApplicationIdentity>,
}

impl Default for ApplicationIdentityRegistryFile {
    fn default() -> Self {
        Self {
            schema_version: APPLICATION_IDENTITY_REGISTRY_SCHEMA.to_string(),
            identities: Vec::new(),
        }
    }
}

/// Read all registered service principals in deterministic application-id
/// order. Returned identities contain policy metadata only.
pub fn list_application_identities(
    path: impl AsRef<Path>,
) -> Result<Vec<ApplicationIdentity>, DaemonServiceRuntimeError> {
    let _guard = registry_lock()
        .lock()
        .expect("application identity registry lock poisoned");
    Ok(read_registry(path.as_ref())?.identities)
}

pub fn read_application_identity(
    path: impl AsRef<Path>,
    application_id: &str,
) -> Result<Option<ApplicationIdentity>, DaemonServiceRuntimeError> {
    let _guard = registry_lock()
        .lock()
        .expect("application identity registry lock poisoned");
    Ok(read_registry(path.as_ref())?
        .identities
        .into_iter()
        .find(|identity| identity.application_id == application_id))
}

/// Add or replace a daemon-owned service principal after validating its public
/// policy. Replacing an identity is the rotation boundary; key material and
/// token invalidation are deliberately handled by the exchange layer.
pub fn upsert_application_identity(
    path: impl AsRef<Path>,
    identity: ApplicationIdentity,
) -> Result<(), DaemonServiceRuntimeError> {
    let _guard = registry_lock()
        .lock()
        .expect("application identity registry lock poisoned");
    identity
        .validate()
        .map_err(|error| invalid_identity(error.to_string()))?;
    let path = path.as_ref();
    let mut registry = read_registry(path)?;
    if let Some(existing) = registry
        .identities
        .iter_mut()
        .find(|existing| existing.application_id == identity.application_id)
    {
        *existing = identity;
    } else {
        registry.identities.push(identity);
    }
    registry
        .identities
        .sort_by(|left, right| left.application_id.cmp(&right.application_id));
    write_registry(path, &registry)
}

/// Mark a service principal inactive without deleting its audit-relevant
/// identity metadata. Access-token validation rejects inactive identities.
pub fn deactivate_application_identity(
    path: impl AsRef<Path>,
    application_id: &str,
) -> Result<bool, DaemonServiceRuntimeError> {
    let _guard = registry_lock()
        .lock()
        .expect("application identity registry lock poisoned");
    let path = path.as_ref();
    let mut registry = read_registry(path)?;
    let Some(identity) = registry
        .identities
        .iter_mut()
        .find(|identity| identity.application_id == application_id)
    else {
        return Ok(false);
    };
    identity.active = false;
    write_registry(path, &registry)?;
    Ok(true)
}

fn read_registry(
    path: &Path,
) -> Result<ApplicationIdentityRegistryFile, DaemonServiceRuntimeError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(ApplicationIdentityRegistryFile::default())
        }
        Err(error) => return Err(registry_io(path, error)),
    };
    let registry: ApplicationIdentityRegistryFile =
        serde_json::from_reader(file).map_err(|error| {
            invalid_identity(format!(
                "invalid application identity registry {}: {error}",
                path.display()
            ))
        })?;
    if registry.schema_version != APPLICATION_IDENTITY_REGISTRY_SCHEMA {
        return Err(invalid_identity(format!(
            "unsupported application identity registry schema {}",
            registry.schema_version
        )));
    }
    let mut application_ids = BTreeSet::new();
    for identity in &registry.identities {
        identity
            .validate()
            .map_err(|error| invalid_identity(error.to_string()))?;
        if !application_ids.insert(identity.application_id.as_str()) {
            return Err(invalid_identity(format!(
                "duplicate application identity {}",
                identity.application_id
            )));
        }
    }
    Ok(registry)
}

fn write_registry(
    path: &Path,
    registry: &ApplicationIdentityRegistryFile,
) -> Result<(), DaemonServiceRuntimeError> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid_identity("application identity registry has no parent"))?;
    fs::create_dir_all(parent).map_err(|error| registry_io(parent, error))?;
    let temporary = parent.join(format!(
        ".{}.tmp-{}-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("application-identities"),
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    let bytes = serde_json::to_vec_pretty(registry).map_err(|error| {
        invalid_identity(format!("serialize application identity registry: {error}"))
    })?;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| registry_io(&temporary, error))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| registry_io(&temporary, error))?;
    drop(file);
    fs::rename(&temporary, path).map_err(|error| registry_io(path, error))?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| registry_io(parent, error))
}

fn invalid_identity(message: impl Into<String>) -> DaemonServiceRuntimeError {
    DaemonServiceRuntimeError::UnsupportedOperation {
        operation: format!("invalid application identity registry: {}", message.into()),
    }
}

fn registry_io(path: &Path, error: io::Error) -> DaemonServiceRuntimeError {
    DaemonServiceRuntimeError::UnsupportedOperation {
        operation: format!(
            "application identity registry I/O {}: {error}",
            path.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        deactivate_application_identity, default_application_identity_registry_path,
        list_application_identities, read_application_identity, upsert_application_identity,
        APPLICATION_IDENTITY_REGISTRY_SCHEMA,
    };
    use dasobjectstore_core::application_auth::{
        ApplicationCredentialKind, ApplicationEnvironment, ApplicationIdentity,
        ApplicationOperation, ApplicationScope, APPLICATION_AUTH_SCHEMA_VERSION,
    };
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_core::ingress::IngressOrigin;
    use dasobjectstore_core::object_type::ObjectType;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn root(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let root = std::env::var_os("DASOBJECTSTORE_CODEX_VALIDATION_ROOT")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|home| PathBuf::from(home).join(".dasobjectstore-codex-validation"))
            })
            .unwrap_or_else(std::env::temp_dir)
            .join(format!(
                "application-identities-{label}-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(&root).expect("fixture root");
        root
    }

    fn identity(application_id: &str) -> ApplicationIdentity {
        ApplicationIdentity {
            schema_version: APPLICATION_AUTH_SCHEMA_VERSION.to_string(),
            application_id: application_id.to_string(),
            owner: "mnemosyne".to_string(),
            purpose: "sequencing ingest".to_string(),
            environment: ApplicationEnvironment::Production,
            credential_kind: ApplicationCredentialKind::AsymmetricKey,
            scope: ApplicationScope {
                store_ids: vec![StoreId::new("codex").expect("store")],
                prefixes: vec!["analysis".to_string()],
                object_types: vec![ObjectType::Fastq],
                operations: vec![ApplicationOperation::Write],
                ingress_origin: IngressOrigin::Synoptikon,
                max_object_bytes: Some(10_000),
                max_total_bytes: Some(100_000),
            },
            issued_at_unix_seconds: 1_000,
            expires_at_unix_seconds: 100_000,
            active: true,
        }
    }

    #[test]
    fn default_registry_path_is_state_scoped() {
        assert_eq!(
            default_application_identity_registry_path("/var/lib/dasobjectstore"),
            PathBuf::from("/var/lib/dasobjectstore/application-identities.json")
        );
    }

    #[test]
    fn upsert_and_read_store_policy_only_without_secrets_or_paths() {
        let path = root("round-trip").join("identities.json");
        let principal = identity("synoptikon-ingest");
        upsert_application_identity(&path, principal.clone()).expect("write");
        assert_eq!(
            read_application_identity(&path, "synoptikon-ingest").expect("read"),
            Some(principal)
        );
        let encoded = fs::read_to_string(&path).expect("registry bytes");
        assert!(encoded.contains(APPLICATION_IDENTITY_REGISTRY_SCHEMA));
        assert!(!encoded.contains("private_key"));
        assert!(!encoded.contains("/srv"));
    }

    #[test]
    fn identities_are_sorted_and_deactivation_is_non_destructive() {
        let path = root("deactivate").join("identities.json");
        upsert_application_identity(&path, identity("zeta")).expect("write zeta");
        upsert_application_identity(&path, identity("alpha")).expect("write alpha");
        assert_eq!(
            list_application_identities(&path)
                .expect("list")
                .into_iter()
                .map(|identity| identity.application_id)
                .collect::<Vec<_>>(),
            vec!["alpha", "zeta"]
        );
        assert!(deactivate_application_identity(&path, "alpha").expect("deactivate"));
        assert!(!deactivate_application_identity(&path, "missing").expect("missing"));
        let alpha = read_application_identity(&path, "alpha")
            .expect("read")
            .expect("alpha");
        assert!(!alpha.active);
    }

    #[test]
    fn concurrent_upserts_preserve_all_identities() {
        let path = root("concurrent-upserts").join("identities.json");
        let left_path = path.clone();
        let left = std::thread::spawn(move || {
            upsert_application_identity(&left_path, identity("left")).expect("write left")
        });
        let right_path = path.clone();
        let right = std::thread::spawn(move || {
            upsert_application_identity(&right_path, identity("right")).expect("write right")
        });
        left.join().expect("left joins");
        right.join().expect("right joins");

        let ids = list_application_identities(&path)
            .expect("list identities")
            .into_iter()
            .map(|identity| identity.application_id)
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["left", "right"]);
    }

    #[test]
    fn invalid_identity_is_rejected_before_registry_write() {
        let path = root("invalid").join("identities.json");
        let mut principal = identity("invalid");
        principal.scope.prefixes = vec!["/host/path".to_string()];
        assert!(upsert_application_identity(&path, principal).is_err());
        assert!(!path.exists());
    }
}
