use crate::dashboard::{DashboardHealthStateView, HomeDashboardView};
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub(crate) struct CachedHomeDashboardView {
    pub snapshot: HomeDashboardView,
    pub stale: bool,
    pub warning: Option<String>,
}

#[derive(Default)]
struct HomeDashboardSnapshotCache {
    snapshot: Option<HomeDashboardView>,
}

impl HomeDashboardSnapshotCache {
    fn capture(&mut self, live: HomeDashboardView) -> Result<CachedHomeDashboardView, String> {
        let usable = matches!(
            live.health.state,
            DashboardHealthStateView::Healthy | DashboardHealthStateView::Watch
        );
        if usable {
            self.snapshot = Some(live.clone());
            return Ok(CachedHomeDashboardView {
                snapshot: live,
                stale: false,
                warning: None,
            });
        }

        let Some(snapshot) = self.snapshot.clone() else {
            return Err(
                "appliance status is unavailable and no successful snapshot has been cached"
                    .to_string(),
            );
        };
        Ok(CachedHomeDashboardView {
            snapshot,
            stale: true,
            warning: Some(
                "appliance status is degraded; showing the last successful snapshot; retry shortly"
                    .to_string(),
            ),
        })
    }
}

fn home_dashboard_snapshot_cache() -> &'static Mutex<HomeDashboardSnapshotCache> {
    static CACHE: OnceLock<Mutex<HomeDashboardSnapshotCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HomeDashboardSnapshotCache::default()))
}

pub(crate) fn cached_home_dashboard() -> Result<CachedHomeDashboardView, String> {
    let live = super::live_home_dashboard();
    home_dashboard_snapshot_cache()
        .lock()
        .map_err(|_| "appliance status cache is unavailable".to_string())?
        .capture(live)
}

#[cfg(test)]
mod tests {
    use super::{CachedHomeDashboardView, HomeDashboardSnapshotCache};
    use crate::dashboard::{DashboardHealthStateView, HomeDashboardView};

    #[test]
    fn retains_last_usable_snapshot_on_degraded_refresh() {
        let usable = HomeDashboardView::bootstrap_fixture();
        let mut cache = HomeDashboardSnapshotCache::default();
        let first = cache
            .capture(usable.clone())
            .expect("usable snapshot cached");
        assert_eq!(
            first,
            CachedHomeDashboardView {
                snapshot: usable.clone(),
                stale: false,
                warning: None,
            }
        );

        let mut degraded = usable;
        degraded.health.state = DashboardHealthStateView::Degraded;
        let stale = cache.capture(degraded).expect("stale snapshot retained");
        assert!(stale.stale);
        assert_eq!(stale.snapshot.health.state, DashboardHealthStateView::Watch);
        assert!(stale
            .warning
            .as_deref()
            .is_some_and(|warning| warning.contains("retry")));
    }

    #[test]
    fn fails_closed_before_first_usable_snapshot() {
        let mut cache = HomeDashboardSnapshotCache::default();
        let mut degraded = HomeDashboardView::bootstrap_fixture();
        degraded.health.state = DashboardHealthStateView::Degraded;

        let error = cache
            .capture(degraded)
            .expect_err("cold start must fail closed");
        assert!(error.contains("no successful snapshot"));
    }
}
