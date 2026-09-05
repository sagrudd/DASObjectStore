//! Immutable catalog for daemon-admitted custody stores.
//!
//! The catalog is intentionally outside the normal mutable store registry.
//! Each accepted definition is recorded once as one canonical JSON line.  The
//! store-specific create-new claim and short-lived append lock make duplicate
//! or concurrent replacement attempts fail closed; a crash before the durable
//! append leaves an unresolved claim rather than allowing a replacement.

use crate::custody::{
    custody_store_definition_sha256, CustodyStoreDefinitionV1, R237_BOOTSTRAP_STORE_ID,
};
use crate::provider::ObjectServiceError;
use chrono::{DateTime, SecondsFormat, Utc};
use dasobjectstore_core::ids::StoreId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

#[cfg(target_os = "macos")]
const DEFAULT_CUSTODY_CATALOG_PATH: &str = "/usr/local/etc/dasobjectstore/custody-catalog.jsonl";
#[cfg(not(target_os = "macos"))]
const DEFAULT_CUSTODY_CATALOG_PATH: &str = "/var/lib/dasobjectstore/custody-catalog.jsonl";

#[cfg(unix)]
const CATALOG_DIR_MODE: u32 = 0o750;
#[cfg(unix)]
const CATALOG_FILE_MODE: u32 = 0o640;

/// A resolved catalog identity injected into a normal storage plane.  Normal
/// paths may not select an environment-dependent or spelling-ambiguous
/// alternate catalog once this binding exists.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CustodyCatalogBinding {
    canonical_path: PathBuf,
}

impl CustodyCatalogBinding {
    pub fn new(path: impl AsRef<Path>) -> Result<Self, ObjectServiceError> {
        Ok(Self {
            canonical_path: resolve_custody_path(path.as_ref())?,
        })
    }

    pub fn path(&self) -> &Path {
        &self.canonical_path
    }
}

/// A sealed custody admission recorded by the daemon.
///
/// This deliberately contains only the admission definition, where its
/// already-created ledger resides, the definition digest, and its creation
/// time.  It has no lifecycle, replacement, migration, or deletion state.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CustodyCatalogEntryV1 {
    pub definition: CustodyStoreDefinitionV1,
    pub ledger_path: PathBuf,
    /// Digest of the sealed ledger configuration observed immediately after
    /// create-new admission. It prevents an alternate same-id/same-bucket
    /// ledger from being substituted during a later retain operation.
    pub ledger_configuration_sha256: String,
    /// Canonical digest of the catalogued admission definition used to bind
    /// the credential authority. This is intentionally distinct from the
    /// ledger configuration digest above.
    pub configuration_sha256: String,
    pub created_at_utc: String,
}

/// A durable, create-new reservation made before a fresh ledger is created.
/// It is intentionally neither serializable nor releasable. A crash after the
/// claim is a terminal incomplete admission, never permission to re-use the
/// store id or bucket through an ordinary path.
#[derive(Debug)]
pub struct CustodyCatalogAdmissionClaim {
    catalog_path: PathBuf,
    store_id: StoreId,
    bucket_name: String,
}

impl CustodyCatalogEntryV1 {
    fn validate(&self) -> Result<(), ObjectServiceError> {
        self.definition.validate()?;
        if self.definition.store_id.as_str() == R237_BOOTSTRAP_STORE_ID {
            return Err(invalid(
                "the retired r237 bootstrap namespace cannot enter the custody catalog",
            ));
        }
        if !self.ledger_path.is_absolute() {
            return Err(invalid("custody catalog ledger_path must be absolute"));
        }
        if resolve_custody_path(&self.ledger_path)? != self.ledger_path {
            return Err(invalid(
                "custody catalog ledger_path is not its canonical claimed identity",
            ));
        }
        let expected_digest = custody_store_definition_sha256(&self.definition)?;
        if self.configuration_sha256 != expected_digest {
            return Err(invalid(
                "custody catalog configuration_sha256 does not bind its sealed definition",
            ));
        }
        validate_sha256(
            "custody catalog ledger_configuration_sha256",
            &self.ledger_configuration_sha256,
        )?;
        canonical_timestamp("custody catalog created_at_utc", &self.created_at_utc)?;
        Ok(())
    }
}

/// Returns the daemon-owned path for the immutable custody catalog.
pub fn default_custody_catalog_path() -> PathBuf {
    PathBuf::from(DEFAULT_CUSTODY_CATALOG_PATH)
}

/// Daemon-derived ledger path for an immutable custody definition.  The
/// public daemon API never accepts a caller-selected ledger path: doing so
/// would let an ordinary client redirect sealed state into a mutable location.
pub fn default_custody_ledger_path(store_id: &StoreId) -> Result<PathBuf, ObjectServiceError> {
    custody_ledger_path_for_catalog(default_custody_catalog_path(), store_id)
}

/// Derive a sealed ledger path below one daemon-selected catalog. This exists
/// for daemon composition and tests; it is never a client transport field.
pub fn custody_ledger_path_for_catalog(
    catalog_path: impl AsRef<Path>,
    store_id: &StoreId,
) -> Result<PathBuf, ObjectServiceError> {
    if store_id.as_str() == R237_BOOTSTRAP_STORE_ID {
        return Err(invalid(
            "the retired r237 bootstrap namespace has no custody ledger path",
        ));
    }
    let canonical_catalog_path = resolve_custody_path(catalog_path.as_ref())?;
    let parent = canonical_catalog_path
        .parent()
        .ok_or_else(|| invalid("custody catalog path must name a file below a parent directory"))?;
    let digest = hex::encode(Sha256::digest(store_id.as_str().as_bytes()));
    Ok(parent
        .join("custody-ledgers")
        .join(format!("{digest}.sqlite")))
}

/// Append exactly one sealed custody definition to the immutable catalog.
///
/// There is intentionally no idempotent success, update, replacement,
/// migration, or delete operation.  A request for an existing store id is a
/// terminal conflict, even if every supplied value equals the recorded entry.
/// If a process crashes after claiming an id but before the catalog append,
/// later calls fail closed instead of attempting a replacement.
pub fn create_custody_catalog_entry(
    catalog_path: impl AsRef<Path>,
    definition: &CustodyStoreDefinitionV1,
    ledger_path: impl AsRef<Path>,
    ledger_configuration_sha256: impl AsRef<str>,
    created_at_utc: impl AsRef<str>,
) -> Result<CustodyCatalogEntryV1, ObjectServiceError> {
    if !ledger_path.as_ref().is_absolute() {
        return Err(invalid("custody catalog ledger_path must be absolute"));
    }
    let ledger_path = resolve_custody_path(ledger_path.as_ref())?;
    let claim = claim_custody_catalog_admission(catalog_path, definition)?;
    append_claimed_custody_catalog_entry(
        claim,
        definition,
        ledger_path,
        ledger_configuration_sha256,
        created_at_utc,
    )
}

/// Atomically reserve a custody store id and bucket name before any backend
/// provision or ledger creation. The reservation is deliberately one-way.
pub fn claim_custody_catalog_admission(
    catalog_path: impl AsRef<Path>,
    definition: &CustodyStoreDefinitionV1,
) -> Result<CustodyCatalogAdmissionClaim, ObjectServiceError> {
    definition.validate()?;
    if definition.store_id.as_str() == R237_BOOTSTRAP_STORE_ID {
        return Err(invalid(
            "the retired r237 bootstrap namespace cannot enter the custody catalog",
        ));
    }

    let catalog_path = resolve_custody_path(catalog_path.as_ref())?;
    prepare_catalog_parent(&catalog_path)?;
    prepare_custody_ledger_parent(&catalog_path)?;
    create_catalog_if_absent(&catalog_path)?;

    let existing = read_custody_catalog(&catalog_path)?;
    if existing
        .iter()
        .any(|stored| stored.definition.store_id == definition.store_id)
    {
        return Err(existing_entry_error(&definition.store_id));
    }
    if existing
        .iter()
        .any(|stored| stored.definition.bucket_name == definition.bucket_name)
    {
        return Err(existing_bucket_error(&definition.bucket_name));
    }

    let claim_path = store_claim_path(&catalog_path, &definition.store_id)?;
    claim_store_id(&claim_path)?;
    let bucket_claim = bucket_claim_path(&catalog_path, &definition.bucket_name)?;
    claim_bucket_name(&bucket_claim)?;
    Ok(CustodyCatalogAdmissionClaim {
        catalog_path,
        store_id: definition.store_id.clone(),
        bucket_name: definition.bucket_name.clone(),
    })
}

/// Append an immutable entry only for a matching, already durable admission
/// claim. It has no recovery or claim-release operation.
pub fn append_claimed_custody_catalog_entry(
    claim: CustodyCatalogAdmissionClaim,
    definition: &CustodyStoreDefinitionV1,
    ledger_path: impl AsRef<Path>,
    ledger_configuration_sha256: impl AsRef<str>,
    created_at_utc: impl AsRef<str>,
) -> Result<CustodyCatalogEntryV1, ObjectServiceError> {
    definition.validate()?;
    if claim.store_id != definition.store_id || claim.bucket_name != definition.bucket_name {
        return Err(invalid(
            "custody catalog admission claim is not bound to this sealed definition",
        ));
    }
    if !ledger_path.as_ref().is_absolute() {
        return Err(invalid("custody catalog ledger_path must be absolute"));
    }
    let ledger_path = resolve_custody_path(ledger_path.as_ref())?;
    let entry = CustodyCatalogEntryV1 {
        definition: definition.clone(),
        ledger_path,
        ledger_configuration_sha256: ledger_configuration_sha256.as_ref().to_string(),
        configuration_sha256: custody_store_definition_sha256(definition)?,
        created_at_utc: canonical_timestamp(
            "custody catalog created_at_utc",
            created_at_utc.as_ref(),
        )?,
    };
    entry.validate()?;
    let catalog_path = claim.catalog_path;
    if !store_claim_path(&catalog_path, &entry.definition.store_id)?.exists()
        || !bucket_claim_path(&catalog_path, &entry.definition.bucket_name)?.exists()
    {
        return Err(invalid(
            "custody catalog admission claim is missing; replacement is not supported",
        ));
    }

    let append_lock = append_lock_path(&catalog_path)?;
    acquire_append_lock(&append_lock)?;

    let append_result = (|| {
        let current = read_custody_catalog(&catalog_path)?;
        if current
            .iter()
            .any(|stored| stored.definition.store_id == entry.definition.store_id)
        {
            return Err(existing_entry_error(&entry.definition.store_id));
        }
        if current
            .iter()
            .any(|stored| stored.definition.bucket_name == entry.definition.bucket_name)
        {
            return Err(existing_bucket_error(&entry.definition.bucket_name));
        }
        append_entry(&catalog_path, &entry)?;
        Ok(())
    })();

    if append_result.is_ok() {
        release_append_lock(&append_lock)?;
    }

    append_result?;
    Ok(entry)
}

/// Read every sealed catalog entry without offering a mutation path.
pub fn read_custody_catalog(
    catalog_path: impl AsRef<Path>,
) -> Result<Vec<CustodyCatalogEntryV1>, ObjectServiceError> {
    let catalog_path = resolve_custody_path(catalog_path.as_ref())?;
    let file = match File::open(&catalog_path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(command_error("open custody catalog", &catalog_path, error)),
    };

    let mut entries = Vec::new();
    let mut store_ids = BTreeSet::new();
    let mut bucket_names = BTreeSet::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|error| {
            ObjectServiceError::CommandFailed(format!(
                "read custody catalog {} line {}: {error}",
                catalog_path.display(),
                index + 1
            ))
        })?;
        if line.is_empty() {
            return Err(invalid(&format!(
                "custody catalog {} has an empty record at line {}",
                catalog_path.display(),
                index + 1
            )));
        }
        let entry: CustodyCatalogEntryV1 = serde_json::from_str(&line).map_err(|error| {
            invalid(&format!(
                "read custody catalog {} line {}: {error}",
                catalog_path.display(),
                index + 1
            ))
        })?;
        entry.validate()?;
        let canonical = serde_jcs::to_string(&entry).map_err(|error| {
            invalid(&format!(
                "canonicalize custody catalog {} line {}: {error}",
                catalog_path.display(),
                index + 1
            ))
        })?;
        if canonical != line {
            return Err(invalid(&format!(
                "custody catalog {} line {} is not a sealed canonical record",
                catalog_path.display(),
                index + 1
            )));
        }
        if !store_ids.insert(entry.definition.store_id.as_str().to_string()) {
            return Err(invalid(&format!(
                "custody catalog {} has a duplicate store id {}",
                catalog_path.display(),
                entry.definition.store_id
            )));
        }
        if !bucket_names.insert(entry.definition.bucket_name.clone()) {
            return Err(invalid(&format!(
                "custody catalog {} has a duplicate bucket {}",
                catalog_path.display(),
                entry.definition.bucket_name
            )));
        }
        entries.push(entry);
    }
    Ok(entries)
}

/// Returns whether an immutable custody admission exists for `store_id`.
///
/// A dangling create-new claim is treated as an error, not absence: normal
/// mutation paths must stop rather than race a crashed admission attempt.
pub fn catalog_contains_store(
    catalog_path: impl AsRef<Path>,
    store_id: &StoreId,
) -> Result<bool, ObjectServiceError> {
    if store_id.as_str() == R237_BOOTSTRAP_STORE_ID {
        return Err(invalid(
            "the retired r237 bootstrap namespace is not a mutable store",
        ));
    }
    let catalog_path = resolve_custody_path(catalog_path.as_ref())?;
    let entries = read_custody_catalog(&catalog_path)?;
    if entries
        .iter()
        .any(|entry| entry.definition.store_id == *store_id)
    {
        return Ok(true);
    }
    let claim_path = store_claim_path(&catalog_path, store_id)?;
    match fs::metadata(&claim_path) {
        Ok(_) => Err(invalid(&format!(
            "custody catalog has an unresolved create-new claim for store id {store_id}; manual replacement is not supported"
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(command_error(
            "inspect custody catalog claim",
            &claim_path,
            error,
        )),
    }
}

/// Returns whether an immutable custody admission or incomplete create-new
/// claim owns `bucket_name`. It is separate from the StoreId lookup so a
/// normal owner-capable route cannot alias the bucket under another id.
pub fn catalog_contains_bucket(
    catalog_path: impl AsRef<Path>,
    bucket_name: &str,
) -> Result<bool, ObjectServiceError> {
    if bucket_name.trim().is_empty() {
        return Err(invalid("custody catalog bucket name must not be blank"));
    }
    let catalog_path = resolve_custody_path(catalog_path.as_ref())?;
    if read_custody_catalog(&catalog_path)?
        .iter()
        .any(|entry| entry.definition.bucket_name == bucket_name)
    {
        return Ok(true);
    }
    let claim_path = bucket_claim_path(&catalog_path, bucket_name)?;
    match fs::metadata(&claim_path) {
        Ok(_) => Err(invalid(&format!(
            "custody catalog has an unresolved create-new claim for bucket {bucket_name}; normal provisioning is forbidden"
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(command_error(
            "inspect custody catalog bucket claim",
            &claim_path,
            error,
        )),
    }
}

/// Refuse an ordinary mutable route when its target is owned by the sealed
/// custody catalog. This lookup is read-only: a malformed catalog or dangling
/// create-new claim is also a terminal error, rather than permission to race a
/// normal owner-capable route.
pub fn reject_catalogued_custody_mutation(
    catalog_path: impl AsRef<Path>,
    store_id: &StoreId,
    operation: &str,
) -> Result<(), ObjectServiceError> {
    if operation.trim().is_empty() {
        return Err(invalid("custody mutation operation must not be blank"));
    }
    if catalog_contains_store(catalog_path, store_id)? {
        return Err(invalid(&format!(
            "sealed custody store {store_id} rejects ordinary {operation}; use the dedicated daemon custody-retain route"
        )));
    }
    Ok(())
}

/// Refuse a normal definition which aliases either a sealed custody store id
/// *or its fresh bucket*. Checking only the id would let an owner-capable
/// normal provisioning request take over the custody bucket under a new id.
pub fn reject_catalogued_custody_definition(
    catalog_path: impl AsRef<Path>,
    store_id: &StoreId,
    bucket_name: &str,
    operation: &str,
) -> Result<(), ObjectServiceError> {
    if operation.trim().is_empty() || bucket_name.trim().is_empty() {
        return Err(invalid(
            "custody definition operation and bucket name must not be blank",
        ));
    }
    let catalog_path = catalog_path.as_ref();
    if catalog_contains_store(catalog_path, store_id)? {
        return Err(invalid(&format!(
            "sealed custody store {store_id} rejects ordinary {operation}; use the dedicated daemon custody-retain route"
        )));
    }
    if catalog_contains_bucket(catalog_path, bucket_name)? {
        return Err(invalid(&format!(
            "sealed custody bucket {bucket_name} rejects ordinary {operation}; another store id cannot alias it"
        )));
    }
    Ok(())
}

/// Refuse a normal mutation using the exact daemon-configured catalog
/// identity. This closes the old default/environment split: a caller cannot
/// silently inspect a different catalog from the active custody plane.
pub fn reject_bound_catalogued_custody_mutation(
    binding: &CustodyCatalogBinding,
    store_id: &StoreId,
    operation: &str,
) -> Result<(), ObjectServiceError> {
    reject_catalogued_custody_mutation(binding.path(), store_id, operation)
}

/// Refuse a normal definition including a bucket alias using the exact active
/// catalog identity.
pub fn reject_bound_catalogued_custody_definition(
    binding: &CustodyCatalogBinding,
    store_id: &StoreId,
    bucket_name: &str,
    operation: &str,
) -> Result<(), ObjectServiceError> {
    reject_catalogued_custody_definition(binding.path(), store_id, bucket_name, operation)
}

/// Resolve an absolute custody path without accepting `.`/`..` spelling or a
/// symlink in any existing ancestor. A non-existing tail is reconstructed
/// beneath the canonical existing parent, so a future claim cannot compare a
/// raw spelling with a different resolved path.
fn resolve_custody_path(path: &Path) -> Result<PathBuf, ObjectServiceError> {
    if !path.is_absolute() {
        return Err(invalid("custody path must be absolute"));
    }
    // macOS presents the system-owned `/var` compatibility symlink to
    // `/private/var`; accepting that one documented kernel namespace alias
    // means we compare its canonical identity, not that raw spelling. Any
    // caller-created symlink below it remains a hard refusal.
    #[cfg(target_os = "macos")]
    let configured = match path.strip_prefix("/var") {
        Ok(tail) => fs::canonicalize("/var")
            .map_err(|error| command_error("canonicalise macOS /var", Path::new("/var"), error))?
            .join(tail),
        Err(_) => path.to_path_buf(),
    };
    #[cfg(not(target_os = "macos"))]
    let configured = path.to_path_buf();
    let components = configured
        .components()
        .filter_map(|component| match component {
            std::path::Component::RootDir => None,
            std::path::Component::Normal(component) => Some(Ok(component)),
            std::path::Component::CurDir
            | std::path::Component::ParentDir
            | std::path::Component::Prefix(_) => Some(Err(invalid(
                "custody path must not contain normalisation components",
            ))),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if components.is_empty() {
        return Err(invalid("custody path must name a file below root"));
    }

    let mut existing = PathBuf::from(std::path::MAIN_SEPARATOR.to_string());
    let mut missing_at = None;
    for (index, component) in components.iter().enumerate() {
        existing.push(component);
        match fs::symlink_metadata(&existing) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(invalid("custody path contains a symbolic-link component"));
                }
                if index + 1 < components.len() && !metadata.is_dir() {
                    return Err(invalid("custody path contains a non-directory ancestor"));
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                missing_at = Some(index);
                existing.pop();
                break;
            }
            Err(error) => return Err(command_error("inspect custody path", &existing, error)),
        }
    }

    let canonical_existing = fs::canonicalize(&existing)
        .map_err(|error| command_error("canonicalise custody path ancestor", &existing, error))?;
    let resolved = match missing_at {
        Some(index) => components[index..]
            .iter()
            .fold(canonical_existing, |current, component| {
                current.join(component)
            }),
        None => canonical_existing,
    };
    if missing_at.is_none() && resolved != configured {
        return Err(invalid(
            "custody path canonical identity differs from its configured spelling",
        ));
    }
    Ok(resolved)
}

fn prepare_catalog_parent(catalog_path: &Path) -> Result<(), ObjectServiceError> {
    let parent = catalog_path
        .parent()
        .ok_or_else(|| invalid("custody catalog path must name a file below a parent directory"))?;
    fs::create_dir_all(parent)
        .map_err(|error| command_error("create custody catalog directory", parent, error))?;
    restrict_dir(parent)?;

    let claim_dir = catalog_claim_dir(catalog_path)?;
    fs::create_dir_all(&claim_dir).map_err(|error| {
        command_error("create custody catalog claim directory", &claim_dir, error)
    })?;
    restrict_dir(&claim_dir)
}

fn create_catalog_if_absent(catalog_path: &Path) -> Result<(), ObjectServiceError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    configure_private_file(&mut options);
    match options.open(catalog_path) {
        Ok(file) => {
            file.sync_all().map_err(|error| {
                command_error("synchronize new custody catalog", catalog_path, error)
            })?;
            sync_parent(catalog_path)
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(command_error("create custody catalog", catalog_path, error)),
    }
}

/// Establish the daemon-selected ledger directory before recording a
/// one-way admission claim. Ledger creation itself remains `create_new` only;
/// this merely avoids letting a later admission path choose or implicitly
/// create its parent directory.
fn prepare_custody_ledger_parent(catalog_path: &Path) -> Result<(), ObjectServiceError> {
    let parent = catalog_path
        .parent()
        .ok_or_else(|| invalid("custody catalog path must name a file below a parent directory"))?;
    let ledger_parent = parent.join("custody-ledgers");
    fs::create_dir_all(&ledger_parent).map_err(|error| {
        command_error(
            "create daemon custody ledger directory",
            &ledger_parent,
            error,
        )
    })?;
    restrict_dir(&ledger_parent)?;
    sync_parent(&ledger_parent)
}

fn claim_store_id(claim_path: &Path) -> Result<(), ObjectServiceError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    configure_private_file(&mut options);
    match options.open(claim_path) {
        Ok(file) => {
            file.sync_all().map_err(|error| {
                command_error("synchronize custody catalog claim", claim_path, error)
            })?;
            sync_parent(claim_path)
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Err(invalid(&format!(
            "custody catalog store id is already claimed: {}",
            claim_path.display()
        ))),
        Err(error) => Err(command_error(
            "create custody catalog claim",
            claim_path,
            error,
        )),
    }
}

fn claim_bucket_name(claim_path: &Path) -> Result<(), ObjectServiceError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    configure_private_file(&mut options);
    match options.open(claim_path) {
        Ok(file) => {
            file.sync_all().map_err(|error| {
                command_error(
                    "synchronize custody catalog bucket claim",
                    claim_path,
                    error,
                )
            })?;
            sync_parent(claim_path)
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Err(invalid(&format!(
            "custody catalog bucket is already claimed: {}",
            claim_path.display()
        ))),
        Err(error) => Err(command_error(
            "create custody catalog bucket claim",
            claim_path,
            error,
        )),
    }
}

fn acquire_append_lock(lock_path: &Path) -> Result<(), ObjectServiceError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    configure_private_file(&mut options);
    match options.open(lock_path) {
        Ok(file) => {
            file.sync_all().map_err(|error| {
                command_error("synchronize custody catalog append lock", lock_path, error)
            })?;
            sync_parent(lock_path)
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Err(invalid(&format!(
            "custody catalog append is already in progress or a prior append was interrupted: {}",
            lock_path.display()
        ))),
        Err(error) => Err(command_error(
            "acquire custody catalog append lock",
            lock_path,
            error,
        )),
    }
}

fn release_append_lock(lock_path: &Path) -> Result<(), ObjectServiceError> {
    fs::remove_file(lock_path)
        .map_err(|error| command_error("release custody catalog append lock", lock_path, error))?;
    sync_parent(lock_path)
}

fn append_entry(
    catalog_path: &Path,
    entry: &CustodyCatalogEntryV1,
) -> Result<(), ObjectServiceError> {
    let encoded = serde_jcs::to_string(entry)
        .map_err(|error| invalid(&format!("canonicalize custody catalog entry: {error}")))?;
    let mut options = OpenOptions::new();
    options.append(true).write(true);
    let mut file = options
        .open(catalog_path)
        .map_err(|error| command_error("open custody catalog for append", catalog_path, error))?;
    file.write_all(encoded.as_bytes())
        .and_then(|()| file.write_all(b"\n"))
        .map_err(|error| command_error("append custody catalog entry", catalog_path, error))?;
    file.sync_all()
        .map_err(|error| command_error("synchronize custody catalog entry", catalog_path, error))?;
    sync_parent(catalog_path)
}

fn catalog_claim_dir(catalog_path: &Path) -> Result<PathBuf, ObjectServiceError> {
    let parent = catalog_path
        .parent()
        .ok_or_else(|| invalid("custody catalog path must name a file below a parent directory"))?;
    let file_name = catalog_path
        .file_name()
        .ok_or_else(|| invalid("custody catalog path must have a file name"))?;
    Ok(parent.join(format!(".{}.claims", file_name.to_string_lossy())))
}

fn store_claim_path(
    catalog_path: &Path,
    store_id: &StoreId,
) -> Result<PathBuf, ObjectServiceError> {
    let digest = hex::encode(Sha256::digest(store_id.as_str().as_bytes()));
    Ok(catalog_claim_dir(catalog_path)?.join(format!("{digest}.claim")))
}

fn bucket_claim_path(
    catalog_path: &Path,
    bucket_name: &str,
) -> Result<PathBuf, ObjectServiceError> {
    let digest = hex::encode(Sha256::digest(bucket_name.as_bytes()));
    Ok(catalog_claim_dir(catalog_path)?.join(format!("bucket-{digest}.claim")))
}

fn append_lock_path(catalog_path: &Path) -> Result<PathBuf, ObjectServiceError> {
    let parent = catalog_path
        .parent()
        .ok_or_else(|| invalid("custody catalog path must name a file below a parent directory"))?;
    let file_name = catalog_path
        .file_name()
        .ok_or_else(|| invalid("custody catalog path must have a file name"))?;
    Ok(parent.join(format!(".{}.append.lock", file_name.to_string_lossy())))
}

fn canonical_timestamp(field: &str, value: &str) -> Result<String, ObjectServiceError> {
    let parsed = DateTime::parse_from_rfc3339(value)
        .map_err(|error| invalid(&format!("{field} must be RFC3339 UTC: {error}")))?
        .with_timezone(&Utc);
    let canonical = parsed.to_rfc3339_opts(SecondsFormat::Secs, true);
    if canonical != value {
        return Err(invalid(&format!(
            "{field} must be canonical UTC seconds precision ({canonical})"
        )));
    }
    Ok(canonical)
}

fn validate_sha256(field: &str, value: &str) -> Result<(), ObjectServiceError> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid(&format!(
            "{field} must be a lower-level SHA-256 hex digest"
        )));
    }
    Ok(())
}

fn configure_private_file(options: &mut OpenOptions) {
    #[cfg(unix)]
    options.mode(CATALOG_FILE_MODE);
}

fn restrict_dir(path: &Path) -> Result<(), ObjectServiceError> {
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(CATALOG_DIR_MODE))
        .map_err(|error| command_error("restrict custody catalog directory", path, error))?;
    Ok(())
}

fn sync_parent(path: &Path) -> Result<(), ObjectServiceError> {
    #[cfg(unix)]
    {
        let parent = path.parent().ok_or_else(|| {
            invalid("custody catalog path must name a file below a parent directory")
        })?;
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| {
                command_error("synchronize custody catalog directory", parent, error)
            })?;
    }
    Ok(())
}

fn existing_entry_error(store_id: &StoreId) -> ObjectServiceError {
    invalid(&format!(
        "custody catalog entry already exists for store id {store_id}; immutable entries cannot be updated or replaced"
    ))
}

fn existing_bucket_error(bucket_name: &str) -> ObjectServiceError {
    invalid(&format!(
        "custody catalog entry already exists for bucket {bucket_name}; immutable entries cannot be aliased or replaced"
    ))
}

fn command_error(action: &str, path: &Path, error: io::Error) -> ObjectServiceError {
    ObjectServiceError::CommandFailed(format!("{action} {}: {error}", path.display()))
}

fn invalid(message: &str) -> ObjectServiceError {
    ObjectServiceError::InvalidConfiguration(message.to_string())
}

#[cfg(test)]
mod tests {
    #[cfg(not(target_os = "macos"))]
    use super::default_custody_catalog_path;
    use super::{
        catalog_contains_bucket, catalog_contains_store, claim_custody_catalog_admission,
        create_custody_catalog_entry, default_custody_ledger_path, read_custody_catalog,
        reject_catalogued_custody_definition, reject_catalogued_custody_mutation, store_claim_path,
        CustodyCatalogEntryV1,
    };
    use crate::custody::{
        CustodyAssuranceClass, CustodyRetentionMode, CustodyStoreDefinitionV1,
        CustodyStoreProfileV1, CUSTODY_OVERLAY_SCHEMA_V1, CUSTODY_PROFILE_V1,
        R237_BOOTSTRAP_BUCKET_NAME, R237_BOOTSTRAP_STORE_ID,
    };
    use dasobjectstore_core::ids::StoreId;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    const CREATED_AT: &str = "2026-09-05T12:00:00Z";
    const LEDGER_CONFIGURATION_SHA256: &str =
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn temporary_catalog_path(test_name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dasobjectstore-custody-catalog-{test_name}-{nonce}"
        ))
    }

    fn definition(store_id: &str) -> CustodyStoreDefinitionV1 {
        CustodyStoreDefinitionV1 {
            store_id: StoreId::new(store_id).expect("store id"),
            bucket_name: format!("dos-{store_id}-custody"),
            profile: CustodyStoreProfileV1 {
                schema: CUSTODY_OVERLAY_SCHEMA_V1.to_string(),
                profile: CUSTODY_PROFILE_V1.to_string(),
                assurance_class: CustodyAssuranceClass::LocalTrustedAdministratorOverlay,
                retention_mode: CustodyRetentionMode::LocalTrustedAdministratorOverlay,
                target_id: "nuc-192-168-0-193".to_string(),
                retention_until_utc: "2036-09-05T12:00:00Z".to_string(),
                legal_hold: true,
                provisioner_credential_reference: "pistis://custody/provisioner".to_string(),
                provisioner_identity: "custody-provisioner".to_string(),
                writer_credential_reference: "pistis://custody/writer".to_string(),
                writer_identity: "custody-writer".to_string(),
                reader_credential_reference: "pistis://custody/reader".to_string(),
                reader_identity: "custody-reader".to_string(),
            },
        }
    }

    #[test]
    fn creates_durable_append_only_entry_and_supports_read_only_lookup() {
        let catalog = temporary_catalog_path("create").join("catalog.jsonl");
        let definition = definition("custody-a");
        let ledger = PathBuf::from("/var/lib/dasobjectstore/custody/custody-a.sqlite");

        let created = create_custody_catalog_entry(
            &catalog,
            &definition,
            &ledger,
            LEDGER_CONFIGURATION_SHA256,
            CREATED_AT,
        )
        .expect("catalog entry");

        assert_eq!(created.definition, definition);
        assert_eq!(
            created.ledger_path,
            super::resolve_custody_path(&ledger).expect("canonical ledger")
        );
        assert!(catalog_contains_store(&catalog, &created.definition.store_id).expect("lookup"));
        assert_eq!(read_custody_catalog(&catalog).expect("read"), vec![created]);
        let encoded = fs::read_to_string(&catalog).expect("catalog bytes");
        assert_eq!(encoded.lines().count(), 1);

        fs::remove_dir_all(catalog.parent().expect("parent")).expect("cleanup");
    }

    #[test]
    fn rejects_duplicate_store_id_without_replacement_or_second_append() {
        let catalog = temporary_catalog_path("duplicate").join("catalog.jsonl");
        let definition = definition("custody-duplicate");
        let ledger = PathBuf::from("/var/lib/dasobjectstore/custody/duplicate.sqlite");
        create_custody_catalog_entry(
            &catalog,
            &definition,
            &ledger,
            LEDGER_CONFIGURATION_SHA256,
            CREATED_AT,
        )
        .expect("first entry");
        let before = fs::read(&catalog).expect("original catalog");

        let error = create_custody_catalog_entry(
            &catalog,
            &definition,
            &ledger,
            LEDGER_CONFIGURATION_SHA256,
            CREATED_AT,
        )
        .expect_err("second entry must fail");

        assert!(error.to_string().contains("cannot be updated or replaced"));
        assert_eq!(fs::read(&catalog).expect("unchanged catalog"), before);
        fs::remove_dir_all(catalog.parent().expect("parent")).expect("cleanup");
    }

    #[test]
    fn rejects_retired_bootstrap_namespace_before_catalog_mutation() {
        let catalog = temporary_catalog_path("retired").join("catalog.jsonl");
        let mut retired = definition(R237_BOOTSTRAP_STORE_ID);
        retired.bucket_name = R237_BOOTSTRAP_BUCKET_NAME.to_string();

        let error = create_custody_catalog_entry(
            &catalog,
            &retired,
            "/var/lib/dasobjectstore/custody/retired.sqlite",
            LEDGER_CONFIGURATION_SHA256,
            CREATED_AT,
        )
        .expect_err("retired bootstrap namespace must fail");

        assert!(error.to_string().contains("retired r237 bootstrap"));
        assert!(!catalog.exists());
    }

    #[test]
    fn rejects_relative_ledger_path_before_catalog_mutation() {
        let catalog = temporary_catalog_path("relative-ledger").join("catalog.jsonl");

        let error = create_custody_catalog_entry(
            &catalog,
            &definition("custody-relative-ledger"),
            "custody/relative.sqlite",
            LEDGER_CONFIGURATION_SHA256,
            CREATED_AT,
        )
        .expect_err("relative ledger path must fail");

        assert!(error.to_string().contains("ledger_path must be absolute"));
        assert!(!catalog.exists());
    }

    #[test]
    fn malformed_or_duplicate_records_fail_closed() {
        let catalog = temporary_catalog_path("malformed").join("catalog.jsonl");
        fs::create_dir_all(catalog.parent().expect("parent")).expect("catalog parent");
        fs::write(&catalog, "{\"definition\":").expect("partial record");
        let store = StoreId::new("custody-malformed").expect("store");

        assert!(read_custody_catalog(&catalog).is_err());
        assert!(catalog_contains_store(&catalog, &store).is_err());
        fs::remove_dir_all(catalog.parent().expect("parent")).expect("cleanup");
    }

    #[test]
    fn dangling_create_new_claim_blocks_normal_path_lookup() {
        let catalog = temporary_catalog_path("claim").join("catalog.jsonl");
        let store = StoreId::new("custody-claimed").expect("store");
        let claim = store_claim_path(&catalog, &store).expect("claim path");
        fs::create_dir_all(claim.parent().expect("claim parent")).expect("claim parent");
        fs::write(&claim, []).expect("claim");

        let error = catalog_contains_store(&catalog, &store).expect_err("claim must fail closed");

        assert!(error.to_string().contains("unresolved create-new claim"));
        fs::remove_dir_all(catalog.parent().expect("parent")).expect("cleanup");
    }

    #[test]
    fn strict_entries_reject_unexpected_state_fields() {
        let catalog = temporary_catalog_path("strict").join("catalog.jsonl");
        fs::create_dir_all(catalog.parent().expect("parent")).expect("catalog parent");
        let entry = CustodyCatalogEntryV1 {
            definition: definition("custody-strict"),
            ledger_path: PathBuf::from("/var/lib/dasobjectstore/custody/strict.sqlite"),
            ledger_configuration_sha256: LEDGER_CONFIGURATION_SHA256.to_string(),
            configuration_sha256: "not-used".to_string(),
            created_at_utc: CREATED_AT.to_string(),
        };
        let mut value = serde_json::to_value(entry).expect("serialize");
        value.as_object_mut().expect("object").insert(
            "lifecycle".to_string(),
            serde_json::Value::String("deleted".to_string()),
        );
        fs::write(
            &catalog,
            format!("{}\n", serde_json::to_string(&value).expect("json")),
        )
        .expect("write");

        let error = read_custody_catalog(&catalog).expect_err("unexpected field must fail");

        assert!(error.to_string().contains("unknown field"));
        fs::remove_dir_all(catalog.parent().expect("parent")).expect("cleanup");
    }

    #[test]
    fn sealed_entry_blocks_ordinary_mutation_before_that_route_can_act() {
        let catalog = temporary_catalog_path("ordinary-route").join("catalog.jsonl");
        let definition = definition("custody-ordinary-route");
        create_custody_catalog_entry(
            &catalog,
            &definition,
            "/var/lib/dasobjectstore/custody/ordinary-route.sqlite",
            LEDGER_CONFIGURATION_SHA256,
            CREATED_AT,
        )
        .expect("catalog entry");

        let error =
            reject_catalogued_custody_mutation(&catalog, &definition.store_id, "multipart upload")
                .expect_err("normal mutation must be denied");

        assert!(error
            .to_string()
            .contains("dedicated daemon custody-retain"));
        fs::remove_dir_all(catalog.parent().expect("parent")).expect("cleanup");
    }

    #[test]
    fn sealed_bucket_cannot_be_aliased_by_an_ordinary_store_id() {
        let catalog = temporary_catalog_path("bucket-alias").join("catalog.jsonl");
        let definition = definition("custody-bucket-owner");
        create_custody_catalog_entry(
            &catalog,
            &definition,
            "/var/lib/dasobjectstore/custody/bucket-owner.sqlite",
            LEDGER_CONFIGURATION_SHA256,
            CREATED_AT,
        )
        .expect("catalog entry");
        let alias = StoreId::new("ordinary-alias").expect("alias store id");

        let error = reject_catalogued_custody_definition(
            &catalog,
            &alias,
            &definition.bucket_name,
            "normal Garage provisioning",
        )
        .expect_err("bucket alias must fail");

        assert!(error.to_string().contains("cannot alias"));
        fs::remove_dir_all(catalog.parent().expect("parent")).expect("cleanup");
    }

    #[test]
    fn incomplete_admission_claim_blocks_bucket_alias_before_ledger_creation() {
        let catalog = temporary_catalog_path("bucket-claim").join("catalog.jsonl");
        let definition = definition("custody-claimed-bucket");
        claim_custody_catalog_admission(&catalog, &definition).expect("fresh claim");
        let alias = StoreId::new("ordinary-claimed-alias").expect("alias store id");

        assert!(catalog_contains_bucket(&catalog, &definition.bucket_name).is_err());
        assert!(reject_catalogued_custody_definition(
            &catalog,
            &alias,
            &definition.bucket_name,
            "normal Garage provisioning",
        )
        .is_err());
        assert!(read_custody_catalog(&catalog)
            .expect("no entry before append")
            .is_empty());
        fs::remove_dir_all(catalog.parent().expect("parent")).expect("cleanup");
    }

    #[test]
    fn default_ledger_path_is_daemon_derived_and_does_not_embed_store_text() {
        let path = default_custody_ledger_path(&StoreId::new("custody-ledger-path").unwrap())
            .expect("derived path");
        assert!(path.is_absolute());
        assert_eq!(
            path.extension().and_then(|value| value.to_str()),
            Some("sqlite")
        );
        assert!(!path.to_string_lossy().contains("custody-ledger-path"));
    }

    #[test]
    fn rejects_a_nested_unknown_profile_field_instead_of_silently_dropping_it() {
        let catalog = temporary_catalog_path("nested-strict").join("catalog.jsonl");
        fs::create_dir_all(catalog.parent().expect("parent")).expect("catalog parent");
        let entry = create_custody_catalog_entry(
            &catalog,
            &definition("custody-nested-strict"),
            "/var/lib/dasobjectstore/custody/nested-strict.sqlite",
            LEDGER_CONFIGURATION_SHA256,
            CREATED_AT,
        )
        .expect("entry");
        let mut value = serde_json::to_value(entry).expect("serialize");
        value["definition"]["profile"]["unreviewed_mode"] =
            serde_json::Value::String("replace".to_string());
        fs::write(
            &catalog,
            format!("{}\n", serde_json::to_string(&value).expect("json")),
        )
        .expect("write");

        let error = read_custody_catalog(&catalog).expect_err("nested field must fail");

        assert!(error.to_string().contains("sealed canonical record"));
        fs::remove_dir_all(catalog.parent().expect("parent")).expect("cleanup");
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn linux_default_uses_a_distinct_daemon_catalog_path() {
        assert_eq!(
            default_custody_catalog_path(),
            PathBuf::from("/var/lib/dasobjectstore/custody-catalog.jsonl")
        );
    }
}
