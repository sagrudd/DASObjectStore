//! Source-only, fail-closed custody substrate for the S6 dossier corpus.
//!
//! This module deliberately has no provider, daemon, Pistis, filesystem, or
//! network implementation.  It validates the documented `0.178.0` corpus
//! shape, drives only caller-supplied fixed-peer ports, and returns records
//! which a later reviewed daemon boundary must retain.  It does not record S6
//! and is not authority to build, install, or activate a package.

use crate::{
    AuthorityScopeV1, DigestV1, EvidenceRefV1, JenkinsDossierEvidenceProjectionV1, ObjectRefV1,
    JENKINS_DOSSIER_EVIDENCE_PROJECTION_V1_SCHEMA,
};
use chrono::DateTime;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeSet;
use std::fmt::{self, Display};
use std::io::{Cursor, Read};

pub const S6_DOSSIER_CORPUS_V1_SCHEMA: &str = "mnemosyne.expedition.s6-dossier-corpus.v1";
pub const S6_DOSSIER_MANIFEST_V1_SCHEMA: &str =
    "mnemosyne.expedition.s6-dossier-corpus-manifest.v1";
pub const S6_DOSSIER_SUBJECT_V1_SCHEMA: &str = "mnemosyne.expedition.s6-dossier-subject.v1";
pub const S6_DOSSIER_CUSTODY_BINDING_V1_SCHEMA: &str =
    "mnemosyne.expedition.s6-dossier-custody-binding.v1";
pub const S6_DOSSIER_READBACK_RECEIPT_V1_SCHEMA: &str =
    "mnemosyne.das.s6-dossier-readback-receipt.v1";
pub const S6_DOSSIER_FIXED_PEER_GRANT_V1_SCHEMA: &str =
    "dasobjectstore.s6-dossier-fixed-peer-grant.v1";
pub const S6_DOSSIER_PROFILE_ID: &str = "dasobjectstore-0180-nuc-debian";
pub const S6_DOSSIER_PACKAGE_VERSION: &str = "0.178.0";
pub const S6_DOSSIER_OBJECT_PREFIX: &str = "expedition/release-trains";
pub const S6_DOSSIER_WRITER_SCOPE: &str = "s6-dossier-corpus-write";
pub const S6_DOSSIER_READER_SCOPE: &str = "s6-dossier-corpus-readback";
pub const S6_DOSSIER_WRITE_CAPABILITY: &str = "dasobjectstore.retained-evidence.write";
pub const S6_DOSSIER_READ_CAPABILITY: &str = "dasobjectstore.retained-evidence.read";

const ENVELOPE_MAGIC: &[u8; 8] = b"MNS6DCRP";
const ENVELOPE_VERSION: u8 = 1;
const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;
const MAX_SUBJECT_BYTES: usize = 16 * 1024 * 1024;
const MAX_CUSTODY_BINDING_BYTES: u64 = 64 * 1024;
const SUBJECT_DOMAIN_PREFIX: &[u8] = b"mnemosyne.expedition.s6-dossier-subject.v1\0";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct S6DossierMemberV1 {
    pub logical_name: String,
    pub media_type: String,
    pub size: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct S6DossierMemberRefV1 {
    pub logical_name: String,
    pub sha256: String,
    pub size: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct S6DossierCorpusManifestV1 {
    pub schema: String,
    pub members: Vec<S6DossierMemberV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct S6DossierSigningAuthorityV1 {
    pub authority_id: String,
    pub authority_record: S6DossierMemberRefV1,
    pub public_key_pem: S6DossierMemberRefV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct S6DossierReleaseV1 {
    pub profile_id: String,
    pub selected_product_ids: Vec<String>,
    pub package_format: String,
    pub architecture: String,
    pub signing_authority: S6DossierSigningAuthorityV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct S6DossierS0V1 {
    pub release_input: S6DossierMemberRefV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct S6DossierS1V1 {
    pub source_registry_review: S6DossierMemberRefV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct S6DossierS2V1 {
    pub certificate: S6DossierMemberRefV1,
    pub sealed_plan: S6DossierMemberRefV1,
    pub preflight: S6DossierMemberRefV1,
    pub s0_manifest: S6DossierMemberRefV1,
    pub predecessor_lock: S6DossierMemberRefV1,
    pub successor_lock: S6DossierMemberRefV1,
    pub products_registry: S6DossierMemberRefV1,
    pub source_dependencies_registry: S6DossierMemberRefV1,
    pub release_control_authorities_registry: S6DossierMemberRefV1,
    pub predecessor_catalogue: S6DossierMemberRefV1,
    pub successor_catalogue: S6DossierMemberRefV1,
    pub successor_sources_lock: S6DossierMemberRefV1,
    pub package_plan: S6DossierMemberRefV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct S6DossierS3V1 {
    pub acceptance_attestation: S6DossierMemberRefV1,
    pub accepted_lock: S6DossierMemberRefV1,
    pub canonical_main_witness: S6DossierMemberRefV1,
    pub authority_pem: S6DossierMemberRefV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct S6DossierS4V1 {
    pub projection: S6DossierMemberRefV1,
    pub catalogue: S6DossierMemberRefV1,
    pub profile: S6DossierMemberRefV1,
    pub source_lock: S6DossierMemberRefV1,
    pub acceptance_receipt: S6DossierMemberRefV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct S6DossierContinuityV1 {
    pub kind: String,
    pub record: S6DossierMemberRefV1,
    pub fallback_reason: Option<S6DossierMemberRefV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct S6DossierPackageV1 {
    pub component_id: String,
    pub package_name: String,
    pub package_version: String,
    pub architecture: String,
    pub package: S6DossierMemberRefV1,
    pub sbom: S6DossierMemberRefV1,
    pub provenance: S6DossierMemberRefV1,
    pub source_revision: String,
    pub source_tree_sha256: String,
    pub cargo_lock: S6DossierMemberRefV1,
    pub build_action: String,
    pub continuity: S6DossierContinuityV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct S6DossierS5V1 {
    pub build_receipt: S6DossierMemberRefV1,
    pub packages: Vec<S6DossierPackageV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct S6DossierSubjectCorpusV1 {
    pub manifest_sha256: String,
    pub members: Vec<S6DossierMemberV1>,
}

/// Strict pre-custody subject.  This is deliberately outside the corpus: if
/// it were a member, its required complete inventory would include its own
/// raw digest and create the prohibited fixed point.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct S6DossierSubjectV1 {
    pub schema: String,
    pub train_id: String,
    pub release: S6DossierReleaseV1,
    pub s0: S6DossierS0V1,
    pub s1: S6DossierS1V1,
    pub s2: S6DossierS2V1,
    pub s3: S6DossierS3V1,
    pub s4: S6DossierS4V1,
    pub s5: S6DossierS5V1,
    pub custody_binding: S6DossierMemberRefV1,
    pub corpus: S6DossierSubjectCorpusV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct S6DossierPeerIdentityV1 {
    pub identity: String,
    pub scope: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct S6DossierCustodyBindingV1 {
    pub schema: String,
    pub train_id: String,
    pub authority_scope: AuthorityScopeV1,
    pub store_id: String,
    pub object_id: String,
    pub object_version: u64,
    pub evidence_kind: String,
    pub evidence_revision: u64,
    pub writer: S6DossierPeerIdentityV1,
    pub reader: S6DossierPeerIdentityV1,
}

/// Credential-free Pistis/Prosopikon facts at the existing retained-dossier
/// service boundary.  A later daemon integration must bind this to its Unix
/// peer and live authority decision; this type never treats it as a secret.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct S6DossierFixedPeerGrantV1 {
    pub schema: String,
    pub peer_identity: String,
    pub authority_id: String,
    pub authority_revision: u64,
    pub session_id: String,
    pub principal_id: String,
    pub entitlement_assignment_id: String,
    pub entitlement: String,
    pub session_expires_at_utc: String,
    pub capability: String,
    pub canonical_prefix: String,
    pub dossier_subject_sha256: String,
    pub evidence_revision: u64,
    pub authority_scope: AuthorityScopeV1,
}

/// Non-secret opaque labels supplied by separately initialised fixed peers.
/// Equality is a denial: the writer cannot manufacture its own readback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct S6DossierPeerChannelV1 {
    pub grant: S6DossierFixedPeerGrantV1,
    pub credential_binding_id: String,
    pub process_instance_id: String,
    pub cache_instance_id: String,
    pub upload_handle_id: String,
    pub staging_path_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedS6DossierCorpusV1 {
    pub manifest: S6DossierCorpusManifestV1,
    pub manifest_sha256: String,
    pub corpus_sha256: String,
    pub corpus_size: u64,
    custody_binding_raw: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct S6DossierCustodyPreflightV1 {
    pub subject: S6DossierSubjectV1,
    pub dossier_subject_sha256: String,
    pub custody_binding: S6DossierCustodyBindingV1,
    pub corpus: VerifiedS6DossierCorpusV1,
    pub storage_key: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct S6DossierRawAttachmentV1 {
    pub schema: String,
    pub sha256: String,
    pub size: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct S6DossierReadbackReceiptV1 {
    pub schema: String,
    pub train_id: String,
    pub dossier_subject_sha256: String,
    pub corpus_sha256: String,
    pub corpus_size: String,
    pub manifest_sha256: String,
    pub members: Vec<S6DossierMemberV1>,
    pub authority_scope: AuthorityScopeV1,
    pub store_id: String,
    pub object_id: String,
    pub object_version: u64,
    pub evidence_kind: String,
    pub evidence_revision: u64,
    pub writer_identity: String,
    pub writer_scope: String,
    pub reader_identity: String,
    pub reader_scope: String,
    pub object_ref: S6DossierRawAttachmentV1,
    pub evidence_ref: S6DossierRawAttachmentV1,
    pub write_outcome: String,
    pub readback_result: String,
    pub readback_at: String,
    pub receipt_digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum S6DossierCreateOutcomeV1 {
    Created,
    AlreadyExists,
    Conflict,
}

/// The writer is opened only after all local preflight checks pass.
pub trait S6DossierWriterPortV1 {
    fn store_id(&self) -> &str;
    fn peer_identity(&self) -> &str;
    fn create_if_absent(
        &mut self,
        storage_key: &str,
        corpus: &[u8],
    ) -> Result<S6DossierCreateOutcomeV1, S6DossierCustodyError>;
}

/// The reader must be a separately initialised fixed-peer path.  Returning a
/// writer cache, staging file, or other store is rejected by the caller-owned
/// port identity facts before any receipt can be made.
pub trait S6DossierReaderPortV1 {
    fn store_id(&self) -> &str;
    fn peer_identity(&self) -> &str;
    fn read_independently(
        &mut self,
        storage_key: &str,
    ) -> Result<Box<dyn Read>, S6DossierCustodyError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct S6DossierRetentionResultV1 {
    pub preflight: S6DossierCustodyPreflightV1,
    pub receipt: S6DossierReadbackReceiptV1,
    pub receipt_jcs: Vec<u8>,
    pub object_ref_jcs: Vec<u8>,
    pub evidence_ref_jcs: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum S6DossierCustodyError {
    InvalidEnvelope,
    TruncatedEnvelope,
    ExtraEnvelopeBytes,
    InvalidManifest,
    InvalidSubject,
    InvalidCustodyBinding,
    InvalidFixedPeerGrant,
    SharedPeerChannel,
    StoreMismatch,
    PeerMismatch,
    ImmutableConflict,
    WriteFailed,
    ReadbackUnavailable,
    ReadbackMismatch,
    InvalidReceipt,
}

impl Display for S6DossierCustodyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidEnvelope => "invalid_envelope",
            Self::TruncatedEnvelope => "truncated_envelope",
            Self::ExtraEnvelopeBytes => "extra_envelope_bytes",
            Self::InvalidManifest => "invalid_manifest",
            Self::InvalidSubject => "invalid_subject",
            Self::InvalidCustodyBinding => "invalid_custody_binding",
            Self::InvalidFixedPeerGrant => "invalid_fixed_peer_grant",
            Self::SharedPeerChannel => "shared_peer_channel",
            Self::StoreMismatch => "store_mismatch",
            Self::PeerMismatch => "peer_mismatch",
            Self::ImmutableConflict => "immutable_conflict",
            Self::WriteFailed => "write_failed",
            Self::ReadbackUnavailable => "readback_unavailable",
            Self::ReadbackMismatch => "readback_mismatch",
            Self::InvalidReceipt => "invalid_receipt",
        })
    }
}

impl std::error::Error for S6DossierCustodyError {}

/// Parse the raw external subject, stream-verify the envelope, and prove the
/// fixed-peer preconditions.  It has no writer or reader parameter and hence
/// no storage side effect.
pub fn preflight_s6_dossier_custody(
    subject_jcs: &[u8],
    corpus: &[u8],
    writer: &S6DossierPeerChannelV1,
    reader: &S6DossierPeerChannelV1,
) -> Result<S6DossierCustodyPreflightV1, S6DossierCustodyError> {
    if subject_jcs.len() > MAX_SUBJECT_BYTES {
        return Err(S6DossierCustodyError::InvalidSubject);
    }
    let subject: S6DossierSubjectV1 =
        strict_jcs(subject_jcs, S6DossierCustodyError::InvalidSubject)?;
    validate_subject_shape(&subject)?;
    let dossier_subject_sha256 = domain_digest(SUBJECT_DOMAIN_PREFIX, subject_jcs);
    let mut cursor = Cursor::new(corpus);
    let verified = verify_s6_dossier_corpus(&mut cursor, &subject.custody_binding)?;
    if verified.manifest_sha256 != subject.corpus.manifest_sha256
        || verified.manifest.members != subject.corpus.members
    {
        return Err(S6DossierCustodyError::InvalidSubject);
    }
    let binding: S6DossierCustodyBindingV1 = strict_jcs(
        &verified.custody_binding_raw,
        S6DossierCustodyError::InvalidCustodyBinding,
    )?;
    validate_binding(&binding, &subject, &verified)?;
    validate_peer_channels(writer, reader, &binding, &dossier_subject_sha256)?;
    Ok(S6DossierCustodyPreflightV1 {
        storage_key: canonical_storage_key(&subject.train_id, &verified.corpus_sha256)?,
        subject,
        dossier_subject_sha256,
        custody_binding: binding,
        corpus: verified,
    })
}

/// Complete the source-only immutable custody transaction through separately
/// supplied ports.  Both creation and an equal replay require an independent
/// byte-for-byte readback before the strict receipt is returned.
pub fn retain_s6_dossier_corpus(
    subject_jcs: &[u8],
    corpus: &[u8],
    writer_channel: &S6DossierPeerChannelV1,
    reader_channel: &S6DossierPeerChannelV1,
    readback_at: &str,
    writer: &mut dyn S6DossierWriterPortV1,
    reader: &mut dyn S6DossierReaderPortV1,
) -> Result<S6DossierRetentionResultV1, S6DossierCustodyError> {
    let preflight =
        preflight_s6_dossier_custody(subject_jcs, corpus, writer_channel, reader_channel)?;
    if writer.store_id() != preflight.custody_binding.store_id
        || reader.store_id() != preflight.custody_binding.store_id
    {
        return Err(S6DossierCustodyError::StoreMismatch);
    }
    if writer.peer_identity() != writer_channel.grant.peer_identity
        || reader.peer_identity() != reader_channel.grant.peer_identity
    {
        return Err(S6DossierCustodyError::PeerMismatch);
    }
    let write_outcome = match writer.create_if_absent(&preflight.storage_key, corpus)? {
        S6DossierCreateOutcomeV1::Created => "created",
        S6DossierCreateOutcomeV1::AlreadyExists => "existing-equal",
        S6DossierCreateOutcomeV1::Conflict => return Err(S6DossierCustodyError::ImmutableConflict),
    };
    let mut independent_reader = reader
        .read_independently(&preflight.storage_key)
        .map_err(|_| S6DossierCustodyError::ReadbackUnavailable)?;
    let readback =
        verify_s6_dossier_corpus(&mut independent_reader, &preflight.subject.custody_binding)
            .map_err(|_| S6DossierCustodyError::ReadbackMismatch)?;
    if readback.manifest != preflight.corpus.manifest
        || readback.manifest_sha256 != preflight.corpus.manifest_sha256
        || readback.corpus_sha256 != preflight.corpus.corpus_sha256
        || readback.corpus_size != preflight.corpus.corpus_size
        || readback.custody_binding_raw != preflight.corpus.custody_binding_raw
    {
        return Err(S6DossierCustodyError::ReadbackMismatch);
    }
    let (object_ref, evidence_ref, object_ref_jcs, evidence_ref_jcs) =
        project_references(&preflight)?;
    let mut receipt = S6DossierReadbackReceiptV1 {
        schema: S6_DOSSIER_READBACK_RECEIPT_V1_SCHEMA.to_owned(),
        train_id: preflight.subject.train_id.clone(),
        dossier_subject_sha256: preflight.dossier_subject_sha256.clone(),
        corpus_sha256: preflight.corpus.corpus_sha256.clone(),
        corpus_size: preflight.corpus.corpus_size.to_string(),
        manifest_sha256: preflight.corpus.manifest_sha256.clone(),
        members: preflight.corpus.manifest.members.clone(),
        authority_scope: preflight.custody_binding.authority_scope.clone(),
        store_id: preflight.custody_binding.store_id.clone(),
        object_id: preflight.custody_binding.object_id.clone(),
        object_version: preflight.custody_binding.object_version,
        evidence_kind: preflight.custody_binding.evidence_kind.clone(),
        evidence_revision: preflight.custody_binding.evidence_revision,
        writer_identity: preflight.custody_binding.writer.identity.clone(),
        writer_scope: preflight.custody_binding.writer.scope.clone(),
        reader_identity: preflight.custody_binding.reader.identity.clone(),
        reader_scope: preflight.custody_binding.reader.scope.clone(),
        object_ref: attachment(&object_ref_jcs, &object_ref.schema),
        evidence_ref: attachment(&evidence_ref_jcs, &evidence_ref.schema),
        write_outcome: write_outcome.to_owned(),
        readback_result: "verified".to_owned(),
        readback_at: readback_at.to_owned(),
        receipt_digest: String::new(),
    };
    receipt.receipt_digest = receipt_digest(&receipt)?;
    let receipt_jcs = jcs(&receipt, S6DossierCustodyError::InvalidReceipt)?;
    verify_s6_dossier_readback_receipt(
        &receipt_jcs,
        &preflight,
        &object_ref_jcs,
        &evidence_ref_jcs,
    )?;
    Ok(S6DossierRetentionResultV1 {
        preflight,
        receipt,
        receipt_jcs,
        object_ref_jcs,
        evidence_ref_jcs,
    })
}

/// Stream-verify the deterministic corpus and capture only the small,
/// subject-named custody-binding member.  Other members are hashed in bounded
/// chunks and are never materialised under their logical names.
pub fn verify_s6_dossier_corpus(
    reader: &mut dyn Read,
    custody_binding_ref: &S6DossierMemberRefV1,
) -> Result<VerifiedS6DossierCorpusV1, S6DossierCustodyError> {
    validate_member_ref(custody_binding_ref).map_err(|_| S6DossierCustodyError::InvalidSubject)?;
    let mut envelope_hasher = Sha256::new();
    let mut corpus_size = 0_u64;
    let mut magic = [0_u8; 8];
    read_exact_hashed(reader, &mut magic, &mut envelope_hasher, &mut corpus_size)?;
    if &magic != ENVELOPE_MAGIC {
        return Err(S6DossierCustodyError::InvalidEnvelope);
    }
    let mut version = [0_u8; 1];
    read_exact_hashed(reader, &mut version, &mut envelope_hasher, &mut corpus_size)?;
    if version[0] != ENVELOPE_VERSION {
        return Err(S6DossierCustodyError::InvalidEnvelope);
    }
    let mut manifest_length = [0_u8; 8];
    read_exact_hashed(
        reader,
        &mut manifest_length,
        &mut envelope_hasher,
        &mut corpus_size,
    )?;
    let manifest_length = u64::from_be_bytes(manifest_length);
    if !(2..=MAX_MANIFEST_BYTES).contains(&manifest_length) {
        return Err(S6DossierCustodyError::InvalidEnvelope);
    }
    let manifest_length_usize =
        usize::try_from(manifest_length).map_err(|_| S6DossierCustodyError::InvalidEnvelope)?;
    let mut manifest_raw = vec![0_u8; manifest_length_usize];
    read_exact_hashed(
        reader,
        &mut manifest_raw,
        &mut envelope_hasher,
        &mut corpus_size,
    )?;
    let manifest: S6DossierCorpusManifestV1 =
        strict_jcs(&manifest_raw, S6DossierCustodyError::InvalidManifest)?;
    validate_manifest(&manifest)?;
    let manifest_sha256 = raw_digest(&manifest_raw);
    let mut binding_raw = None;
    for member in &manifest.members {
        let member_size =
            parse_size(&member.size).map_err(|_| S6DossierCustodyError::InvalidManifest)?;
        let capture = member_matches_ref(member, custody_binding_ref);
        if capture && member_size > MAX_CUSTODY_BINDING_BYTES {
            return Err(S6DossierCustodyError::InvalidCustodyBinding);
        }
        let (member_sha256, captured) = stream_member(
            reader,
            member_size,
            &mut envelope_hasher,
            &mut corpus_size,
            capture,
        )?;
        if format!("sha256:{member_sha256}") != member.sha256 {
            return Err(S6DossierCustodyError::InvalidEnvelope);
        }
        if capture
            && (member.media_type != "application/json"
                || binding_raw.replace(captured.unwrap_or_default()).is_some())
        {
            return Err(S6DossierCustodyError::InvalidCustodyBinding);
        }
    }
    let mut extra = [0_u8; 1];
    match reader.read(&mut extra) {
        Ok(0) => {}
        Ok(_) => return Err(S6DossierCustodyError::ExtraEnvelopeBytes),
        Err(_) => return Err(S6DossierCustodyError::TruncatedEnvelope),
    }
    let custody_binding_raw = binding_raw.ok_or(S6DossierCustodyError::InvalidCustodyBinding)?;
    Ok(VerifiedS6DossierCorpusV1 {
        manifest,
        manifest_sha256,
        corpus_sha256: format!("sha256:{:x}", envelope_hasher.finalize()),
        corpus_size,
        custody_binding_raw,
    })
}

/// Verify a receipt and its raw reference attachments without trusting a
/// caller-supplied deserialised reference or a provider response summary.
pub fn verify_s6_dossier_readback_receipt(
    receipt_jcs: &[u8],
    preflight: &S6DossierCustodyPreflightV1,
    object_ref_jcs: &[u8],
    evidence_ref_jcs: &[u8],
) -> Result<S6DossierReadbackReceiptV1, S6DossierCustodyError> {
    let receipt: S6DossierReadbackReceiptV1 =
        strict_jcs(receipt_jcs, S6DossierCustodyError::InvalidReceipt)?;
    if receipt.schema != S6_DOSSIER_READBACK_RECEIPT_V1_SCHEMA
        || receipt.train_id != preflight.subject.train_id
        || receipt.dossier_subject_sha256 != preflight.dossier_subject_sha256
        || receipt.corpus_sha256 != preflight.corpus.corpus_sha256
        || receipt.corpus_size != preflight.corpus.corpus_size.to_string()
        || receipt.manifest_sha256 != preflight.corpus.manifest_sha256
        || receipt.members != preflight.corpus.manifest.members
        || receipt.authority_scope != preflight.custody_binding.authority_scope
        || receipt.store_id != preflight.custody_binding.store_id
        || receipt.object_id != preflight.custody_binding.object_id
        || receipt.object_version != preflight.custody_binding.object_version
        || receipt.evidence_kind != preflight.custody_binding.evidence_kind
        || receipt.evidence_revision != preflight.custody_binding.evidence_revision
        || receipt.writer_identity != preflight.custody_binding.writer.identity
        || receipt.writer_scope != preflight.custody_binding.writer.scope
        || receipt.reader_identity != preflight.custody_binding.reader.identity
        || receipt.reader_scope != preflight.custody_binding.reader.scope
        || receipt.writer_identity == receipt.reader_identity
        || !matches!(receipt.write_outcome.as_str(), "created" | "existing-equal")
        || receipt.readback_result != "verified"
        || !receipt.readback_at.ends_with('Z')
        || DateTime::parse_from_rfc3339(&receipt.readback_at).is_err()
        || receipt.receipt_digest != receipt_digest(&receipt)?
    {
        return Err(S6DossierCustodyError::InvalidReceipt);
    }
    let object_ref: ObjectRefV1 =
        strict_jcs(object_ref_jcs, S6DossierCustodyError::InvalidReceipt)?;
    let evidence_ref: EvidenceRefV1 =
        strict_jcs(evidence_ref_jcs, S6DossierCustodyError::InvalidReceipt)?;
    object_ref
        .validate()
        .map_err(|_| S6DossierCustodyError::InvalidReceipt)?;
    evidence_ref
        .validate()
        .map_err(|_| S6DossierCustodyError::InvalidReceipt)?;
    if receipt.object_ref != attachment(object_ref_jcs, &object_ref.schema)
        || receipt.evidence_ref != attachment(evidence_ref_jcs, &evidence_ref.schema)
        || evidence_ref.object_ref != object_ref
        || evidence_ref.evidence_kind != "jenkins.dossier"
        || evidence_ref.evidence_revision != preflight.custody_binding.evidence_revision
        || evidence_ref.subject_digest.algorithm != "sha256"
        || evidence_ref.subject_digest.value
            != preflight
                .dossier_subject_sha256
                .trim_start_matches("sha256:")
        || object_ref.authority_scope != preflight.custody_binding.authority_scope
        || object_ref.store_id != preflight.custody_binding.store_id
        || object_ref.object_id != preflight.custody_binding.object_id
        || object_ref.object_version != preflight.custody_binding.object_version
        || object_ref.content_digest.algorithm != "sha256"
        || object_ref.content_digest.value
            != preflight.corpus.corpus_sha256.trim_start_matches("sha256:")
        || object_ref.size_bytes != preflight.corpus.corpus_size
    {
        return Err(S6DossierCustodyError::InvalidReceipt);
    }
    Ok(receipt)
}

fn validate_subject_shape(subject: &S6DossierSubjectV1) -> Result<(), S6DossierCustodyError> {
    if subject.schema != S6_DOSSIER_SUBJECT_V1_SCHEMA
        || !valid_identifier(&subject.train_id)
        || subject.release.profile_id != S6_DOSSIER_PROFILE_ID
        || subject.release.selected_product_ids != ["dasobjectstore"]
        || subject.release.package_format != "deb"
        || subject.release.architecture != "amd64"
        || !valid_identifier(&subject.release.signing_authority.authority_id)
        || subject.s5.packages.len() != 1
        || subject.s2.s0_manifest != subject.s0.release_input
    {
        return Err(S6DossierCustodyError::InvalidSubject);
    }
    validate_manifest(&S6DossierCorpusManifestV1 {
        schema: S6_DOSSIER_MANIFEST_V1_SCHEMA.to_owned(),
        members: subject.corpus.members.clone(),
    })?;
    validate_digest(&subject.corpus.manifest_sha256)
        .map_err(|_| S6DossierCustodyError::InvalidSubject)?;
    let package = &subject.s5.packages[0];
    if package.component_id != "dasobjectstore"
        || package.package_name != "dasobjectstore"
        || package.package_version != S6_DOSSIER_PACKAGE_VERSION
        || package.architecture != "amd64"
        || !matches!(
            package.build_action.as_str(),
            "source-rebuild" | "metadata-reattest" | "reuse"
        )
        || !is_git_revision(&package.source_revision)
        || validate_digest(&package.source_tree_sha256).is_err()
        || !matches!(
            package.continuity.kind.as_str(),
            "signed-predecessor" | "source-fallback"
        )
        || (package.continuity.kind == "signed-predecessor"
            && package.continuity.fallback_reason.is_some())
        || (package.continuity.kind == "source-fallback"
            && package.continuity.fallback_reason.is_none())
    {
        return Err(S6DossierCustodyError::InvalidSubject);
    }
    for reference in subject_member_refs(subject) {
        validate_member_ref(reference).map_err(|_| S6DossierCustodyError::InvalidSubject)?;
        if subject
            .corpus
            .members
            .iter()
            .filter(|member| member_matches_ref(member, reference))
            .count()
            != 1
        {
            return Err(S6DossierCustodyError::InvalidSubject);
        }
    }
    Ok(())
}

fn subject_member_refs(subject: &S6DossierSubjectV1) -> Vec<&S6DossierMemberRefV1> {
    let s2 = &subject.s2;
    let s3 = &subject.s3;
    let s4 = &subject.s4;
    let package = &subject.s5.packages[0];
    let mut references = vec![
        &subject.release.signing_authority.authority_record,
        &subject.release.signing_authority.public_key_pem,
        &subject.s0.release_input,
        &subject.s1.source_registry_review,
        &s2.certificate,
        &s2.sealed_plan,
        &s2.preflight,
        &s2.s0_manifest,
        &s2.predecessor_lock,
        &s2.successor_lock,
        &s2.products_registry,
        &s2.source_dependencies_registry,
        &s2.release_control_authorities_registry,
        &s2.predecessor_catalogue,
        &s2.successor_catalogue,
        &s2.successor_sources_lock,
        &s2.package_plan,
        &s3.acceptance_attestation,
        &s3.accepted_lock,
        &s3.canonical_main_witness,
        &s3.authority_pem,
        &s4.projection,
        &s4.catalogue,
        &s4.profile,
        &s4.source_lock,
        &s4.acceptance_receipt,
        &subject.s5.build_receipt,
        &package.package,
        &package.sbom,
        &package.provenance,
        &package.cargo_lock,
        &package.continuity.record,
        &subject.custody_binding,
    ];
    if let Some(fallback_reason) = &package.continuity.fallback_reason {
        references.push(fallback_reason);
    }
    references
}

fn validate_binding(
    binding: &S6DossierCustodyBindingV1,
    subject: &S6DossierSubjectV1,
    corpus: &VerifiedS6DossierCorpusV1,
) -> Result<(), S6DossierCustodyError> {
    if binding.schema != S6_DOSSIER_CUSTODY_BINDING_V1_SCHEMA
        || binding.train_id != subject.train_id
        || binding.evidence_kind != "jenkins.dossier"
        || binding.object_version == 0
        || binding.evidence_revision == 0
        || binding.writer.scope != S6_DOSSIER_WRITER_SCOPE
        || binding.reader.scope != S6_DOSSIER_READER_SCOPE
        || binding.writer.identity == binding.reader.identity
        || !valid_identifier(&binding.writer.identity)
        || !valid_identifier(&binding.reader.identity)
    {
        return Err(S6DossierCustodyError::InvalidCustodyBinding);
    }
    let mut reference = ObjectRefV1 {
        schema: "dasobjectstore.object_ref.v1".to_owned(),
        authority_scope: binding.authority_scope.clone(),
        store_id: binding.store_id.clone(),
        object_id: binding.object_id.clone(),
        object_version: binding.object_version,
        size_bytes: corpus.corpus_size,
        content_digest: DigestV1 {
            algorithm: "sha256".to_owned(),
            value: corpus
                .corpus_sha256
                .trim_start_matches("sha256:")
                .to_owned(),
        },
        domain_digest: DigestV1 {
            algorithm: "sha256".to_owned(),
            value: String::new(),
        },
    };
    reference.domain_digest.value = reference.expected_domain_digest();
    reference
        .validate()
        .map_err(|_| S6DossierCustodyError::InvalidCustodyBinding)
}

fn validate_peer_channels(
    writer: &S6DossierPeerChannelV1,
    reader: &S6DossierPeerChannelV1,
    binding: &S6DossierCustodyBindingV1,
    dossier_subject_sha256: &str,
) -> Result<(), S6DossierCustodyError> {
    validate_grant(
        &writer.grant,
        S6_DOSSIER_WRITE_CAPABILITY,
        &binding.writer.identity,
        binding,
        dossier_subject_sha256,
    )?;
    validate_grant(
        &reader.grant,
        S6_DOSSIER_READ_CAPABILITY,
        &binding.reader.identity,
        binding,
        dossier_subject_sha256,
    )?;
    for (writer_value, reader_value) in [
        (&writer.grant.peer_identity, &reader.grant.peer_identity),
        (&writer.grant.session_id, &reader.grant.session_id),
        (&writer.grant.principal_id, &reader.grant.principal_id),
        (
            &writer.grant.entitlement_assignment_id,
            &reader.grant.entitlement_assignment_id,
        ),
        (&writer.credential_binding_id, &reader.credential_binding_id),
        (&writer.process_instance_id, &reader.process_instance_id),
        (&writer.cache_instance_id, &reader.cache_instance_id),
        (&writer.upload_handle_id, &reader.upload_handle_id),
        (&writer.staging_path_id, &reader.staging_path_id),
    ] {
        if writer_value == reader_value {
            return Err(S6DossierCustodyError::SharedPeerChannel);
        }
    }
    for value in [
        &writer.credential_binding_id,
        &writer.process_instance_id,
        &writer.cache_instance_id,
        &writer.upload_handle_id,
        &writer.staging_path_id,
        &reader.credential_binding_id,
        &reader.process_instance_id,
        &reader.cache_instance_id,
        &reader.upload_handle_id,
        &reader.staging_path_id,
    ] {
        if !valid_identifier(value) {
            return Err(S6DossierCustodyError::InvalidFixedPeerGrant);
        }
    }
    Ok(())
}

fn validate_grant(
    grant: &S6DossierFixedPeerGrantV1,
    expected_capability: &str,
    expected_identity: &str,
    binding: &S6DossierCustodyBindingV1,
    dossier_subject_sha256: &str,
) -> Result<(), S6DossierCustodyError> {
    if grant.schema != S6_DOSSIER_FIXED_PEER_GRANT_V1_SCHEMA
        || grant.peer_identity != expected_identity
        || grant.authority_revision == 0
        || grant.evidence_revision != binding.evidence_revision
        || grant.capability != expected_capability
        || grant.canonical_prefix != S6_DOSSIER_OBJECT_PREFIX
        || grant.dossier_subject_sha256 != dossier_subject_sha256
        || grant.authority_scope != binding.authority_scope
        || !matches!(grant.entitlement.as_str(), "operate" | "administer")
        || !grant.session_expires_at_utc.ends_with('Z')
        || DateTime::parse_from_rfc3339(&grant.session_expires_at_utc).is_err()
    {
        return Err(S6DossierCustodyError::InvalidFixedPeerGrant);
    }
    for value in [
        &grant.peer_identity,
        &grant.authority_id,
        &grant.session_id,
        &grant.principal_id,
        &grant.entitlement_assignment_id,
    ] {
        if !valid_identifier(value) {
            return Err(S6DossierCustodyError::InvalidFixedPeerGrant);
        }
    }
    Ok(())
}

fn validate_manifest(manifest: &S6DossierCorpusManifestV1) -> Result<(), S6DossierCustodyError> {
    if manifest.schema != S6_DOSSIER_MANIFEST_V1_SCHEMA || manifest.members.is_empty() {
        return Err(S6DossierCustodyError::InvalidManifest);
    }
    let mut prior_name = None;
    let mut payloads = BTreeSet::new();
    for member in &manifest.members {
        validate_member(member).map_err(|_| S6DossierCustodyError::InvalidManifest)?;
        if prior_name
            .as_ref()
            .is_some_and(|prior: &String| prior.as_bytes() >= member.logical_name.as_bytes())
            || !payloads.insert((member.size.clone(), member.sha256.clone()))
        {
            return Err(S6DossierCustodyError::InvalidManifest);
        }
        prior_name = Some(member.logical_name.clone());
    }
    Ok(())
}

fn validate_member(member: &S6DossierMemberV1) -> Result<(), ()> {
    validate_logical_name(&member.logical_name)?;
    validate_media_type(&member.media_type)?;
    parse_size(&member.size)?;
    validate_digest(&member.sha256)?;
    Ok(())
}

fn validate_member_ref(reference: &S6DossierMemberRefV1) -> Result<(), ()> {
    validate_logical_name(&reference.logical_name)?;
    parse_size(&reference.size)?;
    validate_digest(&reference.sha256)
}

fn member_matches_ref(member: &S6DossierMemberV1, reference: &S6DossierMemberRefV1) -> bool {
    member.logical_name == reference.logical_name
        && member.sha256 == reference.sha256
        && member.size == reference.size
}

fn validate_logical_name(value: &str) -> Result<(), ()> {
    if value.is_empty()
        || !value.is_ascii()
        || value.starts_with('/')
        || value.contains('\\')
        || value.contains('%')
        || value.contains('\0')
        || value.contains("//")
    {
        return Err(());
    }
    for component in value.split('/') {
        if component.is_empty()
            || matches!(component, "." | "..")
            || component.len() > 128
            || !component.bytes().enumerate().all(|(index, byte)| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || (index != 0 && matches!(byte, b'.' | b'_' | b'-'))
            })
        {
            return Err(());
        }
    }
    Ok(())
}

fn validate_media_type(value: &str) -> Result<(), ()> {
    let Some((major, minor)) = value.split_once('/') else {
        return Err(());
    };
    if major.is_empty() || minor.is_empty() || minor.contains('/') {
        return Err(());
    }
    for part in [major, minor] {
        if !part.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(
                    byte,
                    b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-'
                )
        }) {
            return Err(());
        }
    }
    Ok(())
}

fn parse_size(value: &str) -> Result<u64, ()> {
    if value.is_empty()
        || (!matches!(value, "0") && value.starts_with('0'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(());
    }
    value.parse().map_err(|_| ())
}

fn validate_digest(value: &str) -> Result<(), ()> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(());
    };
    if hex.len() == 64
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(())
    }
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.is_ascii()
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || (index != 0 && matches!(byte, b'.' | b'_' | b'-'))
        })
}

fn is_git_revision(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn strict_jcs<T>(raw: &[u8], error: S6DossierCustodyError) -> Result<T, S6DossierCustodyError>
where
    T: DeserializeOwned,
{
    let value: serde_json::Value = serde_json::from_slice(raw).map_err(|_| error.clone())?;
    let canonical = serde_jcs::to_vec(&value).map_err(|_| error.clone())?;
    if canonical != raw {
        return Err(error);
    }
    serde_json::from_value(value).map_err(|_| error)
}

fn jcs<T: Serialize>(
    value: &T,
    error: S6DossierCustodyError,
) -> Result<Vec<u8>, S6DossierCustodyError> {
    serde_jcs::to_vec(value).map_err(|_| error)
}

fn read_exact_hashed(
    reader: &mut dyn Read,
    bytes: &mut [u8],
    envelope_hasher: &mut Sha256,
    corpus_size: &mut u64,
) -> Result<(), S6DossierCustodyError> {
    reader
        .read_exact(bytes)
        .map_err(|_| S6DossierCustodyError::TruncatedEnvelope)?;
    envelope_hasher.update(&*bytes);
    *corpus_size = corpus_size
        .checked_add(bytes.len() as u64)
        .ok_or(S6DossierCustodyError::InvalidEnvelope)?;
    Ok(())
}

fn stream_member(
    reader: &mut dyn Read,
    size: u64,
    envelope_hasher: &mut Sha256,
    corpus_size: &mut u64,
    capture: bool,
) -> Result<(String, Option<Vec<u8>>), S6DossierCustodyError> {
    let mut remaining = size;
    let mut member_hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut captured = if capture {
        Some(Vec::with_capacity(
            usize::try_from(size).map_err(|_| S6DossierCustodyError::InvalidCustodyBinding)?,
        ))
    } else {
        None
    };
    while remaining > 0 {
        let requested = usize::try_from(remaining.min(buffer.len() as u64))
            .map_err(|_| S6DossierCustodyError::InvalidEnvelope)?;
        let read = reader
            .read(&mut buffer[..requested])
            .map_err(|_| S6DossierCustodyError::TruncatedEnvelope)?;
        if read == 0 {
            return Err(S6DossierCustodyError::TruncatedEnvelope);
        }
        envelope_hasher.update(&buffer[..read]);
        member_hasher.update(&buffer[..read]);
        if let Some(captured) = &mut captured {
            captured.extend_from_slice(&buffer[..read]);
        }
        *corpus_size = corpus_size
            .checked_add(read as u64)
            .ok_or(S6DossierCustodyError::InvalidEnvelope)?;
        remaining -= read as u64;
    }
    Ok((format!("{:x}", member_hasher.finalize()), captured))
}

fn canonical_storage_key(
    train_id: &str,
    corpus_sha256: &str,
) -> Result<String, S6DossierCustodyError> {
    if !valid_identifier(train_id) {
        return Err(S6DossierCustodyError::InvalidSubject);
    }
    validate_digest(corpus_sha256).map_err(|_| S6DossierCustodyError::InvalidEnvelope)?;
    Ok(format!(
        "{S6_DOSSIER_OBJECT_PREFIX}/{train_id}/{}",
        corpus_sha256.trim_start_matches("sha256:")
    ))
}

fn raw_digest(raw: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(raw))
}

fn domain_digest(prefix: &[u8], raw: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prefix);
    hasher.update(raw);
    format!("sha256:{:x}", hasher.finalize())
}

fn project_references(
    preflight: &S6DossierCustodyPreflightV1,
) -> Result<(ObjectRefV1, EvidenceRefV1, Vec<u8>, Vec<u8>), S6DossierCustodyError> {
    let evidence = JenkinsDossierEvidenceProjectionV1 {
        schema: JENKINS_DOSSIER_EVIDENCE_PROJECTION_V1_SCHEMA.to_owned(),
        authority_scope: preflight.custody_binding.authority_scope.clone(),
        store_id: preflight.custody_binding.store_id.clone(),
        object_id: preflight.custody_binding.object_id.clone(),
        object_version: preflight.custody_binding.object_version,
        size_bytes: preflight.corpus.corpus_size,
        content_sha256: preflight
            .corpus
            .corpus_sha256
            .trim_start_matches("sha256:")
            .to_owned(),
        dossier_digest: preflight.dossier_subject_sha256.clone(),
        evidence_revision: preflight.custody_binding.evidence_revision,
    }
    .project()
    .map_err(|_| S6DossierCustodyError::InvalidCustodyBinding)?;
    let object = evidence.object_ref.clone();
    let object_jcs = jcs(&object, S6DossierCustodyError::InvalidReceipt)?;
    let evidence_jcs = jcs(&evidence, S6DossierCustodyError::InvalidReceipt)?;
    Ok((object, evidence, object_jcs, evidence_jcs))
}

fn attachment(raw: &[u8], schema: &str) -> S6DossierRawAttachmentV1 {
    S6DossierRawAttachmentV1 {
        schema: schema.to_owned(),
        sha256: raw_digest(raw),
        size: raw.len().to_string(),
    }
}

fn receipt_digest(receipt: &S6DossierReadbackReceiptV1) -> Result<String, S6DossierCustodyError> {
    let mut value =
        serde_json::to_value(receipt).map_err(|_| S6DossierCustodyError::InvalidReceipt)?;
    value
        .as_object_mut()
        .ok_or(S6DossierCustodyError::InvalidReceipt)?
        .remove("receipt_digest");
    Ok(raw_digest(&jcs(
        &value,
        S6DossierCustodyError::InvalidReceipt,
    )?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::io::Cursor;

    const TRAIN: &str = "das-0180-nuc";

    #[derive(Default)]
    struct MemoryWriter {
        store_id: String,
        peer: String,
        objects: BTreeMap<String, Vec<u8>>,
        writes: usize,
        force_conflict: bool,
    }

    impl S6DossierWriterPortV1 for MemoryWriter {
        fn store_id(&self) -> &str {
            &self.store_id
        }

        fn peer_identity(&self) -> &str {
            &self.peer
        }

        fn create_if_absent(
            &mut self,
            storage_key: &str,
            corpus: &[u8],
        ) -> Result<S6DossierCreateOutcomeV1, S6DossierCustodyError> {
            self.writes += 1;
            if self.force_conflict {
                return Ok(S6DossierCreateOutcomeV1::Conflict);
            }
            match self.objects.get(storage_key) {
                Some(existing) if existing == corpus => Ok(S6DossierCreateOutcomeV1::AlreadyExists),
                Some(_) => Ok(S6DossierCreateOutcomeV1::Conflict),
                None => {
                    self.objects.insert(storage_key.to_owned(), corpus.to_vec());
                    Ok(S6DossierCreateOutcomeV1::Created)
                }
            }
        }
    }

    struct MemoryReader {
        store_id: String,
        peer: String,
        objects: BTreeMap<String, Vec<u8>>,
    }

    impl S6DossierReaderPortV1 for MemoryReader {
        fn store_id(&self) -> &str {
            &self.store_id
        }

        fn peer_identity(&self) -> &str {
            &self.peer
        }

        fn read_independently(
            &mut self,
            storage_key: &str,
        ) -> Result<Box<dyn Read>, S6DossierCustodyError> {
            let bytes = self
                .objects
                .get(storage_key)
                .cloned()
                .ok_or(S6DossierCustodyError::ReadbackUnavailable)?;
            Ok(Box::new(Cursor::new(bytes)))
        }
    }

    fn digest(bytes: &[u8]) -> String {
        raw_digest(bytes)
    }

    fn member(name: &str, media_type: &str, bytes: &[u8]) -> S6DossierMemberV1 {
        S6DossierMemberV1 {
            logical_name: name.to_owned(),
            media_type: media_type.to_owned(),
            size: bytes.len().to_string(),
            sha256: digest(bytes),
        }
    }

    fn reference(member: &S6DossierMemberV1) -> S6DossierMemberRefV1 {
        S6DossierMemberRefV1 {
            logical_name: member.logical_name.clone(),
            sha256: member.sha256.clone(),
            size: member.size.clone(),
        }
    }

    fn scope() -> AuthorityScopeV1 {
        AuthorityScopeV1 {
            installation_id: "nuc-193".to_owned(),
            site_trust_domain_id: Some("mnemosyne".to_owned()),
            tenant_id: None,
            project_id: Some("das".to_owned()),
        }
    }

    fn binding() -> S6DossierCustodyBindingV1 {
        S6DossierCustodyBindingV1 {
            schema: S6_DOSSIER_CUSTODY_BINDING_V1_SCHEMA.to_owned(),
            train_id: TRAIN.to_owned(),
            authority_scope: scope(),
            store_id: "dossiers".to_owned(),
            object_id: "das0180s6".to_owned(),
            object_version: 1,
            evidence_kind: "jenkins.dossier".to_owned(),
            evidence_revision: 1,
            writer: S6DossierPeerIdentityV1 {
                identity: "s6-writer".to_owned(),
                scope: S6_DOSSIER_WRITER_SCOPE.to_owned(),
            },
            reader: S6DossierPeerIdentityV1 {
                identity: "s6-reader".to_owned(),
                scope: S6_DOSSIER_READER_SCOPE.to_owned(),
            },
        }
    }

    fn records(binding_raw: &[u8]) -> Vec<(String, String, Vec<u8>)> {
        let mut records = vec![
            (
                "a-authority.json".to_owned(),
                "application/json".to_owned(),
                b"{}".to_vec(),
            ),
            (
                "b-pem.json".to_owned(),
                "application/json".to_owned(),
                b"{}".to_vec(),
            ),
            (
                "c-release.json".to_owned(),
                "application/json".to_owned(),
                b"{}".to_vec(),
            ),
            (
                "d-review.json".to_owned(),
                "application/json".to_owned(),
                b"{}".to_vec(),
            ),
            (
                "e-s2-certificate.json".to_owned(),
                "application/json".to_owned(),
                b"{}".to_vec(),
            ),
            (
                "f-s2-plan.json".to_owned(),
                "application/json".to_owned(),
                b"{}".to_vec(),
            ),
            (
                "g-s2-preflight.json".to_owned(),
                "application/json".to_owned(),
                b"{}".to_vec(),
            ),
            (
                "h-s2-predecessor-lock.json".to_owned(),
                "application/json".to_owned(),
                b"{}".to_vec(),
            ),
            (
                "i-s2-successor-lock.json".to_owned(),
                "application/json".to_owned(),
                b"{}".to_vec(),
            ),
            (
                "j-s2-products.json".to_owned(),
                "application/json".to_owned(),
                b"{}".to_vec(),
            ),
            (
                "k-s2-dependencies.json".to_owned(),
                "application/json".to_owned(),
                b"{}".to_vec(),
            ),
            (
                "l-s2-authorities.json".to_owned(),
                "application/json".to_owned(),
                b"{}".to_vec(),
            ),
            (
                "m-s2-predecessor-catalogue.json".to_owned(),
                "application/json".to_owned(),
                b"{}".to_vec(),
            ),
            (
                "n-s2-successor-catalogue.json".to_owned(),
                "application/json".to_owned(),
                b"{}".to_vec(),
            ),
            (
                "o-s2-sources-lock.json".to_owned(),
                "application/json".to_owned(),
                b"{}".to_vec(),
            ),
            (
                "p-s2-package-plan.json".to_owned(),
                "application/json".to_owned(),
                b"{}".to_vec(),
            ),
            (
                "q-s3-attestation.json".to_owned(),
                "application/json".to_owned(),
                b"{}".to_vec(),
            ),
            (
                "r-s3-lock.json".to_owned(),
                "application/json".to_owned(),
                b"{}".to_vec(),
            ),
            (
                "s-s3-main.json".to_owned(),
                "application/json".to_owned(),
                b"{}".to_vec(),
            ),
            (
                "t-s4-projection.json".to_owned(),
                "application/json".to_owned(),
                b"{}".to_vec(),
            ),
            (
                "u-s4-catalogue.json".to_owned(),
                "application/json".to_owned(),
                b"{}".to_vec(),
            ),
            (
                "v-s4-profile.json".to_owned(),
                "application/json".to_owned(),
                b"{}".to_vec(),
            ),
            (
                "w-s4-sources-lock.json".to_owned(),
                "application/json".to_owned(),
                b"{}".to_vec(),
            ),
            (
                "x-s4-acceptance.json".to_owned(),
                "application/json".to_owned(),
                b"{}".to_vec(),
            ),
            (
                "y-s5-build.json".to_owned(),
                "application/json".to_owned(),
                b"{}".to_vec(),
            ),
            (
                "z-custody-binding.json".to_owned(),
                "application/json".to_owned(),
                binding_raw.to_vec(),
            ),
            (
                "za-package.deb".to_owned(),
                "application/octet-stream".to_owned(),
                b"package".to_vec(),
            ),
            (
                "zb-sbom.json".to_owned(),
                "application/json".to_owned(),
                b"{}".to_vec(),
            ),
            (
                "zc-provenance.json".to_owned(),
                "application/json".to_owned(),
                b"{}".to_vec(),
            ),
            (
                "zd-cargo.lock".to_owned(),
                "text/plain".to_owned(),
                b"lock".to_vec(),
            ),
            (
                "ze-continuity.json".to_owned(),
                "application/json".to_owned(),
                b"{}".to_vec(),
            ),
        ];
        // The corpus contract forbids duplicate `(size, sha256)` rows, even
        // where records happen to share a media type.  Give every fixture
        // record distinct raw bytes rather than testing a weaker inventory.
        for (name, _, bytes) in &mut records {
            if bytes.as_slice() == b"{}" {
                *bytes = format!(r#"{{"record":"{name}"}}"#).into_bytes();
            }
        }
        records.sort_by(|left, right| left.0.cmp(&right.0));
        records
    }

    fn fixture() -> (
        Vec<u8>,
        Vec<u8>,
        S6DossierPeerChannelV1,
        S6DossierPeerChannelV1,
    ) {
        let binding_raw = jcs(&binding(), S6DossierCustodyError::InvalidCustodyBinding).unwrap();
        let records = records(&binding_raw);
        let members: Vec<_> = records
            .iter()
            .map(|(name, media, bytes)| member(name, media, bytes))
            .collect();
        let refs: BTreeMap<_, _> = members
            .iter()
            .map(|entry| (entry.logical_name.clone(), reference(entry)))
            .collect();
        let get = |name: &str| refs.get(name).unwrap().clone();
        let package = S6DossierPackageV1 {
            component_id: "dasobjectstore".to_owned(),
            package_name: "dasobjectstore".to_owned(),
            package_version: S6_DOSSIER_PACKAGE_VERSION.to_owned(),
            architecture: "amd64".to_owned(),
            package: get("za-package.deb"),
            sbom: get("zb-sbom.json"),
            provenance: get("zc-provenance.json"),
            source_revision: "a".repeat(40),
            source_tree_sha256: format!("sha256:{}", "b".repeat(64)),
            cargo_lock: get("zd-cargo.lock"),
            build_action: "source-rebuild".to_owned(),
            continuity: S6DossierContinuityV1 {
                kind: "signed-predecessor".to_owned(),
                record: get("ze-continuity.json"),
                fallback_reason: None,
            },
        };
        let manifest = S6DossierCorpusManifestV1 {
            schema: S6_DOSSIER_MANIFEST_V1_SCHEMA.to_owned(),
            members: members.clone(),
        };
        let manifest_raw = jcs(&manifest, S6DossierCustodyError::InvalidManifest).unwrap();
        let subject = S6DossierSubjectV1 {
            schema: S6_DOSSIER_SUBJECT_V1_SCHEMA.to_owned(),
            train_id: TRAIN.to_owned(),
            release: S6DossierReleaseV1 {
                profile_id: S6_DOSSIER_PROFILE_ID.to_owned(),
                selected_product_ids: vec!["dasobjectstore".to_owned()],
                package_format: "deb".to_owned(),
                architecture: "amd64".to_owned(),
                signing_authority: S6DossierSigningAuthorityV1 {
                    authority_id: "s6-authority".to_owned(),
                    authority_record: get("a-authority.json"),
                    public_key_pem: get("b-pem.json"),
                },
            },
            s0: S6DossierS0V1 {
                release_input: get("c-release.json"),
            },
            s1: S6DossierS1V1 {
                source_registry_review: get("d-review.json"),
            },
            s2: S6DossierS2V1 {
                certificate: get("e-s2-certificate.json"),
                sealed_plan: get("f-s2-plan.json"),
                preflight: get("g-s2-preflight.json"),
                s0_manifest: get("c-release.json"),
                predecessor_lock: get("h-s2-predecessor-lock.json"),
                successor_lock: get("i-s2-successor-lock.json"),
                products_registry: get("j-s2-products.json"),
                source_dependencies_registry: get("k-s2-dependencies.json"),
                release_control_authorities_registry: get("l-s2-authorities.json"),
                predecessor_catalogue: get("m-s2-predecessor-catalogue.json"),
                successor_catalogue: get("n-s2-successor-catalogue.json"),
                successor_sources_lock: get("o-s2-sources-lock.json"),
                package_plan: get("p-s2-package-plan.json"),
            },
            s3: S6DossierS3V1 {
                acceptance_attestation: get("q-s3-attestation.json"),
                accepted_lock: get("r-s3-lock.json"),
                canonical_main_witness: get("s-s3-main.json"),
                authority_pem: get("b-pem.json"),
            },
            s4: S6DossierS4V1 {
                projection: get("t-s4-projection.json"),
                catalogue: get("u-s4-catalogue.json"),
                profile: get("v-s4-profile.json"),
                source_lock: get("w-s4-sources-lock.json"),
                acceptance_receipt: get("x-s4-acceptance.json"),
            },
            s5: S6DossierS5V1 {
                build_receipt: get("y-s5-build.json"),
                packages: vec![package],
            },
            custody_binding: get("z-custody-binding.json"),
            corpus: S6DossierSubjectCorpusV1 {
                manifest_sha256: digest(&manifest_raw),
                members,
            },
        };
        let subject_raw = jcs(&subject, S6DossierCustodyError::InvalidSubject).unwrap();
        let mut corpus = Vec::new();
        corpus.extend_from_slice(ENVELOPE_MAGIC);
        corpus.push(ENVELOPE_VERSION);
        corpus.extend_from_slice(&(manifest_raw.len() as u64).to_be_bytes());
        corpus.extend_from_slice(&manifest_raw);
        for (_, _, bytes) in records {
            corpus.extend_from_slice(&bytes);
        }
        let subject_digest = domain_digest(SUBJECT_DOMAIN_PREFIX, &subject_raw);
        let channel = |identity: &str, capability: &str, serial: &str| S6DossierPeerChannelV1 {
            grant: S6DossierFixedPeerGrantV1 {
                schema: S6_DOSSIER_FIXED_PEER_GRANT_V1_SCHEMA.to_owned(),
                peer_identity: identity.to_owned(),
                authority_id: "pistis-authority".to_owned(),
                authority_revision: 1,
                session_id: format!("session-{serial}"),
                principal_id: format!("principal-{serial}"),
                entitlement_assignment_id: format!("assignment-{serial}"),
                entitlement: "operate".to_owned(),
                session_expires_at_utc: "2030-01-01T00:00:00Z".to_owned(),
                capability: capability.to_owned(),
                canonical_prefix: S6_DOSSIER_OBJECT_PREFIX.to_owned(),
                dossier_subject_sha256: subject_digest.clone(),
                evidence_revision: 1,
                authority_scope: scope(),
            },
            credential_binding_id: format!("credential-{serial}"),
            process_instance_id: format!("process-{serial}"),
            cache_instance_id: format!("cache-{serial}"),
            upload_handle_id: format!("upload-{serial}"),
            staging_path_id: format!("staging-{serial}"),
        };
        (
            subject_raw,
            corpus,
            channel("s6-writer", S6_DOSSIER_WRITE_CAPABILITY, "writer"),
            channel("s6-reader", S6_DOSSIER_READ_CAPABILITY, "reader"),
        )
    }

    #[test]
    fn preflight_streams_the_exact_scoped_0180_corpus_without_side_effects() {
        let (subject, corpus, writer, reader) = fixture();
        let preflight = preflight_s6_dossier_custody(&subject, &corpus, &writer, &reader).unwrap();
        assert_eq!(preflight.subject.release.profile_id, S6_DOSSIER_PROFILE_ID);
        assert_eq!(
            preflight.subject.release.selected_product_ids,
            ["dasobjectstore"]
        );
        assert_eq!(
            preflight.storage_key,
            format!(
                "{S6_DOSSIER_OBJECT_PREFIX}/{TRAIN}/{}",
                preflight.corpus.corpus_sha256.trim_start_matches("sha256:")
            )
        );
        assert!(preflight
            .storage_key
            .starts_with("expedition/release-trains/"));
    }

    #[test]
    fn subject_domain_prefix_is_the_published_43_byte_vector() {
        assert_eq!(SUBJECT_DOMAIN_PREFIX.len(), 43);
        assert_eq!(
            domain_digest(SUBJECT_DOMAIN_PREFIX, b"{}"),
            "sha256:38a78dfd3eae9685cfa03b936f8507880640008e45b452760377eda59c36248e"
        );
        assert_eq!(
            domain_digest(b"mnemosyne.expedition.s6-dossier-subject.v1\\0", b"{}"),
            "sha256:52978698f3b848d9f1601c0de2ae14987669fa97dbb666fa1a05f1bbf94fe76d"
        );
    }

    #[test]
    fn malformed_truncated_extra_and_digest_substituted_corpora_fail_before_write() {
        let (subject, corpus, writer_channel, reader_channel) = fixture();
        let mut writer = MemoryWriter {
            store_id: "dossiers".to_owned(),
            peer: "s6-writer".to_owned(),
            ..Default::default()
        };
        let mut reader = MemoryReader {
            store_id: "dossiers".to_owned(),
            peer: "s6-reader".to_owned(),
            objects: BTreeMap::new(),
        };
        let mut cases = Vec::new();
        cases.push(corpus[..16].to_vec());
        let mut extra = corpus.clone();
        extra.push(b'!');
        cases.push(extra);
        let mut substituted = corpus.clone();
        *substituted.last_mut().unwrap() ^= 1;
        cases.push(substituted);
        for invalid in cases {
            assert!(retain_s6_dossier_corpus(
                &subject,
                &invalid,
                &writer_channel,
                &reader_channel,
                "2030-01-01T00:00:00Z",
                &mut writer,
                &mut reader
            )
            .is_err());
            assert_eq!(
                writer.writes, 0,
                "invalid corpus must not open a write path"
            );
        }
    }

    #[test]
    fn immutable_create_equal_replay_and_different_conflict_require_independent_readback() {
        let (subject, corpus, writer_channel, reader_channel) = fixture();
        let mut writer = MemoryWriter {
            store_id: "dossiers".to_owned(),
            peer: "s6-writer".to_owned(),
            ..Default::default()
        };
        let key = preflight_s6_dossier_custody(&subject, &corpus, &writer_channel, &reader_channel)
            .unwrap()
            .storage_key;
        let mut reader = MemoryReader {
            store_id: "dossiers".to_owned(),
            peer: "s6-reader".to_owned(),
            objects: BTreeMap::new(),
        };
        // A real fixed-peer backend makes the created corpus visible only to
        // the independent reader. The test double exposes it through a
        // different reader map, never through the writer buffer.
        reader.objects.insert(key.clone(), corpus.clone());
        let created = retain_s6_dossier_corpus(
            &subject,
            &corpus,
            &writer_channel,
            &reader_channel,
            "2030-01-01T00:00:00Z",
            &mut writer,
            &mut reader,
        )
        .unwrap();
        assert_eq!(created.receipt.write_outcome, "created");
        assert_eq!(writer.writes, 1);
        let replay = retain_s6_dossier_corpus(
            &subject,
            &corpus,
            &writer_channel,
            &reader_channel,
            "2030-01-01T00:00:00Z",
            &mut writer,
            &mut reader,
        )
        .unwrap();
        assert_eq!(replay.receipt.write_outcome, "existing-equal");
        writer.force_conflict = true;
        assert_eq!(
            retain_s6_dossier_corpus(
                &subject,
                &corpus,
                &writer_channel,
                &reader_channel,
                "2030-01-01T00:00:00Z",
                &mut writer,
                &mut reader,
            ),
            Err(S6DossierCustodyError::ImmutableConflict)
        );
    }

    #[test]
    fn shared_identity_credential_process_cache_upload_or_staging_denies() {
        let (subject, corpus, writer, mut reader) = fixture();
        for field in 0..9 {
            let mut candidate = reader.clone();
            match field {
                0 => candidate.grant.peer_identity = writer.grant.peer_identity.clone(),
                1 => candidate.grant.session_id = writer.grant.session_id.clone(),
                2 => candidate.grant.principal_id = writer.grant.principal_id.clone(),
                3 => {
                    candidate.grant.entitlement_assignment_id =
                        writer.grant.entitlement_assignment_id.clone()
                }
                4 => candidate.credential_binding_id = writer.credential_binding_id.clone(),
                5 => candidate.process_instance_id = writer.process_instance_id.clone(),
                6 => candidate.cache_instance_id = writer.cache_instance_id.clone(),
                7 => candidate.upload_handle_id = writer.upload_handle_id.clone(),
                _ => candidate.staging_path_id = writer.staging_path_id.clone(),
            }
            assert!(matches!(
                preflight_s6_dossier_custody(&subject, &corpus, &writer, &candidate),
                Err(S6DossierCustodyError::SharedPeerChannel)
                    | Err(S6DossierCustodyError::InvalidFixedPeerGrant)
            ));
        }
        reader.grant.capability = S6_DOSSIER_WRITE_CAPABILITY.to_owned();
        assert_eq!(
            preflight_s6_dossier_custody(&subject, &corpus, &writer, &reader),
            Err(S6DossierCustodyError::InvalidFixedPeerGrant)
        );
    }

    #[test]
    fn receipt_is_strict_jcs_and_cross_checks_raw_reference_attachments() {
        let (subject, corpus, writer_channel, reader_channel) = fixture();
        let preflight =
            preflight_s6_dossier_custody(&subject, &corpus, &writer_channel, &reader_channel)
                .unwrap();
        let mut writer = MemoryWriter {
            store_id: "dossiers".to_owned(),
            peer: "s6-writer".to_owned(),
            objects: BTreeMap::new(),
            writes: 0,
            force_conflict: false,
        };
        writer
            .objects
            .insert(preflight.storage_key.clone(), corpus.clone());
        let mut reader = MemoryReader {
            store_id: "dossiers".to_owned(),
            peer: "s6-reader".to_owned(),
            objects: BTreeMap::from([(preflight.storage_key.clone(), corpus)]),
        };
        let corpus_for_replay = reader.objects.get(&preflight.storage_key).unwrap().clone();
        let result = retain_s6_dossier_corpus(
            &subject,
            &corpus_for_replay,
            &writer_channel,
            &reader_channel,
            "2030-01-01T00:00:00Z",
            &mut writer,
            &mut reader,
        )
        .unwrap();
        verify_s6_dossier_readback_receipt(
            &result.receipt_jcs,
            &result.preflight,
            &result.object_ref_jcs,
            &result.evidence_ref_jcs,
        )
        .unwrap();
        let mut receipt: serde_json::Value = serde_json::from_slice(&result.receipt_jcs).unwrap();
        receipt["reader_identity"] = serde_json::Value::String("s6-writer".to_owned());
        let tampered = serde_jcs::to_vec(&receipt).unwrap();
        assert_eq!(
            verify_s6_dossier_readback_receipt(
                &tampered,
                &result.preflight,
                &result.object_ref_jcs,
                &result.evidence_ref_jcs
            ),
            Err(S6DossierCustodyError::InvalidReceipt)
        );
    }

    #[test]
    fn manifest_subject_and_binding_substitutions_fail_closed() {
        let (subject, corpus, writer, reader) = fixture();
        let mut non_jcs_subject = subject.clone();
        non_jcs_subject.push(b'\n');
        assert_eq!(
            preflight_s6_dossier_custody(&non_jcs_subject, &corpus, &writer, &reader),
            Err(S6DossierCustodyError::InvalidSubject)
        );
        let mut wrong_profile: serde_json::Value = serde_json::from_slice(&subject).unwrap();
        wrong_profile["release"]["profile_id"] = serde_json::Value::String("r237".to_owned());
        let wrong_profile = serde_jcs::to_vec(&wrong_profile).unwrap();
        assert_eq!(
            preflight_s6_dossier_custody(&wrong_profile, &corpus, &writer, &reader),
            Err(S6DossierCustodyError::InvalidSubject)
        );
        let mut wrong_binding = corpus.clone();
        let offset = wrong_binding.len() - 1;
        wrong_binding[offset] ^= 1;
        assert!(preflight_s6_dossier_custody(&subject, &wrong_binding, &writer, &reader).is_err());
    }

    #[test]
    fn every_required_s2_member_and_store_binding_is_a_prewrite_gate() {
        let (subject, corpus, writer_channel, reader_channel) = fixture();
        let decoded: serde_json::Value = serde_json::from_slice(&subject).unwrap();
        for field in [
            "certificate",
            "sealed_plan",
            "preflight",
            "s0_manifest",
            "predecessor_lock",
            "successor_lock",
            "products_registry",
            "source_dependencies_registry",
            "release_control_authorities_registry",
            "predecessor_catalogue",
            "successor_catalogue",
            "successor_sources_lock",
            "package_plan",
        ] {
            let mut missing = decoded.clone();
            missing["s2"].as_object_mut().unwrap().remove(field);
            assert_eq!(
                preflight_s6_dossier_custody(
                    &serde_jcs::to_vec(&missing).unwrap(),
                    &corpus,
                    &writer_channel,
                    &reader_channel,
                ),
                Err(S6DossierCustodyError::InvalidSubject),
                "S2 member {field} is mandatory"
            );
        }
        let mut unexpected = decoded;
        unexpected["s2"]["unexpected"] = serde_json::Value::Bool(true);
        assert_eq!(
            preflight_s6_dossier_custody(
                &serde_jcs::to_vec(&unexpected).unwrap(),
                &corpus,
                &writer_channel,
                &reader_channel,
            ),
            Err(S6DossierCustodyError::InvalidSubject)
        );

        let mut writer = MemoryWriter {
            store_id: "other-store".to_owned(),
            peer: "s6-writer".to_owned(),
            ..Default::default()
        };
        let mut reader = MemoryReader {
            store_id: "dossiers".to_owned(),
            peer: "s6-reader".to_owned(),
            objects: BTreeMap::new(),
        };
        assert_eq!(
            retain_s6_dossier_corpus(
                &subject,
                &corpus,
                &writer_channel,
                &reader_channel,
                "2030-01-01T00:00:00Z",
                &mut writer,
                &mut reader,
            ),
            Err(S6DossierCustodyError::StoreMismatch)
        );
        assert_eq!(writer.writes, 0, "store mismatch must precede create");
    }
}
