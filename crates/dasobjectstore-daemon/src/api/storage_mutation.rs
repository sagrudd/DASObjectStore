use dasobjectstore_metadata::{StoreDeleteReport as MetadataStoreDeleteReport, StoreDrainReport};
use dasobjectstore_object_service::{
    StoreRegistryDeleteReport, SubObjectRegistryStoreDeleteReport,
};
use serde::{Deserialize, Serialize};

pub const STORE_DRAIN_CONFIRMATION: &str = "confirm store drain";
pub const STORE_DELETE_CONFIRMATION: &str = "confirm store delete";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreDrainRequest {
    pub store_id: String,
    pub dry_run: bool,
    pub allow_store_drain: bool,
    pub confirmation_marker: String,
}

impl StoreDrainRequest {
    pub fn validate(&self) -> Result<(), StoreDrainValidationError> {
        if self.store_id.trim().is_empty() {
            return Err(StoreDrainValidationError::BlankField { field: "store_id" });
        }
        if !self.dry_run && self.confirmation_marker != STORE_DRAIN_CONFIRMATION {
            return Err(StoreDrainValidationError::ConfirmationMismatch);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreDrainResponse {
    pub report: StoreDrainReport,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreDeleteRequest {
    pub store_id: String,
    pub dry_run: bool,
    pub allow_store_delete: bool,
    pub confirmation_marker: String,
}

impl StoreDeleteRequest {
    pub fn validate(&self) -> Result<(), StoreDeleteValidationError> {
        if self.store_id.trim().is_empty() {
            return Err(StoreDeleteValidationError::BlankField { field: "store_id" });
        }
        if !self.dry_run && self.confirmation_marker != STORE_DELETE_CONFIRMATION {
            return Err(StoreDeleteValidationError::ConfirmationMismatch);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreDeleteResponse {
    pub report: StoreDeleteCommandReport,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreDeleteCommandReport {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_retirement: Option<ProfileRetirementReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<MetadataStoreDeleteReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_registry: Option<StoreRegistryDeleteReport>,
    pub portable_registry: Option<StoreRegistryDeleteReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_subobjects: Option<SubObjectRegistryStoreDeleteReport>,
    pub portable_subobjects: Option<SubObjectRegistryStoreDeleteReport>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProfileRetirementReport {
    pub store_id: String,
    pub dry_run: bool,
    pub already_retired: bool,
    pub shared_objects_removed: usize,
    pub shared_transactions_removed: usize,
    pub private_catalogue_retained: bool,
    pub payloads_retained: bool,
    pub quota_ledger_retained: bool,
    pub registry_definition_retained: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum StoreDeleteValidationError {
    BlankField { field: &'static str },
    ConfirmationMismatch,
}

impl std::fmt::Display for StoreDeleteValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BlankField { field } => write!(formatter, "{field} must not be blank"),
            Self::ConfirmationMismatch => write!(
                formatter,
                "confirmation_marker must equal {STORE_DELETE_CONFIRMATION:?}"
            ),
        }
    }
}

impl std::error::Error for StoreDeleteValidationError {}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum StoreDrainValidationError {
    BlankField { field: &'static str },
    ConfirmationMismatch,
}

impl std::fmt::Display for StoreDrainValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BlankField { field } => write!(formatter, "{field} must not be blank"),
            Self::ConfirmationMismatch => write!(
                formatter,
                "confirmation_marker must equal {STORE_DRAIN_CONFIRMATION:?}"
            ),
        }
    }
}

impl std::error::Error for StoreDrainValidationError {}

#[cfg(test)]
mod tests {
    use super::{
        StoreDeleteRequest, StoreDeleteValidationError, StoreDrainRequest,
        StoreDrainValidationError, STORE_DELETE_CONFIRMATION, STORE_DRAIN_CONFIRMATION,
    };

    fn request() -> StoreDrainRequest {
        StoreDrainRequest {
            store_id: "archive".to_string(),
            dry_run: false,
            allow_store_drain: true,
            confirmation_marker: STORE_DRAIN_CONFIRMATION.to_string(),
        }
    }

    #[test]
    fn destructive_drain_requires_confirmation() {
        let mut request = request();
        request.confirmation_marker.clear();

        assert_eq!(
            request.validate(),
            Err(StoreDrainValidationError::ConfirmationMismatch)
        );
    }

    #[test]
    fn dry_run_allows_missing_confirmation_for_read_only_preview() {
        let mut request = request();
        request.dry_run = true;
        request.confirmation_marker.clear();

        assert_eq!(request.validate(), Ok(()));
    }

    #[test]
    fn destructive_delete_requires_confirmation() {
        let request = StoreDeleteRequest {
            store_id: "archive".to_string(),
            dry_run: false,
            allow_store_delete: true,
            confirmation_marker: String::new(),
        };

        assert_eq!(
            request.validate(),
            Err(StoreDeleteValidationError::ConfirmationMismatch)
        );
    }

    #[test]
    fn dry_run_delete_allows_missing_confirmation() {
        let request = StoreDeleteRequest {
            store_id: "archive".to_string(),
            dry_run: true,
            allow_store_delete: false,
            confirmation_marker: String::new(),
        };

        assert_eq!(request.validate(), Ok(()));
        assert_eq!(STORE_DELETE_CONFIRMATION, "confirm store delete");
    }
}
