use super::{
    admin_jobs::AdminJobRegistry,
    service::{DaemonServiceRuntimeError, ServiceCommandRunner},
};
use crate::api::{
    decide_remote_easyconnect_upload_admission, DaemonIngestQueueDepths, DaemonIngestTelemetry,
    DaemonJobEvent, DaemonJobId, DaemonJobIdError, DaemonJobKind, DaemonJobProgress,
    DaemonJobState, DaemonJobSummary, DaemonSsdPressure, RemoteEasyconnectUploadAdmissionDecision,
    RemoteEasyconnectUploadAdmissionRequest,
};
use dasobjectstore_core::remote_upload::{
    RemoteUploadBackpressureAction, RemoteUploadBackpressurePolicy,
};
use std::{
    fmt,
    sync::{Arc, Mutex},
};

#[derive(Debug, Default)]
pub struct RemoteUploadAdmissionGate {
    state: Mutex<RemoteUploadRuntimeSnapshot>,
}

impl RemoteUploadAdmissionGate {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> RemoteUploadRuntimeSnapshot {
        *self.state.lock().expect("remote upload gate lock poisoned")
    }

    pub fn observe_queue_depths(
        &self,
        depths: RemoteUploadQueueDepths,
    ) -> RemoteUploadRuntimeSnapshot {
        let mut state = self.state.lock().expect("remote upload gate lock poisoned");
        state.ssd_stage_queue_depth = depths.ssd_stage_queue_depth;
        state.hdd_landing_queue_depth = depths.hdd_landing_queue_depth;
        state.verification_queue_depth = depths.verification_queue_depth;
        *state
    }

    pub fn observe_ingest_queue_depths(
        &self,
        depths: DaemonIngestQueueDepths,
    ) -> RemoteUploadRuntimeSnapshot {
        self.observe_queue_depths(RemoteUploadQueueDepths::from(depths))
    }

    pub fn observe_ingest_telemetry(
        &self,
        telemetry: DaemonIngestTelemetry,
    ) -> RemoteUploadRuntimeSnapshot {
        self.observe_ingest_queue_depths(telemetry.queue_depths)
    }

    pub fn admission_decision(
        &self,
        policy: RemoteUploadBackpressurePolicy,
        ssd_pressure: DaemonSsdPressure,
    ) -> RemoteEasyconnectUploadAdmissionDecision {
        let state = self.snapshot();
        admission_decision_for_state(policy, ssd_pressure, state)
    }

    pub fn admission_decision_from_request(
        &self,
        request: RemoteEasyconnectUploadAdmissionRequest,
    ) -> RemoteEasyconnectUploadAdmissionDecision {
        let state = self.observe_queue_depths(RemoteUploadQueueDepths {
            ssd_stage_queue_depth: request.ssd_stage_queue_depth,
            hdd_landing_queue_depth: request.hdd_landing_queue_depth,
            verification_queue_depth: request.verification_queue_depth,
        });
        admission_decision_for_state(request.policy, request.ssd_pressure, state)
    }

    pub fn try_begin_s3_transfer(
        &self,
        policy: RemoteUploadBackpressurePolicy,
        ssd_pressure: DaemonSsdPressure,
    ) -> RemoteEasyconnectUploadAdmissionDecision {
        let (_, decision) = self.try_begin_s3_transfer_inner(policy, ssd_pressure);
        decision
    }

    pub fn try_acquire_s3_transfer(
        self: &Arc<Self>,
        policy: RemoteUploadBackpressurePolicy,
        ssd_pressure: DaemonSsdPressure,
    ) -> Result<RemoteUploadS3TransferPermit, RemoteEasyconnectUploadAdmissionDecision> {
        let (admitted, decision) = self.try_begin_s3_transfer_inner(policy, ssd_pressure);
        if admitted {
            Ok(RemoteUploadS3TransferPermit {
                gate: Arc::clone(self),
                released: false,
            })
        } else {
            Err(decision)
        }
    }

    pub fn run_s3_transfer<T, E>(
        self: &Arc<Self>,
        policy: RemoteUploadBackpressurePolicy,
        ssd_pressure: DaemonSsdPressure,
        transfer: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, RemoteUploadS3TransferRunError<E>> {
        let permit = self
            .try_acquire_s3_transfer(policy, ssd_pressure)
            .map_err(RemoteUploadS3TransferRunError::Admission)?;
        let result = transfer().map_err(RemoteUploadS3TransferRunError::Transfer);
        drop(permit);
        result
    }

    pub fn run_s3_transfer_job<E>(
        self: &Arc<Self>,
        job: RemoteUploadS3TransferJob,
        transfer: impl FnOnce() -> Result<(), E>,
    ) -> RemoteUploadS3TransferJobSummary
    where
        E: fmt::Display,
    {
        let outcome = match self.run_s3_transfer(job.policy, job.ssd_pressure, transfer) {
            Ok(()) => RemoteUploadS3TransferJobOutcome::Completed,
            Err(RemoteUploadS3TransferRunError::Admission(decision)) => {
                RemoteUploadS3TransferJobOutcome::NotAdmitted(decision)
            }
            Err(RemoteUploadS3TransferRunError::Transfer(error)) => {
                RemoteUploadS3TransferJobOutcome::Failed {
                    error: error.to_string(),
                }
            }
        };

        RemoteUploadS3TransferJobSummary {
            job_id: job.job_id,
            object_store: job.object_store,
            source_bytes: job.source_bytes,
            outcome,
            runtime_after: self.snapshot(),
        }
    }

    fn try_begin_s3_transfer_inner(
        &self,
        policy: RemoteUploadBackpressurePolicy,
        ssd_pressure: DaemonSsdPressure,
    ) -> (bool, RemoteEasyconnectUploadAdmissionDecision) {
        let mut state = self.state.lock().expect("remote upload gate lock poisoned");
        let decision = admission_decision_for_state(policy, ssd_pressure, *state);
        if decision.action == RemoteUploadBackpressureAction::Accept {
            state.active_s3_transfers = state.active_s3_transfers.saturating_add(1);
            return (true, decision);
        }
        (false, decision)
    }

    pub fn finish_s3_transfer(&self) -> RemoteUploadRuntimeSnapshot {
        let mut state = self.state.lock().expect("remote upload gate lock poisoned");
        state.active_s3_transfers = state.active_s3_transfers.saturating_sub(1);
        *state
    }
}

#[derive(Debug)]
pub struct RemoteUploadS3TransferPermit {
    gate: Arc<RemoteUploadAdmissionGate>,
    released: bool,
}

impl RemoteUploadS3TransferPermit {
    pub fn release(mut self) -> RemoteUploadRuntimeSnapshot {
        self.released = true;
        self.gate.finish_s3_transfer()
    }
}

impl Drop for RemoteUploadS3TransferPermit {
    fn drop(&mut self) {
        if !self.released {
            self.gate.finish_s3_transfer();
            self.released = true;
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum RemoteUploadS3TransferRunError<E> {
    Admission(RemoteEasyconnectUploadAdmissionDecision),
    Transfer(E),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RemoteUploadRuntimeSnapshot {
    pub active_s3_transfers: u16,
    pub ssd_stage_queue_depth: u32,
    pub hdd_landing_queue_depth: u32,
    pub verification_queue_depth: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RemoteUploadQueueDepths {
    pub ssd_stage_queue_depth: u32,
    pub hdd_landing_queue_depth: u32,
    pub verification_queue_depth: u32,
}

impl From<DaemonIngestQueueDepths> for RemoteUploadQueueDepths {
    fn from(depths: DaemonIngestQueueDepths) -> Self {
        Self {
            ssd_stage_queue_depth: depths.ssd_stage,
            hdd_landing_queue_depth: depths.hdd_write,
            verification_queue_depth: depths.verification,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteUploadS3TransferJob {
    pub job_id: String,
    pub object_store: String,
    pub source_bytes: u64,
    pub policy: RemoteUploadBackpressurePolicy,
    pub ssd_pressure: DaemonSsdPressure,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteUploadS3TransferJobSummary {
    pub job_id: String,
    pub object_store: String,
    pub source_bytes: u64,
    pub outcome: RemoteUploadS3TransferJobOutcome,
    pub runtime_after: RemoteUploadRuntimeSnapshot,
}

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
    pub source_bytes: u64,
    pub progress_updated_at_utc: String,
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
            source_bytes,
            progress_updated_at_utc: progress_updated_at_utc.into(),
            progress_message: None,
        }
    }

    pub fn with_display_args(mut self, display_args: Vec<String>) -> Self {
        self.display_args = display_args;
        self
    }

    pub fn with_progress_message(mut self, message: impl Into<String>) -> Self {
        self.progress_message = Some(message.into());
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
            .run_with_display_args(&self.plan.program, &self.plan.args, &self.plan.display_args)
            .map_err(|error| RemoteUploadS3ByteTransferError::new(error.to_string()))?;
        progress
            .record_progress(RemoteUploadS3TransferProgressUpdate {
                bytes_done: self.plan.source_bytes,
                updated_at_utc: self.plan.progress_updated_at_utc.clone(),
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
    pub submitted_at_utc: String,
    pub started_at_utc: String,
    pub finished_at_utc: String,
    pub progress_updated_at_utc: String,
    pub actor: Option<String>,
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
    .with_display_args(request.display_args);
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

    pub fn run_with_progress<E>(
        &self,
        request: RemoteUploadS3TransferWorkerRequest,
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
                });
            }
        };

        let running_job = running_daemon_job_for_s3_transfer(
            &job,
            submitted_at_utc.clone(),
            started_at_utc,
            actor.clone(),
        )?;
        let running_event = daemon_job_event_for_summary(running_job.clone());
        self.registry.record(running_job)?;

        let mut progress_reporter = RemoteUploadS3TransferProgressReporter {
            registry: self.registry,
            job_id: DaemonJobId::new(job.job_id.clone())
                .map_err(|_| DaemonServiceRuntimeError::InvalidJobId(job.job_id.clone()))?,
            object_store: job.object_store.clone(),
            source_bytes: job.source_bytes,
            submitted_at_utc: submitted_at_utc.clone(),
            actor: actor.clone(),
            events: Vec::new(),
        };

        let outcome = match transfer(&mut progress_reporter) {
            Ok(()) => RemoteUploadS3TransferJobOutcome::Completed,
            Err(error) => RemoteUploadS3TransferJobOutcome::Failed {
                error: error.to_string(),
            },
        };
        let progress_events = progress_reporter.into_events();
        drop(permit);

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
}

pub struct RemoteUploadS3TransferProgressReporter<'a> {
    registry: &'a dyn AdminJobRegistry,
    job_id: DaemonJobId,
    object_store: String,
    source_bytes: u64,
    submitted_at_utc: String,
    actor: Option<String>,
    events: Vec<DaemonJobEvent>,
}

impl RemoteUploadS3TransferProgressReporter<'_> {
    pub fn record_progress(
        &mut self,
        update: RemoteUploadS3TransferProgressUpdate,
    ) -> Result<DaemonJobEvent, DaemonServiceRuntimeError> {
        let bytes_done = update.bytes_done.min(self.source_bytes);
        let job = DaemonJobSummary {
            job_id: self.job_id.clone(),
            kind: DaemonJobKind::RemoteUpload,
            state: DaemonJobState::Running,
            progress: DaemonJobProgress {
                stage: "remote_s3_transfer_running".to_string(),
                work_bytes_done: bytes_done,
                work_bytes_total: self.source_bytes,
                work_units_done: 0,
                work_units_total: 1,
                message: update.message.or_else(|| {
                    Some(format!(
                        "remote upload S3 transfer running for {}",
                        self.object_store
                    ))
                }),
            },
            submitted_at_utc: self.submitted_at_utc.clone(),
            updated_at_utc: update.updated_at_utc,
            actor: self.actor.clone(),
            failure_message: None,
        };
        let event = daemon_job_event_for_summary(job.clone());
        self.registry.record(job)?;
        self.events.push(event.clone());
        Ok(event)
    }

    fn into_events(self) -> Vec<DaemonJobEvent> {
        self.events
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteUploadS3TransferProgressUpdate {
    pub bytes_done: u64,
    pub updated_at_utc: String,
    pub message: Option<String>,
}

impl RemoteUploadS3TransferJobSummary {
    pub fn daemon_job_summary(
        &self,
        submitted_at_utc: impl Into<String>,
        updated_at_utc: impl Into<String>,
        actor: Option<String>,
    ) -> Result<DaemonJobSummary, DaemonJobIdError> {
        let job_id = DaemonJobId::new(self.job_id.clone())?;
        let state = self.daemon_job_state();
        let failure_message = self.failure_message();

        Ok(DaemonJobSummary {
            job_id,
            kind: DaemonJobKind::RemoteUpload,
            state,
            progress: self.daemon_job_progress(),
            submitted_at_utc: submitted_at_utc.into(),
            updated_at_utc: updated_at_utc.into(),
            actor,
            failure_message,
        })
    }

    pub fn daemon_job_event(
        &self,
        submitted_at_utc: impl Into<String>,
        updated_at_utc: impl Into<String>,
        actor: Option<String>,
    ) -> Result<DaemonJobEvent, DaemonJobIdError> {
        let job = self.daemon_job_summary(submitted_at_utc, updated_at_utc, actor)?;
        Ok(daemon_job_event_for_summary(job))
    }

    fn daemon_job_state(&self) -> DaemonJobState {
        match &self.outcome {
            RemoteUploadS3TransferJobOutcome::Completed => DaemonJobState::Complete,
            RemoteUploadS3TransferJobOutcome::Failed { .. } => DaemonJobState::Failed,
            RemoteUploadS3TransferJobOutcome::NotAdmitted(decision) => match decision.action {
                RemoteUploadBackpressureAction::Accept
                | RemoteUploadBackpressureAction::PauseNewTransfers => DaemonJobState::Waiting,
                RemoteUploadBackpressureAction::RejectNewTransfers => DaemonJobState::Failed,
            },
        }
    }

    fn daemon_job_progress(&self) -> DaemonJobProgress {
        match &self.outcome {
            RemoteUploadS3TransferJobOutcome::Completed => DaemonJobProgress {
                stage: "remote_s3_transfer_complete".to_string(),
                work_bytes_done: self.source_bytes,
                work_bytes_total: self.source_bytes,
                work_units_done: 1,
                work_units_total: 1,
                message: Some(format!(
                    "remote upload S3 transfer completed for {}",
                    self.object_store
                )),
            },
            RemoteUploadS3TransferJobOutcome::Failed { error } => DaemonJobProgress {
                stage: "remote_s3_transfer_failed".to_string(),
                work_bytes_done: 0,
                work_bytes_total: self.source_bytes,
                work_units_done: 0,
                work_units_total: 1,
                message: Some(error.clone()),
            },
            RemoteUploadS3TransferJobOutcome::NotAdmitted(decision) => DaemonJobProgress {
                stage: match decision.action {
                    RemoteUploadBackpressureAction::RejectNewTransfers => {
                        "remote_s3_admission_rejected"
                    }
                    RemoteUploadBackpressureAction::Accept
                    | RemoteUploadBackpressureAction::PauseNewTransfers => {
                        "remote_s3_admission_wait"
                    }
                }
                .to_string(),
                work_bytes_done: 0,
                work_bytes_total: self.source_bytes,
                work_units_done: 0,
                work_units_total: 1,
                message: Some(decision.message.clone()),
            },
        }
    }

    fn failure_message(&self) -> Option<String> {
        match &self.outcome {
            RemoteUploadS3TransferJobOutcome::Failed { error } => Some(error.clone()),
            RemoteUploadS3TransferJobOutcome::NotAdmitted(decision)
                if decision.action == RemoteUploadBackpressureAction::RejectNewTransfers =>
            {
                Some(decision.message.clone())
            }
            _ => None,
        }
    }
}

fn daemon_job_event_for_summary(job: DaemonJobSummary) -> DaemonJobEvent {
    match job.state {
        DaemonJobState::Complete => DaemonJobEvent::Complete(job),
        DaemonJobState::Failed => DaemonJobEvent::Failed(job),
        _ => DaemonJobEvent::Progress(job),
    }
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

fn admission_decision_for_state(
    policy: RemoteUploadBackpressurePolicy,
    ssd_pressure: DaemonSsdPressure,
    state: RemoteUploadRuntimeSnapshot,
) -> RemoteEasyconnectUploadAdmissionDecision {
    decide_remote_easyconnect_upload_admission(RemoteEasyconnectUploadAdmissionRequest {
        policy,
        ssd_pressure,
        active_s3_transfers: state.active_s3_transfers,
        ssd_stage_queue_depth: state.ssd_stage_queue_depth,
        hdd_landing_queue_depth: state.hdd_landing_queue_depth,
        verification_queue_depth: state.verification_queue_depth,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        record_remote_upload_s3_transfer_job, run_remote_easyconnect_aws_cli_upload_job,
        RemoteEasyconnectAwsCliUploadJobRequest, RemoteUploadAdmissionGate,
        RemoteUploadAwsCliByteTransfer, RemoteUploadAwsCliTransferPlan, RemoteUploadQueueDepths,
        RemoteUploadS3ByteTransfer, RemoteUploadS3ByteTransferError, RemoteUploadS3TransferJob,
        RemoteUploadS3TransferJobOutcome, RemoteUploadS3TransferJobSummary,
        RemoteUploadS3TransferProgressReporter, RemoteUploadS3TransferProgressUpdate,
        RemoteUploadS3TransferWorker, RemoteUploadS3TransferWorkerRequest,
    };
    use crate::api::{
        DaemonIngestQueueDepths, DaemonIngestTelemetry, DaemonJobEvent, DaemonJobKind,
        DaemonJobState, DaemonJobStatusRequest, DaemonSsdPressure,
        RemoteEasyconnectUploadAdmissionDecision, RemoteEasyconnectUploadBackpressureReason,
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
                scan: 99,
                source_read: 88,
                ssd_stage: 0,
                hdd_write: policy.max_hdd_landing_queue_depth,
                verification: 0,
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
            Some("typed byte transfer copied 32 bytes")
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
            self.calls.borrow_mut().push(FakeRemoteUploadCommandCall {
                program: program.to_string(),
                args: args.to_vec(),
                display_args: display_args.to_vec(),
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
            submitted_at_utc: "2026-07-09T14:10:00Z".to_string(),
            started_at_utc: "2026-07-09T14:10:05Z".to_string(),
            finished_at_utc: "2026-07-09T14:11:00Z".to_string(),
            progress_updated_at_utc: "2026-07-09T14:10:30Z".to_string(),
            actor: Some("stephen".to_string()),
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
