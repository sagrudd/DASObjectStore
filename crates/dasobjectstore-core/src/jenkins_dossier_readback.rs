//! Exact, bounded read-back verification for a retained Jenkins dossier.
//!
//! The daemon owns the source reader and the authority decision. This module
//! only consumes that reader so callers cannot turn a byte comparison into a
//! filesystem, provider, session, or local-account authority path.

use crate::EvidenceRefV1;
use sha2::{Digest as _, Sha256};
use std::fmt::{self, Display};
use std::io::Read;

/// One daemon-owned verification result for the exact retained evidence object.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JenkinsDossierReadbackV1 {
    pub evidence: EvidenceRefV1,
    pub size_bytes: u64,
    pub content_sha256: String,
}

/// Read an exact immutable object once and verify its canonical evidence facts.
///
/// The reader is capped at the declared size plus one byte, so a malformed or
/// substituted provider cannot cause unbounded read-back. This calculation is
/// deliberately not an evidence issuer, persistence layer, capability, or
/// authority decision.
pub fn verify_jenkins_dossier_readback(
    evidence: EvidenceRefV1,
    reader: &mut dyn Read,
) -> Result<JenkinsDossierReadbackV1, JenkinsDossierReadbackError> {
    evidence
        .validate()
        .map_err(|_| JenkinsDossierReadbackError::InvalidEvidence)?;
    if evidence.evidence_kind != crate::JENKINS_DOSSIER_EVIDENCE_KIND {
        return Err(JenkinsDossierReadbackError::InvalidEvidence);
    }

    let expected_size = evidence.object_ref.size_bytes;
    let mut remaining = expected_size;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    while remaining > 0 {
        let requested = usize::try_from(remaining.min(buffer.len() as u64))
            .expect("bounded buffer length fits usize");
        let read = reader
            .read(&mut buffer[..requested])
            .map_err(|_| JenkinsDossierReadbackError::ReadFailed)?;
        if read == 0 {
            return Err(JenkinsDossierReadbackError::SizeMismatch);
        }
        hasher.update(&buffer[..read]);
        remaining -= read as u64;
    }
    let mut extra = [0_u8; 1];
    if reader
        .read(&mut extra)
        .map_err(|_| JenkinsDossierReadbackError::ReadFailed)?
        != 0
    {
        return Err(JenkinsDossierReadbackError::SizeMismatch);
    }

    let content_sha256 = format!("{:x}", hasher.finalize());
    if content_sha256 != evidence.object_ref.content_digest.value {
        return Err(JenkinsDossierReadbackError::DigestMismatch);
    }
    Ok(JenkinsDossierReadbackV1 {
        evidence,
        size_bytes: expected_size,
        content_sha256,
    })
}

/// Fail-closed exact read-back errors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JenkinsDossierReadbackError {
    InvalidEvidence,
    ReadFailed,
    SizeMismatch,
    DigestMismatch,
}

impl Display for JenkinsDossierReadbackError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidEvidence => "invalid_evidence",
            Self::ReadFailed => "readback_unavailable",
            Self::SizeMismatch | Self::DigestMismatch => "readback_mismatch",
        })
    }
}

impl std::error::Error for JenkinsDossierReadbackError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AuthorityScopeV1, EvidenceRefV1, JenkinsDossierEvidenceProjectionV1};
    use std::io::Cursor;

    fn evidence(bytes: &[u8]) -> EvidenceRefV1 {
        JenkinsDossierEvidenceProjectionV1 {
            schema: crate::JENKINS_DOSSIER_EVIDENCE_PROJECTION_V1_SCHEMA.to_owned(),
            authority_scope: AuthorityScopeV1 {
                installation_id: "installation-1".to_owned(),
                site_trust_domain_id: Some("site-1".to_owned()),
                tenant_id: None,
                project_id: None,
            },
            store_id: "dossiers".to_owned(),
            object_id: "dossier-0001".to_owned(),
            object_version: 1,
            size_bytes: bytes.len() as u64,
            content_sha256: format!("{:x}", Sha256::digest(bytes)),
            dossier_digest: format!("sha256:{}", "b".repeat(64)),
            evidence_revision: 1,
        }
        .project()
        .expect("valid projection")
    }

    #[test]
    fn accepts_only_the_exact_bounded_readback() {
        let bytes = b"retained dossier";
        let value = verify_jenkins_dossier_readback(evidence(bytes), &mut Cursor::new(bytes))
            .expect("exact bytes pass");
        assert_eq!(value.size_bytes, bytes.len() as u64);
    }

    #[test]
    fn rejects_short_extra_and_substituted_bytes() {
        let bytes = b"retained dossier";
        assert_eq!(
            verify_jenkins_dossier_readback(evidence(bytes), &mut Cursor::new(&bytes[..3])),
            Err(JenkinsDossierReadbackError::SizeMismatch)
        );
        let mut extra = bytes.to_vec();
        extra.push(b'!');
        assert_eq!(
            verify_jenkins_dossier_readback(evidence(bytes), &mut Cursor::new(extra)),
            Err(JenkinsDossierReadbackError::SizeMismatch)
        );
        assert_eq!(
            verify_jenkins_dossier_readback(evidence(bytes), &mut Cursor::new(b"retained Dossier")),
            Err(JenkinsDossierReadbackError::DigestMismatch)
        );
    }
}
