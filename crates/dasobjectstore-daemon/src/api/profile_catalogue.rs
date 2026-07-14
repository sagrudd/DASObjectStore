use crate::api::DaemonRequestValidationError;
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_core::object_catalogue::PortableObjectCatalogue;
use serde::{Deserialize, Serialize};

pub const PROFILE_CATALOGUE_SCHEMA_VERSION: &str = "dasobjectstore.profile_catalogue.v1";

/// Export the daemon-authoritative logical catalogue for a bounded profile.
/// The response is portable metadata only; payload paths and credentials never
/// cross the client boundary.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileCatalogueExportRequest {
    pub store_id: StoreId,
}

impl ProfileCatalogueExportRequest {
    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        if self.store_id.as_str().trim().is_empty() {
            return Err(DaemonRequestValidationError::BlankField { field: "store_id" });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileCatalogueExportResponse {
    pub schema_version: String,
    pub store_id: StoreId,
    pub catalogue: PortableObjectCatalogue,
}

impl ProfileCatalogueExportResponse {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != PROFILE_CATALOGUE_SCHEMA_VERSION {
            return Err("unsupported profile catalogue schema".to_string());
        }
        if self.store_id != self.catalogue.store_id {
            return Err("profile catalogue response store identity mismatch".to_string());
        }
        self.catalogue
            .validate()
            .map_err(|error| format!("invalid profile catalogue response: {error}"))
    }
}

/// Import is a verified catalogue handoff, not an untrusted metadata merge.
/// The daemon checks destination payloads and only then commits catalogue rows.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileCatalogueImportRequest {
    pub store_id: StoreId,
    /// Stable caller-owned transaction identity used for replay-safe handoff.
    pub transaction_id: String,
    /// Private logical namespace; backend paths never belong in this field.
    pub profile_namespace: String,
    pub catalogue: PortableObjectCatalogue,
}

impl ProfileCatalogueImportRequest {
    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        if self.store_id.as_str().trim().is_empty() {
            return Err(DaemonRequestValidationError::BlankField { field: "store_id" });
        }
        if self.transaction_id.trim().is_empty() {
            return Err(DaemonRequestValidationError::BlankField {
                field: "transaction_id",
            });
        }
        if self.profile_namespace.trim().is_empty() {
            return Err(DaemonRequestValidationError::BlankField {
                field: "profile_namespace",
            });
        }
        self.catalogue
            .validate()
            .map_err(|error| DaemonRequestValidationError::InvalidPolicy {
                message: format!("invalid portable profile catalogue: {error}"),
            })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileCatalogueImportResponse {
    pub schema_version: String,
    pub store_id: StoreId,
    pub imported_objects: u64,
    /// Import never retires the source; a separate migration transition is
    /// required after destination verification and operator confirmation.
    pub source_retained: bool,
}

impl ProfileCatalogueImportResponse {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != PROFILE_CATALOGUE_SCHEMA_VERSION {
            return Err("unsupported profile catalogue schema".to_string());
        }
        if self.store_id.as_str().trim().is_empty() || !self.source_retained {
            return Err("profile catalogue import must retain the source".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_response_requires_source_retention() {
        let response = ProfileCatalogueImportResponse {
            schema_version: PROFILE_CATALOGUE_SCHEMA_VERSION.to_string(),
            store_id: StoreId::new("codex").expect("store id"),
            imported_objects: 1,
            source_retained: false,
        };
        assert!(response.validate().is_err());
    }

    #[test]
    fn import_request_requires_explicit_replay_identity() {
        let request = ProfileCatalogueImportRequest {
            store_id: StoreId::new("codex").expect("store id"),
            transaction_id: String::new(),
            profile_namespace: "folder:codex".to_string(),
            catalogue: PortableObjectCatalogue {
                schema_version: 1,
                store_id: StoreId::new("codex").expect("store id"),
                objects: Vec::new(),
            },
        };
        assert!(request.validate().is_err());
    }
}
