use crate::dashboard::{
    DashboardWarning, DestageQueueObjectView, DestageQueueView, IngestJobStateView,
    IngestQueueJobView, IngestQueueView, QueuePressureView,
};
use crate::endpoints::{
    EndpointInventoryItemView, EndpointInventoryView, EndpointValidationStateView,
};
use crate::workspaces::{
    default_activity_categories, ActivityTaskKindView, ActivityTaskStateView, ActivityTaskView,
    ActivityWorkspaceView,
};
use dasobjectstore_core::lifecycle::{ObjectState, PoolState};
use dasobjectstore_daemon::{
    DaemonClient, DaemonJobKind, DaemonJobListRequest, DaemonJobListResponse, DaemonJobState,
    DaemonJobSummary, DaemonRuntimeConfig, UnixSocketDaemonTransport,
};
use dasobjectstore_metadata::{
    read_ingest_queue, read_pool_repair_activity, IngestQueueJob, IngestQueueSnapshot,
    PoolRepairActivityEvent, PoolRepairActivitySnapshot, LIVE_SQLITE_FILE_NAME, METADATA_DIR_NAME,
};
use std::path::{Path, PathBuf};

const ACTIVITY_JOB_LIMIT: usize = 50;
const ACTIVITY_LIVE_SQLITE_ENV: &str = "DASOBJECTSTORE_LIVE_SQLITE_PATH";
const DEFAULT_SSD_ROOT_ENV: &str = "DASOBJECTSTORE_SSD_ROOT";
const DEFAULT_SSD_ROOT: &str = "/srv/dasobjectstore/ssd";

pub fn live_activity_workspace() -> ActivityWorkspaceView {
    let live_sqlite_path = activity_live_sqlite_path();
    let (ingest, destage, mut warnings) =
        match activity_queue_views_from_live_sqlite(&live_sqlite_path) {
            Ok((ingest, destage)) => (Some(ingest), Some(destage), Vec::new()),
            Err(warning) => (None, None, vec![warning]),
        };

    let client = DaemonClient::new(UnixSocketDaemonTransport::new(
        DaemonRuntimeConfig::default_packaged().socket_path,
    ));
    let mut tasks = match client.list_jobs(DaemonJobListRequest {
        limit: Some(ACTIVITY_JOB_LIMIT),
    }) {
        Ok(response) => activity_tasks_from_daemon_jobs(&response),
        Err(err) => {
            warnings.push(DashboardWarning::new(
                "daemon_activity_unavailable",
                format!("Daemon activity job list is unavailable: {err}"),
            ));
            Vec::new()
        }
    };
    match activity_repair_tasks_from_live_sqlite(&live_sqlite_path) {
        Ok(repair_tasks) => tasks.extend(repair_tasks),
        Err(warning) => warnings.push(warning),
    }
    let endpoint_inventory = crate::endpoints_aggregator::live_endpoint_inventory();
    warnings.extend(
        endpoint_inventory
            .warnings
            .iter()
            .map(|warning| DashboardWarning::new(warning.code.clone(), warning.message.clone())),
    );
    tasks.extend(activity_tasks_from_endpoint_inventory(&endpoint_inventory));

    let mut view = ActivityWorkspaceView::from_sections(ingest, destage, tasks)
        .with_categories(default_activity_categories());
    view.warnings.extend(warnings);
    view
}

fn activity_repair_tasks_from_live_sqlite(
    live_sqlite_path: &Path,
) -> Result<Vec<ActivityTaskView>, DashboardWarning> {
    if !live_sqlite_path.exists() {
        return Ok(Vec::new());
    }

    read_pool_repair_activity(live_sqlite_path)
        .map(|snapshot| activity_tasks_from_repair_snapshot(&snapshot))
        .map_err(|err| {
            DashboardWarning::new(
                "activity_repair_events_unavailable",
                format!("Live repair metadata cannot be read: {err}"),
            )
        })
}

pub fn activity_tasks_from_repair_snapshot(
    snapshot: &PoolRepairActivitySnapshot,
) -> Vec<ActivityTaskView> {
    snapshot
        .events
        .iter()
        .map(activity_task_from_repair_event)
        .collect()
}

fn activity_task_from_repair_event(event: &PoolRepairActivityEvent) -> ActivityTaskView {
    let state = match event.state {
        PoolState::Repairing => ActivityTaskStateView::Running,
        PoolState::Degraded => ActivityTaskStateView::Waiting,
        PoolState::New | PoolState::Clean | PoolState::Dirty | PoolState::ReadOnly => {
            ActivityTaskStateView::Complete
        }
    };
    let label = event.reason.clone().unwrap_or_else(|| match event.state {
        PoolState::Repairing => format!("Pool {} repair is in progress.", event.pool_id),
        PoolState::Degraded => format!(
            "Pool {} is degraded and needs repair review.",
            event.pool_id
        ),
        PoolState::New | PoolState::Clean | PoolState::Dirty | PoolState::ReadOnly => {
            format!("Pool {} does not report active repair work.", event.pool_id)
        }
    });

    let warnings = match event.state {
        PoolState::Repairing => vec![DashboardWarning::new(
            "pool_repairing",
            "Pool repair is in progress; write operations should remain blocked.",
        )],
        PoolState::Degraded => vec![DashboardWarning::new(
            "pool_degraded",
            "Pool is degraded; review repair or replacement plans before writes continue.",
        )],
        PoolState::New | PoolState::Clean | PoolState::Dirty | PoolState::ReadOnly => Vec::new(),
    };

    ActivityTaskView {
        task_id: format!("pool-repair-{}", event.pool_id),
        kind: ActivityTaskKindView::Repair,
        state,
        label,
        updated_at_utc: event.updated_at_utc.clone(),
        warnings,
    }
}

pub fn activity_tasks_from_endpoint_inventory(
    inventory: &EndpointInventoryView,
) -> Vec<ActivityTaskView> {
    inventory
        .endpoints
        .iter()
        .map(activity_task_from_endpoint)
        .collect()
}

fn activity_task_from_endpoint(endpoint: &EndpointInventoryItemView) -> ActivityTaskView {
    let state = match endpoint.validation.state {
        EndpointValidationStateView::Draft
        | EndpointValidationStateView::PendingValidation
        | EndpointValidationStateView::Unknown => ActivityTaskStateView::Waiting,
        EndpointValidationStateView::Validated => ActivityTaskStateView::Complete,
        EndpointValidationStateView::Degraded | EndpointValidationStateView::Rejected => {
            ActivityTaskStateView::Failed
        }
    };
    let warnings = endpoint
        .warnings
        .iter()
        .map(|warning| DashboardWarning::new(warning.code.clone(), warning.message.clone()))
        .collect();
    let label = endpoint.validation.message.clone().unwrap_or_else(|| {
        format!(
            "Endpoint {} validation is {}.",
            endpoint.display_name,
            endpoint_validation_state_label(endpoint.validation.state)
        )
    });

    ActivityTaskView {
        task_id: format!("endpoint-validation-{}", endpoint.endpoint_id),
        kind: ActivityTaskKindView::EndpointValidation,
        state,
        label,
        updated_at_utc: endpoint
            .validation
            .checked_at_utc
            .clone()
            .unwrap_or_else(|| "not_checked".to_string()),
        warnings,
    }
}

fn endpoint_validation_state_label(state: EndpointValidationStateView) -> &'static str {
    match state {
        EndpointValidationStateView::Draft => "draft",
        EndpointValidationStateView::PendingValidation => "pending validation",
        EndpointValidationStateView::Validated => "validated",
        EndpointValidationStateView::Degraded => "degraded",
        EndpointValidationStateView::Rejected => "rejected",
        EndpointValidationStateView::Unknown => "unknown",
    }
}

#[cfg(test)]
pub fn activity_workspace_from_daemon_jobs(
    response: DaemonJobListResponse,
) -> ActivityWorkspaceView {
    ActivityWorkspaceView::bootstrap().with_tasks(activity_tasks_from_daemon_jobs(&response))
}

fn activity_tasks_from_daemon_jobs(response: &DaemonJobListResponse) -> Vec<ActivityTaskView> {
    response
        .jobs
        .iter()
        .map(activity_task_from_daemon_job)
        .collect()
}

fn activity_live_sqlite_path() -> PathBuf {
    std::env::var_os(ACTIVITY_LIVE_SQLITE_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os(DEFAULT_SSD_ROOT_ENV)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(DEFAULT_SSD_ROOT))
                .join(METADATA_DIR_NAME)
                .join(LIVE_SQLITE_FILE_NAME)
        })
}

fn activity_queue_views_from_live_sqlite(
    live_sqlite_path: &Path,
) -> Result<(IngestQueueView, DestageQueueView), DashboardWarning> {
    if !live_sqlite_path.exists() {
        return Err(DashboardWarning::new(
            "activity_ingest_queue_unavailable",
            format!(
                "Live ingest queue metadata is unavailable at {}.",
                live_sqlite_path.display()
            ),
        ));
    }

    read_ingest_queue(live_sqlite_path)
        .map(|snapshot| activity_queue_views_from_snapshot(&snapshot))
        .map_err(|err| {
            DashboardWarning::new(
                "activity_ingest_queue_unavailable",
                format!("Live ingest queue metadata cannot be read: {err}"),
            )
        })
}

pub fn activity_queue_views_from_snapshot(
    snapshot: &IngestQueueSnapshot,
) -> (IngestQueueView, DestageQueueView) {
    let ingest_jobs = snapshot.jobs.iter().map(ingest_job_view).collect();
    let destage_objects = snapshot
        .jobs
        .iter()
        .filter_map(destage_object_view_from_ingest_job)
        .collect();

    (
        IngestQueueView::from_jobs(QueuePressureView::Normal, ingest_jobs),
        DestageQueueView::from_objects(destage_objects),
    )
}

fn ingest_job_view(job: &IngestQueueJob) -> IngestQueueJobView {
    let state = ingest_job_state_view(&job.state);
    let mut warnings = Vec::new();

    if let Some(message) = &job.failure_message {
        let code = match state {
            IngestJobStateView::Cancelled => "ingest_job_cancelled",
            IngestJobStateView::Failed => "ingest_job_failed",
            _ => "ingest_job_message",
        };
        warnings.push(DashboardWarning::new(code, message.clone()));
    }

    if job
        .expected_size_bytes
        .is_some_and(|expected| job.received_bytes > expected)
    {
        warnings.push(DashboardWarning::new(
            "ingest_size_exceeded",
            "Received bytes exceed the expected object size.",
        ));
    }

    if state == IngestJobStateView::Failed && job.failure_message.is_none() {
        warnings.push(DashboardWarning::new(
            "ingest_job_failed",
            "Ingest job failed before settlement.",
        ));
    }

    IngestQueueJobView {
        ingest_job_id: job.ingest_job_id.to_string(),
        store_id: job.store_id.to_string(),
        object_id: job.object_id.as_ref().map(ToString::to_string),
        state,
        priority: job.priority,
        received_bytes: job.received_bytes,
        expected_size_bytes: job.expected_size_bytes,
        updated_at_utc: job.updated_at_utc.clone(),
        warnings,
    }
}

fn ingest_job_state_view(state: &str) -> IngestJobStateView {
    match state.trim().to_ascii_lowercase().replace('-', "_").as_str() {
        "queued" => IngestJobStateView::Queued,
        "receiving" => IngestJobStateView::Receiving,
        "received" => IngestJobStateView::Received,
        "hashing" => IngestJobStateView::Hashing,
        "ready_for_placement" | "readyforplacement" => IngestJobStateView::ReadyForPlacement,
        "destaging" => IngestJobStateView::Destaging,
        "complete" => IngestJobStateView::Complete,
        "cancelled" | "canceled" => IngestJobStateView::Cancelled,
        "failed" => IngestJobStateView::Failed,
        _ => IngestJobStateView::Failed,
    }
}

fn destage_object_view_from_ingest_job(job: &IngestQueueJob) -> Option<DestageQueueObjectView> {
    let object_id = job.object_id.as_ref()?;
    let state = ingest_job_state_view(&job.state);
    let (object_state, copy_count, required_copies) = match state {
        IngestJobStateView::Queued | IngestJobStateView::Receiving => return None,
        IngestJobStateView::Received => (ObjectState::ReceivedOnSsd, 0, 1),
        IngestJobStateView::Hashing => (ObjectState::HashVerified, 0, 1),
        IngestJobStateView::ReadyForPlacement => (ObjectState::PlacementPlanned, 0, 1),
        IngestJobStateView::Destaging => (ObjectState::CopyingToHdd, 0, 1),
        IngestJobStateView::Complete => (ObjectState::Protected, 1, 1),
        IngestJobStateView::Failed | IngestJobStateView::Cancelled => {
            (ObjectState::RedownloadRequired, 0, 1)
        }
    };

    Some(DestageQueueObjectView::from_object(
        object_id,
        &job.store_id,
        object_state,
        copy_count,
        required_copies,
        job.updated_at_utc.clone(),
    ))
}

fn activity_task_from_daemon_job(job: &DaemonJobSummary) -> ActivityTaskView {
    let mut warnings = Vec::new();
    if let Some(message) = &job.failure_message {
        let code = if job.state == DaemonJobState::Failed {
            "daemon_job_failed"
        } else {
            "daemon_job_message"
        };
        warnings.push(DashboardWarning::new(code, message.clone()));
    }

    ActivityTaskView {
        task_id: job.job_id.to_string(),
        kind: activity_kind_from_daemon_job(&job.kind),
        state: activity_state_from_daemon_job(&job.state),
        label: activity_label_from_daemon_job(job),
        updated_at_utc: job.updated_at_utc.clone(),
        warnings,
    }
}

fn activity_kind_from_daemon_job(kind: &DaemonJobKind) -> ActivityTaskKindView {
    match kind {
        DaemonJobKind::IngestFiles | DaemonJobKind::DirectImport | DaemonJobKind::RemoteUpload => {
            ActivityTaskKindView::Ingest
        }
        DaemonJobKind::DiskDrain
        | DaemonJobKind::DiskRetire
        | DaemonJobKind::DiskReplace
        | DaemonJobKind::Repair => ActivityTaskKindView::Repair,
        DaemonJobKind::EnclosurePreparation => ActivityTaskKindView::EnclosurePreparation,
        DaemonJobKind::EndpointValidation => ActivityTaskKindView::EndpointValidation,
        DaemonJobKind::ObjectStoreCreation => ActivityTaskKindView::ObjectStoreCreation,
        DaemonJobKind::ServiceOperation | DaemonJobKind::SystemAdministration => {
            ActivityTaskKindView::SystemAdministration
        }
    }
}

fn activity_state_from_daemon_job(state: &DaemonJobState) -> ActivityTaskStateView {
    match state {
        DaemonJobState::Queued => ActivityTaskStateView::Queued,
        DaemonJobState::Running => ActivityTaskStateView::Running,
        DaemonJobState::Waiting => ActivityTaskStateView::Waiting,
        DaemonJobState::Complete => ActivityTaskStateView::Complete,
        DaemonJobState::Failed => ActivityTaskStateView::Failed,
        DaemonJobState::Cancelled => ActivityTaskStateView::Cancelled,
    }
}

fn activity_label_from_daemon_job(job: &DaemonJobSummary) -> String {
    job.progress
        .message
        .clone()
        .or_else(|| job.failure_message.clone())
        .unwrap_or_else(|| format!("{:?} job {}", job.kind, job.job_id))
}

#[cfg(test)]
mod tests {
    use super::{
        activity_queue_views_from_snapshot, activity_tasks_from_endpoint_inventory,
        activity_tasks_from_repair_snapshot, activity_workspace_from_daemon_jobs,
    };
    use dasobjectstore_core::ids::{IngestJobId, ObjectId, StoreId};
    use dasobjectstore_core::lifecycle::PoolState;
    use dasobjectstore_core::object_type::ObjectType;
    use dasobjectstore_daemon::{
        DaemonJobId, DaemonJobKind, DaemonJobListResponse, DaemonJobProgress, DaemonJobState,
        DaemonJobSummary,
    };
    use dasobjectstore_metadata::{
        IngestQueueJob, IngestQueueSnapshot, PoolRepairActivityEvent, PoolRepairActivitySnapshot,
    };
    use std::path::PathBuf;

    #[test]
    fn maps_daemon_jobs_into_activity_tasks() {
        let view = activity_workspace_from_daemon_jobs(DaemonJobListResponse {
            jobs: vec![
                daemon_job(
                    "admin-1",
                    DaemonJobKind::SystemAdministration,
                    DaemonJobState::Running,
                    Some("create local writer group"),
                    None,
                ),
                daemon_job(
                    "repair-1",
                    DaemonJobKind::DiskReplace,
                    DaemonJobState::Cancelled,
                    None,
                    Some("operator cancelled"),
                ),
            ],
        });

        assert_eq!(view.categories.len(), 8);
        assert_eq!(
            view.tasks[0].kind,
            crate::ActivityTaskKindView::SystemAdministration
        );
        assert_eq!(view.tasks[0].state, crate::ActivityTaskStateView::Running);
        assert_eq!(view.tasks[0].label, "create local writer group");
        assert_eq!(view.tasks[1].kind, crate::ActivityTaskKindView::Repair);
        assert_eq!(view.tasks[1].state, crate::ActivityTaskStateView::Cancelled);
        assert_eq!(view.tasks[1].warnings[0].code, "daemon_job_message");
    }

    #[test]
    fn maps_live_ingest_queue_into_activity_sections() {
        let snapshot = IngestQueueSnapshot {
            live_sqlite_path: PathBuf::from("/tmp/live.sqlite"),
            jobs: vec![
                ingest_queue_job("job-queued", None, "Queued", 0, Some(1024), None),
                ingest_queue_job(
                    "job-destaging",
                    Some("object-destaging"),
                    "Destaging",
                    512,
                    Some(1024),
                    None,
                ),
                ingest_queue_job(
                    "job-complete",
                    Some("object-complete"),
                    "Complete",
                    1024,
                    Some(1024),
                    None,
                ),
                ingest_queue_job(
                    "job-cancelled",
                    Some("object-cancelled"),
                    "Cancelled",
                    128,
                    Some(1024),
                    Some("operator drained the queue"),
                ),
            ],
        };

        let (ingest, destage) = activity_queue_views_from_snapshot(&snapshot);

        assert_eq!(ingest.queued_jobs, 1);
        assert_eq!(ingest.active_jobs, 2);
        assert_eq!(ingest.failed_jobs, 0);
        assert_eq!(
            ingest.jobs[3].state,
            crate::dashboard::IngestJobStateView::Cancelled
        );
        assert_eq!(ingest.jobs[3].warnings[0].code, "ingest_job_cancelled");
        assert_eq!(destage.pending_objects, 0);
        assert_eq!(destage.copying_objects, 1);
        assert_eq!(destage.verified_objects, 1);
        assert_eq!(
            destage.objects[2].warnings[0].code,
            "object_redownload_required"
        );
    }

    #[test]
    fn maps_repair_pool_state_into_activity_tasks() {
        let snapshot = PoolRepairActivitySnapshot {
            live_sqlite_path: PathBuf::from("/tmp/live.sqlite"),
            events: vec![
                PoolRepairActivityEvent {
                    pool_id: dasobjectstore_core::ids::PoolId::new("pool-repair").expect("pool id"),
                    state: PoolState::Repairing,
                    marker_kind: Some("repair_import".to_string()),
                    reason: Some("checksum repair".to_string()),
                    updated_at_utc: "2026-07-09T00:02:00Z".to_string(),
                },
                PoolRepairActivityEvent {
                    pool_id: dasobjectstore_core::ids::PoolId::new("pool-degraded")
                        .expect("pool id"),
                    state: PoolState::Degraded,
                    marker_kind: None,
                    reason: None,
                    updated_at_utc: "2026-07-09T00:03:00Z".to_string(),
                },
            ],
        };

        let tasks = activity_tasks_from_repair_snapshot(&snapshot);

        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].kind, crate::ActivityTaskKindView::Repair);
        assert_eq!(tasks[0].state, crate::ActivityTaskStateView::Running);
        assert_eq!(tasks[0].label, "checksum repair");
        assert_eq!(tasks[0].warnings[0].code, "pool_repairing");
        assert_eq!(tasks[1].state, crate::ActivityTaskStateView::Waiting);
        assert_eq!(tasks[1].warnings[0].code, "pool_degraded");
    }

    #[test]
    fn maps_endpoint_validation_inventory_into_activity_tasks() {
        let inventory = crate::EndpointInventoryView::from_endpoints(vec![
            crate::EndpointInventoryItemView::new(
                "endpoint-pending",
                "NAS staging",
                crate::EndpointKindView::DasobjectstoreNfs,
                "https://nas.example.test:9443",
                crate::EndpointValidationView::new(
                    crate::EndpointValidationStateView::PendingValidation,
                )
                .with_check("2026-07-09T00:01:00Z", "Runtime probe queued."),
            ),
            crate::EndpointInventoryItemView::new(
                "endpoint-rejected",
                "Rejected endpoint",
                crate::EndpointKindView::S3Compatible,
                "https://s3.example.test",
                crate::EndpointValidationView::new(crate::EndpointValidationStateView::Rejected)
                    .with_check("2026-07-09T00:02:00Z", "Credential check failed."),
            ),
        ]);

        let tasks = activity_tasks_from_endpoint_inventory(&inventory);

        assert_eq!(tasks.len(), 2);
        assert_eq!(
            tasks[0].kind,
            crate::ActivityTaskKindView::EndpointValidation
        );
        assert_eq!(tasks[0].state, crate::ActivityTaskStateView::Waiting);
        assert_eq!(tasks[0].label, "Runtime probe queued.");
        assert_eq!(tasks[1].state, crate::ActivityTaskStateView::Failed);
        assert_eq!(tasks[1].warnings[0].code, "endpoint_rejected");
    }

    fn daemon_job(
        job_id: &str,
        kind: DaemonJobKind,
        state: DaemonJobState,
        message: Option<&str>,
        failure_message: Option<&str>,
    ) -> DaemonJobSummary {
        DaemonJobSummary {
            job_id: DaemonJobId::new(job_id).expect("job id"),
            kind,
            state,
            progress: DaemonJobProgress {
                message: message.map(str::to_string),
                ..DaemonJobProgress::default()
            },
            submitted_at_utc: "2026-07-09T00:00:00Z".to_string(),
            updated_at_utc: "2026-07-09T00:01:00Z".to_string(),
            actor: Some("operator".to_string()),
            failure_message: failure_message.map(str::to_string),
        }
    }

    fn ingest_queue_job(
        ingest_job_id: &str,
        object_id: Option<&str>,
        state: &str,
        received_bytes: u64,
        expected_size_bytes: Option<u64>,
        failure_message: Option<&str>,
    ) -> IngestQueueJob {
        IngestQueueJob {
            ingest_job_id: IngestJobId::new(ingest_job_id).expect("ingest job id"),
            store_id: StoreId::new("store-a").expect("store id"),
            object_id: object_id.map(|value| ObjectId::new(value).expect("object id")),
            object_type: ObjectType::Naive,
            state: state.to_string(),
            ingest_mode: "files".to_string(),
            acknowledgement_policy: "strict".to_string(),
            priority: 0,
            staging_path: "/srv/dasobjectstore/ssd/.dasobjectstore/ingest/job".to_string(),
            expected_size_bytes,
            received_bytes,
            content_hash: None,
            content_hash_algorithm: None,
            failure_message: failure_message.map(str::to_string),
            created_at_utc: "2026-07-09T00:00:00Z".to_string(),
            updated_at_utc: "2026-07-09T00:01:00Z".to_string(),
        }
    }
}
