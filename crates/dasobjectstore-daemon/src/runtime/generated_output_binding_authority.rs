//! Daemon-trusted admissions for generated-output bindings.
//!
//! This registry is intentionally distinct from read-capability authority.
//! A record proves only that a DASObjectStore administrator admitted a
//! validated generated-output policy; it neither mints a capability nor makes
//! any provider write route available.

use super::DaemonServiceRuntimeError;
use dasobjectstore_core::application_auth_v2::GeneratedOutputBindingV1;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub const GENERATED_OUTPUT_BINDING_AUTHORITY_SCHEMA: &str =
    "dasobjectstore.generated_output_binding_authority.v1";
pub const GENERATED_OUTPUT_BINDING_AUTHORITY_FILE_NAME: &str =
    "generated-output-binding-authority.json";

static REGISTRY_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TrustedGeneratedOutputBindingAuthority {
    pub binding: GeneratedOutputBindingV1,
    pub binding_digest_sha256: String,
    pub admitted_at_unix_seconds: u64,
    pub active: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    schema_version: String,
    records: Vec<TrustedGeneratedOutputBindingAuthority>,
}

impl Default for Registry {
    fn default() -> Self {
        Self {
            schema_version: GENERATED_OUTPUT_BINDING_AUTHORITY_SCHEMA.to_string(),
            records: Vec::new(),
        }
    }
}

pub fn generated_output_binding_authority_path(state_dir: impl AsRef<Path>) -> PathBuf {
    state_dir
        .as_ref()
        .join(GENERATED_OUTPUT_BINDING_AUTHORITY_FILE_NAME)
}

/// Replace the record for one stable binding identity after validation. The
/// state file contains public policy metadata only and is mode 0600 because
/// the association of tenants, projects, and ObjectStores is operationally
/// sensitive.
pub fn upsert_trusted_generated_output_binding_authority(
    path: impl AsRef<Path>,
    record: TrustedGeneratedOutputBindingAuthority,
) -> Result<(), DaemonServiceRuntimeError> {
    validate_record(&record)?;
    let _guard = lock()?;
    let path = path.as_ref();
    let mut registry = read(path)?;
    if let Some(existing) = registry
        .records
        .iter_mut()
        .find(|existing| existing.binding.binding_id == record.binding.binding_id)
    {
        *existing = record;
    } else {
        registry.records.push(record);
    }
    registry
        .records
        .sort_by(|left, right| left.binding.binding_id.cmp(&right.binding.binding_id));
    validate_registry(&registry)?;
    write_private_json(path, &registry)
}

fn validate_record(
    record: &TrustedGeneratedOutputBindingAuthority,
) -> Result<(), DaemonServiceRuntimeError> {
    if !record.active || !is_sha256(&record.binding_digest_sha256) {
        return Err(invalid(
            "trusted generated-output binding authority is invalid",
        ));
    }
    record
        .binding
        .validate_at(record.admitted_at_unix_seconds)
        .map_err(|error| invalid(format!("generated-output binding is invalid: {error}")))
}

fn validate_registry(registry: &Registry) -> Result<(), DaemonServiceRuntimeError> {
    if registry.schema_version != GENERATED_OUTPUT_BINDING_AUTHORITY_SCHEMA {
        return Err(invalid(
            "unsupported generated-output binding authority registry schema",
        ));
    }
    let mut identities = BTreeSet::new();
    for record in &registry.records {
        validate_record(record)?;
        if !identities.insert(record.binding.binding_id.as_str()) {
            return Err(invalid(
                "duplicate generated-output binding authority identity is ambiguous",
            ));
        }
    }
    Ok(())
}

fn lock() -> Result<std::sync::MutexGuard<'static, ()>, DaemonServiceRuntimeError> {
    REGISTRY_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| invalid("generated-output binding authority registry lock poisoned"))
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
    let parent = path
        .parent()
        .ok_or_else(|| invalid("registry path has no parent"))?;
    fs::create_dir_all(parent)
        .map_err(|error| invalid(format!("create {}: {error}", parent.display())))?;
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
        File::open(parent)?.sync_all()?;
        Ok::<(), io::Error>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|error| invalid(format!("write {}: {error}", path.display())))
}

fn temporary_path(path: &Path) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    path.with_extension(format!("tmp-{}-{suffix}", std::process::id()))
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
        ERGASTERION_GENERATED_OUTPUT_BINDING_SCHEMA_VERSION_V1,
        ERGASTERION_GENERATED_OUTPUT_POLICY_CLASS,
    };
    use dasobjectstore_core::ids::StoreId;

    fn record() -> TrustedGeneratedOutputBindingAuthority {
        TrustedGeneratedOutputBindingAuthority {
            binding: GeneratedOutputBindingV1 {
                schema_version: ERGASTERION_GENERATED_OUTPUT_BINDING_SCHEMA_VERSION_V1.to_string(),
                binding_id: "generated-output-current".to_string(),
                application_id: "ergasterion-output-completion".to_string(),
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
                    prefixes: vec!["project-rna/outputs".to_string()],
                    operations: vec![
                        ApplicationOperation::Write,
                        ApplicationOperation::CompleteUpload,
                        ApplicationOperation::Verify,
                    ],
                },
                policy_class: ERGASTERION_GENERATED_OUTPUT_POLICY_CLASS.to_string(),
                max_object_bytes: 1024,
                max_total_bytes: 2048,
                issued_at: "2026-07-27T10:00:00Z".to_string(),
                expires_at: "2099-07-27T10:10:00Z".to_string(),
                status: GovernedBindingStatus::Active,
            },
            binding_digest_sha256: "b".repeat(64),
            admitted_at_unix_seconds: 1_800_000_000,
            active: true,
        }
    }

    #[test]
    fn persists_only_valid_active_generated_output_admission() {
        let path = std::env::temp_dir().join(format!(
            "dasobjectstore-generated-output-authority-{}.json",
            std::process::id()
        ));
        let admission = record();
        upsert_trusted_generated_output_binding_authority(&path, admission).unwrap();
        let rendered = fs::read_to_string(&path).unwrap();
        assert!(rendered.contains("generated-output-current"));
        assert!(!rendered.contains("private_key"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn rejects_inactive_or_malformed_admissions() {
        let path = std::env::temp_dir().join(format!(
            "dasobjectstore-generated-output-authority-invalid-{}.json",
            std::process::id()
        ));
        let mut admission = record();
        admission.active = false;
        assert!(upsert_trusted_generated_output_binding_authority(&path, admission).is_err());
        assert!(!path.exists());
    }
}
