use dasobjectstore_core::application_auth::UploadCompletionCapability;
use serde::{Deserialize, Serialize};

pub const APPLICATION_UPLOAD_COMPLETION_CAPABILITY_ROUTE: &str =
    "/api/v1/application-auth/upload-completions/capabilities";
pub const APPLICATION_UPLOAD_COMPLETION_ROUTE: &str =
    "/api/v1/application-auth/upload-completions/complete";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationUploadCapabilityIssueRequest {
    pub session_id: String,
    pub renewal_token: String,
    pub application_id: String,
    pub upload_id: String,
    pub object_store: String,
    pub object_id: String,
    pub object_version: u64,
    pub object_key: String,
    pub expected_size_bytes: u64,
    pub expected_checksum: String,
    pub audience: String,
    pub provider: String,
    pub bucket: String,
    pub endpoint_url: String,
    #[serde(default)]
    pub requested_ttl_seconds: Option<u64>,
}

impl ApplicationUploadCapabilityIssueRequest {
    pub fn validate(&self) -> Result<(), String> {
        for (name, value) in [
            ("session_id", self.session_id.as_str()),
            ("renewal_token", self.renewal_token.as_str()),
            ("application_id", self.application_id.as_str()),
            ("upload_id", self.upload_id.as_str()),
            ("object_store", self.object_store.as_str()),
            ("object_id", self.object_id.as_str()),
            ("object_key", self.object_key.as_str()),
            ("audience", self.audience.as_str()),
            ("provider", self.provider.as_str()),
            ("bucket", self.bucket.as_str()),
            ("endpoint_url", self.endpoint_url.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(format!("{name} must not be blank"));
            }
        }
        if !self.expected_checksum.starts_with("sha256:")
            || self.expected_checksum.len() != 71
            || !self.expected_checksum[7..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("expected_checksum must be a sha256 digest".to_string());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationUploadCapabilityIssueResponse {
    pub capability: UploadCompletionCapability,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationUploadCompletionRequest {
    pub capability: UploadCompletionCapability,
}

impl ApplicationUploadCompletionRequest {
    pub fn validate(&self) -> Result<(), String> {
        self.capability
            .validate()
            .map_err(|error| error.to_string())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationUploadCompletionOutcome {
    Committed,
    AlreadyCommitted,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationUploadCompletionResponse {
    pub capability_id: String,
    pub outcome: ApplicationUploadCompletionOutcome,
}
