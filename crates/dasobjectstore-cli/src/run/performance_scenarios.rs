use super::*;

pub(super) fn execute_performance_scenarios(
    writer: &mut impl Write,
    workload: &PerformanceWorkload,
    file_orders: &[PerformanceFileOrder],
    scenario_plan: &PerformanceScenarioPlan,
    ssd_bench_root: &Path,
    hdd_bench_roots: &[(DiskId, PathBuf)],
    redundancy: usize,
    tui: bool,
    scenario_total: usize,
    report_path: &Path,
    json_path: &Path,
) -> Result<PerformanceBenchmarkResults, CliError> {
    let mut scenario_done = 0_usize;
    if tui {
        render_performance_tui_snapshot(
            writer,
            &PerformanceTuiSnapshot {
                phase: "preparing",
                scenario: "preparing",
                activity: "Preparing performance scenarios".to_string(),
                objective: format!(
                    "selected scenarios: {}; HDD concurrency: {}",
                    scenario_plan.scenario_names().join(", "),
                    format_concurrency_list(&scenario_plan.concurrency_values())
                ),
                bounds: performance_selected_matrix_bounds(&workload, &scenario_plan),
                scenario_done,
                scenario_total,
                file_done: 0,
                current_file: None,
                file_count: workload.file_count(),
                processed_bytes: 0,
                total_bytes: workload.total_bytes(),
                hdd_concurrency: 0,
                current_rate: None,
                ssd_write_rate: None,
                ssd_read_rate: None,
                hdd_write_rate: None,
                hdd_disk_rates: Vec::new(),
                active_hdd_landing: Vec::new(),
                aggregate_rate: None,
                report_path: &report_path,
                json_path: &json_path,
            },
        )?;
    }
    let mut ssd_only = Vec::new();
    let mut ssd_stage_then_drain = Vec::new();
    let mut ssd_pipeline = Vec::new();
    let mut direct_hdd = Vec::new();
    for &file_order in file_orders {
        let workload = ordered_performance_workload(&workload, file_order);
        if !tui {
            writeln!(
                writer,
                "performance-test: file order {}",
                file_order.as_str()
            )?;
        }
        if scenario_plan.include_ssd_only {
            if !tui {
                writeln!(
                writer,
                "scenario ssd-only: writing all source payloads to SSD, then reading all payloads back from SSD"
            )?;
            }
            let tui_context = tui.then_some(PerformanceTuiContext {
                scenario_done,
                scenario_total,
                report_path: &report_path,
                json_path: &json_path,
            });
            let scenario =
                benchmark_ssd_only(&ssd_bench_root, &workload, writer, !tui, tui_context)?;
            scenario_done += 1;
            if tui {
                render_performance_tui_snapshot(
                    writer,
                    &PerformanceTuiSnapshot {
                        phase: "ssd-only complete",
                        scenario: "ssd-only",
                        activity: "SSD-only scenario complete".to_string(),
                        objective: performance_scenario_objective(
                            PerformanceScenarioKind::SsdOnly,
                            0,
                        ),
                        bounds: performance_scenario_bounds(
                            &workload,
                            PerformanceScenarioKind::SsdOnly,
                            0,
                        ),
                        scenario_done,
                        scenario_total,
                        file_done: workload.file_count(),
                        current_file: None,
                        file_count: workload.file_count(),
                        processed_bytes: scenario.total_bytes,
                        total_bytes: workload.total_bytes(),
                        hdd_concurrency: 0,
                        current_rate: None,
                        ssd_write_rate: measurement_rate(
                            scenario.file_results.iter().map(|row| row.ssd_write),
                        ),
                        ssd_read_rate: measurement_rate(
                            scenario.file_results.iter().map(|row| row.ssd_read),
                        ),
                        hdd_write_rate: None,
                        hdd_disk_rates: Vec::new(),
                        active_hdd_landing: Vec::new(),
                        aggregate_rate: Some(
                            scenario.total_bytes as f64 / scenario.elapsed_seconds.max(0.001),
                        ),
                        report_path: &report_path,
                        json_path: &json_path,
                    },
                )?;
            }
            ssd_only.push(scenario);
        }

        for &concurrency in &scenario_plan.ssd_stage_then_drain {
            if !tui {
                writeln!(
                writer,
                "scenario ssd-stage-then-drain: stage all selected files to SSD, then drain with {} HDD worker(s)",
                concurrency
            )?;
            }
            let scenario = benchmark_ssd_stage_then_drain(
                &ssd_bench_root,
                &hdd_bench_roots,
                &workload,
                concurrency,
                redundancy,
                writer,
                !tui,
                tui.then_some(PerformanceTuiContext {
                    scenario_done,
                    scenario_total,
                    report_path: &report_path,
                    json_path: &json_path,
                }),
            )?;
            scenario_done += 1;
            if tui {
                let (hdd_write_rate, _hdd_disk_rates) = performance_hdd_tui_rates(&scenario);
                render_performance_tui_snapshot(
                    writer,
                    &PerformanceTuiSnapshot {
                        phase: "ssd-stage-then-drain complete",
                        scenario: "ssd-stage-then-drain",
                        activity: format!(
                        "Separated SSD stage then HDD drain complete with {concurrency} worker(s)"
                    ),
                        objective: performance_scenario_objective(
                            PerformanceScenarioKind::SsdStageThenDrain,
                            concurrency,
                        ),
                        bounds: performance_scenario_bounds(
                            &workload,
                            PerformanceScenarioKind::SsdStageThenDrain,
                            concurrency,
                        ),
                        scenario_done,
                        scenario_total,
                        file_done: workload.file_count(),
                        current_file: None,
                        file_count: workload.file_count(),
                        processed_bytes: scenario.total_bytes,
                        total_bytes: workload.total_bytes(),
                        hdd_concurrency: concurrency,
                        current_rate: None,
                        ssd_write_rate: measurement_rate(
                            scenario.file_results.iter().map(|row| row.ssd_write),
                        ),
                        ssd_read_rate: measurement_rate(
                            scenario.file_results.iter().map(|row| row.ssd_read),
                        ),
                        hdd_write_rate,
                        hdd_disk_rates: Vec::new(),
                        active_hdd_landing: Vec::new(),
                        aggregate_rate: Some(
                            scenario.total_bytes as f64 / scenario.elapsed_seconds.max(0.001),
                        ),
                        report_path: &report_path,
                        json_path: &json_path,
                    },
                )?;
            }
            ssd_stage_then_drain.push(scenario);
        }

        for &concurrency in &scenario_plan.ssd_pipeline {
            if !tui {
                writeln!(
                writer,
                "scenario ssd-overlap-drain: SSD ingest with {} overlapping FIFO HDD drain worker(s)",
                concurrency
            )?;
            }
            let scenario = benchmark_ssd_pipeline(
                &ssd_bench_root,
                &hdd_bench_roots,
                &workload,
                concurrency,
                redundancy,
                writer,
                !tui,
                tui.then_some(PerformanceTuiContext {
                    scenario_done,
                    scenario_total,
                    report_path: &report_path,
                    json_path: &json_path,
                }),
            )?;
            scenario_done += 1;
            if tui {
                let (hdd_write_rate, _hdd_disk_rates) = performance_hdd_tui_rates(&scenario);
                render_performance_tui_snapshot(
                writer,
                &PerformanceTuiSnapshot {
                    phase: "ssd-overlap-drain complete",
                    scenario: "ssd-overlap-drain",
                    activity: format!(
                        "Overlapping SSD ingest and FIFO HDD drain complete with {concurrency} worker(s)"
                    ),
                    objective: performance_scenario_objective(
                        PerformanceScenarioKind::SsdPipeline,
                        concurrency,
                    ),
                    bounds: performance_scenario_bounds(
                        &workload,
                        PerformanceScenarioKind::SsdPipeline,
                        concurrency,
                    ),
                    scenario_done,
                    scenario_total,
                    file_done: workload.file_count(),
                    current_file: None,
                    file_count: workload.file_count(),
                    processed_bytes: scenario.total_bytes,
                    total_bytes: workload.total_bytes(),
                    hdd_concurrency: concurrency,
                    current_rate: None,
                    ssd_write_rate: measurement_rate(
                        scenario.file_results.iter().map(|row| row.ssd_write),
                    ),
                    ssd_read_rate: measurement_rate(
                        scenario.file_results.iter().map(|row| row.ssd_read),
                    ),
                    hdd_write_rate,
                    hdd_disk_rates: Vec::new(),
                    active_hdd_landing: Vec::new(),
                    aggregate_rate: Some(
                        scenario.total_bytes as f64 / scenario.elapsed_seconds.max(0.001),
                    ),
                    report_path: &report_path,
                    json_path: &json_path,
                },
            )?;
            }
            ssd_pipeline.push(scenario);
        }

        for &concurrency in &scenario_plan.direct_hdd {
            if !tui {
                writeln!(
                    writer,
                    "scenario direct-hdd: direct source-to-HDD ingest with {} worker(s)",
                    concurrency
                )?;
            } else {
                render_performance_tui_snapshot(
                    writer,
                    &PerformanceTuiSnapshot {
                        phase: "direct-hdd active",
                        scenario: "direct-hdd",
                        activity: format!(
                            "Writing source payloads directly to HDD with {concurrency} worker(s)"
                        ),
                        objective: performance_scenario_objective(
                            PerformanceScenarioKind::DirectHdd,
                            concurrency,
                        ),
                        bounds: performance_scenario_bounds(
                            &workload,
                            PerformanceScenarioKind::DirectHdd,
                            concurrency,
                        ),
                        scenario_done,
                        scenario_total,
                        file_done: 0,
                        current_file: None,
                        file_count: workload.file_count(),
                        processed_bytes: 0,
                        total_bytes: workload.total_bytes(),
                        hdd_concurrency: concurrency,
                        current_rate: None,
                        ssd_write_rate: None,
                        ssd_read_rate: None,
                        hdd_write_rate: None,
                        hdd_disk_rates: Vec::new(),
                        active_hdd_landing: Vec::new(),
                        aggregate_rate: None,
                        report_path: &report_path,
                        json_path: &json_path,
                    },
                )?;
            }
            let scenario = benchmark_direct_hdd(
                &hdd_bench_roots,
                &workload,
                concurrency,
                redundancy,
                writer,
                !tui,
                if tui {
                    Some(PerformanceTuiContext {
                        scenario_done,
                        scenario_total,
                        report_path: &report_path,
                        json_path: &json_path,
                    })
                } else {
                    None
                },
            )?;
            scenario_done += 1;
            if tui {
                let (hdd_write_rate, _hdd_disk_rates) = performance_hdd_tui_rates(&scenario);
                render_performance_tui_snapshot(
                    writer,
                    &PerformanceTuiSnapshot {
                        phase: "direct-hdd complete",
                        scenario: "direct-hdd",
                        activity: format!(
                            "Direct-to-HDD scenario complete with {concurrency} worker(s)"
                        ),
                        objective: performance_scenario_objective(
                            PerformanceScenarioKind::DirectHdd,
                            concurrency,
                        ),
                        bounds: performance_scenario_bounds(
                            &workload,
                            PerformanceScenarioKind::DirectHdd,
                            concurrency,
                        ),
                        scenario_done,
                        scenario_total,
                        file_done: workload.file_count(),
                        current_file: None,
                        file_count: workload.file_count(),
                        processed_bytes: scenario.total_bytes,
                        total_bytes: workload.total_bytes(),
                        hdd_concurrency: concurrency,
                        current_rate: None,
                        ssd_write_rate: None,
                        ssd_read_rate: None,
                        hdd_write_rate,
                        hdd_disk_rates: Vec::new(),
                        active_hdd_landing: Vec::new(),
                        aggregate_rate: Some(
                            scenario.total_bytes as f64 / scenario.elapsed_seconds.max(0.001),
                        ),
                        report_path: &report_path,
                        json_path: &json_path,
                    },
                )?;
            }
            direct_hdd.push(scenario);
        }
    }

    Ok(PerformanceBenchmarkResults {
        ssd_only,
        ssd_stage_then_drain,
        ssd_pipeline,
        direct_hdd,
    })
}
