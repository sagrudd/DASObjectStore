//! Application-authorized exact-object deletion contract.

use dasobjectstore_core::ids::{ObjectId, StoreId};
use serde::{Deserialize, Serialize};

pub const APPLICATION_OBJECT_DELETE_ROUTE: &str = "/api/v1/application-auth/object-deletions";
pub const APPLICATION_OBJECT_DELETE_SCHEMA_VERSION: &str =
    "dasobjectstore.application_object_delete.v1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationObjectDeleteReason {
    UserRequested,
    SourceRemoved,
    PolicyRequired,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationObjectDeleteRequest {
    pub schema_version: String,
    pub request_id: String,
    pub session_id: String,
    pub renewal_token: String,
    pub application_id: String,
    pub object_store: String,
    pub object_id: String,
    pub object_version: u64,
    pub object_key: String,
    pub expected_size_bytes: u64,
    pub expected_checksum: String,
    pub provider: String,
    pub bucket: String,
    pub endpoint_url: String,
    pub reason: ApplicationObjectDeleteReason,
}

impl ApplicationObjectDeleteRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != APPLICATION_OBJECT_DELETE_SCHEMA_VERSION {
            return Err("unsupported application object delete schema".to_string());
        }
        for (name, value) in [
            ("request_id", self.request_id.as_str()),
            ("session_id", self.session_id.as_str()),
            ("renewal_token", self.renewal_token.as_str()),
            ("application_id", self.application_id.as_str()),
            ("object_store", self.object_store.as_str()),
            ("object_id", self.object_id.as_str()),
            ("object_key", self.object_key.as_str()),
            ("provider", self.provider.as_str()),
            ("bucket", self.bucket.as_str()),
            ("endpoint_url", self.endpoint_url.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(format!("{name} must not be blank"));
            }
        }
        if self.object_version == 0 {
            return Err("object_version must be greater than zero".to_string());
        }
        if self.expected_size_bytes == 0 {
            return Err("expected_size_bytes must be greater than zero".to_string());
        }
        if !self.expected_checksum.starts_with("sha256:")
            || self.expected_checksum.len() != 71
            || !self.expected_checksum[7..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("expected_checksum must be a sha256 digest".to_string());
        }
        StoreId::new(self.object_store.clone()).map_err(|error| error.to_string())?;
        ObjectId::new(self.object_id.clone()).map_err(|error| error.to_string())?;
        if self.object_key.starts_with('/')
            || self.object_key.contains('\\')
            || self
                .object_key
                .split('/')
                .any(|segment| segment.is_empty() || segment == "." || segment == "..")
        {
            return Err("object_key must be a normalized logical key".to_string());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationObjectDeleteOutcome {
    Deleted,
    AlreadyAbsent,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationObjectDeleteResponse {
    pub schema_version: String,
    pub request_id: String,
    pub outcome: ApplicationObjectDeleteOutcome,
    pub audit_event_id: String,
}

impl ApplicationObjectDeleteResponse {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != APPLICATION_OBJECT_DELETE_SCHEMA_VERSION {
            return Err("unsupported application object delete schema".to_string());
        }
        if self.request_id.trim().is_empty() || self.audit_event_id.trim().is_empty() {
            return Err("delete response identity must not be blank".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> ApplicationObjectDeleteRequest {
        ApplicationObjectDeleteRequest {
            schema_version: APPLICATION_OBJECT_DELETE_SCHEMA_VERSION.to_string(),
            request_id: "delete-1".to_string(),
            session_id: "session-1".to_string(),
            renewal_token: "renewal-1".to_string(),
            application_id: "pinakotheke".to_string(),
            object_store: "pinakotheke-media".to_string(),
            object_id: "media-1".to_string(),
            object_version: 1,
            object_key: "x.com/artist/explicit_original/media-1".to_string(),
            expected_size_bytes: 42,
            expected_checksum: format!("sha256:{}", "a".repeat(64)),
            provider: "garage".to_string(),
            bucket: "pinakotheke-media".to_string(),
            endpoint_url: "http://127.0.0.1:3900".to_string(),
            reason: ApplicationObjectDeleteReason::UserRequested,
        }
    }

    #[test]
    fn request_is_exact_and_strict() {
        request().validate().expect("request validates");
        let mut unsafe_key = request();
        unsafe_key.object_key = "../other".to_string();
        assert_eq!(
            unsafe_key.validate().unwrap_err(),
            "object_key must be a normalized logical key"
        );
        let value = serde_json::to_value(request()).expect("serialize");
        let mut object = value.as_object().expect("object").clone();
        object.insert("raw_credentials".into(), serde_json::json!("forbidden"));
        assert!(serde_json::from_value::<ApplicationObjectDeleteRequest>(
            serde_json::Value::Object(object)
        )
        .is_err());
    }
}
