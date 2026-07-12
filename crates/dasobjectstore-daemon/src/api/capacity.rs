//! Transport-neutral capacity admission contracts.
//!
//! The daemon owns the observed policy and filesystem values. These DTOs carry
//! a validated request and a stable decision shape without allowing callers to
//! provide trusted usage or free-space observations.

use dasobjectstore_core::ids::StoreId;
use dasobjectstore_core::ingress::IngressOrigin;
use dasobjectstore_core::store::{
    evaluate_capacity_admission, CapacityAdmissionError, CapacityAdmissionInput,
    CapacityLedgerError, CapacityPolicy, CapacityReservationLedger, LogicalObjectVersionCharge,
};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CapacityAdmissionRequest {
    pub store_id: String,
    pub requested_bytes: u64,
    pub copy_count: u8,
    #[serde(default)]
    pub ingress_origin: IngressOrigin,
    #[serde(default)]
    pub client_request_id: Option<String>,
}

impl CapacityAdmissionRequest {
    pub fn requires_ssd_staging(&self) -> bool {
        self.ingress_origin.requires_ssd_staging()
    }

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CapacityAdmissionReservationError {
    InvalidRequest(CapacityAdmissionValidationError),
    MissingReservationId,
    Rejected(CapacityAdmissionResponse),
    Reservation(CapacityLedgerError),
}

impl Display for CapacityAdmissionReservationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest(error) => write!(formatter, "invalid admission request: {error}"),
            Self::MissingReservationId => {
                formatter.write_str("client_request_id is required for reservation")
            }
            Self::Rejected(response) => write!(
                formatter,
                "capacity admission rejected: {}",
                response
                    .message
                    .as_deref()
                    .unwrap_or("capacity policy rejected request")
            ),
            Self::Reservation(error) => {
                write!(formatter, "capacity reservation failed: {error:?}")
            }
        }
    }
}

impl std::error::Error for CapacityAdmissionReservationError {}

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
    pub backend_free_bytes: u64,
    pub backend_available_bytes: u64,
    pub ssd_available_bytes: Option<u64>,
    pub required_backend_bytes: u64,
    pub required_ssd_bytes: u64,
    pub copy_amplification_basis_points: u32,
    pub warning_threshold_basis_points: u16,
    pub critical_threshold_basis_points: u16,
    pub message: Option<String>,
}

impl CapacityAdmissionResponse {
    /// Evaluate using daemon-owned logical usage and reservations. Backend and
    /// SSD free-space observations remain explicit inputs from the daemon's
    /// platform/backend probes; callers cannot override ledger accounting.
    pub fn evaluate_with_ledger(
        request: &CapacityAdmissionRequest,
        policy: &CapacityPolicy,
        ledger: &CapacityReservationLedger,
        backend_free_bytes: u64,
        ssd_free_bytes: u64,
    ) -> Result<Self, CapacityAdmissionValidationError> {
        Self::evaluate(
            request,
            policy,
            CapacityAdmissionInput {
                requested_bytes: request.requested_bytes,
                copy_count: request.copy_count,
                requires_ssd_staging: request.requires_ssd_staging(),
                used_bytes: ledger.used_bytes(),
                reserved_bytes: ledger.reserved_bytes(),
                backend_free_bytes,
                ssd_free_bytes,
            },
        )
    }

    /// Evaluate and reserve one logical object version while the ledger is
    /// exclusively borrowed by the daemon. Rejections never mutate the
    /// ledger; an admitted request uses its client request ID as the durable
    /// reservation key.
    pub fn evaluate_and_reserve(
        request: &CapacityAdmissionRequest,
        policy: &CapacityPolicy,
        ledger: &mut CapacityReservationLedger,
        backend_free_bytes: u64,
        ssd_free_bytes: u64,
    ) -> Result<Self, CapacityAdmissionReservationError> {
        request
            .validate()
            .map_err(CapacityAdmissionReservationError::InvalidRequest)?;
        let reservation_id = request
            .client_request_id
            .as_deref()
            .ok_or(CapacityAdmissionReservationError::MissingReservationId)?;
        let response =
            Self::evaluate_with_ledger(request, policy, ledger, backend_free_bytes, ssd_free_bytes)
                .map_err(CapacityAdmissionReservationError::InvalidRequest)?;
        if response.decision == CapacityAdmissionDecision::Rejected {
            return Err(CapacityAdmissionReservationError::Rejected(response));
        }
        ledger
            .reserve_object_version(
                reservation_id.to_string(),
                LogicalObjectVersionCharge::new(request.requested_bytes),
            )
            .map_err(CapacityAdmissionReservationError::Reservation)?;
        Ok(response)
    }

    pub fn evaluate(
        request: &CapacityAdmissionRequest,
        policy: &CapacityPolicy,
        input: CapacityAdmissionInput,
    ) -> Result<Self, CapacityAdmissionValidationError> {
        let store_id = request.validate()?;
        let input = CapacityAdmissionInput {
            requested_bytes: request.requested_bytes,
            copy_count: request.copy_count,
            requires_ssd_staging: request.requires_ssd_staging(),
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
        let copy_amplification_basis_points = if request.requested_bytes == 0 {
            0
        } else {
            required_backend_bytes
                .saturating_mul(10_000)
                .checked_div(request.requested_bytes)
                .unwrap_or(u64::from(u32::MAX))
                .min(u64::from(u32::MAX)) as u32
        };
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
            requires_ssd_staging: request.requires_ssd_staging(),
            logical_limit_bytes: policy.logical_limit_bytes,
            used_bytes: input.used_bytes,
            reserved_bytes: input.reserved_bytes,
            logical_available_bytes,
            backend_free_bytes: input.backend_free_bytes,
            backend_available_bytes,
            ssd_available_bytes: request
                .requires_ssd_staging()
                .then_some(input.ssd_free_bytes),
            required_backend_bytes,
            required_ssd_bytes: request
                .requires_ssd_staging()
                .then_some(request.requested_bytes)
                .unwrap_or(0),
            copy_amplification_basis_points,
            warning_threshold_basis_points: policy.warning_threshold_basis_points,
            critical_threshold_basis_points: policy.critical_threshold_basis_points,
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
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || *byte == b'_'
                || *byte == b'-'
                || *byte == b'.'
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
            ingress_origin: IngressOrigin::LocalServerSsdFirst,
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
        assert_eq!(encoded["ingress_origin"], "local_server_ssd_first");
        assert_eq!(encoded["client_request_id"], "request-1");
    }

    #[test]
    fn accepts_legacy_store_ids_with_dotted_release_components() {
        let mut request = request();
        request.store_id = "zymo_fecal_2025.05".to_string();
        request.validate().expect("dotted store id remains valid");
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
        assert_eq!(response.backend_free_bytes, 1_000);
        assert_eq!(response.ssd_available_bytes, Some(500));
        assert_eq!(response.copy_amplification_basis_points, 20_000);
        assert_eq!(response.warning_threshold_basis_points, 8_000);
        assert_eq!(response.critical_threshold_basis_points, 9_500);
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
        request.ingress_origin = IngressOrigin::LocalServerDirectImport;
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
    fn ledger_evaluation_uses_daemon_owned_usage_and_reservations() {
        let mut ledger = CapacityReservationLedger::new(CapacityPolicy::bounded(1_000, 0), 700)
            .expect("ledger policy is valid");
        ledger
            .reserve("active-upload", 100)
            .expect("reservation fits");
        let response = CapacityAdmissionResponse::evaluate_with_ledger(
            &request(),
            &CapacityPolicy::bounded(1_000, 0),
            &ledger,
            1_000,
            500,
        )
        .expect("ledger observation evaluates");
        assert_eq!(response.used_bytes, 700);
        assert_eq!(response.reserved_bytes, 100);
        assert_eq!(response.logical_available_bytes, Some(200));
        assert_eq!(response.decision, CapacityAdmissionDecision::Admitted);
    }

    #[test]
    fn evaluate_and_reserve_is_atomic_for_admitted_and_rejected_requests() {
        let policy = CapacityPolicy::bounded(1_000, 0);
        let mut ledger =
            CapacityReservationLedger::new(policy.clone(), 700).expect("ledger policy is valid");
        let response = CapacityAdmissionResponse::evaluate_and_reserve(
            &request(),
            &policy,
            &mut ledger,
            1_000,
            500,
        )
        .expect("admitted request reserves");
        assert_eq!(response.decision, CapacityAdmissionDecision::Admitted);
        assert_eq!(ledger.reservation_bytes("request-1"), Some(100));

        let mut rejected_request = request();
        rejected_request.client_request_id = Some("rejected".to_string());
        rejected_request.requested_bytes = 300;
        let error = CapacityAdmissionResponse::evaluate_and_reserve(
            &rejected_request,
            &policy,
            &mut ledger,
            1_000,
            500,
        )
        .expect_err("logical quota rejects request");
        assert!(matches!(
            error,
            CapacityAdmissionReservationError::Rejected(response)
                if response.reason == Some(CapacityAdmissionRejectionReason::LogicalQuota)
        ));
        assert_eq!(ledger.reservation_bytes("rejected"), None);
    }

    #[test]
    fn evaluate_and_reserve_requires_client_request_id() {
        let mut request = request();
        request.client_request_id = None;
        let policy = CapacityPolicy::bounded(1_000, 0);
        let mut ledger =
            CapacityReservationLedger::new(policy.clone(), 0).expect("ledger policy is valid");
        assert_eq!(
            CapacityAdmissionResponse::evaluate_and_reserve(
                &request,
                &policy,
                &mut ledger,
                1_000,
                500,
            ),
            Err(CapacityAdmissionReservationError::MissingReservationId)
        );
        assert_eq!(ledger.reserved_bytes(), 0);
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
