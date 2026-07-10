use super::super::{admin_jobs::AdminJobRegistry, service::DaemonServiceRuntimeError};
use super::{
    daemon_job_event_for_summary, record_remote_upload_s3_transfer_job,
    run_remote_upload_cancellation_cleanup, RemoteUploadAdmissionGate,
    RemoteUploadCancellationCleanupPlan, RemoteUploadCancellationCleanupRunReport,
    RemoteUploadCancellationCleanupWorker, RemoteUploadRuntimeSnapshot, RemoteUploadS3ByteTransfer,
    RemoteUploadS3TransferJob, RemoteUploadS3TransferJobOutcome, RemoteUploadS3TransferJobSummary,
    RemoteUploadS3TransferProgressReporter,
};
use crate::api::{
    DaemonJobEvent, DaemonJobId, DaemonJobKind, DaemonJobProgress, DaemonJobState, DaemonJobSummary,
};
use std::{fmt, sync::Arc};

pub struct RemoteUploadS3TransferWorker<'a> {
    gate: Arc<RemoteUploadAdmissionGate>,
    registry: &'a dyn AdminJobRegistry,
}

impl<'a> RemoteUploadS3TransferWorker<'a> {
    pub fn new(gate: Arc<RemoteUploadAdmissionGate>, registry: &'a dyn AdminJobRegistry) -> Self {
        Self { gate, registry }
    }

    pub fn run<E>(
        &self,
        request: RemoteUploadS3TransferWorkerRequest,
        transfer: impl FnOnce() -> Result<(), E>,
    ) -> Result<RemoteUploadS3TransferWorkerReport, DaemonServiceRuntimeError>
    where
        E: fmt::Display,
    {
        self.run_with_progress(request, |_| transfer())
    }

    pub fn run_byte_transfer(
        &self,
        request: RemoteUploadS3TransferWorkerRequest,
        transfer: &(impl RemoteUploadS3ByteTransfer + ?Sized),
    ) -> Result<RemoteUploadS3TransferWorkerReport, DaemonServiceRuntimeError> {
        self.run_with_progress(request, |progress| transfer.transfer(progress))
    }

    pub fn run_with_cleanup_on_failure<E>(
        &self,
        request: RemoteUploadS3TransferWorkerRequest,
        cleanup_plan: RemoteUploadCancellationCleanupPlan,
        cleanup_worker: &dyn RemoteUploadCancellationCleanupWorker,
        transfer: impl FnOnce(&mut RemoteUploadS3TransferProgressReporter<'_>) -> Result<(), E>,
    ) -> Result<RemoteUploadS3TransferWorkerReport, DaemonServiceRuntimeError>
    where
        E: fmt::Display,
    {
        self.run_with_progress_inner(request, Some((cleanup_plan, cleanup_worker)), transfer)
    }

    pub fn run_with_progress<E>(
        &self,
        request: RemoteUploadS3TransferWorkerRequest,
        transfer: impl FnOnce(&mut RemoteUploadS3TransferProgressReporter<'_>) -> Result<(), E>,
    ) -> Result<RemoteUploadS3TransferWorkerReport, DaemonServiceRuntimeError>
    where
        E: fmt::Display,
    {
        self.run_with_progress_inner(request, None, transfer)
    }

    fn run_with_progress_inner<E>(
        &self,
        request: RemoteUploadS3TransferWorkerRequest,
        cleanup_on_failure: Option<(
            RemoteUploadCancellationCleanupPlan,
            &dyn RemoteUploadCancellationCleanupWorker,
        )>,
        transfer: impl FnOnce(&mut RemoteUploadS3TransferProgressReporter<'_>) -> Result<(), E>,
    ) -> Result<RemoteUploadS3TransferWorkerReport, DaemonServiceRuntimeError>
    where
        E: fmt::Display,
    {
        let RemoteUploadS3TransferWorkerRequest {
            job,
            submitted_at_utc,
            started_at_utc,
            finished_at_utc,
            actor,
        } = request;

        let permit = match self
            .gate
            .try_acquire_s3_transfer(job.policy, job.ssd_pressure)
        {
            Ok(permit) => permit,
            Err(decision) => {
                let summary = RemoteUploadS3TransferJobSummary {
                    job_id: job.job_id,
                    object_store: job.object_store,
                    source_bytes: job.source_bytes,
                    outcome: RemoteUploadS3TransferJobOutcome::NotAdmitted(decision),
                    runtime_after: self.gate.snapshot(),
                };
                let final_event = record_remote_upload_s3_transfer_job(
                    self.registry,
                    &summary,
                    submitted_at_utc,
                    finished_at_utc,
                    actor,
                )?;
                return Ok(RemoteUploadS3TransferWorkerReport {
                    running_event: None,
                    progress_events: Vec::new(),
                    final_event,
                    runtime_after: summary.runtime_after,
                    cleanup_report: None,
                });
            }
        };

        let running_job = running_daemon_job_for_s3_transfer(
            &job,
            submitted_at_utc.clone(),
            started_at_utc.clone(),
            actor.clone(),
        )?;
        let running_event = daemon_job_event_for_summary(running_job.clone());
        self.registry.record(running_job)?;

        let mut progress_reporter = RemoteUploadS3TransferProgressReporter::new(
            self.registry,
            Arc::clone(&self.gate),
            job.job_id.clone(),
            job.object_store.clone(),
            job.source_bytes,
            submitted_at_utc.clone(),
            started_at_utc,
            actor.clone(),
        )?;

        let transfer_result = transfer(&mut progress_reporter);
        let progress_events = progress_reporter.into_events();
        drop(permit);

        let (outcome, cleanup_report) = match transfer_result {
            Ok(()) => (RemoteUploadS3TransferJobOutcome::Completed, None),
            Err(error) => {
                let cleanup_report = cleanup_on_failure
                    .map(|(plan, worker)| run_remote_upload_cancellation_cleanup(plan, worker));
                (
                    RemoteUploadS3TransferJobOutcome::Failed {
                        error: error.to_string(),
                    },
                    cleanup_report,
                )
            }
        };

        let summary = RemoteUploadS3TransferJobSummary {
            job_id: job.job_id,
            object_store: job.object_store,
            source_bytes: job.source_bytes,
            outcome,
            runtime_after: self.gate.snapshot(),
        };
        let final_event = record_remote_upload_s3_transfer_job(
            self.registry,
            &summary,
            submitted_at_utc,
            finished_at_utc,
            actor,
        )?;

        Ok(RemoteUploadS3TransferWorkerReport {
            running_event: Some(running_event),
            progress_events,
            final_event,
            runtime_after: summary.runtime_after,
            cleanup_report,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteUploadS3TransferWorkerRequest {
    pub job: RemoteUploadS3TransferJob,
    pub submitted_at_utc: String,
    pub started_at_utc: String,
    pub finished_at_utc: String,
    pub actor: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteUploadS3TransferWorkerReport {
    pub running_event: Option<DaemonJobEvent>,
    pub progress_events: Vec<DaemonJobEvent>,
    pub final_event: DaemonJobEvent,
    pub runtime_after: RemoteUploadRuntimeSnapshot,
    pub cleanup_report: Option<RemoteUploadCancellationCleanupRunReport>,
}

fn running_daemon_job_for_s3_transfer(
    job: &RemoteUploadS3TransferJob,
    submitted_at_utc: String,
    updated_at_utc: String,
    actor: Option<String>,
) -> Result<DaemonJobSummary, DaemonServiceRuntimeError> {
    let job_id = DaemonJobId::new(job.job_id.clone())
        .map_err(|_| DaemonServiceRuntimeError::InvalidJobId(job.job_id.clone()))?;

    Ok(DaemonJobSummary {
        job_id,
        kind: DaemonJobKind::RemoteUpload,
        state: DaemonJobState::Running,
        progress: DaemonJobProgress {
            stage: "remote_s3_transfer_running".to_string(),
            work_bytes_done: 0,
            work_bytes_total: job.source_bytes,
            work_units_done: 0,
            work_units_total: 1,
            message: Some(format!(
                "remote upload S3 transfer running for {}",
                job.object_store
            )),
        },
        submitted_at_utc,
        updated_at_utc,
        actor,
        failure_message: None,
    })
}
