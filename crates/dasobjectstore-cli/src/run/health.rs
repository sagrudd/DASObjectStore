//! Health command dispatch and output selection.

use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct HealthReport {
    pub(super) platform: HostPlatform,
    pub(super) disks: Vec<DiskHealthSummary>,
    pub(super) warnings: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct DiskHealthSummary {
    pub(super) device_path: Option<String>,
    pub(super) model_hint: Option<String>,
    pub(super) serial_hint: Option<String>,
    pub(super) size_bytes: Option<u64>,
    pub(super) transport: Transport,
    pub(super) smart_passed: Option<bool>,
    pub(super) signals: HealthSignals,
    pub(super) score: HealthScore,
    pub(super) warnings: Vec<String>,
}

impl DiskHealthSummary {
    fn from_observed(
        observed: &ObservedDisk,
        health_report: Result<DiskHealthReport, ProbeError>,
    ) -> Self {
        let mut warnings = Vec::new();
        let mut health = None;

        if observed.device_path.is_none() {
            warnings.push("disk has no device path; SMART health was not queried".to_string());
        }

        match health_report {
            Ok(report) => {
                warnings.extend(report.warnings.clone());
                health = Some(report);
            }
            Err(err) => warnings.push(err.to_string()),
        }

        let signals = health
            .as_ref()
            .map(|report| report.signals)
            .unwrap_or_default();
        let score = HealthScore::from_signals(&signals);

        Self {
            device_path: health
                .as_ref()
                .and_then(|report| report.device_path.clone())
                .or_else(|| observed.device_path.clone()),
            model_hint: health
                .as_ref()
                .and_then(|report| report.model_hint.clone())
                .or_else(|| observed.model_hint.clone()),
            serial_hint: health
                .as_ref()
                .and_then(|report| report.serial_hint.clone())
                .or_else(|| observed.serial_hint.clone()),
            size_bytes: observed.size_bytes,
            transport: observed.transport,
            smart_passed: health.as_ref().and_then(|report| report.smart_passed),
            signals,
            score,
            warnings,
        }
    }
}

pub(super) fn read_current_platform_health() -> Result<HealthReport, CliError> {
    let mut probe = probe_current_platform()?;
    probe.enclosures = group_enclosures(&probe.disks);

    let runner = SystemCommandRunner;
    let disks = probe
        .disks
        .iter()
        .map(|disk| {
            let health_report = disk
                .device_path
                .as_deref()
                .map(|device_path| read_disk_health_for_current_platform(&runner, device_path))
                .unwrap_or_else(|| {
                    Err(ProbeError::ParseFailed {
                        source: "health".to_string(),
                        message: "disk has no device path".to_string(),
                    })
                });
            DiskHealthSummary::from_observed(disk, health_report)
        })
        .collect();

    Ok(HealthReport {
        platform: probe.platform,
        disks,
        warnings: probe
            .warnings
            .into_iter()
            .map(|warning| format!("{}: {}", warning.code, warning.message))
            .collect(),
    })
}

#[cfg(target_os = "linux")]
fn read_disk_health_for_current_platform(
    runner: &SystemCommandRunner,
    device_path: &str,
) -> Result<DiskHealthReport, ProbeError> {
    read_smartctl_health(runner, device_path)
}

#[cfg(target_os = "macos")]
fn read_disk_health_for_current_platform(
    runner: &SystemCommandRunner,
    device_path: &str,
) -> Result<DiskHealthReport, ProbeError> {
    read_diskutil_health(runner, device_path)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn read_disk_health_for_current_platform(
    _runner: &SystemCommandRunner,
    _device_path: &str,
) -> Result<DiskHealthReport, ProbeError> {
    Err(ProbeError::UnsupportedPlatform {
        platform: std::env::consts::OS.to_string(),
    })
}

pub(super) fn run_health(args: &HealthArgs, writer: &mut impl Write) -> Result<(), CliError> {
    let selected_modes = [
        args.summary(),
        args.verbose(),
        args.connections(),
        args.json(),
    ]
    .into_iter()
    .filter(|selected| *selected)
    .count();
    if selected_modes > 1 {
        return Err(CliError::UnsupportedHealthFormat);
    }

    if args.connections() {
        let report = super::read_current_platform_connection_status()?;
        write_host_connection_status(&report, writer)?;
    } else if args.json() {
        let report = read_current_platform_health()?;
        write_health_json(&report, writer)?;
    } else if args.verbose() {
        let report = read_current_platform_health()?;
        write_health_verbose(&report, writer)?;
    } else {
        let report = read_current_platform_health()?;
        write_health_summary(&report, writer)?;
    }

    Ok(())
}
