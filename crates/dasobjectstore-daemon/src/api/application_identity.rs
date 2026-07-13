use crate::api::{DaemonJobAcceptedResponse, DaemonJobId, DaemonJobKind};
use dasobjectstore_core::application_auth::{
    ApplicationAuthValidationError, ApplicationIdentity, ApplicationKeyDescriptor,
};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

pub const APPLICATION_IDENTITY_REGISTRATION_CONFIRMATION: &str =
    "confirm application identity registration";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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
pub struct ApplicationIdentityRegistrationResponse {
    pub accepted: DaemonJobAcceptedResponse,
    pub identity: ApplicationIdentity,
    pub replaced: bool,
    pub administrator_actor: Option<String>,
}

impl ApplicationIdentityRegistrationResponse {
    pub fn accepted(
        job_id: DaemonJobId,
        accepted_at_utc: impl Into<String>,
        request: ApplicationIdentityRegistrationRequest,
        replaced: bool,
    ) -> Self {
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
        ApplicationIdentityRegistrationRequest, ApplicationIdentityRegistrationResponse,
        ApplicationKeyRegistrationRequest, APPLICATION_IDENTITY_REGISTRATION_CONFIRMATION,
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
}
