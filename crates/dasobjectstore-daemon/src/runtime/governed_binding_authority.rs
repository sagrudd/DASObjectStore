//! Daemon-trusted current authority for governed ObjectStore bindings.
//!
//! Exchange callers carry a binding and sign its bytes, but that does not make
//! the binding authoritative. This registry is populated only by a trusted
//! integration or an authenticated administrator and requires the caller's
//! binding identity and both authority revisions to match the one current
//! daemon record exactly.

use super::application_capability_ledger::ApplicationCapabilityClaims;
use super::DaemonServiceRuntimeError;
use dasobjectstore_core::application_auth_v2::{
    GovernedBindingAuthorityVerificationInputV2, GovernedHostAuthorityV2,
    GovernedObjectStoreBindingV2, GovernedProsopikonAuthorityV2,
};
use dasobjectstore_core::ids::StoreId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub const GOVERNED_BINDING_AUTHORITY_SCHEMA: &str = "dasobjectstore.governed_binding_authority.v1";
pub const GOVERNED_BINDING_AUTHORITY_FILE_NAME: &str = "governed-binding-authority.json";

static REGISTRY_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TrustedGovernedBindingAuthority {
    pub binding_id: String,
    pub object_store_id: StoreId,
    pub binding_digest_sha256: String,
    pub tenant_id: String,
    pub host_authority: GovernedHostAuthorityV2,
    pub prosopikon_authority: GovernedProsopikonAuthorityV2,
    pub admitted_at_unix_seconds: u64,
    pub expires_at_unix_seconds: u64,
    pub active: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at_unix_seconds: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    schema_version: String,
    records: Vec<TrustedGovernedBindingAuthority>,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            schema_version: GOVERNED_BINDING_AUTHORITY_SCHEMA.to_string(),
            records: Vec::new(),
        }
    }
}

pub fn governed_binding_authority_path(state_dir: impl AsRef<Path>) -> PathBuf {
    state_dir
        .as_ref()
        .join(GOVERNED_BINDING_AUTHORITY_FILE_NAME)
}

pub fn upsert_trusted_governed_binding_authority(
    path: impl AsRef<Path>,
    record: TrustedGovernedBindingAuthority,
) -> Result<(), DaemonServiceRuntimeError> {
    validate_record(&record)?;
    let _guard = lock()?;
    let path = path.as_ref();
    let mut registry = read(path)?;
    if let Some(existing) = registry
        .records
        .iter_mut()
        .find(|item| item.binding_id == record.binding_id)
    {
        *existing = record;
    } else {
        registry.records.push(record);
    }
    registry
        .records
        .sort_by(|left, right| left.binding_id.cmp(&right.binding_id));
    validate_registry(&registry)?;
    write_private_json(path, &registry)
}

pub fn revoke_trusted_governed_binding_authority(
    path: impl AsRef<Path>,
    binding_id: &str,
    revoked_at_unix_seconds: u64,
) -> Result<bool, DaemonServiceRuntimeError> {
    if binding_id.trim().is_empty() || revoked_at_unix_seconds == 0 {
        return Err(invalid("binding revocation identity and time are required"));
    }
    let _guard = lock()?;
    let path = path.as_ref();
    let mut registry = read(path)?;
    let Some(record) = registry
        .records
        .iter_mut()
        .find(|item| item.binding_id == binding_id)
    else {
        return Ok(false);
    };
    record.active = false;
    record.revoked_at_unix_seconds = Some(revoked_at_unix_seconds);
    write_private_json(path, &registry)?;
    Ok(true)
}

/// Resolve the one current trusted authority and compare the complete binding
/// identity and revisions. Unknown, unavailable, revoked, expired, stale, and
/// ambiguous authority all fail closed.
pub fn verify_current_governed_binding_authority(
    path: impl AsRef<Path>,
    binding: &GovernedObjectStoreBindingV2,
    binding_digest_sha256: &str,
    now_unix_seconds: u64,
) -> Result<GovernedBindingAuthorityVerificationInputV2, DaemonServiceRuntimeError> {
    if !is_sha256(binding_digest_sha256) {
        return Err(invalid("governed binding digest is invalid"));
    }
    binding
        .validate_at(now_unix_seconds)
        .map_err(|error| invalid(format!("governed binding is invalid: {error}")))?;
    let _guard = lock()?;
    let registry = read(path.as_ref())?;
    let matching = registry
        .records
        .iter()
        .filter(|record| {
            record.binding_id == binding.binding_id
                && record.object_store_id == binding.object_store_id
        })
        .collect::<Vec<_>>();
    let [record] = matching.as_slice() else {
        return Err(invalid(
            "governed binding authority is unavailable or ambiguous",
        ));
    };
    if !record.active
        || record.revoked_at_unix_seconds.is_some()
        || now_unix_seconds < record.admitted_at_unix_seconds
        || now_unix_seconds >= record.expires_at_unix_seconds
        || record.binding_digest_sha256 != binding_digest_sha256
    {
        return Err(invalid(
            "governed binding authority is inactive, revoked, or expired",
        ));
    }
    let authority = GovernedBindingAuthorityVerificationInputV2 {
        tenant_id: record.tenant_id.clone(),
        host_authority: record.host_authority.clone(),
        prosopikon_authority: record.prosopikon_authority.clone(),
    };
    binding
        .verify_current_authority(&authority)
        .map_err(|error| invalid(format!("governed binding authority rejected: {error}")))?;
    Ok(authority)
}

/// Non-secret discovery readiness. Authority identities and revisions never
/// leave the daemon through this projection.
pub fn governed_binding_authority_ready(
    path: impl AsRef<Path>,
    now_unix_seconds: u64,
) -> Result<bool, DaemonServiceRuntimeError> {
    let _guard = lock()?;
    let registry = read(path.as_ref())?;
    Ok(registry.records.iter().any(|record| {
        record.active
            && record.revoked_at_unix_seconds.is_none()
            && record.admitted_at_unix_seconds <= now_unix_seconds
            && now_unix_seconds < record.expires_at_unix_seconds
    }))
}

/// Revalidate all authority dimensions captured at issuance before every
/// provider request. No caller-supplied binding is needed at this stage.
pub fn verify_current_governed_authority_claims(
    path: impl AsRef<Path>,
    claims: &ApplicationCapabilityClaims,
    now_unix_seconds: u64,
) -> Result<(), DaemonServiceRuntimeError> {
    let _guard = lock()?;
    let registry = read(path.as_ref())?;
    let matching = registry
        .records
        .iter()
        .filter(|record| {
            record.binding_id == claims.binding_id
                && record.object_store_id.as_str() == claims.store_id
        })
        .collect::<Vec<_>>();
    let [record] = matching.as_slice() else {
        return Err(invalid(
            "governed binding authority is unavailable or ambiguous",
        ));
    };
    if !record.active
        || record.revoked_at_unix_seconds.is_some()
        || now_unix_seconds < record.admitted_at_unix_seconds
        || now_unix_seconds >= record.expires_at_unix_seconds
        || record.binding_digest_sha256 != claims.binding_digest_sha256
        || record.tenant_id != claims.tenant_id
        || record.host_authority != claims.host_authority
        || record.prosopikon_authority != claims.prosopikon_authority
    {
        return Err(invalid(
            "governed binding authority changed, was revoked, or expired",
        ));
    }
    Ok(())
}

fn validate_record(
    record: &TrustedGovernedBindingAuthority,
) -> Result<(), DaemonServiceRuntimeError> {
    if record.binding_id.trim().is_empty()
        || !is_sha256(&record.binding_digest_sha256)
        || record.tenant_id.trim().is_empty()
        || record.host_authority.authority_id.trim().is_empty()
        || record.host_authority.project_id.trim().is_empty()
        || record.prosopikon_authority.authority_id.trim().is_empty()
        || record.host_authority.project_revision == 0
        || record.prosopikon_authority.authority_revision == 0
        || record.admitted_at_unix_seconds >= record.expires_at_unix_seconds
        || (record.active && record.revoked_at_unix_seconds.is_some())
        || (!record.active && record.revoked_at_unix_seconds.is_none())
    {
        return Err(invalid("trusted governed binding authority is invalid"));
    }
    Ok(())
}

fn validate_registry(registry: &Registry) -> Result<(), DaemonServiceRuntimeError> {
    if registry.schema_version != GOVERNED_BINDING_AUTHORITY_SCHEMA {
        return Err(invalid(
            "unsupported governed binding authority registry schema",
        ));
    }
    let mut identities = BTreeSet::new();
    for record in &registry.records {
        validate_record(record)?;
        if !identities.insert(record.binding_id.as_str()) {
            return Err(invalid(
                "duplicate governed binding authority identity is ambiguous",
            ));
        }
    }
    Ok(())
}

fn lock() -> Result<std::sync::MutexGuard<'static, ()>, DaemonServiceRuntimeError> {
    REGISTRY_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| invalid("governed binding authority registry lock poisoned"))
}

fn read(path: &Path) -> Result<Registry, DaemonServiceRuntimeError> {
    match File::open(path) {
        Ok(file) => {
            validate_private_file(path)?;
            let registry: Registry = serde_json::from_reader(file)
                .map_err(|error| invalid(format!("parse {}: {error}", path.display())))?;
            validate_registry(&registry)?;
            Ok(registry)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Registry::default()),
        Err(error) => Err(invalid(format!("open {}: {error}", path.display()))),
    }
}

fn write_private_json(path: &Path, registry: &Registry) -> Result<(), DaemonServiceRuntimeError> {
    validate_registry(registry)?;
    ensure_parent(path)?;
    let temporary = temporary_path(path);
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options.open(&temporary)?;
        serde_json::to_writer_pretty(&mut file, registry).map_err(io::Error::other)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        validate_private_file(path).map_err(io::Error::other)?;
        sync_parent(path)?;
        Ok::<(), io::Error>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|error| invalid(format!("write {}: {error}", path.display())))
}

fn ensure_parent(path: &Path) -> Result<(), DaemonServiceRuntimeError> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid("registry path has no parent"))?;
    fs::create_dir_all(parent)
        .map_err(|error| invalid(format!("create {}: {error}", parent.display())))?;
    Ok(())
}

fn temporary_path(path: &Path) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    path.with_extension(format!("tmp-{}-{suffix}", std::process::id()))
}

fn sync_parent(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}

#[cfg(unix)]
fn validate_private_file(path: &Path) -> Result<(), DaemonServiceRuntimeError> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| invalid(format!("inspect {}: {error}", path.display())))?;
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.permissions().mode() & 0o777 != 0o600
    {
        return Err(invalid(format!(
            "{} must be a regular mode-0600 file owned by the daemon user",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_private_file(path: &Path) -> Result<(), DaemonServiceRuntimeError> {
    if !path.is_file() {
        return Err(invalid(format!(
            "{} must be a regular file",
            path.display()
        )));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> DaemonServiceRuntimeError {
    DaemonServiceRuntimeError::UnsupportedOperation {
        operation: message.into(),
    }
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dasobjectstore_core::application_auth::{
        ApplicationOperation, GovernedBindingStatus, GovernedObjectStoreBindingScope,
    };
    use dasobjectstore_core::application_auth_v2::{
        GovernedHostAuthorityV2, GovernedHostModeV2, GovernedProsopikonAuthorityV2,
    };

    fn binding() -> GovernedObjectStoreBindingV2 {
        GovernedObjectStoreBindingV2 {
            schema_version: "ergasterion.object-store-binding.v2".to_string(),
            binding_id: "binding-current".to_string(),
            host_authority: GovernedHostAuthorityV2 {
                mode: GovernedHostModeV2::Monas,
                authority_id: "1fda5cc0-7180-4cef-aef3-4942458f7a9e".to_string(),
                project_id: "project-rna".to_string(),
                project_revision: 7,
            },
            prosopikon_authority: GovernedProsopikonAuthorityV2 {
                authority_id: "8b1aaf69-74b8-48bc-a163-883fd3c693a3".to_string(),
                authority_revision: 11,
            },
            tenant_id: "6dd29575-9763-4e2b-8255-6bf7380f3813".to_string(),
            object_store_id: StoreId::new("science").unwrap(),
            scope: GovernedObjectStoreBindingScope {
                prefixes: vec!["project-rna/inputs".to_string()],
                operations: vec![ApplicationOperation::Read],
            },
            issued_at: "2026-07-27T10:00:00Z".to_string(),
            expires_at: "2099-07-27T10:10:00Z".to_string(),
            status: GovernedBindingStatus::Active,
        }
    }

    fn record(binding: &GovernedObjectStoreBindingV2) -> TrustedGovernedBindingAuthority {
        TrustedGovernedBindingAuthority {
            binding_id: binding.binding_id.clone(),
            object_store_id: binding.object_store_id.clone(),
            binding_digest_sha256: "b".repeat(64),
            tenant_id: binding.tenant_id.clone(),
            host_authority: binding.host_authority.clone(),
            prosopikon_authority: binding.prosopikon_authority.clone(),
            admitted_at_unix_seconds: 1,
            expires_at_unix_seconds: u64::MAX,
            active: true,
            revoked_at_unix_seconds: None,
        }
    }

    #[test]
    fn exact_current_authority_survives_restart_and_stale_revision_fails_closed() {
        let path = fixture("restart");
        let binding = binding();
        upsert_trusted_governed_binding_authority(&path, record(&binding)).unwrap();
        assert_eq!(
            verify_current_governed_binding_authority(
                &path,
                &binding,
                &"b".repeat(64),
                1_800_000_000
            )
            .unwrap(),
            GovernedBindingAuthorityVerificationInputV2 {
                tenant_id: binding.tenant_id.clone(),
                host_authority: binding.host_authority.clone(),
                prosopikon_authority: binding.prosopikon_authority.clone(),
            }
        );
        let mut stale = binding.clone();
        stale.host_authority.project_revision += 1;
        assert!(verify_current_governed_binding_authority(
            &path,
            &stale,
            &"b".repeat(64),
            1_800_000_000
        )
        .is_err());
        assert!(verify_current_governed_binding_authority(
            &path,
            &binding,
            &"c".repeat(64),
            1_800_000_000
        )
        .is_err());
        revoke_trusted_governed_binding_authority(&path, &binding.binding_id, 1_800_000_001)
            .unwrap();
        assert!(verify_current_governed_binding_authority(
            &path,
            &binding,
            &"b".repeat(64),
            1_800_000_002
        )
        .is_err());
        assert!(!governed_binding_authority_ready(&path, 1_800_000_002).unwrap());
        let _ = fs::remove_file(path);
    }

    #[cfg(unix)]
    #[test]
    fn persistence_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let path = fixture("private");
        let binding = binding();
        upsert_trusted_governed_binding_authority(&path, record(&binding)).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn issued_claims_fail_when_current_authority_revision_changes() {
        use crate::runtime::ApplicationCapabilityClaims;
        let path = fixture("claim-revalidation");
        let binding = binding();
        upsert_trusted_governed_binding_authority(&path, record(&binding)).unwrap();
        let claims = ApplicationCapabilityClaims {
            application_id: "app-7e4a31c9b260".to_string(),
            key_id: "ergasterion-ed25519".to_string(),
            binding_id: binding.binding_id.clone(),
            binding_digest_sha256: "b".repeat(64),
            tenant_id: binding.tenant_id.clone(),
            host_authority: binding.host_authority.clone(),
            prosopikon_authority: binding.prosopikon_authority.clone(),
            audience: "ergasterion-governed-data-service".to_string(),
            store_id: binding.object_store_id.to_string(),
            prefixes: vec!["project-rna/inputs".to_string()],
            operations: vec!["read".to_string()],
            max_object_bytes: 10,
            max_total_bytes: 20,
        };
        verify_current_governed_authority_claims(&path, &claims, 1_800_000_000).unwrap();
        let mut changed = record(&binding);
        changed.host_authority.project_revision += 1;
        upsert_trusted_governed_binding_authority(&path, changed).unwrap();
        assert!(verify_current_governed_authority_claims(&path, &claims, 1_800_000_000).is_err());
        let _ = fs::remove_file(path);
    }

    fn fixture(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "dasobjectstore-governed-authority-{label}-{}-{}.json",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
}
