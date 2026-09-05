//! Strict off-NUC custody attestation journal.
//!
//! This is the only formal-gate attestation model supported by the local
//! trusted-administrator overlay. It is deliberately separate from the
//! earlier observation DTOs: a record is accepted only from exact received
//! JCS bytes, under one pinned Ed25519 key, after an off-NUC journal has
//! durably issued a pre-read request. The journal consumes the request on its
//! first terminal result, including failure, timeout, and incompleteness.
//!
//! The journal itself belongs off the NUC, Garage, BaseCamp, and their backup
//! paths. This source code does not activate it or distribute a signing key.

use crate::provider::ObjectServiceError;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::{DateTime, SecondsFormat, Utc};
use ed25519_dalek::{Signature, VerifyingKey};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub const CUSTODY_SIGNED_RECORD_SCHEMA_V1: &str =
    "dasobjectstore.local_trusted_administrator_custody_signed_record.v1";
pub const CUSTODY_OFF_NUC_PRE_READ_REQUEST_SCHEMA_V1: &str =
    "dasobjectstore.local_trusted_administrator_custody_pre_read_request.v1";
pub const CUSTODY_OFF_NUC_ATTESTATION_SCHEMA_V2: &str =
    "dasobjectstore.local_trusted_administrator_custody_off_nuc_attestation.v2";
pub const CUSTODY_ATTESTATION_ALGORITHM_ED25519: &str = "ed25519";
pub const CUSTODY_ASSURANCE_CLASS: &str = "local_trusted_administrator_overlay";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CustodyEd25519AuthorityV1 {
    pub authority_id: String,
    pub algorithm: String,
    /// RFC 4648 standard Base64 encoding of the exact 32-byte public key.
    pub public_key_base64: String,
    pub public_key_sha256: String,
}

impl CustodyEd25519AuthorityV1 {
    pub fn validate(&self) -> Result<VerifyingKey, ObjectServiceError> {
        nonblank("custody authority id", &self.authority_id)?;
        if self.algorithm != CUSTODY_ATTESTATION_ALGORITHM_ED25519 {
            return Err(invalid("custody signing authority must use ed25519"));
        }
        let key = strict_base64("custody Ed25519 public key", &self.public_key_base64)?;
        let bytes: [u8; 32] = key
            .as_slice()
            .try_into()
            .map_err(|_| invalid("custody Ed25519 public key must be exactly 32 bytes"))?;
        if sha256_hex(&key) != self.public_key_sha256 {
            return Err(invalid(
                "custody Ed25519 public key digest does not match key",
            ));
        }
        VerifyingKey::from_bytes(&bytes)
            .map_err(|error| invalid(format!("custody Ed25519 public key is invalid: {error}")))
    }
}

/// The exact JSON envelope retained by the off-NUC journal. `raw_jcs` is not
/// reconstructed from this value: callers must supply and retain the original
/// bytes through the strict ingress functions below.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CustodySignedRecordV1<T> {
    pub schema: String,
    pub body: T,
    pub authority: CustodyEd25519AuthorityV1,
    /// RFC 4648 standard Base64 encoding of the exact 64-byte Ed25519
    /// signature over JCS(body), not a digest string or re-encoded variant.
    pub signature_base64: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CustodyOffNucPreReadRequestV1 {
    pub schema: String,
    pub assurance_class: String,
    pub request_id: String,
    pub release_train: String,
    pub release_stage: String,
    pub purpose: String,
    pub verifier_id: String,
    pub target_id: String,
    pub machine_identity_sha256: String,
    pub s3_endpoint_authority: String,
    pub endpoint_authority_sha256: String,
    pub tls_peer_sha256: String,
    pub routing_sha256: String,
    pub reader_identity: String,
    pub store_id: String,
    pub bucket_name: String,
    pub stores_namespace_sha256: String,
    pub object_lock_policy_sha256: String,
    pub lock_ledger_sha256: String,
    pub ledger_head_sha256: String,
    pub inventory_sha256: String,
    pub lockset_sha256: String,
    pub verifier_executable_sha256: String,
    pub verifier_provenance_sha256: String,
    pub receipt_jcs_sha256: String,
    pub nonce: String,
    pub sequence: u64,
    pub previous_request_sha256: Option<String>,
    pub issued_at_utc: String,
    pub expires_at_utc: String,
}

impl CustodyOffNucPreReadRequestV1 {
    fn validate(&self, now_utc: &str) -> Result<(), ObjectServiceError> {
        if self.schema != CUSTODY_OFF_NUC_PRE_READ_REQUEST_SCHEMA_V1
            || self.assurance_class != CUSTODY_ASSURANCE_CLASS
        {
            return Err(invalid(
                "unsupported custody pre-read request schema or assurance class",
            ));
        }
        uuid("custody pre-read request id", &self.request_id)?;
        uuid("custody pre-read nonce", &self.nonce)?;
        for (field, value) in [
            ("custody release train", &self.release_train),
            ("custody release stage", &self.release_stage),
            ("custody purpose", &self.purpose),
            ("custody verifier id", &self.verifier_id),
            ("custody target id", &self.target_id),
            ("custody endpoint authority", &self.s3_endpoint_authority),
            ("custody reader identity", &self.reader_identity),
            ("custody store id", &self.store_id),
            ("custody bucket name", &self.bucket_name),
            ("custody nonce", &self.nonce),
        ] {
            nonblank(field, value)?;
        }
        if self.sequence == 0 {
            return Err(invalid(
                "custody pre-read request sequence must be greater than zero",
            ));
        }
        for (field, value) in [
            ("custody machine identity", &self.machine_identity_sha256),
            (
                "custody endpoint authority",
                &self.endpoint_authority_sha256,
            ),
            ("custody TLS peer", &self.tls_peer_sha256),
            ("custody routing", &self.routing_sha256),
            ("custody stores namespace", &self.stores_namespace_sha256),
            (
                "custody Object Lock policy",
                &self.object_lock_policy_sha256,
            ),
            ("custody lock ledger", &self.lock_ledger_sha256),
            ("custody ledger head", &self.ledger_head_sha256),
            ("custody inventory", &self.inventory_sha256),
            ("custody lockset", &self.lockset_sha256),
            (
                "custody verifier executable",
                &self.verifier_executable_sha256,
            ),
            (
                "custody verifier provenance",
                &self.verifier_provenance_sha256,
            ),
            ("custody receipt JCS", &self.receipt_jcs_sha256),
        ] {
            sha256(field, value)?;
        }
        if let Some(previous) = &self.previous_request_sha256 {
            sha256("custody previous request", previous)?;
        }
        let issued = timestamp(
            "custody pre-read request issued_at_utc",
            &self.issued_at_utc,
        )?;
        let expires = timestamp(
            "custody pre-read request expires_at_utc",
            &self.expires_at_utc,
        )?;
        let now = timestamp("custody pre-read journal now_utc", now_utc)?;
        if issued > now || expires <= now || expires <= issued {
            return Err(invalid("custody pre-read request is not currently valid"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CustodyOffNucObservationResult {
    Passed,
    Failed,
    TimedOut,
    Incomplete,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CustodyOffNucAttestationV2 {
    pub schema: String,
    pub attestation_id: String,
    /// A verbatim typed repeat of the signed, issued pre-read request. Its
    /// canonical digest binds all target measurements to the observation.
    pub request: CustodyOffNucPreReadRequestV1,
    pub pre_read_request_sha256: String,
    /// A journal-minted marker. The verifier can only obtain it by atomically
    /// beginning the unique remote-read attempt before it contacts a target.
    pub pre_read_attempt_marker_sha256: String,
    pub observation_result: CustodyOffNucObservationResult,
    pub observed_at_utc: String,
    pub custody_marker_sha256: String,
    pub raw_evidence_sha256: String,
    pub receipt_jcs_sha256: String,
    pub direct_readback_sha256: String,
    pub result_detail: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CustodyFormalGateExpectationV2 {
    /// The entire source request is the formal measurement contract. This
    /// makes omission or substitution of any measurement a hard failure.
    pub request: CustodyOffNucPreReadRequestV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CustodyFormalGateConsumptionV2 {
    pub consumption_id: String,
    pub request_id: String,
    pub attestation_id: String,
    pub request_raw_jcs_sha256: String,
    pub attestation_raw_jcs_sha256: String,
    pub pre_read_attempt_marker_sha256: String,
    pub target_measurements_sha256: String,
    pub raw_evidence_sha256: String,
    pub custody_marker_sha256: String,
    pub receipt_jcs_sha256: String,
    pub ledger_head_sha256: String,
    pub object_lock_policy_sha256: String,
    pub consumed_at_utc: String,
}

/// A non-secret, single-use permission to perform the target read. It is
/// minted and durably recorded before the verifier receives access to the
/// remote-read adapter; completion requires its exact marker digest.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CustodyOffNucReadAttemptV1 {
    pub request_id: String,
    pub target_id: String,
    pub nonce: String,
    pub sequence: u64,
    pub previous_request_sha256: Option<String>,
    pub attempt_marker_sha256: String,
    pub started_at_utc: String,
}

pub type CustodySignedPreReadRequestV1 = CustodySignedRecordV1<CustodyOffNucPreReadRequestV1>;
pub type CustodySignedAttestationV2 = CustodySignedRecordV1<CustodyOffNucAttestationV2>;

/// Durable journal stored only at the off-NUC verifier authority boundary.
pub struct CustodyOffNucJournal {
    path: PathBuf,
}

impl CustodyOffNucJournal {
    pub fn create(path: impl AsRef<Path>) -> Result<Self, ObjectServiceError> {
        let path = path.as_ref().to_path_buf();
        if path.exists() {
            return Err(invalid(
                "custody off-NUC journal already exists; replacement is forbidden",
            ));
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                ObjectServiceError::CommandFailed(format!(
                    "create custody journal directory: {error}"
                ))
            })?;
        }
        let journal = Self { path };
        let connection = journal.open_rw()?;
        connection.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA synchronous = FULL;
             CREATE TABLE issued_pre_read_requests (
                 request_id TEXT PRIMARY KEY, target_id TEXT NOT NULL, nonce TEXT NOT NULL,
                 raw_jcs BLOB NOT NULL, raw_sha256 TEXT NOT NULL UNIQUE, issued_at_utc TEXT NOT NULL,
                 expires_at_utc TEXT NOT NULL, sequence INTEGER NOT NULL,
                 previous_request_sha256 TEXT, status TEXT NOT NULL CHECK(status IN ('issued','started','terminal')),
                 UNIQUE(target_id, nonce));
             CREATE UNIQUE INDEX one_active_custody_pre_read_sequence
                 ON issued_pre_read_requests(target_id, sequence)
                 WHERE status IN ('issued','started');
             CREATE TABLE first_attempts (
                 request_id TEXT PRIMARY KEY REFERENCES issued_pre_read_requests(request_id),
                 raw_jcs BLOB, raw_sha256 TEXT, attestation_id TEXT,
                 attempt_marker_sha256 TEXT NOT NULL, result TEXT NOT NULL
                     CHECK(result IN ('started','passed','failed','timed_out','incomplete')),
                 detail TEXT, started_at_utc TEXT NOT NULL, attempted_at_utc TEXT,
                 UNIQUE(attestation_id));
             CREATE TABLE checkpoints (
                 target_id TEXT PRIMARY KEY, sequence INTEGER NOT NULL, request_sha256 TEXT NOT NULL,
                 request_id TEXT NOT NULL, updated_at_utc TEXT NOT NULL);
             CREATE TABLE formal_consumptions (
                 request_id TEXT PRIMARY KEY REFERENCES issued_pre_read_requests(request_id),
                 attestation_id TEXT NOT NULL UNIQUE, attestation_raw_sha256 TEXT NOT NULL UNIQUE,
                 attempt_marker_sha256 TEXT NOT NULL, consumption_jcs TEXT NOT NULL,
                 consumed_at_utc TEXT NOT NULL);",
        )
        .map_err(sql("initialise off-NUC custody journal"))?;
        Ok(journal)
    }

    /// Reopen an existing off-NUC journal after a verifier restart. It never
    /// creates/replaces state; a durable `started` attempt therefore remains
    /// an immutable denial of a replacement target read.
    pub fn open_existing(path: impl AsRef<Path>) -> Result<Self, ObjectServiceError> {
        let path = path.as_ref().to_path_buf();
        if !path.is_file() {
            return Err(invalid(
                "custody off-NUC journal must already exist before reopening",
            ));
        }
        let journal = Self { path };
        let connection = journal.open_rw()?;
        connection
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type='table' AND name='issued_pre_read_requests'",
                [],
                |_| Ok(()),
            )
            .map_err(sql("verify existing off-NUC custody journal schema"))?;
        Ok(journal)
    }

    pub fn issue_pre_read_request(
        &self,
        raw_jcs: &[u8],
        pinned_authority: &CustodyEd25519AuthorityV1,
        now_utc: &str,
    ) -> Result<String, ObjectServiceError> {
        let record: CustodySignedPreReadRequestV1 = strict_jcs(raw_jcs)?;
        verify_signed(&record, pinned_authority)?;
        record.body.validate(now_utc)?;
        let digest = sha256_hex(raw_jcs);
        let mut connection = self.open_rw()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql("start custody pre-read issuance"))?;
        reserve_issued_sequence(&transaction, &record.body)?;
        transaction
            .execute(
                "INSERT INTO issued_pre_read_requests \
                 (request_id,target_id,nonce,raw_jcs,raw_sha256,issued_at_utc,expires_at_utc,sequence,previous_request_sha256,status) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,'issued')",
                params![
                    record.body.request_id,
                    record.body.target_id,
                    record.body.nonce,
                    raw_jcs,
                    digest,
                    record.body.issued_at_utc,
                    record.body.expires_at_utc,
                    record.body.sequence,
                    record.body.previous_request_sha256,
                ],
            )
            .map_err(sql("durably issue custody pre-read request before remote read"))?;
        transaction
            .commit()
            .map_err(sql("commit custody pre-read issuance"))?;
        Ok(digest)
    }

    /// Atomically reserve the sole permitted remote read for an issued nonce.
    /// Callers receive this value *before* they are permitted to invoke their
    /// reader. A second begin, post-terminal begin, or a handcrafted marker
    /// fails closed. The durable `started` row is deliberately retained across
    /// a verifier crash so a subsequent process cannot perform a replacement
    /// read under the same nonce.
    pub fn begin_pre_read_attempt(
        &self,
        request_id: &str,
        started_at_utc: &str,
    ) -> Result<CustodyOffNucReadAttemptV1, ObjectServiceError> {
        timestamp("custody pre-read attempt start", started_at_utc)?;
        let mut connection = self.open_rw()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql("start custody pre-read attempt"))?;
        let (target_id, nonce, sequence, previous, expires): (
            String,
            String,
            u64,
            Option<String>,
            String,
        ) = transaction
            .query_row(
                "SELECT target_id,nonce,sequence,previous_request_sha256,expires_at_utc \
                     FROM issued_pre_read_requests WHERE request_id=?1 AND status='issued'",
                params![request_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .map_err(sql("load issued custody pre-read attempt"))?;
        if timestamp("custody pre-read attempt start", started_at_utc)?
            >= timestamp("custody request expiry", &expires)?
        {
            return Err(invalid(
                "custody pre-read attempt cannot begin after request expiry",
            ));
        }
        let marker = sha256_hex(format!(
            "custody-off-nuc-pre-read-attempt-v1\\0{request_id}\\0{target_id}\\0{nonce}\\0{sequence}\\0{started_at_utc}\\0{}",
            Uuid::new_v4()
        ));
        let changed = transaction
            .execute(
                "UPDATE issued_pre_read_requests SET status='started' WHERE request_id=?1 AND status='issued'",
                params![request_id],
            )
            .map_err(sql("mark custody pre-read attempt started"))?;
        if changed != 1 {
            return Err(invalid(
                "custody pre-read attempt already started or terminal",
            ));
        }
        transaction
            .execute(
                "INSERT INTO first_attempts \
                 (request_id,raw_jcs,raw_sha256,attestation_id,attempt_marker_sha256,result,detail,started_at_utc,attempted_at_utc) \
                 VALUES (?1,NULL,NULL,NULL,?2,'started',NULL,?3,NULL)",
                params![request_id, marker, started_at_utc],
            )
            .map_err(sql("persist immutable custody pre-read attempt marker"))?;
        transaction
            .commit()
            .map_err(sql("commit custody pre-read attempt start"))?;
        Ok(CustodyOffNucReadAttemptV1 {
            request_id: request_id.to_string(),
            target_id,
            nonce,
            sequence,
            previous_request_sha256: previous,
            attempt_marker_sha256: marker,
            started_at_utc: started_at_utc.to_string(),
        })
    }

    /// The supported remote-read entry point. Its closure receives a
    /// journal-minted permit only after `started` and the immutable marker are
    /// committed. An adapter error is terminalised as `incomplete`; a process
    /// crash leaves the started marker in place and denies replacement reads
    /// on reopen. Callers must pass the returned evidence to a separately
    /// signed attestation; this method never performs a hidden retry.
    pub fn perform_pre_read<T>(
        &self,
        request_id: &str,
        started_at_utc: &str,
        reader: impl FnOnce(&CustodyOffNucReadAttemptV1) -> Result<T, ObjectServiceError>,
    ) -> Result<T, ObjectServiceError> {
        let attempt = self.begin_pre_read_attempt(request_id, started_at_utc)?;
        match reader(&attempt) {
            Ok(value) => Ok(value),
            Err(error) => {
                self.record_terminal_failure(
                    request_id,
                    CustodyOffNucObservationResult::Incomplete,
                    &format!("target read failed after begun attempt: {error}"),
                    started_at_utc,
                )?;
                Err(error)
            }
        }
    }

    /// Persist a timeout, malformed-response, or incomplete terminal result.
    /// This completes a previously begun request before any retry can observe
    /// it. A caller cannot report a terminal outcome for a read it never
    /// reserved.
    pub fn record_terminal_failure(
        &self,
        request_id: &str,
        result: CustodyOffNucObservationResult,
        detail: &str,
        attempted_at_utc: &str,
    ) -> Result<(), ObjectServiceError> {
        if result == CustodyOffNucObservationResult::Passed {
            return Err(invalid(
                "terminal failure recorder cannot record a passing result",
            ));
        }
        timestamp("custody terminal attempt time", attempted_at_utc)?;
        nonblank("custody terminal attempt detail", detail)?;
        let mut connection = self.open_rw()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql("start custody terminal attempt"))?;
        complete_started_request(
            &transaction,
            request_id,
            None,
            None,
            None,
            result_name(result),
            detail,
            attempted_at_utc,
        )?;
        transaction
            .commit()
            .map_err(sql("commit custody terminal attempt"))
    }

    pub fn accept_signed_attestation(
        &self,
        issued_request_id: &str,
        raw_jcs: &[u8],
        pinned_authority: &CustodyEd25519AuthorityV1,
        now_utc: &str,
    ) -> Result<(), ObjectServiceError> {
        let parsed: Result<CustodySignedAttestationV2, _> = strict_jcs(raw_jcs);
        let attempted = (|| {
            let record = parsed
                .as_ref()
                .map_err(|error| invalid(error.to_string()))?;
            if record.body.request.request_id != issued_request_id {
                return Err(invalid(
                    "custody attestation does not name the issued pre-read request",
                ));
            }
            verify_signed(record, pinned_authority)?;
            validate_attestation(&record.body, now_utc)?;
            Ok(record)
        })();
        let mut connection = self.open_rw()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql("start custody off-NUC attestation acceptance"))?;
        let outcome = attempted.and_then(|record| {
            accept_attestation_transaction(&transaction, record, raw_jcs, now_utc)
        });
        match outcome {
            Ok(()) => transaction
                .commit()
                .map_err(sql("commit custody passing attestation")),
            Err(error) => {
                // An envelope that names an issued request gets a durable
                // terminal failed attempt even when signature or measurements
                // fail. A totally unparsable wire record must use the explicit
                // request-id failure recorder above.
                let parsed_attestation_id = parsed
                    .as_ref()
                    .ok()
                    .map(|record| record.body.attestation_id.as_str());
                let terminal_attestation_id = match parsed_attestation_id {
                    Some(attestation_id)
                        if attestation_id_exists(&transaction, attestation_id)? =>
                    {
                        None
                    }
                    value => value,
                };
                let _ = complete_started_request(
                    &transaction,
                    issued_request_id,
                    Some(raw_jcs),
                    Some(&sha256_hex(raw_jcs)),
                    terminal_attestation_id,
                    "failed",
                    &error.to_string(),
                    now_utc,
                );
                transaction
                    .commit()
                    .map_err(sql("commit custody failed attestation"))?;
                Err(error)
            }
        }
    }

    pub fn consume_for_formal_gate(
        &self,
        request_id: &str,
        expectation: &CustodyFormalGateExpectationV2,
        pinned_authority: &CustodyEd25519AuthorityV1,
        now_utc: &str,
    ) -> Result<CustodyFormalGateConsumptionV2, ObjectServiceError> {
        expectation.request.validate(now_utc)?;
        let mut connection = self.open_rw()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql("start custody formal-gate consumption"))?;
        let (request_raw, request_digest, expires): (Vec<u8>, String, String) = transaction
            .query_row(
                "SELECT raw_jcs,raw_sha256,expires_at_utc FROM issued_pre_read_requests WHERE request_id=?1",
                params![request_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(sql("load issued custody pre-read request"))?;
        if timestamp("custody formal-gate now", now_utc)?
            >= timestamp("custody request expiry", &expires)?
        {
            return Err(invalid("custody formal-gate request has expired"));
        }
        let signed_request: CustodySignedPreReadRequestV1 = strict_jcs(&request_raw)?;
        verify_signed(&signed_request, pinned_authority)?;
        if signed_request.body != expectation.request {
            return Err(invalid(
                "custody formal-gate expectation does not exactly match issued measurements",
            ));
        }
        let (attestation_raw, attestation_digest, attestation_id, result, attempt_marker): (
            Vec<u8>,
            String,
            String,
            String,
            String,
        ) = transaction
            .query_row(
                "SELECT raw_jcs,raw_sha256,attestation_id,result,attempt_marker_sha256 \
                 FROM first_attempts WHERE request_id=?1",
                params![request_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .map_err(sql("load terminal custody verifier attempt"))?;
        if result != "passed" {
            return Err(invalid(
                "only a passing first off-NUC custody attempt can reach a formal gate",
            ));
        }
        let signed_attestation: CustodySignedAttestationV2 = strict_jcs(&attestation_raw)?;
        verify_signed(&signed_attestation, pinned_authority)?;
        validate_attestation(&signed_attestation.body, now_utc)?;
        if sha256_hex(&attestation_raw) != attestation_digest
            || signed_attestation.body.attestation_id != attestation_id
            || signed_attestation.body.pre_read_attempt_marker_sha256 != attempt_marker
        {
            return Err(invalid(
                "custody formal-gate stored attestation id, raw digest, or pre-read marker is substituted",
            ));
        }
        if signed_attestation.body.request != expectation.request
            || signed_attestation.body.pre_read_request_sha256 != request_digest
        {
            return Err(invalid(
                "custody formal-gate attestation does not exactly bind the issued request",
            ));
        }
        let consumption = CustodyFormalGateConsumptionV2 {
            consumption_id: Uuid::new_v4().to_string(),
            request_id: request_id.to_string(),
            attestation_id,
            request_raw_jcs_sha256: request_digest,
            attestation_raw_jcs_sha256: attestation_digest,
            pre_read_attempt_marker_sha256: attempt_marker,
            target_measurements_sha256: sha256_json(&expectation.request)?,
            raw_evidence_sha256: signed_attestation.body.raw_evidence_sha256,
            custody_marker_sha256: signed_attestation.body.custody_marker_sha256,
            receipt_jcs_sha256: signed_attestation.body.receipt_jcs_sha256,
            ledger_head_sha256: signed_attestation.body.request.ledger_head_sha256,
            object_lock_policy_sha256: signed_attestation.body.request.object_lock_policy_sha256,
            consumed_at_utc: now_utc.to_string(),
        };
        let consumption_jcs = jcs(&consumption)?;
        transaction
            .execute(
                "INSERT INTO formal_consumptions \
                 (request_id,attestation_id,attestation_raw_sha256,attempt_marker_sha256,consumption_jcs,consumed_at_utc) \
                 VALUES (?1,?2,?3,?4,?5,?6)",
                params![
                    request_id,
                    consumption.attestation_id,
                    consumption.attestation_raw_jcs_sha256,
                    consumption.pre_read_attempt_marker_sha256,
                    consumption_jcs,
                    now_utc,
                ],
            )
            .map_err(sql("atomically consume custody formal-gate attestation"))?;
        transaction
            .commit()
            .map_err(sql("commit custody formal-gate consumption"))?;
        Ok(consumption)
    }

    fn open_rw(&self) -> Result<Connection, ObjectServiceError> {
        Connection::open_with_flags(
            &self.path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        )
        .map_err(sql("open off-NUC custody journal"))
    }
}

/// Reserve the precise next target sequence before a request can be begun.
/// A failed terminal attempt releases no state from the ledger but also does
/// not advance the successful checkpoint; a new request may then reserve the
/// same next sequence with a new nonce. The partial unique index prevents two
/// active requests from racing to observe the same predecessor.
fn reserve_issued_sequence(
    transaction: &rusqlite::Transaction<'_>,
    request: &CustodyOffNucPreReadRequestV1,
) -> Result<(), ObjectServiceError> {
    let previous: Option<(u64, String)> = transaction
        .query_row(
            "SELECT sequence,request_sha256 FROM checkpoints WHERE target_id=?1",
            params![request.target_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(sql("read custody verifier checkpoint for issuance"))?;
    match previous {
        None if request.sequence == 1 && request.previous_request_sha256.is_none() => Ok(()),
        Some((sequence, prior))
            if request.sequence == sequence + 1
                && request.previous_request_sha256.as_deref() == Some(prior.as_str()) =>
        {
            Ok(())
        }
        _ => Err(invalid(
            "custody pre-read issuance sequence or predecessor is not monotonic",
        )),
    }
}

fn accept_attestation_transaction(
    transaction: &rusqlite::Transaction<'_>,
    record: &CustodySignedAttestationV2,
    raw_jcs: &[u8],
    now_utc: &str,
) -> Result<(), ObjectServiceError> {
    let body = &record.body;
    let (request_raw, request_digest, status, target): (Vec<u8>, String, String, String) = transaction
        .query_row(
            "SELECT raw_jcs,raw_sha256,status,target_id FROM issued_pre_read_requests WHERE request_id=?1",
            params![body.request.request_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(sql("load issued custody pre-read request for first attempt"))?;
    if status != "started" {
        return Err(invalid(
            "custody attestation requires a durably begun pre-read attempt before any target read",
        ));
    }
    let issued: CustodySignedPreReadRequestV1 = strict_jcs(&request_raw)?;
    if body.pre_read_request_sha256 != request_digest
        || body.request != issued.body
        || target != body.request.target_id
    {
        return Err(invalid(
            "custody attestation substitutes or detaches the issued pre-read request",
        ));
    }
    let marker: String = transaction
        .query_row(
            "SELECT attempt_marker_sha256 FROM first_attempts \
             WHERE request_id=?1 AND result='started'",
            params![body.request.request_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql("load immutable custody pre-read attempt marker"))?
        .ok_or_else(|| invalid("custody pre-read attempt marker is absent or already terminal"))?;
    if body.pre_read_attempt_marker_sha256 != marker {
        return Err(invalid(
            "custody attestation does not bind the journal-minted pre-read attempt marker",
        ));
    }
    if attestation_id_exists(transaction, &body.attestation_id)? {
        return Err(invalid(
            "custody attestation id is already retained by another first attempt",
        ));
    }
    let attestation_digest = sha256_hex(raw_jcs);
    complete_started_request(
        transaction,
        &body.request.request_id,
        Some(raw_jcs),
        Some(&attestation_digest),
        Some(&body.attestation_id),
        "passed",
        &body.result_detail,
        now_utc,
    )?;
    transaction
        .execute(
            "INSERT INTO checkpoints (target_id,sequence,request_sha256,request_id,updated_at_utc) VALUES (?1,?2,?3,?4,?5) \
             ON CONFLICT(target_id) DO UPDATE SET sequence=excluded.sequence,request_sha256=excluded.request_sha256,request_id=excluded.request_id,updated_at_utc=excluded.updated_at_utc",
            params![target, body.request.sequence, request_digest, body.request.request_id, now_utc],
        )
        .map_err(sql("advance custody off-NUC verifier checkpoint"))?;
    Ok(())
}

fn complete_started_request(
    transaction: &rusqlite::Transaction<'_>,
    request_id: &str,
    raw_jcs: Option<&[u8]>,
    raw_sha256: Option<&str>,
    attestation_id: Option<&str>,
    result: &str,
    detail: &str,
    attempted_at_utc: &str,
) -> Result<(), ObjectServiceError> {
    let changed = transaction
        .execute(
            "UPDATE first_attempts SET raw_jcs=?2,raw_sha256=?3,attestation_id=?4,result=?5,detail=?6,attempted_at_utc=?7 \
             WHERE request_id=?1 AND result='started'",
            params![request_id, raw_jcs, raw_sha256, attestation_id, result, detail, attempted_at_utc],
        )
        .map_err(sql("persist terminal first custody verifier attempt"))?;
    if changed != 1 {
        return Err(invalid(
            "custody pre-read attempt marker is absent or already terminal",
        ));
    }
    let changed = transaction
        .execute(
            "UPDATE issued_pre_read_requests SET status='terminal' WHERE request_id=?1 AND status='started'",
            params![request_id],
        )
        .map_err(sql("complete started custody pre-read request"))?;
    if changed != 1 {
        return Err(invalid(
            "custody pre-read request is absent, not begun, or already terminal",
        ));
    }
    Ok(())
}

fn attestation_id_exists(
    transaction: &rusqlite::Transaction<'_>,
    attestation_id: &str,
) -> Result<bool, ObjectServiceError> {
    transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM first_attempts WHERE attestation_id=?1)",
            params![attestation_id],
            |row| row.get(0),
        )
        .map_err(sql("check custody attestation id uniqueness"))
}

fn validate_attestation(
    body: &CustodyOffNucAttestationV2,
    now_utc: &str,
) -> Result<(), ObjectServiceError> {
    if body.schema != CUSTODY_OFF_NUC_ATTESTATION_SCHEMA_V2 {
        return Err(invalid("unsupported custody off-NUC attestation schema"));
    }
    uuid("custody off-NUC attestation id", &body.attestation_id)?;
    body.request.validate(now_utc)?;
    if body.observation_result != CustodyOffNucObservationResult::Passed {
        return Err(invalid(
            "only passing custody attestation results are admissible",
        ));
    }
    timestamp("custody attestation observed_at_utc", &body.observed_at_utc)?;
    for (field, value) in [
        ("custody pre-read request", &body.pre_read_request_sha256),
        (
            "custody pre-read attempt marker",
            &body.pre_read_attempt_marker_sha256,
        ),
        ("custody marker", &body.custody_marker_sha256),
        ("custody raw evidence", &body.raw_evidence_sha256),
        ("custody receipt JCS", &body.receipt_jcs_sha256),
        ("custody direct readback", &body.direct_readback_sha256),
    ] {
        sha256(field, value)?;
    }
    if body.receipt_jcs_sha256 != body.request.receipt_jcs_sha256 {
        return Err(invalid(
            "custody attestation receipt digest does not match issued measurement",
        ));
    }
    nonblank("custody attestation result detail", &body.result_detail)
}

fn verify_signed<T: Serialize>(
    record: &CustodySignedRecordV1<T>,
    pinned: &CustodyEd25519AuthorityV1,
) -> Result<(), ObjectServiceError> {
    if record.schema != CUSTODY_SIGNED_RECORD_SCHEMA_V1 || record.authority != *pinned {
        return Err(invalid(
            "custody signed record does not use the pinned authority contract",
        ));
    }
    let key = pinned.validate()?;
    let signature_bytes = strict_base64("custody Ed25519 signature", &record.signature_base64)?;
    let signature = Signature::from_slice(&signature_bytes).map_err(|error| {
        invalid(format!(
            "custody Ed25519 signature must be exactly 64 bytes: {error}"
        ))
    })?;
    key.verify_strict(jcs(&record.body)?.as_bytes(), &signature)
        .map_err(|_| invalid("custody Ed25519 signature verification failed"))
}

fn strict_jcs<T: DeserializeOwned + Serialize>(raw: &[u8]) -> Result<T, ObjectServiceError> {
    let text = std::str::from_utf8(raw)
        .map_err(|_| invalid("custody signed record raw bytes must be UTF-8 JCS"))?;
    let value: T = serde_json::from_str(text)
        .map_err(|error| invalid(format!("decode custody signed record: {error}")))?;
    if jcs(&value)? != text {
        return Err(invalid(
            "custody signed record must be exact canonical JCS bytes",
        ));
    }
    Ok(value)
}

fn strict_base64(field: &str, value: &str) -> Result<Vec<u8>, ObjectServiceError> {
    let bytes = STANDARD
        .decode(value)
        .map_err(|error| invalid(format!("{field} must be RFC4648 standard Base64: {error}")))?;
    if STANDARD.encode(&bytes) != value {
        return Err(invalid(format!(
            "{field} must use canonical RFC4648 standard Base64"
        )));
    }
    Ok(bytes)
}

fn jcs(value: &impl Serialize) -> Result<String, ObjectServiceError> {
    serde_jcs::to_string(value)
        .map_err(|error| invalid(format!("canonicalise custody signed JCS: {error}")))
}

fn sha256_json(value: &impl Serialize) -> Result<String, ObjectServiceError> {
    Ok(sha256_hex(jcs(value)?.as_bytes()))
}

fn sha256_hex(value: impl AsRef<[u8]>) -> String {
    hex::encode(Sha256::digest(value.as_ref()))
}

fn sha256(field: &str, value: &str) -> Result<(), ObjectServiceError> {
    if value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err(invalid(format!(
            "{field} must be a canonical lowercase SHA-256 hex digest"
        )))
    }
}

fn timestamp(field: &str, value: &str) -> Result<DateTime<Utc>, ObjectServiceError> {
    let parsed = DateTime::parse_from_rfc3339(value)
        .map_err(|error| invalid(format!("{field} must be RFC3339 UTC: {error}")))?;
    if parsed.offset().local_minus_utc() != 0 {
        return Err(invalid(format!("{field} must use a UTC Z offset")));
    }
    let canonical = parsed
        .with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Secs, true);
    if canonical != value {
        return Err(invalid(format!(
            "{field} must use canonical RFC3339 UTC seconds"
        )));
    }
    Ok(parsed.with_timezone(&Utc))
}

fn uuid(field: &str, value: &str) -> Result<(), ObjectServiceError> {
    let parsed = Uuid::parse_str(value)
        .map_err(|error| invalid(format!("{field} must be a UUID: {error}")))?;
    if parsed.to_string() != value {
        return Err(invalid(format!(
            "{field} must use canonical lowercase UUID text"
        )));
    }
    Ok(())
}

fn nonblank(field: &str, value: &str) -> Result<(), ObjectServiceError> {
    if value.trim().is_empty() {
        Err(invalid(format!("{field} must not be blank")))
    } else {
        Ok(())
    }
}

fn result_name(value: CustodyOffNucObservationResult) -> &'static str {
    match value {
        CustodyOffNucObservationResult::Passed => "passed",
        CustodyOffNucObservationResult::Failed => "failed",
        CustodyOffNucObservationResult::TimedOut => "timed_out",
        CustodyOffNucObservationResult::Incomplete => "incomplete",
    }
}

fn invalid(message: impl Into<String>) -> ObjectServiceError {
    ObjectServiceError::InvalidConfiguration(message.into())
}

fn sql(operation: &'static str) -> impl FnOnce(rusqlite::Error) -> ObjectServiceError {
    move |error| ObjectServiceError::CommandFailed(format!("{operation}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn digest(label: &str) -> String {
        sha256_hex(label)
    }

    fn authority() -> (SigningKey, CustodyEd25519AuthorityV1) {
        let signer = SigningKey::from_bytes(&[7; 32]);
        let public = signer.verifying_key().to_bytes();
        (
            signer,
            CustodyEd25519AuthorityV1 {
                authority_id: "off-nuc-verifier-a".to_string(),
                algorithm: CUSTODY_ATTESTATION_ALGORITHM_ED25519.to_string(),
                public_key_base64: STANDARD.encode(public),
                public_key_sha256: sha256_hex(public),
            },
        )
    }

    fn request(sequence: u64, previous: Option<String>) -> CustodyOffNucPreReadRequestV1 {
        CustodyOffNucPreReadRequestV1 {
            schema: CUSTODY_OFF_NUC_PRE_READ_REQUEST_SCHEMA_V1.to_string(),
            assurance_class: CUSTODY_ASSURANCE_CLASS.to_string(),
            request_id: Uuid::new_v4().to_string(),
            release_train: "r237".to_string(),
            release_stage: "s4".to_string(),
            purpose: "custody".to_string(),
            verifier_id: "verifier-a".to_string(),
            target_id: "nuc-193".to_string(),
            machine_identity_sha256: digest("machine"),
            s3_endpoint_authority: "http://192.168.0.193:3900".to_string(),
            endpoint_authority_sha256: digest("endpoint"),
            tls_peer_sha256: digest("tls"),
            routing_sha256: digest("route"),
            reader_identity: "reader-a".to_string(),
            store_id: "custody-a".to_string(),
            bucket_name: "custody-a".to_string(),
            stores_namespace_sha256: digest("namespace"),
            object_lock_policy_sha256: digest("policy"),
            lock_ledger_sha256: digest("lock-ledger"),
            ledger_head_sha256: digest("ledger-head"),
            inventory_sha256: digest("inventory"),
            lockset_sha256: digest("lockset"),
            verifier_executable_sha256: digest("exe"),
            verifier_provenance_sha256: digest("provenance"),
            receipt_jcs_sha256: digest("receipt"),
            nonce: Uuid::new_v4().to_string(),
            sequence,
            previous_request_sha256: previous,
            issued_at_utc: "2026-09-05T10:00:00Z".to_string(),
            expires_at_utc: "2026-09-05T12:00:00Z".to_string(),
        }
    }

    fn sign<T: Serialize>(
        body: T,
        signer: &SigningKey,
        authority: &CustodyEd25519AuthorityV1,
    ) -> Vec<u8> {
        let signature = signer.sign(jcs(&body).unwrap().as_bytes());
        jcs(&CustodySignedRecordV1 {
            schema: CUSTODY_SIGNED_RECORD_SCHEMA_V1.to_string(),
            body,
            authority: authority.clone(),
            signature_base64: STANDARD.encode(signature.to_bytes()),
        })
        .unwrap()
        .into_bytes()
    }

    #[test]
    fn strict_jcs_ed25519_journal_consumes_one_passing_attempt_and_formal_gate() {
        let root = std::env::temp_dir().join(format!("das-custody-attestation-{}", Uuid::new_v4()));
        let journal = CustodyOffNucJournal::create(root.join("journal.sqlite")).unwrap();
        let (signer, authority) = authority();
        let request = request(1, None);
        let raw_request = sign(request.clone(), &signer, &authority);
        let request_digest = journal
            .issue_pre_read_request(&raw_request, &authority, "2026-09-05T10:01:00Z")
            .unwrap();
        let attempt = journal
            .begin_pre_read_attempt(&request.request_id, "2026-09-05T10:01:30Z")
            .unwrap();
        let body = CustodyOffNucAttestationV2 {
            schema: CUSTODY_OFF_NUC_ATTESTATION_SCHEMA_V2.to_string(),
            attestation_id: Uuid::new_v4().to_string(),
            request: request.clone(),
            pre_read_request_sha256: request_digest,
            pre_read_attempt_marker_sha256: attempt.attempt_marker_sha256,
            observation_result: CustodyOffNucObservationResult::Passed,
            observed_at_utc: "2026-09-05T10:02:00Z".to_string(),
            custody_marker_sha256: digest("marker"),
            raw_evidence_sha256: digest("evidence"),
            receipt_jcs_sha256: request.receipt_jcs_sha256.clone(),
            direct_readback_sha256: digest("readback"),
            result_detail: "passed".to_string(),
        };
        let expected_attestation_id = body.attestation_id.clone();
        let raw_attestation = sign(body, &signer, &authority);
        let expected_attestation_digest = sha256_hex(&raw_attestation);
        journal
            .accept_signed_attestation(
                &request.request_id,
                &raw_attestation,
                &authority,
                "2026-09-05T10:03:00Z",
            )
            .unwrap();
        let consumed = journal
            .consume_for_formal_gate(
                &request.request_id,
                &CustodyFormalGateExpectationV2 {
                    request: request.clone(),
                },
                &authority,
                "2026-09-05T10:04:00Z",
            )
            .unwrap();
        assert_eq!(consumed.request_id, request.request_id);
        assert_eq!(consumed.attestation_id, expected_attestation_id);
        assert_eq!(
            consumed.attestation_raw_jcs_sha256,
            expected_attestation_digest
        );
        let connection = Connection::open(&journal.path).unwrap();
        let persisted: (String, String, String, String) = connection
            .query_row(
                "SELECT raw_sha256,attestation_id,result,attempt_marker_sha256 \
                 FROM first_attempts WHERE request_id=?1",
                params![request.request_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(persisted.0, expected_attestation_digest);
        assert_eq!(persisted.1, expected_attestation_id);
        assert_eq!(persisted.2, "passed");
        assert_eq!(persisted.3, consumed.pre_read_attempt_marker_sha256);
        let formal: (String, String, String) = connection
            .query_row(
                "SELECT attestation_id,attestation_raw_sha256,attempt_marker_sha256 \
                 FROM formal_consumptions WHERE request_id=?1",
                params![request.request_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(formal.0, consumed.attestation_id);
        assert_eq!(formal.1, consumed.attestation_raw_jcs_sha256);
        assert_eq!(formal.2, consumed.pre_read_attempt_marker_sha256);
        assert!(journal
            .consume_for_formal_gate(
                &request.request_id,
                &CustodyFormalGateExpectationV2 {
                    request: request.clone()
                },
                &authority,
                "2026-09-05T10:04:01Z"
            )
            .is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn invalid_or_timeout_first_attempt_blocks_a_later_replacement() {
        let root = std::env::temp_dir().join(format!("das-custody-attempt-{}", Uuid::new_v4()));
        let journal = CustodyOffNucJournal::create(root.join("journal.sqlite")).unwrap();
        let (signer, authority) = authority();
        let request = request(1, None);
        let raw_request = sign(request.clone(), &signer, &authority);
        let digest_request = journal
            .issue_pre_read_request(&raw_request, &authority, "2026-09-05T10:01:00Z")
            .unwrap();
        let attempt = journal
            .begin_pre_read_attempt(&request.request_id, "2026-09-05T10:01:30Z")
            .unwrap();
        journal
            .record_terminal_failure(
                &request.request_id,
                CustodyOffNucObservationResult::TimedOut,
                "deadline",
                "2026-09-05T10:02:00Z",
            )
            .unwrap();
        let request_id = request.request_id.clone();
        let body = CustodyOffNucAttestationV2 {
            schema: CUSTODY_OFF_NUC_ATTESTATION_SCHEMA_V2.to_string(),
            attestation_id: Uuid::new_v4().to_string(),
            request,
            pre_read_request_sha256: digest_request,
            pre_read_attempt_marker_sha256: attempt.attempt_marker_sha256,
            observation_result: CustodyOffNucObservationResult::Passed,
            observed_at_utc: "2026-09-05T10:02:00Z".to_string(),
            custody_marker_sha256: digest("marker"),
            raw_evidence_sha256: digest("evidence"),
            receipt_jcs_sha256: digest("receipt"),
            direct_readback_sha256: digest("readback"),
            result_detail: "passed".to_string(),
        };
        assert!(journal
            .accept_signed_attestation(
                &request_id,
                &sign(body, &signer, &authority),
                &authority,
                "2026-09-05T10:03:00Z"
            )
            .is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn raw_jcs_and_signature_substitution_are_rejected() {
        let (signer, authority) = authority();
        let raw = sign(request(1, None), &signer, &authority);
        let mut whitespace = raw.clone();
        whitespace.insert(0, b' ');
        assert!(strict_jcs::<CustodySignedPreReadRequestV1>(&whitespace).is_err());
        let mut record: CustodySignedPreReadRequestV1 = strict_jcs(&raw).unwrap();
        record.signature_base64 = STANDARD.encode([0_u8; 64]);
        assert!(verify_signed(&record, &authority).is_err());
        let mut different = authority.clone();
        different.authority_id = "other".to_string();
        assert!(verify_signed(
            &strict_jcs::<CustodySignedPreReadRequestV1>(&raw).unwrap(),
            &different
        )
        .is_err());
    }

    #[test]
    fn journal_begins_before_the_sole_remote_read_and_crash_or_race_cannot_replace_it() {
        let root = std::env::temp_dir().join(format!("das-custody-begin-{}", Uuid::new_v4()));
        let path = root.join("journal.sqlite");
        let journal = CustodyOffNucJournal::create(&path).unwrap();
        let (signer, authority) = authority();
        let first = request(1, None);
        let raw_first = sign(first.clone(), &signer, &authority);
        journal
            .issue_pre_read_request(&raw_first, &authority, "2026-09-05T10:01:00Z")
            .unwrap();
        let second = request(1, None);
        assert!(journal
            .issue_pre_read_request(
                &sign(second.clone(), &signer, &authority),
                &authority,
                "2026-09-05T10:01:01Z",
            )
            .is_err());

        let mut reader_observed_started_marker = false;
        journal
            .perform_pre_read(&first.request_id, "2026-09-05T10:01:30Z", |permit| {
                let connection = Connection::open(&path).unwrap();
                let persisted: (String, String) = connection
                    .query_row(
                        "SELECT status,attempt_marker_sha256 FROM issued_pre_read_requests \
                             JOIN first_attempts USING(request_id) WHERE request_id=?1",
                        params![permit.request_id],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .unwrap();
                assert_eq!(persisted.0, "started");
                assert_eq!(persisted.1, permit.attempt_marker_sha256);
                reader_observed_started_marker = true;
                Ok(())
            })
            .unwrap();
        assert!(reader_observed_started_marker);
        assert!(journal
            .begin_pre_read_attempt(&first.request_id, "2026-09-05T10:01:31Z")
            .is_err());
        drop(journal);
        let reopened = CustodyOffNucJournal::open_existing(&path).unwrap();
        assert!(reopened
            .begin_pre_read_attempt(&first.request_id, "2026-09-05T10:01:32Z")
            .is_err());
        reopened
            .record_terminal_failure(
                &first.request_id,
                CustodyOffNucObservationResult::TimedOut,
                "verifier crashed after the durable pre-read marker",
                "2026-09-05T10:02:00Z",
            )
            .unwrap();
        // Terminal failure frees a new *nonce* to reserve the still-unmet
        // sequence, but never permits a second read under the original nonce.
        reopened
            .issue_pre_read_request(
                &sign(second, &signer, &authority),
                &authority,
                "2026-09-05T10:02:01Z",
            )
            .unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn duplicate_attestation_id_is_terminal_and_never_swaps_the_raw_digest_mapping() {
        let root =
            std::env::temp_dir().join(format!("das-custody-attestation-id-{}", Uuid::new_v4()));
        let journal = CustodyOffNucJournal::create(root.join("journal.sqlite")).unwrap();
        let (signer, authority) = authority();
        let first = request(1, None);
        let first_raw = sign(first.clone(), &signer, &authority);
        let first_digest = journal
            .issue_pre_read_request(&first_raw, &authority, "2026-09-05T10:01:00Z")
            .unwrap();
        let first_attempt = journal
            .begin_pre_read_attempt(&first.request_id, "2026-09-05T10:01:30Z")
            .unwrap();
        let duplicate_id = Uuid::new_v4().to_string();
        let first_body = CustodyOffNucAttestationV2 {
            schema: CUSTODY_OFF_NUC_ATTESTATION_SCHEMA_V2.to_string(),
            attestation_id: duplicate_id.clone(),
            request: first.clone(),
            pre_read_request_sha256: first_digest.clone(),
            pre_read_attempt_marker_sha256: first_attempt.attempt_marker_sha256,
            observation_result: CustodyOffNucObservationResult::Passed,
            observed_at_utc: "2026-09-05T10:02:00Z".to_string(),
            custody_marker_sha256: digest("first-marker"),
            raw_evidence_sha256: digest("first-evidence"),
            receipt_jcs_sha256: first.receipt_jcs_sha256.clone(),
            direct_readback_sha256: digest("first-readback"),
            result_detail: "passed".to_string(),
        };
        journal
            .accept_signed_attestation(
                &first.request_id,
                &sign(first_body, &signer, &authority),
                &authority,
                "2026-09-05T10:03:00Z",
            )
            .unwrap();
        let second = request(2, Some(first_digest));
        let second_raw = sign(second.clone(), &signer, &authority);
        let second_digest = journal
            .issue_pre_read_request(&second_raw, &authority, "2026-09-05T10:04:00Z")
            .unwrap();
        let second_attempt = journal
            .begin_pre_read_attempt(&second.request_id, "2026-09-05T10:04:30Z")
            .unwrap();
        let duplicate_body = CustodyOffNucAttestationV2 {
            schema: CUSTODY_OFF_NUC_ATTESTATION_SCHEMA_V2.to_string(),
            attestation_id: duplicate_id,
            request: second.clone(),
            pre_read_request_sha256: second_digest,
            pre_read_attempt_marker_sha256: second_attempt.attempt_marker_sha256,
            observation_result: CustodyOffNucObservationResult::Passed,
            observed_at_utc: "2026-09-05T10:05:00Z".to_string(),
            custody_marker_sha256: digest("second-marker"),
            raw_evidence_sha256: digest("second-evidence"),
            receipt_jcs_sha256: second.receipt_jcs_sha256.clone(),
            direct_readback_sha256: digest("second-readback"),
            result_detail: "passed".to_string(),
        };
        assert!(journal
            .accept_signed_attestation(
                &second.request_id,
                &sign(duplicate_body, &signer, &authority),
                &authority,
                "2026-09-05T10:06:00Z",
            )
            .is_err());
        let connection = Connection::open(&journal.path).unwrap();
        let retained: (Option<String>, Option<String>, String) = connection
            .query_row(
                "SELECT attestation_id,raw_sha256,result FROM first_attempts WHERE request_id=?1",
                params![second.request_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(retained.0, None, "a duplicate id cannot be re-retained");
        assert!(retained.1.is_some(), "raw response digest is retained");
        assert_eq!(retained.2, "failed");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn malformed_wire_response_consumes_the_issued_nonce_before_any_retry() {
        let root = std::env::temp_dir().join(format!("das-custody-malformed-{}", Uuid::new_v4()));
        let journal = CustodyOffNucJournal::create(root.join("journal.sqlite")).unwrap();
        let (signer, authority) = authority();
        let request = request(1, None);
        let raw_request = sign(request.clone(), &signer, &authority);
        let request_digest = journal
            .issue_pre_read_request(&raw_request, &authority, "2026-09-05T10:01:00Z")
            .unwrap();
        let attempt = journal
            .begin_pre_read_attempt(&request.request_id, "2026-09-05T10:01:30Z")
            .unwrap();
        assert!(journal
            .accept_signed_attestation(
                &request.request_id,
                b"not-json",
                &authority,
                "2026-09-05T10:02:00Z"
            )
            .is_err());
        let body = CustodyOffNucAttestationV2 {
            schema: CUSTODY_OFF_NUC_ATTESTATION_SCHEMA_V2.to_string(),
            attestation_id: Uuid::new_v4().to_string(),
            request: request.clone(),
            pre_read_request_sha256: request_digest,
            pre_read_attempt_marker_sha256: attempt.attempt_marker_sha256,
            observation_result: CustodyOffNucObservationResult::Passed,
            observed_at_utc: "2026-09-05T10:02:00Z".to_string(),
            custody_marker_sha256: digest("marker"),
            raw_evidence_sha256: digest("evidence"),
            receipt_jcs_sha256: request.receipt_jcs_sha256.clone(),
            direct_readback_sha256: digest("readback"),
            result_detail: "passed".to_string(),
        };
        assert!(journal
            .accept_signed_attestation(
                &request.request_id,
                &sign(body, &signer, &authority),
                &authority,
                "2026-09-05T10:03:00Z"
            )
            .is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn each_attestation_measurement_is_bound_to_the_issued_request() {
        let root = std::env::temp_dir().join(format!("das-custody-measurement-{}", Uuid::new_v4()));
        let journal = CustodyOffNucJournal::create(root.join("journal.sqlite")).unwrap();
        let (signer, authority) = authority();
        let request = request(1, None);
        let raw_request = sign(request.clone(), &signer, &authority);
        let request_digest = journal
            .issue_pre_read_request(&raw_request, &authority, "2026-09-05T10:01:00Z")
            .unwrap();
        let attempt = journal
            .begin_pre_read_attempt(&request.request_id, "2026-09-05T10:01:30Z")
            .unwrap();
        let mut substituted = request.clone();
        substituted.tls_peer_sha256 = digest("substituted-tls");
        let body = CustodyOffNucAttestationV2 {
            schema: CUSTODY_OFF_NUC_ATTESTATION_SCHEMA_V2.to_string(),
            attestation_id: Uuid::new_v4().to_string(),
            request: substituted,
            pre_read_request_sha256: request_digest,
            pre_read_attempt_marker_sha256: attempt.attempt_marker_sha256,
            observation_result: CustodyOffNucObservationResult::Passed,
            observed_at_utc: "2026-09-05T10:02:00Z".to_string(),
            custody_marker_sha256: digest("marker"),
            raw_evidence_sha256: digest("evidence"),
            receipt_jcs_sha256: request.receipt_jcs_sha256.clone(),
            direct_readback_sha256: digest("readback"),
            result_detail: "passed".to_string(),
        };
        assert!(journal
            .accept_signed_attestation(
                &request.request_id,
                &sign(body, &signer, &authority),
                &authority,
                "2026-09-05T10:03:00Z"
            )
            .is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn formal_gate_refuses_a_substituted_full_measurement_contract() {
        let root = std::env::temp_dir().join(format!(
            "das-custody-formal-substitution-{}",
            Uuid::new_v4()
        ));
        let journal = CustodyOffNucJournal::create(root.join("journal.sqlite")).unwrap();
        let (signer, authority) = authority();
        let request = request(1, None);
        let raw_request = sign(request.clone(), &signer, &authority);
        let request_digest = journal
            .issue_pre_read_request(&raw_request, &authority, "2026-09-05T10:01:00Z")
            .unwrap();
        let attempt = journal
            .begin_pre_read_attempt(&request.request_id, "2026-09-05T10:01:30Z")
            .unwrap();
        let body = CustodyOffNucAttestationV2 {
            schema: CUSTODY_OFF_NUC_ATTESTATION_SCHEMA_V2.to_string(),
            attestation_id: Uuid::new_v4().to_string(),
            request: request.clone(),
            pre_read_request_sha256: request_digest,
            pre_read_attempt_marker_sha256: attempt.attempt_marker_sha256,
            observation_result: CustodyOffNucObservationResult::Passed,
            observed_at_utc: "2026-09-05T10:02:00Z".to_string(),
            custody_marker_sha256: digest("marker"),
            raw_evidence_sha256: digest("evidence"),
            receipt_jcs_sha256: request.receipt_jcs_sha256.clone(),
            direct_readback_sha256: digest("readback"),
            result_detail: "passed".to_string(),
        };
        journal
            .accept_signed_attestation(
                &request.request_id,
                &sign(body, &signer, &authority),
                &authority,
                "2026-09-05T10:03:00Z",
            )
            .unwrap();
        let mut expectation = request.clone();
        expectation.object_lock_policy_sha256 = digest("substituted-policy");
        assert!(journal
            .consume_for_formal_gate(
                &request.request_id,
                &CustodyFormalGateExpectationV2 {
                    request: expectation
                },
                &authority,
                "2026-09-05T10:04:00Z"
            )
            .is_err());
        assert!(journal
            .consume_for_formal_gate(
                &request.request_id,
                &CustodyFormalGateExpectationV2 {
                    request: request.clone(),
                },
                &authority,
                "2026-09-05T10:04:00Z"
            )
            .is_ok());
        let _ = fs::remove_dir_all(root);
    }
}
