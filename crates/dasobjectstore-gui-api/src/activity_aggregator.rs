use crate::dashboard::DashboardWarning;
use crate::workspaces::{
    ActivityTaskKindView, ActivityTaskStateView, ActivityTaskView, ActivityWorkspaceView,
};
use dasobjectstore_daemon::{
    DaemonClient, DaemonJobKind, DaemonJobListRequest, DaemonJobListResponse, DaemonJobState,
    DaemonJobSummary, DaemonRuntimeConfig, UnixSocketDaemonTransport,
};

const ACTIVITY_JOB_LIMIT: usize = 50;

pub fn live_activity_workspace() -> ActivityWorkspaceView {
    let client = DaemonClient::new(UnixSocketDaemonTransport::new(
        DaemonRuntimeConfig::default_packaged().socket_path,
    ));
    match client.list_jobs(DaemonJobListRequest {
        limit: Some(ACTIVITY_JOB_LIMIT),
    }) {
        Ok(response) => activity_workspace_from_daemon_jobs(response),
        Err(err) => {
            let mut view = ActivityWorkspaceView::bootstrap();
            view.warnings.push(DashboardWarning::new(
                "daemon_activity_unavailable",
                format!("Daemon activity job list is unavailable: {err}"),
            ));
            view
        }
    }
}

pub fn activity_workspace_from_daemon_jobs(
    response: DaemonJobListResponse,
) -> ActivityWorkspaceView {
    ActivityWorkspaceView::bootstrap().with_tasks(
        response
            .jobs
            .iter()
            .map(activity_task_from_daemon_job)
            .collect(),
    )
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
        DaemonJobKind::IngestFiles | DaemonJobKind::DirectImport => ActivityTaskKindView::Ingest,
        DaemonJobKind::DiskDrain
        | DaemonJobKind::DiskRetire
        | DaemonJobKind::DiskReplace
        | DaemonJobKind::Repair => ActivityTaskKindView::Repair,
        DaemonJobKind::EnclosurePreparation => ActivityTaskKindView::EnclosurePreparation,
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
    use super::activity_workspace_from_daemon_jobs;
    use dasobjectstore_daemon::{
        DaemonJobId, DaemonJobKind, DaemonJobListResponse, DaemonJobProgress, DaemonJobState,
        DaemonJobSummary,
    };

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
}
