//! Shared daemon job DTOs for all client-facing job workflows.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DaemonJobId(String);

impl DaemonJobId {
    pub fn new(value: impl Into<String>) -> Result<Self, DaemonJobIdError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(DaemonJobIdError::Blank);
        }

        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DaemonJobId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for DaemonJobId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for DaemonJobId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DaemonJobIdError {
    Blank,
}

impl fmt::Display for DaemonJobIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Blank => formatter.write_str("daemon job id cannot be blank"),
        }
    }
}

impl std::error::Error for DaemonJobIdError {}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonJobKind {
    IngestFiles,
    DirectImport,
    DiskDrain,
    DiskRetire,
    DiskReplace,
    EnclosurePreparation,
    Repair,
    ServiceOperation,
    SystemAdministration,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonJobState {
    Queued,
    Running,
    Waiting,
    Complete,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonJobProgress {
    pub stage: String,
    pub work_bytes_done: u64,
    pub work_bytes_total: u64,
    pub work_units_done: u64,
    pub work_units_total: u64,
    pub message: Option<String>,
}

impl DaemonJobProgress {
    pub fn percent_complete(&self) -> Option<u8> {
        if self.work_bytes_total == 0 {
            return None;
        }

        let percent = self.work_bytes_done.saturating_mul(100) / self.work_bytes_total;
        Some(percent.min(100) as u8)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonJobSummary {
    pub job_id: DaemonJobId,
    pub kind: DaemonJobKind,
    pub state: DaemonJobState,
    pub progress: DaemonJobProgress,
    pub submitted_at_utc: String,
    pub updated_at_utc: String,
    pub actor: Option<String>,
    pub failure_message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonJobAcceptedResponse {
    pub job_id: DaemonJobId,
    pub kind: DaemonJobKind,
    pub accepted_at_utc: String,
    pub dry_run: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonJobStatusRequest {
    pub job_id: DaemonJobId,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonJobStatusResponse {
    pub job: DaemonJobSummary,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonJobCancelRequest {
    pub job_id: DaemonJobId,
    pub reason: Option<String>,
}

impl DaemonJobCancelRequest {
    pub fn validate(&self) -> Result<(), DaemonJobValidationError> {
        if self
            .reason
            .as_deref()
            .is_some_and(|reason| reason.trim().is_empty())
        {
            return Err(DaemonJobValidationError::BlankCancellationReason);
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonJobCancelResponse {
    pub job_id: DaemonJobId,
    pub accepted: bool,
    pub state: DaemonJobState,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "event", content = "payload")]
pub enum DaemonJobEvent {
    Accepted(DaemonJobAcceptedResponse),
    Progress(DaemonJobSummary),
    Complete(DaemonJobSummary),
    Failed(DaemonJobSummary),
    Cancelled(DaemonJobCancelResponse),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DaemonJobValidationError {
    BlankCancellationReason,
}

impl fmt::Display for DaemonJobValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BlankCancellationReason => {
                formatter.write_str("cancellation reason cannot be blank")
            }
        }
    }
}

impl std::error::Error for DaemonJobValidationError {}

#[cfg(test)]
mod tests {
    use super::{
        DaemonJobCancelRequest, DaemonJobId, DaemonJobKind, DaemonJobProgress, DaemonJobState,
        DaemonJobStatusResponse, DaemonJobSummary,
    };

    #[test]
    fn rejects_blank_job_id_on_deserialize() {
        let result = serde_json::from_str::<DaemonJobId>("\"   \"");

        assert!(result.is_err());
    }

    #[test]
    fn serializes_job_status_with_stable_case() {
        let response = DaemonJobStatusResponse {
            job: DaemonJobSummary {
                job_id: DaemonJobId::new("job-1").expect("job id"),
                kind: DaemonJobKind::IngestFiles,
                state: DaemonJobState::Running,
                progress: DaemonJobProgress {
                    stage: "copying".to_string(),
                    work_bytes_done: 50,
                    work_bytes_total: 100,
                    work_units_done: 1,
                    work_units_total: 2,
                    message: None,
                },
                submitted_at_utc: "2026-07-07T10:20:12Z".to_string(),
                updated_at_utc: "2026-07-07T10:21:12Z".to_string(),
                actor: Some("stephen".to_string()),
                failure_message: None,
            },
        };

        let encoded = serde_json::to_value(response).expect("status serializes");

        assert_eq!(encoded["job"]["kind"], "ingest_files");
        assert_eq!(encoded["job"]["state"], "running");
    }

    #[test]
    fn caps_progress_percent() {
        let progress = DaemonJobProgress {
            work_bytes_done: 200,
            work_bytes_total: 100,
            ..DaemonJobProgress::default()
        };

        assert_eq!(progress.percent_complete(), Some(100));
    }

    #[test]
    fn rejects_blank_cancel_reason() {
        let request = DaemonJobCancelRequest {
            job_id: DaemonJobId::new("job-1").expect("job id"),
            reason: Some(" ".to_string()),
        };

        assert!(request.validate().is_err());
    }
}
