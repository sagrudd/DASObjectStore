use super::{CapacityStatusResponse, ProfileInspectionRootState};
use dasobjectstore_core::deployment::{DeploymentProfile, HostMode};
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_core::protection::ProtectionPolicy;
use serde::{Deserialize, Serialize};

pub const PROFILE_READINESS_SCHEMA_VERSION: &str = "dasobjectstore.profile_readiness.v1";
pub const PROFILE_READINESS_ROUTE: &str = "/api/v1/profile-readiness/stores/{store_id}";
/// Public producer declaration consumed by the limited Monas-to-Phoreus
/// forwarding profile. It is deliberately a readiness binding, not a storage
/// credential, backend-root disclosure, work-admission capability, or package
/// qualification statement.
pub const PHOREUS_LIMITED_PROFILE_BINDING_CONTRACT: &str =
    "dasobjectstore.phoreus-limited-profile-binding.v1";
pub const PHOREUS_LIMITED_PROFILE_BINDING_VERSION: &str = "1.0.0";

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

/// Reject any declaration other than a compatible v1 limited-binding producer
/// contract. This keeps consumers from upgrading a path-free readiness result
/// into a claim from a substituted or future-major interface.
pub fn validate_phoreus_limited_profile_binding_contract(
    contract_id: &str,
    contract_version: &str,
) -> Result<(), &'static str> {
    if contract_id != PHOREUS_LIMITED_PROFILE_BINDING_CONTRACT {
        return Err("unsupported Phoreus limited-profile binding contract");
    }
    if !phoreus_limited_profile_binding_version_is_compatible(contract_version) {
        return Err("unsupported Phoreus limited-profile binding contract version");
    }
    Ok(())
}

fn phoreus_limited_profile_binding_version_is_compatible(value: &str) -> bool {
    let mut parts = value.split('.');
    let Some(major) = parts.next().and_then(|part| part.parse::<u64>().ok()) else {
        return false;
    };
    let Some(_minor) = parts.next().and_then(|part| part.parse::<u64>().ok()) else {
        return false;
    };
    let Some(_patch) = parts.next().and_then(|part| part.parse::<u64>().ok()) else {
        return false;
    };
    parts.next().is_none() && major == 1
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

    #[test]
    fn limited_phoreus_binding_rejects_substitution_and_incompatible_versions() {
        validate_phoreus_limited_profile_binding_contract(
            PHOREUS_LIMITED_PROFILE_BINDING_CONTRACT,
            PHOREUS_LIMITED_PROFILE_BINDING_VERSION,
        )
        .expect("current v1 declaration");
        validate_phoreus_limited_profile_binding_contract(
            PHOREUS_LIMITED_PROFILE_BINDING_CONTRACT,
            "1.99.99",
        )
        .expect("compatible v1 declaration");
        assert!(validate_phoreus_limited_profile_binding_contract(
            "dasobjectstore.profile_binding_registry.v1",
            PHOREUS_LIMITED_PROFILE_BINDING_VERSION,
        )
        .is_err());
        assert!(validate_phoreus_limited_profile_binding_contract(
            PHOREUS_LIMITED_PROFILE_BINDING_CONTRACT,
            "0.9.9",
        )
        .is_err());
        assert!(validate_phoreus_limited_profile_binding_contract(
            PHOREUS_LIMITED_PROFILE_BINDING_CONTRACT,
            "2.0.0",
        )
        .is_err());
    }

    #[test]
    fn published_limited_profile_declaration_matches_the_api_producer() {
        let declaration: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../docs/contracts/phoreus-limited-profile-binding-v1.json"
        ))
        .expect("valid public declaration");
        assert_eq!(
            declaration["schema_version"],
            PHOREUS_LIMITED_PROFILE_BINDING_CONTRACT
        );
        assert_eq!(
            declaration["contract_version"],
            PHOREUS_LIMITED_PROFILE_BINDING_VERSION
        );
        assert_eq!(
            declaration["producer"]["compatible_package_range"],
            ">=0.177.1,<0.179.0"
        );
        assert_eq!(env!("CARGO_PKG_VERSION"), "0.178.0");
        assert_eq!(
            declaration["readiness_evidence"]["schema_version"],
            PROFILE_READINESS_SCHEMA_VERSION
        );
        assert!(declaration["excluded_surfaces"]
            .as_array()
            .expect("excluded surfaces")
            .iter()
            .any(|value| value == "full_phoreus_monolith"));
    }
}
