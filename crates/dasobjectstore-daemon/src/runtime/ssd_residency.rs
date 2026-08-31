//! Evidence-based accounting for data physically resident on the managed SSD.
//!
//! This report intentionally separates native payloads from provider-managed
//! data and operational metadata.  It never deletes data: its purpose is to
//! make a full SSD explainable before the bounded housekeeping workers act.

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

pub const SSD_RESIDENCY_REPORT_SCHEMA: &str = "dasobjectstore.disk_housekeeping.ssd_residency.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SsdResidencyCoverage {
    Complete,
    Partial,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct SsdResidencyBucket {
    pub files: u64,
    pub bytes: u64,
    /// Percentage of all native payload bytes, in basis points.  Provider
    /// data and daemon metadata are deliberately excluded from this value.
    pub fraction_basis_points: u16,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SsdResidencyReport {
    pub schema: &'static str,
    pub generated_at_utc: String,
    pub coverage: SsdResidencyCoverage,
    pub native_payload_bytes: u64,
    pub settled_uncleared: SsdResidencyBucket,
    pub awaiting_hdd: SsdResidencyBucket,
    pub orphaned_unlanded: SsdResidencyBucket,
    pub unexplained: SsdResidencyBucket,
    pub provider_managed_bytes: u64,
    pub operational_metadata_bytes: u64,
    pub missing_managed_payloads: u64,
    pub missing_managed_payload_bytes: u64,
    pub unreadable_entries: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Disposition {
    SettledUncleared,
    AwaitingHdd,
    OrphanedUnlanded,
}

#[derive(Clone, Debug)]
struct ExpectedPayload {
    bytes: u64,
    disposition: Disposition,
}

#[derive(Default)]
struct ExpectedPayloads {
    entries: BTreeMap<PathBuf, ExpectedPayload>,
    unsafe_rows: u64,
}

/// Inspect both the authoritative native-SSD table and regular files below a
/// managed SSD root.  A malformed metadata path, a duplicate expected path,
/// an unreadable directory, or a symlink causes fail-closed partial coverage;
/// no questionable file is treated as eligible for removal.
pub fn build_ssd_residency_report(
    ssd_root: impl AsRef<Path>,
    live_sqlite_path: impl AsRef<Path>,
    generated_at_utc: &str,
) -> Result<SsdResidencyReport, SsdResidencyError> {
    let root = canonical_directory(ssd_root.as_ref())?;
    let expected = expected_native_payloads(live_sqlite_path.as_ref())?;
    let mut report = SsdResidencyReport {
        schema: SSD_RESIDENCY_REPORT_SCHEMA,
        generated_at_utc: generated_at_utc.to_string(),
        coverage: SsdResidencyCoverage::Complete,
        native_payload_bytes: 0,
        settled_uncleared: SsdResidencyBucket::default(),
        awaiting_hdd: SsdResidencyBucket::default(),
        orphaned_unlanded: SsdResidencyBucket::default(),
        unexplained: SsdResidencyBucket::default(),
        provider_managed_bytes: 0,
        operational_metadata_bytes: 0,
        missing_managed_payloads: 0,
        missing_managed_payload_bytes: 0,
        unreadable_entries: 0,
    };
    for _ in 0..expected.unsafe_rows {
        mark_partial(&mut report);
    }
    let mut seen = BTreeSet::new();
    let scan_issues = scan_regular_files(&root, &mut |path, bytes| {
        let relative = match path.strip_prefix(&root) {
            Ok(relative) => relative.to_path_buf(),
            Err(_) => {
                mark_partial(&mut report);
                return;
            }
        };
        if let Some(payload) = expected.entries.get(&relative) {
            seen.insert(relative);
            if payload.bytes != bytes {
                record(&mut report.orphaned_unlanded, bytes);
                return;
            }
            match payload.disposition {
                Disposition::SettledUncleared => record(&mut report.settled_uncleared, bytes),
                Disposition::AwaitingHdd => record(&mut report.awaiting_hdd, bytes),
                Disposition::OrphanedUnlanded => record(&mut report.orphaned_unlanded, bytes),
            }
        } else if is_provider_managed(&relative) {
            report.provider_managed_bytes = report.provider_managed_bytes.saturating_add(bytes);
        } else if is_operational_metadata(&relative) {
            report.operational_metadata_bytes =
                report.operational_metadata_bytes.saturating_add(bytes);
        } else {
            record(&mut report.unexplained, bytes);
        }
    })?;
    for _ in 0..scan_issues {
        mark_partial(&mut report);
    }

    for (path, payload) in expected.entries {
        if !seen.contains(&path) {
            report.missing_managed_payloads = report.missing_managed_payloads.saturating_add(1);
            report.missing_managed_payload_bytes = report
                .missing_managed_payload_bytes
                .saturating_add(payload.bytes);
            mark_partial(&mut report);
        }
    }
    report.native_payload_bytes = report
        .settled_uncleared
        .bytes
        .saturating_add(report.awaiting_hdd.bytes)
        .saturating_add(report.orphaned_unlanded.bytes)
        .saturating_add(report.unexplained.bytes);
    let denominator = report.native_payload_bytes.max(1);
    for bucket in [
        &mut report.settled_uncleared,
        &mut report.awaiting_hdd,
        &mut report.orphaned_unlanded,
        &mut report.unexplained,
    ] {
        bucket.fraction_basis_points = u16::try_from(
            bucket
                .bytes
                .saturating_mul(10_000)
                .checked_div(denominator)
                .unwrap_or(0)
                .min(10_000),
        )
        .unwrap_or(10_000);
    }
    Ok(report)
}

pub fn persist_ssd_residency_report(
    path: impl AsRef<Path>,
    report: &SsdResidencyReport,
) -> Result<(), SsdResidencyError> {
    let path = path.as_ref();
    let parent = path
        .parent()
        .ok_or_else(|| SsdResidencyError::UnsafePath(path.to_path_buf()))?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let encoded = serde_json::to_vec_pretty(report)?;
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)?;
    file.write_all(&encoded)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    fs::rename(&temporary, path)?;
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

fn expected_native_payloads(
    live_sqlite_path: &Path,
) -> Result<ExpectedPayloads, SsdResidencyError> {
    let connection = Connection::open_with_flags(
        live_sqlite_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let mut statement = connection.prepare(
        "SELECT s.relative_path, s.size_bytes, s.eviction_eligible, s.evicted_at_utc,
                q.state, q.required_copy_count, q.verified_copy_count
           FROM ssd_object_placements s
           LEFT JOIN destage_queue q ON q.object_id=s.object_id",
    )?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, bool>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<i64>>(5)?,
            row.get::<_, Option<i64>>(6)?,
        ))
    })?;
    let mut expected = ExpectedPayloads::default();
    let mut duplicate_paths = BTreeSet::new();
    for row in rows {
        let (relative, bytes, eviction_eligible, evicted, queue_state, required, verified) = row?;
        let Ok(bytes) = u64::try_from(bytes) else {
            expected.unsafe_rows = expected.unsafe_rows.saturating_add(1);
            continue;
        };
        let Some(path) = safe_relative_path(&relative) else {
            expected.unsafe_rows = expected.unsafe_rows.saturating_add(1);
            continue;
        };
        let disposition = if eviction_eligible
            && evicted.is_none()
            && queue_state.as_deref() == Some("hdd_copy_verified")
            && verified.unwrap_or(-1) >= required.unwrap_or(i64::MAX)
        {
            Disposition::SettledUncleared
        } else if evicted.is_none()
            && matches!(
                queue_state.as_deref(),
                Some("queued_for_hdd" | "hdd_copying" | "destage_failed" | "paused")
            )
        {
            Disposition::AwaitingHdd
        } else {
            Disposition::OrphanedUnlanded
        };
        // A duplicate SSD path cannot be safely attributed to either object.
        // Leave it in the physical scan's unexplained bucket instead.
        if duplicate_paths.contains(&path) {
            continue;
        }
        if expected
            .entries
            .insert(path.clone(), ExpectedPayload { bytes, disposition })
            .is_some()
        {
            expected.entries.remove(&path);
            duplicate_paths.insert(path);
            expected.unsafe_rows = expected.unsafe_rows.saturating_add(1);
        }
    }
    Ok(expected)
}

fn scan_regular_files(
    root: &Path,
    visitor: &mut impl FnMut(&Path, u64),
) -> Result<u64, SsdResidencyError> {
    let mut pending = vec![root.to_path_buf()];
    let mut issues = 0_u64;
    while let Some(directory) = pending.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(_) => {
                issues = issues.saturating_add(1);
                continue;
            }
        };
        for entry in entries {
            let Ok(entry) = entry else {
                issues = issues.saturating_add(1);
                continue;
            };
            let path = entry.path();
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(_) => {
                    issues = issues.saturating_add(1);
                    continue;
                }
            };
            if metadata.file_type().is_symlink() {
                issues = issues.saturating_add(1);
            } else if metadata.is_dir() {
                pending.push(path);
            } else if metadata.is_file() {
                visitor(&path, metadata.len());
            } else {
                issues = issues.saturating_add(1);
            }
        }
    }
    Ok(issues)
}

fn safe_relative_path(value: &str) -> Option<PathBuf> {
    let path = Path::new(value);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        None
    } else {
        Some(path.to_path_buf())
    }
}

fn is_provider_managed(path: &Path) -> bool {
    path.components()
        .next()
        .is_some_and(|component| component.as_os_str() == "garage")
}

fn is_operational_metadata(path: &Path) -> bool {
    let value = path.to_string_lossy();
    value == ".dasobjectstore/live.sqlite"
        || value.starts_with(".dasobjectstore/live.sqlite-")
        || value == ".dasobjectstore/device.env"
        || value.ends_with(".json")
        || value.ends_with(".env")
}

fn record(bucket: &mut SsdResidencyBucket, bytes: u64) {
    bucket.files = bucket.files.saturating_add(1);
    bucket.bytes = bucket.bytes.saturating_add(bytes);
}

fn mark_partial(report: &mut SsdResidencyReport) {
    report.coverage = SsdResidencyCoverage::Partial;
    report.unreadable_entries = report.unreadable_entries.saturating_add(1);
}

fn canonical_directory(path: &Path) -> Result<PathBuf, SsdResidencyError> {
    let canonical = fs::canonicalize(path)?;
    if !canonical.is_dir() {
        return Err(SsdResidencyError::UnsafePath(path.to_path_buf()));
    }
    Ok(canonical)
}

#[derive(Debug)]
pub enum SsdResidencyError {
    Io(io::Error),
    Sqlite(rusqlite::Error),
    Json(serde_json::Error),
    UnsafePath(PathBuf),
}

impl std::fmt::Display for SsdResidencyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "SSD residency inspection failed: {error}"),
            Self::Sqlite(error) => write!(formatter, "SSD residency metadata failed: {error}"),
            Self::Json(error) => write!(formatter, "SSD residency report encoding failed: {error}"),
            Self::UnsafePath(path) => {
                write!(formatter, "unsafe SSD residency path: {}", path.display())
            }
        }
    }
}

impl std::error::Error for SsdResidencyError {}

impl From<io::Error> for SsdResidencyError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<rusqlite::Error> for SsdResidencyError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<serde_json::Error> for SsdResidencyError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dasobjectstore_metadata::LIVE_SCHEMA_SQL;
    use rusqlite::params;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn classifies_native_ssd_bytes_and_retains_unexplained_data() {
        let root = temporary_root();
        let ssd = root.join("ssd");
        fs::create_dir_all(ssd.join(".dasobjectstore/ingest/jobs/settled")).expect("settled root");
        fs::create_dir_all(ssd.join(".dasobjectstore/ingest/jobs/queued")).expect("queued root");
        fs::create_dir_all(ssd.join(".dasobjectstore/ingest/jobs/orphan")).expect("orphan root");
        fs::write(
            ssd.join(".dasobjectstore/ingest/jobs/settled/payload"),
            b"settled",
        )
        .expect("settled");
        fs::write(
            ssd.join(".dasobjectstore/ingest/jobs/queued/payload"),
            b"queued",
        )
        .expect("queued");
        fs::write(
            ssd.join(".dasobjectstore/ingest/jobs/orphan/payload"),
            b"orphan",
        )
        .expect("orphan");
        fs::write(ssd.join("foreign-payload"), b"unknown").expect("unknown");
        fs::create_dir_all(ssd.join("garage")).expect("garage");
        fs::write(ssd.join("garage/index"), b"provider").expect("provider");

        let database = ssd.join(".dasobjectstore/live.sqlite");
        let connection = Connection::open(&database).expect("database");
        connection.execute_batch(LIVE_SCHEMA_SQL).expect("schema");
        connection
            .execute(
                "INSERT INTO pools(pool_id,state,created_at_utc,updated_at_utc)
             VALUES('pool-a','Clean','now','now')",
                [],
            )
            .expect("pool");
        connection.execute(
            "INSERT INTO stores(store_id,pool_id,class,policy_json,created_at_utc,updated_at_utc)
             VALUES('store-a','pool-a','generated_data','{}','now','now')",
            [],
        ).expect("store");
        for (object_id, path, size, eligible, state, required, verified) in [
            (
                "settled",
                ".dasobjectstore/ingest/jobs/settled/payload",
                7_i64,
                true,
                "hdd_copy_verified",
                1_i64,
                1_i64,
            ),
            (
                "queued",
                ".dasobjectstore/ingest/jobs/queued/payload",
                6_i64,
                false,
                "queued_for_hdd",
                1_i64,
                0_i64,
            ),
            (
                "orphan",
                ".dasobjectstore/ingest/jobs/orphan/payload",
                6_i64,
                false,
                "needs_review",
                1_i64,
                0_i64,
            ),
        ] {
            connection.execute(
                "INSERT INTO objects(object_id,store_id,object_type,state,size_bytes,content_hash,created_at_utc,updated_at_utc)
                 VALUES(?1,'store-a','naive','PlacementPlanned',?2,'hash','now','now')",
                params![object_id, size],
            ).expect("object");
            connection.execute(
                "INSERT INTO ssd_object_placements(object_id,store_id,relative_path,size_bytes,content_hash_algorithm,content_hash,verified_at_utc,eviction_eligible,created_at_utc,updated_at_utc)
                 VALUES(?1,'store-a',?2,?3,'sha256','hash','now',?4,'now','now')",
                params![object_id, path, size, eligible],
            ).expect("placement");
            connection.execute(
                "INSERT INTO destage_queue(destage_job_id,store_id,object_id,state,expected_size_bytes,content_hash_algorithm,content_hash,acknowledgement_policy,required_copy_count,priority,max_attempts,verified_copy_count,created_at_utc,updated_at_utc)
                 VALUES(?1,'store-a',?1,?2,?3,'sha256','hash','after_ssd_ingest',?4,0,3,?5,'now','now')",
                params![object_id, state, size, required, verified],
            ).expect("queue");
        }
        drop(connection);

        let report =
            build_ssd_residency_report(&ssd, &database, "2026-08-31T00:00:00Z").expect("report");
        assert_eq!(report.coverage, SsdResidencyCoverage::Complete);
        assert_eq!(report.settled_uncleared.bytes, 7);
        assert_eq!(report.awaiting_hdd.bytes, 6);
        assert_eq!(report.orphaned_unlanded.bytes, 6);
        assert_eq!(report.unexplained.bytes, 7);
        assert_eq!(report.provider_managed_bytes, 8);
        assert_eq!(report.native_payload_bytes, 26);
        assert_eq!(report.settled_uncleared.fraction_basis_points, 2_692);
        fs::remove_dir_all(root).expect("cleanup");
    }

    fn temporary_root() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dasobjectstore-ssd-residency-{}-{nonce}",
            std::process::id()
        ))
    }
}
