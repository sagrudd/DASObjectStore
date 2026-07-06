use crate::boundary::{synoptikon_object_store_boundary, HostStorageBoundary};
use crate::validation::is_uuid_like;
use dasobjectstore_object_service::ObjectServiceProviderId;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

pub const MNEION_S3_BACKEND_KIND: &str = "S3-Compatible";
pub const MNEION_DASOBJECTSTORE_DAS_BACKEND_KIND: &str = "DASObjectStore-DAS";
pub const MNEION_DASOBJECTSTORE_NFS_BACKEND_KIND: &str = "DASObjectStore-NFS";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MneionDasObjectStoreEndpointKind {
    DasobjectstoreDas,
    DasobjectstoreNfs,
    S3Compatible,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MneionDasObjectStoreEndpoint {
    pub identifier: String,
    pub display_name: String,
    pub endpoint_kind: MneionDasObjectStoreEndpointKind,
    pub manager_product_id: String,
    pub object_contract: MneionEndpointObjectContract,
    pub location: MneionDasObjectStoreEndpointLocation,
}

impl MneionDasObjectStoreEndpoint {
    pub fn das_backed(
        identifier: impl Into<String>,
        display_name: impl Into<String>,
        pool_id: impl Into<String>,
        service_endpoint: impl Into<String>,
    ) -> Self {
        Self {
            identifier: identifier.into(),
            display_name: display_name.into(),
            endpoint_kind: MneionDasObjectStoreEndpointKind::DasobjectstoreDas,
            manager_product_id: crate::DASOBJECTSTORE_PRODUCT_ID.to_string(),
            object_contract: MneionEndpointObjectContract::ObjectStyle,
            location: MneionDasObjectStoreEndpointLocation::Das {
                pool_id: pool_id.into(),
                service_endpoint: service_endpoint.into(),
            },
        }
    }

    pub fn nfs_backed(
        identifier: impl Into<String>,
        display_name: impl Into<String>,
        export_id: impl Into<String>,
        service_endpoint: impl Into<String>,
    ) -> Self {
        Self {
            identifier: identifier.into(),
            display_name: display_name.into(),
            endpoint_kind: MneionDasObjectStoreEndpointKind::DasobjectstoreNfs,
            manager_product_id: crate::DASOBJECTSTORE_PRODUCT_ID.to_string(),
            object_contract: MneionEndpointObjectContract::ObjectStyle,
            location: MneionDasObjectStoreEndpointLocation::Nfs {
                export_id: export_id.into(),
                service_endpoint: service_endpoint.into(),
            },
        }
    }

    pub fn s3_compatible(
        identifier: impl Into<String>,
        display_name: impl Into<String>,
        provider_id: ObjectServiceProviderId,
        endpoint: impl Into<String>,
    ) -> Self {
        Self {
            identifier: identifier.into(),
            display_name: display_name.into(),
            endpoint_kind: MneionDasObjectStoreEndpointKind::S3Compatible,
            manager_product_id: crate::DASOBJECTSTORE_PRODUCT_ID.to_string(),
            object_contract: MneionEndpointObjectContract::ObjectStyle,
            location: MneionDasObjectStoreEndpointLocation::S3Compatible {
                provider_id,
                endpoint: endpoint.into(),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MneionEndpointObjectContract {
    ObjectStyle,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "location_kind", rename_all = "snake_case")]
pub enum MneionDasObjectStoreEndpointLocation {
    Das {
        pool_id: String,
        service_endpoint: String,
    },
    Nfs {
        export_id: String,
        service_endpoint: String,
    },
    S3Compatible {
        provider_id: ObjectServiceProviderId,
        endpoint: String,
    },
}

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
pub struct MneionManagedStorageDefinitionExport {
    pub managed_endpoint: MneionDasObjectStoreEndpoint,
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

pub fn export_mneion_das_storage_definition(
    endpoint: &MneionDasObjectStoreEndpoint,
) -> Result<MneionManagedStorageDefinitionExport, MneionStorageDefinitionError> {
    validate_managed_endpoint_base(endpoint)?;
    let service_endpoint = match &endpoint.location {
        MneionDasObjectStoreEndpointLocation::Das {
            service_endpoint, ..
        } => service_endpoint,
        _ => {
            return Err(MneionStorageDefinitionError::UnsupportedEndpointKind {
                endpoint_kind: endpoint.endpoint_kind,
            });
        }
    };
    validate_endpoint(service_endpoint)?;

    Ok(MneionManagedStorageDefinitionExport {
        managed_endpoint: trimmed_managed_endpoint(endpoint),
        object_store_create_request: MneionObjectStoreCreateRequest {
            identifier: endpoint.identifier.trim().to_string(),
            display_name: endpoint.display_name.trim().to_string(),
            backend_kind: MNEION_DASOBJECTSTORE_DAS_BACKEND_KIND.to_string(),
            endpoint: Some(service_endpoint.trim().to_string()),
            nfs_server: None,
            nfs_export_path: None,
            nfs_root_prefix: None,
            nfs_protocol: None,
            nfs_auth_profile: None,
        },
        host_storage_boundary: synoptikon_object_store_boundary(),
        notes: vec![
            "Generated for a DASObjectStore-managed local DAS endpoint.".to_string(),
            "Mneion should treat this as a managed storage appliance endpoint, not a generic POSIX path.".to_string(),
            "Products should continue to use Limen-mediated object-style ingress and egress.".to_string(),
        ],
    })
}

pub fn export_mneion_nfs_storage_definition(
    endpoint: &MneionDasObjectStoreEndpoint,
) -> Result<MneionManagedStorageDefinitionExport, MneionStorageDefinitionError> {
    validate_managed_endpoint_base(endpoint)?;
    let service_endpoint = match &endpoint.location {
        MneionDasObjectStoreEndpointLocation::Nfs {
            service_endpoint, ..
        } => service_endpoint,
        _ => {
            return Err(MneionStorageDefinitionError::UnsupportedEndpointKind {
                endpoint_kind: endpoint.endpoint_kind,
            });
        }
    };
    validate_endpoint(service_endpoint)?;

    Ok(MneionManagedStorageDefinitionExport {
        managed_endpoint: trimmed_managed_endpoint(endpoint),
        object_store_create_request: MneionObjectStoreCreateRequest {
            identifier: endpoint.identifier.trim().to_string(),
            display_name: endpoint.display_name.trim().to_string(),
            backend_kind: MNEION_DASOBJECTSTORE_NFS_BACKEND_KIND.to_string(),
            endpoint: Some(service_endpoint.trim().to_string()),
            nfs_server: None,
            nfs_export_path: None,
            nfs_root_prefix: None,
            nfs_protocol: None,
            nfs_auth_profile: None,
        },
        host_storage_boundary: synoptikon_object_store_boundary(),
        notes: vec![
            "Generated for a DASObjectStore-managed external NAS/NFS endpoint.".to_string(),
            "Mneion should treat this as a managed storage appliance endpoint, not a generic POSIX path.".to_string(),
            "Raw NAS export details remain DASObjectStore validation inputs, not tenant-facing storage contracts.".to_string(),
        ],
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MneionStorageDefinitionError {
    InvalidIdentifier {
        value: String,
    },
    BlankDisplayName,
    BlankEndpoint,
    UnsupportedEndpoint {
        value: String,
    },
    UnsupportedEndpointKind {
        endpoint_kind: MneionDasObjectStoreEndpointKind,
    },
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
            Self::UnsupportedEndpointKind { endpoint_kind } => write!(
                formatter,
                "Mneion managed storage definition export received unsupported endpoint kind: {endpoint_kind:?}"
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
    if is_uuid_like(trimmed) {
        Ok(())
    } else {
        Err(MneionStorageDefinitionError::InvalidIdentifier {
            value: trimmed.to_string(),
        })
    }
}

fn validate_managed_endpoint_base(
    endpoint: &MneionDasObjectStoreEndpoint,
) -> Result<(), MneionStorageDefinitionError> {
    validate_uuid_like(&endpoint.identifier)?;
    validate_display_name(&endpoint.display_name)?;
    Ok(())
}

fn trimmed_managed_endpoint(
    endpoint: &MneionDasObjectStoreEndpoint,
) -> MneionDasObjectStoreEndpoint {
    MneionDasObjectStoreEndpoint {
        identifier: endpoint.identifier.trim().to_string(),
        display_name: endpoint.display_name.trim().to_string(),
        endpoint_kind: endpoint.endpoint_kind,
        manager_product_id: endpoint.manager_product_id.trim().to_string(),
        object_contract: endpoint.object_contract,
        location: trimmed_location(&endpoint.location),
    }
}

fn trimmed_location(
    location: &MneionDasObjectStoreEndpointLocation,
) -> MneionDasObjectStoreEndpointLocation {
    match location {
        MneionDasObjectStoreEndpointLocation::Das {
            pool_id,
            service_endpoint,
        } => MneionDasObjectStoreEndpointLocation::Das {
            pool_id: pool_id.trim().to_string(),
            service_endpoint: service_endpoint.trim().to_string(),
        },
        MneionDasObjectStoreEndpointLocation::Nfs {
            export_id,
            service_endpoint,
        } => MneionDasObjectStoreEndpointLocation::Nfs {
            export_id: export_id.trim().to_string(),
            service_endpoint: service_endpoint.trim().to_string(),
        },
        MneionDasObjectStoreEndpointLocation::S3Compatible {
            provider_id,
            endpoint,
        } => MneionDasObjectStoreEndpointLocation::S3Compatible {
            provider_id: *provider_id,
            endpoint: endpoint.trim().to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{
        export_mneion_das_storage_definition, export_mneion_nfs_storage_definition,
        export_mneion_storage_definition, MneionDasObjectStoreEndpoint,
        MneionDasObjectStoreEndpointKind, MneionEndpointObjectContract,
        MneionStorageDefinitionError, MneionStorageDefinitionRequest,
        MNEION_DASOBJECTSTORE_DAS_BACKEND_KIND, MNEION_DASOBJECTSTORE_NFS_BACKEND_KIND,
        MNEION_S3_BACKEND_KIND,
    };
    use dasobjectstore_object_service::ObjectServiceProviderId;
    use serde_json::json;

    const STORE_UUID: &str = "4f0a1ba7-9f00-422b-bf18-87567b076daa";

    #[test]
    fn models_das_backed_endpoint_variant() {
        let endpoint = MneionDasObjectStoreEndpoint::das_backed(
            STORE_UUID,
            "DAS pool endpoint",
            "pool-1",
            "http://127.0.0.1:3900",
        );

        assert_eq!(
            endpoint.endpoint_kind,
            MneionDasObjectStoreEndpointKind::DasobjectstoreDas
        );
        assert_eq!(
            endpoint.object_contract,
            MneionEndpointObjectContract::ObjectStyle
        );

        let serialized = serde_json::to_value(endpoint).expect("endpoint serializes");
        assert_eq!(serialized["endpoint_kind"], "dasobjectstore_das");
        assert_eq!(serialized["manager_product_id"], "dasobjectstore");
        assert_eq!(serialized["object_contract"], "object_style");
        assert_eq!(serialized["location"]["location_kind"], "das");
        assert_eq!(serialized["location"]["pool_id"], "pool-1");
        assert_eq!(
            serialized["location"]["service_endpoint"],
            "http://127.0.0.1:3900"
        );
    }

    #[test]
    fn models_nfs_backed_endpoint_variant() {
        let endpoint = MneionDasObjectStoreEndpoint::nfs_backed(
            STORE_UUID,
            "NAS endpoint",
            "nas-export-1",
            "http://nas-gateway.local:3900",
        );

        assert_eq!(
            endpoint.endpoint_kind,
            MneionDasObjectStoreEndpointKind::DasobjectstoreNfs
        );

        let serialized = serde_json::to_value(endpoint).expect("endpoint serializes");
        assert_eq!(serialized["endpoint_kind"], "dasobjectstore_nfs");
        assert_eq!(serialized["location"]["location_kind"], "nfs");
        assert_eq!(serialized["location"]["export_id"], "nas-export-1");
        assert_eq!(
            serialized["location"]["service_endpoint"],
            "http://nas-gateway.local:3900"
        );
    }

    #[test]
    fn models_s3_compatible_endpoint_variant() {
        let endpoint = MneionDasObjectStoreEndpoint::s3_compatible(
            STORE_UUID,
            "S3 endpoint",
            ObjectServiceProviderId::Garage,
            "http://127.0.0.1:3900",
        );

        assert_eq!(
            endpoint.endpoint_kind,
            MneionDasObjectStoreEndpointKind::S3Compatible
        );

        let serialized = serde_json::to_value(endpoint).expect("endpoint serializes");
        assert_eq!(serialized["endpoint_kind"], "s3_compatible");
        assert_eq!(serialized["location"]["location_kind"], "s3_compatible");
        assert_eq!(serialized["location"]["provider_id"], "Garage");
        assert_eq!(serialized["location"]["endpoint"], "http://127.0.0.1:3900");
    }

    #[test]
    fn exports_das_backed_storage_definition_bundle() {
        let endpoint = MneionDasObjectStoreEndpoint::das_backed(
            STORE_UUID,
            "DAS pool endpoint",
            "pool-1",
            "http://127.0.0.1:3900",
        );

        let export = export_mneion_das_storage_definition(&endpoint).expect("DAS export succeeds");

        assert_eq!(
            export.object_store_create_request.backend_kind,
            MNEION_DASOBJECTSTORE_DAS_BACKEND_KIND
        );
        assert_eq!(
            export.object_store_create_request.endpoint.as_deref(),
            Some("http://127.0.0.1:3900")
        );
        assert_eq!(
            export.managed_endpoint.endpoint_kind,
            MneionDasObjectStoreEndpointKind::DasobjectstoreDas
        );
        assert_eq!(export.managed_endpoint.manager_product_id, "dasobjectstore");
        assert!(export.object_store_create_request.nfs_export_path.is_none());
        assert!(export
            .notes
            .iter()
            .any(|note| note.contains("managed storage appliance")));
    }

    #[test]
    fn trims_das_backed_storage_definition_export_inputs() {
        let endpoint = MneionDasObjectStoreEndpoint::das_backed(
            format!(" {STORE_UUID} "),
            " DAS pool endpoint ",
            " pool-1 ",
            " http://127.0.0.1:3900 ",
        );

        let export = export_mneion_das_storage_definition(&endpoint).expect("DAS export succeeds");

        assert_eq!(export.object_store_create_request.identifier, STORE_UUID);
        assert_eq!(
            export.object_store_create_request.display_name,
            "DAS pool endpoint"
        );
        assert_eq!(
            export.object_store_create_request.endpoint.as_deref(),
            Some("http://127.0.0.1:3900")
        );
        assert_eq!(export.managed_endpoint.identifier, STORE_UUID);
        assert_eq!(export.managed_endpoint.display_name, "DAS pool endpoint");
        assert_eq!(
            serde_json::to_value(export.managed_endpoint).expect("endpoint serializes")["location"]
                ["pool_id"],
            "pool-1"
        );
    }

    #[test]
    fn rejects_nfs_endpoint_for_das_backed_storage_definition_export() {
        let endpoint = MneionDasObjectStoreEndpoint::nfs_backed(
            STORE_UUID,
            "NAS endpoint",
            "nas-export-1",
            "http://nas-gateway.local:3900",
        );

        let err = export_mneion_das_storage_definition(&endpoint)
            .expect_err("wrong endpoint kind rejected");

        assert_eq!(
            err,
            MneionStorageDefinitionError::UnsupportedEndpointKind {
                endpoint_kind: MneionDasObjectStoreEndpointKind::DasobjectstoreNfs
            }
        );
    }

    #[test]
    fn exports_nfs_backed_storage_definition_bundle() {
        let endpoint = MneionDasObjectStoreEndpoint::nfs_backed(
            STORE_UUID,
            "NAS endpoint",
            "nas-export-1",
            "https://nas-gateway.local:3900",
        );

        let export = export_mneion_nfs_storage_definition(&endpoint).expect("NFS export succeeds");

        assert_eq!(
            export.object_store_create_request.backend_kind,
            MNEION_DASOBJECTSTORE_NFS_BACKEND_KIND
        );
        assert_eq!(
            export.object_store_create_request.endpoint.as_deref(),
            Some("https://nas-gateway.local:3900")
        );
        assert_eq!(
            export.managed_endpoint.endpoint_kind,
            MneionDasObjectStoreEndpointKind::DasobjectstoreNfs
        );
        assert_eq!(export.managed_endpoint.manager_product_id, "dasobjectstore");
        assert!(export.object_store_create_request.nfs_export_path.is_none());
        assert!(export
            .notes
            .iter()
            .any(|note| note.contains("not tenant-facing storage contracts")));
    }

    #[test]
    fn trims_nfs_backed_storage_definition_export_inputs() {
        let endpoint = MneionDasObjectStoreEndpoint::nfs_backed(
            format!(" {STORE_UUID} "),
            " NAS endpoint ",
            " nas-export-1 ",
            " https://nas-gateway.local:3900 ",
        );

        let export = export_mneion_nfs_storage_definition(&endpoint).expect("NFS export succeeds");

        assert_eq!(export.object_store_create_request.identifier, STORE_UUID);
        assert_eq!(
            export.object_store_create_request.display_name,
            "NAS endpoint"
        );
        assert_eq!(
            export.object_store_create_request.endpoint.as_deref(),
            Some("https://nas-gateway.local:3900")
        );
        assert_eq!(export.managed_endpoint.identifier, STORE_UUID);
        assert_eq!(export.managed_endpoint.display_name, "NAS endpoint");
        assert_eq!(
            serde_json::to_value(export.managed_endpoint).expect("endpoint serializes")["location"]
                ["export_id"],
            "nas-export-1"
        );
    }

    #[test]
    fn rejects_das_endpoint_for_nfs_backed_storage_definition_export() {
        let endpoint = MneionDasObjectStoreEndpoint::das_backed(
            STORE_UUID,
            "DAS pool endpoint",
            "pool-1",
            "http://127.0.0.1:3900",
        );

        let err = export_mneion_nfs_storage_definition(&endpoint)
            .expect_err("wrong endpoint kind rejected");

        assert_eq!(
            err,
            MneionStorageDefinitionError::UnsupportedEndpointKind {
                endpoint_kind: MneionDasObjectStoreEndpointKind::DasobjectstoreDas
            }
        );
    }

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
