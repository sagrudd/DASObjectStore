//! Axum API boundary for GUI-facing DASObjectStore views.

pub mod actions;
pub mod dashboard;
pub mod routes;
pub mod server_config;
pub mod tls_assets;
pub mod view;

pub use actions::{
    action_catalog, plan_action, GuiActionCatalog, GuiActionDescriptor, GuiActionExecution,
    GuiActionKind, GuiActionPlan, GuiActionPlanError, GuiActionPlanRequest, GuiActionSafety,
};
pub use dashboard::{
    DashboardActionKind, DashboardActionPriority, DashboardAttentionSourceKind,
    DashboardAttentionSourceView, DashboardAttentionView, DashboardRequiredActionView,
    DashboardSeverity, DashboardWarning, DashboardWarningItemView, DestageQueueObjectView,
    DestageQueueView, DiskHealthView, HealthSignalsView, HealthStateView, IngestJobStateView,
    IngestProgressView, IngestQueueJobView, IngestQueueView, ObjectStateView, PoolAccessMode,
    PoolStateView, PoolStatusView, QueuePressureView,
};
pub use routes::gui_api_router;
pub use server_config::{
    StandaloneServerConfig, StandaloneServerConfigError, StandaloneTlsConfig,
    DEFAULT_STANDALONE_PUBLIC_BASE_URL, DEFAULT_TLS_CERTIFICATE_RELATIVE_PATH,
    DEFAULT_TLS_PRIVATE_KEY_RELATIVE_PATH,
};
pub use tls_assets::{
    ensure_standalone_tls_assets, load_standalone_tls_assets, StandaloneTlsAssetError,
    StandaloneTlsAssetReport, StandaloneTlsAssets,
};

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
