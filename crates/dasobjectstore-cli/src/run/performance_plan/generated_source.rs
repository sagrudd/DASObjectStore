use super::*;

#[derive(Debug)]
pub(in crate::run) struct PerformanceGeneratedSource {
    pub(in crate::run) root: PathBuf,
}

impl PerformanceGeneratedSource {
    fn new(root: PathBuf) -> Result<Self, CliError> {
        if root.exists() {
            fs::remove_dir_all(&root)?;
        }
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }
}

impl Drop for PerformanceGeneratedSource {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub(in crate::run) fn materialize_generated_performance_workload(
    workload: &mut PerformanceWorkload,
    tmp_dir: &Path,
    run_id: &str,
    writer: &mut dyn Write,
    tui: bool,
    report_path: &Path,
    json_path: &Path,
    scenario_total: usize,
) -> Result<Option<PerformanceGeneratedSource>, CliError> {
    if workload.kind != PerformanceWorkloadKind::Generated {
        return Ok(None);
    }

    let source = PerformanceGeneratedSource::new(
        tmp_dir.join(format!("dasobjectstore-performance-source-{run_id}")),
    )?;
    let total_bytes = workload.total_bytes();
    if !tui {
        writeln!(
            writer,
            "performance-test: generating {} random source file(s), {} total, under {}",
            workload.file_count(),
            format_bytes(total_bytes as f64),
            source.root.display()
        )?;
    }

    let mut completed_bytes = 0_u64;
    let payload_count = workload.file_count();
    for payload in &mut workload.payloads {
        check_performance_cancelled()?;
        let file_index = payload.file_index;
        let destination = source.root.join(&payload.relative_path);
        let mut progress = |written: u64, seconds: f64| -> Result<(), CliError> {
            if tui {
                render_performance_tui_snapshot(
                    writer,
                    &PerformanceTuiSnapshot {
                        phase: "generating source",
                        scenario: "source-prep",
                        activity: format!(
                            "Generating source file {}/{}",
                            file_index + 1,
                            payload_count
                        ),
                        objective: "create all generated random source files before benchmark upload begins".to_string(),
                        bounds: format!(
                            "generated workload; {} file(s), {} total; source files are removed after completion or cancellation",
                            payload_count,
                            format_bytes(total_bytes as f64)
                        ),
                        scenario_done: 0,
                        scenario_total,
                        file_done: file_index,
                        current_file: Some(file_index + 1),
                        file_count: payload_count,
                        processed_bytes: completed_bytes.saturating_add(written),
                        total_bytes,
                        hdd_concurrency: 0,
                        current_rate: Some(written as f64 / seconds.max(0.001)),
                        ssd_write_rate: None,
                        ssd_read_rate: None,
                        hdd_write_rate: None,
                        hdd_disk_rates: Vec::new(),
                        active_hdd_landing: Vec::new(),
                        aggregate_rate: None,
                        report_path,
                        json_path,
                    },
                )?;
            }
            Ok(())
        };
        measure_generate_random_file_with_progress(
            &destination,
            payload.size_bytes,
            file_index,
            Some(&mut progress),
            PerformanceCopySyncPolicy::SyncAll,
        )?;
        payload.source_path = Some(destination);
        completed_bytes = completed_bytes.saturating_add(payload.size_bytes);
    }

    if tui {
        render_performance_tui_snapshot(
            writer,
            &PerformanceTuiSnapshot {
                phase: "source generation complete",
                scenario: "source-prep",
                activity: "Generated source workload is ready for benchmark upload".to_string(),
                objective: "create all generated random source files before benchmark upload begins"
                    .to_string(),
                bounds: format!(
                    "generated workload; {} file(s), {} total; source files are removed after completion or cancellation",
                    payload_count,
                    format_bytes(total_bytes as f64)
                ),
                scenario_done: 0,
                scenario_total,
                file_done: payload_count,
                current_file: None,
                file_count: payload_count,
                processed_bytes: completed_bytes,
                total_bytes,
                hdd_concurrency: 0,
                current_rate: None,
                ssd_write_rate: None,
                ssd_read_rate: None,
                hdd_write_rate: None,
                hdd_disk_rates: Vec::new(),
                active_hdd_landing: Vec::new(),
                aggregate_rate: None,
                report_path,
                json_path,
            },
        )?;
    }

    Ok(Some(source))
}

#[cfg(unix)]
pub(in crate::run) fn check_performance_cancelled() -> Result<(), CliError> {
    if UPLOAD_CANCELLED.load(Ordering::SeqCst) {
        Err(CliError::CommandFailed(
            "performance-test cancelled by Ctrl-C; temporary objectstore cleanup requested"
                .to_string(),
        ))
    } else {
        Ok(())
    }
}

#[cfg(not(unix))]
pub(in crate::run) fn check_performance_cancelled() -> Result<(), CliError> {
    Ok(())
}
