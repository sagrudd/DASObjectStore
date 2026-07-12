//! Transport-neutral capacity admission contracts.
//!
//! The daemon owns the observed policy and filesystem values. These DTOs carry
//! a validated request and a stable decision shape without allowing callers to
//! provide trusted usage or free-space observations.

use dasobjectstore_core::ids::StoreId;
use dasobjectstore_core::store::{
    evaluate_capacity_admission, CapacityAdmissionError, CapacityAdmissionInput, CapacityPolicy,
};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CapacityAdmissionRequest {
    pub store_id: String,
    pub requested_bytes: u64,
    pub copy_count: u8,
    pub requires_ssd_staging: bool,
    #[serde(default)]
    pub client_request_id: Option<String>,
}

impl CapacityAdmissionRequest {
    pub fn validate(&self) -> Result<StoreId, CapacityAdmissionValidationError> {
        if !is_safe_store_id(&self.store_id) {
            return Err(CapacityAdmissionValidationError::InvalidStoreId);
        }
        if self.copy_count == 0 {
            return Err(CapacityAdmissionValidationError::InvalidCopyCount);
        }
        if self
            .client_request_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(CapacityAdmissionValidationError::BlankClientRequestId);
        }
        StoreId::new(self.store_id.clone())
            .map_err(|_| CapacityAdmissionValidationError::InvalidStoreId)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapacityAdmissionValidationError {
    InvalidStoreId,
    InvalidCopyCount,
    BlankClientRequestId,
}

impl Display for CapacityAdmissionValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidStoreId => formatter.write_str("store_id must be a lowercase safe name"),
            Self::InvalidCopyCount => formatter.write_str("copy_count must be greater than zero"),
            Self::BlankClientRequestId => {
                formatter.write_str("client_request_id must not be blank")
            }
        }
    }
}

impl std::error::Error for CapacityAdmissionValidationError {}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapacityAdmissionDecision {
    Admitted,
    Rejected,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapacityAdmissionRejectionReason {
    LogicalQuota,
    BackendReserve,
    SsdStaging,
    ArithmeticOverflow,
    InvalidPolicy,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CapacityAdmissionResponse {
    pub store_id: StoreId,
    pub decision: CapacityAdmissionDecision,
    pub reason: Option<CapacityAdmissionRejectionReason>,
    pub requested_bytes: u64,
    pub copy_count: u8,
    pub requires_ssd_staging: bool,
    pub logical_limit_bytes: Option<u64>,
    pub used_bytes: u64,
    pub reserved_bytes: u64,
    pub logical_available_bytes: Option<u64>,
    pub backend_available_bytes: u64,
    pub ssd_available_bytes: Option<u64>,
    pub required_backend_bytes: u64,
    pub required_ssd_bytes: u64,
    pub message: Option<String>,
}

impl CapacityAdmissionResponse {
    pub fn evaluate(
        request: &CapacityAdmissionRequest,
        policy: &CapacityPolicy,
        input: CapacityAdmissionInput,
    ) -> Result<Self, CapacityAdmissionValidationError> {
        let store_id = request.validate()?;
        let input = CapacityAdmissionInput {
            requested_bytes: request.requested_bytes,
            copy_count: request.copy_count,
            requires_ssd_staging: request.requires_ssd_staging,
            ..input
        };
        let result = evaluate_capacity_admission(policy, input);
        let required_backend_bytes = request
            .requested_bytes
            .checked_mul(u64::from(request.copy_count))
            .unwrap_or(u64::MAX);
        let logical_available_bytes = policy.logical_limit_bytes.map(|limit| {
            limit
                .saturating_sub(policy.backend_reserve_bytes)
                .saturating_sub(input.used_bytes)
                .saturating_sub(input.reserved_bytes)
        });
        let backend_available_bytes = input
            .backend_free_bytes
            .saturating_sub(policy.backend_reserve_bytes);
        let (decision, reason, message) = match result {
            Ok(_) => (CapacityAdmissionDecision::Admitted, None, None),
            Err(error) => (
                CapacityAdmissionDecision::Rejected,
                Some(map_rejection_reason(error)),
                Some(rejection_message(error).to_string()),
            ),
        };
        Ok(Self {
            store_id,
            decision,
            reason,
            requested_bytes: request.requested_bytes,
            copy_count: request.copy_count,
            requires_ssd_staging: request.requires_ssd_staging,
            logical_limit_bytes: policy.logical_limit_bytes,
            used_bytes: input.used_bytes,
            reserved_bytes: input.reserved_bytes,
            logical_available_bytes,
            backend_available_bytes,
            ssd_available_bytes: request.requires_ssd_staging.then_some(input.ssd_free_bytes),
            required_backend_bytes,
            required_ssd_bytes: request
                .requires_ssd_staging
                .then_some(request.requested_bytes)
                .unwrap_or(0),
            message,
        })
    }
}

fn map_rejection_reason(error: CapacityAdmissionError) -> CapacityAdmissionRejectionReason {
    match error {
        CapacityAdmissionError::InvalidCopyCount => CapacityAdmissionRejectionReason::InvalidPolicy,
        CapacityAdmissionError::Overflow => CapacityAdmissionRejectionReason::ArithmeticOverflow,
        CapacityAdmissionError::LogicalQuota { .. } => {
            CapacityAdmissionRejectionReason::LogicalQuota
        }
        CapacityAdmissionError::BackendReserve { .. } => {
            CapacityAdmissionRejectionReason::BackendReserve
        }
        CapacityAdmissionError::SsdStaging { .. } => CapacityAdmissionRejectionReason::SsdStaging,
    }
}

fn rejection_message(error: CapacityAdmissionError) -> &'static str {
    match error {
        CapacityAdmissionError::InvalidCopyCount => "copy_count must be greater than zero",
        CapacityAdmissionError::Overflow => "capacity arithmetic overflow",
        CapacityAdmissionError::LogicalQuota { .. } => "logical capacity quota exceeded",
        CapacityAdmissionError::BackendReserve { .. } => "backend reserve would be consumed",
        CapacityAdmissionError::SsdStaging { .. } => "SSD staging capacity exhausted",
    }
}

fn is_safe_store_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 128 || !value.is_ascii() {
        return false;
    }
    let first = bytes[0];
    (first.is_ascii_lowercase() || first.is_ascii_digit())
        && bytes[1..].iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_' || *byte == b'-'
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request() -> CapacityAdmissionRequest {
        CapacityAdmissionRequest {
            store_id: "codex".to_string(),
            requested_bytes: 100,
            copy_count: 2,
            requires_ssd_staging: true,
            client_request_id: Some("request-1".to_string()),
        }
    }

    fn input() -> CapacityAdmissionInput {
        CapacityAdmissionInput {
            requested_bytes: 100,
            copy_count: 2,
            requires_ssd_staging: true,
            used_bytes: 100,
            reserved_bytes: 20,
            backend_free_bytes: 1_000,
            ssd_free_bytes: 500,
        }
    }

    #[test]
    fn validates_zero_byte_request_and_serializes_stable_shape() {
        let mut request = request();
        request.requested_bytes = 0;
        request.validate().expect("zero-byte request is valid");
        let encoded = serde_json::to_value(request).expect("request serializes");
        assert_eq!(encoded["requires_ssd_staging"], true);
        assert_eq!(encoded["client_request_id"], "request-1");
    }

    #[test]
    fn rejects_invalid_request_fields() {
        let mut request = request();
        request.copy_count = 0;
        assert_eq!(
            request.validate(),
            Err(CapacityAdmissionValidationError::InvalidCopyCount)
        );
        request.copy_count = 1;
        request.store_id = "Not Safe".to_string();
        assert_eq!(
            request.validate(),
            Err(CapacityAdmissionValidationError::InvalidStoreId)
        );
        request.store_id = "safe".to_string();
        request.client_request_id = Some("  ".to_string());
        assert_eq!(
            request.validate(),
            Err(CapacityAdmissionValidationError::BlankClientRequestId)
        );
    }

    #[test]
    fn evaluates_admission_and_preserves_capacity_observations() {
        let response = CapacityAdmissionResponse::evaluate(
            &request(),
            &CapacityPolicy::bounded(1_000, 100),
            input(),
        )
        .expect("request evaluates");
        assert_eq!(response.decision, CapacityAdmissionDecision::Admitted);
        assert_eq!(response.required_backend_bytes, 200);
        assert_eq!(response.required_ssd_bytes, 100);
        assert_eq!(response.logical_available_bytes, Some(780));
        assert_eq!(response.backend_available_bytes, 900);
        assert_eq!(response.ssd_available_bytes, Some(500));
    }

    #[test]
    fn maps_each_core_rejection_to_stable_reason() {
        let policy = CapacityPolicy::bounded(200, 10);
        let mut logical = input();
        logical.requested_bytes = 100;
        logical.used_bytes = 100;
        assert_eq!(
            CapacityAdmissionResponse::evaluate(&request(), &policy, logical)
                .expect("response")
                .reason,
            Some(CapacityAdmissionRejectionReason::LogicalQuota)
        );
        let mut backend = input();
        backend.used_bytes = 0;
        backend.reserved_bytes = 0;
        backend.backend_free_bytes = 100;
        assert_eq!(
            CapacityAdmissionResponse::evaluate(&request(), &policy, backend)
                .expect("response")
                .reason,
            Some(CapacityAdmissionRejectionReason::BackendReserve)
        );
        let mut ssd = input();
        ssd.used_bytes = 0;
        ssd.reserved_bytes = 0;
        ssd.backend_free_bytes = 1_000;
        ssd.ssd_free_bytes = 50;
        assert_eq!(
            CapacityAdmissionResponse::evaluate(&request(), &policy, ssd)
                .expect("response")
                .reason,
            Some(CapacityAdmissionRejectionReason::SsdStaging)
        );
        let mut overflow = input();
        overflow.requested_bytes = u64::MAX;
        overflow.copy_count = u8::MAX;
        let mut overflow_request = request();
        overflow_request.requested_bytes = u64::MAX;
        overflow_request.copy_count = u8::MAX;
        assert_eq!(
            CapacityAdmissionResponse::evaluate(
                &overflow_request,
                &CapacityPolicy::default(),
                CapacityAdmissionInput {
                    backend_free_bytes: u64::MAX,
                    ssd_free_bytes: u64::MAX,
                    requires_ssd_staging: true,
                    ..overflow
                },
            )
            .expect("response")
            .reason,
            Some(CapacityAdmissionRejectionReason::ArithmeticOverflow)
        );
    }

    #[test]
    fn direct_admission_omits_ssd_observations() {
        let mut request = request();
        request.requires_ssd_staging = false;
        let response = CapacityAdmissionResponse::evaluate(
            &request,
            &CapacityPolicy::bounded(1_000, 100),
            CapacityAdmissionInput {
                ssd_free_bytes: 0,
                ..input()
            },
        )
        .expect("direct request evaluates");
        assert_eq!(response.decision, CapacityAdmissionDecision::Admitted);
        assert_eq!(response.reason, None);
        assert_eq!(response.ssd_available_bytes, None);
        assert_eq!(response.required_ssd_bytes, 0);
    }

    #[test]
    fn response_json_uses_snake_case_decision_and_reason() {
        let mut response = CapacityAdmissionResponse::evaluate(
            &request(),
            &CapacityPolicy::bounded(1_000, 100),
            input(),
        )
        .expect("request evaluates");
        response.decision = CapacityAdmissionDecision::Rejected;
        response.reason = Some(CapacityAdmissionRejectionReason::BackendReserve);
        let encoded = serde_json::to_value(response).expect("response serializes");
        assert_eq!(encoded["decision"], json!("rejected"));
        assert_eq!(encoded["reason"], json!("backend_reserve"));
        assert!(encoded["ssd_available_bytes"].is_number());
    }
}
