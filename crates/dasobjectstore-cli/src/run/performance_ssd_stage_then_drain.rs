use super::*;

pub(super) fn benchmark_ssd_stage_then_drain(
    ssd_bench_root: &Path,
    hdd_bench_roots: &[(DiskId, PathBuf)],
    workload: &PerformanceWorkload,
    concurrency: usize,
    redundancy: usize,
    writer: &mut impl Write,
    log_progress: bool,
    tui_context: Option<PerformanceTuiContext<'_>>,
) -> Result<PerformanceScenarioResult, CliError> {
    let started = Instant::now();
    let io_sampler = PerformanceIoSampler::start(performance_io_devices(
        Some(ssd_bench_root),
        hdd_bench_roots,
    ));
    let ssd_settler = PerformanceSsdSettler::start(PERFORMANCE_SSD_SETTLE_QUEUE_CAPACITY);
    let scenario_root = ssd_bench_root
        .join("ssd-stage-then-drain")
        .join(format!("c{concurrency}"));
    let residency_budget = performance_ssd_residency_budget(&scenario_root)?;
    let batches = plan_ssd_residency_batches(workload, residency_budget)?;
    let batch_count = batches.len();
    let queue_capacity = hdd_queue_capacity(concurrency, redundancy);
    let scheduler = new_shared_disk_placement_scheduler(hdd_bench_roots)?;
    let mut file_results = Vec::<PerformanceFileResult>::new();
    let mut disk_results = Vec::<PerformanceDiskResult>::new();
    let mut total_bytes = 0_u64;

    for (batch_index, batch) in batches.into_iter().enumerate() {
        let mut staged_jobs = Vec::new();
        let batch_bytes = batch.iter().map(|payload| payload.size_bytes).sum::<u64>();
        for payload in batch {
            check_performance_cancelled()?;
            let ssd_path = scenario_root.join(&payload.relative_path);
            if let Some(context) = tui_context {
                render_performance_tui_snapshot(
                    writer,
                    &PerformanceTuiSnapshot {
                        phase: "ssd-stage-then-drain staging",
                        scenario: "ssd-stage-then-drain",
                        activity: format!(
                            "Staging batch {}/{} file {}/{} to SSD before batch drain: {}",
                            batch_index + 1,
                            batch_count,
                            payload.file_index + 1,
                            workload.file_count(),
                            payload.relative_path.display()
                        ),
                        objective: performance_scenario_objective(
                            PerformanceScenarioKind::SsdStageThenDrain,
                            concurrency,
                        ),
                        bounds: performance_scenario_bounds(
                            workload,
                            PerformanceScenarioKind::SsdStageThenDrain,
                            concurrency,
                        ),
                        scenario_done: context.scenario_done,
                        scenario_total: context.scenario_total,
                        file_done: payload.file_index,
                        current_file: Some(payload.file_index + 1),
                        file_count: workload.file_count(),
                        processed_bytes: total_bytes,
                        total_bytes: workload.total_bytes(),
                        hdd_concurrency: 0,
                        current_rate: None,
                        ssd_write_rate: measurement_rate(
                            file_results.iter().map(|row| row.ssd_write),
                        ),
                        ssd_read_rate: measurement_rate(
                            file_results.iter().map(|row| row.ssd_read),
                        ),
                        hdd_write_rate: None,
                        hdd_disk_rates: Vec::new(),
                        active_hdd_landing: Vec::new(),
                        aggregate_rate: None,
                        report_path: context.report_path,
                        json_path: context.json_path,
                    },
                )?;
            }
            let ssd_write = match if let Some(context) = tui_context {
                let mut progress = |bytes: u64, seconds: f64| -> Result<(), CliError> {
                    render_performance_tui_snapshot(
                        writer,
                        &PerformanceTuiSnapshot {
                            phase: "ssd-stage-then-drain staging",
                            scenario: "ssd-stage-then-drain",
                            activity: format!(
                                "Staging batch {}/{} file {}/{} to SSD before drain: {} ({}/{})",
                                batch_index + 1,
                                batch_count,
                                payload.file_index + 1,
                                workload.file_count(),
                                payload.relative_path.display(),
                                format_bytes(bytes as f64),
                                format_bytes(payload.size_bytes as f64)
                            ),
                            objective: performance_scenario_objective(
                                PerformanceScenarioKind::SsdStageThenDrain,
                                concurrency,
                            ),
                            bounds: performance_scenario_bounds(
                                workload,
                                PerformanceScenarioKind::SsdStageThenDrain,
                                concurrency,
                            ),
                            scenario_done: context.scenario_done,
                            scenario_total: context.scenario_total,
                            file_done: payload.file_index,
                            current_file: Some(payload.file_index + 1),
                            file_count: workload.file_count(),
                            processed_bytes: total_bytes.saturating_add(bytes),
                            total_bytes: workload.total_bytes(),
                            hdd_concurrency: 0,
                            current_rate: Some(bytes as f64 / seconds.max(0.001)),
                            ssd_write_rate: measurement_rate_with_current(
                                file_results.iter().map(|row| row.ssd_write),
                                bytes,
                                seconds,
                            ),
                            ssd_read_rate: measurement_rate(
                                file_results.iter().map(|row| row.ssd_read),
                            ),
                            hdd_write_rate: None,
                            hdd_disk_rates: Vec::new(),
                            active_hdd_landing: Vec::new(),
                            aggregate_rate: None,
                            report_path: context.report_path,
                            json_path: context.json_path,
                        },
                    )
                };
                measure_ssd_stage_payload_with_progress(
                    &payload,
                    &ssd_path,
                    payload.file_index,
                    Some(&mut progress),
                    &ssd_settler,
                )
            } else {
                measure_ssd_stage_payload(&payload, &ssd_path, &ssd_settler)
            } {
                Ok(measurement) => measurement,
                Err(err) => {
                    let _ = fs::remove_file(&ssd_path);
                    return Err(err);
                }
            };
            total_bytes = total_bytes.saturating_add(ssd_write.bytes);
            file_results.push(PerformanceFileResult {
                file_index: payload.file_index,
                ssd_write,
                ssd_read: zero_measurement(),
            });
            for copy_index in 0..redundancy {
                staged_jobs.push(SsdPipelineJob {
                    file_index: payload.file_index,
                    copy_index,
                    relative_path: payload.relative_path.clone(),
                    ssd_path: ssd_path.clone(),
                    size_bytes: payload.size_bytes,
                });
            }
            if log_progress {
                writeln!(
                    writer,
                    "ssd-stage-then-drain c{} batch {}/{} file {}/{}: SSD write {}/s staged for later HDD drain",
                    concurrency,
                    batch_index + 1,
                    batch_count,
                    payload.file_index + 1,
                    workload.file_count(),
                    format_bytes(throughput(ssd_write))
                )?;
            }
        }

        if let Some(context) = tui_context {
            render_performance_tui_snapshot(
                writer,
                &PerformanceTuiSnapshot {
                    phase: "ssd-stage-then-drain draining",
                    scenario: "ssd-stage-then-drain",
                    activity: format!(
                        "Batch {}/{} staged {}; draining SSD backlog with {concurrency} HDD worker(s)",
                        batch_index + 1,
                        batch_count,
                        format_bytes(batch_bytes as f64)
                    ),
                    objective: performance_scenario_objective(
                        PerformanceScenarioKind::SsdStageThenDrain,
                        concurrency,
                    ),
                    bounds: performance_scenario_bounds(
                        workload,
                        PerformanceScenarioKind::SsdStageThenDrain,
                        concurrency,
                    ),
                    scenario_done: context.scenario_done,
                    scenario_total: context.scenario_total,
                    file_done: file_results.len() as u32,
                    current_file: None,
                    file_count: workload.file_count(),
                    processed_bytes: total_bytes,
                    total_bytes: workload.total_bytes(),
                    hdd_concurrency: concurrency,
                    current_rate: None,
                    ssd_write_rate: measurement_rate(file_results.iter().map(|row| row.ssd_write)),
                    ssd_read_rate: measurement_rate(file_results.iter().map(|row| row.ssd_read)),
                    hdd_write_rate: None,
                    hdd_disk_rates: Vec::new(),
                    active_hdd_landing: Vec::new(),
                    aggregate_rate: Some(
                        total_bytes as f64 / started.elapsed().as_secs_f64().max(0.001),
                    ),
                    report_path: context.report_path,
                    json_path: context.json_path,
                },
            )?;
        }

        let (sender, receiver) = mpsc::sync_channel::<SsdPipelineJob>(queue_capacity);
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
        for _ in 0..concurrency {
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
                    let job = {
                        let receiver = receiver.lock().map_err(|_| {
                            CliError::CommandFailed(
                                "performance-test staged HDD queue lock poisoned".to_string(),
                            )
                        })?;
                        receiver.recv()
                    };
                    let Ok(job) = job else {
                        break;
                    };
                    hdd_jobs_started.fetch_add(1, Ordering::SeqCst);
                    let placement = reserve_performance_disk_for_file(&scheduler, job.file_index)?;
                    let destination = placement
                        .root_path
                        .join("ssd-stage-then-drain")
                        .join(format!("c{concurrency}"))
                        .join(&job.relative_path);
                    let active_key = (job.file_index, job.copy_index);
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
                                file_index: job.file_index,
                                copy_index: job.copy_index,
                                relative_path: job.relative_path.clone(),
                                disk_id: placement.disk_id.clone(),
                                size_bytes: job.size_bytes,
                                bytes_written: 0,
                                started: Instant::now(),
                                phase: PerformanceCopyProgressPhase::Copying,
                            },
                        );
                    let mut last_progress_bytes = 0_u64;
                    let mut last_read_seconds = 0.0_f64;
                    let mut last_write_seconds = 0.0_f64;
                    let mut progress =
                        |copy_progress: PerformanceSplitCopyProgress| -> Result<(), CliError> {
                            let delta = copy_progress.bytes.saturating_sub(last_progress_bytes);
                            last_progress_bytes = copy_progress.bytes;
                            let delta_read_seconds =
                                (copy_progress.source_read_seconds - last_read_seconds).max(0.0);
                            last_read_seconds = copy_progress.source_read_seconds;
                            let delta_write_seconds = (copy_progress.destination_write_seconds
                                - last_write_seconds)
                                .max(0.0);
                            last_write_seconds = copy_progress.destination_write_seconds;
                            if delta > 0 {
                                hdd_bytes_transferred.fetch_add(delta, Ordering::SeqCst);
                                live_rates.add_ssd_read_interval(delta, delta_read_seconds)?;
                            }
                            live_rates.add_hdd_write_interval(
                                &placement.disk_id,
                                delta,
                                delta_write_seconds,
                            )?;
                            if let Some(active) = active_hdd_writes
                                .lock()
                                .map_err(|_| {
                                    CliError::CommandFailed(
                                        "performance-test active HDD write lock poisoned"
                                            .to_string(),
                                    )
                                })?
                                .get_mut(&active_key)
                            {
                                active.bytes_written = copy_progress.bytes;
                                active.phase = copy_progress.phase;
                            }
                            Ok(())
                        };
                    let measurement = measure_copy_with_split_progress(
                        &job.ssd_path,
                        &destination,
                        Some(&mut progress),
                    );
                    let _ = fs::remove_file(&destination);
                    let _ = active_hdd_writes
                        .lock()
                        .map(|mut active| active.remove(&active_key));
                    let measurement = match measurement {
                        Ok(measurement) => measurement,
                        Err(err) => {
                            let _ =
                                complete_performance_disk(&scheduler, &placement.disk_id, 0, 0.0);
                            return Err(err);
                        }
                    };
                    hdd_jobs_completed.fetch_add(1, Ordering::SeqCst);
                    complete_performance_disk(
                        &scheduler,
                        &placement.disk_id,
                        measurement.destination_write.bytes,
                        measurement.destination_write.seconds,
                    )?;
                    worker_results
                        .lock()
                        .map_err(|_| {
                            CliError::CommandFailed(
                                "performance-test result lock poisoned".to_string(),
                            )
                        })?
                        .push(PerformanceDiskResult {
                            file_index: job.file_index,
                            copy_index: job.copy_index,
                            concurrency,
                            scenario: PerformanceScenarioKind::SsdStageThenDrain,
                            disk_id: placement.disk_id,
                            ssd_read: measurement.source_read,
                            write: measurement.destination_write,
                        });
                }
                Ok(())
            }));
        }
        let total_hdd_jobs = staged_jobs.len();
        let mut submitted_hdd_jobs = 0_usize;
        for job in staged_jobs {
            let mut pending_job = Some(job);
            loop {
                check_performance_cancelled()?;
                let job = pending_job.take().expect("pending staged HDD job");
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
                                    kind: PerformanceScenarioKind::SsdStageThenDrain,
                                    concurrency,
                                    submitted_jobs: submitted_hdd_jobs,
                                    total_jobs: total_hdd_jobs,
                                    started_jobs: hdd_jobs_started.load(Ordering::SeqCst) as usize,
                                    completed_jobs: hdd_jobs_completed.load(Ordering::SeqCst)
                                        as usize,
                                    transferred_bytes: hdd_bytes_transferred.load(Ordering::SeqCst),
                                    ssd_read_rate: rate_snapshot.ssd_read_rate,
                                    hdd_write_rate: rate_snapshot.hdd_write_rate,
                                    hdd_disk_rates: active_hdd_disk_rates(&active_hdd_writes)?,
                                    active_hdd_landing: active_hdd_landing_lines(
                                        &active_hdd_writes,
                                        workload.file_count(),
                                    )?,
                                },
                            )?;
                        }
                        thread::sleep(std::time::Duration::from_millis(250));
                    }
                    Err(mpsc::TrySendError::Disconnected(_)) => {
                        return Err(CliError::CommandFailed(
                            "performance-test staged HDD workers stopped early".to_string(),
                        ));
                    }
                }
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
                        kind: PerformanceScenarioKind::SsdStageThenDrain,
                        concurrency,
                        submitted_jobs: submitted_hdd_jobs,
                        total_jobs: total_hdd_jobs,
                        started_jobs: hdd_jobs_started.load(Ordering::SeqCst) as usize,
                        completed_jobs: hdd_jobs_completed.load(Ordering::SeqCst) as usize,
                        transferred_bytes: hdd_bytes_transferred.load(Ordering::SeqCst),
                        ssd_read_rate: rate_snapshot.ssd_read_rate,
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
                thread::sleep(std::time::Duration::from_millis(500));
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
                        "performance-test staged HDD worker panicked".to_string(),
                    ));
                }
            };
        }
        if let Some(err) = worker_error {
            return Err(err);
        }
        let mut batch_disk_results = Arc::try_unwrap(worker_results)
            .map_err(|_| {
                CliError::CommandFailed("performance-test result lock still shared".to_string())
            })?
            .into_inner()
            .map_err(|_| {
                CliError::CommandFailed("performance-test result lock poisoned".to_string())
            })?;
        for job in batch_disk_results.iter() {
            let ssd_path = scenario_root.join(
                workload
                    .payloads
                    .iter()
                    .find(|payload| payload.file_index == job.file_index)
                    .map(|payload| payload.relative_path.clone())
                    .unwrap_or_else(|| PathBuf::from(format!("missing-{}", job.file_index))),
            );
            let _ = fs::remove_file(&ssd_path);
        }
        disk_results.append(&mut batch_disk_results);
    }

    ssd_settler.finish()?;
    let io_samples = io_sampler.stop();
    let elapsed_seconds = started.elapsed().as_secs_f64().max(0.001);
    disk_results.sort_by(|left, right| {
        left.file_index
            .cmp(&right.file_index)
            .then_with(|| left.copy_index.cmp(&right.copy_index))
            .then_with(|| left.disk_id.cmp(&right.disk_id))
    });
    update_file_read_measurements_from_disk_results(&mut file_results, &disk_results);
    let physical_hdd_write_bytes = disk_results.iter().map(|row| row.write.bytes).sum::<u64>();
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
            "ssd-stage-then-drain c{}: aggregate landing {}/s",
            concurrency,
            format_bytes(physical_hdd_write_bytes as f64 / elapsed_seconds)
        )?;
    }
    Ok(PerformanceScenarioResult {
        kind: PerformanceScenarioKind::SsdStageThenDrain,
        file_order: workload.file_order,
        concurrency,
        redundancy,
        queue_capacity,
        elapsed_seconds,
        total_bytes: physical_hdd_write_bytes,
        logical_source_bytes: workload.total_bytes(),
        physical_hdd_write_bytes,
        hdd_write_operations: disk_results.len(),
        hdd_drain_started_before_all_ssd_staged: false,
        file_results,
        disk_results,
        io_samples,
        concurrency_result: PerformanceConcurrencyResult {
            concurrency,
            scenario: PerformanceScenarioKind::SsdStageThenDrain,
            aggregate_bytes: physical_hdd_write_bytes,
            seconds: elapsed_seconds,
            slowest_seconds,
            members,
        },
    })
}
