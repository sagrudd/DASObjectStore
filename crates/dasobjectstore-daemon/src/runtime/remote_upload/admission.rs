use super::RemoteUploadS3TransferJobOutcome;
use crate::api::{
    decide_remote_easyconnect_upload_admission, DaemonIngestQueueDepths, DaemonIngestTelemetry,
    DaemonSsdPressure, RemoteEasyconnectUploadAdmissionDecision,
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
        let mut state = self.state.lock().expect("remote upload gate lock poisoned");
        state.ssd_stage_queue_depth = telemetry.queue_depths.ssd_stage;
        state.hdd_landing_queue_depth = telemetry.queue_depths.hdd_write;
        state.verification_queue_depth = telemetry.queue_depths.verification;
        state.active_hdd_writers = telemetry.workers.hdd_write.active;
        *state
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
    pub active_hdd_writers: u16,
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

fn _admission_module_marker() {}
