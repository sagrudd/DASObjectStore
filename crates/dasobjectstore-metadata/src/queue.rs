use crate::SsdPressure;
use dasobjectstore_core::ids::IngestJobId;
use dasobjectstore_core::lifecycle::IngestJobState;
use serde::Serialize;

pub const DEFAULT_HIGH_WATERMARK_MINIMUM_PRIORITY: i32 = 10;
pub const DEFAULT_CRITICAL_WATERMARK_MINIMUM_PRIORITY: i32 = 100;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IngestQueueEntry {
    pub ingest_job_id: IngestJobId,
    pub state: IngestJobState,
    pub priority: i32,
    pub created_at_utc: String,
}

impl IngestQueueEntry {
    pub fn new(
        ingest_job_id: IngestJobId,
        state: IngestJobState,
        priority: i32,
        created_at_utc: impl Into<String>,
    ) -> Self {
        Self {
            ingest_job_id,
            state,
            priority,
            created_at_utc: created_at_utc.into(),
        }
    }

    pub fn is_active(&self) -> bool {
        !matches!(
            self.state,
            IngestJobState::Complete | IngestJobState::Failed
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IngestBackpressurePolicy {
    pub high_watermark_minimum_priority: i32,
    pub critical_watermark_minimum_priority: i32,
}

impl IngestBackpressurePolicy {
    pub fn plan(&self, pressure: SsdPressure, entries: &[IngestQueueEntry]) -> IngestQueuePlan {
        let mut active_entries: Vec<_> = entries.iter().filter(|entry| entry.is_active()).collect();
        active_entries.sort_by(compare_queue_entries);

        let mut runnable = Vec::new();
        let mut paused = Vec::new();

        for entry in active_entries {
            if self.allows_priority(pressure, entry.priority) {
                runnable.push(entry.ingest_job_id.clone());
            } else {
                paused.push(entry.ingest_job_id.clone());
            }
        }

        IngestQueuePlan {
            pressure,
            runnable,
            paused,
        }
    }

    pub fn admission(&self, pressure: SsdPressure, priority: i32) -> IngestAdmission {
        if self.allows_priority(pressure, priority) {
            return IngestAdmission::Accept;
        }

        match pressure {
            SsdPressure::AcceptingWrites => IngestAdmission::Accept,
            SsdPressure::HighWatermark => IngestAdmission::Backpressure,
            SsdPressure::Critical => IngestAdmission::Reject,
        }
    }

    fn allows_priority(&self, pressure: SsdPressure, priority: i32) -> bool {
        match pressure {
            SsdPressure::AcceptingWrites => true,
            SsdPressure::HighWatermark => priority >= self.high_watermark_minimum_priority,
            SsdPressure::Critical => priority >= self.critical_watermark_minimum_priority,
        }
    }
}

impl Default for IngestBackpressurePolicy {
    fn default() -> Self {
        Self {
            high_watermark_minimum_priority: DEFAULT_HIGH_WATERMARK_MINIMUM_PRIORITY,
            critical_watermark_minimum_priority: DEFAULT_CRITICAL_WATERMARK_MINIMUM_PRIORITY,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum IngestAdmission {
    Accept,
    Backpressure,
    Reject,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IngestQueuePlan {
    pub pressure: SsdPressure,
    pub runnable: Vec<IngestJobId>,
    pub paused: Vec<IngestJobId>,
}

fn compare_queue_entries(
    left: &&IngestQueueEntry,
    right: &&IngestQueueEntry,
) -> std::cmp::Ordering {
    right
        .priority
        .cmp(&left.priority)
        .then_with(|| left.created_at_utc.cmp(&right.created_at_utc))
        .then_with(|| left.ingest_job_id.cmp(&right.ingest_job_id))
}

#[cfg(test)]
mod tests {
    use super::{IngestAdmission, IngestBackpressurePolicy, IngestQueueEntry};
    use crate::SsdPressure;
    use dasobjectstore_core::ids::IngestJobId;
    use dasobjectstore_core::lifecycle::IngestJobState;

    #[test]
    fn accepting_writes_runs_active_jobs_by_priority_then_age() {
        let policy = IngestBackpressurePolicy::default();
        let plan = policy.plan(
            SsdPressure::AcceptingWrites,
            &[
                entry("job-low", IngestJobState::Queued, 0, "2026-01-01T00:00:00Z"),
                entry(
                    "job-newer-high",
                    IngestJobState::Queued,
                    20,
                    "2026-01-03T00:00:00Z",
                ),
                entry(
                    "job-older-high",
                    IngestJobState::Receiving,
                    20,
                    "2026-01-02T00:00:00Z",
                ),
                entry(
                    "job-complete",
                    IngestJobState::Complete,
                    100,
                    "2026-01-01T00:00:00Z",
                ),
            ],
        );

        assert_eq!(
            ids(&plan.runnable),
            vec!["job-older-high", "job-newer-high", "job-low"]
        );
        assert!(plan.paused.is_empty());
    }

    #[test]
    fn high_watermark_pauses_lower_priority_jobs() {
        let policy = IngestBackpressurePolicy::default();
        let plan = policy.plan(
            SsdPressure::HighWatermark,
            &[
                entry(
                    "job-cache",
                    IngestJobState::Queued,
                    0,
                    "2026-01-01T00:00:00Z",
                ),
                entry(
                    "job-generated",
                    IngestJobState::Receiving,
                    10,
                    "2026-01-01T00:00:01Z",
                ),
            ],
        );

        assert_eq!(ids(&plan.runnable), vec!["job-generated"]);
        assert_eq!(ids(&plan.paused), vec!["job-cache"]);
    }

    #[test]
    fn critical_pressure_only_runs_critical_priority_jobs() {
        let policy = IngestBackpressurePolicy::default();
        let plan = policy.plan(
            SsdPressure::Critical,
            &[
                entry(
                    "job-generated",
                    IngestJobState::Queued,
                    10,
                    "2026-01-01T00:00:00Z",
                ),
                entry(
                    "job-critical",
                    IngestJobState::Hashing,
                    100,
                    "2026-01-01T00:00:01Z",
                ),
            ],
        );

        assert_eq!(ids(&plan.runnable), vec!["job-critical"]);
        assert_eq!(ids(&plan.paused), vec!["job-generated"]);
    }

    #[test]
    fn admission_applies_pressure_thresholds() {
        let policy = IngestBackpressurePolicy::default();

        assert_eq!(
            policy.admission(SsdPressure::AcceptingWrites, 0),
            IngestAdmission::Accept
        );
        assert_eq!(
            policy.admission(SsdPressure::HighWatermark, 0),
            IngestAdmission::Backpressure
        );
        assert_eq!(
            policy.admission(SsdPressure::HighWatermark, 10),
            IngestAdmission::Accept
        );
        assert_eq!(
            policy.admission(SsdPressure::Critical, 10),
            IngestAdmission::Reject
        );
        assert_eq!(
            policy.admission(SsdPressure::Critical, 100),
            IngestAdmission::Accept
        );
    }

    fn entry(
        id: &str,
        state: IngestJobState,
        priority: i32,
        created_at_utc: &str,
    ) -> IngestQueueEntry {
        IngestQueueEntry::new(
            IngestJobId::new(id).expect("ingest job id"),
            state,
            priority,
            created_at_utc,
        )
    }

    fn ids(job_ids: &[IngestJobId]) -> Vec<&str> {
        job_ids.iter().map(|job_id| job_id.as_str()).collect()
    }
}
