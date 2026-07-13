use super::{CapacityStatusResponse, ProfileInspectionRootState};
use dasobjectstore_core::deployment::{DeploymentProfile, HostMode};
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_core::protection::ProtectionPolicy;
use serde::{Deserialize, Serialize};

pub const PROFILE_READINESS_SCHEMA_VERSION: &str = "dasobjectstore.profile_readiness.v1";

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
}
