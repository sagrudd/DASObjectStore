//! Daemon-owned public-key/certificate descriptors for application identity
//! rotation. Private keys and bearer tokens are intentionally out of this
//! registry.

use super::DaemonServiceRuntimeError;
use dasobjectstore_core::application_auth::ApplicationKeyDescriptor;
use dasobjectstore_core::application_auth::{
    ApplicationCredentialKind, ApplicationIdentity, ApplicationKeyAlgorithm,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub const APPLICATION_KEY_REGISTRY_SCHEMA: &str = "dasobjectstore.application_key_registry.v1";
pub const APPLICATION_KEY_REGISTRY_FILE_NAME: &str = "application-keys.json";
pub const APPLICATION_KEY_REGISTRY_ENV: &str = "DASOBJECTSTORE_APPLICATION_KEYS_PATH";

static APPLICATION_KEY_REGISTRY_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn registry_lock() -> &'static Mutex<()> {
    APPLICATION_KEY_REGISTRY_LOCK.get_or_init(|| Mutex::new(()))
}

pub fn default_application_key_registry_path(state_dir: impl AsRef<Path>) -> PathBuf {
    state_dir.as_ref().join(APPLICATION_KEY_REGISTRY_FILE_NAME)
}

pub fn application_key_registry_path(state_dir: impl AsRef<Path>) -> PathBuf {
    std::env::var_os(APPLICATION_KEY_REGISTRY_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| default_application_key_registry_path(state_dir))
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct ApplicationKeyRegistryFile {
    schema_version: String,
    keys: Vec<ApplicationKeyDescriptor>,
}

impl Default for ApplicationKeyRegistryFile {
    fn default() -> Self {
        Self {
            schema_version: APPLICATION_KEY_REGISTRY_SCHEMA.to_string(),
            keys: Vec::new(),
        }
    }
}

pub fn list_application_keys(
    path: impl AsRef<Path>,
) -> Result<Vec<ApplicationKeyDescriptor>, DaemonServiceRuntimeError> {
    let _guard = registry_lock()
        .lock()
        .expect("application key registry lock poisoned");
    Ok(read_registry(path.as_ref())?.keys)
}

pub fn read_application_key(
    path: impl AsRef<Path>,
    application_id: &str,
    key_id: &str,
) -> Result<Option<ApplicationKeyDescriptor>, DaemonServiceRuntimeError> {
    let _guard = registry_lock()
        .lock()
        .expect("application key registry lock poisoned");
    Ok(read_registry(path.as_ref())?
        .keys
        .into_iter()
        .find(|key| key.application_id == application_id && key.key_id == key_id))
}

/// Resolve a CA-verified client certificate to one active daemon-owned
/// application identity. Multiple active certificate descriptors for the same
/// identity permit controlled rotation overlap; ambiguous cross-identity
/// fingerprints fail closed.
pub fn resolve_mtls_application_identity(
    identity_registry_path: impl AsRef<Path>,
    key_registry_path: impl AsRef<Path>,
    certificate_der: &[u8],
    now_unix_seconds: u64,
) -> Result<ApplicationIdentity, DaemonServiceRuntimeError> {
    let fingerprint = format!(
        "sha256:{}",
        Sha256::digest(certificate_der)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    resolve_mtls_application_identity_by_fingerprint(
        identity_registry_path,
        key_registry_path,
        &fingerprint,
        now_unix_seconds,
    )
}

pub fn resolve_mtls_application_identity_by_fingerprint(
    identity_registry_path: impl AsRef<Path>,
    key_registry_path: impl AsRef<Path>,
    certificate_fingerprint_sha256: &str,
    now_unix_seconds: u64,
) -> Result<ApplicationIdentity, DaemonServiceRuntimeError> {
    let matching = list_application_keys(key_registry_path)?
        .into_iter()
        .filter(|key| {
            key.algorithm == ApplicationKeyAlgorithm::MtlsCertificate
                && key
                    .public_key_fingerprint
                    .eq_ignore_ascii_case(certificate_fingerprint_sha256)
                && key.active
                && key.issued_at_unix_seconds <= now_unix_seconds
                && now_unix_seconds < key.expires_at_unix_seconds
        })
        .collect::<Vec<_>>();
    let Some(first) = matching.first() else {
        return Err(invalid_key(
            "client certificate is not mapped to an active application key",
        ));
    };
    if matching
        .iter()
        .any(|key| key.application_id != first.application_id)
    {
        return Err(invalid_key(
            "client certificate fingerprint maps to multiple application identities",
        ));
    }
    let identity = super::read_application_identity(identity_registry_path, &first.application_id)?
        .ok_or_else(|| invalid_key("mapped application identity is not registered"))?;
    if !identity.active
        || identity.credential_kind != ApplicationCredentialKind::MtlsCertificate
        || identity.issued_at_unix_seconds > now_unix_seconds
        || now_unix_seconds >= identity.expires_at_unix_seconds
    {
        return Err(invalid_key(
            "mapped application identity is inactive, expired, or not mTLS-enabled",
        ));
    }
    Ok(identity)
}

pub fn upsert_application_key(
    path: impl AsRef<Path>,
    key: ApplicationKeyDescriptor,
) -> Result<(), DaemonServiceRuntimeError> {
    let _guard = registry_lock()
        .lock()
        .expect("application key registry lock poisoned");
    key.validate()
        .map_err(|error| invalid_key(error.to_string()))?;
    let path = path.as_ref();
    let mut registry = read_registry(path)?;
    if let Some(existing) = registry.keys.iter_mut().find(|existing| {
        existing.application_id == key.application_id && existing.key_id == key.key_id
    }) {
        *existing = key;
    } else {
        registry.keys.push(key);
    }
    registry.keys.sort_by(|left, right| {
        left.application_id
            .cmp(&right.application_id)
            .then_with(|| left.key_id.cmp(&right.key_id))
    });
    write_registry(path, &registry)
}

pub fn deactivate_application_key(
    path: impl AsRef<Path>,
    application_id: &str,
    key_id: &str,
) -> Result<bool, DaemonServiceRuntimeError> {
    let _guard = registry_lock()
        .lock()
        .expect("application key registry lock poisoned");
    let path = path.as_ref();
    let mut registry = read_registry(path)?;
    let Some(key) = registry
        .keys
        .iter_mut()
        .find(|key| key.application_id == application_id && key.key_id == key_id)
    else {
        return Ok(false);
    };
    key.active = false;
    write_registry(path, &registry)?;
    Ok(true)
}

fn read_registry(path: &Path) -> Result<ApplicationKeyRegistryFile, DaemonServiceRuntimeError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(ApplicationKeyRegistryFile::default())
        }
        Err(error) => return Err(registry_io(path, error)),
    };
    let registry: ApplicationKeyRegistryFile = serde_json::from_reader(file).map_err(|error| {
        invalid_key(format!(
            "invalid application key registry {}: {error}",
            path.display()
        ))
    })?;
    if registry.schema_version != APPLICATION_KEY_REGISTRY_SCHEMA {
        return Err(invalid_key(format!(
            "unsupported application key registry schema {}",
            registry.schema_version
        )));
    }
    let mut key_ids = BTreeSet::new();
    for key in &registry.keys {
        key.validate()
            .map_err(|error| invalid_key(error.to_string()))?;
        let unique_id = format!("{}\0{}", key.application_id, key.key_id);
        if !key_ids.insert(unique_id) {
            return Err(invalid_key(format!(
                "duplicate application key {}/{}",
                key.application_id, key.key_id
            )));
        }
    }
    let mut registry = registry;
    registry.keys.sort_by(|left, right| {
        left.application_id
            .cmp(&right.application_id)
            .then_with(|| left.key_id.cmp(&right.key_id))
    });
    Ok(registry)
}

fn write_registry(
    path: &Path,
    registry: &ApplicationKeyRegistryFile,
) -> Result<(), DaemonServiceRuntimeError> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid_key("application key registry has no parent"))?;
    fs::create_dir_all(parent).map_err(|error| registry_io(parent, error))?;
    let temporary = parent.join(format!(
        ".{}.tmp-{}-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("application-keys"),
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    let bytes = serde_json::to_vec_pretty(registry)
        .map_err(|error| invalid_key(format!("serialize application key registry: {error}")))?;
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

fn invalid_key(message: impl Into<String>) -> DaemonServiceRuntimeError {
    DaemonServiceRuntimeError::UnsupportedOperation {
        operation: format!("invalid application key registry: {}", message.into()),
    }
}

fn registry_io(path: &Path, error: io::Error) -> DaemonServiceRuntimeError {
    DaemonServiceRuntimeError::UnsupportedOperation {
        operation: format!("application key registry I/O {}: {error}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        deactivate_application_key, default_application_key_registry_path, list_application_keys,
        read_application_key, resolve_mtls_application_identity, upsert_application_key,
        APPLICATION_KEY_REGISTRY_SCHEMA,
    };
    use crate::runtime::upsert_application_identity;
    use dasobjectstore_core::application_auth::{
        ApplicationCredentialKind, ApplicationEnvironment, ApplicationIdentity,
        ApplicationKeyAlgorithm, ApplicationKeyDescriptor, ApplicationOperation, ApplicationScope,
        APPLICATION_AUTH_SCHEMA_VERSION,
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
                "application-keys-{label}-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(&root).expect("fixture root");
        root
    }

    fn key(application_id: &str, key_id: &str) -> ApplicationKeyDescriptor {
        ApplicationKeyDescriptor {
            schema_version: APPLICATION_AUTH_SCHEMA_VERSION.to_string(),
            application_id: application_id.to_string(),
            key_id: key_id.to_string(),
            algorithm: ApplicationKeyAlgorithm::Ed25519,
            public_key_fingerprint: format!("sha256:{}", "a".repeat(64)),
            public_key_material: None,
            issued_at_unix_seconds: 1_000,
            expires_at_unix_seconds: 100_000,
            active: true,
        }
    }

    fn mtls_identity(application_id: &str) -> ApplicationIdentity {
        ApplicationIdentity {
            schema_version: APPLICATION_AUTH_SCHEMA_VERSION.to_string(),
            application_id: application_id.to_string(),
            owner: "operator".to_string(),
            purpose: "unattended ingress".to_string(),
            environment: ApplicationEnvironment::Production,
            credential_kind: ApplicationCredentialKind::MtlsCertificate,
            scope: ApplicationScope {
                store_ids: vec![StoreId::new("codex").expect("store")],
                prefixes: vec!["ingress".to_string()],
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

    fn mtls_key(
        application_id: &str,
        key_id: &str,
        certificate: &[u8],
    ) -> ApplicationKeyDescriptor {
        use sha2::{Digest, Sha256};
        let fingerprint = Sha256::digest(certificate)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        ApplicationKeyDescriptor {
            algorithm: ApplicationKeyAlgorithm::MtlsCertificate,
            public_key_fingerprint: format!("sha256:{fingerprint}"),
            ..key(application_id, key_id)
        }
    }

    #[test]
    fn default_registry_path_is_state_scoped() {
        assert_eq!(
            default_application_key_registry_path("/var/lib/dasobjectstore"),
            PathBuf::from("/var/lib/dasobjectstore/application-keys.json")
        );
    }

    #[test]
    fn key_rotation_metadata_round_trips_without_private_material() {
        let path = root("round-trip").join("keys.json");
        upsert_application_key(&path, key("synoptikon-ingest", "key-1")).expect("write");
        let found = read_application_key(&path, "synoptikon-ingest", "key-1")
            .expect("read")
            .expect("key");
        assert_eq!(found.key_id, "key-1");
        let encoded = fs::read_to_string(&path).expect("registry bytes");
        assert!(encoded.contains(APPLICATION_KEY_REGISTRY_SCHEMA));
        assert!(!encoded.contains("private_key"));
        assert!(!encoded.contains("/srv"));
    }

    #[test]
    fn keys_are_sorted_and_deactivation_preserves_descriptor() {
        let path = root("deactivate").join("keys.json");
        upsert_application_key(&path, key("zeta", "key-2")).expect("write");
        upsert_application_key(&path, key("alpha", "key-1")).expect("write");
        assert_eq!(list_application_keys(&path).expect("list").len(), 2);
        assert!(deactivate_application_key(&path, "alpha", "key-1").expect("deactivate"));
        assert!(!deactivate_application_key(&path, "missing", "key-1").expect("missing"));
        assert!(
            !read_application_key(&path, "alpha", "key-1")
                .expect("read")
                .expect("key")
                .active
        );
    }

    #[test]
    fn reads_normalize_restored_key_order() {
        let path = root("restore-order").join("keys.json");
        let payload = serde_json::json!({
            "schema_version": APPLICATION_KEY_REGISTRY_SCHEMA,
            "keys": [key("synoptikon", "zeta"), key("synoptikon", "alpha")]
        });
        fs::write(&path, serde_json::to_vec(&payload).expect("encode")).expect("write");
        let ids = list_application_keys(&path)
            .expect("list keys")
            .into_iter()
            .map(|key| key.key_id)
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["alpha", "zeta"]);
    }

    #[test]
    fn concurrent_key_upserts_preserve_all_descriptors() {
        let path = root("concurrent-upserts").join("keys.json");
        let left_path = path.clone();
        let left = std::thread::spawn(move || {
            upsert_application_key(&left_path, key("synoptikon", "left")).expect("write left")
        });
        let right_path = path.clone();
        let right = std::thread::spawn(move || {
            upsert_application_key(&right_path, key("synoptikon", "right")).expect("write right")
        });
        left.join().expect("left joins");
        right.join().expect("right joins");

        let ids = list_application_keys(&path)
            .expect("list keys")
            .into_iter()
            .map(|key| key.key_id)
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["left", "right"]);
    }

    #[test]
    fn mtls_resolution_allows_rotation_overlap_and_fails_closed() {
        let root = root("mtls-resolution");
        let identities = root.join("identities.json");
        let keys = root.join("keys.json");
        let certificate = b"full-client-certificate-der";
        upsert_application_identity(&identities, mtls_identity("synoptikon")).expect("identity");
        upsert_application_key(&keys, mtls_key("synoptikon", "old", certificate))
            .expect("old certificate");
        upsert_application_key(&keys, mtls_key("synoptikon", "new", certificate))
            .expect("rotation certificate");

        let resolved = resolve_mtls_application_identity(&identities, &keys, certificate, 2_000)
            .expect("same-identity overlap resolves");
        assert_eq!(resolved.application_id, "synoptikon");

        upsert_application_identity(&identities, mtls_identity("monas")).expect("second identity");
        upsert_application_key(&keys, mtls_key("monas", "duplicate", certificate))
            .expect("ambiguous certificate");
        assert!(resolve_mtls_application_identity(&identities, &keys, certificate, 2_000).is_err());
        assert!(resolve_mtls_application_identity(&identities, &keys, b"unknown", 2_000).is_err());
        assert!(
            resolve_mtls_application_identity(&identities, &keys, certificate, 100_000).is_err()
        );
    }
}
