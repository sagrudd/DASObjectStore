use crate::{validation::is_uuid_like, MneionDasObjectStoreEndpoint};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

pub const NAS_NFS_ENDPOINT_DEFINITION_SCHEMA_VERSION: &str = "dasobjectstore.nas_nfs_endpoint.v1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NasNfsEndpointValidationStatus {
    Draft,
    PendingValidation,
    Validated,
    Degraded,
    Rejected,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NasNfsEndpointDefinition {
    pub schema_version: String,
    pub identifier: String,
    pub display_name: String,
    pub nfs_server: String,
    pub nfs_export_path: String,
    pub object_service_endpoint: String,
    pub credential_reference: String,
    pub tls_ca_reference: Option<String>,
    pub tls_server_name: Option<String>,
    pub status: NasNfsEndpointValidationStatus,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ValidatedNasNfsEndpointDefinition {
    pub definition: NasNfsEndpointDefinition,
    pub mneion_endpoint: MneionDasObjectStoreEndpoint,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NasNfsEndpointValidationError {
    InvalidSchemaVersion { value: String },
    InvalidIdentifier { value: String },
    BlankDisplayName,
    InvalidNfsServer { value: String },
    InvalidNfsExportPath { value: String },
    InvalidObjectServiceEndpoint { value: String },
    InvalidCredentialReference { value: String },
    InvalidTlsCaReference { value: String },
    InvalidTlsServerName { value: String },
    RejectedEndpoint,
}

impl Display for NasNfsEndpointValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSchemaVersion { value } => write!(
                formatter,
                "NAS/NFS endpoint schema_version must be {NAS_NFS_ENDPOINT_DEFINITION_SCHEMA_VERSION}: {value}"
            ),
            Self::InvalidIdentifier { value } => {
                write!(formatter, "NAS/NFS endpoint identifier must be a UUID: {value}")
            }
            Self::BlankDisplayName => {
                formatter.write_str("NAS/NFS endpoint display_name must not be blank")
            }
            Self::InvalidNfsServer { value } => write!(
                formatter,
                "NAS/NFS endpoint nfs_server must be a host name or address without path separators: {value}"
            ),
            Self::InvalidNfsExportPath { value } => write!(
                formatter,
                "NAS/NFS endpoint nfs_export_path must be an absolute export path without parent traversal: {value}"
            ),
            Self::InvalidObjectServiceEndpoint { value } => write!(
                formatter,
                "NAS/NFS endpoint object_service_endpoint must start with http:// or https://: {value}"
            ),
            Self::InvalidCredentialReference { value } => write!(
                formatter,
                "NAS/NFS endpoint credential_reference must be a non-secret reference URI: {value}"
            ),
            Self::InvalidTlsCaReference { value } => write!(
                formatter,
                "NAS/NFS endpoint tls_ca_reference must be a non-secret reference URI when present: {value}"
            ),
            Self::InvalidTlsServerName { value } => write!(
                formatter,
                "NAS/NFS endpoint tls_server_name must not contain whitespace or path separators: {value}"
            ),
            Self::RejectedEndpoint => {
                formatter.write_str("NAS/NFS endpoint status must not be rejected")
            }
        }
    }
}

impl std::error::Error for NasNfsEndpointValidationError {}

pub fn validate_nas_nfs_endpoint_definition(
    definition: &NasNfsEndpointDefinition,
) -> Result<ValidatedNasNfsEndpointDefinition, NasNfsEndpointValidationError> {
    validate_schema_version(&definition.schema_version)?;
    validate_identifier(&definition.identifier)?;
    validate_display_name(&definition.display_name)?;
    validate_nfs_server(&definition.nfs_server)?;
    validate_nfs_export_path(&definition.nfs_export_path)?;
    validate_object_service_endpoint(&definition.object_service_endpoint)?;
    validate_reference(&definition.credential_reference, ReferenceField::Credential)?;
    if let Some(reference) = &definition.tls_ca_reference {
        validate_reference(reference, ReferenceField::TlsCa)?;
    }
    if let Some(name) = &definition.tls_server_name {
        validate_tls_server_name(name)?;
    }
    if definition.status == NasNfsEndpointValidationStatus::Rejected {
        return Err(NasNfsEndpointValidationError::RejectedEndpoint);
    }

    let trimmed = trimmed_definition(definition);
    Ok(ValidatedNasNfsEndpointDefinition {
        mneion_endpoint: MneionDasObjectStoreEndpoint::nfs_backed(
            trimmed.identifier.clone(),
            trimmed.display_name.clone(),
            trimmed.identifier.clone(),
            trimmed.object_service_endpoint.clone(),
        ),
        definition: trimmed,
    })
}

fn validate_schema_version(value: &str) -> Result<(), NasNfsEndpointValidationError> {
    if value == NAS_NFS_ENDPOINT_DEFINITION_SCHEMA_VERSION {
        Ok(())
    } else {
        Err(NasNfsEndpointValidationError::InvalidSchemaVersion {
            value: value.to_string(),
        })
    }
}

fn validate_identifier(value: &str) -> Result<(), NasNfsEndpointValidationError> {
    let trimmed = value.trim();
    if is_uuid_like(trimmed) {
        Ok(())
    } else {
        Err(NasNfsEndpointValidationError::InvalidIdentifier {
            value: trimmed.to_string(),
        })
    }
}

fn validate_display_name(value: &str) -> Result<(), NasNfsEndpointValidationError> {
    if value.trim().is_empty() {
        Err(NasNfsEndpointValidationError::BlankDisplayName)
    } else {
        Ok(())
    }
}

fn validate_nfs_server(value: &str) -> Result<(), NasNfsEndpointValidationError> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed
            .chars()
            .any(|ch| ch.is_whitespace() || matches!(ch, '/' | '\\'))
    {
        return Err(NasNfsEndpointValidationError::InvalidNfsServer {
            value: trimmed.to_string(),
        });
    }
    Ok(())
}

fn validate_nfs_export_path(value: &str) -> Result<(), NasNfsEndpointValidationError> {
    let trimmed = value.trim();
    if !trimmed.starts_with('/')
        || trimmed.split('/').any(|part| part == "..")
        || trimmed.contains('\\')
    {
        return Err(NasNfsEndpointValidationError::InvalidNfsExportPath {
            value: trimmed.to_string(),
        });
    }
    Ok(())
}

fn validate_object_service_endpoint(value: &str) -> Result<(), NasNfsEndpointValidationError> {
    let trimmed = value.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        Ok(())
    } else {
        Err(
            NasNfsEndpointValidationError::InvalidObjectServiceEndpoint {
                value: trimmed.to_string(),
            },
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReferenceField {
    Credential,
    TlsCa,
}

fn validate_reference(
    value: &str,
    field: ReferenceField,
) -> Result<(), NasNfsEndpointValidationError> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.len() > 256
        || !trimmed.contains("://")
        || trimmed.chars().any(char::is_whitespace)
    {
        return Err(match field {
            ReferenceField::Credential => {
                NasNfsEndpointValidationError::InvalidCredentialReference {
                    value: trimmed.to_string(),
                }
            }
            ReferenceField::TlsCa => NasNfsEndpointValidationError::InvalidTlsCaReference {
                value: trimmed.to_string(),
            },
        });
    }
    Ok(())
}

fn validate_tls_server_name(value: &str) -> Result<(), NasNfsEndpointValidationError> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed
            .chars()
            .any(|ch| ch.is_whitespace() || matches!(ch, '/' | '\\'))
    {
        return Err(NasNfsEndpointValidationError::InvalidTlsServerName {
            value: trimmed.to_string(),
        });
    }
    Ok(())
}

fn trimmed_definition(definition: &NasNfsEndpointDefinition) -> NasNfsEndpointDefinition {
    NasNfsEndpointDefinition {
        schema_version: definition.schema_version.clone(),
        identifier: definition.identifier.trim().to_string(),
        display_name: definition.display_name.trim().to_string(),
        nfs_server: definition.nfs_server.trim().to_string(),
        nfs_export_path: definition.nfs_export_path.trim().to_string(),
        object_service_endpoint: definition.object_service_endpoint.trim().to_string(),
        credential_reference: definition.credential_reference.trim().to_string(),
        tls_ca_reference: definition
            .tls_ca_reference
            .as_ref()
            .map(|value| value.trim().to_string()),
        tls_server_name: definition
            .tls_server_name
            .as_ref()
            .map(|value| value.trim().to_string()),
        status: definition.status,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        validate_nas_nfs_endpoint_definition, NasNfsEndpointDefinition,
        NasNfsEndpointValidationError, NasNfsEndpointValidationStatus,
        NAS_NFS_ENDPOINT_DEFINITION_SCHEMA_VERSION,
    };
    use crate::{
        MneionDasObjectStoreEndpointKind, MneionDasObjectStoreEndpointLocation,
        MneionEndpointObjectContract,
    };

    const ENDPOINT_UUID: &str = "ad255a8f-0058-4790-a640-758c573f2db1";

    #[test]
    fn validates_nas_nfs_endpoint_definition() {
        let definition = valid_definition();

        let validated =
            validate_nas_nfs_endpoint_definition(&definition).expect("NAS/NFS endpoint validates");

        assert_eq!(validated.definition, definition);
        assert_eq!(
            validated.mneion_endpoint.endpoint_kind,
            MneionDasObjectStoreEndpointKind::DasobjectstoreNfs
        );
        assert_eq!(
            validated.mneion_endpoint.object_contract,
            MneionEndpointObjectContract::ObjectStyle
        );
        assert_eq!(
            validated.mneion_endpoint.manager_product_id,
            "dasobjectstore"
        );
        match validated.mneion_endpoint.location {
            MneionDasObjectStoreEndpointLocation::Nfs {
                export_id,
                service_endpoint,
            } => {
                assert_eq!(export_id, ENDPOINT_UUID);
                assert_eq!(service_endpoint, "https://nas-gateway.local:3900");
            }
            other => panic!("expected NFS endpoint location, got {other:?}"),
        }
    }

    #[test]
    fn trims_nas_nfs_endpoint_definition_fields() {
        let definition = NasNfsEndpointDefinition {
            identifier: format!(" {ENDPOINT_UUID} "),
            display_name: " Shared NAS ".to_string(),
            nfs_server: " nas-01.local ".to_string(),
            nfs_export_path: " /exports/bioinformatics ".to_string(),
            object_service_endpoint: " https://nas-gateway.local:3900 ".to_string(),
            credential_reference: " secret://dasobjectstore/nas/shared ".to_string(),
            tls_ca_reference: Some(" secret://dasobjectstore/ca/nas ".to_string()),
            tls_server_name: Some(" nas-gateway.local ".to_string()),
            ..valid_definition()
        };

        let validated =
            validate_nas_nfs_endpoint_definition(&definition).expect("NAS/NFS endpoint validates");

        assert_eq!(validated.definition.identifier, ENDPOINT_UUID);
        assert_eq!(validated.definition.display_name, "Shared NAS");
        assert_eq!(validated.definition.nfs_server, "nas-01.local");
        assert_eq!(
            validated.definition.nfs_export_path,
            "/exports/bioinformatics"
        );
        assert_eq!(
            validated.definition.credential_reference,
            "secret://dasobjectstore/nas/shared"
        );
        assert_eq!(
            validated.definition.tls_ca_reference.as_deref(),
            Some("secret://dasobjectstore/ca/nas")
        );
        assert_eq!(
            validated.definition.tls_server_name.as_deref(),
            Some("nas-gateway.local")
        );
    }

    #[test]
    fn serializes_status_with_contract_field_names() {
        let encoded = serde_json::to_value(valid_definition()).expect("definition serializes");

        assert_eq!(
            encoded["schema_version"],
            NAS_NFS_ENDPOINT_DEFINITION_SCHEMA_VERSION
        );
        assert_eq!(encoded["status"], "validated");
        assert_eq!(
            encoded["credential_reference"],
            "secret://dasobjectstore/nas/shared"
        );
        assert_eq!(
            encoded["tls_ca_reference"],
            "secret://dasobjectstore/ca/nas"
        );
    }

    #[test]
    fn rejects_relative_export_paths() {
        let definition = NasNfsEndpointDefinition {
            nfs_export_path: "exports/bioinformatics".to_string(),
            ..valid_definition()
        };

        let err = validate_nas_nfs_endpoint_definition(&definition)
            .expect_err("relative export path rejected");

        assert_eq!(
            err,
            NasNfsEndpointValidationError::InvalidNfsExportPath {
                value: "exports/bioinformatics".to_string()
            }
        );
    }

    #[test]
    fn rejects_missing_credential_reference() {
        let definition = NasNfsEndpointDefinition {
            credential_reference: " ".to_string(),
            ..valid_definition()
        };

        let err = validate_nas_nfs_endpoint_definition(&definition)
            .expect_err("credential reference rejected");

        assert_eq!(
            err,
            NasNfsEndpointValidationError::InvalidCredentialReference {
                value: String::new()
            }
        );
    }

    #[test]
    fn rejects_rejected_endpoint_status() {
        let definition = NasNfsEndpointDefinition {
            status: NasNfsEndpointValidationStatus::Rejected,
            ..valid_definition()
        };

        let err = validate_nas_nfs_endpoint_definition(&definition)
            .expect_err("rejected endpoint status rejected");

        assert_eq!(err, NasNfsEndpointValidationError::RejectedEndpoint);
    }

    fn valid_definition() -> NasNfsEndpointDefinition {
        NasNfsEndpointDefinition {
            schema_version: NAS_NFS_ENDPOINT_DEFINITION_SCHEMA_VERSION.to_string(),
            identifier: ENDPOINT_UUID.to_string(),
            display_name: "Shared NAS".to_string(),
            nfs_server: "nas-01.local".to_string(),
            nfs_export_path: "/exports/bioinformatics".to_string(),
            object_service_endpoint: "https://nas-gateway.local:3900".to_string(),
            credential_reference: "secret://dasobjectstore/nas/shared".to_string(),
            tls_ca_reference: Some("secret://dasobjectstore/ca/nas".to_string()),
            tls_server_name: Some("nas-gateway.local".to_string()),
            status: NasNfsEndpointValidationStatus::Validated,
        }
    }
}
