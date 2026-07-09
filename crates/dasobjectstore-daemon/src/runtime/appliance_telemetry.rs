//! Linux appliance telemetry collectors and daemon-owned collection loop.

mod linux;
mod model;
mod service_loop;
mod sessions;

pub use linux::{
    collect_linux_cpu_telemetry, collect_linux_disk_capacity_telemetry,
    collect_linux_disk_io_telemetry, collect_linux_memory_telemetry, parse_linux_cpu_snapshot,
    parse_linux_diskstats, LinuxProcTelemetryCollector, DEFAULT_APPLIANCE_TELEMETRY_HDD_ROOT,
};
pub use model::{
    ApplianceCpuTelemetry, ApplianceDiskCapacityTelemetry, ApplianceDiskIoTelemetry,
    ApplianceEnclosureTelemetry, ApplianceMemoryTelemetry, ApplianceSessionTelemetry,
    ApplianceTelemetryCollectionQuality, ApplianceTelemetryCollectorError,
    ApplianceTelemetryMissingDataMarker, ApplianceTelemetryMissingReason, ApplianceTelemetrySample,
    ApplianceTelemetrySampleSet, ApplianceTelemetrySource, LinuxCpuSnapshot, LinuxDiskIoCounters,
    LinuxHostTelemetrySample, APPLIANCE_TELEMETRY_DIR_NAME,
    APPLIANCE_TELEMETRY_FAST_CADENCE_SECONDS, APPLIANCE_TELEMETRY_FILE_NAME,
    APPLIANCE_TELEMETRY_NORMAL_CADENCE_SECONDS, APPLIANCE_TELEMETRY_SCHEMA_VERSION,
};
pub use service_loop::{
    appliance_sample_set, appliance_telemetry_state_path, validate_appliance_telemetry_cadence,
    ApplianceHostTelemetryCollector, ApplianceTelemetryLoop, ApplianceTelemetryLoopConfig,
    ApplianceTelemetryLoopError, ApplianceTelemetrySink, ApplianceTelemetrySleeper,
    FileBackedApplianceTelemetrySink, ThreadApplianceTelemetrySleeper,
};
pub use sessions::{
    collect_appliance_session_telemetry, DEFAULT_LOCAL_GROUP_PATH,
    DEFAULT_REMOTE_EASYCONNECT_SESSION_PATH, DEFAULT_STANDALONE_AUTH_ROOT,
};
