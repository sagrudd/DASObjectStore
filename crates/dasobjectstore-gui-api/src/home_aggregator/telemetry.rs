//! Appliance telemetry loading and dashboard-card projections.

use super::{format_tib, mib_per_second, percent_basis_points, percent_u8};
use crate::dashboard::{
    ActiveUsersSummaryView, CapacitySummaryView, CpuUsageSummaryView, DashboardWarning,
    DiskIoSummaryView, MemoryStressStateView, MemoryStressView, TelemetryCardStateView,
};
use dasobjectstore_core::utc::parse_utc_timestamp_seconds;
use dasobjectstore_daemon::{ApplianceTelemetrySample, ApplianceTelemetrySampleSet};
use std::fs;
use std::path::Path;

pub(super) fn read(path: &Path) -> Result<Option<ApplianceTelemetrySampleSet>, DashboardWarning> {
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

pub(super) fn capacity_summary(
    sample_set: &ApplianceTelemetrySampleSet,
    window: dasobjectstore_daemon::api::ApplianceTelemetryWindow,
) -> Option<CapacitySummaryView> {
    let latest = latest(sample_set, window)?;
    let total = sum_present(latest.disks.iter().map(|disk| disk.total_bytes))?;
    let available = sum_present(latest.disks.iter().map(|disk| disk.available_bytes))?.min(total);
    let used = total.saturating_sub(available);
    Some(CapacitySummaryView {
        total_tib: format_tib(total),
        used_tib: format_tib(used),
        free_tib: format_tib(available),
        used_percent_basis_points: percent_basis_points(used, total),
    })
}

pub(super) fn memory_stress(
    sample_set: &ApplianceTelemetrySampleSet,
    window: dasobjectstore_daemon::api::ApplianceTelemetryWindow,
) -> Option<MemoryStressView> {
    let latest = latest(sample_set, window)?;
    let total = latest.memory.total_bytes?;
    let available = latest.memory.available_bytes?.min(total);
    let pressure_percent = latest
        .memory
        .used_percent
        .and_then(percent_float_u8)
        .unwrap_or_else(|| percent_u8(total.saturating_sub(available), total));
    let swap_total = latest.memory.swap_total_bytes.unwrap_or(0);
    let swap_used_percent = percent_u8(
        latest.memory.swap_used_bytes.unwrap_or(0).min(swap_total),
        swap_total,
    );
    let state = memory_state(pressure_percent);
    let warning = pressure_warning(state, pressure_percent);
    Some(MemoryStressView {
        state,
        pressure_percent,
        swap_used_percent,
        page_cache_tib: "0.0".to_string(),
        warning,
    })
}

pub(super) fn disk_io_summary(
    sample_set: &ApplianceTelemetrySampleSet,
    window: dasobjectstore_daemon::api::ApplianceTelemetryWindow,
) -> Option<DiskIoSummaryView> {
    let latest = latest(sample_set, window)?;
    let (mut read_bytes, mut write_bytes, mut read_ops, mut write_ops) = (0.0, 0.0, 0.0, 0.0);
    let (mut busiest_disk, mut saw_value) = (None::<(String, f64)>, false);
    for disk_io in &latest.disk_io {
        let read = finite_nonnegative(disk_io.read_bytes_per_second).unwrap_or(0.0);
        let write = finite_nonnegative(disk_io.write_bytes_per_second).unwrap_or(0.0);
        if disk_io.read_bytes_per_second.is_some() || disk_io.write_bytes_per_second.is_some() {
            saw_value = true;
            read_bytes += read;
            write_bytes += write;
            if busiest_disk
                .as_ref()
                .is_none_or(|(_, current)| read + write > *current)
            {
                busiest_disk = Some((disk_io.disk_id.clone(), read + write));
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
        busiest_disk_id: busiest_disk.map(|(id, _)| id),
        state: TelemetryCardStateView::Nominal,
        message: None,
    })
}

pub(super) fn cpu_usage_summary(
    sample_set: &ApplianceTelemetrySampleSet,
    window: dasobjectstore_daemon::api::ApplianceTelemetryWindow,
) -> Option<CpuUsageSummaryView> {
    let latest = latest(sample_set, window)?;
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

pub(super) fn active_users_summary(
    sample_set: &ApplianceTelemetrySampleSet,
    window: dasobjectstore_daemon::api::ApplianceTelemetryWindow,
) -> Option<ActiveUsersSummaryView> {
    let sessions = &latest(sample_set, window)?.sessions;
    if sessions.web_active_sessions.is_none()
        && sessions.remote_agent_active_sessions.is_none()
        && sessions.distinct_logged_in_users.is_none()
        && sessions.administrator_sessions.is_none()
        && sessions.operator_sessions.is_none()
    {
        return None;
    }
    let remote = sessions.remote_agent_active_sessions.unwrap_or(0);
    Some(ActiveUsersSummaryView {
        available: true,
        active_sessions: sessions
            .web_active_sessions
            .unwrap_or(0)
            .saturating_add(remote),
        distinct_logged_in_users: sessions.distinct_logged_in_users.unwrap_or(0),
        administrator_sessions: sessions.administrator_sessions.unwrap_or(0),
        operator_sessions: sessions.operator_sessions.unwrap_or(0),
        remote_agent_sessions: remote,
        state: TelemetryCardStateView::Nominal,
        message: None,
    })
}

fn latest(
    set: &ApplianceTelemetrySampleSet,
    window: dasobjectstore_daemon::api::ApplianceTelemetryWindow,
) -> Option<&ApplianceTelemetrySample> {
    samples_in_window(set, window)
        .into_iter()
        .max_by_key(|(timestamp, _)| *timestamp)
        .map(|(_, sample)| sample)
}

fn samples_in_window(
    set: &ApplianceTelemetrySampleSet,
    window: dasobjectstore_daemon::api::ApplianceTelemetryWindow,
) -> Vec<(i64, &ApplianceTelemetrySample)> {
    let mut samples = set
        .samples
        .iter()
        .filter_map(|sample| {
            parse_utc_timestamp_seconds(&sample.timestamp_utc).map(|timestamp| (timestamp, sample))
        })
        .collect::<Vec<_>>();
    samples.sort_by_key(|(timestamp, _)| *timestamp);
    let Some((newest, _)) = samples.last() else {
        return Vec::new();
    };
    let oldest = newest.saturating_sub(window.seconds());
    samples
        .into_iter()
        .filter(|(timestamp, _)| *timestamp >= oldest)
        .collect()
}

fn sum_present(values: impl Iterator<Item = Option<u64>>) -> Option<u64> {
    let mut total = 0u64;
    let mut saw = false;
    for value in values.flatten() {
        saw = true;
        total = total.saturating_add(value);
    }
    saw.then_some(total)
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
fn memory_state(pressure: u8) -> MemoryStressStateView {
    match pressure {
        0..=69 => MemoryStressStateView::Nominal,
        70..=84 => MemoryStressStateView::Elevated,
        85..=94 => MemoryStressStateView::High,
        _ => MemoryStressStateView::Critical,
    }
}
fn pressure_warning(state: MemoryStressStateView, pressure: u8) -> Option<DashboardWarning> {
    matches!(
        state,
        MemoryStressStateView::Elevated
            | MemoryStressStateView::High
            | MemoryStressStateView::Critical
    )
    .then(|| {
        DashboardWarning::new(
            "memory_pressure",
            format!("Memory pressure is {pressure}% on this appliance."),
        )
    })
}
