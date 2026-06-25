//! Provider-neutral object-service orchestration contracts.

use dasobjectstore_core::ids::StoreId;
use dasobjectstore_core::store::StorePolicy;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};
use std::str::FromStr;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ObjectServiceProviderId {
    Garage,
    Rustfs,
}

impl ObjectServiceProviderId {
    pub const ALL: [Self; 2] = [Self::Garage, Self::Rustfs];

    pub fn name(self) -> &'static str {
        match self {
            Self::Garage => "garage",
            Self::Rustfs => "rustfs",
        }
    }
}

impl Display for ObjectServiceProviderId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

impl FromStr for ObjectServiceProviderId {
    type Err = ObjectServiceProviderParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "garage" => Ok(Self::Garage),
            "rustfs" => Ok(Self::Rustfs),
            _ => Err(ObjectServiceProviderParseError {
                value: value.to_string(),
            }),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectServiceProviderParseError {
    value: String,
}

impl Display for ObjectServiceProviderParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "unknown object service provider `{}`; expected one of: garage, rustfs",
            self.value
        )
    }
}

impl std::error::Error for ObjectServiceProviderParseError {}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderDescriptor {
    pub id: ObjectServiceProviderId,
    pub display_name: String,
    pub s3_compatible: bool,
    pub docker_compose_supported: bool,
    pub native_service_supported: bool,
}

impl ProviderDescriptor {
    pub fn garage() -> Self {
        Self {
            id: ObjectServiceProviderId::Garage,
            display_name: "Garage".to_string(),
            s3_compatible: true,
            docker_compose_supported: true,
            native_service_supported: false,
        }
    }

    pub fn rustfs() -> Self {
        Self {
            id: ObjectServiceProviderId::Rustfs,
            display_name: "RustFS".to_string(),
            s3_compatible: true,
            docker_compose_supported: true,
            native_service_supported: false,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreBucketBinding {
    pub store_id: StoreId,
    pub policy: StorePolicy,
    pub bucket_name: String,
    pub credential_reference: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ComposeRenderRequest {
    pub project_name: String,
    pub ssd_metadata_path: String,
    pub hdd_data_path: String,
    pub store_bindings: Vec<StoreBucketBinding>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RenderedCompose {
    pub provider_id: ObjectServiceProviderId,
    pub compose_yaml: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ServiceState {
    Unknown,
    Stopped,
    Starting,
    Running,
    Degraded,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ServiceStatus {
    pub provider_id: ObjectServiceProviderId,
    pub state: ServiceState,
    pub endpoint: Option<String>,
    pub message: Option<String>,
}

pub trait ObjectServiceProvider {
    fn descriptor(&self) -> &ProviderDescriptor;

    fn render_compose(
        &self,
        request: &ComposeRenderRequest,
    ) -> Result<RenderedCompose, ObjectServiceError>;

    fn inspect_status(&self) -> Result<ServiceStatus, ObjectServiceError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObjectServiceError {
    UnsupportedProvider(ObjectServiceProviderId),
    InvalidConfiguration(String),
    CommandFailed(String),
}

impl Display for ObjectServiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedProvider(provider) => {
                write!(formatter, "unsupported object service provider: {provider}")
            }
            Self::InvalidConfiguration(message) => {
                write!(formatter, "invalid object service configuration: {message}")
            }
            Self::CommandFailed(message) => {
                write!(formatter, "object service command failed: {message}")
            }
        }
    }
}

impl std::error::Error for ObjectServiceError {}

#[cfg(test)]
mod tests {
    use super::{
        ComposeRenderRequest, ObjectServiceProviderId, ProviderDescriptor, RenderedCompose,
        ServiceState, ServiceStatus,
    };

    #[test]
    fn provider_ids_use_stable_names() {
        assert_eq!(ObjectServiceProviderId::Garage.name(), "garage");
        assert_eq!(ObjectServiceProviderId::Rustfs.name(), "rustfs");
        assert_eq!("garage".parse(), Ok(ObjectServiceProviderId::Garage));
        assert!("unknown".parse::<ObjectServiceProviderId>().is_err());
    }

    #[test]
    fn built_in_descriptors_capture_mvp_candidates() {
        let garage = ProviderDescriptor::garage();
        let rustfs = ProviderDescriptor::rustfs();

        assert_eq!(garage.id, ObjectServiceProviderId::Garage);
        assert_eq!(garage.display_name, "Garage");
        assert!(garage.s3_compatible);
        assert!(garage.docker_compose_supported);
        assert!(!garage.native_service_supported);
        assert_eq!(rustfs.id, ObjectServiceProviderId::Rustfs);
    }

    #[test]
    fn compose_request_round_trips_as_json() {
        let request = ComposeRenderRequest {
            project_name: "dasobjectstore-test".to_string(),
            ssd_metadata_path: "/ssd/meta".to_string(),
            hdd_data_path: "/hdd/data".to_string(),
            store_bindings: Vec::new(),
        };

        let encoded = serde_json::to_string(&request).expect("request serializes");
        let decoded: ComposeRenderRequest =
            serde_json::from_str(&encoded).expect("request deserializes");

        assert_eq!(decoded, request);
    }

    #[test]
    fn service_status_round_trips_as_json() {
        let status = ServiceStatus {
            provider_id: ObjectServiceProviderId::Garage,
            state: ServiceState::Running,
            endpoint: Some("http://127.0.0.1:3900".to_string()),
            message: None,
        };

        let encoded = serde_json::to_string(&status).expect("status serializes");
        let decoded: ServiceStatus = serde_json::from_str(&encoded).expect("status deserializes");

        assert_eq!(decoded, status);
    }

    #[test]
    fn rendered_compose_round_trips_as_json() {
        let rendered = RenderedCompose {
            provider_id: ObjectServiceProviderId::Rustfs,
            compose_yaml: "services: {}\n".to_string(),
        };

        let encoded = serde_json::to_string(&rendered).expect("rendered compose serializes");
        let decoded: RenderedCompose =
            serde_json::from_str(&encoded).expect("rendered compose deserializes");

        assert_eq!(decoded, rendered);
    }
}
