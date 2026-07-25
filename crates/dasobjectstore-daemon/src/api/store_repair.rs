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
    /// Optional exact three-object remote reconciliation contract. Local
    /// administrative repairs may omit it; remote callers must provide it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub s3_expectation: Option<StoreRepairS3Expectation>,
    /// Stable caller identity used to recover an ambiguous response without
    /// creating another reconciliation operation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StoreRepairS3Expectation {
    pub payload_key: String,
    pub expected_bytes: u64,
    pub expected_sha256: String,
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
        if let Some(expectation) = &self.s3_expectation {
            let expected_prefix = self.s3_prefix.as_deref().unwrap_or_default();
            if expectation.payload_key.trim().is_empty()
                || expectation.payload_key != expected_prefix
                || expectation.expected_bytes == 0
                || expectation.expected_sha256.len() != 64
                || !expectation
                    .expected_sha256
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(StoreRepairValidationError::InvalidS3Expectation);
            }
        }
        if self
            .idempotency_key
            .as_deref()
            .is_some_and(|value| value.trim().is_empty() || value.len() > 200)
        {
            return Err(StoreRepairValidationError::InvalidIdempotencyKey);
        }
        Ok(())
    }

    pub fn reconciliation_operation_id(
        &self,
        accepted_at_utc: &str,
    ) -> Result<super::DaemonJobId, super::DaemonJobIdError> {
        use sha2::{Digest, Sha256};
        let store_id = self
            .store_id
            .as_ref()
            .map(StoreId::as_str)
            .unwrap_or("unknown");
        let suffix = if let Some(idempotency_key) = self.idempotency_key.as_deref() {
            let mut digest = Sha256::new();
            digest.update(b"dasobjectstore.remote-reconciliation.v1\0");
            digest.update(store_id.as_bytes());
            digest.update(b"\0");
            digest.update(self.s3_prefix.as_deref().unwrap_or_default().as_bytes());
            digest.update(b"\0");
            digest.update(idempotency_key.as_bytes());
            if let Some(expectation) = &self.s3_expectation {
                digest.update(b"\0");
                digest.update(expectation.expected_bytes.to_be_bytes());
                digest.update(expectation.expected_sha256.as_bytes());
            }
            format!("{:x}", digest.finalize())[..24].to_string()
        } else {
            accepted_at_utc
                .chars()
                .map(|character| {
                    if character.is_ascii_alphanumeric() {
                        character
                    } else {
                        '-'
                    }
                })
                .collect::<String>()
                .trim_matches('-')
                .to_ascii_lowercase()
        };
        super::DaemonJobId::new(format!("store-repair-s3-{store_id}-{suffix}"))
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
    InvalidS3Expectation,
    InvalidIdempotencyKey,
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
            Self::InvalidS3Expectation => formatter.write_str(
                "S3 reconciliation expectation must match the exact payload key, positive size, and SHA-256",
            ),
            Self::InvalidIdempotencyKey => {
                formatter.write_str("S3 reconciliation idempotency key is invalid")
            }
        }
    }
}

impl std::error::Error for StoreRepairValidationError {}

#[cfg(test)]
mod tests {
    use super::{
        StoreRepairRequest, StoreRepairS3Expectation, StoreRepairValidationError,
        STORE_REPAIR_CONFIRMATION,
    };
    use dasobjectstore_core::ids::StoreId;

    #[test]
    fn apply_requires_explicit_confirmation() {
        let request = StoreRepairRequest {
            store_id: None,
            dry_run: false,
            confirmation: String::new(),
            reconcile_s3: false,
            s3_prefix: None,
            s3_expectation: None,
            idempotency_key: None,
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
            s3_expectation: None,
            idempotency_key: None,
        };
        assert!(request.validate().is_ok());
    }

    #[test]
    fn exact_remote_group_requires_matching_size_digest_and_prefix() {
        let request = StoreRepairRequest {
            store_id: Some(StoreId::new("epic_collection").expect("store")),
            dry_run: false,
            confirmation: STORE_REPAIR_CONFIRMATION.to_string(),
            reconcile_s3: true,
            s3_prefix: Some("EPICv1/GSE224365_RAW.tar".to_string()),
            s3_expectation: Some(StoreRepairS3Expectation {
                payload_key: "EPICv1/GSE224365_RAW.tar".to_string(),
                expected_bytes: 10_705_582_080,
                expected_sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                    .to_string(),
            }),
            idempotency_key: Some("epic-GSE224365-v1".to_string()),
        };
        request.validate().expect("valid remote reconciliation");
        let first = request
            .reconciliation_operation_id("2026-07-25T10:00:00Z")
            .expect("operation");
        let retry = request
            .reconciliation_operation_id("2026-07-25T11:00:00Z")
            .expect("operation");
        assert_eq!(first, retry);

        let mut wrong = request;
        wrong.s3_expectation.as_mut().unwrap().expected_bytes = 0;
        assert_eq!(
            wrong.validate(),
            Err(StoreRepairValidationError::InvalidS3Expectation)
        );
    }
}
