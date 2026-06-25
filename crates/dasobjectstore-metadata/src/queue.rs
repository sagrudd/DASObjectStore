use crate::SsdPressure;
use dasobjectstore_core::ids::{IngestJobId, InvalidId, ObjectId, StoreId};
use dasobjectstore_core::lifecycle::IngestJobState;
use rusqlite::Connection;
use serde::Serialize;
use std::fmt::{self, Display};
use std::path::{Path, PathBuf};

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

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IngestQueueSnapshot {
    pub live_sqlite_path: PathBuf,
    pub jobs: Vec<IngestQueueJob>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IngestQueueJob {
    pub ingest_job_id: IngestJobId,
    pub store_id: StoreId,
    pub object_id: Option<ObjectId>,
    pub state: String,
    pub ingest_mode: String,
    pub acknowledgement_policy: String,
    pub priority: i32,
    pub staging_path: String,
    pub expected_size_bytes: Option<u64>,
    pub received_bytes: u64,
    pub content_hash: Option<String>,
    pub content_hash_algorithm: Option<String>,
    pub failure_message: Option<String>,
    pub created_at_utc: String,
    pub updated_at_utc: String,
}

pub fn read_ingest_queue(
    live_sqlite_path: impl AsRef<Path>,
) -> Result<IngestQueueSnapshot, IngestQueueReadError> {
    let live_sqlite_path = live_sqlite_path.as_ref();
    let connection = Connection::open(live_sqlite_path)?;
    let mut statement = connection.prepare(
        "SELECT
            ingest_job_id,
            store_id,
            object_id,
            state,
            ingest_mode,
            acknowledgement_policy,
            priority,
            staging_path,
            expected_size_bytes,
            received_bytes,
            content_hash,
            content_hash_algorithm,
            failure_message,
            created_at_utc,
            updated_at_utc
         FROM ingest_jobs
         ORDER BY priority DESC, created_at_utc ASC, ingest_job_id ASC",
    )?;
    let rows = statement.query_map([], read_ingest_queue_job)?;

    let mut jobs = Vec::new();
    for row in rows {
        jobs.push(row?);
    }

    Ok(IngestQueueSnapshot {
        live_sqlite_path: live_sqlite_path.to_path_buf(),
        jobs,
    })
}

#[derive(Debug)]
pub enum IngestQueueReadError {
    Sqlite(rusqlite::Error),
    InvalidIdentifier {
        field: &'static str,
        source: InvalidId,
    },
    NegativeByteCount {
        field: &'static str,
        value: i64,
    },
}

impl Display for IngestQueueReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(err) => write!(formatter, "failed to read ingest queue metadata: {err}"),
            Self::InvalidIdentifier { field, source } => {
                write!(formatter, "invalid ingest queue {field}: {source}")
            }
            Self::NegativeByteCount { field, value } => {
                write!(formatter, "invalid negative ingest queue {field}: {value}")
            }
        }
    }
}

impl std::error::Error for IngestQueueReadError {}

impl From<rusqlite::Error> for IngestQueueReadError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Sqlite(err)
    }
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

fn read_ingest_queue_job(row: &rusqlite::Row<'_>) -> Result<IngestQueueJob, rusqlite::Error> {
    let ingest_job_id = parse_id("ingest_job_id", row.get::<_, String>(0)?)?;
    let store_id = parse_id("store_id", row.get::<_, String>(1)?)?;
    let object_id = parse_optional_id("object_id", row.get::<_, Option<String>>(2)?)?;
    let expected_size_bytes = optional_u64("expected_size_bytes", row.get::<_, Option<i64>>(8)?)?;
    let received_bytes = required_u64("received_bytes", row.get::<_, i64>(9)?)?;

    Ok(IngestQueueJob {
        ingest_job_id,
        store_id,
        object_id,
        state: row.get(3)?,
        ingest_mode: row.get(4)?,
        acknowledgement_policy: row.get(5)?,
        priority: row.get(6)?,
        staging_path: row.get(7)?,
        expected_size_bytes,
        received_bytes,
        content_hash: row.get(10)?,
        content_hash_algorithm: row.get(11)?,
        failure_message: row.get(12)?,
        created_at_utc: row.get(13)?,
        updated_at_utc: row.get(14)?,
    })
}

fn parse_id<T>(field: &'static str, value: String) -> Result<T, rusqlite::Error>
where
    T: std::str::FromStr<Err = InvalidId>,
{
    value.parse().map_err(|source| {
        rusqlite::Error::ToSqlConversionFailure(Box::new(IngestQueueReadError::InvalidIdentifier {
            field,
            source,
        }))
    })
}

fn parse_optional_id<T>(
    field: &'static str,
    value: Option<String>,
) -> Result<Option<T>, rusqlite::Error>
where
    T: std::str::FromStr<Err = InvalidId>,
{
    value.map(|value| parse_id(field, value)).transpose()
}

fn required_u64(field: &'static str, value: i64) -> Result<u64, rusqlite::Error> {
    u64::try_from(value).map_err(|_| {
        rusqlite::Error::ToSqlConversionFailure(Box::new(IngestQueueReadError::NegativeByteCount {
            field,
            value,
        }))
    })
}

fn optional_u64(field: &'static str, value: Option<i64>) -> Result<Option<u64>, rusqlite::Error> {
    value.map(|value| required_u64(field, value)).transpose()
}

#[cfg(test)]
mod tests {
    use super::{read_ingest_queue, IngestAdmission, IngestBackpressurePolicy, IngestQueueEntry};
    use crate::SsdPressure;
    use crate::LIVE_SCHEMA_SQL;
    use dasobjectstore_core::ids::IngestJobId;
    use dasobjectstore_core::lifecycle::IngestJobState;
    use rusqlite::Connection;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

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

    #[test]
    fn reads_ingest_queue_from_live_sqlite_ordered_by_priority_then_age() {
        let root = temp_root("ingest-queue");
        fs::create_dir_all(&root).expect("create temp root");
        let live_sqlite_path = root.join("live.sqlite");
        let connection = Connection::open(&live_sqlite_path).expect("open sqlite");
        connection
            .execute_batch(LIVE_SCHEMA_SQL)
            .expect("schema applies");
        insert_store(&connection);
        insert_job(
            &connection,
            "job-low",
            "Queued",
            0,
            "2026-01-01T00:00:00Z",
            128,
        );
        insert_job(
            &connection,
            "job-high",
            "Receiving",
            20,
            "2026-01-01T00:00:01Z",
            64,
        );

        let snapshot = read_ingest_queue(&live_sqlite_path).expect("queue reads");

        assert_eq!(snapshot.live_sqlite_path, live_sqlite_path);
        assert_eq!(snapshot.jobs.len(), 2);
        assert_eq!(snapshot.jobs[0].ingest_job_id.as_str(), "job-high");
        assert_eq!(snapshot.jobs[0].priority, 20);
        assert_eq!(snapshot.jobs[0].received_bytes, 64);
        assert_eq!(snapshot.jobs[1].ingest_job_id.as_str(), "job-low");

        fs::remove_dir_all(root).expect("cleanup temp root");
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

    fn insert_store(connection: &Connection) {
        connection
            .execute(
                "INSERT INTO pools (pool_id, state, created_at_utc, updated_at_utc)
                 VALUES ('pool-a', 'Clean', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )
            .expect("pool inserts");
        connection
            .execute(
                "INSERT INTO stores (
                    store_id,
                    pool_id,
                    class,
                    policy_json,
                    created_at_utc,
                    updated_at_utc
                 ) VALUES (
                    'store-a',
                    'pool-a',
                    'generated_data',
                    '{}',
                    '2026-01-01T00:00:00Z',
                    '2026-01-01T00:00:00Z'
                 )",
                [],
            )
            .expect("store inserts");
    }

    fn insert_job(
        connection: &Connection,
        ingest_job_id: &str,
        state: &str,
        priority: i32,
        created_at_utc: &str,
        received_bytes: u64,
    ) {
        connection
            .execute(
                "INSERT INTO ingest_jobs (
                    ingest_job_id,
                    store_id,
                    state,
                    ingest_mode,
                    acknowledgement_policy,
                    priority,
                    staging_path,
                    expected_size_bytes,
                    received_bytes,
                    created_at_utc,
                    updated_at_utc
                 ) VALUES (?1, 'store-a', ?2, 'SsdFirst', 'AfterHddPlacement', ?3, ?4, 256, ?5, ?6, ?6)",
                (
                    ingest_job_id,
                    state,
                    priority,
                    format!("/ssd/.dasobjectstore/ingest/jobs/{ingest_job_id}"),
                    received_bytes,
                    created_at_utc,
                ),
            )
            .expect("job inserts");
    }

    fn temp_root(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dasobjectstore-{name}-{}-{nanos}",
            std::process::id()
        ))
    }
}
