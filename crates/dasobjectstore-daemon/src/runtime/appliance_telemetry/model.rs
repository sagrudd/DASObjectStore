use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

pub const APPLIANCE_TELEMETRY_SCHEMA_VERSION: &str = "dasobjectstore.appliance_telemetry.v1";
pub const APPLIANCE_TELEMETRY_DIR_NAME: &str = "telemetry";
pub const APPLIANCE_TELEMETRY_FILE_NAME: &str = "appliance-telemetry.v1.json";
pub const APPLIANCE_TELEMETRY_FAST_CADENCE_SECONDS: u64 = 6;
pub const APPLIANCE_TELEMETRY_NORMAL_CADENCE_SECONDS: u64 = 30;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApplianceCpuTelemetry {
    pub usage_percent: Option<f64>,
    pub load_average_1m: Option<f64>,
    pub load_average_5m: Option<f64>,
    pub load_average_15m: Option<f64>,
    pub logical_core_count: Option<u64>,
    pub missing_reason: Option<ApplianceTelemetryMissingReason>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApplianceMemoryTelemetry {
    pub total_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
    pub used_percent: Option<f64>,
    pub swap_total_bytes: Option<u64>,
    pub swap_used_bytes: Option<u64>,
    pub missing_reason: Option<ApplianceTelemetryMissingReason>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LinuxCpuSnapshot {
    pub total_jiffies: u64,
    pub idle_jiffies: u64,
    pub logical_core_count: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LinuxHostTelemetrySample {
    pub cpu: ApplianceCpuTelemetry,
    pub memory: ApplianceMemoryTelemetry,
    pub enclosures: Vec<ApplianceEnclosureTelemetry>,
    pub disks: Vec<ApplianceDiskCapacityTelemetry>,
    pub disk_io: Vec<ApplianceDiskIoTelemetry>,
    pub sessions: ApplianceSessionTelemetry,
    pub cpu_snapshot: LinuxCpuSnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinuxDiskIoCounters {
    pub device_name: String,
    pub read_operations: u64,
    pub write_operations: u64,
    pub sectors_read: u64,
    pub sectors_written: u64,
    pub read_time_millis: u64,
    pub write_time_millis: u64,
    pub io_time_millis: u64,
    pub weighted_io_time_millis: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApplianceTelemetrySampleSet {
    pub schema_version: String,
    pub generated_at_utc: String,
    pub cadence_seconds: f64,
    pub source: ApplianceTelemetrySource,
    pub samples: Vec<ApplianceTelemetrySample>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApplianceTelemetrySource {
    pub appliance_id: String,
    pub host_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApplianceTelemetrySample {
    pub timestamp_utc: String,
    pub collection_quality: ApplianceTelemetryCollectionQuality,
    pub missing_data: Vec<ApplianceTelemetryMissingDataMarker>,
    pub cpu: ApplianceCpuTelemetry,
    pub memory: ApplianceMemoryTelemetry,
    pub enclosures: Vec<ApplianceEnclosureTelemetry>,
    pub disks: Vec<ApplianceDiskCapacityTelemetry>,
    pub disk_io: Vec<ApplianceDiskIoTelemetry>,
    pub sessions: ApplianceSessionTelemetry,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApplianceEnclosureTelemetry {
    pub enclosure_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub disk_ids: Vec<String>,
    pub total_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
    pub used_percent: Option<f64>,
    pub missing_reason: Option<ApplianceTelemetryMissingReason>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApplianceDiskCapacityTelemetry {
    pub disk_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub mount_path: String,
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enclosure_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bay_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filesystem: Option<String>,
    pub total_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
    pub used_percent: Option<f64>,
    pub missing_reason: Option<ApplianceTelemetryMissingReason>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApplianceDiskIoTelemetry {
    pub disk_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub mount_path: String,
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enclosure_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bay_label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,
    pub read_bytes_per_second: Option<f64>,
    pub write_bytes_per_second: Option<f64>,
    pub read_operations_per_second: Option<f64>,
    pub write_operations_per_second: Option<f64>,
    pub average_await_millis: Option<f64>,
    pub io_time_percent: Option<f64>,
    pub missing_reason: Option<ApplianceTelemetryMissingReason>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplianceTelemetryCollectionQuality {
    Complete,
    Partial,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApplianceTelemetryMissingDataMarker {
    pub path: String,
    pub reason: ApplianceTelemetryMissingReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplianceTelemetryMissingReason {
    CollectorUnavailable,
    PermissionDenied,
    UnsupportedPlatform,
    DeviceMissing,
    CounterReset,
    DaemonStartup,
    FirstSampleWarmup,
    SampleTimeout,
    NotConfigured,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApplianceSessionTelemetry {
    pub web_active_sessions: Option<u64>,
    pub remote_agent_active_sessions: Option<u64>,
    pub distinct_logged_in_users: Option<u64>,
    pub administrator_sessions: Option<u64>,
    pub operator_sessions: Option<u64>,
    pub missing_reason: Option<ApplianceTelemetryMissingReason>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApplianceTelemetryCollectorError {
    Io { path: PathBuf, message: String },
    InvalidDeviceMarker { path: PathBuf, message: String },
    InvalidProcDiskstats(String),
    InvalidProcStat(String),
}

impl fmt::Display for ApplianceTelemetryCollectorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, message } => write!(
                formatter,
                "read Linux telemetry path {}: {message}",
                path.display()
            ),
            Self::InvalidDeviceMarker { path, message } => write!(
                formatter,
                "invalid DASObjectStore device marker {}: {message}",
                path.display()
            ),
            Self::InvalidProcDiskstats(message) => {
                write!(formatter, "invalid /proc/diskstats: {message}")
            }
            Self::InvalidProcStat(message) => write!(formatter, "invalid /proc/stat: {message}"),
        }
    }
}

impl std::error::Error for ApplianceTelemetryCollectorError {}
