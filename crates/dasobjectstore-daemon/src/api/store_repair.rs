use dasobjectstore_core::ids::StoreId;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

pub const STORE_REPAIR_CONFIRMATION: &str = "confirm store repair";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreRepairRequest {
    pub store_id: Option<StoreId>,
    pub dry_run: bool,
    pub confirmation: String,
    /// Fetch uncatalogued objects from the provisioned Garage bucket and ingest
    /// them through SSD staging before scanning managed payloads.
    #[serde(default)]
    pub reconcile_s3: bool,
    #[serde(default)]
    pub s3_prefix: Option<String>,
}

impl StoreRepairRequest {
    pub fn validate(&self) -> Result<(), StoreRepairValidationError> {
        if !self.dry_run && self.confirmation != STORE_REPAIR_CONFIRMATION {
            return Err(StoreRepairValidationError::ConfirmationMismatch);
        }
        if self.reconcile_s3 && self.store_id.is_none() {
            return Err(StoreRepairValidationError::StoreRequiredForS3Reconciliation);
        }
        if self
            .s3_prefix
            .as_deref()
            .is_some_and(|prefix| prefix.trim().is_empty())
        {
            return Err(StoreRepairValidationError::BlankS3Prefix);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreRepairResponse {
    pub report: StoreRepairReport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub s3_reconciliation: Option<StoreRepairS3Reconciliation>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreRepairS3Reconciliation {
    pub bucket_name: String,
    pub prefix: Option<String>,
    pub staging_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_path: Option<String>,
    pub ingest_job_id: Option<String>,
    pub dry_run: bool,
    #[serde(default)]
    pub completed_snapshot_outcome: CompletedSnapshotOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome_detail: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletedSnapshotOutcome {
    #[default]
    NotApplicable,
    CompletedSnapshotAdopted,
    AlreadyDurable,
    RetainedUnsafe,
    Reclaimed,
}

impl CompletedSnapshotOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotApplicable => "not_applicable",
            Self::CompletedSnapshotAdopted => "completed_snapshot_adopted",
            Self::AlreadyDurable => "already_durable",
            Self::RetainedUnsafe => "retained_unsafe",
            Self::Reclaimed => "reclaimed",
        }
    }
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
    StoreRequiredForS3Reconciliation,
    BlankS3Prefix,
}

impl Display for StoreRepairValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConfirmationMismatch => write!(
                formatter,
                "store repair requires confirmation phrase: {STORE_REPAIR_CONFIRMATION}"
            ),
            Self::StoreRequiredForS3Reconciliation => {
                formatter.write_str("S3 reconciliation requires a single ObjectStore identifier")
            }
            Self::BlankS3Prefix => {
                formatter.write_str("S3 reconciliation prefix must not be blank")
            }
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
            reconcile_s3: false,
            s3_prefix: None,
        };
        assert_eq!(
            request.validate(),
            Err(StoreRepairValidationError::ConfirmationMismatch)
        );
        let request = StoreRepairRequest {
            store_id: None,
            dry_run: false,
            confirmation: STORE_REPAIR_CONFIRMATION.to_string(),
            reconcile_s3: false,
            s3_prefix: None,
        };
        assert!(request.validate().is_ok());
    }
}
