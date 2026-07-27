//! Daemon policy façade over the persistent metadata scheduler.

use dasobjectstore_core::ids::{ObjectId, StoreId};
use dasobjectstore_metadata::{
    claim_next_scheduler_job, claim_next_scheduler_job_in_class, submit_scheduler_job,
    SchedulerClaimRequest, SchedulerClassPolicy, SchedulerError, SchedulerJob, SchedulerJobRequest,
};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DaemonWorkClass {
    DirectS3,
    NativeIngest,
    RemoteReconcile,
    Destage,
    VerificationRepair,
}

impl DaemonWorkClass {
    fn policy(self) -> SchedulerClassPolicy {
        let (name, weight, active, bytes) = match self {
            Self::Destage => ("destage", 8, 8, 8_u64 << 40),
            Self::DirectS3 => ("direct_s3", 4, 8, 2_u64 << 40),
            Self::NativeIngest => ("native_ingest", 4, 4, 2_u64 << 40),
            Self::RemoteReconcile => ("remote_reconcile", 2, 2, 1_u64 << 40),
            Self::VerificationRepair => ("verification_repair", 1, 2, 1_u64 << 40),
        };
        SchedulerClassPolicy {
            work_class: name.to_string(),
            weight,
            max_active_jobs: active,
            max_active_bytes: bytes,
        }
    }
}

#[derive(Clone, Debug)]
pub struct DaemonWorkSubmission {
    pub job_id: String,
    pub idempotency_key: String,
    pub request_digest: String,
    pub class: DaemonWorkClass,
    pub origin: String,
    pub store_id: StoreId,
    pub object_id: Option<ObjectId>,
    pub priority: i32,
    pub byte_cost: u64,
    pub acknowledgement_policy: String,
    pub required_copy_count: u8,
    pub created_at_utc: String,
}

#[derive(Clone, Debug)]
pub struct PersistentWorkScheduler {
    live_sqlite_path: PathBuf,
}

impl PersistentWorkScheduler {
    pub fn new(live_sqlite_path: PathBuf) -> Self {
        Self { live_sqlite_path }
    }

    pub fn submit(&self, work: DaemonWorkSubmission) -> Result<SchedulerJob, SchedulerError> {
        submit_scheduler_job(&SchedulerJobRequest {
            live_sqlite_path: self.live_sqlite_path.clone(),
            scheduler_job_id: work.job_id,
            idempotency_key: work.idempotency_key,
            request_digest: work.request_digest,
            class: work.class.policy(),
            origin: work.origin,
            store_id: work.store_id,
            object_id: work.object_id,
            priority: work.priority,
            byte_cost: work.byte_cost,
            acknowledgement_policy: work.acknowledgement_policy,
            required_copy_count: work.required_copy_count,
            max_attempts: 8,
            created_at_utc: work.created_at_utc,
        })
    }

    pub fn claim(
        &self,
        worker: &str,
        now_utc: &str,
        lease_expires_at_utc: &str,
    ) -> Result<Option<SchedulerJob>, SchedulerError> {
        claim_next_scheduler_job(&SchedulerClaimRequest {
            live_sqlite_path: self.live_sqlite_path.clone(),
            worker: worker.to_string(),
            now_utc: now_utc.to_string(),
            lease_expires_at_utc: lease_expires_at_utc.to_string(),
        })
    }

    pub fn claim_class(
        &self,
        class: DaemonWorkClass,
        worker: &str,
        now_utc: &str,
        lease_expires_at_utc: &str,
    ) -> Result<Option<SchedulerJob>, SchedulerError> {
        claim_next_scheduler_job_in_class(
            &SchedulerClaimRequest {
                live_sqlite_path: self.live_sqlite_path.clone(),
                worker: worker.to_string(),
                now_utc: now_utc.to_string(),
                lease_expires_at_utc: lease_expires_at_utc.to_string(),
            },
            &class.policy().work_class,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settlement_has_greater_weight_than_new_ingress() {
        assert!(
            DaemonWorkClass::Destage.policy().weight > DaemonWorkClass::DirectS3.policy().weight
        );
        assert!(
            DaemonWorkClass::DirectS3.policy().weight
                > DaemonWorkClass::VerificationRepair.policy().weight
        );
    }
}
