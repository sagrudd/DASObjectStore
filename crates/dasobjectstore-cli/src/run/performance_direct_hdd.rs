use super::*;

pub(super) fn benchmark_direct_hdd(
    hdd_bench_roots: &[(DiskId, PathBuf)],
    workload: &PerformanceWorkload,
    concurrency: usize,
    redundancy: usize,
    writer: &mut impl Write,
    log_progress: bool,
    tui_context: Option<PerformanceTuiContext<'_>>,
) -> Result<PerformanceScenarioResult, CliError> {
    let started = Instant::now();
    let io_sampler = PerformanceIoSampler::start(performance_io_devices(None, hdd_bench_roots));
    let queue_capacity = hdd_queue_capacity(concurrency, redundancy);
    let scheduler = new_shared_disk_placement_scheduler(hdd_bench_roots)?;
    let (sender, receiver) = mpsc::sync_channel::<DirectHddJob>(queue_capacity);
    let receiver = Arc::new(Mutex::new(receiver));
    let worker_results = Arc::new(Mutex::new(Vec::<PerformanceDiskResult>::new()));
    let hdd_jobs_started = Arc::new(AtomicU32::new(0));
    let hdd_jobs_completed = Arc::new(AtomicU32::new(0));
    let hdd_bytes_transferred = Arc::new(AtomicU64::new(0));
    let live_rates = PerformanceLiveRateCounters::default();
    let active_hdd_writes = Arc::new(Mutex::new(
        BTreeMap::<ActiveHddWriteKey, ActiveHddWrite>::new(),
    ));
    let mut handles = Vec::new();
    for worker_index in 0..concurrency {
        let receiver = Arc::clone(&receiver);
        let scheduler = Arc::clone(&scheduler);
        let worker_results = Arc::clone(&worker_results);
        let hdd_jobs_started = Arc::clone(&hdd_jobs_started);
        let hdd_jobs_completed = Arc::clone(&hdd_jobs_completed);
        let hdd_bytes_transferred = Arc::clone(&hdd_bytes_transferred);
        let live_rates = live_rates.clone();
        let active_hdd_writes = Arc::clone(&active_hdd_writes);
        handles.push(thread::spawn(move || -> Result<(), CliError> {
            loop {
                check_performance_cancelled()?;
                let payload = {
                    let receiver = receiver.lock().map_err(|_| {
                        CliError::CommandFailed(
                            "performance-test direct HDD queue lock poisoned".to_string(),
                        )
                    })?;
                    receiver.recv()
                };
                let Ok(job) = payload else {
                    break;
                };
                hdd_jobs_started.fetch_add(1, Ordering::SeqCst);
                let placement =
                    reserve_performance_disk_for_file(&scheduler, job.payload.file_index)?;
                let destination = placement
                    .root_path
                    .join("direct-hdd")
                    .join(format!("c{concurrency}"))
                    .join(&job.payload.relative_path);
                let active_key = (job.payload.file_index, job.copy_index);
                active_hdd_writes
                    .lock()
                    .map_err(|_| {
                        CliError::CommandFailed(
                            "performance-test active HDD write lock poisoned".to_string(),
                        )
                    })?
                    .insert(
                        active_key,
                        ActiveHddWrite {
                            file_index: job.payload.file_index,
                            copy_index: job.copy_index,
                            relative_path: job.payload.relative_path.clone(),
                            disk_id: placement.disk_id.clone(),
                            size_bytes: job.payload.size_bytes,
                            bytes_written: 0,
                            started: Instant::now(),
                            phase: PerformanceCopyProgressPhase::Copying,
                        },
                    );
                let mut last_progress_bytes = 0_u64;
                let mut last_write_seconds = 0.0_f64;
                let mut progress = |bytes: u64,
                                    write_seconds: f64,
                                    phase: PerformanceCopyProgressPhase|
                 -> Result<(), CliError> {
                    let delta = bytes.saturating_sub(last_progress_bytes);
                    last_progress_bytes = bytes;
                    let delta_write_seconds = (write_seconds - last_write_seconds).max(0.0);
                    last_write_seconds = write_seconds;
                    hdd_bytes_transferred.fetch_add(delta, Ordering::SeqCst);
                    live_rates.add_hdd_write_interval(
                        &placement.disk_id,
                        delta,
                        delta_write_seconds,
                    )?;
                    if let Some(active) = active_hdd_writes
                        .lock()
                        .map_err(|_| {
                            CliError::CommandFailed(
                                "performance-test active HDD write lock poisoned".to_string(),
                            )
                        })?
                        .get_mut(&active_key)
                    {
                        active.bytes_written = bytes;
                        active.phase = phase;
                    }
                    Ok(())
                };
                let measurement = if let Some(source) = &job.payload.source_path {
                    let mut split_progress =
                        |copy_progress: PerformanceSplitCopyProgress| -> Result<(), CliError> {
                            progress(
                                copy_progress.bytes,
                                copy_progress.destination_write_seconds,
                                copy_progress.phase,
                            )
                        };
                    measure_copy_with_split_progress(
                        source,
                        &destination,
                        Some(&mut split_progress),
                    )
                    .map(|measurement| measurement.destination_write)
                } else {
                    let mut generated_progress =
                        |bytes: u64, seconds: f64| -> Result<(), CliError> {
                            progress(bytes, seconds, PerformanceCopyProgressPhase::Copying)
                        };
                    measure_land_payload_with_progress_and_sync_policy(
                        &job.payload,
                        &destination,
                        job.payload.file_index ^ worker_index as u32 ^ job.copy_index as u32,
                        Some(&mut generated_progress),
                        PerformanceCopySyncPolicy::SyncAll,
                    )
                };
                let _ = fs::remove_file(&destination);
                let _ = active_hdd_writes
                    .lock()
                    .map(|mut active| active.remove(&active_key));
                let measurement = match measurement {
                    Ok(measurement) => measurement,
                    Err(err) => {
                        let _ = complete_performance_disk(&scheduler, &placement.disk_id, 0, 0.0);
                        return Err(err);
                    }
                };
                complete_performance_disk(
                    &scheduler,
                    &placement.disk_id,
                    measurement.bytes,
                    measurement.seconds,
                )?;
                hdd_jobs_completed.fetch_add(1, Ordering::SeqCst);
                worker_results
                    .lock()
                    .map_err(|_| {
                        CliError::CommandFailed("performance-test result lock poisoned".to_string())
                    })?
                    .push(PerformanceDiskResult {
                        file_index: job.payload.file_index,
                        copy_index: job.copy_index,
                        concurrency,
                        scenario: PerformanceScenarioKind::DirectHdd,
                        disk_id: placement.disk_id,
                        ssd_read: zero_measurement(),
                        write: measurement,
                    });
            }
            Ok(())
        }));
    }
    let mut producer_error = None;
    let total_hdd_jobs = workload.file_count() as usize * redundancy;
    let mut submitted_hdd_jobs = 0_usize;
    for payload in &workload.payloads {
        if let Err(err) = check_performance_cancelled() {
            producer_error = Some(err);
            break;
        }
        for copy_index in 0..redundancy {
            let mut pending_job = Some(DirectHddJob {
                payload: payload.clone(),
                copy_index,
            });
            loop {
                check_performance_cancelled()?;
                let job = pending_job.take().expect("pending direct HDD job");
                match sender.try_send(job) {
                    Ok(()) => {
                        submitted_hdd_jobs += 1;
                        break;
                    }
                    Err(mpsc::TrySendError::Full(job)) => {
                        pending_job = Some(job);
                        if let Some(context) = tui_context {
                            let rate_snapshot = live_rates.snapshot()?;
                            render_hdd_drain_tui_snapshot(
                                writer,
                                HddDrainTuiState {
                                    context,
                                    workload,
                                    kind: PerformanceScenarioKind::DirectHdd,
                                    concurrency,
                                    submitted_jobs: submitted_hdd_jobs,
                                    total_jobs: total_hdd_jobs,
                                    started_jobs: hdd_jobs_started.load(Ordering::SeqCst) as usize,
                                    completed_jobs: hdd_jobs_completed.load(Ordering::SeqCst)
                                        as usize,
                                    transferred_bytes: hdd_bytes_transferred.load(Ordering::SeqCst),
                                    ssd_read_rate: None,
                                    hdd_write_rate: rate_snapshot.hdd_write_rate,
                                    hdd_disk_rates: active_hdd_disk_rates(&active_hdd_writes)?,
                                    active_hdd_landing: active_hdd_landing_lines(
                                        &active_hdd_writes,
                                        workload.file_count(),
                                    )?,
                                },
                            )?;
                        }
                        thread::sleep(Duration::from_millis(250));
                    }
                    Err(mpsc::TrySendError::Disconnected(_)) => {
                        producer_error = Some(CliError::CommandFailed(
                            "performance-test direct HDD workers stopped early".to_string(),
                        ));
                        break;
                    }
                }
            }
            if producer_error.is_some() {
                break;
            }
            if let Some(context) = tui_context {
                let rate_snapshot = live_rates.snapshot()?;
                render_hdd_drain_tui_snapshot(
                    writer,
                    HddDrainTuiState {
                        context,
                        workload,
                        kind: PerformanceScenarioKind::DirectHdd,
                        concurrency,
                        submitted_jobs: submitted_hdd_jobs,
                        total_jobs: total_hdd_jobs,
                        started_jobs: hdd_jobs_started.load(Ordering::SeqCst) as usize,
                        completed_jobs: hdd_jobs_completed.load(Ordering::SeqCst) as usize,
                        transferred_bytes: hdd_bytes_transferred.load(Ordering::SeqCst),
                        ssd_read_rate: None,
                        hdd_write_rate: rate_snapshot.hdd_write_rate,
                        hdd_disk_rates: active_hdd_disk_rates(&active_hdd_writes)?,
                        active_hdd_landing: active_hdd_landing_lines(
                            &active_hdd_writes,
                            workload.file_count(),
                        )?,
                    },
                )?;
            }
        }
        if producer_error.is_some() {
            break;
        }
    }
    drop(sender);
    if let Some(context) = tui_context {
        while (hdd_jobs_completed.load(Ordering::SeqCst) as usize) < total_hdd_jobs {
            check_performance_cancelled()?;
            let rate_snapshot = live_rates.snapshot()?;
            render_hdd_drain_tui_snapshot(
                writer,
                HddDrainTuiState {
                    context,
                    workload,
                    kind: PerformanceScenarioKind::DirectHdd,
                    concurrency,
                    submitted_jobs: submitted_hdd_jobs,
                    total_jobs: total_hdd_jobs,
                    started_jobs: hdd_jobs_started.load(Ordering::SeqCst) as usize,
                    completed_jobs: hdd_jobs_completed.load(Ordering::SeqCst) as usize,
                    transferred_bytes: hdd_bytes_transferred.load(Ordering::SeqCst),
                    ssd_read_rate: None,
                    hdd_write_rate: rate_snapshot.hdd_write_rate,
                    hdd_disk_rates: active_hdd_disk_rates(&active_hdd_writes)?,
                    active_hdd_landing: active_hdd_landing_lines(
                        &active_hdd_writes,
                        workload.file_count(),
                    )?,
                },
            )?;
            if handles.iter().all(|handle| handle.is_finished()) {
                break;
            }
            thread::sleep(Duration::from_millis(500));
        }
    }
    let mut worker_error = None;
    for handle in handles {
        match handle.join() {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                let _ = worker_error.get_or_insert(err);
            }
            Err(_) => {
                let _ = worker_error.get_or_insert(CliError::CommandFailed(
                    "performance-test direct HDD worker panicked".to_string(),
                ));
            }
        };
    }
    if let Some(err) = producer_error.or(worker_error) {
        return Err(err);
    }
    let io_samples = io_sampler.stop();
    let elapsed_seconds = started.elapsed().as_secs_f64().max(0.001);
    let mut disk_results = Arc::try_unwrap(worker_results)
        .map_err(|_| {
            CliError::CommandFailed("performance-test result lock still shared".to_string())
        })?
        .into_inner()
        .map_err(|_| {
            CliError::CommandFailed("performance-test result lock poisoned".to_string())
        })?;
    disk_results.sort_by(|left, right| {
        left.file_index
            .cmp(&right.file_index)
            .then_with(|| left.copy_index.cmp(&right.copy_index))
            .then_with(|| left.disk_id.cmp(&right.disk_id))
    });
    let total_bytes = disk_results.iter().map(|row| row.write.bytes).sum::<u64>();
    let slowest_seconds = disk_results
        .iter()
        .map(|row| row.write.seconds)
        .fold(0.0_f64, f64::max);
    let members = disk_results
        .iter()
        .map(|row| row.disk_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if log_progress {
        writeln!(
            writer,
            "direct-hdd c{}: aggregate landing {}/s",
            concurrency,
            format_bytes(total_bytes as f64 / elapsed_seconds)
        )?;
    }
    Ok(PerformanceScenarioResult {
        kind: PerformanceScenarioKind::DirectHdd,
        file_order: workload.file_order,
        concurrency,
        redundancy,
        queue_capacity,
        elapsed_seconds,
        total_bytes,
        logical_source_bytes: workload.total_bytes(),
        physical_hdd_write_bytes: total_bytes,
        hdd_write_operations: disk_results.len(),
        hdd_drain_started_before_all_ssd_staged: false,
        file_results: Vec::new(),
        disk_results,
        io_samples,
        concurrency_result: PerformanceConcurrencyResult {
            concurrency,
            scenario: PerformanceScenarioKind::DirectHdd,
            aggregate_bytes: total_bytes,
            seconds: elapsed_seconds,
            slowest_seconds,
            members,
        },
    })
}
