use super::model::{
    ApplianceTelemetryCollectionQuality, ApplianceTelemetryCollectorError,
    ApplianceTelemetryMissingDataMarker, ApplianceTelemetryMissingReason, ApplianceTelemetrySample,
    ApplianceTelemetrySampleSet, ApplianceTelemetrySource, LinuxCpuSnapshot,
    LinuxHostTelemetrySample, APPLIANCE_TELEMETRY_DIR_NAME,
    APPLIANCE_TELEMETRY_FAST_CADENCE_SECONDS, APPLIANCE_TELEMETRY_FILE_NAME,
    APPLIANCE_TELEMETRY_NORMAL_CADENCE_SECONDS, APPLIANCE_TELEMETRY_SCHEMA_VERSION,
};
use std::fmt;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

const APPLIANCE_TELEMETRY_DIR_MODE: u32 = 0o750;
const APPLIANCE_TELEMETRY_FILE_MODE: u32 = 0o640;

pub trait ApplianceHostTelemetryCollector {
    fn collect(
        &mut self,
        previous_cpu: Option<&LinuxCpuSnapshot>,
        elapsed_seconds: u64,
        timestamp_utc: &str,
    ) -> Result<LinuxHostTelemetrySample, ApplianceTelemetryCollectorError>;
}

pub trait ApplianceTelemetrySink {
    fn record(
        &mut self,
        sample_set: &ApplianceTelemetrySampleSet,
    ) -> Result<(), ApplianceTelemetryLoopError>;
}

pub trait ApplianceTelemetrySleeper {
    fn sleep(&mut self, duration: Duration);
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplianceTelemetryLoopConfig {
    pub cadence_seconds: u64,
    pub source: ApplianceTelemetrySource,
}

impl ApplianceTelemetryLoopConfig {
    pub fn new(
        cadence_seconds: u64,
        source: ApplianceTelemetrySource,
    ) -> Result<Self, ApplianceTelemetryLoopError> {
        validate_appliance_telemetry_cadence(cadence_seconds)?;
        if source.appliance_id.trim().is_empty() {
            return Err(ApplianceTelemetryLoopError::InvalidSource(
                "appliance_id must not be blank".to_string(),
            ));
        }
        if source.host_id.trim().is_empty() {
            return Err(ApplianceTelemetryLoopError::InvalidSource(
                "host_id must not be blank".to_string(),
            ));
        }
        Ok(Self {
            cadence_seconds,
            source,
        })
    }

    pub fn cadence(&self) -> Duration {
        Duration::from_secs(self.cadence_seconds)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApplianceTelemetryLoopError {
    InvalidCadenceSeconds(u64),
    InvalidSource(String),
    Collector(String),
    Sink(String),
}

impl fmt::Display for ApplianceTelemetryLoopError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCadenceSeconds(seconds) => write!(
                formatter,
                "unsupported telemetry cadence {seconds}s; supported cadences are 6s and 30s"
            ),
            Self::InvalidSource(message) => {
                write!(formatter, "invalid telemetry source: {message}")
            }
            Self::Collector(message) => write!(formatter, "collect appliance telemetry: {message}"),
            Self::Sink(message) => write!(formatter, "record appliance telemetry: {message}"),
        }
    }
}

impl std::error::Error for ApplianceTelemetryLoopError {}

pub fn validate_appliance_telemetry_cadence(
    cadence_seconds: u64,
) -> Result<(), ApplianceTelemetryLoopError> {
    match cadence_seconds {
        APPLIANCE_TELEMETRY_FAST_CADENCE_SECONDS | APPLIANCE_TELEMETRY_NORMAL_CADENCE_SECONDS => {
            Ok(())
        }
        other => Err(ApplianceTelemetryLoopError::InvalidCadenceSeconds(other)),
    }
}

#[derive(Debug)]
pub struct ApplianceTelemetryLoop<C> {
    config: ApplianceTelemetryLoopConfig,
    collector: C,
    previous_cpu: Option<LinuxCpuSnapshot>,
    samples_collected: u64,
}

impl<C> ApplianceTelemetryLoop<C>
where
    C: ApplianceHostTelemetryCollector,
{
    pub fn new(config: ApplianceTelemetryLoopConfig, collector: C) -> Self {
        Self {
            config,
            collector,
            previous_cpu: None,
            samples_collected: 0,
        }
    }

    pub fn collect_once(
        &mut self,
        timestamp_utc: impl Into<String>,
    ) -> Result<ApplianceTelemetrySampleSet, ApplianceTelemetryLoopError> {
        let timestamp_utc = timestamp_utc.into();
        let host = self
            .collector
            .collect(
                self.previous_cpu.as_ref(),
                self.config.cadence_seconds,
                &timestamp_utc,
            )
            .map_err(|error| ApplianceTelemetryLoopError::Collector(error.to_string()))?;
        self.previous_cpu = Some(host.cpu_snapshot);
        self.samples_collected = self.samples_collected.saturating_add(1);
        Ok(appliance_sample_set(
            self.config.cadence_seconds,
            self.config.source.clone(),
            timestamp_utc,
            host,
        ))
    }

    pub fn run_iterations<S, T>(
        &mut self,
        sink: &mut S,
        sleeper: &mut T,
        timestamps_utc: impl IntoIterator<Item = String>,
    ) -> Result<u64, ApplianceTelemetryLoopError>
    where
        S: ApplianceTelemetrySink,
        T: ApplianceTelemetrySleeper,
    {
        let mut written = 0u64;
        for timestamp_utc in timestamps_utc {
            let sample_set = self.collect_once(timestamp_utc)?;
            sink.record(&sample_set)?;
            written = written.saturating_add(1);
            sleeper.sleep(self.config.cadence());
        }
        Ok(written)
    }

    pub fn samples_collected(&self) -> u64 {
        self.samples_collected
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileBackedApplianceTelemetrySink {
    path: PathBuf,
}

impl FileBackedApplianceTelemetrySink {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl ApplianceTelemetrySink for FileBackedApplianceTelemetrySink {
    fn record(
        &mut self,
        sample_set: &ApplianceTelemetrySampleSet,
    ) -> Result<(), ApplianceTelemetryLoopError> {
        write_appliance_telemetry_state(&self.path, sample_set)
    }
}

#[derive(Default)]
pub struct ThreadApplianceTelemetrySleeper;

impl ApplianceTelemetrySleeper for ThreadApplianceTelemetrySleeper {
    fn sleep(&mut self, duration: Duration) {
        thread::sleep(duration);
    }
}

pub fn appliance_telemetry_state_path(state_dir: impl AsRef<Path>) -> PathBuf {
    state_dir
        .as_ref()
        .join(APPLIANCE_TELEMETRY_DIR_NAME)
        .join(APPLIANCE_TELEMETRY_FILE_NAME)
}

pub fn appliance_sample_set(
    cadence_seconds: u64,
    source: ApplianceTelemetrySource,
    timestamp_utc: String,
    host: LinuxHostTelemetrySample,
) -> ApplianceTelemetrySampleSet {
    let mut missing_data = Vec::new();
    push_optional_missing_marker(
        &mut missing_data,
        "cpu.usage_percent",
        &host.cpu.missing_reason,
    );
    push_optional_missing_marker(&mut missing_data, "memory", &host.memory.missing_reason);
    if host.disks.is_empty() {
        missing_data.push(ApplianceTelemetryMissingDataMarker {
            path: "disks.capacity".to_string(),
            reason: ApplianceTelemetryMissingReason::DeviceMissing,
            detail: Some("no managed HDD roots with DASObjectStore markers were found".to_string()),
        });
    }
    if host.enclosures.is_empty() {
        missing_data.push(ApplianceTelemetryMissingDataMarker {
            path: "enclosures".to_string(),
            reason: ApplianceTelemetryMissingReason::NotConfigured,
            detail: Some(
                "physical enclosure association is pending marker or bay-registry data".to_string(),
            ),
        });
    }
    for disk in &host.disks {
        push_optional_missing_marker(
            &mut missing_data,
            &format!("disks.{}.capacity", disk.disk_id),
            &disk.missing_reason,
        );
    }
    if host.disk_io.is_empty() {
        missing_data.push(ApplianceTelemetryMissingDataMarker {
            path: "disks.io".to_string(),
            reason: ApplianceTelemetryMissingReason::DeviceMissing,
            detail: Some("no managed HDD IO samples were collected".to_string()),
        });
    }
    for disk_io in &host.disk_io {
        push_optional_missing_marker(
            &mut missing_data,
            &format!("disks.{}.io", disk_io.disk_id),
            &disk_io.missing_reason,
        );
    }
    push_optional_missing_marker(&mut missing_data, "sessions", &host.sessions.missing_reason);

    let collection_quality = if missing_data.is_empty() {
        ApplianceTelemetryCollectionQuality::Complete
    } else {
        ApplianceTelemetryCollectionQuality::Partial
    };
    let sample = ApplianceTelemetrySample {
        timestamp_utc: timestamp_utc.clone(),
        collection_quality,
        missing_data,
        cpu: host.cpu,
        memory: host.memory,
        enclosures: host.enclosures,
        disks: host.disks,
        disk_io: host.disk_io,
        sessions: host.sessions,
    };

    ApplianceTelemetrySampleSet {
        schema_version: APPLIANCE_TELEMETRY_SCHEMA_VERSION.to_string(),
        generated_at_utc: timestamp_utc,
        cadence_seconds: cadence_seconds as f64,
        source,
        samples: vec![sample],
    }
}

fn push_optional_missing_marker(
    missing_data: &mut Vec<ApplianceTelemetryMissingDataMarker>,
    path: &str,
    reason: &Option<ApplianceTelemetryMissingReason>,
) {
    if let Some(reason) = reason {
        missing_data.push(ApplianceTelemetryMissingDataMarker {
            path: path.to_string(),
            reason: *reason,
            detail: None,
        });
    }
}

fn write_appliance_telemetry_state(
    path: &Path,
    sample_set: &ApplianceTelemetrySampleSet,
) -> Result<(), ApplianceTelemetryLoopError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            ApplianceTelemetryLoopError::Sink(format!(
                "create telemetry state directory {}: {error}",
                parent.display()
            ))
        })?;
        set_unix_permissions(
            parent,
            APPLIANCE_TELEMETRY_DIR_MODE,
            "telemetry state directory",
        )?;
    }
    let tmp_path = path.with_file_name(format!(
        ".{}.tmp-{}",
        APPLIANCE_TELEMETRY_FILE_NAME,
        std::process::id()
    ));
    {
        let mut file = File::create(&tmp_path).map_err(|error| {
            ApplianceTelemetryLoopError::Sink(format!(
                "create telemetry temp file {}: {error}",
                tmp_path.display()
            ))
        })?;
        serde_json::to_writer_pretty(&mut file, sample_set).map_err(|error| {
            ApplianceTelemetryLoopError::Sink(format!(
                "write telemetry temp file {}: {error}",
                tmp_path.display()
            ))
        })?;
        set_unix_permissions(
            &tmp_path,
            APPLIANCE_TELEMETRY_FILE_MODE,
            "telemetry temp file",
        )?;
        file.write_all(b"\n").map_err(|error| {
            ApplianceTelemetryLoopError::Sink(format!(
                "finish telemetry temp file {}: {error}",
                tmp_path.display()
            ))
        })?;
        file.sync_all().map_err(|error| {
            ApplianceTelemetryLoopError::Sink(format!(
                "sync telemetry temp file {}: {error}",
                tmp_path.display()
            ))
        })?;
    }
    fs::rename(&tmp_path, path).map_err(|error| {
        ApplianceTelemetryLoopError::Sink(format!(
            "replace telemetry state file {}: {error}",
            path.display()
        ))
    })?;
    if let Some(parent) = path.parent() {
        sync_parent_directory(parent)?;
    }
    Ok(())
}

#[cfg(unix)]
fn set_unix_permissions(
    path: &Path,
    mode: u32,
    label: &str,
) -> Result<(), ApplianceTelemetryLoopError> {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(|error| {
        ApplianceTelemetryLoopError::Sink(format!(
            "set {label} permissions {}: {error}",
            path.display()
        ))
    })
}

#[cfg(not(unix))]
fn set_unix_permissions(
    _path: &Path,
    _mode: u32,
    _label: &str,
) -> Result<(), ApplianceTelemetryLoopError> {
    Ok(())
}

fn sync_parent_directory(path: &Path) -> Result<(), ApplianceTelemetryLoopError> {
    File::open(path)
        .and_then(|dir| dir.sync_all())
        .map_err(|error| {
            ApplianceTelemetryLoopError::Sink(format!(
                "sync telemetry state directory {}: {error}",
                path.display()
            ))
        })
}
