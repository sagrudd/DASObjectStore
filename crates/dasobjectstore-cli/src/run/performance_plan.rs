use super::*;

pub(super) use super::performance_tui::{
    render_hdd_drain_tui_snapshot, render_performance_tui_snapshot, HddDrainTuiState,
    PerformanceTuiContext, PerformanceTuiSnapshot,
};
#[allow(unused_imports)]
pub(super) use super::performance_workload::{
    apply_performance_file_order, assign_performance_file_indexes,
    collect_performance_source_files, metadata_modified_unix_nanos, ordered_performance_workload,
    plan_performance_workload, select_performance_source_files, shuffle_performance_payloads,
    source_performance_workload,
};

#[derive(Clone, Copy, Debug)]
pub(super) struct PerformanceMeasurement {
    pub(super) bytes: u64,
    pub(super) seconds: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PerformanceWorkloadKind {
    Generated,
    SourceFolder,
}

impl PerformanceWorkloadKind {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Generated => "generated",
            Self::SourceFolder => "source-folder",
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct PerformancePayload {
    pub(super) file_index: u32,
    pub(super) relative_path: PathBuf,
    pub(super) source_path: Option<PathBuf>,
    pub(super) size_bytes: u64,
    pub(super) modified_unix_nanos: u128,
}

#[derive(Clone, Debug)]
pub(super) struct PerformanceWorkload {
    pub(super) kind: PerformanceWorkloadKind,
    pub(super) source_path: Option<PathBuf>,
    pub(super) source_cap_bytes: Option<u64>,
    pub(super) file_selection: PerformanceFileSelection,
    pub(super) file_order: PerformanceFileOrder,
    pub(super) discovered_file_count: u32,
    pub(super) discovered_total_bytes: u64,
    pub(super) payloads: Vec<PerformancePayload>,
}

impl PerformanceWorkload {
    pub(super) fn file_count(&self) -> u32 {
        self.payloads.len() as u32
    }

    pub(super) fn total_bytes(&self) -> u64 {
        self.payloads
            .iter()
            .map(|payload| payload.size_bytes)
            .sum::<u64>()
    }

    pub(super) fn nominal_file_size(&self) -> u64 {
        match self.payloads.as_slice() {
            [] => 0,
            [payload] => payload.size_bytes,
            payloads => {
                let total = payloads
                    .iter()
                    .map(|payload| payload.size_bytes)
                    .sum::<u64>();
                total / payloads.len() as u64
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct PerformanceFileResult {
    pub(super) file_index: u32,
    pub(super) ssd_write: PerformanceMeasurement,
    pub(super) ssd_read: PerformanceMeasurement,
}

#[derive(Clone, Debug)]
pub(super) struct PerformanceDiskResult {
    pub(super) file_index: u32,
    pub(super) copy_index: usize,
    pub(super) concurrency: usize,
    pub(super) scenario: PerformanceScenarioKind,
    pub(super) disk_id: DiskId,
    pub(super) ssd_read: PerformanceMeasurement,
    pub(super) write: PerformanceMeasurement,
}

#[derive(Clone, Debug)]
pub(super) struct PerformanceIoSample {
    pub(super) elapsed_second: u64,
    pub(super) device_label: String,
    pub(super) device_name: String,
    pub(super) read_bytes_per_second: u64,
    pub(super) write_bytes_per_second: u64,
}

#[derive(Clone, Debug)]
pub(super) struct PerformanceConcurrencyResult {
    pub(super) concurrency: usize,
    pub(super) scenario: PerformanceScenarioKind,
    pub(super) aggregate_bytes: u64,
    pub(super) seconds: f64,
    pub(super) slowest_seconds: f64,
    pub(super) members: Vec<DiskId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PerformanceScenarioKind {
    SsdOnly,
    SsdStageThenDrain,
    SsdPipeline,
    DirectHdd,
}

impl PerformanceScenarioKind {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::SsdOnly => "ssd-only",
            Self::SsdStageThenDrain => "ssd-stage-then-drain",
            Self::SsdPipeline => "ssd-overlap-drain",
            Self::DirectHdd => "direct-hdd",
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::SsdOnly => "SSD-only ingest",
            Self::SsdStageThenDrain => "SSD stage then HDD drain",
            Self::SsdPipeline => "SSD ingest with overlapping HDD drain",
            Self::DirectHdd => "Direct source-to-HDD ingest",
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct PerformanceScenarioPlan {
    pub(super) include_ssd_only: bool,
    pub(super) ssd_stage_then_drain: Vec<usize>,
    pub(super) ssd_pipeline: Vec<usize>,
    pub(super) direct_hdd: Vec<usize>,
}

impl PerformanceScenarioPlan {
    pub(super) fn scenario_total(&self) -> usize {
        usize::from(self.include_ssd_only)
            + self.ssd_stage_then_drain.len()
            + self.ssd_pipeline.len()
            + self.direct_hdd.len()
    }

    pub(super) fn max_concurrency(&self) -> usize {
        self.ssd_stage_then_drain
            .iter()
            .chain(self.ssd_pipeline.iter())
            .chain(self.direct_hdd.iter())
            .copied()
            .max()
            .unwrap_or(0)
    }

    pub(super) fn concurrency_values(&self) -> Vec<usize> {
        self.ssd_stage_then_drain
            .iter()
            .chain(self.ssd_pipeline.iter())
            .chain(self.direct_hdd.iter())
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    pub(super) fn scenario_names(&self) -> Vec<&'static str> {
        let mut names = Vec::new();
        if self.include_ssd_only {
            names.push(PerformanceScenarioKind::SsdOnly.as_str());
        }
        if !self.ssd_stage_then_drain.is_empty() {
            names.push(PerformanceScenarioKind::SsdStageThenDrain.as_str());
        }
        if !self.ssd_pipeline.is_empty() {
            names.push(PerformanceScenarioKind::SsdPipeline.as_str());
        }
        if !self.direct_hdd.is_empty() {
            names.push(PerformanceScenarioKind::DirectHdd.as_str());
        }
        names
    }
}

pub(super) fn plan_performance_scenario_matrix(
    args: &PerformanceTestArgs,
    disk_count: usize,
) -> Result<PerformanceScenarioPlan, CliError> {
    if args.max_hdd_concurrency() == 0 {
        return Err(CliError::CommandFailed(
            "performance-test requires --max-hdd-concurrency greater than 0".to_string(),
        ));
    }
    let concurrency = selected_hdd_concurrency(args, disk_count)?;
    let mut include_ssd_only = false;
    let mut ssd_stage_then_drain = Vec::new();
    let mut ssd_pipeline = Vec::new();
    let mut direct_hdd = Vec::new();

    let selections = if args.scenarios().is_empty() {
        vec![
            PerformanceScenarioSelection::SsdOnly,
            PerformanceScenarioSelection::SsdStageThenDrain,
            PerformanceScenarioSelection::SsdOverlapDrain,
            PerformanceScenarioSelection::DirectHdd,
        ]
    } else {
        args.scenarios()
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
    };

    for selection in selections {
        match selection {
            PerformanceScenarioSelection::SsdOnly => include_ssd_only = true,
            PerformanceScenarioSelection::SsdStageThenDrain => {
                ssd_stage_then_drain = concurrency.clone();
            }
            PerformanceScenarioSelection::SsdOverlapDrain => {
                ssd_pipeline = concurrency.clone();
            }
            PerformanceScenarioSelection::DirectHdd => {
                direct_hdd = concurrency.clone();
            }
        }
    }

    let plan = PerformanceScenarioPlan {
        include_ssd_only,
        ssd_stage_then_drain,
        ssd_pipeline,
        direct_hdd,
    };
    if plan.scenario_total() == 0 {
        return Err(CliError::CommandFailed(
            "performance-test selected no benchmark scenarios".to_string(),
        ));
    }
    Ok(plan)
}

pub(super) fn selected_hdd_concurrency(
    args: &PerformanceTestArgs,
    disk_count: usize,
) -> Result<Vec<usize>, CliError> {
    let selected = if args.hdd_concurrency().is_empty() {
        (1..=args.max_hdd_concurrency()).collect::<Vec<_>>()
    } else {
        args.hdd_concurrency().to_vec()
    };
    let selected = selected.into_iter().collect::<BTreeSet<_>>();
    if selected.contains(&0) {
        return Err(CliError::CommandFailed(
            "performance-test --hdd-concurrency values must be greater than 0".to_string(),
        ));
    }
    if let Some(over_limit) = selected.iter().find(|value| **value > disk_count) {
        return Err(CliError::CommandFailed(format!(
            "performance-test --hdd-concurrency {over_limit} requires at least {over_limit} managed HDD roots; found {disk_count}"
        )));
    }
    Ok(selected.into_iter().collect())
}

pub(super) fn format_concurrency_list(values: &[usize]) -> String {
    if values.is_empty() {
        return "none".to_string();
    }
    values
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

pub(super) fn performance_selected_matrix_bounds(
    workload: &PerformanceWorkload,
    plan: &PerformanceScenarioPlan,
) -> String {
    format!(
        "selected {} file(s), {}; scenarios {}; HDD concurrency {}",
        workload.file_count(),
        format_bytes(workload.total_bytes() as f64),
        plan.scenario_names().join(", "),
        format_concurrency_list(&plan.concurrency_values())
    )
}

pub(super) fn performance_scenario_objective(
    kind: PerformanceScenarioKind,
    concurrency: usize,
) -> String {
    match kind {
        PerformanceScenarioKind::SsdOnly => {
            "measure separated phases: write every selected file to SSD, then read every selected file back from SSD".to_string()
        }
        PerformanceScenarioKind::SsdStageThenDrain => format!(
            "measure separated phases: stage every selected file to SSD, then drain with {concurrency} HDD worker(s)"
        ),
        PerformanceScenarioKind::SsdPipeline => format!(
            "measure overlapping SSD ingest and FIFO HDD drain with {concurrency} worker(s)"
        ),
        PerformanceScenarioKind::DirectHdd => format!(
            "measure direct source-to-HDD landing with {concurrency} worker(s), bypassing SSD"
        ),
    }
}

pub(super) fn performance_scenario_bounds(
    workload: &PerformanceWorkload,
    kind: PerformanceScenarioKind,
    concurrency: usize,
) -> String {
    let cap = workload
        .source_cap_bytes
        .map(|bytes| format!(" cap {}", format_bytes(bytes as f64)))
        .unwrap_or_else(|| " no cap".to_string());
    let file_selection = if workload.kind == PerformanceWorkloadKind::SourceFolder
        && workload.source_cap_bytes.is_some()
    {
        format!(" {} selection", workload.file_selection.as_str())
    } else {
        String::new()
    };
    let selection = if workload.kind == PerformanceWorkloadKind::SourceFolder
        && workload.source_cap_bytes.is_some()
    {
        format!(
            "selected {}/{} file(s), {}/{}{};",
            workload.file_count(),
            workload.discovered_file_count,
            format_bytes(workload.total_bytes() as f64),
            format_bytes(workload.discovered_total_bytes as f64),
            file_selection
        )
    } else {
        format!(
            "selected {} file(s), {};",
            workload.file_count(),
            format_bytes(workload.total_bytes() as f64)
        )
    };
    let file_order = format!(" file order {}", workload.file_order.as_str());
    match kind {
        PerformanceScenarioKind::SsdOnly => format!(
            "{selection}{cap};{file_order}; SSD residency grows to the measured safe SSD budget or selected total {}, whichever is smaller, before each readback batch",
            format_bytes(workload.total_bytes() as f64)
        ),
        PerformanceScenarioKind::SsdStageThenDrain => format!(
            "{selection}{cap};{file_order}; SSD residency grows to the measured safe SSD budget or selected total {}, whichever is smaller, before each HDD drain batch",
            format_bytes(workload.total_bytes() as f64)
        ),
        PerformanceScenarioKind::SsdPipeline => format!(
            "{selection}{cap};{file_order}; HDD drain starts as soon as a staged file is queued; SSD backlog is bounded by measured safe SSD capacity while drain at {concurrency} worker(s) catches up"
        ),
        PerformanceScenarioKind::DirectHdd => {
            format!("{selection}{cap};{file_order}; SSD residency is zero for this scenario")
        }
    }
}

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct DiskIoCounters {
    pub(super) read_sectors: u64,
    pub(super) write_sectors: u64,
}

pub(super) struct PerformanceIoSampler {
    pub(super) stop_sender: Option<mpsc::Sender<()>>,
    pub(super) handle: Option<thread::JoinHandle<Vec<PerformanceIoSample>>>,
}

impl PerformanceIoSampler {
    pub(super) fn start(devices: Vec<(String, PathBuf)>) -> Self {
        #[cfg(target_os = "linux")]
        {
            let devices = devices
                .into_iter()
                .filter_map(|(label, path)| {
                    diskstats_device_for_path(&path).map(|device_name| (label, device_name))
                })
                .collect::<Vec<_>>();
            if devices.is_empty() {
                return Self::disabled();
            }
            let (stop_sender, stop_receiver) = mpsc::channel::<()>();
            let handle =
                thread::spawn(move || sample_disk_io_until_stopped(devices, stop_receiver));
            Self {
                stop_sender: Some(stop_sender),
                handle: Some(handle),
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = devices;
            Self::disabled()
        }
    }

    pub(super) fn disabled() -> Self {
        Self {
            stop_sender: None,
            handle: None,
        }
    }

    pub(super) fn stop(mut self) -> Vec<PerformanceIoSample> {
        self.stop_and_join()
    }

    pub(super) fn stop_and_join(&mut self) -> Vec<PerformanceIoSample> {
        if let Some(sender) = self.stop_sender.take() {
            let _ = sender.send(());
        }
        if let Some(handle) = self.handle.take() {
            return handle.join().unwrap_or_default();
        }
        Vec::new()
    }
}

impl Drop for PerformanceIoSampler {
    fn drop(&mut self) {
        let _ = self.stop_and_join();
    }
}

pub(super) fn performance_io_devices(
    ssd_root: Option<&Path>,
    hdd_roots: &[(DiskId, PathBuf)],
) -> Vec<(String, PathBuf)> {
    let mut devices = Vec::new();
    if let Some(ssd_root) = ssd_root {
        devices.push(("ssd".to_string(), ssd_root.to_path_buf()));
    }
    devices.extend(
        hdd_roots
            .iter()
            .map(|(disk_id, root)| (disk_id.as_str().to_string(), root.clone())),
    );
    devices
}

#[cfg(target_os = "linux")]
pub(super) fn sample_disk_io_until_stopped(
    devices: Vec<(String, String)>,
    stop_receiver: mpsc::Receiver<()>,
) -> Vec<PerformanceIoSample> {
    let mut samples = Vec::new();
    let mut previous = read_proc_diskstats().unwrap_or_default();
    let started = Instant::now();
    let mut previous_sample_at = started;
    loop {
        match stop_receiver.recv_timeout(Duration::from_secs(1)) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let sampled_at = Instant::now();
        let interval_seconds = sampled_at
            .duration_since(previous_sample_at)
            .as_secs_f64()
            .max(0.001);
        let current = read_proc_diskstats().unwrap_or_default();
        let elapsed_second = started.elapsed().as_secs().max(1);
        for (label, device_name) in &devices {
            let Some(previous_counters) = previous.get(device_name) else {
                continue;
            };
            let Some(current_counters) = current.get(device_name) else {
                continue;
            };
            samples.push(PerformanceIoSample {
                elapsed_second,
                device_label: label.clone(),
                device_name: device_name.clone(),
                read_bytes_per_second: ((current_counters
                    .read_sectors
                    .saturating_sub(previous_counters.read_sectors)
                    .saturating_mul(DISKSTAT_SECTOR_BYTES)
                    as f64)
                    / interval_seconds)
                    .round() as u64,
                write_bytes_per_second: ((current_counters
                    .write_sectors
                    .saturating_sub(previous_counters.write_sectors)
                    .saturating_mul(DISKSTAT_SECTOR_BYTES)
                    as f64)
                    / interval_seconds)
                    .round() as u64,
            });
        }
        previous = current;
        previous_sample_at = sampled_at;
    }
    samples
}

#[cfg(target_os = "linux")]
pub(super) const DISKSTAT_SECTOR_BYTES: u64 = 512;

#[cfg(target_os = "linux")]
pub(super) fn read_proc_diskstats() -> io::Result<BTreeMap<String, DiskIoCounters>> {
    fs::read_to_string("/proc/diskstats").map(|contents| parse_proc_diskstats(&contents))
}

#[cfg(target_os = "linux")]
pub(super) fn parse_proc_diskstats(contents: &str) -> BTreeMap<String, DiskIoCounters> {
    let mut counters = BTreeMap::new();
    for line in contents.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 10 {
            continue;
        }
        let Ok(read_sectors) = fields[5].parse::<u64>() else {
            continue;
        };
        let Ok(write_sectors) = fields[9].parse::<u64>() else {
            continue;
        };
        counters.insert(
            fields[2].to_string(),
            DiskIoCounters {
                read_sectors,
                write_sectors,
            },
        );
    }
    counters
}

#[cfg(target_os = "linux")]
pub(super) fn diskstats_device_for_path(path: &Path) -> Option<String> {
    let output = ProcessCommand::new("df")
        .arg("-P")
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mount_device = stdout.lines().nth(1)?.split_whitespace().next()?.trim();
    let device_name = Path::new(mount_device)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(mount_device)
        .trim_start_matches("/dev/");
    if device_name.is_empty() {
        None
    } else {
        Some(device_name.to_string())
    }
}

#[derive(Clone, Debug)]
pub(super) struct PerformanceScenarioResult {
    pub(super) kind: PerformanceScenarioKind,
    pub(super) file_order: PerformanceFileOrder,
    pub(super) concurrency: usize,
    pub(super) redundancy: usize,
    pub(super) queue_capacity: usize,
    pub(super) elapsed_seconds: f64,
    pub(super) total_bytes: u64,
    pub(super) logical_source_bytes: u64,
    pub(super) physical_hdd_write_bytes: u64,
    pub(super) hdd_write_operations: usize,
    pub(super) hdd_drain_started_before_all_ssd_staged: bool,
    pub(super) file_results: Vec<PerformanceFileResult>,
    pub(super) disk_results: Vec<PerformanceDiskResult>,
    pub(super) io_samples: Vec<PerformanceIoSample>,
    pub(super) concurrency_result: PerformanceConcurrencyResult,
}

#[derive(Clone, Debug)]
pub(super) struct PerformanceBenchmarkResults {
    pub(super) ssd_only: Vec<PerformanceScenarioResult>,
    pub(super) ssd_stage_then_drain: Vec<PerformanceScenarioResult>,
    pub(super) ssd_pipeline: Vec<PerformanceScenarioResult>,
    pub(super) direct_hdd: Vec<PerformanceScenarioResult>,
}

#[derive(Clone, Debug)]
pub(super) struct PerformanceRecommendation {
    pub(super) strategy: PerformanceScenarioKind,
    pub(super) file_order: PerformanceFileOrder,
    pub(super) hdd_concurrency: usize,
    pub(super) aggregate_bytes_per_second: f64,
    pub(super) reason: String,
}

#[derive(Clone, Debug)]
pub(super) struct PerformanceReport {
    pub(super) run_id: String,
    pub(super) generated_at_utc: String,
    pub(super) repository_revision: String,
    pub(super) file_size: u64,
    pub(super) file_count: u32,
    pub(super) workload_kind: PerformanceWorkloadKind,
    pub(super) source_path: Option<PathBuf>,
    pub(super) source_cap_bytes: Option<u64>,
    pub(super) file_selection: PerformanceFileSelection,
    pub(super) file_orders: Vec<PerformanceFileOrder>,
    pub(super) discovered_file_count: u32,
    pub(super) discovered_total_bytes: u64,
    pub(super) total_source_bytes: u64,
    pub(super) ssd_root: PathBuf,
    pub(super) hdd_root: PathBuf,
    pub(super) disk_count: usize,
    pub(super) max_concurrency: usize,
    pub(super) redundancy: usize,
    pub(super) elapsed_seconds: f64,
    pub(super) results: PerformanceBenchmarkResults,
    pub(super) recommendation: PerformanceRecommendation,
    pub(super) authoritative: bool,
    pub(super) authoritative_path: Option<PathBuf>,
    pub(super) tmp_dir: PathBuf,
    pub(super) disks: Vec<(DiskId, PathBuf)>,
    pub(super) reproduction_args: Vec<String>,
    pub(super) keep_temp: bool,
    pub(super) json_path: PathBuf,
    pub(super) qr_path: PathBuf,
    pub(super) pdf_path: PathBuf,
    pub(super) reproduce_command: String,
    pub(super) reproduction_payload_sha256: String,
    pub(super) qr_status: String,
}

pub(super) fn performance_test_reproduction_args(
    args: &PerformanceTestArgs,
    ssd_root: &Path,
    hdd_root: &Path,
    tmp_dir: &Path,
    report_path: &Path,
) -> Vec<String> {
    let mut command = vec!["dasobjectstore".to_string(), "performance-test".to_string()];
    if let Some(source) = args.source() {
        command.push("--source".to_string());
        command.push(source.display().to_string());
        if let Some(cap) = args.cap() {
            command.push("--cap".to_string());
            command.push(cap.to_string());
        }
        command.push("--file_select".to_string());
        command.push(args.file_select().as_str().to_string());
    } else {
        if let Some(file_size) = args.file_size() {
            command.push("--file_size".to_string());
            command.push(file_size.to_string());
        }
        if let Some(file_count) = args.file_count() {
            command.push("--file_count".to_string());
            command.push(file_count.to_string());
        }
    }
    for file_order in args.file_orders() {
        command.push("--file_order".to_string());
        command.push(file_order.as_str().to_string());
    }
    for scenario in args.scenarios() {
        command.push("--scenario".to_string());
        command.push(performance_scenario_selection_name(*scenario).to_string());
    }
    if !args.hdd_concurrency().is_empty() {
        command.push("--hdd-concurrency".to_string());
        command.push(format_concurrency_list(args.hdd_concurrency()));
    }
    command.extend([
        "--max-hdd-concurrency".to_string(),
        args.max_hdd_concurrency().to_string(),
        "--redundancy".to_string(),
        args.redundancy().to_string(),
        "--ssd-root".to_string(),
        ssd_root.display().to_string(),
        "--hdd-root".to_string(),
        hdd_root.display().to_string(),
        "--tmp-dir".to_string(),
        tmp_dir.display().to_string(),
        "--report".to_string(),
        report_path.display().to_string(),
    ]);
    if let Some(json_artifact) = args.json_artifact() {
        command.push("--json-artifact".to_string());
        command.push(json_artifact.display().to_string());
    }
    if args.authoritative() {
        command.push("--authoritative".to_string());
    }
    if args.keep_temp() {
        command.push("--keep-temp".to_string());
    }
    command
}

pub(super) fn performance_scenario_selection_name(
    selection: PerformanceScenarioSelection,
) -> &'static str {
    match selection {
        PerformanceScenarioSelection::SsdOnly => "ssd-only",
        PerformanceScenarioSelection::SsdStageThenDrain => "ssd-stage-then-drain",
        PerformanceScenarioSelection::SsdOverlapDrain => "ssd-overlap-drain",
        PerformanceScenarioSelection::DirectHdd => "direct-hdd",
    }
}

pub(super) fn validate_pdf_report_path(path: &Path) -> Result<(), CliError> {
    let is_pdf = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"));
    if is_pdf {
        Ok(())
    } else {
        Err(CliError::CommandFailed(format!(
            "performance-test --report must be a PDF path ending in .pdf; got {}",
            path.display()
        )))
    }
}

pub(super) fn shell_join(args: &[String]) -> String {
    args.iter()
        .map(|argument| shell_quote(argument))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn shell_quote(argument: &str) -> String {
    if argument
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "-_./:=+".contains(character))
    {
        return argument.to_string();
    }
    format!("'{}'", argument.replace('\'', "'\\''"))
}

pub(super) fn parse_binary_size(value: &str) -> Result<u64, CliError> {
    let trimmed = value.trim();
    let number_end = trimmed
        .find(|character: char| !(character.is_ascii_digit() || character == '.'))
        .unwrap_or(trimmed.len());
    let (number, unit) = trimmed.split_at(number_end);
    if number.is_empty() {
        return Err(CliError::CommandFailed(format!(
            "invalid size '{value}'; expected e.g. 100MiB, 1GiB, 1.1TiB"
        )));
    }
    let number = number.parse::<f64>().map_err(|_| {
        CliError::CommandFailed(format!(
            "invalid size '{value}'; expected e.g. 100MiB, 1GiB, 1.1TiB"
        ))
    })?;
    let multiplier = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1_f64,
        "kib" | "ki" => 1024_f64,
        "mib" | "mi" => 1024_f64.powi(2),
        "gib" | "gi" => 1024_f64.powi(3),
        "tib" | "ti" => 1024_f64.powi(4),
        "kb" | "k" => 1000_f64,
        "mb" | "m" => 1000_f64.powi(2),
        "gb" | "g" => 1000_f64.powi(3),
        "tb" | "t" => 1000_f64.powi(4),
        _ => {
            return Err(CliError::CommandFailed(format!(
                "invalid size unit '{unit}' in '{value}'"
            )));
        }
    };
    let bytes = number * multiplier;
    if !bytes.is_finite() || bytes <= 0.0 || bytes > u64::MAX as f64 {
        return Err(CliError::CommandFailed(format!(
            "invalid size '{value}'; byte count is out of range"
        )));
    }
    Ok(bytes.round() as u64)
}

pub(super) fn require_admin_for_performance_test() -> Result<(), CliError> {
    if current_user_is_root()? {
        return Ok(());
    }

    Err(CliError::CommandFailed(
        "performance-test requires an administrative user because it performs sustained direct DAS IO; rerun with sudo".to_string(),
    ))
}

#[derive(Debug)]
pub(super) struct PerformanceTemporaryObjectStore {
    pub(super) ssd_root: PathBuf,
    pub(super) hdd_roots: Vec<PathBuf>,
    pub(super) keep: bool,
}

impl PerformanceTemporaryObjectStore {
    pub(super) fn new(
        ssd_root: PathBuf,
        hdd_roots: Vec<PathBuf>,
        keep: bool,
        run_id: &str,
    ) -> Result<Self, CliError> {
        let temporary = Self {
            ssd_root,
            hdd_roots,
            keep,
        };
        temporary.write_gc_markers(run_id, "active")?;
        Ok(temporary)
    }

    fn write_gc_markers(&self, run_id: &str, state: &str) -> Result<(), CliError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .to_string();
        let marker = dasobjectstore_daemon::runtime::PerformanceGcMarker {
            schema: dasobjectstore_daemon::runtime::PERFORMANCE_GC_MARKER_SCHEMA.to_string(),
            run_id: run_id.to_string(),
            state: state.to_string(),
            keep_temp: self.keep,
            created_at_utc: now.clone(),
            updated_at_utc: now,
        };
        let encoded = serde_json::to_vec_pretty(&marker)?;
        for root in std::iter::once(&self.ssd_root).chain(self.hdd_roots.iter()) {
            fs::write(
                root.join(dasobjectstore_daemon::runtime::PERFORMANCE_GC_MARKER_FILE),
                &encoded,
            )?;
        }
        Ok(())
    }
}

impl Drop for PerformanceTemporaryObjectStore {
    fn drop(&mut self) {
        let run_id = self
            .ssd_root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown")
            .to_string();
        let _ = self.write_gc_markers(&run_id, if self.keep { "retained" } else { "complete" });
        if self.keep {
            return;
        }
        let _ = fs::remove_dir_all(&self.ssd_root);
        for root in &self.hdd_roots {
            let _ = fs::remove_dir_all(root);
        }
    }
}

#[derive(Debug)]
pub(super) struct PerformanceGeneratedSource {
    pub(super) root: PathBuf,
}

impl PerformanceGeneratedSource {
    pub(super) fn new(root: PathBuf) -> Result<Self, CliError> {
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

pub(super) fn materialize_generated_performance_workload(
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
pub(super) fn check_performance_cancelled() -> Result<(), CliError> {
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
pub(super) fn check_performance_cancelled() -> Result<(), CliError> {
    Ok(())
}
