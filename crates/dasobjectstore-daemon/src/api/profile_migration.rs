use crate::api::{DaemonJobAcceptedResponse, DaemonJobId, DaemonJobKind};
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_core::migration::MigrationState;
use serde::{Deserialize, Serialize};

pub const PROFILE_MIGRATION_CONFIRMATION: &str = "confirm profile migration";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileMigrationRequest {
    pub migration_id: String,
    pub source_store_id: String,
    pub destination_store_id: String,
    #[serde(default)]
    pub client_request_id: Option<String>,
    #[serde(default)]
    pub administrator_actor: Option<String>,
    pub confirmation_marker: String,
}

impl ProfileMigrationRequest {
    pub fn validate(&self) -> Result<(), ProfileMigrationValidationError> {
        if self.migration_id.is_empty()
            || self.migration_id.len() > 128
            || !self
                .migration_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(ProfileMigrationValidationError::InvalidMigrationId(
                "migration_id must contain only 1-128 ASCII letters, digits, '-' or '_'"
                    .to_string(),
            ));
        }
        let source = StoreId::new(self.source_store_id.clone())
            .map_err(|error| ProfileMigrationValidationError::InvalidStoreId(error.to_string()))?;
        let destination = StoreId::new(self.destination_store_id.clone())
            .map_err(|error| ProfileMigrationValidationError::InvalidStoreId(error.to_string()))?;
        if source == destination {
            return Err(ProfileMigrationValidationError::SameSourceAndDestination);
        }
        if self
            .client_request_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(ProfileMigrationValidationError::BlankClientRequestId);
        }
        if self
            .administrator_actor
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(ProfileMigrationValidationError::BlankAdministratorActor);
        }
        if self.confirmation_marker.trim() != PROFILE_MIGRATION_CONFIRMATION {
            return Err(ProfileMigrationValidationError::ConfirmationMismatch);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileMigrationResponse {
    pub accepted: DaemonJobAcceptedResponse,
    pub migration_id: String,
    pub source_store_id: String,
    pub destination_store_id: String,
    pub verified_object_count: u64,
    pub destination_used_bytes: u64,
    pub state: MigrationState,
    pub source_retained: bool,
    pub administrator_actor: String,
}

impl ProfileMigrationResponse {
    #[allow(clippy::too_many_arguments)]
    pub fn completed(
        job_id: DaemonJobId,
        accepted_at_utc: impl Into<String>,
        request: ProfileMigrationRequest,
        verified_object_count: u64,
        destination_used_bytes: u64,
        state: MigrationState,
        source_retained: bool,
        administrator_actor: String,
    ) -> Self {
        Self {
            accepted: DaemonJobAcceptedResponse {
                job_id,
                kind: DaemonJobKind::ProfileMigration,
                accepted_at_utc: accepted_at_utc.into(),
                dry_run: false,
            },
            migration_id: request.migration_id,
            source_store_id: request.source_store_id,
            destination_store_id: request.destination_store_id,
            verified_object_count,
            destination_used_bytes,
            state,
            source_retained,
            administrator_actor,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProfileMigrationValidationError {
    InvalidMigrationId(String),
    InvalidStoreId(String),
    SameSourceAndDestination,
    BlankClientRequestId,
    BlankAdministratorActor,
    ConfirmationMismatch,
}

impl std::fmt::Display for ProfileMigrationValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMigrationId(message) | Self::InvalidStoreId(message) => {
                formatter.write_str(message)
            }
            Self::SameSourceAndDestination => {
                formatter.write_str("migration source and destination stores must differ")
            }
            Self::BlankClientRequestId => {
                formatter.write_str("client_request_id must not be blank")
            }
            Self::BlankAdministratorActor => {
                formatter.write_str("administrator_actor must not be blank")
            }
            Self::ConfirmationMismatch => write!(
                formatter,
                "confirmation_marker must exactly match \"{PROFILE_MIGRATION_CONFIRMATION}\""
            ),
        }
    }
}

impl std::error::Error for ProfileMigrationValidationError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> ProfileMigrationRequest {
        ProfileMigrationRequest {
            migration_id: "promotion-1".to_string(),
            source_store_id: "source-store".to_string(),
            destination_store_id: "destination-store".to_string(),
            client_request_id: Some("request-1".to_string()),
            administrator_actor: None,
            confirmation_marker: PROFILE_MIGRATION_CONFIRMATION.to_string(),
        }
    }

    #[test]
    fn request_is_path_free_and_strict() {
        let request = request();
        request.validate().expect("request");
        let encoded = serde_json::to_value(request).expect("JSON");
        assert!(encoded.get("backend_root").is_none());
        assert!(encoded.get("checkpoint_path").is_none());
        let mut encoded = encoded;
        encoded["unexpected"] = serde_json::json!(true);
        assert!(serde_json::from_value::<ProfileMigrationRequest>(encoded).is_err());
    }

    #[test]
    fn rejects_unsafe_transaction_and_same_store() {
        let mut unsafe_request = request();
        unsafe_request.migration_id = "../escape".to_string();
        assert!(matches!(
            unsafe_request.validate(),
            Err(ProfileMigrationValidationError::InvalidMigrationId(_))
        ));
        let mut request = request();
        request.destination_store_id = request.source_store_id.clone();
        assert_eq!(
            request.validate(),
            Err(ProfileMigrationValidationError::SameSourceAndDestination)
        );
    }
}
