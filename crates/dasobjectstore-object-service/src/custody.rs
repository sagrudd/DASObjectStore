//! Local trusted-administrator custody-retention overlay.
//!
//! Garage is S3-compatible storage; it does **not** provide a storage-provider
//! immutability boundary.  It supplies a
//! deliberately narrow, application-enforced custody ledger for a fresh,
//! dedicated Garage bucket.  The supported DAS APIs cannot delete, overwrite,
//! shorten retention, clear a legal hold, or replace a sealed configuration.
//!
//! A NUC administrator who can alter Garage, Docker, this SQLite file, the
//! host clock, its disks, or backups can still defeat the overlay.  Callers
//! must record that trusted-administrator assumption in the release policy;
//! this module is never evidence of independently administered or regulatory
//! independently administered retained storage.

use crate::provider::ObjectServiceError;
use chrono::{DateTime, SecondsFormat, Utc};
use dasobjectstore_core::ids::StoreId;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

pub const CUSTODY_OVERLAY_SCHEMA_V1: &str =
    "dasobjectstore.local_trusted_administrator_custody_overlay.v1";
pub const CUSTODY_PROFILE_V1: &str = "local_garage_trusted_administrator_custody_overlay_v1";
pub const CUSTODY_ASSURANCE_CLASS_LOCAL_TRUSTED_ADMINISTRATOR_OVERLAY: &str =
    "local_trusted_administrator_overlay";
pub const CUSTODY_RECEIPT_SCHEMA_V1: &str =
    "dasobjectstore.local_trusted_administrator_custody_readback_receipt.v1";
pub const R237_BOOTSTRAP_STORE_ID: &str = "r237_s4_bootstrap_custody";
pub const R237_BOOTSTRAP_BUCKET_NAME: &str = "dos-r237-s4-bootstrap-custody";

const CUSTODY_OBJECT_PREFIX: &str = "custody/sha256/";
const CUSTODY_LEDGER_SCHEMA_VERSION: i64 = 1;

/// The only assurance class implemented by this source release.  It is
/// intentionally explicit so a caller cannot relabel local Garage as an
/// independently administered provider.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CustodyAssuranceClass {
    LocalTrustedAdministratorOverlay,
}

impl CustodyAssuranceClass {
    pub fn name(self) -> &'static str {
        match self {
            Self::LocalTrustedAdministratorOverlay => {
                CUSTODY_ASSURANCE_CLASS_LOCAL_TRUSTED_ADMINISTRATOR_OVERLAY
            }
        }
    }
}

/// The only retention mode is append-only with a permanent legal hold.  There
/// is no governance/bypass mode.  Adding a future mode would require a new
/// reviewed schema and release policy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CustodyRetentionMode {
    LocalTrustedAdministratorOverlay,
}

/// A target-bound, sealed profile for one fresh custody store.
///
/// Credential values are never represented here; only the separate custody
/// references and reader identity are durable.  The normal mutable registry
/// and normal Garage provisioning APIs refuse this profile.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CustodyStoreProfileV1 {
    pub schema: String,
    pub profile: String,
    pub assurance_class: CustodyAssuranceClass,
    pub retention_mode: CustodyRetentionMode,
    pub target_id: String,
    pub retention_until_utc: String,
    pub legal_hold: bool,
    pub writer_credential_reference: String,
    pub reader_credential_reference: String,
    pub reader_identity: String,
}

impl CustodyStoreProfileV1 {
    pub fn validate(&self) -> Result<(), ObjectServiceError> {
        if self.schema != CUSTODY_OVERLAY_SCHEMA_V1 {
            return Err(invalid("unsupported custody overlay schema"));
        }
        if self.profile != CUSTODY_PROFILE_V1 {
            return Err(invalid("unsupported custody store profile"));
        }
        if self.assurance_class != CustodyAssuranceClass::LocalTrustedAdministratorOverlay {
            return Err(invalid("unsupported custody assurance class"));
        }
        if self.retention_mode != CustodyRetentionMode::LocalTrustedAdministratorOverlay {
            return Err(invalid(
                "only append-only legal-hold custody retention is supported",
            ));
        }
        require_nonblank("target_id", &self.target_id)?;
        canonical_timestamp("retention_until_utc", &self.retention_until_utc)?;
        if !self.legal_hold {
            return Err(invalid(
                "a custody profile must require a legal hold; hold release is not implemented",
            ));
        }
        require_nonblank(
            "writer_credential_reference",
            &self.writer_credential_reference,
        )?;
        require_nonblank(
            "reader_credential_reference",
            &self.reader_credential_reference,
        )?;
        if self.writer_credential_reference == self.reader_credential_reference {
            return Err(invalid(
                "custody reader and writer credential references must be distinct",
            ));
        }
        require_nonblank("reader_identity", &self.reader_identity)?;
        Ok(())
    }
}

/// Returns true for a historical bootstrap namespace which must never be
/// adopted by the custody overlay.
pub fn custody_bucket_is_reserved(bucket_name: &str) -> bool {
    bucket_name == R237_BOOTSTRAP_BUCKET_NAME
}

/// Content-addressed object keys have no mutable caller-controlled component.
pub fn custody_object_key(content_sha256: &str) -> Result<String, ObjectServiceError> {
    validate_sha256("content_sha256", content_sha256)?;
    Ok(format!("{CUSTODY_OBJECT_PREFIX}{content_sha256}"))
}

/// A Garage key used only for a custody writer or independent reader.
///
/// The secret is intentionally redacted from Debug and all provisioning plan
/// renderings.  It is not compatible with the normal owner-capable key path.
#[derive(Clone, Eq, PartialEq)]
pub struct CustodyGarageCredential {
    pub credential_reference: String,
    pub access_key_id: String,
    secret_access_key: String,
}

impl CustodyGarageCredential {
    pub fn new(
        credential_reference: impl Into<String>,
        access_key_id: impl Into<String>,
        secret_access_key: impl Into<String>,
    ) -> Result<Self, ObjectServiceError> {
        let credential = Self {
            credential_reference: credential_reference.into(),
            access_key_id: access_key_id.into(),
            secret_access_key: secret_access_key.into(),
        };
        require_nonblank(
            "custody credential reference",
            &credential.credential_reference,
        )?;
        require_nonblank("custody access key id", &credential.access_key_id)?;
        require_nonblank("custody secret access key", &credential.secret_access_key)?;
        Ok(credential)
    }

    fn secret_access_key(&self) -> &str {
        &self.secret_access_key
    }
}

impl fmt::Debug for CustodyGarageCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CustodyGarageCredential")
            .field("credential_reference", &self.credential_reference)
            .field("access_key_id", &self.access_key_id)
            .field("secret_access_key", &"REDACTED")
            .finish()
    }
}

/// All information required for a dedicated, non-owner custody bucket plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CustodyGarageProvisioningRequest {
    pub store_id: StoreId,
    pub bucket_name: String,
    pub profile: CustodyStoreProfileV1,
    /// Separate attended Garage-administration identity. It creates the fresh
    /// bucket and two runtime grants only; it is neither retained nor emitted
    /// by the plan and is not a runtime object credential.
    pub provisioner: CustodyGarageProvisionerIdentity,
    pub writer: CustodyGarageCredential,
    pub reader: CustodyGarageCredential,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CustodyGarageProvisionerIdentity {
    pub identity: String,
    pub credential_reference: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CustodyGarageProvisioningPlan {
    pub commands: Vec<CustodyGarageProvisioningCommand>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CustodyGarageProvisioningCommand {
    pub operation: CustodyGarageProvisioningOperation,
    args: Vec<CustodyGarageArgument>,
}

impl CustodyGarageProvisioningCommand {
    pub fn argv(&self) -> Vec<String> {
        self.args.iter().map(CustodyGarageArgument::value).collect()
    }

    pub fn redacted_argv(&self) -> Vec<String> {
        self.args
            .iter()
            .map(CustodyGarageArgument::redacted_value)
            .collect()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CustodyGarageProvisioningOperation {
    ImportWriterKey,
    ImportReaderKey,
    CreateFreshBucket,
    AllowWriter,
    AllowReader,
}

#[derive(Clone, Eq, PartialEq)]
struct CustodyGarageArgument {
    value: String,
    sensitive: bool,
}

impl fmt::Debug for CustodyGarageArgument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.redacted_value())
    }
}

impl CustodyGarageArgument {
    fn public(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            sensitive: false,
        }
    }

    fn sensitive(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            sensitive: true,
        }
    }

    fn value(&self) -> String {
        self.value.clone()
    }

    fn redacted_value(&self) -> String {
        if self.sensitive {
            "<redacted>".to_string()
        } else {
            self.value.clone()
        }
    }
}

/// Build a reviewable Garage plan.  This plan has no executor.  The only
/// grants are write for the writer and read for the reader; it never contains
/// `--owner`, list, delete, copy, lifecycle, or bucket-administration rights.
pub fn plan_custody_garage_provisioning(
    request: &CustodyGarageProvisioningRequest,
) -> Result<CustodyGarageProvisioningPlan, ObjectServiceError> {
    validate_custody_provisioning_request(request)?;
    let writer_name = format!("dasobjectstore:custody:{}:writer", request.store_id);
    let reader_name = format!("dasobjectstore:custody:{}:reader", request.store_id);
    let import = |operation, name: String, credential: &CustodyGarageCredential| {
        CustodyGarageProvisioningCommand {
            operation,
            args: vec![
                CustodyGarageArgument::public("key"),
                CustodyGarageArgument::public("import"),
                CustodyGarageArgument::public("--yes"),
                CustodyGarageArgument::public("-n"),
                CustodyGarageArgument::public(name),
                CustodyGarageArgument::public(&credential.access_key_id),
                CustodyGarageArgument::sensitive(credential.secret_access_key()),
            ],
        }
    };
    let allow = |operation, capability: &str, credential: &CustodyGarageCredential| {
        CustodyGarageProvisioningCommand {
            operation,
            args: vec![
                CustodyGarageArgument::public("bucket"),
                CustodyGarageArgument::public("allow"),
                CustodyGarageArgument::public(capability),
                CustodyGarageArgument::public(&request.bucket_name),
                CustodyGarageArgument::public("--key"),
                CustodyGarageArgument::public(&credential.access_key_id),
            ],
        }
    };

    Ok(CustodyGarageProvisioningPlan {
        commands: vec![
            import(
                CustodyGarageProvisioningOperation::ImportWriterKey,
                writer_name,
                &request.writer,
            ),
            import(
                CustodyGarageProvisioningOperation::ImportReaderKey,
                reader_name,
                &request.reader,
            ),
            CustodyGarageProvisioningCommand {
                operation: CustodyGarageProvisioningOperation::CreateFreshBucket,
                args: vec![
                    CustodyGarageArgument::public("bucket"),
                    CustodyGarageArgument::public("create"),
                    CustodyGarageArgument::public(&request.bucket_name),
                ],
            },
            allow(
                CustodyGarageProvisioningOperation::AllowWriter,
                "--write",
                &request.writer,
            ),
            allow(
                CustodyGarageProvisioningOperation::AllowReader,
                "--read",
                &request.reader,
            ),
        ],
    })
}

/// State returned by a writer before an immutable content-addressed put.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CustodyObjectState {
    Missing,
    Existing {
        content_sha256: String,
        content_length: u64,
    },
}

/// The writer boundary may only implement create-if-absent semantics.  An S3
/// `PutObject` implementation that overwrites an existing key cannot satisfy
/// this trait's contract.
pub trait CustodyObjectWriter {
    fn identity(&self) -> &str;
    fn object_state(&mut self, object_key: &str) -> Result<CustodyObjectState, ObjectServiceError>;
    fn put_if_absent(&mut self, object_key: &str, bytes: &[u8]) -> Result<(), ObjectServiceError>;
}

/// A distinct reader boundary that recomputes readback bytes.  A writer-side
/// response is not sufficient for a custody receipt.
pub trait CustodyObjectReader {
    fn identity(&self) -> &str;
    fn read_exact(&mut self, object_key: &str) -> Result<Vec<u8>, ObjectServiceError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CustodyObjectInputV1 {
    pub object_type: String,
    pub bytes: Vec<u8>,
    pub retained_at_utc: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CustodyReadbackObservationV1 {
    pub reader_identity: String,
    pub observed_at_utc: String,
    pub content_sha256: String,
    pub content_length: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CustodyIntegrityReceiptV1 {
    pub schema: String,
    pub assurance_class: String,
    pub store_id: StoreId,
    pub bucket_name: String,
    pub target_id: String,
    pub object_id: String,
    pub object_key: String,
    pub content_sha256: String,
    pub content_length: u64,
    pub object_type: String,
    pub version: u64,
    pub retention_until_utc: String,
    pub legal_hold: bool,
    pub reader_identity: String,
    pub observed_at_utc: String,
    pub configuration_sha256: String,
    pub ledger_event_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CustodyLedgerInspectionV1 {
    pub schema: String,
    pub ledger_path: PathBuf,
    pub store_id: StoreId,
    pub bucket_name: String,
    pub target_id: String,
    pub assurance_class: String,
    pub configuration_sha256: String,
    pub committed_object_versions: u64,
    pub committed_receipts: u64,
    pub ledger_head_sha256: String,
}

/// Mutations for which this custody overlay has no supported API.  Keeping
/// this exhaustive makes adapters reject a newly introduced S3 mutation until
/// it has been reviewed against the same sealed ledger state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CustodyForbiddenMutation {
    Delete,
    Overwrite,
    Copy,
    MultipartUpload,
    Restore,
    Reconcile,
    Lifecycle,
    ShortenRetention,
    ClearLegalHold,
    ReplaceConfiguration,
    ReinitialiseLedger,
}

/// Returns the permanent fail-closed result for an unsupported custody
/// mutation.  There is deliberately no bypass or administrative variant.
pub fn reject_custody_mutation(
    operation: CustodyForbiddenMutation,
) -> Result<(), ObjectServiceError> {
    Err(invalid(format!(
        "custody mutation {} is forbidden by the local trusted-administrator overlay",
        custody_forbidden_mutation_name(operation)
    )))
}

fn custody_forbidden_mutation_name(operation: CustodyForbiddenMutation) -> &'static str {
    match operation {
        CustodyForbiddenMutation::Delete => "delete",
        CustodyForbiddenMutation::Overwrite => "overwrite",
        CustodyForbiddenMutation::Copy => "copy",
        CustodyForbiddenMutation::MultipartUpload => "multipart_upload",
        CustodyForbiddenMutation::Restore => "restore",
        CustodyForbiddenMutation::Reconcile => "reconcile",
        CustodyForbiddenMutation::Lifecycle => "lifecycle",
        CustodyForbiddenMutation::ShortenRetention => "shorten_retention",
        CustodyForbiddenMutation::ClearLegalHold => "clear_legal_hold",
        CustodyForbiddenMutation::ReplaceConfiguration => "replace_configuration",
        CustodyForbiddenMutation::ReinitialiseLedger => "reinitialise_ledger",
    }
}

pub const CUSTODY_OFF_NUC_ATTESTATION_SCHEMA_V1: &str =
    "dasobjectstore.local_trusted_administrator_custody_off_nuc_attestation.v1";

/// A signed, expiring observation made by an off-NUC verifier after it has
/// independently read the object and ledger.  This record binds the custody
/// result to the DAS binary/image/configuration/endpoint and the full
/// inventory and marker digests observed by that verifier.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CustodyOffNucAttestationV1 {
    pub body: CustodyOffNucAttestationBodyV1,
    pub authority_id: String,
    pub signature: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CustodyOffNucAttestationBodyV1 {
    pub schema: String,
    pub assurance_class: String,
    pub verifier_id: String,
    pub target_id: String,
    pub nonce: String,
    pub sequence: u64,
    pub previous_attestation_sha256: Option<String>,
    pub issued_at_utc: String,
    pub expires_at_utc: String,
    pub receipt: CustodyIntegrityReceiptV1,
    pub ledger_head_sha256: String,
    pub das_executable_sha256: String,
    pub garage_image_digest: String,
    pub garage_config_sha256: String,
    pub s3_endpoint: String,
    pub full_inventory_sha256: String,
    pub custody_marker_sha256: String,
}

/// Durable state held *outside* the NUC, Garage, BaseCamp, and ordinary host
/// backup paths.  Its implementation must atomically compare and persist the
/// previous checkpoint; this trait keeps that custody boundary explicit.
pub trait CustodyOffNucVerifierState {
    fn checkpoint(
        &self,
        target_id: &str,
    ) -> Result<Option<CustodyOffNucVerifierCheckpointV1>, ObjectServiceError>;
    fn nonce_seen(&self, target_id: &str, nonce: &str) -> Result<bool, ObjectServiceError>;
    fn compare_and_store(
        &mut self,
        expected_previous: Option<&CustodyOffNucVerifierCheckpointV1>,
        next: CustodyOffNucVerifierCheckpointV1,
    ) -> Result<(), ObjectServiceError>;
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CustodyOffNucVerifierCheckpointV1 {
    pub target_id: String,
    pub authority_id: String,
    pub verifier_id: String,
    pub sequence: u64,
    pub nonce: String,
    pub attestation_sha256: String,
    pub ledger_head_sha256: String,
    pub expires_at_utc: String,
}

/// The off-NUC authority owns the signing key.  DAS only receives the public
/// verification capability; it never generates, finds, imports, or stores a
/// signer credential.
pub trait CustodyOffNucAttestationAuthority {
    fn authority_id(&self) -> &str;
    fn verify(&self, canonical_body: &[u8], signature: &str) -> Result<(), ObjectServiceError>;
}

/// Verify a signed off-NUC attestation and atomically advance independent
/// monotonic state.  Replayed, stale, substituted, expired, unsigned, and
/// rollback observations are all rejected before checkpoint mutation.
pub fn accept_custody_off_nuc_attestation(
    attestation: &CustodyOffNucAttestationV1,
    expected_target_id: &str,
    now_utc: &str,
    authority: &impl CustodyOffNucAttestationAuthority,
    state: &mut impl CustodyOffNucVerifierState,
) -> Result<CustodyOffNucVerifierCheckpointV1, ObjectServiceError> {
    let body = &attestation.body;
    validate_off_nuc_attestation_body(body, expected_target_id, now_utc)?;
    if attestation.authority_id != authority.authority_id() {
        return Err(invalid(
            "custody attestation authority does not match pinned verifier authority",
        ));
    }
    require_nonblank("custody attestation signature", &attestation.signature)?;
    let canonical_body = canonical_json(body)?;
    authority.verify(canonical_body.as_bytes(), &attestation.signature)?;
    if state.nonce_seen(&body.target_id, &body.nonce)? {
        return Err(invalid(
            "custody attestation nonce has already been accepted",
        ));
    }
    let previous = state.checkpoint(&body.target_id)?;
    match &previous {
        None if body.sequence == 1 && body.previous_attestation_sha256.is_none() => {}
        Some(checkpoint)
            if body.sequence
                == checkpoint
                    .sequence
                    .checked_add(1)
                    .ok_or_else(|| invalid("custody verifier checkpoint sequence overflow"))?
                && body.previous_attestation_sha256.as_deref()
                    == Some(checkpoint.attestation_sha256.as_str()) => {}
        _ => {
            return Err(invalid(
                "custody attestation sequence or previous hash does not continue off-NUC state",
            ));
        }
    }
    let attestation_sha256 = sha256_hex(canonical_json(attestation)?.as_bytes());
    let checkpoint = CustodyOffNucVerifierCheckpointV1 {
        target_id: body.target_id.clone(),
        authority_id: attestation.authority_id.clone(),
        verifier_id: body.verifier_id.clone(),
        sequence: body.sequence,
        nonce: body.nonce.clone(),
        attestation_sha256,
        ledger_head_sha256: body.ledger_head_sha256.clone(),
        expires_at_utc: body.expires_at_utc.clone(),
    };
    state.compare_and_store(previous.as_ref(), checkpoint.clone())?;
    Ok(checkpoint)
}

fn validate_off_nuc_attestation_body(
    body: &CustodyOffNucAttestationBodyV1,
    expected_target_id: &str,
    now_utc: &str,
) -> Result<(), ObjectServiceError> {
    if body.schema != CUSTODY_OFF_NUC_ATTESTATION_SCHEMA_V1
        || body.assurance_class != CUSTODY_ASSURANCE_CLASS_LOCAL_TRUSTED_ADMINISTRATOR_OVERLAY
        || body.target_id != expected_target_id
        || body.receipt.target_id != body.target_id
        || body.receipt.assurance_class
            != CUSTODY_ASSURANCE_CLASS_LOCAL_TRUSTED_ADMINISTRATOR_OVERLAY
    {
        return Err(invalid(
            "custody attestation is not bound to the expected overlay target",
        ));
    }
    require_nonblank("custody verifier id", &body.verifier_id)?;
    require_nonblank("custody attestation nonce", &body.nonce)?;
    if body.sequence == 0 {
        return Err(invalid(
            "custody attestation sequence must be greater than zero",
        ));
    }
    let issued = canonical_timestamp("custody attestation issued_at_utc", &body.issued_at_utc)?;
    let expires = canonical_timestamp("custody attestation expires_at_utc", &body.expires_at_utc)?;
    let now = canonical_timestamp("custody verifier now_utc", now_utc)?;
    if issued > now || expires <= now || expires <= issued {
        return Err(invalid("custody attestation is not currently valid"));
    }
    validate_sha256("custody ledger head", &body.ledger_head_sha256)?;
    validate_sha256("DAS executable", &body.das_executable_sha256)?;
    validate_sha256("Garage configuration", &body.garage_config_sha256)?;
    validate_sha256("full inventory", &body.full_inventory_sha256)?;
    validate_sha256("custody marker", &body.custody_marker_sha256)?;
    require_nonblank("Garage image digest", &body.garage_image_digest)?;
    require_nonblank("S3 endpoint", &body.s3_endpoint)?;
    if body.receipt.ledger_event_sha256.is_empty() || body.receipt.configuration_sha256.is_empty() {
        return Err(invalid(
            "custody attestation receipt omits immutable ledger binding",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct CustodySealedConfigurationV1 {
    schema: String,
    store_id: StoreId,
    bucket_name: String,
    profile: CustodyStoreProfileV1,
    created_at_utc: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct CustodyLedgerEventV1 {
    schema: String,
    sequence: u64,
    operation: String,
    object_id: String,
    object_key: String,
    content_sha256: String,
    content_length: u64,
    object_type: String,
    version: u64,
    retention_until_utc: String,
    legal_hold: bool,
    observed_at_utc: String,
    previous_event_sha256: Option<String>,
}

#[derive(Clone, Debug)]
struct StoredObjectVersion {
    object_id: String,
    object_key: String,
    content_sha256: String,
    content_length: u64,
    object_type: String,
    version: u64,
    retention_until_utc: String,
    legal_hold: bool,
    event_sha256: String,
}

/// Create a fresh custody ledger.  Existing paths are terminal refusals: no
/// existing mutable registry, bucket, ledger, or bootstrap namespace can be
/// silently adopted or repaired by this API.
pub fn create_custody_ledger(
    path: impl AsRef<Path>,
    store_id: StoreId,
    bucket_name: impl Into<String>,
    profile: CustodyStoreProfileV1,
    created_at_utc: impl AsRef<str>,
) -> Result<CustodyLedgerInspectionV1, ObjectServiceError> {
    profile.validate()?;
    let path = path.as_ref();
    let bucket_name = bucket_name.into();
    validate_new_custody_store(&store_id, &bucket_name, &profile, created_at_utc.as_ref())?;
    create_private_new_file(path)?;

    let mut connection = open_ledger(path)?;
    initialise_schema(&mut connection)?;
    let configuration = CustodySealedConfigurationV1 {
        schema: CUSTODY_OVERLAY_SCHEMA_V1.to_string(),
        store_id,
        bucket_name,
        profile,
        created_at_utc: created_at_utc.as_ref().to_string(),
    };
    let configuration_jcs = canonical_json(&configuration)?;
    let configuration_sha256 = sha256_hex(configuration_jcs.as_bytes());
    let transaction = connection
        .transaction()
        .map_err(sql_error("start custody configuration transaction"))?;
    transaction
        .execute(
            "INSERT INTO custody_store_configuration \
             (singleton, schema_version, configuration_jcs, configuration_sha256) \
             VALUES (1, ?1, ?2, ?3)",
            params![
                CUSTODY_LEDGER_SCHEMA_VERSION,
                configuration_jcs,
                configuration_sha256
            ],
        )
        .map_err(sql_error("seal custody store configuration"))?;
    transaction
        .commit()
        .map_err(sql_error("commit custody store configuration"))?;
    sync_ledger(path)?;
    inspect_custody_ledger(path)
}

/// Retain one content-addressed object after a separate reader has read back
/// the exact bytes.  The ledger is committed only after the readback digest
/// and length match; a crash after a backend put but before ledger commit
/// leaves an orphan and fails closed rather than deleting or repairing it.
pub fn retain_custody_object_with_readback(
    path: impl AsRef<Path>,
    input: CustodyObjectInputV1,
    writer: &mut impl CustodyObjectWriter,
    reader: &mut impl CustodyObjectReader,
) -> Result<CustodyIntegrityReceiptV1, ObjectServiceError> {
    let path = path.as_ref();
    validate_custody_input(&input)?;
    let configuration = read_sealed_configuration(path)?;
    validate_reader_writer(&configuration.profile, writer.identity(), reader.identity())?;
    if configuration.profile.retention_until_utc <= input.retained_at_utc {
        return Err(invalid(
            "custody retention_until_utc must be after the retained object timestamp",
        ));
    }

    let content_sha256 = sha256_hex(&input.bytes);
    let object_key = custody_object_key(&content_sha256)?;
    let object_id = format!("sha256:{content_sha256}");
    let content_length = input.bytes.len() as u64;

    let mut connection = open_ledger(path)?;
    let existing = latest_object_version(&connection, &object_id)?;
    if let Some(existing) = existing {
        ensure_exact_existing_object(
            &existing,
            &object_key,
            &content_sha256,
            content_length,
            &input.object_type,
        )?;
        let observation = independent_readback(
            reader,
            &object_key,
            &content_sha256,
            content_length,
            &input.retained_at_utc,
        )?;
        let receipt =
            existing_receipt(&connection, &object_id, existing.version)?.ok_or_else(|| {
                invalid(
                    "existing custody object has no immutable readback receipt; refusing adoption",
                )
            })?;
        verify_receipt_fields(&configuration, &existing, &receipt, &observation)?;
        return Ok(receipt);
    }

    match writer.object_state(&object_key)? {
        CustodyObjectState::Missing => writer.put_if_absent(&object_key, &input.bytes)?,
        CustodyObjectState::Existing {
            content_sha256: existing_sha256,
            content_length: existing_length,
        } if existing_sha256 == content_sha256 && existing_length == content_length => {
            return Err(invalid(
                "Garage contains an unledgered custody object; adoption is forbidden",
            ));
        }
        CustodyObjectState::Existing { .. } => {
            return Err(invalid(
                "content-addressed custody key collision or overwrite risk detected",
            ));
        }
    }

    let observation = independent_readback(
        reader,
        &object_key,
        &content_sha256,
        content_length,
        &input.retained_at_utc,
    )?;
    let transaction = connection
        .transaction()
        .map_err(sql_error("start custody retain transaction"))?;
    let (version, event_sha256) = append_object_version(
        &transaction,
        &object_id,
        &object_key,
        &content_sha256,
        content_length,
        &input.object_type,
        1,
        &configuration.profile.retention_until_utc,
        &input.retained_at_utc,
        "retain",
    )?;
    let record = StoredObjectVersion {
        object_id: object_id.clone(),
        object_key: object_key.clone(),
        content_sha256: content_sha256.clone(),
        content_length,
        object_type: input.object_type.clone(),
        version,
        retention_until_utc: configuration.profile.retention_until_utc.clone(),
        legal_hold: true,
        event_sha256,
    };
    let receipt = receipt_for(&configuration, &record, observation);
    append_receipt(&transaction, &receipt)?;
    transaction
        .commit()
        .map_err(sql_error("commit custody retain transaction"))?;
    sync_ledger(path)?;
    Ok(receipt)
}

/// Append a later custody retention version.  This API has no matching
/// shorten, unlock, delete, purge, copy, or legal-hold-release operation.
pub fn append_custody_retention_extension(
    path: impl AsRef<Path>,
    object_id: &str,
    later_retention_until_utc: &str,
    observed_at_utc: &str,
    reader: &mut impl CustodyObjectReader,
) -> Result<CustodyIntegrityReceiptV1, ObjectServiceError> {
    require_nonblank("object_id", object_id)?;
    canonical_timestamp("later_retention_until_utc", later_retention_until_utc)?;
    canonical_timestamp("observed_at_utc", observed_at_utc)?;
    let path = path.as_ref();
    let configuration = read_sealed_configuration(path)?;
    if reader.identity() != configuration.profile.reader_identity {
        return Err(invalid(
            "custody extension requires the sealed independent reader identity",
        ));
    }
    let mut connection = open_ledger(path)?;
    let previous = latest_object_version(&connection, object_id)?
        .ok_or_else(|| invalid("cannot extend retention for an unknown custody object"))?;
    if !previous.legal_hold {
        return Err(invalid("custody object is missing required legal hold"));
    }
    if later_retention_until_utc <= previous.retention_until_utc.as_str() {
        return Err(invalid(
            "custody retention extension must be strictly later; shortening and no-op replacement are forbidden",
        ));
    }
    let observation = independent_readback(
        reader,
        &previous.object_key,
        &previous.content_sha256,
        previous.content_length,
        observed_at_utc,
    )?;
    let next_version = previous
        .version
        .checked_add(1)
        .ok_or_else(|| invalid("custody version overflow"))?;
    let transaction = connection
        .transaction()
        .map_err(sql_error("start custody extension transaction"))?;
    let (_, event_sha256) = append_object_version(
        &transaction,
        &previous.object_id,
        &previous.object_key,
        &previous.content_sha256,
        previous.content_length,
        &previous.object_type,
        next_version,
        later_retention_until_utc,
        observed_at_utc,
        "extend_retention",
    )?;
    let record = StoredObjectVersion {
        retention_until_utc: later_retention_until_utc.to_string(),
        event_sha256,
        version: next_version,
        ..previous
    };
    let receipt = receipt_for(&configuration, &record, observation);
    append_receipt(&transaction, &receipt)?;
    transaction
        .commit()
        .map_err(sql_error("commit custody extension transaction"))?;
    sync_ledger(path)?;
    Ok(receipt)
}

/// Independently reread the object and verify the immutable ledger chain and
/// stored receipt.  This detects ordinary corruption or accidental changes.
/// It cannot protect against a local administrator altering both Garage and
/// the ledger, a threat declared by the assurance class.
pub fn verify_custody_readback_receipt(
    path: impl AsRef<Path>,
    receipt: &CustodyIntegrityReceiptV1,
    reader: &mut impl CustodyObjectReader,
) -> Result<(), ObjectServiceError> {
    let path = path.as_ref();
    let configuration = read_sealed_configuration(path)?;
    if receipt.schema != CUSTODY_RECEIPT_SCHEMA_V1
        || receipt.assurance_class != CUSTODY_ASSURANCE_CLASS_LOCAL_TRUSTED_ADMINISTRATOR_OVERLAY
        || receipt.store_id != configuration.store_id
        || receipt.bucket_name != configuration.bucket_name
        || receipt.target_id != configuration.profile.target_id
    {
        return Err(invalid(
            "custody receipt is not bound to this sealed store configuration",
        ));
    }
    validate_reader_writer(
        &configuration.profile,
        "verification-writer-placeholder",
        reader.identity(),
    )
    .or_else(|error| {
        // Verification has no writer.  Preserve the actual reader identity
        // check without treating the placeholder as a retained writer.
        if reader.identity() == configuration.profile.reader_identity {
            Ok(())
        } else {
            Err(error)
        }
    })?;
    let connection = open_ledger(path)?;
    verify_event_chain(&connection)?;
    let stored = latest_object_version(&connection, &receipt.object_id)?
        .ok_or_else(|| invalid("custody receipt refers to an unknown object"))?;
    if stored.version != receipt.version {
        return Err(invalid(
            "custody receipt refers to a non-current object version",
        ));
    }
    let persisted = existing_receipt(&connection, &receipt.object_id, receipt.version)?
        .ok_or_else(|| invalid("custody receipt is not present in immutable ledger"))?;
    if &persisted != receipt {
        return Err(invalid(
            "custody receipt differs from immutable ledger receipt",
        ));
    }
    let bytes = reader.read_exact(&receipt.object_key)?;
    let observation = CustodyReadbackObservationV1 {
        reader_identity: reader.identity().to_string(),
        observed_at_utc: receipt.observed_at_utc.clone(),
        content_sha256: sha256_hex(&bytes),
        content_length: bytes.len() as u64,
    };
    verify_receipt_fields(&configuration, &stored, receipt, &observation)
}

/// Read only.  It never creates a missing ledger, directories, database, or
/// connection sidecar, which makes it safe for source-only preflight tests.
pub fn inspect_custody_ledger(
    path: impl AsRef<Path>,
) -> Result<CustodyLedgerInspectionV1, ObjectServiceError> {
    let path = path.as_ref();
    let configuration = read_sealed_configuration(path)?;
    let connection = open_ledger_read_only(path)?;
    verify_event_chain(&connection)?;
    let configuration_sha256: String = connection
        .query_row(
            "SELECT configuration_sha256 FROM custody_store_configuration WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .map_err(sql_error("read custody configuration digest"))?;
    let object_versions: u64 = connection
        .query_row("SELECT COUNT(*) FROM custody_object_versions", [], |row| {
            row.get(0)
        })
        .map_err(sql_error("count custody object versions"))?;
    let receipts: u64 = connection
        .query_row(
            "SELECT COUNT(*) FROM custody_readback_receipts",
            [],
            |row| row.get(0),
        )
        .map_err(sql_error("count custody receipts"))?;
    let head = connection
        .query_row(
            "SELECT event_sha256 FROM custody_events ORDER BY sequence DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql_error("read custody ledger head"))?
        .unwrap_or_else(|| sha256_hex(b""));
    Ok(CustodyLedgerInspectionV1 {
        schema: CUSTODY_OVERLAY_SCHEMA_V1.to_string(),
        ledger_path: path.to_path_buf(),
        store_id: configuration.store_id,
        bucket_name: configuration.bucket_name,
        target_id: configuration.profile.target_id,
        assurance_class: configuration.profile.assurance_class.name().to_string(),
        configuration_sha256,
        committed_object_versions: object_versions,
        committed_receipts: receipts,
        ledger_head_sha256: head,
    })
}

fn validate_custody_provisioning_request(
    request: &CustodyGarageProvisioningRequest,
) -> Result<(), ObjectServiceError> {
    validate_new_custody_store(
        &request.store_id,
        &request.bucket_name,
        &request.profile,
        "2026-01-01T00:00:00Z",
    )?;
    if request.writer.credential_reference != request.profile.writer_credential_reference
        || request.reader.credential_reference != request.profile.reader_credential_reference
    {
        return Err(invalid(
            "custody provisioner credentials must exactly match the sealed profile references",
        ));
    }
    if request.writer.access_key_id == request.reader.access_key_id {
        return Err(invalid(
            "custody writer and reader Garage access keys must be distinct",
        ));
    }
    require_nonblank(
        "custody provisioner identity",
        &request.provisioner.identity,
    )?;
    require_nonblank(
        "custody provisioner credential reference",
        &request.provisioner.credential_reference,
    )?;
    if request.provisioner.credential_reference == request.writer.credential_reference
        || request.provisioner.credential_reference == request.reader.credential_reference
        || request.provisioner.identity == request.profile.reader_identity
    {
        return Err(invalid(
            "custody provisioner, writer, and reader identities must be distinct",
        ));
    }
    Ok(())
}

fn validate_new_custody_store(
    store_id: &StoreId,
    bucket_name: &str,
    profile: &CustodyStoreProfileV1,
    created_at_utc: &str,
) -> Result<(), ObjectServiceError> {
    profile.validate()?;
    require_nonblank("bucket_name", bucket_name)?;
    if custody_bucket_is_reserved(bucket_name) || store_id.as_str() == R237_BOOTSTRAP_STORE_ID {
        return Err(invalid(
            "the r237 bootstrap namespace is permanently ineligible for custody adoption",
        ));
    }
    let created_at_utc = canonical_timestamp("created_at_utc", created_at_utc)?;
    if profile.retention_until_utc <= created_at_utc {
        return Err(invalid(
            "custody retention_until_utc must be after configuration creation",
        ));
    }
    Ok(())
}

fn validate_custody_input(input: &CustodyObjectInputV1) -> Result<(), ObjectServiceError> {
    require_nonblank("object_type", &input.object_type)?;
    canonical_timestamp("retained_at_utc", &input.retained_at_utc)?;
    if input.bytes.is_empty() {
        return Err(invalid("a custody object must not be empty"));
    }
    Ok(())
}

fn validate_reader_writer(
    profile: &CustodyStoreProfileV1,
    writer_identity: &str,
    reader_identity: &str,
) -> Result<(), ObjectServiceError> {
    require_nonblank("custody writer identity", writer_identity)?;
    if reader_identity != profile.reader_identity {
        return Err(invalid(
            "custody reader identity does not match sealed profile",
        ));
    }
    if writer_identity == reader_identity {
        return Err(invalid(
            "custody writer and reader identities must be independently distinct",
        ));
    }
    Ok(())
}

fn independent_readback(
    reader: &mut impl CustodyObjectReader,
    object_key: &str,
    expected_sha256: &str,
    expected_length: u64,
    observed_at_utc: &str,
) -> Result<CustodyReadbackObservationV1, ObjectServiceError> {
    canonical_timestamp("observed_at_utc", observed_at_utc)?;
    let bytes = reader.read_exact(object_key)?;
    let observation = CustodyReadbackObservationV1 {
        reader_identity: reader.identity().to_string(),
        observed_at_utc: observed_at_utc.to_string(),
        content_sha256: sha256_hex(&bytes),
        content_length: bytes.len() as u64,
    };
    if observation.content_sha256 != expected_sha256
        || observation.content_length != expected_length
    {
        return Err(invalid(
            "independent custody readback digest or length does not match retained object",
        ));
    }
    Ok(observation)
}

fn ensure_exact_existing_object(
    existing: &StoredObjectVersion,
    object_key: &str,
    content_sha256: &str,
    content_length: u64,
    object_type: &str,
) -> Result<(), ObjectServiceError> {
    if existing.object_key != object_key
        || existing.content_sha256 != content_sha256
        || existing.content_length != content_length
        || existing.object_type != object_type
        || !existing.legal_hold
    {
        return Err(invalid(
            "existing custody object conflicts with requested immutable object or lacks legal hold",
        ));
    }
    Ok(())
}

fn receipt_for(
    configuration: &CustodySealedConfigurationV1,
    record: &StoredObjectVersion,
    observation: CustodyReadbackObservationV1,
) -> CustodyIntegrityReceiptV1 {
    CustodyIntegrityReceiptV1 {
        schema: CUSTODY_RECEIPT_SCHEMA_V1.to_string(),
        assurance_class: configuration.profile.assurance_class.name().to_string(),
        store_id: configuration.store_id.clone(),
        bucket_name: configuration.bucket_name.clone(),
        target_id: configuration.profile.target_id.clone(),
        object_id: record.object_id.clone(),
        object_key: record.object_key.clone(),
        content_sha256: record.content_sha256.clone(),
        content_length: record.content_length,
        object_type: record.object_type.clone(),
        version: record.version,
        retention_until_utc: record.retention_until_utc.clone(),
        legal_hold: record.legal_hold,
        reader_identity: observation.reader_identity,
        observed_at_utc: observation.observed_at_utc,
        configuration_sha256: sealed_configuration_sha256(configuration)
            .expect("sealed configuration was validated and serializable"),
        ledger_event_sha256: record.event_sha256.clone(),
    }
}

fn verify_receipt_fields(
    configuration: &CustodySealedConfigurationV1,
    stored: &StoredObjectVersion,
    receipt: &CustodyIntegrityReceiptV1,
    observation: &CustodyReadbackObservationV1,
) -> Result<(), ObjectServiceError> {
    if receipt.reader_identity != configuration.profile.reader_identity
        || observation.reader_identity != configuration.profile.reader_identity
        || !receipt.legal_hold
        || receipt.object_id != stored.object_id
        || receipt.object_key != stored.object_key
        || receipt.content_sha256 != stored.content_sha256
        || receipt.content_length != stored.content_length
        || receipt.object_type != stored.object_type
        || receipt.version != stored.version
        || receipt.retention_until_utc != stored.retention_until_utc
        || receipt.ledger_event_sha256 != stored.event_sha256
        || receipt.configuration_sha256 != sealed_configuration_sha256(configuration)?
        || receipt.content_sha256 != observation.content_sha256
        || receipt.content_length != observation.content_length
    {
        return Err(invalid(
            "custody readback receipt does not match sealed immutable record",
        ));
    }
    Ok(())
}

fn append_object_version(
    transaction: &rusqlite::Transaction<'_>,
    object_id: &str,
    object_key: &str,
    content_sha256: &str,
    content_length: u64,
    object_type: &str,
    version: u64,
    retention_until_utc: &str,
    observed_at_utc: &str,
    operation: &str,
) -> Result<(u64, String), ObjectServiceError> {
    let sequence: u64 = transaction
        .query_row(
            "SELECT COALESCE(MAX(sequence), 0) + 1 FROM custody_events",
            [],
            |row| row.get(0),
        )
        .map_err(sql_error("allocate custody event sequence"))?;
    let previous_event_sha256 = transaction
        .query_row(
            "SELECT event_sha256 FROM custody_events ORDER BY sequence DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql_error("read custody event chain head"))?;
    let event = CustodyLedgerEventV1 {
        schema: CUSTODY_OVERLAY_SCHEMA_V1.to_string(),
        sequence,
        operation: operation.to_string(),
        object_id: object_id.to_string(),
        object_key: object_key.to_string(),
        content_sha256: content_sha256.to_string(),
        content_length,
        object_type: object_type.to_string(),
        version,
        retention_until_utc: retention_until_utc.to_string(),
        legal_hold: true,
        observed_at_utc: observed_at_utc.to_string(),
        previous_event_sha256: previous_event_sha256.clone(),
    };
    let event_jcs = canonical_json(&event)?;
    let event_sha256 = sha256_hex(event_jcs.as_bytes());
    transaction
        .execute(
            "INSERT INTO custody_events (sequence, event_jcs, previous_event_sha256, event_sha256) \
             VALUES (?1, ?2, ?3, ?4)",
            params![sequence, event_jcs, previous_event_sha256, event_sha256],
        )
        .map_err(sql_error("append custody event"))?;
    transaction
        .execute(
            "INSERT INTO custody_object_versions \
             (object_id, version, object_key, content_sha256, content_length, object_type, \
              retention_until_utc, legal_hold, event_sha256) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8)",
            params![
                object_id,
                version,
                object_key,
                content_sha256,
                content_length,
                object_type,
                retention_until_utc,
                event_sha256
            ],
        )
        .map_err(sql_error("append immutable custody object version"))?;
    Ok((version, event_sha256))
}

fn append_receipt(
    transaction: &rusqlite::Transaction<'_>,
    receipt: &CustodyIntegrityReceiptV1,
) -> Result<(), ObjectServiceError> {
    let receipt_jcs = canonical_json(receipt)?;
    let receipt_sha256 = sha256_hex(receipt_jcs.as_bytes());
    transaction
        .execute(
            "INSERT INTO custody_readback_receipts \
             (object_id, version, receipt_jcs, receipt_sha256) VALUES (?1, ?2, ?3, ?4)",
            params![
                receipt.object_id,
                receipt.version,
                receipt_jcs,
                receipt_sha256
            ],
        )
        .map_err(sql_error("append immutable custody readback receipt"))?;
    Ok(())
}

fn existing_receipt(
    connection: &Connection,
    object_id: &str,
    version: u64,
) -> Result<Option<CustodyIntegrityReceiptV1>, ObjectServiceError> {
    connection
        .query_row(
            "SELECT receipt_jcs FROM custody_readback_receipts WHERE object_id = ?1 AND version = ?2",
            params![object_id, version],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(sql_error("read custody receipt"))?
        .map(|receipt_jcs| {
            serde_json::from_str(&receipt_jcs)
                .map_err(|error| invalid(format!("decode immutable custody receipt: {error}")))
        })
        .transpose()
}

fn latest_object_version(
    connection: &Connection,
    object_id: &str,
) -> Result<Option<StoredObjectVersion>, ObjectServiceError> {
    connection
        .query_row(
            "SELECT object_id, object_key, content_sha256, content_length, object_type, version, \
                    retention_until_utc, legal_hold, event_sha256 \
             FROM custody_object_versions WHERE object_id = ?1 ORDER BY version DESC LIMIT 1",
            params![object_id],
            |row| {
                Ok(StoredObjectVersion {
                    object_id: row.get(0)?,
                    object_key: row.get(1)?,
                    content_sha256: row.get(2)?,
                    content_length: row.get(3)?,
                    object_type: row.get(4)?,
                    version: row.get(5)?,
                    legal_hold: row.get::<_, i64>(7)? == 1,
                    retention_until_utc: row.get(6)?,
                    event_sha256: row.get(8)?,
                })
            },
        )
        .optional()
        .map_err(sql_error("read latest custody object version"))
}

fn read_sealed_configuration(
    path: &Path,
) -> Result<CustodySealedConfigurationV1, ObjectServiceError> {
    let connection = open_ledger_read_only(path)?;
    let (configuration_jcs, configuration_sha256): (String, String) = connection
        .query_row(
            "SELECT configuration_jcs, configuration_sha256 FROM custody_store_configuration WHERE singleton = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(sql_error("read sealed custody store configuration"))?;
    if sha256_hex(configuration_jcs.as_bytes()) != configuration_sha256 {
        return Err(invalid(
            "sealed custody configuration digest does not match durable record",
        ));
    }
    let configuration: CustodySealedConfigurationV1 = serde_json::from_str(&configuration_jcs)
        .map_err(|error| invalid(format!("decode sealed custody configuration: {error}")))?;
    validate_new_custody_store(
        &configuration.store_id,
        &configuration.bucket_name,
        &configuration.profile,
        &configuration.created_at_utc,
    )?;
    if canonical_json(&configuration)? != configuration_jcs {
        return Err(invalid("sealed custody configuration is not canonical JCS"));
    }
    Ok(configuration)
}

fn sealed_configuration_sha256(
    configuration: &CustodySealedConfigurationV1,
) -> Result<String, ObjectServiceError> {
    Ok(sha256_hex(canonical_json(configuration)?.as_bytes()))
}

fn verify_event_chain(connection: &Connection) -> Result<(), ObjectServiceError> {
    let mut statement = connection
        .prepare(
            "SELECT sequence, event_jcs, previous_event_sha256, event_sha256 \
             FROM custody_events ORDER BY sequence ASC",
        )
        .map_err(sql_error("prepare custody event-chain verification"))?;
    let mut rows = statement
        .query([])
        .map_err(sql_error("query custody event-chain verification"))?;
    let mut expected_sequence = 1_u64;
    let mut previous: Option<String> = None;
    while let Some(row) = rows
        .next()
        .map_err(sql_error("read custody event-chain row"))?
    {
        let sequence: u64 = row
            .get(0)
            .map_err(sql_error("read custody event sequence"))?;
        let event_jcs: String = row.get(1).map_err(sql_error("read custody event body"))?;
        let stored_previous: Option<String> = row
            .get(2)
            .map_err(sql_error("read custody previous event digest"))?;
        let stored_hash: String = row.get(3).map_err(sql_error("read custody event digest"))?;
        if sequence != expected_sequence
            || stored_previous != previous
            || sha256_hex(event_jcs.as_bytes()) != stored_hash
        {
            return Err(invalid("custody append-only event chain is inconsistent"));
        }
        let event: CustodyLedgerEventV1 = serde_json::from_str(&event_jcs)
            .map_err(|error| invalid(format!("decode custody event: {error}")))?;
        if event.sequence != sequence
            || event.previous_event_sha256 != stored_previous
            || event.legal_hold != true
            || canonical_json(&event)? != event_jcs
        {
            return Err(invalid(
                "custody event is malformed or lacks required legal hold",
            ));
        }
        previous = Some(stored_hash);
        expected_sequence = expected_sequence
            .checked_add(1)
            .ok_or_else(|| invalid("custody event sequence overflow"))?;
    }
    Ok(())
}

fn initialise_schema(connection: &mut Connection) -> Result<(), ObjectServiceError> {
    connection
        .execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = DELETE;
             PRAGMA synchronous = FULL;
             CREATE TABLE custody_store_configuration (
                 singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                 schema_version INTEGER NOT NULL,
                 configuration_jcs TEXT NOT NULL,
                 configuration_sha256 TEXT NOT NULL
             );
             CREATE TABLE custody_events (
                 sequence INTEGER PRIMARY KEY,
                 event_jcs TEXT NOT NULL,
                 previous_event_sha256 TEXT,
                 event_sha256 TEXT NOT NULL UNIQUE
             );
             CREATE TABLE custody_object_versions (
                 object_id TEXT NOT NULL,
                 version INTEGER NOT NULL,
                 object_key TEXT NOT NULL,
                 content_sha256 TEXT NOT NULL,
                 content_length INTEGER NOT NULL,
                 object_type TEXT NOT NULL,
                 retention_until_utc TEXT NOT NULL,
                 legal_hold INTEGER NOT NULL CHECK (legal_hold = 1),
                 event_sha256 TEXT NOT NULL UNIQUE REFERENCES custody_events(event_sha256),
                 PRIMARY KEY (object_id, version),
                 UNIQUE (object_key, version)
             );
             CREATE TABLE custody_readback_receipts (
                 object_id TEXT NOT NULL,
                 version INTEGER NOT NULL,
                 receipt_jcs TEXT NOT NULL,
                 receipt_sha256 TEXT NOT NULL UNIQUE,
                 PRIMARY KEY (object_id, version),
                 FOREIGN KEY (object_id, version)
                     REFERENCES custody_object_versions(object_id, version)
             );
             CREATE TRIGGER custody_configuration_no_update BEFORE UPDATE ON custody_store_configuration
                 BEGIN SELECT RAISE(ABORT, 'custody configuration is sealed'); END;
             CREATE TRIGGER custody_configuration_no_delete BEFORE DELETE ON custody_store_configuration
                 BEGIN SELECT RAISE(ABORT, 'custody configuration is sealed'); END;
             CREATE TRIGGER custody_events_no_update BEFORE UPDATE ON custody_events
                 BEGIN SELECT RAISE(ABORT, 'custody events are append-only'); END;
             CREATE TRIGGER custody_events_no_delete BEFORE DELETE ON custody_events
                 BEGIN SELECT RAISE(ABORT, 'custody events are append-only'); END;
             CREATE TRIGGER custody_object_versions_no_update BEFORE UPDATE ON custody_object_versions
                 BEGIN SELECT RAISE(ABORT, 'custody object versions are append-only'); END;
             CREATE TRIGGER custody_object_versions_no_delete BEFORE DELETE ON custody_object_versions
                 BEGIN SELECT RAISE(ABORT, 'custody object versions are append-only'); END;
             CREATE TRIGGER custody_receipts_no_update BEFORE UPDATE ON custody_readback_receipts
                 BEGIN SELECT RAISE(ABORT, 'custody receipts are append-only'); END;
             CREATE TRIGGER custody_receipts_no_delete BEFORE DELETE ON custody_readback_receipts
                 BEGIN SELECT RAISE(ABORT, 'custody receipts are append-only'); END;",
        )
        .map_err(sql_error("initialise custody ledger schema"))
}

fn create_private_new_file(path: &Path) -> Result<File, ObjectServiceError> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid("custody ledger path has no parent"))?;
    if !parent.is_dir() {
        return Err(invalid(
            "custody ledger parent must already exist; implicit directory creation is forbidden",
        ));
    }
    let mut options = OpenOptions::new();
    options.write(true).read(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let file = options.open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            invalid(format!(
                "custody ledger {} already exists; adoption or replacement is forbidden",
                path.display()
            ))
        } else {
            ObjectServiceError::CommandFailed(format!(
                "create fresh custody ledger {}: {error}",
                path.display()
            ))
        }
    })?;
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(|error| {
        ObjectServiceError::CommandFailed(format!(
            "restrict custody ledger {}: {error}",
            path.display()
        ))
    })?;
    file.sync_all().map_err(|error| {
        ObjectServiceError::CommandFailed(format!(
            "sync fresh custody ledger {}: {error}",
            path.display()
        ))
    })?;
    Ok(file)
}

fn sync_ledger(path: &Path) -> Result<(), ObjectServiceError> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|error| {
            ObjectServiceError::CommandFailed(format!(
                "sync custody ledger {}: {error}",
                path.display()
            ))
        })
}

fn open_ledger(path: &Path) -> Result<Connection, ObjectServiceError> {
    if !path.is_file() {
        return Err(invalid(format!(
            "custody ledger {} does not exist",
            path.display()
        )));
    }
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(sql_error("open custody ledger"))?;
    connection
        .execute_batch("PRAGMA foreign_keys = ON; PRAGMA synchronous = FULL;")
        .map_err(sql_error("configure custody ledger"))?;
    Ok(connection)
}

fn open_ledger_read_only(path: &Path) -> Result<Connection, ObjectServiceError> {
    if !path.is_file() {
        return Err(invalid(format!(
            "custody ledger {} does not exist",
            path.display()
        )));
    }
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(sql_error("open custody ledger read-only"))
}

fn canonical_json(value: &impl Serialize) -> Result<String, ObjectServiceError> {
    serde_jcs::to_string(value)
        .map_err(|error| invalid(format!("canonicalise custody JCS: {error}")))
}

fn canonical_timestamp(field: &str, value: &str) -> Result<String, ObjectServiceError> {
    let parsed = DateTime::parse_from_rfc3339(value)
        .map_err(|error| invalid(format!("{field} must be an RFC3339 UTC timestamp: {error}")))?;
    if parsed.offset().local_minus_utc() != 0 {
        return Err(invalid(format!("{field} must use a UTC Z offset")));
    }
    let canonical = parsed
        .with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Secs, true);
    if value != canonical {
        return Err(invalid(format!(
            "{field} must be canonical RFC3339 UTC seconds"
        )));
    }
    Ok(canonical)
}

fn validate_sha256(field: &str, value: &str) -> Result<(), ObjectServiceError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(invalid(format!(
            "{field} must be a lower-case SHA-256 hex digest"
        )));
    }
    Ok(())
}

fn sha256_hex(value: &[u8]) -> String {
    hex::encode(Sha256::digest(value))
}

fn require_nonblank(field: &str, value: &str) -> Result<(), ObjectServiceError> {
    if value.trim().is_empty() {
        return Err(invalid(format!("{field} must not be blank")));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> ObjectServiceError {
    ObjectServiceError::InvalidConfiguration(message.into())
}

fn sql_error(operation: &'static str) -> impl FnOnce(rusqlite::Error) -> ObjectServiceError {
    move |error| ObjectServiceError::CommandFailed(format!("{operation}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};
    use std::time::{SystemTime, UNIX_EPOCH};

    const CREATED: &str = "2026-09-05T10:00:00Z";
    const RETENTION: &str = "2027-09-05T10:00:00Z";

    #[test]
    fn custody_plan_has_separate_read_write_keys_and_never_owner() {
        let request = request("formal-custody", "dos-formal-custody");
        let plan = plan_custody_garage_provisioning(&request).expect("plan");
        assert_eq!(plan.commands.len(), 5);
        let rendered = plan
            .commands
            .iter()
            .flat_map(CustodyGarageProvisioningCommand::argv)
            .collect::<Vec<_>>();
        assert!(rendered.iter().any(|value| value == "--write"));
        assert!(rendered.iter().any(|value| value == "--read"));
        assert!(!rendered.iter().any(|value| value == "--owner"));
        assert!(!rendered.iter().any(|value| value == "--delete"));
        assert!(!format!("{plan:?}").contains("writer-secret"));
        assert!(!plan.commands[0]
            .redacted_argv()
            .iter()
            .any(|value| value == "writer-secret"));
    }

    #[test]
    fn profile_rejects_shared_credentials_missing_hold_and_noncanonical_time() {
        let mut invalid_profile = profile();
        invalid_profile.reader_credential_reference =
            invalid_profile.writer_credential_reference.clone();
        assert!(invalid_profile.validate().is_err());
        let mut invalid_profile = profile();
        invalid_profile.legal_hold = false;
        assert!(invalid_profile.validate().is_err());
        let mut invalid_profile = profile();
        invalid_profile.retention_until_utc = "2027-09-05T11:00:00+01:00".to_string();
        assert!(invalid_profile.validate().is_err());
    }

    #[test]
    fn creates_sealed_append_only_ledger_and_independently_reads_back() {
        let root = temp_root("retain");
        let path = root.join("custody.sqlite");
        create_custody_ledger(
            &path,
            store_id("formal-custody"),
            "dos-formal-custody",
            profile(),
            CREATED,
        )
        .expect("ledger created");
        let backend = MemoryObjectStore::default();
        let receipt = retain_custody_object_with_readback(
            &path,
            input(b"important corpus"),
            &mut Writer { backend: &backend },
            &mut Reader { backend: &backend },
        )
        .expect("retained with independent readback");
        assert!(receipt.legal_hold);
        assert_eq!(receipt.version, 1);
        assert_eq!(
            receipt.assurance_class,
            CUSTODY_ASSURANCE_CLASS_LOCAL_TRUSTED_ADMINISTRATOR_OVERLAY
        );
        verify_custody_readback_receipt(&path, &receipt, &mut Reader { backend: &backend })
            .expect("receipt independently verifies");
        let inspection = inspect_custody_ledger(&path).expect("inspect");
        assert_eq!(inspection.committed_object_versions, 1);
        assert_eq!(inspection.committed_receipts, 1);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn exact_duplicate_is_idempotent_but_partial_write_is_never_adopted() {
        let root = temp_root("duplicate-and-partial");
        let path = root.join("custody.sqlite");
        create_custody_ledger(
            &path,
            store_id("formal-custody"),
            "dos-formal-custody",
            profile(),
            CREATED,
        )
        .expect("ledger created");
        let backend = MemoryObjectStore::default();
        let first = retain_custody_object_with_readback(
            &path,
            input(b"duplicate"),
            &mut Writer { backend: &backend },
            &mut Reader { backend: &backend },
        )
        .expect("first retention");
        let duplicate = retain_custody_object_with_readback(
            &path,
            input(b"duplicate"),
            &mut Writer { backend: &backend },
            &mut Reader { backend: &backend },
        )
        .expect("exact duplicate is idempotent");
        assert_eq!(duplicate, first);
        assert_eq!(
            inspect_custody_ledger(&path)
                .unwrap()
                .committed_object_versions,
            1
        );

        let partial = input(b"partial-write");
        assert!(retain_custody_object_with_readback(
            &path,
            partial.clone(),
            &mut Writer { backend: &backend },
            &mut FailingReader,
        )
        .is_err());
        assert_eq!(
            inspect_custody_ledger(&path)
                .unwrap()
                .committed_object_versions,
            1
        );
        assert!(retain_custody_object_with_readback(
            &path,
            partial,
            &mut Writer { backend: &backend },
            &mut Reader { backend: &backend },
        )
        .is_err());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn existing_ledger_and_bootstrap_namespace_are_never_adopted() {
        let root = temp_root("no-adopt");
        let path = root.join("custody.sqlite");
        create_custody_ledger(
            &path,
            store_id("formal-custody"),
            "dos-formal-custody",
            profile(),
            CREATED,
        )
        .expect("ledger created");
        assert!(create_custody_ledger(
            &path,
            store_id("formal-custody"),
            "dos-formal-custody",
            profile(),
            CREATED,
        )
        .is_err());
        assert!(create_custody_ledger(
            root.join("bootstrap.sqlite"),
            store_id(R237_BOOTSTRAP_STORE_ID),
            R237_BOOTSTRAP_BUCKET_NAME,
            profile(),
            CREATED,
        )
        .is_err());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn extension_can_only_be_append_only_and_later_with_readback() {
        let root = temp_root("extension");
        let path = root.join("custody.sqlite");
        create_custody_ledger(
            &path,
            store_id("formal-custody"),
            "dos-formal-custody",
            profile(),
            CREATED,
        )
        .expect("ledger created");
        let backend = MemoryObjectStore::default();
        let receipt = retain_custody_object_with_readback(
            &path,
            input(b"retained"),
            &mut Writer { backend: &backend },
            &mut Reader { backend: &backend },
        )
        .expect("retained");
        assert!(append_custody_retention_extension(
            &path,
            &receipt.object_id,
            "2027-01-01T00:00:00Z",
            "2026-09-05T12:00:00Z",
            &mut Reader { backend: &backend },
        )
        .is_err());
        let extended = append_custody_retention_extension(
            &path,
            &receipt.object_id,
            "2028-09-05T10:00:00Z",
            "2026-09-05T12:00:00Z",
            &mut Reader { backend: &backend },
        )
        .expect("later extension");
        assert_eq!(extended.version, 2);
        assert_eq!(extended.retention_until_utc, "2028-09-05T10:00:00Z");
        assert_eq!(
            inspect_custody_ledger(&path)
                .unwrap()
                .committed_object_versions,
            2
        );
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn rejects_writer_reader_conflation_or_unledgered_backend_object() {
        let root = temp_root("conflation");
        let path = root.join("custody.sqlite");
        create_custody_ledger(
            &path,
            store_id("formal-custody"),
            "dos-formal-custody",
            profile(),
            CREATED,
        )
        .expect("ledger created");
        let backend = MemoryObjectStore::default();
        let error = retain_custody_object_with_readback(
            &path,
            input(b"one"),
            &mut Conflated { backend: &backend },
            &mut Reader { backend: &backend },
        )
        .expect_err("writer reader identities must differ");
        assert!(error.to_string().contains("identities"));

        let bytes = b"unledgered".to_vec();
        let key = custody_object_key(&sha256_hex(&bytes)).unwrap();
        backend.objects.borrow_mut().insert(key, bytes);
        let error = retain_custody_object_with_readback(
            &path,
            input(b"unledgered"),
            &mut Writer { backend: &backend },
            &mut Reader { backend: &backend },
        )
        .expect_err("unledgered object cannot be adopted");
        assert!(error.to_string().contains("adoption"));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn read_only_inspection_has_no_side_effect_for_absent_ledger() {
        let root = temp_root("read-only");
        let path = root.join("absent.sqlite");
        assert!(inspect_custody_ledger(&path).is_err());
        assert!(!path.exists());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn sqlite_triggers_reject_mutation_and_deletion() {
        let root = temp_root("triggers");
        let path = root.join("custody.sqlite");
        create_custody_ledger(
            &path,
            store_id("formal-custody"),
            "dos-formal-custody",
            profile(),
            CREATED,
        )
        .expect("ledger created");
        let backend = MemoryObjectStore::default();
        let receipt = retain_custody_object_with_readback(
            &path,
            input(b"locked"),
            &mut Writer { backend: &backend },
            &mut Reader { backend: &backend },
        )
        .expect("retained");
        let connection = open_ledger(&path).expect("open");
        assert!(connection
            .execute(
                "DELETE FROM custody_object_versions WHERE object_id = ?1",
                params![receipt.object_id],
            )
            .is_err());
        assert!(connection
            .execute(
                "UPDATE custody_store_configuration SET configuration_jcs = '{}'",
                [],
            )
            .is_err());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn independent_verification_detects_ordinary_ledger_tampering() {
        let root = temp_root("event-tamper");
        let path = root.join("custody.sqlite");
        create_custody_ledger(
            &path,
            store_id("formal-custody"),
            "dos-formal-custody",
            profile(),
            CREATED,
        )
        .expect("ledger created");
        let backend = MemoryObjectStore::default();
        let receipt = retain_custody_object_with_readback(
            &path,
            input(b"tamper-check"),
            &mut Writer { backend: &backend },
            &mut Reader { backend: &backend },
        )
        .expect("retained");
        let connection = open_ledger(&path).expect("open");
        connection
            .execute("DROP TRIGGER custody_events_no_update", [])
            .expect("administrative tamper fixture removes guard");
        connection
            .execute(
                "UPDATE custody_events SET event_jcs = '{}' WHERE sequence = 1",
                [],
            )
            .expect("administrative tamper fixture changes event");
        assert!(verify_custody_readback_receipt(
            &path,
            &receipt,
            &mut Reader { backend: &backend },
        )
        .is_err());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn all_supported_custody_mutation_bypasses_are_denied() {
        for operation in [
            CustodyForbiddenMutation::Delete,
            CustodyForbiddenMutation::Overwrite,
            CustodyForbiddenMutation::Copy,
            CustodyForbiddenMutation::MultipartUpload,
            CustodyForbiddenMutation::Restore,
            CustodyForbiddenMutation::Reconcile,
            CustodyForbiddenMutation::Lifecycle,
            CustodyForbiddenMutation::ShortenRetention,
            CustodyForbiddenMutation::ClearLegalHold,
            CustodyForbiddenMutation::ReplaceConfiguration,
            CustodyForbiddenMutation::ReinitialiseLedger,
        ] {
            assert!(reject_custody_mutation(operation).is_err());
        }
    }

    #[test]
    fn off_nuc_attestation_requires_signature_monotonic_nonce_and_expiry() {
        let authority = TestAuthority;
        let mut state = TestOffNucState::default();
        let first = signed_attestation(1, None, "nonce-1", "2026-09-05T11:00:00Z");
        let checkpoint = accept_custody_off_nuc_attestation(
            &first,
            "nuc-192.168.0.193",
            "2026-09-05T10:30:00Z",
            &authority,
            &mut state,
        )
        .expect("first attestation accepted");
        assert_eq!(checkpoint.sequence, 1);
        assert!(accept_custody_off_nuc_attestation(
            &first,
            "nuc-192.168.0.193",
            "2026-09-05T10:30:00Z",
            &authority,
            &mut state,
        )
        .is_err());

        let second = signed_attestation(
            2,
            Some(checkpoint.attestation_sha256),
            "nonce-2",
            "2026-09-05T11:00:00Z",
        );
        assert_eq!(
            accept_custody_off_nuc_attestation(
                &second,
                "nuc-192.168.0.193",
                "2026-09-05T10:30:00Z",
                &authority,
                &mut state,
            )
            .expect("second attestation accepted")
            .sequence,
            2
        );

        let expired = signed_attestation(
            3,
            Some(
                state
                    .checkpoint
                    .as_ref()
                    .unwrap()
                    .attestation_sha256
                    .clone(),
            ),
            "nonce-3",
            "2026-09-05T10:15:00Z",
        );
        assert!(accept_custody_off_nuc_attestation(
            &expired,
            "nuc-192.168.0.193",
            "2026-09-05T10:30:00Z",
            &authority,
            &mut state,
        )
        .is_err());

        let mut substituted = signed_attestation(
            3,
            Some(
                state
                    .checkpoint
                    .as_ref()
                    .unwrap()
                    .attestation_sha256
                    .clone(),
            ),
            "nonce-4",
            "2026-09-05T11:00:00Z",
        );
        substituted.signature = "substituted".to_string();
        assert!(accept_custody_off_nuc_attestation(
            &substituted,
            "nuc-192.168.0.193",
            "2026-09-05T10:30:00Z",
            &authority,
            &mut state,
        )
        .is_err());
    }

    fn profile() -> CustodyStoreProfileV1 {
        CustodyStoreProfileV1 {
            schema: CUSTODY_OVERLAY_SCHEMA_V1.to_string(),
            profile: CUSTODY_PROFILE_V1.to_string(),
            assurance_class: CustodyAssuranceClass::LocalTrustedAdministratorOverlay,
            retention_mode: CustodyRetentionMode::LocalTrustedAdministratorOverlay,
            target_id: "nuc-192.168.0.193".to_string(),
            retention_until_utc: RETENTION.to_string(),
            legal_hold: true,
            writer_credential_reference: "secret://custody/writer".to_string(),
            reader_credential_reference: "secret://custody/reader".to_string(),
            reader_identity: "custody-reader-v1".to_string(),
        }
    }

    fn signed_attestation(
        sequence: u64,
        previous_attestation_sha256: Option<String>,
        nonce: &str,
        expires_at_utc: &str,
    ) -> CustodyOffNucAttestationV1 {
        let body = CustodyOffNucAttestationBodyV1 {
            schema: CUSTODY_OFF_NUC_ATTESTATION_SCHEMA_V1.to_string(),
            assurance_class: CUSTODY_ASSURANCE_CLASS_LOCAL_TRUSTED_ADMINISTRATOR_OVERLAY
                .to_string(),
            verifier_id: "off-nuc-verifier-v1".to_string(),
            target_id: "nuc-192.168.0.193".to_string(),
            nonce: nonce.to_string(),
            sequence,
            previous_attestation_sha256,
            issued_at_utc: "2026-09-05T10:00:00Z".to_string(),
            expires_at_utc: expires_at_utc.to_string(),
            receipt: CustodyIntegrityReceiptV1 {
                schema: CUSTODY_RECEIPT_SCHEMA_V1.to_string(),
                assurance_class: CUSTODY_ASSURANCE_CLASS_LOCAL_TRUSTED_ADMINISTRATOR_OVERLAY
                    .to_string(),
                store_id: store_id("formal-custody"),
                bucket_name: "dos-formal-custody".to_string(),
                target_id: "nuc-192.168.0.193".to_string(),
                object_id: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                    .to_string(),
                object_key: "custody/sha256/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                    .to_string(),
                content_sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                    .to_string(),
                content_length: 1,
                object_type: "release_corpus".to_string(),
                version: 1,
                retention_until_utc: RETENTION.to_string(),
                legal_hold: true,
                reader_identity: "custody-reader-v1".to_string(),
                observed_at_utc: "2026-09-05T10:00:00Z".to_string(),
                configuration_sha256:
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        .to_string(),
                ledger_event_sha256:
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                        .to_string(),
            },
            ledger_head_sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .to_string(),
            das_executable_sha256:
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            garage_image_digest: "sha256:garage-image-digest".to_string(),
            garage_config_sha256:
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            s3_endpoint: "https://nuc.example.invalid:3900".to_string(),
            full_inventory_sha256:
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            custody_marker_sha256:
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
        };
        CustodyOffNucAttestationV1 {
            signature: sha256_hex(canonical_json(&body).unwrap().as_bytes()),
            authority_id: "off-nuc-authority-v1".to_string(),
            body,
        }
    }

    fn request(store: &str, bucket: &str) -> CustodyGarageProvisioningRequest {
        CustodyGarageProvisioningRequest {
            store_id: store_id(store),
            bucket_name: bucket.to_string(),
            profile: profile(),
            provisioner: CustodyGarageProvisionerIdentity {
                identity: "custody-provisioner-v1".to_string(),
                credential_reference: "secret://custody/provisioner".to_string(),
            },
            writer: CustodyGarageCredential::new(
                "secret://custody/writer",
                "writer-access",
                "writer-secret",
            )
            .unwrap(),
            reader: CustodyGarageCredential::new(
                "secret://custody/reader",
                "reader-access",
                "reader-secret",
            )
            .unwrap(),
        }
    }

    fn input(bytes: &[u8]) -> CustodyObjectInputV1 {
        CustodyObjectInputV1 {
            object_type: "release_corpus".to_string(),
            bytes: bytes.to_vec(),
            retained_at_utc: "2026-09-05T11:00:00Z".to_string(),
        }
    }

    fn store_id(value: &str) -> StoreId {
        StoreId::new(value).expect("store id")
    }

    use std::cell::RefCell;

    #[derive(Default)]
    struct MemoryObjectStore {
        objects: RefCell<BTreeMap<String, Vec<u8>>>,
    }

    struct Writer<'a> {
        backend: &'a MemoryObjectStore,
    }

    impl CustodyObjectWriter for Writer<'_> {
        fn identity(&self) -> &str {
            "custody-writer-v1"
        }

        fn object_state(
            &mut self,
            object_key: &str,
        ) -> Result<CustodyObjectState, ObjectServiceError> {
            Ok(match self.backend.objects.borrow().get(object_key) {
                Some(bytes) => CustodyObjectState::Existing {
                    content_sha256: sha256_hex(bytes),
                    content_length: bytes.len() as u64,
                },
                None => CustodyObjectState::Missing,
            })
        }

        fn put_if_absent(
            &mut self,
            object_key: &str,
            bytes: &[u8],
        ) -> Result<(), ObjectServiceError> {
            if self.backend.objects.borrow().contains_key(object_key) {
                return Err(invalid("memory writer refused overwrite"));
            }
            self.backend
                .objects
                .borrow_mut()
                .insert(object_key.to_string(), bytes.to_vec());
            Ok(())
        }
    }

    struct Reader<'a> {
        backend: &'a MemoryObjectStore,
    }

    struct FailingReader;

    impl CustodyObjectReader for FailingReader {
        fn identity(&self) -> &str {
            "custody-reader-v1"
        }

        fn read_exact(&mut self, _object_key: &str) -> Result<Vec<u8>, ObjectServiceError> {
            Err(invalid("simulated independent reader failure"))
        }
    }

    impl CustodyObjectReader for Reader<'_> {
        fn identity(&self) -> &str {
            "custody-reader-v1"
        }

        fn read_exact(&mut self, object_key: &str) -> Result<Vec<u8>, ObjectServiceError> {
            self.backend
                .objects
                .borrow()
                .get(object_key)
                .cloned()
                .ok_or_else(|| invalid("memory reader missing object"))
        }
    }

    struct Conflated<'a> {
        backend: &'a MemoryObjectStore,
    }

    struct TestAuthority;

    impl CustodyOffNucAttestationAuthority for TestAuthority {
        fn authority_id(&self) -> &str {
            "off-nuc-authority-v1"
        }

        fn verify(&self, canonical_body: &[u8], signature: &str) -> Result<(), ObjectServiceError> {
            if signature == sha256_hex(canonical_body) {
                Ok(())
            } else {
                Err(invalid("test authority signature verification failed"))
            }
        }
    }

    #[derive(Default)]
    struct TestOffNucState {
        checkpoint: Option<CustodyOffNucVerifierCheckpointV1>,
        seen_nonces: BTreeSet<String>,
    }

    impl CustodyOffNucVerifierState for TestOffNucState {
        fn checkpoint(
            &self,
            target_id: &str,
        ) -> Result<Option<CustodyOffNucVerifierCheckpointV1>, ObjectServiceError> {
            Ok(self
                .checkpoint
                .as_ref()
                .filter(|checkpoint| checkpoint.target_id == target_id)
                .cloned())
        }

        fn nonce_seen(&self, _target_id: &str, nonce: &str) -> Result<bool, ObjectServiceError> {
            Ok(self.seen_nonces.contains(nonce))
        }

        fn compare_and_store(
            &mut self,
            expected_previous: Option<&CustodyOffNucVerifierCheckpointV1>,
            next: CustodyOffNucVerifierCheckpointV1,
        ) -> Result<(), ObjectServiceError> {
            if self.checkpoint.as_ref() != expected_previous {
                return Err(invalid("test off-NUC compare-and-store conflict"));
            }
            self.seen_nonces.insert(next.nonce.clone());
            self.checkpoint = Some(next);
            Ok(())
        }
    }

    impl CustodyObjectWriter for Conflated<'_> {
        fn identity(&self) -> &str {
            "custody-reader-v1"
        }

        fn object_state(
            &mut self,
            object_key: &str,
        ) -> Result<CustodyObjectState, ObjectServiceError> {
            Writer {
                backend: self.backend,
            }
            .object_state(object_key)
        }

        fn put_if_absent(
            &mut self,
            object_key: &str,
            bytes: &[u8],
        ) -> Result<(), ObjectServiceError> {
            Writer {
                backend: self.backend,
            }
            .put_if_absent(object_key, bytes)
        }
    }

    fn temp_root(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "dasobjectstore-custody-{name}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("temp root");
        path
    }
}
