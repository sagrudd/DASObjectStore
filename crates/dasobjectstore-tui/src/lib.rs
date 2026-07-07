//! Embedded terminal rendering helpers for DASObjectStore CLI actions.
//!
//! These helpers are intentionally library-only. DASObjectStore does not expose
//! a standalone TUI command; long-running commands may opt into embedded
//! graphical terminal views with flags such as `dasobjectstore ingest files
//! --tui`.

pub mod layout;
pub mod monitoring;
pub mod planning;
pub mod resource;
pub mod upload;

pub use layout::{classify_terminal_layout, TerminalLayout};
pub use monitoring::{
    format_rate_label, AttachState, Bottleneck, CompletionFraction, DaemonActionSupport,
    HddPressureTelemetry, IngestProgressTelemetry, IngestRunState, KeyboardActionDisplay,
    KeyboardActionKind, KeyboardActionModel, LiveIngestTelemetry, LiveMonitoringDisplay,
    PipelinePressureLevel, PressureTrend, QueueDepthTelemetry, SourceThrottleState,
    SsdPressureLevel, SsdPressureTelemetry, ThroughputTelemetry, ThroughputTrend, TuiErrorKind,
    TuiErrorState, VerificationStatus, VerificationTelemetry, WorkerActivity, WorkerTelemetry,
};
pub use planning::{
    format_size_label, ImportDescriptionMetadata, ImportDescriptionMetadataDisplay,
    ImportLaunchBlocker, ImportLaunchConfirmation, ImportLaunchReview, ImportMetadataError,
    ImportMetadataField, ImportPlan, ImportPlanningSummary, ImportTarget, ResourceCap,
    ResourceUsePlan, SourcePath, IMPORT_LAUNCH_CONFIRMATION_PHRASE,
};
pub use resource::{ResourcePolicyDisplay, ResourcePolicySummary, WorkerCounts};
pub use upload::{UploadTui, UploadTuiContext};

/// Returns the TUI crate version.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::version;

    #[test]
    fn exposes_package_version() {
        assert_eq!(version(), env!("CARGO_PKG_VERSION"));
    }
}
