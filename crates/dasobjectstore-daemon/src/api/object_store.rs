use crate::api::{DaemonJobAcceptedResponse, DaemonJobId, DaemonJobKind};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};
use std::path::PathBuf;

pub const OBJECT_STORE_CREATE_CONFIRMATION: &str = "confirm create objectstore";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CreateObjectStoreRequest {
    pub store_id: String,
    pub store_class: String,
    pub required_copies: u8,
    pub bucket: Option<String>,
    pub writer_group: String,
    pub ssd_root: PathBuf,
    pub object_type: String,
    pub enclosure_id: Option<String>,
    pub public: bool,
    pub writeable: bool,
    pub capacity_behavior: String,
    pub retention: String,
    pub endpoint_export_mode: String,
    pub dry_run: bool,
    pub client_request_id: Option<String>,
    pub administrator_actor: Option<String>,
    pub confirmation_marker: String,
}

impl CreateObjectStoreRequest {
    pub fn validate(&self) -> Result<(), CreateObjectStoreValidationError> {
        validate_safe_name("store_id", &self.store_id)?;
        validate_safe_name("store_class", &self.store_class)?;
        validate_safe_name("writer_group", &self.writer_group)?;
        validate_safe_name("object_type", &self.object_type)?;
        validate_safe_name("capacity_behavior", &self.capacity_behavior)?;
        validate_safe_name("retention", &self.retention)?;
        validate_safe_name("endpoint_export_mode", &self.endpoint_export_mode)?;
        validate_optional_safe_name("bucket", self.bucket.as_deref())?;
        validate_optional_safe_name("enclosure_id", self.enclosure_id.as_deref())?;
        validate_optional_safe_name("administrator_actor", self.administrator_actor.as_deref())?;
        if self.required_copies == 0 || self.required_copies > 3 {
            return Err(CreateObjectStoreValidationError::InvalidCopyCount {
                copies: self.required_copies,
            });
        }
        if !self.ssd_root.is_absolute() {
            return Err(CreateObjectStoreValidationError::RelativePath {
                field: "ssd_root",
                path: self.ssd_root.clone(),
            });
        }
        if self
            .client_request_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(CreateObjectStoreValidationError::BlankClientRequestId);
        }
        if self.confirmation_marker.trim() != OBJECT_STORE_CREATE_CONFIRMATION {
            return Err(CreateObjectStoreValidationError::ConfirmationMismatch);
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CreateObjectStoreResponse {
    pub accepted: DaemonJobAcceptedResponse,
    pub store_id: String,
    pub store_class: String,
    pub required_copies: u8,
    pub bucket: Option<String>,
    pub writer_group: String,
    pub ssd_root: PathBuf,
    pub object_type: String,
    pub enclosure_id: Option<String>,
    pub public: bool,
    pub writeable: bool,
    pub capacity_behavior: String,
    pub retention: String,
    pub endpoint_export_mode: String,
    pub administrator_actor: Option<String>,
}

impl CreateObjectStoreResponse {
    pub fn accepted(
        job_id: DaemonJobId,
        accepted_at_utc: impl Into<String>,
        request: CreateObjectStoreRequest,
    ) -> Self {
        Self {
            accepted: DaemonJobAcceptedResponse {
                job_id,
                kind: DaemonJobKind::ObjectStoreCreation,
                accepted_at_utc: accepted_at_utc.into(),
                dry_run: request.dry_run,
            },
            store_id: request.store_id,
            store_class: request.store_class,
            required_copies: request.required_copies,
            bucket: request.bucket,
            writer_group: request.writer_group,
            ssd_root: request.ssd_root,
            object_type: request.object_type,
            enclosure_id: request.enclosure_id,
            public: request.public,
            writeable: request.writeable,
            capacity_behavior: request.capacity_behavior,
            retention: request.retention,
            endpoint_export_mode: request.endpoint_export_mode,
            administrator_actor: request.administrator_actor,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CreateObjectStoreValidationError {
    BlankField { field: &'static str },
    UnsafeName { field: &'static str, value: String },
    InvalidCopyCount { copies: u8 },
    RelativePath { field: &'static str, path: PathBuf },
    BlankClientRequestId,
    ConfirmationMismatch,
}

impl Display for CreateObjectStoreValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BlankField { field } => write!(formatter, "{field} must not be blank"),
            Self::UnsafeName { field, value } => write!(
                formatter,
                "{field} must be a conservative POSIX-style local name: {value}"
            ),
            Self::InvalidCopyCount { copies } => {
                write!(
                    formatter,
                    "required_copies must be between 1 and 3: {copies}"
                )
            }
            Self::RelativePath { field, path } => {
                write!(formatter, "{field} must be absolute: {}", path.display())
            }
            Self::BlankClientRequestId => {
                formatter.write_str("client_request_id must not be blank")
            }
            Self::ConfirmationMismatch => write!(
                formatter,
                "confirmation_marker must exactly match \"{OBJECT_STORE_CREATE_CONFIRMATION}\""
            ),
        }
    }
}

impl std::error::Error for CreateObjectStoreValidationError {}

fn validate_safe_name(
    field: &'static str,
    value: &str,
) -> Result<(), CreateObjectStoreValidationError> {
    if value.trim().is_empty() {
        return Err(CreateObjectStoreValidationError::BlankField { field });
    }
    if !is_safe_name(value) {
        return Err(CreateObjectStoreValidationError::UnsafeName {
            field,
            value: value.to_string(),
        });
    }
    Ok(())
}

fn validate_optional_safe_name(
    field: &'static str,
    value: Option<&str>,
) -> Result<(), CreateObjectStoreValidationError> {
    let Some(value) = value else {
        return Ok(());
    };
    validate_safe_name(field, value)
}

fn is_safe_name(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 128 || !value.is_ascii() {
        return false;
    }
    let first = bytes[0];
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return false;
    }
    bytes[1..].iter().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_' || *byte == b'-'
    })
}

#[cfg(test)]
mod tests {
    use super::{
        CreateObjectStoreRequest, CreateObjectStoreResponse, CreateObjectStoreValidationError,
        OBJECT_STORE_CREATE_CONFIRMATION,
    };
    use crate::api::{DaemonJobId, DaemonJobKind};
    use std::path::PathBuf;

    fn valid_request() -> CreateObjectStoreRequest {
        CreateObjectStoreRequest {
            store_id: "generated-data".to_string(),
            store_class: "generated_data".to_string(),
            required_copies: 2,
            bucket: Some("generated-data".to_string()),
            writer_group: "bioinformatics".to_string(),
            ssd_root: PathBuf::from("/srv/dasobjectstore/ssd"),
            object_type: "pod5".to_string(),
            enclosure_id: Some("qnap-tl-d800c-01".to_string()),
            public: false,
            writeable: true,
            capacity_behavior: "balanced".to_string(),
            retention: "standard".to_string(),
            endpoint_export_mode: "s3_bucket".to_string(),
            dry_run: false,
            client_request_id: Some("request-1".to_string()),
            administrator_actor: Some("stephen".to_string()),
            confirmation_marker: OBJECT_STORE_CREATE_CONFIRMATION.to_string(),
        }
    }

    #[test]
    fn validates_confirmed_object_store_request() {
        valid_request().validate().expect("request validates");
    }

    #[test]
    fn rejects_blank_store_id() {
        let request = CreateObjectStoreRequest {
            store_id: " ".to_string(),
            ..valid_request()
        };

        assert_eq!(
            request.validate(),
            Err(CreateObjectStoreValidationError::BlankField { field: "store_id" })
        );
    }

    #[test]
    fn rejects_invalid_copy_count() {
        let request = CreateObjectStoreRequest {
            required_copies: 4,
            ..valid_request()
        };

        assert_eq!(
            request.validate(),
            Err(CreateObjectStoreValidationError::InvalidCopyCount { copies: 4 })
        );
    }

    #[test]
    fn rejects_relative_ssd_root() {
        let request = CreateObjectStoreRequest {
            ssd_root: PathBuf::from("ssd"),
            ..valid_request()
        };

        assert!(matches!(
            request.validate(),
            Err(CreateObjectStoreValidationError::RelativePath {
                field: "ssd_root",
                ..
            })
        ));
    }

    #[test]
    fn accepted_response_carries_audit_context() {
        let response = CreateObjectStoreResponse::accepted(
            DaemonJobId::new("objectstore-create-1").expect("job id"),
            "2026-07-08T20:45:00Z",
            valid_request(),
        );

        assert_eq!(response.accepted.kind, DaemonJobKind::ObjectStoreCreation);
        assert_eq!(response.store_id, "generated-data");
        assert_eq!(response.administrator_actor.as_deref(), Some("stephen"));
    }
}
