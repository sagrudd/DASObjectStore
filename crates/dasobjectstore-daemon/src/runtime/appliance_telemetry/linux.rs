use super::model::{
    ApplianceCpuTelemetry, ApplianceMemoryTelemetry, ApplianceTelemetryCollectorError,
    ApplianceTelemetryMissingReason, LinuxCpuSnapshot, LinuxHostTelemetrySample,
};
use super::service_loop::ApplianceHostTelemetryCollector;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinuxProcTelemetryCollector {
    proc_root: PathBuf,
}

impl LinuxProcTelemetryCollector {
    pub fn new(proc_root: impl Into<PathBuf>) -> Self {
        Self {
            proc_root: proc_root.into(),
        }
    }

    pub fn proc_root(&self) -> &Path {
        &self.proc_root
    }

    pub fn collect(
        &self,
        previous_cpu: Option<&LinuxCpuSnapshot>,
    ) -> Result<LinuxHostTelemetrySample, ApplianceTelemetryCollectorError> {
        let proc_stat = self.read_proc_file("stat")?;
        let proc_loadavg = self.read_proc_file("loadavg")?;
        let proc_meminfo = self.read_proc_file("meminfo")?;
        let cpu_snapshot = parse_linux_cpu_snapshot(&proc_stat)?;

        Ok(LinuxHostTelemetrySample {
            cpu: collect_linux_cpu_telemetry(previous_cpu, &cpu_snapshot, &proc_loadavg),
            memory: collect_linux_memory_telemetry(&proc_meminfo),
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
