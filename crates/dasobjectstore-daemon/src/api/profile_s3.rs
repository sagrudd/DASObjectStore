use crate::api::DaemonRequestValidationError;
use dasobjectstore_core::backend::BackendObjectKey;
use dasobjectstore_core::ids::StoreId;
use serde::{Deserialize, Serialize};

pub const PROFILE_S3_SCHEMA_VERSION: &str = "dasobjectstore.profile_s3.v1";
pub const PROFILE_S3_MAX_KEYS: u16 = 1_000;
pub const PROFILE_S3_MAX_MULTIPART_PARTS: usize = 10_000;

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

/// Versioned, path-free multipart completion request for the future HTTP
/// gateway.  The daemon resolves `store_id` and `reservation_id`; callers
/// never provide backend roots, provider credentials, or staging locations.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileS3MultipartCompletionRequest {
    pub store_id: StoreId,
    pub reservation_id: String,
    pub key: BackendObjectKey,
    pub expected_size_bytes: u64,
    pub parts: Vec<ProfileS3MultipartPartRequest>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileS3MultipartPartRequest {
    pub part_number: u32,
    pub size_bytes: u64,
    pub checksum: String,
}

impl ProfileS3MultipartCompletionRequest {
    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        if self.store_id.as_str().trim().is_empty() {
            return Err(DaemonRequestValidationError::BlankField { field: "store_id" });
        }
        if self.reservation_id.trim().is_empty() {
            return Err(DaemonRequestValidationError::BlankField {
                field: "reservation_id",
            });
        }
        if self.key.object_id.trim().is_empty() || self.key.object_id.starts_with('/') {
            return Err(DaemonRequestValidationError::UnsupportedFieldValue {
                field: "key",
                value: self.key.object_id.clone(),
            });
        }
        if self.expected_size_bytes == 0 || self.parts.is_empty() {
            return Err(DaemonRequestValidationError::InvalidPolicy {
                message: "multipart completion requires a non-empty object and parts".to_string(),
            });
        }
        if self.parts.len() > PROFILE_S3_MAX_MULTIPART_PARTS {
            return Err(DaemonRequestValidationError::UnsupportedFieldValue {
                field: "parts",
                value: self.parts.len().to_string(),
            });
        }
        let mut previous = 0_u32;
        let total = self.parts.iter().try_fold(0_u64, |total, part| {
            if part.part_number == 0 || part.part_number <= previous {
                return Err(DaemonRequestValidationError::InvalidPolicy {
                    message: "multipart parts must be strictly ordered and unique".to_string(),
                });
            }
            previous = part.part_number;
            if part.size_bytes == 0 || !is_sha256_checksum(&part.checksum) {
                return Err(DaemonRequestValidationError::InvalidPolicy {
                    message: "multipart parts require a non-zero size and sha256 checksum"
                        .to_string(),
                });
            }
            total.checked_add(part.size_bytes).ok_or_else(|| {
                DaemonRequestValidationError::InvalidPolicy {
                    message: "multipart size overflow".to_string(),
                }
            })
        })?;
        if total != self.expected_size_bytes {
            return Err(DaemonRequestValidationError::InvalidPolicy {
                message: format!(
                    "multipart part total {total} does not match expected size {}",
                    self.expected_size_bytes
                ),
            });
        }
        Ok(())
    }
}

/// HTTP completion acknowledgement. `committed` is true only after the
/// daemon has durably finalized the staged object and committed its catalogue
/// record; a gateway must not infer storage state from provider responses.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileS3MultipartCompletionResponse {
    pub schema_version: String,
    pub store_id: StoreId,
    pub reservation_id: String,
    pub key: BackendObjectKey,
    pub committed: bool,
}

impl ProfileS3MultipartCompletionResponse {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != PROFILE_S3_SCHEMA_VERSION {
            return Err("unsupported profile S3 schema".to_string());
        }
        if self.store_id.as_str().trim().is_empty() || self.reservation_id.trim().is_empty() {
            return Err("multipart completion response identity must not be blank".to_string());
        }
        if self.key.object_id.trim().is_empty() || self.key.object_id.starts_with('/') {
            return Err("multipart completion response key must be relative".to_string());
        }
        Ok(())
    }
}

fn is_sha256_checksum(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
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

    #[test]
    fn multipart_request_matches_runtime_validation_and_is_path_free() {
        let request = ProfileS3MultipartCompletionRequest {
            store_id: StoreId::new("codex").expect("store id"),
            reservation_id: "reservation-1".to_string(),
            key: BackendObjectKey {
                object_id: "writes/multipart.fastq".to_string(),
                version: 1,
            },
            expected_size_bytes: 3,
            parts: vec![ProfileS3MultipartPartRequest {
                part_number: 1,
                size_bytes: 3,
                checksum: format!("sha256:{}", "a".repeat(64)),
            }],
        };
        request.validate().expect("request validates");
        let json = serde_json::to_string(&request).expect("request serializes");
        assert!(!json.contains("backend_root"));
        assert!(!json.contains("credentials"));
        assert!(!json.contains("staging"));
    }

    #[test]
    fn multipart_request_rejects_reordering_checksum_and_size_drift() {
        let mut request = ProfileS3MultipartCompletionRequest {
            store_id: StoreId::new("codex").expect("store id"),
            reservation_id: "reservation-1".to_string(),
            key: BackendObjectKey {
                object_id: "writes/multipart.fastq".to_string(),
                version: 1,
            },
            expected_size_bytes: 3,
            parts: vec![
                ProfileS3MultipartPartRequest {
                    part_number: 2,
                    size_bytes: 1,
                    checksum: format!("sha256:{}", "a".repeat(64)),
                },
                ProfileS3MultipartPartRequest {
                    part_number: 1,
                    size_bytes: 2,
                    checksum: format!("sha256:{}", "b".repeat(64)),
                },
            ],
        };
        assert!(matches!(
            request.validate(),
            Err(DaemonRequestValidationError::InvalidPolicy { .. })
        ));
        request.parts[0].part_number = 1;
        request.parts[1].part_number = 2;
        request.parts[1].checksum = "sha256:not-hex".to_string();
        assert!(matches!(
            request.validate(),
            Err(DaemonRequestValidationError::InvalidPolicy { .. })
        ));
        request.parts[1].checksum = format!("sha256:{}", "b".repeat(64));
        request.expected_size_bytes = 4;
        assert!(matches!(
            request.validate(),
            Err(DaemonRequestValidationError::InvalidPolicy { .. })
        ));
    }

    #[test]
    fn multipart_response_requires_version_and_identity() {
        let response = ProfileS3MultipartCompletionResponse {
            schema_version: PROFILE_S3_SCHEMA_VERSION.to_string(),
            store_id: StoreId::new("codex").expect("store id"),
            reservation_id: "reservation-1".to_string(),
            key: BackendObjectKey {
                object_id: "writes/multipart.fastq".to_string(),
                version: 1,
            },
            committed: true,
        };
        response.validate().expect("response validates");
    }
}
