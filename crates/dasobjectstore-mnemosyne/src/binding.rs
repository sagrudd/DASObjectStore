use crate::validation::is_uuid_like;
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MneionBindingSnippetError {
    InvalidObjectStoreIdentifier { value: String },
    InvalidGovernanceDomainId { value: String },
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

#[cfg(test)]
mod tests {
    use super::{
        export_mneion_binding_snippet, MneionBindingSnippetError, MneionBindingSnippetRequest,
        INTERNAL_MNEION_GOVERNANCE_DOMAIN_ID,
    };
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
