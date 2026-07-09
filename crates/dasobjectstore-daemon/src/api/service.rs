use crate::api::{
    DaemonJobAcceptedResponse, DaemonJobId, DaemonJobKind, DaemonRequestValidationError,
};
use dasobjectstore_object_service::{ObjectServiceProviderId, ServiceState};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonServiceStatusRequest {
    pub include_detail: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonServiceStatusResponse {
    pub provider_id: ObjectServiceProviderId,
    pub state: ServiceState,
    pub endpoint: Option<String>,
    pub message: Option<String>,
    pub detail: Option<DaemonServiceStatusDetail>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonServiceStatusDetail {
    pub compose_project: String,
    pub service_name: String,
    pub config_path: String,
    pub metadata_path: String,
    pub data_path: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonServiceLifecycleRequest {
    pub operation: DaemonServiceOperation,
    pub provider_id: ObjectServiceProviderId,
    pub dry_run: bool,
    pub client_request_id: Option<String>,
}

impl DaemonServiceLifecycleRequest {
    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        if self.provider_id != ObjectServiceProviderId::Garage {
            return Err(DaemonRequestValidationError::UnsupportedServiceProvider {
                provider: self.provider_id.name().to_string(),
            });
        }
        if self
            .client_request_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(DaemonRequestValidationError::BlankClientRequestId);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonServiceProvisionRequest {
    pub provider_id: ObjectServiceProviderId,
    pub dry_run: bool,
    #[serde(default)]
    pub rotate_credentials: bool,
    pub client_request_id: Option<String>,
}

impl DaemonServiceProvisionRequest {
    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        if self.provider_id != ObjectServiceProviderId::Garage {
            return Err(DaemonRequestValidationError::UnsupportedServiceProvider {
                provider: self.provider_id.name().to_string(),
            });
        }
        if self
            .client_request_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(DaemonRequestValidationError::BlankClientRequestId);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonServiceOperation {
    Start,
    Stop,
    Restart,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonServiceProvisionResponse {
    pub accepted: DaemonJobAcceptedResponse,
    pub provider_id: ObjectServiceProviderId,
    pub registry_path: String,
    pub credential_registry_path: String,
    pub stores: usize,
    pub buckets: usize,
    pub commands: usize,
    pub credentials_issued: usize,
    pub credentials_reused: usize,
    pub credentials_rotated: usize,
}

impl DaemonServiceProvisionResponse {
    pub fn accepted(
        job_id: DaemonJobId,
        accepted_at_utc: impl Into<String>,
        dry_run: bool,
        provider_id: ObjectServiceProviderId,
        registry_path: impl Into<String>,
        credential_registry_path: impl Into<String>,
        stores: usize,
        buckets: usize,
        commands: usize,
        credentials_issued: usize,
        credentials_reused: usize,
        credentials_rotated: usize,
    ) -> Self {
        Self {
            accepted: DaemonJobAcceptedResponse {
                job_id,
                kind: DaemonJobKind::ServiceOperation,
                accepted_at_utc: accepted_at_utc.into(),
                dry_run,
            },
            provider_id,
            registry_path: registry_path.into(),
            credential_registry_path: credential_registry_path.into(),
            stores,
            buckets,
            commands,
            credentials_issued,
            credentials_reused,
            credentials_rotated,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonServiceLifecycleResponse {
    pub accepted: DaemonJobAcceptedResponse,
    pub operation: DaemonServiceOperation,
    pub provider_id: ObjectServiceProviderId,
}

impl DaemonServiceLifecycleResponse {
    pub fn accepted(
        job_id: DaemonJobId,
        accepted_at_utc: impl Into<String>,
        dry_run: bool,
        operation: DaemonServiceOperation,
        provider_id: ObjectServiceProviderId,
    ) -> Self {
        Self {
            accepted: DaemonJobAcceptedResponse {
                job_id,
                kind: DaemonJobKind::ServiceOperation,
                accepted_at_utc: accepted_at_utc.into(),
                dry_run,
            },
            operation,
            provider_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DaemonServiceLifecycleRequest, DaemonServiceLifecycleResponse, DaemonServiceOperation,
        DaemonServiceProvisionRequest, DaemonServiceProvisionResponse, DaemonServiceStatusDetail,
        DaemonServiceStatusResponse,
    };
    use crate::api::{DaemonJobId, DaemonJobKind, DaemonRequestValidationError};
    use dasobjectstore_object_service::{ObjectServiceProviderId, ServiceState};

    #[test]
    fn service_status_serializes_selected_provider() {
        let response = DaemonServiceStatusResponse {
            provider_id: ObjectServiceProviderId::Garage,
            state: ServiceState::Running,
            endpoint: Some("http://127.0.0.1:3900".to_string()),
            message: None,
            detail: Some(DaemonServiceStatusDetail {
                compose_project: "dasobjectstore".to_string(),
                service_name: "garage".to_string(),
                config_path: "/etc/dasobjectstore/garage.toml".to_string(),
                metadata_path: "/var/lib/dasobjectstore/garage/meta".to_string(),
                data_path: "/srv/dasobjectstore/hdd/garage".to_string(),
            }),
        };

        let encoded = serde_json::to_value(response).expect("status serializes");

        assert_eq!(encoded["provider_id"], "Garage");
        assert_eq!(encoded["state"], "Running");
        assert_eq!(encoded["detail"]["service_name"], "garage");
    }

    #[test]
    fn lifecycle_request_accepts_garage() {
        let request = DaemonServiceLifecycleRequest {
            operation: DaemonServiceOperation::Start,
            provider_id: ObjectServiceProviderId::Garage,
            dry_run: false,
            client_request_id: Some("request-1".to_string()),
        };

        request.validate().expect("Garage lifecycle is valid");
    }

    #[test]
    fn lifecycle_request_rejects_non_selected_provider() {
        let request = DaemonServiceLifecycleRequest {
            operation: DaemonServiceOperation::Start,
            provider_id: ObjectServiceProviderId::Rustfs,
            dry_run: false,
            client_request_id: None,
        };

        let err = request.validate().expect_err("RustFS is not selected");

        assert_eq!(
            err,
            DaemonRequestValidationError::UnsupportedServiceProvider {
                provider: "rustfs".to_string(),
            }
        );
    }

    #[test]
    fn provision_request_rejects_non_selected_provider() {
        let request = DaemonServiceProvisionRequest {
            provider_id: ObjectServiceProviderId::Rustfs,
            dry_run: false,
            rotate_credentials: false,
            client_request_id: None,
        };

        let err = request.validate().expect_err("RustFS is not selected");

        assert_eq!(
            err,
            DaemonRequestValidationError::UnsupportedServiceProvider {
                provider: "rustfs".to_string(),
            }
        );
    }

    #[test]
    fn lifecycle_response_uses_service_operation_job_kind() {
        let response = DaemonServiceLifecycleResponse::accepted(
            DaemonJobId::new("service-1").expect("job id"),
            "2026-07-07T11:38:12Z",
            true,
            DaemonServiceOperation::Restart,
            ObjectServiceProviderId::Garage,
        );

        assert_eq!(response.accepted.kind, DaemonJobKind::ServiceOperation);
        assert!(response.accepted.dry_run);
    }

    #[test]
    fn provision_response_uses_service_operation_job_kind() {
        let response = DaemonServiceProvisionResponse::accepted(
            DaemonJobId::new("service-provision-1").expect("job id"),
            "2026-07-07T12:05:42Z",
            true,
            ObjectServiceProviderId::Garage,
            "/etc/dasobjectstore/stores.json",
            "/var/lib/dasobjectstore/object-service/garage-credentials.json",
            2,
            2,
            6,
            1,
            1,
            0,
        );

        assert_eq!(response.accepted.kind, DaemonJobKind::ServiceOperation);
        assert_eq!(response.buckets, 2);
        assert_eq!(response.commands, 6);
        assert_eq!(response.credentials_issued, 1);
        assert_eq!(response.credentials_reused, 1);
        assert_eq!(response.credentials_rotated, 0);
        assert!(response.accepted.dry_run);
    }
}
