//! Idle-gated background verification, rebalancing, and disk evacuation.

mod operation;

use super::ingest_files::discover_managed_hdd_roots;
use super::LiveStatusRegistry;
use dasobjectstore_core::ids::DiskId;
use dasobjectstore_core::utc::parse_utc_timestamp_seconds;
use dasobjectstore_metadata::assurance::{
    assurance_relocation_committed, complete_assurance_drain_if_empty,
};
use dasobjectstore_metadata::{
    acquire_disk_capacity_claims, assurance_primary_work_pending, commit_assurance_relocation,
    list_assurance_disk_states, list_assurance_placement_candidates, measure_ssd_capacity,
    read_outstanding_disk_capacity, record_assurance_hash_failure, record_assurance_verification,
    release_disk_capacity_claims, write_verified_hdd_copy_with_controlled_progress,
    AssuranceMetadataError, AssurancePlacementCandidate, DiskCapacityClaimAllocation,
    DiskCapacityClaimError, DiskCapacityClaimKind, DiskCapacityClaimRequest, HddCopyError,
    HddCopyRequest,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::{self, Display};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use operation::{DurableAssuranceOperation, OperationPhase};

pub const DEFAULT_ASSURANCE_POLL_SECONDS: u64 = 30;
pub const DEFAULT_ASSURANCE_IDLE_GRACE_SECONDS: u64 = 10 * 60;
pub const DEFAULT_ASSURANCE_VERIFY_AFTER_SECONDS: u64 = 30 * 24 * 60 * 60;
pub const DEFAULT_ASSURANCE_IMBALANCE_BASIS_POINTS: u16 = 500;
pub const DEFAULT_ASSURANCE_MAX_OBJECT_BYTES: u64 = 128 * 1024 * 1024 * 1024;
pub const DEFAULT_ASSURANCE_IDLE_IO_BYTES_PER_SECOND: u64 = 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StorageAssuranceConfig {
    pub enabled: bool,
    pub poll_seconds: u64,
    pub idle_grace_seconds: u64,
    pub verify_after_seconds: u64,
    pub imbalance_basis_points: u16,
    pub max_object_bytes: u64,
    pub idle_io_bytes_per_second: u64,
    pub live_sqlite_path: PathBuf,
    pub hdd_root: PathBuf,
    pub latest_report_path: PathBuf,
    pub operation_journal_path: PathBuf,
}

impl StorageAssuranceConfig {
    pub fn from_environment(state_dir: &Path) -> Result<Self, StorageAssuranceError> {
        let ssd_root = std::env::var_os("DASOBJECTSTORE_SSD_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/srv/dasobjectstore/ssd"));
        let hdd_root = std::env::var_os("DASOBJECTSTORE_HDD_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/srv/dasobjectstore/hdd"));
        let config = Self {
            enabled: env_bool("DASOBJECTSTORE_ASSURANCE_ENABLED", true)?,
            poll_seconds: env_u64(
                "DASOBJECTSTORE_ASSURANCE_POLL_SECONDS",
                DEFAULT_ASSURANCE_POLL_SECONDS,
            )?,
            idle_grace_seconds: env_u64(
                "DASOBJECTSTORE_ASSURANCE_IDLE_GRACE_SECONDS",
                DEFAULT_ASSURANCE_IDLE_GRACE_SECONDS,
            )?,
            verify_after_seconds: env_u64(
                "DASOBJECTSTORE_ASSURANCE_VERIFY_AFTER_SECONDS",
                DEFAULT_ASSURANCE_VERIFY_AFTER_SECONDS,
            )?,
            imbalance_basis_points: u16::try_from(env_u64(
                "DASOBJECTSTORE_ASSURANCE_IMBALANCE_BASIS_POINTS",
                u64::from(DEFAULT_ASSURANCE_IMBALANCE_BASIS_POINTS),
            )?)
            .map_err(|_| {
                StorageAssuranceError::InvalidConfiguration(
                    "imbalance basis points exceed u16".to_string(),
                )
            })?,
            max_object_bytes: env_u64(
                "DASOBJECTSTORE_ASSURANCE_MAX_OBJECT_BYTES",
                DEFAULT_ASSURANCE_MAX_OBJECT_BYTES,
            )?,
            idle_io_bytes_per_second: env_u64(
                "DASOBJECTSTORE_ASSURANCE_IDLE_IO_BYTES_PER_SECOND",
                DEFAULT_ASSURANCE_IDLE_IO_BYTES_PER_SECOND,
            )?,
            live_sqlite_path: ssd_root.join(".dasobjectstore/live.sqlite"),
            hdd_root,
            latest_report_path: state_dir.join("storage-assurance/latest.json"),
            operation_journal_path: state_dir.join("storage-assurance/operation.json"),
        };
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), StorageAssuranceError> {
        if self.poll_seconds == 0
            || self.idle_grace_seconds < self.poll_seconds
            || self.verify_after_seconds == 0
            || self.max_object_bytes == 0
            || self.imbalance_basis_points > 10_000
        {
            return Err(StorageAssuranceError::InvalidConfiguration(
                "poll must be non-zero, idle grace must cover one poll, verification/max size must be non-zero, and imbalance must be <=10000 basis points".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct IdleObservation {
    primary_work_pending: bool,
    live_ingests: bool,
    garbage_collection_running: bool,
    io_bytes_per_second: u64,
}

#[derive(Debug)]
struct IdleGate {
    idle_since: Option<Instant>,
    required_idle: Duration,
    maximum_io_bytes_per_second: u64,
}

impl IdleGate {
    fn new(required_idle: Duration, maximum_io_bytes_per_second: u64) -> Self {
        Self {
            idle_since: None,
            required_idle,
            maximum_io_bytes_per_second,
        }
    }

    fn observe(&mut self, now: Instant, observation: IdleObservation) -> bool {
        let busy = observation.primary_work_pending
            || observation.live_ingests
            || observation.garbage_collection_running
            || observation.io_bytes_per_second > self.maximum_io_bytes_per_second;
        if busy {
            self.idle_since = None;
            return false;
        }
        let idle_since = self.idle_since.get_or_insert(now);
        now.duration_since(*idle_since) >= self.required_idle
    }

    fn reset(&mut self) {
        self.idle_since = None;
    }
}

#[derive(Clone, Debug)]
struct MeasuredRoot {
    disk_id: DiskId,
    root_path: PathBuf,
    available_bytes: u64,
    total_bytes: u64,
    state: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageAssuranceAction {
    Evacuate,
    Rebalance,
    Verify,
    Idle,
}

impl StorageAssuranceAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Evacuate => "evacuate",
            Self::Rebalance => "rebalance",
            Self::Verify => "verify",
            Self::Idle => "idle",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StorageAssuranceReport {
    pub schema: &'static str,
    pub completed_at_utc: String,
    pub success: bool,
    pub action: StorageAssuranceAction,
    pub object_id: Option<String>,
    pub source_disk_id: Option<String>,
    pub destination_disk_id: Option<String>,
    pub bytes: u64,
    pub source_removed: bool,
    pub message: String,
}

pub fn spawn_storage_assurance_loop(
    config: StorageAssuranceConfig,
    live_status_registry: Arc<LiveStatusRegistry>,
    now_utc: fn() -> String,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut gate = IdleGate::new(
            Duration::from_secs(config.idle_grace_seconds),
            config.idle_io_bytes_per_second,
        );
        let mut previous_io = read_linux_disk_io_bytes().ok();
        let mut previous_io_at = Instant::now();
        loop {
            thread::sleep(Duration::from_secs(config.poll_seconds));
            let observed_at = Instant::now();
            let current_io = read_linux_disk_io_bytes().ok();
            let io_rate = match (previous_io, current_io) {
                (Some(previous), Some(current)) => current
                    .saturating_sub(previous)
                    .checked_div(observed_at.duration_since(previous_io_at).as_secs().max(1))
                    .unwrap_or(0),
                _ => u64::MAX,
            };
            previous_io = current_io;
            previous_io_at = observed_at;
            let snapshot = live_status_registry.snapshot(now_utc());
            let primary_work_pending =
                assurance_primary_work_pending(&config.live_sqlite_path).unwrap_or(true);
            let observation = IdleObservation {
                primary_work_pending,
                live_ingests: snapshot.aggregate.active_ingests > 0,
                garbage_collection_running: snapshot
                    .garbage_collection
                    .is_some_and(|collection| collection.running),
                io_bytes_per_second: io_rate,
            };
            if !gate.observe(observed_at, observation) {
                continue;
            }
            let result =
                run_one_storage_assurance(&config, Arc::clone(&live_status_registry), &now_utc());
            match result {
                Ok(report) => {
                    if let Err(error) = persist_report(&config.latest_report_path, &report) {
                        eprintln!("storage assurance report persistence failed: {error}");
                    }
                }
                Err(error) => {
                    eprintln!("storage assurance retained source data: {error}");
                    let report = StorageAssuranceReport {
                        schema: "dasobjectstore.storage_assurance.report.v1",
                        completed_at_utc: now_utc(),
                        success: false,
                        action: StorageAssuranceAction::Idle,
                        object_id: None,
                        source_disk_id: None,
                        destination_disk_id: None,
                        bytes: 0,
                        source_removed: false,
                        message: error.to_string(),
                    };
                    if let Err(report_error) = persist_report(&config.latest_report_path, &report) {
                        eprintln!(
                            "storage assurance failure report persistence failed: {report_error}"
                        );
                    }
                }
            }
            gate.reset();
            previous_io = read_linux_disk_io_bytes().ok();
            previous_io_at = Instant::now();
        }
    })
}

pub fn run_one_storage_assurance(
    config: &StorageAssuranceConfig,
    live_status_registry: Arc<LiveStatusRegistry>,
    now_utc: &str,
) -> Result<StorageAssuranceReport, StorageAssuranceError> {
    let roots = measured_roots(config)?;
    if let Some(report) =
        recover_assurance_operation(config, &roots, Arc::clone(&live_status_registry), now_utc)?
    {
        return Ok(report);
    }
    for root in roots
        .iter()
        .filter(|root| root.state.eq_ignore_ascii_case("draining"))
    {
        let completion =
            complete_assurance_drain_if_empty(&config.live_sqlite_path, &root.disk_id, now_utc)?;
        if completion.transitioned_to_retired {
            return Ok(StorageAssuranceReport {
                schema: "dasobjectstore.storage_assurance.report.v1",
                completed_at_utc: now_utc.to_string(),
                success: true,
                action: StorageAssuranceAction::Evacuate,
                object_id: None,
                source_disk_id: Some(root.disk_id.to_string()),
                destination_disk_id: None,
                bytes: 0,
                source_removed: false,
                message: "drain completed; empty disk is retired and offline-ready".to_string(),
            });
        }
    }
    let candidates = list_assurance_placement_candidates(&config.live_sqlite_path)?;
    let now_seconds = parse_utc_timestamp_seconds(now_utc)
        .ok_or_else(|| StorageAssuranceError::InvalidTimestamp(now_utc.to_string()))?;
    let selected = select_action(config, &roots, &candidates, now_seconds);
    let Some((action, candidate, destination)) = selected else {
        if let Some(blocked) = candidates.iter().find(|candidate| {
            matches!(
                candidate.disk_state.to_ascii_lowercase().as_str(),
                "draining" | "suspect"
            )
        }) {
            return Err(StorageAssuranceError::EvacuationBlocked {
                object_id: blocked.object_id.to_string(),
                disk_id: blocked.disk_id.clone(),
                reason: "no healthy destination has capacity and copy separation".to_string(),
            });
        }
        return Ok(StorageAssuranceReport {
            schema: "dasobjectstore.storage_assurance.report.v1",
            completed_at_utc: now_utc.to_string(),
            success: true,
            action: StorageAssuranceAction::Idle,
            object_id: None,
            source_disk_id: None,
            destination_disk_id: None,
            bytes: 0,
            source_removed: false,
            message: "no evacuation, balance, or stale-verification candidate".to_string(),
        });
    };
    let source_root = roots
        .iter()
        .find(|root| root.disk_id == candidate.disk_id)
        .ok_or_else(|| StorageAssuranceError::MissingDiskRoot(candidate.disk_id.clone()))?;
    let relative = safe_relative_path(&candidate.relative_path)?;
    let source_path = source_root.root_path.join(&relative);
    let expected_hash = normalize_sha256(&candidate.content_hash)?;
    let source_hash = hash_file_sha256_controlled(&source_path, || {
        assurance_should_preempt(config, &live_status_registry, now_utc)
    })?;
    if source_hash != expected_hash {
        record_assurance_hash_failure(
            &config.live_sqlite_path,
            &candidate.placement_id,
            &candidate.object_id,
            now_utc,
        )?;
        return Err(StorageAssuranceError::HashMismatch {
            object_id: candidate.object_id.to_string(),
            disk_id: candidate.disk_id.clone(),
            expected: expected_hash,
            actual: source_hash,
        });
    }
    if action == StorageAssuranceAction::Verify {
        record_assurance_verification(
            &config.live_sqlite_path,
            &candidate.placement_id,
            &candidate.object_id,
            now_utc,
        )?;
        return Ok(report_for(
            action,
            candidate,
            None,
            now_utc,
            false,
            "placement hash reverified",
        ));
    }

    let destination =
        destination.ok_or_else(|| StorageAssuranceError::MissingAssuranceDestination)?;
    let claim_kind = claim_kind(action)?;
    let mut operation = DurableAssuranceOperation::deterministic(
        action,
        candidate.clone(),
        destination.disk_id.clone(),
        now_utc,
    );
    operation::persist(&config.operation_journal_path, &operation)?;
    let claim_owner = operation.claim_owner.clone();
    let raw_capacity = measure_ssd_capacity(&destination.root_path)
        .map_err(|error| StorageAssuranceError::Discovery(error.to_string()))?;
    acquire_disk_capacity_claims(&DiskCapacityClaimRequest {
        live_sqlite_path: config.live_sqlite_path.clone(),
        kind: claim_kind,
        owner_id: claim_owner.clone(),
        request_id: claim_owner.clone(),
        request_digest: format!(
            "{}:{}:{}:{}",
            candidate.object_id.as_str(),
            destination.disk_id.as_str(),
            candidate.size_bytes,
            expected_hash
        ),
        lease_owner: Some("storage-assurance".to_string()),
        lease_expires_at_utc: None,
        created_at_utc: now_utc.to_string(),
        allocations: vec![DiskCapacityClaimAllocation {
            disk_id: destination.disk_id.clone(),
            measured_available_bytes: raw_capacity.available_bytes,
            requested_bytes: candidate.size_bytes,
        }],
    })?;
    operation.advance(OperationPhase::Claimed, now_utc);
    operation::persist(&config.operation_journal_path, &operation)?;
    let destination_path = destination.root_path.join(&relative);
    let request = HddCopyRequest::new(
        candidate.object_id.clone(),
        destination.disk_id.clone(),
        1,
        &source_path,
        &destination_path,
        &expected_hash,
    );
    let copy_result = write_verified_hdd_copy_with_controlled_progress(&request, |_| {
        if assurance_should_preempt(config, &live_status_registry, now_utc) {
            return Err(HddCopyError::Cancelled);
        }
        Ok(())
    });
    if let Err(error) = copy_result {
        release_disk_capacity_claims(&config.live_sqlite_path, claim_kind, &claim_owner, now_utc)?;
        operation::remove(&config.operation_journal_path)?;
        return Err(error.into());
    }
    operation.advance(OperationPhase::Copied, now_utc);
    operation::persist(&config.operation_journal_path, &operation)?;
    commit_assurance_relocation(
        &config.live_sqlite_path,
        candidate,
        &destination.disk_id,
        &candidate.relative_path,
        now_utc,
    )?;
    operation.advance(OperationPhase::Promoted, now_utc);
    operation::persist(&config.operation_journal_path, &operation)?;
    release_disk_capacity_claims(&config.live_sqlite_path, claim_kind, &claim_owner, now_utc)?;
    let source_removed = match fs::remove_file(&source_path) {
        Ok(()) => {
            if let Some(parent) = source_path.parent() {
                File::open(parent)?.sync_all()?;
            }
            true
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => true,
        Err(error) => {
            eprintln!(
                "storage assurance retained redundant source for {} after metadata promotion: {error}",
                candidate.object_id
            );
            false
        }
    };
    operation::remove(&config.operation_journal_path)?;
    let _ =
        complete_assurance_drain_if_empty(&config.live_sqlite_path, &candidate.disk_id, now_utc)?;
    Ok(report_for(
        action,
        candidate,
        Some(destination),
        now_utc,
        source_removed,
        if source_removed {
            "verified relocation committed and source removed"
        } else {
            "verified relocation committed; redundant source retained for garbage collection"
        },
    ))
}

fn recover_assurance_operation(
    config: &StorageAssuranceConfig,
    roots: &[MeasuredRoot],
    live_status_registry: Arc<LiveStatusRegistry>,
    now_utc: &str,
) -> Result<Option<StorageAssuranceReport>, StorageAssuranceError> {
    let Some(mut operation) = operation::read(&config.operation_journal_path)? else {
        return Ok(None);
    };
    let claim_kind = claim_kind(operation.action)?;
    let source_root = roots
        .iter()
        .find(|root| root.disk_id == operation.candidate.disk_id)
        .ok_or_else(|| {
            StorageAssuranceError::MissingDiskRoot(operation.candidate.disk_id.clone())
        })?;
    let destination_root = roots
        .iter()
        .find(|root| root.disk_id == operation.destination_disk_id)
        .ok_or_else(|| {
            StorageAssuranceError::MissingDiskRoot(operation.destination_disk_id.clone())
        })?;
    let relative = safe_relative_path(&operation.destination_relative_path)?;
    let source_path = source_root.root_path.join(&relative);
    let destination_path = destination_root.root_path.join(&relative);
    let expected_hash = normalize_sha256(&operation.candidate.content_hash)?;

    if operation.phase == OperationPhase::Planned {
        release_disk_capacity_claims(
            &config.live_sqlite_path,
            claim_kind,
            &operation.claim_owner,
            now_utc,
        )?;
        operation::remove(&config.operation_journal_path)?;
        return Ok(None);
    }
    if operation.phase == OperationPhase::Claimed {
        if !destination_path.is_file() {
            release_disk_capacity_claims(
                &config.live_sqlite_path,
                claim_kind,
                &operation.claim_owner,
                now_utc,
            )?;
            operation::remove(&config.operation_journal_path)?;
            return Ok(None);
        }
        let actual_hash = hash_file_sha256_controlled(&destination_path, || {
            assurance_should_preempt(config, &live_status_registry, now_utc)
        })?;
        if actual_hash != expected_hash {
            return Err(StorageAssuranceError::AmbiguousRecovery(
                operation.operation_id,
            ));
        }
        operation.advance(OperationPhase::Copied, now_utc);
        operation::persist(&config.operation_journal_path, &operation)?;
    }

    let committed = assurance_relocation_committed(
        &config.live_sqlite_path,
        &operation.candidate.object_id,
        &operation.candidate.placement_id,
        &operation.destination_disk_id,
        &operation.destination_relative_path,
        &operation.candidate.content_hash,
    )?;
    if operation.phase == OperationPhase::Copied && !committed {
        if !destination_path.is_file() {
            release_disk_capacity_claims(
                &config.live_sqlite_path,
                claim_kind,
                &operation.claim_owner,
                now_utc,
            )?;
            operation::remove(&config.operation_journal_path)?;
            return Ok(None);
        }
        let actual_hash = hash_file_sha256_controlled(&destination_path, || {
            assurance_should_preempt(config, &live_status_registry, now_utc)
        })?;
        if actual_hash != expected_hash {
            return Err(StorageAssuranceError::HashMismatch {
                object_id: operation.candidate.object_id.to_string(),
                disk_id: operation.destination_disk_id.clone(),
                expected: expected_hash,
                actual: actual_hash,
            });
        }
        commit_assurance_relocation(
            &config.live_sqlite_path,
            &operation.candidate,
            &operation.destination_disk_id,
            &operation.destination_relative_path,
            now_utc,
        )?;
        operation.advance(OperationPhase::Promoted, now_utc);
        operation::persist(&config.operation_journal_path, &operation)?;
    } else if !committed {
        return Err(StorageAssuranceError::AmbiguousRecovery(
            operation.operation_id,
        ));
    }

    release_disk_capacity_claims(
        &config.live_sqlite_path,
        claim_kind,
        &operation.claim_owner,
        now_utc,
    )?;
    let source_removed = remove_redundant_source(&source_path)?;
    operation::remove(&config.operation_journal_path)?;
    let _ = complete_assurance_drain_if_empty(
        &config.live_sqlite_path,
        &operation.candidate.disk_id,
        now_utc,
    )?;
    Ok(Some(report_for(
        operation.action,
        &operation.candidate,
        Some(destination_root),
        now_utc,
        source_removed,
        "restart recovered verified relocation without recopying",
    )))
}

fn claim_kind(
    action: StorageAssuranceAction,
) -> Result<DiskCapacityClaimKind, StorageAssuranceError> {
    match action {
        StorageAssuranceAction::Evacuate => Ok(DiskCapacityClaimKind::Evacuation),
        StorageAssuranceAction::Rebalance => Ok(DiskCapacityClaimKind::Repair),
        StorageAssuranceAction::Verify | StorageAssuranceAction::Idle => {
            Err(StorageAssuranceError::MissingAssuranceDestination)
        }
    }
}

fn remove_redundant_source(path: &Path) -> Result<bool, StorageAssuranceError> {
    match fs::remove_file(path) {
        Ok(()) => {
            if let Some(parent) = path.parent() {
                File::open(parent)?.sync_all()?;
            }
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(true),
        Err(_) => Ok(false),
    }
}

fn assurance_should_preempt(
    config: &StorageAssuranceConfig,
    live_status_registry: &LiveStatusRegistry,
    now_utc: &str,
) -> bool {
    let snapshot = live_status_registry.snapshot(now_utc.to_string());
    snapshot.aggregate.active_ingests > 0
        || snapshot
            .garbage_collection
            .is_some_and(|collection| collection.running)
        || assurance_primary_work_pending(&config.live_sqlite_path).unwrap_or(true)
}

fn hash_file_sha256_controlled(
    path: &Path,
    mut should_cancel: impl FnMut() -> bool,
) -> Result<String, StorageAssuranceError> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        if should_cancel() {
            return Err(StorageAssuranceError::Preempted);
        }
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn select_action<'a>(
    config: &StorageAssuranceConfig,
    roots: &'a [MeasuredRoot],
    candidates: &'a [AssurancePlacementCandidate],
    now_seconds: i64,
) -> Option<(
    StorageAssuranceAction,
    &'a AssurancePlacementCandidate,
    Option<&'a MeasuredRoot>,
)> {
    for candidate in candidates.iter().filter(|candidate| {
        matches!(
            candidate.disk_state.to_ascii_lowercase().as_str(),
            "draining" | "suspect"
        )
    }) {
        if let Some(destination) = best_destination(config, roots, candidate) {
            return Some((
                StorageAssuranceAction::Evacuate,
                candidate,
                Some(destination),
            ));
        }
    }
    let mut ordered_roots = roots
        .iter()
        .filter(|root| destination_state_allowed(&root.state))
        .collect::<Vec<_>>();
    ordered_roots.sort_by(|left, right| compare_free_fraction(left, right));
    for source in &ordered_roots {
        for destination in ordered_roots.iter().rev() {
            let gap = free_basis_points(destination).saturating_sub(free_basis_points(source));
            if source.disk_id == destination.disk_id
                || gap < u64::from(config.imbalance_basis_points)
            {
                continue;
            }
            if let Some(candidate) = candidates.iter().find(|candidate| {
                candidate.disk_id == source.disk_id
                    && candidate.size_bytes <= destination.available_bytes
                    && !candidate
                        .existing_disk_ids
                        .iter()
                        .any(|disk_id| disk_id == &destination.disk_id)
            }) {
                return Some((
                    StorageAssuranceAction::Rebalance,
                    candidate,
                    Some(destination),
                ));
            }
        }
    }
    candidates
        .iter()
        .find(|candidate| {
            candidate
                .verified_at_utc
                .as_deref()
                .and_then(parse_utc_timestamp_seconds)
                .is_none_or(|verified| {
                    now_seconds.saturating_sub(verified)
                        >= i64::try_from(config.verify_after_seconds).unwrap_or(i64::MAX)
                })
        })
        .map(|candidate| (StorageAssuranceAction::Verify, candidate, None))
}

fn best_destination<'a>(
    _config: &StorageAssuranceConfig,
    roots: &'a [MeasuredRoot],
    candidate: &AssurancePlacementCandidate,
) -> Option<&'a MeasuredRoot> {
    roots
        .iter()
        .filter(|root| {
            root.disk_id != candidate.disk_id
                && destination_state_allowed(&root.state)
                && root.available_bytes >= candidate.size_bytes
                && !candidate
                    .existing_disk_ids
                    .iter()
                    .any(|disk_id| disk_id == &root.disk_id)
        })
        .max_by(|left, right| compare_free_fraction(left, right))
}

fn measured_roots(
    config: &StorageAssuranceConfig,
) -> Result<Vec<MeasuredRoot>, StorageAssuranceError> {
    let outstanding = read_outstanding_disk_capacity(&config.live_sqlite_path)?;
    let states = list_assurance_disk_states(&config.live_sqlite_path)?
        .into_iter()
        .map(|disk| (disk.disk_id.to_string(), disk.state))
        .collect::<BTreeMap<_, _>>();
    discover_managed_hdd_roots(&config.hdd_root)
        .map_err(|error| StorageAssuranceError::Discovery(error.to_string()))?
        .into_iter()
        .filter_map(|root| {
            let state = states.get(root.disk_id.as_str())?.clone();
            let capacity = measure_ssd_capacity(&root.root_path).ok()?;
            let available_bytes = capacity
                .available_bytes
                .saturating_sub(outstanding.get(&root.disk_id).copied().unwrap_or(0));
            Some(MeasuredRoot {
                disk_id: root.disk_id,
                root_path: root.root_path,
                available_bytes,
                total_bytes: capacity.total_bytes,
                state,
            })
        })
        .collect::<Vec<_>>()
        .pipe(Ok)
}

fn compare_free_fraction(left: &MeasuredRoot, right: &MeasuredRoot) -> std::cmp::Ordering {
    (u128::from(left.available_bytes) * u128::from(right.total_bytes))
        .cmp(&(u128::from(right.available_bytes) * u128::from(left.total_bytes)))
        .then_with(|| right.disk_id.cmp(&left.disk_id))
}

fn free_basis_points(root: &MeasuredRoot) -> u64 {
    root.available_bytes
        .saturating_mul(10_000)
        .checked_div(root.total_bytes.max(1))
        .unwrap_or(0)
}

fn destination_state_allowed(state: &str) -> bool {
    matches!(state.to_ascii_lowercase().as_str(), "healthy" | "watch")
}

fn safe_relative_path(value: &str) -> Result<PathBuf, StorageAssuranceError> {
    let path = Path::new(value);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(StorageAssuranceError::UnsafeRelativePath(value.to_string()));
    }
    Ok(path.to_path_buf())
}

fn normalize_sha256(value: &str) -> Result<String, StorageAssuranceError> {
    let normalized = value.strip_prefix("sha256:").unwrap_or(value);
    if normalized.len() == 64 && normalized.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(normalized.to_ascii_lowercase())
    } else {
        Err(StorageAssuranceError::InvalidHash(value.to_string()))
    }
}

fn report_for(
    action: StorageAssuranceAction,
    candidate: &AssurancePlacementCandidate,
    destination: Option<&MeasuredRoot>,
    now_utc: &str,
    source_removed: bool,
    message: &str,
) -> StorageAssuranceReport {
    StorageAssuranceReport {
        schema: "dasobjectstore.storage_assurance.report.v1",
        completed_at_utc: now_utc.to_string(),
        success: true,
        action,
        object_id: Some(candidate.object_id.to_string()),
        source_disk_id: Some(candidate.disk_id.to_string()),
        destination_disk_id: destination.map(|root| root.disk_id.to_string()),
        bytes: candidate.size_bytes,
        source_removed,
        message: message.to_string(),
    }
}

fn persist_report(path: &Path, report: &StorageAssuranceReport) -> Result<(), io::Error> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "report has no parent"))?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temporary)?;
    serde_json::to_writer_pretty(&mut file, report)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    fs::rename(&temporary, path)?;
    File::open(parent)?.sync_all()
}

#[cfg(target_os = "linux")]
fn read_linux_disk_io_bytes() -> Result<u64, io::Error> {
    let content = fs::read_to_string("/proc/diskstats")?;
    let mut sectors = 0u64;
    for line in content.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() >= 10 {
            sectors = sectors
                .saturating_add(fields[5].parse::<u64>().unwrap_or(0))
                .saturating_add(fields[9].parse::<u64>().unwrap_or(0));
        }
    }
    Ok(sectors.saturating_mul(512))
}

#[cfg(not(target_os = "linux"))]
fn read_linux_disk_io_bytes() -> Result<u64, io::Error> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "host disk IO sampling requires Linux",
    ))
}

fn env_u64(name: &str, default: u64) -> Result<u64, StorageAssuranceError> {
    match std::env::var(name) {
        Ok(value) => value.parse().map_err(|_| {
            StorageAssuranceError::InvalidConfiguration(format!("{name} must be an integer"))
        }),
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(StorageAssuranceError::InvalidConfiguration(format!(
            "{name}: {error}"
        ))),
    }
}

fn env_bool(name: &str, default: bool) -> Result<bool, StorageAssuranceError> {
    match std::env::var(name) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Ok(true),
            "0" | "false" | "no" | "off" => Ok(false),
            _ => Err(StorageAssuranceError::InvalidConfiguration(format!(
                "{name} must be true or false"
            ))),
        },
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(StorageAssuranceError::InvalidConfiguration(format!(
            "{name}: {error}"
        ))),
    }
}

trait Pipe: Sized {
    fn pipe<T>(self, operation: impl FnOnce(Self) -> T) -> T {
        operation(self)
    }
}
impl<T> Pipe for T {}

#[derive(Debug)]
pub enum StorageAssuranceError {
    InvalidConfiguration(String),
    InvalidTimestamp(String),
    InvalidHash(String),
    UnsafeRelativePath(String),
    MissingDiskRoot(DiskId),
    MissingAssuranceDestination,
    Discovery(String),
    HashMismatch {
        object_id: String,
        disk_id: DiskId,
        expected: String,
        actual: String,
    },
    EvacuationBlocked {
        object_id: String,
        disk_id: DiskId,
        reason: String,
    },
    AmbiguousRecovery(String),
    Preempted,
    Metadata(AssuranceMetadataError),
    CapacityClaim(DiskCapacityClaimError),
    Copy(HddCopyError),
    Io(io::Error),
}

impl Display for StorageAssuranceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration(message) => write!(formatter, "invalid assurance configuration: {message}"),
            Self::InvalidTimestamp(value) => write!(formatter, "invalid assurance timestamp {value}"),
            Self::InvalidHash(value) => write!(formatter, "invalid assurance SHA-256 {value}"),
            Self::UnsafeRelativePath(value) => write!(formatter, "unsafe assurance placement path {value}"),
            Self::MissingDiskRoot(disk_id) => write!(formatter, "managed assurance root missing for {disk_id}"),
            Self::MissingAssuranceDestination => formatter.write_str("assurance action requires a destination"),
            Self::Discovery(error) => write!(formatter, "assurance disk discovery failed: {error}"),
            Self::HashMismatch { object_id, disk_id, expected, actual } => write!(formatter, "assurance hash mismatch for {object_id} on {disk_id}: expected {expected}, got {actual}"),
            Self::EvacuationBlocked { object_id, disk_id, reason } => write!(formatter, "assurance evacuation blocked for {object_id} on {disk_id}: {reason}"),
            Self::AmbiguousRecovery(operation_id) => write!(formatter, "assurance operation {operation_id} has ambiguous restart evidence; source retained"),
            Self::Preempted => formatter.write_str("storage assurance preempted by primary work"),
            Self::Metadata(error) => error.fmt(formatter),
            Self::CapacityClaim(error) => error.fmt(formatter),
            Self::Copy(error) => error.fmt(formatter),
            Self::Io(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for StorageAssuranceError {}

impl From<AssuranceMetadataError> for StorageAssuranceError {
    fn from(error: AssuranceMetadataError) -> Self {
        Self::Metadata(error)
    }
}
impl From<DiskCapacityClaimError> for StorageAssuranceError {
    fn from(error: DiskCapacityClaimError) -> Self {
        Self::CapacityClaim(error)
    }
}
impl From<HddCopyError> for StorageAssuranceError {
    fn from(error: HddCopyError) -> Self {
        Self::Copy(error)
    }
}
impl From<io::Error> for StorageAssuranceError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dasobjectstore_metadata::{hash_file_sha256, object_commit::placement_id, LIVE_SCHEMA_SQL};
    use rusqlite::{params, Connection};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn idle_gate_requires_continuous_quiescence_and_resets_on_io() {
        let start = Instant::now();
        let mut gate = IdleGate::new(Duration::from_secs(60), 100);
        let idle = IdleObservation {
            primary_work_pending: false,
            live_ingests: false,
            garbage_collection_running: false,
            io_bytes_per_second: 0,
        };
        assert!(!gate.observe(start, idle));
        assert!(!gate.observe(start + Duration::from_secs(59), idle));
        assert!(gate.observe(start + Duration::from_secs(60), idle));
        assert!(!gate.observe(
            start + Duration::from_secs(61),
            IdleObservation {
                io_bytes_per_second: 101,
                ..idle
            }
        ));
        assert!(!gate.observe(start + Duration::from_secs(120), idle));
    }

    #[test]
    fn fractional_free_space_not_lexical_or_absolute_bytes_controls_order() {
        let small = root("disk-z", 30, 100);
        let large = root("disk-a", 250, 1000);
        assert_eq!(
            compare_free_fraction(&small, &large),
            std::cmp::Ordering::Greater
        );
        assert_eq!(free_basis_points(&small), 3_000);
        assert_eq!(free_basis_points(&large), 2_500);
    }

    #[test]
    fn draining_disk_evacuation_precedes_balance_and_scrub() {
        let config = test_config();
        let roots = vec![root("disk-a", 10, 100), root("disk-b", 80, 100)];
        let mut candidate = candidate("disk-a");
        candidate.disk_state = "Draining".to_string();
        let candidates = vec![candidate];
        let selected = select_action(&config, &roots, &candidates, i64::MAX).expect("action");
        assert_eq!(selected.0, StorageAssuranceAction::Evacuate);
        assert_eq!(selected.2.expect("destination").disk_id.as_str(), "disk-b");
    }

    #[test]
    fn balance_moves_from_most_used_to_fractionally_freest_disk() {
        let config = test_config();
        let roots = vec![
            root("disk-a", 10, 100),
            root("disk-b", 50, 100),
            root("disk-c", 300, 1000),
        ];
        let candidates = vec![candidate("disk-a")];
        let selected = select_action(&config, &roots, &candidates, 0).expect("action");
        assert_eq!(selected.0, StorageAssuranceAction::Rebalance);
        assert_eq!(selected.2.expect("destination").disk_id.as_str(), "disk-b");
    }

    #[test]
    fn balance_tries_second_freest_destination_when_first_is_ineligible() {
        let config = test_config();
        let roots = vec![
            root("disk-a", 10, 100),
            root("disk-b", 90, 100),
            root("disk-c", 80, 100),
        ];
        let mut object = candidate("disk-a");
        object
            .existing_disk_ids
            .push("disk-b".parse().expect("disk"));
        let candidates = vec![object];
        let selected = select_action(&config, &roots, &candidates, 0).expect("action");
        assert_eq!(selected.0, StorageAssuranceAction::Rebalance);
        assert_eq!(selected.2.expect("destination").disk_id.as_str(), "disk-c");
    }

    #[test]
    fn large_placement_is_not_silently_excluded_from_evacuation_or_scrub() {
        let mut config = test_config();
        config.max_object_bytes = 1;
        let roots = vec![root("disk-a", 10, 100), root("disk-b", 90, 100)];
        let mut object = candidate("disk-a");
        object.disk_state = "Draining".to_string();
        object.size_bytes = 5;
        let candidates = [object];
        let selected = select_action(&config, &roots, &candidates, i64::MAX).expect("action");
        assert_eq!(selected.0, StorageAssuranceAction::Evacuate);
    }

    #[test]
    fn controlled_hash_stops_before_reading_more_work_after_preemption() {
        let root = temp_root("hash-preemption");
        fs::create_dir_all(&root).expect("root");
        let payload = root.join("payload");
        fs::write(&payload, vec![7_u8; 2 * 1024 * 1024]).expect("payload");
        let mut checks = 0;
        let error = hash_file_sha256_controlled(&payload, || {
            checks += 1;
            checks > 1
        })
        .expect_err("preempted");
        assert!(matches!(error, StorageAssuranceError::Preempted));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn idle_assurance_relocates_and_verifies_before_removing_draining_source() {
        let root = temp_root("verified-relocation");
        let hdd_root = root.join("hdd");
        let state_root = root.join("state");
        let live_sqlite_path = root.join("live.sqlite");
        let relative = PathBuf::from("objects/aa/object-a/payload");
        for disk in ["disk-a", "disk-b"] {
            let disk_root = hdd_root.join(disk);
            fs::create_dir_all(disk_root.join(".dasobjectstore")).expect("marker root");
            fs::write(
                disk_root.join(".dasobjectstore/device.env"),
                format!("role=hdd:{disk}\n"),
            )
            .expect("marker");
        }
        let source = hdd_root.join("disk-a").join(&relative);
        fs::create_dir_all(source.parent().expect("source parent")).expect("source dir");
        fs::write(&source, b"assurance payload").expect("source");
        let hash = hash_file_sha256(&source).expect("hash");
        let connection = Connection::open(&live_sqlite_path).expect("open");
        connection.execute_batch(LIVE_SCHEMA_SQL).expect("schema");
        connection
            .execute(
                "INSERT INTO pools(pool_id,state,created_at_utc,updated_at_utc)
             VALUES('pool-a','Clean','now','now')",
                [],
            )
            .expect("pool");
        for (disk, state) in [("disk-a", "Draining"), ("disk-b", "Watch")] {
            connection
                .execute(
                    "INSERT INTO disks(
                    disk_id,pool_id,role,state,created_at_utc,updated_at_utc
                 ) VALUES(?1,'pool-a','hdd_capacity',?2,'now','now')",
                    params![disk, state],
                )
                .expect("disk");
        }
        connection
            .execute(
                "INSERT INTO stores(
                store_id,pool_id,class,policy_json,created_at_utc,updated_at_utc
             ) VALUES('store-a','pool-a','generated_data','{}','now','now')",
                [],
            )
            .expect("store");
        connection
            .execute(
                "INSERT INTO objects(
                object_id,store_id,object_type,state,size_bytes,content_hash,
                created_at_utc,updated_at_utc
             ) VALUES('object-a','store-a','naive','HddCopyVerified',?1,?2,'now','now')",
                params![b"assurance payload".len() as u64, hash],
            )
            .expect("object");
        connection
            .execute(
                "INSERT INTO placements VALUES(
                ?1,'object-a','disk-a',?2,?3,
                '2026-01-01T00:00:00Z','2026-01-01T00:00:00Z'
             )",
                params![
                    placement_id("object-a", "disk-a", relative.to_str().expect("relative")),
                    relative.to_str().expect("relative"),
                    hash,
                ],
            )
            .expect("placement");
        drop(connection);
        let config = StorageAssuranceConfig {
            live_sqlite_path: live_sqlite_path.clone(),
            hdd_root: hdd_root.clone(),
            latest_report_path: state_root.join("latest.json"),
            operation_journal_path: state_root.join("operation.json"),
            ..test_config()
        };

        let report = run_one_storage_assurance(
            &config,
            Arc::new(LiveStatusRegistry::default()),
            "2026-07-24T00:00:00Z",
        )
        .expect("assurance");

        assert_eq!(report.action, StorageAssuranceAction::Evacuate);
        assert!(report.source_removed);
        assert!(!source.exists());
        assert_eq!(
            fs::read(hdd_root.join("disk-b").join(&relative)).expect("destination"),
            b"assurance payload"
        );
        let disk: String = Connection::open(&live_sqlite_path)
            .expect("open")
            .query_row(
                "SELECT disk_id FROM placements WHERE object_id='object-a'",
                [],
                |row| row.get(0),
            )
            .expect("placement");
        assert_eq!(disk, "disk-b");
        let source_state: String = Connection::open(&live_sqlite_path)
            .expect("open")
            .query_row(
                "SELECT state FROM disks WHERE disk_id='disk-a'",
                [],
                |row| row.get(0),
            )
            .expect("source state");
        assert_eq!(source_state, "Retired");
        assert!(!config.operation_journal_path.exists());
        fs::remove_dir_all(root).expect("cleanup");
    }

    fn root(id: &str, available: u64, total: u64) -> MeasuredRoot {
        MeasuredRoot {
            disk_id: DiskId::new(id).expect("disk id"),
            root_path: PathBuf::from(format!("/{id}")),
            available_bytes: available,
            total_bytes: total,
            state: "Watch".to_string(),
        }
    }

    fn candidate(disk_id: &str) -> AssurancePlacementCandidate {
        AssurancePlacementCandidate {
            placement_id: "placement-a".parse().expect("placement"),
            object_id: "object-a".parse().expect("object"),
            store_id: "store-a".parse().expect("store"),
            disk_id: disk_id.parse().expect("disk"),
            disk_state: "Watch".to_string(),
            relative_path: "objects/aa/object-a/payload".to_string(),
            size_bytes: 5,
            content_hash: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .to_string(),
            verified_at_utc: Some("2026-01-01T00:00:00Z".to_string()),
            existing_disk_ids: vec![disk_id.parse().expect("disk")],
        }
    }

    fn test_config() -> StorageAssuranceConfig {
        StorageAssuranceConfig {
            enabled: true,
            poll_seconds: 30,
            idle_grace_seconds: 60,
            verify_after_seconds: 60,
            imbalance_basis_points: 500,
            max_object_bytes: 1024,
            idle_io_bytes_per_second: 1024,
            live_sqlite_path: PathBuf::from("/live.sqlite"),
            hdd_root: PathBuf::from("/hdd"),
            latest_report_path: PathBuf::from("/state/latest.json"),
            operation_journal_path: PathBuf::from("/state/operation.json"),
        }
    }

    fn temp_root(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "dasobjectstore-storage-assurance-{label}-{}-{unique}",
            std::process::id()
        ))
    }
}
