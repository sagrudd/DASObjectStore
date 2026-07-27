//! Replay-safe opaque application capability authority.
//!
//! The ledger persists only capability and nonce digests. The opaque token is
//! deterministically derived from a daemon-custodied random master key so an
//! exact exchange retry can return the same token after restart without
//! storing plaintext bearer material.

use super::DaemonServiceRuntimeError;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use dasobjectstore_core::application_auth_v2::{
    GovernedHostAuthorityV2, GovernedProsopikonAuthorityV2,
};
use ring::{hmac, rand as ring_rand};
use ring_rand::SecureRandom;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub const APPLICATION_CAPABILITY_LEDGER_SCHEMA: &str =
    "dasobjectstore.application_capability_ledger.v1";
pub const APPLICATION_CAPABILITY_LEDGER_FILE_NAME: &str = "application-capability-ledger.json";
pub const APPLICATION_CAPABILITY_MASTER_KEY_FILE_NAME: &str = "application-capability-master.key";
pub const APPLICATION_OPAQUE_CAPABILITY_PREFIX: &str = "dosc_v2_";

static LEDGER_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationCapabilityClaims {
    pub application_id: String,
    pub key_id: String,
    pub binding_id: String,
    pub binding_digest_sha256: String,
    pub tenant_id: String,
    pub host_authority: GovernedHostAuthorityV2,
    pub prosopikon_authority: GovernedProsopikonAuthorityV2,
    pub audience: String,
    pub store_id: String,
    pub prefixes: Vec<String>,
    pub operations: Vec<String>,
    pub max_object_bytes: u64,
    pub max_total_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationCapabilityIssue {
    pub request_id: String,
    pub request_digest_sha256: String,
    pub nonce: Vec<u8>,
    pub issued_at_unix_seconds: u64,
    pub expires_at_unix_seconds: u64,
    pub claims: ApplicationCapabilityClaims,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IssuedApplicationCapability {
    pub capability_id: String,
    pub opaque_capability: String,
    pub issued_at_unix_seconds: u64,
    pub expires_at_unix_seconds: u64,
    pub exact_replay: bool,
    pub claims: ApplicationCapabilityClaims,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationCapabilityUse {
    pub audience: String,
    pub store_id: String,
    pub object_key: String,
    pub operation: String,
    pub bytes: u64,
    pub now_unix_seconds: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedApplicationCapability {
    pub capability_id: String,
    pub claims: ApplicationCapabilityClaims,
    pub accounted_bytes: u64,
    pub expires_at_unix_seconds: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum CapabilityState {
    Active,
    Revoked,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CapabilityRecord {
    application_id: String,
    request_id: String,
    request_digest_sha256: String,
    nonce_digest_sha256: String,
    capability_id: String,
    capability_digest_sha256: String,
    issued_at_unix_seconds: u64,
    expires_at_unix_seconds: u64,
    claims: ApplicationCapabilityClaims,
    accounted_bytes: u64,
    state: CapabilityState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    revoked_at_unix_seconds: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    schema_version: String,
    records: Vec<CapabilityRecord>,
}

impl Default for Ledger {
    fn default() -> Self {
        Self {
            schema_version: APPLICATION_CAPABILITY_LEDGER_SCHEMA.to_string(),
            records: Vec::new(),
        }
    }
}

pub fn application_capability_ledger_path(state_dir: impl AsRef<Path>) -> PathBuf {
    state_dir
        .as_ref()
        .join(APPLICATION_CAPABILITY_LEDGER_FILE_NAME)
}

pub fn application_capability_master_key_path(state_dir: impl AsRef<Path>) -> PathBuf {
    state_dir
        .as_ref()
        .join(APPLICATION_CAPABILITY_MASTER_KEY_FILE_NAME)
}

pub fn issue_opaque_application_capability(
    ledger_path: impl AsRef<Path>,
    master_key_path: impl AsRef<Path>,
    issue: ApplicationCapabilityIssue,
    now_unix_seconds: u64,
) -> Result<IssuedApplicationCapability, DaemonServiceRuntimeError> {
    validate_issue(&issue, now_unix_seconds)?;
    let _guard = lock()?;
    let ledger_path = ledger_path.as_ref();
    let master = load_or_create_master_key(master_key_path.as_ref())?;
    let mut ledger = read_ledger(ledger_path)?;
    retain_live(&mut ledger, now_unix_seconds);

    if let Some(existing) = ledger.records.iter().find(|record| {
        record.application_id == issue.claims.application_id
            && record.request_id == issue.request_id
    }) {
        if existing.request_digest_sha256 != issue.request_digest_sha256
            || existing.nonce_digest_sha256 != digest_bytes(&issue.nonce)
            || existing.claims != issue.claims
            || existing.issued_at_unix_seconds != issue.issued_at_unix_seconds
            || existing.expires_at_unix_seconds != issue.expires_at_unix_seconds
        {
            return Err(invalid("changed application exchange replay detected"));
        }
        if existing.state != CapabilityState::Active
            || now_unix_seconds >= existing.expires_at_unix_seconds
        {
            return Err(invalid("application capability is revoked or expired"));
        }
        let token = derive_token(&master, &issue);
        if digest_text(&token) != existing.capability_digest_sha256 {
            return Err(invalid(
                "application capability master key does not match ledger",
            ));
        }
        write_ledger(ledger_path, &ledger)?;
        return Ok(issued(existing, token, true));
    }

    let nonce_digest = digest_bytes(&issue.nonce);
    if ledger.records.iter().any(|record| {
        record.application_id == issue.claims.application_id
            && record.nonce_digest_sha256 == nonce_digest
    }) {
        return Err(invalid("application exchange nonce replay detected"));
    }

    let token = derive_token(&master, &issue);
    let capability_id = derive_capability_id(&master, &issue);
    let record = CapabilityRecord {
        application_id: issue.claims.application_id.clone(),
        request_id: issue.request_id.clone(),
        request_digest_sha256: issue.request_digest_sha256,
        nonce_digest_sha256: nonce_digest,
        capability_id,
        capability_digest_sha256: digest_text(&token),
        issued_at_unix_seconds: issue.issued_at_unix_seconds,
        expires_at_unix_seconds: issue.expires_at_unix_seconds,
        claims: issue.claims,
        accounted_bytes: 0,
        state: CapabilityState::Active,
        revoked_at_unix_seconds: None,
    };
    let result = issued(&record, token, false);
    ledger.records.push(record);
    ledger.records.sort_by(|left, right| {
        left.application_id
            .cmp(&right.application_id)
            .then_with(|| left.request_id.cmp(&right.request_id))
    });
    validate_ledger(&ledger)?;
    write_ledger(ledger_path, &ledger)?;
    Ok(result)
}

/// Replace one still-active capability during its final five minutes. The old
/// capability and the new issuance are persisted in one atomic ledger write.
/// An exact retry returns the same derived token even though the predecessor
/// has already been revoked.
pub fn renew_opaque_application_capability(
    ledger_path: impl AsRef<Path>,
    master_key_path: impl AsRef<Path>,
    prior_capability_id: &str,
    issue: ApplicationCapabilityIssue,
    now_unix_seconds: u64,
) -> Result<IssuedApplicationCapability, DaemonServiceRuntimeError> {
    validate_issue(&issue, now_unix_seconds)?;
    if prior_capability_id.trim().is_empty() {
        return Err(invalid("prior application capability identity is required"));
    }
    let _guard = lock()?;
    let ledger_path = ledger_path.as_ref();
    let master = load_or_create_master_key(master_key_path.as_ref())?;
    let mut ledger = read_ledger(ledger_path)?;
    retain_live(&mut ledger, now_unix_seconds);

    // The replacement is checked first so a network retry remains idempotent
    // after the predecessor was atomically revoked.
    if let Some(existing) = ledger.records.iter().find(|record| {
        record.application_id == issue.claims.application_id
            && record.request_id == issue.request_id
    }) {
        if existing.request_digest_sha256 != issue.request_digest_sha256
            || existing.nonce_digest_sha256 != digest_bytes(&issue.nonce)
            || existing.claims != issue.claims
            || existing.issued_at_unix_seconds != issue.issued_at_unix_seconds
            || existing.expires_at_unix_seconds != issue.expires_at_unix_seconds
            || existing.state != CapabilityState::Active
            || now_unix_seconds >= existing.expires_at_unix_seconds
        {
            return Err(invalid(
                "changed application capability renewal replay detected",
            ));
        }
        let token = derive_token(&master, &issue);
        if digest_text(&token) != existing.capability_digest_sha256 {
            return Err(invalid(
                "application capability master key does not match ledger",
            ));
        }
        return Ok(issued(existing, token, true));
    }

    let prior_index = ledger
        .records
        .iter()
        .position(|record| record.capability_id == prior_capability_id)
        .ok_or_else(|| invalid("prior application capability is unknown or expired"))?;
    {
        let prior = &ledger.records[prior_index];
        if prior.state != CapabilityState::Active
            || prior.revoked_at_unix_seconds.is_some()
            || now_unix_seconds < prior.issued_at_unix_seconds
            || now_unix_seconds >= prior.expires_at_unix_seconds
            || now_unix_seconds < prior.expires_at_unix_seconds.saturating_sub(300)
        {
            return Err(invalid(
                "application capability renewal is outside the final renewal window",
            ));
        }
        if prior.claims != issue.claims {
            return Err(invalid(
                "application capability renewal cannot change authority or scope",
            ));
        }
    }
    let nonce_digest = digest_bytes(&issue.nonce);
    if ledger.records.iter().any(|record| {
        record.application_id == issue.claims.application_id
            && record.nonce_digest_sha256 == nonce_digest
    }) {
        return Err(invalid(
            "application capability renewal nonce replay detected",
        ));
    }

    let token = derive_token(&master, &issue);
    let capability_id = derive_capability_id(&master, &issue);
    let record = CapabilityRecord {
        application_id: issue.claims.application_id.clone(),
        request_id: issue.request_id.clone(),
        request_digest_sha256: issue.request_digest_sha256,
        nonce_digest_sha256: nonce_digest,
        capability_id,
        capability_digest_sha256: digest_text(&token),
        issued_at_unix_seconds: issue.issued_at_unix_seconds,
        expires_at_unix_seconds: issue.expires_at_unix_seconds,
        claims: issue.claims,
        accounted_bytes: 0,
        state: CapabilityState::Active,
        revoked_at_unix_seconds: None,
    };
    let result = issued(&record, token, false);
    ledger.records[prior_index].state = CapabilityState::Revoked;
    ledger.records[prior_index].revoked_at_unix_seconds = Some(now_unix_seconds);
    ledger.records.push(record);
    ledger.records.sort_by(|left, right| {
        left.application_id
            .cmp(&right.application_id)
            .then_with(|| left.request_id.cmp(&right.request_id))
    });
    validate_ledger(&ledger)?;
    write_ledger(ledger_path, &ledger)?;
    Ok(result)
}

/// Validate and account one provider request atomically. Failed requests do
/// not consume byte budget.
pub fn validate_and_account_application_capability(
    ledger_path: impl AsRef<Path>,
    opaque_capability: &str,
    request: &ApplicationCapabilityUse,
) -> Result<ValidatedApplicationCapability, DaemonServiceRuntimeError> {
    if !opaque_capability.starts_with(APPLICATION_OPAQUE_CAPABILITY_PREFIX) {
        return Err(invalid("opaque application capability is malformed"));
    }
    validate_use(request)?;
    let _guard = lock()?;
    let ledger_path = ledger_path.as_ref();
    let mut ledger = read_ledger(ledger_path)?;
    retain_live(&mut ledger, request.now_unix_seconds);
    let supplied_digest = digest_text(opaque_capability);
    let record = ledger
        .records
        .iter_mut()
        .find(|record| record.capability_digest_sha256 == supplied_digest)
        .ok_or_else(|| invalid("opaque application capability is unknown or expired"))?;
    if record.state != CapabilityState::Active
        || record.revoked_at_unix_seconds.is_some()
        || request.now_unix_seconds < record.issued_at_unix_seconds
        || request.now_unix_seconds >= record.expires_at_unix_seconds
    {
        return Err(invalid("opaque application capability is inactive"));
    }
    authorize_use(&record.claims, request)?;
    let next_bytes = record
        .accounted_bytes
        .checked_add(request.bytes)
        .ok_or_else(|| invalid("application capability byte accounting overflow"))?;
    if request.bytes > record.claims.max_object_bytes || next_bytes > record.claims.max_total_bytes
    {
        return Err(invalid("application capability byte budget exceeded"));
    }
    record.accounted_bytes = next_bytes;
    let result = ValidatedApplicationCapability {
        capability_id: record.capability_id.clone(),
        claims: record.claims.clone(),
        accounted_bytes: record.accounted_bytes,
        expires_at_unix_seconds: record.expires_at_unix_seconds,
    };
    write_ledger(ledger_path, &ledger)?;
    Ok(result)
}

pub fn revoke_application_capabilities(
    ledger_path: impl AsRef<Path>,
    application_id: &str,
    key_id: Option<&str>,
    binding_id: Option<&str>,
    revoked_at_unix_seconds: u64,
) -> Result<u64, DaemonServiceRuntimeError> {
    if application_id.trim().is_empty() || revoked_at_unix_seconds == 0 {
        return Err(invalid("application capability revocation is invalid"));
    }
    let _guard = lock()?;
    let ledger_path = ledger_path.as_ref();
    let mut ledger = read_ledger(ledger_path)?;
    let mut revoked = 0_u64;
    for record in &mut ledger.records {
        if record.application_id == application_id
            && key_id.is_none_or(|value| record.claims.key_id == value)
            && binding_id.is_none_or(|value| record.claims.binding_id == value)
            && record.state == CapabilityState::Active
        {
            record.state = CapabilityState::Revoked;
            record.revoked_at_unix_seconds = Some(revoked_at_unix_seconds);
            revoked += 1;
        }
    }
    write_ledger(ledger_path, &ledger)?;
    Ok(revoked)
}

fn authorize_use(
    claims: &ApplicationCapabilityClaims,
    request: &ApplicationCapabilityUse,
) -> Result<(), DaemonServiceRuntimeError> {
    if request.audience != claims.audience
        || request.store_id != claims.store_id
        || !claims
            .operations
            .iter()
            .any(|item| item == &request.operation)
        || !claims
            .prefixes
            .iter()
            .any(|prefix| prefix_contains(prefix, &request.object_key))
    {
        return Err(invalid("application capability scope denied"));
    }
    Ok(())
}

fn validate_issue(
    issue: &ApplicationCapabilityIssue,
    now: u64,
) -> Result<(), DaemonServiceRuntimeError> {
    if issue.request_id.trim().is_empty()
        || issue.nonce.len() != 32
        || !is_sha256(&issue.request_digest_sha256)
        || issue.issued_at_unix_seconds.abs_diff(now) > 30
        || issue.expires_at_unix_seconds <= issue.issued_at_unix_seconds
        || issue.expires_at_unix_seconds - issue.issued_at_unix_seconds > 900
    {
        return Err(invalid("application capability issue request is invalid"));
    }
    validate_claims(&issue.claims)
}

fn validate_claims(claims: &ApplicationCapabilityClaims) -> Result<(), DaemonServiceRuntimeError> {
    if claims.application_id.trim().is_empty()
        || claims.key_id.trim().is_empty()
        || claims.binding_id.trim().is_empty()
        || !is_sha256(&claims.binding_digest_sha256)
        || claims.tenant_id.trim().is_empty()
        || claims.host_authority.authority_id.trim().is_empty()
        || claims.host_authority.project_id.trim().is_empty()
        || claims.host_authority.project_revision == 0
        || claims.prosopikon_authority.authority_id.trim().is_empty()
        || claims.prosopikon_authority.authority_revision == 0
        || claims.audience.trim().is_empty()
        || claims.store_id.trim().is_empty()
        || claims.prefixes.is_empty()
        || claims.operations.is_empty()
        || claims.max_object_bytes == 0
        || claims.max_total_bytes == 0
        || claims.max_object_bytes > claims.max_total_bytes
        || claims
            .prefixes
            .iter()
            .any(|prefix| !valid_logical_prefix(prefix))
        || claims
            .operations
            .iter()
            .any(|operation| !matches!(operation.as_str(), "list" | "read" | "verify"))
    {
        return Err(invalid("application capability claims are invalid"));
    }
    Ok(())
}

fn validate_use(request: &ApplicationCapabilityUse) -> Result<(), DaemonServiceRuntimeError> {
    if request.audience.trim().is_empty()
        || request.store_id.trim().is_empty()
        || request.operation.trim().is_empty()
        || !valid_logical_prefix(&request.object_key)
    {
        return Err(invalid("application capability use is invalid"));
    }
    Ok(())
}

fn validate_ledger(ledger: &Ledger) -> Result<(), DaemonServiceRuntimeError> {
    if ledger.schema_version != APPLICATION_CAPABILITY_LEDGER_SCHEMA {
        return Err(invalid("unsupported application capability ledger schema"));
    }
    let mut requests = BTreeSet::new();
    let mut nonces = BTreeSet::new();
    let mut capabilities = BTreeSet::new();
    for record in &ledger.records {
        validate_claims(&record.claims)?;
        if record.application_id != record.claims.application_id
            || !is_sha256(&record.request_digest_sha256)
            || !is_sha256(&record.nonce_digest_sha256)
            || !is_sha256(&record.capability_digest_sha256)
            || record.issued_at_unix_seconds >= record.expires_at_unix_seconds
            || record.accounted_bytes > record.claims.max_total_bytes
            || !requests.insert((record.application_id.as_str(), record.request_id.as_str()))
            || !nonces.insert((
                record.application_id.as_str(),
                record.nonce_digest_sha256.as_str(),
            ))
            || !capabilities.insert(record.capability_digest_sha256.as_str())
            || (record.state == CapabilityState::Active && record.revoked_at_unix_seconds.is_some())
            || (record.state == CapabilityState::Revoked
                && record.revoked_at_unix_seconds.is_none())
        {
            return Err(invalid("application capability ledger is invalid"));
        }
    }
    Ok(())
}

fn retain_live(ledger: &mut Ledger, now: u64) {
    ledger
        .records
        .retain(|record| record.expires_at_unix_seconds.saturating_add(30) > now);
}

fn issued(
    record: &CapabilityRecord,
    token: String,
    exact_replay: bool,
) -> IssuedApplicationCapability {
    IssuedApplicationCapability {
        capability_id: record.capability_id.clone(),
        opaque_capability: token,
        issued_at_unix_seconds: record.issued_at_unix_seconds,
        expires_at_unix_seconds: record.expires_at_unix_seconds,
        exact_replay,
        claims: record.claims.clone(),
    }
}

fn derive_token(master: &[u8; 32], issue: &ApplicationCapabilityIssue) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, master);
    let context = format!(
        "dasobjectstore.application-capability.v2\0{}\0{}\0{}",
        issue.claims.application_id, issue.request_id, issue.request_digest_sha256
    );
    format!(
        "{APPLICATION_OPAQUE_CAPABILITY_PREFIX}{}",
        URL_SAFE_NO_PAD.encode(hmac::sign(&key, context.as_bytes()).as_ref())
    )
}

fn derive_capability_id(master: &[u8; 32], issue: &ApplicationCapabilityIssue) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, master);
    let context = format!(
        "dasobjectstore.application-capability-id.v2\0{}\0{}",
        issue.claims.application_id, issue.request_id
    );
    let encoded = URL_SAFE_NO_PAD.encode(hmac::sign(&key, context.as_bytes()).as_ref());
    format!("cap-{}", &encoded[..24])
}

fn load_or_create_master_key(path: &Path) -> Result<[u8; 32], DaemonServiceRuntimeError> {
    match File::open(path) {
        Ok(mut file) => {
            validate_private_file(path)?;
            let mut key = [0_u8; 32];
            file.read_exact(&mut key)
                .map_err(|error| invalid(format!("read {}: {error}", path.display())))?;
            let mut extra = [0_u8; 1];
            if file
                .read(&mut extra)
                .map_err(|error| invalid(error.to_string()))?
                != 0
            {
                return Err(invalid(
                    "application capability master key has invalid length",
                ));
            }
            Ok(key)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            ensure_parent(path)?;
            let mut key = [0_u8; 32];
            ring_rand::SystemRandom::new()
                .fill(&mut key)
                .map_err(|_| invalid("OS CSPRNG could not create capability master key"))?;
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
                file.write_all(&key)?;
                file.sync_all()?;
                match fs::hard_link(&temporary, path) {
                    Ok(()) => {
                        fs::remove_file(&temporary)?;
                    }
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                        fs::remove_file(&temporary)?;
                        validate_private_file(path).map_err(io::Error::other)?;
                        return load_existing_master_key(path);
                    }
                    Err(error) => return Err(error),
                }
                validate_private_file(path).map_err(io::Error::other)?;
                sync_parent(path)?;
                Ok(key)
            })();
            if result.is_err() {
                let _ = fs::remove_file(&temporary);
            }
            result.map_err(|error| invalid(format!("create {}: {error}", path.display())))
        }
        Err(error) => Err(invalid(format!("open {}: {error}", path.display()))),
    }
}

fn load_existing_master_key(path: &Path) -> io::Result<[u8; 32]> {
    let bytes = fs::read(path)?;
    bytes
        .try_into()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid master key length"))
}

fn read_ledger(path: &Path) -> Result<Ledger, DaemonServiceRuntimeError> {
    match File::open(path) {
        Ok(file) => {
            validate_private_file(path)?;
            let ledger: Ledger = serde_json::from_reader(file)
                .map_err(|error| invalid(format!("parse {}: {error}", path.display())))?;
            validate_ledger(&ledger)?;
            Ok(ledger)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Ledger::default()),
        Err(error) => Err(invalid(format!("open {}: {error}", path.display()))),
    }
}

fn write_ledger(path: &Path, ledger: &Ledger) -> Result<(), DaemonServiceRuntimeError> {
    validate_ledger(ledger)?;
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
        serde_json::to_writer_pretty(&mut file, ledger).map_err(io::Error::other)?;
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

fn lock() -> Result<std::sync::MutexGuard<'static, ()>, DaemonServiceRuntimeError> {
    LEDGER_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| invalid("application capability ledger lock poisoned"))
}

fn ensure_parent(path: &Path) -> Result<(), DaemonServiceRuntimeError> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid("capability authority path has no parent"))?;
    fs::create_dir_all(parent)
        .map_err(|error| invalid(format!("create {}: {error}", parent.display())))
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

fn digest_text(value: &str) -> String {
    digest_bytes(value.as_bytes())
}

fn digest_bytes(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_logical_prefix(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.contains('\\')
        && value
            .split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}

fn prefix_contains(prefix: &str, key: &str) -> bool {
    key == prefix
        || key
            .strip_prefix(prefix)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn invalid(message: impl Into<String>) -> DaemonServiceRuntimeError {
    DaemonServiceRuntimeError::UnsupportedOperation {
        operation: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dasobjectstore_core::application_auth_v2::{
        GovernedHostAuthorityV2, GovernedHostModeV2, GovernedProsopikonAuthorityV2,
    };
    use std::sync::{Arc, Barrier};
    use std::thread;

    fn claims() -> ApplicationCapabilityClaims {
        ApplicationCapabilityClaims {
            application_id: "app-7e4a31c9b260".to_string(),
            key_id: "ergasterion-ed25519".to_string(),
            binding_id: "binding-current".to_string(),
            binding_digest_sha256: "b".repeat(64),
            tenant_id: "6dd29575-9763-4e2b-8255-6bf7380f3813".to_string(),
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
            audience: "ergasterion-governed-data-service".to_string(),
            store_id: "science".to_string(),
            prefixes: vec!["project-rna/inputs".to_string()],
            operations: vec!["list".to_string(), "read".to_string(), "verify".to_string()],
            max_object_bytes: 10,
            max_total_bytes: 20,
        }
    }

    fn issue(request_id: &str, nonce: u8) -> ApplicationCapabilityIssue {
        ApplicationCapabilityIssue {
            request_id: request_id.to_string(),
            request_digest_sha256: "a".repeat(64),
            nonce: vec![nonce; 32],
            issued_at_unix_seconds: 100,
            expires_at_unix_seconds: 1_000,
            claims: claims(),
        }
    }

    #[test]
    fn exact_replay_survives_restart_and_plaintext_is_not_persisted() {
        let (ledger, master) = fixture("restart");
        let first =
            issue_opaque_application_capability(&ledger, &master, issue("request-1", 1), 100)
                .unwrap();
        let replay =
            issue_opaque_application_capability(&ledger, &master, issue("request-1", 1), 101)
                .unwrap();
        assert_eq!(first.opaque_capability, replay.opaque_capability);
        assert!(replay.exact_replay);
        let persisted = fs::read_to_string(&ledger).unwrap();
        assert!(!persisted.contains(&first.opaque_capability));
        assert!(!persisted.contains(&URL_SAFE_NO_PAD.encode(vec![1_u8; 32])));
        cleanup(&ledger, &master);
    }

    #[test]
    fn changed_request_and_reused_nonce_fail() {
        let (ledger, master) = fixture("replay");
        issue_opaque_application_capability(&ledger, &master, issue("request-1", 1), 100).unwrap();
        let mut changed = issue("request-1", 1);
        changed.request_digest_sha256 = "c".repeat(64);
        assert!(issue_opaque_application_capability(&ledger, &master, changed, 100).is_err());
        assert!(
            issue_opaque_application_capability(&ledger, &master, issue("request-2", 1), 100)
                .is_err()
        );
        cleanup(&ledger, &master);
    }

    #[test]
    fn concurrent_exchange_issues_one_exact_capability() {
        let (ledger, master) = fixture("concurrent");
        let barrier = Arc::new(Barrier::new(5));
        let handles = (0..4)
            .map(|_| {
                let ledger = ledger.clone();
                let master = master.clone();
                let barrier = barrier.clone();
                thread::spawn(move || {
                    barrier.wait();
                    issue_opaque_application_capability(ledger, master, issue("request-1", 1), 100)
                        .unwrap()
                        .opaque_capability
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        let tokens = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<BTreeSet<_>>();
        assert_eq!(tokens.len(), 1);
        cleanup(&ledger, &master);
    }

    #[test]
    fn renewal_is_final_window_only_atomic_and_exactly_replayable() {
        let (ledger, master) = fixture("renewal");
        let prior =
            issue_opaque_application_capability(&ledger, &master, issue("request-1", 1), 100)
                .unwrap();
        let mut too_early = issue("renewal-early", 2);
        too_early.issued_at_unix_seconds = 699;
        too_early.expires_at_unix_seconds = 1_500;
        assert!(renew_opaque_application_capability(
            &ledger,
            &master,
            &prior.capability_id,
            too_early,
            699
        )
        .is_err());

        let mut renewal = issue("renewal-1", 3);
        renewal.issued_at_unix_seconds = 700;
        renewal.expires_at_unix_seconds = 1_500;
        let replacement = renew_opaque_application_capability(
            &ledger,
            &master,
            &prior.capability_id,
            renewal.clone(),
            700,
        )
        .unwrap();
        assert!(!replacement.exact_replay);
        let replay = renew_opaque_application_capability(
            &ledger,
            &master,
            &prior.capability_id,
            renewal,
            701,
        )
        .unwrap();
        assert!(replay.exact_replay);
        assert_eq!(replacement.opaque_capability, replay.opaque_capability);

        let use_request = ApplicationCapabilityUse {
            audience: claims().audience,
            store_id: "science".to_string(),
            object_key: "project-rna/inputs/a.cram".to_string(),
            operation: "read".to_string(),
            bytes: 1,
            now_unix_seconds: 701,
        };
        assert!(validate_and_account_application_capability(
            &ledger,
            &prior.opaque_capability,
            &use_request
        )
        .is_err());
        assert!(validate_and_account_application_capability(
            &ledger,
            &replacement.opaque_capability,
            &use_request
        )
        .is_ok());
        let persisted = fs::read_to_string(&ledger).unwrap();
        assert!(!persisted.contains(&replacement.opaque_capability));
        cleanup(&ledger, &master);
    }

    #[test]
    fn validation_rejects_tamper_expiry_scope_budget_and_revocation() {
        let (ledger, master) = fixture("validation");
        let issued =
            issue_opaque_application_capability(&ledger, &master, issue("request-1", 1), 100)
                .unwrap();
        let use_request = ApplicationCapabilityUse {
            audience: claims().audience,
            store_id: "science".to_string(),
            object_key: "project-rna/inputs/a.cram".to_string(),
            operation: "read".to_string(),
            bytes: 10,
            now_unix_seconds: 101,
        };
        assert!(validate_and_account_application_capability(
            &ledger,
            &issued.opaque_capability,
            &use_request
        )
        .is_ok());
        assert!(validate_and_account_application_capability(
            &ledger,
            &format!("{}x", issued.opaque_capability),
            &use_request
        )
        .is_err());
        let mut outside = use_request.clone();
        outside.object_key = "another-project/a.cram".to_string();
        assert!(validate_and_account_application_capability(
            &ledger,
            &issued.opaque_capability,
            &outside
        )
        .is_err());
        let mut over_budget = use_request.clone();
        over_budget.bytes = 11;
        assert!(validate_and_account_application_capability(
            &ledger,
            &issued.opaque_capability,
            &over_budget
        )
        .is_err());
        assert_eq!(
            revoke_application_capabilities(&ledger, &claims().application_id, None, None, 102)
                .unwrap(),
            1
        );
        assert!(validate_and_account_application_capability(
            &ledger,
            &issued.opaque_capability,
            &use_request
        )
        .is_err());

        let second =
            issue_opaque_application_capability(&ledger, &master, issue("request-2", 2), 100)
                .unwrap();
        let mut expired = use_request;
        expired.now_unix_seconds = 1_001;
        assert!(validate_and_account_application_capability(
            &ledger,
            &second.opaque_capability,
            &expired
        )
        .is_err());
        cleanup(&ledger, &master);
    }

    #[cfg(unix)]
    #[test]
    fn ledger_and_master_key_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let (ledger, master) = fixture("private");
        issue_opaque_application_capability(&ledger, &master, issue("request-1", 1), 100).unwrap();
        assert_eq!(
            fs::metadata(&ledger).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(&master).unwrap().permissions().mode() & 0o777,
            0o600
        );
        cleanup(&ledger, &master);
    }

    fn fixture(label: &str) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-capability-ledger-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        (
            root.join(APPLICATION_CAPABILITY_LEDGER_FILE_NAME),
            root.join(APPLICATION_CAPABILITY_MASTER_KEY_FILE_NAME),
        )
    }

    fn cleanup(ledger: &Path, master: &Path) {
        if let Some(root) = ledger.parent() {
            let _ = fs::remove_dir_all(root);
        }
        let _ = fs::remove_file(master);
    }
}
