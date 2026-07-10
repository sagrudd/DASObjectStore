use dasobjectstore_core::ids::StoreId;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

pub const STORE_DEDUPLICATE_CONFIRMATION: &str = "confirm store deduplicate";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreDeduplicateRequest {
    pub store_id: Option<StoreId>,
    pub dry_run: bool,
    pub confirmation: String,
}

impl StoreDeduplicateRequest {
    pub fn validate(&self) -> Result<(), StoreDeduplicateValidationError> {
        if !self.dry_run && self.confirmation != STORE_DEDUPLICATE_CONFIRMATION {
            return Err(StoreDeduplicateValidationError::ConfirmationMismatch);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreDeduplicateResponse {
    pub report: StoreDeduplicateReport,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreDeduplicateReport {
    pub metadata_path: String,
    pub dry_run: bool,
    pub payloads_hashed: u64,
    pub hash_errors: u64,
    pub duplicate_content_groups: u64,
    pub duplicate_placement_rows: u64,
    pub metadata_rows_removed: u64,
    pub hashes_recorded: u64,
    pub warning: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StoreDeduplicateValidationError {
    ConfirmationMismatch,
}

impl Display for StoreDeduplicateValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConfirmationMismatch => write!(
                formatter,
                "store deduplicate requires confirmation phrase: {STORE_DEDUPLICATE_CONFIRMATION}"
            ),
        }
    }
}

impl std::error::Error for StoreDeduplicateValidationError {}

#[cfg(test)]
mod tests {
    use super::{
        StoreDeduplicateRequest, StoreDeduplicateValidationError, STORE_DEDUPLICATE_CONFIRMATION,
    };

    #[test]
    fn apply_requires_explicit_confirmation() {
        let request = StoreDeduplicateRequest {
            store_id: None,
            dry_run: false,
            confirmation: String::new(),
        };
        assert_eq!(
            request.validate(),
            Err(StoreDeduplicateValidationError::ConfirmationMismatch)
        );
        let request = StoreDeduplicateRequest {
            store_id: None,
            dry_run: false,
            confirmation: STORE_DEDUPLICATE_CONFIRMATION.to_string(),
        };
        assert!(request.validate().is_ok());
    }
}
