use dasobjectstore_core::deployment::{DeploymentProfile, HostMode};
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_core::protection::ProtectionPolicy;
use serde::{Deserialize, Serialize};

pub const PROFILE_INSPECTION_SCHEMA_VERSION: &str = "dasobjectstore.profile_inspection.v1";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileInspectionRequest {
    pub store_id: StoreId,
}

impl ProfileInspectionRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.store_id.as_str().trim().is_empty() {
            return Err("store_id must not be blank".to_string());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileInspectionRootState {
    Available,
    Missing,
    NotDirectory,
    Unreadable,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileInspectionResponse {
    pub schema_version: String,
    pub store_id: StoreId,
    pub deployment_profile: DeploymentProfile,
    pub host_mode: HostMode,
    pub protection: ProtectionPolicy,
    pub root_state: ProfileInspectionRootState,
    pub unmanaged_path_count: usize,
    pub unsafe_path_count: usize,
    pub warnings: Vec<String>,
}

impl ProfileInspectionResponse {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != PROFILE_INSPECTION_SCHEMA_VERSION {
            return Err("unsupported profile inspection schema".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_and_response_round_trip_without_paths() {
        let request = ProfileInspectionRequest {
            store_id: StoreId::new("codex").expect("store id"),
        };
        let response = ProfileInspectionResponse {
            schema_version: PROFILE_INSPECTION_SCHEMA_VERSION.to_string(),
            store_id: request.store_id.clone(),
            deployment_profile: DeploymentProfile::Folder,
            host_mode: HostMode::PerUser,
            protection: ProtectionPolicy::LocalOnly,
            root_state: ProfileInspectionRootState::Available,
            unmanaged_path_count: 2,
            unsafe_path_count: 1,
            warnings: vec![],
        };
        let json = serde_json::to_string(&response).expect("serialize");
        assert!(!json.contains("backend_root"));
        assert!(!json.contains("staging"));
        let decoded: ProfileInspectionResponse = serde_json::from_str(&json).expect("decode");
        assert_eq!(decoded, response);
        decoded.validate().expect("schema");
    }
}
