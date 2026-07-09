use dasobjectstore_daemon::{
    collect_linux_cpu_telemetry, collect_linux_memory_telemetry, parse_linux_cpu_snapshot,
    ApplianceTelemetryCollectorError, LinuxProcTelemetryCollector,
};
use serde_json::json;
use std::fs;

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
        telemetry.missing_reason.as_deref(),
        Some("initial_cpu_snapshot")
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
        telemetry.missing_reason.as_deref(),
        Some("memavailable_missing")
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
