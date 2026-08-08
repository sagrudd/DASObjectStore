//! Canonical, non-authorising projection for a retained Jenkins dossier.
//!
//! The input to this module must come from a DAS daemon transaction which has
//! already committed the immutable object version and verified the caller's
//! Monas/Pistis scope.  This module deliberately owns neither of those
//! authority checks: it only prevents a caller from inventing a second
//! `jenkins.dossier` reference grammar.

use crate::{AuthorityScopeV1, DigestV1, EvidenceRefV1, ObjectRefV1, ReferenceValidationError};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

/// Schema for a committed-object projection request.
pub const JENKINS_DOSSIER_EVIDENCE_PROJECTION_V1_SCHEMA: &str =
    "dasobjectstore.jenkins_dossier_evidence_projection.v1";

/// The only v1 evidence kind accepted for a retained Expedition dossier.
pub const JENKINS_DOSSIER_EVIDENCE_KIND: &str = "jenkins.dossier";

/// Canonical facts required to project a committed retained dossier object.
///
/// `dossier_digest` is the domain-separated Jenkins dossier subject digest in
/// the exact `sha256:<lowercase-hex>` spelling. Its hexadecimal payload becomes
/// the `EvidenceRefV1.subject_digest`. The producer excludes only external
/// artifact references from this subject projection, so the DAS assertion can
/// be embedded in the final canonical dossier without creating a hash fixed
/// point. The final canonical dossier digest remains reference-complete and is
/// separately bound by the Jenkins approval.
///
/// This is a non-secret data type.  It contains no URI, backend location,
/// bearer capability, credential, session token, local username, or OS role.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JenkinsDossierEvidenceProjectionV1 {
    pub schema: String,
    pub authority_scope: AuthorityScopeV1,
    pub store_id: String,
    pub object_id: String,
    pub object_version: u64,
    pub size_bytes: u64,
    pub content_sha256: String,
    pub dossier_digest: String,
    pub evidence_revision: u64,
}

impl JenkinsDossierEvidenceProjectionV1 {
    /// Build the owner-defined canonical EvidenceRef from already committed
    /// object facts.
    ///
    /// This function performs no lookup, storage read, write, persistence,
    /// capability verification, session verification, promotion, or retention
    /// change. A successful result is therefore not authority to expose or
    /// retain evidence. The daemon admission/settlement transaction must prove
    /// all of those independent preconditions before using this projection.
    pub fn project(&self) -> Result<EvidenceRefV1, JenkinsDossierEvidenceProjectionError> {
        if self.schema != JENKINS_DOSSIER_EVIDENCE_PROJECTION_V1_SCHEMA {
            return Err(JenkinsDossierEvidenceProjectionError::UnsupportedSchema);
        }

        let mut object_ref = ObjectRefV1 {
            schema: "dasobjectstore.object_ref.v1".to_owned(),
            authority_scope: self.authority_scope.clone(),
            store_id: self.store_id.clone(),
            object_id: self.object_id.clone(),
            object_version: self.object_version,
            size_bytes: self.size_bytes,
            content_digest: DigestV1 {
                algorithm: "sha256".to_owned(),
                value: self.content_sha256.clone(),
            },
            domain_digest: DigestV1 {
                algorithm: "sha256".to_owned(),
                value: String::new(),
            },
        };
        object_ref.domain_digest.value = object_ref.expected_domain_digest();
        object_ref
            .validate()
            .map_err(JenkinsDossierEvidenceProjectionError::InvalidObjectReference)?;

        let subject_digest = parse_dossier_digest(&self.dossier_digest)?;
        let mut evidence_ref = EvidenceRefV1 {
            schema: "dasobjectstore.evidence_ref.v1".to_owned(),
            object_ref,
            evidence_kind: JENKINS_DOSSIER_EVIDENCE_KIND.to_owned(),
            evidence_revision: self.evidence_revision,
            subject_digest: DigestV1 {
                algorithm: "sha256".to_owned(),
                value: subject_digest,
            },
            domain_digest: DigestV1 {
                algorithm: "sha256".to_owned(),
                value: String::new(),
            },
        };
        evidence_ref.domain_digest.value = evidence_ref.expected_domain_digest();
        evidence_ref
            .validate()
            .map_err(JenkinsDossierEvidenceProjectionError::InvalidEvidenceReference)?;
        Ok(evidence_ref)
    }
}

/// Stable, fail-closed projection errors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JenkinsDossierEvidenceProjectionError {
    UnsupportedSchema,
    InvalidObjectReference(ReferenceValidationError),
    InvalidDossierDigest,
    InvalidEvidenceReference(ReferenceValidationError),
}

impl Display for JenkinsDossierEvidenceProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnsupportedSchema => "unsupported_schema",
            Self::InvalidDossierDigest
            | Self::InvalidObjectReference(_)
            | Self::InvalidEvidenceReference(_) => "invalid_input",
        })
    }
}

impl std::error::Error for JenkinsDossierEvidenceProjectionError {}

fn parse_dossier_digest(value: &str) -> Result<String, JenkinsDossierEvidenceProjectionError> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(JenkinsDossierEvidenceProjectionError::InvalidDossierDigest);
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(JenkinsDossierEvidenceProjectionError::InvalidDossierDigest);
    }
    Ok(hex.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projection() -> JenkinsDossierEvidenceProjectionV1 {
        JenkinsDossierEvidenceProjectionV1 {
            schema: JENKINS_DOSSIER_EVIDENCE_PROJECTION_V1_SCHEMA.to_owned(),
            authority_scope: AuthorityScopeV1 {
                installation_id: "installation-0001".to_owned(),
                site_trust_domain_id: Some("site-0001".to_owned()),
                tenant_id: Some("tenant-0001".to_owned()),
                project_id: Some("project-0001".to_owned()),
            },
            store_id: "evidence-store".to_owned(),
            object_id: "object-0001".to_owned(),
            object_version: 1,
            size_bytes: 4096,
            content_sha256: "a".repeat(64),
            dossier_digest: format!("sha256:{}", "b".repeat(64)),
            evidence_revision: 1,
        }
    }

    #[test]
    fn projects_exact_jenkins_dossier_evidence() {
        let projected = projection().project().expect("project reference");
        assert_eq!(projected.evidence_kind, JENKINS_DOSSIER_EVIDENCE_KIND);
        assert_eq!(projected.subject_digest.value, "b".repeat(64));
        assert_eq!(projected.object_ref.content_digest.value, "a".repeat(64));
        projected
            .validate()
            .expect("canonical reference remains valid");
    }

    #[test]
    fn rejects_noncanonical_dossier_digest_before_projection() {
        for digest in [
            "b".repeat(64),
            format!("sha256:{}", "B".repeat(64)),
            format!("sha512:{}", "b".repeat(64)),
            format!("sha256:{}", "b".repeat(63)),
        ] {
            let mut value = projection();
            value.dossier_digest = digest;
            assert_eq!(
                value.project(),
                Err(JenkinsDossierEvidenceProjectionError::InvalidDossierDigest)
            );
        }
    }

    #[test]
    fn rejects_unsafe_object_facts_and_unknown_schema() {
        let mut value = projection();
        value.object_id = "../artifact".to_owned();
        assert!(matches!(
            value.project(),
            Err(JenkinsDossierEvidenceProjectionError::InvalidObjectReference(_))
        ));

        let mut value = projection();
        value.schema = "dasobjectstore.jenkins_dossier_evidence_projection.v2".to_owned();
        assert_eq!(
            value.project(),
            Err(JenkinsDossierEvidenceProjectionError::UnsupportedSchema)
        );
    }
}
