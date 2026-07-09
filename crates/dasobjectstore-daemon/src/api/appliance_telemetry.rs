use crate::runtime::{
    ApplianceDiskCapacityTelemetry, ApplianceDiskIoTelemetry, ApplianceTelemetryMissingReason,
    ApplianceTelemetrySample, ApplianceTelemetrySampleSet, ApplianceTelemetrySource,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApplianceTelemetryRequest {
    pub window: ApplianceTelemetryWindow,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplianceTelemetryWindow {
    #[default]
    OneHour,
    OneDay,
    TenDays,
    ThreeMonths,
}

impl ApplianceTelemetryWindow {
    pub fn seconds(self) -> i64 {
        match self {
            Self::OneHour => 60 * 60,
            Self::OneDay => 24 * 60 * 60,
            Self::TenDays => 10 * 24 * 60 * 60,
            Self::ThreeMonths => 92 * 24 * 60 * 60,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplianceTelemetryState {
    Available,
    Missing,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApplianceTelemetryResponse {
    pub state: ApplianceTelemetryState,
    pub generated_at_utc: Option<String>,
    pub cadence_seconds: Option<u64>,
    pub source: Option<ApplianceTelemetrySource>,
    pub requested_window: ApplianceTelemetryWindow,
    pub available_windows: Vec<ApplianceTelemetryWindowAvailability>,
    pub current: Option<ApplianceTelemetryCurrentSummary>,
    pub series: ApplianceTelemetrySeries,
    pub missing_data_intervals: Vec<ApplianceTelemetryMissingInterval>,
}

impl ApplianceTelemetryResponse {
    pub fn missing(requested_window: ApplianceTelemetryWindow) -> Self {
        Self {
            state: ApplianceTelemetryState::Missing,
            generated_at_utc: None,
            cadence_seconds: None,
            source: None,
            requested_window,
            available_windows: Vec::new(),
            current: None,
            series: ApplianceTelemetrySeries::default(),
            missing_data_intervals: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApplianceTelemetryWindowAvailability {
    pub window: ApplianceTelemetryWindow,
    pub oldest_sample_utc: Option<String>,
    pub newest_sample_utc: Option<String>,
    pub sample_count: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApplianceTelemetryCurrentSummary {
    pub timestamp_utc: String,
    pub cpu_usage_percent_basis_points: Option<u16>,
    pub memory_used_percent_basis_points: Option<u16>,
    pub memory_total_bytes: Option<u64>,
    pub memory_available_bytes: Option<u64>,
    pub sessions: ApplianceTelemetrySessionSummary,
    pub capacity: ApplianceTelemetryCapacitySummary,
    pub disks: Vec<ApplianceTelemetryDiskCapacitySummary>,
    pub disk_io: Vec<ApplianceTelemetryDiskIoSummary>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApplianceTelemetrySessionSummary {
    pub web_active_sessions: Option<u64>,
    pub remote_agent_active_sessions: Option<u64>,
    pub distinct_logged_in_users: Option<u64>,
    pub administrator_sessions: Option<u64>,
    pub operator_sessions: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApplianceTelemetryCapacitySummary {
    pub total_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
    pub used_percent_basis_points: Option<u16>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApplianceTelemetryDiskCapacitySummary {
    pub disk_id: String,
    pub label: Option<String>,
    pub mount_path: String,
    pub role: String,
    pub enclosure_id: Option<String>,
    pub total_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
    pub used_percent_basis_points: Option<u16>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApplianceTelemetryDiskIoSummary {
    pub disk_id: String,
    pub label: Option<String>,
    pub mount_path: String,
    pub role: String,
    pub enclosure_id: Option<String>,
    pub read_bytes_per_second: Option<u64>,
    pub write_bytes_per_second: Option<u64>,
    pub read_operations_per_second: Option<u64>,
    pub write_operations_per_second: Option<u64>,
    pub average_await_micros: Option<u64>,
    pub io_time_percent_basis_points: Option<u16>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApplianceTelemetrySeries {
    pub cpu_usage: Vec<ApplianceTelemetryPercentPoint>,
    pub memory_used: Vec<ApplianceTelemetryPercentPoint>,
    pub capacity: Vec<ApplianceTelemetryCapacityPoint>,
    pub sessions: Vec<ApplianceTelemetrySessionPoint>,
    pub disk_io: Vec<ApplianceTelemetryDiskIoSeries>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApplianceTelemetryPercentPoint {
    pub timestamp_utc: String,
    pub value_basis_points: Option<u16>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApplianceTelemetryCapacityPoint {
    pub timestamp_utc: String,
    pub total_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
    pub used_percent_basis_points: Option<u16>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApplianceTelemetrySessionPoint {
    pub timestamp_utc: String,
    pub web_active_sessions: Option<u64>,
    pub remote_agent_active_sessions: Option<u64>,
    pub distinct_logged_in_users: Option<u64>,
    pub administrator_sessions: Option<u64>,
    pub operator_sessions: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApplianceTelemetryDiskIoSeries {
    pub disk_id: String,
    pub label: Option<String>,
    pub points: Vec<ApplianceTelemetryDiskIoPoint>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApplianceTelemetryDiskIoPoint {
    pub timestamp_utc: String,
    pub read_bytes_per_second: Option<u64>,
    pub write_bytes_per_second: Option<u64>,
    pub read_operations_per_second: Option<u64>,
    pub write_operations_per_second: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApplianceTelemetryMissingInterval {
    pub path: String,
    pub reason: ApplianceTelemetryMissingReason,
    pub start_utc: String,
    pub end_utc: String,
    pub sample_count: u64,
}

pub fn query_appliance_telemetry(
    sample_set: &ApplianceTelemetrySampleSet,
    request: &ApplianceTelemetryRequest,
) -> ApplianceTelemetryResponse {
    let samples = samples_in_window(&sample_set.samples, request.window);
    let current = samples.last().map(|sample| current_summary(sample));
    ApplianceTelemetryResponse {
        state: ApplianceTelemetryState::Available,
        generated_at_utc: Some(sample_set.generated_at_utc.clone()),
        cadence_seconds: Some(sample_set.cadence_seconds.round().max(0.0) as u64),
        source: Some(sample_set.source.clone()),
        requested_window: request.window,
        available_windows: available_windows(&sample_set.samples),
        current,
        series: series_from_samples(&samples),
        missing_data_intervals: missing_intervals(&samples),
    }
}

fn samples_in_window(
    samples: &[ApplianceTelemetrySample],
    window: ApplianceTelemetryWindow,
) -> Vec<ApplianceTelemetrySample> {
    let newest = samples
        .iter()
        .filter_map(|sample| parse_utc_timestamp_seconds(&sample.timestamp_utc))
        .max();
    let Some(newest) = newest else {
        return Vec::new();
    };
    let oldest_allowed = newest.saturating_sub(window.seconds());
    let mut filtered = samples
        .iter()
        .filter_map(|sample| {
            let timestamp = parse_utc_timestamp_seconds(&sample.timestamp_utc)?;
            (timestamp >= oldest_allowed).then(|| sample.clone())
        })
        .collect::<Vec<_>>();
    filtered.sort_by(|left, right| left.timestamp_utc.cmp(&right.timestamp_utc));
    filtered
}

fn available_windows(
    samples: &[ApplianceTelemetrySample],
) -> Vec<ApplianceTelemetryWindowAvailability> {
    [
        ApplianceTelemetryWindow::OneHour,
        ApplianceTelemetryWindow::OneDay,
        ApplianceTelemetryWindow::TenDays,
        ApplianceTelemetryWindow::ThreeMonths,
    ]
    .into_iter()
    .map(|window| {
        let samples = samples_in_window(samples, window);
        ApplianceTelemetryWindowAvailability {
            window,
            oldest_sample_utc: samples.first().map(|sample| sample.timestamp_utc.clone()),
            newest_sample_utc: samples.last().map(|sample| sample.timestamp_utc.clone()),
            sample_count: samples.len() as u64,
        }
    })
    .collect()
}

fn current_summary(sample: &ApplianceTelemetrySample) -> ApplianceTelemetryCurrentSummary {
    ApplianceTelemetryCurrentSummary {
        timestamp_utc: sample.timestamp_utc.clone(),
        cpu_usage_percent_basis_points: percent_basis_points(sample.cpu.usage_percent),
        memory_used_percent_basis_points: percent_basis_points(sample.memory.used_percent),
        memory_total_bytes: sample.memory.total_bytes,
        memory_available_bytes: sample.memory.available_bytes,
        sessions: ApplianceTelemetrySessionSummary {
            web_active_sessions: sample.sessions.web_active_sessions,
            remote_agent_active_sessions: sample.sessions.remote_agent_active_sessions,
            distinct_logged_in_users: sample.sessions.distinct_logged_in_users,
            administrator_sessions: sample.sessions.administrator_sessions,
            operator_sessions: sample.sessions.operator_sessions,
        },
        capacity: capacity_summary(&sample.disks),
        disks: sample.disks.iter().map(disk_capacity_summary).collect(),
        disk_io: sample.disk_io.iter().map(disk_io_summary).collect(),
    }
}

fn capacity_summary(disks: &[ApplianceDiskCapacityTelemetry]) -> ApplianceTelemetryCapacitySummary {
    let total = sum_present(disks.iter().map(|disk| disk.total_bytes));
    let available = sum_present(disks.iter().map(|disk| disk.available_bytes));
    ApplianceTelemetryCapacitySummary {
        total_bytes: total,
        available_bytes: available,
        used_percent_basis_points: capacity_used_basis_points(total, available),
    }
}

fn disk_capacity_summary(
    disk: &ApplianceDiskCapacityTelemetry,
) -> ApplianceTelemetryDiskCapacitySummary {
    ApplianceTelemetryDiskCapacitySummary {
        disk_id: disk.disk_id.clone(),
        label: disk.label.clone(),
        mount_path: disk.mount_path.clone(),
        role: disk.role.clone(),
        enclosure_id: disk.enclosure_id.clone(),
        total_bytes: disk.total_bytes,
        available_bytes: disk.available_bytes,
        used_percent_basis_points: percent_basis_points(disk.used_percent),
    }
}

fn disk_io_summary(disk_io: &ApplianceDiskIoTelemetry) -> ApplianceTelemetryDiskIoSummary {
    ApplianceTelemetryDiskIoSummary {
        disk_id: disk_io.disk_id.clone(),
        label: disk_io.label.clone(),
        mount_path: disk_io.mount_path.clone(),
        role: disk_io.role.clone(),
        enclosure_id: disk_io.enclosure_id.clone(),
        read_bytes_per_second: rounded_u64(disk_io.read_bytes_per_second),
        write_bytes_per_second: rounded_u64(disk_io.write_bytes_per_second),
        read_operations_per_second: rounded_u64(disk_io.read_operations_per_second),
        write_operations_per_second: rounded_u64(disk_io.write_operations_per_second),
        average_await_micros: disk_io
            .average_await_millis
            .and_then(|value| rounded_u64(Some(value * 1_000.0))),
        io_time_percent_basis_points: percent_basis_points(disk_io.io_time_percent),
    }
}

fn series_from_samples(samples: &[ApplianceTelemetrySample]) -> ApplianceTelemetrySeries {
    let mut disk_io_points: BTreeMap<String, ApplianceTelemetryDiskIoSeries> = BTreeMap::new();
    for sample in samples {
        for disk_io in &sample.disk_io {
            let series = disk_io_points
                .entry(disk_io.disk_id.clone())
                .or_insert_with(|| ApplianceTelemetryDiskIoSeries {
                    disk_id: disk_io.disk_id.clone(),
                    label: disk_io.label.clone(),
                    points: Vec::new(),
                });
            series.points.push(ApplianceTelemetryDiskIoPoint {
                timestamp_utc: sample.timestamp_utc.clone(),
                read_bytes_per_second: rounded_u64(disk_io.read_bytes_per_second),
                write_bytes_per_second: rounded_u64(disk_io.write_bytes_per_second),
                read_operations_per_second: rounded_u64(disk_io.read_operations_per_second),
                write_operations_per_second: rounded_u64(disk_io.write_operations_per_second),
            });
        }
    }

    ApplianceTelemetrySeries {
        cpu_usage: samples
            .iter()
            .map(|sample| ApplianceTelemetryPercentPoint {
                timestamp_utc: sample.timestamp_utc.clone(),
                value_basis_points: percent_basis_points(sample.cpu.usage_percent),
            })
            .collect(),
        memory_used: samples
            .iter()
            .map(|sample| ApplianceTelemetryPercentPoint {
                timestamp_utc: sample.timestamp_utc.clone(),
                value_basis_points: percent_basis_points(sample.memory.used_percent),
            })
            .collect(),
        capacity: samples
            .iter()
            .map(|sample| {
                let capacity = capacity_summary(&sample.disks);
                ApplianceTelemetryCapacityPoint {
                    timestamp_utc: sample.timestamp_utc.clone(),
                    total_bytes: capacity.total_bytes,
                    available_bytes: capacity.available_bytes,
                    used_percent_basis_points: capacity.used_percent_basis_points,
                }
            })
            .collect(),
        sessions: samples
            .iter()
            .map(|sample| ApplianceTelemetrySessionPoint {
                timestamp_utc: sample.timestamp_utc.clone(),
                web_active_sessions: sample.sessions.web_active_sessions,
                remote_agent_active_sessions: sample.sessions.remote_agent_active_sessions,
                distinct_logged_in_users: sample.sessions.distinct_logged_in_users,
                administrator_sessions: sample.sessions.administrator_sessions,
                operator_sessions: sample.sessions.operator_sessions,
            })
            .collect(),
        disk_io: disk_io_points.into_values().collect(),
    }
}

fn missing_intervals(
    samples: &[ApplianceTelemetrySample],
) -> Vec<ApplianceTelemetryMissingInterval> {
    let mut intervals = Vec::new();
    let mut open = Vec::<ApplianceTelemetryMissingInterval>::new();

    for sample in samples {
        let current = sample
            .missing_data
            .iter()
            .map(|marker| (marker.path.clone(), marker.reason))
            .collect::<Vec<_>>();
        let mut index = 0;
        while index < open.len() {
            let is_still_missing = current
                .iter()
                .any(|(path, reason)| open[index].path == *path && open[index].reason == *reason);
            if is_still_missing {
                index += 1;
            } else {
                intervals.push(open.remove(index));
            }
        }

        for (path, reason) in current {
            if let Some(interval) = open
                .iter_mut()
                .find(|interval| interval.path == path && interval.reason == reason)
            {
                interval.end_utc = sample.timestamp_utc.clone();
                interval.sample_count = interval.sample_count.saturating_add(1);
            } else {
                open.push(ApplianceTelemetryMissingInterval {
                    path,
                    reason,
                    start_utc: sample.timestamp_utc.clone(),
                    end_utc: sample.timestamp_utc.clone(),
                    sample_count: 1,
                });
            }
        }
    }
    intervals.extend(open);
    intervals.sort_by(|left, right| {
        left.start_utc
            .cmp(&right.start_utc)
            .then_with(|| left.path.cmp(&right.path))
    });
    intervals
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

fn capacity_used_basis_points(total: Option<u64>, available: Option<u64>) -> Option<u16> {
    let total = total?;
    if total == 0 {
        return None;
    }
    let available = available.unwrap_or(0).min(total);
    Some((((total - available) as u128 * 10_000) / total as u128).min(10_000) as u16)
}

fn percent_basis_points(value: Option<f64>) -> Option<u16> {
    let value = value?;
    if !value.is_finite() {
        return None;
    }
    Some((value.clamp(0.0, 100.0) * 100.0).round() as u16)
}

fn rounded_u64(value: Option<f64>) -> Option<u64> {
    let value = value?;
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    Some(value.round() as u64)
}

fn parse_utc_timestamp_seconds(value: &str) -> Option<i64> {
    let value = value.strip_suffix('Z')?;
    let (date, time) = value.split_once('T')?;
    let mut date_parts = date.split('-');
    let year = date_parts.next()?.parse::<i32>().ok()?;
    let month = date_parts.next()?.parse::<u32>().ok()?;
    let day = date_parts.next()?.parse::<u32>().ok()?;
    if date_parts.next().is_some() {
        return None;
    }
    let time = time.split_once('.').map_or(time, |(whole, _)| whole);
    let mut time_parts = time.split(':');
    let hour = time_parts.next()?.parse::<u32>().ok()?;
    let minute = time_parts.next()?.parse::<u32>().ok()?;
    let second = time_parts.next()?.parse::<u32>().ok()?;
    if time_parts.next().is_some()
        || hour > 23
        || minute > 59
        || second > 59
        || !(1..=12).contains(&month)
    {
        return None;
    }
    let month_day_count = days_in_month(year, month);
    if day == 0 || day > month_day_count {
        return None;
    }
    let mut days = 0i64;
    for current_year in 1970..year {
        days += if is_leap_year(current_year) { 366 } else { 365 };
    }
    for current_month in 1..month {
        days += i64::from(days_in_month(year, current_month));
    }
    days += i64::from(day - 1);
    Some(days * 86_400 + i64::from(hour * 3_600 + minute * 60 + second))
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

#[cfg(test)]
mod tests {
    use super::{query_appliance_telemetry, ApplianceTelemetryRequest, ApplianceTelemetryWindow};
    use crate::runtime::{
        ApplianceCpuTelemetry, ApplianceDiskCapacityTelemetry, ApplianceDiskIoTelemetry,
        ApplianceMemoryTelemetry, ApplianceSessionTelemetry, ApplianceTelemetryCollectionQuality,
        ApplianceTelemetryMissingDataMarker, ApplianceTelemetryMissingReason,
        ApplianceTelemetrySample, ApplianceTelemetrySampleSet, ApplianceTelemetrySource,
    };

    #[test]
    fn query_returns_current_summary_series_and_missing_intervals_for_window() {
        let sample_set = ApplianceTelemetrySampleSet {
            schema_version: "dasobjectstore.appliance_telemetry.v1".to_string(),
            generated_at_utc: "2026-07-09T18:30:00Z".to_string(),
            cadence_seconds: 30.0,
            source: source(),
            samples: vec![
                sample("2026-07-09T18:00:00Z", None, Some(1), true),
                sample("2026-07-09T18:29:30Z", Some(40.25), Some(2), false),
                sample("2026-07-09T18:30:00Z", Some(41.0), Some(3), false),
            ],
        };

        let response = query_appliance_telemetry(
            &sample_set,
            &ApplianceTelemetryRequest {
                window: ApplianceTelemetryWindow::OneHour,
            },
        );

        let current = response.current.expect("current summary");
        assert_eq!(current.timestamp_utc, "2026-07-09T18:30:00Z");
        assert_eq!(current.cpu_usage_percent_basis_points, Some(4_100));
        assert_eq!(current.memory_used_percent_basis_points, Some(2_500));
        assert_eq!(current.sessions.web_active_sessions, Some(3));
        assert_eq!(current.capacity.total_bytes, Some(1_000));
        assert_eq!(current.capacity.available_bytes, Some(700));
        assert_eq!(current.capacity.used_percent_basis_points, Some(3_000));
        assert_eq!(current.disk_io[0].read_bytes_per_second, Some(512));

        assert_eq!(response.series.cpu_usage.len(), 3);
        assert_eq!(response.series.disk_io[0].points.len(), 3);
        assert_eq!(
            response.available_windows[0].window,
            ApplianceTelemetryWindow::OneHour
        );
        assert_eq!(response.available_windows[0].sample_count, 3);
        assert_eq!(response.available_windows[1].sample_count, 3);
        assert!(response
            .missing_data_intervals
            .iter()
            .any(|interval| interval.path == "cpu.usage_percent"
                && interval.reason == ApplianceTelemetryMissingReason::DaemonStartup));
    }

    fn source() -> ApplianceTelemetrySource {
        ApplianceTelemetrySource {
            appliance_id: "fixture-appliance".to_string(),
            host_id: "fixture-host".to_string(),
            hostname: Some("fixture-hostname".to_string()),
        }
    }

    fn sample(
        timestamp_utc: &str,
        cpu_usage_percent: Option<f64>,
        web_sessions: Option<u64>,
        missing_cpu: bool,
    ) -> ApplianceTelemetrySample {
        let missing_data = if missing_cpu {
            vec![ApplianceTelemetryMissingDataMarker {
                path: "cpu.usage_percent".to_string(),
                reason: ApplianceTelemetryMissingReason::DaemonStartup,
                detail: None,
            }]
        } else {
            Vec::new()
        };
        ApplianceTelemetrySample {
            timestamp_utc: timestamp_utc.to_string(),
            collection_quality: if missing_data.is_empty() {
                ApplianceTelemetryCollectionQuality::Complete
            } else {
                ApplianceTelemetryCollectionQuality::Partial
            },
            missing_data,
            cpu: ApplianceCpuTelemetry {
                usage_percent: cpu_usage_percent,
                load_average_1m: Some(0.1),
                load_average_5m: Some(0.2),
                load_average_15m: Some(0.3),
                logical_core_count: Some(2),
                missing_reason: None,
            },
            memory: ApplianceMemoryTelemetry {
                total_bytes: Some(100),
                available_bytes: Some(75),
                used_percent: Some(25.0),
                swap_total_bytes: Some(0),
                swap_used_bytes: Some(0),
                missing_reason: None,
            },
            enclosures: Vec::new(),
            disks: vec![ApplianceDiskCapacityTelemetry {
                disk_id: "qnap-a".to_string(),
                label: Some("QNAP bay 1".to_string()),
                mount_path: "/srv/dasobjectstore/hdd/qnap-a".to_string(),
                role: "hdd".to_string(),
                enclosure_id: Some("qnap-tl-d800c-01".to_string()),
                device_path: Some("/dev/disk/by-id/fixture-a".to_string()),
                filesystem: Some("ext4".to_string()),
                total_bytes: Some(1_000),
                available_bytes: Some(700),
                used_percent: Some(30.0),
                missing_reason: None,
            }],
            disk_io: vec![ApplianceDiskIoTelemetry {
                disk_id: "qnap-a".to_string(),
                label: Some("QNAP bay 1".to_string()),
                mount_path: "/srv/dasobjectstore/hdd/qnap-a".to_string(),
                role: "hdd".to_string(),
                enclosure_id: Some("qnap-tl-d800c-01".to_string()),
                device_path: Some("/dev/disk/by-id/fixture-a".to_string()),
                device_name: Some("sda".to_string()),
                read_bytes_per_second: Some(512.0),
                write_bytes_per_second: Some(256.0),
                read_operations_per_second: Some(2.0),
                write_operations_per_second: Some(1.0),
                average_await_millis: Some(2.5),
                io_time_percent: Some(10.0),
                missing_reason: None,
            }],
            sessions: ApplianceSessionTelemetry {
                web_active_sessions: web_sessions,
                remote_agent_active_sessions: Some(1),
                distinct_logged_in_users: web_sessions,
                administrator_sessions: Some(1),
                operator_sessions: Some(1),
                missing_reason: None,
            },
        }
    }
}
