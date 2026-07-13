use super::backpressure::{
    DaemonSourceReadBackpressureAction, DaemonSourceReadBackpressureInput,
    DaemonSourceReadBackpressurePolicy, DaemonSourceReadBackpressureReason,
};
use super::scheduling::{
    DaemonIngestAdaptiveSchedulerInput, DaemonIngestAdaptiveSchedulingLimit,
    DaemonIngestAdaptiveWorkerSchedule,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestAdmissionInput {
    pub scheduler: DaemonIngestAdaptiveSchedulerInput,
    pub source_read: DaemonSourceReadBackpressureInput,
    pub source_read_policy: DaemonSourceReadBackpressurePolicy,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonIngestAdmissionAction {
    Run,
    Throttle,
    Block,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonIngestAdmissionReason {
    None,
    SourceRead(DaemonSourceReadBackpressureReason),
    Scheduler(DaemonIngestAdaptiveSchedulingLimit),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestAdmissionDecision {
    pub action: DaemonIngestAdmissionAction,
    pub reason: DaemonIngestAdmissionReason,
    pub schedule: DaemonIngestAdaptiveWorkerSchedule,
}

impl DaemonIngestAdmissionDecision {
    pub fn should_run(self) -> bool {
        self.action == DaemonIngestAdmissionAction::Run
    }

    pub fn should_throttle(self) -> bool {
        self.action == DaemonIngestAdmissionAction::Throttle
    }

    pub fn should_block(self) -> bool {
        self.action == DaemonIngestAdmissionAction::Block
    }
}

pub fn decide_ingest_admission(input: DaemonIngestAdmissionInput) -> DaemonIngestAdmissionDecision {
    let schedule = input.scheduler.schedule();
    let source_read = input.source_read.classify(&input.source_read_policy);

    if source_read.action == DaemonSourceReadBackpressureAction::Block {
        return DaemonIngestAdmissionDecision {
            action: DaemonIngestAdmissionAction::Block,
            reason: DaemonIngestAdmissionReason::SourceRead(source_read.reason),
            schedule,
        };
    }

    if schedule.worker_counts.source_read == 0 {
        return DaemonIngestAdmissionDecision {
            action: DaemonIngestAdmissionAction::Block,
            reason: DaemonIngestAdmissionReason::Scheduler(schedule.limiting_factor),
            schedule,
        };
    }

    if source_read.action == DaemonSourceReadBackpressureAction::Throttle
        || !matches!(
            schedule.limiting_factor,
            DaemonIngestAdaptiveSchedulingLimit::None
        )
    {
        return DaemonIngestAdmissionDecision {
            action: DaemonIngestAdmissionAction::Throttle,
            reason: if source_read.action == DaemonSourceReadBackpressureAction::Throttle {
                DaemonIngestAdmissionReason::SourceRead(source_read.reason)
            } else {
                DaemonIngestAdmissionReason::Scheduler(schedule.limiting_factor)
            },
            schedule,
        };
    }

    DaemonIngestAdmissionDecision {
        action: DaemonIngestAdmissionAction::Run,
        reason: DaemonIngestAdmissionReason::None,
        schedule,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::health::DaemonSsdPressure;
    use crate::api::ingest::{
        DaemonIngestHddTargetQueue, DaemonIngestResourcePolicy, DaemonIngestTelemetry,
        DaemonIngestWorkerCounts,
    };
    use dasobjectstore_core::ids::DiskId;
    use dasobjectstore_core::lifecycle::HealthState;

    fn input() -> DaemonIngestAdmissionInput {
        let resource_policy = DaemonIngestResourcePolicy {
            worker_counts: DaemonIngestWorkerCounts {
                source_read: 2,
                ssd_stage: 2,
                ..DaemonIngestWorkerCounts::default()
            },
            system_safety_reserve: super::super::resource::DaemonIngestSystemSafetyReserve {
                cpu_cores: 0,
                memory_bytes: 0,
            },
            ..DaemonIngestResourcePolicy::default()
        };
        DaemonIngestAdmissionInput {
            scheduler: DaemonIngestAdaptiveSchedulerInput {
                available_cpu_cores: 8,
                resource_policy,
                telemetry: DaemonIngestTelemetry::default(),
                hdd_targets: vec![DaemonIngestHddTargetQueue {
                    target_id: "hdd-a".to_string(),
                    disk_id: DiskId::new("hdd-a").expect("disk id"),
                    capacity: super::super::scheduling::DaemonIngestTargetCapacity {
                        total_bytes: 1_000,
                        available_bytes: 1_000,
                        reserved_bytes: 0,
                    },
                    queue: super::super::scheduling::DaemonIngestHddQueueState {
                        queue_depth: 0,
                        queued_bytes: 0,
                        max_queue_depth: 4,
                    },
                    write_throughput_bytes_per_second: 0,
                    health: HealthState::Healthy,
                    pressure: Default::default(),
                    failure_state: Default::default(),
                }],
            },
            source_read: Default::default(),
            source_read_policy: Default::default(),
        }
    }

    #[test]
    fn combines_source_block_with_scheduler_snapshot() {
        let mut input = input();
        input.source_read.ssd_pressure = DaemonSsdPressure::Critical;

        let decision = decide_ingest_admission(input);

        assert!(decision.should_block());
        assert_eq!(
            decision.reason,
            DaemonIngestAdmissionReason::SourceRead(
                DaemonSourceReadBackpressureReason::SsdPressure
            )
        );
        assert!(decision.schedule.worker_counts.source_read > 0);
    }

    #[test]
    fn throttles_when_scheduler_reports_memory_pressure() {
        let mut input = input();
        input.scheduler.telemetry.system.memory_used_bytes = 95;
        input.scheduler.telemetry.system.memory_budget_bytes = Some(100);

        let decision = decide_ingest_admission(input);

        assert!(decision.should_throttle());
        assert_eq!(
            decision.reason,
            DaemonIngestAdmissionReason::Scheduler(
                DaemonIngestAdaptiveSchedulingLimit::MemoryPressure
            )
        );
    }

    #[test]
    fn runs_when_source_and_scheduler_are_clear() {
        let decision = decide_ingest_admission(input());

        assert!(decision.should_run());
        assert_eq!(decision.reason, DaemonIngestAdmissionReason::None);
        assert!(decision.schedule.worker_counts.source_read > 0);
    }
}
