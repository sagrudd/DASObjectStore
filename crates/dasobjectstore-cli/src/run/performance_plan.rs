use super::*;

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

pub(super) fn plan_performance_workload(
    args: &PerformanceTestArgs,
) -> Result<PerformanceWorkload, CliError> {
    let cap_bytes = args.cap().map(parse_binary_size).transpose()?;
    let primary_file_order = args
        .file_orders()
        .first()
        .copied()
        .unwrap_or(PerformanceFileOrder::SizeDesc);
    match (args.source(), args.file_size(), args.file_count()) {
        (Some(source), None, None) => {
            source_performance_workload(source, cap_bytes, args.file_select(), primary_file_order)
        }
        (None, Some(file_size), Some(file_count)) => {
            if cap_bytes.is_some() {
                return Err(CliError::CommandFailed(
                    "performance-test --cap can only be used with --source".to_string(),
                ));
            }
            if file_count == 0 {
                return Err(CliError::CommandFailed(
                    "performance-test requires --file_count greater than 0".to_string(),
                ));
            }
            let size_bytes = parse_binary_size(file_size)?;
            let payloads = (0..file_count)
                .map(|file_index| PerformancePayload {
                    file_index,
                    relative_path: PathBuf::from(format!("generated-{file_index:05}.bin")),
                    source_path: None,
                    size_bytes,
                    modified_unix_nanos: u128::from(file_index),
                })
                .collect::<Vec<_>>();
            let mut payloads = payloads;
            apply_performance_file_order(&mut payloads, primary_file_order);
            assign_performance_file_indexes(&mut payloads);
            Ok(PerformanceWorkload {
                kind: PerformanceWorkloadKind::Generated,
                source_path: None,
                source_cap_bytes: None,
                file_selection: args.file_select(),
                file_order: primary_file_order,
                discovered_file_count: file_count,
                discovered_total_bytes: size_bytes.saturating_mul(u64::from(file_count)),
                payloads,
            })
        }
        (Some(_), Some(_), _) | (Some(_), _, Some(_)) => Err(CliError::CommandFailed(
            "performance-test accepts either --source or --file_size/--file_count, not both"
                .to_string(),
        )),
        (None, _, _) => Err(CliError::CommandFailed(
            "performance-test requires either --source <DIR> or both --file_size and --file_count"
                .to_string(),
        )),
    }
}

pub(super) fn source_performance_workload(
    source: &Path,
    cap_bytes: Option<u64>,
    file_selection: PerformanceFileSelection,
    file_order: PerformanceFileOrder,
) -> Result<PerformanceWorkload, CliError> {
    if !source.is_dir() {
        return Err(CliError::CommandFailed(format!(
            "performance-test source {} is not a directory",
            source.display()
        )));
    }
    let mut files = Vec::new();
    collect_performance_source_files(source, source, &mut files)?;
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    let discovered_file_count = files.len();
    let discovered_total_bytes = files.iter().map(|payload| payload.size_bytes).sum::<u64>();
    if files.is_empty() {
        return Err(CliError::CommandFailed(format!(
            "performance-test source {} contains no files",
            source.display()
        )));
    }
    if files.len() > u32::MAX as usize {
        return Err(CliError::CommandFailed(format!(
            "performance-test source {} contains more than {} files",
            source.display(),
            u32::MAX
        )));
    }
    if let Some(cap_bytes) = cap_bytes {
        files = select_performance_source_files(files, cap_bytes, file_selection, source)?;
    }
    apply_performance_file_order(&mut files, file_order);
    assign_performance_file_indexes(&mut files);
    Ok(PerformanceWorkload {
        kind: PerformanceWorkloadKind::SourceFolder,
        source_path: Some(source.to_path_buf()),
        source_cap_bytes: cap_bytes,
        file_selection,
        file_order,
        discovered_file_count: discovered_file_count as u32,
        discovered_total_bytes,
        payloads: files,
    })
}

pub(super) fn select_performance_source_files(
    mut files: Vec<PerformancePayload>,
    cap_bytes: u64,
    file_selection: PerformanceFileSelection,
    source: &Path,
) -> Result<Vec<PerformancePayload>, CliError> {
    match file_selection {
        PerformanceFileSelection::Random => shuffle_performance_payloads(&mut files),
        PerformanceFileSelection::Smaller => files.sort_by(|left, right| {
            left.size_bytes
                .cmp(&right.size_bytes)
                .then_with(|| left.relative_path.cmp(&right.relative_path))
        }),
        PerformanceFileSelection::Larger => files.sort_by(|left, right| {
            right
                .size_bytes
                .cmp(&left.size_bytes)
                .then_with(|| left.relative_path.cmp(&right.relative_path))
        }),
    }

    let mut selected = Vec::new();
    let mut selected_bytes = 0_u64;
    for payload in files {
        let next_bytes = selected_bytes.saturating_add(payload.size_bytes);
        if next_bytes <= cap_bytes {
            selected_bytes = next_bytes;
            selected.push(payload);
        }
    }
    if selected.is_empty() {
        return Err(CliError::CommandFailed(format!(
            "performance-test --cap {} is smaller than every selectable source file in {}",
            format_bytes(cap_bytes as f64),
            source.display()
        )));
    }
    Ok(selected)
}

pub(super) fn ordered_performance_workload(
    workload: &PerformanceWorkload,
    file_order: PerformanceFileOrder,
) -> PerformanceWorkload {
    let mut ordered = workload.clone();
    ordered.file_order = file_order;
    apply_performance_file_order(&mut ordered.payloads, file_order);
    assign_performance_file_indexes(&mut ordered.payloads);
    ordered
}

pub(super) fn apply_performance_file_order(
    files: &mut [PerformancePayload],
    file_order: PerformanceFileOrder,
) {
    match file_order {
        PerformanceFileOrder::Fifo => {
            files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        }
        PerformanceFileOrder::SizeAsc => files.sort_by(|left, right| {
            left.size_bytes
                .cmp(&right.size_bytes)
                .then_with(|| left.relative_path.cmp(&right.relative_path))
        }),
        PerformanceFileOrder::SizeDesc => files.sort_by(|left, right| {
            right
                .size_bytes
                .cmp(&left.size_bytes)
                .then_with(|| left.relative_path.cmp(&right.relative_path))
        }),
        PerformanceFileOrder::TimeAsc => files.sort_by(|left, right| {
            left.modified_unix_nanos
                .cmp(&right.modified_unix_nanos)
                .then_with(|| left.relative_path.cmp(&right.relative_path))
        }),
        PerformanceFileOrder::TimeDesc => files.sort_by(|left, right| {
            right
                .modified_unix_nanos
                .cmp(&left.modified_unix_nanos)
                .then_with(|| left.relative_path.cmp(&right.relative_path))
        }),
    }
}

pub(super) fn assign_performance_file_indexes(files: &mut [PerformancePayload]) {
    for (index, payload) in files.iter_mut().enumerate() {
        payload.file_index = index as u32;
    }
}

pub(super) fn shuffle_performance_payloads(files: &mut [PerformancePayload]) {
    let mut rng = OsRng;
    for index in (1..files.len()).rev() {
        let swap_index = (rng.next_u64() % (index as u64 + 1)) as usize;
        files.swap(index, swap_index);
    }
}

pub(super) fn collect_performance_source_files(
    root: &Path,
    current: &Path,
    files: &mut Vec<PerformancePayload>,
) -> Result<(), CliError> {
    let mut entries = fs::read_dir(current)?.collect::<Result<Vec<_>, io::Error>>()?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            collect_performance_source_files(root, &path, files)?;
        } else if metadata.is_file() {
            let relative_path = path
                .strip_prefix(root)
                .map_err(|err| CliError::CommandFailed(err.to_string()))?
                .to_path_buf();
            files.push(PerformancePayload {
                file_index: 0,
                relative_path,
                source_path: Some(path),
                size_bytes: metadata.len(),
                modified_unix_nanos: metadata_modified_unix_nanos(&metadata),
            });
        }
    }
    Ok(())
}

pub(super) fn metadata_modified_unix_nanos(metadata: &fs::Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
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
    pub(super) fn new(ssd_root: PathBuf, hdd_roots: Vec<PathBuf>, keep: bool) -> Self {
        Self {
            ssd_root,
            hdd_roots,
            keep,
        }
    }
}

impl Drop for PerformanceTemporaryObjectStore {
    fn drop(&mut self) {
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

pub(super) struct PerformanceTuiSnapshot<'a> {
    pub(super) phase: &'a str,
    pub(super) scenario: &'a str,
    pub(super) activity: String,
    pub(super) objective: String,
    pub(super) bounds: String,
    pub(super) scenario_done: usize,
    pub(super) scenario_total: usize,
    pub(super) file_done: u32,
    pub(super) current_file: Option<u32>,
    pub(super) file_count: u32,
    pub(super) processed_bytes: u64,
    pub(super) total_bytes: u64,
    pub(super) hdd_concurrency: usize,
    pub(super) current_rate: Option<f64>,
    pub(super) ssd_write_rate: Option<f64>,
    pub(super) ssd_read_rate: Option<f64>,
    pub(super) hdd_write_rate: Option<f64>,
    pub(super) hdd_disk_rates: Vec<String>,
    pub(super) active_hdd_landing: Vec<String>,
    pub(super) aggregate_rate: Option<f64>,
    pub(super) report_path: &'a Path,
    pub(super) json_path: &'a Path,
}

#[derive(Clone, Copy)]
pub(super) struct PerformanceTuiContext<'a> {
    pub(super) scenario_done: usize,
    pub(super) scenario_total: usize,
    pub(super) report_path: &'a Path,
    pub(super) json_path: &'a Path,
}

pub(super) fn render_performance_tui_snapshot(
    writer: &mut (impl Write + ?Sized),
    snapshot: &PerformanceTuiSnapshot<'_>,
) -> Result<(), CliError> {
    let visible_active_landing_rows = snapshot.active_hdd_landing.len().min(8);
    let hidden_active_landing_rows = snapshot
        .active_hdd_landing
        .len()
        .saturating_sub(visible_active_landing_rows);
    let landing_height = if snapshot.active_hdd_landing.is_empty() {
        5
    } else {
        (3 + visible_active_landing_rows + usize::from(hidden_active_landing_rows > 0)).clamp(6, 12)
            as u16
    };
    let mut area = performance_tui_area();
    area.height = area.height.max(landing_height.saturating_add(31));
    let backend = TestBackend::new(area.width, area.height);
    let mut terminal =
        Terminal::new(backend).expect("test backend terminal creation is infallible");
    let current_fraction = if snapshot.file_count == 0 {
        0.0
    } else {
        f64::from(snapshot.file_done.min(snapshot.file_count)) / f64::from(snapshot.file_count)
    };
    let percent = if snapshot.scenario_total == 0 {
        0
    } else {
        ((((snapshot.scenario_done as f64 + current_fraction) / snapshot.scenario_total as f64)
            * 100.0)
            .round()
            .clamp(0.0, 100.0)) as u16
    };
    terminal.draw(|frame| {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(6),
                Constraint::Length(3),
                Constraint::Length(7),
                Constraint::Length(landing_height),
                Constraint::Length(6),
                Constraint::Length(5),
                Constraint::Min(4),
            ])
            .split(frame.area());
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(vec![Span::styled(
                    "DASObjectStore Performance Test",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )]),
                Line::from(format!("Phase: {}", snapshot.phase)),
                Line::from(format!("Scenario: {}", snapshot.scenario)),
                Line::from(format!("Now: {}", snapshot.activity)),
            ])
            .block(Block::default().borders(Borders::ALL).title("Context")),
            chunks[0],
        );
        frame.render_widget(
            Gauge::default()
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Scenario Progress"),
                )
                .gauge_style(Style::default().fg(Color::Green))
                .percent(percent),
            chunks[1],
        );
        let rate = snapshot
            .aggregate_rate
            .map(|rate| format!("{}/s", format_bytes(rate)))
            .unwrap_or_else(|| "pending".to_string());
        let current_rate = snapshot
            .current_rate
            .map(|rate| format!("{}/s", format_bytes(rate)))
            .unwrap_or_else(|| "pending".to_string());
        let inferred_ssd_write_rate = snapshot.ssd_write_rate.or_else(|| {
            if snapshot.phase.contains("staging")
                || snapshot.activity.starts_with("Writing")
                || snapshot.activity.starts_with("Queued")
            {
                snapshot.current_rate
            } else {
                None
            }
        });
        let inferred_ssd_read_rate = snapshot.ssd_read_rate.or_else(|| {
            if snapshot.phase.contains("readback") || snapshot.activity.starts_with("Reading") {
                snapshot.current_rate
            } else {
                None
            }
        });
        let ssd_write_rate = inferred_ssd_write_rate
            .map(|rate| format!("{}/s", format_bytes(rate)))
            .unwrap_or_else(|| "pending".to_string());
        let ssd_read_rate = inferred_ssd_read_rate
            .map(|rate| format!("{}/s", format_bytes(rate)))
            .unwrap_or_else(|| "pending".to_string());
        let hdd_write_rate = snapshot
            .hdd_write_rate
            .map(|rate| format!("{}/s", format_bytes(rate)))
            .unwrap_or_else(|| "pending".to_string());
        let hdd_disk_rates = if snapshot.hdd_disk_rates.is_empty() {
            "pending".to_string()
        } else {
            snapshot.hdd_disk_rates.join("; ")
        };
        let active_landing_lines = if snapshot.active_hdd_landing.is_empty() {
            vec![Line::from("Active landing: idle")]
        } else {
            std::iter::once(Line::from("Active landing:"))
                .chain(
                    snapshot
                        .active_hdd_landing
                        .iter()
                        .take(visible_active_landing_rows)
                        .map(|line| Line::from(format!("  {line}"))),
                )
                .chain((hidden_active_landing_rows > 0).then(|| {
                    Line::from(format!(
                        "  ... {hidden_active_landing_rows} more active transfer(s)"
                    ))
                }))
                .collect::<Vec<_>>()
        };
        let current_file = snapshot
            .current_file
            .map(|file| file.to_string())
            .unwrap_or_else(|| "-".to_string());
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(format!(
                    "Scenarios: {}/{}",
                    snapshot.scenario_done, snapshot.scenario_total
                )),
                Line::from(format!(
                    "Current scenario files: {}/{} (active {})",
                    snapshot.file_done, snapshot.file_count, current_file
                )),
                Line::from(format!(
                    "Current scenario bytes: {}/{}",
                    format_bytes(snapshot.processed_bytes as f64),
                    format_bytes(snapshot.total_bytes as f64)
                )),
                Line::from(format!("HDD concurrency: {}", snapshot.hdd_concurrency)),
                Line::from(format!("Current operation rate: {current_rate}")),
            ])
            .block(Block::default().borders(Borders::ALL).title("Workload")),
            chunks[2],
        );
        frame.render_widget(
            Paragraph::new(active_landing_lines)
                .wrap(Wrap { trim: true })
                .block(Block::default().borders(Borders::ALL).title("HDD Landing")),
            chunks[3],
        );
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(format!("SSD write rate: {ssd_write_rate}")),
                Line::from(format!("SSD read rate: {ssd_read_rate}")),
                Line::from(format!("HDD aggregate average: {hdd_write_rate}")),
                Line::from(format!("HDD active disk writes: {hdd_disk_rates}")),
                Line::from(format!("Scenario aggregate rate: {rate}")),
            ])
            .wrap(Wrap { trim: true })
            .block(Block::default().borders(Borders::ALL).title("Rates")),
            chunks[4],
        );
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(format!("Objective: {}", snapshot.objective)),
                Line::from(format!("Bounds: {}", snapshot.bounds)),
                Line::from("Ctrl-C requests cancellation and temporary objectstore cleanup."),
                Line::from("SSD pipeline scenarios stage to SSD while HDD drain workers consume the FIFO queue."),
            ])
            .wrap(Wrap { trim: true })
            .block(Block::default().borders(Borders::ALL).title("Scenario Details")),
            chunks[5],
        );
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(format!("PDF: {}", snapshot.report_path.display())),
                Line::from(format!("JSON: {}", snapshot.json_path.display())),
            ])
            .block(Block::default().borders(Borders::ALL).title("Artifacts")),
            chunks[6],
        );
    })
    .expect("test backend drawing is infallible");
    write!(writer, "\x1b[2J\x1b[H")?;
    let buffer = terminal.backend().buffer();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            write!(writer, "{}", buffer[(x, y)].symbol())?;
        }
        if y + 1 < buffer.area.height {
            write!(writer, "\r\n")?;
        }
    }
    writer.flush()?;
    Ok(())
}

pub(super) struct HddDrainTuiState<'a> {
    pub(super) context: PerformanceTuiContext<'a>,
    pub(super) workload: &'a PerformanceWorkload,
    pub(super) kind: PerformanceScenarioKind,
    pub(super) concurrency: usize,
    pub(super) submitted_jobs: usize,
    pub(super) total_jobs: usize,
    pub(super) started_jobs: usize,
    pub(super) completed_jobs: usize,
    pub(super) transferred_bytes: u64,
    pub(super) ssd_read_rate: Option<f64>,
    pub(super) hdd_write_rate: Option<f64>,
    pub(super) hdd_disk_rates: Vec<String>,
    pub(super) active_hdd_landing: Vec<String>,
}

pub(super) fn render_hdd_drain_tui_snapshot(
    writer: &mut impl Write,
    state: HddDrainTuiState<'_>,
) -> Result<(), CliError> {
    let active_jobs = state.started_jobs.saturating_sub(state.completed_jobs);
    let queued_jobs = state.submitted_jobs.saturating_sub(state.started_jobs);
    let pending_submission = state.total_jobs.saturating_sub(state.submitted_jobs);
    let total_bytes = state.workload.total_bytes().saturating_mul(
        state
            .total_jobs
            .checked_div(state.workload.file_count().max(1) as usize)
            .unwrap_or(1)
            .max(1) as u64,
    );
    render_performance_tui_snapshot(
        writer,
        &PerformanceTuiSnapshot {
            phase: "hdd-drain active",
            scenario: state.kind.as_str(),
            activity: format!(
                "HDD drain copy jobs: drained {}/{}, draining {}, queued {}, pending submission {}",
                state.completed_jobs,
                state.total_jobs,
                active_jobs,
                queued_jobs,
                pending_submission
            ),
            objective: performance_scenario_objective(state.kind, state.concurrency),
            bounds: performance_scenario_bounds(state.workload, state.kind, state.concurrency),
            scenario_done: state.context.scenario_done,
            scenario_total: state.context.scenario_total,
            file_done: state.completed_jobs.min(u32::MAX as usize) as u32,
            current_file: None,
            file_count: state.total_jobs.min(u32::MAX as usize) as u32,
            processed_bytes: state.transferred_bytes,
            total_bytes,
            hdd_concurrency: state.concurrency,
            current_rate: state.hdd_write_rate,
            ssd_write_rate: None,
            ssd_read_rate: state.ssd_read_rate,
            hdd_write_rate: state.hdd_write_rate,
            hdd_disk_rates: state.hdd_disk_rates,
            active_hdd_landing: state.active_hdd_landing,
            aggregate_rate: state.hdd_write_rate,
            report_path: state.context.report_path,
            json_path: state.context.json_path,
        },
    )
}

pub(super) fn performance_tui_area() -> Rect {
    let env_size = std::env::var("COLUMNS")
        .ok()
        .and_then(|columns| columns.parse::<u16>().ok())
        .zip(
            std::env::var("LINES")
                .ok()
                .and_then(|lines| lines.parse::<u16>().ok()),
        );
    let (width, height) = env_size
        .or_else(performance_terminal_size_from_ioctl)
        .unwrap_or((110, 24));
    Rect::new(0, 0, width.max(80), height.max(32))
}

#[cfg(unix)]
pub(super) fn performance_terminal_size_from_ioctl() -> Option<(u16, u16)> {
    let stdout = std::io::stdout();
    let mut winsize = libc::winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    let result = unsafe { libc::ioctl(stdout.as_raw_fd(), libc::TIOCGWINSZ, &mut winsize) };
    if result == 0 && winsize.ws_col > 0 && winsize.ws_row > 0 {
        Some((winsize.ws_col, winsize.ws_row))
    } else {
        None
    }
}

#[cfg(not(unix))]
pub(super) fn performance_terminal_size_from_ioctl() -> Option<(u16, u16)> {
    None
}
