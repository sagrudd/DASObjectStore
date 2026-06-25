use dasobjectstore_core::ids::PoolId;
use dasobjectstore_core::lifecycle::PoolState;
use serde::{Deserialize, Serialize};

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
    fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
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

#[cfg(test)]
mod tests {
    use super::{PoolAccessMode, PoolStateView, PoolStatusView};
    use dasobjectstore_core::ids::PoolId;
    use dasobjectstore_core::lifecycle::PoolState;

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
}
