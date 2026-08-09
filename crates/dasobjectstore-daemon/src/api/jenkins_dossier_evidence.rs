//! Fixed-peer, verified-scope read-back contract for retained Jenkins evidence.

use super::ObjectBrowserVerifiedSubject;
use dasobjectstore_core::{ids::StoreId, JenkinsDossierEvidenceProjectionV1};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

/// Versioned daemon operation for retained Jenkins dossier evidence.
pub const JENKINS_DOSSIER_EVIDENCE_SETTLEMENT_V1_SCHEMA: &str =
    "dasobjectstore.jenkins_dossier_evidence_settlement.v1";

/// An exact evidence projection and the only permitted provider authority form.
///
/// The caller cannot attach an application capability, a local actor, a
/// cookie, or a fallback subject. The daemon binds `verified_subject` to its
/// fixed GUI/API service peer before reading any provider bytes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JenkinsDossierEvidenceSettlementRequest {
    pub schema_version: String,
    pub request_id: String,
    pub projection: JenkinsDossierEvidenceProjectionV1,
    pub verified_subject: ObjectBrowserVerifiedSubject,
}

impl JenkinsDossierEvidenceSettlementRequest {
    pub fn validate(&self) -> Result<(), JenkinsDossierEvidenceSettlementValidationError> {
        if self.schema_version != JENKINS_DOSSIER_EVIDENCE_SETTLEMENT_V1_SCHEMA {
            return Err(JenkinsDossierEvidenceSettlementValidationError::UnsupportedSchema);
        }
        if !valid_identifier(&self.request_id) {
            return Err(JenkinsDossierEvidenceSettlementValidationError::InvalidRequestId);
        }
        self.projection
            .project()
            .map_err(JenkinsDossierEvidenceSettlementValidationError::InvalidProjection)?;
        let store_id = StoreId::new(self.projection.store_id.clone())
            .map_err(|_| JenkinsDossierEvidenceSettlementValidationError::InvalidStoreId)?;
        self.verified_subject
            .validate_for_endpoint(&store_id, Some(&self.projection.object_id))
            .map_err(|error| {
                JenkinsDossierEvidenceSettlementValidationError::InvalidVerifiedSubject(
                    error.to_string(),
                )
            })?;
        Ok(())
    }

    pub(crate) fn store_id(&self) -> StoreId {
        StoreId::new(self.projection.store_id.clone()).expect("validated store id")
    }
}

/// Redacted proof of one daemon-owned, independently verified object read-back.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JenkinsDossierEvidenceSettlementResponse {
    pub schema_version: String,
    pub request_id: String,
    pub evidence: dasobjectstore_core::EvidenceRefV1,
    pub size_bytes: u64,
    pub content_sha256: String,
    pub observed_at_utc: String,
}

/// Request-shape failures never grant a read fallback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JenkinsDossierEvidenceSettlementValidationError {
    UnsupportedSchema,
    InvalidRequestId,
    InvalidStoreId,
    InvalidProjection(dasobjectstore_core::JenkinsDossierEvidenceProjectionError),
    InvalidVerifiedSubject(String),
}

impl Display for JenkinsDossierEvidenceSettlementValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::UnsupportedSchema => "unsupported_schema",
            Self::InvalidRequestId
            | Self::InvalidStoreId
            | Self::InvalidProjection(_)
            | Self::InvalidVerifiedSubject(_) => "invalid_input",
        })
    }
}

impl std::error::Error for JenkinsDossierEvidenceSettlementValidationError {}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || (index != 0 && matches!(byte, b'.' | b'_' | b'-'))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{
        ObjectBrowserVerifiedSubject, OBJECT_BROWSER_GUI_API_PEER_IDENTITY,
        OBJECT_BROWSER_VERIFIED_SUBJECT_SCHEMA_VERSION,
    };
    use dasobjectstore_core::AuthorityScopeV1;

    fn request() -> JenkinsDossierEvidenceSettlementRequest {
        JenkinsDossierEvidenceSettlementRequest {
            schema_version: JENKINS_DOSSIER_EVIDENCE_SETTLEMENT_V1_SCHEMA.to_owned(),
            request_id: "jenkins.dossier.1".to_owned(),
            projection: JenkinsDossierEvidenceProjectionV1 {
                schema: dasobjectstore_core::JENKINS_DOSSIER_EVIDENCE_PROJECTION_V1_SCHEMA
                    .to_owned(),
                authority_scope: AuthorityScopeV1 {
                    installation_id: "installation-1".to_owned(),
                    site_trust_domain_id: Some("site-1".to_owned()),
                    tenant_id: None,
                    project_id: None,
                },
                store_id: "dossiers".to_owned(),
                object_id: "dossier-0001".to_owned(),
                object_version: 1,
                size_bytes: 16,
                content_sha256: "a".repeat(64),
                dossier_digest: format!("sha256:{}", "b".repeat(64)),
                evidence_revision: 1,
            },
            verified_subject: ObjectBrowserVerifiedSubject {
                schema_version: OBJECT_BROWSER_VERIFIED_SUBJECT_SCHEMA_VERSION.to_owned(),
                peer_identity: OBJECT_BROWSER_GUI_API_PEER_IDENTITY.to_owned(),
                subject_id: "pistis.subject.1".to_owned(),
                session_id: "session.1".to_owned(),
                correlation_id: "correlation.1".to_owned(),
                store_id: StoreId::new("dossiers").expect("store"),
                canonical_prefix: String::new(),
            },
        }
    }

    #[test]
    fn accepts_only_a_versioned_verified_scope_request() {
        request().validate().expect("verified request validates");
    }

    #[test]
    fn rejects_scope_widening_and_noncanonical_request_id() {
        let mut widened = request();
        widened.verified_subject.canonical_prefix = "other".to_owned();
        assert!(matches!(
            widened.validate(),
            Err(JenkinsDossierEvidenceSettlementValidationError::InvalidVerifiedSubject(_))
        ));
        let mut invalid = request();
        invalid.request_id = "Jenkins".to_owned();
        assert_eq!(
            invalid.validate(),
            Err(JenkinsDossierEvidenceSettlementValidationError::InvalidRequestId)
        );
    }
}
