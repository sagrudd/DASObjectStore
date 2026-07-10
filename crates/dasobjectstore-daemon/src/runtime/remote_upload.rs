use super::{
    admin_jobs::AdminJobRegistry,
    service::{DaemonServiceRuntimeError, ServiceCommandRunner},
};
use crate::api::{
    DaemonJobEvent, DaemonJobId, DaemonJobKind, DaemonJobProgress, DaemonJobState,
    DaemonJobSummary, DaemonSsdPressure, RemoteEasyconnectUploadAdmissionDecision,
};
use dasobjectstore_core::remote_upload::RemoteUploadBackpressurePolicy;
use std::{fmt, sync::Arc};

mod admission;
mod cleanup;
mod progress;
pub use admission::*;
pub use cleanup::*;
use progress::daemon_job_event_for_summary;
pub use progress::*;

pub fn record_remote_upload_s3_transfer_job(
    registry: &(impl AdminJobRegistry + ?Sized),
    summary: &RemoteUploadS3TransferJobSummary,
    submitted_at_utc: impl Into<String>,
    updated_at_utc: impl Into<String>,
    actor: Option<String>,
) -> Result<DaemonJobEvent, DaemonServiceRuntimeError> {
    let job = summary
        .daemon_job_summary(submitted_at_utc, updated_at_utc, actor)
        .map_err(|_| DaemonServiceRuntimeError::InvalidJobId(summary.job_id.clone()))?;
    let event = daemon_job_event_for_summary(job.clone());
    registry.record(job)?;
    Ok(event)
}

pub trait RemoteUploadS3ByteTransfer {
    fn transfer(
        &self,
        progress: &mut RemoteUploadS3TransferProgressReporter<'_>,
    ) -> Result<(), RemoteUploadS3ByteTransferError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteUploadS3ByteTransferError {
    message: String,
}

impl RemoteUploadS3ByteTransferError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for RemoteUploadS3ByteTransferError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RemoteUploadS3ByteTransferError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteUploadAwsCliTransferPlan {
    pub program: String,
    pub args: Vec<String>,
    pub display_args: Vec<String>,
    pub environment: Vec<(String, String)>,
    pub source_bytes: u64,
    pub progress_updated_at_utc: String,
    pub progress_telemetry: Option<RemoteUploadProgressTelemetry>,
    pub progress_message: Option<String>,
}

impl RemoteUploadAwsCliTransferPlan {
    pub fn new(
        program: impl Into<String>,
        args: Vec<String>,
        source_bytes: u64,
        progress_updated_at_utc: impl Into<String>,
    ) -> Self {
        Self {
            program: program.into(),
            display_args: args.clone(),
            args,
            environment: Vec::new(),
            source_bytes,
            progress_updated_at_utc: progress_updated_at_utc.into(),
            progress_telemetry: None,
            progress_message: None,
        }
    }

    pub fn with_display_args(mut self, display_args: Vec<String>) -> Self {
        self.display_args = display_args;
        self
    }

    pub fn with_environment(mut self, environment: Vec<(String, String)>) -> Self {
        self.environment = environment;
        self
    }

    pub fn with_progress_message(mut self, message: impl Into<String>) -> Self {
        self.progress_message = Some(message.into());
        self
    }

    pub fn with_progress_telemetry(mut self, telemetry: RemoteUploadProgressTelemetry) -> Self {
        self.progress_telemetry = Some(telemetry);
        self
    }

    fn validate(&self) -> Result<(), RemoteUploadS3ByteTransferError> {
        if self.program.trim().is_empty() {
            return Err(RemoteUploadS3ByteTransferError::new(
                "remote upload AWS CLI program must not be blank",
            ));
        }
        if self.progress_updated_at_utc.trim().is_empty() {
            return Err(RemoteUploadS3ByteTransferError::new(
                "remote upload AWS CLI progress timestamp must not be blank",
            ));
        }
        Ok(())
    }
}

pub struct RemoteUploadAwsCliByteTransfer<'a> {
    plan: RemoteUploadAwsCliTransferPlan,
    runner: &'a dyn ServiceCommandRunner,
}

impl<'a> RemoteUploadAwsCliByteTransfer<'a> {
    pub fn new(plan: RemoteUploadAwsCliTransferPlan, runner: &'a dyn ServiceCommandRunner) -> Self {
        Self { plan, runner }
    }
}

impl RemoteUploadS3ByteTransfer for RemoteUploadAwsCliByteTransfer<'_> {
    fn transfer(
        &self,
        progress: &mut RemoteUploadS3TransferProgressReporter<'_>,
    ) -> Result<(), RemoteUploadS3ByteTransferError> {
        self.plan.validate()?;
        self.runner
            .run_with_display_args_and_env(
                &self.plan.program,
                &self.plan.args,
                &self.plan.display_args,
                &self.plan.environment,
            )
            .map_err(|error| RemoteUploadS3ByteTransferError::new(error.to_string()))?;
        progress
            .record_progress(RemoteUploadS3TransferProgressUpdate {
                bytes_done: self.plan.source_bytes,
                updated_at_utc: self.plan.progress_updated_at_utc.clone(),
                telemetry: self.plan.progress_telemetry.clone(),
                message: self
                    .plan
                    .progress_message
                    .clone()
                    .or_else(|| Some("remote upload AWS CLI transfer completed".to_string())),
            })
            .map_err(|error| RemoteUploadS3ByteTransferError::new(error.to_string()))?;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteEasyconnectAwsCliUploadJobRequest {
    pub job_id: String,
    pub object_store: String,
    pub source_bytes: u64,
    pub policy: RemoteUploadBackpressurePolicy,
    pub ssd_pressure: DaemonSsdPressure,
    pub program: String,
    pub args: Vec<String>,
    pub display_args: Vec<String>,
    pub environment: Vec<(String, String)>,
    pub submitted_at_utc: String,
    pub started_at_utc: String,
    pub finished_at_utc: String,
    pub progress_updated_at_utc: String,
    pub actor: Option<String>,
    pub progress_telemetry: Option<RemoteUploadProgressTelemetry>,
    pub progress_message: Option<String>,
}

pub fn run_remote_easyconnect_aws_cli_upload_job(
    registry: &dyn AdminJobRegistry,
    gate: Arc<RemoteUploadAdmissionGate>,
    runner: &dyn ServiceCommandRunner,
    request: RemoteEasyconnectAwsCliUploadJobRequest,
) -> Result<RemoteUploadS3TransferWorkerReport, DaemonServiceRuntimeError> {
    let worker_request = RemoteUploadS3TransferWorkerRequest {
        job: RemoteUploadS3TransferJob {
            job_id: request.job_id,
            object_store: request.object_store,
            source_bytes: request.source_bytes,
            policy: request.policy,
            ssd_pressure: request.ssd_pressure,
        },
        submitted_at_utc: request.submitted_at_utc,
        started_at_utc: request.started_at_utc,
        finished_at_utc: request.finished_at_utc,
        actor: request.actor,
    };
    let mut transfer_plan = RemoteUploadAwsCliTransferPlan::new(
        request.program,
        request.args,
        request.source_bytes,
        request.progress_updated_at_utc,
    )
    .with_display_args(request.display_args)
    .with_environment(request.environment);
    if let Some(telemetry) = request.progress_telemetry {
        transfer_plan = transfer_plan.with_progress_telemetry(telemetry);
    }
    if let Some(message) = request.progress_message {
        transfer_plan = transfer_plan.with_progress_message(message);
    }
    let transfer = RemoteUploadAwsCliByteTransfer::new(transfer_plan, runner);
    RemoteUploadS3TransferWorker::new(gate, registry).run_byte_transfer(worker_request, &transfer)
}

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RemoteUploadS3TransferJobOutcome {
    Completed,
    Failed { error: String },
    NotAdmitted(RemoteEasyconnectUploadAdmissionDecision),
}

fn non_blank(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{
        plan_remote_upload_cancellation_cleanup, record_remote_upload_s3_transfer_job,
        run_remote_easyconnect_aws_cli_upload_job, run_remote_upload_cancellation_cleanup,
        RemoteEasyconnectAwsCliUploadJobRequest, RemoteUploadAdmissionGate,
        RemoteUploadAwsCliByteTransfer, RemoteUploadAwsCliTransferPlan,
        RemoteUploadCancellationCleanupAction, RemoteUploadCancellationCleanupActionState,
        RemoteUploadCancellationCleanupError, RemoteUploadCancellationCleanupPlan,
        RemoteUploadCancellationCleanupRequest, RemoteUploadCancellationCleanupRuntime,
        RemoteUploadCancellationCleanupRuntimeConfig, RemoteUploadCancellationCleanupScope,
        RemoteUploadCancellationCleanupWorker, RemoteUploadMultipartAbortConfig,
        RemoteUploadProgressTelemetry, RemoteUploadQueueDepths, RemoteUploadS3ByteTransfer,
        RemoteUploadS3ByteTransferError, RemoteUploadS3TransferJob,
        RemoteUploadS3TransferJobOutcome, RemoteUploadS3TransferJobSummary,
        RemoteUploadS3TransferProgressReporter, RemoteUploadS3TransferProgressUpdate,
        RemoteUploadS3TransferWorker, RemoteUploadS3TransferWorkerRequest,
    };
    use crate::api::{
        DaemonIngestQueueDepths, DaemonIngestTelemetry, DaemonIngestWorkerActivity,
        DaemonIngestWorkerTelemetry, DaemonJobEvent, DaemonJobKind, DaemonJobState,
        DaemonJobStatusRequest, DaemonSsdPressure, RemoteEasyconnectUploadAdmissionDecision,
        RemoteEasyconnectUploadBackpressureReason,
    };
    use crate::runtime::{admin_job_registry_path, AdminJobRegistry, FileBackedAdminJobRegistry};
    use crate::runtime::{DaemonServiceRuntimeError, ServiceCommandOutput, ServiceCommandRunner};
    use dasobjectstore_core::remote_upload::{
        RemoteUploadBackpressureAction, RemoteUploadBackpressurePolicy,
    };

    #[test]
    fn gate_increments_active_s3_transfers_only_when_admitted() {
        let gate = RemoteUploadAdmissionGate::new();
        let policy = RemoteUploadBackpressurePolicy::default();

        let first = gate.try_begin_s3_transfer(policy, DaemonSsdPressure::AcceptingWrites);
        let second = gate.try_begin_s3_transfer(policy, DaemonSsdPressure::AcceptingWrites);
        let third = gate.try_begin_s3_transfer(policy, DaemonSsdPressure::AcceptingWrites);

        assert_eq!(first.action, RemoteUploadBackpressureAction::Accept);
        assert_eq!(second.action, RemoteUploadBackpressureAction::Accept);
        assert_eq!(
            third.action,
            RemoteUploadBackpressureAction::PauseNewTransfers
        );
        assert_eq!(
            third.reason,
            RemoteEasyconnectUploadBackpressureReason::S3TransferConcurrencyFull
        );
        assert_eq!(gate.snapshot().active_s3_transfers, 2);
    }

    #[test]
    fn s3_transfer_permit_releases_capacity_on_drop() {
        let gate = std::sync::Arc::new(RemoteUploadAdmissionGate::new());
        let policy = RemoteUploadBackpressurePolicy::default();

        let permit = gate
            .try_acquire_s3_transfer(policy, DaemonSsdPressure::AcceptingWrites)
            .expect("first transfer admitted");
        assert_eq!(gate.snapshot().active_s3_transfers, 1);

        drop(permit);

        assert_eq!(gate.snapshot().active_s3_transfers, 0);
    }

    #[test]
    fn s3_transfer_permit_can_be_released_explicitly_once() {
        let gate = std::sync::Arc::new(RemoteUploadAdmissionGate::new());
        let policy = RemoteUploadBackpressurePolicy::default();
        let permit = gate
            .try_acquire_s3_transfer(policy, DaemonSsdPressure::AcceptingWrites)
            .expect("first transfer admitted");

        let snapshot = permit.release();

        assert_eq!(snapshot.active_s3_transfers, 0);
        assert_eq!(gate.snapshot().active_s3_transfers, 0);
    }

    #[test]
    fn blocked_s3_transfer_permit_does_not_increment_capacity() {
        let gate = std::sync::Arc::new(RemoteUploadAdmissionGate::new());
        let policy = RemoteUploadBackpressurePolicy::default();
        let _first = gate
            .try_acquire_s3_transfer(policy, DaemonSsdPressure::AcceptingWrites)
            .expect("first transfer admitted");
        let _second = gate
            .try_acquire_s3_transfer(policy, DaemonSsdPressure::AcceptingWrites)
            .expect("second transfer admitted");

        let decision = gate
            .try_acquire_s3_transfer(policy, DaemonSsdPressure::AcceptingWrites)
            .expect_err("third transfer blocked");

        assert_eq!(
            decision.action,
            RemoteUploadBackpressureAction::PauseNewTransfers
        );
        assert_eq!(gate.snapshot().active_s3_transfers, 2);
    }

    #[test]
    fn run_s3_transfer_releases_capacity_after_success() {
        let gate = std::sync::Arc::new(RemoteUploadAdmissionGate::new());
        let policy = RemoteUploadBackpressurePolicy::default();

        let result = gate.run_s3_transfer(policy, DaemonSsdPressure::AcceptingWrites, || {
            assert_eq!(gate.snapshot().active_s3_transfers, 1);
            Ok::<_, &'static str>("uploaded")
        });

        assert_eq!(result, Ok("uploaded"));
        assert_eq!(gate.snapshot().active_s3_transfers, 0);
    }

    #[test]
    fn run_s3_transfer_releases_capacity_after_transfer_error() {
        let gate = std::sync::Arc::new(RemoteUploadAdmissionGate::new());
        let policy = RemoteUploadBackpressurePolicy::default();

        let result = gate.run_s3_transfer(policy, DaemonSsdPressure::AcceptingWrites, || {
            assert_eq!(gate.snapshot().active_s3_transfers, 1);
            Err::<(), _>("network failed")
        });

        assert_eq!(
            result,
            Err(super::RemoteUploadS3TransferRunError::Transfer(
                "network failed"
            ))
        );
        assert_eq!(gate.snapshot().active_s3_transfers, 0);
    }

    #[test]
    fn run_s3_transfer_does_not_execute_when_admission_blocks() {
        let gate = std::sync::Arc::new(RemoteUploadAdmissionGate::new());
        let policy = RemoteUploadBackpressurePolicy::default();
        let _first = gate
            .try_acquire_s3_transfer(policy, DaemonSsdPressure::AcceptingWrites)
            .expect("first transfer admitted");
        let _second = gate
            .try_acquire_s3_transfer(policy, DaemonSsdPressure::AcceptingWrites)
            .expect("second transfer admitted");

        let result: Result<(), super::RemoteUploadS3TransferRunError<&'static str>> = gate
            .run_s3_transfer(policy, DaemonSsdPressure::AcceptingWrites, || {
                panic!("blocked transfer must not execute")
            });

        let Err(super::RemoteUploadS3TransferRunError::Admission(decision)) = result else {
            panic!("expected admission failure");
        };
        assert_eq!(
            decision.action,
            RemoteUploadBackpressureAction::PauseNewTransfers
        );
        assert_eq!(gate.snapshot().active_s3_transfers, 2);
    }

    #[test]
    fn gate_uses_observed_daemon_queue_depths_for_admission() {
        let gate = RemoteUploadAdmissionGate::new();
        let policy = RemoteUploadBackpressurePolicy::default();
        gate.observe_queue_depths(RemoteUploadQueueDepths {
            ssd_stage_queue_depth: policy.max_ssd_stage_queue_depth,
            hdd_landing_queue_depth: 0,
            verification_queue_depth: 0,
        });

        let decision = gate.admission_decision(policy, DaemonSsdPressure::AcceptingWrites);

        assert_eq!(
            decision.action,
            RemoteUploadBackpressureAction::PauseNewTransfers
        );
        assert_eq!(
            decision.reason,
            RemoteEasyconnectUploadBackpressureReason::SsdStageQueueFull
        );
    }

    #[test]
    fn remote_upload_queue_depths_are_derived_from_ingest_worker_queues() {
        let depths = RemoteUploadQueueDepths::from(DaemonIngestQueueDepths {
            scan: 99,
            source_read: 88,
            ssd_stage: 3,
            hdd_write: 5,
            verification: 7,
        });

        assert_eq!(
            depths,
            RemoteUploadQueueDepths {
                ssd_stage_queue_depth: 3,
                hdd_landing_queue_depth: 5,
                verification_queue_depth: 7,
            }
        );
    }

    #[test]
    fn gate_observes_ingest_telemetry_queue_depths_for_admission() {
        let gate = RemoteUploadAdmissionGate::new();
        let policy = RemoteUploadBackpressurePolicy::default();
        let telemetry = DaemonIngestTelemetry {
            queue_depths: DaemonIngestQueueDepths {
                hdd_write: policy.max_hdd_landing_queue_depth,
                ..DaemonIngestQueueDepths::default()
            },
            ..DaemonIngestTelemetry::default()
        };
        let snapshot = gate.observe_ingest_telemetry(telemetry);
        let decision = gate.admission_decision(policy, DaemonSsdPressure::AcceptingWrites);

        assert_eq!(snapshot.ssd_stage_queue_depth, 0);
        assert_eq!(
            snapshot.hdd_landing_queue_depth,
            policy.max_hdd_landing_queue_depth
        );
        assert_eq!(snapshot.verification_queue_depth, 0);
        assert_eq!(snapshot.active_hdd_writers, 0);
        assert_eq!(
            decision.reason,
            RemoteEasyconnectUploadBackpressureReason::HddLandingQueueFull
        );
    }

    #[test]
    fn finishing_s3_transfer_saturates_at_zero() {
        let gate = RemoteUploadAdmissionGate::new();

        let snapshot = gate.finish_s3_transfer();

        assert_eq!(snapshot.active_s3_transfers, 0);
    }

    #[test]
    fn s3_transfer_job_runs_through_gate_and_reports_completion() {
        let gate = std::sync::Arc::new(RemoteUploadAdmissionGate::new());
        let mut called = false;

        let summary = gate.run_s3_transfer_job(
            transfer_job(RemoteUploadBackpressurePolicy::default()),
            || {
                called = true;
                assert_eq!(gate.snapshot().active_s3_transfers, 1);
                Ok::<(), &'static str>(())
            },
        );

        assert!(called);
        assert_eq!(summary.job_id, "remote-upload-job-001");
        assert_eq!(summary.object_store, "zymo_fecal_2025.05");
        assert_eq!(summary.source_bytes, 42);
        assert_eq!(summary.outcome, RemoteUploadS3TransferJobOutcome::Completed);
        assert_eq!(summary.runtime_after.active_s3_transfers, 0);
        assert_eq!(gate.snapshot().active_s3_transfers, 0);
    }

    #[test]
    fn s3_transfer_job_reports_transfer_failure_and_releases_capacity() {
        let gate = std::sync::Arc::new(RemoteUploadAdmissionGate::new());

        let summary = gate.run_s3_transfer_job(
            transfer_job(RemoteUploadBackpressurePolicy::default()),
            || {
                assert_eq!(gate.snapshot().active_s3_transfers, 1);
                Err::<(), _>("multipart upload failed")
            },
        );

        assert_eq!(
            summary.outcome,
            RemoteUploadS3TransferJobOutcome::Failed {
                error: "multipart upload failed".to_string()
            }
        );
        assert_eq!(summary.runtime_after.active_s3_transfers, 0);
        assert_eq!(gate.snapshot().active_s3_transfers, 0);
    }

    #[test]
    fn s3_transfer_job_reports_admission_failure_without_running_transfer() {
        let gate = std::sync::Arc::new(RemoteUploadAdmissionGate::new());
        let policy = RemoteUploadBackpressurePolicy::default();
        let _first = gate
            .try_acquire_s3_transfer(policy, DaemonSsdPressure::AcceptingWrites)
            .expect("first transfer admitted");
        let _second = gate
            .try_acquire_s3_transfer(policy, DaemonSsdPressure::AcceptingWrites)
            .expect("second transfer admitted");

        let summary = gate
            .run_s3_transfer_job(transfer_job(policy), || -> Result<(), &'static str> {
                panic!("blocked job transfer must not execute")
            });

        let RemoteUploadS3TransferJobOutcome::NotAdmitted(decision) = summary.outcome else {
            panic!("expected admission failure");
        };
        assert_eq!(
            decision.action,
            RemoteUploadBackpressureAction::PauseNewTransfers
        );
        assert_eq!(summary.runtime_after.active_s3_transfers, 2);
        assert_eq!(gate.snapshot().active_s3_transfers, 2);
    }

    #[test]
    fn completed_s3_transfer_job_maps_to_complete_daemon_job_event() {
        let summary = RemoteUploadS3TransferJobSummary {
            job_id: "remote-upload-job-001".to_string(),
            object_store: "zymo_fecal_2025.05".to_string(),
            source_bytes: 42,
            outcome: RemoteUploadS3TransferJobOutcome::Completed,
            runtime_after: Default::default(),
        };

        let event = summary
            .daemon_job_event(
                "2026-07-09T14:00:00Z",
                "2026-07-09T14:00:30Z",
                Some("stephen".to_string()),
            )
            .expect("event");

        let DaemonJobEvent::Complete(job) = event else {
            panic!("expected complete job event");
        };
        assert_eq!(job.job_id.as_str(), "remote-upload-job-001");
        assert_eq!(job.kind, DaemonJobKind::RemoteUpload);
        assert_eq!(job.state, DaemonJobState::Complete);
        assert_eq!(job.progress.work_bytes_done, 42);
        assert_eq!(job.progress.work_bytes_total, 42);
        assert_eq!(job.failure_message, None);
    }

    #[test]
    fn paused_s3_transfer_job_maps_to_waiting_daemon_job_progress_event() {
        let summary = RemoteUploadS3TransferJobSummary {
            job_id: "remote-upload-job-002".to_string(),
            object_store: "zymo_fecal_2025.05".to_string(),
            source_bytes: 42,
            outcome: RemoteUploadS3TransferJobOutcome::NotAdmitted(admission_decision(
                RemoteUploadBackpressureAction::PauseNewTransfers,
            )),
            runtime_after: Default::default(),
        };

        let event = summary
            .daemon_job_event("2026-07-09T14:00:00Z", "2026-07-09T14:00:30Z", None)
            .expect("event");

        let DaemonJobEvent::Progress(job) = event else {
            panic!("expected progress job event");
        };
        assert_eq!(job.kind, DaemonJobKind::RemoteUpload);
        assert_eq!(job.state, DaemonJobState::Waiting);
        assert_eq!(job.progress.stage, "remote_s3_admission_wait");
        assert_eq!(
            job.progress.message.as_deref(),
            Some("Remote upload intake is temporarily paused.")
        );
        assert_eq!(job.failure_message, None);
    }

    #[test]
    fn rejected_s3_transfer_job_maps_to_failed_daemon_job_event() {
        let summary = RemoteUploadS3TransferJobSummary {
            job_id: "remote-upload-job-003".to_string(),
            object_store: "zymo_fecal_2025.05".to_string(),
            source_bytes: 42,
            outcome: RemoteUploadS3TransferJobOutcome::NotAdmitted(admission_decision(
                RemoteUploadBackpressureAction::RejectNewTransfers,
            )),
            runtime_after: Default::default(),
        };

        let event = summary
            .daemon_job_event("2026-07-09T14:00:00Z", "2026-07-09T14:00:30Z", None)
            .expect("event");

        let DaemonJobEvent::Failed(job) = event else {
            panic!("expected failed job event");
        };
        assert_eq!(job.kind, DaemonJobKind::RemoteUpload);
        assert_eq!(job.state, DaemonJobState::Failed);
        assert_eq!(job.progress.stage, "remote_s3_admission_rejected");
        assert_eq!(
            job.failure_message.as_deref(),
            Some("Remote upload intake is temporarily paused.")
        );
    }

    #[test]
    fn records_s3_transfer_job_summary_in_daemon_job_registry() {
        let root = temp_root("remote-upload-registry");
        let registry = FileBackedAdminJobRegistry::new(admin_job_registry_path(&root));
        let summary = RemoteUploadS3TransferJobSummary {
            job_id: "remote-upload-job-004".to_string(),
            object_store: "zymo_fecal_2025.05".to_string(),
            source_bytes: 42,
            outcome: RemoteUploadS3TransferJobOutcome::Completed,
            runtime_after: Default::default(),
        };

        let event = record_remote_upload_s3_transfer_job(
            &registry,
            &summary,
            "2026-07-09T14:00:00Z",
            "2026-07-09T14:01:00Z",
            Some("stephen".to_string()),
        )
        .expect("job recorded");

        let DaemonJobEvent::Complete(recorded_event_job) = event else {
            panic!("expected complete event");
        };
        assert_eq!(recorded_event_job.kind, DaemonJobKind::RemoteUpload);

        let status = registry
            .status(DaemonJobStatusRequest {
                job_id: recorded_event_job.job_id.clone(),
            })
            .expect("recorded job status");
        assert_eq!(status.job.kind, DaemonJobKind::RemoteUpload);
        assert_eq!(status.job.state, DaemonJobState::Complete);
        assert_eq!(status.job.progress.stage, "remote_s3_transfer_complete");
        assert_eq!(status.job.progress.work_bytes_total, 42);

        cleanup(&root);
    }

    #[test]
    fn transfer_worker_records_running_then_complete_and_releases_capacity() {
        let root = temp_root("remote-upload-worker-complete");
        let registry = FileBackedAdminJobRegistry::new(admin_job_registry_path(&root));
        let gate = std::sync::Arc::new(RemoteUploadAdmissionGate::new());
        let worker = RemoteUploadS3TransferWorker::new(std::sync::Arc::clone(&gate), &registry);
        let mut called = false;

        let report = worker
            .run(worker_request("remote-upload-job-005"), || {
                called = true;
                assert_eq!(gate.snapshot().active_s3_transfers, 1);
                Ok::<(), &'static str>(())
            })
            .expect("worker completed");

        assert!(called);
        let Some(DaemonJobEvent::Progress(running_job)) = report.running_event else {
            panic!("expected running progress event");
        };
        assert_eq!(running_job.state, DaemonJobState::Running);
        assert_eq!(running_job.progress.stage, "remote_s3_transfer_running");
        assert!(report.progress_events.is_empty());
        let DaemonJobEvent::Complete(final_job) = report.final_event else {
            panic!("expected complete final event");
        };
        assert_eq!(final_job.state, DaemonJobState::Complete);
        assert_eq!(report.runtime_after.active_s3_transfers, 0);
        assert_eq!(gate.snapshot().active_s3_transfers, 0);

        let status = registry
            .status(DaemonJobStatusRequest {
                job_id: final_job.job_id,
            })
            .expect("final status");
        assert_eq!(status.job.state, DaemonJobState::Complete);
        assert_eq!(status.job.progress.stage, "remote_s3_transfer_complete");

        cleanup(&root);
    }

    #[test]
    fn transfer_worker_records_waiting_without_running_transfer_when_admission_blocks() {
        let root = temp_root("remote-upload-worker-blocked");
        let registry = FileBackedAdminJobRegistry::new(admin_job_registry_path(&root));
        let gate = std::sync::Arc::new(RemoteUploadAdmissionGate::new());
        let policy = RemoteUploadBackpressurePolicy::default();
        let _first = gate
            .try_acquire_s3_transfer(policy, DaemonSsdPressure::AcceptingWrites)
            .expect("first transfer admitted");
        let _second = gate
            .try_acquire_s3_transfer(policy, DaemonSsdPressure::AcceptingWrites)
            .expect("second transfer admitted");
        let worker = RemoteUploadS3TransferWorker::new(std::sync::Arc::clone(&gate), &registry);

        let report = worker
            .run(
                worker_request("remote-upload-job-006"),
                || -> Result<(), &'static str> {
                    panic!("blocked worker transfer must not execute")
                },
            )
            .expect("blocked worker reported");

        assert_eq!(report.running_event, None);
        assert!(report.progress_events.is_empty());
        let DaemonJobEvent::Progress(final_job) = report.final_event else {
            panic!("expected waiting progress event");
        };
        assert_eq!(final_job.state, DaemonJobState::Waiting);
        assert_eq!(final_job.progress.stage, "remote_s3_admission_wait");
        assert_eq!(report.runtime_after.active_s3_transfers, 2);
        assert_eq!(gate.snapshot().active_s3_transfers, 2);

        let status = registry
            .status(DaemonJobStatusRequest {
                job_id: final_job.job_id,
            })
            .expect("waiting status");
        assert_eq!(status.job.state, DaemonJobState::Waiting);
        assert_eq!(status.job.progress.stage, "remote_s3_admission_wait");

        cleanup(&root);
    }

    #[test]
    fn transfer_worker_records_live_byte_progress_before_final_state() {
        let root = temp_root("remote-upload-worker-progress");
        let registry = FileBackedAdminJobRegistry::new(admin_job_registry_path(&root));
        let gate = std::sync::Arc::new(RemoteUploadAdmissionGate::new());
        let worker = RemoteUploadS3TransferWorker::new(std::sync::Arc::clone(&gate), &registry);

        let report = worker
            .run_with_progress(worker_request("remote-upload-job-007"), |progress| {
                let event = progress
                    .record_progress(RemoteUploadS3TransferProgressUpdate {
                        bytes_done: 21,
                        updated_at_utc: "2026-07-09T14:10:30Z".to_string(),
                        telemetry: None,
                        message: Some("uploaded 21 bytes".to_string()),
                    })
                    .expect("progress recorded");

                let DaemonJobEvent::Progress(job) = event else {
                    panic!("expected progress event");
                };
                assert_eq!(job.state, DaemonJobState::Running);
                assert_eq!(job.progress.work_bytes_done, 21);
                assert_eq!(job.progress.work_bytes_total, 42);
                assert_eq!(job.progress.message.as_deref(), Some("uploaded 21 bytes"));
                Ok::<(), &'static str>(())
            })
            .expect("worker completed");

        assert_eq!(report.progress_events.len(), 1);
        let DaemonJobEvent::Progress(progress_job) = &report.progress_events[0] else {
            panic!("expected progress event in report");
        };
        assert_eq!(progress_job.progress.work_bytes_done, 21);
        assert_eq!(progress_job.updated_at_utc, "2026-07-09T14:10:30Z");

        let DaemonJobEvent::Complete(final_job) = report.final_event else {
            panic!("expected complete final event");
        };
        let status = registry
            .status(DaemonJobStatusRequest {
                job_id: final_job.job_id,
            })
            .expect("final status");
        assert_eq!(status.job.state, DaemonJobState::Complete);
        assert_eq!(status.job.progress.work_bytes_done, 42);
        assert_eq!(report.runtime_after.active_s3_transfers, 0);

        cleanup(&root);
    }

    #[test]
    fn transfer_worker_records_structured_remote_upload_telemetry_in_progress_message() {
        let root = temp_root("remote-upload-worker-telemetry-progress");
        let registry = FileBackedAdminJobRegistry::new(admin_job_registry_path(&root));
        let gate = std::sync::Arc::new(RemoteUploadAdmissionGate::new());
        let worker = RemoteUploadS3TransferWorker::new(std::sync::Arc::clone(&gate), &registry);

        let report = worker
            .run_with_progress(worker_request("remote-upload-job-032"), |progress| {
                progress
                    .record_progress(RemoteUploadS3TransferProgressUpdate {
                        bytes_done: 21,
                        updated_at_utc: "2026-07-09T14:10:30Z".to_string(),
                        telemetry: Some(RemoteUploadProgressTelemetry {
                            source_scan_count: Some(246),
                            staged_bytes: Some(16),
                            s3_bytes_per_second: Some(8),
                            ssd_queue_depth: Some(3),
                            hdd_landing_queue_depth: Some(5),
                            active_hdd_writers: Some(7),
                            verification_state: Some("pending".to_string()),
                            session_renewal_status: Some("renewal_not_required".to_string()),
                        }),
                        message: Some("uploaded 21 bytes".to_string()),
                    })
                    .expect("progress recorded");
                Ok::<(), &'static str>(())
            })
            .expect("worker completed");

        let DaemonJobEvent::Progress(progress_job) = &report.progress_events[0] else {
            panic!("expected progress event");
        };
        assert_eq!(
            progress_job.progress.message.as_deref(),
            Some(
                "uploaded 21 bytes | source_files=246 staged_bytes=16 s3_bytes_per_second=8 ssd_queue_depth=3 hdd_landing_queue_depth=5 active_hdd_writers=7 verification_state=pending session_renewal_status=renewal_not_required"
            )
        );

        cleanup(&root);
    }

    #[test]
    fn transfer_worker_derives_s3_rate_telemetry_from_progress_timestamps() {
        let root = temp_root("remote-upload-worker-rate-progress");
        let registry = FileBackedAdminJobRegistry::new(admin_job_registry_path(&root));
        let gate = std::sync::Arc::new(RemoteUploadAdmissionGate::new());
        let worker = RemoteUploadS3TransferWorker::new(std::sync::Arc::clone(&gate), &registry);

        let report = worker
            .run_with_progress(worker_request("remote-upload-job-033"), |progress| {
                progress
                    .record_progress(RemoteUploadS3TransferProgressUpdate {
                        bytes_done: 40,
                        updated_at_utc: "2026-07-09T14:10:15Z".to_string(),
                        telemetry: None,
                        message: Some("uploaded 40 bytes".to_string()),
                    })
                    .expect("progress recorded");
                Ok::<(), &'static str>(())
            })
            .expect("worker completed");

        let DaemonJobEvent::Progress(progress_job) = &report.progress_events[0] else {
            panic!("expected progress event");
        };
        assert_eq!(
            progress_job.progress.message.as_deref(),
            Some("uploaded 40 bytes | s3_bytes_per_second=4")
        );

        cleanup(&root);
    }

    #[test]
    fn transfer_worker_records_queue_depth_telemetry_from_gate_snapshot() {
        let root = temp_root("remote-upload-worker-queue-progress");
        let registry = FileBackedAdminJobRegistry::new(admin_job_registry_path(&root));
        let gate = std::sync::Arc::new(RemoteUploadAdmissionGate::new());
        gate.observe_queue_depths(RemoteUploadQueueDepths {
            ssd_stage_queue_depth: 1,
            hdd_landing_queue_depth: 2,
            verification_queue_depth: 0,
        });
        let worker = RemoteUploadS3TransferWorker::new(std::sync::Arc::clone(&gate), &registry);

        let report = worker
            .run_with_progress(worker_request("remote-upload-job-034"), |progress| {
                progress
                    .record_progress(RemoteUploadS3TransferProgressUpdate {
                        bytes_done: 20,
                        updated_at_utc: "2026-07-09T14:10:15Z".to_string(),
                        telemetry: None,
                        message: Some("uploaded 20 bytes".to_string()),
                    })
                    .expect("progress recorded");
                Ok::<(), &'static str>(())
            })
            .expect("worker completed");

        let DaemonJobEvent::Progress(progress_job) = &report.progress_events[0] else {
            panic!("expected progress event");
        };
        assert_eq!(
            progress_job.progress.message.as_deref(),
            Some(
                "uploaded 20 bytes | s3_bytes_per_second=2 ssd_queue_depth=1 hdd_landing_queue_depth=2"
            )
        );

        cleanup(&root);
    }

    #[test]
    fn transfer_worker_records_active_hdd_writer_and_verification_telemetry() {
        let root = temp_root("remote-upload-worker-writer-verification-progress");
        let registry = FileBackedAdminJobRegistry::new(admin_job_registry_path(&root));
        let gate = std::sync::Arc::new(RemoteUploadAdmissionGate::new());
        gate.observe_ingest_telemetry(DaemonIngestTelemetry {
            queue_depths: DaemonIngestQueueDepths {
                verification: 3,
                ..DaemonIngestQueueDepths::default()
            },
            workers: DaemonIngestWorkerTelemetry {
                hdd_write: DaemonIngestWorkerActivity { active: 2, idle: 5 },
                ..DaemonIngestWorkerTelemetry::default()
            },
            ..DaemonIngestTelemetry::default()
        });
        let worker = RemoteUploadS3TransferWorker::new(std::sync::Arc::clone(&gate), &registry);

        let report = worker
            .run_with_progress(worker_request("remote-upload-job-035"), |progress| {
                progress
                    .record_progress(RemoteUploadS3TransferProgressUpdate {
                        bytes_done: 20,
                        updated_at_utc: "2026-07-09T14:10:15Z".to_string(),
                        telemetry: None,
                        message: Some("uploaded 20 bytes".to_string()),
                    })
                    .expect("progress recorded");
                Ok::<(), &'static str>(())
            })
            .expect("worker completed");

        let DaemonJobEvent::Progress(progress_job) = &report.progress_events[0] else {
            panic!("expected progress event");
        };
        assert_eq!(
            progress_job.progress.message.as_deref(),
            Some(
                "uploaded 20 bytes | s3_bytes_per_second=2 active_hdd_writers=2 verification_state=pending"
            )
        );

        cleanup(&root);
    }

    #[test]
    fn transfer_worker_runs_typed_byte_transfer_under_admission_gate() {
        let root = temp_root("remote-upload-worker-byte-transfer");
        let registry = FileBackedAdminJobRegistry::new(admin_job_registry_path(&root));
        let gate = std::sync::Arc::new(RemoteUploadAdmissionGate::new());
        let worker = RemoteUploadS3TransferWorker::new(std::sync::Arc::clone(&gate), &registry);
        let transfer = RecordingByteTransfer { bytes_done: 32 };

        let report = worker
            .run_byte_transfer(worker_request("remote-upload-job-008"), &transfer)
            .expect("byte transfer completed");

        assert_eq!(report.progress_events.len(), 1);
        let DaemonJobEvent::Progress(progress_job) = &report.progress_events[0] else {
            panic!("expected progress event");
        };
        assert_eq!(progress_job.progress.work_bytes_done, 32);
        assert_eq!(
            progress_job.progress.message.as_deref(),
            Some("typed byte transfer copied 32 bytes | s3_bytes_per_second=1")
        );
        let DaemonJobEvent::Complete(final_job) = report.final_event else {
            panic!("expected complete final event");
        };
        assert_eq!(final_job.progress.stage, "remote_s3_transfer_complete");
        assert_eq!(gate.snapshot().active_s3_transfers, 0);

        cleanup(&root);
    }

    #[test]
    fn transfer_worker_records_typed_byte_transfer_error_and_releases_capacity() {
        let root = temp_root("remote-upload-worker-byte-transfer-error");
        let registry = FileBackedAdminJobRegistry::new(admin_job_registry_path(&root));
        let gate = std::sync::Arc::new(RemoteUploadAdmissionGate::new());
        let worker = RemoteUploadS3TransferWorker::new(std::sync::Arc::clone(&gate), &registry);
        let transfer = FailingByteTransfer;

        let report = worker
            .run_byte_transfer(worker_request("remote-upload-job-009"), &transfer)
            .expect("byte transfer failure recorded");

        let DaemonJobEvent::Failed(final_job) = report.final_event else {
            panic!("expected failed final event");
        };
        assert_eq!(final_job.state, DaemonJobState::Failed);
        assert_eq!(final_job.progress.stage, "remote_s3_transfer_failed");
        assert_eq!(
            final_job.failure_message.as_deref(),
            Some("object service rejected part")
        );
        assert_eq!(gate.snapshot().active_s3_transfers, 0);

        cleanup(&root);
    }

    #[test]
    fn transfer_worker_runs_cleanup_plan_after_transfer_failure() {
        let root = temp_root("remote-upload-worker-cleanup-after-failure");
        let registry = FileBackedAdminJobRegistry::new(admin_job_registry_path(&root));
        let gate = std::sync::Arc::new(RemoteUploadAdmissionGate::new());
        let worker = RemoteUploadS3TransferWorker::new(std::sync::Arc::clone(&gate), &registry);
        let cleanup_worker = RecordingCleanupWorker::default();
        let cleanup_plan = cleanup_plan_for_job("remote-upload-job-030");

        let report = worker
            .run_with_cleanup_on_failure(
                worker_request("remote-upload-job-030"),
                cleanup_plan,
                &cleanup_worker,
                |_progress| Err::<(), _>("multipart upload interrupted"),
            )
            .expect("failure cleanup reported");

        let DaemonJobEvent::Failed(final_job) = report.final_event else {
            panic!("expected failed final event");
        };
        assert_eq!(final_job.progress.stage, "remote_s3_transfer_failed");
        assert_eq!(gate.snapshot().active_s3_transfers, 0);
        let cleanup_report = report.cleanup_report.expect("cleanup report");
        assert!(!cleanup_report.plan.resumable);
        assert!(cleanup_report.completed());
        assert_eq!(cleanup_report.action_reports.len(), 2);
        assert_eq!(
            cleanup_worker.calls.borrow().as_slice(),
            [
                RemoteUploadCancellationCleanupScope::PartialSsdStage,
                RemoteUploadCancellationCleanupScope::FailedMultipartUpload,
            ]
        );

        cleanup(&root);
    }

    #[test]
    fn multipart_fake_records_part_progress_and_aborts_after_interruption() {
        let root = temp_root("remote-upload-worker-multipart-interrupted");
        let stage_root = root.join("ssd-stage");
        std::fs::create_dir_all(stage_root.join("remote-upload-job-036"))
            .expect("stage dir created");
        std::fs::write(
            stage_root.join("remote-upload-job-036").join("part-0002"),
            b"partial",
        )
        .expect("stage part written");
        let registry = FileBackedAdminJobRegistry::new(admin_job_registry_path(&root));
        let gate = std::sync::Arc::new(RemoteUploadAdmissionGate::new());
        let worker = RemoteUploadS3TransferWorker::new(std::sync::Arc::clone(&gate), &registry);
        let runner = FakeRemoteUploadCommandRunner::default();
        let cleanup_worker = RemoteUploadCancellationCleanupRuntime::new(
            cleanup_runtime_config(
                &root,
                Some(RemoteUploadMultipartAbortConfig {
                    program: "aws".to_string(),
                    endpoint_url: "http://127.0.0.1:3900".to_string(),
                    bucket: "dos-zymo-fecal-2025-05".to_string(),
                    object_key: "raw/PAW10254/reads.fastq.gz".to_string(),
                    environment: vec![(
                        "AWS_SESSION_TOKEN".to_string(),
                        "temporary-session".to_string(),
                    )],
                }),
            ),
            &runner,
        );
        let cleanup_plan =
            plan_remote_upload_cancellation_cleanup(RemoteUploadCancellationCleanupRequest {
                job_id: "remote-upload-job-036".to_string(),
                object_store: "zymo_fecal_2025.05".to_string(),
                source_bytes: 16,
                staged_object_prefix: Some("remote-upload-job-036".to_string()),
                multipart_upload_id: Some("multipart-036".to_string()),
                session_id: None,
                pairing_id: None,
                browser_handoff_id: None,
                reason: Some("multipart transfer interrupted".to_string()),
            });

        let report = worker
            .run_with_cleanup_on_failure(
                worker_request("remote-upload-job-036"),
                cleanup_plan,
                &cleanup_worker,
                |progress| {
                    progress
                        .record_progress(RemoteUploadS3TransferProgressUpdate {
                            bytes_done: 8,
                            updated_at_utc: "2026-07-09T14:10:20Z".to_string(),
                            telemetry: None,
                            message: Some("multipart part 1 uploaded".to_string()),
                        })
                        .expect("first part progress recorded");
                    progress
                        .record_progress(RemoteUploadS3TransferProgressUpdate {
                            bytes_done: 12,
                            updated_at_utc: "2026-07-09T14:10:30Z".to_string(),
                            telemetry: None,
                            message: Some("multipart part 2 interrupted".to_string()),
                        })
                        .expect("second part progress recorded");
                    Err::<(), _>("multipart transfer interrupted after part 2")
                },
            )
            .expect("interrupted multipart upload reported");

        assert_eq!(report.progress_events.len(), 2);
        let DaemonJobEvent::Progress(second_part) = &report.progress_events[1] else {
            panic!("expected multipart progress event");
        };
        assert_eq!(second_part.progress.work_bytes_done, 12);
        assert!(second_part
            .progress
            .message
            .as_deref()
            .expect("progress message")
            .contains("multipart part 2 interrupted"));
        let DaemonJobEvent::Failed(final_job) = report.final_event else {
            panic!("expected failed final event");
        };
        assert_eq!(final_job.progress.stage, "remote_s3_transfer_failed");
        assert_eq!(
            final_job.failure_message.as_deref(),
            Some("multipart transfer interrupted after part 2")
        );
        let cleanup_report = report.cleanup_report.expect("cleanup report");
        assert!(cleanup_report.completed());
        assert_eq!(cleanup_report.action_reports.len(), 2);
        assert!(!stage_root.join("remote-upload-job-036").exists());
        let calls = runner.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].args,
            [
                "--endpoint-url",
                "http://127.0.0.1:3900",
                "s3api",
                "abort-multipart-upload",
                "--bucket",
                "dos-zymo-fecal-2025-05",
                "--key",
                "raw/PAW10254/reads.fastq.gz",
                "--upload-id",
                "multipart-036"
            ]
        );
        assert_eq!(
            calls[0].environment,
            [(
                "AWS_SESSION_TOKEN".to_string(),
                "temporary-session".to_string()
            )]
        );
        assert_eq!(gate.snapshot().active_s3_transfers, 0);

        cleanup(&root);
    }

    #[test]
    fn failed_paired_upload_cleans_session_state_after_renewal_progress() {
        let root = temp_root("remote-upload-paired-session-cleanup");
        let registry = FileBackedAdminJobRegistry::new(admin_job_registry_path(&root));
        let gate = std::sync::Arc::new(RemoteUploadAdmissionGate::new());
        let worker = RemoteUploadS3TransferWorker::new(std::sync::Arc::clone(&gate), &registry);
        let session_root = root.join("sessions");
        std::fs::create_dir_all(&session_root).expect("session dir created");
        std::fs::write(session_root.join("session-1"), b"paired session").expect("session written");
        let runner = FakeRemoteUploadCommandRunner::default();
        let cleanup_worker = RemoteUploadCancellationCleanupRuntime::new(
            cleanup_runtime_config(&root, None),
            &runner,
        );
        let cleanup_plan =
            plan_remote_upload_cancellation_cleanup(RemoteUploadCancellationCleanupRequest {
                job_id: "remote-upload-job-033".to_string(),
                object_store: "zymo_fecal_2025.05".to_string(),
                source_bytes: 42,
                staged_object_prefix: None,
                multipart_upload_id: None,
                session_id: Some("session-1".to_string()),
                pairing_id: None,
                browser_handoff_id: None,
                reason: Some("paired CLI agent disconnected during active upload".to_string()),
            });

        let report = worker
            .run_with_cleanup_on_failure(
                worker_request("remote-upload-job-033"),
                cleanup_plan,
                &cleanup_worker,
                |progress| {
                    progress
                        .record_progress(RemoteUploadS3TransferProgressUpdate {
                            bytes_done: 21,
                            updated_at_utc: "2026-07-09T14:10:30Z".to_string(),
                            telemetry: Some(RemoteUploadProgressTelemetry {
                                session_renewal_status: Some("renewed_during_upload".to_string()),
                                ..RemoteUploadProgressTelemetry::default()
                            }),
                            message: Some("uploaded 21 bytes before reconnect".to_string()),
                        })
                        .expect("progress recorded");
                    Err::<(), _>("paired CLI agent disconnected")
                },
            )
            .expect("failure cleanup reported");

        let DaemonJobEvent::Progress(progress_job) = &report.progress_events[0] else {
            panic!("expected progress event");
        };
        assert!(progress_job
            .progress
            .message
            .as_deref()
            .expect("progress message")
            .contains("session_renewal_status=renewed_during_upload"));

        let DaemonJobEvent::Failed(final_job) = report.final_event else {
            panic!("expected failed final event");
        };
        assert_eq!(final_job.progress.stage, "remote_s3_transfer_failed");
        assert_eq!(
            final_job.failure_message.as_deref(),
            Some("paired CLI agent disconnected")
        );
        let cleanup_report = report.cleanup_report.expect("cleanup report");
        assert!(cleanup_report.completed());
        assert_eq!(cleanup_report.action_reports.len(), 1);
        assert_eq!(
            cleanup_report.action_reports[0].action.scope,
            RemoteUploadCancellationCleanupScope::AbandonedSession
        );
        assert!(!session_root.join("session-1").exists());

        let status = registry
            .status(DaemonJobStatusRequest {
                job_id: final_job.job_id,
            })
            .expect("final status");
        assert_eq!(status.job.state, DaemonJobState::Failed);
        assert_eq!(gate.snapshot().active_s3_transfers, 0);
        assert!(runner.calls.borrow().is_empty());

        cleanup(&root);
    }

    #[test]
    fn transfer_worker_skips_cleanup_plan_after_success() {
        let root = temp_root("remote-upload-worker-cleanup-after-success");
        let registry = FileBackedAdminJobRegistry::new(admin_job_registry_path(&root));
        let gate = std::sync::Arc::new(RemoteUploadAdmissionGate::new());
        let worker = RemoteUploadS3TransferWorker::new(std::sync::Arc::clone(&gate), &registry);
        let cleanup_worker = RecordingCleanupWorker::default();
        let cleanup_plan = cleanup_plan_for_job("remote-upload-job-031");

        let report = worker
            .run_with_cleanup_on_failure(
                worker_request("remote-upload-job-031"),
                cleanup_plan,
                &cleanup_worker,
                |_progress| Ok::<(), &'static str>(()),
            )
            .expect("success reported");

        let DaemonJobEvent::Complete(final_job) = report.final_event else {
            panic!("expected complete final event");
        };
        assert_eq!(final_job.progress.stage, "remote_s3_transfer_complete");
        assert_eq!(report.cleanup_report, None);
        assert!(cleanup_worker.calls.borrow().is_empty());
        assert_eq!(gate.snapshot().active_s3_transfers, 0);

        cleanup(&root);
    }

    #[test]
    fn aws_cli_byte_transfer_runs_redacted_command_and_records_completion_progress() {
        let root = temp_root("remote-upload-aws-cli-transfer");
        let registry = FileBackedAdminJobRegistry::new(admin_job_registry_path(&root));
        let gate = std::sync::Arc::new(RemoteUploadAdmissionGate::new());
        let worker = RemoteUploadS3TransferWorker::new(std::sync::Arc::clone(&gate), &registry);
        let runner = FakeRemoteUploadCommandRunner::default();
        let plan = RemoteUploadAwsCliTransferPlan::new(
            "aws",
            vec![
                "--endpoint-url".to_string(),
                "http://127.0.0.1:3900".to_string(),
                "s3".to_string(),
                "cp".to_string(),
                "/data/reads.fastq.gz".to_string(),
                "s3://dos-zymo/raw/reads.fastq.gz".to_string(),
            ],
            42,
            "2026-07-09T14:31:00Z",
        )
        .with_display_args(vec![
            "--endpoint-url".to_string(),
            "<redacted>".to_string(),
            "s3".to_string(),
            "cp".to_string(),
            "/data/reads.fastq.gz".to_string(),
            "s3://dos-zymo/raw/reads.fastq.gz".to_string(),
        ])
        .with_environment(vec![(
            "AWS_ACCESS_KEY_ID".to_string(),
            "AKIAEXAMPLE".to_string(),
        )])
        .with_progress_message("aws CLI transfer copied 42 bytes");
        let transfer = RemoteUploadAwsCliByteTransfer::new(plan, &runner);

        let report = worker
            .run_byte_transfer(worker_request("remote-upload-job-010"), &transfer)
            .expect("aws transfer completed");

        let calls = runner.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].program, "aws");
        assert_eq!(calls[0].args[1], "http://127.0.0.1:3900");
        assert_eq!(calls[0].display_args[1], "<redacted>");
        assert_eq!(
            calls[0].environment,
            [("AWS_ACCESS_KEY_ID".to_string(), "AKIAEXAMPLE".to_string())]
        );
        assert_eq!(report.progress_events.len(), 1);
        let DaemonJobEvent::Progress(progress_job) = &report.progress_events[0] else {
            panic!("expected completion progress event");
        };
        assert_eq!(progress_job.progress.work_bytes_done, 42);
        assert_eq!(
            progress_job.progress.message.as_deref(),
            Some("aws CLI transfer copied 42 bytes")
        );
        let DaemonJobEvent::Complete(final_job) = report.final_event else {
            panic!("expected complete final event");
        };
        assert_eq!(final_job.state, DaemonJobState::Complete);
        assert_eq!(gate.snapshot().active_s3_transfers, 0);

        cleanup(&root);
    }

    #[test]
    fn aws_cli_byte_transfer_failure_records_failed_job_and_releases_capacity() {
        let root = temp_root("remote-upload-aws-cli-transfer-failed");
        let registry = FileBackedAdminJobRegistry::new(admin_job_registry_path(&root));
        let gate = std::sync::Arc::new(RemoteUploadAdmissionGate::new());
        let worker = RemoteUploadS3TransferWorker::new(std::sync::Arc::clone(&gate), &registry);
        let runner = FakeRemoteUploadCommandRunner {
            fail: true,
            ..FakeRemoteUploadCommandRunner::default()
        };
        let plan = RemoteUploadAwsCliTransferPlan::new(
            "aws",
            vec!["s3".to_string(), "cp".to_string()],
            42,
            "2026-07-09T14:31:00Z",
        );
        let transfer = RemoteUploadAwsCliByteTransfer::new(plan, &runner);

        let report = worker
            .run_byte_transfer(worker_request("remote-upload-job-011"), &transfer)
            .expect("aws failure recorded");

        let DaemonJobEvent::Failed(final_job) = report.final_event else {
            panic!("expected failed final event");
        };
        assert_eq!(final_job.state, DaemonJobState::Failed);
        assert!(final_job
            .failure_message
            .as_deref()
            .expect("failure message")
            .contains("aws s3 cp exited with status exit status: 1"));
        assert!(report.progress_events.is_empty());
        assert_eq!(gate.snapshot().active_s3_transfers, 0);

        cleanup(&root);
    }

    #[test]
    fn easyconnect_aws_cli_upload_job_runs_transfer_and_persists_final_status() {
        let root = temp_root("remote-upload-easyconnect-aws-cli-job");
        let registry = FileBackedAdminJobRegistry::new(admin_job_registry_path(&root));
        let gate = std::sync::Arc::new(RemoteUploadAdmissionGate::new());
        let runner = FakeRemoteUploadCommandRunner::default();

        let report = run_remote_easyconnect_aws_cli_upload_job(
            &registry,
            std::sync::Arc::clone(&gate),
            &runner,
            easyconnect_aws_cli_job_request("remote-upload-job-012"),
        )
        .expect("easyconnect aws cli upload job completed");

        let calls = runner.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].program, "aws");
        assert_eq!(calls[0].args[0], "s3");
        assert_eq!(calls[0].display_args[2], "<source-redacted>");
        assert_eq!(report.progress_events.len(), 1);
        let DaemonJobEvent::Progress(progress_job) = &report.progress_events[0] else {
            panic!("expected progress event");
        };
        let progress_message = progress_job
            .progress
            .message
            .as_deref()
            .expect("progress message");
        assert!(progress_message.contains("source_files=1"));
        assert!(progress_message.contains("staged_bytes=42"));
        let DaemonJobEvent::Complete(final_job) = report.final_event else {
            panic!("expected complete final event");
        };
        let status = registry
            .status(DaemonJobStatusRequest {
                job_id: final_job.job_id,
            })
            .expect("final status");
        assert_eq!(status.job.kind, DaemonJobKind::RemoteUpload);
        assert_eq!(status.job.state, DaemonJobState::Complete);
        assert_eq!(status.job.progress.work_bytes_done, 42);
        assert_eq!(status.job.actor.as_deref(), Some("stephen"));
        assert_eq!(gate.snapshot().active_s3_transfers, 0);

        cleanup(&root);
    }

    #[test]
    fn cancellation_cleanup_plan_requires_partial_stage_and_multipart_cleanup() {
        let plan =
            plan_remote_upload_cancellation_cleanup(RemoteUploadCancellationCleanupRequest {
                job_id: "remote-upload-job-020".to_string(),
                object_store: "zymo_fecal_2025.05".to_string(),
                source_bytes: 42,
                staged_object_prefix: Some("ssd/remote-upload-job-020".to_string()),
                multipart_upload_id: Some("multipart-123".to_string()),
                session_id: Some("session-1".to_string()),
                pairing_id: Some("pairing-1".to_string()),
                browser_handoff_id: Some("handoff-1".to_string()),
                reason: Some("user cancelled upload".to_string()),
            });

        assert!(plan.requires_work());
        assert!(plan.requires_multipart_abort());
        assert!(!plan.resumable);
        assert_eq!(plan.actions.len(), 5);
        assert_eq!(
            plan.actions[0].scope,
            RemoteUploadCancellationCleanupScope::PartialSsdStage
        );
        assert!(plan.actions[0].required);
        assert_eq!(
            plan.actions[1].scope,
            RemoteUploadCancellationCleanupScope::FailedMultipartUpload
        );
        assert!(plan.actions[1].required);
        assert!(plan.actions[1].reason.contains("user cancelled upload"));
    }

    #[test]
    fn cancellation_cleanup_plan_can_be_resumable_when_only_session_state_remains() {
        let plan =
            plan_remote_upload_cancellation_cleanup(RemoteUploadCancellationCleanupRequest {
                job_id: "remote-upload-job-021".to_string(),
                object_store: "zymo_fecal_2025.05".to_string(),
                source_bytes: 0,
                staged_object_prefix: None,
                multipart_upload_id: None,
                session_id: Some("session-1".to_string()),
                pairing_id: Some(" ".to_string()),
                browser_handoff_id: Some("handoff-1".to_string()),
                reason: None,
            });

        assert!(plan.requires_work());
        assert!(!plan.requires_multipart_abort());
        assert!(plan.resumable);
        assert_eq!(plan.actions.len(), 2);
        assert_eq!(
            plan.actions[0].scope,
            RemoteUploadCancellationCleanupScope::AbandonedSession
        );
        assert_eq!(
            plan.actions[1].scope,
            RemoteUploadCancellationCleanupScope::InterruptedBrowserHandoff
        );
        assert!(plan.actions.iter().all(|action| !action.required));
    }

    #[test]
    fn cancellation_cleanup_worker_runs_all_actions_and_reports_success() {
        let worker = RecordingCleanupWorker::default();
        let plan =
            plan_remote_upload_cancellation_cleanup(RemoteUploadCancellationCleanupRequest {
                job_id: "remote-upload-job-022".to_string(),
                object_store: "zymo_fecal_2025.05".to_string(),
                source_bytes: 42,
                staged_object_prefix: Some("ssd/remote-upload-job-022".to_string()),
                multipart_upload_id: Some("multipart-123".to_string()),
                session_id: None,
                pairing_id: None,
                browser_handoff_id: None,
                reason: Some("operator cancelled upload".to_string()),
            });

        let report = run_remote_upload_cancellation_cleanup(plan, &worker);

        assert!(report.completed());
        assert!(report.failed_actions().is_empty());
        assert_eq!(
            worker.calls.borrow().as_slice(),
            [
                RemoteUploadCancellationCleanupScope::PartialSsdStage,
                RemoteUploadCancellationCleanupScope::FailedMultipartUpload,
            ]
        );
        assert_eq!(report.action_reports.len(), 2);
        assert_eq!(
            report.action_reports[0].state,
            RemoteUploadCancellationCleanupActionState::Complete
        );
    }

    #[test]
    fn cancellation_cleanup_worker_continues_after_failed_action() {
        let worker = RecordingCleanupWorker {
            fail_scope: Some(RemoteUploadCancellationCleanupScope::FailedMultipartUpload),
            ..RecordingCleanupWorker::default()
        };
        let plan =
            plan_remote_upload_cancellation_cleanup(RemoteUploadCancellationCleanupRequest {
                job_id: "remote-upload-job-023".to_string(),
                object_store: "zymo_fecal_2025.05".to_string(),
                source_bytes: 42,
                staged_object_prefix: Some("ssd/remote-upload-job-023".to_string()),
                multipart_upload_id: Some("multipart-456".to_string()),
                session_id: Some("session-1".to_string()),
                pairing_id: None,
                browser_handoff_id: None,
                reason: Some("network interrupted upload".to_string()),
            });

        let report = run_remote_upload_cancellation_cleanup(plan, &worker);
        let failed_actions = report.failed_actions();

        assert!(!report.completed());
        assert_eq!(failed_actions.len(), 1);
        assert_eq!(
            failed_actions[0].action.scope,
            RemoteUploadCancellationCleanupScope::FailedMultipartUpload
        );
        assert!(failed_actions[0]
            .error
            .as_deref()
            .expect("failed action has an error")
            .contains("cleanup failed"));
        assert_eq!(
            worker.calls.borrow().as_slice(),
            [
                RemoteUploadCancellationCleanupScope::PartialSsdStage,
                RemoteUploadCancellationCleanupScope::FailedMultipartUpload,
                RemoteUploadCancellationCleanupScope::AbandonedSession,
            ]
        );
        assert_eq!(
            report.action_reports[2].state,
            RemoteUploadCancellationCleanupActionState::Complete
        );
    }

    #[test]
    fn concrete_cleanup_worker_removes_managed_stage_and_state_records() {
        let root = temp_root("remote-upload-concrete-cleanup");
        let stage_root = root.join("ssd-stage");
        let session_root = root.join("sessions");
        let pairing_root = root.join("pairings");
        let handoff_root = root.join("handoffs");
        std::fs::create_dir_all(stage_root.join("remote-upload-job-024"))
            .expect("stage dir created");
        std::fs::write(
            stage_root.join("remote-upload-job-024").join("part"),
            b"partial",
        )
        .expect("stage file written");
        std::fs::create_dir_all(&session_root).expect("session dir created");
        std::fs::create_dir_all(&pairing_root).expect("pairing dir created");
        std::fs::create_dir_all(&handoff_root).expect("handoff dir created");
        std::fs::write(session_root.join("session-1"), b"session").expect("session written");
        std::fs::write(pairing_root.join("pairing-1"), b"pairing").expect("pairing written");
        std::fs::write(handoff_root.join("handoff-1"), b"handoff").expect("handoff written");
        let runner = FakeRemoteUploadCommandRunner::default();
        let worker = RemoteUploadCancellationCleanupRuntime::new(
            cleanup_runtime_config(&root, None),
            &runner,
        );
        let plan =
            plan_remote_upload_cancellation_cleanup(RemoteUploadCancellationCleanupRequest {
                job_id: "remote-upload-job-024".to_string(),
                object_store: "zymo_fecal_2025.05".to_string(),
                source_bytes: 42,
                staged_object_prefix: Some("remote-upload-job-024".to_string()),
                multipart_upload_id: None,
                session_id: Some("session-1".to_string()),
                pairing_id: Some("pairing-1".to_string()),
                browser_handoff_id: Some("handoff-1".to_string()),
                reason: Some("user cancelled upload".to_string()),
            });

        let report = run_remote_upload_cancellation_cleanup(plan, &worker);

        assert!(report.completed());
        assert!(!stage_root.join("remote-upload-job-024").exists());
        assert!(!session_root.join("session-1").exists());
        assert!(!pairing_root.join("pairing-1").exists());
        assert!(!handoff_root.join("handoff-1").exists());
        assert!(runner.calls.borrow().is_empty());

        cleanup(&root);
    }

    #[test]
    fn concrete_cleanup_worker_aborts_multipart_upload_with_aws_cli() {
        let root = temp_root("remote-upload-concrete-multipart-cleanup");
        let runner = FakeRemoteUploadCommandRunner::default();
        let worker = RemoteUploadCancellationCleanupRuntime::new(
            cleanup_runtime_config(
                &root,
                Some(RemoteUploadMultipartAbortConfig {
                    program: "aws".to_string(),
                    endpoint_url: "http://127.0.0.1:3900".to_string(),
                    bucket: "dos-zymo".to_string(),
                    object_key: "raw/reads.fastq.gz".to_string(),
                    environment: vec![("AWS_ACCESS_KEY_ID".to_string(), "AKIAEXAMPLE".to_string())],
                }),
            ),
            &runner,
        );
        let plan =
            plan_remote_upload_cancellation_cleanup(RemoteUploadCancellationCleanupRequest {
                job_id: "remote-upload-job-025".to_string(),
                object_store: "zymo_fecal_2025.05".to_string(),
                source_bytes: 42,
                staged_object_prefix: None,
                multipart_upload_id: Some("upload-123".to_string()),
                session_id: None,
                pairing_id: None,
                browser_handoff_id: None,
                reason: Some("transfer failed".to_string()),
            });

        let report = run_remote_upload_cancellation_cleanup(plan, &worker);

        assert!(report.completed());
        let calls = runner.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].program, "aws");
        assert_eq!(
            calls[0].args,
            [
                "--endpoint-url",
                "http://127.0.0.1:3900",
                "s3api",
                "abort-multipart-upload",
                "--bucket",
                "dos-zymo",
                "--key",
                "raw/reads.fastq.gz",
                "--upload-id",
                "upload-123"
            ]
        );
        assert_eq!(
            calls[0].environment,
            [("AWS_ACCESS_KEY_ID".to_string(), "AKIAEXAMPLE".to_string())]
        );

        cleanup(&root);
    }

    #[test]
    fn concrete_cleanup_worker_rejects_path_escape_identifiers() {
        let root = temp_root("remote-upload-concrete-cleanup-escape");
        std::fs::create_dir_all(root.join("ssd-stage")).expect("stage dir created");
        let outside = root.join("outside");
        std::fs::write(&outside, b"must remain").expect("outside file written");
        let runner = FakeRemoteUploadCommandRunner::default();
        let worker = RemoteUploadCancellationCleanupRuntime::new(
            cleanup_runtime_config(&root, None),
            &runner,
        );
        let plan =
            plan_remote_upload_cancellation_cleanup(RemoteUploadCancellationCleanupRequest {
                job_id: "remote-upload-job-026".to_string(),
                object_store: "zymo_fecal_2025.05".to_string(),
                source_bytes: 42,
                staged_object_prefix: Some("../outside".to_string()),
                multipart_upload_id: None,
                session_id: None,
                pairing_id: None,
                browser_handoff_id: None,
                reason: Some("malformed cleanup request".to_string()),
            });

        let report = run_remote_upload_cancellation_cleanup(plan, &worker);
        let failed_actions = report.failed_actions();

        assert!(!report.completed());
        assert_eq!(failed_actions.len(), 1);
        assert!(failed_actions[0]
            .error
            .as_deref()
            .expect("failure message")
            .contains("managed root"));
        assert!(outside.exists());
        assert!(runner.calls.borrow().is_empty());

        cleanup(&root);
    }

    #[derive(Default)]
    struct RecordingCleanupWorker {
        calls: std::cell::RefCell<Vec<RemoteUploadCancellationCleanupScope>>,
        fail_scope: Option<RemoteUploadCancellationCleanupScope>,
    }

    impl RemoteUploadCancellationCleanupWorker for RecordingCleanupWorker {
        fn cleanup(
            &self,
            action: &RemoteUploadCancellationCleanupAction,
        ) -> Result<(), RemoteUploadCancellationCleanupError> {
            self.calls.borrow_mut().push(action.scope);
            if self.fail_scope == Some(action.scope) {
                return Err(RemoteUploadCancellationCleanupError::new(format!(
                    "cleanup failed for {:?}",
                    action.scope
                )));
            }
            Ok(())
        }
    }

    fn cleanup_plan_for_job(job_id: &str) -> RemoteUploadCancellationCleanupPlan {
        plan_remote_upload_cancellation_cleanup(RemoteUploadCancellationCleanupRequest {
            job_id: job_id.to_string(),
            object_store: "zymo_fecal_2025.05".to_string(),
            source_bytes: 42,
            staged_object_prefix: Some(format!("ssd/{job_id}")),
            multipart_upload_id: Some(format!("multipart-{job_id}")),
            session_id: None,
            pairing_id: None,
            browser_handoff_id: None,
            reason: Some("transfer failed".to_string()),
        })
    }

    fn cleanup_runtime_config(
        root: &std::path::Path,
        multipart_abort: Option<RemoteUploadMultipartAbortConfig>,
    ) -> RemoteUploadCancellationCleanupRuntimeConfig {
        RemoteUploadCancellationCleanupRuntimeConfig {
            ssd_stage_root: root.join("ssd-stage"),
            session_state_root: root.join("sessions"),
            pairing_state_root: root.join("pairings"),
            browser_handoff_state_root: root.join("handoffs"),
            multipart_abort,
        }
    }

    struct RecordingByteTransfer {
        bytes_done: u64,
    }

    impl RemoteUploadS3ByteTransfer for RecordingByteTransfer {
        fn transfer(
            &self,
            progress: &mut RemoteUploadS3TransferProgressReporter<'_>,
        ) -> Result<(), RemoteUploadS3ByteTransferError> {
            progress
                .record_progress(RemoteUploadS3TransferProgressUpdate {
                    bytes_done: self.bytes_done,
                    updated_at_utc: "2026-07-09T14:10:30Z".to_string(),
                    telemetry: None,
                    message: Some(format!(
                        "typed byte transfer copied {} bytes",
                        self.bytes_done
                    )),
                })
                .map_err(|error| RemoteUploadS3ByteTransferError::new(error.to_string()))?;
            Ok(())
        }
    }

    struct FailingByteTransfer;

    impl RemoteUploadS3ByteTransfer for FailingByteTransfer {
        fn transfer(
            &self,
            _progress: &mut RemoteUploadS3TransferProgressReporter<'_>,
        ) -> Result<(), RemoteUploadS3ByteTransferError> {
            Err(RemoteUploadS3ByteTransferError::new(
                "object service rejected part",
            ))
        }
    }

    #[derive(Default)]
    struct FakeRemoteUploadCommandRunner {
        calls: std::cell::RefCell<Vec<FakeRemoteUploadCommandCall>>,
        fail: bool,
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct FakeRemoteUploadCommandCall {
        program: String,
        args: Vec<String>,
        display_args: Vec<String>,
        environment: Vec<(String, String)>,
    }

    impl ServiceCommandRunner for FakeRemoteUploadCommandRunner {
        fn run(
            &self,
            program: &str,
            args: &[String],
        ) -> Result<ServiceCommandOutput, DaemonServiceRuntimeError> {
            self.run_with_display_args(program, args, args)
        }

        fn run_with_display_args(
            &self,
            program: &str,
            args: &[String],
            display_args: &[String],
        ) -> Result<ServiceCommandOutput, DaemonServiceRuntimeError> {
            self.run_with_display_args_and_env(program, args, display_args, &[])
        }

        fn run_with_display_args_and_env(
            &self,
            program: &str,
            args: &[String],
            display_args: &[String],
            environment: &[(String, String)],
        ) -> Result<ServiceCommandOutput, DaemonServiceRuntimeError> {
            self.calls.borrow_mut().push(FakeRemoteUploadCommandCall {
                program: program.to_string(),
                args: args.to_vec(),
                display_args: display_args.to_vec(),
                environment: environment.to_vec(),
            });
            if self.fail {
                return Err(DaemonServiceRuntimeError::CommandFailed {
                    program: program.to_string(),
                    args: display_args.to_vec(),
                    status: "exit status: 1".to_string(),
                    stderr: "multipart upload failed".to_string(),
                });
            }
            Ok(ServiceCommandOutput {
                stdout: "ok\n".to_string(),
            })
        }
    }

    fn transfer_job(policy: RemoteUploadBackpressurePolicy) -> RemoteUploadS3TransferJob {
        RemoteUploadS3TransferJob {
            job_id: "remote-upload-job-001".to_string(),
            object_store: "zymo_fecal_2025.05".to_string(),
            source_bytes: 42,
            policy,
            ssd_pressure: DaemonSsdPressure::AcceptingWrites,
        }
    }

    fn worker_request(job_id: &str) -> RemoteUploadS3TransferWorkerRequest {
        RemoteUploadS3TransferWorkerRequest {
            job: RemoteUploadS3TransferJob {
                job_id: job_id.to_string(),
                object_store: "zymo_fecal_2025.05".to_string(),
                source_bytes: 42,
                policy: RemoteUploadBackpressurePolicy::default(),
                ssd_pressure: DaemonSsdPressure::AcceptingWrites,
            },
            submitted_at_utc: "2026-07-09T14:10:00Z".to_string(),
            started_at_utc: "2026-07-09T14:10:05Z".to_string(),
            finished_at_utc: "2026-07-09T14:11:00Z".to_string(),
            actor: Some("stephen".to_string()),
        }
    }

    fn easyconnect_aws_cli_job_request(job_id: &str) -> RemoteEasyconnectAwsCliUploadJobRequest {
        RemoteEasyconnectAwsCliUploadJobRequest {
            job_id: job_id.to_string(),
            object_store: "zymo_fecal_2025.05".to_string(),
            source_bytes: 42,
            policy: RemoteUploadBackpressurePolicy::default(),
            ssd_pressure: DaemonSsdPressure::AcceptingWrites,
            program: "aws".to_string(),
            args: vec![
                "s3".to_string(),
                "cp".to_string(),
                "/private/source/reads.fastq.gz".to_string(),
                "s3://dos-zymo/raw/reads.fastq.gz".to_string(),
            ],
            display_args: vec![
                "s3".to_string(),
                "cp".to_string(),
                "<source-redacted>".to_string(),
                "s3://dos-zymo/raw/reads.fastq.gz".to_string(),
            ],
            environment: vec![("AWS_ACCESS_KEY_ID".to_string(), "AKIAEXAMPLE".to_string())],
            submitted_at_utc: "2026-07-09T14:10:00Z".to_string(),
            started_at_utc: "2026-07-09T14:10:05Z".to_string(),
            finished_at_utc: "2026-07-09T14:11:00Z".to_string(),
            progress_updated_at_utc: "2026-07-09T14:10:30Z".to_string(),
            actor: Some("stephen".to_string()),
            progress_telemetry: Some(RemoteUploadProgressTelemetry {
                source_scan_count: Some(1),
                staged_bytes: Some(42),
                ..RemoteUploadProgressTelemetry::default()
            }),
            progress_message: Some("easyconnect AWS CLI transfer copied 42 bytes".to_string()),
        }
    }

    fn admission_decision(
        action: RemoteUploadBackpressureAction,
    ) -> RemoteEasyconnectUploadAdmissionDecision {
        RemoteEasyconnectUploadAdmissionDecision {
            action,
            reason: RemoteEasyconnectUploadBackpressureReason::SsdHighPressure,
            retry_after_seconds: Some(30),
            message: "Remote upload intake is temporarily paused.".to_string(),
        }
    }

    fn temp_root(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("dasobjectstore-{label}-{}", std::process::id()))
    }

    fn cleanup(root: &std::path::Path) {
        let _ = std::fs::remove_dir_all(root);
    }
}
