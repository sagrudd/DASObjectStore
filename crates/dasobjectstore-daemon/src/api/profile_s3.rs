use crate::api::DaemonRequestValidationError;
use dasobjectstore_core::backend::BackendObjectKey;
use dasobjectstore_core::ids::StoreId;
use serde::{Deserialize, Serialize};

pub const PROFILE_S3_SCHEMA_VERSION: &str = "dasobjectstore.profile_s3.v1";
pub const PROFILE_S3_MAX_KEYS: u16 = 1_000;

/// Bounded catalogue query for the future profile-S3/Web transport. The
/// request contains logical identity only; backend roots and provider
/// credentials remain daemon-owned.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileS3ListRequest {
    pub store_id: StoreId,
    pub prefix: Option<String>,
    #[serde(default)]
    pub offset: u64,
    #[serde(default = "default_profile_s3_limit")]
    pub limit: u16,
}

impl ProfileS3ListRequest {
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
        if self.limit == 0 || self.limit > PROFILE_S3_MAX_KEYS {
            return Err(DaemonRequestValidationError::UnsupportedFieldValue {
                field: "limit",
                value: self.limit.to_string(),
            });
        }
        Ok(())
    }
}

fn default_profile_s3_limit() -> u16 {
    100
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileS3ListResponse {
    pub schema_version: String,
    pub store_id: StoreId,
    pub objects: Vec<ProfileS3ObjectView>,
    pub next_offset: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileS3ObjectView {
    pub key: BackendObjectKey,
    pub size_bytes: u64,
    pub checksum: String,
}

impl ProfileS3ListResponse {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != PROFILE_S3_SCHEMA_VERSION {
            return Err("unsupported profile S3 schema".to_string());
        }
        if self.objects.len() > PROFILE_S3_MAX_KEYS as usize {
            return Err("profile S3 response exceeds maximum key count".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_request_rejects_blank_filters_and_unbounded_pages() {
        let mut request = ProfileS3ListRequest {
            store_id: StoreId::new("codex").expect("store id"),
            prefix: Some(" ".to_string()),
            offset: 0,
            limit: 100,
        };
        assert!(matches!(
            request.validate(),
            Err(DaemonRequestValidationError::BlankField { field: "prefix" })
        ));
        request.prefix = None;
        request.limit = PROFILE_S3_MAX_KEYS + 1;
        assert!(matches!(
            request.validate(),
            Err(DaemonRequestValidationError::UnsupportedFieldValue { field: "limit", .. })
        ));
    }

    #[test]
    fn list_response_is_versioned_and_path_free() {
        let response = ProfileS3ListResponse {
            schema_version: PROFILE_S3_SCHEMA_VERSION.to_string(),
            store_id: StoreId::new("codex").expect("store id"),
            objects: vec![ProfileS3ObjectView {
                key: BackendObjectKey {
                    object_id: "reads/sample.fastq".to_string(),
                    version: 1,
                },
                size_bytes: 12,
                checksum: "sha256:abc".to_string(),
            }],
            next_offset: Some(1),
        };
        let json = serde_json::to_string(&response).expect("response serializes");
        assert!(!json.contains("backend_root"));
        assert!(!json.contains("location"));
        response.validate().expect("schema validates");
    }
}
