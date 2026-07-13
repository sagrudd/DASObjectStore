use super::model::{
    ApplianceCpuTelemetry, ApplianceDiskCapacityTelemetry, ApplianceDiskIoTelemetry,
    ApplianceEnclosureTelemetry, ApplianceMemoryTelemetry, ApplianceTelemetryCollectorError,
    ApplianceTelemetryMissingReason, LinuxCpuSnapshot, LinuxDiskIoCounters,
    LinuxHostTelemetrySample,
};
use super::service_loop::ApplianceHostTelemetryCollector;
use super::sessions::{
    collect_appliance_session_telemetry, DEFAULT_LOCAL_GROUP_PATH,
    DEFAULT_REMOTE_EASYCONNECT_SESSION_PATH, DEFAULT_STANDALONE_AUTH_ROOT,
};
use dasobjectstore_metadata::{measure_ssd_capacity, SsdCapacity, SsdCapacityMeasurementError};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const DEFAULT_APPLIANCE_TELEMETRY_HDD_ROOT: &str = "/srv/dasobjectstore/hdd";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinuxProcTelemetryCollector {
    proc_root: PathBuf,
    hdd_root: Option<PathBuf>,
    web_auth_root: Option<PathBuf>,
    remote_session_path: Option<PathBuf>,
    local_group_path: Option<PathBuf>,
    sys_root: Option<PathBuf>,
    previous_diskstats: Option<BTreeMap<String, LinuxDiskIoCounters>>,
}

impl LinuxProcTelemetryCollector {
    pub fn new(proc_root: impl Into<PathBuf>) -> Self {
        Self {
            proc_root: proc_root.into(),
            hdd_root: None,
            web_auth_root: None,
            remote_session_path: None,
            local_group_path: None,
            sys_root: None,
            previous_diskstats: None,
        }
    }

    pub fn with_hdd_root(mut self, hdd_root: impl Into<PathBuf>) -> Self {
        self.hdd_root = Some(hdd_root.into());
        self
    }

    pub fn with_session_sources(
        mut self,
        web_auth_root: impl Into<PathBuf>,
        remote_session_path: impl Into<PathBuf>,
        local_group_path: impl Into<PathBuf>,
    ) -> Self {
        self.web_auth_root = Some(web_auth_root.into());
        self.remote_session_path = Some(remote_session_path.into());
        self.local_group_path = Some(local_group_path.into());
        self
    }

    pub fn with_sys_root(mut self, sys_root: impl Into<PathBuf>) -> Self {
        self.sys_root = Some(sys_root.into());
        self
    }

    pub fn proc_root(&self) -> &Path {
        &self.proc_root
    }

    pub fn hdd_root(&self) -> Option<&Path> {
        self.hdd_root.as_deref()
    }

    pub fn web_auth_root(&self) -> Option<&Path> {
        self.web_auth_root.as_deref()
    }

    pub fn remote_session_path(&self) -> Option<&Path> {
        self.remote_session_path.as_deref()
    }

    pub fn collect(
        &mut self,
        previous_cpu: Option<&LinuxCpuSnapshot>,
        elapsed_seconds: u64,
        timestamp_utc: &str,
    ) -> Result<LinuxHostTelemetrySample, ApplianceTelemetryCollectorError> {
        let proc_stat = self.read_proc_file("stat")?;
        let proc_loadavg = self.read_proc_file("loadavg")?;
        let proc_meminfo = self.read_proc_file("meminfo")?;
        let cpu_snapshot = parse_linux_cpu_snapshot(&proc_stat)?;
        let (enclosures, disks, disk_io) = match self.hdd_root.as_deref() {
            Some(hdd_root) => {
                let (enclosures, disks) = collect_linux_disk_capacity_telemetry(hdd_root)?;
                let proc_diskstats = self.read_proc_file("diskstats")?;
                let current_diskstats = parse_linux_diskstats(&proc_diskstats)?;
                let disk_io = collect_linux_disk_io_telemetry_with_sys_root(
                    hdd_root,
                    &current_diskstats,
                    self.previous_diskstats.as_ref(),
                    elapsed_seconds,
                    self.sys_root.as_deref(),
                )?;
                self.previous_diskstats = Some(current_diskstats);
                (enclosures, disks, disk_io)
            }
            None => (Vec::new(), Vec::new(), Vec::new()),
        };

        Ok(LinuxHostTelemetrySample {
            cpu: collect_linux_cpu_telemetry(previous_cpu, &cpu_snapshot, &proc_loadavg),
            memory: collect_linux_memory_telemetry(&proc_meminfo),
            enclosures,
            disks,
            disk_io,
            sessions: collect_appliance_session_telemetry(
                self.web_auth_root.as_deref(),
                self.remote_session_path.as_deref(),
                self.local_group_path.as_deref(),
                timestamp_utc,
                unix_now_seconds(),
            ),
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
        Self::new("/proc")
            .with_hdd_root(DEFAULT_APPLIANCE_TELEMETRY_HDD_ROOT)
            .with_session_sources(
                DEFAULT_STANDALONE_AUTH_ROOT,
                DEFAULT_REMOTE_EASYCONNECT_SESSION_PATH,
                DEFAULT_LOCAL_GROUP_PATH,
            )
    }
}

impl ApplianceHostTelemetryCollector for LinuxProcTelemetryCollector {
    fn collect(
        &mut self,
        previous_cpu: Option<&LinuxCpuSnapshot>,
        elapsed_seconds: u64,
        timestamp_utc: &str,
    ) -> Result<LinuxHostTelemetrySample, ApplianceTelemetryCollectorError> {
        LinuxProcTelemetryCollector::collect(self, previous_cpu, elapsed_seconds, timestamp_utc)
    }
}

fn unix_now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
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
        let Some(marker) = parse_managed_hdd_marker(&mount_path, &marker_path, &marker)? else {
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
            bay_label: marker.bay_label,
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

pub fn parse_linux_diskstats(
    proc_diskstats: &str,
) -> Result<BTreeMap<String, LinuxDiskIoCounters>, ApplianceTelemetryCollectorError> {
    let mut counters = BTreeMap::new();
    for line in proc_diskstats
        .lines()
        .filter(|line| !line.trim().is_empty())
    {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 14 {
            return Err(ApplianceTelemetryCollectorError::InvalidProcDiskstats(
                "diskstats line has fewer than 14 fields".to_string(),
            ));
        }
        let device_name = fields[2].to_string();
        counters.insert(
            device_name.clone(),
            LinuxDiskIoCounters {
                device_name,
                read_operations: parse_diskstats_u64(fields[3], "reads completed")?,
                write_operations: parse_diskstats_u64(fields[7], "writes completed")?,
                sectors_read: parse_diskstats_u64(fields[5], "sectors read")?,
                sectors_written: parse_diskstats_u64(fields[9], "sectors written")?,
                read_time_millis: parse_diskstats_u64(fields[6], "read time ms")?,
                write_time_millis: parse_diskstats_u64(fields[10], "write time ms")?,
                io_time_millis: parse_diskstats_u64(fields[12], "io time ms")?,
                weighted_io_time_millis: parse_diskstats_u64(fields[13], "weighted io time ms")?,
            },
        );
    }
    Ok(counters)
}

pub fn collect_linux_disk_io_telemetry(
    hdd_root: impl AsRef<Path>,
    current_diskstats: &BTreeMap<String, LinuxDiskIoCounters>,
    previous_diskstats: Option<&BTreeMap<String, LinuxDiskIoCounters>>,
    elapsed_seconds: u64,
) -> Result<Vec<ApplianceDiskIoTelemetry>, ApplianceTelemetryCollectorError> {
    collect_linux_disk_io_telemetry_with_sys_root(
        hdd_root,
        current_diskstats,
        previous_diskstats,
        elapsed_seconds,
        None,
    )
}

fn collect_linux_disk_io_telemetry_with_sys_root(
    hdd_root: impl AsRef<Path>,
    current_diskstats: &BTreeMap<String, LinuxDiskIoCounters>,
    previous_diskstats: Option<&BTreeMap<String, LinuxDiskIoCounters>>,
    elapsed_seconds: u64,
    sys_root: Option<&Path>,
) -> Result<Vec<ApplianceDiskIoTelemetry>, ApplianceTelemetryCollectorError> {
    let markers = managed_hdd_markers(hdd_root.as_ref())?;
    let mut telemetry = Vec::new();

    for marker in markers {
        let device_name = resolve_diskstats_device_name(&marker, current_diskstats, sys_root);
        let current = device_name
            .as_ref()
            .and_then(|name| current_diskstats.get(name));
        let previous = match (device_name.as_ref(), previous_diskstats) {
            (Some(name), Some(previous_diskstats)) => previous_diskstats.get(name),
            _ => None,
        };
        let (
            read_bytes_per_second,
            write_bytes_per_second,
            read_operations_per_second,
            write_operations_per_second,
            average_await_millis,
            io_time_percent,
            missing_reason,
        ) = disk_io_rates(current, previous, elapsed_seconds);

        telemetry.push(ApplianceDiskIoTelemetry {
            disk_id: marker.disk_id,
            label: marker.label,
            mount_path: marker.mount_path.to_string_lossy().to_string(),
            role: "hdd".to_string(),
            enclosure_id: marker.enclosure_id,
            bay_label: marker.bay_label,
            device_path: marker.device_path,
            device_name,
            read_bytes_per_second,
            write_bytes_per_second,
            read_operations_per_second,
            write_operations_per_second,
            average_await_millis,
            io_time_percent,
            missing_reason,
        });
    }

    telemetry.sort_by(|left, right| left.disk_id.cmp(&right.disk_id));
    Ok(telemetry)
}

#[derive(Debug)]
struct ManagedHddMarker {
    disk_id: String,
    label: Option<String>,
    enclosure_id: Option<String>,
    bay_label: Option<String>,
    device_path: Option<String>,
    filesystem: Option<String>,
    diskstats_device_name: Option<String>,
    mount_path: PathBuf,
}

fn parse_managed_hdd_marker(
    mount_path: &Path,
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

    let diskstats_device_name =
        optional_marker_value(&values, "diskstats_device").or_else(|| marker_device_name(&values));
    if let Some(name) = diskstats_device_name.as_deref() {
        if name.is_empty()
            || name.contains('/')
            || name.contains('\\')
            || name == "."
            || name == ".."
        {
            return Err(ApplianceTelemetryCollectorError::InvalidDeviceMarker {
                path: marker_path.to_path_buf(),
                message: "diskstats_device must be a basename without path separators".to_string(),
            });
        }
    }

    Ok(Some(ManagedHddMarker {
        disk_id: disk_id.to_string(),
        label: optional_marker_value(&values, "label").or_else(|| Some(disk_id.to_string())),
        enclosure_id: optional_marker_value(&values, "enclosure_id"),
        bay_label: optional_marker_value(&values, "bay_label"),
        diskstats_device_name,
        device_path: optional_marker_value(&values, "device"),
        filesystem: optional_marker_value(&values, "filesystem"),
        mount_path: mount_path.to_path_buf(),
    }))
}

fn managed_hdd_markers(
    hdd_root: &Path,
) -> Result<Vec<ManagedHddMarker>, ApplianceTelemetryCollectorError> {
    let entries = match fs::read_dir(hdd_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(ApplianceTelemetryCollectorError::Io {
                path: hdd_root.to_path_buf(),
                message: error.to_string(),
            });
        }
    };
    let mut markers = Vec::new();
    let mut disk_ids = std::collections::BTreeSet::new();
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
        if let Some(marker) = parse_managed_hdd_marker(&mount_path, &marker_path, &marker)? {
            if !disk_ids.insert(marker.disk_id.clone()) {
                return Err(ApplianceTelemetryCollectorError::InvalidDeviceMarker {
                    path: marker_path,
                    message: format!("duplicate managed HDD disk id: {}", marker.disk_id),
                });
            }
            markers.push(marker);
        }
    }
    markers.sort_by(|left, right| left.disk_id.cmp(&right.disk_id));
    Ok(markers)
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

fn marker_device_name(values: &BTreeMap<&str, &str>) -> Option<String> {
    optional_marker_value(values, "device")
        .and_then(|device| Path::new(&device).file_name().map(|name| name.to_owned()))
        .map(|name| name.to_string_lossy().to_string())
}

fn resolve_diskstats_device_name(
    marker: &ManagedHddMarker,
    current_diskstats: &BTreeMap<String, LinuxDiskIoCounters>,
    sys_root: Option<&Path>,
) -> Option<String> {
    let mut candidates = Vec::new();
    if let Some(name) = marker.diskstats_device_name.as_deref() {
        candidates.push(name.to_string());
    }
    if let Some(device_path) = marker.device_path.as_deref() {
        if let Some(name) = Path::new(device_path).file_name() {
            candidates.push(name.to_string_lossy().to_string());
        }
    }

    for candidate in &candidates {
        if current_diskstats.contains_key(candidate) {
            return Some(candidate.clone());
        }
    }

    let Some(sys_root) = sys_root else {
        return None;
    };
    for candidate in candidates {
        for alias_root in [
            sys_root.join("class/block"),
            sys_root.join("dev/disk/by-id"),
            sys_root.join("dev/disk/by-path"),
        ] {
            let alias = alias_root.join(&candidate);
            let Ok(target) = fs::canonicalize(&alias) else {
                continue;
            };
            let Some(name) = target.file_name() else {
                continue;
            };
            let name = name.to_string_lossy().to_string();
            if current_diskstats.contains_key(&name) {
                return Some(name);
            }
        }
    }
    None
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

fn parse_diskstats_u64(value: &str, label: &str) -> Result<u64, ApplianceTelemetryCollectorError> {
    value.parse::<u64>().map_err(|error| {
        ApplianceTelemetryCollectorError::InvalidProcDiskstats(format!(
            "{label} field {value:?} is not an integer: {error}"
        ))
    })
}

fn disk_io_rates(
    current: Option<&LinuxDiskIoCounters>,
    previous: Option<&LinuxDiskIoCounters>,
    elapsed_seconds: u64,
) -> (
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<ApplianceTelemetryMissingReason>,
) {
    let Some(current) = current else {
        return missing_disk_io_rates(ApplianceTelemetryMissingReason::DeviceMissing);
    };
    let Some(previous) = previous else {
        return missing_disk_io_rates(ApplianceTelemetryMissingReason::FirstSampleWarmup);
    };
    if elapsed_seconds == 0 || diskstats_counter_reset(current, previous) {
        return missing_disk_io_rates(ApplianceTelemetryMissingReason::CounterReset);
    }

    let elapsed = elapsed_seconds as f64;
    let read_ops_delta = current
        .read_operations
        .saturating_sub(previous.read_operations);
    let write_ops_delta = current
        .write_operations
        .saturating_sub(previous.write_operations);
    let sectors_read_delta = current.sectors_read.saturating_sub(previous.sectors_read);
    let sectors_written_delta = current
        .sectors_written
        .saturating_sub(previous.sectors_written);
    let service_time_delta = current
        .read_time_millis
        .saturating_sub(previous.read_time_millis)
        .saturating_add(
            current
                .write_time_millis
                .saturating_sub(previous.write_time_millis),
        );
    let io_time_delta = current
        .io_time_millis
        .saturating_sub(previous.io_time_millis);
    let ops_delta = read_ops_delta.saturating_add(write_ops_delta);

    (
        Some(rate(sectors_read_delta.saturating_mul(512), elapsed)),
        Some(rate(sectors_written_delta.saturating_mul(512), elapsed)),
        Some(rate(read_ops_delta, elapsed)),
        Some(rate(write_ops_delta, elapsed)),
        if ops_delta == 0 {
            None
        } else {
            Some(round_two_decimals(
                service_time_delta as f64 / ops_delta as f64,
            ))
        },
        Some(round_two_decimals(
            (io_time_delta as f64 * 100.0) / (elapsed * 1000.0),
        )),
        None,
    )
}

fn missing_disk_io_rates(
    reason: ApplianceTelemetryMissingReason,
) -> (
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<f64>,
    Option<ApplianceTelemetryMissingReason>,
) {
    (None, None, None, None, None, None, Some(reason))
}

fn diskstats_counter_reset(current: &LinuxDiskIoCounters, previous: &LinuxDiskIoCounters) -> bool {
    current.read_operations < previous.read_operations
        || current.write_operations < previous.write_operations
        || current.sectors_read < previous.sectors_read
        || current.sectors_written < previous.sectors_written
        || current.read_time_millis < previous.read_time_millis
        || current.write_time_millis < previous.write_time_millis
        || current.io_time_millis < previous.io_time_millis
}

fn rate(delta: u64, elapsed_seconds: f64) -> f64 {
    round_two_decimals(delta as f64 / elapsed_seconds)
}

fn round_two_decimals(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
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
