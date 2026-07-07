use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApiHealth {
    pub service: String,
    pub version: String,
    pub status: ApiStatus,
}

impl ApiHealth {
    pub fn development(version: impl Into<String>) -> Self {
        Self {
            service: "dasobjectstore-gui-api".to_string(),
            version: version.into(),
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

#[cfg(test)]
mod tests {
    use super::{api_health, ApiHealth, ApiStatus};

    #[test]
    fn builds_development_health_view() {
        let health = api_health();

        assert_eq!(
            health,
            ApiHealth {
                service: "dasobjectstore-gui-api".to_string(),
                version: dasobjectstore_core::VERSION.to_string(),
                status: ApiStatus::Development,
            }
        );
    }

    #[test]
    fn serializes_health_status_as_snake_case() {
        let encoded = serde_json::to_value(api_health()).expect("health serializes");

        assert_eq!(encoded["status"], "development");
    }
}
