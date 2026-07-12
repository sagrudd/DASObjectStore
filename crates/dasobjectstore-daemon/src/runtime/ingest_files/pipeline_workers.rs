use super::*;

pub(super) fn spawn_ssd_flush_worker(
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

pub(super) fn sync_pending_ssd_stage(
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

pub(super) fn spawn_hdd_settlement_workers(
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

pub(super) fn settle_hdd_payload_with_controlled_progress(
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

pub(super) fn enqueue_ssd_flush_work(
    flush_tx: &mpsc::SyncSender<SsdFlushWork>,
    mut work: SsdFlushWork,
    event_rx: &mpsc::Receiver<HddSettlementEvent>,
    state: &mut PipelineProgressState,
    job_id: &IngestJobId,
    endpoint: &StoreId,
    progress: &mut impl FnMut(DaemonIngestProgressEvent) -> Result<(), DaemonIngestFilesRuntimeError>,
    live_sqlite_path: &Path,
    recorded_at_utc: &str,
    capacity_reservations: &mut IngestCapacityReservations,
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
                    capacity_reservations,
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

pub(super) fn enqueue_hdd_settlement_work(
    settle_tx: &mpsc::SyncSender<HddSettlementWork>,
    mut work: HddSettlementWork,
    event_rx: &mpsc::Receiver<HddSettlementEvent>,
    state: &mut PipelineProgressState,
    job_id: &IngestJobId,
    endpoint: &StoreId,
    progress: &mut impl FnMut(DaemonIngestProgressEvent) -> Result<(), DaemonIngestFilesRuntimeError>,
    live_sqlite_path: &Path,
    recorded_at_utc: &str,
    capacity_reservations: &mut IngestCapacityReservations,
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
                    capacity_reservations,
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

pub(super) fn wait_for_ssd_admission(
    ssd_root: &Path,
    capacity_policy: &SsdCapacityPolicy,
    event_rx: &mpsc::Receiver<HddSettlementEvent>,
    state: &mut PipelineProgressState,
    job_id: &IngestJobId,
    endpoint: &StoreId,
    progress: &mut impl FnMut(DaemonIngestProgressEvent) -> Result<(), DaemonIngestFilesRuntimeError>,
    live_sqlite_path: &Path,
    recorded_at_utc: &str,
    capacity_reservations: &mut IngestCapacityReservations,
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
                    capacity_reservations,
                )?;
            }
        }
    }
}
