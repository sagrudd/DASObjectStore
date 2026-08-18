//! DAS-owned, path-free projection contract for the Synoptikon demonstration.
//!
//! The producer and consumer identities and the TLS S3 endpoint are deliberately
//! fixed for this deployment slice. Consumers cannot select a managed path,
//! bucket, endpoint, host, or storage disposition. A live adapter must obtain
//! the readiness facts from DAS authorities and perform transfer through the
//! existing scoped application identity and upload-completion boundaries.

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::fmt::{self, Display};
use std::fs::OpenOptions;
use std::io::Read;
#[cfg(unix)]
use std::os::unix::{fs::MetadataExt, fs::OpenOptionsExt};
use std::path::Path;

pub const SYNOPTIKON_PROJECTION_REQUEST_V1_SCHEMA: &str =
    "dasobjectstore.synoptikon_projection_request.v1";
pub const SYNOPTIKON_PROJECTION_READINESS_V1_SCHEMA: &str =
    "dasobjectstore.synoptikon_projection_readiness.v1";
pub const SYNOPTIKON_PROJECTION_SETTLEMENT_V1_SCHEMA: &str =
    "dasobjectstore.synoptikon_projection_settlement.v1";
pub const DAS_AUTHENTICATED_PROJECTION_READINESS_V1_SCHEMA: &str =
    "dasobjectstore.authenticated_synoptikon_projection_readiness.v1";
pub const DAS_MAPPING_EXCLUSION_SETTLEMENT_V1_SCHEMA: &str =
    "dasobjectstore.mapping_exclusion_settlement.v1";

pub const SYNOPTIKON_PROJECTION_PRODUCER_PRODUCT: &str = "syno_plug_demo";
pub const SYNOPTIKON_PROJECTION_PRODUCER_HOST: &str = "nuc-192-168-0-193";
pub const SYNOPTIKON_PROJECTION_CONSUMER_PRODUCT: &str = "oikodome";
pub const SYNOPTIKON_PROJECTION_CONSUMER_HOST: &str = "gb10-192-168-0-48";
pub const SYNOPTIKON_PROJECTION_ENDPOINT: &str = "https://192.168.0.193:3900";
pub const SYNOPTIKON_PROJECTION_OWNER_KEY_PATH: &str =
    "/var/lib/dasobjectstore/projection-authority/synoptikon-owner-hmac.key";
pub const SYNOPTIKON_PROJECTION_TLS_EXPECTATION_PATH: &str =
    "/etc/dasobjectstore/synoptikon-projection-peer.sha256";
pub const SYNOPTIKON_PROJECTION_TLS_CERTIFICATE_PATH: &str =
    "/etc/dasobjectstore/synoptikon-projection-peer.pem";
pub const SYNOPTIKON_PROJECTION_MAX_LIFETIME_SECONDS: u64 = 300;
pub const SYNOPTIKON_PROJECTION_MAX_READINESS_AGE_SECONDS: u64 = 60;
pub const SYNOPTIKON_PROJECTION_MAX_HDD_REPLICAS: usize = 16;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SynoptikonProjectionRequestV1 {
    pub schema_version: String,
    pub projection_id: String,
    pub producer_product: String,
    pub producer_host: String,
    pub consumer_product: String,
    pub consumer_host: String,
    pub object_store_id: String,
    pub object_id: String,
    pub object_version: u64,
    pub object_key: String,
    pub generation: u64,
    pub source_size_bytes: u64,
    pub source_sha256: String,
    pub nonce: String,
    pub requested_at_unix_seconds: u64,
    pub expires_at_unix_seconds: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DasUploadCompletionEvidenceV1 {
    pub receipt_id: String,
    pub receipt_sha256: String,
    pub upload_id: String,
    pub source_size_bytes: u64,
    pub source_sha256: String,
    pub disposition: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DasCatalogueObjectEvidenceV1 {
    pub snapshot_sha256: String,
    pub object_store_id: String,
    pub object_id: String,
    pub object_version: u64,
    pub object_key: String,
    pub source_size_bytes: u64,
    pub source_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DasProviderGroupStatusEvidenceV1 {
    pub status_sha256: String,
    pub object_store_id: String,
    pub object_id: String,
    pub object_version: u64,
    pub settled: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DasHddReplicaEvidenceV1 {
    pub replica_id: String,
    pub placement_sha256: String,
    pub verified_size_bytes: u64,
    pub verified_sha256: String,
    pub disposition: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DasCatalogueMappingEvidenceV1 {
    pub snapshot_sha256: String,
    pub ambiguous_unmapped_objects: u64,
    pub observed_at_unix_seconds: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DasMappingExclusionSettlementV1 {
    pub schema_version: String,
    pub projection_id: String,
    pub generation: u64,
    pub source_sha256: String,
    pub excluded_object_count: u64,
    pub authority_evidence_sha256: String,
    pub catalogue_snapshot_sha256: String,
    pub excluded_set_sha256: String,
    pub settled_at_unix_seconds: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SynoptikonProjectionReadinessV1 {
    pub schema_version: String,
    pub projection_id: String,
    pub generation: u64,
    pub source_sha256: String,
    pub nonce: String,
    pub authority_sequence: u64,
    pub endpoint_url: String,
    pub expected_tls_peer_certificate_sha256: String,
    pub observed_tls_peer_certificate_sha256: String,
    pub daemon_ready: bool,
    pub s3_endpoint_ready: bool,
    pub catalogue_current: bool,
    pub upload_completion: DasUploadCompletionEvidenceV1,
    pub catalogue_object: DasCatalogueObjectEvidenceV1,
    pub provider_group_status: DasProviderGroupStatusEvidenceV1,
    pub hdd_replicas: Vec<DasHddReplicaEvidenceV1>,
    pub hdd_settlement_reference_sha256: String,
    pub catalogue_mapping: DasCatalogueMappingEvidenceV1,
    #[serde(default)]
    pub mapping_exclusion: Option<DasMappingExclusionSettlementV1>,
    pub observed_at_unix_seconds: u64,
    pub expires_at_unix_seconds: u64,
}

/// Authenticated owner-side readiness envelope.
///
/// The HMAC key is retained by the DAS daemon. It is never part of this record,
/// a consumer manifest, or the projection settlement.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DasAuthenticatedProjectionReadinessV1 {
    pub schema_version: String,
    pub readiness: SynoptikonProjectionReadinessV1,
    pub authentication_hmac_sha256: String,
}

/// Opaque proof that the readiness record was authenticated by the DAS owner.
/// Raw caller-deserialised readiness cannot be passed to settlement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedSynoptikonProjectionReadinessV1 {
    readiness: SynoptikonProjectionReadinessV1,
}

pub fn verify_das_owned_synoptikon_projection_readiness(
    envelope: &DasAuthenticatedProjectionReadinessV1,
) -> Result<VerifiedSynoptikonProjectionReadinessV1, SynoptikonProjectionError> {
    let owner_hmac_key = read_fixed_owner_key()?;
    let tls_expectation = read_fixed_tls_expectation()?;
    verify_readiness_with_owner_key(envelope, &owner_hmac_key, &tls_expectation)
}

/// Authenticate readiness assembled inside the DAS daemon with its fixed,
/// descriptor-validated owner key. Callers cannot supply or select the key.
pub fn authenticate_das_owned_synoptikon_projection_readiness(
    readiness: SynoptikonProjectionReadinessV1,
) -> Result<DasAuthenticatedProjectionReadinessV1, SynoptikonProjectionError> {
    let owner_hmac_key = read_fixed_owner_key()?;
    let bytes = serde_jcs::to_vec(&readiness)
        .map_err(|_| SynoptikonProjectionError::OwnerAuthenticationDenied)?;
    Ok(DasAuthenticatedProjectionReadinessV1 {
        schema_version: DAS_AUTHENTICATED_PROJECTION_READINESS_V1_SCHEMA.to_owned(),
        authentication_hmac_sha256: hmac_sha256(&owner_hmac_key, &bytes),
        readiness,
    })
}

fn verify_readiness_with_owner_key(
    envelope: &DasAuthenticatedProjectionReadinessV1,
    owner_hmac_key: &[u8],
    tls_expectation: &str,
) -> Result<VerifiedSynoptikonProjectionReadinessV1, SynoptikonProjectionError> {
    if envelope.schema_version != DAS_AUTHENTICATED_PROJECTION_READINESS_V1_SCHEMA
        || owner_hmac_key.len() < 32
        || !valid_sha256(&envelope.authentication_hmac_sha256)
    {
        return Err(SynoptikonProjectionError::OwnerAuthenticationDenied);
    }
    let bytes = serde_jcs::to_vec(&envelope.readiness)
        .map_err(|_| SynoptikonProjectionError::OwnerAuthenticationDenied)?;
    let expected = hmac_sha256(owner_hmac_key, &bytes);
    if !constant_time_eq(
        expected.as_bytes(),
        envelope.authentication_hmac_sha256.as_bytes(),
    ) {
        return Err(SynoptikonProjectionError::OwnerAuthenticationDenied);
    }
    if !valid_sha256(tls_expectation)
        || envelope.readiness.expected_tls_peer_certificate_sha256 != tls_expectation
        || envelope.readiness.observed_tls_peer_certificate_sha256 != tls_expectation
    {
        return Err(SynoptikonProjectionError::TransportMismatch);
    }
    Ok(VerifiedSynoptikonProjectionReadinessV1 {
        readiness: envelope.readiness.clone(),
    })
}

fn read_fixed_owner_key() -> Result<Vec<u8>, SynoptikonProjectionError> {
    read_protected_owner_key(Path::new(SYNOPTIKON_PROJECTION_OWNER_KEY_PATH))
}

fn read_fixed_tls_expectation() -> Result<String, SynoptikonProjectionError> {
    let bytes =
        read_protected_exact_file(Path::new(SYNOPTIKON_PROJECTION_TLS_EXPECTATION_PATH), 64)?;
    String::from_utf8(bytes)
        .ok()
        .filter(|value| valid_sha256(value))
        .ok_or(SynoptikonProjectionError::OwnerAuthenticationUnavailable)
}

#[cfg(unix)]
fn read_protected_owner_key(path: &Path) -> Result<Vec<u8>, SynoptikonProjectionError> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| SynoptikonProjectionError::OwnerAuthenticationUnavailable)?;
    read_validated_owner_key_file(file)
}

#[cfg(unix)]
fn read_protected_exact_file(
    path: &Path,
    expected_len: u64,
) -> Result<Vec<u8>, SynoptikonProjectionError> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| SynoptikonProjectionError::OwnerAuthenticationUnavailable)?;
    let metadata = file
        .metadata()
        .map_err(|_| SynoptikonProjectionError::OwnerAuthenticationUnavailable)?;
    if !metadata.file_type().is_file()
        || metadata.nlink() != 1
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.gid() != unsafe { libc::getegid() }
        || metadata.mode() & 0o7777 != 0o600
        || metadata.len() != expected_len
    {
        return Err(SynoptikonProjectionError::OwnerAuthenticationUnavailable);
    }
    let mut bytes = Vec::with_capacity(expected_len as usize);
    file.take(expected_len + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| SynoptikonProjectionError::OwnerAuthenticationUnavailable)?;
    if bytes.len() as u64 != expected_len {
        return Err(SynoptikonProjectionError::OwnerAuthenticationUnavailable);
    }
    Ok(bytes)
}

#[cfg(unix)]
fn read_validated_owner_key_file(
    file: std::fs::File,
) -> Result<Vec<u8>, SynoptikonProjectionError> {
    let metadata = file
        .metadata()
        .map_err(|_| SynoptikonProjectionError::OwnerAuthenticationUnavailable)?;
    if !metadata.file_type().is_file()
        || metadata.nlink() != 1
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.gid() != unsafe { libc::getegid() }
        || metadata.mode() & 0o7777 != 0o600
        || metadata.len() < 32
        || metadata.len() > 64
    {
        return Err(SynoptikonProjectionError::OwnerAuthenticationUnavailable);
    }
    let mut key = Vec::with_capacity(metadata.len() as usize);
    file.take(65)
        .read_to_end(&mut key)
        .map_err(|_| SynoptikonProjectionError::OwnerAuthenticationUnavailable)?;
    if !(32..=64).contains(&key.len()) {
        return Err(SynoptikonProjectionError::OwnerAuthenticationUnavailable);
    }
    Ok(key)
}

#[cfg(not(unix))]
fn read_protected_owner_key(_: &Path) -> Result<Vec<u8>, SynoptikonProjectionError> {
    Err(SynoptikonProjectionError::OwnerAuthenticationUnavailable)
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SynoptikonProjectionDispositionV1 {
    HddSettled,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SynoptikonProjectionSettlementV1 {
    pub schema_version: String,
    pub projection_id: String,
    pub request_sha256: String,
    pub readiness_sha256: String,
    pub generation: u64,
    pub source_sha256: String,
    pub object_store_id: String,
    pub object_id: String,
    pub object_version: u64,
    pub nonce: String,
    pub authority_sequence: u64,
    pub upload_completion_receipt_sha256: String,
    pub catalogue_snapshot_sha256: String,
    pub provider_group_status_sha256: String,
    pub hdd_settlement_reference_sha256: String,
    pub hdd_replica_count: u64,
    pub disposition: SynoptikonProjectionDispositionV1,
    pub settled_at_unix_seconds: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SynoptikonProjectionSettlementOutcomeV1 {
    pub settlement: SynoptikonProjectionSettlementV1,
    pub exact_replay: bool,
}

pub fn settle_synoptikon_projection(
    request: &SynoptikonProjectionRequestV1,
    verified_readiness: &VerifiedSynoptikonProjectionReadinessV1,
    settled_at_unix_seconds: u64,
    existing: Option<&SynoptikonProjectionSettlementV1>,
) -> Result<SynoptikonProjectionSettlementOutcomeV1, SynoptikonProjectionError> {
    let readiness = &verified_readiness.readiness;
    validate_request(request)?;
    if request.requested_at_unix_seconds > settled_at_unix_seconds {
        return Err(SynoptikonProjectionError::InvalidRequest);
    }
    validate_readiness(request, readiness, settled_at_unix_seconds)?;
    let request_sha256 = canonical_sha256(request)?;
    let readiness_sha256 = canonical_sha256(readiness)?;
    if let Some(existing) = existing {
        validate_settlement(existing)?;
        if existing.projection_id == request.projection_id
            && existing.request_sha256 == request_sha256
            && existing.readiness_sha256 == readiness_sha256
            && existing.generation == request.generation
            && existing.source_sha256 == request.source_sha256
            && existing.object_store_id == request.object_store_id
            && existing.object_id == request.object_id
            && existing.object_version == request.object_version
            && existing.nonce == request.nonce
            && existing.authority_sequence == readiness.authority_sequence
            && existing.upload_completion_receipt_sha256
                == readiness.upload_completion.receipt_sha256
            && existing.catalogue_snapshot_sha256 == readiness.catalogue_object.snapshot_sha256
            && existing.provider_group_status_sha256
                == readiness.provider_group_status.status_sha256
            && existing.hdd_settlement_reference_sha256 == readiness.hdd_settlement_reference_sha256
            && existing.hdd_replica_count == readiness.hdd_replicas.len() as u64
            && existing.disposition == SynoptikonProjectionDispositionV1::HddSettled
            && existing.settled_at_unix_seconds == readiness.observed_at_unix_seconds
        {
            return Ok(SynoptikonProjectionSettlementOutcomeV1 {
                settlement: existing.clone(),
                exact_replay: true,
            });
        }
        return Err(SynoptikonProjectionError::ConflictingReplay);
    }
    let settlement = SynoptikonProjectionSettlementV1 {
        schema_version: SYNOPTIKON_PROJECTION_SETTLEMENT_V1_SCHEMA.to_owned(),
        projection_id: request.projection_id.clone(),
        request_sha256,
        readiness_sha256,
        generation: request.generation,
        source_sha256: request.source_sha256.clone(),
        object_store_id: request.object_store_id.clone(),
        object_id: request.object_id.clone(),
        object_version: request.object_version,
        nonce: request.nonce.clone(),
        authority_sequence: readiness.authority_sequence,
        upload_completion_receipt_sha256: readiness.upload_completion.receipt_sha256.clone(),
        catalogue_snapshot_sha256: readiness.catalogue_object.snapshot_sha256.clone(),
        provider_group_status_sha256: readiness.provider_group_status.status_sha256.clone(),
        hdd_settlement_reference_sha256: readiness.hdd_settlement_reference_sha256.clone(),
        hdd_replica_count: readiness.hdd_replicas.len() as u64,
        disposition: SynoptikonProjectionDispositionV1::HddSettled,
        // The terminal timestamp is owner-authenticated evidence rather than
        // caller-selected wall time. The argument remains the current time
        // used above for freshness and expiry validation.
        settled_at_unix_seconds: readiness.observed_at_unix_seconds,
    };
    Ok(SynoptikonProjectionSettlementOutcomeV1 {
        settlement,
        exact_replay: false,
    })
}

fn validate_request(
    request: &SynoptikonProjectionRequestV1,
) -> Result<(), SynoptikonProjectionError> {
    if request.schema_version != SYNOPTIKON_PROJECTION_REQUEST_V1_SCHEMA {
        return Err(SynoptikonProjectionError::UnsupportedSchema);
    }
    if request.producer_product != SYNOPTIKON_PROJECTION_PRODUCER_PRODUCT
        || request.producer_host != SYNOPTIKON_PROJECTION_PRODUCER_HOST
        || request.consumer_product != SYNOPTIKON_PROJECTION_CONSUMER_PRODUCT
        || request.consumer_host != SYNOPTIKON_PROJECTION_CONSUMER_HOST
    {
        return Err(SynoptikonProjectionError::IdentityMismatch);
    }
    for value in [
        request.projection_id.as_str(),
        request.object_store_id.as_str(),
        request.object_id.as_str(),
        request.object_key.as_str(),
    ] {
        if !valid_identifier(value) {
            return Err(SynoptikonProjectionError::InvalidRequest);
        }
    }
    if request.object_version == 0
        || request.generation == 0
        || request.source_size_bytes == 0
        || !valid_sha256(&request.source_sha256)
        || !valid_sha256(&request.nonce)
        || request.requested_at_unix_seconds == 0
        || request.expires_at_unix_seconds <= request.requested_at_unix_seconds
        || request.expires_at_unix_seconds - request.requested_at_unix_seconds
            > SYNOPTIKON_PROJECTION_MAX_LIFETIME_SECONDS
    {
        return Err(SynoptikonProjectionError::InvalidRequest);
    }
    Ok(())
}

/// Validate the canonical projection request at the daemon's trusted time.
pub fn validate_synoptikon_projection_request(
    request: &SynoptikonProjectionRequestV1,
    now_unix_seconds: u64,
) -> Result<(), SynoptikonProjectionError> {
    validate_request(request)?;
    if request.requested_at_unix_seconds > now_unix_seconds
        || now_unix_seconds >= request.expires_at_unix_seconds
    {
        return Err(SynoptikonProjectionError::InvalidRequest);
    }
    Ok(())
}

/// Return the SHA-256 fingerprint of the single DER leaf certificate carried
/// by a PEM document. A chain or non-certificate trailing data is rejected.
pub fn synoptikon_tls_leaf_der_sha256(
    pem_bytes: &[u8],
) -> Result<String, SynoptikonProjectionError> {
    let (remaining, pem) = x509_parser::pem::parse_x509_pem(pem_bytes)
        .map_err(|_| SynoptikonProjectionError::TransportMismatch)?;
    if !remaining.iter().all(u8::is_ascii_whitespace) || pem.label != "CERTIFICATE" {
        return Err(SynoptikonProjectionError::TransportMismatch);
    }
    x509_parser::parse_x509_certificate(&pem.contents)
        .map_err(|_| SynoptikonProjectionError::TransportMismatch)?;
    Ok(format!("{:x}", Sha256::digest(&pem.contents)))
}

fn validate_readiness(
    request: &SynoptikonProjectionRequestV1,
    readiness: &SynoptikonProjectionReadinessV1,
    now: u64,
) -> Result<(), SynoptikonProjectionError> {
    if readiness.schema_version != SYNOPTIKON_PROJECTION_READINESS_V1_SCHEMA {
        return Err(SynoptikonProjectionError::UnsupportedSchema);
    }
    if readiness.projection_id != request.projection_id
        || readiness.generation != request.generation
        || readiness.source_sha256 != request.source_sha256
        || readiness.nonce != request.nonce
    {
        return Err(SynoptikonProjectionError::StaleOrSubstitutedReadiness);
    }
    if readiness.endpoint_url != SYNOPTIKON_PROJECTION_ENDPOINT
        || !valid_sha256(&readiness.expected_tls_peer_certificate_sha256)
        || !valid_sha256(&readiness.observed_tls_peer_certificate_sha256)
        || readiness.expected_tls_peer_certificate_sha256
            != readiness.observed_tls_peer_certificate_sha256
    {
        return Err(SynoptikonProjectionError::TransportMismatch);
    }
    if !readiness.daemon_ready || !readiness.s3_endpoint_ready || !readiness.catalogue_current {
        return Err(SynoptikonProjectionError::NotReady);
    }
    if readiness.authority_sequence == 0
        || readiness.observed_at_unix_seconds == 0
        || readiness.observed_at_unix_seconds > now
        || now.saturating_sub(readiness.observed_at_unix_seconds)
            > SYNOPTIKON_PROJECTION_MAX_READINESS_AGE_SECONDS
        || readiness.expires_at_unix_seconds <= readiness.observed_at_unix_seconds
        || now >= readiness.expires_at_unix_seconds
        || now >= request.expires_at_unix_seconds
    {
        return Err(SynoptikonProjectionError::InvalidReadiness);
    }
    validate_object_evidence(request, readiness)?;
    match (
        readiness.catalogue_mapping.ambiguous_unmapped_objects,
        &readiness.mapping_exclusion,
    ) {
        (0, None) => Ok(()),
        (0, Some(_)) => Err(SynoptikonProjectionError::InvalidMappingSettlement),
        (count, Some(exclusion))
            if exclusion.schema_version == DAS_MAPPING_EXCLUSION_SETTLEMENT_V1_SCHEMA
                && exclusion.projection_id == request.projection_id
                && exclusion.generation == request.generation
                && exclusion.source_sha256 == request.source_sha256
                && exclusion.excluded_object_count == count
                && exclusion.catalogue_snapshot_sha256
                    == readiness.catalogue_mapping.snapshot_sha256
                && valid_sha256(&exclusion.excluded_set_sha256)
                && exclusion.settled_at_unix_seconds > 0
                && exclusion.settled_at_unix_seconds <= readiness.observed_at_unix_seconds
                && valid_sha256(&exclusion.authority_evidence_sha256) =>
        {
            Ok(())
        }
        (_, Some(_)) => Err(SynoptikonProjectionError::InvalidMappingSettlement),
        (_, None) => Err(SynoptikonProjectionError::AmbiguousMapping),
    }
}

fn validate_settlement(
    settlement: &SynoptikonProjectionSettlementV1,
) -> Result<(), SynoptikonProjectionError> {
    if settlement.schema_version != SYNOPTIKON_PROJECTION_SETTLEMENT_V1_SCHEMA
        || !valid_sha256(&settlement.request_sha256)
        || !valid_sha256(&settlement.readiness_sha256)
        || !valid_sha256(&settlement.source_sha256)
        || settlement.generation == 0
        || settlement.object_version == 0
        || !valid_sha256(&settlement.nonce)
        || settlement.authority_sequence == 0
        || !valid_sha256(&settlement.upload_completion_receipt_sha256)
        || !valid_sha256(&settlement.catalogue_snapshot_sha256)
        || !valid_sha256(&settlement.provider_group_status_sha256)
        || !valid_sha256(&settlement.hdd_settlement_reference_sha256)
        || settlement.hdd_replica_count == 0
        || settlement.settled_at_unix_seconds == 0
    {
        return Err(SynoptikonProjectionError::InvalidExistingSettlement);
    }
    Ok(())
}

fn validate_object_evidence(
    request: &SynoptikonProjectionRequestV1,
    readiness: &SynoptikonProjectionReadinessV1,
) -> Result<(), SynoptikonProjectionError> {
    let upload = &readiness.upload_completion;
    let catalogue = &readiness.catalogue_object;
    let provider = &readiness.provider_group_status;
    if !valid_identifier(&upload.receipt_id)
        || !valid_identifier(&upload.upload_id)
        || !valid_sha256(&upload.receipt_sha256)
        || upload.source_size_bytes != request.source_size_bytes
        || upload.source_sha256 != request.source_sha256
        || upload.disposition != "committed"
        || !valid_sha256(&catalogue.snapshot_sha256)
        || catalogue.object_store_id != request.object_store_id
        || catalogue.object_id != request.object_id
        || catalogue.object_version != request.object_version
        || catalogue.object_key != request.object_key
        || catalogue.source_size_bytes != request.source_size_bytes
        || catalogue.source_sha256 != request.source_sha256
        || !valid_sha256(&provider.status_sha256)
        || provider.object_store_id != request.object_store_id
        || provider.object_id != request.object_id
        || provider.object_version != request.object_version
        || !provider.settled
        || readiness.hdd_replicas.is_empty()
        || readiness.hdd_replicas.len() > SYNOPTIKON_PROJECTION_MAX_HDD_REPLICAS
        || !valid_sha256(&readiness.hdd_settlement_reference_sha256)
        || !valid_sha256(&readiness.catalogue_mapping.snapshot_sha256)
        || readiness.catalogue_mapping.observed_at_unix_seconds
            != readiness.observed_at_unix_seconds
        || catalogue.snapshot_sha256 != readiness.catalogue_mapping.snapshot_sha256
    {
        return Err(SynoptikonProjectionError::ObjectEvidenceMismatch);
    }
    let mut replica_ids = std::collections::BTreeSet::new();
    for replica in &readiness.hdd_replicas {
        if !valid_identifier(&replica.replica_id)
            || !replica_ids.insert(replica.replica_id.as_str())
            || !valid_sha256(&replica.placement_sha256)
            || replica.verified_size_bytes != request.source_size_bytes
            || replica.verified_sha256 != request.source_sha256
            || replica.disposition != "hdd_verified"
        {
            return Err(SynoptikonProjectionError::ObjectEvidenceMismatch);
        }
    }
    Ok(())
}

fn canonical_sha256(value: &impl Serialize) -> Result<String, SynoptikonProjectionError> {
    let bytes = serde_jcs::to_vec(value).map_err(|_| SynoptikonProjectionError::InvalidRequest)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> String {
    const BLOCK: usize = 64;
    let mut key_block = [0_u8; BLOCK];
    if key.len() > BLOCK {
        key_block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }
    let mut inner_pad = [0x36_u8; BLOCK];
    let mut outer_pad = [0x5c_u8; BLOCK];
    for index in 0..BLOCK {
        inner_pad[index] ^= key_block[index];
        outer_pad[index] ^= key_block[index];
    }
    let mut inner = Sha256::new();
    inner.update(inner_pad);
    inner.update(message);
    let inner_digest = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(outer_pad);
    outer.update(inner_digest);
    format!("{:x}", outer.finalize())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.is_ascii()
        && !value.starts_with('/')
        && !value.ends_with('/')
        && !value.contains("..")
        && !value.contains('\\')
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'.' | b'_' | b'-' | b'/')
        })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SynoptikonProjectionError {
    UnsupportedSchema,
    InvalidRequest,
    IdentityMismatch,
    TransportMismatch,
    NotReady,
    InvalidReadiness,
    StaleOrSubstitutedReadiness,
    AmbiguousMapping,
    InvalidMappingSettlement,
    InvalidExistingSettlement,
    ConflictingReplay,
    OwnerAuthenticationDenied,
    OwnerAuthenticationUnavailable,
    ObjectEvidenceMismatch,
}

impl Display for SynoptikonProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnsupportedSchema => "unsupported_schema",
            Self::InvalidRequest => "invalid_request",
            Self::IdentityMismatch => "identity_mismatch",
            Self::TransportMismatch => "transport_mismatch",
            Self::NotReady => "not_ready",
            Self::InvalidReadiness => "invalid_readiness",
            Self::StaleOrSubstitutedReadiness => "stale_or_substituted_readiness",
            Self::AmbiguousMapping => "ambiguous_mapping",
            Self::InvalidMappingSettlement => "invalid_mapping_settlement",
            Self::InvalidExistingSettlement => "invalid_existing_settlement",
            Self::ConflictingReplay => "conflicting_replay",
            Self::OwnerAuthenticationDenied => "owner_authentication_denied",
            Self::OwnerAuthenticationUnavailable => "owner_authentication_unavailable",
            Self::ObjectEvidenceMismatch => "object_evidence_mismatch",
        })
    }
}

impl std::error::Error for SynoptikonProjectionError {}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_787_040_000;
    const OWNER_KEY: &[u8] = b"das-owner-fixture-key-32-bytes-minimum";

    fn request() -> SynoptikonProjectionRequestV1 {
        serde_json::from_slice(include_bytes!(
            "../fixtures/synoptikon-projection/request-v1.json"
        ))
        .expect("request fixture")
    }

    fn readiness() -> SynoptikonProjectionReadinessV1 {
        serde_json::from_slice(include_bytes!(
            "../fixtures/synoptikon-projection/readiness-v1.json"
        ))
        .expect("readiness fixture")
    }

    fn verified(
        readiness: SynoptikonProjectionReadinessV1,
    ) -> VerifiedSynoptikonProjectionReadinessV1 {
        let bytes = serde_jcs::to_vec(&readiness).unwrap();
        let tls_expectation = readiness.expected_tls_peer_certificate_sha256.clone();
        verify_readiness_with_owner_key(
            &DasAuthenticatedProjectionReadinessV1 {
                schema_version: DAS_AUTHENTICATED_PROJECTION_READINESS_V1_SCHEMA.to_owned(),
                readiness,
                authentication_hmac_sha256: hmac_sha256(OWNER_KEY, &bytes),
            },
            OWNER_KEY,
            &tls_expectation,
        )
        .expect("owner proof")
    }

    #[test]
    fn exact_fixed_identity_settles_and_replays_idempotently() {
        let first = settle_synoptikon_projection(&request(), &verified(readiness()), NOW, None)
            .expect("settlement");
        assert!(!first.exact_replay);
        let replay = settle_synoptikon_projection(
            &request(),
            &verified(readiness()),
            NOW,
            Some(&first.settlement),
        )
        .expect("exact replay");
        assert!(replay.exact_replay);
        assert_eq!(first.settlement, replay.settlement);
    }

    #[test]
    fn live_ambiguous_330_objects_fail_closed_without_das_exclusion() {
        let mut readiness = readiness();
        readiness.catalogue_mapping.ambiguous_unmapped_objects = 330;
        assert_eq!(
            settle_synoptikon_projection(&request(), &verified(readiness), NOW, None),
            Err(SynoptikonProjectionError::AmbiguousMapping)
        );
    }

    #[test]
    fn exact_das_exclusion_can_settle_the_same_source_generation() {
        let request = request();
        let mut readiness = readiness();
        readiness.catalogue_mapping.ambiguous_unmapped_objects = 330;
        readiness.mapping_exclusion = Some(DasMappingExclusionSettlementV1 {
            schema_version: DAS_MAPPING_EXCLUSION_SETTLEMENT_V1_SCHEMA.to_owned(),
            projection_id: request.projection_id.clone(),
            generation: request.generation,
            source_sha256: request.source_sha256.clone(),
            excluded_object_count: 330,
            authority_evidence_sha256: "ee".repeat(32),
            catalogue_snapshot_sha256: readiness.catalogue_mapping.snapshot_sha256.clone(),
            excluded_set_sha256: "ab".repeat(32),
            settled_at_unix_seconds: readiness.observed_at_unix_seconds,
        });
        settle_synoptikon_projection(&request, &verified(readiness.clone()), NOW, None)
            .expect("DAS exclusion settlement");
        readiness.mapping_exclusion.as_mut().unwrap().generation += 1;
        assert_eq!(
            settle_synoptikon_projection(&request, &verified(readiness), NOW, None),
            Err(SynoptikonProjectionError::InvalidMappingSettlement)
        );
    }

    #[test]
    fn absent_3900_stale_generation_and_source_substitution_fail_closed() {
        let request = request();
        let mut absent = readiness();
        absent.s3_endpoint_ready = false;
        assert_eq!(
            settle_synoptikon_projection(&request, &verified(absent), NOW, None),
            Err(SynoptikonProjectionError::NotReady)
        );
        let mut stale = readiness();
        stale.generation += 1;
        assert_eq!(
            settle_synoptikon_projection(&request, &verified(stale), NOW, None),
            Err(SynoptikonProjectionError::StaleOrSubstitutedReadiness)
        );
        let mut substituted = readiness();
        substituted.source_sha256 = "ff".repeat(32);
        assert_eq!(
            settle_synoptikon_projection(&request, &verified(substituted), NOW, None),
            Err(SynoptikonProjectionError::StaleOrSubstitutedReadiness)
        );
    }

    #[test]
    fn endpoint_and_host_substitution_fail_closed() {
        let mut substituted_request = request();
        substituted_request.consumer_host = "other-host".to_owned();
        assert_eq!(
            settle_synoptikon_projection(&substituted_request, &verified(readiness()), NOW, None),
            Err(SynoptikonProjectionError::IdentityMismatch)
        );
        let request = request();
        let mut readiness = readiness();
        readiness.endpoint_url = "https://192.168.0.192:3900".to_owned();
        assert_eq!(
            settle_synoptikon_projection(&request, &verified(readiness), NOW, None),
            Err(SynoptikonProjectionError::TransportMismatch)
        );
    }

    #[test]
    fn tls_identity_hashes_the_der_leaf_not_pem_encoding() {
        let certified = rcgen::generate_simple_self_signed(vec!["192.168.0.193".to_owned()])
            .expect("leaf certificate");
        let pem = certified.cert.pem();
        let fingerprint = synoptikon_tls_leaf_der_sha256(pem.as_bytes()).expect("DER fingerprint");
        assert_eq!(
            fingerprint,
            format!("{:x}", Sha256::digest(certified.cert.der()))
        );
        assert_ne!(fingerprint, format!("{:x}", Sha256::digest(pem.as_bytes())));
        let chained = format!("{pem}{pem}");
        assert_eq!(
            synoptikon_tls_leaf_der_sha256(chained.as_bytes()),
            Err(SynoptikonProjectionError::TransportMismatch)
        );
    }

    #[test]
    fn changed_replay_and_path_fields_are_rejected() {
        let first = settle_synoptikon_projection(&request(), &verified(readiness()), NOW, None)
            .expect("settlement");
        let replay = settle_synoptikon_projection(
            &request(),
            &verified(readiness()),
            NOW + 1,
            Some(&first.settlement),
        )
        .expect("retry time does not change durable settlement");
        assert!(replay.exact_replay);
        assert_eq!(
            replay.settlement.settled_at_unix_seconds,
            readiness().observed_at_unix_seconds
        );
        let mut changed = request();
        changed.object_id = "syno-demo-object-0002".to_owned();
        assert_eq!(
            settle_synoptikon_projection(
                &changed,
                &verified(readiness()),
                NOW + 1,
                Some(&first.settlement),
            ),
            Err(SynoptikonProjectionError::ObjectEvidenceMismatch)
        );
        let mut value: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../fixtures/synoptikon-projection/request-v1.json"
        ))
        .unwrap();
        value["managed_path"] = serde_json::json!("/srv/dasobjectstore/private");
        assert!(serde_json::from_value::<SynoptikonProjectionRequestV1>(value).is_err());

        let mut changed_sequence = readiness();
        changed_sequence.authority_sequence += 1;
        assert_eq!(
            settle_synoptikon_projection(
                &request(),
                &verified(changed_sequence),
                NOW + 1,
                Some(&first.settlement),
            ),
            Err(SynoptikonProjectionError::ConflictingReplay)
        );
        let mut fabricated_time = first.settlement.clone();
        fabricated_time.settled_at_unix_seconds += 1;
        assert_eq!(
            settle_synoptikon_projection(
                &request(),
                &verified(readiness()),
                NOW + 1,
                Some(&fabricated_time),
            ),
            Err(SynoptikonProjectionError::ConflictingReplay)
        );

        let mut fabricated = first.settlement.clone();
        fabricated.hdd_replica_count += 1;
        assert_eq!(
            settle_synoptikon_projection(
                &request(),
                &verified(readiness()),
                NOW + 1,
                Some(&fabricated),
            ),
            Err(SynoptikonProjectionError::ConflictingReplay)
        );
    }

    #[test]
    fn forged_peer_and_mapping_exclusion_are_denied_before_settlement() {
        let mut forged = readiness();
        forged.catalogue_mapping.ambiguous_unmapped_objects = 330;
        forged.mapping_exclusion = Some(DasMappingExclusionSettlementV1 {
            schema_version: DAS_MAPPING_EXCLUSION_SETTLEMENT_V1_SCHEMA.to_owned(),
            projection_id: request().projection_id,
            generation: 1,
            source_sha256: request().source_sha256,
            excluded_object_count: 330,
            authority_evidence_sha256: "ee".repeat(32),
            catalogue_snapshot_sha256: forged.catalogue_mapping.snapshot_sha256.clone(),
            excluded_set_sha256: "ab".repeat(32),
            settled_at_unix_seconds: forged.observed_at_unix_seconds,
        });
        let bytes = serde_jcs::to_vec(&forged).unwrap();
        let mut envelope = DasAuthenticatedProjectionReadinessV1 {
            schema_version: DAS_AUTHENTICATED_PROJECTION_READINESS_V1_SCHEMA.to_owned(),
            readiness: forged,
            authentication_hmac_sha256: hmac_sha256(OWNER_KEY, &bytes),
        };
        envelope.readiness.observed_tls_peer_certificate_sha256 = "bb".repeat(32);
        assert_eq!(
            verify_readiness_with_owner_key(&envelope, OWNER_KEY, &"aa".repeat(32),),
            Err(SynoptikonProjectionError::OwnerAuthenticationDenied)
        );
        envelope.authentication_hmac_sha256 = "cc".repeat(32);
        assert_eq!(
            verify_readiness_with_owner_key(&envelope, OWNER_KEY, &"aa".repeat(32),),
            Err(SynoptikonProjectionError::OwnerAuthenticationDenied)
        );
    }

    #[test]
    fn stale_nonce_and_object_evidence_substitution_are_denied() {
        let request = request();
        let mut stale = readiness();
        stale.observed_at_unix_seconds = NOW - SYNOPTIKON_PROJECTION_MAX_READINESS_AGE_SECONDS - 1;
        stale.catalogue_mapping.observed_at_unix_seconds = stale.observed_at_unix_seconds;
        assert_eq!(
            settle_synoptikon_projection(&request, &verified(stale), NOW, None),
            Err(SynoptikonProjectionError::InvalidReadiness)
        );
        let mut wrong_nonce = readiness();
        wrong_nonce.nonce = "99".repeat(32);
        assert_eq!(
            settle_synoptikon_projection(&request, &verified(wrong_nonce), NOW, None),
            Err(SynoptikonProjectionError::StaleOrSubstitutedReadiness)
        );
        let mut wrong_receipt = readiness();
        wrong_receipt.upload_completion.source_sha256 = "99".repeat(32);
        assert_eq!(
            settle_synoptikon_projection(&request, &verified(wrong_receipt), NOW, None),
            Err(SynoptikonProjectionError::ObjectEvidenceMismatch)
        );
        let mut wrong_replica = readiness();
        wrong_replica.hdd_replicas[0].verified_size_bytes += 1;
        assert_eq!(
            settle_synoptikon_projection(&request, &verified(wrong_replica), NOW, None),
            Err(SynoptikonProjectionError::ObjectEvidenceMismatch)
        );
        let mut duplicate_replica = readiness();
        duplicate_replica
            .hdd_replicas
            .push(duplicate_replica.hdd_replicas[0].clone());
        assert_eq!(
            settle_synoptikon_projection(&request, &verified(duplicate_replica), NOW, None),
            Err(SynoptikonProjectionError::ObjectEvidenceMismatch)
        );
        let mut excessive_replicas = readiness();
        excessive_replicas.hdd_replicas = (0..=SYNOPTIKON_PROJECTION_MAX_HDD_REPLICAS)
            .map(|index| {
                let mut replica = excessive_replicas.hdd_replicas[0].clone();
                replica.replica_id = format!("hdd-replica-{index}");
                replica
            })
            .collect();
        assert_eq!(
            settle_synoptikon_projection(&request, &verified(excessive_replicas), NOW, None),
            Err(SynoptikonProjectionError::ObjectEvidenceMismatch)
        );
    }

    #[cfg(unix)]
    #[test]
    fn protected_owner_key_rejects_symlink_hardlink_mode_and_substitution() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let root = std::env::temp_dir().join(format!(
            "das-synoptikon-owner-key-{}-{}",
            std::process::id(),
            NOW
        ));
        std::fs::create_dir_all(&root).unwrap();
        let key = root.join("owner.key");
        std::fs::write(&key, OWNER_KEY).unwrap();
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(read_protected_owner_key(&key).unwrap(), OWNER_KEY);

        let symlink_path = root.join("symlink.key");
        symlink(&key, &symlink_path).unwrap();
        assert_eq!(
            read_protected_owner_key(&symlink_path),
            Err(SynoptikonProjectionError::OwnerAuthenticationUnavailable)
        );
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o4600)).unwrap();
        assert_eq!(
            read_protected_owner_key(&key),
            Err(SynoptikonProjectionError::OwnerAuthenticationUnavailable)
        );
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
        let hardlink = root.join("hardlink.key");
        std::fs::hard_link(&key, &hardlink).unwrap();
        assert_eq!(
            read_protected_owner_key(&key),
            Err(SynoptikonProjectionError::OwnerAuthenticationUnavailable)
        );
        std::fs::remove_file(&hardlink).unwrap();

        let held = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&key)
            .unwrap();
        let replacement = root.join("replacement.key");
        std::fs::write(&replacement, b"different-owner-key-32-bytes-minimum").unwrap();
        std::fs::set_permissions(&replacement, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::rename(&replacement, &key).unwrap();
        assert_eq!(
            read_validated_owner_key_file(held),
            Err(SynoptikonProjectionError::OwnerAuthenticationUnavailable)
        );
        assert_ne!(read_protected_owner_key(&key).unwrap(), OWNER_KEY);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn protected_tls_expectation_rejects_symlink_hardlink_mode_and_length() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let root = std::env::temp_dir().join(format!(
            "das-synoptikon-tls-expectation-{}-{}",
            std::process::id(),
            NOW
        ));
        std::fs::create_dir_all(&root).unwrap();
        let expectation = root.join("peer.sha256");
        std::fs::write(&expectation, "aa".repeat(32)).unwrap();
        std::fs::set_permissions(&expectation, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            read_protected_exact_file(&expectation, 64).unwrap(),
            vec![b'a'; 64]
        );

        let symlink_path = root.join("peer-link.sha256");
        symlink(&expectation, &symlink_path).unwrap();
        assert_eq!(
            read_protected_exact_file(&symlink_path, 64),
            Err(SynoptikonProjectionError::OwnerAuthenticationUnavailable)
        );
        std::fs::set_permissions(&expectation, std::fs::Permissions::from_mode(0o4600)).unwrap();
        assert_eq!(
            read_protected_exact_file(&expectation, 64),
            Err(SynoptikonProjectionError::OwnerAuthenticationUnavailable)
        );
        std::fs::set_permissions(&expectation, std::fs::Permissions::from_mode(0o600)).unwrap();
        let hardlink = root.join("peer-hardlink.sha256");
        std::fs::hard_link(&expectation, &hardlink).unwrap();
        assert_eq!(
            read_protected_exact_file(&expectation, 64),
            Err(SynoptikonProjectionError::OwnerAuthenticationUnavailable)
        );
        std::fs::remove_file(&hardlink).unwrap();
        std::fs::write(&expectation, "aa").unwrap();
        assert_eq!(
            read_protected_exact_file(&expectation, 64),
            Err(SynoptikonProjectionError::OwnerAuthenticationUnavailable)
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
