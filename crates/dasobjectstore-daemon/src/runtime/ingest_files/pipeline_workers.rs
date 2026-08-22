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
    live_sqlite_path: PathBuf,
    ingest_job_id: IngestJobId,
    recorded_at_utc: String,
) -> Vec<thread::JoinHandle<()>> {
    let settle_rx = Arc::new(Mutex::new(settle_rx));
    (0..worker_count.max(1))
        .map(|_| {
            let settle_rx = Arc::clone(&settle_rx);
            let event_tx = event_tx.clone();
            let scheduler = Arc::clone(&scheduler);
            let live_sqlite_path = live_sqlite_path.clone();
            let ingest_job_id = ingest_job_id.clone();
            let recorded_at_utc = recorded_at_utc.clone();
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
                let claim_owner = format!(
                    "{}:{}",
                    ingest_job_id.as_str(),
                    work.entry.object_id.as_str()
                );
                let allocations = roots
                    .iter()
                    .map(|root| {
                        let capacity = measure_ssd_capacity(&root.root_path)
                            .map_err(|error| error.to_string())?;
                        Ok(DiskCapacityClaimAllocation {
                            disk_id: root.disk_id.clone(),
                            measured_available_bytes: capacity.available_bytes,
                            requested_bytes: work.entry.size_bytes.max(1),
                        })
                    })
                    .collect::<Result<Vec<_>, String>>();
                let claim_result = allocations.and_then(|allocations| {
                    acquire_disk_capacity_claims(&DiskCapacityClaimRequest {
                        live_sqlite_path: live_sqlite_path.clone(),
                        kind: DiskCapacityClaimKind::Ingest,
                        owner_id: claim_owner.clone(),
                        request_id: format!(
                            "ingest:{}:{}",
                            ingest_job_id.as_str(),
                            work.entry.object_id.as_str()
                        ),
                        request_digest: format!(
                            "{}:{}:{}",
                            work.entry.object_id.as_str(),
                            work.entry.size_bytes,
                            work.payload.copy_count()
                        ),
                        lease_owner: Some(ingest_job_id.as_str().to_string()),
                        lease_expires_at_utc: None,
                        created_at_utc: recorded_at_utc.clone(),
                        allocations,
                    })
                    .map_err(|error| error.to_string())
                });
                if let Err(error) = claim_result {
                    let _ = release_hdd_settlement_roots(&scheduler, &roots, 0);
                    let _ = event_tx.send(HddSettlementEvent::Failed {
                        error: ObjectPutError::Io(io::Error::other(error)),
                    });
                    break;
                }
                if event_tx
                    .send(HddSettlementEvent::Started {
                        entry: work.entry.clone(),
                        roots: roots.clone(),
                    })
                    .is_err()
                {
                    let _ = release_hdd_settlement_roots(&scheduler, &roots, 0);
                    let _ = release_disk_capacity_claims(
                        &live_sqlite_path,
                        DiskCapacityClaimKind::Ingest,
                        &claim_owner,
                        &recorded_at_utc,
                    );
                    break;
                }
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
                    let release_error = release_disk_capacity_claims(
                        &live_sqlite_path,
                        DiskCapacityClaimKind::Ingest,
                        &claim_owner,
                        &recorded_at_utc,
                    )
                    .err();
                    let _ = event_tx.send(HddSettlementEvent::Failed {
                        error: ObjectPutError::Io(io::Error::other(match release_error {
                            Some(release_error) => format!(
                                "{error}; failed to release stopped ingest capacity claim: {release_error}"
                            ),
                            None => error.to_string(),
                        })),
                    });
                    break;
                }
                match result {
                    Ok(report) => {
                        if event_tx
                            .send(HddSettlementEvent::Settled {
                                entry: work.entry,
                                report,
                            })
                            .is_err()
                        {
                            let _ = release_disk_capacity_claims(
                                &live_sqlite_path,
                                DiskCapacityClaimKind::Ingest,
                                &claim_owner,
                                &recorded_at_utc,
                            );
                            break;
                        }
                    }
                    Err(error) => {
                        let release_error = release_disk_capacity_claims(
                            &live_sqlite_path,
                            DiskCapacityClaimKind::Ingest,
                            &claim_owner,
                            &recorded_at_utc,
                        )
                        .err();
                        let error = match release_error {
                            Some(release_error) => ObjectPutError::Io(io::Error::other(format!(
                                "{error}; failed to release stopped ingest capacity claim: {release_error}"
                            ))),
                            None => error,
                        };
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
        }
        HddSettlementPayload::Direct(request) => {
            put_object_direct_to_hdd_with_controlled_progress(request, progress)
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

#[cfg(test)]
mod tests {
    use super::*;
    use dasobjectstore_metadata::{read_outstanding_disk_capacity, LIVE_SCHEMA_SQL};
    use rusqlite::Connection;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn disconnected_requester_releases_acquired_hdd_capacity_claim() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-disconnected-ingest-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let hdd_root = root.join("hdd-a");
        fs::create_dir_all(&hdd_root).expect("HDD root");
        let live_sqlite_path = root.join("live.sqlite");
        let connection = Connection::open(&live_sqlite_path).expect("live metadata");
        connection.execute_batch(LIVE_SCHEMA_SQL).expect("schema");
        connection
            .execute(
                "INSERT INTO pools (pool_id,state,created_at_utc,updated_at_utc)
                 VALUES ('pool-a','Clean','2026-07-25T00:00:00Z','2026-07-25T00:00:00Z')",
                [],
            )
            .expect("pool");
        connection
            .execute(
                "INSERT INTO disks (
                    disk_id,pool_id,role,state,created_at_utc,updated_at_utc
                 ) VALUES (
                    'disk-a','pool-a','hdd_capacity','Healthy',
                    '2026-07-25T00:00:00Z','2026-07-25T00:00:00Z'
                 )",
                [],
            )
            .expect("disk");
        drop(connection);

        let roots = vec![DiskCopyRoot::new(
            DiskId::new("disk-a").expect("disk id"),
            &hdd_root,
        )];
        let scheduler = new_shared_hdd_settlement_scheduler_with_claims(&roots, &BTreeMap::new())
            .expect("scheduler");
        let source_path = root.join("source.bin");
        fs::write(&source_path, b"data").expect("source");
        let object_id = ObjectId::new("store-a/source.bin").expect("object id");
        let (settle_tx, settle_rx) = mpsc::sync_channel(1);
        let (event_tx, event_rx) = mpsc::channel();
        drop(event_rx);
        let workers = spawn_hdd_settlement_workers(
            settle_rx,
            event_tx,
            1,
            scheduler,
            live_sqlite_path.clone(),
            IngestJobId::new("ingest-files-disconnected").expect("job id"),
            "2026-07-25T00:00:00Z".to_string(),
        );
        settle_tx
            .send(HddSettlementWork {
                entry: FileIngestEntry {
                    source_path: source_path.clone(),
                    relative_path: PathBuf::from("source.bin"),
                    object_id: object_id.clone(),
                    size_bytes: 4,
                    file_index: 1,
                },
                payload: HddSettlementPayload::Direct(DirectObjectPutRequest::new(
                    object_id,
                    source_path,
                    Vec::new(),
                    1,
                )),
            })
            .expect("work accepted");
        drop(settle_tx);
        for worker in workers {
            worker.join().expect("worker exits");
        }

        assert!(read_outstanding_disk_capacity(&live_sqlite_path)
            .expect("capacity claims")
            .is_empty());
        let state: String = Connection::open(&live_sqlite_path)
            .expect("metadata")
            .query_row(
                "SELECT state FROM disk_capacity_claims WHERE claim_kind='ingest'",
                [],
                |row| row.get(0),
            )
            .expect("released claim remains auditable");
        assert_eq!(state, "released");
        fs::remove_dir_all(root).expect("cleanup");
    }
}
