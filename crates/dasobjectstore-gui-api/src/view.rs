use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApiHealth {
    pub service: String,
    pub version: String,
    pub instance_id: String,
    pub status: ApiStatus,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApiLiveness {
    pub service: String,
    pub version: String,
    pub instance_id: String,
    pub status: ApiLivenessStatus,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiLivenessStatus {
    Ready,
}

impl ApiHealth {
    pub fn development(version: impl Into<String>) -> Self {
        Self {
            service: "dasobjectstore-gui-api".to_string(),
            version: version.into(),
            instance_id: api_instance_id().to_string(),
            status: ApiStatus::Development,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiStatus {
    Development,
}

pub fn api_health() -> ApiHealth {
    ApiHealth::development(dasobjectstore_core::VERSION)
}

pub fn api_liveness() -> ApiLiveness {
    ApiLiveness {
        service: "dasobjectstore-gui-api".to_string(),
        version: dasobjectstore_core::VERSION.to_string(),
        instance_id: api_instance_id().to_string(),
        status: ApiLivenessStatus::Ready,
    }
}

pub fn api_instance_id() -> &'static str {
    static INSTANCE_ID: OnceLock<String> = OnceLock::new();
    INSTANCE_ID.get_or_init(|| Uuid::new_v4().to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        api_health, api_instance_id, api_liveness, ApiHealth, ApiLivenessStatus, ApiStatus,
    };

    #[test]
    fn builds_development_health_view() {
        let health = api_health();

        assert_eq!(
            health,
            ApiHealth {
                service: "dasobjectstore-gui-api".to_string(),
                version: dasobjectstore_core::VERSION.to_string(),
                instance_id: api_instance_id().to_string(),
                status: ApiStatus::Development,
            }
        );
    }

    #[test]
    fn health_instance_id_is_stable_for_process_lifetime() {
        assert_eq!(api_instance_id(), api_health().instance_id);
        assert_eq!(api_health().instance_id, api_health().instance_id);
    }

    #[test]
    fn serializes_health_status_as_snake_case() {
        let encoded = serde_json::to_value(api_health()).expect("health serializes");

        assert_eq!(encoded["status"], "development");
        assert!(encoded["instance_id"].is_string());
    }

    #[test]
    fn liveness_is_ready_without_daemon_dependency() {
        let liveness = api_liveness();
        assert_eq!(liveness.status, ApiLivenessStatus::Ready);
        assert_eq!(liveness.instance_id, api_instance_id());
        assert_eq!(liveness.service, "dasobjectstore-gui-api");
    }
}
