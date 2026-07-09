//! Linux appliance telemetry collectors and daemon-owned collection loop.

mod linux;
mod model;
mod service_loop;

pub use linux::{
    collect_linux_cpu_telemetry, collect_linux_memory_telemetry, parse_linux_cpu_snapshot,
    LinuxProcTelemetryCollector,
};
pub use model::{
    ApplianceCpuTelemetry, ApplianceMemoryTelemetry, ApplianceSessionTelemetry,
    ApplianceTelemetryCollectionQuality, ApplianceTelemetryCollectorError,
    ApplianceTelemetryMissingDataMarker, ApplianceTelemetryMissingReason, ApplianceTelemetrySample,
    ApplianceTelemetrySampleSet, ApplianceTelemetrySource, LinuxCpuSnapshot,
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
