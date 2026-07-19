//! Fail-closed garbage collection for daemon-owned transient storage.
//!
//! Collection is deliberately evidence based.  A directory name or an old
//! modification time is never, by itself, authority to remove data.

#[cfg(test)]
use rusqlite::params;
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const GARBAGE_COLLECTION_REPORT_SCHEMA: &str = "dasobjectstore.garbage_collection.report.v1";
pub const PERFORMANCE_GC_MARKER_SCHEMA: &str = "dasobjectstore.performance_test.ownership.v1";
pub const PERFORMANCE_GC_MARKER_FILE: &str = ".dasobjectstore-gc.json";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GarbageCollectMode {
    Inventory,
    Reclaim,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GarbageCollectTrigger {
    Startup,
    Scheduled,
    Manual,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GarbageCollectKind {
    IngestJob,
    PerformanceTest,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GarbageCollectDecision {
    Reclaimable,
    Reclaimed,
    Retained,
    Failed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GarbageCollectItem {
    pub kind: GarbageCollectKind,
    /// Relative to the configured SSD root. Never an arbitrary host path.
    pub managed_path: String,
    pub bytes: u64,
    pub decision: GarbageCollectDecision,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GarbageCollectReport {
    pub schema: String,
    pub run_id: String,
    pub trigger: GarbageCollectTrigger,
    pub mode: GarbageCollectMode,
    pub started_at_utc: String,
    pub completed_at_utc: String,
    pub candidate_bytes: u64,
    pub reclaimed_bytes: u64,
    pub retained_bytes: u64,
    pub items: Vec<GarbageCollectItem>,
}

#[derive(Clone, Debug)]
pub struct GarbageCollectorConfig {
    pub ssd_root: PathBuf,
    pub live_sqlite_path: PathBuf,
    pub report_journal_path: PathBuf,
    pub terminal_grace: Duration,
    pub maximum_items_per_run: usize,
}

impl GarbageCollectorConfig {
    pub fn for_daemon_state(ssd_root: impl Into<PathBuf>, state_dir: impl AsRef<Path>) -> Self {
        let ssd_root = ssd_root.into();
        Self {
            live_sqlite_path: ssd_root.join(".dasobjectstore/live.sqlite"),
            ssd_root,
            report_journal_path: state_dir.as_ref().join("garbage-collection/latest.json"),
            terminal_grace: Duration::from_secs(24 * 60 * 60),
            maximum_items_per_run: 256,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PerformanceGcMarker {
    pub schema: String,
    pub run_id: String,
    pub state: String,
    pub keep_temp: bool,
    pub created_at_utc: String,
    pub updated_at_utc: String,
}

pub fn run_garbage_collection(
    config: &GarbageCollectorConfig,
    mode: GarbageCollectMode,
    trigger: GarbageCollectTrigger,
    run_id: impl Into<String>,
    now_utc: &str,
    now: SystemTime,
) -> Result<GarbageCollectReport, GarbageCollectError> {
    let root = canonical_directory(&config.ssd_root)?;
    let mut items = Vec::new();
    scan_ingest_jobs(config, &root, mode, now, &mut items)?;
    if items.len() < config.maximum_items_per_run {
        scan_performance_runs(config, &root, mode, now, &mut items)?;
    }
    items.truncate(config.maximum_items_per_run);
    let candidate_bytes = items
        .iter()
        .filter(|item| {
            matches!(
                item.decision,
                GarbageCollectDecision::Reclaimable | GarbageCollectDecision::Reclaimed
            )
        })
        .map(|item| item.bytes)
        .sum();
    let reclaimed_bytes = items
        .iter()
        .filter(|item| item.decision == GarbageCollectDecision::Reclaimed)
        .map(|item| item.bytes)
        .sum();
    let retained_bytes = items
        .iter()
        .filter(|item| item.decision == GarbageCollectDecision::Retained)
        .map(|item| item.bytes)
        .sum();
    Ok(GarbageCollectReport {
        schema: GARBAGE_COLLECTION_REPORT_SCHEMA.to_string(),
        run_id: run_id.into(),
        trigger,
        mode,
        started_at_utc: now_utc.to_string(),
        completed_at_utc: now_utc.to_string(),
        candidate_bytes,
        reclaimed_bytes,
        retained_bytes,
        items,
    })
}

pub fn persist_garbage_collection_report(
    path: impl AsRef<Path>,
    report: &GarbageCollectReport,
) -> Result<(), GarbageCollectError> {
    let path = path.as_ref();
    let parent = path
        .parent()
        .ok_or(GarbageCollectError::UnsafePath(path.to_path_buf()))?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension(format!("tmp-{}", report.run_id));
    let encoded = serde_json::to_vec_pretty(report)?;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)?;
    file.write_all(&encoded)?;
    file.sync_all()?;
    fs::rename(&temporary, path)?;
    FileSync::sync_directory(parent)?;
    Ok(())
}

fn scan_ingest_jobs(
    config: &GarbageCollectorConfig,
    root: &Path,
    mode: GarbageCollectMode,
    now: SystemTime,
    items: &mut Vec<GarbageCollectItem>,
) -> Result<(), GarbageCollectError> {
    let jobs = root.join(".dasobjectstore/ingest/jobs");
    scan_immediate_directories(
        &jobs,
        |candidate| {
            let relative = managed_relative(root, candidate)?;
            let bytes = checked_managed_tree_size(root, candidate)?;
            let job_id = candidate
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or_else(|| GarbageCollectError::UnsafePath(candidate.to_path_buf()))?;
            let proof = prove_ingest_job(config, job_id, candidate, now)?;
            finish_candidate(
                config,
                root,
                candidate,
                relative,
                GarbageCollectKind::IngestJob,
                bytes,
                proof,
                mode,
            )
        },
        items,
    )
}

fn scan_performance_runs(
    config: &GarbageCollectorConfig,
    root: &Path,
    mode: GarbageCollectMode,
    now: SystemTime,
    items: &mut Vec<GarbageCollectItem>,
) -> Result<(), GarbageCollectError> {
    let runs = root.join(".dasobjectstore/performance-test");
    scan_immediate_directories(
        &runs,
        |candidate| {
            let relative = managed_relative(root, candidate)?;
            let bytes = checked_managed_tree_size(root, candidate)?;
            let marker_path = candidate.join(PERFORMANCE_GC_MARKER_FILE);
            let proof = match read_marker(&marker_path) {
                Ok(marker) if marker.keep_temp => Proof::retain("performance_keep_requested"),
                Ok(marker) if marker.schema != PERFORMANCE_GC_MARKER_SCHEMA => {
                    Proof::retain("performance_marker_schema_unsupported")
                }
                Ok(marker)
                    if !matches!(
                        marker.state.as_str(),
                        "complete" | "cancelled" | "abandoned"
                    ) =>
                {
                    Proof::retain("performance_run_active")
                }
                Ok(marker) if old_enough(candidate, config.terminal_grace, now)? => Proof::reclaim(
                    "performance_terminal_marker",
                    vec![
                        format!("run_id={}", marker.run_id),
                        format!("state={}", marker.state),
                    ],
                ),
                Ok(_) => Proof::retain("performance_terminal_grace"),
                Err(GarbageCollectError::Io(error))
                    if error.kind() == std::io::ErrorKind::NotFound =>
                {
                    Proof::retain("performance_legacy_unowned")
                }
                Err(error) => return Err(error),
            };
            finish_candidate(
                config,
                root,
                candidate,
                relative,
                GarbageCollectKind::PerformanceTest,
                bytes,
                proof,
                mode,
            )
        },
        items,
    )
}

fn scan_immediate_directories(
    parent: &Path,
    mut inspect: impl FnMut(&Path) -> Result<GarbageCollectItem, GarbageCollectError>,
    items: &mut Vec<GarbageCollectItem>,
) -> Result<(), GarbageCollectError> {
    let entries = match fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            items.push(GarbageCollectItem {
                kind: GarbageCollectKind::IngestJob,
                managed_path: entry.file_name().to_string_lossy().into_owned(),
                bytes: 0,
                decision: GarbageCollectDecision::Retained,
                reason: "unsafe_or_unknown_entry".to_string(),
                evidence: Vec::new(),
            });
            continue;
        }
        items.push(inspect(&entry.path())?);
    }
    Ok(())
}

fn prove_ingest_job(
    config: &GarbageCollectorConfig,
    job_id: &str,
    candidate: &Path,
    now: SystemTime,
) -> Result<Proof, GarbageCollectError> {
    let connection = Connection::open_with_flags(
        &config.live_sqlite_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let job: Option<(String, Option<String>)> = connection
        .query_row(
            "SELECT state, object_id FROM ingest_jobs WHERE ingest_job_id=?1",
            [job_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((state, object_id)) = job else {
        return Ok(Proof::retain("ingest_job_metadata_missing"));
    };
    if !matches!(state.as_str(), "Complete" | "Failed" | "Cancelled") {
        return Ok(Proof::retain("ingest_job_active"));
    }
    if !old_enough(candidate, config.terminal_grace, now)? {
        return Ok(Proof::retain("ingest_job_terminal_grace"));
    }
    let Some(object_id) = object_id else {
        return if matches!(state.as_str(), "Failed" | "Cancelled") {
            Ok(Proof::reclaim(
                "terminal_unacknowledged_ingest",
                vec![format!("job_id={job_id}"), format!("state={state}")],
            ))
        } else {
            Ok(Proof::retain("complete_job_without_object_evidence"))
        };
    };
    let ssd: Option<(bool, Option<String>, String)> = connection
        .query_row(
            "SELECT eviction_eligible, evicted_at_utc, relative_path FROM ssd_object_placements WHERE object_id=?1",
            [&object_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    if let Some((eligible, evicted, relative)) = ssd {
        if evicted.is_some() {
            return Ok(Proof::reclaim(
                "ssd_placement_already_evicted",
                vec![format!("object_id={object_id}")],
            ));
        }
        let expected = candidate.join("payload");
        let recorded = config.ssd_root.join(relative);
        if recorded != expected {
            return Ok(Proof::retain("ssd_placement_path_mismatch"));
        }
        let settled: Option<(String, i64, i64)> = connection
            .query_row(
                "SELECT state, required_copy_count, verified_copy_count FROM destage_queue WHERE object_id=?1",
                [&object_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        if eligible
            && settled.as_ref().is_some_and(|(state, required, verified)| {
                state == "hdd_copy_verified" && verified >= required
            })
        {
            return Ok(Proof::reclaim_with_ssd_eviction(
                "verified_hdd_policy_allows_ssd_eviction",
                vec![format!("object_id={object_id}")],
                object_id,
            ));
        }
        return Ok(Proof::retain("ssd_copy_still_required"));
    }
    let verified_hdd: i64 = connection.query_row(
        "SELECT COUNT(*) FROM placements WHERE object_id=?1 AND verified_at_utc IS NOT NULL",
        [&object_id],
        |row| row.get(0),
    )?;
    if state == "Complete" && verified_hdd > 0 {
        Ok(Proof::reclaim(
            "legacy_ingest_has_verified_hdd_placement",
            vec![
                format!("object_id={object_id}"),
                format!("verified_hdd={verified_hdd}"),
            ],
        ))
    } else {
        Ok(Proof::retain("object_durability_not_proven"))
    }
}

fn finish_candidate(
    config: &GarbageCollectorConfig,
    root: &Path,
    candidate: &Path,
    managed_path: String,
    kind: GarbageCollectKind,
    bytes: u64,
    proof: Proof,
    mode: GarbageCollectMode,
) -> Result<GarbageCollectItem, GarbageCollectError> {
    if !proof.reclaimable {
        return Ok(GarbageCollectItem {
            kind,
            managed_path,
            bytes,
            decision: GarbageCollectDecision::Retained,
            reason: proof.reason,
            evidence: proof.evidence,
        });
    }
    if mode == GarbageCollectMode::Inventory {
        return Ok(GarbageCollectItem {
            kind,
            managed_path,
            bytes,
            decision: GarbageCollectDecision::Reclaimable,
            reason: proof.reason,
            evidence: proof.evidence,
        });
    }
    let quarantined = quarantine_candidate(root, candidate)?;
    if let Some(object_id) = proof.mark_ssd_evicted.as_deref() {
        let mark_result = Connection::open(&config.live_sqlite_path)
            .and_then(|connection| {
                connection.execute(
                    "UPDATE ssd_object_placements SET evicted_at_utc=COALESCE(evicted_at_utc,'garbage-collected'), updated_at_utc='garbage-collected' WHERE object_id=?1 AND eviction_eligible=1",
                    [object_id],
                )
            });
        match mark_result {
            Ok(1) => {}
            Ok(_) => {
                restore_quarantined_candidate(candidate, &quarantined)?;
                return Err(GarbageCollectError::SsdEvictionMarkMissing(
                    object_id.to_string(),
                ));
            }
            Err(error) => {
                restore_quarantined_candidate(candidate, &quarantined)?;
                return Err(error.into());
            }
        }
    }
    fs::remove_dir_all(&quarantined)?;
    FileSync::sync_directory(
        quarantined
            .parent()
            .ok_or_else(|| GarbageCollectError::UnsafePath(quarantined.clone()))?,
    )?;
    Ok(GarbageCollectItem {
        kind,
        managed_path,
        bytes,
        decision: GarbageCollectDecision::Reclaimed,
        reason: proof.reason,
        evidence: proof.evidence,
    })
}

fn quarantine_candidate(root: &Path, candidate: &Path) -> Result<PathBuf, GarbageCollectError> {
    let relative = candidate
        .strip_prefix(root)
        .map_err(|_| GarbageCollectError::UnsafePath(candidate.to_path_buf()))?;
    let encoded = relative
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>();
    let quarantine_root = root.join(".dasobjectstore/.gc-quarantine");
    fs::create_dir_all(&quarantine_root)?;
    reject_symlink(&quarantine_root)?;
    let destination = quarantine_root.join(format!("{}-{}", encoded, unique_suffix()));
    fs::rename(candidate, &destination)?;
    FileSync::sync_directory(
        candidate
            .parent()
            .ok_or_else(|| GarbageCollectError::UnsafePath(candidate.to_path_buf()))?,
    )?;
    FileSync::sync_directory(&quarantine_root)?;
    Ok(destination)
}

pub(crate) fn reclaim_managed_directory(
    root: &Path,
    candidate: &Path,
) -> Result<(), GarbageCollectError> {
    let quarantined = quarantine_candidate(root, candidate)?;
    fs::remove_dir_all(&quarantined)?;
    FileSync::sync_directory(
        quarantined
            .parent()
            .ok_or_else(|| GarbageCollectError::UnsafePath(quarantined.clone()))?,
    )
}

fn restore_quarantined_candidate(
    candidate: &Path,
    quarantined: &Path,
) -> Result<(), GarbageCollectError> {
    fs::rename(quarantined, candidate)?;
    FileSync::sync_directory(
        candidate
            .parent()
            .ok_or_else(|| GarbageCollectError::UnsafePath(candidate.to_path_buf()))?,
    )?;
    Ok(())
}

pub(crate) fn checked_managed_tree_size(
    root: &Path,
    candidate: &Path,
) -> Result<u64, GarbageCollectError> {
    let root_metadata = fs::metadata(root)?;
    let root_device = device_id(&root_metadata);
    let mut pending = vec![candidate.to_path_buf()];
    let mut bytes = 0_u64;
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() || device_id(&metadata) != root_device {
            return Err(GarbageCollectError::UnsafePath(path));
        }
        if metadata.is_file() {
            if hard_link_count(&metadata) > 1 {
                return Err(GarbageCollectError::HardLinkedFile(path));
            }
            bytes = bytes.saturating_add(metadata.len());
        } else if metadata.is_dir() {
            for entry in fs::read_dir(path)? {
                pending.push(entry?.path());
            }
        } else {
            return Err(GarbageCollectError::UnsafePath(path));
        }
    }
    Ok(bytes)
}

fn canonical_directory(path: &Path) -> Result<PathBuf, GarbageCollectError> {
    reject_symlink(path)?;
    let canonical = fs::canonicalize(path)?;
    if !fs::metadata(&canonical)?.is_dir() {
        return Err(GarbageCollectError::UnsafePath(path.to_path_buf()));
    }
    Ok(canonical)
}

fn reject_symlink(path: &Path) -> Result<(), GarbageCollectError> {
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(GarbageCollectError::UnsafePath(path.to_path_buf()));
    }
    Ok(())
}

fn managed_relative(root: &Path, candidate: &Path) -> Result<String, GarbageCollectError> {
    let relative = candidate
        .strip_prefix(root)
        .map_err(|_| GarbageCollectError::UnsafePath(candidate.to_path_buf()))?;
    if relative.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(GarbageCollectError::UnsafePath(candidate.to_path_buf()));
    }
    Ok(relative.to_string_lossy().into_owned())
}

fn old_enough(path: &Path, grace: Duration, now: SystemTime) -> Result<bool, GarbageCollectError> {
    let modified = fs::symlink_metadata(path)?.modified()?;
    Ok(now.duration_since(modified).unwrap_or_default() >= grace)
}

fn read_marker(path: &Path) -> Result<PerformanceGcMarker, GarbageCollectError> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

fn unique_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

#[cfg(unix)]
fn device_id(metadata: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    metadata.dev()
}

#[cfg(not(unix))]
fn device_id(_: &fs::Metadata) -> u64 {
    0
}

#[cfg(unix)]
fn hard_link_count(metadata: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    metadata.nlink()
}

#[cfg(not(unix))]
fn hard_link_count(_: &fs::Metadata) -> u64 {
    1
}

struct FileSync;
impl FileSync {
    fn sync_directory(path: &Path) -> Result<(), GarbageCollectError> {
        OpenOptions::new().read(true).open(path)?.sync_all()?;
        Ok(())
    }
}

struct Proof {
    reclaimable: bool,
    reason: String,
    evidence: Vec<String>,
    mark_ssd_evicted: Option<String>,
}

impl Proof {
    fn retain(reason: &str) -> Self {
        Self {
            reclaimable: false,
            reason: reason.to_string(),
            evidence: Vec::new(),
            mark_ssd_evicted: None,
        }
    }
    fn reclaim(reason: &str, evidence: Vec<String>) -> Self {
        Self {
            reclaimable: true,
            reason: reason.to_string(),
            evidence,
            mark_ssd_evicted: None,
        }
    }
    fn reclaim_with_ssd_eviction(reason: &str, evidence: Vec<String>, object_id: String) -> Self {
        Self {
            reclaimable: true,
            reason: reason.to_string(),
            evidence,
            mark_ssd_evicted: Some(object_id),
        }
    }
}

#[derive(Debug)]
pub enum GarbageCollectError {
    Io(std::io::Error),
    Sqlite(rusqlite::Error),
    Json(serde_json::Error),
    UnsafePath(PathBuf),
    HardLinkedFile(PathBuf),
    SsdEvictionMarkMissing(String),
}

impl Display for GarbageCollectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "garbage collection IO failed: {error}"),
            Self::Sqlite(error) => write!(
                formatter,
                "garbage collection metadata proof failed: {error}"
            ),
            Self::Json(error) => write!(
                formatter,
                "garbage collection marker/report failed: {error}"
            ),
            Self::UnsafePath(path) => write!(
                formatter,
                "refusing unsafe garbage collection path: {}",
                path.display()
            ),
            Self::HardLinkedFile(path) => write!(
                formatter,
                "refusing hard-linked garbage collection payload: {}",
                path.display()
            ),
            Self::SsdEvictionMarkMissing(object_id) => write!(
                formatter,
                "garbage collection removed a proven SSD copy but could not mark its placement evicted: {object_id}"
            ),
        }
    }
}

impl std::error::Error for GarbageCollectError {}
impl From<std::io::Error> for GarbageCollectError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}
impl From<rusqlite::Error> for GarbageCollectError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sqlite(value)
    }
}
impl From<serde_json::Error> for GarbageCollectError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dasobjectstore_metadata::LIVE_SCHEMA_SQL;

    #[test]
    fn inventory_and_reclaim_terminal_unacknowledged_job() {
        let fixture = Fixture::new("terminal");
        fixture.insert_store();
        fixture.insert_job("job-a", "Failed", None);
        let job = fixture.job("job-a");
        fs::write(job.join("payload"), b"recoverable").expect("payload");
        let inventory = fixture.run(GarbageCollectMode::Inventory);
        assert_eq!(
            inventory.items[0].decision,
            GarbageCollectDecision::Reclaimable
        );
        assert!(job.exists());
        let reclaim = fixture.run(GarbageCollectMode::Reclaim);
        assert_eq!(reclaim.items[0].decision, GarbageCollectDecision::Reclaimed);
        assert!(!job.exists());
    }

    #[test]
    fn active_and_unknown_ingest_jobs_are_retained() {
        let fixture = Fixture::new("active");
        fixture.insert_store();
        fixture.insert_job("job-active", "StagingSsd", None);
        fs::create_dir_all(fixture.job("job-unknown")).expect("unknown");
        let report = fixture.run(GarbageCollectMode::Reclaim);
        assert!(report
            .items
            .iter()
            .all(|item| item.decision == GarbageCollectDecision::Retained));
    }

    #[test]
    fn performance_requires_supported_terminal_marker() {
        let fixture = Fixture::new("performance");
        let legacy = fixture.performance("legacy");
        fs::create_dir_all(&legacy).expect("legacy");
        let marked = fixture.performance("marked");
        fs::create_dir_all(&marked).expect("marked");
        fs::write(
            marked.join(PERFORMANCE_GC_MARKER_FILE),
            serde_json::to_vec(&PerformanceGcMarker {
                schema: PERFORMANCE_GC_MARKER_SCHEMA.to_string(),
                run_id: "marked".to_string(),
                state: "complete".to_string(),
                keep_temp: false,
                created_at_utc: "2026-01-01T00:00:00Z".to_string(),
                updated_at_utc: "2026-01-01T00:00:00Z".to_string(),
            })
            .expect("json"),
        )
        .expect("marker");
        let report = fixture.run(GarbageCollectMode::Inventory);
        assert!(report
            .items
            .iter()
            .any(|item| item.reason == "performance_legacy_unowned"));
        assert!(report
            .items
            .iter()
            .any(|item| item.reason == "performance_terminal_marker"));
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_and_hard_links_fail_closed() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new("links");
        fixture.insert_store();
        fixture.insert_job("job-link", "Failed", None);
        let outside = fixture.root.join("outside");
        fs::write(&outside, b"outside").expect("outside");
        symlink(&outside, fixture.job("job-link").join("payload")).expect("symlink");
        assert!(fixture.try_run(GarbageCollectMode::Reclaim).is_err());

        fs::remove_file(fixture.job("job-link").join("payload")).expect("unlink");
        fs::remove_file(&outside).expect("remove outside");
        fs::write(fixture.job("job-link").join("payload"), b"hard").expect("payload");
        fs::hard_link(fixture.job("job-link").join("payload"), &outside).expect("hardlink");
        assert!(fixture.try_run(GarbageCollectMode::Reclaim).is_err());
    }

    struct Fixture {
        root: PathBuf,
        config: GarbageCollectorConfig,
    }
    impl Fixture {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!("dos-gc-{name}-{}", unique_suffix()));
            let ssd = root.join("ssd");
            fs::create_dir_all(ssd.join(".dasobjectstore/ingest/jobs")).expect("jobs");
            fs::create_dir_all(ssd.join(".dasobjectstore/performance-test")).expect("perf");
            let db = ssd.join(".dasobjectstore/live.sqlite");
            Connection::open(&db)
                .expect("db")
                .execute_batch(LIVE_SCHEMA_SQL)
                .expect("schema");
            let config = GarbageCollectorConfig {
                ssd_root: ssd,
                live_sqlite_path: db,
                report_journal_path: root.join("state/gc.json"),
                terminal_grace: Duration::ZERO,
                maximum_items_per_run: 100,
            };
            Self { root, config }
        }
        fn insert_store(&self) {
            let db = Connection::open(&self.config.live_sqlite_path).expect("db");
            db.execute(
                "INSERT OR IGNORE INTO pools VALUES('pool','Active','now','now')",
                [],
            )
            .expect("pool");
            db.execute("INSERT OR IGNORE INTO stores VALUES('store','pool','GeneratedData','{}','now','now')", []).expect("store");
        }
        fn insert_job(&self, id: &str, state: &str, object: Option<&str>) {
            fs::create_dir_all(self.job(id)).expect("job");
            Connection::open(&self.config.live_sqlite_path).expect("db").execute(
                "INSERT INTO ingest_jobs(ingest_job_id,store_id,object_id,state,ingest_mode,acknowledgement_policy,staging_path,created_at_utc,updated_at_utc) VALUES(?1,'store',?2,?3,'SsdFirst','AfterSsdIngest',?4,'now','now')",
                params![id, object, state, self.job(id).join("payload").display().to_string()]).expect("job row");
        }
        fn job(&self, id: &str) -> PathBuf {
            self.config
                .ssd_root
                .join(".dasobjectstore/ingest/jobs")
                .join(id)
        }
        fn performance(&self, id: &str) -> PathBuf {
            self.config
                .ssd_root
                .join(".dasobjectstore/performance-test")
                .join(id)
        }
        fn try_run(
            &self,
            mode: GarbageCollectMode,
        ) -> Result<GarbageCollectReport, GarbageCollectError> {
            run_garbage_collection(
                &self.config,
                mode,
                GarbageCollectTrigger::Startup,
                "run",
                "2026-07-19T00:00:00Z",
                SystemTime::now(),
            )
        }
        fn run(&self, mode: GarbageCollectMode) -> GarbageCollectReport {
            self.try_run(mode).expect("gc")
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}
