//! Axum API boundary for GUI-facing DASObjectStore views.

pub mod dashboard;
pub mod routes;
pub mod view;

pub use dashboard::{
    DashboardActionKind, DashboardActionPriority, DashboardAttentionSourceKind,
    DashboardAttentionSourceView, DashboardAttentionView, DashboardRequiredActionView,
    DashboardSeverity, DashboardWarning, DashboardWarningItemView, DestageQueueObjectView,
    DestageQueueView, DiskHealthView, HealthSignalsView, HealthStateView, IngestJobStateView,
    IngestProgressView, IngestQueueJobView, IngestQueueView, ObjectStateView, PoolAccessMode,
    PoolStateView, PoolStatusView, QueuePressureView,
};
pub use routes::gui_api_router;

/// Returns the GUI API crate version.
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
