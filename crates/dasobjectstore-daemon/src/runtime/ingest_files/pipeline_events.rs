use super::*;

pub(super) fn drain_hdd_settlement_events(
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

pub(super) fn object_progress_event(
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

pub(super) fn object_progress_stage_key(progress: &ObjectPutProgress) -> String {
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

pub(super) fn daemon_stage_for_object_progress(progress: &ObjectPutProgress) -> DaemonIngestStage {
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

pub(super) fn pipeline_stage_for_object_progress(
    progress: &ObjectPutProgress,
) -> DaemonIngestPipelineStage {
    match progress.stage {
        ObjectPutProgressStage::SsdIngest => DaemonIngestPipelineStage::SsdStage,
        ObjectPutProgressStage::SsdFlush => DaemonIngestPipelineStage::SsdFlush,
        ObjectPutProgressStage::HddCopy { .. } => DaemonIngestPipelineStage::HddWrite,
        ObjectPutProgressStage::HddFsync { .. } => DaemonIngestPipelineStage::HddFsync,
        ObjectPutProgressStage::HddRename { .. } => DaemonIngestPipelineStage::HddRename,
    }
}

pub(super) fn stage_message_for_object_progress(
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
