use crate::dashboard::DashboardWarning;
use dasobjectstore_core::ids::{IngestJobId, ObjectId, StoreId};
use dasobjectstore_core::lifecycle::IngestJobState;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IngestQueueView {
    pub pressure: QueuePressureView,
    pub queued_jobs: usize,
    pub active_jobs: usize,
    pub failed_jobs: usize,
    pub jobs: Vec<IngestQueueJobView>,
    pub warnings: Vec<DashboardWarning>,
}

impl IngestQueueView {
    pub fn from_jobs(pressure: QueuePressureView, jobs: Vec<IngestQueueJobView>) -> Self {
        let queued_jobs = jobs.iter().filter(|job| job.state.is_queued()).count();
        let active_jobs = jobs.iter().filter(|job| job.state.is_active()).count();
        let failed_jobs = jobs.iter().filter(|job| job.state.is_failed()).count();

        Self {
            pressure,
            queued_jobs,
            active_jobs,
            failed_jobs,
            warnings: ingest_queue_warnings(pressure, failed_jobs),
            jobs,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueuePressureView {
    Normal,
    HighWatermark,
    Critical,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IngestQueueJobView {
    pub ingest_job_id: String,
    pub store_id: String,
    pub object_id: Option<String>,
    pub state: IngestJobStateView,
    pub priority: i32,
    pub received_bytes: u64,
    pub expected_size_bytes: Option<u64>,
    pub updated_at_utc: String,
    pub warnings: Vec<DashboardWarning>,
}

impl IngestQueueJobView {
    pub fn from_ingest_job(
        ingest_job_id: &IngestJobId,
        store_id: &StoreId,
        object_id: Option<&ObjectId>,
        state: IngestJobState,
        priority: i32,
        progress: IngestProgressView,
        updated_at_utc: impl Into<String>,
    ) -> Self {
        Self {
            ingest_job_id: ingest_job_id.to_string(),
            store_id: store_id.to_string(),
            object_id: object_id.map(ToString::to_string),
            state: IngestJobStateView::from(state),
            priority,
            received_bytes: progress.received_bytes,
            expected_size_bytes: progress.expected_size_bytes,
            updated_at_utc: updated_at_utc.into(),
            warnings: ingest_job_warnings(
                state,
                progress.received_bytes,
                progress.expected_size_bytes,
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IngestProgressView {
    pub received_bytes: u64,
    pub expected_size_bytes: Option<u64>,
}

impl IngestProgressView {
    pub fn new(received_bytes: u64, expected_size_bytes: Option<u64>) -> Self {
        Self {
            received_bytes,
            expected_size_bytes,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IngestJobStateView {
    Queued,
    Receiving,
    Received,
    Hashing,
    ReadyForPlacement,
    Destaging,
    Complete,
    Failed,
    Cancelled,
}

impl IngestJobStateView {
    fn is_queued(self) -> bool {
        matches!(self, Self::Queued)
    }

    fn is_active(self) -> bool {
        !matches!(self, Self::Complete | Self::Failed | Self::Cancelled)
    }

    fn is_failed(self) -> bool {
        matches!(self, Self::Failed)
    }
}

impl From<IngestJobState> for IngestJobStateView {
    fn from(state: IngestJobState) -> Self {
        match state {
            IngestJobState::Queued => Self::Queued,
            IngestJobState::Receiving => Self::Receiving,
            IngestJobState::Received => Self::Received,
            IngestJobState::Hashing => Self::Hashing,
            IngestJobState::ReadyForPlacement => Self::ReadyForPlacement,
            IngestJobState::Destaging => Self::Destaging,
            IngestJobState::Complete => Self::Complete,
            IngestJobState::Failed => Self::Failed,
        }
    }
}

fn ingest_queue_warnings(pressure: QueuePressureView, failed_jobs: usize) -> Vec<DashboardWarning> {
    let mut warnings = Vec::new();

    match pressure {
        QueuePressureView::Normal => {}
        QueuePressureView::HighWatermark => warnings.push(DashboardWarning::new(
            "ingest_high_watermark",
            "SSD ingest pressure is high; lower-priority ingest may pause.",
        )),
        QueuePressureView::Critical => warnings.push(DashboardWarning::new(
            "ingest_critical_pressure",
            "SSD ingest pressure is critical; new writes may be blocked.",
        )),
    }

    if failed_jobs > 0 {
        warnings.push(DashboardWarning::new(
            "ingest_failed_jobs",
            "One or more ingest jobs have failed and need review.",
        ));
    }

    warnings
}

fn ingest_job_warnings(
    state: IngestJobState,
    received_bytes: u64,
    expected_size_bytes: Option<u64>,
) -> Vec<DashboardWarning> {
    let mut warnings = Vec::new();

    if matches!(state, IngestJobState::Failed) {
        warnings.push(DashboardWarning::new(
            "ingest_job_failed",
            "Ingest job failed before settlement.",
        ));
    }

    if expected_size_bytes.is_some_and(|expected| received_bytes > expected) {
        warnings.push(DashboardWarning::new(
            "ingest_size_exceeded",
            "Received bytes exceed the expected object size.",
        ));
    }

    warnings
}

#[cfg(test)]
mod tests {
    use super::{
        IngestJobStateView, IngestProgressView, IngestQueueJobView, IngestQueueView,
        QueuePressureView,
    };
    use dasobjectstore_core::ids::{IngestJobId, ObjectId, StoreId};
    use dasobjectstore_core::lifecycle::IngestJobState;

    #[test]
    fn builds_ingest_queue_view_from_jobs() {
        let store_id = StoreId::new("store-a").expect("store id");
        let object_id = ObjectId::new("object-a").expect("object id");
        let queued = IngestQueueJobView::from_ingest_job(
            &IngestJobId::new("job-a").expect("job id"),
            &store_id,
            Some(&object_id),
            IngestJobState::Queued,
            10,
            IngestProgressView::new(0, Some(4096)),
            "2026-01-05T00:00:00Z",
        );
        let failed = IngestQueueJobView::from_ingest_job(
            &IngestJobId::new("job-b").expect("job id"),
            &store_id,
            None,
            IngestJobState::Failed,
            0,
            IngestProgressView::new(8192, Some(4096)),
            "2026-01-05T00:01:00Z",
        );

        let view = IngestQueueView::from_jobs(QueuePressureView::Critical, vec![queued, failed]);

        assert_eq!(view.queued_jobs, 1);
        assert_eq!(view.active_jobs, 1);
        assert_eq!(view.failed_jobs, 1);
        assert_eq!(view.jobs[0].state, IngestJobStateView::Queued);
        assert_eq!(view.jobs[1].warnings[0].code, "ingest_job_failed");
        assert_eq!(view.warnings[0].code, "ingest_critical_pressure");
    }

    #[test]
    fn serializes_ingest_queue_for_dashboard_contract() {
        let job = IngestQueueJobView::from_ingest_job(
            &IngestJobId::new("job-a").expect("job id"),
            &StoreId::new("store-a").expect("store id"),
            None,
            IngestJobState::ReadyForPlacement,
            5,
            IngestProgressView::new(4096, Some(4096)),
            "2026-01-05T00:00:00Z",
        );
        let view = IngestQueueView::from_jobs(QueuePressureView::HighWatermark, vec![job]);

        let encoded = serde_json::to_value(view).expect("ingest queue serializes");

        assert_eq!(encoded["pressure"], "high_watermark");
        assert_eq!(encoded["jobs"][0]["state"], "ready_for_placement");
        assert_eq!(encoded["active_jobs"], 1);
    }
}
