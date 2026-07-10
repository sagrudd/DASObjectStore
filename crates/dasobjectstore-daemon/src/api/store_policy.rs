use crate::api::{DaemonJobAcceptedResponse, DaemonJobId, DaemonJobKind};
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_core::store::IngestMode;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

pub const DIRECT_TO_HDD_POLICY_CONFIRMATION: &str = "confirm direct hdd ingest";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UpdateObjectStoreIngestPolicyRequest {
    pub store_id: String,
    pub ingest_mode: String,
    #[serde(default)]
    pub dry_run: bool,
    pub client_request_id: Option<String>,
    pub administrator_actor: Option<String>,
    #[serde(default)]
    pub confirmation_marker: String,
}

impl UpdateObjectStoreIngestPolicyRequest {
    pub fn validate(&self) -> Result<IngestMode, UpdateObjectStoreIngestPolicyValidationError> {
        let store_id = StoreId::new(self.store_id.trim()).map_err(|_| {
            UpdateObjectStoreIngestPolicyValidationError::InvalidStoreId(self.store_id.clone())
        })?;
        if store_id.as_str() != self.store_id {
            return Err(
                UpdateObjectStoreIngestPolicyValidationError::InvalidStoreId(self.store_id.clone()),
            );
        }
        if self
            .client_request_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(UpdateObjectStoreIngestPolicyValidationError::BlankClientRequestId);
        }
        let mode = match self.ingest_mode.as_str() {
            "ssd_first" => IngestMode::SsdFirst,
            "direct_to_hdd" => IngestMode::DirectToHdd,
            _ => {
                return Err(
                    UpdateObjectStoreIngestPolicyValidationError::InvalidIngestMode(
                        self.ingest_mode.clone(),
                    ),
                )
            }
        };
        if mode == IngestMode::DirectToHdd
            && self.confirmation_marker.trim() != DIRECT_TO_HDD_POLICY_CONFIRMATION
        {
            return Err(UpdateObjectStoreIngestPolicyValidationError::ConfirmationMismatch);
        }
        Ok(mode)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UpdateObjectStoreIngestPolicyResponse {
    pub accepted: DaemonJobAcceptedResponse,
    pub store_id: StoreId,
    pub previous_ingest_mode: IngestMode,
    pub ingest_mode: IngestMode,
    pub changed: bool,
    pub administrator_actor: Option<String>,
}

impl UpdateObjectStoreIngestPolicyResponse {
    pub fn accepted(
        job_id: DaemonJobId,
        accepted_at_utc: impl Into<String>,
        request: &UpdateObjectStoreIngestPolicyRequest,
        previous_ingest_mode: IngestMode,
        ingest_mode: IngestMode,
        store_id: StoreId,
    ) -> Self {
        Self {
            accepted: DaemonJobAcceptedResponse {
                job_id,
                kind: DaemonJobKind::SystemAdministration,
                accepted_at_utc: accepted_at_utc.into(),
                dry_run: request.dry_run,
            },
            store_id,
            previous_ingest_mode,
            ingest_mode,
            changed: previous_ingest_mode != ingest_mode,
            administrator_actor: request.administrator_actor.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UpdateObjectStoreIngestPolicyValidationError {
    InvalidStoreId(String),
    InvalidIngestMode(String),
    BlankClientRequestId,
    ConfirmationMismatch,
}

impl Display for UpdateObjectStoreIngestPolicyValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidStoreId(value) => write!(formatter, "invalid store_id: {value}"),
            Self::InvalidIngestMode(value) => write!(formatter, "unsupported ingest_mode: {value}"),
            Self::BlankClientRequestId => formatter.write_str("client_request_id must not be blank"),
            Self::ConfirmationMismatch => write!(
                formatter,
                "confirmation_marker must exactly match \"{DIRECT_TO_HDD_POLICY_CONFIRMATION}\" when selecting direct_to_hdd"
            ),
        }
    }
}

impl std::error::Error for UpdateObjectStoreIngestPolicyValidationError {}

#[cfg(test)]
mod tests {
    use super::{
        UpdateObjectStoreIngestPolicyRequest, UpdateObjectStoreIngestPolicyValidationError,
        DIRECT_TO_HDD_POLICY_CONFIRMATION,
    };

    fn request(mode: &str) -> UpdateObjectStoreIngestPolicyRequest {
        UpdateObjectStoreIngestPolicyRequest {
            store_id: "zymo".to_string(),
            ingest_mode: mode.to_string(),
            dry_run: false,
            client_request_id: Some("policy-1".to_string()),
            administrator_actor: Some("stephen".to_string()),
            confirmation_marker: String::new(),
        }
    }

    #[test]
    fn ssd_first_does_not_require_risky_confirmation() {
        assert!(request("ssd_first").validate().is_ok());
    }

    #[test]
    fn direct_to_hdd_requires_exact_confirmation() {
        let mut request = request("direct_to_hdd");
        assert_eq!(
            request.validate(),
            Err(UpdateObjectStoreIngestPolicyValidationError::ConfirmationMismatch)
        );
        request.confirmation_marker = DIRECT_TO_HDD_POLICY_CONFIRMATION.to_string();
        assert!(request.validate().is_ok());
    }
}
