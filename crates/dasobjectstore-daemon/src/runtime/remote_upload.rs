use crate::api::{
    decide_remote_easyconnect_upload_admission, DaemonSsdPressure,
    RemoteEasyconnectUploadAdmissionDecision, RemoteEasyconnectUploadAdmissionRequest,
};
use dasobjectstore_core::remote_upload::{
    RemoteUploadBackpressureAction, RemoteUploadBackpressurePolicy,
};
use std::sync::{Arc, Mutex};

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
    use super::{RemoteUploadAdmissionGate, RemoteUploadQueueDepths};
    use crate::api::{DaemonSsdPressure, RemoteEasyconnectUploadBackpressureReason};
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
    fn finishing_s3_transfer_saturates_at_zero() {
        let gate = RemoteUploadAdmissionGate::new();

        let snapshot = gate.finish_s3_transfer();

        assert_eq!(snapshot.active_s3_transfers, 0);
    }
}
