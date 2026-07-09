use dasobjectstore_core::health::{HealthScore, HealthSignals};
use dasobjectstore_core::ids::{DiskId, PoolId};
use dasobjectstore_core::lifecycle::{HealthState, PoolState};
use serde::{Deserialize, Serialize};

mod attention;
mod destage_queue;
mod ingest_queue;
mod redesign;

pub use attention::{
    DashboardActionKind, DashboardActionPriority, DashboardAttentionSourceKind,
    DashboardAttentionSourceView, DashboardAttentionView, DashboardRequiredActionView,
    DashboardSeverity, DashboardWarningItemView,
};
pub use destage_queue::{DestageQueueObjectView, DestageQueueView, ObjectStateView};
pub use ingest_queue::{
    IngestJobStateView, IngestProgressView, IngestQueueJobView, IngestQueueView, QueuePressureView,
};
pub use redesign::{
    ActiveUsersSummaryView, AddEnclosureAffordanceView, CapacitySummaryView, CpuUsageSummaryView,
    CreateObjectStoreAffordanceView, CreateObjectStoreDefaultsView, CreateObjectStoreFieldView,
    DasEnclosureCardView, DasEnclosureDetailView, DashboardHealthStateView, DiskIoSummaryView,
    DriveCountSummaryView, EnclosureConnectionView, EnclosureDriveSlotView, EnclosuresPageView,
    HealthSummaryView, HomeDashboardView, MemoryStressStateView, MemoryStressView,
    ObjectServiceStatusView, ObjectStoreCardView, ObjectStoresPageView, SmartWarningView,
    SmartWarningsSummaryView, StorageGroupView, StoreClassOptionView, TelemetryCardStateView,
    ThroughputDayView, ThroughputSummaryView, WriterPolicyReadinessView,
    REDESIGN_DASHBOARD_SCHEMA_VERSION,
};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PoolStatusView {
    pub pool_id: String,
    pub state: PoolStateView,
    pub access_mode: PoolAccessMode,
    pub disk_count: usize,
    pub updated_at_utc: String,
    pub warnings: Vec<DashboardWarning>,
}

impl PoolStatusView {
    pub fn from_pool_summary(
        pool_id: &PoolId,
        state: PoolState,
        disk_count: usize,
        updated_at_utc: impl Into<String>,
    ) -> Self {
        Self {
            pool_id: pool_id.to_string(),
            state: PoolStateView::from(state),
            access_mode: PoolAccessMode::from(state),
            disk_count,
            updated_at_utc: updated_at_utc.into(),
            warnings: pool_warnings(state),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolStateView {
    New,
    Clean,
    Dirty,
    ReadOnly,
    Repairing,
    Degraded,
}

impl From<PoolState> for PoolStateView {
    fn from(state: PoolState) -> Self {
        match state {
            PoolState::New => Self::New,
            PoolState::Clean => Self::Clean,
            PoolState::Dirty => Self::Dirty,
            PoolState::ReadOnly => Self::ReadOnly,
            PoolState::Repairing => Self::Repairing,
            PoolState::Degraded => Self::Degraded,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolAccessMode {
    ReadWrite,
    ReadOnly,
    RepairRequired,
    Initialization,
}

impl From<PoolState> for PoolAccessMode {
    fn from(state: PoolState) -> Self {
        match state {
            PoolState::New => Self::Initialization,
            PoolState::Clean | PoolState::Dirty => Self::ReadWrite,
            PoolState::ReadOnly => Self::ReadOnly,
            PoolState::Repairing | PoolState::Degraded => Self::RepairRequired,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DashboardWarning {
    pub code: String,
    pub message: String,
}

impl DashboardWarning {
    pub(crate) fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

fn pool_warnings(state: PoolState) -> Vec<DashboardWarning> {
    match state {
        PoolState::Dirty => vec![DashboardWarning::new(
            "pool_dirty",
            "Pool was not cleanly detached; read-only import is recommended.",
        )],
        PoolState::Repairing => vec![DashboardWarning::new(
            "pool_repairing",
            "Pool repair is in progress; write operations should remain blocked.",
        )],
        PoolState::Degraded => vec![DashboardWarning::new(
            "pool_degraded",
            "Pool is degraded; health and drain planning should be reviewed.",
        )],
        PoolState::New | PoolState::Clean | PoolState::ReadOnly => Vec::new(),
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiskHealthView {
    pub disk_id: String,
    pub state: HealthStateView,
    pub score: u8,
    pub placement_eligible: bool,
    pub signals: HealthSignalsView,
    pub warnings: Vec<DashboardWarning>,
}

impl DiskHealthView {
    pub fn from_health(disk_id: &DiskId, score: HealthScore, signals: &HealthSignals) -> Self {
        Self {
            disk_id: disk_id.to_string(),
            state: HealthStateView::from(score.state),
            score: score.value,
            placement_eligible: placement_eligible(score.state),
            signals: HealthSignalsView::from(signals),
            warnings: disk_warnings(score.state, signals),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthStateView {
    Healthy,
    Watch,
    Suspect,
    Draining,
    Retired,
    Failed,
}

impl From<HealthState> for HealthStateView {
    fn from(state: HealthState) -> Self {
        match state {
            HealthState::Healthy => Self::Healthy,
            HealthState::Watch => Self::Watch,
            HealthState::Suspect => Self::Suspect,
            HealthState::Draining => Self::Draining,
            HealthState::Retired => Self::Retired,
            HealthState::Failed => Self::Failed,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HealthSignalsView {
    pub smart_warnings: u16,
    pub io_errors: u16,
    pub checksum_failures: u16,
    pub usb_resets: u16,
    pub temperature_celsius: Option<u8>,
    pub benchmark_drift_percent: Option<u8>,
}

impl From<&HealthSignals> for HealthSignalsView {
    fn from(signals: &HealthSignals) -> Self {
        Self {
            smart_warnings: signals.smart_warnings,
            io_errors: signals.io_errors,
            checksum_failures: signals.checksum_failures,
            usb_resets: signals.usb_resets,
            temperature_celsius: signals.temperature_celsius,
            benchmark_drift_percent: signals.benchmark_drift_percent,
        }
    }
}

fn placement_eligible(state: HealthState) -> bool {
    matches!(state, HealthState::Healthy | HealthState::Watch)
}

fn disk_warnings(state: HealthState, signals: &HealthSignals) -> Vec<DashboardWarning> {
    let mut warnings = Vec::new();
    if !placement_eligible(state) {
        warnings.push(DashboardWarning::new(
            "disk_not_placement_eligible",
            "Disk health state blocks new protected placement.",
        ));
    }
    if signals.smart_warnings > 0 {
        warnings.push(DashboardWarning::new(
            "disk_smart_warning",
            "SMART warnings have been observed for this disk.",
        ));
    }
    if signals.io_errors > 0 || signals.checksum_failures > 0 {
        warnings.push(DashboardWarning::new(
            "disk_data_integrity_warning",
            "IO errors or checksum failures have been observed for this disk.",
        ));
    }

    warnings
}

#[cfg(test)]
mod tests {
    use super::{DiskHealthView, HealthStateView, PoolAccessMode, PoolStateView, PoolStatusView};
    use dasobjectstore_core::health::{HealthScore, HealthSignals};
    use dasobjectstore_core::ids::{DiskId, PoolId};
    use dasobjectstore_core::lifecycle::{HealthState, PoolState};

    #[test]
    fn builds_pool_status_view_from_core_state() {
        let pool_id = PoolId::new("pool-a").expect("pool id");

        let view = PoolStatusView::from_pool_summary(
            &pool_id,
            PoolState::Dirty,
            2,
            "2026-01-05T00:00:00Z",
        );

        assert_eq!(view.pool_id, "pool-a");
        assert_eq!(view.state, PoolStateView::Dirty);
        assert_eq!(view.access_mode, PoolAccessMode::ReadWrite);
        assert_eq!(view.disk_count, 2);
        assert_eq!(view.warnings[0].code, "pool_dirty");
    }

    #[test]
    fn serializes_pool_status_for_dashboard_contract() {
        let pool_id = PoolId::new("pool-a").expect("pool id");
        let view = PoolStatusView::from_pool_summary(
            &pool_id,
            PoolState::ReadOnly,
            1,
            "2026-01-05T00:00:00Z",
        );

        let encoded = serde_json::to_value(view).expect("pool status serializes");

        assert_eq!(encoded["state"], "read_only");
        assert_eq!(encoded["access_mode"], "read_only");
        assert_eq!(
            encoded["warnings"]
                .as_array()
                .expect("warnings array")
                .len(),
            0
        );
    }

    #[test]
    fn builds_disk_health_view_from_core_health() {
        let disk_id = DiskId::new("disk-a").expect("disk id");
        let signals = HealthSignals {
            smart_warnings: 1,
            ..HealthSignals::default()
        };
        let score = HealthScore::from_signals(&signals);

        let view = DiskHealthView::from_health(&disk_id, score, &signals);

        assert_eq!(view.disk_id, "disk-a");
        assert_eq!(view.state, HealthStateView::Watch);
        assert_eq!(view.score, 75);
        assert!(view.placement_eligible);
        assert_eq!(view.signals.smart_warnings, 1);
        assert_eq!(view.warnings[0].code, "disk_smart_warning");
    }

    #[test]
    fn serializes_disk_health_for_dashboard_contract() {
        let disk_id = DiskId::new("disk-a").expect("disk id");
        let signals = HealthSignals::default();
        let view = DiskHealthView::from_health(
            &disk_id,
            HealthScore {
                value: 0,
                state: HealthState::Failed,
            },
            &signals,
        );

        let encoded = serde_json::to_value(view).expect("disk health serializes");

        assert_eq!(encoded["state"], "failed");
        assert_eq!(encoded["placement_eligible"], false);
        assert_eq!(
            encoded["warnings"][0]["code"],
            "disk_not_placement_eligible"
        );
    }
}
