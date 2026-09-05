//! Dedicated daemon admission for the local trusted-administrator custody overlay.

use crate::api::{DaemonJobAcceptedResponse, DaemonJobId, DaemonJobKind, PreverifiedHostSubject};
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_object_service::{
    CustodyFreshBucketProofV1, CustodyIntegrityReceiptV1, CustodyObjectInputV1,
    CustodyStoreDefinitionV1,
};
use serde::{Deserialize, Serialize};

pub const CUSTODY_ADMISSION_CONFIRMATION: &str = "confirm custody admission";
pub const CUSTODY_RETAIN_CONFIRMATION: &str = "confirm custody retain";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CustodyAdmissionRequest {
    pub definition: CustodyStoreDefinitionV1,
    pub fresh_bucket_proof: CustodyFreshBucketProofV1,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_subject: Option<PreverifiedHostSubject>,
    pub confirmation_marker: String,
}

impl CustodyAdmissionRequest {
    pub fn validate(&self) -> Result<(), CustodyAdmissionValidationError> {
        self.definition.validate().map_err(|error| {
            CustodyAdmissionValidationError::InvalidDefinition(error.to_string())
        })?;
        self.fresh_bucket_proof
            .validate_for_definition(&self.definition, &self.fresh_bucket_proof.created_at_utc)
            .map_err(|error| {
                CustodyAdmissionValidationError::InvalidDefinition(error.to_string())
            })?;
        if self.confirmation_marker != CUSTODY_ADMISSION_CONFIRMATION {
            return Err(CustodyAdmissionValidationError::ConfirmationMismatch);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CustodyAdmissionResponse {
    pub accepted: DaemonJobAcceptedResponse,
    pub store_id: String,
    pub bucket_name: String,
}

impl CustodyAdmissionResponse {
    pub fn accepted(
        job_id: DaemonJobId,
        accepted_at_utc: impl Into<String>,
        request: &CustodyAdmissionRequest,
    ) -> Self {
        Self {
            accepted: DaemonJobAcceptedResponse {
                job_id,
                kind: DaemonJobKind::ObjectStoreCreation,
                accepted_at_utc: accepted_at_utc.into(),
                dry_run: request.dry_run,
            },
            store_id: request.definition.store_id.to_string(),
            bucket_name: request.definition.bucket_name.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CustodyAdmissionValidationError {
    InvalidDefinition(String),
    ConfirmationMismatch,
}

impl std::fmt::Display for CustodyAdmissionValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDefinition(error) => formatter.write_str(error),
            Self::ConfirmationMismatch => {
                formatter.write_str("custody admission confirmation marker does not match")
            }
        }
    }
}

impl std::error::Error for CustodyAdmissionValidationError {}

/// A secret-free, one-use request to retain exactly one custody object. The
/// two handoff references are resolved only inside the daemon by a configured
/// attended credential authority; they are never normal Garage registry
/// records and are never API credential fields.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CustodyRetainRequest {
    pub store_id: String,
    pub input: CustodyObjectInputV1,
    pub writer_handoff_reference: String,
    pub reader_handoff_reference: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_subject: Option<PreverifiedHostSubject>,
    pub confirmation_marker: String,
}

impl CustodyRetainRequest {
    pub fn validate(&self) -> Result<(), CustodyRetainValidationError> {
        if StoreId::new(self.store_id.clone()).is_err()
            || self.writer_handoff_reference.trim().is_empty()
            || self.reader_handoff_reference.trim().is_empty()
            || self.writer_handoff_reference == self.reader_handoff_reference
        {
            return Err(CustodyRetainValidationError::InvalidIdentityBinding);
        }
        if self.confirmation_marker != CUSTODY_RETAIN_CONFIRMATION {
            return Err(CustodyRetainValidationError::ConfirmationMismatch);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CustodyRetainResponse {
    pub receipt: CustodyIntegrityReceiptV1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CustodyRetainValidationError {
    InvalidIdentityBinding,
    ConfirmationMismatch,
}

impl std::fmt::Display for CustodyRetainValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidIdentityBinding => formatter.write_str(
                "custody retain requires distinct nonblank writer and reader handoff references",
            ),
            Self::ConfirmationMismatch => {
                formatter.write_str("custody retain confirmation marker does not match")
            }
        }
    }
}

impl std::error::Error for CustodyRetainValidationError {}

#[cfg(test)]
mod tests {
    use super::{CustodyRetainRequest, CUSTODY_RETAIN_CONFIRMATION};
    use dasobjectstore_object_service::CustodyObjectInputV1;

    fn request() -> CustodyRetainRequest {
        CustodyRetainRequest {
            store_id: "custody-sealed".to_string(),
            input: CustodyObjectInputV1 {
                object_type: "application/test".to_string(),
                bytes: vec![1, 2, 3],
                retained_at_utc: "2026-09-05T12:00:00Z".to_string(),
            },
            writer_handoff_reference: "attended://one-use/writer".to_string(),
            reader_handoff_reference: "attended://one-use/reader".to_string(),
            verified_subject: None,
            confirmation_marker: CUSTODY_RETAIN_CONFIRMATION.to_string(),
        }
    }

    #[test]
    fn retain_transport_has_no_credential_or_sealed_path_escape_hatch() {
        let encoded = serde_json::to_value(request()).expect("encode request");
        assert!(encoded.get("ledger_path").is_none());
        assert!(encoded.get("custody_catalog_path").is_none());
        assert!(encoded.get("credential").is_none());

        for forbidden in [
            ("ledger_path", serde_json::json!("/mutable/ledger.sqlite")),
            (
                "custody_catalog_path",
                serde_json::json!("/mutable/catalog.jsonl"),
            ),
            ("raw_secret", serde_json::json!("must-not-cross-api")),
        ] {
            let mut attempted = encoded.clone();
            attempted[forbidden.0] = forbidden.1;
            assert!(serde_json::from_value::<CustodyRetainRequest>(attempted).is_err());
        }
    }

    #[test]
    fn retain_requires_distinct_one_use_handoff_references() {
        let mut request = request();
        request.reader_handoff_reference = request.writer_handoff_reference.clone();
        assert!(request.validate().is_err());
    }
}
