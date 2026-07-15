use crate::api::DaemonRequestValidationError;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationMtlsAuthorizationContext {
    Connection,
    Request,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationMtlsAuthorizationRequest {
    pub certificate_fingerprint_sha256: String,
    pub requested_application_id: Option<String>,
    pub context: ApplicationMtlsAuthorizationContext,
}

impl ApplicationMtlsAuthorizationRequest {
    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        let fingerprint = self
            .certificate_fingerprint_sha256
            .strip_prefix("sha256:")
            .unwrap_or_default();
        if fingerprint.len() != 64
            || !fingerprint
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(DaemonRequestValidationError::UnsupportedFieldValue {
                field: "certificate_fingerprint_sha256",
                value: "must be sha256: followed by 64 lowercase hexadecimal characters"
                    .to_string(),
            });
        }
        if self
            .requested_application_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(DaemonRequestValidationError::BlankField {
                field: "requested_application_id",
            });
        }
        if self.context == ApplicationMtlsAuthorizationContext::Request
            && self.requested_application_id.is_none()
        {
            return Err(DaemonRequestValidationError::BlankField {
                field: "requested_application_id",
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationMtlsAuthorizationResponse {
    pub authorized: bool,
    pub application_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{ApplicationMtlsAuthorizationContext, ApplicationMtlsAuthorizationRequest};

    #[test]
    fn request_context_requires_a_claimed_application() {
        let request = ApplicationMtlsAuthorizationRequest {
            certificate_fingerprint_sha256: format!("sha256:{}", "a".repeat(64)),
            requested_application_id: None,
            context: ApplicationMtlsAuthorizationContext::Request,
        };
        assert!(request.validate().is_err());
    }

    #[test]
    fn fingerprint_is_canonical_and_path_free() {
        let request = ApplicationMtlsAuthorizationRequest {
            certificate_fingerprint_sha256: format!("sha256:{}", "0a".repeat(32)),
            requested_application_id: None,
            context: ApplicationMtlsAuthorizationContext::Connection,
        };
        request.validate().expect("canonical fingerprint");
        let encoded = serde_json::to_string(&request).expect("serialize");
        assert!(!encoded.contains('/'));
    }
}
