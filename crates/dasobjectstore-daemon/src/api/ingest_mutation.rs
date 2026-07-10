use dasobjectstore_metadata::IngestQueueDrainReport;
use serde::{Deserialize, Serialize};

pub const INGEST_QUEUE_DRAIN_CONFIRMATION: &str = "confirm ingest queue drain";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IngestQueueDrainRequest {
    pub store_id: String,
    pub reason: String,
    pub dry_run: bool,
    pub allow_ingest_queue_drain: bool,
    pub confirmation_marker: String,
}

impl IngestQueueDrainRequest {
    pub fn validate(&self) -> Result<(), IngestQueueDrainValidationError> {
        if self.store_id.trim().is_empty() {
            return Err(IngestQueueDrainValidationError::BlankField { field: "store_id" });
        }
        if self.reason.trim().is_empty() {
            return Err(IngestQueueDrainValidationError::BlankField { field: "reason" });
        }
        if !self.dry_run && self.confirmation_marker != INGEST_QUEUE_DRAIN_CONFIRMATION {
            return Err(IngestQueueDrainValidationError::ConfirmationMismatch);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IngestQueueDrainResponse {
    pub report: IngestQueueDrainReport,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum IngestQueueDrainValidationError {
    BlankField { field: &'static str },
    ConfirmationMismatch,
}

impl std::fmt::Display for IngestQueueDrainValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BlankField { field } => write!(formatter, "{field} must not be blank"),
            Self::ConfirmationMismatch => write!(
                formatter,
                "confirmation_marker must equal {INGEST_QUEUE_DRAIN_CONFIRMATION:?}"
            ),
        }
    }
}

impl std::error::Error for IngestQueueDrainValidationError {}

#[cfg(test)]
mod tests {
    use super::{
        IngestQueueDrainRequest, IngestQueueDrainValidationError, INGEST_QUEUE_DRAIN_CONFIRMATION,
    };

    fn request() -> IngestQueueDrainRequest {
        IngestQueueDrainRequest {
            store_id: "archive".to_string(),
            reason: "operator requested drain".to_string(),
            dry_run: false,
            allow_ingest_queue_drain: true,
            confirmation_marker: INGEST_QUEUE_DRAIN_CONFIRMATION.to_string(),
        }
    }

    #[test]
    fn destructive_queue_drain_requires_confirmation() {
        let mut request = request();
        request.confirmation_marker.clear();

        assert_eq!(
            request.validate(),
            Err(IngestQueueDrainValidationError::ConfirmationMismatch)
        );
    }
}
