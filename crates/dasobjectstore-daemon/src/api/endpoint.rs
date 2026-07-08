use crate::api::{DaemonJobAcceptedResponse, DaemonJobId, DaemonJobKind};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

pub const ENDPOINT_RECORD_CONFIRMATION: &str = "record endpoint inventory";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UpsertEndpointInventoryRequest {
    pub endpoint_id: String,
    pub display_name: String,
    pub kind: DaemonEndpointKind,
    pub object_service_url: String,
    pub validation: DaemonEndpointValidation,
    #[serde(default = "default_manager_product_id")]
    pub manager_product_id: String,
    #[serde(default)]
    pub active_bindings: Vec<DaemonEndpointBinding>,
    pub dry_run: bool,
    pub client_request_id: Option<String>,
    pub administrator_actor: Option<String>,
    pub confirmation_marker: Option<String>,
}

impl UpsertEndpointInventoryRequest {
    pub fn validate(&self) -> Result<(), EndpointInventoryValidationError> {
        validate_local_name("endpoint_id", &self.endpoint_id)?;
        require_nonblank("display_name", &self.display_name)?;
        require_nonblank("object_service_url", &self.object_service_url)?;
        if !self.object_service_url.starts_with("http://")
            && !self.object_service_url.starts_with("https://")
        {
            return Err(EndpointInventoryValidationError::InvalidUrl {
                field: "object_service_url",
                value: self.object_service_url.clone(),
            });
        }
        validate_local_name("manager_product_id", &self.manager_product_id)?;
        validate_client_request_id(self.client_request_id.as_deref())?;
        if let Some(actor) = self.administrator_actor.as_deref() {
            validate_local_name("administrator_actor", actor)?;
        }
        self.validation.validate()?;
        for binding in &self.active_bindings {
            binding.validate()?;
        }
        if !self.dry_run
            && self.confirmation_marker.as_deref() != Some(ENDPOINT_RECORD_CONFIRMATION)
        {
            return Err(EndpointInventoryValidationError::ConfirmationMismatch);
        }

        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonEndpointKind {
    DasobjectstoreDas,
    DasobjectstoreNfs,
    S3Compatible,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonEndpointValidation {
    pub state: DaemonEndpointValidationState,
    pub checked_at_utc: Option<String>,
    pub message: Option<String>,
}

impl DaemonEndpointValidation {
    fn validate(&self) -> Result<(), EndpointInventoryValidationError> {
        if self
            .checked_at_utc
            .as_ref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(EndpointInventoryValidationError::BlankField {
                field: "validation.checked_at_utc",
            });
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonEndpointValidationState {
    Draft,
    PendingValidation,
    Validated,
    Degraded,
    Rejected,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonEndpointBinding {
    pub binding_id: String,
    pub governance_domain: String,
    pub store_id: String,
    pub readiness: DaemonEndpointBindingReadiness,
}

impl DaemonEndpointBinding {
    fn validate(&self) -> Result<(), EndpointInventoryValidationError> {
        validate_local_name("active_bindings.binding_id", &self.binding_id)?;
        validate_local_name("active_bindings.governance_domain", &self.governance_domain)?;
        validate_local_name("active_bindings.store_id", &self.store_id)?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonEndpointBindingReadiness {
    Ready,
    Degraded,
    Blocked,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UpsertEndpointInventoryResponse {
    pub accepted: DaemonJobAcceptedResponse,
    pub endpoint_id: String,
    pub display_name: String,
    pub kind: DaemonEndpointKind,
    pub validation_state: DaemonEndpointValidationState,
    pub registry_path: String,
    pub administrator_actor: Option<String>,
}

impl UpsertEndpointInventoryResponse {
    pub fn accepted(
        job_id: DaemonJobId,
        accepted_at_utc: impl Into<String>,
        registry_path: impl Into<String>,
        request: UpsertEndpointInventoryRequest,
    ) -> Self {
        Self {
            accepted: DaemonJobAcceptedResponse {
                job_id,
                kind: DaemonJobKind::EndpointValidation,
                accepted_at_utc: accepted_at_utc.into(),
                dry_run: request.dry_run,
            },
            endpoint_id: request.endpoint_id,
            display_name: request.display_name,
            kind: request.kind,
            validation_state: request.validation.state,
            registry_path: registry_path.into(),
            administrator_actor: request.administrator_actor,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EndpointInventoryValidationError {
    BlankField { field: &'static str },
    UnsafeLocalName { field: &'static str, value: String },
    InvalidUrl { field: &'static str, value: String },
    BlankClientRequestId,
    ConfirmationMismatch,
}

impl Display for EndpointInventoryValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BlankField { field } => write!(formatter, "{field} must not be blank"),
            Self::UnsafeLocalName { field, value } => {
                write!(formatter, "{field} has unsafe value `{value}`")
            }
            Self::InvalidUrl { field, value } => {
                write!(formatter, "{field} must be http(s): `{value}`")
            }
            Self::BlankClientRequestId => formatter.write_str("client_request_id cannot be blank"),
            Self::ConfirmationMismatch => write!(
                formatter,
                "confirmation_marker must be `{ENDPOINT_RECORD_CONFIRMATION}`"
            ),
        }
    }
}

impl std::error::Error for EndpointInventoryValidationError {}

fn require_nonblank(
    field: &'static str,
    value: &str,
) -> Result<(), EndpointInventoryValidationError> {
    if value.trim().is_empty() {
        Err(EndpointInventoryValidationError::BlankField { field })
    } else {
        Ok(())
    }
}

fn validate_local_name(
    field: &'static str,
    value: &str,
) -> Result<(), EndpointInventoryValidationError> {
    require_nonblank(field, value)?;
    if !value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.'))
    {
        return Err(EndpointInventoryValidationError::UnsafeLocalName {
            field,
            value: value.to_string(),
        });
    }
    Ok(())
}

fn validate_client_request_id(value: Option<&str>) -> Result<(), EndpointInventoryValidationError> {
    if value.is_some_and(|value| value.trim().is_empty()) {
        Err(EndpointInventoryValidationError::BlankClientRequestId)
    } else {
        Ok(())
    }
}

fn default_manager_product_id() -> String {
    "dasobjectstore".to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        DaemonEndpointKind, DaemonEndpointValidation, DaemonEndpointValidationState,
        EndpointInventoryValidationError, UpsertEndpointInventoryRequest,
        ENDPOINT_RECORD_CONFIRMATION,
    };

    #[test]
    fn validates_endpoint_inventory_upsert() {
        let request = valid_request();

        assert_eq!(request.validate(), Ok(()));
    }

    #[test]
    fn rejects_unconfirmed_endpoint_inventory_upsert() {
        let request = UpsertEndpointInventoryRequest {
            confirmation_marker: None,
            ..valid_request()
        };

        assert_eq!(
            request.validate(),
            Err(EndpointInventoryValidationError::ConfirmationMismatch)
        );
    }

    fn valid_request() -> UpsertEndpointInventoryRequest {
        UpsertEndpointInventoryRequest {
            endpoint_id: "endpoint-nfs".to_string(),
            display_name: "NAS staging".to_string(),
            kind: DaemonEndpointKind::DasobjectstoreNfs,
            object_service_url: "https://nas.example.test:9443".to_string(),
            validation: DaemonEndpointValidation {
                state: DaemonEndpointValidationState::Validated,
                checked_at_utc: Some("2026-07-09T00:00:00Z".to_string()),
                message: None,
            },
            manager_product_id: "dasobjectstore".to_string(),
            active_bindings: Vec::new(),
            dry_run: false,
            client_request_id: Some("request-1".to_string()),
            administrator_actor: Some("admin".to_string()),
            confirmation_marker: Some(ENDPOINT_RECORD_CONFIRMATION.to_string()),
        }
    }
}
