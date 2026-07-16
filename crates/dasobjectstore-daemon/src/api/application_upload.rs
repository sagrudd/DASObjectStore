use dasobjectstore_core::application_auth::UploadCompletionCapability;
use dasobjectstore_core::ids::ObjectId;
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
        ObjectId::new(self.object_id.clone()).map_err(|error| error.to_string())?;
        if self.object_version == 0 {
            return Err("object_version must be greater than zero".to_string());
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

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> ApplicationUploadCapabilityIssueRequest {
        ApplicationUploadCapabilityIssueRequest {
            session_id: "session-1".to_string(),
            renewal_token: "renewal".to_string(),
            application_id: "synoptikon".to_string(),
            upload_id: "upload-1".to_string(),
            object_store: "science".to_string(),
            object_id: "object-1".to_string(),
            object_version: 1,
            object_key: "analysis/object-1".to_string(),
            expected_size_bytes: 5,
            expected_checksum: format!("sha256:{}", "a".repeat(64)),
            audience: "dasobjectstored".to_string(),
            provider: "garage".to_string(),
            bucket: "science".to_string(),
            endpoint_url: "http://127.0.0.1:3900".to_string(),
            requested_ttl_seconds: Some(600),
        }
    }

    #[test]
    fn rejects_provider_identity_that_cannot_be_catalogued_before_admission() {
        let mut request = request();
        request.object_version = 0;
        assert_eq!(
            request.validate().unwrap_err(),
            "object_version must be greater than zero"
        );
    }
}
