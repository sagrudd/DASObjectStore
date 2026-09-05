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

pub const CUSTODY_CATALOG_ENV: &str = "DASOBJECTSTORE_CUSTODY_CATALOG_PATH";

#[cfg(target_os = "macos")]
const DEFAULT_CUSTODY_CATALOG_PATH: &str = "/usr/local/etc/dasobjectstore/custody-catalog.jsonl";
#[cfg(not(target_os = "macos"))]
const DEFAULT_CUSTODY_CATALOG_PATH: &str = "/var/lib/dasobjectstore/custody-catalog.jsonl";

#[cfg(unix)]
const CATALOG_DIR_MODE: u32 = 0o750;
#[cfg(unix)]
const CATALOG_FILE_MODE: u32 = 0o640;

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
    pub configuration_sha256: String,
    pub created_at_utc: String,
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
        let expected_digest = custody_store_definition_sha256(&self.definition)?;
        if self.configuration_sha256 != expected_digest {
            return Err(invalid(
                "custody catalog configuration_sha256 does not bind its sealed definition",
            ));
        }
        canonical_timestamp("custody catalog created_at_utc", &self.created_at_utc)?;
        Ok(())
    }
}

/// Returns the daemon-owned path for the immutable custody catalog.
pub fn default_custody_catalog_path() -> PathBuf {
    std::env::var_os(CUSTODY_CATALOG_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CUSTODY_CATALOG_PATH))
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
    created_at_utc: impl AsRef<str>,
) -> Result<CustodyCatalogEntryV1, ObjectServiceError> {
    definition.validate()?;
    if definition.store_id.as_str() == R237_BOOTSTRAP_STORE_ID {
        return Err(invalid(
            "the retired r237 bootstrap namespace cannot enter the custody catalog",
        ));
    }

    let ledger_path = ledger_path.as_ref();
    if !ledger_path.is_absolute() {
        return Err(invalid("custody catalog ledger_path must be absolute"));
    }

    let entry = CustodyCatalogEntryV1 {
        definition: definition.clone(),
        ledger_path: ledger_path.to_path_buf(),
        configuration_sha256: custody_store_definition_sha256(definition)?,
        created_at_utc: canonical_timestamp(
            "custody catalog created_at_utc",
            created_at_utc.as_ref(),
        )?,
    };
    entry.validate()?;

    let catalog_path = catalog_path.as_ref();
    prepare_catalog_parent(catalog_path)?;
    create_catalog_if_absent(catalog_path)?;

    let existing = read_custody_catalog(catalog_path)?;
    if existing
        .iter()
        .any(|stored| stored.definition.store_id == entry.definition.store_id)
    {
        return Err(existing_entry_error(&entry.definition.store_id));
    }

    let claim_path = store_claim_path(catalog_path, &entry.definition.store_id)?;
    claim_store_id(&claim_path)?;

    let append_lock = append_lock_path(catalog_path)?;
    acquire_append_lock(&append_lock)?;

    let append_result = (|| {
        let current = read_custody_catalog(catalog_path)?;
        if current
            .iter()
            .any(|stored| stored.definition.store_id == entry.definition.store_id)
        {
            return Err(existing_entry_error(&entry.definition.store_id));
        }
        append_entry(catalog_path, &entry)?;
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
    let catalog_path = catalog_path.as_ref();
    let file = match File::open(catalog_path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(command_error("open custody catalog", catalog_path, error)),
    };

    let mut entries = Vec::new();
    let mut store_ids = BTreeSet::new();
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
    let catalog_path = catalog_path.as_ref();
    let entries = read_custody_catalog(catalog_path)?;
    if entries
        .iter()
        .any(|entry| entry.definition.store_id == *store_id)
    {
        return Ok(true);
    }
    let claim_path = store_claim_path(catalog_path, store_id)?;
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
        catalog_contains_store, create_custody_catalog_entry, read_custody_catalog,
        store_claim_path, CustodyCatalogEntryV1,
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

        let created = create_custody_catalog_entry(&catalog, &definition, &ledger, CREATED_AT)
            .expect("catalog entry");

        assert_eq!(created.definition, definition);
        assert_eq!(created.ledger_path, ledger);
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
        create_custody_catalog_entry(&catalog, &definition, &ledger, CREATED_AT)
            .expect("first entry");
        let before = fs::read(&catalog).expect("original catalog");

        let error = create_custody_catalog_entry(&catalog, &definition, &ledger, CREATED_AT)
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
    fn rejects_a_nested_unknown_profile_field_instead_of_silently_dropping_it() {
        let catalog = temporary_catalog_path("nested-strict").join("catalog.jsonl");
        fs::create_dir_all(catalog.parent().expect("parent")).expect("catalog parent");
        let entry = create_custody_catalog_entry(
            &catalog,
            &definition("custody-nested-strict"),
            "/var/lib/dasobjectstore/custody/nested-strict.sqlite",
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
