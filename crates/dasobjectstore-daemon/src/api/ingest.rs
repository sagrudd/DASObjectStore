use crate::api::health::DaemonSsdPressure;
use dasobjectstore_core::ids::{DiskId, IngestJobId, ObjectId, StoreId};
use dasobjectstore_core::lifecycle::IngestJobState;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SubmitIngestFilesRequest {
    pub endpoint: StoreId,
    pub source_path: PathBuf,
    pub copies: Option<u8>,
    pub dry_run: bool,
    pub client_request_id: Option<String>,
}

impl SubmitIngestFilesRequest {
    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        if !self.source_path.is_absolute() {
            return Err(DaemonRequestValidationError::RelativeSourcePath {
                path: self.source_path.clone(),
            });
        }
        if self.copies == Some(0) {
            return Err(DaemonRequestValidationError::InvalidCopyCount { copies: 0 });
        }
        if self
            .client_request_id
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(DaemonRequestValidationError::BlankClientRequestId);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SubmitIngestFilesResponse {
    pub job_id: IngestJobId,
    pub accepted_at_utc: String,
    pub dry_run: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IngestJobStatusRequest {
    pub job_id: IngestJobId,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IngestJobStatusResponse {
    pub job_id: IngestJobId,
    pub endpoint: StoreId,
    pub state: IngestJobState,
    pub progress: DaemonIngestProgressEvent,
    pub updated_at_utc: String,
    pub failure_message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CancelIngestJobRequest {
    pub job_id: IngestJobId,
    pub reason: Option<String>,
}

impl CancelIngestJobRequest {
    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        if self
            .reason
            .as_deref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(DaemonRequestValidationError::BlankCancellationReason);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CancelIngestJobResponse {
    pub job_id: IngestJobId,
    pub accepted: bool,
    pub state: IngestJobState,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestProgressEvent {
    pub job_id: IngestJobId,
    pub endpoint: StoreId,
    pub stage: DaemonIngestStage,
    pub work_bytes_done: u64,
    pub work_bytes_total: Option<u64>,
    pub files_done: u64,
    pub files_total: Option<u64>,
    pub current_object_id: Option<ObjectId>,
    pub ssd_pressure: Option<DaemonSsdPressure>,
    pub message: Option<String>,
}

impl DaemonIngestProgressEvent {
    pub fn percent_complete(&self) -> Option<u8> {
        let total = self.work_bytes_total?;
        if total == 0 {
            return Some(100);
        }
        Some(((self.work_bytes_done.saturating_mul(100)) / total).min(100) as u8)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum DaemonIngestStage {
    Queued,
    SsdIngest,
    HddCopy { disk_id: DiskId, copy_number: u8 },
    Complete,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DaemonRequestValidationError {
    RelativeSourcePath { path: PathBuf },
    InvalidCopyCount { copies: u8 },
    BlankClientRequestId,
    BlankCancellationReason,
    UnsupportedServiceProvider { provider: String },
}

impl Display for DaemonRequestValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RelativeSourcePath { path } => {
                write!(
                    formatter,
                    "ingest source path must be absolute: {}",
                    path.display()
                )
            }
            Self::InvalidCopyCount { copies } => {
                write!(formatter, "copy count must be greater than zero: {copies}")
            }
            Self::BlankClientRequestId => {
                formatter.write_str("client_request_id must not be blank")
            }
            Self::BlankCancellationReason => {
                formatter.write_str("cancellation reason must not be blank")
            }
            Self::UnsupportedServiceProvider { provider } => write!(
                formatter,
                "unsupported object service provider for daemon lifecycle operation: {provider}"
            ),
        }
    }
}

impl std::error::Error for DaemonRequestValidationError {}

#[cfg(test)]
mod tests {
    use super::{
        CancelIngestJobRequest, DaemonIngestProgressEvent, DaemonIngestStage,
        DaemonRequestValidationError, SubmitIngestFilesRequest,
    };
    use crate::api::health::DaemonSsdPressure;
    use dasobjectstore_core::ids::{DiskId, IngestJobId, ObjectId, StoreId};

    #[test]
    fn validates_absolute_ingest_submission() {
        let request = SubmitIngestFilesRequest {
            endpoint: StoreId::new("zymo").expect("store id"),
            source_path: "/mnt/external/zymo".into(),
            copies: Some(1),
            dry_run: false,
            client_request_id: Some("request-a".to_string()),
        };

        request.validate().expect("request is valid");
    }

    #[test]
    fn rejects_relative_ingest_source_path() {
        let request = SubmitIngestFilesRequest {
            endpoint: StoreId::new("zymo").expect("store id"),
            source_path: "relative/source".into(),
            copies: Some(1),
            dry_run: false,
            client_request_id: None,
        };

        let err = request.validate().expect_err("relative source rejected");

        assert_eq!(
            err,
            DaemonRequestValidationError::RelativeSourcePath {
                path: "relative/source".into()
            }
        );
    }

    #[test]
    fn rejects_zero_copy_override() {
        let request = SubmitIngestFilesRequest {
            endpoint: StoreId::new("zymo").expect("store id"),
            source_path: "/mnt/external/zymo".into(),
            copies: Some(0),
            dry_run: false,
            client_request_id: None,
        };

        let err = request.validate().expect_err("zero copies rejected");

        assert_eq!(
            err,
            DaemonRequestValidationError::InvalidCopyCount { copies: 0 }
        );
    }

    #[test]
    fn rejects_blank_cancellation_reason() {
        let request = CancelIngestJobRequest {
            job_id: IngestJobId::new("job-a").expect("job id"),
            reason: Some(" ".to_string()),
        };

        let err = request.validate().expect_err("blank reason rejected");

        assert_eq!(err, DaemonRequestValidationError::BlankCancellationReason);
    }

    #[test]
    fn progress_event_reports_bounded_percent() {
        let event = DaemonIngestProgressEvent {
            job_id: IngestJobId::new("job-a").expect("job id"),
            endpoint: StoreId::new("zymo").expect("store id"),
            stage: DaemonIngestStage::HddCopy {
                disk_id: DiskId::new("qnap-1062").expect("disk id"),
                copy_number: 1,
            },
            work_bytes_done: 150,
            work_bytes_total: Some(100),
            files_done: 1,
            files_total: Some(1),
            current_object_id: Some(ObjectId::new("zymo/sample.fastq.gz").expect("object id")),
            ssd_pressure: Some(DaemonSsdPressure::AcceptingWrites),
            message: None,
        };

        assert_eq!(event.percent_complete(), Some(100));
    }
}
