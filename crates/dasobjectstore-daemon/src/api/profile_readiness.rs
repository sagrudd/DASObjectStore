use super::{CapacityStatusResponse, ProfileInspectionRootState};
use dasobjectstore_core::deployment::{DeploymentProfile, HostMode};
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_core::protection::ProtectionPolicy;
use serde::{Deserialize, Serialize};

pub const PROFILE_READINESS_SCHEMA_VERSION: &str = "dasobjectstore.profile_readiness.v1";
pub const PROFILE_READINESS_ROUTE: &str = "/api/v1/profile-readiness/stores/{store_id}";

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileLifecycleState {
    #[default]
    Active,
    Retiring,
    Retired,
    Recovering,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileReadinessRequest {
    pub store_id: StoreId,
}

impl ProfileReadinessRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.store_id.as_str().trim().is_empty() {
            return Err("store_id must not be blank".to_string());
        }
        Ok(())
    }
}

/// Read-only runtime readiness for one registered folder/drive profile.
/// Paths and provider credentials never cross this boundary.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileReadinessResponse {
    pub schema_version: String,
    pub store_id: StoreId,
    pub deployment_profile: DeploymentProfile,
    pub host_mode: HostMode,
    pub protection: ProtectionPolicy,
    #[serde(default)]
    pub lifecycle_state: ProfileLifecycleState,
    pub root_state: ProfileInspectionRootState,
    pub ready: bool,
    pub reasons: Vec<String>,
    pub capacity: Option<CapacityStatusResponse>,
}

impl ProfileReadinessResponse {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != PROFILE_READINESS_SCHEMA_VERSION {
            return Err("unsupported profile readiness schema".to_string());
        }
        if self.store_id.as_str().trim().is_empty() {
            return Err("store_id must not be blank".to_string());
        }
        if self.ready && !self.reasons.is_empty() {
            return Err("ready profile readiness cannot contain reasons".to_string());
        }
        if self.ready && self.lifecycle_state != ProfileLifecycleState::Active {
            return Err("only active profiles can be ready".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_is_path_free_and_requires_consistent_ready_state() {
        let response = ProfileReadinessResponse {
            schema_version: PROFILE_READINESS_SCHEMA_VERSION.to_string(),
            store_id: StoreId::new("codex").expect("store"),
            deployment_profile: DeploymentProfile::Folder,
            host_mode: HostMode::PerUser,
            protection: ProtectionPolicy::LocalOnly,
            lifecycle_state: ProfileLifecycleState::Active,
            root_state: ProfileInspectionRootState::Available,
            ready: false,
            reasons: vec!["capacity unavailable".to_string()],
            capacity: None,
        };
        let encoded = serde_json::to_string(&response).expect("serialize");
        assert!(!encoded.contains("backend_root"));
        serde_json::from_str::<ProfileReadinessResponse>(&encoded)
            .expect("decode")
            .validate()
            .expect("valid response");
    }

    #[test]
    fn readiness_route_is_stable_and_store_scoped() {
        assert_eq!(
            PROFILE_READINESS_ROUTE,
            "/api/v1/profile-readiness/stores/{store_id}"
        );
        assert!(PROFILE_READINESS_ROUTE.contains("{store_id}"));
    }

    #[test]
    fn legacy_readiness_without_lifecycle_defaults_to_active() {
        let payload = serde_json::json!({
            "schema_version": PROFILE_READINESS_SCHEMA_VERSION,
            "store_id": "codex",
            "deployment_profile": "folder",
            "host_mode": "per_user",
            "protection": "local_only",
            "root_state": "available",
            "ready": true,
            "reasons": [],
            "capacity": null
        });
        let response: ProfileReadinessResponse =
            serde_json::from_value(payload).expect("legacy readiness");
        assert_eq!(response.lifecycle_state, ProfileLifecycleState::Active);
        response.validate().expect("legacy response validates");
    }
}
