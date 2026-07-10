use dasobjectstore_metadata::StoreDrainReport;
use serde::{Deserialize, Serialize};

pub const STORE_DRAIN_CONFIRMATION: &str = "confirm store drain";

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
    use super::{StoreDrainRequest, StoreDrainValidationError, STORE_DRAIN_CONFIRMATION};

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
}
