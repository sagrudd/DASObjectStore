use super::*;

pub(super) fn benchmark_ssd_only(
    ssd_bench_root: &Path,
    workload: &PerformanceWorkload,
    writer: &mut impl Write,
    log_progress: bool,
    tui_context: Option<PerformanceTuiContext<'_>>,
) -> Result<PerformanceScenarioResult, CliError> {
    let started = Instant::now();
    let io_sampler = PerformanceIoSampler::start(performance_io_devices(Some(ssd_bench_root), &[]));
    let ssd_settler = PerformanceSsdSettler::start(PERFORMANCE_SSD_SETTLE_QUEUE_CAPACITY);
    let scenario_root = ssd_bench_root.join("ssd-only");
    let residency_budget = performance_ssd_residency_budget(&scenario_root)?;
    let batches = plan_ssd_residency_batches(workload, residency_budget)?;
    let batch_count = batches.len();
    let mut file_results = Vec::<PerformanceFileResult>::new();
    let mut ssd_write_measurements = Vec::<PerformanceMeasurement>::new();
    let mut written_bytes = 0_u64;
    let mut read_bytes = 0_u64;

    for (batch_index, batch) in batches.into_iter().enumerate() {
        let batch_bytes = batch.iter().map(|payload| payload.size_bytes).sum::<u64>();
        let mut staged_payloads =
            Vec::<(PerformancePayload, PathBuf, PerformanceMeasurement)>::new();
        for payload in batch {
            check_performance_cancelled()?;
            let destination = scenario_root.join(&payload.relative_path);
            if let Some(context) = tui_context {
                render_performance_tui_snapshot(
                    writer,
                    &PerformanceTuiSnapshot {
                        phase: "ssd-only write phase",
                        scenario: "ssd-only",
                        activity: format!(
                            "Writing batch {}/{} file {}/{} to SSD: {}",
                            batch_index + 1,
                            batch_count,
                            payload.file_index + 1,
                            workload.file_count(),
                            payload.relative_path.display()
                        ),
                        objective: performance_scenario_objective(
                            PerformanceScenarioKind::SsdOnly,
                            0,
                        ),
                        bounds: performance_scenario_bounds(
                            workload,
                            PerformanceScenarioKind::SsdOnly,
                            0,
                        ),
                        scenario_done: context.scenario_done,
                        scenario_total: context.scenario_total,
                        file_done: payload.file_index,
                        current_file: Some(payload.file_index + 1),
                        file_count: workload.file_count(),
                        processed_bytes: written_bytes,
                        total_bytes: workload.total_bytes(),
                        hdd_concurrency: 0,
                        current_rate: None,
                        ssd_write_rate: measurement_rate(ssd_write_measurements.iter().copied()),
                        ssd_read_rate: None,
                        hdd_write_rate: None,
                        hdd_disk_rates: Vec::new(),
                        active_hdd_landing: Vec::new(),
                        aggregate_rate: None,
                        report_path: context.report_path,
                        json_path: context.json_path,
                    },
                )?;
            }
            let ssd_write = if let Some(context) = tui_context {
                let mut progress = |bytes: u64, seconds: f64| -> Result<(), CliError> {
                    render_performance_tui_snapshot(
                        writer,
                        &PerformanceTuiSnapshot {
                            phase: "ssd-only write phase",
                            scenario: "ssd-only",
                            activity: format!(
                                "Writing batch {}/{} file {}/{} to SSD: {} ({}/{})",
                                batch_index + 1,
                                batch_count,
                                payload.file_index + 1,
                                workload.file_count(),
                                payload.relative_path.display(),
                                format_bytes(bytes as f64),
                                format_bytes(payload.size_bytes as f64)
                            ),
                            objective: performance_scenario_objective(
                                PerformanceScenarioKind::SsdOnly,
                                0,
                            ),
                            bounds: performance_scenario_bounds(
                                workload,
                                PerformanceScenarioKind::SsdOnly,
                                0,
                            ),
                            scenario_done: context.scenario_done,
                            scenario_total: context.scenario_total,
                            file_done: payload.file_index,
                            current_file: Some(payload.file_index + 1),
                            file_count: workload.file_count(),
                            processed_bytes: written_bytes.saturating_add(bytes),
                            total_bytes: workload.total_bytes(),
                            hdd_concurrency: 0,
                            current_rate: Some(bytes as f64 / seconds.max(0.001)),
                            ssd_write_rate: measurement_rate_with_current(
                                ssd_write_measurements.iter().copied(),
                                bytes,
                                seconds,
                            ),
                            ssd_read_rate: None,
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
                    &destination,
                    payload.file_index,
                    Some(&mut progress),
                    &ssd_settler,
                )?
            } else {
                measure_ssd_stage_payload(&payload, &destination, &ssd_settler)?
            };
            written_bytes = written_bytes.saturating_add(ssd_write.bytes);
            ssd_write_measurements.push(ssd_write);
            if log_progress {
                writeln!(
                    writer,
                    "ssd-only write batch {}/{} file {}/{}: SSD write {}/s",
                    batch_index + 1,
                    batch_count,
                    payload.file_index + 1,
                    workload.file_count(),
                    format_bytes(throughput(ssd_write))
                )?;
            }
            staged_payloads.push((payload, destination, ssd_write));
        }

        if let Some(context) = tui_context {
            render_performance_tui_snapshot(
                writer,
                &PerformanceTuiSnapshot {
                    phase: "ssd-only readback phase",
                    scenario: "ssd-only",
                    activity: format!(
                        "Batch {}/{} staged {}; reading it back from SSD before the next batch",
                        batch_index + 1,
                        batch_count,
                        format_bytes(batch_bytes as f64)
                    ),
                    objective: performance_scenario_objective(PerformanceScenarioKind::SsdOnly, 0),
                    bounds: performance_scenario_bounds(
                        workload,
                        PerformanceScenarioKind::SsdOnly,
                        0,
                    ),
                    scenario_done: context.scenario_done,
                    scenario_total: context.scenario_total,
                    file_done: file_results.len() as u32,
                    current_file: None,
                    file_count: workload.file_count(),
                    processed_bytes: read_bytes,
                    total_bytes: workload.total_bytes(),
                    hdd_concurrency: 0,
                    current_rate: None,
                    ssd_write_rate: measurement_rate(ssd_write_measurements.iter().copied()),
                    ssd_read_rate: measurement_rate(file_results.iter().map(|row| row.ssd_read)),
                    hdd_write_rate: None,
                    hdd_disk_rates: Vec::new(),
                    active_hdd_landing: Vec::new(),
                    aggregate_rate: None,
                    report_path: context.report_path,
                    json_path: context.json_path,
                },
            )?;
        }
        for (payload, destination, ssd_write) in staged_payloads {
            check_performance_cancelled()?;
            let completed_reads = file_results.len() as u32;
            let ssd_read = if let Some(context) = tui_context {
                let mut progress = |bytes: u64, seconds: f64| -> Result<(), CliError> {
                    render_performance_tui_snapshot(
                        writer,
                        &PerformanceTuiSnapshot {
                            phase: "ssd-only readback phase",
                            scenario: "ssd-only",
                            activity: format!(
                                "Reading batch {}/{} file {}/{} back from SSD: {} ({})",
                                batch_index + 1,
                                batch_count,
                                payload.file_index + 1,
                                workload.file_count(),
                                payload.relative_path.display(),
                                format_bytes(bytes as f64)
                            ),
                            objective: performance_scenario_objective(
                                PerformanceScenarioKind::SsdOnly,
                                0,
                            ),
                            bounds: performance_scenario_bounds(
                                workload,
                                PerformanceScenarioKind::SsdOnly,
                                0,
                            ),
                            scenario_done: context.scenario_done,
                            scenario_total: context.scenario_total,
                            file_done: completed_reads,
                            current_file: Some(payload.file_index + 1),
                            file_count: workload.file_count(),
                            processed_bytes: read_bytes.saturating_add(bytes),
                            total_bytes: workload.total_bytes(),
                            hdd_concurrency: 0,
                            current_rate: Some(bytes as f64 / seconds.max(0.001)),
                            ssd_write_rate: measurement_rate(
                                ssd_write_measurements.iter().copied(),
                            ),
                            ssd_read_rate: measurement_rate_with_current(
                                file_results.iter().map(|row| row.ssd_read),
                                bytes,
                                seconds,
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
                measure_read_with_progress(&destination, Some(&mut progress))?
            } else {
                measure_read(&destination)?
            };
            let _ = fs::remove_file(&destination);
            read_bytes = read_bytes.saturating_add(ssd_read.bytes);
            file_results.push(PerformanceFileResult {
                file_index: payload.file_index,
                ssd_write,
                ssd_read,
            });
            if log_progress {
                writeln!(
                    writer,
                    "ssd-only read batch {}/{} file {}/{}: SSD read {}/s",
                    batch_index + 1,
                    batch_count,
                    payload.file_index + 1,
                    workload.file_count(),
                    format_bytes(throughput(ssd_read))
                )?;
            }
        }
    }
    ssd_settler.finish()?;
    let io_samples = io_sampler.stop();
    let elapsed_seconds = started.elapsed().as_secs_f64().max(0.001);
    let total_bytes = written_bytes;
    Ok(PerformanceScenarioResult {
        kind: PerformanceScenarioKind::SsdOnly,
        file_order: workload.file_order,
        concurrency: 0,
        redundancy: 1,
        queue_capacity: 0,
        elapsed_seconds,
        total_bytes,
        logical_source_bytes: total_bytes,
        physical_hdd_write_bytes: 0,
        hdd_write_operations: 0,
        hdd_drain_started_before_all_ssd_staged: false,
        file_results,
        disk_results: Vec::new(),
        io_samples,
        concurrency_result: PerformanceConcurrencyResult {
            concurrency: 0,
            scenario: PerformanceScenarioKind::SsdOnly,
            aggregate_bytes: total_bytes,
            seconds: elapsed_seconds,
            slowest_seconds: 0.0,
            members: Vec::new(),
        },
    })
}
