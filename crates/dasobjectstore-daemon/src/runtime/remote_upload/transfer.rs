use super::super::capacity_provider::CapacityAdmissionProvider;
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
    CapacityAdmissionDecision, DaemonJobEvent, DaemonJobId, DaemonJobKind, DaemonJobProgress,
    DaemonJobState, DaemonJobSummary,
};
use dasobjectstore_core::ids::StoreId;
use std::{fmt, sync::Arc};

pub struct RemoteUploadS3TransferWorker<'a> {
    gate: Arc<RemoteUploadAdmissionGate>,
    registry: &'a dyn AdminJobRegistry,
    capacity_provider: Option<Arc<dyn CapacityAdmissionProvider>>,
}

/// The daemon-owned handoff that must succeed before a remote provider upload
/// is reported as complete. Implementations may commit a manifest/catalogue
/// transaction, but this boundary deliberately does not prescribe storage.
pub trait RemoteUploadCompletionCommit {
    fn commit(
        &self,
        record: &RemoteUploadCompletionRecord,
    ) -> Result<(), RemoteUploadCompletionCommitError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteUploadCompletionRecord {
    pub job_id: String,
    pub object_store: String,
    pub source_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteUploadCompletionCommitError {
    message: String,
}

impl RemoteUploadCompletionCommitError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for RemoteUploadCompletionCommitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RemoteUploadCompletionCommitError {}

impl<'a> RemoteUploadS3TransferWorker<'a> {
    pub fn new(gate: Arc<RemoteUploadAdmissionGate>, registry: &'a dyn AdminJobRegistry) -> Self {
        Self {
            gate,
            registry,
            capacity_provider: None,
        }
    }

    pub fn with_capacity_admission_provider(
        mut self,
        provider: Arc<dyn CapacityAdmissionProvider>,
    ) -> Self {
        self.capacity_provider = Some(provider);
        self
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

    /// Run a transfer and require the daemon-owned completion handoff before
    /// publishing a completed job event. A handoff failure is a transfer
    /// failure from the caller's perspective, so existing cleanup hooks and
    /// reservation release behavior remain in force.
    pub fn run_with_completion<E>(
        &self,
        request: RemoteUploadS3TransferWorkerRequest,
        completion: &dyn RemoteUploadCompletionCommit,
        transfer: impl FnOnce(&mut RemoteUploadS3TransferProgressReporter<'_>) -> Result<(), E>,
    ) -> Result<RemoteUploadS3TransferWorkerReport, DaemonServiceRuntimeError>
    where
        E: fmt::Display,
    {
        self.run_with_progress_inner(request, None, Some(completion), transfer)
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
        self.run_with_progress_inner(
            request,
            Some((cleanup_plan, cleanup_worker)),
            None,
            transfer,
        )
    }

    pub fn run_with_progress<E>(
        &self,
        request: RemoteUploadS3TransferWorkerRequest,
        transfer: impl FnOnce(&mut RemoteUploadS3TransferProgressReporter<'_>) -> Result<(), E>,
    ) -> Result<RemoteUploadS3TransferWorkerReport, DaemonServiceRuntimeError>
    where
        E: fmt::Display,
    {
        self.run_with_progress_inner(request, None, None, transfer)
    }

    fn run_with_progress_inner<E>(
        &self,
        request: RemoteUploadS3TransferWorkerRequest,
        cleanup_on_failure: Option<(
            RemoteUploadCancellationCleanupPlan,
            &dyn RemoteUploadCancellationCleanupWorker,
        )>,
        completion: Option<&dyn RemoteUploadCompletionCommit>,
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

        let capacity_reservation = if let Some(provider) = &self.capacity_provider {
            let response = provider
                .admit_remote_upload(&job.object_store, job.source_bytes, &job.job_id)
                .map_err(|error| DaemonServiceRuntimeError::UnsupportedOperation {
                    operation: format!("remote upload capacity admission failed: {error}"),
                })?;
            if response.decision != CapacityAdmissionDecision::Admitted {
                let summary = RemoteUploadS3TransferJobSummary {
                    job_id: job.job_id,
                    object_store: job.object_store,
                    source_bytes: job.source_bytes,
                    outcome: RemoteUploadS3TransferJobOutcome::Failed {
                        error: response
                            .message
                            .unwrap_or_else(|| "remote upload capacity rejected".to_string()),
                    },
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
            Some(Arc::clone(provider))
        } else {
            None
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

        let transfer_result = transfer(&mut progress_reporter).map_err(|error| error.to_string());
        let transfer_result = transfer_result.and_then(|()| {
            completion
                .map(|completion| {
                    completion
                        .commit(&RemoteUploadCompletionRecord {
                            job_id: job.job_id.clone(),
                            object_store: job.object_store.clone(),
                            source_bytes: job.source_bytes,
                        })
                        .map_err(|error| error.to_string())
                })
                .transpose()
                .map(|_| ())
        });
        let transfer_result = match (transfer_result, capacity_reservation) {
            (Ok(()), Some(provider)) => {
                let store_id = StoreId::new(job.object_store.clone()).map_err(|error| {
                    DaemonServiceRuntimeError::UnsupportedOperation {
                        operation: format!("remote upload object store is invalid: {error}"),
                    }
                })?;
                match provider.commit(&store_id, &job.job_id) {
                    Ok(()) => Ok(()),
                    Err(error) => match provider.release(&store_id, &job.job_id) {
                        Ok(()) => Err(error.to_string()),
                        Err(release_error) => {
                            Err(format!("{error}; capacity release failed: {release_error}"))
                        }
                    },
                }
            }
            (Err(error), Some(provider)) => {
                let store_id = StoreId::new(job.object_store.clone()).map_err(|error| {
                    DaemonServiceRuntimeError::UnsupportedOperation {
                        operation: format!("remote upload object store is invalid: {error}"),
                    }
                })?;
                let release = provider.release(&store_id, &job.job_id);
                match release {
                    Ok(()) => Err(error),
                    Err(release_error) => {
                        Err(format!("{error}; capacity release failed: {release_error}"))
                    }
                }
            }
            (result, None) => result,
        };
        let progress_events = progress_reporter.into_events();
        drop(permit);

        let (outcome, cleanup_report) = match transfer_result {
            Ok(()) => (RemoteUploadS3TransferJobOutcome::Completed, None),
            Err(error) => {
                let cleanup_report = cleanup_on_failure
                    .map(|(plan, worker)| run_remote_upload_cancellation_cleanup(plan, worker));
                (
                    RemoteUploadS3TransferJobOutcome::Failed { error },
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
