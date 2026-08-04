//! Synthetic, non-production scoped read-back for the Mnemosyne demonstration.
//!
//! This module deliberately does **not** issue a capability, evaluate Site
//! Trust, create an object, resolve a reference, write storage, or persist a
//! settlement.  Monas owns issuance, Proxenos owns Site Trust evaluation, and
//! Thesaurophylax owns signing/custody.  The narrow verifier port makes that
//! separation explicit: this module can consume only a capability which an
//! external Monas adapter has already verified.

use crate::{EvidenceRefV1, ObjectRefV1, ReferenceValidationError};
use chrono::{DateTime, Duration, Utc};
use sha2::{Digest as _, Sha256};
use std::fmt::{self, Display};

/// Stable, non-secret shape supplied to the DAS consumer verifier.
pub const MONAS_SCOPED_READ_CAPABILITY_V1_SCHEMA: &str = "monas.das_scoped_read_capability.v1";
/// Stable synthetic settlement schema. It is not a durable DAS receipt.
pub const DAS_SYNTHETIC_READBACK_SETTLEMENT_V1_SCHEMA: &str =
    "dasobjectstore.synthetic_readback_settlement.v1";
/// The only retention classification permitted by this fixture-only path.
pub const SYNTHETIC_SEVEN_DAY_RETENTION_CLASS: &str = "synthetic_seven_day";

/// A redacted capability projection for one exact object read-back.
///
/// The value contains no bearer token, credential, key, URL, or filesystem
/// path. It has no authority by itself; callers must provide an independent
/// [`MonasScopedReadCapabilityVerifier`] to use it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MonasScopedReadCapabilityV1 {
    pub schema: String,
    pub capability_id: String,
    pub session_id: String,
    pub object: ObjectRefV1,
    pub expires_at: DateTime<Utc>,
}

impl MonasScopedReadCapabilityV1 {
    fn validate_shape(&self, now: DateTime<Utc>) -> Result<(), SyntheticReadbackError> {
        if self.schema != MONAS_SCOPED_READ_CAPABILITY_V1_SCHEMA {
            return Err(SyntheticReadbackError::UnsupportedCapabilitySchema);
        }
        if !valid_identifier(&self.capability_id) || !valid_identifier(&self.session_id) {
            return Err(SyntheticReadbackError::InvalidCapability);
        }
        self.object
            .validate()
            .map_err(SyntheticReadbackError::InvalidObject)?;
        if now >= self.expires_at {
            return Err(SyntheticReadbackError::ExpiredCapability);
        }
        Ok(())
    }
}

/// External Monas boundary for one already-issued, session-bound capability.
///
/// Implementors must verify the exact capability with Monas before returning
/// `Ok(())`; a synthetic fixture may implement this port only in a test. The
/// port intentionally has no issuance, approval, trust-evaluation, signing,
/// recovery, or storage methods.
pub trait MonasScopedReadCapabilityVerifier {
    fn verify(
        &self,
        capability: &MonasScopedReadCapabilityV1,
        now: DateTime<Utc>,
    ) -> Result<(), SyntheticReadbackError>;
}

/// Read-only bytes supplied by an already-authorised demonstration adapter.
///
/// No storage backend is consulted. The caller retains responsibility for
/// obtaining the bytes through the future supported DAS daemon boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntheticReadbackInputV1 {
    pub capability: MonasScopedReadCapabilityV1,
    pub evidence: EvidenceRefV1,
    pub bytes: Vec<u8>,
    pub observed_at: DateTime<Utc>,
}

/// Redacted, immutable observation of one exact synthetic read-back.
///
/// This record is evidence for the demonstration fixture only. It cannot be
/// used as a capability, a storage health assertion, a production receipt, or
/// authority to retry, promote, publish, schedule, or mutate anything.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyntheticReadbackSettlementV1 {
    pub schema: String,
    pub object: ObjectRefV1,
    pub evidence: EvidenceRefV1,
    pub size_bytes: u64,
    pub content_sha256: String,
    pub observed_at: DateTime<Utc>,
    pub retain_until: DateTime<Utc>,
    pub retention_class: &'static str,
}

/// Verify one externally-authorised synthetic read-back and emit redacted
/// digest/size evidence. This pure function performs no storage mutation.
pub fn verify_synthetic_scoped_readback(
    verifier: &dyn MonasScopedReadCapabilityVerifier,
    input: &SyntheticReadbackInputV1,
) -> Result<SyntheticReadbackSettlementV1, SyntheticReadbackError> {
    input.capability.validate_shape(input.observed_at)?;
    verifier.verify(&input.capability, input.observed_at)?;
    input
        .evidence
        .validate()
        .map_err(SyntheticReadbackError::InvalidEvidence)?;
    if input.evidence.object_ref != input.capability.object {
        return Err(SyntheticReadbackError::EvidenceObjectMismatch);
    }

    let size_bytes =
        u64::try_from(input.bytes.len()).map_err(|_| SyntheticReadbackError::SizeMismatch)?;
    if size_bytes != input.capability.object.size_bytes {
        return Err(SyntheticReadbackError::SizeMismatch);
    }
    let content_sha256 = format!("{:x}", Sha256::digest(&input.bytes));
    if content_sha256 != input.capability.object.content_digest.value {
        return Err(SyntheticReadbackError::DigestMismatch);
    }

    Ok(SyntheticReadbackSettlementV1 {
        schema: DAS_SYNTHETIC_READBACK_SETTLEMENT_V1_SCHEMA.to_owned(),
        object: input.capability.object.clone(),
        evidence: input.evidence.clone(),
        size_bytes,
        content_sha256,
        observed_at: input.observed_at,
        retain_until: input.observed_at + Duration::days(7),
        retention_class: SYNTHETIC_SEVEN_DAY_RETENTION_CLASS,
    })
}

/// Fail-closed synthetic read-back error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SyntheticReadbackError {
    UnsupportedCapabilitySchema,
    InvalidCapability,
    InvalidObject(ReferenceValidationError),
    InvalidEvidence(ReferenceValidationError),
    ExpiredCapability,
    CapabilityDenied,
    EvidenceObjectMismatch,
    SizeMismatch,
    DigestMismatch,
}

impl Display for SyntheticReadbackError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnsupportedCapabilitySchema => "unsupported_capability_schema",
            Self::InvalidCapability | Self::InvalidObject(_) | Self::InvalidEvidence(_) => {
                "invalid_input"
            }
            Self::ExpiredCapability | Self::CapabilityDenied => "capability_denied",
            Self::EvidenceObjectMismatch | Self::SizeMismatch | Self::DigestMismatch => {
                "readback_mismatch"
            }
        })
    }
}

impl std::error::Error for SyntheticReadbackError {}

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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    struct Accept;
    impl MonasScopedReadCapabilityVerifier for Accept {
        fn verify(
            &self,
            _: &MonasScopedReadCapabilityV1,
            _: DateTime<Utc>,
        ) -> Result<(), SyntheticReadbackError> {
            Ok(())
        }
    }

    struct Deny;
    impl MonasScopedReadCapabilityVerifier for Deny {
        fn verify(
            &self,
            _: &MonasScopedReadCapabilityV1,
            _: DateTime<Utc>,
        ) -> Result<(), SyntheticReadbackError> {
            Err(SyntheticReadbackError::CapabilityDenied)
        }
    }

    fn input() -> SyntheticReadbackInputV1 {
        let bytes = b"synthetic-demo-object".to_vec();
        let mut evidence = EvidenceRefV1::decode(include_bytes!(
            "../../dasobjectstore-reference/fixtures/evidence-ref-v1.json"
        ))
        .expect("fixture evidence");
        evidence.object_ref.size_bytes = bytes.len() as u64;
        evidence.object_ref.content_digest.value = format!("{:x}", Sha256::digest(&bytes));
        evidence.object_ref.domain_digest.value = evidence.object_ref.expected_domain_digest();
        evidence.domain_digest.value = evidence.expected_domain_digest();
        let observed_at = Utc.with_ymd_and_hms(2026, 8, 3, 10, 0, 0).unwrap();
        SyntheticReadbackInputV1 {
            capability: MonasScopedReadCapabilityV1 {
                schema: MONAS_SCOPED_READ_CAPABILITY_V1_SCHEMA.to_owned(),
                capability_id: "demo.capability.1".to_owned(),
                session_id: "demo.session.1".to_owned(),
                object: evidence.object_ref.clone(),
                expires_at: observed_at + Duration::minutes(5),
            },
            evidence,
            bytes,
            observed_at,
        }
    }

    #[test]
    fn external_capability_and_exact_bytes_produce_seven_day_synthetic_evidence() {
        let input = input();
        let result = verify_synthetic_scoped_readback(&Accept, &input).expect("read-back");
        assert_eq!(result.object, input.capability.object);
        assert_eq!(result.size_bytes, input.bytes.len() as u64);
        assert_eq!(result.retain_until, input.observed_at + Duration::days(7));
        assert_eq!(result.retention_class, SYNTHETIC_SEVEN_DAY_RETENTION_CLASS);
    }

    #[test]
    fn verifier_denial_is_fail_closed_before_evidence_is_accepted() {
        assert_eq!(
            verify_synthetic_scoped_readback(&Deny, &input()),
            Err(SyntheticReadbackError::CapabilityDenied)
        );
    }

    #[test]
    fn expiry_evidence_substitution_size_and_digest_mismatch_are_rejected() {
        let mut expired = input();
        expired.capability.expires_at = expired.observed_at;
        assert_eq!(
            verify_synthetic_scoped_readback(&Accept, &expired),
            Err(SyntheticReadbackError::ExpiredCapability)
        );

        let mut substituted = input();
        substituted.evidence.object_ref.object_version = 2;
        substituted.evidence.object_ref.domain_digest.value =
            substituted.evidence.object_ref.expected_domain_digest();
        substituted.evidence.domain_digest.value = substituted.evidence.expected_domain_digest();
        assert_eq!(
            verify_synthetic_scoped_readback(&Accept, &substituted),
            Err(SyntheticReadbackError::EvidenceObjectMismatch)
        );

        let mut size = input();
        size.bytes.push(b'!');
        assert_eq!(
            verify_synthetic_scoped_readback(&Accept, &size),
            Err(SyntheticReadbackError::SizeMismatch)
        );

        let mut digest = input();
        digest.bytes[0] = b'X';
        assert_eq!(
            verify_synthetic_scoped_readback(&Accept, &digest),
            Err(SyntheticReadbackError::DigestMismatch)
        );
    }
}
