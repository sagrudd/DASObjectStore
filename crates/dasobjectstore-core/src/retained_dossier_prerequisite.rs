//! Pure, fail-closed DAS evidence check for a retained Jenkins dossier.
//!
//! This contract is deliberately an input verifier, not a dossier store, a
//! Monas session verifier, or a DAS resolver.  The Jenkins/Monas adapter must
//! establish its authenticated session and obtain bytes through the daemon
//! boundary before calling this code.  In particular, a successful result is
//! not authority to promote, publish, schedule, retain, or delete an object.

use crate::{EvidenceRefV1, ReferenceValidationError};
use sha2::{Digest as _, Sha256};
use std::fmt::{self, Display};

/// Schema for one pinned DAS evidence prerequisite of a retained dossier.
pub const JENKINS_RETAINED_DOSSIER_DAS_PREREQUISITE_V1_SCHEMA: &str =
    "dasobjectstore.jenkins_retained_dossier_prerequisite.v1";

/// Non-secret, exact DAS identity expected by one Jenkins source revision.
///
/// The object digest and size are carried by `expected_evidence.object_ref`.
/// The revision is an immutable Git object identifier, not a branch or tag.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JenkinsRetainedDossierDasPrerequisiteV1 {
    pub schema: String,
    pub jenkins_revision: String,
    pub expected_evidence: EvidenceRefV1,
}

impl JenkinsRetainedDossierDasPrerequisiteV1 {
    /// Validates the pinned declaration without consulting a service.
    pub fn validate(&self) -> Result<(), RetainedDossierPrerequisiteError> {
        if self.schema != JENKINS_RETAINED_DOSSIER_DAS_PREREQUISITE_V1_SCHEMA {
            return Err(RetainedDossierPrerequisiteError::UnsupportedSchema);
        }
        if !is_git_revision(&self.jenkins_revision) {
            return Err(RetainedDossierPrerequisiteError::InvalidJenkinsRevision);
        }
        self.expected_evidence
            .validate()
            .map_err(RetainedDossierPrerequisiteError::InvalidExpectedEvidence)
    }

    /// Verifies supplied evidence and the exact already-read bytes.
    ///
    /// This pure calculation makes no catalogue lookup, daemon request,
    /// session check, write, or promotion attempt.  Those boundaries remain
    /// separately mandatory and fail closed outside this helper.
    pub fn verify_readback(
        &self,
        supplied_evidence: &EvidenceRefV1,
        bytes: &[u8],
    ) -> Result<VerifiedRetainedDossierDasReadbackV1, RetainedDossierPrerequisiteError> {
        self.validate()?;
        supplied_evidence
            .validate()
            .map_err(RetainedDossierPrerequisiteError::InvalidSuppliedEvidence)?;
        if supplied_evidence != &self.expected_evidence {
            return Err(RetainedDossierPrerequisiteError::EvidenceMismatch);
        }

        let size_bytes = u64::try_from(bytes.len())
            .map_err(|_| RetainedDossierPrerequisiteError::SizeMismatch)?;
        if size_bytes != supplied_evidence.object_ref.size_bytes {
            return Err(RetainedDossierPrerequisiteError::SizeMismatch);
        }
        let content_sha256 = format!("{:x}", Sha256::digest(bytes));
        if content_sha256 != supplied_evidence.object_ref.content_digest.value {
            return Err(RetainedDossierPrerequisiteError::DigestMismatch);
        }

        Ok(VerifiedRetainedDossierDasReadbackV1 {
            jenkins_revision: self.jenkins_revision.clone(),
            evidence: supplied_evidence.clone(),
            size_bytes,
            content_sha256,
        })
    }
}

/// Result of a local exact-byte comparison, deliberately not a receipt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedRetainedDossierDasReadbackV1 {
    pub jenkins_revision: String,
    pub evidence: EvidenceRefV1,
    pub size_bytes: u64,
    pub content_sha256: String,
}

/// Fail-closed retained-dossier prerequisite error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RetainedDossierPrerequisiteError {
    UnsupportedSchema,
    InvalidJenkinsRevision,
    InvalidExpectedEvidence(ReferenceValidationError),
    InvalidSuppliedEvidence(ReferenceValidationError),
    EvidenceMismatch,
    SizeMismatch,
    DigestMismatch,
}

impl Display for RetainedDossierPrerequisiteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnsupportedSchema => "unsupported_schema",
            Self::InvalidJenkinsRevision
            | Self::InvalidExpectedEvidence(_)
            | Self::InvalidSuppliedEvidence(_) => "invalid_input",
            Self::EvidenceMismatch => "prerequisite_unavailable",
            Self::SizeMismatch | Self::DigestMismatch => "readback_mismatch",
        })
    }
}

impl std::error::Error for RetainedDossierPrerequisiteError {}

fn is_git_revision(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes() -> Vec<u8> {
        b"retained-jenkins-dossier-fixture".to_vec()
    }

    fn evidence() -> EvidenceRefV1 {
        let mut evidence = EvidenceRefV1::decode(include_bytes!(
            "../../dasobjectstore-reference/fixtures/evidence-ref-v1.json"
        ))
        .expect("fixture evidence");
        let bytes = bytes();
        evidence.object_ref.size_bytes = bytes.len() as u64;
        evidence.object_ref.content_digest.value = format!("{:x}", Sha256::digest(&bytes));
        evidence.object_ref.domain_digest.value = evidence.object_ref.expected_domain_digest();
        evidence.domain_digest.value = evidence.expected_domain_digest();
        evidence
    }

    fn prerequisite() -> JenkinsRetainedDossierDasPrerequisiteV1 {
        JenkinsRetainedDossierDasPrerequisiteV1 {
            schema: JENKINS_RETAINED_DOSSIER_DAS_PREREQUISITE_V1_SCHEMA.to_owned(),
            jenkins_revision: "0123456789abcdef0123456789abcdef01234567".to_owned(),
            expected_evidence: evidence(),
        }
    }

    #[test]
    fn exact_evidence_and_bytes_are_the_only_success() {
        let evidence = evidence();
        let verified = prerequisite()
            .verify_readback(&evidence, &bytes())
            .expect("exact bytes pass");
        assert_eq!(verified.evidence, evidence);
        assert_eq!(
            verified.content_sha256,
            evidence.object_ref.content_digest.value
        );
    }

    #[test]
    fn mismatched_evidence_size_and_digest_fail_closed() {
        let mut substituted = evidence();
        substituted.evidence_revision = 2;
        substituted.domain_digest.value = substituted.expected_domain_digest();
        assert_eq!(
            prerequisite().verify_readback(&substituted, &bytes()),
            Err(RetainedDossierPrerequisiteError::EvidenceMismatch)
        );

        let mut wrong_size = bytes();
        wrong_size.push(b'!');
        assert_eq!(
            prerequisite().verify_readback(&evidence(), &wrong_size),
            Err(RetainedDossierPrerequisiteError::SizeMismatch)
        );

        let mut wrong_digest = bytes();
        wrong_digest[0] = b'X';
        assert_eq!(
            prerequisite().verify_readback(&evidence(), &wrong_digest),
            Err(RetainedDossierPrerequisiteError::DigestMismatch)
        );
    }

    #[test]
    fn malformed_pinned_declaration_is_rejected_before_readback() {
        let mut invalid = prerequisite();
        invalid.jenkins_revision = "main".to_owned();
        assert_eq!(
            invalid.verify_readback(&evidence(), &bytes()),
            Err(RetainedDossierPrerequisiteError::InvalidJenkinsRevision)
        );
    }
}
