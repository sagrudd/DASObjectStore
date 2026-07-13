use crate::api::{DaemonRequestValidationError, ObjectBrowserDelegatedActor};
use dasobjectstore_core::backend::BackendObjectKey;
use dasobjectstore_core::deployment::DeploymentProfile;
use dasobjectstore_core::ids::StoreId;
use serde::{Deserialize, Serialize};

pub const PROFILE_BROWSER_SCHEMA_VERSION: &str = "dasobjectstore.profile_browser.v1";
pub const PROFILE_BROWSER_MAX_PAGE_LIMIT: u16 = 500;

/// Read-only browser query for profile-owned catalogues. Unlike the appliance
/// browser this contract deliberately has no placement, lifecycle, or object
/// type fields: those values are not authoritative for a folder profile.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileBrowserRequest {
    pub store_id: StoreId,
    pub prefix: Option<String>,
    pub search: Option<String>,
    #[serde(default)]
    pub offset: u64,
    #[serde(default = "default_profile_browser_limit")]
    pub limit: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegated_actor: Option<ObjectBrowserDelegatedActor>,
}

impl ProfileBrowserRequest {
    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        if self.store_id.as_str().trim().is_empty() {
            return Err(DaemonRequestValidationError::BlankField { field: "store_id" });
        }
        if self
            .prefix
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(DaemonRequestValidationError::BlankField { field: "prefix" });
        }
        if self
            .search
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(DaemonRequestValidationError::BlankField { field: "search" });
        }
        if self.limit == 0 || self.limit > PROFILE_BROWSER_MAX_PAGE_LIMIT {
            return Err(DaemonRequestValidationError::UnsupportedFieldValue {
                field: "limit",
                value: self.limit.to_string(),
            });
        }
        if let Some(actor) = &self.delegated_actor {
            actor.validate()?;
        }
        Ok(())
    }
}

fn default_profile_browser_limit() -> u16 {
    100
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileBrowserResponse {
    pub schema_version: String,
    pub store_id: StoreId,
    pub profile: DeploymentProfile,
    pub entries: Vec<ProfileBrowserEntry>,
    pub next_offset: Option<u64>,
    pub total_entries: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileBrowserEntry {
    pub key: BackendObjectKey,
    pub size_bytes: u64,
    pub checksum: String,
}

impl ProfileBrowserResponse {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != PROFILE_BROWSER_SCHEMA_VERSION {
            return Err("unsupported profile browser schema".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_rejects_unbounded_pages_and_blank_filters() {
        let mut request = ProfileBrowserRequest {
            store_id: StoreId::new("codex").expect("store id"),
            prefix: Some(" ".to_string()),
            search: None,
            offset: 0,
            limit: 100,
            delegated_actor: None,
        };
        assert!(matches!(
            request.validate(),
            Err(DaemonRequestValidationError::BlankField { field: "prefix" })
        ));
        request.prefix = None;
        request.limit = PROFILE_BROWSER_MAX_PAGE_LIMIT + 1;
        assert!(matches!(
            request.validate(),
            Err(DaemonRequestValidationError::UnsupportedFieldValue { field: "limit", .. })
        ));
    }

    #[test]
    fn response_is_versioned_and_does_not_carry_backend_location() {
        let response = ProfileBrowserResponse {
            schema_version: PROFILE_BROWSER_SCHEMA_VERSION.to_string(),
            store_id: StoreId::new("codex").expect("store id"),
            profile: DeploymentProfile::Folder,
            entries: vec![ProfileBrowserEntry {
                key: BackendObjectKey {
                    object_id: "reads/sample.fastq".to_string(),
                    version: 1,
                },
                size_bytes: 12,
                checksum: "abc".to_string(),
            }],
            next_offset: None,
            total_entries: 1,
        };
        let json = serde_json::to_string(&response).expect("response serializes");
        assert!(!json.contains("backend_root"));
        assert!(!json.contains("location"));
        response.validate().expect("schema validates");
    }
}
