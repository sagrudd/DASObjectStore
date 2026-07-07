//! Terminal operator scaffold for DASObjectStore.
//!
//! Framework decision: future interactive screens should use `ratatui` for
//! widget/layout rendering with `crossterm` for terminal events. That pairing is
//! the current Rust TUI default, keeps the operator surface Rust-first, and can
//! be added when the daemon event contract is ready. This first Milestone 18
//! slice intentionally keeps the crate renderer-neutral so import planning,
//! resource policy previews, and terminal layout choices are testable without a
//! full terminal loop.

pub mod layout;
pub mod monitoring;
pub mod planning;
pub mod resource;

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

/// Returns the TUI crate version.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::version;

    #[test]
    fn exposes_package_version() {
        assert_eq!(version(), "0.0.0");
    }
}
