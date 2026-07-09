use crate::api::{
    DaemonIngestPipelineStage, DaemonIngestProgressEvent, DaemonIngestQueueDepths,
    DaemonIngestStage, DaemonIngestTelemetry, DaemonIngestWorkerActivity,
    DaemonIngestWorkerTelemetry, DaemonSsdPressure, SubmitIngestFilesRequest,
    SubmitIngestFilesResponse,
};
use crate::runtime::{
    authoritative_performance_recommendation_path, read_authoritative_ingest_policy,
    AuthoritativeIngestPolicy, DEFAULT_DAEMON_STATE_DIR,
};
use dasobjectstore_core::ids::{DiskId, IngestJobId, ObjectId, StoreId};
use dasobjectstore_core::object_type::ObjectType;
use dasobjectstore_metadata::{
    measure_ssd_capacity, settle_staged_object_to_hdd_with_controlled_progress, DiskCopyRoot,
    IngestJobPaths, IngestStagingLayout, IngestWriteReport, ObjectPutError, ObjectPutProgress,
    ObjectPutProgressStage, ObjectPutRequest, SsdCapacityPolicy, SsdPressure, StagedObjectPut,
};
use dasobjectstore_object_service::{
    default_store_registry_path, default_subobject_registry_path, read_store_registry,
    read_subobject_registry, ObjectServiceError, StoreServiceDefinition,
};
use std::collections::BTreeMap;
use std::fmt::{self, Display};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread;

const SSD_ROOT_ENV: &str = "DASOBJECTSTORE_SSD_ROOT";
const HDD_ROOT_ENV: &str = "DASOBJECTSTORE_HDD_ROOT";
const DEFAULT_SSD_ROOT: &str = "/srv/dasobjectstore/ssd";
const DEFAULT_HDD_ROOT: &str = "/srv/dasobjectstore/hdd";
const HDD_SETTLEMENT_QUEUE_CAPACITY: usize = 4;
const SSD_FLUSH_QUEUE_CAPACITY: usize = 2;
const MAX_HDD_SETTLEMENT_WORKERS: usize = 32;

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
    executor.submit(request, accepted_at_utc, progress)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FileIngestEntry {
    source_path: PathBuf,
    relative_path: PathBuf,
    object_id: ObjectId,
    size_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ResolvedIngestEndpoint {
    endpoint_name: String,
    endpoint_kind: &'static str,
    store: StoreServiceDefinition,
    object_prefix: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LocalFileIngestExecutor {
    ssd_root: PathBuf,
    hdd_root: PathBuf,
    authoritative_policy_path: PathBuf,
    store_registry_path: PathBuf,
    subobject_registry_path: PathBuf,
}

impl LocalFileIngestExecutor {
    fn from_environment() -> Self {
        Self {
            ssd_root: default_ssd_root(),
            hdd_root: default_hdd_root(),
            authoritative_policy_path: default_authoritative_policy_path(),
            store_registry_path: default_store_registry_path(),
            subobject_registry_path: default_subobject_registry_path(),
        }
    }

    fn submit(
        &self,
        request: SubmitIngestFilesRequest,
        accepted_at_utc: &str,
        progress: impl FnMut(DaemonIngestProgressEvent) -> Result<(), DaemonIngestFilesRuntimeError>,
    ) -> Result<SubmitIngestFilesResponse, DaemonIngestFilesRuntimeError> {
        let job_id = ingest_job_id(accepted_at_utc)?;
        let summary = self.execute(request, &job_id, progress)?;
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
        let files = collect_ingest_files(&request.source_path, &endpoint.object_prefix)?;
        let source_bytes = files.iter().map(|entry| entry.size_bytes).sum::<u64>();
        let total_work_bytes = source_bytes.saturating_mul(u64::from(copies) + 1);
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
            resource_policy: None,
            message: Some(format!(
                "planned {} file(s), {} source byte(s), {} copy/copies",
                files.len(),
                source_bytes,
                copies
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
                resource_policy: None,
                message: Some("dry run complete; no files imported".to_string()),
            })?;
            return Ok(summary);
        }

        let ingest_policy = read_ingest_policy(&self.authoritative_policy_path)?;
        let hdd_worker_count = ingest_policy
            .hdd_settlement_concurrency
            .min(managed_disk_roots.len())
            .clamp(1, MAX_HDD_SETTLEMENT_WORKERS);
        let mut state = PipelineProgressState::new(
            files.len() as u64,
            source_bytes,
            total_work_bytes,
            hdd_worker_count as u16,
        );
        let capacity_policy = SsdCapacityPolicy::default();
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
            wait_for_ssd_admission(
                &self.ssd_root,
                &capacity_policy,
                &event_rx,
                &mut state,
                job_id,
                &request.endpoint,
                &mut progress,
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
            )?;
            drain_hdd_settlement_events(
                &event_rx,
                &mut state,
                job_id,
                &request.endpoint,
                &mut progress,
                false,
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
            resource_policy: None,
            message: Some("file ingest complete".to_string()),
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
    staged: StagedObjectPut,
}

#[derive(Clone, Debug)]
struct HddSettlementDiskState {
    disk_id: DiskId,
    root_path: PathBuf,
    active: bool,
    total_bytes: u64,
    available_bytes: u64,
    assigned_bytes: u64,
}

#[derive(Debug)]
struct HddSettlementScheduler {
    disks: Vec<HddSettlementDiskState>,
}

type SharedHddSettlementScheduler = Arc<(Mutex<HddSettlementScheduler>, Condvar)>;

impl HddSettlementScheduler {
    fn new(roots: &[DiskCopyRoot]) -> Result<Self, DaemonIngestFilesRuntimeError> {
        Ok(Self {
            disks: roots
                .iter()
                .map(|root| {
                    let capacity = measure_ssd_capacity(&root.root_path)?;
                    Ok(HddSettlementDiskState {
                        disk_id: root.disk_id.clone(),
                        root_path: root.root_path.clone(),
                        active: false,
                        total_bytes: capacity.total_bytes,
                        available_bytes: capacity.available_bytes,
                        assigned_bytes: 0,
                    })
                })
                .collect::<Result<Vec<_>, DaemonIngestFilesRuntimeError>>()?,
        })
    }

    fn reserve_roots(
        &mut self,
        copy_count: usize,
        object_size_bytes: u64,
    ) -> Result<Option<Vec<DiskCopyRoot>>, DaemonIngestFilesRuntimeError> {
        let eligible_count = self
            .disks
            .iter()
            .filter(|disk| disk.projected_available_bytes() >= object_size_bytes)
            .count();
        if eligible_count < copy_count {
            return Err(DaemonIngestFilesRuntimeError::CommandFailed(format!(
                "HDD settlement needs {copy_count} disk(s) with at least {object_size_bytes} byte(s) free; found {eligible_count}"
            )));
        }

        let mut candidates = self
            .disks
            .iter()
            .enumerate()
            .filter(|(_, disk)| {
                !disk.active && disk.projected_available_bytes() >= object_size_bytes
            })
            .collect::<Vec<_>>();
        if candidates.len() < copy_count {
            return Ok(None);
        }
        candidates.sort_by(|(_, left), (_, right)| compare_hdd_settlement_disks(right, left));
        let selected = candidates
            .into_iter()
            .take(copy_count)
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let mut roots = Vec::with_capacity(copy_count);
        for index in selected {
            let disk = &mut self.disks[index];
            disk.active = true;
            roots.push(DiskCopyRoot::new(
                disk.disk_id.clone(),
                disk.root_path.clone(),
            ));
        }
        Ok(Some(roots))
    }

    fn release_roots(&mut self, roots: &[DiskCopyRoot], bytes_per_root: u64) {
        for root in roots {
            if let Some(disk) = self
                .disks
                .iter_mut()
                .find(|disk| disk.disk_id == root.disk_id)
            {
                disk.active = false;
                disk.assigned_bytes = disk.assigned_bytes.saturating_add(bytes_per_root);
            }
        }
    }
}

impl HddSettlementDiskState {
    fn projected_available_bytes(&self) -> u64 {
        self.available_bytes.saturating_sub(self.assigned_bytes)
    }
}

fn new_shared_hdd_settlement_scheduler(
    roots: &[DiskCopyRoot],
) -> Result<SharedHddSettlementScheduler, DaemonIngestFilesRuntimeError> {
    Ok(Arc::new((
        Mutex::new(HddSettlementScheduler::new(roots)?),
        Condvar::new(),
    )))
}

fn reserve_hdd_settlement_roots(
    scheduler: &SharedHddSettlementScheduler,
    copy_count: usize,
    object_size_bytes: u64,
) -> Result<Vec<DiskCopyRoot>, DaemonIngestFilesRuntimeError> {
    let (lock, condvar) = &**scheduler;
    let mut scheduler = lock.lock().map_err(|_| {
        DaemonIngestFilesRuntimeError::CommandFailed(
            "HDD settlement scheduler lock poisoned".to_string(),
        )
    })?;
    loop {
        if let Some(roots) = scheduler.reserve_roots(copy_count, object_size_bytes)? {
            return Ok(roots);
        }
        scheduler = condvar.wait(scheduler).map_err(|_| {
            DaemonIngestFilesRuntimeError::CommandFailed(
                "HDD settlement scheduler lock poisoned".to_string(),
            )
        })?;
    }
}

fn release_hdd_settlement_roots(
    scheduler: &SharedHddSettlementScheduler,
    roots: &[DiskCopyRoot],
    bytes_per_root: u64,
) -> Result<(), DaemonIngestFilesRuntimeError> {
    let (lock, condvar) = &**scheduler;
    let mut scheduler = lock.lock().map_err(|_| {
        DaemonIngestFilesRuntimeError::CommandFailed(
            "HDD settlement scheduler lock poisoned".to_string(),
        )
    })?;
    scheduler.release_roots(roots, bytes_per_root);
    condvar.notify_all();
    Ok(())
}

fn compare_hdd_settlement_disks(
    left: &HddSettlementDiskState,
    right: &HddSettlementDiskState,
) -> std::cmp::Ordering {
    let left_free = left.projected_available_bytes();
    let right_free = right.projected_available_bytes();
    (u128::from(left_free) * u128::from(right.total_bytes.max(1)))
        .cmp(&(u128::from(right_free) * u128::from(left.total_bytes.max(1))))
        .then_with(|| left_free.cmp(&right_free))
        .then_with(|| right.disk_id.cmp(&left.disk_id))
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
    },
    Progress {
        entry: FileIngestEntry,
        progress: ObjectPutProgress,
    },
    Settled {
        entry: FileIngestEntry,
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
    progress_offsets: BTreeMap<(ObjectId, String), u64>,
}

impl PipelineProgressState {
    fn new(
        total_files: u64,
        source_bytes_total: u64,
        work_bytes_total: u64,
        hdd_worker_count: u16,
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
            progress_offsets: BTreeMap::new(),
        }
    }

    fn apply_object_progress(&mut self, entry: &FileIngestEntry, progress: &ObjectPutProgress) {
        let key = (
            progress.object_id.clone(),
            object_progress_stage_key(progress),
        );
        let previous = *self.progress_offsets.get(&key).unwrap_or(&0);
        let current = progress.bytes_written;
        let delta = current.saturating_sub(previous);
        self.progress_offsets.insert(key, current);
        self.completed_work_bytes = self.completed_work_bytes.saturating_add(delta);
        if matches!(progress.stage, ObjectPutProgressStage::SsdIngest) {
            let previous_source = previous.min(entry.size_bytes);
            let current_source = current.min(entry.size_bytes);
            self.completed_source_bytes = self
                .completed_source_bytes
                .saturating_add(current_source.saturating_sub(previous_source));
        }
    }

    fn source_pending(&self) -> u32 {
        self.total_files
            .saturating_sub(self.staged_files)
            .saturating_sub(u64::from(self.ssd_active))
            .min(u64::from(u32::MAX)) as u32
    }

    fn telemetry(&self) -> DaemonIngestTelemetry {
        let mut telemetry = DaemonIngestTelemetry::default();
        telemetry.queue_depths = DaemonIngestQueueDepths {
            source_read: self.source_pending(),
            hdd_write: self.hdd_queued,
            ..DaemonIngestQueueDepths::default()
        };
        telemetry.workers = DaemonIngestWorkerTelemetry {
            ssd_stage: DaemonIngestWorkerActivity {
                active: self.ssd_active,
                idle: u16::from(self.ssd_active == 0),
            },
            hdd_write: DaemonIngestWorkerActivity {
                active: self.hdd_active,
                idle: self.hdd_worker_count.saturating_sub(self.hdd_active),
            },
            ..DaemonIngestWorkerTelemetry::default()
        };
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
                            staged,
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
                    work.staged.copy_count as usize,
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
                });
                let entry = work.entry.clone();
                let mut staged = work.staged;
                staged.disk_roots = roots.clone();
                let result =
                    settle_staged_object_to_hdd_with_controlled_progress(staged, |progress| {
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
                    Ok(_report) => {
                        let _ = event_tx.send(HddSettlementEvent::Settled { entry: work.entry });
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

fn enqueue_ssd_flush_work(
    flush_tx: &mpsc::SyncSender<SsdFlushWork>,
    mut work: SsdFlushWork,
    event_rx: &mpsc::Receiver<HddSettlementEvent>,
    state: &mut PipelineProgressState,
    job_id: &IngestJobId,
    endpoint: &StoreId,
    progress: &mut impl FnMut(DaemonIngestProgressEvent) -> Result<(), DaemonIngestFilesRuntimeError>,
) -> Result<(), DaemonIngestFilesRuntimeError> {
    loop {
        match flush_tx.try_send(work) {
            Ok(()) => return Ok(()),
            Err(mpsc::TrySendError::Full(returned_work)) => {
                work = returned_work;
                drain_hdd_settlement_events(event_rx, state, job_id, endpoint, progress, true)?;
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                return Err(DaemonIngestFilesRuntimeError::CommandFailed(
                    "SSD flush worker stopped before accepting staged object".to_string(),
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
                    resource_policy: None,
                    message: Some(format!(
                        "SSD pressure {:?}; pausing source ingress while HDD settlement drains",
                        state.ssd_pressure
                    )),
                })?;
                drain_hdd_settlement_events(event_rx, state, job_id, endpoint, progress, true)?;
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

fn drain_hdd_settlement_events(
    event_rx: &mpsc::Receiver<HddSettlementEvent>,
    state: &mut PipelineProgressState,
    job_id: &IngestJobId,
    endpoint: &StoreId,
    progress: &mut impl FnMut(DaemonIngestProgressEvent) -> Result<(), DaemonIngestFilesRuntimeError>,
    block: bool,
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
                    current_object_id: Some(entry.object_id),
                    ssd_pressure: Some(state.ssd_pressure),
                    telemetry: Some(state.telemetry()),
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
                    current_object_id: Some(entry.object_id),
                    ssd_pressure: Some(state.ssd_pressure),
                    telemetry: Some(state.telemetry()),
                    resource_policy: None,
                    message: Some(format!(
                        "SSD payload synced and queued for HDD settlement: {}",
                        entry.relative_path.to_string_lossy()
                    )),
                })?;
            }
            HddSettlementEvent::Started { entry } => {
                state.hdd_queued = state.hdd_queued.saturating_sub(1);
                state.hdd_active = state.hdd_active.saturating_add(1);
                progress(DaemonIngestProgressEvent {
                    job_id: job_id.clone(),
                    endpoint: endpoint.clone(),
                    stage: DaemonIngestStage::HddCopy {
                        disk_id: DiskId::new("pending").expect("valid pending disk id"),
                        copy_number: 1,
                    },
                    pipeline_stage: Some(DaemonIngestPipelineStage::HddWrite),
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
                    resource_policy: None,
                    message: Some(format!(
                        "HDD settlement started: {}",
                        entry.relative_path.to_string_lossy()
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
            HddSettlementEvent::Settled { entry } => {
                state.hdd_active = state.hdd_active.saturating_sub(1);
                state.completed_files = state.completed_files.saturating_add(1);
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
                    current_object_id: Some(entry.object_id),
                    ssd_pressure: Some(state.ssd_pressure),
                    telemetry: Some(state.telemetry()),
                    resource_policy: None,
                    message: Some(format!(
                        "file settled: {}",
                        entry.relative_path.to_string_lossy()
                    )),
                })?;
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
    }
}

pub(crate) fn default_ssd_root() -> PathBuf {
    std::env::var_os(SSD_ROOT_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_SSD_ROOT))
}

pub(crate) fn default_hdd_root() -> PathBuf {
    std::env::var_os(HDD_ROOT_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_HDD_ROOT))
}

fn default_authoritative_policy_path() -> PathBuf {
    authoritative_performance_recommendation_path(DEFAULT_DAEMON_STATE_DIR)
}

fn read_ingest_policy(
    path: &Path,
) -> Result<AuthoritativeIngestPolicy, DaemonIngestFilesRuntimeError> {
    read_authoritative_ingest_policy(path)
        .map_err(|err| DaemonIngestFilesRuntimeError::CommandFailed(err.to_string()))
        .map(|policy| policy.unwrap_or_default())
}

fn validate_known_ssd_root(path: &Path) -> Result<(), DaemonIngestFilesRuntimeError> {
    let marker = read_device_marker(path).map_err(|err| {
        DaemonIngestFilesRuntimeError::CommandFailed(format!(
            "{} is not a known DASObjectStore SSD root: {err}",
            path.display()
        ))
    })?;
    if !marker.lines().any(|line| line == "role=ssd") {
        return Err(DaemonIngestFilesRuntimeError::CommandFailed(format!(
            "{} is not a DASObjectStore SSD root; expected role=ssd in .dasobjectstore/device.env",
            path.display()
        )));
    }

    Ok(())
}

fn read_device_marker(path: &Path) -> Result<String, io::Error> {
    fs::read_to_string(path.join(".dasobjectstore").join("device.env"))
}

pub(crate) fn discover_managed_hdd_roots(
    hdd_root: &Path,
) -> Result<Vec<DiskCopyRoot>, DaemonIngestFilesRuntimeError> {
    let mut roots = Vec::new();
    let entries = match fs::read_dir(hdd_root) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(roots),
        Err(err) => return Err(DaemonIngestFilesRuntimeError::Io(err)),
    };

    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let root_path = entry.path();
        let marker = match read_device_marker(&root_path) {
            Ok(marker) => marker,
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => return Err(DaemonIngestFilesRuntimeError::Io(err)),
        };
        let Some(disk_id) = hdd_disk_id_from_marker(&marker)? else {
            continue;
        };
        roots.push(DiskCopyRoot::new(disk_id, root_path));
    }

    roots.sort_by(|left, right| left.disk_id.cmp(&right.disk_id));
    Ok(roots)
}

fn hdd_disk_id_from_marker(marker: &str) -> Result<Option<DiskId>, DaemonIngestFilesRuntimeError> {
    for line in marker.lines() {
        let Some(role) = line.strip_prefix("role=") else {
            continue;
        };
        let Some(disk_id) = role.strip_prefix("hdd:") else {
            return Ok(None);
        };
        return DiskId::new(disk_id)
            .map(Some)
            .map_err(|err| DaemonIngestFilesRuntimeError::CommandFailed(err.to_string()));
    }

    Ok(None)
}

fn resolve_ingest_endpoint(
    endpoint: &StoreId,
    store_registry_path: &Path,
    subobject_registry_path: &Path,
) -> Result<ResolvedIngestEndpoint, DaemonIngestFilesRuntimeError> {
    let stores = read_store_registry(store_registry_path)?;
    let store_match = stores
        .iter()
        .find(|definition| definition.store_id == *endpoint);
    let subobjects = read_subobject_registry(subobject_registry_path)?;
    let subobject_match = subobjects
        .iter()
        .find(|definition| definition.name == endpoint.as_str());

    if store_match.is_some() && subobject_match.is_some() {
        return Err(DaemonIngestFilesRuntimeError::CommandFailed(format!(
            "ingest endpoint {} is ambiguous; both an object store and a SubObject use that name",
            endpoint
        )));
    }

    if let Some(store) = store_match {
        return Ok(ResolvedIngestEndpoint {
            endpoint_name: endpoint.as_str().to_string(),
            endpoint_kind: "object_store",
            store: store.clone(),
            object_prefix: store.store_id.as_str().to_string(),
        });
    }

    if let Some(subobject) = subobject_match {
        let store = stores
            .iter()
            .find(|definition| definition.store_id == subobject.store_id)
            .ok_or_else(|| {
                DaemonIngestFilesRuntimeError::CommandFailed(format!(
                    "SubObject {} references missing store {} in {}",
                    subobject.name,
                    subobject.store_id,
                    store_registry_path.display()
                ))
            })?;
        return Ok(ResolvedIngestEndpoint {
            endpoint_name: subobject.name.clone(),
            endpoint_kind: "subobject",
            store: store.clone(),
            object_prefix: subobject.object_prefix(),
        });
    }

    Err(DaemonIngestFilesRuntimeError::CommandFailed(format!(
        "ingest endpoint {} was not found in {} or {}",
        endpoint,
        store_registry_path.display(),
        subobject_registry_path.display()
    )))
}

fn collect_ingest_files(
    root: &Path,
    object_prefix: &str,
) -> Result<Vec<FileIngestEntry>, DaemonIngestFilesRuntimeError> {
    if !root.is_dir() {
        return Err(DaemonIngestFilesRuntimeError::CommandFailed(format!(
            "ingest source must be a directory: {}",
            root.display()
        )));
    }

    let mut files = Vec::new();
    collect_ingest_files_into(root, root, object_prefix, &mut files)?;
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));

    Ok(files)
}

fn collect_ingest_files_into(
    root: &Path,
    current: &Path,
    object_prefix: &str,
    files: &mut Vec<FileIngestEntry>,
) -> Result<(), DaemonIngestFilesRuntimeError> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        if is_hidden_entry_name(&entry.file_name()) {
            continue;
        }
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_ingest_files_into(root, &path, object_prefix, files)?;
        } else if file_type.is_file() {
            let metadata = entry.metadata()?;
            let relative_path = path
                .strip_prefix(root)
                .map_err(|err| DaemonIngestFilesRuntimeError::CommandFailed(err.to_string()))?
                .to_path_buf();
            files.push(FileIngestEntry {
                object_id: object_id_for_ingested_file(object_prefix, &relative_path)?,
                source_path: path,
                relative_path,
                size_bytes: metadata.len(),
            });
        }
    }

    Ok(())
}

fn is_hidden_entry_name(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().starts_with('.')
}

fn object_id_for_ingested_file(
    object_prefix: &str,
    relative_path: &Path,
) -> Result<ObjectId, DaemonIngestFilesRuntimeError> {
    let relative = relative_path
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    ObjectId::new(format!("{object_prefix}/{relative}"))
        .map_err(|err| DaemonIngestFilesRuntimeError::CommandFailed(err.to_string()))
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
        collect_ingest_files, sync_pending_ssd_stage, FileIngestEntry, HddSettlementDiskState,
        HddSettlementScheduler, LocalFileIngestExecutor, PendingSsdStage, PipelineProgressState,
        SSD_ROOT_ENV,
    };
    use crate::api::{DaemonIngestConflictPolicy, DaemonSsdPressure, SubmitIngestFilesRequest};
    use dasobjectstore_core::ids::{IngestJobId, ObjectId, StoreId};
    use dasobjectstore_core::object_type::ObjectType;
    use dasobjectstore_core::store::{StoreClass, StorePolicy};
    use dasobjectstore_metadata::{
        IngestStagingLayout, ObjectPutProgress, ObjectPutProgressStage, ObjectPutRequest,
    };
    use dasobjectstore_object_service::StoreServiceDefinition;
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
            authoritative_policy_path: root.join("missing-authoritative-policy.json"),
            store_registry_path: registry_path,
            subobject_registry_path,
        };

        let mut progress_events = Vec::new();
        let response = executor
            .submit(
                SubmitIngestFilesRequest {
                    endpoint: StoreId::new("zymo_fecal_2025.05").expect("store id"),
                    source_path: source_root,
                    object_type: ObjectType::Fastq,
                    copies: Some(1),
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
        let mut state = PipelineProgressState::new(10, 1_000, 2_000, 3);
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
                stage: ObjectPutProgressStage::HddCopy {
                    disk_id: "disk-a".to_string(),
                    copy_number: 1,
                },
                bytes_written: 25,
            },
        );

        let telemetry = state.telemetry();
        assert_eq!(state.completed_source_bytes, 40);
        assert_eq!(state.completed_work_bytes, 65);
        assert_eq!(telemetry.queue_depths.source_read, 6);
        assert_eq!(telemetry.queue_depths.hdd_write, 2);
        assert_eq!(telemetry.workers.ssd_stage.active, 1);
        assert_eq!(telemetry.workers.hdd_write.active, 1);
        assert_eq!(telemetry.workers.hdd_write.idle, 2);
        assert_eq!(telemetry.pressure.ssd, DaemonSsdPressure::High);
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
        let definition = StoreServiceDefinition {
            store_id: StoreId::new("zymo_fecal_2025.05").expect("store id"),
            policy: StorePolicy::defaults_for(StoreClass::ReproducibleCache),
            bucket_name: Some("dos-zymo-fecal-2025-05".to_string()),
            reader_group: None,
            writer_group: None,
            public: false,
        };
        let json = serde_json::to_string_pretty(&vec![definition]).expect("store registry json");
        fs::write(path, json).expect("store registry");
    }

    fn temp_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!("{label}-{}-{nanos}", std::process::id()))
    }
}
