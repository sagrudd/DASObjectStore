//! Per-store object service credential generation.

use crate::provider::ObjectServiceError;
use dasobjectstore_core::ids::StoreId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

const ACCESS_KEY_RANDOM_BYTES: usize = 10;
const SECRET_KEY_RANDOM_BYTES: usize = 32;
pub const GARAGE_CREDENTIAL_REGISTRY_ENV: &str = "DASOBJECTSTORE_GARAGE_CREDENTIAL_REGISTRY_PATH";

#[cfg(target_os = "macos")]
const DEFAULT_GARAGE_CREDENTIAL_REGISTRY_PATH: &str =
    "/usr/local/var/lib/dasobjectstore/object-service/garage-credentials.json";
#[cfg(not(target_os = "macos"))]
const DEFAULT_GARAGE_CREDENTIAL_REGISTRY_PATH: &str =
    "/var/lib/dasobjectstore/object-service/garage-credentials.json";

#[cfg(unix)]
const CREDENTIAL_REGISTRY_DIR_MODE: u32 = 0o700;
#[cfg(unix)]
const CREDENTIAL_REGISTRY_FILE_MODE: u32 = 0o600;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreCredentialRequest {
    pub store_id: StoreId,
    pub bucket_name: String,
}

#[derive(Clone, Eq, PartialEq)]
pub struct StoreServiceCredential {
    pub store_id: StoreId,
    pub bucket_name: String,
    pub credential_reference: String,
    pub access_key_id: String,
    pub secret_access_key: SecretAccessKey,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CredentialReferenceManifest {
    pub format_version: u16,
    pub generated_at_utc: String,
    pub references: Vec<StoreCredentialReference>,
}

impl CredentialReferenceManifest {
    pub fn from_credentials(
        generated_at_utc: impl Into<String>,
        credentials: &[StoreServiceCredential],
    ) -> Self {
        Self {
            format_version: 1,
            generated_at_utc: generated_at_utc.into(),
            references: credentials
                .iter()
                .map(StoreCredentialReference::from_credential)
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreCredentialReference {
    pub store_id: StoreId,
    pub bucket_name: String,
    pub credential_reference: String,
    pub access_key_id: String,
}

impl StoreCredentialReference {
    pub fn from_credential(credential: &StoreServiceCredential) -> Self {
        Self {
            store_id: credential.store_id.clone(),
            bucket_name: credential.bucket_name.clone(),
            credential_reference: credential.credential_reference.clone(),
            access_key_id: credential.access_key_id.clone(),
        }
    }
}

impl fmt::Debug for StoreServiceCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StoreServiceCredential")
            .field("store_id", &self.store_id)
            .field("bucket_name", &self.bucket_name)
            .field("credential_reference", &self.credential_reference)
            .field("access_key_id", &self.access_key_id)
            .field("secret_access_key", &self.secret_access_key)
            .finish()
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct SecretAccessKey(String);

impl SecretAccessKey {
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretAccessKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SecretAccessKey(REDACTED)")
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ManagedCredentialRegistry {
    pub format_version: u16,
    pub updated_at_utc: String,
    pub credentials: Vec<ManagedStoreCredentialRecord>,
    #[serde(default)]
    pub audit: Vec<ManagedCredentialAuditEvent>,
}

impl ManagedCredentialRegistry {
    pub fn empty(updated_at_utc: impl Into<String>) -> Self {
        Self {
            format_version: 1,
            updated_at_utc: updated_at_utc.into(),
            credentials: Vec::new(),
            audit: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ManagedStoreCredentialRecord {
    pub store_id: StoreId,
    pub bucket_name: String,
    pub credential_reference: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub issued_at_utc: String,
    pub rotated_at_utc: Option<String>,
    pub revision: u32,
}

impl ManagedStoreCredentialRecord {
    fn from_credential(
        credential: StoreServiceCredential,
        issued_at_utc: impl Into<String>,
        revision: u32,
    ) -> Self {
        Self {
            store_id: credential.store_id,
            bucket_name: credential.bucket_name,
            credential_reference: credential.credential_reference,
            access_key_id: credential.access_key_id,
            secret_access_key: credential.secret_access_key.expose_secret().to_string(),
            issued_at_utc: issued_at_utc.into(),
            rotated_at_utc: None,
            revision,
        }
    }

    fn to_credential(&self) -> StoreServiceCredential {
        StoreServiceCredential {
            store_id: self.store_id.clone(),
            bucket_name: self.bucket_name.clone(),
            credential_reference: self.credential_reference.clone(),
            access_key_id: self.access_key_id.clone(),
            secret_access_key: SecretAccessKey(self.secret_access_key.clone()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ManagedCredentialAuditEvent {
    pub at_utc: String,
    pub store_id: StoreId,
    pub bucket_name: String,
    pub action: ManagedCredentialAuditAction,
    pub access_key_id: String,
    pub previous_access_key_id: Option<String>,
    pub credential_reference: String,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagedCredentialAuditAction {
    Issued,
    Reused,
    Rotated,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedCredentialResolution {
    pub registry_path: PathBuf,
    pub credentials: Vec<StoreServiceCredential>,
    pub issued: usize,
    pub reused: usize,
    pub rotated: usize,
}

pub trait CredentialEntropy {
    fn fill(&mut self, bytes: &mut [u8]) -> Result<(), ObjectServiceError>;
}

#[derive(Debug, Default)]
pub struct SystemCredentialEntropy;

impl CredentialEntropy for SystemCredentialEntropy {
    fn fill(&mut self, bytes: &mut [u8]) -> Result<(), ObjectServiceError> {
        let mut device = File::open("/dev/urandom").map_err(|error| {
            ObjectServiceError::CommandFailed(format!("open random source: {error}"))
        })?;
        device.read_exact(bytes).map_err(|error| {
            ObjectServiceError::CommandFailed(format!("read random source: {error}"))
        })
    }
}

pub fn generate_per_store_credentials(
    requests: &[StoreCredentialRequest],
    entropy: &mut impl CredentialEntropy,
) -> Result<Vec<StoreServiceCredential>, ObjectServiceError> {
    validate_requests(requests)?;

    requests
        .iter()
        .map(|request| generate_store_credential(request, entropy))
        .collect()
}

pub fn default_garage_credential_registry_path() -> PathBuf {
    std::env::var_os(GARAGE_CREDENTIAL_REGISTRY_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_GARAGE_CREDENTIAL_REGISTRY_PATH))
}

pub fn resolve_managed_store_credentials(
    registry_path: impl AsRef<Path>,
    requests: &[StoreCredentialRequest],
    now_utc: impl AsRef<str>,
    rotate_existing: bool,
    entropy: &mut impl CredentialEntropy,
) -> Result<ManagedCredentialResolution, ObjectServiceError> {
    validate_requests(requests)?;
    let registry_path = registry_path.as_ref();
    let now_utc = now_utc.as_ref();
    reject_blank("now_utc", now_utc)?;

    let mut registry = read_managed_credential_registry(registry_path, now_utc)?;
    let mut credentials = Vec::with_capacity(requests.len());
    let mut issued = 0;
    let mut reused = 0;
    let mut rotated = 0;

    for request in requests {
        let existing_index = registry
            .credentials
            .iter()
            .position(|record| record.store_id == request.store_id);
        match existing_index {
            Some(index) if !rotate_existing => {
                validate_record_matches_request(&registry.credentials[index], request)?;
                let record = registry.credentials[index].clone();
                credentials.push(record.to_credential());
                reused += 1;
                registry.audit.push(audit_event(
                    now_utc,
                    request,
                    ManagedCredentialAuditAction::Reused,
                    record.access_key_id,
                    None,
                    "reused persisted Garage credential",
                ));
            }
            Some(index) => {
                validate_record_matches_request(&registry.credentials[index], request)?;
                let previous_access_key_id = registry.credentials[index].access_key_id.clone();
                let revision = registry.credentials[index].revision.saturating_add(1);
                let credential = generate_store_credential(request, entropy)?;
                let record = ManagedStoreCredentialRecord::from_credential(
                    credential.clone(),
                    now_utc,
                    revision,
                );
                registry.credentials[index] = ManagedStoreCredentialRecord {
                    rotated_at_utc: Some(now_utc.to_string()),
                    ..record.clone()
                };
                credentials.push(credential);
                rotated += 1;
                registry.audit.push(audit_event(
                    now_utc,
                    request,
                    ManagedCredentialAuditAction::Rotated,
                    record.access_key_id,
                    Some(previous_access_key_id),
                    "rotated persisted Garage credential",
                ));
            }
            None => {
                let credential = generate_store_credential(request, entropy)?;
                let record =
                    ManagedStoreCredentialRecord::from_credential(credential.clone(), now_utc, 1);
                registry.credentials.push(record.clone());
                credentials.push(credential);
                issued += 1;
                registry.audit.push(audit_event(
                    now_utc,
                    request,
                    ManagedCredentialAuditAction::Issued,
                    record.access_key_id,
                    None,
                    "issued persisted Garage credential",
                ));
            }
        }
    }

    registry.updated_at_utc = now_utc.to_string();
    validate_managed_credential_registry(&registry)?;
    write_managed_credential_registry(registry_path, &registry)?;

    Ok(ManagedCredentialResolution {
        registry_path: registry_path.to_path_buf(),
        credentials,
        issued,
        reused,
        rotated,
    })
}

pub fn write_credential_reference_manifest(
    path: impl AsRef<Path>,
    manifest: &CredentialReferenceManifest,
) -> Result<(), ObjectServiceError> {
    let file = File::create(path.as_ref()).map_err(|error| {
        ObjectServiceError::CommandFailed(format!("create credential reference manifest: {error}"))
    })?;
    serde_json::to_writer_pretty(file, manifest).map_err(|error| {
        ObjectServiceError::CommandFailed(format!("write credential reference manifest: {error}"))
    })
}

pub fn read_managed_credential_registry(
    path: impl AsRef<Path>,
    now_utc: impl Into<String>,
) -> Result<ManagedCredentialRegistry, ObjectServiceError> {
    let path = path.as_ref();
    match File::open(path) {
        Ok(file) => serde_json::from_reader(file).map_err(|error| {
            ObjectServiceError::InvalidConfiguration(format!(
                "read managed credential registry {}: {error}",
                path.display()
            ))
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(ManagedCredentialRegistry::empty(now_utc))
        }
        Err(error) => Err(ObjectServiceError::CommandFailed(format!(
            "open managed credential registry {}: {error}",
            path.display()
        ))),
    }
}

pub fn write_managed_credential_registry(
    path: impl AsRef<Path>,
    registry: &ManagedCredentialRegistry,
) -> Result<(), ObjectServiceError> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            ObjectServiceError::CommandFailed(format!(
                "create managed credential registry directory {}: {error}",
                parent.display()
            ))
        })?;
        restrict_credential_registry_dir(parent)?;
    }

    let file = create_private_credential_registry_file(path).map_err(|error| {
        ObjectServiceError::CommandFailed(format!(
            "create managed credential registry {}: {error}",
            path.display()
        ))
    })?;
    serde_json::to_writer_pretty(file, registry).map_err(|error| {
        ObjectServiceError::CommandFailed(format!(
            "write managed credential registry {}: {error}",
            path.display()
        ))
    })
}

fn validate_requests(requests: &[StoreCredentialRequest]) -> Result<(), ObjectServiceError> {
    if requests.is_empty() {
        return Err(ObjectServiceError::InvalidConfiguration(
            "at least one store credential request is required".to_string(),
        ));
    }

    let mut store_ids = BTreeSet::new();
    let mut bucket_names = BTreeSet::new();
    for request in requests {
        reject_blank("bucket_name", &request.bucket_name)?;
        if !store_ids.insert(request.store_id.as_str()) {
            return Err(ObjectServiceError::InvalidConfiguration(format!(
                "duplicate credential request for store: {}",
                request.store_id
            )));
        }
        if !bucket_names.insert(request.bucket_name.as_str()) {
            return Err(ObjectServiceError::InvalidConfiguration(format!(
                "duplicate credential request for bucket: {}",
                request.bucket_name
            )));
        }
    }

    Ok(())
}

fn validate_record_matches_request(
    record: &ManagedStoreCredentialRecord,
    request: &StoreCredentialRequest,
) -> Result<(), ObjectServiceError> {
    if record.bucket_name != request.bucket_name {
        return Err(ObjectServiceError::InvalidConfiguration(format!(
            "stored Garage credential for store {} is bound to bucket {}, but registry requests {}",
            request.store_id, record.bucket_name, request.bucket_name
        )));
    }
    if record.credential_reference != credential_reference_for_store(&request.store_id) {
        return Err(ObjectServiceError::InvalidConfiguration(format!(
            "stored Garage credential reference for store {} is inconsistent",
            request.store_id
        )));
    }
    reject_blank("access_key_id", &record.access_key_id)?;
    reject_blank("secret_access_key", &record.secret_access_key)?;
    Ok(())
}

fn validate_managed_credential_registry(
    registry: &ManagedCredentialRegistry,
) -> Result<(), ObjectServiceError> {
    if registry.format_version != 1 {
        return Err(ObjectServiceError::InvalidConfiguration(format!(
            "unsupported managed credential registry version: {}",
            registry.format_version
        )));
    }
    reject_blank("updated_at_utc", &registry.updated_at_utc)?;
    let mut store_ids = BTreeSet::new();
    let mut bucket_names = BTreeSet::new();
    let mut access_key_ids = BTreeSet::new();
    for record in &registry.credentials {
        reject_blank("bucket_name", &record.bucket_name)?;
        reject_blank("credential_reference", &record.credential_reference)?;
        reject_blank("access_key_id", &record.access_key_id)?;
        reject_blank("secret_access_key", &record.secret_access_key)?;
        reject_blank("issued_at_utc", &record.issued_at_utc)?;
        if record.revision == 0 {
            return Err(ObjectServiceError::InvalidConfiguration(format!(
                "managed credential revision must be greater than zero for store {}",
                record.store_id
            )));
        }
        if !store_ids.insert(record.store_id.as_str()) {
            return Err(ObjectServiceError::InvalidConfiguration(format!(
                "duplicate managed credential for store: {}",
                record.store_id
            )));
        }
        if !bucket_names.insert(record.bucket_name.as_str()) {
            return Err(ObjectServiceError::InvalidConfiguration(format!(
                "duplicate managed credential bucket: {}",
                record.bucket_name
            )));
        }
        if !access_key_ids.insert(record.access_key_id.as_str()) {
            return Err(ObjectServiceError::InvalidConfiguration(format!(
                "duplicate managed credential access key id: {}",
                record.access_key_id
            )));
        }
    }
    Ok(())
}

fn audit_event(
    now_utc: &str,
    request: &StoreCredentialRequest,
    action: ManagedCredentialAuditAction,
    access_key_id: String,
    previous_access_key_id: Option<String>,
    message: impl Into<String>,
) -> ManagedCredentialAuditEvent {
    ManagedCredentialAuditEvent {
        at_utc: now_utc.to_string(),
        store_id: request.store_id.clone(),
        bucket_name: request.bucket_name.clone(),
        action,
        access_key_id,
        previous_access_key_id,
        credential_reference: credential_reference_for_store(&request.store_id),
        message: message.into(),
    }
}

fn generate_store_credential(
    request: &StoreCredentialRequest,
    entropy: &mut impl CredentialEntropy,
) -> Result<StoreServiceCredential, ObjectServiceError> {
    let mut access_key_bytes = [0_u8; ACCESS_KEY_RANDOM_BYTES];
    let mut secret_key_bytes = [0_u8; SECRET_KEY_RANDOM_BYTES];
    entropy.fill(&mut access_key_bytes)?;
    entropy.fill(&mut secret_key_bytes)?;

    Ok(StoreServiceCredential {
        store_id: request.store_id.clone(),
        bucket_name: request.bucket_name.clone(),
        credential_reference: credential_reference_for_store(&request.store_id),
        access_key_id: format!("DOS{}", hex_upper(&access_key_bytes)),
        secret_access_key: SecretAccessKey(hex_lower(&secret_key_bytes)),
    })
}

pub fn credential_reference_for_store(store_id: &StoreId) -> String {
    format!("secret://dasobjectstore/stores/{store_id}/s3")
}

fn reject_blank(field: &str, value: &str) -> Result<(), ObjectServiceError> {
    if value.trim().is_empty() {
        return Err(ObjectServiceError::InvalidConfiguration(format!(
            "{field} must not be blank"
        )));
    }

    Ok(())
}

fn create_private_credential_registry_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);

    #[cfg(unix)]
    options.mode(CREDENTIAL_REGISTRY_FILE_MODE);

    options.open(path)
}

fn restrict_credential_registry_dir(path: &Path) -> Result<(), ObjectServiceError> {
    #[cfg(unix)]
    {
        fs::set_permissions(
            path,
            fs::Permissions::from_mode(CREDENTIAL_REGISTRY_DIR_MODE),
        )
        .map_err(|error| {
            ObjectServiceError::CommandFailed(format!(
                "restrict managed credential registry directory {}: {error}",
                path.display()
            ))
        })?;
    }
    Ok(())
}

fn hex_lower(bytes: &[u8]) -> String {
    const TABLE: &[u8; 16] = b"0123456789abcdef";
    hex_encode(bytes, TABLE)
}

fn hex_upper(bytes: &[u8]) -> String {
    const TABLE: &[u8; 16] = b"0123456789ABCDEF";
    hex_encode(bytes, TABLE)
}

fn hex_encode(bytes: &[u8], table: &[u8; 16]) -> String {
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(table[(byte >> 4) as usize] as char);
        encoded.push(table[(byte & 0x0f) as usize] as char);
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::{
        generate_per_store_credentials, read_managed_credential_registry,
        resolve_managed_store_credentials, write_credential_reference_manifest, CredentialEntropy,
        CredentialReferenceManifest, ManagedCredentialAuditAction, ObjectServiceError,
        StoreCredentialReference, StoreCredentialRequest,
    };
    use dasobjectstore_core::ids::StoreId;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn generates_distinct_per_store_credentials() {
        let mut entropy = FixedEntropy::new();
        let credentials = generate_per_store_credentials(
            &[
                request("generated", "generated-data"),
                request("critical", "critical-metadata"),
            ],
            &mut entropy,
        )
        .expect("credentials generated");

        assert_eq!(credentials.len(), 2);
        assert_eq!(credentials[0].store_id.as_str(), "generated");
        assert_eq!(credentials[0].bucket_name, "generated-data");
        assert_eq!(
            credentials[0].credential_reference,
            "secret://dasobjectstore/stores/generated/s3"
        );
        assert!(credentials[0].access_key_id.starts_with("DOS"));
        assert_ne!(credentials[0].access_key_id, credentials[1].access_key_id);
        assert_ne!(
            credentials[0].secret_access_key.expose_secret(),
            credentials[1].secret_access_key.expose_secret()
        );
    }

    #[test]
    fn debug_output_redacts_secret_access_key() {
        let mut entropy = FixedEntropy::new();
        let credential =
            generate_per_store_credentials(&[request("generated", "generated-data")], &mut entropy)
                .expect("credentials generated")
                .remove(0);

        let debug = format!("{credential:?}");

        assert!(debug.contains("SecretAccessKey(REDACTED)"));
        assert!(!debug.contains(credential.secret_access_key.expose_secret()));
    }

    #[test]
    fn credential_reference_manifest_excludes_secret_material() {
        let mut entropy = FixedEntropy::new();
        let credentials =
            generate_per_store_credentials(&[request("generated", "generated-data")], &mut entropy)
                .expect("credentials generated");
        let secret = credentials[0].secret_access_key.expose_secret().to_string();

        let manifest =
            CredentialReferenceManifest::from_credentials("2026-01-01T00:00:00Z", &credentials);
        let encoded = serde_json::to_string(&manifest).expect("manifest serializes");

        assert!(encoded.contains(&credentials[0].credential_reference));
        assert!(encoded.contains(&credentials[0].access_key_id));
        assert!(!encoded.contains(&secret));
    }

    #[test]
    fn credential_reference_round_trips_without_secret() {
        let mut entropy = FixedEntropy::new();
        let credential =
            generate_per_store_credentials(&[request("generated", "generated-data")], &mut entropy)
                .expect("credentials generated")
                .remove(0);

        let reference = StoreCredentialReference::from_credential(&credential);
        let encoded = serde_json::to_string(&reference).expect("reference serializes");
        let decoded: StoreCredentialReference =
            serde_json::from_str(&encoded).expect("reference deserializes");

        assert_eq!(decoded, reference);
        assert!(!encoded.contains(credential.secret_access_key.expose_secret()));
    }

    #[test]
    fn writes_credential_reference_manifest_without_secret() {
        let mut entropy = FixedEntropy::new();
        let credentials =
            generate_per_store_credentials(&[request("generated", "generated-data")], &mut entropy)
                .expect("credentials generated");
        let secret = credentials[0].secret_access_key.expose_secret().to_string();
        let manifest =
            CredentialReferenceManifest::from_credentials("2026-01-01T00:00:00Z", &credentials);
        let path = temp_manifest_path("credential-reference-manifest");

        write_credential_reference_manifest(&path, &manifest).expect("manifest writes");
        let written = fs::read_to_string(&path).expect("manifest reads");
        fs::remove_file(&path).expect("temp manifest removed");

        assert!(written.contains("secret://dasobjectstore/stores/generated/s3"));
        assert!(!written.contains(&secret));
    }

    #[test]
    fn managed_registry_persists_and_reuses_credentials() {
        let path = temp_registry_path("managed-credentials-reuse");
        let first = resolve_managed_store_credentials(
            &path,
            &[request("generated", "generated-data")],
            "2026-07-09T10:00:00Z",
            false,
            &mut FixedEntropy::new(),
        )
        .expect("credential issued");
        let first_access_key = first.credentials[0].access_key_id.clone();
        let first_secret = first.credentials[0]
            .secret_access_key
            .expose_secret()
            .to_string();

        let second = resolve_managed_store_credentials(
            &path,
            &[request("generated", "generated-data")],
            "2026-07-09T10:05:00Z",
            false,
            &mut FixedEntropy::new(),
        )
        .expect("credential reused");

        assert_eq!(first.issued, 1);
        assert_eq!(second.reused, 1);
        assert_eq!(second.credentials[0].access_key_id, first_access_key);
        assert_eq!(
            second.credentials[0].secret_access_key.expose_secret(),
            first_secret
        );
        let registry =
            read_managed_credential_registry(&path, "unused").expect("registry reads back");
        fs::remove_file(&path).expect("temp registry removed");
        assert_eq!(registry.credentials.len(), 1);
        assert_eq!(registry.audit.len(), 2);
        assert_eq!(
            registry.audit[0].action,
            ManagedCredentialAuditAction::Issued
        );
        assert_eq!(
            registry.audit[1].action,
            ManagedCredentialAuditAction::Reused
        );
    }

    #[test]
    fn managed_registry_rotates_credentials_only_when_requested() {
        let path = temp_registry_path("managed-credentials-rotate");
        let mut entropy = FixedEntropy::new();
        let first = resolve_managed_store_credentials(
            &path,
            &[request("generated", "generated-data")],
            "2026-07-09T10:00:00Z",
            false,
            &mut entropy,
        )
        .expect("credential issued");
        let first_access_key = first.credentials[0].access_key_id.clone();

        let rotated = resolve_managed_store_credentials(
            &path,
            &[request("generated", "generated-data")],
            "2026-07-09T10:10:00Z",
            true,
            &mut entropy,
        )
        .expect("credential rotated");

        assert_eq!(rotated.rotated, 1);
        assert_ne!(rotated.credentials[0].access_key_id, first_access_key);
        let registry =
            read_managed_credential_registry(&path, "unused").expect("registry reads back");
        fs::remove_file(&path).expect("temp registry removed");
        assert_eq!(registry.credentials[0].revision, 2);
        assert_eq!(registry.audit.len(), 2);
        assert_eq!(
            registry.audit[1].action,
            ManagedCredentialAuditAction::Rotated
        );
        assert_eq!(
            registry.audit[1].previous_access_key_id.as_deref(),
            Some(first_access_key.as_str())
        );
    }

    #[test]
    fn rejects_duplicate_store_requests() {
        let mut entropy = FixedEntropy::new();

        let err = generate_per_store_credentials(
            &[
                request("generated", "generated-data"),
                request("generated", "generated-data-alt"),
            ],
            &mut entropy,
        )
        .expect_err("duplicate store rejected");

        assert!(err
            .to_string()
            .contains("duplicate credential request for store"));
    }

    #[test]
    fn rejects_duplicate_bucket_requests() {
        let mut entropy = FixedEntropy::new();

        let err = generate_per_store_credentials(
            &[
                request("generated", "shared"),
                request("critical", "shared"),
            ],
            &mut entropy,
        )
        .expect_err("duplicate bucket rejected");

        assert!(err
            .to_string()
            .contains("duplicate credential request for bucket"));
    }

    fn request(store_id: &str, bucket_name: &str) -> StoreCredentialRequest {
        StoreCredentialRequest {
            store_id: StoreId::new(store_id).expect("store id"),
            bucket_name: bucket_name.to_string(),
        }
    }

    fn temp_manifest_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "dasobjectstore-{name}-{}-{}.json",
            std::process::id(),
            unique_suffix()
        ))
    }

    fn temp_registry_path(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!(
                "dasobjectstore-{name}-{}-{}",
                std::process::id(),
                unique_suffix()
            ))
            .join("registry.json")
    }

    fn unique_suffix() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time after epoch")
            .as_nanos()
    }

    struct FixedEntropy {
        next: u8,
    }

    impl FixedEntropy {
        fn new() -> Self {
            Self { next: 1 }
        }
    }

    impl CredentialEntropy for FixedEntropy {
        fn fill(&mut self, bytes: &mut [u8]) -> Result<(), ObjectServiceError> {
            for byte in bytes {
                *byte = self.next;
                self.next = self.next.wrapping_add(1);
            }
            Ok(())
        }
    }
}
