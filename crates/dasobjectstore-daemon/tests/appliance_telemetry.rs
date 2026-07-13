use dasobjectstore_daemon::{
    appliance_sample_set, appliance_telemetry_state_path, collect_appliance_session_telemetry,
    collect_linux_cpu_telemetry, collect_linux_disk_capacity_telemetry,
    collect_linux_disk_io_telemetry, collect_linux_memory_telemetry, parse_linux_cpu_snapshot,
    parse_linux_diskstats, validate_appliance_telemetry_cadence, ApplianceDiskCapacityTelemetry,
    ApplianceEnclosureTelemetry, ApplianceHostTelemetryCollector, ApplianceMemoryTelemetry,
    ApplianceTelemetryCollectorError, ApplianceTelemetryLoop, ApplianceTelemetryLoopConfig,
    ApplianceTelemetryLoopError, ApplianceTelemetryMissingReason, ApplianceTelemetrySink,
    ApplianceTelemetrySleeper, ApplianceTelemetrySource, FileBackedApplianceTelemetrySink,
    LinuxCpuSnapshot, LinuxHostTelemetrySample, LinuxProcTelemetryCollector,
    APPLIANCE_TELEMETRY_FILE_NAME,
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

const PROC_DISKSTATS_1: &str = "\
   8       0 sda 100 0 2000 300 40 0 1000 200 0 500 600
   8      16 sdb 10 0 20 3 4 0 8 2 0 6 7
";

const PROC_DISKSTATS_2: &str = "\
   8       0 sda 130 0 2600 360 70 0 1600 290 0 620 760
   8      16 sdb 11 0 22 4 5 0 10 3 0 7 8
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
fn first_disk_io_sample_warmup_reason_uses_public_schema_name() {
    let encoded = serde_json::to_value(ApplianceTelemetryMissingReason::FirstSampleWarmup)
        .expect("warmup reason serializes");
    assert_eq!(encoded, "first_sample_warmup");
}

#[test]
fn disk_capacity_telemetry_reads_managed_hdd_root_markers() {
    let root = temp_root("appliance-telemetry-hdd-capacity");
    let hdd_root = root.join("hdd");
    let disk_root = hdd_root.join("disk-a");
    fs::create_dir_all(disk_root.join(".dasobjectstore")).expect("marker directory");
    fs::write(
        disk_root.join(".dasobjectstore/device.env"),
        "role=hdd:qnap-a\nlabel=QNAP bay 1\nenclosure_id=qnap-tl-d800c-01\nbay_label=1\ndevice=/dev/disk/by-id/fixture-a\nfilesystem=ext4\n",
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
    assert_eq!(disks[0].bay_label.as_deref(), Some("1"));
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
fn parses_linux_diskstats_fixture() {
    let counters = parse_linux_diskstats(PROC_DISKSTATS_1).expect("diskstats parse");
    let sda = counters.get("sda").expect("sda counters");

    assert_eq!(sda.device_name, "sda");
    assert_eq!(sda.read_operations, 100);
    assert_eq!(sda.write_operations, 40);
    assert_eq!(sda.sectors_read, 2000);
    assert_eq!(sda.sectors_written, 1000);
    assert_eq!(sda.io_time_millis, 500);
}

#[test]
fn disk_io_telemetry_uses_managed_hdd_markers_and_diskstats_deltas() {
    let root = temp_root("appliance-telemetry-hdd-io");
    let hdd_root = root.join("hdd");
    let disk_root = hdd_root.join("disk-a");
    fs::create_dir_all(disk_root.join(".dasobjectstore")).expect("marker directory");
    fs::write(
        disk_root.join(".dasobjectstore/device.env"),
        "role=hdd:qnap-a\nlabel=QNAP bay 1\nenclosure_id=qnap-tl-d800c-01\nbay_label=1\ndevice=/dev/disk/by-id/fixture-a\ndiskstats_device=sda\nfilesystem=ext4\n",
    )
    .expect("device marker written");
    let previous = parse_linux_diskstats(PROC_DISKSTATS_1).expect("previous diskstats");
    let current = parse_linux_diskstats(PROC_DISKSTATS_2).expect("current diskstats");

    let telemetry = collect_linux_disk_io_telemetry(&hdd_root, &current, Some(&previous), 6)
        .expect("disk io telemetry");
    fs::remove_dir_all(&root).expect("fixture root removed");

    assert_eq!(telemetry.len(), 1);
    assert_eq!(telemetry[0].disk_id, "qnap-a");
    assert_eq!(telemetry[0].label.as_deref(), Some("QNAP bay 1"));
    assert_eq!(telemetry[0].device_name.as_deref(), Some("sda"));
    assert_eq!(telemetry[0].bay_label.as_deref(), Some("1"));
    assert_eq!(telemetry[0].read_bytes_per_second, Some(51_200.0));
    assert_eq!(telemetry[0].write_bytes_per_second, Some(51_200.0));
    assert_eq!(telemetry[0].read_operations_per_second, Some(5.0));
    assert_eq!(telemetry[0].write_operations_per_second, Some(5.0));
    assert_eq!(telemetry[0].average_await_millis, Some(2.5));
    assert_eq!(telemetry[0].io_time_percent, Some(2.0));
    assert_eq!(telemetry[0].missing_reason, None);
}

#[test]
fn disk_io_marker_rejects_path_bearing_diskstats_device() {
    let root = temp_root("appliance-telemetry-invalid-device-marker");
    let hdd_root = root.join("hdd");
    let disk_root = hdd_root.join("disk-a");
    fs::create_dir_all(disk_root.join(".dasobjectstore")).expect("marker directory");
    fs::write(
        disk_root.join(".dasobjectstore/device.env"),
        "role=hdd:qnap-a\ndevice=/dev/disk/by-id/fixture-a\ndiskstats_device=/dev/sda\n",
    )
    .expect("device marker written");
    let current = parse_linux_diskstats(PROC_DISKSTATS_1).expect("diskstats parse");

    let error = collect_linux_disk_io_telemetry(&hdd_root, &current, None, 6)
        .expect_err("path-bearing marker rejected");
    fs::remove_dir_all(&root).expect("fixture root removed");
    assert!(error
        .to_string()
        .contains("basename without path separators"));
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
    let sample_set = loop_runner
        .collect_once("2026-07-09T17:29:30Z")
        .expect("second sample collected");
    sink.record(&sample_set).expect("second state written");
    let written: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&state_path).expect("state reads"))
            .expect("state is json");

    assert!(state_path.ends_with(APPLIANCE_TELEMETRY_FILE_NAME));
    assert_eq!(
        written["schema_version"],
        "dasobjectstore.appliance_telemetry.v1"
    );
    assert_eq!(written["cadence_seconds"], 30.0);
    assert_eq!(written["generated_at_utc"], "2026-07-09T17:29:30Z");
    assert_eq!(
        written["samples"].as_array().expect("samples array").len(),
        2
    );
    assert_eq!(
        written["samples"][0]["sessions"]["missing_reason"],
        "not_configured"
    );
    let missing_paths = written["samples"][0]["missing_data"]
        .as_array()
        .expect("missing data array")
        .iter()
        .map(|marker| marker["path"].as_str().expect("missing data path"))
        .collect::<Vec<_>>();
    assert!(missing_paths.contains(&"cpu.usage_percent"));
    assert!(missing_paths.contains(&"disks.capacity"));
    assert!(missing_paths.contains(&"disks.io"));
    assert!(missing_paths.contains(&"sessions"));

    assert_no_telemetry_temp_files(&state_path);
    fs::remove_dir_all(&root).expect("fixture root removed");
}

#[test]
fn file_backed_sink_bounds_telemetry_retention_by_chart_windows() {
    let root = temp_root("appliance-telemetry-retention");
    let state_path = appliance_telemetry_state_path(&root);
    let mut sink = FileBackedApplianceTelemetrySink::new(&state_path);

    for timestamp in [
        "2026-03-01T00:00:00Z",
        "2026-06-20T12:00:00Z",
        "2026-06-20T12:30:00Z",
        "2026-07-08T17:00:00Z",
        "2026-07-08T17:05:00Z",
        "2026-07-09T16:30:00Z",
        "2026-07-09T16:30:30Z",
        "2026-07-09T17:59:30Z",
        "2026-07-09T18:00:00Z",
    ] {
        let sample_set = sample_set_at(timestamp);
        sink.record(&sample_set).expect("state written");
    }

    let written: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&state_path).expect("state reads"))
            .expect("state is json");
    fs::remove_dir_all(&root).expect("fixture root removed");

    let timestamps = written["samples"]
        .as_array()
        .expect("samples array")
        .iter()
        .map(|sample| {
            sample["timestamp_utc"]
                .as_str()
                .expect("sample timestamp")
                .to_string()
        })
        .collect::<Vec<_>>();

    assert_eq!(
        timestamps,
        vec![
            "2026-06-20T12:30:00Z",
            "2026-07-08T17:05:00Z",
            "2026-07-09T16:30:30Z",
            "2026-07-09T17:59:30Z",
            "2026-07-09T18:00:00Z"
        ]
    );
    assert_eq!(written["generated_at_utc"], "2026-07-09T18:00:00Z");
}

#[test]
fn file_backed_sink_preserves_corrupt_json_and_starts_fresh_history() {
    let root = temp_root("appliance-telemetry-corrupt-recovery");
    let state_path = appliance_telemetry_state_path(&root);
    fs::create_dir_all(state_path.parent().expect("telemetry parent")).expect("telemetry dir");
    fs::write(&state_path, "{ this is not json").expect("corrupt state written");
    let mut sink = FileBackedApplianceTelemetrySink::new(&state_path);

    let sample_set = sample_set_at("2026-07-09T18:01:00Z");
    sink.record(&sample_set).expect("state recovered");

    let written: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&state_path).expect("state reads"))
            .expect("state is json");
    let corrupt_files = telemetry_dir_entries(&state_path)
        .into_iter()
        .filter(|name| name.starts_with("corrupt-appliance-telemetry.v1-"))
        .collect::<Vec<_>>();
    assert_eq!(corrupt_files.len(), 1);
    let corrupt_path = state_path
        .parent()
        .expect("telemetry parent")
        .join(&corrupt_files[0]);
    assert_eq!(
        fs::read_to_string(&corrupt_path).expect("corrupt state reads"),
        "{ this is not json"
    );
    fs::remove_dir_all(&root).expect("fixture root removed");

    assert_eq!(written["generated_at_utc"], "2026-07-09T18:01:00Z");
    assert_eq!(
        written["samples"].as_array().expect("samples array").len(),
        1
    );
}

#[test]
fn file_backed_sink_preserves_enclosure_and_disk_identity_across_samples() {
    let root = temp_root("appliance-telemetry-identity-retained");
    let state_path = appliance_telemetry_state_path(&root);
    let mut sink = FileBackedApplianceTelemetrySink::new(&state_path);

    for timestamp in ["2026-07-09T18:02:00Z", "2026-07-09T18:02:30Z"] {
        let sample_set = sample_set_with_disk_identity(timestamp);
        sink.record(&sample_set).expect("state written");
    }

    let written: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&state_path).expect("state reads"))
            .expect("state is json");
    assert_no_telemetry_temp_files(&state_path);
    fs::remove_dir_all(&root).expect("fixture root removed");

    let samples = written["samples"].as_array().expect("samples array");
    assert_eq!(samples.len(), 2);
    for sample in samples {
        assert_eq!(sample["enclosures"][0]["enclosure_id"], "qnap-tl-d800c-01");
        assert_eq!(sample["enclosures"][0]["disk_ids"][0], "qnap-a");
        assert_eq!(sample["disks"][0]["disk_id"], "qnap-a");
        assert_eq!(sample["disks"][0]["label"], "QNAP bay 1");
        assert_eq!(sample["disks"][0]["enclosure_id"], "qnap-tl-d800c-01");
        assert_eq!(sample["disks"][0]["bay_label"], "1");
        assert_eq!(
            sample["disks"][0]["device_path"],
            "/dev/disk/by-id/fixture-a"
        );
    }
}

#[test]
fn proc_collector_reads_fixture_directory_without_live_host_state() {
    let root = temp_root("appliance-telemetry-proc-fixture");
    fs::write(root.join("stat"), PROC_STAT_2).expect("stat fixture written");
    fs::write(root.join("loadavg"), "1.00 0.50 0.25 1/100 123\n").expect("loadavg fixture written");
    fs::write(root.join("meminfo"), PROC_MEMINFO).expect("meminfo fixture written");

    let previous = parse_linux_cpu_snapshot(PROC_STAT_1).expect("previous snapshot");
    let mut collector = LinuxProcTelemetryCollector::new(&root);
    let sample = collector
        .collect(Some(&previous), 30, "2026-07-09T18:00:00Z")
        .expect("fixture telemetry collected");
    fs::remove_dir_all(&root).expect("fixture root removed");

    assert_eq!(sample.cpu.usage_percent, Some(54.55));
    assert_eq!(sample.memory.used_percent, Some(25.0));
    assert_eq!(sample.cpu_snapshot.logical_core_count, 2);
    assert!(sample.disk_io.is_empty());
}

#[test]
fn proc_collector_retains_diskstats_for_cadence_aware_disk_io_rates() {
    let root = temp_root("appliance-telemetry-proc-diskstats-retained");
    let proc_root = root.join("proc");
    let hdd_root = root.join("hdd");
    let disk_root = hdd_root.join("disk-a");
    fs::create_dir_all(&proc_root).expect("proc fixture directory");
    fs::create_dir_all(disk_root.join(".dasobjectstore")).expect("marker directory");
    fs::write(proc_root.join("stat"), PROC_STAT_1).expect("first stat fixture written");
    fs::write(proc_root.join("loadavg"), "1.00 0.50 0.25 1/100 123\n")
        .expect("loadavg fixture written");
    fs::write(proc_root.join("meminfo"), PROC_MEMINFO).expect("meminfo fixture written");
    fs::write(proc_root.join("diskstats"), PROC_DISKSTATS_1)
        .expect("first diskstats fixture written");
    fs::write(
        disk_root.join(".dasobjectstore/device.env"),
        "role=hdd:qnap-a\nlabel=QNAP bay 1\nenclosure_id=qnap-tl-d800c-01\nbay_label=1\ndevice=/dev/disk/by-id/fixture-a\ndiskstats_device=sda\nfilesystem=ext4\n",
    )
    .expect("device marker written");

    let mut collector = LinuxProcTelemetryCollector::new(&proc_root).with_hdd_root(&hdd_root);
    let first = collector
        .collect(None, 6, "2026-07-09T18:00:00Z")
        .expect("first sample collected");
    fs::write(proc_root.join("stat"), PROC_STAT_2).expect("second stat fixture written");
    fs::write(proc_root.join("diskstats"), PROC_DISKSTATS_2)
        .expect("second diskstats fixture written");
    let second = collector
        .collect(Some(&first.cpu_snapshot), 6, "2026-07-09T18:00:06Z")
        .expect("second sample collected");
    fs::remove_dir_all(&root).expect("fixture root removed");

    assert_eq!(first.disk_io.len(), 1);
    assert_eq!(first.disk_io[0].disk_id, "qnap-a");
    assert_eq!(
        first.disk_io[0].missing_reason,
        Some(ApplianceTelemetryMissingReason::FirstSampleWarmup)
    );

    assert_eq!(second.disk_io.len(), 1);
    assert_eq!(second.disk_io[0].disk_id, "qnap-a");
    assert_eq!(second.disk_io[0].read_bytes_per_second, Some(51_200.0));
    assert_eq!(second.disk_io[0].write_bytes_per_second, Some(51_200.0));
    assert_eq!(second.disk_io[0].read_operations_per_second, Some(5.0));
    assert_eq!(second.disk_io[0].write_operations_per_second, Some(5.0));
    assert_eq!(second.disk_io[0].average_await_millis, Some(2.5));
    assert_eq!(second.disk_io[0].io_time_percent, Some(2.0));
    assert_eq!(second.disk_io[0].missing_reason, None);
}

#[test]
fn proc_collector_resolves_stable_device_alias_through_sysfs_fixture() {
    let root = temp_root("appliance-telemetry-device-alias");
    let proc_root = root.join("proc");
    let hdd_root = root.join("hdd");
    let sys_root = root.join("sys");
    let disk_root = hdd_root.join("disk-a");
    fs::create_dir_all(&proc_root).expect("proc fixture directory");
    fs::create_dir_all(disk_root.join(".dasobjectstore")).expect("marker directory");
    fs::create_dir_all(sys_root.join("class/block/sda")).expect("sysfs block fixture");
    fs::create_dir_all(sys_root.join("dev/disk/by-id")).expect("device alias fixture");
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        "../../../class/block/sda",
        sys_root.join("dev/disk/by-id/fixture-a"),
    )
    .expect("stable device alias symlink");
    fs::write(proc_root.join("stat"), PROC_STAT_1).expect("stat fixture written");
    fs::write(proc_root.join("loadavg"), "1.00 0.50 0.25 1/100 123\n")
        .expect("loadavg fixture written");
    fs::write(proc_root.join("meminfo"), PROC_MEMINFO).expect("meminfo fixture written");
    fs::write(proc_root.join("diskstats"), PROC_DISKSTATS_1).expect("diskstats fixture written");
    fs::write(
        disk_root.join(".dasobjectstore/device.env"),
        "role=hdd:qnap-a\nlabel=QNAP bay 1\ndevice=/dev/disk/by-id/fixture-a\nfilesystem=ext4\n",
    )
    .expect("device marker written");

    let mut collector = LinuxProcTelemetryCollector::new(&proc_root)
        .with_hdd_root(&hdd_root)
        .with_sys_root(&sys_root);
    let sample = collector
        .collect(None, 6, "2026-07-09T18:00:00Z")
        .expect("fixture telemetry collected");
    fs::remove_dir_all(&root).expect("fixture root removed");

    assert_eq!(sample.disk_io.len(), 1);
    assert_eq!(sample.disk_io[0].device_name.as_deref(), Some("sda"));
    assert_eq!(
        sample.disk_io[0].missing_reason,
        Some(ApplianceTelemetryMissingReason::FirstSampleWarmup)
    );
}

#[test]
fn proc_collector_covers_partition_usb_device_mapper_and_missing_fixtures() {
    let root = temp_root("appliance-telemetry-topology-fixtures");
    let proc_root = root.join("proc");
    let hdd_root = root.join("hdd");
    let sys_root = root.join("sys");
    fs::create_dir_all(&proc_root).expect("proc fixture directory");
    fs::create_dir_all(sys_root.join("class/block/sda")).expect("sata sysfs fixture");
    fs::create_dir_all(sys_root.join("class/block/sdb")).expect("usb sysfs fixture");
    fs::create_dir_all(sys_root.join("class/block/dm-0")).expect("dm sysfs fixture");
    fs::create_dir_all(sys_root.join("dev/disk/by-id")).expect("by-id fixture");
    fs::create_dir_all(sys_root.join("dev/disk/by-path")).expect("by-path fixture");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            "../../../class/block/sdb",
            sys_root.join("dev/disk/by-id/fixture-usb"),
        )
        .expect("usb alias symlink");
        std::os::unix::fs::symlink(
            "../../../class/block/dm-0",
            sys_root.join("dev/disk/by-path/fixture-dm"),
        )
        .expect("dm alias symlink");
    }

    let first_diskstats = "\
   8       0 sda 100 0 2000 300 40 0 1000 200 0 500 600
   8       1 sdb1 10 0 20 3 4 0 8 2 0 6 7
   8      16 sdb 20 0 40 6 8 0 16 4 0 12 14
 253       0 dm-0 30 0 60 9 12 0 24 6 0 18 21
";
    let second_diskstats = "\
   8       0 sda 130 0 2600 360 70 0 1600 290 0 620 760
   8       1 sdb1 20 0 40 6 8 0 16 4 0 12 14
   8      16 sdb 50 0 100 12 16 0 32 8 0 24 28
 253       0 dm-0 45 0 90 14 18 0 36 9 0 27 32
";
    fs::write(proc_root.join("stat"), PROC_STAT_1).expect("stat fixture written");
    fs::write(proc_root.join("loadavg"), "1.00 0.50 0.25 1/100 123\n")
        .expect("loadavg fixture written");
    fs::write(proc_root.join("meminfo"), PROC_MEMINFO).expect("meminfo fixture written");
    fs::write(proc_root.join("diskstats"), first_diskstats).expect("first diskstats written");

    for (directory, marker) in [
        (
            "disk-sata",
            "role=hdd:sata\ndevice=/dev/sda\nenclosure_id=fixture\nbay_label=1\n",
        ),
        (
            "disk-partition",
            "role=hdd:partition\ndiskstats_device=sdb1\ndevice=/dev/sdb1\nenclosure_id=fixture\nbay_label=2\n",
        ),
        (
            "disk-usb",
            "role=hdd:usb\ndevice=/dev/disk/by-id/fixture-usb\nenclosure_id=fixture\nbay_label=3\n",
        ),
        (
            "disk-dm",
            "role=hdd:dm\ndevice=/dev/disk/by-path/fixture-dm\nenclosure_id=fixture\nbay_label=4\n",
        ),
        (
            "disk-missing",
            "role=hdd:missing\ndevice=/dev/not-present\nenclosure_id=fixture\nbay_label=5\n",
        ),
    ] {
        let disk_root = hdd_root.join(directory);
        fs::create_dir_all(disk_root.join(".dasobjectstore")).expect("marker directory");
        fs::write(disk_root.join(".dasobjectstore/device.env"), marker)
            .expect("device marker written");
    }

    let mut collector = LinuxProcTelemetryCollector::new(&proc_root)
        .with_hdd_root(&hdd_root)
        .with_sys_root(&sys_root);
    let first = collector
        .collect(None, 6, "2026-07-09T18:00:00Z")
        .expect("first topology sample collected");
    fs::write(proc_root.join("diskstats"), second_diskstats).expect("second diskstats written");
    let second = collector
        .collect(Some(&first.cpu_snapshot), 6, "2026-07-09T18:00:06Z")
        .expect("second topology sample collected");
    fs::remove_dir_all(&root).expect("fixture root removed");

    assert_eq!(first.disk_io.len(), 5);
    for disk in &first.disk_io {
        assert_eq!(
            disk.missing_reason,
            Some(if disk.disk_id == "missing" {
                ApplianceTelemetryMissingReason::DeviceMissing
            } else {
                ApplianceTelemetryMissingReason::FirstSampleWarmup
            })
        );
    }

    let by_id = second
        .disk_io
        .iter()
        .map(|disk| (disk.disk_id.as_str(), disk))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(by_id["sata"].device_name.as_deref(), Some("sda"));
    assert_eq!(by_id["partition"].device_name.as_deref(), Some("sdb1"));
    assert_eq!(by_id["usb"].device_name.as_deref(), Some("sdb"));
    assert_eq!(by_id["dm"].device_name.as_deref(), Some("dm-0"));
    for disk_id in ["sata", "partition", "usb", "dm"] {
        assert!(by_id[disk_id].read_bytes_per_second.unwrap_or_default() > 0.0);
        assert_eq!(by_id[disk_id].missing_reason, None);
    }
    assert_eq!(by_id["missing"].device_name, None);
    assert_eq!(
        by_id["missing"].missing_reason,
        Some(ApplianceTelemetryMissingReason::DeviceMissing)
    );
}

#[test]
fn session_telemetry_counts_web_and_remote_agent_sessions() {
    let root = temp_root("appliance-telemetry-sessions");
    let auth_root = root.join("auth");
    let remote_session_path = root.join("remote-easyconnect/sessions.json");
    let group_path = root.join("group");
    fs::create_dir_all(&auth_root).expect("auth fixture directory");
    fs::create_dir_all(remote_session_path.parent().expect("remote session parent"))
        .expect("remote session fixture directory");
    fs::write(
        auth_root.join("registry.json"),
        r#"{
          "users": [
            {
              "username": "admin",
              "sessions": [
                {"expires_at_utc": "2026-07-09T19:00:00Z", "revoked_at_utc": null}
              ]
            },
            {
              "username": "stephen",
              "sessions": [
                {"expires_at_utc": "2026-07-09T20:00:00Z", "revoked_at_utc": null},
                {"expires_at_utc": "2026-07-09T20:00:00Z", "revoked_at_utc": "2026-07-09T17:00:00Z"},
                {"expires_at_utc": "2026-07-09T17:00:00Z", "revoked_at_utc": null}
              ]
            }
          ]
        }"#,
    )
    .expect("web auth registry written");
    fs::write(
        &remote_session_path,
        r#"{
          "schema_version": "dasobjectstore.remote_easyconnect.sessions.v1",
          "sessions": [
            {
              "session_id": "remote-1",
              "approved_actor": "stephen",
              "expires_at_utc": "2026-07-09T19:30:00Z",
              "revoked_at_utc": null
            },
            {
              "session_id": "remote-2",
              "approved_actor": "operator",
              "expires_at_utc": "2026-07-09T17:30:00Z",
              "revoked_at_utc": null
            },
            {
              "session_id": "remote-3",
              "approved_actor": "admin",
              "expires_at_utc": "2026-07-09T19:30:00Z",
              "revoked_at_utc": "2026-07-09T17:45:00Z"
            }
          ]
        }"#,
    )
    .expect("remote session registry written");
    fs::write(
        &group_path,
        "sudo:x:27:admin\nusers:x:100:stephen,operator\n",
    )
    .expect("group fixture written");

    let telemetry = collect_appliance_session_telemetry(
        Some(&auth_root),
        Some(&remote_session_path),
        Some(&group_path),
        "2026-07-09T18:00:00Z",
        0,
    );
    fs::remove_dir_all(&root).expect("fixture root removed");

    assert_eq!(telemetry.web_active_sessions, Some(2));
    assert_eq!(telemetry.remote_agent_active_sessions, Some(1));
    assert_eq!(telemetry.distinct_logged_in_users, Some(2));
    assert_eq!(telemetry.administrator_sessions, Some(1));
    assert_eq!(telemetry.operator_sessions, Some(2));
    assert_eq!(telemetry.missing_reason, None);
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

fn sample_set_at(timestamp: &str) -> dasobjectstore_daemon::ApplianceTelemetrySampleSet {
    let config = ApplianceTelemetryLoopConfig::new(30, source()).expect("loop config");
    ApplianceTelemetryLoop::new(config, FakeCollector::new())
        .collect_once(timestamp)
        .expect("sample collected")
}

fn sample_set_with_disk_identity(
    timestamp: &str,
) -> dasobjectstore_daemon::ApplianceTelemetrySampleSet {
    let snapshot = LinuxCpuSnapshot {
        total_jiffies: 100,
        idle_jiffies: 90,
        logical_core_count: 2,
    };
    appliance_sample_set(
        30,
        source(),
        timestamp.to_string(),
        LinuxHostTelemetrySample {
            cpu: collect_linux_cpu_telemetry(None, &snapshot, "0.10 0.20 0.30 1/10 42"),
            memory: ApplianceMemoryTelemetry {
                total_bytes: Some(100),
                available_bytes: Some(75),
                used_percent: Some(25.0),
                swap_total_bytes: Some(0),
                swap_used_bytes: Some(0),
                missing_reason: None,
            },
            enclosures: vec![ApplianceEnclosureTelemetry {
                enclosure_id: "qnap-tl-d800c-01".to_string(),
                label: Some("QNAP TL-D800C".to_string()),
                disk_ids: vec!["qnap-a".to_string()],
                total_bytes: Some(1_000),
                available_bytes: Some(700),
                used_percent: Some(30.0),
                missing_reason: None,
            }],
            disks: vec![ApplianceDiskCapacityTelemetry {
                disk_id: "qnap-a".to_string(),
                label: Some("QNAP bay 1".to_string()),
                mount_path: "/srv/dasobjectstore/hdd/qnap-a".to_string(),
                role: "hdd".to_string(),
                enclosure_id: Some("qnap-tl-d800c-01".to_string()),
                bay_label: Some("1".to_string()),
                device_path: Some("/dev/disk/by-id/fixture-a".to_string()),
                filesystem: Some("ext4".to_string()),
                total_bytes: Some(1_000),
                available_bytes: Some(700),
                used_percent: Some(30.0),
                missing_reason: None,
            }],
            disk_io: Vec::new(),
            sessions: dasobjectstore_daemon::ApplianceSessionTelemetry {
                web_active_sessions: Some(1),
                remote_agent_active_sessions: Some(0),
                distinct_logged_in_users: Some(1),
                administrator_sessions: Some(1),
                operator_sessions: Some(0),
                missing_reason: None,
            },
            cpu_snapshot: snapshot,
        },
    )
}

fn assert_no_telemetry_temp_files(state_path: &std::path::Path) {
    let temp_prefix = format!(".{APPLIANCE_TELEMETRY_FILE_NAME}.tmp-");
    let temp_files = telemetry_dir_entries(state_path)
        .into_iter()
        .filter(|name| name.starts_with(&temp_prefix))
        .collect::<Vec<_>>();
    assert!(
        temp_files.is_empty(),
        "unexpected telemetry temp files: {temp_files:?}"
    );
}

fn telemetry_dir_entries(state_path: &std::path::Path) -> Vec<String> {
    fs::read_dir(state_path.parent().expect("telemetry parent"))
        .expect("telemetry directory reads")
        .map(|entry| {
            entry
                .expect("telemetry entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>()
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
        _elapsed_seconds: u64,
        _timestamp_utc: &str,
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
            disk_io: Vec::new(),
            sessions: dasobjectstore_daemon::ApplianceSessionTelemetry {
                web_active_sessions: None,
                remote_agent_active_sessions: None,
                distinct_logged_in_users: None,
                administrator_sessions: None,
                operator_sessions: None,
                missing_reason: Some(ApplianceTelemetryMissingReason::NotConfigured),
            },
            cpu_snapshot: snapshot,
        })
    }
}
