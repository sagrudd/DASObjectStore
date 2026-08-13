use dasobjectstore_metadata::DestageRetryReport;
use serde::{Deserialize, Serialize};

pub const DESTAGE_RETRY_CONFIRMATION: &str = "confirm retry needs-review destage";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DestageRetryRequest {
    pub store_id: String,
    /// Explicit optimistic guard. Only `needs_review` is accepted.
    pub from_state: String,
    pub dry_run: bool,
    pub allow_destage_retry: bool,
    pub confirmation_marker: String,
    /// Subject copied only by the fixed host service after Pistis has
    /// authenticated the human administrator.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_subject: Option<String>,
}

impl DestageRetryRequest {
    pub fn validate(&self) -> Result<(), DestageRetryValidationError> {
        if self.store_id.trim().is_empty() {
            return Err(DestageRetryValidationError::BlankStoreId);
        }
        if self.from_state != "needs_review" {
            return Err(DestageRetryValidationError::UnsafeFromState);
        }
        if !self.dry_run && self.confirmation_marker != DESTAGE_RETRY_CONFIRMATION {
            return Err(DestageRetryValidationError::ConfirmationMismatch);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DestageRetryResponse {
    pub report: DestageRetryReport,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum DestageRetryValidationError {
    BlankStoreId,
    UnsafeFromState,
    ConfirmationMismatch,
}

impl std::fmt::Display for DestageRetryValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BlankStoreId => formatter.write_str("store_id must not be blank"),
            Self::UnsafeFromState => formatter.write_str("from_state must equal \"needs_review\""),
            Self::ConfirmationMismatch => write!(
                formatter,
                "confirmation_marker must equal {DESTAGE_RETRY_CONFIRMATION:?}"
            ),
        }
    }
}

impl std::error::Error for DestageRetryValidationError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_broad_state_and_requires_confirmation_for_apply() {
        let mut request = DestageRetryRequest {
            store_id: "archive".to_string(),
            from_state: "destage_failed".to_string(),
            dry_run: false,
            allow_destage_retry: true,
            confirmation_marker: DESTAGE_RETRY_CONFIRMATION.to_string(),
            verified_subject: None,
        };
        assert_eq!(
            request.validate(),
            Err(DestageRetryValidationError::UnsafeFromState)
        );
        request.from_state = "needs_review".to_string();
        request.confirmation_marker.clear();
        assert_eq!(
            request.validate(),
            Err(DestageRetryValidationError::ConfirmationMismatch)
        );
    }
}
