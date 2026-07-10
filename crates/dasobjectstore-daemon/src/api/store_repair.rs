use dasobjectstore_core::ids::StoreId;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

pub const STORE_REPAIR_CONFIRMATION: &str = "confirm store repair";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreRepairRequest {
    pub store_id: Option<StoreId>,
    pub dry_run: bool,
    pub confirmation: String,
}

impl StoreRepairRequest {
    pub fn validate(&self) -> Result<(), StoreRepairValidationError> {
        if !self.dry_run && self.confirmation != STORE_REPAIR_CONFIRMATION {
            return Err(StoreRepairValidationError::ConfirmationMismatch);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreRepairResponse {
    pub report: StoreRepairReport,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreRepairReport {
    pub metadata_path: String,
    pub backup_path: Option<String>,
    pub dry_run: bool,
    pub stores_scanned: usize,
    pub payload_files: u64,
    pub objects_recovered: u64,
    pub placements_recovered: u64,
    pub payload_bytes: u64,
    pub partial_duplicates_omitted: u64,
    pub hashes_verified: bool,
    pub warning: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StoreRepairValidationError {
    ConfirmationMismatch,
}

impl Display for StoreRepairValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConfirmationMismatch => write!(
                formatter,
                "store repair requires confirmation phrase: {STORE_REPAIR_CONFIRMATION}"
            ),
        }
    }
}

impl std::error::Error for StoreRepairValidationError {}

#[cfg(test)]
mod tests {
    use super::{StoreRepairRequest, StoreRepairValidationError, STORE_REPAIR_CONFIRMATION};

    #[test]
    fn apply_requires_explicit_confirmation() {
        let request = StoreRepairRequest {
            store_id: None,
            dry_run: false,
            confirmation: String::new(),
        };
        assert_eq!(
            request.validate(),
            Err(StoreRepairValidationError::ConfirmationMismatch)
        );
        let request = StoreRepairRequest {
            store_id: None,
            dry_run: false,
            confirmation: STORE_REPAIR_CONFIRMATION.to_string(),
        };
        assert!(request.validate().is_ok());
    }
}
