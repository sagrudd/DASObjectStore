#![allow(dead_code)] // Transitional: telemetry helpers are being moved to `telemetry` below.

use crate::dashboard::{
    ActiveUsersSummaryView, CapacitySummaryView, CpuUsageSummaryView,
    CreateObjectStoreAffordanceView, DasEnclosureCardView, DashboardHealthStateView,
    DashboardWarning, DiskIoSummaryView, DriveCountSummaryView, EnclosureConnectionView,
    HealthSummaryView, HomeDashboardView, MemoryStressStateView, MemoryStressView,
    ObjectServiceStatusView, SmartWarningView, SmartWarningsSummaryView, TelemetryCardStateView,
    TelemetryWindowControlView, TelemetryWindowOptionView, ThroughputDayView,
    ThroughputSummaryView, REDESIGN_DASHBOARD_SCHEMA_VERSION,
};
use crate::object_stores_aggregator::registry_object_store_cards;
use dasobjectstore_core::utc::parse_utc_timestamp_seconds;
use dasobjectstore_daemon::api::ApplianceTelemetryWindow;
use dasobjectstore_daemon::{
    appliance_telemetry_state_path, ApplianceTelemetrySample, ApplianceTelemetrySampleSet,
    DEFAULT_DAEMON_STATE_DIR,
};
use dasobjectstore_object_service::default_store_registry_path;
use serde::Deserialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

mod object_service;
mod telemetry;

pub(crate) const DEFAULT_SSD_ROOT: &str = "/srv/dasobjectstore/ssd";
pub(crate) const DEFAULT_HDD_ROOT: &str = "/srv/dasobjectstore/hdd";
const DEFAULT_THROUGHPUT_PATH: &str = "/var/lib/dasobjectstore/telemetry/throughput-7d.json";
const DEFAULT_SMART_WARNINGS_PATH: &str = "/var/lib/dasobjectstore/health/smart-warnings.json";
const DEFAULT_MEMINFO_PATH: &str = "/proc/meminfo";

#[derive(Clone, Debug)]
struct HomeDashboardAggregatorConfig {
    ssd_root: PathBuf,
    hdd_root: PathBuf,
    store_registry_path: PathBuf,
    appliance_telemetry_path: PathBuf,
    throughput_path: PathBuf,
    smart_warnings_path: PathBuf,
    meminfo_path: PathBuf,
    object_service_status: Option<ObjectServiceStatusView>,
    telemetry_window: ApplianceTelemetryWindow,
}

impl HomeDashboardAggregatorConfig {
    fn from_env() -> Self {
        Self {
            ssd_root: env_path("DASOBJECTSTORE_SSD_ROOT", DEFAULT_SSD_ROOT),
            hdd_root: env_path("DASOBJECTSTORE_HDD_ROOT", DEFAULT_HDD_ROOT),
            store_registry_path: default_store_registry_path(),
            appliance_telemetry_path: env::var_os("DASOBJECTSTORE_WEB_APPLIANCE_TELEMETRY_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|| appliance_telemetry_state_path(DEFAULT_DAEMON_STATE_DIR)),
            throughput_path: env_path(
                "DASOBJECTSTORE_WEB_THROUGHPUT_PATH",
                DEFAULT_THROUGHPUT_PATH,
            ),
            smart_warnings_path: env_path(
                "DASOBJECTSTORE_WEB_SMART_WARNINGS_PATH",
                DEFAULT_SMART_WARNINGS_PATH,
            ),
            meminfo_path: env_path("DASOBJECTSTORE_WEB_MEMINFO_PATH", DEFAULT_MEMINFO_PATH),
            object_service_status: None,
            telemetry_window: ApplianceTelemetryWindow::default(),
        }
    }
}

pub(crate) fn live_home_dashboard() -> HomeDashboardView {
    live_home_dashboard_for_window(ApplianceTelemetryWindow::default())
}

pub(crate) fn live_home_dashboard_for_window(
    window: ApplianceTelemetryWindow,
) -> HomeDashboardView {
    let mut config = HomeDashboardAggregatorConfig::from_env();
    config.telemetry_window = window;
    build_home_dashboard(config)
}

fn build_home_dashboard(config: HomeDashboardAggregatorConfig) -> HomeDashboardView {
    let generated_at_utc = now_utc_string();
    let mut source_warnings = Vec::new();

    let ssd_capacity = capacity_for_root(&config.ssd_root);
    if !config.ssd_root.exists() {
        source_warnings.push(DashboardWarning::new(
            "ssd_root_missing",
            format!(
                "Managed SSD root is not present at {}.",
                config.ssd_root.display()
            ),
        ));
    } else if ssd_capacity.is_none() {
        source_warnings.push(DashboardWarning::new(
            "ssd_capacity_unavailable",
            format!(
                "The Web API could not measure managed SSD capacity at {}.",
                config.ssd_root.display()
            ),
        ));
    }

    let hdd_roots = discover_hdd_roots(&config.hdd_root, &mut source_warnings);
    let hdd_capacities = hdd_roots
        .iter()
        .filter_map(|root| capacity_for_root(root))
        .collect::<Vec<_>>();
    if hdd_roots
        .iter()
        .any(|root| capacity_for_root(root).is_none())
    {
        source_warnings.push(DashboardWarning::new(
            "hdd_capacity_partial",
            "One or more managed HDD roots could not be measured by the Web API.",
        ));
    }

    let telemetry = telemetry::read(&config.appliance_telemetry_path).unwrap_or_else(|warning| {
        if config.appliance_telemetry_path.exists() {
            source_warnings.push(warning);
        }
        None
    });

    let all_capacities = ssd_capacity
        .iter()
        .chain(hdd_capacities.iter())
        .copied()
        .collect::<Vec<_>>();
    let capacity = telemetry
        .as_ref()
        .and_then(|sample_set| telemetry::capacity_summary(sample_set, config.telemetry_window))
        .unwrap_or_else(|| capacity_summary(&all_capacities));
    let drive_count = drive_count_summary(ssd_capacity.is_some(), hdd_capacities.len());
    let mounted_enclosures = enclosure_cards(
        &config.hdd_root,
        &hdd_roots,
        &hdd_capacities,
        &generated_at_utc,
        &source_warnings,
    );
    let smart_warnings =
        read_smart_warnings(&config.smart_warnings_path).unwrap_or_else(|warning| {
            if config.smart_warnings_path.exists() {
                source_warnings.push(warning);
            }
            Vec::new()
        });
    let object_stores =
        registry_object_store_cards(&config.store_registry_path, None, &[], &mut source_warnings);
    let object_service = config
        .object_service_status
        .unwrap_or_else(object_service::status);
    if let Some(message) = &object_service.message {
        source_warnings.push(DashboardWarning::new(
            "object_service_status",
            message.clone(),
        ));
    }
    let memory_stress = telemetry
        .as_ref()
        .and_then(|sample_set| telemetry::memory_stress(sample_set, config.telemetry_window))
        .unwrap_or_else(|| memory_stress(&config.meminfo_path, &mut source_warnings));
    let throughput_7d = telemetry
        .as_ref()
        .and_then(|sample_set| telemetry_throughput(sample_set, config.telemetry_window))
        .or_else(|| read_throughput_7d(&config.throughput_path))
        .unwrap_or_else(|| {
            source_warnings.push(DashboardWarning::new(
                "throughput_telemetry_unavailable",
                "Seven-day throughput telemetry has not yet been written for the Web dashboard.",
            ));
            ThroughputSummaryView::unavailable(
                "Seven-day throughput telemetry has not yet been written for the Web dashboard.",
            )
        });
    let disk_io = telemetry
        .as_ref()
        .and_then(|sample_set| telemetry::disk_io_summary(sample_set, config.telemetry_window))
        .unwrap_or_else(|| {
            DiskIoSummaryView::unavailable("Disk IO telemetry is not available yet.")
        });
    let cpu_usage = telemetry
        .as_ref()
        .and_then(|sample_set| telemetry::cpu_usage_summary(sample_set, config.telemetry_window))
        .unwrap_or_else(|| CpuUsageSummaryView::unavailable("CPU telemetry is not available yet."));
    let active_users = telemetry
        .as_ref()
        .and_then(|sample_set| telemetry::active_users_summary(sample_set, config.telemetry_window))
        .unwrap_or_else(|| {
            ActiveUsersSummaryView::unavailable("Session telemetry is not available yet.")
        });

    let warning_count = source_warnings.len() + smart_warnings.len();
    let health_state = if warning_count == 0 {
        DashboardHealthStateView::Healthy
    } else if hdd_capacities.is_empty() {
        DashboardHealthStateView::Degraded
    } else {
        DashboardHealthStateView::Watch
    };

    HomeDashboardView {
        schema_version: REDESIGN_DASHBOARD_SCHEMA_VERSION.to_string(),
        generated_at_utc: generated_at_utc.clone(),
        health: HealthSummaryView {
            state: health_state,
            label: health_label(health_state, hdd_capacities.len(), object_stores.len())
                .to_string(),
            warning_count,
            critical_count: usize::from(matches!(health_state, DashboardHealthStateView::Critical)),
            action_count: source_warnings.len(),
            last_checked_at_utc: Some(generated_at_utc),
        },
        drives: drive_count,
        capacity,
        mounted_enclosures,
        telemetry_window: telemetry_window_control(config.telemetry_window),
        throughput_7d,
        disk_io,
        cpu_usage,
        active_users,
        ingest: None,
        destage: None,
        object_service,
        memory_stress,
        smart_warnings: SmartWarningsSummaryView::from_warnings(smart_warnings),
        object_stores,
        create_object_store: CreateObjectStoreAffordanceView::admin_required(),
    }
}

fn read_appliance_telemetry(
    path: &Path,
) -> Result<Option<ApplianceTelemetrySampleSet>, DashboardWarning> {
    match fs::read_to_string(path) {
        Ok(contents) => serde_json::from_str::<ApplianceTelemetrySampleSet>(&contents)
            .map(Some)
            .map_err(|error| {
                DashboardWarning::new(
                    "appliance_telemetry_invalid",
                    format!(
                        "Appliance telemetry {} is invalid JSON: {error}.",
                        path.display()
                    ),
                )
            }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(DashboardWarning::new(
            "appliance_telemetry_unreadable",
            format!(
                "Appliance telemetry could not be read from {}: {error}.",
                path.display()
            ),
        )),
    }
}

pub(crate) fn env_path(name: &str, default: &str) -> PathBuf {
    env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default))
}

pub(crate) fn discover_hdd_roots(
    hdd_root: &Path,
    warnings: &mut Vec<DashboardWarning>,
) -> Vec<PathBuf> {
    let entries = match fs::read_dir(hdd_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            warnings.push(DashboardWarning::new(
                "hdd_root_missing",
                format!("Managed HDD root is not present at {}.", hdd_root.display()),
            ));
            return Vec::new();
        }
        Err(error) => {
            warnings.push(DashboardWarning::new(
                "hdd_root_unreadable",
                format!(
                    "Managed HDD root {} could not be read: {error}.",
                    hdd_root.display()
                ),
            ));
            return Vec::new();
        }
    };

    let mut roots = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter(|path| is_managed_hdd_root(path))
        .collect::<Vec<_>>();
    roots.sort();

    if roots.is_empty() {
        warnings.push(DashboardWarning::new(
            "hdd_inventory_empty",
            "No managed HDD roots were detected for the Home dashboard.",
        ));
    }

    roots
}

fn is_managed_hdd_root(path: &Path) -> bool {
    let marker = path.join(".dasobjectstore").join("device.env");
    fs::read_to_string(marker)
        .map(|contents| contents.lines().any(|line| line.starts_with("role=hdd:")))
        .unwrap_or(false)
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct FilesystemCapacity {
    pub(crate) total_bytes: u64,
    pub(crate) available_bytes: u64,
}

#[cfg(unix)]
pub(crate) fn capacity_for_root(path: &Path) -> Option<FilesystemCapacity> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let encoded = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    let result = unsafe { libc::statvfs(encoded.as_ptr(), stat.as_mut_ptr()) };
    if result != 0 {
        return None;
    }
    let stat = unsafe { stat.assume_init() };
    let fragment_size = stat.f_frsize as u64;
    Some(FilesystemCapacity {
        total_bytes: (stat.f_blocks as u64).saturating_mul(fragment_size),
        available_bytes: (stat.f_bavail as u64).saturating_mul(fragment_size),
    })
}

#[cfg(not(unix))]
pub(crate) fn capacity_for_root(_path: &Path) -> Option<FilesystemCapacity> {
    None
}

pub(crate) fn capacity_summary(capacities: &[FilesystemCapacity]) -> CapacitySummaryView {
    let total = capacities
        .iter()
        .map(|capacity| capacity.total_bytes)
        .sum::<u64>();
    let available = capacities
        .iter()
        .map(|capacity| capacity.available_bytes)
        .sum::<u64>();
    let used = total.saturating_sub(available);

    CapacitySummaryView {
        total_tib: format_tib(total),
        used_tib: format_tib(used),
        free_tib: format_tib(available),
        used_percent_basis_points: percent_basis_points(used, total),
    }
}

pub(crate) fn drive_count_summary(ssd_present: bool, hdd_count: usize) -> DriveCountSummaryView {
    let mounted = usize::from(ssd_present) + hdd_count;
    DriveCountSummaryView {
        total: mounted,
        mounted,
        healthy: mounted,
        watch: 0,
        suspect: 0,
        failed: 0,
    }
}

fn enclosure_cards(
    hdd_root: &Path,
    hdd_roots: &[PathBuf],
    hdd_capacities: &[FilesystemCapacity],
    generated_at_utc: &str,
    source_warnings: &[DashboardWarning],
) -> Vec<DasEnclosureCardView> {
    if hdd_roots.is_empty() {
        return Vec::new();
    }

    vec![DasEnclosureCardView {
        enclosure_id: "managed-das-hdd-roots".to_string(),
        display_name: "Managed DAS HDD roots".to_string(),
        mount_path: hdd_root.display().to_string(),
        connection: EnclosureConnectionView {
            bus: "managed-root".to_string(),
            protocol: "filesystem".to_string(),
            link_speed: "host reported".to_string(),
        },
        health: if source_warnings.is_empty() {
            DashboardHealthStateView::Healthy
        } else {
            DashboardHealthStateView::Watch
        },
        drive_count: drive_count_summary(false, hdd_roots.len()),
        capacity: capacity_summary(hdd_capacities),
        last_seen_at_utc: generated_at_utc.to_string(),
        warnings: source_warnings.to_vec(),
    }]
}

fn memory_stress(path: &Path, warnings: &mut Vec<DashboardWarning>) -> MemoryStressView {
    let meminfo = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) => {
            warnings.push(DashboardWarning::new(
                "memory_telemetry_unavailable",
                format!(
                    "Memory telemetry could not be read from {}: {error}.",
                    path.display()
                ),
            ));
            return MemoryStressView {
                state: MemoryStressStateView::Elevated,
                pressure_percent: 0,
                swap_used_percent: 0,
                page_cache_tib: "0.0".to_string(),
                warning: Some(DashboardWarning::new(
                    "memory_telemetry_unavailable",
                    "Memory telemetry could not be read by the Web dashboard.",
                )),
            };
        }
    };

    let total = meminfo_kib(&meminfo, "MemTotal").unwrap_or(0);
    let available = meminfo_kib(&meminfo, "MemAvailable").unwrap_or(0);
    let cached = meminfo_kib(&meminfo, "Cached").unwrap_or(0);
    let swap_total = meminfo_kib(&meminfo, "SwapTotal").unwrap_or(0);
    let swap_free = meminfo_kib(&meminfo, "SwapFree").unwrap_or(0);
    let pressure_percent = percent_u8(total.saturating_sub(available), total);
    let swap_used_percent = percent_u8(swap_total.saturating_sub(swap_free), swap_total);
    let state = match pressure_percent {
        0..=69 => MemoryStressStateView::Nominal,
        70..=84 => MemoryStressStateView::Elevated,
        85..=94 => MemoryStressStateView::High,
        _ => MemoryStressStateView::Critical,
    };
    let warning = matches!(
        state,
        MemoryStressStateView::Elevated
            | MemoryStressStateView::High
            | MemoryStressStateView::Critical
    )
    .then(|| {
        DashboardWarning::new(
            "memory_pressure",
            format!("Memory pressure is {pressure_percent}% on this appliance."),
        )
    });

    MemoryStressView {
        state,
        pressure_percent,
        swap_used_percent,
        page_cache_tib: format_tib(cached.saturating_mul(1024)),
        warning,
    }
}

fn meminfo_kib(contents: &str, key: &str) -> Option<u64> {
    contents.lines().find_map(|line| {
        let (name, rest) = line.split_once(':')?;
        if name != key {
            return None;
        }
        rest.split_whitespace().next()?.parse::<u64>().ok()
    })
}

fn telemetry_capacity_summary(
    sample_set: &ApplianceTelemetrySampleSet,
    window: ApplianceTelemetryWindow,
) -> Option<CapacitySummaryView> {
    let latest = latest_telemetry_sample(sample_set, window)?;
    let total = sum_present(latest.disks.iter().map(|disk| disk.total_bytes))?;
    let available = sum_present(latest.disks.iter().map(|disk| disk.available_bytes))?;
    let available = available.min(total);
    let used = total.saturating_sub(available);
    Some(CapacitySummaryView {
        total_tib: format_tib(total),
        used_tib: format_tib(used),
        free_tib: format_tib(available),
        used_percent_basis_points: percent_basis_points(used, total),
    })
}

fn telemetry_memory_stress(
    sample_set: &ApplianceTelemetrySampleSet,
    window: ApplianceTelemetryWindow,
) -> Option<MemoryStressView> {
    let latest = latest_telemetry_sample(sample_set, window)?;
    let total = latest.memory.total_bytes?;
    let available = latest.memory.available_bytes?.min(total);
    let used = total.saturating_sub(available);
    let pressure_percent = latest
        .memory
        .used_percent
        .and_then(percent_float_u8)
        .unwrap_or_else(|| percent_u8(used, total));
    let swap_total = latest.memory.swap_total_bytes.unwrap_or(0);
    let swap_used = latest.memory.swap_used_bytes.unwrap_or(0).min(swap_total);
    let swap_used_percent = percent_u8(swap_used, swap_total);
    let state = match pressure_percent {
        0..=69 => MemoryStressStateView::Nominal,
        70..=84 => MemoryStressStateView::Elevated,
        85..=94 => MemoryStressStateView::High,
        _ => MemoryStressStateView::Critical,
    };
    let warning = matches!(
        state,
        MemoryStressStateView::Elevated
            | MemoryStressStateView::High
            | MemoryStressStateView::Critical
    )
    .then(|| {
        DashboardWarning::new(
            "memory_pressure",
            format!("Memory pressure is {pressure_percent}% on this appliance."),
        )
    });

    Some(MemoryStressView {
        state,
        pressure_percent,
        swap_used_percent,
        page_cache_tib: "0.0".to_string(),
        warning,
    })
}

fn telemetry_throughput(
    sample_set: &ApplianceTelemetrySampleSet,
    window: ApplianceTelemetryWindow,
) -> Option<ThroughputSummaryView> {
    let samples = telemetry_samples_in_window(sample_set, window);
    if samples.is_empty() {
        return None;
    }

    let mut read_rates = Vec::new();
    let mut write_rates = Vec::new();
    let mut daily: std::collections::BTreeMap<String, (u64, u64)> =
        std::collections::BTreeMap::new();
    for (_, sample) in samples {
        let day = sample.timestamp_utc.chars().take(10).collect::<String>();
        let seconds = sample_set.cadence_seconds.max(0.0);
        let mut sample_read = None::<f64>;
        let mut sample_write = None::<f64>;
        for disk_io in &sample.disk_io {
            if let Some(read) = disk_io
                .read_bytes_per_second
                .filter(|value| value.is_finite())
            {
                let read = read.max(0.0);
                sample_read = Some(sample_read.unwrap_or(0.0) + read);
                if seconds > 0.0 {
                    let entry = daily.entry(day.clone()).or_default();
                    entry.0 = entry.0.saturating_add((read * seconds).round() as u64);
                }
            }
            if let Some(write) = disk_io
                .write_bytes_per_second
                .filter(|value| value.is_finite())
            {
                let write = write.max(0.0);
                sample_write = Some(sample_write.unwrap_or(0.0) + write);
                if seconds > 0.0 {
                    let entry = daily.entry(day.clone()).or_default();
                    entry.1 = entry.1.saturating_add((write * seconds).round() as u64);
                }
            }
        }
        if let Some(read) = sample_read {
            read_rates.push(read);
        }
        if let Some(write) = sample_write {
            write_rates.push(write);
        }
    }
    if read_rates.is_empty() && write_rates.is_empty() {
        return None;
    }

    let read_bytes = daily.values().map(|(read, _)| *read).sum::<u64>();
    let written_bytes = daily.values().map(|(_, written)| *written).sum::<u64>();
    Some(ThroughputSummaryView {
        window_days: telemetry_window_days(window),
        read_tib: format_tib(read_bytes),
        written_tib: format_tib(written_bytes),
        ingest_tib: format_tib(written_bytes),
        avg_read_mib_s: mib_per_second(mean_rate(&read_rates)),
        avg_write_mib_s: mib_per_second(mean_rate(&write_rates)),
        source: "daemon_disk_io".to_string(),
        message: None,
        daily: daily
            .into_iter()
            .map(|(date, (read_bytes, written_bytes))| ThroughputDayView {
                date,
                read_tib: format_tib(read_bytes),
                written_tib: format_tib(written_bytes),
                ingest_tib: format_tib(written_bytes),
            })
            .collect(),
    })
}

fn telemetry_disk_io_summary(
    sample_set: &ApplianceTelemetrySampleSet,
    window: ApplianceTelemetryWindow,
) -> Option<DiskIoSummaryView> {
    let latest = latest_telemetry_sample(sample_set, window)?;
    let mut read_bytes = 0.0;
    let mut write_bytes = 0.0;
    let mut read_ops = 0.0;
    let mut write_ops = 0.0;
    let mut busiest_disk = None::<(String, f64)>;
    let mut saw_value = false;

    for disk_io in &latest.disk_io {
        let read = finite_nonnegative(disk_io.read_bytes_per_second).unwrap_or(0.0);
        let write = finite_nonnegative(disk_io.write_bytes_per_second).unwrap_or(0.0);
        let disk_total = read + write;
        if disk_io.read_bytes_per_second.is_some() || disk_io.write_bytes_per_second.is_some() {
            saw_value = true;
            read_bytes += read;
            write_bytes += write;
            if busiest_disk
                .as_ref()
                .is_none_or(|(_, current)| disk_total > *current)
            {
                busiest_disk = Some((disk_io.disk_id.clone(), disk_total));
            }
        }
        if let Some(value) = finite_nonnegative(disk_io.read_operations_per_second) {
            saw_value = true;
            read_ops += value;
        }
        if let Some(value) = finite_nonnegative(disk_io.write_operations_per_second) {
            saw_value = true;
            write_ops += value;
        }
    }

    saw_value.then(|| DiskIoSummaryView {
        available: true,
        read_mib_s: mib_per_second(read_bytes.round() as u64),
        write_mib_s: mib_per_second(write_bytes.round() as u64),
        read_ops_s: rounded_u32(read_ops),
        write_ops_s: rounded_u32(write_ops),
        busiest_disk_id: busiest_disk.map(|(disk_id, _)| disk_id),
        sample_timestamp_utc: None,
        sample_age_seconds: None,
        per_disk: Vec::new(),
        collection_quality: None,
        missing_data: Vec::new(),
        state: TelemetryCardStateView::Nominal,
        message: None,
    })
}

fn telemetry_cpu_usage_summary(
    sample_set: &ApplianceTelemetrySampleSet,
    window: ApplianceTelemetryWindow,
) -> Option<CpuUsageSummaryView> {
    let latest = latest_telemetry_sample(sample_set, window)?;
    let usage_percent = latest.cpu.usage_percent.and_then(percent_float_u8);
    let load_average_1m = latest
        .cpu
        .load_average_1m
        .filter(|value| value.is_finite())
        .map(|value| format!("{:.2}", value.max(0.0)));
    if usage_percent.is_none()
        && load_average_1m.is_none()
        && latest.cpu.logical_core_count.is_none()
    {
        return None;
    }
    let state = match usage_percent.unwrap_or(0) {
        0..=69 => TelemetryCardStateView::Nominal,
        70..=84 => TelemetryCardStateView::Elevated,
        85..=94 => TelemetryCardStateView::High,
        _ => TelemetryCardStateView::High,
    };

    Some(CpuUsageSummaryView {
        available: true,
        usage_percent,
        load_average_1m,
        logical_core_count: latest.cpu.logical_core_count,
        state,
        message: None,
    })
}

fn telemetry_active_users_summary(
    sample_set: &ApplianceTelemetrySampleSet,
    window: ApplianceTelemetryWindow,
) -> Option<ActiveUsersSummaryView> {
    let latest = latest_telemetry_sample(sample_set, window)?;
    let sessions = &latest.sessions;
    if sessions.web_active_sessions.is_none()
        && sessions.remote_agent_active_sessions.is_none()
        && sessions.distinct_logged_in_users.is_none()
        && sessions.administrator_sessions.is_none()
        && sessions.operator_sessions.is_none()
    {
        return None;
    }
    let web = sessions.web_active_sessions.unwrap_or(0);
    let remote = sessions.remote_agent_active_sessions.unwrap_or(0);

    Some(ActiveUsersSummaryView {
        available: true,
        active_sessions: web.saturating_add(remote),
        distinct_logged_in_users: sessions.distinct_logged_in_users.unwrap_or(0),
        administrator_sessions: sessions.administrator_sessions.unwrap_or(0),
        operator_sessions: sessions.operator_sessions.unwrap_or(0),
        remote_agent_sessions: remote,
        state: TelemetryCardStateView::Nominal,
        message: None,
    })
}

fn latest_telemetry_sample(
    sample_set: &ApplianceTelemetrySampleSet,
    window: ApplianceTelemetryWindow,
) -> Option<&ApplianceTelemetrySample> {
    telemetry_samples_in_window(sample_set, window)
        .into_iter()
        .max_by_key(|(timestamp, _)| *timestamp)
        .map(|(_, sample)| sample)
}

fn telemetry_samples_in_window(
    sample_set: &ApplianceTelemetrySampleSet,
    window: ApplianceTelemetryWindow,
) -> Vec<(i64, &ApplianceTelemetrySample)> {
    let samples = sorted_telemetry_samples(sample_set);
    let Some((newest_timestamp, _)) = samples.last() else {
        return Vec::new();
    };
    let oldest_allowed = newest_timestamp.saturating_sub(window.seconds());
    samples
        .into_iter()
        .filter(|(timestamp, _)| *timestamp >= oldest_allowed)
        .collect()
}

fn telemetry_window_control(selected: ApplianceTelemetryWindow) -> TelemetryWindowControlView {
    TelemetryWindowControlView {
        selected: telemetry_window_value(selected).to_string(),
        selected_label: telemetry_window_label(selected).to_string(),
        options: [
            ApplianceTelemetryWindow::OneHour,
            ApplianceTelemetryWindow::OneDay,
            ApplianceTelemetryWindow::TenDays,
            ApplianceTelemetryWindow::ThreeMonths,
        ]
        .into_iter()
        .map(|window| TelemetryWindowOptionView {
            value: telemetry_window_value(window).to_string(),
            label: telemetry_window_label(window).to_string(),
            selected: window == selected,
        })
        .collect(),
    }
}

fn telemetry_window_value(window: ApplianceTelemetryWindow) -> &'static str {
    match window {
        ApplianceTelemetryWindow::OneHour => "one_hour",
        ApplianceTelemetryWindow::OneDay => "one_day",
        ApplianceTelemetryWindow::TenDays => "ten_days",
        ApplianceTelemetryWindow::ThreeMonths => "three_months",
    }
}

fn telemetry_window_label(window: ApplianceTelemetryWindow) -> &'static str {
    match window {
        ApplianceTelemetryWindow::OneHour => "1 hour",
        ApplianceTelemetryWindow::OneDay => "1 day",
        ApplianceTelemetryWindow::TenDays => "10 days",
        ApplianceTelemetryWindow::ThreeMonths => "3 months",
    }
}

fn telemetry_window_days(window: ApplianceTelemetryWindow) -> u8 {
    match window {
        ApplianceTelemetryWindow::OneHour => 0,
        ApplianceTelemetryWindow::OneDay => 1,
        ApplianceTelemetryWindow::TenDays => 10,
        ApplianceTelemetryWindow::ThreeMonths => 92,
    }
}

fn sorted_telemetry_samples(
    sample_set: &ApplianceTelemetrySampleSet,
) -> Vec<(i64, &ApplianceTelemetrySample)> {
    let mut samples = sample_set
        .samples
        .iter()
        .filter_map(|sample| {
            parse_utc_timestamp_seconds(&sample.timestamp_utc).map(|timestamp| (timestamp, sample))
        })
        .collect::<Vec<_>>();
    samples.sort_by_key(|(timestamp, _)| *timestamp);
    samples
}

fn sum_present(values: impl Iterator<Item = Option<u64>>) -> Option<u64> {
    let mut saw_value = false;
    let mut total = 0u64;
    for value in values.flatten() {
        saw_value = true;
        total = total.saturating_add(value);
    }
    saw_value.then_some(total)
}

fn percent_float_u8(value: f64) -> Option<u8> {
    value
        .is_finite()
        .then(|| value.clamp(0.0, 100.0).round() as u8)
}

fn finite_nonnegative(value: Option<f64>) -> Option<f64> {
    value
        .filter(|value| value.is_finite())
        .map(|value| value.max(0.0))
}

fn rounded_u32(value: f64) -> u32 {
    value.round().clamp(0.0, f64::from(u32::MAX)) as u32
}

fn mean_rate(values: &[f64]) -> u64 {
    if values.is_empty() {
        return 0;
    }
    (values.iter().sum::<f64>() / values.len() as f64).round() as u64
}

#[derive(Debug, Deserialize)]
struct ThroughputJson {
    #[serde(default = "default_window_days")]
    window_days: u8,
    #[serde(default)]
    read_bytes: u64,
    #[serde(default)]
    written_bytes: u64,
    #[serde(default)]
    ingest_bytes: u64,
    #[serde(default)]
    avg_read_bytes_per_second: u64,
    #[serde(default)]
    avg_write_bytes_per_second: u64,
    #[serde(default)]
    daily: Vec<ThroughputDayJson>,
}

#[derive(Debug, Deserialize)]
struct ThroughputDayJson {
    date: String,
    #[serde(default)]
    read_bytes: u64,
    #[serde(default)]
    written_bytes: u64,
    #[serde(default)]
    ingest_bytes: u64,
}

fn read_throughput_7d(path: &Path) -> Option<ThroughputSummaryView> {
    let contents = fs::read_to_string(path).ok()?;
    let parsed = serde_json::from_str::<ThroughputJson>(&contents).ok()?;
    Some(ThroughputSummaryView {
        window_days: parsed.window_days,
        read_tib: format_tib(parsed.read_bytes),
        written_tib: format_tib(parsed.written_bytes),
        ingest_tib: format_tib(parsed.ingest_bytes),
        avg_read_mib_s: mib_per_second(parsed.avg_read_bytes_per_second),
        avg_write_mib_s: mib_per_second(parsed.avg_write_bytes_per_second),
        source: "legacy_file".to_string(),
        message: None,
        daily: parsed
            .daily
            .into_iter()
            .map(|day| ThroughputDayView {
                date: day.date,
                read_tib: format_tib(day.read_bytes),
                written_tib: format_tib(day.written_bytes),
                ingest_tib: format_tib(day.ingest_bytes),
            })
            .collect(),
    })
}

fn default_window_days() -> u8 {
    7
}

fn read_smart_warnings(path: &Path) -> Result<Vec<SmartWarningView>, DashboardWarning> {
    let contents = fs::read_to_string(path).map_err(|error| {
        DashboardWarning::new(
            "smart_warning_telemetry_unreadable",
            format!(
                "SMART warning telemetry could not be read from {}: {error}.",
                path.display()
            ),
        )
    })?;
    serde_json::from_str::<Vec<SmartWarningView>>(&contents).map_err(|error| {
        DashboardWarning::new(
            "smart_warning_telemetry_invalid",
            format!(
                "SMART warning telemetry {} is invalid JSON: {error}.",
                path.display()
            ),
        )
    })
}

fn health_label(
    state: DashboardHealthStateView,
    hdd_count: usize,
    store_count: usize,
) -> &'static str {
    match (state, hdd_count, store_count) {
        (DashboardHealthStateView::Healthy, _, _) => "Live inventory healthy",
        (_, 0, _) => "Managed storage unavailable",
        (_, _, 0) => "ObjectStore registry empty",
        _ => "Live inventory watch",
    }
}

pub(super) fn percent_basis_points(used: u64, total: u64) -> u16 {
    if total == 0 {
        return 0;
    }
    ((u128::from(used) * 10_000) / u128::from(total)).min(u128::from(u16::MAX)) as u16
}

pub(super) fn percent_u8(used: u64, total: u64) -> u8 {
    if total == 0 {
        return 0;
    }
    ((u128::from(used) * 100) / u128::from(total)).min(100) as u8
}

pub(super) fn format_tib(bytes: u64) -> String {
    const TIB: f64 = 1024.0 * 1024.0 * 1024.0 * 1024.0;
    format!("{:.1}", bytes as f64 / TIB)
}

pub(super) fn mib_per_second(bytes_per_second: u64) -> u32 {
    (bytes_per_second / (1024 * 1024)).min(u64::from(u32::MAX)) as u32
}

pub(crate) fn now_utc_string() -> String {
    Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok()
        .and_then(|output| output.status.success().then_some(output.stdout))
        .and_then(|stdout| String::from_utf8(stdout).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            let seconds = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_secs())
                .unwrap_or_default();
            format!("unix:{seconds}")
        })
}

#[cfg(test)]
mod tests {
    use super::{build_home_dashboard, HomeDashboardAggregatorConfig};
    use crate::dashboard::ObjectServiceStatusView;
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_core::store::{StoreClass, StorePolicy};
    use dasobjectstore_daemon::api::ApplianceTelemetryWindow;
    use dasobjectstore_object_service::StoreServiceDefinition;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn home_aggregator_uses_managed_roots_registry_memory_and_throughput() {
        let root = temp_root("home-aggregator-live");
        let ssd_root = root.join("ssd");
        let hdd_root = root.join("hdd");
        let disk_a = hdd_root.join("qnap-1057");
        fs::create_dir_all(ssd_root.join(".dasobjectstore")).expect("ssd root");
        fs::create_dir_all(disk_a.join(".dasobjectstore")).expect("hdd root");
        fs::write(
            disk_a.join(".dasobjectstore/device.env"),
            "role=hdd:qnap-1057\n",
        )
        .expect("hdd marker");
        let registry_path = root.join("stores.json");
        let store = StoreServiceDefinition {
            store_id: StoreId::new("zymo").expect("store id"),
            policy: StorePolicy::defaults_for(StoreClass::GeneratedData),
            bucket_name: Some("dos-zymo".to_string()),
            reader_group: None,
            writer_group: Some("bioinformatics".to_string()),
            public: false,
        };
        fs::write(
            &registry_path,
            serde_json::to_string_pretty(&vec![store]).expect("registry json"),
        )
        .expect("registry write");
        let meminfo_path = root.join("meminfo");
        fs::write(
            &meminfo_path,
            "MemTotal:       1000000 kB\nMemAvailable:    750000 kB\nCached:          250000 kB\nSwapTotal:            0 kB\nSwapFree:             0 kB\n",
        )
        .expect("meminfo");
        let throughput_path = root.join("throughput.json");
        fs::write(
            &throughput_path,
            r#"{"window_days":7,"read_bytes":1099511627776,"written_bytes":2199023255552,"ingest_bytes":3298534883328,"avg_read_bytes_per_second":104857600,"avg_write_bytes_per_second":209715200,"daily":[{"date":"2026-07-08","read_bytes":1099511627776,"written_bytes":2199023255552,"ingest_bytes":3298534883328}]}"#,
        )
        .expect("throughput");

        let view = build_home_dashboard(HomeDashboardAggregatorConfig {
            ssd_root,
            hdd_root,
            store_registry_path: registry_path,
            appliance_telemetry_path: root.join("missing-appliance-telemetry.json"),
            throughput_path,
            smart_warnings_path: root.join("missing-smart.json"),
            meminfo_path,
            object_service_status: Some(healthy_object_service_status()),
            telemetry_window: ApplianceTelemetryWindow::default(),
        });

        assert_eq!(view.health.label, "Live inventory healthy");
        assert_eq!(view.drives.mounted, 2);
        assert_eq!(view.mounted_enclosures.len(), 1);
        assert_eq!(view.object_stores.len(), 1);
        assert_eq!(view.object_stores[0].store_id, "zymo");
        assert_eq!(
            view.object_stores[0].writer_group.as_deref(),
            Some("bioinformatics")
        );
        assert_eq!(view.memory_stress.pressure_percent, 25);
        assert_eq!(view.throughput_7d.avg_write_mib_s, 200);
        assert_eq!(view.throughput_7d.source, "legacy_file");
        assert_eq!(view.throughput_7d.daily.len(), 1);
        assert!(view.object_service.remote_ready);
        assert_eq!(
            view.object_service.remote_url.as_deref(),
            Some("http://192.168.1.192:3900")
        );
    }

    #[test]
    fn home_aggregator_reports_missing_managed_storage_without_bootstrap_fixture() {
        let root = temp_root("home-aggregator-missing");

        let view = build_home_dashboard(HomeDashboardAggregatorConfig {
            ssd_root: root.join("missing-ssd"),
            hdd_root: root.join("missing-hdd"),
            store_registry_path: root.join("missing-stores.json"),
            appliance_telemetry_path: root.join("missing-appliance-telemetry.json"),
            throughput_path: root.join("missing-throughput.json"),
            smart_warnings_path: root.join("missing-smart.json"),
            meminfo_path: root.join("missing-meminfo"),
            object_service_status: Some(ObjectServiceStatusView {
                active: false,
                remote_ready: false,
                bind_address: "0.0.0.0".to_string(),
                port: 3900,
                local_url: "http://127.0.0.1:3900".to_string(),
                remote_url: None,
                service_state: None,
                message: Some("S3-compatible object service is not reachable.".to_string()),
            }),
            telemetry_window: ApplianceTelemetryWindow::default(),
        });

        assert_eq!(
            view.health.state,
            crate::dashboard::DashboardHealthStateView::Degraded
        );
        assert_eq!(view.health.label, "Managed storage unavailable");
        assert_ne!(view.health.label, "Inventory pending");
        assert_eq!(view.drives.total, 0);
        assert!(view.health.warning_count >= 3);
        assert!(!view.object_service.remote_ready);
    }

    #[test]
    fn home_aggregator_prefers_appliance_telemetry_for_existing_summary_cards() {
        let root = temp_root("home-aggregator-appliance-telemetry");
        let ssd_root = root.join("ssd");
        let hdd_root = root.join("hdd");
        let disk_a = hdd_root.join("qnap-1057");
        fs::create_dir_all(ssd_root.join(".dasobjectstore")).expect("ssd root");
        fs::create_dir_all(disk_a.join(".dasobjectstore")).expect("hdd root");
        fs::write(
            disk_a.join(".dasobjectstore/device.env"),
            "role=hdd:qnap-1057\n",
        )
        .expect("hdd marker");
        let registry_path = root.join("stores.json");
        fs::write(&registry_path, "[]").expect("registry write");
        let meminfo_path = root.join("meminfo");
        fs::write(
            &meminfo_path,
            "MemTotal:       1000000 kB\nMemAvailable:    900000 kB\nCached:          100000 kB\nSwapTotal:            0 kB\nSwapFree:             0 kB\n",
        )
        .expect("meminfo");
        let throughput_path = root.join("throughput.json");
        fs::write(
            &throughput_path,
            r#"{"window_days":7,"avg_write_bytes_per_second":1048576}"#,
        )
        .expect("throughput");
        let telemetry_path = root.join("appliance-telemetry.v1.json");
        fs::write(&telemetry_path, appliance_telemetry_json()).expect("telemetry write");

        let view = build_home_dashboard(HomeDashboardAggregatorConfig {
            ssd_root,
            hdd_root,
            store_registry_path: registry_path,
            appliance_telemetry_path: telemetry_path,
            throughput_path,
            smart_warnings_path: root.join("missing-smart.json"),
            meminfo_path,
            object_service_status: Some(healthy_object_service_status()),
            telemetry_window: ApplianceTelemetryWindow::TenDays,
        });

        assert_eq!(view.telemetry_window.selected, "ten_days");
        assert_eq!(view.telemetry_window.selected_label, "10 days");
        assert_eq!(view.throughput_7d.window_days, 10);
        assert_eq!(view.capacity.total_tib, "4.0");
        assert_eq!(view.capacity.free_tib, "3.0");
        assert_eq!(view.capacity.used_percent_basis_points, 2_500);
        assert_eq!(view.memory_stress.pressure_percent, 80);
        assert_eq!(
            view.memory_stress.state,
            crate::dashboard::MemoryStressStateView::Elevated
        );
        assert_eq!(view.throughput_7d.avg_read_mib_s, 10);
        assert_eq!(view.throughput_7d.avg_write_mib_s, 20);
        assert_eq!(view.throughput_7d.source, "daemon_disk_io");
        assert_eq!(view.throughput_7d.daily.len(), 1);
        assert!(view.disk_io.available);
        assert_eq!(view.disk_io.read_mib_s, 10);
        assert_eq!(view.disk_io.write_mib_s, 20);
        assert_eq!(view.disk_io.busiest_disk_id.as_deref(), Some("qnap-1057"));
        assert!(view.cpu_usage.available);
        assert_eq!(view.cpu_usage.usage_percent, Some(12));
        assert_eq!(view.cpu_usage.logical_core_count, Some(2));
        assert!(view.active_users.available);
        assert_eq!(view.active_users.active_sessions, 1);
        assert_eq!(view.active_users.distinct_logged_in_users, 1);
    }

    fn healthy_object_service_status() -> ObjectServiceStatusView {
        ObjectServiceStatusView {
            active: true,
            remote_ready: true,
            bind_address: "0.0.0.0".to_string(),
            port: 3900,
            local_url: "http://127.0.0.1:3900".to_string(),
            remote_url: Some("http://192.168.1.192:3900".to_string()),
            service_state: Some("Up 1 minute".to_string()),
            message: None,
        }
    }

    fn temp_root(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("dos-gui-{label}-{unique}"));
        fs::create_dir_all(&root).expect("temp root");
        root
    }

    fn appliance_telemetry_json() -> &'static str {
        r#"{
          "schema_version": "dasobjectstore.appliance_telemetry.v1",
          "generated_at_utc": "2026-07-09T18:30:00Z",
          "cadence_seconds": 30.0,
          "source": {
            "appliance_id": "fixture-appliance",
            "host_id": "fixture-host",
            "hostname": "fixture-hostname"
          },
          "samples": [
            {
              "timestamp_utc": "2026-07-09T18:29:30Z",
              "collection_quality": "complete",
              "missing_data": [],
              "cpu": {
                "usage_percent": 10.0,
                "load_average_1m": 0.1,
                "load_average_5m": 0.2,
                "load_average_15m": 0.3,
                "logical_core_count": 2,
                "missing_reason": null
              },
              "memory": {
                "total_bytes": 1000,
                "available_bytes": 200,
                "used_percent": 80.0,
                "swap_total_bytes": 100,
                "swap_used_bytes": 5,
                "missing_reason": null
              },
              "enclosures": [],
              "disks": [{
                "disk_id": "qnap-1057",
                "label": "QNAP bay 1",
                "mount_path": "/srv/dasobjectstore/hdd/qnap-1057",
                "role": "hdd",
                "enclosure_id": "qnap",
                "device_path": "/dev/disk/by-id/qnap-1057",
                "filesystem": "ext4",
                "total_bytes": 4398046511104,
                "available_bytes": 3298534883328,
                "used_percent": 25.0,
                "missing_reason": null
              }],
              "disk_io": [{
                "disk_id": "qnap-1057",
                "label": "QNAP bay 1",
                "mount_path": "/srv/dasobjectstore/hdd/qnap-1057",
                "role": "hdd",
                "enclosure_id": "qnap",
                "device_path": "/dev/disk/by-id/qnap-1057",
                "device_name": "sda",
                "read_bytes_per_second": 10485760.0,
                "write_bytes_per_second": 20971520.0,
                "read_operations_per_second": 10.0,
                "write_operations_per_second": 20.0,
                "average_await_millis": 2.0,
                "io_time_percent": 5.0,
                "missing_reason": null
              }],
              "sessions": {
                "web_active_sessions": 1,
                "remote_agent_active_sessions": 0,
                "distinct_logged_in_users": 1,
                "administrator_sessions": 1,
                "operator_sessions": 0,
                "missing_reason": null
              }
            },
            {
              "timestamp_utc": "2026-07-09T18:30:00Z",
              "collection_quality": "complete",
              "missing_data": [],
              "cpu": {
                "usage_percent": 12.0,
                "load_average_1m": 0.1,
                "load_average_5m": 0.2,
                "load_average_15m": 0.3,
                "logical_core_count": 2,
                "missing_reason": null
              },
              "memory": {
                "total_bytes": 1000,
                "available_bytes": 200,
                "used_percent": 80.0,
                "swap_total_bytes": 100,
                "swap_used_bytes": 5,
                "missing_reason": null
              },
              "enclosures": [],
              "disks": [{
                "disk_id": "qnap-1057",
                "label": "QNAP bay 1",
                "mount_path": "/srv/dasobjectstore/hdd/qnap-1057",
                "role": "hdd",
                "enclosure_id": "qnap",
                "device_path": "/dev/disk/by-id/qnap-1057",
                "filesystem": "ext4",
                "total_bytes": 4398046511104,
                "available_bytes": 3298534883328,
                "used_percent": 25.0,
                "missing_reason": null
              }],
              "disk_io": [{
                "disk_id": "qnap-1057",
                "label": "QNAP bay 1",
                "mount_path": "/srv/dasobjectstore/hdd/qnap-1057",
                "role": "hdd",
                "enclosure_id": "qnap",
                "device_path": "/dev/disk/by-id/qnap-1057",
                "device_name": "sda",
                "read_bytes_per_second": 10485760.0,
                "write_bytes_per_second": 20971520.0,
                "read_operations_per_second": 10.0,
                "write_operations_per_second": 20.0,
                "average_await_millis": 2.0,
                "io_time_percent": 5.0,
                "missing_reason": null
              }],
              "sessions": {
                "web_active_sessions": 1,
                "remote_agent_active_sessions": 0,
                "distinct_logged_in_users": 1,
                "administrator_sessions": 1,
                "operator_sessions": 0,
                "missing_reason": null
              }
            }
          ]
        }"#
    }
}
