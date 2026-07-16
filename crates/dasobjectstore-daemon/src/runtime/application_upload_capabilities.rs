use super::DaemonServiceRuntimeError;
use crate::runtime::RemoteUploadProviderCompletion;
use dasobjectstore_core::application_auth::UploadCompletionCapability;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub const APPLICATION_UPLOAD_CAPABILITY_SCHEMA: &str =
    "dasobjectstore.application_upload_capabilities.v1";
pub const APPLICATION_UPLOAD_CAPABILITY_FILE_NAME: &str = "application-upload-capabilities.json";
static REGISTRY_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PendingApplicationUploadCapability {
    pub capability: UploadCompletionCapability,
    pub completion: RemoteUploadProviderCompletion,
    /// Daemon-owned logical-capacity reservation. Older transient records may
    /// omit it, but completion must fail closed rather than publish uncharged
    /// data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capacity_reservation_id: Option<String>,
    #[serde(default)]
    pub capacity_settlement: ApplicationUploadCapacitySettlement,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationUploadCapacitySettlement {
    #[default]
    Reserved,
    Prepared,
    Committed,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    schema_version: String,
    records: Vec<PendingApplicationUploadCapability>,
}

pub fn application_upload_capability_path(state_dir: impl AsRef<Path>) -> PathBuf {
    state_dir
        .as_ref()
        .join(APPLICATION_UPLOAD_CAPABILITY_FILE_NAME)
}

pub fn issue_application_upload_capability(
    path: impl AsRef<Path>,
    record: PendingApplicationUploadCapability,
    now: u64,
) -> Result<(), DaemonServiceRuntimeError> {
    let _guard = lock()?;
    record
        .capability
        .validate()
        .map_err(|error| invalid(error.to_string()))?;
    if record.capability.upload_id != record.completion.upload_id
        || record.capability.object_key != record.completion.object_key
        || record.capability.expected_checksum != record.completion.expected_checksum
    {
        return Err(invalid(
            "upload capability and provider completion identities differ",
        ));
    }
    let mut registry = read(path.as_ref())?;
    registry
        .records
        .retain(|item| item.capability.expires_at_unix_seconds > now);
    if registry.records.iter().any(|item| {
        item.capability.capability_id == record.capability.capability_id
            || item.capability.nonce == record.capability.nonce
    }) {
        return Err(invalid("upload capability identity already exists"));
    }
    registry.records.push(record);
    write(path.as_ref(), &registry)
}

pub fn read_application_upload_capability(
    path: impl AsRef<Path>,
    supplied: &UploadCompletionCapability,
    now: u64,
) -> Result<PendingApplicationUploadCapability, DaemonServiceRuntimeError> {
    let _guard = lock()?;
    let mut registry = read(path.as_ref())?;
    registry
        .records
        .retain(|item| item.capability.expires_at_unix_seconds > now);
    let found = registry
        .records
        .iter()
        .find(|item| {
            item.capability.capability_id == supplied.capability_id
                && item.capability.application_id == supplied.application_id
        })
        .cloned();
    write(path.as_ref(), &registry)?;
    match found {
        Some(record) if record.capability == *supplied => Ok(record),
        Some(_) => Err(invalid(
            "supplied upload capability does not match daemon issuance",
        )),
        None => Err(invalid("upload capability was not issued or has expired")),
    }
}

pub fn prepare_application_upload_capacity_settlement(
    path: impl AsRef<Path>,
    capability_id: &str,
) -> Result<ApplicationUploadCapacitySettlement, DaemonServiceRuntimeError> {
    update_settlement(path.as_ref(), capability_id, |state| match state {
        ApplicationUploadCapacitySettlement::Reserved => {
            ApplicationUploadCapacitySettlement::Prepared
        }
        state => state,
    })
}

pub fn commit_application_upload_capacity_settlement(
    path: impl AsRef<Path>,
    capability_id: &str,
) -> Result<ApplicationUploadCapacitySettlement, DaemonServiceRuntimeError> {
    update_settlement(path.as_ref(), capability_id, |state| match state {
        ApplicationUploadCapacitySettlement::Prepared
        | ApplicationUploadCapacitySettlement::Committed => {
            ApplicationUploadCapacitySettlement::Committed
        }
        ApplicationUploadCapacitySettlement::Reserved => state,
    })
}

fn update_settlement(
    path: &Path,
    capability_id: &str,
    transition: impl FnOnce(ApplicationUploadCapacitySettlement) -> ApplicationUploadCapacitySettlement,
) -> Result<ApplicationUploadCapacitySettlement, DaemonServiceRuntimeError> {
    let _guard = lock()?;
    let mut registry = read(path)?;
    let record = registry
        .records
        .iter_mut()
        .find(|record| record.capability.capability_id == capability_id)
        .ok_or_else(|| invalid("upload capacity settlement capability is not registered"))?;
    let next = transition(record.capacity_settlement);
    if next == ApplicationUploadCapacitySettlement::Reserved
        && record.capacity_settlement == ApplicationUploadCapacitySettlement::Reserved
    {
        return Err(invalid(
            "upload capacity settlement must be prepared before commit",
        ));
    }
    record.capacity_settlement = next;
    write(path, &registry)?;
    Ok(next)
}

fn lock() -> Result<std::sync::MutexGuard<'static, ()>, DaemonServiceRuntimeError> {
    REGISTRY_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| invalid("application upload capability registry lock poisoned"))
}

fn read(path: &Path) -> Result<Registry, DaemonServiceRuntimeError> {
    match File::open(path) {
        Ok(file) => {
            let registry: Registry = serde_json::from_reader(file)
                .map_err(|error| invalid(format!("parse {}: {error}", path.display())))?;
            if registry.schema_version != APPLICATION_UPLOAD_CAPABILITY_SCHEMA {
                return Err(invalid(
                    "unsupported application upload capability registry schema",
                ));
            }
            Ok(registry)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Registry {
            schema_version: APPLICATION_UPLOAD_CAPABILITY_SCHEMA.to_string(),
            records: Vec::new(),
        }),
        Err(error) => Err(invalid(format!("open {}: {error}", path.display()))),
    }
}

fn write(path: &Path, registry: &Registry) -> Result<(), DaemonServiceRuntimeError> {
    if let Some(parent) = path.parent() {
        let created = !parent.exists();
        fs::create_dir_all(parent).map_err(|e| invalid(e.to_string()))?;
        #[cfg(unix)]
        if created {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
                .map_err(|e| invalid(e.to_string()))?;
        }
    }
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = path.with_extension(format!("tmp-{suffix}"));
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|e| invalid(e.to_string()))?;
    serde_json::to_writer_pretty(&mut file, registry).map_err(|e| invalid(e.to_string()))?;
    file.write_all(b"\n")
        .and_then(|_| file.sync_all())
        .map_err(|e| invalid(e.to_string()))?;
    fs::rename(&temporary, path).map_err(|e| invalid(e.to_string()))
}

fn invalid(message: impl Into<String>) -> DaemonServiceRuntimeError {
    DaemonServiceRuntimeError::UnsupportedOperation {
        operation: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dasobjectstore_core::application_auth::APPLICATION_AUTH_SCHEMA_VERSION;
    use dasobjectstore_core::ids::StoreId;

    fn record(id: &str) -> PendingApplicationUploadCapability {
        PendingApplicationUploadCapability {
            capability: UploadCompletionCapability {
                schema_version: APPLICATION_AUTH_SCHEMA_VERSION.to_string(),
                capability_id: id.to_string(),
                application_id: "synoptikon".to_string(),
                session_id: "session-1".to_string(),
                upload_id: "upload-1".to_string(),
                store_id: StoreId::new("science").unwrap(),
                object_key: "runs/a.fastq".to_string(),
                expected_size_bytes: 7,
                expected_checksum: format!("sha256:{}", "a".repeat(64)),
                audience: "dasobjectstored".to_string(),
                issued_at_unix_seconds: 10,
                expires_at_unix_seconds: 100,
                nonce: format!("nonce-{id}"),
            },
            completion: RemoteUploadProviderCompletion {
                upload_id: "upload-1".to_string(),
                provider: "garage".to_string(),
                bucket: "science".to_string(),
                object_id: "object-1".to_string(),
                object_version: 1,
                object_key: "runs/a.fastq".to_string(),
                expected_checksum: format!("sha256:{}", "a".repeat(64)),
                endpoint_url: "https://object.example".to_string(),
            },
            capacity_reservation_id: Some(format!("application-upload-{id}")),
            capacity_settlement: ApplicationUploadCapacitySettlement::Reserved,
        }
    }

    #[test]
    fn requires_exact_daemon_issued_capability_and_prunes_expired_records() {
        let path = std::env::temp_dir().join(format!(
            "dasobjectstore-upload-capability-{}-{}.json",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let issued = record("capability-1");
        issue_application_upload_capability(&path, issued.clone(), 10).unwrap();
        assert_eq!(
            read_application_upload_capability(&path, &issued.capability, 20).unwrap(),
            issued
        );
        assert_eq!(
            prepare_application_upload_capacity_settlement(&path, &issued.capability.capability_id)
                .unwrap(),
            ApplicationUploadCapacitySettlement::Prepared
        );
        assert_eq!(
            commit_application_upload_capacity_settlement(&path, &issued.capability.capability_id)
                .unwrap(),
            ApplicationUploadCapacitySettlement::Committed
        );
        assert_eq!(
            read_application_upload_capability(&path, &issued.capability, 20)
                .unwrap()
                .capacity_settlement,
            ApplicationUploadCapacitySettlement::Committed
        );
        let mut forged = issued.capability.clone();
        forged.expected_size_bytes += 1;
        assert!(read_application_upload_capability(&path, &forged, 20).is_err());
        assert!(read_application_upload_capability(&path, &issued.capability, 100).is_err());
        let contents = fs::read_to_string(&path).unwrap();
        assert!(!contents.contains("renewal_token"));
        assert!(!contents.contains("secret_access_key"));
        let _ = fs::remove_file(path);
    }
}
