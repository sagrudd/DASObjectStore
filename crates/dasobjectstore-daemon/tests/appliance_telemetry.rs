use dasobjectstore_daemon::{
    appliance_telemetry_state_path, collect_linux_cpu_telemetry,
    collect_linux_disk_capacity_telemetry, collect_linux_memory_telemetry,
    parse_linux_cpu_snapshot, validate_appliance_telemetry_cadence,
    ApplianceHostTelemetryCollector, ApplianceMemoryTelemetry, ApplianceTelemetryCollectorError,
    ApplianceTelemetryLoop, ApplianceTelemetryLoopConfig, ApplianceTelemetryLoopError,
    ApplianceTelemetryMissingReason, ApplianceTelemetrySink, ApplianceTelemetrySleeper,
    ApplianceTelemetrySource, FileBackedApplianceTelemetrySink, LinuxCpuSnapshot,
    LinuxHostTelemetrySample, LinuxProcTelemetryCollector, APPLIANCE_TELEMETRY_FILE_NAME,
};
use serde_json::json;
use std::fs;
use std::time::Duration;

const PROC_STAT_1: &str = "\
cpu  100 20 30 800 50 0 0 0 0 0
cpu0 50 10 15 400 25 0 0 0 0 0
cpu1 50 10 15 400 25 0 0 0 0 0
";

const PROC_STAT_2: &str = "\
cpu  180 20 70 900 50 0 0 0 0 0
cpu0 90 10 35 450 25 0 0 0 0 0
cpu1 90 10 35 450 25 0 0 0 0 0
";

const PROC_MEMINFO: &str = "\
MemTotal:       1000000 kB
MemAvailable:    750000 kB
Cached:          250000 kB
SwapTotal:       200000 kB
SwapFree:        150000 kB
";

#[test]
fn parses_linux_cpu_snapshot_from_proc_stat_fixture() {
    let snapshot = parse_linux_cpu_snapshot(PROC_STAT_1).expect("snapshot parses");

    assert_eq!(snapshot.total_jiffies, 1000);
    assert_eq!(snapshot.idle_jiffies, 850);
    assert_eq!(snapshot.logical_core_count, 2);
}

#[test]
fn cpu_telemetry_uses_two_proc_stat_fixtures_for_usage_percent() {
    let first = parse_linux_cpu_snapshot(PROC_STAT_1).expect("first snapshot");
    let second = parse_linux_cpu_snapshot(PROC_STAT_2).expect("second snapshot");
    let telemetry = collect_linux_cpu_telemetry(Some(&first), &second, "1.25 0.75 0.50 1/99 42");

    assert_eq!(telemetry.usage_percent, Some(54.55));
    assert_eq!(telemetry.load_average_1m, Some(1.25));
    assert_eq!(telemetry.load_average_5m, Some(0.75));
    assert_eq!(telemetry.load_average_15m, Some(0.50));
    assert_eq!(telemetry.logical_core_count, Some(2));
    assert_eq!(telemetry.missing_reason, None);
}

#[test]
fn first_cpu_sample_reports_missing_usage_until_next_snapshot() {
    let snapshot = parse_linux_cpu_snapshot(PROC_STAT_1).expect("snapshot parses");
    let telemetry = collect_linux_cpu_telemetry(None, &snapshot, "0.00 0.00 0.00 1/99 42");

    assert_eq!(telemetry.usage_percent, None);
    assert_eq!(
        telemetry.missing_reason,
        Some(ApplianceTelemetryMissingReason::DaemonStartup)
    );
}

#[test]
fn memory_telemetry_parses_proc_meminfo_fixture() {
    let telemetry = collect_linux_memory_telemetry(PROC_MEMINFO);

    assert_eq!(telemetry.total_bytes, Some(1_024_000_000));
    assert_eq!(telemetry.available_bytes, Some(768_000_000));
    assert_eq!(telemetry.used_percent, Some(25.0));
    assert_eq!(telemetry.swap_total_bytes, Some(204_800_000));
    assert_eq!(telemetry.swap_used_bytes, Some(51_200_000));
    assert_eq!(telemetry.missing_reason, None);
}

#[test]
fn memory_telemetry_reports_missing_available_memory() {
    let telemetry = collect_linux_memory_telemetry("MemTotal: 1000 kB\n");

    assert_eq!(telemetry.total_bytes, Some(1_024_000));
    assert_eq!(telemetry.available_bytes, None);
    assert_eq!(telemetry.used_percent, None);
    assert_eq!(
        telemetry.missing_reason,
        Some(ApplianceTelemetryMissingReason::CollectorUnavailable)
    );
}

#[test]
fn cpu_memory_telemetry_serialize_with_schema_field_names() {
    let memory = collect_linux_memory_telemetry(PROC_MEMINFO);
    let encoded = serde_json::to_value(memory).expect("memory serializes");

    assert_eq!(
        encoded,
        json!({
            "total_bytes": 1024000000u64,
            "available_bytes": 768000000u64,
            "used_percent": 25.0,
            "swap_total_bytes": 204800000u64,
            "swap_used_bytes": 51200000u64,
            "missing_reason": null
        })
    );
}

#[test]
fn disk_capacity_telemetry_reads_managed_hdd_root_markers() {
    let root = temp_root("appliance-telemetry-hdd-capacity");
    let hdd_root = root.join("hdd");
    let disk_root = hdd_root.join("disk-a");
    fs::create_dir_all(disk_root.join(".dasobjectstore")).expect("marker directory");
    fs::write(
        disk_root.join(".dasobjectstore/device.env"),
        "role=hdd:qnap-a\nlabel=QNAP bay 1\nenclosure_id=qnap-tl-d800c-01\ndevice=/dev/disk/by-id/fixture-a\nfilesystem=ext4\n",
    )
    .expect("device marker written");

    let (enclosures, disks) =
        collect_linux_disk_capacity_telemetry(&hdd_root).expect("capacity telemetry");
    fs::remove_dir_all(&root).expect("fixture root removed");

    assert_eq!(disks.len(), 1);
    assert_eq!(disks[0].disk_id, "qnap-a");
    assert_eq!(disks[0].label.as_deref(), Some("QNAP bay 1"));
    assert!(disks[0].mount_path.ends_with("/hdd/disk-a"));
    assert_eq!(disks[0].role, "hdd");
    assert_eq!(disks[0].enclosure_id.as_deref(), Some("qnap-tl-d800c-01"));
    assert_eq!(
        disks[0].device_path.as_deref(),
        Some("/dev/disk/by-id/fixture-a")
    );
    assert_eq!(disks[0].filesystem.as_deref(), Some("ext4"));
    assert!(disks[0].total_bytes.unwrap_or_default() > 0);
    assert!(disks[0].available_bytes.unwrap_or_default() > 0);
    assert!(disks[0].used_percent.is_some());
    assert_eq!(disks[0].missing_reason, None);

    assert_eq!(enclosures.len(), 1);
    assert_eq!(enclosures[0].enclosure_id, "qnap-tl-d800c-01");
    assert_eq!(enclosures[0].disk_ids, vec!["qnap-a".to_string()]);
    assert_eq!(enclosures[0].total_bytes, disks[0].total_bytes);
    assert_eq!(enclosures[0].available_bytes, disks[0].available_bytes);
}

#[test]
fn telemetry_cadence_accepts_initial_supported_values() {
    validate_appliance_telemetry_cadence(6).expect("fast cadence accepted");
    validate_appliance_telemetry_cadence(30).expect("normal cadence accepted");

    assert_eq!(
        validate_appliance_telemetry_cadence(5).expect_err("5s is not supported"),
        ApplianceTelemetryLoopError::InvalidCadenceSeconds(5)
    );
}

#[test]
fn telemetry_loop_runs_repeated_collection_without_sleeping_in_tests() {
    let config = ApplianceTelemetryLoopConfig::new(6, source()).expect("loop config");
    let mut loop_runner = ApplianceTelemetryLoop::new(config, FakeCollector::new());
    let mut sink = MemorySink::default();
    let mut sleeper = RecordingSleeper::default();

    let written = loop_runner
        .run_iterations(
            &mut sink,
            &mut sleeper,
            [
                "2026-07-09T17:28:00Z".to_string(),
                "2026-07-09T17:28:06Z".to_string(),
            ],
        )
        .expect("loop iterations run");

    assert_eq!(written, 2);
    assert_eq!(loop_runner.samples_collected(), 2);
    assert_eq!(
        sleeper.sleeps,
        vec![Duration::from_secs(6), Duration::from_secs(6)]
    );
    assert_eq!(sink.records.len(), 2);
    assert_eq!(
        sink.records[0]["samples"][0]["cpu"]["missing_reason"],
        "daemon_startup"
    );
    assert_eq!(sink.records[1]["samples"][0]["cpu"]["usage_percent"], 50.0);
}

#[test]
fn file_backed_sink_writes_current_schema_shaped_sample_set() {
    let root = temp_root("appliance-telemetry-state");
    let state_path = appliance_telemetry_state_path(&root);
    let config = ApplianceTelemetryLoopConfig::new(30, source()).expect("loop config");
    let mut loop_runner = ApplianceTelemetryLoop::new(config, FakeCollector::new());
    let mut sink = FileBackedApplianceTelemetrySink::new(&state_path);

    let sample_set = loop_runner
        .collect_once("2026-07-09T17:29:00Z")
        .expect("sample collected");
    sink.record(&sample_set).expect("state written");
    let written: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&state_path).expect("state reads"))
            .expect("state is json");
    fs::remove_dir_all(&root).expect("fixture root removed");

    assert!(state_path.ends_with(APPLIANCE_TELEMETRY_FILE_NAME));
    assert_eq!(
        written["schema_version"],
        "dasobjectstore.appliance_telemetry.v1"
    );
    assert_eq!(written["cadence_seconds"], 30.0);
    assert_eq!(
        written["samples"][0]["sessions"]["missing_reason"],
        "not_configured"
    );
}

#[test]
fn proc_collector_reads_fixture_directory_without_live_host_state() {
    let root = temp_root("appliance-telemetry-proc-fixture");
    fs::write(root.join("stat"), PROC_STAT_2).expect("stat fixture written");
    fs::write(root.join("loadavg"), "1.00 0.50 0.25 1/100 123\n").expect("loadavg fixture written");
    fs::write(root.join("meminfo"), PROC_MEMINFO).expect("meminfo fixture written");

    let previous = parse_linux_cpu_snapshot(PROC_STAT_1).expect("previous snapshot");
    let sample = LinuxProcTelemetryCollector::new(&root)
        .collect(Some(&previous))
        .expect("fixture telemetry collected");
    fs::remove_dir_all(&root).expect("fixture root removed");

    assert_eq!(sample.cpu.usage_percent, Some(54.55));
    assert_eq!(sample.memory.used_percent, Some(25.0));
    assert_eq!(sample.cpu_snapshot.logical_core_count, 2);
}

#[test]
fn invalid_proc_stat_reports_parse_error() {
    let error = parse_linux_cpu_snapshot("intr 1 2 3").expect_err("stat is invalid");

    assert_eq!(
        error,
        ApplianceTelemetryCollectorError::InvalidProcStat("missing aggregate cpu line".to_string())
    );
}

fn temp_root(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("dasobjectstore-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("temp root created");
    root
}

fn source() -> ApplianceTelemetrySource {
    ApplianceTelemetrySource {
        appliance_id: "fixture-appliance".to_string(),
        host_id: "fixture-host".to_string(),
        hostname: Some("fixture-hostname".to_string()),
    }
}

#[derive(Default)]
struct MemorySink {
    records: Vec<serde_json::Value>,
}

impl ApplianceTelemetrySink for MemorySink {
    fn record(
        &mut self,
        sample_set: &dasobjectstore_daemon::ApplianceTelemetrySampleSet,
    ) -> Result<(), ApplianceTelemetryLoopError> {
        self.records
            .push(serde_json::to_value(sample_set).expect("sample serializes"));
        Ok(())
    }
}

#[derive(Default)]
struct RecordingSleeper {
    sleeps: Vec<Duration>,
}

impl ApplianceTelemetrySleeper for RecordingSleeper {
    fn sleep(&mut self, duration: Duration) {
        self.sleeps.push(duration);
    }
}

struct FakeCollector {
    snapshots: Vec<LinuxCpuSnapshot>,
}

impl FakeCollector {
    fn new() -> Self {
        Self {
            snapshots: vec![
                LinuxCpuSnapshot {
                    total_jiffies: 100,
                    idle_jiffies: 90,
                    logical_core_count: 2,
                },
                LinuxCpuSnapshot {
                    total_jiffies: 200,
                    idle_jiffies: 140,
                    logical_core_count: 2,
                },
            ],
        }
    }
}

impl ApplianceHostTelemetryCollector for FakeCollector {
    fn collect(
        &mut self,
        previous_cpu: Option<&LinuxCpuSnapshot>,
    ) -> Result<LinuxHostTelemetrySample, ApplianceTelemetryCollectorError> {
        let snapshot = self.snapshots.remove(0);
        Ok(LinuxHostTelemetrySample {
            cpu: collect_linux_cpu_telemetry(previous_cpu, &snapshot, "0.10 0.20 0.30 1/10 42"),
            memory: ApplianceMemoryTelemetry {
                total_bytes: Some(100),
                available_bytes: Some(75),
                used_percent: Some(25.0),
                swap_total_bytes: Some(0),
                swap_used_bytes: Some(0),
                missing_reason: None,
            },
            enclosures: Vec::new(),
            disks: Vec::new(),
            cpu_snapshot: snapshot,
        })
    }
}
