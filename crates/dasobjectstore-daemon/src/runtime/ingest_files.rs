use crate::api::{
    DaemonIngestConflictAction, DaemonIngestConflictPolicy, DaemonIngestHddActiveTransfer,
    DaemonIngestHddTransferPhase, DaemonIngestObjectSnapshot, DaemonIngestPipelineStage,
    DaemonIngestProgressEvent, DaemonIngestQueueDepths, DaemonIngestStage, DaemonIngestTelemetry,
    DaemonIngestWorkerActivity, DaemonIngestWorkerTelemetry, DaemonIngressLandingMode,
    DaemonIngressOrigin, DaemonSsdPressure, SubmitIngestFilesRequest, SubmitIngestFilesResponse,
};
use dasobjectstore_core::ids::{DiskId, IngestJobId, ObjectId, StoreId};
use dasobjectstore_core::object_type::ObjectType;
use dasobjectstore_core::store::{IngestMode, StorePolicy};
use dasobjectstore_metadata::{
    commit_object_put, existing_object_payload_candidate_paths, measure_ssd_capacity,
    put_object_direct_to_hdd_with_controlled_progress, read_object_inspect,
    settle_staged_object_to_hdd_with_controlled_progress, DirectObjectPutRequest, DiskCopyRoot,
    IngestJobPaths, IngestStagingLayout, IngestWriteReport, ObjectInspectError, ObjectPutError,
    ObjectPutProgress, ObjectPutProgressStage, ObjectPutRequest, SsdCapacityPolicy, SsdPressure,
    StagedObjectPut,
};
use dasobjectstore_object_service::{
    default_store_registry_path, default_subobject_registry_path, ObjectServiceError,
};
use std::collections::BTreeMap;
use std::fmt::{self, Display};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::Instant;

const HDD_SETTLEMENT_QUEUE_CAPACITY: usize = 4;
const SSD_FLUSH_QUEUE_CAPACITY: usize = 2;
const HDD_WRITE_RATE_STALE_AFTER: std::time::Duration = std::time::Duration::from_secs(1);

mod endpoint;
mod environment;
mod progress;
mod scheduling;
mod source_classification;

use endpoint::{collect_ingest_files, resolve_ingest_endpoint, FileIngestEntry};
#[cfg(test)]
use environment::SSD_ROOT_ENV;
pub(crate) use environment::{default_hdd_root, default_ssd_root, discover_managed_hdd_roots};
use environment::{default_live_sqlite_path, validate_known_ssd_root};
#[cfg(test)]
use scheduling::{default_hdd_worker_count, HddSettlementDiskState, HddSettlementScheduler};
use scheduling::{
    new_shared_hdd_settlement_scheduler, release_hdd_settlement_roots,
    reserve_hdd_settlement_roots, resolve_hdd_worker_count, SharedHddSettlementScheduler,
};
use source_classification::{
    source_is_server_local, source_topology_details, verified_ingress_origin_with_source_verifier,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DaemonFileIngestSummary {
    pub endpoint_name: String,
    pub endpoint_kind: &'static str,
    pub store_id: StoreId,
    pub object_prefix: String,
    pub files: usize,
    pub source_bytes: u64,
    pub copies: u8,
    pub object_type: ObjectType,
    pub dry_run: bool,
}

pub fn submit_ingest_files_to_local_store(
    request: SubmitIngestFilesRequest,
    accepted_at_utc: &str,
) -> Result<SubmitIngestFilesResponse, DaemonIngestFilesRuntimeError> {
    submit_ingest_files_to_local_store_with_progress(request, accepted_at_utc, |_| Ok(()))
}

pub fn submit_ingest_files_to_local_store_with_progress(
    request: SubmitIngestFilesRequest,
    accepted_at_utc: &str,
    progress: impl FnMut(DaemonIngestProgressEvent) -> Result<(), DaemonIngestFilesRuntimeError>,
) -> Result<SubmitIngestFilesResponse, DaemonIngestFilesRuntimeError> {
    let executor = LocalFileIngestExecutor::from_environment();
    let mut progress = progress::IngestProgressCoalescer::new(progress);
    let response = executor.submit(request, accepted_at_utc, |event| progress.publish(event))?;
    progress.flush()?;
    Ok(response)
}

#[derive(Clone, Debug)]
struct LocalFileIngestExecutor {
    ssd_root: PathBuf,
    hdd_root: PathBuf,
    live_sqlite_path: PathBuf,
    store_registry_path: PathBuf,
    subobject_registry_path: PathBuf,
    source_is_server_local: fn(&Path) -> bool,
    capacity_policy: SsdCapacityPolicy,
}

impl LocalFileIngestExecutor {
    fn from_environment() -> Self {
        Self {
            ssd_root: default_ssd_root(),
            hdd_root: default_hdd_root(),
            live_sqlite_path: default_live_sqlite_path(),
            store_registry_path: default_store_registry_path(),
            subobject_registry_path: default_subobject_registry_path(),
            source_is_server_local,
            capacity_policy: SsdCapacityPolicy::default(),
        }
    }

    fn submit(
        &self,
        request: SubmitIngestFilesRequest,
        accepted_at_utc: &str,
        progress: impl FnMut(DaemonIngestProgressEvent) -> Result<(), DaemonIngestFilesRuntimeError>,
    ) -> Result<SubmitIngestFilesResponse, DaemonIngestFilesRuntimeError> {
        let job_id = ingest_job_id(accepted_at_utc)?;
        let summary = self.execute(request, &job_id, accepted_at_utc, progress)?;
        Ok(SubmitIngestFilesResponse {
            job_id,
            accepted_at_utc: accepted_at_utc.to_string(),
            dry_run: summary.dry_run,
        })
    }

    fn execute(
        &self,
        request: SubmitIngestFilesRequest,
        job_id: &IngestJobId,
        accepted_at_utc: &str,
        mut progress: impl FnMut(DaemonIngestProgressEvent) -> Result<(), DaemonIngestFilesRuntimeError>,
    ) -> Result<DaemonFileIngestSummary, DaemonIngestFilesRuntimeError> {
        validate_known_ssd_root(&self.ssd_root)?;
        let endpoint = resolve_ingest_endpoint(
            &request.endpoint,
            &self.store_registry_path,
            &self.subobject_registry_path,
        )?;
        let managed_disk_roots = discover_managed_hdd_roots(&self.hdd_root)?;
        let copies = request.copies.unwrap_or(endpoint.store.policy.copies);
        if copies == 0 || managed_disk_roots.len() < copies as usize {
            return Err(DaemonIngestFilesRuntimeError::CommandFailed(format!(
                "ingest files requires at least {copies} managed HDD root(s), got {}",
                managed_disk_roots.len()
            )));
        }
        ensure_live_metadata_for_ingest(
            &self.live_sqlite_path,
            &endpoint.store,
            &managed_disk_roots,
            accepted_at_utc,
        )?;
        let hdd_worker_count =
            resolve_hdd_worker_count(request.hdd_workers, managed_disk_roots.len(), copies)?;
        let files = collect_ingest_files(&request.source_path, &endpoint.object_prefix)?;
        let source_bytes = files.iter().map(|entry| entry.size_bytes).sum::<u64>();
        let total_work_bytes = source_bytes.saturating_mul(u64::from(copies) + 1);
        let ingress_origin = verified_ingress_origin_with_source_verifier(
            request.ingress_origin,
            &request.source_path,
            self.source_is_server_local,
        );
        let landing_mode = landing_mode_for_ingest(&endpoint.store.policy, ingress_origin);
        let summary = DaemonFileIngestSummary {
            endpoint_name: endpoint.endpoint_name.clone(),
            endpoint_kind: endpoint.endpoint_kind,
            store_id: endpoint.store.store_id.clone(),
            object_prefix: endpoint.object_prefix.clone(),
            files: files.len(),
            source_bytes,
            copies,
            object_type: request.object_type,
            dry_run: request.dry_run,
        };

        progress(DaemonIngestProgressEvent {
            job_id: job_id.clone(),
            endpoint: request.endpoint.clone(),
            stage: DaemonIngestStage::Queued,
            pipeline_stage: Some(DaemonIngestPipelineStage::Scan),
            work_bytes_done: 0,
            work_bytes_total: Some(total_work_bytes),
            source_bytes_done: Some(0),
            source_bytes_total: Some(source_bytes),
            stage_bytes_done: Some(0),
            stage_bytes_total: Some(source_bytes),
            files_done: 0,
            files_total: Some(files.len() as u64),
            current_object_id: None,
            ssd_pressure: None,
            telemetry: None,
            active_hdd_transfers: Vec::new(),
            resource_policy: None,
            message: Some(format!(
                "preflight: source={} source topology={} {} origin={} store_ingest_mode={:?} landing mode {} reason={}; planned {} file(s), {} source byte(s), {} copy/copies, {} HDD settlement worker(s)",
                request.source_path.display(),
                if matches!(
                    ingress_origin,
                    DaemonIngressOrigin::LocalServer
                        | DaemonIngressOrigin::LocalServerDirectImport
                        | DaemonIngressOrigin::LocalServerSsdFirst
                ) {
                    "verified-server-local"
                } else {
                    "external-or-unverified"
                },
                source_topology_details(&request.source_path),
                ingress_origin,
                endpoint.store.policy.ingest_mode,
                landing_mode,
                if landing_mode == DaemonIngressLandingMode::SsdFirst {
                    "SSD staging selected by verified source classification or store policy"
                } else {
                    "direct HDD selected by explicit local route and DirectToHdd store policy"
                },
                files.len(),
                source_bytes,
                copies,
                hdd_worker_count
            )),
        })?;

        if request.dry_run {
            progress(DaemonIngestProgressEvent {
                job_id: job_id.clone(),
                endpoint: request.endpoint.clone(),
                stage: DaemonIngestStage::Complete,
                pipeline_stage: Some(DaemonIngestPipelineStage::Finalization),
                work_bytes_done: 0,
                work_bytes_total: Some(total_work_bytes),
                source_bytes_done: Some(0),
                source_bytes_total: Some(source_bytes),
                stage_bytes_done: Some(0),
                stage_bytes_total: Some(0),
                files_done: files.len() as u64,
                files_total: Some(files.len() as u64),
                current_object_id: None,
                ssd_pressure: None,
                telemetry: None,
                active_hdd_transfers: Vec::new(),
                resource_policy: None,
                message: Some("dry run complete; no files imported".to_string()),
            })?;
            return Ok(summary);
        }

        if landing_mode == DaemonIngressLandingMode::DirectToHddWhenPolicyAllows {
            return self.execute_direct_to_hdd(
                request,
                job_id,
                summary,
                files,
                managed_disk_roots,
                copies,
                hdd_worker_count,
                source_bytes,
                total_work_bytes,
                accepted_at_utc,
                progress,
            );
        }

        let mut state = PipelineProgressState::new(
            files.len() as u64,
            source_bytes,
            total_work_bytes,
            hdd_worker_count as u16,
            false,
        );
        let capacity_policy = self.capacity_policy.clone();
        let queue_capacity = HDD_SETTLEMENT_QUEUE_CAPACITY.max(hdd_worker_count.saturating_mul(2));
        let (settle_tx, settle_rx) = mpsc::sync_channel::<HddSettlementWork>(queue_capacity);
        let (flush_tx, flush_rx) = mpsc::sync_channel::<SsdFlushWork>(SSD_FLUSH_QUEUE_CAPACITY);
        let (event_tx, event_rx) = mpsc::channel::<HddSettlementEvent>();
        let hdd_scheduler = new_shared_hdd_settlement_scheduler(&managed_disk_roots)?;
        let hdd_workers = spawn_hdd_settlement_workers(
            settle_rx,
            event_tx.clone(),
            hdd_worker_count,
            hdd_scheduler,
        );
        let ssd_flush_worker = spawn_ssd_flush_worker(flush_rx, settle_tx.clone(), event_tx);

        for entry in &files {
            if maybe_skip_existing_object(
                &self.live_sqlite_path,
                &request,
                entry,
                &managed_disk_roots,
                copies,
                &mut state,
                job_id,
                &mut progress,
            )? {
                continue;
            }
            wait_for_ssd_admission(
                &self.ssd_root,
                &capacity_policy,
                &event_rx,
                &mut state,
                job_id,
                &request.endpoint,
                &mut progress,
                &self.live_sqlite_path,
                accepted_at_utc,
            )?;
            let put_request = ObjectPutRequest::new(
                entry.object_id.clone(),
                &entry.source_path,
                &self.ssd_root,
                managed_disk_roots.clone(),
                copies,
            )
            .with_object_type(request.object_type);

            state.ssd_active = state.ssd_active.saturating_add(1);
            let layout = IngestStagingLayout::for_ssd_root(&self.ssd_root);
            layout.create_base_directories()?;
            let put_job_id = IngestJobId::new(format!("put-{}", entry.object_id.as_str()))
                .map_err(|err| DaemonIngestFilesRuntimeError::CommandFailed(err.to_string()))?;
            let job_paths = layout.job_paths(&put_job_id);
            let mut source = fs::File::open(&entry.source_path)?;
            let write_report = match job_paths.write_payload_with_hash_unsynced_controlled_progress(
                &mut source,
                |bytes_written| {
                    let object_progress = ObjectPutProgress {
                        object_id: entry.object_id.clone(),
                        stage: ObjectPutProgressStage::SsdIngest,
                        bytes_written,
                    };
                    drain_hdd_settlement_events(
                        &event_rx,
                        &mut state,
                        job_id,
                        &request.endpoint,
                        &mut progress,
                        false,
                        &self.live_sqlite_path,
                        accepted_at_utc,
                    )
                    .map_err(|err| io::Error::other(err.to_string()))?;
                    state.apply_object_progress(entry, &object_progress);
                    progress(object_progress_event(
                        job_id,
                        &request.endpoint,
                        entry,
                        &state,
                        &object_progress,
                    ))
                    .map_err(|err| io::Error::other(err.to_string()))
                },
            ) {
                Ok(write_report) => write_report,
                Err(err) => {
                    let _ = fs::remove_dir_all(&job_paths.job_root);
                    return Err(err.into());
                }
            };
            state.ssd_active = state.ssd_active.saturating_sub(1);
            enqueue_ssd_flush_work(
                &flush_tx,
                SsdFlushWork {
                    entry: entry.clone(),
                    pending: PendingSsdStage {
                        request: put_request,
                        job_paths,
                        write_report,
                    },
                },
                &event_rx,
                &mut state,
                job_id,
                &request.endpoint,
                &mut progress,
                &self.live_sqlite_path,
                accepted_at_utc,
            )?;
            drain_hdd_settlement_events(
                &event_rx,
                &mut state,
                job_id,
                &request.endpoint,
                &mut progress,
                false,
                &self.live_sqlite_path,
                accepted_at_utc,
            )?;
        }
        drop(flush_tx);
        while !ssd_flush_worker.is_finished() {
            drain_hdd_settlement_events(
                &event_rx,
                &mut state,
                job_id,
                &request.endpoint,
                &mut progress,
                true,
                &self.live_sqlite_path,
                accepted_at_utc,
            )?;
        }
        ssd_flush_worker.join().map_err(|_| {
            DaemonIngestFilesRuntimeError::CommandFailed("SSD flush worker panicked".to_string())
        })?;
        drop(settle_tx);

        while state.completed_files < files.len() as u64 {
            drain_hdd_settlement_events(
                &event_rx,
                &mut state,
                job_id,
                &request.endpoint,
                &mut progress,
                true,
                &self.live_sqlite_path,
                accepted_at_utc,
            )?;
        }
        for hdd_worker in hdd_workers {
            hdd_worker.join().map_err(|_| {
                DaemonIngestFilesRuntimeError::CommandFailed(
                    "HDD settlement worker panicked".to_string(),
                )
            })?;
        }

        progress(DaemonIngestProgressEvent {
            job_id: job_id.clone(),
            endpoint: request.endpoint.clone(),
            stage: DaemonIngestStage::Complete,
            pipeline_stage: Some(DaemonIngestPipelineStage::Finalization),
            work_bytes_done: total_work_bytes,
            work_bytes_total: Some(total_work_bytes),
            source_bytes_done: Some(source_bytes),
            source_bytes_total: Some(source_bytes),
            stage_bytes_done: Some(0),
            stage_bytes_total: Some(0),
            files_done: files.len() as u64,
            files_total: Some(files.len() as u64),
            current_object_id: None,
            ssd_pressure: Some(state.ssd_pressure),
            telemetry: Some(state.telemetry()),
            active_hdd_transfers: Vec::new(),
            resource_policy: None,
            message: Some("file ingest complete".to_string()),
        })?;

        Ok(summary)
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_direct_to_hdd(
        &self,
        request: SubmitIngestFilesRequest,
        job_id: &IngestJobId,
        summary: DaemonFileIngestSummary,
        files: Vec<FileIngestEntry>,
        managed_disk_roots: Vec<DiskCopyRoot>,
        copies: u8,
        hdd_worker_count: usize,
        source_bytes: u64,
        total_work_bytes: u64,
        accepted_at_utc: &str,
        mut progress: impl FnMut(DaemonIngestProgressEvent) -> Result<(), DaemonIngestFilesRuntimeError>,
    ) -> Result<DaemonFileIngestSummary, DaemonIngestFilesRuntimeError> {
        let mut state = PipelineProgressState::new(
            files.len() as u64,
            source_bytes,
            total_work_bytes,
            hdd_worker_count as u16,
            true,
        );
        let queue_capacity = HDD_SETTLEMENT_QUEUE_CAPACITY.max(hdd_worker_count.saturating_mul(2));
        let (settle_tx, settle_rx) = mpsc::sync_channel::<HddSettlementWork>(queue_capacity);
        let (event_tx, event_rx) = mpsc::channel::<HddSettlementEvent>();
        let hdd_scheduler = new_shared_hdd_settlement_scheduler(&managed_disk_roots)?;
        let hdd_workers =
            spawn_hdd_settlement_workers(settle_rx, event_tx, hdd_worker_count, hdd_scheduler);

        for entry in &files {
            if maybe_skip_existing_object(
                &self.live_sqlite_path,
                &request,
                entry,
                &managed_disk_roots,
                copies,
                &mut state,
                job_id,
                &mut progress,
            )? {
                drain_hdd_settlement_events(
                    &event_rx,
                    &mut state,
                    job_id,
                    &request.endpoint,
                    &mut progress,
                    false,
                    &self.live_sqlite_path,
                    accepted_at_utc,
                )?;
                continue;
            }
            state.staged_files = state.staged_files.saturating_add(1);
            state.hdd_queued = state.hdd_queued.saturating_add(1);
            enqueue_hdd_settlement_work(
                &settle_tx,
                HddSettlementWork {
                    entry: entry.clone(),
                    payload: HddSettlementPayload::Direct(
                        DirectObjectPutRequest::new(
                            entry.object_id.clone(),
                            entry.source_path.clone(),
                            managed_disk_roots.clone(),
                            copies,
                        )
                        .with_object_type(request.object_type),
                    ),
                },
                &event_rx,
                &mut state,
                job_id,
                &request.endpoint,
                &mut progress,
                &self.live_sqlite_path,
                accepted_at_utc,
            )?;
            progress(DaemonIngestProgressEvent {
                job_id: job_id.clone(),
                endpoint: request.endpoint.clone(),
                stage: DaemonIngestStage::Queued,
                pipeline_stage: Some(DaemonIngestPipelineStage::HddWrite),
                work_bytes_done: state.completed_work_bytes,
                work_bytes_total: Some(state.work_bytes_total),
                source_bytes_done: Some(state.completed_source_bytes),
                source_bytes_total: Some(state.source_bytes_total),
                stage_bytes_done: Some(0),
                stage_bytes_total: Some(entry.size_bytes),
                files_done: state.completed_files,
                files_total: Some(state.total_files),
                current_object_id: Some(entry.object_id.clone()),
                ssd_pressure: Some(state.ssd_pressure),
                telemetry: Some(state.telemetry()),
                active_hdd_transfers: state.active_hdd_transfer_records(),
                resource_policy: None,
                message: Some(format!(
                    "queued direct HDD copy with inline checksum: {}",
                    entry.relative_path.to_string_lossy()
                )),
            })?;
            drain_hdd_settlement_events(
                &event_rx,
                &mut state,
                job_id,
                &request.endpoint,
                &mut progress,
                false,
                &self.live_sqlite_path,
                accepted_at_utc,
            )?;
        }
        drop(settle_tx);

        while state.completed_files < files.len() as u64 {
            drain_hdd_settlement_events(
                &event_rx,
                &mut state,
                job_id,
                &request.endpoint,
                &mut progress,
                true,
                &self.live_sqlite_path,
                accepted_at_utc,
            )?;
        }
        for hdd_worker in hdd_workers {
            hdd_worker.join().map_err(|_| {
                DaemonIngestFilesRuntimeError::CommandFailed(
                    "HDD settlement worker panicked".to_string(),
                )
            })?;
        }

        progress(DaemonIngestProgressEvent {
            job_id: job_id.clone(),
            endpoint: request.endpoint,
            stage: DaemonIngestStage::Complete,
            pipeline_stage: Some(DaemonIngestPipelineStage::Finalization),
            work_bytes_done: total_work_bytes,
            work_bytes_total: Some(total_work_bytes),
            source_bytes_done: Some(source_bytes),
            source_bytes_total: Some(source_bytes),
            stage_bytes_done: Some(0),
            stage_bytes_total: Some(0),
            files_done: files.len() as u64,
            files_total: Some(files.len() as u64),
            current_object_id: None,
            ssd_pressure: Some(state.ssd_pressure),
            telemetry: Some(state.telemetry()),
            active_hdd_transfers: Vec::new(),
            resource_policy: None,
            message: Some("direct-to-HDD local file ingest complete".to_string()),
        })?;

        Ok(summary)
    }
}

#[derive(Debug)]
struct PendingSsdStage {
    request: ObjectPutRequest,
    job_paths: IngestJobPaths,
    write_report: IngestWriteReport,
}

#[derive(Debug)]
struct SsdFlushWork {
    entry: FileIngestEntry,
    pending: PendingSsdStage,
}

#[derive(Debug)]
struct HddSettlementWork {
    entry: FileIngestEntry,
    payload: HddSettlementPayload,
}

#[derive(Debug)]
enum HddSettlementPayload {
    Staged(StagedObjectPut),
    Direct(DirectObjectPutRequest),
}

impl HddSettlementPayload {
    fn copy_count(&self) -> u8 {
        match self {
            Self::Staged(staged) => staged.copy_count,
            Self::Direct(request) => request.copy_count,
        }
    }

    fn set_disk_roots(&mut self, roots: Vec<DiskCopyRoot>) {
        match self {
            Self::Staged(staged) => staged.disk_roots = roots,
            Self::Direct(request) => request.disk_roots = roots,
        }
    }
}

#[derive(Debug)]
enum HddSettlementEvent {
    SsdFlushStarted {
        entry: FileIngestEntry,
    },
    SsdFlushProgress {
        entry: FileIngestEntry,
        progress: ObjectPutProgress,
    },
    SsdFlushed {
        entry: FileIngestEntry,
    },
    Started {
        entry: FileIngestEntry,
        roots: Vec<DiskCopyRoot>,
    },
    Progress {
        entry: FileIngestEntry,
        progress: ObjectPutProgress,
    },
    Settled {
        entry: FileIngestEntry,
        report: dasobjectstore_metadata::ObjectPutReport,
    },
    Failed {
        error: ObjectPutError,
    },
}

#[derive(Debug)]
struct PipelineProgressState {
    total_files: u64,
    source_bytes_total: u64,
    work_bytes_total: u64,
    completed_files: u64,
    staged_files: u64,
    completed_source_bytes: u64,
    completed_work_bytes: u64,
    ssd_active: u16,
    hdd_active: u16,
    hdd_worker_count: u16,
    hdd_queued: u32,
    ssd_pressure: DaemonSsdPressure,
    count_hdd_copy_as_source: bool,
    progress_offsets: BTreeMap<(ObjectId, String), u64>,
    active_hdd_transfers: BTreeMap<(ObjectId, DiskId, u8), ActiveHddTransferState>,
    source_read_rate: SampledRate,
    ssd_write_rate: SampledRate,
}

#[derive(Debug, Default)]
struct SampledRate {
    last_at: Option<Instant>,
    last_bytes: u64,
    current_bytes_per_second: u64,
}

impl SampledRate {
    fn update(&mut self, bytes: u64) {
        let now = Instant::now();
        if let Some(last_at) = self.last_at {
            let elapsed = now.duration_since(last_at).as_secs_f64();
            if elapsed > 0.0 {
                self.current_bytes_per_second =
                    ((bytes.saturating_sub(self.last_bytes) as f64) / elapsed) as u64;
            }
        }
        self.last_at = Some(now);
        self.last_bytes = bytes;
    }

    fn current(&self) -> u64 {
        self.last_at
            .filter(|at| at.elapsed() <= HDD_WRITE_RATE_STALE_AFTER)
            .map(|_| self.current_bytes_per_second)
            .unwrap_or(0)
    }
}

#[derive(Debug)]
struct ActiveHddTransferState {
    file_index: u64,
    files_total: u64,
    object_id: ObjectId,
    relative_path: String,
    disk_id: DiskId,
    copy_number: u8,
    bytes_done: u64,
    bytes_total: u64,
    last_write_sample_at: Instant,
    last_write_sample_bytes: u64,
    current_bytes_per_second: u64,
    phase: DaemonIngestHddTransferPhase,
    fsync_duration_millis: Option<u64>,
    rename_duration_millis: Option<u64>,
}

impl PipelineProgressState {
    fn new(
        total_files: u64,
        source_bytes_total: u64,
        work_bytes_total: u64,
        hdd_worker_count: u16,
        count_hdd_copy_as_source: bool,
    ) -> Self {
        Self {
            total_files,
            source_bytes_total,
            work_bytes_total,
            completed_files: 0,
            staged_files: 0,
            completed_source_bytes: 0,
            completed_work_bytes: 0,
            ssd_active: 0,
            hdd_active: 0,
            hdd_worker_count: hdd_worker_count.max(1),
            hdd_queued: 0,
            ssd_pressure: DaemonSsdPressure::AcceptingWrites,
            count_hdd_copy_as_source,
            progress_offsets: BTreeMap::new(),
            active_hdd_transfers: BTreeMap::new(),
            source_read_rate: SampledRate::default(),
            ssd_write_rate: SampledRate::default(),
        }
    }

    fn apply_object_progress(&mut self, entry: &FileIngestEntry, progress: &ObjectPutProgress) {
        match &progress.stage {
            ObjectPutProgressStage::SsdIngest | ObjectPutProgressStage::SsdFlush => {
                let key = (
                    progress.object_id.clone(),
                    object_progress_stage_key(progress),
                );
                let previous = *self.progress_offsets.get(&key).unwrap_or(&0);
                let current = progress.bytes_written;
                self.progress_offsets.insert(key, current);
                self.completed_work_bytes = self
                    .completed_work_bytes
                    .saturating_add(current.saturating_sub(previous));
                if matches!(progress.stage, ObjectPutProgressStage::SsdIngest) {
                    self.source_read_rate.update(current);
                    self.ssd_write_rate.update(current);
                    self.completed_source_bytes = self.completed_source_bytes.saturating_add(
                        current
                            .min(entry.size_bytes)
                            .saturating_sub(previous.min(entry.size_bytes)),
                    );
                }
                if matches!(progress.stage, ObjectPutProgressStage::SsdFlush) {
                    self.ssd_write_rate.update(current);
                }
            }
            ObjectPutProgressStage::HddCopy {
                disk_id,
                copy_number,
            } => {
                let key = (
                    progress.object_id.clone(),
                    object_progress_stage_key(progress),
                );
                let previous = *self.progress_offsets.get(&key).unwrap_or(&0);
                let current = progress.bytes_written;
                self.progress_offsets.insert(key, current);
                let delta = current.saturating_sub(previous);
                self.completed_work_bytes = self.completed_work_bytes.saturating_add(delta);
                if self.count_hdd_copy_as_source && *copy_number == 1 {
                    let source_delta = current
                        .min(entry.size_bytes)
                        .saturating_sub(previous.min(entry.size_bytes));
                    self.completed_source_bytes =
                        self.completed_source_bytes.saturating_add(source_delta);
                    self.completed_work_bytes =
                        self.completed_work_bytes.saturating_add(source_delta);
                }
                self.update_hdd_transfer(
                    entry,
                    disk_id,
                    *copy_number,
                    current,
                    DaemonIngestHddTransferPhase::Writing,
                    None,
                    None,
                );
            }
            ObjectPutProgressStage::HddFsync {
                disk_id,
                copy_number,
                duration_millis,
            } => self.update_hdd_transfer(
                entry,
                disk_id,
                *copy_number,
                progress.bytes_written,
                DaemonIngestHddTransferPhase::Fsync,
                *duration_millis,
                None,
            ),
            ObjectPutProgressStage::HddRename {
                disk_id,
                copy_number,
                duration_millis,
            } => self.update_hdd_transfer(
                entry,
                disk_id,
                *copy_number,
                progress.bytes_written,
                DaemonIngestHddTransferPhase::Rename,
                None,
                *duration_millis,
            ),
        }
    }

    fn update_hdd_transfer(
        &mut self,
        entry: &FileIngestEntry,
        disk_id: &str,
        copy_number: u8,
        bytes_done: u64,
        phase: DaemonIngestHddTransferPhase,
        fsync_duration_millis: Option<u64>,
        rename_duration_millis: Option<u64>,
    ) {
        let Ok(disk_id) = DiskId::new(disk_id.to_string()) else {
            return;
        };
        let key = (entry.object_id.clone(), disk_id.clone(), copy_number);
        let transfer =
            self.active_hdd_transfers
                .entry(key)
                .or_insert_with(|| ActiveHddTransferState {
                    file_index: entry.file_index,
                    files_total: self.total_files,
                    object_id: entry.object_id.clone(),
                    relative_path: entry.relative_path.to_string_lossy().to_string(),
                    disk_id,
                    copy_number,
                    bytes_done: 0,
                    bytes_total: entry.size_bytes,
                    last_write_sample_at: Instant::now(),
                    last_write_sample_bytes: 0,
                    current_bytes_per_second: 0,
                    phase: DaemonIngestHddTransferPhase::Writing,
                    fsync_duration_millis: None,
                    rename_duration_millis: None,
                });
        transfer.bytes_done = bytes_done.min(entry.size_bytes);
        transfer.phase = phase;
        if phase == DaemonIngestHddTransferPhase::Writing {
            let now = Instant::now();
            let elapsed = now
                .duration_since(transfer.last_write_sample_at)
                .as_secs_f64()
                .max(0.001);
            let delta = transfer
                .bytes_done
                .saturating_sub(transfer.last_write_sample_bytes);
            transfer.current_bytes_per_second = (delta as f64 / elapsed).round() as u64;
            transfer.last_write_sample_at = now;
            transfer.last_write_sample_bytes = transfer.bytes_done;
        } else {
            transfer.current_bytes_per_second = 0;
        }
        if fsync_duration_millis.is_some() {
            transfer.fsync_duration_millis = fsync_duration_millis;
        }
        if rename_duration_millis.is_some() {
            transfer.rename_duration_millis = rename_duration_millis;
        }
    }

    fn remove_active_hdd_transfers_for_entry(&mut self, entry: &FileIngestEntry) {
        self.active_hdd_transfers
            .retain(|(object_id, _, _), _| object_id != &entry.object_id);
    }

    fn mark_existing_object_skipped(&mut self, entry: &FileIngestEntry, copies: u8) {
        self.completed_files = self.completed_files.saturating_add(1);
        self.staged_files = self.staged_files.saturating_add(1);
        self.completed_source_bytes = self.completed_source_bytes.saturating_add(entry.size_bytes);
        self.completed_work_bytes = self
            .completed_work_bytes
            .saturating_add(entry.size_bytes.saturating_mul(u64::from(copies) + 1));
        self.remove_active_hdd_transfers_for_entry(entry);
    }

    fn active_hdd_transfer_records(&self) -> Vec<DaemonIngestHddActiveTransfer> {
        let now = Instant::now();
        self.active_hdd_transfers
            .values()
            .map(|transfer| DaemonIngestHddActiveTransfer {
                file_index: transfer.file_index,
                files_total: Some(transfer.files_total),
                object_id: transfer.object_id.clone(),
                relative_path: transfer.relative_path.clone(),
                disk_id: transfer.disk_id.clone(),
                copy_number: transfer.copy_number,
                bytes_done: transfer.bytes_done,
                bytes_total: transfer.bytes_total,
                bytes_per_second: if transfer.phase == DaemonIngestHddTransferPhase::Writing
                    && now.duration_since(transfer.last_write_sample_at)
                        <= HDD_WRITE_RATE_STALE_AFTER
                {
                    transfer.current_bytes_per_second
                } else {
                    0
                },
                phase: transfer.phase,
                fsync_duration_millis: transfer.fsync_duration_millis,
                rename_duration_millis: transfer.rename_duration_millis,
            })
            .collect()
    }

    fn source_pending(&self) -> u32 {
        self.total_files
            .saturating_sub(self.staged_files)
            .saturating_sub(u64::from(self.ssd_active))
            .min(u64::from(u32::MAX)) as u32
    }

    fn telemetry(&self) -> DaemonIngestTelemetry {
        let mut telemetry = DaemonIngestTelemetry::default();
        let writing_active = self
            .active_hdd_transfers
            .values()
            .filter(|transfer| transfer.phase == DaemonIngestHddTransferPhase::Writing)
            .count() as u16;
        let finalization_active = self
            .active_hdd_transfers
            .values()
            .filter(|transfer| transfer.phase != DaemonIngestHddTransferPhase::Writing)
            .count() as u16;
        let source_active =
            if self.ssd_active > 0 || (self.count_hdd_copy_as_source && writing_active > 0) {
                1
            } else {
                0
            };
        telemetry.queue_depths = DaemonIngestQueueDepths {
            source_read: self.source_pending(),
            hdd_write: self.hdd_queued,
            ..DaemonIngestQueueDepths::default()
        };
        telemetry.workers = DaemonIngestWorkerTelemetry {
            source_read: DaemonIngestWorkerActivity {
                active: source_active,
                idle: u16::from(source_active == 0),
            },
            ssd_stage: DaemonIngestWorkerActivity {
                active: self.ssd_active,
                idle: u16::from(self.ssd_active == 0),
            },
            hdd_write: DaemonIngestWorkerActivity {
                active: writing_active,
                idle: self.hdd_worker_count.saturating_sub(writing_active),
            },
            finalization: DaemonIngestWorkerActivity {
                active: finalization_active,
                idle: u16::from(finalization_active == 0),
            },
            ..DaemonIngestWorkerTelemetry::default()
        };
        let now = Instant::now();
        let aggregate_hdd_write_bytes_per_second = self
            .active_hdd_transfers
            .values()
            .filter(|transfer| {
                transfer.phase == DaemonIngestHddTransferPhase::Writing
                    && now.duration_since(transfer.last_write_sample_at)
                        <= HDD_WRITE_RATE_STALE_AFTER
            })
            .map(|transfer| transfer.current_bytes_per_second)
            .sum();
        telemetry.throughput.source_read_bytes_per_second = self.source_read_rate.current();
        telemetry.throughput.ssd_write_bytes_per_second = self.ssd_write_rate.current();
        telemetry.throughput.aggregate_hdd_write_bytes_per_second =
            aggregate_hdd_write_bytes_per_second;
        telemetry.throughput.current_bytes_per_second = self
            .source_read_rate
            .current()
            .max(self.ssd_write_rate.current())
            .max(aggregate_hdd_write_bytes_per_second);
        telemetry.pressure.ssd = self.ssd_pressure;
        telemetry
    }
}

fn spawn_ssd_flush_worker(
    flush_rx: mpsc::Receiver<SsdFlushWork>,
    settle_tx: mpsc::SyncSender<HddSettlementWork>,
    event_tx: mpsc::Sender<HddSettlementEvent>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        while let Ok(work) = flush_rx.recv() {
            let _ = event_tx.send(HddSettlementEvent::SsdFlushStarted {
                entry: work.entry.clone(),
            });
            let entry = work.entry.clone();
            let result = sync_pending_ssd_stage(work.pending, |progress| {
                event_tx
                    .send(HddSettlementEvent::SsdFlushProgress {
                        entry: entry.clone(),
                        progress,
                    })
                    .map_err(|_| ObjectPutError::Cancelled)
            });
            match result {
                Ok(staged) => {
                    if settle_tx
                        .send(HddSettlementWork {
                            entry: work.entry.clone(),
                            payload: HddSettlementPayload::Staged(staged),
                        })
                        .is_err()
                    {
                        let _ = event_tx.send(HddSettlementEvent::Failed {
                            error: ObjectPutError::Cancelled,
                        });
                        break;
                    }
                    let _ = event_tx.send(HddSettlementEvent::SsdFlushed { entry: work.entry });
                }
                Err(error) => {
                    let _ = event_tx.send(HddSettlementEvent::Failed { error });
                    break;
                }
            }
        }
    })
}

fn sync_pending_ssd_stage(
    pending: PendingSsdStage,
    mut progress: impl FnMut(ObjectPutProgress) -> Result<(), ObjectPutError>,
) -> Result<StagedObjectPut, ObjectPutError> {
    let request = pending.request;
    pending
        .job_paths
        .sync_payload_with_progress(|bytes_written| {
            progress(ObjectPutProgress {
                object_id: request.object_id.clone(),
                stage: ObjectPutProgressStage::SsdFlush,
                bytes_written,
            })
            .map_err(|err| match err {
                ObjectPutError::Io(err) => err,
                ObjectPutError::Cancelled => {
                    io::Error::new(io::ErrorKind::Interrupted, "object put cancelled")
                }
                other => io::Error::other(other.to_string()),
            })
        })
        .map_err(ObjectPutError::from)?;
    Ok(StagedObjectPut {
        object_id: request.object_id.clone(),
        object_type: request.object_type,
        source_path: request.source_path.clone(),
        job_root: pending.job_paths.job_root.clone(),
        staged_payload_path: pending.job_paths.payload_path.clone(),
        bytes_staged: pending.write_report.bytes_written,
        content_hash_algorithm: pending.write_report.content_hash_algorithm,
        content_hash: pending.write_report.content_hash,
        disk_roots: request.disk_roots,
        copy_count: request.copy_count,
    })
}

fn spawn_hdd_settlement_workers(
    settle_rx: mpsc::Receiver<HddSettlementWork>,
    event_tx: mpsc::Sender<HddSettlementEvent>,
    worker_count: usize,
    scheduler: SharedHddSettlementScheduler,
) -> Vec<thread::JoinHandle<()>> {
    let settle_rx = Arc::new(Mutex::new(settle_rx));
    (0..worker_count.max(1))
        .map(|_| {
            let settle_rx = Arc::clone(&settle_rx);
            let event_tx = event_tx.clone();
            let scheduler = Arc::clone(&scheduler);
            thread::spawn(move || loop {
                let work = {
                    let receiver = match settle_rx.lock() {
                        Ok(receiver) => receiver,
                        Err(_) => break,
                    };
                    receiver.recv()
                };
                let Ok(work) = work else {
                    break;
                };
                let roots = match reserve_hdd_settlement_roots(
                    &scheduler,
                    work.payload.copy_count() as usize,
                    work.entry.size_bytes,
                ) {
                    Ok(roots) => roots,
                    Err(error) => {
                        let _ = event_tx.send(HddSettlementEvent::Failed {
                            error: ObjectPutError::Io(io::Error::other(error.to_string())),
                        });
                        break;
                    }
                };
                let _ = event_tx.send(HddSettlementEvent::Started {
                    entry: work.entry.clone(),
                    roots: roots.clone(),
                });
                let entry = work.entry.clone();
                let mut payload = work.payload;
                payload.set_disk_roots(roots.clone());
                let result = settle_hdd_payload_with_controlled_progress(payload, |progress| {
                    event_tx
                        .send(HddSettlementEvent::Progress {
                            entry: entry.clone(),
                            progress,
                        })
                        .map_err(|_| ObjectPutError::Cancelled)
                });
                if let Err(error) =
                    release_hdd_settlement_roots(&scheduler, &roots, work.entry.size_bytes)
                {
                    let _ = event_tx.send(HddSettlementEvent::Failed {
                        error: ObjectPutError::Io(io::Error::other(error.to_string())),
                    });
                    break;
                }
                match result {
                    Ok(report) => {
                        let _ = event_tx.send(HddSettlementEvent::Settled {
                            entry: work.entry,
                            report,
                        });
                    }
                    Err(error) => {
                        let _ = event_tx.send(HddSettlementEvent::Failed { error });
                        break;
                    }
                }
            })
        })
        .collect()
}

fn settle_hdd_payload_with_controlled_progress(
    payload: HddSettlementPayload,
    progress: impl FnMut(ObjectPutProgress) -> Result<(), ObjectPutError>,
) -> Result<dasobjectstore_metadata::ObjectPutReport, ObjectPutError> {
    match payload {
        HddSettlementPayload::Staged(staged) => {
            settle_staged_object_to_hdd_with_controlled_progress(staged, progress)
                .map_err(Into::into)
        }
        HddSettlementPayload::Direct(request) => {
            put_object_direct_to_hdd_with_controlled_progress(request, progress).map_err(Into::into)
        }
    }
}

fn enqueue_ssd_flush_work(
    flush_tx: &mpsc::SyncSender<SsdFlushWork>,
    mut work: SsdFlushWork,
    event_rx: &mpsc::Receiver<HddSettlementEvent>,
    state: &mut PipelineProgressState,
    job_id: &IngestJobId,
    endpoint: &StoreId,
    progress: &mut impl FnMut(DaemonIngestProgressEvent) -> Result<(), DaemonIngestFilesRuntimeError>,
    live_sqlite_path: &Path,
    recorded_at_utc: &str,
) -> Result<(), DaemonIngestFilesRuntimeError> {
    loop {
        match flush_tx.try_send(work) {
            Ok(()) => return Ok(()),
            Err(mpsc::TrySendError::Full(returned_work)) => {
                work = returned_work;
                drain_hdd_settlement_events(
                    event_rx,
                    state,
                    job_id,
                    endpoint,
                    progress,
                    true,
                    live_sqlite_path,
                    recorded_at_utc,
                )?;
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                return Err(DaemonIngestFilesRuntimeError::CommandFailed(
                    "SSD flush worker stopped before accepting staged object".to_string(),
                ));
            }
        }
    }
}

fn enqueue_hdd_settlement_work(
    settle_tx: &mpsc::SyncSender<HddSettlementWork>,
    mut work: HddSettlementWork,
    event_rx: &mpsc::Receiver<HddSettlementEvent>,
    state: &mut PipelineProgressState,
    job_id: &IngestJobId,
    endpoint: &StoreId,
    progress: &mut impl FnMut(DaemonIngestProgressEvent) -> Result<(), DaemonIngestFilesRuntimeError>,
    live_sqlite_path: &Path,
    recorded_at_utc: &str,
) -> Result<(), DaemonIngestFilesRuntimeError> {
    loop {
        match settle_tx.try_send(work) {
            Ok(()) => return Ok(()),
            Err(mpsc::TrySendError::Full(returned_work)) => {
                work = returned_work;
                drain_hdd_settlement_events(
                    event_rx,
                    state,
                    job_id,
                    endpoint,
                    progress,
                    true,
                    live_sqlite_path,
                    recorded_at_utc,
                )?;
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                return Err(DaemonIngestFilesRuntimeError::CommandFailed(
                    "HDD settlement worker stopped before accepting direct object".to_string(),
                ));
            }
        }
    }
}

fn wait_for_ssd_admission(
    ssd_root: &Path,
    capacity_policy: &SsdCapacityPolicy,
    event_rx: &mpsc::Receiver<HddSettlementEvent>,
    state: &mut PipelineProgressState,
    job_id: &IngestJobId,
    endpoint: &StoreId,
    progress: &mut impl FnMut(DaemonIngestProgressEvent) -> Result<(), DaemonIngestFilesRuntimeError>,
    live_sqlite_path: &Path,
    recorded_at_utc: &str,
) -> Result<(), DaemonIngestFilesRuntimeError> {
    loop {
        state.ssd_pressure = read_daemon_ssd_pressure(ssd_root, capacity_policy)?;
        match state.ssd_pressure {
            DaemonSsdPressure::AcceptingWrites => return Ok(()),
            DaemonSsdPressure::High if state.hdd_active == 0 && state.hdd_queued == 0 => {
                return Ok(());
            }
            DaemonSsdPressure::Critical if state.hdd_active == 0 && state.hdd_queued == 0 => {
                return Err(DaemonIngestFilesRuntimeError::CommandFailed(
                    "SSD pressure is critical and no staged HDD settlement work is available to drain"
                        .to_string(),
                ));
            }
            DaemonSsdPressure::High | DaemonSsdPressure::Critical => {
                progress(DaemonIngestProgressEvent {
                    job_id: job_id.clone(),
                    endpoint: endpoint.clone(),
                    stage: DaemonIngestStage::Queued,
                    pipeline_stage: Some(DaemonIngestPipelineStage::SourceRead),
                    work_bytes_done: state.completed_work_bytes,
                    work_bytes_total: Some(state.work_bytes_total),
                    source_bytes_done: Some(state.completed_source_bytes),
                    source_bytes_total: Some(state.source_bytes_total),
                    stage_bytes_done: Some(0),
                    stage_bytes_total: Some(0),
                    files_done: state.completed_files,
                    files_total: Some(state.total_files),
                    current_object_id: None,
                    ssd_pressure: Some(state.ssd_pressure),
                    telemetry: Some(state.telemetry()),
                    active_hdd_transfers: state.active_hdd_transfer_records(),
                    resource_policy: None,
                    message: Some(format!(
                        "SSD pressure {:?}; pausing source ingress while HDD settlement drains",
                        state.ssd_pressure
                    )),
                })?;
                drain_hdd_settlement_events(
                    event_rx,
                    state,
                    job_id,
                    endpoint,
                    progress,
                    true,
                    live_sqlite_path,
                    recorded_at_utc,
                )?;
            }
        }
    }
}

fn read_daemon_ssd_pressure(
    ssd_root: &Path,
    capacity_policy: &SsdCapacityPolicy,
) -> Result<DaemonSsdPressure, DaemonIngestFilesRuntimeError> {
    let capacity = measure_ssd_capacity(ssd_root)?;
    let pressure = capacity_policy
        .evaluate(&capacity)
        .map_err(|err| DaemonIngestFilesRuntimeError::CommandFailed(err.to_string()))?;
    Ok(match pressure {
        SsdPressure::AcceptingWrites => DaemonSsdPressure::AcceptingWrites,
        SsdPressure::HighWatermark => DaemonSsdPressure::High,
        SsdPressure::Critical => DaemonSsdPressure::Critical,
    })
}

fn ensure_live_metadata_for_ingest(
    live_sqlite_path: &Path,
    store: &dasobjectstore_object_service::StoreServiceDefinition,
    disk_roots: &[DiskCopyRoot],
    recorded_at_utc: &str,
) -> Result<(), DaemonIngestFilesRuntimeError> {
    let mut connection = rusqlite::Connection::open(live_sqlite_path)?;
    connection.execute_batch(dasobjectstore_metadata::LIVE_SCHEMA_SQL)?;
    let transaction = connection.transaction()?;
    let pool_id: String = transaction
        .query_row(
            "SELECT pool_id FROM pools ORDER BY pool_id LIMIT 1",
            [],
            |row| row.get(0),
        )
        .or_else(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => {
                transaction.execute(
                    "INSERT INTO pools (pool_id, state, created_at_utc, updated_at_utc)
                     VALUES ('pool-runtime', 'Clean', ?1, ?1)",
                    [recorded_at_utc],
                )?;
                Ok("pool-runtime".to_string())
            }
            other => Err(other),
        })?;
    for disk_root in disk_roots {
        transaction.execute(
            "INSERT OR IGNORE INTO disks (
                disk_id, pool_id, role, state, created_at_utc, updated_at_utc
             ) VALUES (?1, ?2, 'hdd', 'Healthy', ?3, ?3)",
            rusqlite::params![disk_root.disk_id.as_str(), pool_id, recorded_at_utc],
        )?;
    }
    let policy_json = serde_json::to_string(&store.policy).map_err(|error| {
        DaemonIngestFilesRuntimeError::CommandFailed(format!(
            "failed to serialize ObjectStore policy for metadata: {error}"
        ))
    })?;
    transaction.execute(
        "INSERT OR IGNORE INTO stores (
            store_id, pool_id, class, policy_json, created_at_utc, updated_at_utc
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        rusqlite::params![
            store.store_id.as_str(),
            pool_id,
            store.policy.class.name(),
            policy_json,
            recorded_at_utc,
        ],
    )?;
    transaction.commit()?;
    Ok(())
}

fn landing_mode_for_ingest(
    policy: &StorePolicy,
    origin: DaemonIngressOrigin,
) -> DaemonIngressLandingMode {
    match (origin.landing_mode(), policy.ingest_mode) {
        (DaemonIngressLandingMode::DirectToHddWhenPolicyAllows, IngestMode::DirectToHdd) => {
            DaemonIngressLandingMode::DirectToHddWhenPolicyAllows
        }
        _ => DaemonIngressLandingMode::SsdFirst,
    }
}

fn maybe_skip_existing_object(
    live_sqlite_path: &Path,
    request: &SubmitIngestFilesRequest,
    entry: &FileIngestEntry,
    managed_disk_roots: &[DiskCopyRoot],
    copies: u8,
    state: &mut PipelineProgressState,
    job_id: &IngestJobId,
    progress: &mut impl FnMut(DaemonIngestProgressEvent) -> Result<(), DaemonIngestFilesRuntimeError>,
) -> Result<bool, DaemonIngestFilesRuntimeError> {
    if let Some(existing) = read_existing_object_snapshot(live_sqlite_path, &entry.object_id)? {
        // Strict conflict decisions are deliberately deferred until the copy has
        // produced its checksum in flight. Reading the source into a hash sink
        // here doubles NVMe reads and blocks direct HDD landing.
        let incoming = DaemonIngestObjectSnapshot::new(entry.size_bytes, None::<String>);
        let decision = request
            .conflict_policy
            .decide_existing_object(&existing, &incoming);
        if decision.action == DaemonIngestConflictAction::SkipExistingVersion {
            emit_existing_object_skip(
                request,
                entry,
                copies,
                state,
                job_id,
                progress,
                format!("metadata {:?}", decision.reason),
            )?;
            return Ok(true);
        }
    }

    if request.conflict_policy == DaemonIngestConflictPolicy::Force {
        return Ok(false);
    }
    let payload_candidates =
        existing_payload_candidates_for_object(managed_disk_roots, &entry.object_id)?;
    if payload_candidates.is_empty() {
        return Ok(false);
    }

    match request.conflict_policy {
        DaemonIngestConflictPolicy::Lazy => {
            if payload_candidates.iter().any(|path| {
                path.metadata()
                    .map(|metadata| metadata.len() == entry.size_bytes)
                    .unwrap_or(false)
            }) {
                emit_existing_object_skip(
                    request,
                    entry,
                    copies,
                    state,
                    job_id,
                    progress,
                    "payload size match".to_string(),
                )?;
                return Ok(true);
            }
        }
        DaemonIngestConflictPolicy::Strict => {
            // The content-addressed destination is checked after the in-flight
            // copy has computed its checksum. Never hash the source separately.
        }
        DaemonIngestConflictPolicy::Force => {}
    }

    Ok(false)
}

fn emit_existing_object_skip(
    request: &SubmitIngestFilesRequest,
    entry: &FileIngestEntry,
    copies: u8,
    state: &mut PipelineProgressState,
    job_id: &IngestJobId,
    progress: &mut impl FnMut(DaemonIngestProgressEvent) -> Result<(), DaemonIngestFilesRuntimeError>,
    reason: String,
) -> Result<(), DaemonIngestFilesRuntimeError> {
    state.mark_existing_object_skipped(entry, copies);
    progress(DaemonIngestProgressEvent {
        job_id: job_id.clone(),
        endpoint: request.endpoint.clone(),
        stage: DaemonIngestStage::Complete,
        pipeline_stage: Some(DaemonIngestPipelineStage::Finalization),
        work_bytes_done: state.completed_work_bytes,
        work_bytes_total: Some(state.work_bytes_total),
        source_bytes_done: Some(state.completed_source_bytes),
        source_bytes_total: Some(state.source_bytes_total),
        stage_bytes_done: Some(entry.size_bytes),
        stage_bytes_total: Some(entry.size_bytes),
        files_done: state.completed_files,
        files_total: Some(state.total_files),
        current_object_id: Some(entry.object_id.clone()),
        ssd_pressure: Some(state.ssd_pressure),
        telemetry: Some(state.telemetry()),
        active_hdd_transfers: state.active_hdd_transfer_records(),
        resource_policy: None,
        message: Some(format!(
            "skipped existing object by {} policy ({}): {}",
            request.conflict_policy,
            reason,
            entry.relative_path.to_string_lossy()
        )),
    })
}

fn read_existing_object_snapshot(
    live_sqlite_path: &Path,
    object_id: &ObjectId,
) -> Result<Option<DaemonIngestObjectSnapshot>, DaemonIngestFilesRuntimeError> {
    if !live_sqlite_path.exists() {
        return Ok(None);
    }
    match read_object_inspect(live_sqlite_path, object_id) {
        Ok(summary) => Ok(Some(DaemonIngestObjectSnapshot::new(
            summary.size_bytes.unwrap_or(0),
            summary.content_hash,
        ))),
        Err(ObjectInspectError::ObjectNotFound(_)) => Ok(None),
        Err(ObjectInspectError::Sqlite(err))
            if err.to_string().contains("no such table: objects") =>
        {
            Ok(None)
        }
        Err(err) => Err(DaemonIngestFilesRuntimeError::CommandFailed(format!(
            "failed to inspect existing object metadata for conflict handling: {err}"
        ))),
    }
}

fn existing_payload_candidates_for_object(
    managed_disk_roots: &[DiskCopyRoot],
    object_id: &ObjectId,
) -> Result<Vec<PathBuf>, DaemonIngestFilesRuntimeError> {
    let mut candidates = Vec::new();
    for root in managed_disk_roots {
        let mut root_candidates = existing_object_payload_candidate_paths(root, object_id)?;
        candidates.append(&mut root_candidates);
    }
    Ok(candidates)
}

fn drain_hdd_settlement_events(
    event_rx: &mpsc::Receiver<HddSettlementEvent>,
    state: &mut PipelineProgressState,
    job_id: &IngestJobId,
    endpoint: &StoreId,
    progress: &mut impl FnMut(DaemonIngestProgressEvent) -> Result<(), DaemonIngestFilesRuntimeError>,
    block: bool,
    live_sqlite_path: &Path,
    recorded_at_utc: &str,
) -> Result<(), DaemonIngestFilesRuntimeError> {
    loop {
        let event = if block {
            event_rx.recv().map_err(|_| {
                DaemonIngestFilesRuntimeError::CommandFailed(
                    "HDD settlement worker stopped before completing all staged objects"
                        .to_string(),
                )
            })?
        } else {
            match event_rx.try_recv() {
                Ok(event) => event,
                Err(mpsc::TryRecvError::Empty) => return Ok(()),
                Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
            }
        };

        match event {
            HddSettlementEvent::SsdFlushStarted { entry } => {
                state.ssd_active = state.ssd_active.saturating_add(1);
                progress(DaemonIngestProgressEvent {
                    job_id: job_id.clone(),
                    endpoint: endpoint.clone(),
                    stage: DaemonIngestStage::SsdIngest,
                    pipeline_stage: Some(DaemonIngestPipelineStage::SsdFlush),
                    work_bytes_done: state.completed_work_bytes,
                    work_bytes_total: Some(state.work_bytes_total),
                    source_bytes_done: Some(state.completed_source_bytes),
                    source_bytes_total: Some(state.source_bytes_total),
                    stage_bytes_done: Some(entry.size_bytes),
                    stage_bytes_total: Some(entry.size_bytes),
                    files_done: state.completed_files,
                    files_total: Some(state.total_files),
                    current_object_id: Some(entry.object_id.clone()),
                    ssd_pressure: Some(state.ssd_pressure),
                    telemetry: Some(state.telemetry()),
                    active_hdd_transfers: state.active_hdd_transfer_records(),
                    resource_policy: None,
                    message: Some(format!(
                        "syncing staged SSD payload: {}",
                        entry.relative_path.to_string_lossy()
                    )),
                })?;
            }
            HddSettlementEvent::SsdFlushProgress {
                entry,
                progress: object_progress,
            } => {
                state.apply_object_progress(&entry, &object_progress);
                progress(object_progress_event(
                    job_id,
                    endpoint,
                    &entry,
                    state,
                    &object_progress,
                ))?;
            }
            HddSettlementEvent::SsdFlushed { entry } => {
                state.ssd_active = state.ssd_active.saturating_sub(1);
                state.staged_files = state.staged_files.saturating_add(1);
                state.hdd_queued = state.hdd_queued.saturating_add(1);
                progress(DaemonIngestProgressEvent {
                    job_id: job_id.clone(),
                    endpoint: endpoint.clone(),
                    stage: DaemonIngestStage::SsdIngest,
                    pipeline_stage: Some(DaemonIngestPipelineStage::SsdFlush),
                    work_bytes_done: state.completed_work_bytes,
                    work_bytes_total: Some(state.work_bytes_total),
                    source_bytes_done: Some(state.completed_source_bytes),
                    source_bytes_total: Some(state.source_bytes_total),
                    stage_bytes_done: Some(entry.size_bytes),
                    stage_bytes_total: Some(entry.size_bytes),
                    files_done: state.completed_files,
                    files_total: Some(state.total_files),
                    current_object_id: Some(entry.object_id.clone()),
                    ssd_pressure: Some(state.ssd_pressure),
                    telemetry: Some(state.telemetry()),
                    active_hdd_transfers: state.active_hdd_transfer_records(),
                    resource_policy: None,
                    message: Some(format!(
                        "SSD payload synced and queued for HDD settlement: {}",
                        entry.relative_path.to_string_lossy()
                    )),
                })?;
            }
            HddSettlementEvent::Started { entry, roots } => {
                state.hdd_queued = state.hdd_queued.saturating_sub(1);
                state.hdd_active = state.hdd_active.saturating_add(1);
                for (index, root) in roots.iter().enumerate() {
                    state.update_hdd_transfer(
                        &entry,
                        root.disk_id.as_str(),
                        (index + 1) as u8,
                        0,
                        DaemonIngestHddTransferPhase::Writing,
                        None,
                        None,
                    );
                }
                let first_root = roots.first().expect("HDD placement has a target");
                progress(DaemonIngestProgressEvent {
                    job_id: job_id.clone(),
                    endpoint: endpoint.clone(),
                    stage: DaemonIngestStage::HddCopy {
                        disk_id: first_root.disk_id.clone(),
                        copy_number: 1,
                    },
                    pipeline_stage: Some(DaemonIngestPipelineStage::HddPlacement),
                    work_bytes_done: state.completed_work_bytes,
                    work_bytes_total: Some(state.work_bytes_total),
                    source_bytes_done: Some(state.completed_source_bytes),
                    source_bytes_total: Some(state.source_bytes_total),
                    stage_bytes_done: Some(0),
                    stage_bytes_total: Some(entry.size_bytes),
                    files_done: state.completed_files,
                    files_total: Some(state.total_files),
                    current_object_id: Some(entry.object_id),
                    ssd_pressure: Some(state.ssd_pressure),
                    telemetry: Some(state.telemetry()),
                    active_hdd_transfers: state.active_hdd_transfer_records(),
                    resource_policy: None,
                    message: Some(format!(
                        "HDD targets assigned before write: {} ({})",
                        entry.relative_path.to_string_lossy(),
                        roots
                            .iter()
                            .map(|root| root.disk_id.to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )),
                })?;
            }
            HddSettlementEvent::Progress {
                entry,
                progress: object_progress,
            } => {
                state.apply_object_progress(&entry, &object_progress);
                progress(object_progress_event(
                    job_id,
                    endpoint,
                    &entry,
                    state,
                    &object_progress,
                ))?;
            }
            HddSettlementEvent::Settled { entry, report } => {
                commit_object_put(live_sqlite_path, endpoint, &report, recorded_at_utc).map_err(
                    |error| {
                        DaemonIngestFilesRuntimeError::CommandFailed(format!(
                            "failed to commit completed object metadata: {error}"
                        ))
                    },
                )?;
                state.hdd_active = state.hdd_active.saturating_sub(1);
                state.completed_files = state.completed_files.saturating_add(1);
                let active_hdd_transfers = state.active_hdd_transfer_records();
                progress(DaemonIngestProgressEvent {
                    job_id: job_id.clone(),
                    endpoint: endpoint.clone(),
                    stage: DaemonIngestStage::HddCopy {
                        disk_id: DiskId::new("settled").expect("valid settled disk id"),
                        copy_number: 1,
                    },
                    pipeline_stage: Some(DaemonIngestPipelineStage::Finalization),
                    work_bytes_done: state.completed_work_bytes,
                    work_bytes_total: Some(state.work_bytes_total),
                    source_bytes_done: Some(state.completed_source_bytes),
                    source_bytes_total: Some(state.source_bytes_total),
                    stage_bytes_done: Some(entry.size_bytes),
                    stage_bytes_total: Some(entry.size_bytes),
                    files_done: state.completed_files,
                    files_total: Some(state.total_files),
                    current_object_id: Some(entry.object_id.clone()),
                    ssd_pressure: Some(state.ssd_pressure),
                    telemetry: Some(state.telemetry()),
                    active_hdd_transfers,
                    resource_policy: None,
                    message: Some(format!(
                        "file settled: {}",
                        entry.relative_path.to_string_lossy()
                    )),
                })?;
                state.remove_active_hdd_transfers_for_entry(&entry);
            }
            HddSettlementEvent::Failed { error } => return Err(error.into()),
        }

        if !block {
            continue;
        }
        return Ok(());
    }
}

fn object_progress_event(
    job_id: &IngestJobId,
    endpoint: &StoreId,
    entry: &FileIngestEntry,
    state: &PipelineProgressState,
    progress: &ObjectPutProgress,
) -> DaemonIngestProgressEvent {
    DaemonIngestProgressEvent {
        job_id: job_id.clone(),
        endpoint: endpoint.clone(),
        stage: daemon_stage_for_object_progress(progress),
        pipeline_stage: Some(pipeline_stage_for_object_progress(progress)),
        work_bytes_done: state.completed_work_bytes,
        work_bytes_total: Some(state.work_bytes_total),
        source_bytes_done: Some(state.completed_source_bytes),
        source_bytes_total: Some(state.source_bytes_total),
        stage_bytes_done: Some(progress.bytes_written),
        stage_bytes_total: Some(entry.size_bytes),
        files_done: state.completed_files,
        files_total: Some(state.total_files),
        current_object_id: Some(entry.object_id.clone()),
        ssd_pressure: Some(state.ssd_pressure),
        telemetry: Some(state.telemetry()),
        active_hdd_transfers: state.active_hdd_transfer_records(),
        resource_policy: None,
        message: Some(stage_message_for_object_progress(progress, entry)),
    }
}

fn object_progress_stage_key(progress: &ObjectPutProgress) -> String {
    match &progress.stage {
        ObjectPutProgressStage::SsdIngest | ObjectPutProgressStage::SsdFlush => {
            "ssd-ingest".to_string()
        }
        ObjectPutProgressStage::HddCopy {
            disk_id,
            copy_number,
        } => format!("hdd-copy-{disk_id}-{copy_number}"),
        ObjectPutProgressStage::HddFsync {
            disk_id,
            copy_number,
            ..
        } => format!("hdd-fsync-{disk_id}-{copy_number}"),
        ObjectPutProgressStage::HddRename {
            disk_id,
            copy_number,
            ..
        } => format!("hdd-rename-{disk_id}-{copy_number}"),
    }
}

fn daemon_stage_for_object_progress(progress: &ObjectPutProgress) -> DaemonIngestStage {
    match &progress.stage {
        ObjectPutProgressStage::SsdIngest | ObjectPutProgressStage::SsdFlush => {
            DaemonIngestStage::SsdIngest
        }
        ObjectPutProgressStage::HddCopy {
            disk_id,
            copy_number,
        }
        | ObjectPutProgressStage::HddFsync {
            disk_id,
            copy_number,
            ..
        }
        | ObjectPutProgressStage::HddRename {
            disk_id,
            copy_number,
            ..
        } => DaemonIngestStage::HddCopy {
            disk_id: DiskId::new(disk_id).expect("object progress disk id is valid"),
            copy_number: *copy_number,
        },
    }
}

fn pipeline_stage_for_object_progress(progress: &ObjectPutProgress) -> DaemonIngestPipelineStage {
    match progress.stage {
        ObjectPutProgressStage::SsdIngest => DaemonIngestPipelineStage::SsdStage,
        ObjectPutProgressStage::SsdFlush => DaemonIngestPipelineStage::SsdFlush,
        ObjectPutProgressStage::HddCopy { .. } => DaemonIngestPipelineStage::HddWrite,
        ObjectPutProgressStage::HddFsync { .. } => DaemonIngestPipelineStage::HddFsync,
        ObjectPutProgressStage::HddRename { .. } => DaemonIngestPipelineStage::HddRename,
    }
}

fn stage_message_for_object_progress(
    progress: &ObjectPutProgress,
    entry: &FileIngestEntry,
) -> String {
    match &progress.stage {
        ObjectPutProgressStage::SsdIngest => {
            format!("settling to SSD: {}", entry.relative_path.to_string_lossy())
        }
        ObjectPutProgressStage::SsdFlush => {
            format!(
                "syncing staged SSD payload: {}",
                entry.relative_path.to_string_lossy()
            )
        }
        ObjectPutProgressStage::HddCopy {
            disk_id,
            copy_number,
        } => format!(
            "migrating to HDD {disk_id} copy {copy_number}: {}",
            entry.relative_path.to_string_lossy()
        ),
        ObjectPutProgressStage::HddFsync {
            disk_id,
            copy_number,
            duration_millis: None,
        } => format!(
            "fsyncing HDD {disk_id} copy {copy_number}: {}",
            entry.relative_path.to_string_lossy()
        ),
        ObjectPutProgressStage::HddFsync {
            disk_id,
            copy_number,
            duration_millis: Some(duration_millis),
        } => format!(
            "fsync complete for HDD {disk_id} copy {copy_number} in {duration_millis} ms: {}",
            entry.relative_path.to_string_lossy()
        ),
        ObjectPutProgressStage::HddRename {
            disk_id,
            copy_number,
            duration_millis: None,
        } => format!(
            "atomically renaming HDD {disk_id} copy {copy_number}: {}",
            entry.relative_path.to_string_lossy()
        ),
        ObjectPutProgressStage::HddRename {
            disk_id,
            copy_number,
            duration_millis: Some(duration_millis),
        } => format!(
            "atomic rename complete for HDD {disk_id} copy {copy_number} in {duration_millis} ms: {}",
            entry.relative_path.to_string_lossy()
        ),
    }
}

fn ingest_job_id(accepted_at_utc: &str) -> Result<IngestJobId, DaemonIngestFilesRuntimeError> {
    let job_id_value = format!(
        "ingest-files-{}",
        accepted_at_utc
            .chars()
            .map(|character| if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            })
            .collect::<String>()
            .trim_matches('-')
            .to_ascii_lowercase()
    );
    IngestJobId::new(job_id_value.clone())
        .map_err(|_| DaemonIngestFilesRuntimeError::InvalidJobId(job_id_value))
}

#[derive(Debug)]
pub enum DaemonIngestFilesRuntimeError {
    Io(io::Error),
    ObjectService(ObjectServiceError),
    ObjectPut(dasobjectstore_metadata::ObjectPutError),
    Capacity(dasobjectstore_metadata::SsdCapacityMeasurementError),
    ClientDisconnected(String),
    InvalidJobId(String),
    CommandFailed(String),
}

impl Display for DaemonIngestFilesRuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(formatter, "file ingest IO failed: {err}"),
            Self::ObjectService(err) => Display::fmt(err, formatter),
            Self::ObjectPut(err) => Display::fmt(err, formatter),
            Self::Capacity(err) => Display::fmt(err, formatter),
            Self::ClientDisconnected(message) => formatter.write_str(message),
            Self::InvalidJobId(job_id) => write!(formatter, "invalid ingest job id: {job_id}"),
            Self::CommandFailed(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for DaemonIngestFilesRuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::ObjectService(err) => Some(err),
            Self::ObjectPut(err) => Some(err),
            Self::Capacity(err) => Some(err),
            Self::ClientDisconnected(_) | Self::InvalidJobId(_) | Self::CommandFailed(_) => None,
        }
    }
}

impl From<io::Error> for DaemonIngestFilesRuntimeError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<ObjectServiceError> for DaemonIngestFilesRuntimeError {
    fn from(err: ObjectServiceError) -> Self {
        Self::ObjectService(err)
    }
}

impl From<rusqlite::Error> for DaemonIngestFilesRuntimeError {
    fn from(error: rusqlite::Error) -> Self {
        Self::CommandFailed(format!("live metadata SQLite operation failed: {error}"))
    }
}

impl From<dasobjectstore_metadata::ObjectPutError> for DaemonIngestFilesRuntimeError {
    fn from(err: dasobjectstore_metadata::ObjectPutError) -> Self {
        Self::ObjectPut(err)
    }
}

impl From<dasobjectstore_metadata::SsdCapacityMeasurementError> for DaemonIngestFilesRuntimeError {
    fn from(err: dasobjectstore_metadata::SsdCapacityMeasurementError) -> Self {
        Self::Capacity(err)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        collect_ingest_files, default_hdd_worker_count, landing_mode_for_ingest,
        resolve_hdd_worker_count, sync_pending_ssd_stage, FileIngestEntry, HddSettlementDiskState,
        HddSettlementScheduler, LocalFileIngestExecutor, PendingSsdStage, PipelineProgressState,
        SSD_ROOT_ENV,
    };
    use crate::api::{
        DaemonIngestConflictPolicy, DaemonIngestHddTransferPhase, DaemonIngestPipelineStage,
        DaemonIngressLandingMode, DaemonIngressOrigin, DaemonSsdPressure, SubmitIngestFilesRequest,
    };
    use dasobjectstore_core::ids::{IngestJobId, ObjectId, StoreId};
    use dasobjectstore_core::object_type::ObjectType;
    use dasobjectstore_core::store::{IngestMode, StoreClass, StorePolicy};
    use dasobjectstore_metadata::{
        hash_file_sha256, object_payload_path, DiskCopyRoot, IngestStagingLayout,
        ObjectPutProgress, ObjectPutProgressStage, ObjectPutRequest, SsdCapacityPolicy,
    };
    use dasobjectstore_object_service::StoreServiceDefinition;
    use rusqlite::Connection;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn dry_run_discovers_files_without_copying_payloads() {
        let root = temp_root("daemon-ingest-dry-run");
        let ssd_root = root.join("ssd");
        let hdd_root = root.join("hdd");
        let source_root = root.join("source");
        let registry_path = root.join("stores.json");
        let subobject_registry_path = root.join("subobjects.json");
        write_device_marker(&ssd_root, "role=ssd");
        write_device_marker(&hdd_root.join("disk-a"), "role=hdd:disk-a");
        fs::create_dir_all(source_root.join("nested")).expect("source nested dir");
        fs::write(source_root.join("nested").join("sample.fastq.gz"), b"ACGT")
            .expect("source file");
        write_store_registry(&registry_path);
        fs::write(&subobject_registry_path, "[]\n").expect("subobject registry");

        let executor = LocalFileIngestExecutor {
            ssd_root: ssd_root.clone(),
            hdd_root: hdd_root.clone(),
            live_sqlite_path: ssd_root.join(".dasobjectstore").join("live.sqlite"),
            store_registry_path: registry_path,
            subobject_registry_path,
            source_is_server_local: |_| true,
            capacity_policy: SsdCapacityPolicy::default(),
        };

        let mut progress_events = Vec::new();
        let response = executor
            .submit(
                SubmitIngestFilesRequest {
                    endpoint: StoreId::new("zymo_fecal_2025.05").expect("store id"),
                    source_path: source_root,
                    object_type: ObjectType::Fastq,
                    copies: Some(1),
                    hdd_workers: None,
                    ingress_origin: DaemonIngressOrigin::LocalServer,
                    conflict_policy: DaemonIngestConflictPolicy::Strict,
                    dry_run: true,
                    client_request_id: None,
                },
                "2026-07-07T10:27:12Z",
                |event| {
                    progress_events.push(event);
                    Ok(())
                },
            )
            .expect("dry run succeeds");

        assert_eq!(
            response.job_id.as_str(),
            "ingest-files-2026-07-07t10-27-12z"
        );
        assert!(response.dry_run);
        assert!(!ssd_root.join("ingest").exists());
        let planned = progress_events.first().expect("planned progress event");
        assert_eq!(planned.source_bytes_done, Some(0));
        assert_eq!(planned.source_bytes_total, Some(4));
        assert_eq!(planned.work_bytes_done, 0);
        assert_eq!(planned.work_bytes_total, Some(8));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn local_server_direct_policy_bypasses_ssd_payload_and_writes_hdd_copy() {
        let root = temp_root("daemon-ingest-direct-hdd");
        let ssd_root = root.join("ssd");
        let hdd_root = root.join("hdd");
        let source_root = root.join("source");
        let registry_path = root.join("stores.json");
        let subobject_registry_path = root.join("subobjects.json");
        write_device_marker(&ssd_root, "role=ssd");
        write_device_marker(&hdd_root.join("disk-a"), "role=hdd:disk-a");
        fs::create_dir_all(&source_root).expect("source dir");
        fs::write(
            source_root.join("reference.fa.zst"),
            b"reproducible reference",
        )
        .expect("source file");
        let mut policy = StorePolicy::defaults_for(StoreClass::ReproducibleCache);
        policy.ingest_mode = IngestMode::DirectToHdd;
        write_store_registry_with_policy(&registry_path, policy);
        fs::write(&subobject_registry_path, "[]\n").expect("subobject registry");

        let executor = LocalFileIngestExecutor {
            ssd_root: ssd_root.clone(),
            hdd_root: hdd_root.clone(),
            live_sqlite_path: ssd_root.join(".dasobjectstore").join("live.sqlite"),
            store_registry_path: registry_path,
            subobject_registry_path,
            source_is_server_local: |_| true,
            capacity_policy: SsdCapacityPolicy::default(),
        };

        let mut progress_events = Vec::new();
        executor
            .submit(
                SubmitIngestFilesRequest {
                    endpoint: StoreId::new("zymo_fecal_2025.05").expect("store id"),
                    source_path: source_root,
                    object_type: ObjectType::Fasta,
                    copies: Some(1),
                    hdd_workers: None,
                    ingress_origin: DaemonIngressOrigin::LocalServer,
                    conflict_policy: DaemonIngestConflictPolicy::Strict,
                    dry_run: false,
                    client_request_id: None,
                },
                "2026-07-09T13:02:22Z",
                |event| {
                    progress_events.push(event);
                    Ok(())
                },
            )
            .expect("direct local ingest succeeds");

        assert!(!ssd_root.join(".dasobjectstore/ingest/jobs").exists());
        assert!(!progress_events
            .iter()
            .any(|event| event.pipeline_stage == Some(DaemonIngestPipelineStage::SourceRead)));
        assert!(progress_events
            .iter()
            .any(|event| event.pipeline_stage == Some(DaemonIngestPipelineStage::HddWrite)));
        assert!(progress_events.iter().any(|event| {
            event.pipeline_stage == Some(DaemonIngestPipelineStage::HddPlacement)
                && event.active_hdd_transfers.iter().any(|transfer| {
                    transfer.bytes_done == 0 && transfer.disk_id.as_str() != "pending"
                })
        }));
        assert!(progress_events
            .iter()
            .any(|event| event.pipeline_stage == Some(DaemonIngestPipelineStage::HddFsync)));
        assert!(progress_events
            .iter()
            .any(|event| event.pipeline_stage == Some(DaemonIngestPipelineStage::HddRename)));
        assert!(progress_events.iter().any(|event| {
            event.active_hdd_transfers.iter().any(|transfer| {
                transfer.phase == DaemonIngestHddTransferPhase::Fsync
                    && transfer.bytes_per_second == 0
                    && transfer.fsync_duration_millis.is_some()
            })
        }));
        assert!(progress_events
            .iter()
            .any(|event| !event.active_hdd_transfers.is_empty()));
        assert!(!progress_events
            .iter()
            .any(|event| event.pipeline_stage == Some(DaemonIngestPipelineStage::SsdStage)));
        assert_eq!(
            find_payloads(&hdd_root.join("disk-a").join("objects")),
            vec![b"reproducible reference".to_vec()]
        );

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn external_origins_use_ssd_first_executor_path_under_direct_policy() {
        for origin in [
            DaemonIngressOrigin::UsbMountedDisk,
            DaemonIngressOrigin::WebUpload,
            DaemonIngressOrigin::RemoteS3,
        ] {
            let root = temp_root(&format!("external-origin-{origin}"));
            let ssd_root = root.join("ssd");
            let hdd_root = root.join("hdd");
            let source_root = root.join("source");
            let registry_path = root.join("stores.json");
            let subobject_registry_path = root.join("subobjects.json");
            write_device_marker(&ssd_root, "role=ssd");
            write_device_marker(&hdd_root.join("disk-a"), "role=hdd:disk-a");
            fs::create_dir_all(&source_root).expect("source dir");
            fs::write(source_root.join("reference.fa.zst"), b"external source")
                .expect("source file");
            let mut policy = StorePolicy::defaults_for(StoreClass::ReproducibleCache);
            policy.ingest_mode = IngestMode::DirectToHdd;
            write_store_registry_with_policy(&registry_path, policy);
            fs::write(&subobject_registry_path, "[]\n").expect("subobject registry");

            let executor = LocalFileIngestExecutor {
                ssd_root: ssd_root.clone(),
                hdd_root: hdd_root.clone(),
                live_sqlite_path: ssd_root.join(".dasobjectstore").join("live.sqlite"),
                store_registry_path: registry_path,
                subobject_registry_path,
                source_is_server_local: |_| false,
                capacity_policy: SsdCapacityPolicy::new(99, 100, 0).expect("capacity policy"),
            };
            let mut events = Vec::new();
            executor
                .submit(
                    SubmitIngestFilesRequest {
                        endpoint: StoreId::new("zymo_fecal_2025.05").expect("store id"),
                        source_path: source_root,
                        object_type: ObjectType::Fasta,
                        copies: Some(1),
                        hdd_workers: None,
                        ingress_origin: origin,
                        conflict_policy: DaemonIngestConflictPolicy::Force,
                        dry_run: false,
                        client_request_id: None,
                    },
                    "2026-07-10T13:10:00Z",
                    |event| {
                        events.push(event);
                        Ok(())
                    },
                )
                .expect("external origin ingest succeeds");

            assert!(events.iter().any(|event| {
                event.message.as_deref().is_some_and(|message| {
                    message.starts_with("preflight:")
                        && message.contains("source=")
                        && message.contains("source topology=")
                        && message.contains("mount_point=")
                        && message.contains("filesystem=")
                        && message.contains("backing_device=")
                        && message.contains("major_minor=")
                        && message.contains("origin=")
                        && message.contains("store_ingest_mode=")
                        && message.contains("landing mode ssd_first")
                        && message.contains("reason=")
                })
            }));
            let ssd_stage_index = events
                .iter()
                .position(|event| event.pipeline_stage == Some(DaemonIngestPipelineStage::SsdStage))
                .expect("SSD stage event");
            assert!(events[ssd_stage_index..]
                .iter()
                .any(|event| event.pipeline_stage == Some(DaemonIngestPipelineStage::SsdFlush)));
            fs::remove_dir_all(root).expect("cleanup temp root");
        }
    }

    #[test]
    fn direct_ingest_default_conflict_policy_skips_preflight_source_hash() {
        let root = temp_root("daemon-ingest-direct-default-no-preflight");
        let ssd_root = root.join("ssd");
        let hdd_root = root.join("hdd");
        let source_root = root.join("source");
        let registry_path = root.join("stores.json");
        let subobject_registry_path = root.join("subobjects.json");
        let live_sqlite_path = ssd_root.join(".dasobjectstore").join("live.sqlite");
        write_device_marker(&ssd_root, "role=ssd");
        write_device_marker(&hdd_root.join("disk-a"), "role=hdd:disk-a");
        fs::create_dir_all(&source_root).expect("source dir");
        fs::write(
            source_root.join("reference.fa.zst"),
            b"new reference payload",
        )
        .expect("source file");
        write_existing_object_metadata(
            &live_sqlite_path,
            "zymo_fecal_2025.05/reference.fa.zst",
            "sha256:older-payload",
            21,
        );
        let mut policy = StorePolicy::defaults_for(StoreClass::ReproducibleCache);
        policy.ingest_mode = IngestMode::DirectToHdd;
        write_store_registry_with_policy(&registry_path, policy);
        fs::write(&subobject_registry_path, "[]\n").expect("subobject registry");

        let request: SubmitIngestFilesRequest = serde_json::from_value(serde_json::json!({
            "endpoint": "zymo_fecal_2025.05",
            "source_path": source_root,
            "copies": 1,
            "ingress_origin": "local_server",
            "dry_run": false,
            "client_request_id": null
        }))
        .expect("default request deserializes");
        assert_eq!(request.conflict_policy, DaemonIngestConflictPolicy::Force);

        let executor = LocalFileIngestExecutor {
            ssd_root: ssd_root.clone(),
            hdd_root: hdd_root.clone(),
            live_sqlite_path,
            store_registry_path: registry_path,
            subobject_registry_path,
            source_is_server_local: |_| true,
            capacity_policy: SsdCapacityPolicy::default(),
        };
        let mut progress_events = Vec::new();
        executor
            .submit(request, "2026-07-10T10:00:00Z", |event| {
                progress_events.push(event);
                Ok(())
            })
            .expect("default direct ingest succeeds");

        assert!(progress_events.iter().all(|event| {
            event.pipeline_stage != Some(DaemonIngestPipelineStage::ChecksumManifestCapture)
        }));
        assert!(progress_events.iter().any(|event| {
            event.pipeline_stage == Some(DaemonIngestPipelineStage::HddWrite)
                && !event.active_hdd_transfers.is_empty()
        }));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn direct_ingest_strict_hashes_during_hdd_copy_when_metadata_exists() {
        let root = temp_root("daemon-ingest-direct-strict-in-flight-hash");
        let ssd_root = root.join("ssd");
        let hdd_root = root.join("hdd");
        let source_root = root.join("source");
        let registry_path = root.join("stores.json");
        let subobject_registry_path = root.join("subobjects.json");
        let live_sqlite_path = ssd_root.join(".dasobjectstore").join("live.sqlite");
        write_device_marker(&ssd_root, "role=ssd");
        write_device_marker(&hdd_root.join("disk-a"), "role=hdd:disk-a");
        fs::create_dir_all(&source_root).expect("source dir");
        let source_path = source_root.join("reference.fa.zst");
        fs::write(&source_path, b"reproducible reference").expect("source file");
        let object_id = ObjectId::new("zymo_fecal_2025.05/reference.fa.zst").expect("object id");
        let content_hash = hash_file_sha256(&source_path).expect("source hash");
        write_existing_object_metadata(&live_sqlite_path, object_id.as_str(), &content_hash, 22);
        let mut policy = StorePolicy::defaults_for(StoreClass::ReproducibleCache);
        policy.ingest_mode = IngestMode::DirectToHdd;
        write_store_registry_with_policy(&registry_path, policy);
        fs::write(&subobject_registry_path, "[]\n").expect("subobject registry");

        let executor = LocalFileIngestExecutor {
            ssd_root: ssd_root.clone(),
            hdd_root: hdd_root.clone(),
            live_sqlite_path,
            store_registry_path: registry_path,
            subobject_registry_path,
            source_is_server_local: |_| true,
            capacity_policy: SsdCapacityPolicy::default(),
        };

        let mut progress_events = Vec::new();
        executor
            .submit(
                SubmitIngestFilesRequest {
                    endpoint: StoreId::new("zymo_fecal_2025.05").expect("store id"),
                    source_path: source_root,
                    object_type: ObjectType::Fasta,
                    copies: Some(1),
                    hdd_workers: None,
                    ingress_origin: DaemonIngressOrigin::LocalServer,
                    conflict_policy: DaemonIngestConflictPolicy::Strict,
                    dry_run: false,
                    client_request_id: None,
                },
                "2026-07-09T18:44:22Z",
                |event| {
                    progress_events.push(event);
                    Ok(())
                },
            )
            .expect("strict direct ingest succeeds");

        assert!(!progress_events.iter().any(|event| {
            event.pipeline_stage == Some(DaemonIngestPipelineStage::ChecksumManifestCapture)
        }));
        assert!(progress_events.iter().any(|event| {
            event.pipeline_stage == Some(DaemonIngestPipelineStage::HddWrite)
                && !event.active_hdd_transfers.is_empty()
        }));
        assert_eq!(
            find_payloads(&hdd_root.join("disk-a").join("objects")),
            vec![b"reproducible reference".to_vec()]
        );

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn direct_ingest_strict_deduplicates_existing_payload_after_in_flight_hash() {
        let root = temp_root("daemon-ingest-direct-strict-payload-dedupe");
        let ssd_root = root.join("ssd");
        let hdd_root = root.join("hdd");
        let source_root = root.join("source");
        let registry_path = root.join("stores.json");
        let subobject_registry_path = root.join("subobjects.json");
        write_device_marker(&ssd_root, "role=ssd");
        write_device_marker(&hdd_root.join("disk-a"), "role=hdd:disk-a");
        fs::create_dir_all(&source_root).expect("source dir");
        let source_path = source_root.join("reference.fa.zst");
        fs::write(&source_path, b"reproducible reference").expect("source file");
        let object_id = ObjectId::new("zymo_fecal_2025.05/reference.fa.zst").expect("object id");
        let content_hash = hash_file_sha256(&source_path).expect("source hash");
        let disk_root = DiskCopyRoot::new(
            dasobjectstore_core::ids::DiskId::new("disk-a").expect("disk id"),
            hdd_root.join("disk-a"),
        );
        let payload_path = object_payload_path(&disk_root, &object_id, &content_hash);
        fs::create_dir_all(payload_path.parent().expect("payload parent"))
            .expect("payload parent dir");
        fs::write(&payload_path, b"reproducible reference").expect("existing payload");
        let mut policy = StorePolicy::defaults_for(StoreClass::ReproducibleCache);
        policy.ingest_mode = IngestMode::DirectToHdd;
        write_store_registry_with_policy(&registry_path, policy);
        fs::write(&subobject_registry_path, "[]\n").expect("subobject registry");

        let executor = LocalFileIngestExecutor {
            ssd_root: ssd_root.clone(),
            hdd_root: hdd_root.clone(),
            live_sqlite_path: ssd_root.join(".dasobjectstore").join("live.sqlite"),
            store_registry_path: registry_path,
            subobject_registry_path,
            source_is_server_local: |_| true,
            capacity_policy: SsdCapacityPolicy::default(),
        };

        let mut progress_events = Vec::new();
        executor
            .submit(
                SubmitIngestFilesRequest {
                    endpoint: StoreId::new("zymo_fecal_2025.05").expect("store id"),
                    source_path: source_root,
                    object_type: ObjectType::Fasta,
                    copies: Some(1),
                    hdd_workers: None,
                    ingress_origin: DaemonIngressOrigin::LocalServer,
                    conflict_policy: DaemonIngestConflictPolicy::Strict,
                    dry_run: false,
                    client_request_id: None,
                },
                "2026-07-09T18:55:22Z",
                |event| {
                    progress_events.push(event);
                    Ok(())
                },
            )
            .expect("strict direct ingest deduplicates existing payload");

        assert!(!progress_events.iter().any(|event| {
            event.pipeline_stage == Some(DaemonIngestPipelineStage::ChecksumManifestCapture)
        }));
        assert!(progress_events.iter().any(|event| {
            event.pipeline_stage == Some(DaemonIngestPipelineStage::HddWrite)
                && !event.active_hdd_transfers.is_empty()
        }));
        assert_eq!(
            find_payloads(&hdd_root.join("disk-a").join("objects")),
            vec![b"reproducible reference".to_vec()]
        );

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn direct_policy_only_bypasses_ssd_for_local_server_origins() {
        let mut policy = StorePolicy::defaults_for(StoreClass::ReproducibleCache);
        policy.ingest_mode = IngestMode::DirectToHdd;

        assert_eq!(
            landing_mode_for_ingest(&policy, DaemonIngressOrigin::LocalServer),
            DaemonIngressLandingMode::DirectToHddWhenPolicyAllows
        );
        assert_eq!(
            landing_mode_for_ingest(&policy, DaemonIngressOrigin::LocalServerDirectImport),
            DaemonIngressLandingMode::DirectToHddWhenPolicyAllows
        );
        assert_eq!(
            landing_mode_for_ingest(&policy, DaemonIngressOrigin::LocalServerSsdFirst),
            DaemonIngressLandingMode::SsdFirst
        );
        assert_eq!(
            landing_mode_for_ingest(&policy, DaemonIngressOrigin::UsbMountedDisk),
            DaemonIngressLandingMode::SsdFirst
        );
        assert_eq!(
            landing_mode_for_ingest(&policy, DaemonIngressOrigin::RemoteS3),
            DaemonIngressLandingMode::SsdFirst
        );
        assert_eq!(
            landing_mode_for_ingest(&policy, DaemonIngressOrigin::WebUpload),
            DaemonIngressLandingMode::SsdFirst
        );

        let ssd_first_policy = StorePolicy::defaults_for(StoreClass::GeneratedData);
        assert_eq!(
            landing_mode_for_ingest(
                &ssd_first_policy,
                DaemonIngressOrigin::LocalServerDirectImport
            ),
            DaemonIngressLandingMode::SsdFirst
        );
    }

    #[test]
    fn file_ingest_splits_inline_hash_ssd_sync_and_hdd_settlement() {
        let root = temp_root("daemon-ingest-split-pipeline");
        let ssd_root = root.join("ssd");
        let source_root = root.join("source");
        fs::create_dir_all(&source_root).expect("source dir");
        let source_path = source_root.join("a.fastq.gz");
        fs::write(&source_path, b"AAAABBBB").expect("source file");
        let object_id = ObjectId::new("zymo_fecal_2025.05/a.fastq.gz").expect("object id");
        let layout = IngestStagingLayout::for_ssd_root(&ssd_root);
        let job_id = IngestJobId::new(format!("put-{object_id}")).expect("job id");
        let job_paths = layout.job_paths(&job_id);
        let mut source = fs::File::open(&source_path).expect("source open");
        let mut write_progress = Vec::new();
        let write_report = job_paths
            .write_payload_with_hash_unsynced_controlled_progress(&mut source, |bytes| {
                write_progress.push(bytes);
                Ok(())
            })
            .expect("source writes with inline hash");
        let disk_root = dasobjectstore_metadata::DiskCopyRoot::new(
            dasobjectstore_core::ids::DiskId::new("disk-a").expect("disk id"),
            root.join("hdd").join("disk-a"),
        );
        let request = ObjectPutRequest::new(
            object_id.clone(),
            source_path,
            &ssd_root,
            vec![disk_root],
            1,
        )
        .with_object_type(ObjectType::Fastq);
        let pending = PendingSsdStage {
            request,
            job_paths,
            write_report,
        };
        let mut flush_stages = Vec::new();

        let staged = sync_pending_ssd_stage(pending, |progress| {
            flush_stages.push(progress.stage);
            Ok(())
        })
        .expect("side worker syncs inline-hashed payload");

        assert_eq!(write_progress, vec![8]);
        assert_eq!(staged.bytes_staged, 8);
        assert_eq!(staged.content_hash_algorithm, "sha256");
        assert_eq!(staged.content_hash.len(), 64);
        assert!(flush_stages.contains(&ObjectPutProgressStage::SsdFlush));

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn directory_ingest_skips_hidden_files_and_hidden_directories() {
        let root = temp_root("daemon-ingest-hidden");
        let source_root = root.join("source");
        fs::create_dir_all(source_root.join("nested")).expect("source nested dir");
        fs::create_dir_all(source_root.join(".partial")).expect("hidden source dir");
        fs::write(source_root.join("nested").join("sample.fastq.gz"), b"ACGT")
            .expect("visible source file");
        fs::write(source_root.join(".hidden.pod5.tmp"), b"temporary payload")
            .expect("hidden source file");
        fs::write(
            source_root.join(".partial").join("sample.fastq.gz"),
            b"temporary payload",
        )
        .expect("hidden directory file");

        let files = collect_ingest_files(&source_root, "zymo_fecal_2025.05").expect("files scan");

        assert_eq!(files.len(), 1);
        assert_eq!(
            files[0].relative_path,
            PathBuf::from("nested/sample.fastq.gz")
        );

        fs::remove_dir_all(root).expect("cleanup temp root");
    }

    #[test]
    fn pipeline_progress_tracks_concurrent_workers_fifo_depth_and_pressure() {
        let mut state = PipelineProgressState::new(10, 1_000, 2_000, 3, false);
        state.staged_files = 3;
        state.ssd_active = 1;
        state.hdd_active = 1;
        state.hdd_queued = 2;
        state.ssd_pressure = DaemonSsdPressure::High;
        let entry = FileIngestEntry {
            source_path: PathBuf::from("/source/a.fastq.gz"),
            relative_path: PathBuf::from("a.fastq.gz"),
            object_id: ObjectId::new("store/a.fastq.gz").expect("object id"),
            size_bytes: 100,
            file_index: 1,
        };

        state.apply_object_progress(
            &entry,
            &ObjectPutProgress {
                object_id: entry.object_id.clone(),
                stage: ObjectPutProgressStage::SsdIngest,
                bytes_written: 40,
            },
        );
        state.apply_object_progress(
            &entry,
            &ObjectPutProgress {
                object_id: entry.object_id.clone(),
                stage: ObjectPutProgressStage::SsdIngest,
                bytes_written: 80,
            },
        );
        state.apply_object_progress(
            &entry,
            &ObjectPutProgress {
                object_id: entry.object_id.clone(),
                stage: ObjectPutProgressStage::HddCopy {
                    disk_id: "disk-a".to_string(),
                    copy_number: 1,
                },
                bytes_written: 25,
            },
        );
        state.apply_object_progress(
            &entry,
            &ObjectPutProgress {
                object_id: entry.object_id.clone(),
                stage: ObjectPutProgressStage::HddCopy {
                    disk_id: "disk-a".to_string(),
                    copy_number: 1,
                },
                bytes_written: 50,
            },
        );

        let telemetry = state.telemetry();
        assert_eq!(state.completed_source_bytes, 80);
        assert_eq!(state.completed_work_bytes, 130);
        assert_eq!(telemetry.queue_depths.source_read, 6);
        assert_eq!(telemetry.queue_depths.hdd_write, 2);
        assert_eq!(telemetry.workers.ssd_stage.active, 1);
        assert_eq!(telemetry.workers.hdd_write.active, 1);
        assert_eq!(telemetry.workers.hdd_write.idle, 2);
        assert_eq!(telemetry.pressure.ssd, DaemonSsdPressure::High);
        assert!(telemetry.throughput.source_read_bytes_per_second > 0);
        assert!(telemetry.throughput.ssd_write_bytes_per_second > 0);
        assert!(telemetry.throughput.aggregate_hdd_write_bytes_per_second > 0);
        let active = state.active_hdd_transfer_records();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].file_index, 1);
        assert_eq!(active[0].files_total, Some(10));
        assert_eq!(active[0].disk_id.as_str(), "disk-a");
        assert_eq!(active[0].copy_number, 1);
        assert_eq!(active[0].bytes_done, 50);
        assert_eq!(active[0].bytes_total, 100);
        assert_eq!(active[0].relative_path, "a.fastq.gz");
        assert!(active[0].bytes_per_second > 0);

        state.apply_object_progress(
            &entry,
            &ObjectPutProgress {
                object_id: entry.object_id.clone(),
                stage: ObjectPutProgressStage::HddFsync {
                    disk_id: "disk-a".to_string(),
                    copy_number: 1,
                    duration_millis: Some(7),
                },
                bytes_written: 50,
            },
        );
        let finalizing = state.active_hdd_transfer_records();
        assert_eq!(finalizing[0].bytes_per_second, 0);
        assert_eq!(finalizing[0].fsync_duration_millis, Some(7));
        assert_eq!(
            state
                .telemetry()
                .throughput
                .aggregate_hdd_write_bytes_per_second,
            0
        );
    }

    #[test]
    fn hdd_settlement_scheduler_reserves_only_idle_highest_fraction_disks() {
        let disk_a = dasobjectstore_core::ids::DiskId::new("disk-a").expect("disk id");
        let disk_b = dasobjectstore_core::ids::DiskId::new("disk-b").expect("disk id");
        let disk_c = dasobjectstore_core::ids::DiskId::new("disk-c").expect("disk id");
        let mut scheduler = HddSettlementScheduler {
            disks: vec![
                HddSettlementDiskState {
                    disk_id: disk_a.clone(),
                    root_path: PathBuf::from("/hdd/a"),
                    active: false,
                    total_bytes: 100,
                    available_bytes: 90,
                    assigned_bytes: 0,
                },
                HddSettlementDiskState {
                    disk_id: disk_b,
                    root_path: PathBuf::from("/hdd/b"),
                    active: true,
                    total_bytes: 100,
                    available_bytes: 95,
                    assigned_bytes: 0,
                },
                HddSettlementDiskState {
                    disk_id: disk_c.clone(),
                    root_path: PathBuf::from("/hdd/c"),
                    active: false,
                    total_bytes: 200,
                    available_bytes: 100,
                    assigned_bytes: 0,
                },
            ],
        };

        let roots = scheduler
            .reserve_roots(2, 8)
            .expect("reservation evaluates")
            .expect("two idle disks reserve");
        let blocked = scheduler
            .reserve_roots(1, 8)
            .expect("second reservation evaluates");

        assert_eq!(
            roots
                .iter()
                .map(|root| root.disk_id.clone())
                .collect::<Vec<_>>(),
            vec![disk_a, disk_c]
        );
        assert!(
            blocked.is_none(),
            "active reservations must block instead of assigning a second writer to an HDD"
        );
    }

    #[test]
    fn hdd_settlement_scheduler_enforces_redundancy_on_distinct_eligible_disks() {
        let disk_a = dasobjectstore_core::ids::DiskId::new("disk-a").expect("disk id");
        let disk_b = dasobjectstore_core::ids::DiskId::new("disk-b").expect("disk id");
        let disk_c = dasobjectstore_core::ids::DiskId::new("disk-c").expect("disk id");
        let mut scheduler = HddSettlementScheduler {
            disks: vec![
                HddSettlementDiskState {
                    disk_id: disk_a.clone(),
                    root_path: PathBuf::from("/hdd/a"),
                    active: false,
                    total_bytes: 100,
                    available_bytes: 90,
                    assigned_bytes: 0,
                },
                HddSettlementDiskState {
                    disk_id: disk_b.clone(),
                    root_path: PathBuf::from("/hdd/b"),
                    active: false,
                    total_bytes: 100,
                    available_bytes: 80,
                    assigned_bytes: 0,
                },
                HddSettlementDiskState {
                    disk_id: disk_c.clone(),
                    root_path: PathBuf::from("/hdd/c"),
                    active: false,
                    total_bytes: 100,
                    available_bytes: 70,
                    assigned_bytes: 0,
                },
            ],
        };

        let roots = scheduler
            .reserve_roots(3, 16)
            .expect("reservation evaluates")
            .expect("three copies reserve");

        assert_eq!(
            roots
                .iter()
                .map(|root| root.disk_id.clone())
                .collect::<Vec<_>>(),
            vec![disk_a, disk_b, disk_c]
        );
        assert!(
            scheduler
                .reserve_roots(1, 16)
                .expect("overlapping reservation evaluates")
                .is_none(),
            "active redundant reservation must preserve one writer per HDD"
        );

        scheduler.release_roots(&roots, 16);
        let err = scheduler
            .reserve_roots(4, 16)
            .expect_err("more copies than physical disks rejected");

        assert!(err.to_string().contains("HDD settlement needs 4 disk(s)"));
    }

    #[test]
    fn hdd_settlement_scheduler_rejects_duplicate_physical_disk_ids() {
        let disk_a = dasobjectstore_core::ids::DiskId::new("disk-a").expect("disk id");
        let roots = vec![
            DiskCopyRoot::new(disk_a.clone(), PathBuf::from("/hdd/a1")),
            DiskCopyRoot::new(disk_a, PathBuf::from("/hdd/a2")),
        ];

        let err = HddSettlementScheduler::new(&roots).expect_err("duplicate disk rejected");

        assert!(err.to_string().contains("duplicate disk ID disk-a"));
        assert!(err
            .to_string()
            .contains("redundant copies require distinct physical disks"));
    }

    #[test]
    fn hdd_workers_default_to_up_to_four_concurrent_distinct_disk_sets() {
        for (hdd_count, copies, expected_workers) in [
            (1, 1, 1),
            (3, 1, 3),
            (4, 1, 4),
            (8, 1, 4),
            (4, 2, 2),
            (7, 2, 3),
            (8, 3, 2),
        ] {
            assert_eq!(
                default_hdd_worker_count(hdd_count, copies),
                expected_workers
            );
            assert_eq!(
                resolve_hdd_worker_count(None, hdd_count, copies as u8).expect("workers"),
                expected_workers
            );
        }
    }

    #[test]
    fn hdd_workers_allow_explicit_override_within_detected_hdd_count() {
        assert_eq!(resolve_hdd_worker_count(Some(3), 7, 2).expect("workers"), 3);

        let err = resolve_hdd_worker_count(Some(4), 7, 2).expect_err("too many workers");

        assert!(err
            .to_string()
            .contains("exceeds the 3 concurrent object(s)"));
    }

    #[test]
    fn hdd_workers_reject_zero_and_missing_hdd_roots() {
        assert!(resolve_hdd_worker_count(Some(0), 7, 1)
            .expect_err("zero workers")
            .to_string()
            .contains("greater than zero"));
        assert!(resolve_hdd_worker_count(None, 0, 1)
            .expect_err("missing HDD roots")
            .to_string()
            .contains("at least one managed HDD root"));
    }

    #[test]
    fn default_ssd_root_uses_environment_override() {
        let root = temp_root("daemon-ingest-env-root");
        std::env::set_var(SSD_ROOT_ENV, &root);
        assert_eq!(super::default_ssd_root(), root);
        std::env::remove_var(SSD_ROOT_ENV);
    }

    fn write_device_marker(root: &Path, marker: &str) {
        fs::create_dir_all(root.join(".dasobjectstore")).expect("device marker dir");
        fs::write(root.join(".dasobjectstore").join("device.env"), marker).expect("device marker");
    }

    fn write_store_registry(path: &Path) {
        write_store_registry_with_policy(
            path,
            StorePolicy::defaults_for(StoreClass::ReproducibleCache),
        );
    }

    fn write_store_registry_with_policy(path: &Path, policy: StorePolicy) {
        let definition = StoreServiceDefinition {
            store_id: StoreId::new("zymo_fecal_2025.05").expect("store id"),
            policy,
            bucket_name: Some("dos-zymo-fecal-2025-05".to_string()),
            reader_group: None,
            writer_group: None,
            public: false,
        };
        let json = serde_json::to_string_pretty(&vec![definition]).expect("store registry json");
        fs::write(path, json).expect("store registry");
    }

    fn write_existing_object_metadata(
        live_sqlite_path: &Path,
        object_id: &str,
        content_hash: &str,
        size_bytes: i64,
    ) {
        let parent = live_sqlite_path.parent().expect("live sqlite parent");
        fs::create_dir_all(parent).expect("metadata dir");
        let connection = Connection::open(live_sqlite_path).expect("live sqlite opens");
        connection
            .execute_batch(dasobjectstore_metadata::LIVE_SCHEMA_SQL)
            .expect("metadata schema");
        connection
            .execute(
                "INSERT INTO pools VALUES ('pool-a', 'Clean', 'now', 'now')",
                [],
            )
            .expect("pool metadata");
        connection
            .execute(
                "INSERT INTO stores (store_id, pool_id, class, policy_json, created_at_utc, updated_at_utc)
                 VALUES (?1, 'pool-a', ?2, '{}', 'now', 'now')",
                ("zymo_fecal_2025.05", "reproducible_cache"),
            )
            .expect("store metadata");
        connection
            .execute(
                "INSERT INTO disks (disk_id, pool_id, role, state, created_at_utc, updated_at_utc)
                 VALUES ('disk-a', 'pool-a', 'hdd', 'Healthy', 'now', 'now')",
                [],
            )
            .expect("disk metadata");
        connection
            .execute(
                "INSERT INTO objects (
                    object_id,
                    store_id,
                    object_type,
                    state,
                    size_bytes,
                    content_hash,
                    created_at_utc,
                    updated_at_utc
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                (
                    object_id,
                    "zymo_fecal_2025.05",
                    "fasta",
                    "HddCopyVerified",
                    size_bytes,
                    content_hash,
                    "2026-07-09T18:43:00Z",
                    "2026-07-09T18:43:00Z",
                ),
            )
            .expect("object metadata");
    }

    fn find_payloads(root: &Path) -> Vec<Vec<u8>> {
        let mut payloads = Vec::new();
        if !root.exists() {
            return payloads;
        }
        for entry in fs::read_dir(root).expect("read object tree") {
            let path = entry.expect("object tree entry").path();
            if path.is_dir() {
                payloads.extend(find_payloads(&path));
            } else if path.file_name().and_then(|name| name.to_str()) == Some("payload") {
                payloads.push(fs::read(path).expect("payload reads"));
            }
        }
        payloads.sort();
        payloads
    }

    fn temp_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!("{label}-{}-{nanos}", std::process::id()))
    }
}
