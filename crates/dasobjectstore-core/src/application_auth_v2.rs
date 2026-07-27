//! Strict Ergasterion task-036 governed capability contracts.
//!
//! This additive module does not alter the established
//! `dasobjectstore.application_auth.v1` wire format. Exchange callers supply a
//! signed v2 binding, while the daemon independently obtains and compares the
//! trusted current host and Prosopikon authority state.

use crate::application_auth::{
    ApplicationAuthValidationError, ApplicationOperation, GovernedBindingStatus,
    GovernedObjectStoreBindingScope, MAX_ACCESS_TOKEN_TTL_SECONDS,
};
use crate::ids::StoreId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const GOVERNED_BINDING_SCHEMA_VERSION_V2: &str = "ergasterion.object-store-binding.v2";
pub const ERGASTERION_CAPABILITY_EXCHANGE_SCHEMA_VERSION: &str =
    "dasobjectstore.ergasterion-capability-exchange.v1";
pub const ERGASTERION_CAPABILITY_RENEWAL_SCHEMA_VERSION: &str =
    "dasobjectstore.ergasterion-capability-renewal.v1";
pub const ERGASTERION_CAPABILITY_DISCOVERY_SCHEMA_VERSION: &str =
    "dasobjectstore.ergasterion-capability-discovery.v1";
pub const ERGASTERION_CAPABILITY_RESPONSE_SCHEMA_VERSION: &str =
    "dasobjectstore.ergasterion-capability-response.v1";
pub const ERGASTERION_APPLICATION_ID: &str = "app-7e4a31c9b260";
pub const ERGASTERION_APPLICATION_KEY_ID: &str = "ergasterion-ed25519-2026-07-19";
pub const ERGASTERION_CAPABILITY_AUDIENCE: &str = "ergasterion-governed-data-service";
pub const ERGASTERION_CAPABILITY_MAX_OBJECT_BYTES: u64 = 64 * 1024 * 1024 * 1024;
pub const ERGASTERION_CAPABILITY_MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024 * 1024;
pub const ERGASTERION_CAPABILITY_RENEWAL_WINDOW_SECONDS: u64 = 5 * 60;
pub const ERGASTERION_CAPABILITY_CLOCK_SKEW_SECONDS: u64 = 30;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernedHostModeV2 {
    Monas,
    Synoptikon,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GovernedHostAuthorityV2 {
    pub mode: GovernedHostModeV2,
    pub authority_id: String,
    pub project_id: String,
    pub project_revision: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GovernedProsopikonAuthorityV2 {
    pub authority_id: String,
    pub authority_revision: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GovernedObjectStoreBindingV2 {
    pub schema_version: String,
    pub binding_id: String,
    pub host_authority: GovernedHostAuthorityV2,
    pub prosopikon_authority: GovernedProsopikonAuthorityV2,
    pub tenant_id: String,
    pub object_store_id: StoreId,
    pub scope: GovernedObjectStoreBindingScope,
    pub issued_at: String,
    pub expires_at: String,
    pub status: GovernedBindingStatus,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GovernedBindingAuthorityVerificationInputV2 {
    pub tenant_id: String,
    pub host_authority: GovernedHostAuthorityV2,
    pub prosopikon_authority: GovernedProsopikonAuthorityV2,
}

pub trait GovernedBindingAuthorityVerifierV2 {
    fn verify(
        &self,
        binding: &GovernedObjectStoreBindingV2,
        trusted: &GovernedBindingAuthorityVerificationInputV2,
    ) -> Result<(), ApplicationAuthValidationError>;
}

impl GovernedObjectStoreBindingV2 {
    pub fn validate_at(&self, now: u64) -> Result<(), ApplicationAuthValidationError> {
        if self.schema_version != GOVERNED_BINDING_SCHEMA_VERSION_V2 {
            return Err(ApplicationAuthValidationError::UnsupportedBindingSchema);
        }
        validate_opaque_id("bindingId", &self.binding_id)?;
        validate_uuid(
            "hostAuthority.authorityId",
            &self.host_authority.authority_id,
        )?;
        validate_opaque_id("hostAuthority.projectId", &self.host_authority.project_id)?;
        validate_uuid(
            "prosopikonAuthority.authorityId",
            &self.prosopikon_authority.authority_id,
        )?;
        validate_uuid("tenantId", &self.tenant_id)?;
        if self.host_authority.project_revision == 0
            || self.prosopikon_authority.authority_revision == 0
            || self.scope.prefixes.is_empty()
            || self.scope.operations.is_empty()
            || duplicates(&self.scope.prefixes)
            || duplicates(&self.scope.operations)
            || self.scope.operations.iter().any(|operation| {
                !matches!(
                    operation,
                    ApplicationOperation::List
                        | ApplicationOperation::Read
                        | ApplicationOperation::Verify
                )
            })
        {
            return Err(ApplicationAuthValidationError::InvalidBinding);
        }
        for prefix in &self.scope.prefixes {
            validate_logical_prefix(prefix)?;
        }
        let issued = timestamp(&self.issued_at)?;
        let expires = timestamp(&self.expires_at)?;
        if expires <= issued
            || now.saturating_add(ERGASTERION_CAPABILITY_CLOCK_SKEW_SECONDS) < issued
            || now >= expires.saturating_add(ERGASTERION_CAPABILITY_CLOCK_SKEW_SECONDS)
        {
            return Err(ApplicationAuthValidationError::BindingInactiveOrExpired);
        }
        Ok(())
    }

    pub fn contains(&self, requested: &ErgasterionRequestedScopeV1) -> bool {
        requested.object_store_id == self.object_store_id
            && requested
                .operations
                .iter()
                .all(|operation| self.scope.operations.contains(operation))
            && requested.prefixes.iter().all(|prefix| {
                self.scope
                    .prefixes
                    .iter()
                    .any(|allowed| prefix_contains(allowed, prefix))
            })
    }

    pub fn verify_current_authority(
        &self,
        trusted: &GovernedBindingAuthorityVerificationInputV2,
    ) -> Result<(), ApplicationAuthValidationError> {
        if self.tenant_id != trusted.tenant_id
            || self.host_authority.mode != trusted.host_authority.mode
            || self.host_authority.authority_id != trusted.host_authority.authority_id
            || self.host_authority.project_id != trusted.host_authority.project_id
            || self.prosopikon_authority.authority_id != trusted.prosopikon_authority.authority_id
        {
            return Err(ApplicationAuthValidationError::BindingAuthorityMismatch);
        }
        if self.host_authority.project_revision != trusted.host_authority.project_revision
            || self.prosopikon_authority.authority_revision
                != trusted.prosopikon_authority.authority_revision
        {
            return Err(ApplicationAuthValidationError::BindingAuthorityStale);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ErgasterionRequestedScopeV1 {
    pub object_store_id: StoreId,
    pub prefixes: Vec<String>,
    pub operations: Vec<ApplicationOperation>,
    pub max_object_bytes: u64,
    pub max_total_bytes: u64,
}

impl ErgasterionRequestedScopeV1 {
    pub fn validate(&self) -> Result<(), ApplicationAuthValidationError> {
        if self.prefixes.is_empty()
            || self.operations.is_empty()
            || duplicates(&self.prefixes)
            || duplicates(&self.operations)
            || self.operations.iter().any(|operation| {
                !matches!(
                    operation,
                    ApplicationOperation::List
                        | ApplicationOperation::Read
                        | ApplicationOperation::Verify
                )
            })
            || self.max_object_bytes == 0
            || self.max_object_bytes > ERGASTERION_CAPABILITY_MAX_OBJECT_BYTES
            || self.max_total_bytes == 0
            || self.max_total_bytes > ERGASTERION_CAPABILITY_MAX_TOTAL_BYTES
            || self.max_object_bytes > self.max_total_bytes
        {
            return Err(ApplicationAuthValidationError::ScopeNotContained);
        }
        for prefix in &self.prefixes {
            validate_logical_prefix(prefix)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ErgasterionExchangeProofAlgorithmV1 {
    Ed25519,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ErgasterionExchangeProofV1 {
    pub algorithm: ErgasterionExchangeProofAlgorithmV1,
    pub signature: String,
}

impl ErgasterionExchangeProofV1 {
    fn validate(&self) -> Result<(), ApplicationAuthValidationError> {
        validate_base64url("proof.signature", &self.signature, 86, 86)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ErgasterionCapabilityExchangeRequestV1 {
    pub schema_version: String,
    pub request_id: String,
    pub application_id: String,
    pub key_id: String,
    pub audience: String,
    pub issued_at: String,
    pub expires_at: String,
    pub nonce: String,
    pub binding: GovernedObjectStoreBindingV2,
    pub requested_scope: ErgasterionRequestedScopeV1,
    pub correlation_id: String,
    pub proof: ErgasterionExchangeProofV1,
}

impl ErgasterionCapabilityExchangeRequestV1 {
    pub const SIGNING_DOMAIN: &'static str = "dasobjectstore.ergasterion-capability-exchange.v1\n";

    pub fn validate_at(&self, now: u64) -> Result<(), ApplicationAuthValidationError> {
        validate_request(
            RequestFields {
                schema: &self.schema_version,
                expected_schema: ERGASTERION_CAPABILITY_EXCHANGE_SCHEMA_VERSION,
                request_id: &self.request_id,
                application_id: &self.application_id,
                key_id: &self.key_id,
                audience: &self.audience,
                issued_at: &self.issued_at,
                expires_at: &self.expires_at,
                nonce: &self.nonce,
                binding: &self.binding,
                requested_scope: &self.requested_scope,
                correlation_id: &self.correlation_id,
                proof: &self.proof,
            },
            now,
        )
    }

    /// Proof-free value for RFC 8785 JCS canonicalization by the verifier.
    pub fn proof_free_value(&self) -> serde_json::Value {
        proof_free_value(self)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ErgasterionCapabilityRenewalRequestV1 {
    pub schema_version: String,
    pub request_id: String,
    pub application_id: String,
    pub key_id: String,
    pub audience: String,
    pub capability_id: String,
    pub issued_at: String,
    pub expires_at: String,
    pub nonce: String,
    pub binding: GovernedObjectStoreBindingV2,
    pub requested_scope: ErgasterionRequestedScopeV1,
    pub correlation_id: String,
    pub proof: ErgasterionExchangeProofV1,
}

impl ErgasterionCapabilityRenewalRequestV1 {
    pub const SIGNING_DOMAIN: &'static str = "dasobjectstore.ergasterion-capability-renewal.v1\n";

    pub fn validate_at(&self, now: u64) -> Result<(), ApplicationAuthValidationError> {
        validate_opaque_id("capabilityId", &self.capability_id)?;
        validate_request(
            RequestFields {
                schema: &self.schema_version,
                expected_schema: ERGASTERION_CAPABILITY_RENEWAL_SCHEMA_VERSION,
                request_id: &self.request_id,
                application_id: &self.application_id,
                key_id: &self.key_id,
                audience: &self.audience,
                issued_at: &self.issued_at,
                expires_at: &self.expires_at,
                nonce: &self.nonce,
                binding: &self.binding,
                requested_scope: &self.requested_scope,
                correlation_id: &self.correlation_id,
                proof: &self.proof,
            },
            now,
        )
    }

    pub fn proof_free_value(&self) -> serde_json::Value {
        proof_free_value(self)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErgasterionCapabilityDiscoveryStateV1 {
    Ready,
    Unavailable,
    Disabled,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ErgasterionCapabilityDiscoveryV1 {
    pub schema_version: String,
    pub exchange_contract: String,
    pub binding_schema: String,
    pub state: ErgasterionCapabilityDiscoveryStateV1,
    pub max_capability_lifetime_seconds: u64,
    pub renewal_window_seconds: u64,
    pub clock_skew_seconds: u64,
    pub operations: Vec<ApplicationOperation>,
}

impl ErgasterionCapabilityDiscoveryV1 {
    pub fn validate(&self) -> Result<(), ApplicationAuthValidationError> {
        if self.schema_version != ERGASTERION_CAPABILITY_DISCOVERY_SCHEMA_VERSION
            || self.exchange_contract != ERGASTERION_CAPABILITY_EXCHANGE_SCHEMA_VERSION
            || self.binding_schema != GOVERNED_BINDING_SCHEMA_VERSION_V2
            || self.max_capability_lifetime_seconds != MAX_ACCESS_TOKEN_TTL_SECONDS
            || self.renewal_window_seconds != ERGASTERION_CAPABILITY_RENEWAL_WINDOW_SECONDS
            || self.clock_skew_seconds != ERGASTERION_CAPABILITY_CLOCK_SKEW_SECONDS
            || self.operations
                != vec![
                    ApplicationOperation::List,
                    ApplicationOperation::Read,
                    ApplicationOperation::Verify,
                ]
        {
            return Err(ApplicationAuthValidationError::UnsupportedSchema);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ErgasterionCapabilityResponseV1 {
    pub schema_version: String,
    pub request_id: String,
    pub capability: String,
    pub capability_id: String,
    pub issued_at: String,
    pub expires_at: String,
    pub resolved_scope: ErgasterionRequestedScopeV1,
    pub renewal_window_seconds: u64,
    pub revocation_checked_at: String,
    pub correlation_id: String,
}

impl ErgasterionCapabilityResponseV1 {
    pub fn validate(&self) -> Result<(), ApplicationAuthValidationError> {
        if self.schema_version != ERGASTERION_CAPABILITY_RESPONSE_SCHEMA_VERSION {
            return Err(ApplicationAuthValidationError::UnsupportedSchema);
        }
        validate_uuid("requestId", &self.request_id)?;
        validate_base64url("capability", &self.capability, 43, 512)?;
        validate_opaque_id("capabilityId", &self.capability_id)?;
        validate_correlation_id(&self.correlation_id)?;
        self.resolved_scope.validate()?;
        let issued = timestamp(&self.issued_at)?;
        let expires = timestamp(&self.expires_at)?;
        let checked = timestamp(&self.revocation_checked_at)?;
        if expires <= issued
            || expires - issued > MAX_ACCESS_TOKEN_TTL_SECONDS
            || checked < issued
            || checked > expires
            || self.renewal_window_seconds != ERGASTERION_CAPABILITY_RENEWAL_WINDOW_SECONDS
        {
            return Err(ApplicationAuthValidationError::InvalidLifetime);
        }
        Ok(())
    }
}

struct RequestFields<'a> {
    schema: &'a str,
    expected_schema: &'a str,
    request_id: &'a str,
    application_id: &'a str,
    key_id: &'a str,
    audience: &'a str,
    issued_at: &'a str,
    expires_at: &'a str,
    nonce: &'a str,
    binding: &'a GovernedObjectStoreBindingV2,
    requested_scope: &'a ErgasterionRequestedScopeV1,
    correlation_id: &'a str,
    proof: &'a ErgasterionExchangeProofV1,
}

fn validate_request(
    fields: RequestFields<'_>,
    now: u64,
) -> Result<(), ApplicationAuthValidationError> {
    let RequestFields {
        schema,
        expected_schema,
        request_id,
        application_id,
        key_id,
        audience,
        issued_at,
        expires_at,
        nonce,
        binding,
        requested_scope,
        correlation_id,
        proof,
    } = fields;
    if schema != expected_schema {
        return Err(ApplicationAuthValidationError::UnsupportedSchema);
    }
    validate_uuid("requestId", request_id)?;
    if application_id != ERGASTERION_APPLICATION_ID
        || key_id != ERGASTERION_APPLICATION_KEY_ID
        || audience != ERGASTERION_CAPABILITY_AUDIENCE
    {
        return Err(ApplicationAuthValidationError::IdentityMismatch);
    }
    validate_base64url("nonce", nonce, 43, 43)?;
    validate_correlation_id(correlation_id)?;
    proof.validate()?;
    requested_scope.validate()?;
    let issued = timestamp(issued_at)?;
    let expires = timestamp(expires_at)?;
    if expires <= issued
        || expires - issued > MAX_ACCESS_TOKEN_TTL_SECONDS
        || issued.abs_diff(now) > ERGASTERION_CAPABILITY_CLOCK_SKEW_SECONDS
    {
        return Err(ApplicationAuthValidationError::InvalidLifetime);
    }
    binding.validate_at(now)?;
    if issued < timestamp(&binding.issued_at)?
        || expires > timestamp(&binding.expires_at)?
        || !binding.contains(requested_scope)
    {
        return Err(ApplicationAuthValidationError::BindingScopeNotContained);
    }
    Ok(())
}

fn proof_free_value(value: &impl Serialize) -> serde_json::Value {
    let mut value = serde_json::to_value(value).expect("capability request is serializable");
    value
        .as_object_mut()
        .expect("capability request serializes as object")
        .remove("proof");
    value
}

fn validate_uuid(field: &'static str, value: &str) -> Result<(), ApplicationAuthValidationError> {
    let bytes = value.as_bytes();
    let valid = bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        });
    if valid {
        Ok(())
    } else {
        Err(ApplicationAuthValidationError::UnsafeField(field))
    }
}

fn validate_opaque_id(
    field: &'static str,
    value: &str,
) -> Result<(), ApplicationAuthValidationError> {
    if (3..=128).contains(&value.len())
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        Ok(())
    } else {
        Err(ApplicationAuthValidationError::UnsafeField(field))
    }
}

fn validate_logical_prefix(value: &str) -> Result<(), ApplicationAuthValidationError> {
    if value.is_empty()
        || value.len() > 512
        || value.starts_with('/')
        || value.contains("//")
        || !value.as_bytes()[0].is_ascii_alphanumeric()
        || value.split('/').any(|component| component == "..")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._/-".contains(&byte))
    {
        Err(ApplicationAuthValidationError::UnsafeField("prefix"))
    } else {
        Ok(())
    }
}

fn validate_base64url(
    field: &'static str,
    value: &str,
    minimum: usize,
    maximum: usize,
) -> Result<(), ApplicationAuthValidationError> {
    if (minimum..=maximum).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        Ok(())
    } else {
        Err(ApplicationAuthValidationError::UnsafeField(field))
    }
}

fn validate_correlation_id(value: &str) -> Result<(), ApplicationAuthValidationError> {
    if (1..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    {
        Ok(())
    } else {
        Err(ApplicationAuthValidationError::UnsafeField("correlationId"))
    }
}

fn timestamp(value: &str) -> Result<u64, ApplicationAuthValidationError> {
    let timestamp = DateTime::parse_from_rfc3339(value)
        .map_err(|_| ApplicationAuthValidationError::InvalidBinding)?
        .with_timezone(&Utc)
        .timestamp();
    u64::try_from(timestamp).map_err(|_| ApplicationAuthValidationError::InvalidBinding)
}

fn prefix_contains(allowed: &str, requested: &str) -> bool {
    requested == allowed
        || requested
            .strip_prefix(allowed)
            .is_some_and(|suffix| allowed.ends_with('/') || suffix.starts_with('/'))
}

fn duplicates<T: Eq>(values: &[T]) -> bool {
    values
        .iter()
        .enumerate()
        .any(|(index, value)| values[..index].contains(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str =
        "../../../docs/user/examples/ergasterion-capability-exchange-request-v1.json";

    #[test]
    fn task_036_exchange_fixture_is_byte_compatible() {
        let request: ErgasterionCapabilityExchangeRequestV1 =
            serde_json::from_str(include_str!(concat!(
                "../../../docs/user/examples/",
                "ergasterion-capability-exchange-request-v1.json"
            )))
            .expect("exchange fixture");
        request
            .binding
            .validate_at(timestamp(&request.binding.issued_at).expect("issued"))
            .expect("normative binding shape");
        request
            .requested_scope
            .validate()
            .expect("normative requested scope");
        assert_eq!(
            request.proof_free_value().get("proof"),
            None,
            "{FIXTURE} must be proof-free before JCS"
        );
        assert!(serde_json::to_value(&request)
            .expect("serialize")
            .get("binding")
            .and_then(|binding| binding.get("hostAuthority"))
            .is_some());
    }

    #[test]
    fn task_036_discovery_fixture_is_exact() {
        let discovery: ErgasterionCapabilityDiscoveryV1 = serde_json::from_str(include_str!(
            "../../../docs/user/examples/ergasterion-capability-discovery-v1.json"
        ))
        .expect("discovery fixture");
        discovery.validate().expect("normative discovery");
    }

    #[test]
    fn binding_requires_exact_trusted_current_revisions() {
        let binding: GovernedObjectStoreBindingV2 = serde_json::from_str(include_str!(
            "../../../docs/user/examples/ergasterion-object-store-binding-v2.json"
        ))
        .expect("binding fixture");
        let trusted = GovernedBindingAuthorityVerificationInputV2 {
            tenant_id: binding.tenant_id.clone(),
            host_authority: binding.host_authority.clone(),
            prosopikon_authority: binding.prosopikon_authority.clone(),
        };
        binding
            .verify_current_authority(&trusted)
            .expect("exact current context");
        let mut stale = trusted.clone();
        stale.host_authority.project_revision += 1;
        assert_eq!(
            binding.verify_current_authority(&stale),
            Err(ApplicationAuthValidationError::BindingAuthorityStale)
        );
    }

    #[test]
    fn strict_shapes_and_scope_fail_closed() {
        let value: serde_json::Value = serde_json::from_str(include_str!(
            "../../../docs/user/examples/ergasterion-capability-exchange-request-v1.json"
        ))
        .expect("fixture");
        let mut unknown = value.clone();
        unknown["unexpected"] = serde_json::json!(true);
        assert!(serde_json::from_value::<ErgasterionCapabilityExchangeRequestV1>(unknown).is_err());

        let mut cross_store: ErgasterionCapabilityExchangeRequestV1 =
            serde_json::from_value(value).expect("request");
        cross_store.requested_scope.object_store_id =
            StoreId::new("store-other-001").expect("store");
        let now = timestamp(&cross_store.issued_at).expect("issued");
        assert_eq!(
            cross_store.validate_at(now),
            Err(ApplicationAuthValidationError::BindingScopeNotContained)
        );
    }
}
