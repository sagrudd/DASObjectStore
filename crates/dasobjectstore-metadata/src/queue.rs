use crate::schema::LIVE_SCHEMA_SQL;
use crate::SsdPressure;
use dasobjectstore_core::ids::{IngestJobId, InvalidId, ObjectId, StoreId};
use dasobjectstore_core::lifecycle::IngestJobState;
use dasobjectstore_core::object_type::{ObjectType, ObjectTypeParseError};
use rusqlite::types::Type;
use rusqlite::{params, Connection};
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub enum DestageUrgency {
    Opportunistic,
    Prioritized,
    Urgent,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DestagePriorityPolicy {
    pub accepting_writes_urgency: DestageUrgency,
    pub high_watermark_urgency: DestageUrgency,
    pub critical_watermark_urgency: DestageUrgency,
}

impl DestagePriorityPolicy {
    pub fn urgency(&self, pressure: SsdPressure) -> DestageUrgency {
        match pressure {
            SsdPressure::AcceptingWrites => self.accepting_writes_urgency,
            SsdPressure::HighWatermark => self.high_watermark_urgency,
            SsdPressure::Critical => self.critical_watermark_urgency,
        }
    }

    pub fn prioritizes_destage(&self, pressure: SsdPressure) -> bool {
        matches!(
            self.urgency(pressure),
            DestageUrgency::Prioritized | DestageUrgency::Urgent
        )
    }
}

impl Default for DestagePriorityPolicy {
    fn default() -> Self {
        Self {
            accepting_writes_urgency: DestageUrgency::Opportunistic,
            high_watermark_urgency: DestageUrgency::Prioritized,
            critical_watermark_urgency: DestageUrgency::Urgent,
        }
    }
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IngestQueueDrainRequest {
    pub live_sqlite_path: PathBuf,
    pub store_id: StoreId,
    pub updated_at_utc: String,
    pub reason: String,
    pub dry_run: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IngestQueueDrainReport {
    pub live_sqlite_path: PathBuf,
    pub store_id: StoreId,
    pub dry_run: bool,
    pub jobs_cancelled: usize,
    pub cancelled_job_ids: Vec<IngestJobId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct IngestQueueJob {
    pub ingest_job_id: IngestJobId,
    pub store_id: StoreId,
    pub object_id: Option<ObjectId>,
    pub object_type: ObjectType,
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
    read_ingest_queue_inner(live_sqlite_path.as_ref(), None)
}

pub fn read_ingest_queue_for_store(
    live_sqlite_path: impl AsRef<Path>,
    store_id: &StoreId,
) -> Result<IngestQueueSnapshot, IngestQueueReadError> {
    read_ingest_queue_inner(live_sqlite_path.as_ref(), Some(store_id))
}

fn read_ingest_queue_inner(
    live_sqlite_path: &Path,
    store_id: Option<&StoreId>,
) -> Result<IngestQueueSnapshot, IngestQueueReadError> {
    let connection = Connection::open(live_sqlite_path)?;
    connection.execute_batch(LIVE_SCHEMA_SQL)?;
    let query = match store_id {
        Some(_) => {
            "SELECT
            ingest_job_id,
            store_id,
            object_id,
            object_type,
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
         WHERE store_id = ?1
         ORDER BY priority DESC, created_at_utc ASC, ingest_job_id ASC"
        }
        None => {
            "SELECT
            ingest_job_id,
            store_id,
            object_id,
            object_type,
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
         ORDER BY priority DESC, created_at_utc ASC, ingest_job_id ASC"
        }
    };
    let mut statement = connection.prepare(query)?;

    let mut rows = match store_id {
        Some(store_id) => statement.query(params![store_id.as_str()])?,
        None => statement.query([])?,
    };

    let mut jobs = Vec::new();
    while let Some(row) = rows.next()? {
        jobs.push(read_ingest_queue_job(row)?);
    }

    Ok(IngestQueueSnapshot {
        live_sqlite_path: live_sqlite_path.to_path_buf(),
        jobs,
    })
}

pub fn drain_ingest_queue(
    request: &IngestQueueDrainRequest,
) -> Result<IngestQueueDrainReport, IngestQueueDrainError> {
    let mut connection = Connection::open(&request.live_sqlite_path)?;
    connection.execute_batch(LIVE_SCHEMA_SQL)?;
    let cancelled_job_ids = active_job_ids_for_store(&connection, &request.store_id)?;

    if !request.dry_run && !cancelled_job_ids.is_empty() {
        let transaction = connection.transaction()?;
        transaction.execute(
            "UPDATE ingest_jobs
             SET state = 'Cancelled',
                 failure_message = ?1,
                 updated_at_utc = ?2
             WHERE store_id = ?3
               AND state NOT IN ('Complete', 'Failed', 'Cancelled')",
            params![
                request.reason.trim(),
                request.updated_at_utc,
                request.store_id.as_str()
            ],
        )?;
        transaction.commit()?;
    }

    Ok(IngestQueueDrainReport {
        live_sqlite_path: request.live_sqlite_path.clone(),
        store_id: request.store_id.clone(),
        dry_run: request.dry_run,
        jobs_cancelled: cancelled_job_ids.len(),
        cancelled_job_ids,
    })
}

fn active_job_ids_for_store(
    connection: &Connection,
    store_id: &StoreId,
) -> Result<Vec<IngestJobId>, IngestQueueDrainError> {
    let mut statement = connection.prepare(
        "SELECT ingest_job_id
         FROM ingest_jobs
         WHERE store_id = ?1
           AND state NOT IN ('Complete', 'Failed', 'Cancelled')
         ORDER BY priority DESC, created_at_utc ASC, ingest_job_id ASC",
    )?;
    let rows = statement.query_map(params![store_id.as_str()], |row| {
        parse_id("ingest_job_id", row.get::<_, String>(0)?)
    })?;

    let mut job_ids = Vec::new();
    for row in rows {
        job_ids.push(row?);
    }
    Ok(job_ids)
}

#[derive(Debug)]
pub enum IngestQueueReadError {
    Sqlite(rusqlite::Error),
    InvalidIdentifier {
        field: &'static str,
        source: InvalidId,
    },
    InvalidObjectType {
        value: String,
        source: ObjectTypeParseError,
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
            Self::InvalidObjectType { value, source } => {
                write!(
                    formatter,
                    "invalid ingest queue object_type `{value}`: {source}"
                )
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

#[derive(Debug)]
pub enum IngestQueueDrainError {
    Sqlite(rusqlite::Error),
    InvalidIdentifier {
        field: &'static str,
        source: InvalidId,
    },
}

impl Display for IngestQueueDrainError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(err) => write!(formatter, "failed to drain ingest queue metadata: {err}"),
            Self::InvalidIdentifier { field, source } => {
                write!(formatter, "invalid ingest queue {field}: {source}")
            }
        }
    }
}

impl std::error::Error for IngestQueueDrainError {}

impl From<rusqlite::Error> for IngestQueueDrainError {
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
    let object_type = parse_object_type(row.get::<_, String>(3)?)?;
    let expected_size_bytes = optional_u64("expected_size_bytes", row.get::<_, Option<i64>>(9)?)?;
    let received_bytes = required_u64("received_bytes", row.get::<_, i64>(10)?)?;

    Ok(IngestQueueJob {
        ingest_job_id,
        store_id,
        object_id,
        object_type,
        state: row.get(4)?,
        ingest_mode: row.get(5)?,
        acknowledgement_policy: row.get(6)?,
        priority: row.get(7)?,
        staging_path: row.get(8)?,
        expected_size_bytes,
        received_bytes,
        content_hash: row.get(11)?,
        content_hash_algorithm: row.get(12)?,
        failure_message: row.get(13)?,
        created_at_utc: row.get(14)?,
        updated_at_utc: row.get(15)?,
    })
}

fn parse_object_type(value: String) -> Result<ObjectType, rusqlite::Error> {
    value.parse().map_err(|source| {
        rusqlite::Error::FromSqlConversionFailure(
            3,
            Type::Text,
            Box::new(IngestQueueReadError::InvalidObjectType { value, source }),
        )
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
    use super::{
        drain_ingest_queue, read_ingest_queue, read_ingest_queue_for_store, DestagePriorityPolicy,
        DestageUrgency, IngestAdmission, IngestBackpressurePolicy, IngestQueueDrainRequest,
        IngestQueueEntry,
    };
    use crate::{SsdPressure, LIVE_SCHEMA_SQL};
    use dasobjectstore_core::ids::{IngestJobId, StoreId};
    use dasobjectstore_core::lifecycle::IngestJobState;
    use dasobjectstore_core::object_type::ObjectType;
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
    fn destage_policy_promotes_settlement_as_ssd_pressure_rises() {
        let policy = DestagePriorityPolicy::default();

        assert_eq!(
            policy.urgency(SsdPressure::AcceptingWrites),
            DestageUrgency::Opportunistic
        );
        assert_eq!(
            policy.urgency(SsdPressure::HighWatermark),
            DestageUrgency::Prioritized
        );
        assert_eq!(
            policy.urgency(SsdPressure::Critical),
            DestageUrgency::Urgent
        );
        assert!(!policy.prioritizes_destage(SsdPressure::AcceptingWrites));
        assert!(policy.prioritizes_destage(SsdPressure::HighWatermark));
        assert!(policy.prioritizes_destage(SsdPressure::Critical));
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
        assert_eq!(snapshot.jobs[0].object_type, ObjectType::Fastq);
        assert_eq!(snapshot.jobs[0].priority, 20);
        assert_eq!(snapshot.jobs[0].received_bytes, 64);
        assert_eq!(snapshot.jobs[1].ingest_job_id.as_str(), "job-low");

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn reads_empty_queue_from_older_live_sqlite_without_ingest_jobs_table() {
        let root = temp_root("ingest-queue-old-schema");
        fs::create_dir_all(&root).expect("create temp root");
        let live_sqlite_path = root.join("live.sqlite");
        let connection = Connection::open(&live_sqlite_path).expect("open sqlite");
        connection
            .execute_batch(
                "CREATE TABLE stores (
                    store_id TEXT PRIMARY KEY NOT NULL,
                    pool_id TEXT NOT NULL,
                    class TEXT NOT NULL,
                    policy_json TEXT NOT NULL,
                    created_at_utc TEXT NOT NULL,
                    updated_at_utc TEXT NOT NULL
                );",
            )
            .expect("old schema applies");
        drop(connection);

        let snapshot = read_ingest_queue(&live_sqlite_path).expect("queue reads");
        let connection = Connection::open(&live_sqlite_path).expect("reopen sqlite");
        let table_count: usize = connection
            .query_row(
                "SELECT COUNT(*)
                 FROM sqlite_master
                 WHERE type = 'table' AND name = 'ingest_jobs'",
                [],
                |row| row.get(0),
            )
            .expect("table count reads");

        assert!(snapshot.jobs.is_empty());
        assert_eq!(table_count, 1);

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn reads_ingest_queue_for_one_store() {
        let root = temp_root("ingest-queue-store");
        fs::create_dir_all(&root).expect("create temp root");
        let live_sqlite_path = root.join("live.sqlite");
        let connection = Connection::open(&live_sqlite_path).expect("open sqlite");
        connection
            .execute_batch(LIVE_SCHEMA_SQL)
            .expect("schema applies");
        insert_store(&connection);
        insert_store_named(&connection, "store-b");
        insert_job(
            &connection,
            "job-a",
            "Queued",
            0,
            "2026-01-01T00:00:00Z",
            128,
        );
        insert_job_for_store(
            &connection,
            "store-b",
            "job-b",
            "Queued",
            0,
            "2026-01-01T00:00:01Z",
            64,
        );

        let snapshot = read_ingest_queue_for_store(
            &live_sqlite_path,
            &StoreId::new("store-b").expect("store id"),
        )
        .expect("queue reads");

        assert_eq!(snapshot.jobs.len(), 1);
        assert_eq!(snapshot.jobs[0].ingest_job_id.as_str(), "job-b");

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn drains_active_ingest_queue_jobs_for_store_without_deleting_rows() {
        let root = temp_root("ingest-queue-drain");
        fs::create_dir_all(&root).expect("create temp root");
        let live_sqlite_path = root.join("live.sqlite");
        let connection = Connection::open(&live_sqlite_path).expect("open sqlite");
        connection
            .execute_batch(LIVE_SCHEMA_SQL)
            .expect("schema applies");
        insert_store(&connection);
        insert_job(
            &connection,
            "job-active",
            "Receiving",
            10,
            "2026-01-01T00:00:00Z",
            128,
        );
        insert_job(
            &connection,
            "job-complete",
            "Complete",
            0,
            "2026-01-01T00:00:01Z",
            64,
        );
        drop(connection);

        let request = IngestQueueDrainRequest {
            live_sqlite_path: live_sqlite_path.clone(),
            store_id: StoreId::new("store-a").expect("store id"),
            updated_at_utc: "2026-07-07T12:00:00Z".to_string(),
            reason: "operator drained ingest queue".to_string(),
            dry_run: true,
        };
        let dry_run = drain_ingest_queue(&request).expect("dry run drains");
        assert!(dry_run.dry_run);
        assert_eq!(dry_run.jobs_cancelled, 1);
        assert_eq!(job_state(&live_sqlite_path, "job-active"), "Receiving");

        let report = drain_ingest_queue(&IngestQueueDrainRequest {
            dry_run: false,
            ..request
        })
        .expect("queue drains");

        assert_eq!(report.jobs_cancelled, 1);
        assert_eq!(report.cancelled_job_ids[0].as_str(), "job-active");
        assert_eq!(job_state(&live_sqlite_path, "job-active"), "Cancelled");
        assert_eq!(job_state(&live_sqlite_path, "job-complete"), "Complete");

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
                "INSERT OR IGNORE INTO pools (pool_id, state, created_at_utc, updated_at_utc)
                 VALUES ('pool-a', 'Clean', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )
            .expect("pool inserts");
        insert_store_named(connection, "store-a");
    }

    fn insert_store_named(connection: &Connection, store_id: &str) {
        connection
            .execute(
                "INSERT OR IGNORE INTO stores (
                    store_id,
                    pool_id,
                    class,
                    policy_json,
                    created_at_utc,
                    updated_at_utc
                 ) VALUES (
                    ?1,
                    'pool-a',
                    'generated_data',
                    '{}',
                    '2026-01-01T00:00:00Z',
                    '2026-01-01T00:00:00Z'
                 )",
                [store_id],
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
        insert_job_for_store(
            connection,
            "store-a",
            ingest_job_id,
            state,
            priority,
            created_at_utc,
            received_bytes,
        );
    }

    fn insert_job_for_store(
        connection: &Connection,
        store_id: &str,
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
                    object_type,
                    state,
                    ingest_mode,
                    acknowledgement_policy,
                    priority,
                    staging_path,
                    expected_size_bytes,
                    received_bytes,
                    created_at_utc,
                    updated_at_utc
                 ) VALUES (?1, ?2, 'fastq', ?3, 'SsdFirst', 'AfterHddPlacement', ?4, ?5, 256, ?6, ?7, ?7)",
                (
                    ingest_job_id,
                    store_id,
                    state,
                    priority,
                    format!("/ssd/.dasobjectstore/ingest/jobs/{ingest_job_id}"),
                    received_bytes,
                    created_at_utc,
                ),
            )
            .expect("job inserts");
    }

    fn job_state(live_sqlite_path: &std::path::Path, ingest_job_id: &str) -> String {
        let connection = Connection::open(live_sqlite_path).expect("open sqlite");
        connection
            .query_row(
                "SELECT state FROM ingest_jobs WHERE ingest_job_id = ?1",
                [ingest_job_id],
                |row| row.get(0),
            )
            .expect("job state")
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
