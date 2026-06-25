use crate::dashboard::{
    DashboardWarning, DestageQueueView, DiskHealthView, HealthStateView, IngestQueueView,
    PoolStateView, PoolStatusView, QueuePressureView,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DashboardAttentionView {
    pub highest_severity: DashboardSeverity,
    pub warning_count: usize,
    pub action_count: usize,
    pub warnings: Vec<DashboardWarningItemView>,
    pub required_actions: Vec<DashboardRequiredActionView>,
}

impl DashboardAttentionView {
    pub fn from_sections(
        pool: Option<&PoolStatusView>,
        disks: &[DiskHealthView],
        ingest: Option<&IngestQueueView>,
        destage: Option<&DestageQueueView>,
    ) -> Self {
        let mut warnings = Vec::new();
        let mut actions = Vec::new();

        if let Some(pool) = pool {
            collect_pool_attention(pool, &mut warnings, &mut actions);
        }

        for disk in disks {
            collect_disk_attention(disk, &mut warnings, &mut actions);
        }

        if let Some(ingest) = ingest {
            collect_ingest_attention(ingest, &mut warnings, &mut actions);
        }

        if let Some(destage) = destage {
            collect_destage_attention(destage, &mut warnings, &mut actions);
        }

        deduplicate_actions(&mut actions);
        let highest_severity = warnings
            .iter()
            .map(|warning| warning.severity)
            .max()
            .unwrap_or(DashboardSeverity::Info);

        Self {
            highest_severity,
            warning_count: warnings.len(),
            action_count: actions.len(),
            warnings,
            required_actions: actions,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DashboardWarningItemView {
    pub source: DashboardAttentionSourceView,
    pub severity: DashboardSeverity,
    pub warning: DashboardWarning,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DashboardRequiredActionView {
    pub code: String,
    pub label: String,
    pub priority: DashboardActionPriority,
    pub source: DashboardAttentionSourceView,
    pub action: DashboardActionKind,
    pub confirmation_required: bool,
}

impl DashboardRequiredActionView {
    fn new(
        code: impl Into<String>,
        label: impl Into<String>,
        priority: DashboardActionPriority,
        source: DashboardAttentionSourceView,
        action: DashboardActionKind,
    ) -> Self {
        Self {
            code: code.into(),
            label: label.into(),
            priority,
            source,
            action,
            confirmation_required: false,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DashboardAttentionSourceView {
    pub kind: DashboardAttentionSourceKind,
    pub id: Option<String>,
}

impl DashboardAttentionSourceView {
    fn pool(pool_id: impl Into<String>) -> Self {
        Self {
            kind: DashboardAttentionSourceKind::Pool,
            id: Some(pool_id.into()),
        }
    }

    fn disk(disk_id: impl Into<String>) -> Self {
        Self {
            kind: DashboardAttentionSourceKind::Disk,
            id: Some(disk_id.into()),
        }
    }

    fn ingest_queue() -> Self {
        Self {
            kind: DashboardAttentionSourceKind::IngestQueue,
            id: None,
        }
    }

    fn destage_queue() -> Self {
        Self {
            kind: DashboardAttentionSourceKind::DestageQueue,
            id: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DashboardAttentionSourceKind {
    Pool,
    Disk,
    IngestQueue,
    DestageQueue,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DashboardSeverity {
    Info,
    Warning,
    Critical,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DashboardActionPriority {
    Normal,
    High,
    Critical,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DashboardActionKind {
    RunHealthCheck,
    ImportReadOnly,
    ReviewIngestQueue,
    ReviewDestageQueue,
    PlanDiskDrain,
    ReviewRepairPlan,
}

fn collect_pool_attention(
    pool: &PoolStatusView,
    warnings: &mut Vec<DashboardWarningItemView>,
    actions: &mut Vec<DashboardRequiredActionView>,
) {
    let source = DashboardAttentionSourceView::pool(pool.pool_id.clone());
    collect_warnings(source.clone(), &pool.warnings, warnings);

    match pool.state {
        PoolStateView::Dirty => actions.push(DashboardRequiredActionView::new(
            "pool_import_read_only",
            "Import pool read-only before writes are allowed.",
            DashboardActionPriority::High,
            source,
            DashboardActionKind::ImportReadOnly,
        )),
        PoolStateView::Repairing | PoolStateView::Degraded => {
            actions.push(DashboardRequiredActionView::new(
                "pool_review_repair_plan",
                "Review pool repair state before writes are allowed.",
                DashboardActionPriority::Critical,
                source,
                DashboardActionKind::ReviewRepairPlan,
            ));
        }
        PoolStateView::New | PoolStateView::Clean | PoolStateView::ReadOnly => {}
    }
}

fn collect_disk_attention(
    disk: &DiskHealthView,
    warnings: &mut Vec<DashboardWarningItemView>,
    actions: &mut Vec<DashboardRequiredActionView>,
) {
    let source = DashboardAttentionSourceView::disk(disk.disk_id.clone());
    collect_warnings(source.clone(), &disk.warnings, warnings);

    if !disk.placement_eligible {
        actions.push(DashboardRequiredActionView::new(
            format!("disk_plan_drain:{}", disk.disk_id),
            "Plan disk drain before protected placement resumes.",
            disk_action_priority(disk.state),
            source,
            DashboardActionKind::PlanDiskDrain,
        ));
    } else if !disk.warnings.is_empty() {
        actions.push(DashboardRequiredActionView::new(
            format!("disk_run_health_check:{}", disk.disk_id),
            "Run a disk health check and review warning signals.",
            DashboardActionPriority::Normal,
            source,
            DashboardActionKind::RunHealthCheck,
        ));
    }
}

fn collect_ingest_attention(
    ingest: &IngestQueueView,
    warnings: &mut Vec<DashboardWarningItemView>,
    actions: &mut Vec<DashboardRequiredActionView>,
) {
    let source = DashboardAttentionSourceView::ingest_queue();
    collect_warnings(source.clone(), &ingest.warnings, warnings);

    if ingest.failed_jobs > 0 || matches!(ingest.pressure, QueuePressureView::Critical) {
        actions.push(DashboardRequiredActionView::new(
            "ingest_review_queue",
            "Review failed ingest jobs or critical SSD pressure.",
            ingest_action_priority(ingest),
            source,
            DashboardActionKind::ReviewIngestQueue,
        ));
    }
}

fn collect_destage_attention(
    destage: &DestageQueueView,
    warnings: &mut Vec<DashboardWarningItemView>,
    actions: &mut Vec<DashboardRequiredActionView>,
) {
    let source = DashboardAttentionSourceView::destage_queue();
    collect_warnings(source.clone(), &destage.warnings, warnings);

    if !destage.warnings.is_empty() {
        actions.push(DashboardRequiredActionView::new(
            "destage_review_queue",
            "Review destage objects before SSD eviction.",
            DashboardActionPriority::High,
            source,
            DashboardActionKind::ReviewDestageQueue,
        ));
    }
}

fn collect_warnings(
    source: DashboardAttentionSourceView,
    source_warnings: &[DashboardWarning],
    warnings: &mut Vec<DashboardWarningItemView>,
) {
    warnings.extend(source_warnings.iter().cloned().map(|warning| {
        let severity = warning_severity(warning.code.as_str());
        DashboardWarningItemView {
            source: source.clone(),
            severity,
            warning,
        }
    }));
}

fn warning_severity(code: &str) -> DashboardSeverity {
    match code {
        "pool_degraded"
        | "disk_not_placement_eligible"
        | "disk_data_integrity_warning"
        | "ingest_critical_pressure"
        | "object_redownload_required" => DashboardSeverity::Critical,
        "pool_dirty"
        | "pool_repairing"
        | "disk_smart_warning"
        | "ingest_high_watermark"
        | "ingest_failed_jobs"
        | "ingest_job_failed"
        | "ingest_size_exceeded"
        | "destage_objects_need_review"
        | "object_under_replicated" => DashboardSeverity::Warning,
        _ => DashboardSeverity::Info,
    }
}

fn disk_action_priority(state: HealthStateView) -> DashboardActionPriority {
    match state {
        HealthStateView::Failed | HealthStateView::Retired => DashboardActionPriority::Critical,
        HealthStateView::Suspect | HealthStateView::Draining => DashboardActionPriority::High,
        HealthStateView::Healthy | HealthStateView::Watch => DashboardActionPriority::Normal,
    }
}

fn ingest_action_priority(ingest: &IngestQueueView) -> DashboardActionPriority {
    if matches!(ingest.pressure, QueuePressureView::Critical) {
        DashboardActionPriority::Critical
    } else {
        DashboardActionPriority::High
    }
}

fn deduplicate_actions(actions: &mut Vec<DashboardRequiredActionView>) {
    let mut seen = Vec::new();
    actions.retain(|action| {
        if seen.contains(&action.code) {
            false
        } else {
            seen.push(action.code.clone());
            true
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{
        DashboardActionKind, DashboardActionPriority, DashboardAttentionView, DashboardSeverity,
    };
    use crate::dashboard::{
        DestageQueueObjectView, DestageQueueView, DiskHealthView, IngestProgressView,
        IngestQueueJobView, IngestQueueView, PoolStatusView, QueuePressureView,
    };
    use dasobjectstore_core::health::{HealthScore, HealthSignals};
    use dasobjectstore_core::ids::{DiskId, IngestJobId, ObjectId, PoolId, StoreId};
    use dasobjectstore_core::lifecycle::{HealthState, IngestJobState, ObjectState, PoolState};

    #[test]
    fn aggregates_warnings_and_required_actions_from_dashboard_sections() {
        let pool = PoolStatusView::from_pool_summary(
            &PoolId::new("pool-a").expect("pool id"),
            PoolState::Dirty,
            2,
            "2026-01-05T00:00:00Z",
        );
        let disk = DiskHealthView::from_health(
            &DiskId::new("disk-a").expect("disk id"),
            HealthScore {
                value: 10,
                state: HealthState::Suspect,
            },
            &HealthSignals::default(),
        );
        let ingest = IngestQueueView::from_jobs(
            QueuePressureView::Critical,
            vec![IngestQueueJobView::from_ingest_job(
                &IngestJobId::new("job-a").expect("job id"),
                &StoreId::new("store-a").expect("store id"),
                None,
                IngestJobState::Failed,
                0,
                IngestProgressView::new(0, None),
                "2026-01-05T00:00:00Z",
            )],
        );
        let destage = DestageQueueView::from_objects(vec![DestageQueueObjectView::from_object(
            &ObjectId::new("object-a").expect("object id"),
            &StoreId::new("store-a").expect("store id"),
            ObjectState::Protected,
            1,
            2,
            "2026-01-05T00:00:00Z",
        )]);

        let view = DashboardAttentionView::from_sections(
            Some(&pool),
            &[disk],
            Some(&ingest),
            Some(&destage),
        );

        assert_eq!(view.highest_severity, DashboardSeverity::Critical);
        assert_eq!(view.warning_count, 5);
        assert_eq!(view.action_count, 4);
        assert!(view
            .required_actions
            .iter()
            .any(|action| action.action == DashboardActionKind::ImportReadOnly));
        assert!(view
            .required_actions
            .iter()
            .any(|action| action.action == DashboardActionKind::PlanDiskDrain));
        assert!(view
            .required_actions
            .iter()
            .any(|action| action.priority == DashboardActionPriority::Critical));
    }

    #[test]
    fn serializes_attention_view_for_dashboard_contract() {
        let pool = PoolStatusView::from_pool_summary(
            &PoolId::new("pool-a").expect("pool id"),
            PoolState::Clean,
            1,
            "2026-01-05T00:00:00Z",
        );
        let view = DashboardAttentionView::from_sections(Some(&pool), &[], None, None);

        let encoded = serde_json::to_value(view).expect("attention view serializes");

        assert_eq!(encoded["highest_severity"], "info");
        assert_eq!(encoded["warning_count"], 0);
        assert_eq!(encoded["action_count"], 0);
    }
}
