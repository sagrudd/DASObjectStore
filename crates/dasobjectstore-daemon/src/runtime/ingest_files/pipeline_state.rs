use super::*;

#[derive(Debug)]
pub(super) struct PendingSsdStage {
    pub(super) request: ObjectPutRequest,
    pub(super) job_paths: IngestJobPaths,
    pub(super) write_report: IngestWriteReport,
}

#[derive(Debug)]
pub(super) struct SsdFlushWork {
    pub(super) entry: FileIngestEntry,
    pub(super) pending: PendingSsdStage,
}
#[derive(Debug)]
pub(super) struct HddSettlementWork {
    pub(super) entry: FileIngestEntry,
    pub(super) payload: HddSettlementPayload,
}

#[derive(Debug)]
pub(super) enum HddSettlementPayload {
    Staged(StagedObjectPut),
    Direct(DirectObjectPutRequest),
}

impl HddSettlementPayload {
    pub(super) fn copy_count(&self) -> u8 {
        match self {
            Self::Staged(staged) => staged.copy_count,
            Self::Direct(request) => request.copy_count,
        }
    }

    pub(super) fn set_disk_roots(&mut self, roots: Vec<DiskCopyRoot>) {
        match self {
            Self::Staged(staged) => staged.disk_roots = roots,
            Self::Direct(request) => request.disk_roots = roots,
        }
    }
}

#[derive(Debug)]
pub(super) enum HddSettlementEvent {
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
pub(super) struct PipelineProgressState {
    pub(super) total_files: u64,
    pub(super) source_bytes_total: u64,
    pub(super) work_bytes_total: u64,
    pub(super) completed_files: u64,
    pub(super) staged_files: u64,
    pub(super) completed_source_bytes: u64,
    pub(super) completed_work_bytes: u64,
    pub(super) ssd_active: u16,
    pub(super) hdd_active: u16,
    pub(super) hdd_worker_count: u16,
    pub(super) hdd_queued: u32,
    pub(super) ssd_pressure: DaemonSsdPressure,
    pub(super) count_hdd_copy_as_source: bool,
    pub(super) progress_offsets: BTreeMap<(ObjectId, String), u64>,
    pub(super) active_hdd_transfers: BTreeMap<(ObjectId, DiskId, u8), ActiveHddTransferState>,
    pub(super) source_read_rate: SampledRate,
    pub(super) ssd_write_rate: SampledRate,
}

#[derive(Debug, Default)]
pub(super) struct SampledRate {
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
pub(super) struct ActiveHddTransferState {
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
    pub(super) fn new(
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

    pub(super) fn apply_object_progress(
        &mut self,
        entry: &FileIngestEntry,
        progress: &ObjectPutProgress,
    ) {
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

    pub(super) fn update_hdd_transfer(
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

    pub(super) fn remove_active_hdd_transfers_for_entry(&mut self, entry: &FileIngestEntry) {
        self.active_hdd_transfers
            .retain(|(object_id, _, _), _| object_id != &entry.object_id);
    }

    pub(super) fn mark_existing_object_skipped(&mut self, entry: &FileIngestEntry, copies: u8) {
        self.completed_files = self.completed_files.saturating_add(1);
        self.staged_files = self.staged_files.saturating_add(1);
        self.completed_source_bytes = self.completed_source_bytes.saturating_add(entry.size_bytes);
        self.completed_work_bytes = self
            .completed_work_bytes
            .saturating_add(entry.size_bytes.saturating_mul(u64::from(copies) + 1));
        self.remove_active_hdd_transfers_for_entry(entry);
    }

    pub(super) fn active_hdd_transfer_records(&self) -> Vec<DaemonIngestHddActiveTransfer> {
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

    pub(super) fn source_pending(&self) -> u32 {
        self.total_files
            .saturating_sub(self.staged_files)
            .saturating_sub(u64::from(self.ssd_active))
            .min(u64::from(u32::MAX)) as u32
    }

    pub(super) fn telemetry(&self) -> DaemonIngestTelemetry {
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
