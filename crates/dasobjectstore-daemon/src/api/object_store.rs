use crate::api::{DaemonJobAcceptedResponse, DaemonJobId, DaemonJobKind};
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_core::store::{
    CapacityBehavior, ExportPolicy, RetentionPolicy, StoreClass, StorePolicy,
};
use dasobjectstore_object_service::StoreServiceDefinition;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};
use std::path::PathBuf;
use std::str::FromStr;

pub const OBJECT_STORE_CREATE_CONFIRMATION: &str = "confirm create objectstore";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CreateObjectStoreRequest {
    pub store_id: String,
    pub store_class: String,
    pub required_copies: u8,
    pub bucket: Option<String>,
    #[serde(default)]
    pub reader_group: Option<String>,
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
        validate_store_id(&self.store_id)?;
        validate_safe_name("store_class", &self.store_class)?;
        validate_optional_safe_name("reader_group", self.reader_group.as_deref())?;
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
        self.registry_definition()?;

        Ok(())
    }

    pub fn registry_definition(
        &self,
    ) -> Result<StoreServiceDefinition, CreateObjectStoreValidationError> {
        validate_store_id(&self.store_id)?;
        validate_safe_name("store_class", &self.store_class)?;
        validate_optional_safe_name("reader_group", self.reader_group.as_deref())?;
        validate_safe_name("writer_group", &self.writer_group)?;
        validate_safe_name("object_type", &self.object_type)?;
        validate_safe_name("capacity_behavior", &self.capacity_behavior)?;
        validate_safe_name("retention", &self.retention)?;
        validate_safe_name("endpoint_export_mode", &self.endpoint_export_mode)?;
        validate_optional_safe_name("bucket", self.bucket.as_deref())?;
        if self.required_copies == 0 || self.required_copies > 3 {
            return Err(CreateObjectStoreValidationError::InvalidCopyCount {
                copies: self.required_copies,
            });
        }

        let class = parse_store_class(&self.store_class)?;
        let mut policy = StorePolicy::defaults_for(class);
        policy.copies = self.required_copies;
        policy.capacity_behavior = parse_capacity_behavior(&self.capacity_behavior)?;
        policy.retention_policy = parse_retention_policy(&self.retention)?;
        policy.export_policy = parse_export_policy(&self.endpoint_export_mode)?;
        policy
            .validate()
            .map_err(|error| CreateObjectStoreValidationError::InvalidPolicy {
                message: error.to_string(),
            })?;

        Ok(StoreServiceDefinition {
            store_id: StoreId::new(self.store_id.clone()).expect("validated store id"),
            policy,
            bucket_name: self.bucket.clone(),
            reader_group: self.reader_group.clone(),
            writer_group: Some(self.writer_group.clone()),
            public: self.public,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CreateObjectStoreResponse {
    pub accepted: DaemonJobAcceptedResponse,
    pub store_id: String,
    pub store_class: String,
    pub required_copies: u8,
    pub bucket: Option<String>,
    pub reader_group: Option<String>,
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
            reader_group: request.reader_group,
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
    InvalidFieldValue { field: &'static str, value: String },
    InvalidPolicy { message: String },
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
            Self::InvalidFieldValue { field, value } => {
                write!(formatter, "unsupported {field}: {value}")
            }
            Self::InvalidPolicy { message } => formatter.write_str(message),
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

fn validate_store_id(value: &str) -> Result<(), CreateObjectStoreValidationError> {
    if value.trim().is_empty() {
        return Err(CreateObjectStoreValidationError::BlankField { field: "store_id" });
    }
    if !is_safe_store_id(value) {
        return Err(CreateObjectStoreValidationError::UnsafeName {
            field: "store_id",
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

fn is_safe_store_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > 128 || !value.is_ascii() {
        return false;
    }
    let first = bytes[0];
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return false;
    }
    bytes[1..].iter().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-' | b'.')
    })
}

fn parse_store_class(value: &str) -> Result<StoreClass, CreateObjectStoreValidationError> {
    StoreClass::from_str(value).map_err(|_| CreateObjectStoreValidationError::InvalidFieldValue {
        field: "store_class",
        value: value.to_string(),
    })
}

fn parse_capacity_behavior(
    value: &str,
) -> Result<CapacityBehavior, CreateObjectStoreValidationError> {
    match value {
        "reject_writes" | "conservative" => Ok(CapacityBehavior::RejectWrites),
        "backpressure_by_priority" | "balanced" | "fill_lowest_fractional_usage" => {
            Ok(CapacityBehavior::BackpressureByPriority)
        }
        "mark_redownload_required" | "reproducible_cache" => {
            Ok(CapacityBehavior::MarkRedownloadRequired)
        }
        _ => Err(CreateObjectStoreValidationError::InvalidFieldValue {
            field: "capacity_behavior",
            value: value.to_string(),
        }),
    }
}

fn parse_retention_policy(
    value: &str,
) -> Result<RetentionPolicy, CreateObjectStoreValidationError> {
    match value {
        "immediate_delete" => Ok(RetentionPolicy::ImmediateDelete),
        "tombstone_then_gc" | "standard" | "retain_until_deleted" => {
            Ok(RetentionPolicy::TombstoneThenGc)
        }
        _ => Err(CreateObjectStoreValidationError::InvalidFieldValue {
            field: "retention",
            value: value.to_string(),
        }),
    }
}

fn parse_export_policy(value: &str) -> Result<ExportPolicy, CreateObjectStoreValidationError> {
    match value {
        "s3" | "s3_bucket" => Ok(ExportPolicy::S3),
        "read_only_file_export" | "read_only_export" => Ok(ExportPolicy::ReadOnlyFileExport),
        "disabled" | "internal_only" => Ok(ExportPolicy::Disabled),
        _ => Err(CreateObjectStoreValidationError::InvalidFieldValue {
            field: "endpoint_export_mode",
            value: value.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CreateObjectStoreRequest, CreateObjectStoreResponse, CreateObjectStoreValidationError,
        OBJECT_STORE_CREATE_CONFIRMATION,
    };
    use crate::api::{DaemonJobId, DaemonJobKind};
    use dasobjectstore_core::store::{CapacityBehavior, ExportPolicy, RetentionPolicy, StoreClass};
    use std::path::PathBuf;

    fn valid_request() -> CreateObjectStoreRequest {
        CreateObjectStoreRequest {
            store_id: "generated-data".to_string(),
            store_class: "generated_data".to_string(),
            required_copies: 2,
            bucket: Some("generated-data".to_string()),
            reader_group: Some("bioinformatics-readers".to_string()),
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
    fn request_projects_to_cli_registry_definition_shape() {
        let definition = valid_request()
            .registry_definition()
            .expect("registry definition projects");

        assert_eq!(definition.store_id.as_str(), "generated-data");
        assert_eq!(definition.policy.class, StoreClass::GeneratedData);
        assert_eq!(definition.policy.copies, 2);
        assert_eq!(
            definition.policy.capacity_behavior,
            CapacityBehavior::BackpressureByPriority
        );
        assert_eq!(
            definition.policy.retention_policy,
            RetentionPolicy::TombstoneThenGc
        );
        assert_eq!(definition.policy.export_policy, ExportPolicy::S3);
        assert_eq!(definition.bucket_name.as_deref(), Some("generated-data"));
        assert_eq!(
            definition.reader_group.as_deref(),
            Some("bioinformatics-readers")
        );
        assert_eq!(definition.writer_group.as_deref(), Some("bioinformatics"));
        assert!(!definition.public);
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
    fn preserves_dotted_third_party_dataset_identifiers() {
        for store_id in ["zymo_fecal_2025.05", "colo824_2024.03"] {
            let request = CreateObjectStoreRequest {
                store_id: store_id.to_string(),
                ..valid_request()
            };

            request
                .validate()
                .expect("third-party dotted store id remains valid");
            assert_eq!(
                request
                    .registry_definition()
                    .expect("third-party dotted store id projects")
                    .store_id
                    .as_str(),
                store_id
            );
        }
    }

    #[test]
    fn rejects_store_id_that_could_escape_a_store_namespace() {
        let request = CreateObjectStoreRequest {
            store_id: "../generated-data".to_string(),
            ..valid_request()
        };

        assert_eq!(
            request.validate(),
            Err(CreateObjectStoreValidationError::UnsafeName {
                field: "store_id",
                value: "../generated-data".to_string(),
            })
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
    fn rejects_unsupported_domain_policy_values() {
        let request = CreateObjectStoreRequest {
            capacity_behavior: "fast".to_string(),
            ..valid_request()
        };

        assert_eq!(
            request.validate(),
            Err(CreateObjectStoreValidationError::InvalidFieldValue {
                field: "capacity_behavior",
                value: "fast".to_string()
            })
        );
    }

    #[test]
    fn rejects_policy_combinations_not_accepted_by_cli_registry() {
        let request = CreateObjectStoreRequest {
            store_class: "critical_metadata".to_string(),
            required_copies: 1,
            retention: "immediate_delete".to_string(),
            capacity_behavior: "mark_redownload_required".to_string(),
            ..valid_request()
        };

        let Err(CreateObjectStoreValidationError::InvalidPolicy { message }) = request.validate()
        else {
            panic!("invalid policy combination should be rejected");
        };
        assert!(message.contains("protected store class critical_metadata"));
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
