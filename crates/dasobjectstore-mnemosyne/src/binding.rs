use crate::{
    validation::is_uuid_like, MneionDasObjectStoreEndpoint, MneionDasObjectStoreEndpointKind,
    MneionEndpointObjectContract, DASOBJECTSTORE_PRODUCT_ID,
};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

pub const MNEION_OBJECT_STORE_ADMIN_ENDPOINT: &str = "/api/v1/admin/object-stores";
pub const INTERNAL_MNEION_GOVERNANCE_DOMAIN_ID: &str = "11111111-1111-1111-1111-111111111111";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MneionBindingSnippetRequest {
    pub object_store_identifier: String,
    pub governance_domain_id: String,
    pub note: Option<String>,
}

impl MneionBindingSnippetRequest {
    pub fn new(
        object_store_identifier: impl Into<String>,
        governance_domain_id: impl Into<String>,
    ) -> Self {
        Self {
            object_store_identifier: object_store_identifier.into(),
            governance_domain_id: governance_domain_id.into(),
            note: None,
        }
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MneionBindingSnippetExport {
    pub object_store_identifier: String,
    pub endpoint_path: String,
    pub object_store_link_request: MneionObjectStoreLinkRequest,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MneionObjectStoreLinkRequest {
    pub governance_domain_id: String,
    pub note: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MneionManagedBindingReadiness {
    Ready,
    Degraded,
    Blocked,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MneionManagedStorageBindingRequest {
    pub managed_endpoint: MneionDasObjectStoreEndpoint,
    pub governance_domain_id: String,
    pub binding_readiness: MneionManagedBindingReadiness,
    pub validation_evidence_schema: Option<String>,
    pub health_state: String,
    pub note: Option<String>,
}

impl MneionManagedStorageBindingRequest {
    pub fn new(
        managed_endpoint: MneionDasObjectStoreEndpoint,
        governance_domain_id: impl Into<String>,
    ) -> Self {
        Self {
            managed_endpoint,
            governance_domain_id: governance_domain_id.into(),
            binding_readiness: MneionManagedBindingReadiness::Ready,
            validation_evidence_schema: None,
            health_state: "healthy".to_string(),
            note: None,
        }
    }

    pub fn with_readiness(mut self, readiness: MneionManagedBindingReadiness) -> Self {
        self.binding_readiness = readiness;
        self
    }

    pub fn with_validation_evidence_schema(mut self, schema: impl Into<String>) -> Self {
        self.validation_evidence_schema = Some(schema.into());
        self
    }

    pub fn with_health_state(mut self, health_state: impl Into<String>) -> Self {
        self.health_state = health_state.into();
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MneionManagedStorageBindingExport {
    pub endpoint_path: String,
    pub object_store_link_request: MneionObjectStoreLinkRequest,
    pub managed_binding_contract: MneionManagedStorageBindingContract,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MneionManagedStorageBindingContract {
    pub storage_definition_id: String,
    pub governance_domain_id: String,
    pub endpoint_kind: String,
    pub manager_product_id: String,
    pub object_contract: String,
    pub binding_readiness: MneionManagedBindingReadiness,
    pub validation_evidence_schema: Option<String>,
    pub health_state: String,
    pub raw_paths_are_tenant_facing: bool,
}

pub fn export_mneion_binding_snippet(
    request: &MneionBindingSnippetRequest,
) -> Result<MneionBindingSnippetExport, MneionBindingSnippetError> {
    let object_store_identifier =
        validate_object_store_identifier(&request.object_store_identifier)?;
    let governance_domain_id = validate_governance_domain_id(&request.governance_domain_id)?;
    let note = request
        .note
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    Ok(MneionBindingSnippetExport {
        endpoint_path: format!(
            "{}/{}/link",
            MNEION_OBJECT_STORE_ADMIN_ENDPOINT, object_store_identifier
        ),
        object_store_identifier,
        object_store_link_request: MneionObjectStoreLinkRequest {
            governance_domain_id,
            note,
        },
        notes: vec![
            "Submit this snippet after the object-store definition exists in Mneion.".to_string(),
            "The binding grants governance-domain storage context; it does not expose raw storage to products.".to_string(),
            "Limen remains the mediated ingress and egress boundary for artefacts.".to_string(),
        ],
    })
}

pub fn export_mneion_managed_storage_binding(
    request: &MneionManagedStorageBindingRequest,
) -> Result<MneionManagedStorageBindingExport, MneionBindingSnippetError> {
    let object_store_identifier =
        validate_object_store_identifier(&request.managed_endpoint.identifier)?;
    let governance_domain_id = validate_governance_domain_id(&request.governance_domain_id)?;
    validate_managed_endpoint(&request.managed_endpoint)?;
    validate_binding_readiness(request.binding_readiness)?;
    let health_state = validate_binding_text_field("health_state", &request.health_state)?;
    let validation_evidence_schema = request
        .validation_evidence_schema
        .as_deref()
        .map(|value| validate_binding_text_field("validation_evidence_schema", value))
        .transpose()?;
    let note = request
        .note
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    Ok(MneionManagedStorageBindingExport {
        endpoint_path: format!(
            "{}/{}/link",
            MNEION_OBJECT_STORE_ADMIN_ENDPOINT, object_store_identifier
        ),
        object_store_link_request: MneionObjectStoreLinkRequest {
            governance_domain_id: governance_domain_id.clone(),
            note,
        },
        managed_binding_contract: MneionManagedStorageBindingContract {
            storage_definition_id: object_store_identifier,
            governance_domain_id,
            endpoint_kind: endpoint_kind_contract_name(&request.managed_endpoint),
            manager_product_id: request.managed_endpoint.manager_product_id.trim().to_string(),
            object_contract: object_contract_name(request.managed_endpoint.object_contract),
            binding_readiness: request.binding_readiness,
            validation_evidence_schema,
            health_state,
            raw_paths_are_tenant_facing: false,
        },
        notes: vec![
            "Submit the link request only after the DASObjectStore-managed endpoint is validated and binding-ready.".to_string(),
            "The managed binding contract preserves Mneion as the governance-domain storage authority.".to_string(),
            "Resolved product storage context must remain object-style and must not expose DAS, NAS, NFS, or local filesystem paths.".to_string(),
        ],
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MneionBindingSnippetError {
    InvalidObjectStoreIdentifier {
        value: String,
    },
    InvalidGovernanceDomainId {
        value: String,
    },
    InvalidManagedEndpointManager {
        value: String,
    },
    InvalidManagedEndpointContract {
        value: String,
    },
    InvalidBindingTextField {
        field: &'static str,
        value: String,
    },
    BindingNotReady {
        readiness: MneionManagedBindingReadiness,
    },
}

impl Display for MneionBindingSnippetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidObjectStoreIdentifier { value } => write!(
                formatter,
                "Mneion object-store identifier must be a UUID: {value}"
            ),
            Self::InvalidGovernanceDomainId { value } => {
                write!(
                    formatter,
                    "Mneion governance domain ID must be a UUID: {value}"
                )
            }
            Self::InvalidManagedEndpointManager { value } => write!(
                formatter,
                "managed storage binding endpoint manager_product_id must be dasobjectstore: {value}"
            ),
            Self::InvalidManagedEndpointContract { value } => write!(
                formatter,
                "managed storage binding endpoint object_contract must be object_style: {value}"
            ),
            Self::InvalidBindingTextField { field, value } => write!(
                formatter,
                "managed storage binding field {field} must be 1-128 visible ASCII characters: {value}"
            ),
            Self::BindingNotReady { readiness } => write!(
                formatter,
                "managed storage binding can only be exported when binding_readiness is ready: {readiness:?}"
            ),
        }
    }
}

impl std::error::Error for MneionBindingSnippetError {}

fn validate_object_store_identifier(value: &str) -> Result<String, MneionBindingSnippetError> {
    let trimmed = value.trim();
    if is_uuid_like(trimmed) {
        Ok(trimmed.to_string())
    } else {
        Err(MneionBindingSnippetError::InvalidObjectStoreIdentifier {
            value: trimmed.to_string(),
        })
    }
}

fn validate_governance_domain_id(value: &str) -> Result<String, MneionBindingSnippetError> {
    let trimmed = value.trim();
    if is_uuid_like(trimmed) {
        Ok(trimmed.to_string())
    } else {
        Err(MneionBindingSnippetError::InvalidGovernanceDomainId {
            value: trimmed.to_string(),
        })
    }
}

fn validate_managed_endpoint(
    endpoint: &MneionDasObjectStoreEndpoint,
) -> Result<(), MneionBindingSnippetError> {
    let manager = endpoint.manager_product_id.trim();
    if manager != DASOBJECTSTORE_PRODUCT_ID {
        return Err(MneionBindingSnippetError::InvalidManagedEndpointManager {
            value: manager.to_string(),
        });
    }
    if endpoint.object_contract != MneionEndpointObjectContract::ObjectStyle {
        return Err(MneionBindingSnippetError::InvalidManagedEndpointContract {
            value: object_contract_name(endpoint.object_contract),
        });
    }
    Ok(())
}

fn validate_binding_readiness(
    readiness: MneionManagedBindingReadiness,
) -> Result<(), MneionBindingSnippetError> {
    if readiness == MneionManagedBindingReadiness::Ready {
        Ok(())
    } else {
        Err(MneionBindingSnippetError::BindingNotReady { readiness })
    }
}

fn validate_binding_text_field(
    field: &'static str,
    value: &str,
) -> Result<String, MneionBindingSnippetError> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.len() > 128
        || !trimmed.chars().all(|ch| ch.is_ascii_graphic() || ch == ' ')
    {
        return Err(MneionBindingSnippetError::InvalidBindingTextField {
            field,
            value: trimmed.to_string(),
        });
    }
    Ok(trimmed.to_string())
}

fn endpoint_kind_contract_name(endpoint: &MneionDasObjectStoreEndpoint) -> String {
    match endpoint.endpoint_kind {
        MneionDasObjectStoreEndpointKind::DasobjectstoreDas => "dasobjectstore_das",
        MneionDasObjectStoreEndpointKind::DasobjectstoreNfs => "dasobjectstore_nfs",
        MneionDasObjectStoreEndpointKind::S3Compatible => "s3_compatible",
    }
    .to_string()
}

fn object_contract_name(contract: MneionEndpointObjectContract) -> String {
    match contract {
        MneionEndpointObjectContract::ObjectStyle => "object_style",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        export_mneion_binding_snippet, export_mneion_managed_storage_binding,
        MneionBindingSnippetError, MneionBindingSnippetRequest, MneionManagedBindingReadiness,
        MneionManagedStorageBindingRequest, INTERNAL_MNEION_GOVERNANCE_DOMAIN_ID,
    };
    use crate::MneionDasObjectStoreEndpoint;
    use serde_json::json;

    const STORE_UUID: &str = "4f0a1ba7-9f00-422b-bf18-87567b076daa";
    const TENANT_DOMAIN_UUID: &str = "22222222-2222-2222-2222-222222222222";

    #[test]
    fn exports_mneion_binding_snippet_for_governance_domain() {
        let request = MneionBindingSnippetRequest::new(STORE_UUID, TENANT_DOMAIN_UUID)
            .with_note("DASObjectStore development store");

        let export = export_mneion_binding_snippet(&request).expect("binding exports");

        assert_eq!(export.object_store_identifier, STORE_UUID);
        assert_eq!(
            export.endpoint_path,
            format!("/api/v1/admin/object-stores/{STORE_UUID}/link")
        );
        assert_eq!(
            export.object_store_link_request.governance_domain_id,
            TENANT_DOMAIN_UUID
        );
        assert_eq!(
            export.object_store_link_request.note.as_deref(),
            Some("DASObjectStore development store")
        );
        assert!(export.notes.iter().any(|note| note.contains("Limen")));
    }

    #[test]
    fn exports_reserved_internal_mneion_binding_snippet() {
        let request =
            MneionBindingSnippetRequest::new(STORE_UUID, INTERNAL_MNEION_GOVERNANCE_DOMAIN_ID);

        let export = export_mneion_binding_snippet(&request).expect("binding exports");

        assert_eq!(
            export.object_store_link_request.governance_domain_id,
            INTERNAL_MNEION_GOVERNANCE_DOMAIN_ID
        );
        assert!(export.object_store_link_request.note.is_none());
    }

    #[test]
    fn serializes_link_request_with_mneion_field_names() {
        let request = MneionBindingSnippetRequest::new(STORE_UUID, TENANT_DOMAIN_UUID)
            .with_note("DASObjectStore development store");

        let export = export_mneion_binding_snippet(&request).expect("binding exports");
        let serialized = serde_json::to_value(export.object_store_link_request)
            .expect("link request serializes");

        assert_eq!(
            serialized,
            json!({
                "governance_domain_id": TENANT_DOMAIN_UUID,
                "note": "DASObjectStore development store"
            })
        );
    }

    #[test]
    fn exports_managed_storage_binding_for_governance_domain() {
        let endpoint = MneionDasObjectStoreEndpoint::nfs_backed(
            STORE_UUID,
            "Managed NAS",
            "nas-export-1",
            "https://nas-gateway.local:3900",
        );
        let request = MneionManagedStorageBindingRequest::new(endpoint, TENANT_DOMAIN_UUID)
            .with_validation_evidence_schema("dasobjectstore.nas_nfs_runtime_validation_plan.v1")
            .with_health_state("healthy")
            .with_note("DASObjectStore NAS endpoint");

        let export =
            export_mneion_managed_storage_binding(&request).expect("managed binding exports");

        assert_eq!(
            export.endpoint_path,
            format!("/api/v1/admin/object-stores/{STORE_UUID}/link")
        );
        assert_eq!(
            export.object_store_link_request.governance_domain_id,
            TENANT_DOMAIN_UUID
        );
        assert_eq!(
            export.object_store_link_request.note.as_deref(),
            Some("DASObjectStore NAS endpoint")
        );
        assert_eq!(
            export.managed_binding_contract.storage_definition_id,
            STORE_UUID
        );
        assert_eq!(
            export.managed_binding_contract.endpoint_kind,
            "dasobjectstore_nfs"
        );
        assert_eq!(
            export.managed_binding_contract.manager_product_id,
            "dasobjectstore"
        );
        assert_eq!(
            export.managed_binding_contract.object_contract,
            "object_style"
        );
        assert!(!export.managed_binding_contract.raw_paths_are_tenant_facing);
        assert!(export
            .notes
            .iter()
            .any(|note| note.contains("governance-domain storage authority")));
    }

    #[test]
    fn serializes_managed_binding_contract_without_raw_paths() {
        let endpoint = MneionDasObjectStoreEndpoint::das_backed(
            STORE_UUID,
            "Managed DAS",
            "pool-1",
            "https://dasobjectstore.local:3900",
        );
        let request = MneionManagedStorageBindingRequest::new(endpoint, TENANT_DOMAIN_UUID)
            .with_validation_evidence_schema("dasobjectstore.pool_validation.v1");

        let export =
            export_mneion_managed_storage_binding(&request).expect("managed binding exports");
        let encoded = serde_json::to_value(export.managed_binding_contract)
            .expect("managed contract serializes");

        assert_eq!(
            encoded,
            json!({
                "storage_definition_id": STORE_UUID,
                "governance_domain_id": TENANT_DOMAIN_UUID,
                "endpoint_kind": "dasobjectstore_das",
                "manager_product_id": "dasobjectstore",
                "object_contract": "object_style",
                "binding_readiness": "ready",
                "validation_evidence_schema": "dasobjectstore.pool_validation.v1",
                "health_state": "healthy",
                "raw_paths_are_tenant_facing": false
            })
        );
        assert!(encoded.get("nfs_server").is_none());
        assert!(encoded.get("nfs_export_path").is_none());
    }

    #[test]
    fn rejects_degraded_managed_storage_binding_exports() {
        let endpoint = MneionDasObjectStoreEndpoint::nfs_backed(
            STORE_UUID,
            "Managed NAS",
            "nas-export-1",
            "https://nas-gateway.local:3900",
        );
        let request = MneionManagedStorageBindingRequest::new(endpoint, TENANT_DOMAIN_UUID)
            .with_readiness(MneionManagedBindingReadiness::Degraded);

        let err = export_mneion_managed_storage_binding(&request)
            .expect_err("degraded endpoint not binding-ready");

        assert_eq!(
            err,
            MneionBindingSnippetError::BindingNotReady {
                readiness: MneionManagedBindingReadiness::Degraded
            }
        );
    }

    #[test]
    fn trims_binding_snippet_inputs() {
        let request = MneionBindingSnippetRequest::new(
            format!(" {STORE_UUID} "),
            format!(" {TENANT_DOMAIN_UUID} "),
        )
        .with_note("  DASObjectStore development store  ");

        let export = export_mneion_binding_snippet(&request).expect("binding exports");

        assert_eq!(export.object_store_identifier, STORE_UUID);
        assert_eq!(
            export.object_store_link_request.governance_domain_id,
            TENANT_DOMAIN_UUID
        );
        assert_eq!(
            export.object_store_link_request.note.as_deref(),
            Some("DASObjectStore development store")
        );
    }

    #[test]
    fn rejects_non_uuid_object_store_identifiers() {
        let request = MneionBindingSnippetRequest::new("dasobjectstore-dev", TENANT_DOMAIN_UUID);

        let err = export_mneion_binding_snippet(&request).expect_err("identifier rejected");

        assert_eq!(
            err,
            MneionBindingSnippetError::InvalidObjectStoreIdentifier {
                value: "dasobjectstore-dev".to_string()
            }
        );
    }

    #[test]
    fn rejects_non_uuid_governance_domain_ids() {
        let request = MneionBindingSnippetRequest::new(STORE_UUID, "domain-a");

        let err = export_mneion_binding_snippet(&request).expect_err("domain rejected");

        assert_eq!(
            err,
            MneionBindingSnippetError::InvalidGovernanceDomainId {
                value: "domain-a".to_string()
            }
        );
    }
}
