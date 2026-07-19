//! Durable transaction journal for direct S3 ingress onto managed SSD.
//!
//! The journal deliberately stores an S3 key as metadata only. Neither the
//! bucket nor key participates in a filesystem path. A stable digest of the
//! complete daemon-authorized identity addresses a store-private transaction
//! directory, making retries deterministic without trusting client paths.

use dasobjectstore_core::ids::StoreId;
use dasobjectstore_core::{
    deployment::DeploymentProfile,
    manifest::{BackendReference, ObjectStoreManifest},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::{self, Display};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const JOURNAL_SCHEMA: &str = "dasobjectstore.direct_s3_ingress.journal.v1";
const PRIVATE_NAMESPACE: &str = ".dasobjectstore";
const STORE_NAMESPACE: &str = "stores";
const INGRESS_NAMESPACE: &str = "direct-s3";
const UPLOAD_NAMESPACE: &str = "uploads";
const PROFILE_NAMESPACE: &str = "profile";
const JOURNAL_FILE: &str = "journal.json";
const STAGED_FILE: &str = "payload.part";

static TEMPORARY_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DirectS3IngressIdentity {
    pub store_id: StoreId,
    pub credential_scope_id: String,
    pub bucket: String,
    pub key: String,
    pub object_version: u64,
    pub operation_id: String,
    pub expected_size_bytes: u64,
}

impl DirectS3IngressIdentity {
    pub fn validate(&self) -> Result<(), DirectS3IngressJournalError> {
        for (field, value) in [
            ("credential_scope_id", self.credential_scope_id.as_str()),
            ("bucket", self.bucket.as_str()),
            ("key", self.key.as_str()),
            ("operation_id", self.operation_id.as_str()),
        ] {
            if value.trim().is_empty() || value.contains('\0') {
                return Err(DirectS3IngressJournalError::InvalidIdentity(field));
            }
        }
        if self.object_version == 0 {
            return Err(DirectS3IngressJournalError::InvalidIdentity(
                "object_version",
            ));
        }
        Ok(())
    }

    fn transaction_id(&self) -> Result<String, DirectS3IngressJournalError> {
        self.validate()?;
        let encoded = serde_json::to_vec(self)
            .map_err(|error| DirectS3IngressJournalError::Encode(error.to_string()))?;
        Ok(format!("{:x}", Sha256::digest(encoded)))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectS3IngressState {
    Receiving,
    Verified,
    Published,
    Accepted,
    Aborted,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct DirectS3IngressManifest {
    schema: String,
    transaction_id: String,
    identity: DirectS3IngressIdentity,
    state: DirectS3IngressState,
    staged_file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    received_size_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    terminal_reason: Option<String>,
}

/// One daemon-owned ingress transaction rooted below a store-private managed
/// SSD namespace. Opening the same authorized identity is idempotent; any
/// corrupt or mismatched journal fails closed.
pub struct DirectS3IngressJournal {
    directory: PathBuf,
    manifest: DirectS3IngressManifest,
}

impl DirectS3IngressJournal {
    pub fn open(
        managed_ssd_root: impl AsRef<Path>,
        identity: DirectS3IngressIdentity,
    ) -> Result<Self, DirectS3IngressJournalError> {
        identity.validate()?;
        let transaction_id = identity.transaction_id()?;
        let root = canonical_managed_root(managed_ssd_root.as_ref())?;
        let store_root = direct_s3_store_private_root(&root, &identity.store_id)?;
        let uploads = store_root.join(UPLOAD_NAMESPACE);
        create_private_directory_chain(&root, &uploads)?;
        let directory = uploads.join(&transaction_id);
        create_private_directory(&directory)?;
        sync_directory(&uploads)?;

        let journal_path = directory.join(JOURNAL_FILE);
        let manifest = if journal_path.exists() {
            let file = File::open(&journal_path).map_err(io_error)?;
            let manifest: DirectS3IngressManifest = serde_json::from_reader(file)
                .map_err(|error| DirectS3IngressJournalError::Decode(error.to_string()))?;
            validate_manifest(&manifest)?;
            if manifest.transaction_id != transaction_id || manifest.identity != identity {
                return Err(DirectS3IngressJournalError::IdentityMismatch);
            }
            manifest
        } else {
            let manifest = DirectS3IngressManifest {
                schema: JOURNAL_SCHEMA.to_string(),
                transaction_id,
                identity,
                state: DirectS3IngressState::Receiving,
                staged_file: STAGED_FILE.to_string(),
                received_size_bytes: None,
                sha256: None,
                terminal_reason: None,
            };
            persist_manifest(&directory, &manifest)?;
            manifest
        };
        Ok(Self {
            directory,
            manifest,
        })
    }

    pub fn transaction_id(&self) -> &str {
        &self.manifest.transaction_id
    }

    pub fn identity(&self) -> &DirectS3IngressIdentity {
        &self.manifest.identity
    }

    pub fn state(&self) -> DirectS3IngressState {
        self.manifest.state
    }

    /// The path is daemon-private and is never suitable for a transport
    /// response. Callers should open it with create-new semantics while the
    /// journal is in `receiving` state.
    #[cfg(test)]
    pub(crate) fn staged_file_path(&self) -> PathBuf {
        self.directory.join(STAGED_FILE)
    }

    pub fn mark_verified(
        &mut self,
        received_size_bytes: u64,
        sha256: &str,
    ) -> Result<(), DirectS3IngressJournalError> {
        validate_sha256(sha256)?;
        if received_size_bytes != self.manifest.identity.expected_size_bytes {
            return Err(DirectS3IngressJournalError::SizeMismatch {
                expected: self.manifest.identity.expected_size_bytes,
                actual: received_size_bytes,
            });
        }
        match self.manifest.state {
            DirectS3IngressState::Receiving => {
                self.manifest.state = DirectS3IngressState::Verified;
                self.manifest.received_size_bytes = Some(received_size_bytes);
                self.manifest.sha256 = Some(sha256.to_ascii_lowercase());
                self.persist()
            }
            DirectS3IngressState::Verified
                if self.manifest.received_size_bytes == Some(received_size_bytes)
                    && self
                        .manifest
                        .sha256
                        .as_deref()
                        .is_some_and(|existing| existing.eq_ignore_ascii_case(sha256)) =>
            {
                Ok(())
            }
            _ => Err(DirectS3IngressJournalError::InvalidTransition {
                from: self.manifest.state,
                to: DirectS3IngressState::Verified,
            }),
        }
    }

    pub fn mark_published(&mut self) -> Result<(), DirectS3IngressJournalError> {
        self.transition(
            DirectS3IngressState::Verified,
            DirectS3IngressState::Published,
        )
    }

    pub fn mark_accepted(&mut self) -> Result<(), DirectS3IngressJournalError> {
        self.transition(
            DirectS3IngressState::Published,
            DirectS3IngressState::Accepted,
        )
    }

    pub fn mark_aborted(&mut self, reason: &str) -> Result<(), DirectS3IngressJournalError> {
        if reason.trim().is_empty() || reason.contains('\0') {
            return Err(DirectS3IngressJournalError::InvalidIdentity(
                "terminal_reason",
            ));
        }
        match self.manifest.state {
            DirectS3IngressState::Receiving | DirectS3IngressState::Verified => {
                self.manifest.state = DirectS3IngressState::Aborted;
                self.manifest.terminal_reason = Some(reason.to_string());
                self.persist()
            }
            DirectS3IngressState::Aborted
                if self.manifest.terminal_reason.as_deref() == Some(reason) =>
            {
                Ok(())
            }
            _ => Err(DirectS3IngressJournalError::InvalidTransition {
                from: self.manifest.state,
                to: DirectS3IngressState::Aborted,
            }),
        }
    }

    fn transition(
        &mut self,
        expected: DirectS3IngressState,
        next: DirectS3IngressState,
    ) -> Result<(), DirectS3IngressJournalError> {
        if self.manifest.state == next {
            return Ok(());
        }
        if self.manifest.state != expected {
            return Err(DirectS3IngressJournalError::InvalidTransition {
                from: self.manifest.state,
                to: next,
            });
        }
        self.manifest.state = next;
        self.persist()
    }

    fn persist(&self) -> Result<(), DirectS3IngressJournalError> {
        persist_manifest(&self.directory, &self.manifest)
    }
}

fn canonical_managed_root(path: &Path) -> Result<PathBuf, DirectS3IngressJournalError> {
    if !path.is_absolute() {
        return Err(DirectS3IngressJournalError::UnsafePath(path.to_path_buf()));
    }
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(DirectS3IngressJournalError::UnsafePath(path.to_path_buf()));
    }
    fs::canonicalize(path).map_err(io_error)
}

fn store_namespace_component(store_id: &StoreId) -> String {
    format!("{:x}", Sha256::digest(store_id.as_str().as_bytes()))
}

/// Resolve and create the daemon-private direct-ingress namespace for one
/// ObjectStore on an authoritative managed SSD root. The logical store name is
/// represented by a digest so it can never become a client-controlled path
/// component.
pub fn direct_s3_store_private_root(
    managed_ssd_root: impl AsRef<Path>,
    store_id: &StoreId,
) -> Result<PathBuf, DirectS3IngressJournalError> {
    let root = canonical_managed_root(managed_ssd_root.as_ref())?;
    let store_root = root
        .join(PRIVATE_NAMESPACE)
        .join(STORE_NAMESPACE)
        .join(store_namespace_component(store_id))
        .join(INGRESS_NAMESPACE);
    create_private_directory_chain(&root, &store_root)?;
    Ok(store_root)
}

/// Resolve the store-private profile backend used for direct S3 payloads.
/// This is intentionally distinct from the transaction journal so catalogue
/// scans can never interpret resumable or aborted upload bytes as objects.
pub fn direct_s3_profile_backend_root(
    managed_ssd_root: impl AsRef<Path>,
    store_id: &StoreId,
) -> Result<PathBuf, DirectS3IngressJournalError> {
    let store_root = direct_s3_store_private_root(managed_ssd_root, store_id)?;
    let profile_root = store_root.join(PROFILE_NAMESPACE);
    create_private_directory(&profile_root)?;
    sync_directory(&store_root)?;
    Ok(profile_root)
}

/// Adapt a portable profile binding to the private profile backend used by
/// the provider-stream/S3 surface. Folder profiles retain their established
/// root for compatibility. Drive and appliance profiles receive a
/// store-private folder-shaped implementation detail below their authoritative
/// managed SSD root; the persisted portable binding is never rewritten.
pub fn direct_s3_profile_backend(
    binding: &super::BackendProfileBinding,
) -> Result<(PathBuf, ObjectStoreManifest), DirectS3IngressJournalError> {
    if binding.manifest.deployment_profile == DeploymentProfile::Folder {
        return Ok((binding.backend_root.clone(), binding.manifest.clone()));
    }
    let managed_ssd_root = binding
        .ssd_staging_root
        .as_deref()
        .unwrap_or(&binding.backend_root);
    let root = direct_s3_profile_backend_root(managed_ssd_root, &binding.manifest.store_id)?;
    let mut manifest = binding.manifest.clone();
    manifest.deployment_profile = DeploymentProfile::Folder;
    manifest.backend = BackendReference::Folder {
        root_identity: format!("direct-s3:{}", manifest.store_id.as_str()),
    };
    Ok((root, manifest))
}

fn create_private_directory_chain(
    root: &Path,
    directory: &Path,
) -> Result<(), DirectS3IngressJournalError> {
    let relative = directory
        .strip_prefix(root)
        .map_err(|_| DirectS3IngressJournalError::UnsafePath(directory.to_path_buf()))?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let std::path::Component::Normal(component) = component else {
            return Err(DirectS3IngressJournalError::UnsafePath(
                directory.to_path_buf(),
            ));
        };
        current.push(component);
        create_private_directory(&current)?;
    }
    Ok(())
}

fn create_private_directory(path: &Path) -> Result<(), DirectS3IngressJournalError> {
    match fs::create_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(io_error(error)),
    }
    let metadata = fs::symlink_metadata(path).map_err(io_error)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(DirectS3IngressJournalError::UnsafePath(path.to_path_buf()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(io_error)?;
    }
    Ok(())
}

fn persist_manifest(
    directory: &Path,
    manifest: &DirectS3IngressManifest,
) -> Result<(), DirectS3IngressJournalError> {
    validate_manifest(manifest)?;
    let temporary = directory.join(format!(
        ".{JOURNAL_FILE}.tmp-{}-{}",
        std::process::id(),
        TEMPORARY_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let encoded = serde_json::to_vec_pretty(manifest)
        .map_err(|error| DirectS3IngressJournalError::Encode(error.to_string()))?;
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary).map_err(io_error)?;
    if let Err(error) = file.write_all(&encoded).and_then(|()| file.sync_all()) {
        let _ = fs::remove_file(&temporary);
        return Err(io_error(error));
    }
    drop(file);
    if let Err(error) = fs::rename(&temporary, directory.join(JOURNAL_FILE)) {
        let _ = fs::remove_file(&temporary);
        return Err(io_error(error));
    }
    sync_directory(directory)
}

fn validate_manifest(
    manifest: &DirectS3IngressManifest,
) -> Result<(), DirectS3IngressJournalError> {
    manifest.identity.validate()?;
    if manifest.schema != JOURNAL_SCHEMA
        || manifest.transaction_id != manifest.identity.transaction_id()?
        || manifest.staged_file != STAGED_FILE
    {
        return Err(DirectS3IngressJournalError::InvalidManifest);
    }
    match manifest.state {
        DirectS3IngressState::Receiving => {
            if manifest.received_size_bytes.is_some()
                || manifest.sha256.is_some()
                || manifest.terminal_reason.is_some()
            {
                return Err(DirectS3IngressJournalError::InvalidManifest);
            }
        }
        DirectS3IngressState::Verified
        | DirectS3IngressState::Published
        | DirectS3IngressState::Accepted => {
            if manifest.received_size_bytes != Some(manifest.identity.expected_size_bytes)
                || manifest
                    .sha256
                    .as_deref()
                    .map(validate_sha256)
                    .transpose()?
                    .is_none()
                || manifest.terminal_reason.is_some()
            {
                return Err(DirectS3IngressJournalError::InvalidManifest);
            }
        }
        DirectS3IngressState::Aborted => {
            if manifest
                .terminal_reason
                .as_deref()
                .is_none_or(str::is_empty)
            {
                return Err(DirectS3IngressJournalError::InvalidManifest);
            }
        }
    }
    Ok(())
}

fn validate_sha256(value: &str) -> Result<(), DirectS3IngressJournalError> {
    if value.len() != 71
        || !value.starts_with("sha256:")
        || !value[7..].bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(DirectS3IngressJournalError::InvalidChecksum);
    }
    Ok(())
}

fn sync_directory(path: &Path) -> Result<(), DirectS3IngressJournalError> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(io_error)
}

fn io_error(error: std::io::Error) -> DirectS3IngressJournalError {
    DirectS3IngressJournalError::Io(error.to_string())
}

#[derive(Debug)]
pub enum DirectS3IngressJournalError {
    Io(String),
    Encode(String),
    Decode(String),
    UnsafePath(PathBuf),
    InvalidIdentity(&'static str),
    IdentityMismatch,
    InvalidManifest,
    InvalidChecksum,
    SizeMismatch {
        expected: u64,
        actual: u64,
    },
    InvalidTransition {
        from: DirectS3IngressState,
        to: DirectS3IngressState,
    },
}

impl Display for DirectS3IngressJournalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(message) => {
                write!(formatter, "direct S3 ingress journal IO failed: {message}")
            }
            Self::Encode(message) => write!(
                formatter,
                "direct S3 ingress journal encode failed: {message}"
            ),
            Self::Decode(message) => write!(
                formatter,
                "direct S3 ingress journal decode failed: {message}"
            ),
            Self::UnsafePath(path) => write!(
                formatter,
                "unsafe direct S3 ingress path: {}",
                path.display()
            ),
            Self::InvalidIdentity(field) => write!(
                formatter,
                "invalid direct S3 ingress identity field {field}"
            ),
            Self::IdentityMismatch => {
                formatter.write_str("direct S3 ingress journal identity mismatch")
            }
            Self::InvalidManifest => {
                formatter.write_str("invalid direct S3 ingress journal manifest")
            }
            Self::InvalidChecksum => formatter
                .write_str("direct S3 ingress checksum must be sha256:<64 hexadecimal characters>"),
            Self::SizeMismatch { expected, actual } => write!(
                formatter,
                "direct S3 ingress received {actual} bytes, expected {expected}"
            ),
            Self::InvalidTransition { from, to } => write!(
                formatter,
                "invalid direct S3 ingress transition from {from:?} to {to:?}"
            ),
        }
    }
}

impl std::error::Error for DirectS3IngressJournalError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(label: &str) -> PathBuf {
        let root = std::env::var_os("DASOBJECTSTORE_CODEX_VALIDATION_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("dasobjectstore-codex-validation"))
            .join(format!(
                "direct-s3-journal-{label}-{}-{}",
                std::process::id(),
                TEMPORARY_COUNTER.fetch_add(1, Ordering::Relaxed)
            ));
        fs::create_dir_all(&root).expect("validation root");
        root
    }

    fn identity(key: &str) -> DirectS3IngressIdentity {
        DirectS3IngressIdentity {
            store_id: StoreId::new("epic_collection").expect("store"),
            credential_scope_id: "scope-epic-write".to_string(),
            bucket: "dos-epic-collection".to_string(),
            key: key.to_string(),
            object_version: 1,
            operation_id: "upload-1".to_string(),
            expected_size_bytes: 5,
        }
    }

    fn checksum() -> &'static str {
        "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
    }

    #[test]
    fn progresses_durably_and_reopens_idempotently() {
        let root = root("progress");
        let identity = identity("reads/../literal-key-is-metadata.fastq");
        let mut journal = DirectS3IngressJournal::open(&root, identity.clone()).expect("open");
        assert_eq!(journal.state(), DirectS3IngressState::Receiving);
        assert!(journal
            .staged_file_path()
            .starts_with(fs::canonicalize(&root).expect("canonical root")));
        assert!(!journal
            .staged_file_path()
            .to_string_lossy()
            .contains("reads"));
        journal.mark_verified(5, checksum()).expect("verified");
        journal.mark_published().expect("published");
        journal.mark_accepted().expect("accepted");

        let mut reopened = DirectS3IngressJournal::open(&root, identity).expect("reopen");
        assert_eq!(reopened.state(), DirectS3IngressState::Accepted);
        reopened.mark_accepted().expect("accepted replay");
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn identity_selects_a_store_private_digest_path() {
        let root = root("paths");
        let private_root =
            direct_s3_store_private_root(&root, &StoreId::new("epic_collection").expect("store"))
                .expect("private root");
        let first = DirectS3IngressJournal::open(&root, identity("a/file")).expect("first");
        let second = DirectS3IngressJournal::open(&root, identity("b/file")).expect("second");
        assert_ne!(first.transaction_id(), second.transaction_id());
        let path = first.staged_file_path();
        assert!(path.starts_with(private_root));
        assert!(path.to_string_lossy().contains("/.dasobjectstore/stores/"));
        assert!(!path.to_string_lossy().contains("epic_collection"));
        assert!(!path.to_string_lossy().contains("a/file"));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn rejects_invalid_transitions_size_and_conflicting_verification() {
        let root = root("transitions");
        let mut journal = DirectS3IngressJournal::open(&root, identity("object")).expect("open");
        assert!(matches!(
            journal.mark_published(),
            Err(DirectS3IngressJournalError::InvalidTransition { .. })
        ));
        assert!(matches!(
            journal.mark_verified(4, checksum()),
            Err(DirectS3IngressJournalError::SizeMismatch { .. })
        ));
        journal.mark_verified(5, checksum()).expect("verified");
        assert!(matches!(
            journal.mark_verified(5, &format!("sha256:{}", "0".repeat(64))),
            Err(DirectS3IngressJournalError::InvalidTransition { .. })
        ));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn abort_is_terminal_and_idempotent_for_the_same_reason() {
        let root = root("abort");
        let identity = identity("object");
        let mut journal = DirectS3IngressJournal::open(&root, identity.clone()).expect("open");
        journal.mark_aborted("client disconnected").expect("abort");
        journal
            .mark_aborted("client disconnected")
            .expect("abort replay");
        assert!(journal.mark_verified(5, checksum()).is_err());
        let reopened = DirectS3IngressJournal::open(&root, identity).expect("reopen");
        assert_eq!(reopened.state(), DirectS3IngressState::Aborted);
        fs::remove_dir_all(root).ok();
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_private_namespace() {
        use std::os::unix::fs::symlink;
        let managed_root = root("symlink");
        let outside = root("outside");
        symlink(&outside, managed_root.join(PRIVATE_NAMESPACE)).expect("symlink");
        assert!(matches!(
            DirectS3IngressJournal::open(&managed_root, identity("object")),
            Err(DirectS3IngressJournalError::UnsafePath(_))
        ));
        fs::remove_dir_all(managed_root).ok();
        fs::remove_dir_all(outside).ok();
    }

    #[test]
    fn appliance_binding_resolves_to_a_store_private_profile_backend() {
        let root = root("appliance-profile");
        let binding = super::super::BackendProfileBinding {
            manifest: ObjectStoreManifest {
                schema_version: dasobjectstore_core::manifest::OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
                store_id: StoreId::new("epic_collection").expect("store"),
                deployment_profile: DeploymentProfile::Appliance,
                host_mode: dasobjectstore_core::deployment::HostMode::System,
                protection: dasobjectstore_core::protection::ProtectionPolicy::ApplianceProtected,
                backend: BackendReference::Appliance {
                    pool_id: "local-appliance".to_string(),
                },
            },
            backend_root: fs::canonicalize(&root).expect("canonical root"),
            ssd_staging_root: None,
        };

        let (profile_root, manifest) =
            direct_s3_profile_backend(&binding).expect("direct profile backend");

        assert!(profile_root.starts_with(fs::canonicalize(&root).expect("canonical root")));
        assert_eq!(manifest.store_id, binding.manifest.store_id);
        assert_eq!(manifest.deployment_profile, DeploymentProfile::Folder);
        assert!(matches!(manifest.backend, BackendReference::Folder { .. }));
        assert!(!profile_root.to_string_lossy().contains("epic_collection"));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn corrupt_or_tampered_journal_fails_closed_without_replacement() {
        let root = root("tamper");
        let identity = identity("object");
        let journal = DirectS3IngressJournal::open(&root, identity.clone()).expect("open");
        let journal_path = journal.directory.join(JOURNAL_FILE);
        let mut document: serde_json::Value =
            serde_json::from_slice(&fs::read(&journal_path).expect("read journal"))
                .expect("journal JSON");
        document["identity"]["bucket"] = serde_json::json!("attacker-bucket");
        fs::write(
            &journal_path,
            serde_json::to_vec_pretty(&document).expect("encode tampered journal"),
        )
        .expect("tamper journal");
        let tampered = fs::read(&journal_path).expect("tampered bytes");

        assert!(matches!(
            DirectS3IngressJournal::open(&root, identity),
            Err(DirectS3IngressJournalError::InvalidManifest)
                | Err(DirectS3IngressJournalError::IdentityMismatch)
        ));
        assert_eq!(
            fs::read(&journal_path).expect("journal remains"),
            tampered,
            "opening a corrupt journal must never replace it"
        );
        fs::remove_dir_all(root).ok();
    }

    #[cfg(unix)]
    #[test]
    fn journal_namespace_and_manifest_are_private_to_the_daemon_user() {
        use std::os::unix::fs::PermissionsExt;

        let root = root("permissions");
        let journal = DirectS3IngressJournal::open(&root, identity("object")).expect("open");
        assert_eq!(
            fs::metadata(&journal.directory)
                .expect("transaction metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(journal.directory.join(JOURNAL_FILE))
                .expect("journal metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        fs::remove_dir_all(root).ok();
    }
}
