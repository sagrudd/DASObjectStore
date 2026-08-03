//! Read-only artifact-provenance prerequisite checks for the Monas demo.
//!
//! The contract compares a supplied, already decoded evidence reference with a
//! pinned non-secret prerequisite. It neither resolves the referenced object
//! nor grants authority to read, mutate, settle, or schedule it.

use crate::{EvidenceRefV1, ReferenceValidationError};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

/// Stable schema identifier for a Monas/Oikodome artifact prerequisite.
pub const MONAS_OIKODOME_ARTIFACT_PREREQUISITE_V1_SCHEMA: &str =
    "dasobjectstore.monas_oikodome_artifact_prerequisite.v1";

/// A pinned, non-secret evidence identity required before a demo can proceed.
///
/// A valid instance asserts only that a caller supplied exactly the expected
/// evidence identity. It is not evidence that the object exists or remains
/// readable, settled, authorised, or suitable for a production workflow.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MonasOikodomeArtifactPrerequisiteV1 {
    /// The fixed [`MONAS_OIKODOME_ARTIFACT_PREREQUISITE_V1_SCHEMA`] identifier.
    pub schema: String,
    /// The exact immutable evidence identity required by the demonstration.
    pub expected_evidence: EvidenceRefV1,
}

impl MonasOikodomeArtifactPrerequisiteV1 {
    /// Validates the local, non-secret prerequisite declaration.
    ///
    /// # Errors
    ///
    /// Returns [`ArtifactPrerequisiteError`] if the schema or embedded evidence
    /// identity is invalid.
    pub fn validate(&self) -> Result<(), ArtifactPrerequisiteError> {
        if self.schema != MONAS_OIKODOME_ARTIFACT_PREREQUISITE_V1_SCHEMA {
            return Err(ArtifactPrerequisiteError::UnsupportedSchema);
        }
        self.expected_evidence
            .validate()
            .map_err(ArtifactPrerequisiteError::InvalidExpectedEvidence)
    }

    /// Compares a supplied evidence identity with this exact prerequisite.
    ///
    /// This is a pure comparison after strict evidence validation. It makes no
    /// storage call and cannot establish credentials, trust, a session, or an
    /// object/workflow state transition.
    ///
    /// # Errors
    ///
    /// Returns [`ArtifactPrerequisiteError`] unless both sides are valid and
    /// exactly equal. Any mismatch is intentionally indistinguishable at this
    /// boundary from an unavailable artifact.
    pub fn verify(
        &self,
        supplied_evidence: &EvidenceRefV1,
    ) -> Result<VerifiedMonasOikodomeArtifactV1, ArtifactPrerequisiteError> {
        self.validate()?;
        supplied_evidence
            .validate()
            .map_err(ArtifactPrerequisiteError::InvalidSuppliedEvidence)?;
        if supplied_evidence != &self.expected_evidence {
            return Err(ArtifactPrerequisiteError::EvidenceMismatch);
        }
        Ok(VerifiedMonasOikodomeArtifactV1 {
            evidence: supplied_evidence.clone(),
        })
    }
}

/// A locally verified non-secret evidence identity.
///
/// This value records an exact comparison only. It is deliberately not a
/// capability, approval, receipt, object handle, or storage-health claim.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedMonasOikodomeArtifactV1 {
    /// The exact evidence identity that passed the local comparison.
    pub evidence: EvidenceRefV1,
}

/// Fail-closed prerequisite validation or comparison error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArtifactPrerequisiteError {
    /// The prerequisite schema identifier is not supported.
    UnsupportedSchema,
    /// The locally pinned expected evidence identity is invalid.
    InvalidExpectedEvidence(ReferenceValidationError),
    /// The supplied evidence identity is invalid.
    InvalidSuppliedEvidence(ReferenceValidationError),
    /// Valid evidence differs from the pinned prerequisite identity.
    EvidenceMismatch,
}

impl Display for ArtifactPrerequisiteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnsupportedSchema => "unsupported_schema",
            Self::InvalidExpectedEvidence(_) | Self::InvalidSuppliedEvidence(_) => {
                "invalid_evidence"
            }
            Self::EvidenceMismatch => "prerequisite_unavailable",
        })
    }
}

impl std::error::Error for ArtifactPrerequisiteError {}

#[cfg(test)]
mod tests {
    use super::{
        ArtifactPrerequisiteError, MonasOikodomeArtifactPrerequisiteV1,
        MONAS_OIKODOME_ARTIFACT_PREREQUISITE_V1_SCHEMA,
    };
    use crate::EvidenceRefV1;

    fn evidence() -> EvidenceRefV1 {
        EvidenceRefV1::decode(include_bytes!(
            "../../dasobjectstore-reference/fixtures/evidence-ref-v1.json"
        ))
        .expect("fixture evidence decodes")
    }

    fn prerequisite() -> MonasOikodomeArtifactPrerequisiteV1 {
        MonasOikodomeArtifactPrerequisiteV1 {
            schema: MONAS_OIKODOME_ARTIFACT_PREREQUISITE_V1_SCHEMA.to_string(),
            expected_evidence: evidence(),
        }
    }

    #[test]
    fn exact_valid_evidence_is_the_only_read_only_prerequisite_success() {
        let evidence = evidence();
        let verified = prerequisite()
            .verify(&evidence)
            .expect("exact evidence is accepted");

        assert_eq!(verified.evidence, evidence);
    }

    #[test]
    fn mismatched_but_individually_valid_evidence_fails_closed() {
        let mut substituted = evidence();
        substituted.evidence_revision = 2;
        substituted.domain_digest.value = substituted.expected_domain_digest();
        assert!(substituted.validate().is_ok());

        assert_eq!(
            prerequisite().verify(&substituted),
            Err(ArtifactPrerequisiteError::EvidenceMismatch)
        );
    }

    #[test]
    fn invalid_schema_and_invalid_supplied_evidence_are_rejected() {
        let invalid_schema = MonasOikodomeArtifactPrerequisiteV1 {
            schema: "dasobjectstore.monas_oikodome_artifact_prerequisite.v2".to_string(),
            expected_evidence: evidence(),
        };
        assert_eq!(
            invalid_schema.validate(),
            Err(ArtifactPrerequisiteError::UnsupportedSchema)
        );

        let mut invalid = evidence();
        invalid.domain_digest.value = "0".repeat(64);
        assert!(matches!(
            prerequisite().verify(&invalid),
            Err(ArtifactPrerequisiteError::InvalidSuppliedEvidence(_))
        ));
    }
}
