use crate::api::{DaemonJobAcceptedResponse, DaemonJobId, DaemonJobKind};
use dasobjectstore_core::application_auth::{
    ApplicationAuthValidationError, ApplicationIdentity, ApplicationKeyDescriptor,
    ApplicationOperation, APPLICATION_AUTH_CONTRACT_REVISION, APPLICATION_AUTH_SCHEMA_VERSION,
    GOVERNED_CAPABILITY_CLOCK_SKEW_SECONDS, GOVERNED_CAPABILITY_RENEWAL_WINDOW_SECONDS,
    GOVERNED_REVOCATION_PROPAGATION_SECONDS, MAX_ACCESS_TOKEN_TTL_SECONDS,
};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

pub const APPLICATION_IDENTITY_REGISTRATION_CONFIRMATION: &str =
    "confirm application identity registration";
pub const APPLICATION_CREDENTIAL_REVOCATION_CONFIRMATION: &str =
    "confirm application credential revocation";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationIdentityRegistrationRequest {
    pub identity: ApplicationIdentity,
    pub dry_run: bool,
    pub client_request_id: Option<String>,
    #[serde(default)]
    pub administrator_actor: Option<String>,
    pub confirmation_marker: String,
}

impl ApplicationIdentityRegistrationRequest {
    pub fn validate(&self) -> Result<(), ApplicationIdentityRegistrationValidationError> {
        self.identity
            .validate()
            .map_err(ApplicationIdentityRegistrationValidationError::InvalidIdentity)?;
        if self
            .client_request_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(ApplicationIdentityRegistrationValidationError::BlankClientRequestId);
        }
        if self
            .administrator_actor
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(ApplicationIdentityRegistrationValidationError::BlankAdministratorActor);
        }
        if self.confirmation_marker.trim() != APPLICATION_IDENTITY_REGISTRATION_CONFIRMATION {
            return Err(ApplicationIdentityRegistrationValidationError::ConfirmationMismatch);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationIdentityRegistrationResponse {
    pub accepted: DaemonJobAcceptedResponse,
    pub identity: ApplicationIdentity,
    pub replaced: bool,
    pub administrator_actor: Option<String>,
    /// Non-secret interoperability evidence. Present for identities whose
    /// scope is evaluated from a governed external binding.
    pub registration: Option<ApplicationRegistrationRecord>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationRegistrationRecord {
    pub schema_version: String,
    pub contract_revision: String,
    pub application_id: String,
    pub audience: String,
    pub audit_purpose: String,
    pub binding_schema_version: String,
    pub operations: Vec<ApplicationOperation>,
    pub dynamic_object_store_scope: bool,
    pub dynamic_prefix_scope: bool,
    pub dynamic_per_object_bytes: bool,
    pub dynamic_aggregate_bytes: bool,
    pub max_object_bytes: u64,
    pub max_total_bytes: u64,
    pub max_capability_lifetime_seconds: u64,
    pub renewal_window_seconds: u64,
    pub revocation_propagation_seconds: u64,
    pub replay_protection: String,
    pub clock_skew_seconds: u64,
    pub correlation_id_contract: String,
    pub audit_event_schema: String,
    pub safe_denial_reason: String,
    pub compatibility_procedure: String,
    pub rotation_procedure: String,
    pub incident_procedure: String,
    pub deprovisioning_procedure: String,
}

impl ApplicationRegistrationRecord {
    fn from_identity(identity: &ApplicationIdentity) -> Option<Self> {
        let policy = identity.dynamic_binding.as_ref()?;
        Some(Self {
            schema_version: APPLICATION_AUTH_SCHEMA_VERSION.to_string(),
            contract_revision: APPLICATION_AUTH_CONTRACT_REVISION.to_string(),
            application_id: identity.application_id.clone(),
            audience: policy.audience.clone(),
            audit_purpose: policy.audit_purpose.clone(),
            binding_schema_version: policy.schema_version.clone(),
            operations: identity.scope.operations.clone(),
            dynamic_object_store_scope: true,
            dynamic_prefix_scope: true,
            dynamic_per_object_bytes: true,
            dynamic_aggregate_bytes: true,
            max_object_bytes: policy.max_object_bytes,
            max_total_bytes: policy.max_total_bytes,
            max_capability_lifetime_seconds: MAX_ACCESS_TOKEN_TTL_SECONDS,
            renewal_window_seconds: GOVERNED_CAPABILITY_RENEWAL_WINDOW_SECONDS,
            revocation_propagation_seconds: GOVERNED_REVOCATION_PROPAGATION_SECONDS,
            replay_protection: "signed canonical exchange; deterministic token identity; single-use capability nonce where applicable".to_string(),
            clock_skew_seconds: GOVERNED_CAPABILITY_CLOCK_SKEW_SECONDS,
            correlation_id_contract: "caller-supplied opaque correlation ID, redacted and bounded".to_string(),
            audit_event_schema: "dasobjectstore.application_audit.v1".to_string(),
            safe_denial_reason: "governed_scope_denied".to_string(),
            compatibility_procedure: "parallel-reader minor revisions; explicit migration for changed authority semantics".to_string(),
            rotation_procedure: "register overlapping public key, canary exchange, then revoke prior key".to_string(),
            incident_procedure: "revoke identity or key through daemon administrator API and correlate application audit event IDs".to_string(),
            deprovisioning_procedure: "revoke identity, allow bounded token expiry, verify audit evidence, then remove public descriptors".to_string(),
        })
    }
}

impl ApplicationIdentityRegistrationResponse {
    pub fn accepted(
        job_id: DaemonJobId,
        accepted_at_utc: impl Into<String>,
        request: ApplicationIdentityRegistrationRequest,
        replaced: bool,
    ) -> Self {
        let registration = ApplicationRegistrationRecord::from_identity(&request.identity);
        Self {
            accepted: DaemonJobAcceptedResponse {
                job_id,
                kind: DaemonJobKind::SystemAdministration,
                accepted_at_utc: accepted_at_utc.into(),
                dry_run: request.dry_run,
            },
            identity: request.identity,
            replaced,
            administrator_actor: request.administrator_actor,
            registration,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApplicationIdentityRegistrationValidationError {
    InvalidIdentity(ApplicationAuthValidationError),
    BlankClientRequestId,
    BlankAdministratorActor,
    ConfirmationMismatch,
}

impl Display for ApplicationIdentityRegistrationValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentity(error) => write!(formatter, "invalid application identity: {error}"),
            Self::BlankClientRequestId => formatter.write_str("client_request_id must not be blank"),
            Self::BlankAdministratorActor => {
                formatter.write_str("administrator_actor must not be blank")
            }
            Self::ConfirmationMismatch => write!(
                formatter,
                "confirmation_marker must exactly match \"{APPLICATION_IDENTITY_REGISTRATION_CONFIRMATION}\""
            ),
        }
    }
}

impl std::error::Error for ApplicationIdentityRegistrationValidationError {}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationKeyRegistrationRequest {
    pub key: ApplicationKeyDescriptor,
    pub dry_run: bool,
    pub client_request_id: Option<String>,
    #[serde(default)]
    pub administrator_actor: Option<String>,
    pub confirmation_marker: String,
}

impl ApplicationKeyRegistrationRequest {
    pub fn validate(&self) -> Result<(), ApplicationKeyRegistrationValidationError> {
        self.key
            .validate()
            .map_err(ApplicationKeyRegistrationValidationError::InvalidKey)?;
        if self
            .client_request_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(ApplicationKeyRegistrationValidationError::BlankClientRequestId);
        }
        if self
            .administrator_actor
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(ApplicationKeyRegistrationValidationError::BlankAdministratorActor);
        }
        if self.confirmation_marker.trim() != APPLICATION_IDENTITY_REGISTRATION_CONFIRMATION {
            return Err(ApplicationKeyRegistrationValidationError::ConfirmationMismatch);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationKeyRegistrationResponse {
    pub accepted: DaemonJobAcceptedResponse,
    pub key: ApplicationKeyDescriptor,
    pub replaced: bool,
    pub administrator_actor: Option<String>,
}

impl ApplicationKeyRegistrationResponse {
    pub fn accepted(
        job_id: DaemonJobId,
        accepted_at_utc: impl Into<String>,
        request: ApplicationKeyRegistrationRequest,
        replaced: bool,
    ) -> Self {
        Self {
            accepted: DaemonJobAcceptedResponse {
                job_id,
                kind: DaemonJobKind::SystemAdministration,
                accepted_at_utc: accepted_at_utc.into(),
                dry_run: request.dry_run,
            },
            key: request.key,
            replaced,
            administrator_actor: request.administrator_actor,
        }
    }
}

/// Public, path-free revocation request.  `key_id` is omitted to revoke the
/// service principal and supplied to revoke one registered public key.  The
/// daemon performs the state mutation and records audit metadata; callers do
/// not submit private keys, tokens, or filesystem locations.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationCredentialRevocationRequest {
    pub application_id: String,
    #[serde(default)]
    pub key_id: Option<String>,
    pub reason: String,
    pub dry_run: bool,
    pub client_request_id: Option<String>,
    #[serde(default)]
    pub administrator_actor: Option<String>,
    pub confirmation_marker: String,
}

impl ApplicationCredentialRevocationRequest {
    pub fn validate(&self) -> Result<(), ApplicationCredentialRevocationValidationError> {
        if self.application_id.trim().is_empty() {
            return Err(ApplicationCredentialRevocationValidationError::BlankApplicationId);
        }
        if self
            .key_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(ApplicationCredentialRevocationValidationError::BlankKeyId);
        }
        if self.reason.trim().is_empty() {
            return Err(ApplicationCredentialRevocationValidationError::BlankReason);
        }
        if self
            .client_request_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(ApplicationCredentialRevocationValidationError::BlankClientRequestId);
        }
        if self
            .administrator_actor
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(ApplicationCredentialRevocationValidationError::BlankAdministratorActor);
        }
        if self.confirmation_marker.trim() != APPLICATION_CREDENTIAL_REVOCATION_CONFIRMATION {
            return Err(ApplicationCredentialRevocationValidationError::ConfirmationMismatch);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApplicationCredentialRevocationValidationError {
    BlankApplicationId,
    BlankKeyId,
    BlankReason,
    BlankClientRequestId,
    BlankAdministratorActor,
    ConfirmationMismatch,
}

impl Display for ApplicationCredentialRevocationValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BlankApplicationId => formatter.write_str("application_id must not be blank"),
            Self::BlankKeyId => formatter.write_str("key_id must not be blank when supplied"),
            Self::BlankReason => formatter.write_str("reason must not be blank"),
            Self::BlankClientRequestId => formatter.write_str("client_request_id must not be blank"),
            Self::BlankAdministratorActor => {
                formatter.write_str("administrator_actor must not be blank")
            }
            Self::ConfirmationMismatch => write!(
                formatter,
                "confirmation_marker must exactly match \"{APPLICATION_CREDENTIAL_REVOCATION_CONFIRMATION}\""
            ),
        }
    }
}

impl std::error::Error for ApplicationCredentialRevocationValidationError {}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationCredentialRevocationResponse {
    pub accepted: DaemonJobAcceptedResponse,
    pub application_id: String,
    pub key_id: Option<String>,
    pub revoked: bool,
    pub administrator_actor: Option<String>,
}

impl ApplicationCredentialRevocationResponse {
    pub fn accepted(
        job_id: DaemonJobId,
        accepted_at_utc: impl Into<String>,
        request: ApplicationCredentialRevocationRequest,
        revoked: bool,
    ) -> Self {
        Self {
            accepted: DaemonJobAcceptedResponse {
                job_id,
                kind: DaemonJobKind::SystemAdministration,
                accepted_at_utc: accepted_at_utc.into(),
                dry_run: request.dry_run,
            },
            application_id: request.application_id,
            key_id: request.key_id,
            revoked,
            administrator_actor: request.administrator_actor,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApplicationKeyRegistrationValidationError {
    InvalidKey(ApplicationAuthValidationError),
    BlankClientRequestId,
    BlankAdministratorActor,
    ConfirmationMismatch,
}

impl Display for ApplicationKeyRegistrationValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidKey(error) => write!(formatter, "invalid application key: {error}"),
            Self::BlankClientRequestId => formatter.write_str("client_request_id must not be blank"),
            Self::BlankAdministratorActor => {
                formatter.write_str("administrator_actor must not be blank")
            }
            Self::ConfirmationMismatch => write!(
                formatter,
                "confirmation_marker must exactly match \"{APPLICATION_IDENTITY_REGISTRATION_CONFIRMATION}\""
            ),
        }
    }
}

impl std::error::Error for ApplicationKeyRegistrationValidationError {}

#[cfg(test)]
mod tests {
    use super::{
        ApplicationCredentialRevocationRequest, ApplicationIdentityRegistrationRequest,
        ApplicationIdentityRegistrationResponse, ApplicationKeyRegistrationRequest,
        APPLICATION_CREDENTIAL_REVOCATION_CONFIRMATION,
        APPLICATION_IDENTITY_REGISTRATION_CONFIRMATION,
    };
    use crate::api::{DaemonJobId, DaemonJobKind};
    use dasobjectstore_core::application_auth::{
        ApplicationCredentialKind, ApplicationEnvironment, ApplicationIdentity,
        ApplicationKeyAlgorithm, ApplicationKeyDescriptor, ApplicationOperation, ApplicationScope,
        APPLICATION_AUTH_SCHEMA_VERSION,
    };
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_core::ingress::IngressOrigin;
    use dasobjectstore_core::object_type::ObjectType;

    fn request() -> ApplicationIdentityRegistrationRequest {
        ApplicationIdentityRegistrationRequest {
            identity: ApplicationIdentity {
                schema_version: APPLICATION_AUTH_SCHEMA_VERSION.to_string(),
                application_id: "synoptikon-ingest".to_string(),
                owner: "mnemosyne".to_string(),
                purpose: "sequencing ingest".to_string(),
                environment: ApplicationEnvironment::Production,
                credential_kind: ApplicationCredentialKind::AsymmetricKey,
                scope: ApplicationScope {
                    store_ids: vec![StoreId::new("codex").expect("store")],
                    prefixes: vec!["analysis".to_string()],
                    object_types: vec![ObjectType::Fastq],
                    operations: vec![ApplicationOperation::Write],
                    ingress_origin: IngressOrigin::Synoptikon,
                    max_object_bytes: Some(10_000),
                    max_total_bytes: Some(100_000),
                },
                dynamic_binding: None,
                issued_at_unix_seconds: 1_000,
                expires_at_unix_seconds: 100_000,
                active: true,
            },
            dry_run: false,
            client_request_id: Some("request-1".to_string()),
            administrator_actor: Some("root".to_string()),
            confirmation_marker: APPLICATION_IDENTITY_REGISTRATION_CONFIRMATION.to_string(),
        }
    }

    #[test]
    fn validates_public_registration_contract_without_secret_or_path_fields() {
        let request = request();
        request.validate().expect("valid request");
        let encoded = serde_json::to_string(&request).expect("encode");
        assert!(!encoded.contains("private_key"));
        assert!(!encoded.contains("/srv"));
    }

    #[test]
    fn administrator_identity_request_rejects_unknown_fields() {
        let mut encoded = serde_json::to_value(request()).expect("encode request");
        encoded["unexpected"] = serde_json::json!(true);
        assert!(serde_json::from_value::<ApplicationIdentityRegistrationRequest>(encoded).is_err());
    }

    #[test]
    fn response_preserves_replacement_and_job_authority() {
        let response = ApplicationIdentityRegistrationResponse::accepted(
            DaemonJobId::new("application-identity-1").expect("job id"),
            "2026-07-13T00:00:00Z",
            request(),
            true,
        );
        assert_eq!(response.accepted.kind, DaemonJobKind::SystemAdministration);
        assert!(response.replaced);
        assert_eq!(response.administrator_actor.as_deref(), Some("root"));
    }

    #[test]
    fn validates_public_key_registration_contract() {
        let request = ApplicationKeyRegistrationRequest {
            key: ApplicationKeyDescriptor {
                schema_version: APPLICATION_AUTH_SCHEMA_VERSION.to_string(),
                application_id: "synoptikon-ingest".to_string(),
                key_id: "key-1".to_string(),
                algorithm: ApplicationKeyAlgorithm::Ed25519,
                public_key_fingerprint: format!("sha256:{}", "a".repeat(64)),
                public_key_material: None,
                issued_at_unix_seconds: 1_000,
                expires_at_unix_seconds: 100_000,
                active: true,
            },
            dry_run: true,
            client_request_id: Some("key-request".to_string()),
            administrator_actor: None,
            confirmation_marker: APPLICATION_IDENTITY_REGISTRATION_CONFIRMATION.to_string(),
        };
        request.validate().expect("key request");
        let encoded = serde_json::to_string(&request).expect("encode");
        assert!(!encoded.contains("private_key"));
    }

    #[test]
    fn validates_scoped_revocation_without_secret_or_path_fields() {
        let request = ApplicationCredentialRevocationRequest {
            application_id: "synoptikon-ingest".to_string(),
            key_id: Some("key-1".to_string()),
            reason: "scheduled key rotation".to_string(),
            dry_run: true,
            client_request_id: Some("revoke-1".to_string()),
            administrator_actor: Some("root".to_string()),
            confirmation_marker: APPLICATION_CREDENTIAL_REVOCATION_CONFIRMATION.to_string(),
        };
        request.validate().expect("revocation request");
        let encoded = serde_json::to_string(&request).expect("encode");
        assert!(!encoded.contains("private_key"));
        assert!(!encoded.contains("/srv"));
    }

    #[test]
    fn revocation_requires_confirmation_and_nonblank_reason() {
        let mut request = ApplicationCredentialRevocationRequest {
            application_id: "synoptikon-ingest".to_string(),
            key_id: None,
            reason: " ".to_string(),
            dry_run: false,
            client_request_id: None,
            administrator_actor: None,
            confirmation_marker: APPLICATION_CREDENTIAL_REVOCATION_CONFIRMATION.to_string(),
        };
        assert!(matches!(
            request.validate(),
            Err(super::ApplicationCredentialRevocationValidationError::BlankReason)
        ));
        request.reason = "incident response".to_string();
        request.confirmation_marker = "wrong".to_string();
        assert!(matches!(
            request.validate(),
            Err(super::ApplicationCredentialRevocationValidationError::ConfirmationMismatch)
        ));
    }

    #[test]
    fn governed_registration_fixture_returns_non_secret_contract_evidence() {
        let request: ApplicationIdentityRegistrationRequest = serde_json::from_str(include_str!(
            "../../../../docs/user/examples/ergasterion-application-identity-registration.json"
        ))
        .expect("governed registration fixture");
        request.validate().expect("governed registration validates");
        let response = ApplicationIdentityRegistrationResponse::accepted(
            DaemonJobId::new("governed-registration").expect("job"),
            "2026-07-19T00:00:00Z",
            request,
            false,
        );
        let registration = response.registration.expect("registration evidence");
        assert_eq!(registration.application_id, "app-7e4a31c9b260");
        assert_eq!(registration.safe_denial_reason, "governed_scope_denied");
        assert_eq!(registration.max_capability_lifetime_seconds, 900);
        let encoded = serde_json::to_string(&registration).expect("serialize");
        for forbidden in ["private_key", "client_secret", "bearer_token", "/srv/"] {
            assert!(!encoded.contains(forbidden), "must omit {forbidden}");
        }
    }
}
