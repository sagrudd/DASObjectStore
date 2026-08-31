//! Dedicated Ergasterion governed-capability and object-access contracts.
//!
//! These envelopes cross the local daemon boundary. Bearer material is
//! deliberately redacted from `Debug`, and responses remain path-free.

use super::{
    DaemonRequestValidationError, RemoteObjectGroupStatusRequest, RemoteObjectGroupStatusResponse,
    RemoteObjectSnapshotRequest, RemoteObjectSnapshotResponse,
};
use dasobjectstore_core::application_auth_v2::{
    ErgasterionCapabilityDiscoveryV1, ErgasterionCapabilityExchangeRequestV1,
    ErgasterionCapabilityRenewalRequestV1, ErgasterionCapabilityResponseV1,
};
use serde::{Deserialize, Serialize};
use std::fmt;

pub const ERGASTERION_CAPABILITY_EXCHANGE_ROUTE: &str =
    "/api/v1/application-auth/ergasterion/capability-exchanges";
pub const ERGASTERION_CAPABILITY_RENEWAL_ROUTE: &str =
    "/api/v1/application-auth/ergasterion/capability-exchanges/renewals";
pub const ERGASTERION_CAPABILITY_DISCOVERY_ROUTE: &str =
    "/api/v1/application-auth/ergasterion/capability-discovery";
pub const ERGASTERION_OBJECT_SNAPSHOT_ROUTE: &str =
    "/api/v1/application-auth/ergasterion/objects/snapshot";
pub const ERGASTERION_OBJECT_GROUP_STATUS_ROUTE: &str =
    "/api/v1/application-auth/ergasterion/objects/group-status";
pub const ERGASTERION_OBJECT_READ_ROUTE: &str =
    "/api/v1/application-auth/ergasterion/objects/{store_id}/{version}/{*object_key}";
pub const GOVERNED_BINDING_AUTHORITY_ADMISSION_CONFIRMATION: &str =
    "ADMIT TRUSTED GOVERNED BINDING AUTHORITY";
/// Explicit acknowledgement required before a validated generated-output
/// binding becomes daemon-trusted. This admission does not mint a capability
/// and does not expose an output route.
pub const GENERATED_OUTPUT_BINDING_AUTHORITY_ADMISSION_CONFIRMATION: &str =
    "ADMIT TRUSTED GENERATED OUTPUT BINDING AUTHORITY";
pub const GENERATED_OUTPUT_BINDING_ADMISSION_RECEIPT_SCHEMA_VERSION: &str =
    "dasobjectstore.generated-output-binding-admission-receipt.v1";
pub const GENERATED_OUTPUT_BINDING_ADMISSION_RECEIPT_KIND: &str = "das-output-binding-admission";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GovernedBindingAuthorityAdmissionRequest {
    pub binding: dasobjectstore_core::application_auth_v2::GovernedObjectStoreBindingV2,
    #[serde(default)]
    pub dry_run: bool,
    pub confirmation: String,
}

impl GovernedBindingAuthorityAdmissionRequest {
    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        if self.confirmation != GOVERNED_BINDING_AUTHORITY_ADMISSION_CONFIRMATION {
            return Err(DaemonRequestValidationError::ConfirmationMismatch {
                expected: GOVERNED_BINDING_AUTHORITY_ADMISSION_CONFIRMATION,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GovernedBindingAuthorityAdmissionResponse {
    pub binding_id: String,
    pub object_store_id: dasobjectstore_core::ids::StoreId,
    pub binding_digest_sha256: String,
    pub dry_run: bool,
    pub active: bool,
}

/// Administrator-controlled registration of a generated-output binding. The
/// dynamic application identity and at least one valid public credential are
/// rechecked by the daemon before a non-dry-run record is written.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratedOutputBindingAuthorityAdmissionRequest {
    pub binding: dasobjectstore_core::application_auth_v2::GeneratedOutputBindingV1,
    #[serde(default)]
    pub dry_run: bool,
    pub confirmation: String,
}

impl GeneratedOutputBindingAuthorityAdmissionRequest {
    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        if self.confirmation != GENERATED_OUTPUT_BINDING_AUTHORITY_ADMISSION_CONFIRMATION {
            return Err(DaemonRequestValidationError::ConfirmationMismatch {
                expected: GENERATED_OUTPUT_BINDING_AUTHORITY_ADMISSION_CONFIRMATION,
            });
        }
        Ok(())
    }
}

/// Redacted, provider-produced evidence of a binding-admission decision.
/// It contains neither a credential nor storage topology.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GeneratedOutputBindingAuthorityAdmissionResponse {
    pub receipt_schema_version: String,
    pub receipt_kind: String,
    pub binding_id: String,
    pub application_id: String,
    pub object_store_id: dasobjectstore_core::ids::StoreId,
    pub binding_digest_sha256: String,
    pub admitted_at_unix_seconds: u64,
    pub dry_run: bool,
    pub active: bool,
}

/// An opaque daemon-issued bearer. Its plaintext representation is accepted
/// only at authenticated transport boundaries and is never rendered by
/// `Debug`.
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct OpaqueApplicationCapability(String);

impl OpaqueApplicationCapability {
    pub fn new(value: impl Into<String>) -> Result<Self, DaemonRequestValidationError> {
        let value = value.into();
        if value.len() < 32
            || value.len() > 512
            || value.trim() != value
            || value.chars().any(char::is_whitespace)
        {
            return Err(DaemonRequestValidationError::InvalidPolicy {
                message: "opaque application capability is malformed".to_string(),
            });
        }
        Ok(Self(value))
    }

    pub(crate) fn expose_to_daemon(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for OpaqueApplicationCapability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OpaqueApplicationCapability([REDACTED])")
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ErgasterionCapabilityExchangeRequest {
    pub exchange: ErgasterionCapabilityExchangeRequestV1,
}

impl ErgasterionCapabilityExchangeRequest {
    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        // Time-sensitive validation is repeated by the daemon handler against
        // its own clock. This boundary still rejects malformed serialization.
        if self.exchange.schema_version.trim().is_empty() {
            return Err(DaemonRequestValidationError::InvalidPolicy {
                message: "Ergasterion exchange schema version is required".to_string(),
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ErgasterionCapabilityExchangeResponse {
    pub capability: ErgasterionCapabilityResponseV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ErgasterionCapabilityRenewalRequest {
    pub renewal: ErgasterionCapabilityRenewalRequestV1,
}

impl ErgasterionCapabilityRenewalRequest {
    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        if self.renewal.schema_version.trim().is_empty() {
            return Err(DaemonRequestValidationError::InvalidPolicy {
                message: "Ergasterion renewal schema version is required".to_string(),
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ErgasterionCapabilityDiscoveryResponse {
    pub discovery: ErgasterionCapabilityDiscoveryV1,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ErgasterionObjectSnapshotRequest {
    pub capability: OpaqueApplicationCapability,
    pub snapshot: RemoteObjectSnapshotRequest,
}

impl ErgasterionObjectSnapshotRequest {
    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        self.snapshot.validate()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ErgasterionObjectSnapshotResponse {
    pub snapshot: RemoteObjectSnapshotResponse,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ErgasterionObjectGroupStatusRequest {
    pub capability: OpaqueApplicationCapability,
    pub status: RemoteObjectGroupStatusRequest,
}

impl ErgasterionObjectGroupStatusRequest {
    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        self.status.validate()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ErgasterionObjectGroupStatusResponse {
    pub status: RemoteObjectGroupStatusResponse,
}

#[cfg(test)]
mod tests {
    use super::OpaqueApplicationCapability;

    #[test]
    fn opaque_capability_is_redacted_from_debug() {
        let secret =
            OpaqueApplicationCapability::new("dosc_v2_abcdefghijklmnopqrstuvwxyz0123456789")
                .expect("capability");
        let rendered = format!("{secret:?}");
        assert!(rendered.contains("[REDACTED]"));
        assert!(!rendered.contains("abcdefghijklmnopqrstuvwxyz"));
    }

    #[test]
    fn malformed_capabilities_fail_at_the_daemon_boundary() {
        assert!(OpaqueApplicationCapability::new("short").is_err());
        assert!(OpaqueApplicationCapability::new(format!("{} x", "a".repeat(32))).is_err());
    }
}
