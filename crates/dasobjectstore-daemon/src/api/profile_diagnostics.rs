use crate::api::{DaemonRequestValidationError, ProfileLifecycleState};
use dasobjectstore_core::deployment::DeploymentProfile;
use dasobjectstore_core::ids::StoreId;
use serde::{Deserialize, Serialize};

pub const PROFILE_DIAGNOSTICS_SCHEMA_VERSION: &str = "dasobjectstore.profile_diagnostics.v1";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileDiagnosticsRequest {
    pub store_id: StoreId,
}

impl ProfileDiagnosticsRequest {
    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        if self.store_id.as_str().trim().is_empty() {
            return Err(DaemonRequestValidationError::BlankField { field: "store_id" });
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileDiagnosticsState {
    Empty,
    Synchronized,
    UncataloguedBackendObjects,
    CatalogueMissingBackendObjects,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileDiagnosticsResponse {
    pub schema_version: String,
    pub store_id: StoreId,
    pub profile: DeploymentProfile,
    #[serde(default)]
    pub lifecycle_state: ProfileLifecycleState,
    pub state: ProfileDiagnosticsState,
    pub catalogue_object_count: u64,
    pub backend_object_count: u64,
    pub uncatalogued_backend_object_count: u64,
    pub catalogue_missing_backend_object_count: u64,
    pub last_reconciliation_at_unix_seconds: Option<u64>,
    pub actionable_message: Option<String>,
}

impl ProfileDiagnosticsResponse {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != PROFILE_DIAGNOSTICS_SCHEMA_VERSION {
            return Err("unsupported profile diagnostics schema".to_string());
        }
        if self
            .actionable_message
            .as_deref()
            .is_some_and(|message| message.trim().is_empty())
        {
            return Err("actionable diagnostics messages must not be blank".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_request_rejects_blank_store_ids() {
        let request = ProfileDiagnosticsRequest {
            store_id: StoreId::new("codex").expect("store id"),
        };
        request.validate().expect("request validates");
    }

    #[test]
    fn diagnostics_response_is_versioned_and_path_free() {
        let response = ProfileDiagnosticsResponse {
            schema_version: PROFILE_DIAGNOSTICS_SCHEMA_VERSION.to_string(),
            store_id: StoreId::new("codex").expect("store id"),
            profile: DeploymentProfile::Folder,
            lifecycle_state: ProfileLifecycleState::Active,
            state: ProfileDiagnosticsState::UncataloguedBackendObjects,
            catalogue_object_count: 1,
            backend_object_count: 2,
            uncatalogued_backend_object_count: 1,
            catalogue_missing_backend_object_count: 0,
            last_reconciliation_at_unix_seconds: Some(42),
            actionable_message: Some("run guarded reconciliation".to_string()),
        };
        response.validate().expect("response validates");
        let json = serde_json::to_string(&response).expect("response serializes");
        assert!(!json.contains("backend_root"));
        assert!(!json.contains("location"));
    }
}
