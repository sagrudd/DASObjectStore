use crate::api::DaemonRequestValidationError;
use dasobjectstore_core::backend::{BackendHealth, BackendObjectKey};
use dasobjectstore_core::ids::StoreId;
use serde::{Deserialize, Serialize};

pub const PROFILE_S3_SCHEMA_VERSION: &str = "dasobjectstore.profile_s3.v1";
pub const PROFILE_S3_MAX_KEYS: u16 = 1_000;
pub const PROFILE_S3_MAX_MULTIPART_PARTS: usize = 10_000;
pub const PROFILE_S3_ROUTE_PREFIX: &str = "/api/v1/profile-s3";
pub const PROFILE_S3_OBJECTS_ROUTE: &str = "/api/v1/profile-s3/stores/{store_id}/objects";
pub const PROFILE_S3_OBJECT_ROUTE: &str = "/api/v1/profile-s3/stores/{store_id}/objects/{key}";
pub const PROFILE_S3_DELETE_ROUTE: &str = "/api/v1/profile-s3/stores/{store_id}/objects/{key}";
pub const PROFILE_S3_HEALTH_ROUTE: &str = "/api/v1/profile-s3/stores/{store_id}/health";
pub const PROFILE_S3_MULTIPART_COMPLETE_ROUTE: &str =
    "/api/v1/profile-s3/stores/{store_id}/multipart/{reservation_id}/complete";
pub const PROFILE_S3_MULTIPART_PART_ROUTE: &str =
    "/api/v1/profile-s3/stores/{store_id}/multipart/{reservation_id}/parts/{part_number}";

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

/// Catalogue-authoritative metadata lookup for one logical object. The
/// daemon resolves the registered profile and never exposes backend paths.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileS3HeadRequest {
    pub store_id: StoreId,
    pub key: BackendObjectKey,
}

/// Catalogue-authoritative, idempotent profile-object deletion request.
/// Backend roots and provider credentials remain daemon-owned.
pub type ProfileS3DeleteRequest = ProfileS3HeadRequest;

impl ProfileS3HeadRequest {
    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        if self.store_id.as_str().trim().is_empty() {
            return Err(DaemonRequestValidationError::BlankField { field: "store_id" });
        }
        if let Err(value) = validate_object_key(&self.key) {
            return Err(DaemonRequestValidationError::UnsupportedFieldValue {
                field: "key",
                value,
            });
        }
        Ok(())
    }
}

/// Verification uses the same path-free logical key shape as HEAD while
/// retaining a distinct contract name for callers and audit records.
pub type ProfileS3VerifyRequest = ProfileS3HeadRequest;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileS3HeadResponse {
    pub schema_version: String,
    pub store_id: StoreId,
    pub object: ProfileS3ObjectView,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileS3DeleteResponse {
    pub schema_version: String,
    pub store_id: StoreId,
    pub key: BackendObjectKey,
    pub deleted: bool,
}

impl ProfileS3DeleteResponse {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != PROFILE_S3_SCHEMA_VERSION {
            return Err("unsupported profile S3 schema".to_string());
        }
        if self.store_id.as_str().trim().is_empty() {
            return Err("profile S3 delete response store identity must not be blank".to_string());
        }
        validate_object_key(&self.key)
            .map_err(|value| format!("profile S3 delete response key is invalid: {value}"))
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileS3HealthRequest {
    pub store_id: StoreId,
}

impl ProfileS3HealthRequest {
    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        if self.store_id.as_str().trim().is_empty() {
            return Err(DaemonRequestValidationError::BlankField { field: "store_id" });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileS3HealthResponse {
    pub schema_version: String,
    pub store_id: StoreId,
    pub health: BackendHealth,
}

impl ProfileS3HealthResponse {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != PROFILE_S3_SCHEMA_VERSION {
            return Err("unsupported profile S3 schema".to_string());
        }
        if self.store_id.as_str().trim().is_empty() || self.health.state.trim().is_empty() {
            return Err("profile S3 health identity and state must not be blank".to_string());
        }
        Ok(())
    }
}

impl ProfileS3HeadResponse {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != PROFILE_S3_SCHEMA_VERSION {
            return Err("unsupported profile S3 schema".to_string());
        }
        if self.store_id.as_str().trim().is_empty() {
            return Err("profile S3 response store identity must not be blank".to_string());
        }
        self.object.validate()?;
        Ok(())
    }
}

/// Catalogue-authoritative verification result for one logical profile object.
/// The daemon verifies payload bytes against the durable catalogue before
/// returning success; no backend location is exposed to callers.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileS3VerifyResponse {
    pub schema_version: String,
    pub store_id: StoreId,
    pub object: ProfileS3ObjectView,
    pub verified: bool,
}

impl ProfileS3VerifyResponse {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != PROFILE_S3_SCHEMA_VERSION {
            return Err("unsupported profile S3 schema".to_string());
        }
        if self.store_id.as_str().trim().is_empty() || !self.verified {
            return Err("profile S3 verification response is not verified".to_string());
        }
        self.object.validate()?;
        Ok(())
    }
}

impl ProfileS3ListRequest {
    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        if self.store_id.as_str().trim().is_empty() {
            return Err(DaemonRequestValidationError::BlankField { field: "store_id" });
        }
        if let Some(prefix) = self.prefix.as_deref() {
            if prefix.trim().is_empty() {
                return Err(DaemonRequestValidationError::BlankField { field: "prefix" });
            }
            if let Err(value) = validate_prefix(prefix) {
                return Err(DaemonRequestValidationError::UnsupportedFieldValue {
                    field: "prefix",
                    value,
                });
            }
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

impl ProfileS3ObjectView {
    pub fn validate(&self) -> Result<(), String> {
        validate_object_key(&self.key)
            .map_err(|value| format!("invalid profile S3 key: {value}"))?;
        if !is_sha256_checksum(&self.checksum) {
            return Err("profile S3 object checksum must be a sha256 digest".to_string());
        }
        Ok(())
    }
}

impl ProfileS3ListResponse {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != PROFILE_S3_SCHEMA_VERSION {
            return Err("unsupported profile S3 schema".to_string());
        }
        if self.store_id.as_str().trim().is_empty() {
            return Err("profile S3 response store identity must not be blank".to_string());
        }
        if self.objects.len() > PROFILE_S3_MAX_KEYS as usize {
            return Err("profile S3 response exceeds maximum key count".to_string());
        }
        for object in &self.objects {
            object.validate()?;
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
        if let Err(value) = validate_object_key(&self.key) {
            return Err(DaemonRequestValidationError::UnsupportedFieldValue {
                field: "key",
                value,
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileS3MultipartAbortRequest {
    pub store_id: StoreId,
    pub reservation_id: String,
    pub key: BackendObjectKey,
}

impl ProfileS3MultipartAbortRequest {
    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        if self.store_id.as_str().trim().is_empty() || self.reservation_id.trim().is_empty() {
            return Err(DaemonRequestValidationError::BlankField {
                field: "multipart_abort_identity",
            });
        }
        validate_object_key(&self.key).map_err(|value| {
            DaemonRequestValidationError::UnsupportedFieldValue {
                field: "key",
                value,
            }
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileS3MultipartAbortResponse {
    pub schema_version: String,
    pub store_id: StoreId,
    pub reservation_id: String,
    pub aborted: bool,
}

impl ProfileS3MultipartCompletionResponse {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != PROFILE_S3_SCHEMA_VERSION {
            return Err("unsupported profile S3 schema".to_string());
        }
        if self.store_id.as_str().trim().is_empty() || self.reservation_id.trim().is_empty() {
            return Err("multipart completion response identity must not be blank".to_string());
        }
        validate_object_key(&self.key)
            .map_err(|value| format!("multipart completion response key is invalid: {value}"))?;
        Ok(())
    }
}

fn validate_object_key(key: &BackendObjectKey) -> Result<(), String> {
    if key.version == 0 {
        return Err("object version must be greater than zero".to_string());
    }
    if key.object_id.trim().is_empty()
        || key.object_id.starts_with('/')
        || key.object_id.ends_with('/')
        || key.object_id.contains('\\')
        || key.object_id.contains('\0')
    {
        return Err(key.object_id.clone());
    }
    if key
        .object_id
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(key.object_id.clone());
    }
    Ok(())
}

fn validate_prefix(prefix: &str) -> Result<(), String> {
    if prefix.starts_with('/') || prefix.contains('\\') || prefix.contains('\0') {
        return Err(prefix.to_string());
    }
    let without_trailing_separator = prefix.strip_suffix('/').unwrap_or(prefix);
    if without_trailing_separator
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(prefix.to_string());
    }
    Ok(())
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
    fn list_request_rejects_unsafe_prefixes_but_allows_namespace_trailing_slash() {
        let mut request = ProfileS3ListRequest {
            store_id: StoreId::new("codex").expect("store id"),
            prefix: Some("reads/".to_string()),
            offset: 0,
            limit: 100,
        };
        request.validate().expect("namespace prefix validates");
        for prefix in ["/reads", "reads/../escape", "reads\\escape", "reads/./"] {
            request.prefix = Some(prefix.to_string());
            assert!(matches!(
                request.validate(),
                Err(DaemonRequestValidationError::UnsupportedFieldValue {
                    field: "prefix",
                    ..
                })
            ));
        }
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
                checksum: format!("sha256:{}", "a".repeat(64)),
            }],
            next_offset: Some(1),
        };
        let json = serde_json::to_string(&response).expect("response serializes");
        assert!(!json.contains("backend_root"));
        assert!(!json.contains("location"));
        response.validate().expect("schema validates");
    }

    #[test]
    fn object_views_reject_unsafe_keys_zero_versions_and_short_checksums() {
        let mut response = ProfileS3ListResponse {
            schema_version: PROFILE_S3_SCHEMA_VERSION.to_string(),
            store_id: StoreId::new("codex").expect("store id"),
            objects: vec![ProfileS3ObjectView {
                key: BackendObjectKey {
                    object_id: "../escape".to_string(),
                    version: 1,
                },
                size_bytes: 1,
                checksum: format!("sha256:{}", "a".repeat(64)),
            }],
            next_offset: None,
        };
        assert!(response.validate().is_err());
        response.objects[0].key.object_id = "safe/key".to_string();
        response.objects[0].key.version = 0;
        assert!(response.validate().is_err());
        response.objects[0].key.version = 1;
        response.objects[0].checksum = "sha256:short".to_string();
        assert!(response.validate().is_err());
    }

    #[test]
    fn delete_contract_is_path_free_and_reports_idempotent_result() {
        let request = ProfileS3DeleteRequest {
            store_id: StoreId::new("codex").expect("store id"),
            key: BackendObjectKey {
                object_id: "writes/sample.fastq".to_string(),
                version: 1,
            },
        };
        request.validate().expect("delete request validates");
        let response = ProfileS3DeleteResponse {
            schema_version: PROFILE_S3_SCHEMA_VERSION.to_string(),
            store_id: request.store_id.clone(),
            key: request.key.clone(),
            deleted: false,
        };
        response.validate().expect("delete response validates");
        let json = serde_json::to_string(&response).expect("response serializes");
        assert!(!json.contains("backend_root"));
        assert!(!json.contains("credentials"));
        assert!(json.contains("\"deleted\":false"));
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

    #[test]
    fn profile_s3_routes_are_stable_and_keep_identity_in_the_path() {
        assert_eq!(PROFILE_S3_ROUTE_PREFIX, "/api/v1/profile-s3");
        assert!(PROFILE_S3_OBJECTS_ROUTE.starts_with(PROFILE_S3_ROUTE_PREFIX));
        assert!(PROFILE_S3_OBJECTS_ROUTE.contains("{store_id}"));
        assert!(PROFILE_S3_OBJECT_ROUTE.starts_with(PROFILE_S3_ROUTE_PREFIX));
        assert!(PROFILE_S3_OBJECT_ROUTE.contains("{key}"));
        assert!(PROFILE_S3_HEALTH_ROUTE.starts_with(PROFILE_S3_ROUTE_PREFIX));
        assert!(PROFILE_S3_HEALTH_ROUTE.contains("{store_id}"));
        assert!(PROFILE_S3_MULTIPART_COMPLETE_ROUTE.starts_with(PROFILE_S3_ROUTE_PREFIX));
        assert!(PROFILE_S3_MULTIPART_COMPLETE_ROUTE.contains("{store_id}"));
        assert!(PROFILE_S3_MULTIPART_COMPLETE_ROUTE.contains("{reservation_id}"));
        assert!(PROFILE_S3_MULTIPART_PART_ROUTE.starts_with(PROFILE_S3_ROUTE_PREFIX));
        assert!(PROFILE_S3_MULTIPART_PART_ROUTE.contains("{reservation_id}"));
        assert!(PROFILE_S3_MULTIPART_PART_ROUTE.contains("{part_number}"));
    }

    #[test]
    fn verify_response_requires_verified_state_and_remains_path_free() {
        let response = ProfileS3VerifyResponse {
            schema_version: PROFILE_S3_SCHEMA_VERSION.to_string(),
            store_id: StoreId::new("codex").expect("store id"),
            object: ProfileS3ObjectView {
                key: BackendObjectKey {
                    object_id: "reads/sample.fastq".to_string(),
                    version: 1,
                },
                size_bytes: 4,
                checksum: format!("sha256:{}", "a".repeat(64)),
            },
            verified: true,
        };
        let json = serde_json::to_string(&response).expect("serialize");
        assert!(!json.contains("location"));
        response.validate().expect("schema");
        let mut unverified = response;
        unverified.verified = false;
        assert!(unverified.validate().is_err());
    }

    #[test]
    fn head_request_and_response_are_path_free_and_versioned() {
        let request = ProfileS3HeadRequest {
            store_id: StoreId::new("codex").expect("store id"),
            key: BackendObjectKey {
                object_id: "reads/sample.fastq".to_string(),
                version: 1,
            },
        };
        request.validate().expect("head request validates");
        let response = ProfileS3HeadResponse {
            schema_version: PROFILE_S3_SCHEMA_VERSION.to_string(),
            store_id: request.store_id,
            object: ProfileS3ObjectView {
                key: request.key,
                size_bytes: 4,
                checksum: format!("sha256:{}", "a".repeat(64)),
            },
        };
        response.validate().expect("head response validates");
    }
}
