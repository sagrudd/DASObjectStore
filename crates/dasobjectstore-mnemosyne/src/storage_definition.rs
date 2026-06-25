use crate::boundary::{synoptikon_object_store_boundary, HostStorageBoundary};
use dasobjectstore_object_service::ObjectServiceProviderId;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

pub const MNEION_S3_BACKEND_KIND: &str = "S3-Compatible";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MneionStorageDefinitionRequest {
    pub identifier: String,
    pub display_name: String,
    pub provider_id: ObjectServiceProviderId,
    pub endpoint: String,
}

impl MneionStorageDefinitionRequest {
    pub fn new(
        identifier: impl Into<String>,
        display_name: impl Into<String>,
        provider_id: ObjectServiceProviderId,
        endpoint: impl Into<String>,
    ) -> Self {
        Self {
            identifier: identifier.into(),
            display_name: display_name.into(),
            provider_id,
            endpoint: endpoint.into(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MneionStorageDefinitionExport {
    pub object_store_create_request: MneionObjectStoreCreateRequest,
    pub host_storage_boundary: HostStorageBoundary,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MneionObjectStoreCreateRequest {
    pub identifier: String,
    pub display_name: String,
    pub backend_kind: String,
    pub endpoint: Option<String>,
    pub nfs_server: Option<String>,
    pub nfs_export_path: Option<String>,
    pub nfs_root_prefix: Option<String>,
    pub nfs_protocol: Option<String>,
    pub nfs_auth_profile: Option<String>,
}

pub fn export_mneion_storage_definition(
    request: &MneionStorageDefinitionRequest,
) -> Result<MneionStorageDefinitionExport, MneionStorageDefinitionError> {
    validate_uuid_like(&request.identifier)?;
    validate_display_name(&request.display_name)?;
    validate_endpoint(&request.endpoint)?;

    Ok(MneionStorageDefinitionExport {
        object_store_create_request: MneionObjectStoreCreateRequest {
            identifier: request.identifier.trim().to_string(),
            display_name: request.display_name.trim().to_string(),
            backend_kind: MNEION_S3_BACKEND_KIND.to_string(),
            endpoint: Some(request.endpoint.trim().to_string()),
            nfs_server: None,
            nfs_export_path: None,
            nfs_root_prefix: None,
            nfs_protocol: None,
            nfs_auth_profile: None,
        },
        host_storage_boundary: synoptikon_object_store_boundary(),
        notes: vec![
            format!(
                "Generated for DASObjectStore {} S3-compatible service.",
                request.provider_id
            ),
            "This is a Mneion control-plane object-store definition, not a Limen runtime mount."
                .to_string(),
            "Products should continue to use Limen-mediated object-style ingress and egress."
                .to_string(),
        ],
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MneionStorageDefinitionError {
    InvalidIdentifier { value: String },
    BlankDisplayName,
    BlankEndpoint,
    UnsupportedEndpoint { value: String },
}

impl Display for MneionStorageDefinitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentifier { value } => {
                write!(
                    formatter,
                    "Mneion object-store identifier must be a UUID: {value}"
                )
            }
            Self::BlankDisplayName => {
                formatter.write_str("Mneion object-store display name must not be blank")
            }
            Self::BlankEndpoint => {
                formatter.write_str("Mneion object-store endpoint must not be blank")
            }
            Self::UnsupportedEndpoint { value } => write!(
                formatter,
                "Mneion S3-compatible endpoint must start with http:// or https://: {value}"
            ),
        }
    }
}

impl std::error::Error for MneionStorageDefinitionError {}

fn validate_display_name(value: &str) -> Result<(), MneionStorageDefinitionError> {
    if value.trim().is_empty() {
        Err(MneionStorageDefinitionError::BlankDisplayName)
    } else {
        Ok(())
    }
}

fn validate_endpoint(value: &str) -> Result<(), MneionStorageDefinitionError> {
    let endpoint = value.trim();
    if endpoint.is_empty() {
        return Err(MneionStorageDefinitionError::BlankEndpoint);
    }

    if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        Ok(())
    } else {
        Err(MneionStorageDefinitionError::UnsupportedEndpoint {
            value: endpoint.to_string(),
        })
    }
}

fn validate_uuid_like(value: &str) -> Result<(), MneionStorageDefinitionError> {
    let trimmed = value.trim();
    let parts = trimmed.split('-').collect::<Vec<_>>();
    let valid = parts.len() == 5
        && [8, 4, 4, 4, 12]
            .iter()
            .zip(parts.iter())
            .all(|(expected_len, part)| {
                part.len() == *expected_len && part.chars().all(|ch| ch.is_ascii_hexdigit())
            });

    if valid {
        Ok(())
    } else {
        Err(MneionStorageDefinitionError::InvalidIdentifier {
            value: trimmed.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        export_mneion_storage_definition, MneionStorageDefinitionError,
        MneionStorageDefinitionRequest, MNEION_S3_BACKEND_KIND,
    };
    use dasobjectstore_object_service::ObjectServiceProviderId;
    use serde_json::json;

    const STORE_UUID: &str = "4f0a1ba7-9f00-422b-bf18-87567b076daa";

    #[test]
    fn exports_mneion_s3_compatible_storage_definition() {
        let request = MneionStorageDefinitionRequest::new(
            STORE_UUID,
            "DASObjectStore Development",
            ObjectServiceProviderId::Garage,
            "http://127.0.0.1:3900",
        );

        let export = export_mneion_storage_definition(&request).expect("definition exports");

        assert_eq!(
            export.object_store_create_request.identifier,
            STORE_UUID.to_string()
        );
        assert_eq!(
            export.object_store_create_request.backend_kind,
            MNEION_S3_BACKEND_KIND
        );
        assert_eq!(
            export.object_store_create_request.endpoint.as_deref(),
            Some("http://127.0.0.1:3900")
        );
        assert!(export.object_store_create_request.nfs_server.is_none());
        assert_eq!(
            export.host_storage_boundary.schema_version,
            "mnemosyne.host_storage_boundary.v1"
        );
        assert!(export
            .notes
            .iter()
            .any(|note| note.contains("control-plane")));
    }

    #[test]
    fn serializes_create_request_with_mneion_field_names() {
        let request = MneionStorageDefinitionRequest::new(
            STORE_UUID,
            "DASObjectStore Development",
            ObjectServiceProviderId::Rustfs,
            "https://dasobjectstore.local:9000",
        );

        let export = export_mneion_storage_definition(&request).expect("definition exports");
        let serialized = serde_json::to_value(export.object_store_create_request)
            .expect("create request serializes");

        assert_eq!(
            serialized,
            json!({
                "identifier": STORE_UUID,
                "display_name": "DASObjectStore Development",
                "backend_kind": "S3-Compatible",
                "endpoint": "https://dasobjectstore.local:9000",
                "nfs_server": null,
                "nfs_export_path": null,
                "nfs_root_prefix": null,
                "nfs_protocol": null,
                "nfs_auth_profile": null
            })
        );
    }

    #[test]
    fn trims_mneion_storage_definition_inputs() {
        let request = MneionStorageDefinitionRequest::new(
            format!(" {STORE_UUID} "),
            " DASObjectStore Development ",
            ObjectServiceProviderId::Garage,
            " http://127.0.0.1:3900 ",
        );

        let export = export_mneion_storage_definition(&request).expect("definition exports");

        assert_eq!(export.object_store_create_request.identifier, STORE_UUID);
        assert_eq!(
            export.object_store_create_request.display_name,
            "DASObjectStore Development"
        );
        assert_eq!(
            export.object_store_create_request.endpoint.as_deref(),
            Some("http://127.0.0.1:3900")
        );
    }

    #[test]
    fn rejects_non_uuid_identifiers() {
        let request = MneionStorageDefinitionRequest::new(
            "dasobjectstore-dev",
            "DASObjectStore Development",
            ObjectServiceProviderId::Garage,
            "http://127.0.0.1:3900",
        );

        let err = export_mneion_storage_definition(&request).expect_err("identifier rejected");

        assert_eq!(
            err,
            MneionStorageDefinitionError::InvalidIdentifier {
                value: "dasobjectstore-dev".to_string()
            }
        );
    }

    #[test]
    fn rejects_blank_display_names() {
        let request = MneionStorageDefinitionRequest::new(
            STORE_UUID,
            " ",
            ObjectServiceProviderId::Garage,
            "http://127.0.0.1:3900",
        );

        let err = export_mneion_storage_definition(&request).expect_err("display name rejected");

        assert_eq!(err, MneionStorageDefinitionError::BlankDisplayName);
    }

    #[test]
    fn rejects_non_http_endpoints() {
        let request = MneionStorageDefinitionRequest::new(
            STORE_UUID,
            "DASObjectStore Development",
            ObjectServiceProviderId::Garage,
            "s3://127.0.0.1:3900",
        );

        let err = export_mneion_storage_definition(&request).expect_err("endpoint rejected");

        assert_eq!(
            err,
            MneionStorageDefinitionError::UnsupportedEndpoint {
                value: "s3://127.0.0.1:3900".to_string()
            }
        );
    }
}
