use super::model::{
    ApplianceCpuTelemetry, ApplianceDiskCapacityTelemetry, ApplianceEnclosureTelemetry,
    ApplianceMemoryTelemetry, ApplianceTelemetryCollectorError, ApplianceTelemetryMissingReason,
    LinuxCpuSnapshot, LinuxHostTelemetrySample,
};
use super::service_loop::ApplianceHostTelemetryCollector;
use dasobjectstore_metadata::{measure_ssd_capacity, SsdCapacity, SsdCapacityMeasurementError};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const DEFAULT_APPLIANCE_TELEMETRY_HDD_ROOT: &str = "/srv/dasobjectstore/hdd";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinuxProcTelemetryCollector {
    proc_root: PathBuf,
    hdd_root: Option<PathBuf>,
}

impl LinuxProcTelemetryCollector {
    pub fn new(proc_root: impl Into<PathBuf>) -> Self {
        Self {
            proc_root: proc_root.into(),
            hdd_root: None,
        }
    }

    pub fn with_hdd_root(mut self, hdd_root: impl Into<PathBuf>) -> Self {
        self.hdd_root = Some(hdd_root.into());
        self
    }

    pub fn proc_root(&self) -> &Path {
        &self.proc_root
    }

    pub fn hdd_root(&self) -> Option<&Path> {
        self.hdd_root.as_deref()
    }

    pub fn collect(
        &self,
        previous_cpu: Option<&LinuxCpuSnapshot>,
    ) -> Result<LinuxHostTelemetrySample, ApplianceTelemetryCollectorError> {
        let proc_stat = self.read_proc_file("stat")?;
        let proc_loadavg = self.read_proc_file("loadavg")?;
        let proc_meminfo = self.read_proc_file("meminfo")?;
        let cpu_snapshot = parse_linux_cpu_snapshot(&proc_stat)?;
        let (enclosures, disks) = match self.hdd_root.as_deref() {
            Some(hdd_root) => collect_linux_disk_capacity_telemetry(hdd_root)?,
            None => (Vec::new(), Vec::new()),
        };

        Ok(LinuxHostTelemetrySample {
            cpu: collect_linux_cpu_telemetry(previous_cpu, &cpu_snapshot, &proc_loadavg),
            memory: collect_linux_memory_telemetry(&proc_meminfo),
            enclosures,
            disks,
            cpu_snapshot,
        })
    }

    fn read_proc_file(&self, name: &str) -> Result<String, ApplianceTelemetryCollectorError> {
        let path = self.proc_root.join(name);
        fs::read_to_string(&path).map_err(|error| ApplianceTelemetryCollectorError::Io {
            path,
            message: error.to_string(),
        })
    }
}

impl Default for LinuxProcTelemetryCollector {
    fn default() -> Self {
        Self::new("/proc").with_hdd_root(DEFAULT_APPLIANCE_TELEMETRY_HDD_ROOT)
    }
}

impl ApplianceHostTelemetryCollector for LinuxProcTelemetryCollector {
    fn collect(
        &mut self,
        previous_cpu: Option<&LinuxCpuSnapshot>,
    ) -> Result<LinuxHostTelemetrySample, ApplianceTelemetryCollectorError> {
        LinuxProcTelemetryCollector::collect(self, previous_cpu)
    }
}

pub fn parse_linux_cpu_snapshot(
    proc_stat: &str,
) -> Result<LinuxCpuSnapshot, ApplianceTelemetryCollectorError> {
    let aggregate = proc_stat
        .lines()
        .find(|line| line.starts_with("cpu "))
        .ok_or_else(|| {
            ApplianceTelemetryCollectorError::InvalidProcStat(
                "missing aggregate cpu line".to_string(),
            )
        })?;
    let counters = aggregate
        .split_whitespace()
        .skip(1)
        .map(|field| {
            field.parse::<u64>().map_err(|error| {
                ApplianceTelemetryCollectorError::InvalidProcStat(format!(
                    "cpu counter {field:?} is not an integer: {error}"
                ))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if counters.len() < 5 {
        return Err(ApplianceTelemetryCollectorError::InvalidProcStat(
            "aggregate cpu line has fewer than five counters".to_string(),
        ));
    }

    let total_jiffies = counters.iter().copied().sum();
    let idle_jiffies = counters[3].saturating_add(counters[4]);
    let logical_core_count = proc_stat
        .lines()
        .filter(|line| {
            let Some(rest) = line.strip_prefix("cpu") else {
                return false;
            };
            !rest.is_empty() && rest.chars().next().is_some_and(|ch| ch.is_ascii_digit())
        })
        .count() as u64;

    Ok(LinuxCpuSnapshot {
        total_jiffies,
        idle_jiffies,
        logical_core_count,
    })
}

pub fn collect_linux_cpu_telemetry(
    previous: Option<&LinuxCpuSnapshot>,
    current: &LinuxCpuSnapshot,
    proc_loadavg: &str,
) -> ApplianceCpuTelemetry {
    let (load_average_1m, load_average_5m, load_average_15m) = parse_load_averages(proc_loadavg);
    let (usage_percent, missing_reason) = match previous {
        None => (None, Some(ApplianceTelemetryMissingReason::DaemonStartup)),
        Some(previous) => {
            let total_delta = current.total_jiffies.saturating_sub(previous.total_jiffies);
            let idle_delta = current.idle_jiffies.saturating_sub(previous.idle_jiffies);
            if current.total_jiffies < previous.total_jiffies
                || current.idle_jiffies < previous.idle_jiffies
            {
                (None, Some(ApplianceTelemetryMissingReason::CounterReset))
            } else if total_delta == 0 || idle_delta > total_delta {
                (None, Some(ApplianceTelemetryMissingReason::SampleTimeout))
            } else {
                let busy_delta = total_delta - idle_delta;
                (Some(percent(busy_delta, total_delta)), None)
            }
        }
    };

    ApplianceCpuTelemetry {
        usage_percent,
        load_average_1m,
        load_average_5m,
        load_average_15m,
        logical_core_count: Some(current.logical_core_count),
        missing_reason,
    }
}

pub fn collect_linux_memory_telemetry(proc_meminfo: &str) -> ApplianceMemoryTelemetry {
    let values = parse_meminfo_kib(proc_meminfo);
    let total_bytes = values.get("MemTotal").copied().map(kib_to_bytes);
    let available_bytes = values.get("MemAvailable").copied().map(kib_to_bytes);
    let swap_total_bytes = values.get("SwapTotal").copied().map(kib_to_bytes);
    let swap_free_bytes = values.get("SwapFree").copied().map(kib_to_bytes);
    let swap_used_bytes = match (swap_total_bytes, swap_free_bytes) {
        (Some(total), Some(free)) => Some(total.saturating_sub(free)),
        _ => None,
    };
    let used_percent = match (total_bytes, available_bytes) {
        (Some(total), Some(available)) if total > 0 => {
            Some(percent(total.saturating_sub(available), total))
        }
        _ => None,
    };
    let missing_reason = if total_bytes.is_none() || available_bytes.is_none() {
        Some(ApplianceTelemetryMissingReason::CollectorUnavailable)
    } else {
        None
    };

    ApplianceMemoryTelemetry {
        total_bytes,
        available_bytes,
        used_percent,
        swap_total_bytes,
        swap_used_bytes,
        missing_reason,
    }
}

pub fn collect_linux_disk_capacity_telemetry(
    hdd_root: impl AsRef<Path>,
) -> Result<
    (
        Vec<ApplianceEnclosureTelemetry>,
        Vec<ApplianceDiskCapacityTelemetry>,
    ),
    ApplianceTelemetryCollectorError,
> {
    let hdd_root = hdd_root.as_ref();
    let entries = match fs::read_dir(hdd_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok((Vec::new(), Vec::new()));
        }
        Err(error) => {
            return Err(ApplianceTelemetryCollectorError::Io {
                path: hdd_root.to_path_buf(),
                message: error.to_string(),
            });
        }
    };
    let mut disks = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|error| ApplianceTelemetryCollectorError::Io {
            path: hdd_root.to_path_buf(),
            message: error.to_string(),
        })?;
        let file_type =
            entry
                .file_type()
                .map_err(|error| ApplianceTelemetryCollectorError::Io {
                    path: entry.path(),
                    message: error.to_string(),
                })?;
        if !file_type.is_dir() {
            continue;
        }
        let mount_path = entry.path();
        let marker_path = mount_path.join(".dasobjectstore").join("device.env");
        let marker = match fs::read_to_string(&marker_path) {
            Ok(marker) => marker,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(ApplianceTelemetryCollectorError::Io {
                    path: marker_path,
                    message: error.to_string(),
                });
            }
        };
        let Some(marker) = parse_managed_hdd_marker(&marker_path, &marker)? else {
            continue;
        };
        let (total_bytes, available_bytes, used_percent, missing_reason) =
            match measure_disk_capacity(&mount_path) {
                Ok(capacity) => (
                    Some(capacity.total_bytes),
                    Some(capacity.available_bytes),
                    Some(percent(capacity.used_bytes(), capacity.total_bytes)),
                    None,
                ),
                Err(error) => (
                    None,
                    None,
                    None,
                    Some(missing_reason_for_capacity_error(&error)),
                ),
            };
        disks.push(ApplianceDiskCapacityTelemetry {
            disk_id: marker.disk_id,
            label: marker.label,
            mount_path: mount_path.to_string_lossy().to_string(),
            role: "hdd".to_string(),
            enclosure_id: marker.enclosure_id,
            device_path: marker.device_path,
            filesystem: marker.filesystem,
            total_bytes,
            available_bytes,
            used_percent,
            missing_reason,
        });
    }

    disks.sort_by(|left, right| left.disk_id.cmp(&right.disk_id));
    let enclosures = enclosure_capacity_summaries(&disks);
    Ok((enclosures, disks))
}

#[derive(Debug)]
struct ManagedHddMarker {
    disk_id: String,
    label: Option<String>,
    enclosure_id: Option<String>,
    device_path: Option<String>,
    filesystem: Option<String>,
}

fn parse_managed_hdd_marker(
    marker_path: &Path,
    marker: &str,
) -> Result<Option<ManagedHddMarker>, ApplianceTelemetryCollectorError> {
    let values = parse_device_marker_values(marker);
    let Some(role) = values.get("role") else {
        return Ok(None);
    };
    let Some(disk_id) = role.strip_prefix("hdd:") else {
        return Ok(None);
    };
    if disk_id.trim().is_empty() {
        return Err(ApplianceTelemetryCollectorError::InvalidDeviceMarker {
            path: marker_path.to_path_buf(),
            message: "hdd role has a blank disk id".to_string(),
        });
    }

    Ok(Some(ManagedHddMarker {
        disk_id: disk_id.to_string(),
        label: optional_marker_value(&values, "label").or_else(|| Some(disk_id.to_string())),
        enclosure_id: optional_marker_value(&values, "enclosure_id"),
        device_path: optional_marker_value(&values, "device"),
        filesystem: optional_marker_value(&values, "filesystem"),
    }))
}

fn parse_device_marker_values(marker: &str) -> BTreeMap<&str, &str> {
    marker
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            Some((key.trim(), value.trim()))
        })
        .collect()
}

fn optional_marker_value(values: &BTreeMap<&str, &str>, key: &str) -> Option<String> {
    values
        .get(key)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn missing_reason_for_capacity_error(
    error: &SsdCapacityMeasurementError,
) -> ApplianceTelemetryMissingReason {
    match error {
        SsdCapacityMeasurementError::Io { source, .. }
            if source.kind() == io::ErrorKind::PermissionDenied =>
        {
            ApplianceTelemetryMissingReason::PermissionDenied
        }
        SsdCapacityMeasurementError::UnsupportedPlatform => {
            ApplianceTelemetryMissingReason::UnsupportedPlatform
        }
        _ => ApplianceTelemetryMissingReason::CollectorUnavailable,
    }
}

fn measure_disk_capacity(path: &Path) -> Result<SsdCapacity, SsdCapacityMeasurementError> {
    measure_ssd_capacity(path)
}

fn enclosure_capacity_summaries(
    disks: &[ApplianceDiskCapacityTelemetry],
) -> Vec<ApplianceEnclosureTelemetry> {
    let mut summaries = BTreeMap::<String, ApplianceEnclosureTelemetry>::new();
    for disk in disks {
        let Some(enclosure_id) = disk.enclosure_id.as_ref() else {
            continue;
        };
        let summary =
            summaries
                .entry(enclosure_id.clone())
                .or_insert_with(|| ApplianceEnclosureTelemetry {
                    enclosure_id: enclosure_id.clone(),
                    label: None,
                    disk_ids: Vec::new(),
                    total_bytes: Some(0),
                    available_bytes: Some(0),
                    used_percent: None,
                    missing_reason: None,
                });
        summary.disk_ids.push(disk.disk_id.clone());
        summary.total_bytes = add_optional_capacity(summary.total_bytes, disk.total_bytes);
        summary.available_bytes =
            add_optional_capacity(summary.available_bytes, disk.available_bytes);
        if disk.missing_reason.is_some() {
            summary.missing_reason = disk.missing_reason;
        }
    }

    let mut summaries = summaries.into_values().collect::<Vec<_>>();
    for summary in &mut summaries {
        summary.disk_ids.sort();
        summary.used_percent = match (summary.total_bytes, summary.available_bytes) {
            (Some(total), Some(available)) if total > 0 => {
                Some(percent(total.saturating_sub(available), total))
            }
            _ => None,
        };
    }
    summaries
}

fn add_optional_capacity(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.saturating_add(right)),
        _ => None,
    }
}

fn parse_load_averages(proc_loadavg: &str) -> (Option<f64>, Option<f64>, Option<f64>) {
    let mut fields = proc_loadavg.split_whitespace();
    (
        fields.next().and_then(parse_non_negative_f64),
        fields.next().and_then(parse_non_negative_f64),
        fields.next().and_then(parse_non_negative_f64),
    )
}

fn parse_non_negative_f64(value: &str) -> Option<f64> {
    value
        .parse::<f64>()
        .ok()
        .filter(|parsed| parsed.is_finite() && *parsed >= 0.0)
}

fn parse_meminfo_kib(proc_meminfo: &str) -> BTreeMap<&str, u64> {
    proc_meminfo
        .lines()
        .filter_map(|line| {
            let (key, rest) = line.split_once(':')?;
            let value = rest.split_whitespace().next()?.parse::<u64>().ok()?;
            Some((key, value))
        })
        .collect()
}

fn kib_to_bytes(value: u64) -> u64 {
    value.saturating_mul(1024)
}

fn percent(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        ((numerator as f64 / denominator as f64) * 10_000.0).round() / 100.0
    }
}
