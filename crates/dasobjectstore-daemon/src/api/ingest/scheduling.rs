use super::backpressure::{DaemonSourceReadBackpressurePolicy, DaemonSourceReadPriority};
use super::resource::{DaemonIngestResourcePolicy, DaemonIngestWorkerCounts};
use super::telemetry::{DaemonIngestPressure, DaemonIngestTelemetry};
use crate::api::health::DaemonSsdPressure;
use dasobjectstore_core::ids::DiskId;
use dasobjectstore_core::lifecycle::HealthState;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestSchedulingPolicy {
    #[serde(default)]
    pub source_to_ssd: DaemonSourceToSsdPriorityPolicy,
    pub source_read_backpressure: DaemonSourceReadBackpressurePolicy,
    pub buffer_pools: DaemonIngestBufferPoolPolicySet,
}

impl Default for DaemonIngestSchedulingPolicy {
    fn default() -> Self {
        let buffer_pools = DaemonIngestBufferPoolPolicySet::default();

        Self {
            source_to_ssd: DaemonSourceToSsdPriorityPolicy::from_buffer_pools(&buffer_pools),
            source_read_backpressure: DaemonSourceReadBackpressurePolicy::default(),
            buffer_pools,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonSourceToSsdPriorityPolicy {
    pub priority: DaemonSourceReadPriority,
    pub max_source_read_queue_depth: u32,
    pub max_ssd_stage_queue_depth: u32,
    pub max_in_flight_bytes: u64,
    pub throttle_queue_fill_percent: u8,
    pub block_queue_fill_percent: u8,
}

impl DaemonSourceToSsdPriorityPolicy {
    pub fn from_buffer_pools(buffer_pools: &DaemonIngestBufferPoolPolicySet) -> Self {
        Self {
            priority: DaemonSourceReadPriority::SourceToSsdFirst,
            max_source_read_queue_depth: buffer_pools.read.queue_depth,
            max_ssd_stage_queue_depth: buffer_pools.write.queue_depth,
            max_in_flight_bytes: buffer_pools
                .read
                .maximum_pool_bytes()
                .saturating_add(buffer_pools.write.maximum_pool_bytes()),
            throttle_queue_fill_percent: 75,
            block_queue_fill_percent: 100,
        }
    }

    pub fn has_bounded_pressure_controls(&self) -> bool {
        self.max_source_read_queue_depth > 0
            && self.max_ssd_stage_queue_depth > 0
            && self.max_in_flight_bytes > 0
            && self.throttle_queue_fill_percent > 0
            && self.throttle_queue_fill_percent < self.block_queue_fill_percent
            && self.block_queue_fill_percent <= 100
    }

    pub fn queue_pressure(&self, usage: DaemonSourceToSsdQueueUsage) -> DaemonIngestPressure {
        if !self.has_bounded_pressure_controls() {
            return DaemonIngestPressure::Critical;
        }

        let fill_percent = [
            queue_fill_percent(
                u64::from(usage.source_read_queue_depth),
                u64::from(self.max_source_read_queue_depth),
            ),
            queue_fill_percent(
                u64::from(usage.ssd_stage_queue_depth),
                u64::from(self.max_ssd_stage_queue_depth),
            ),
            queue_fill_percent(usage.in_flight_bytes, self.max_in_flight_bytes),
        ]
        .into_iter()
        .max()
        .unwrap_or(0);

        if fill_percent >= self.block_queue_fill_percent {
            DaemonIngestPressure::Critical
        } else if fill_percent >= self.throttle_queue_fill_percent {
            DaemonIngestPressure::High
        } else if fill_percent > 0 {
            DaemonIngestPressure::Elevated
        } else {
            DaemonIngestPressure::Normal
        }
    }
}

impl Default for DaemonSourceToSsdPriorityPolicy {
    fn default() -> Self {
        Self::from_buffer_pools(&DaemonIngestBufferPoolPolicySet::default())
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonSourceToSsdQueueUsage {
    pub source_read_queue_depth: u32,
    pub ssd_stage_queue_depth: u32,
    pub in_flight_bytes: u64,
}

fn queue_fill_percent(value: u64, limit: u64) -> u8 {
    if limit == 0 {
        return 100;
    }

    ((value.saturating_mul(100)) / limit).min(100) as u8
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestAdaptiveSchedulerInput {
    pub available_cpu_cores: u16,
    pub resource_policy: DaemonIngestResourcePolicy,
    pub telemetry: DaemonIngestTelemetry,
    pub hdd_targets: Vec<DaemonIngestHddTargetQueue>,
}

impl DaemonIngestAdaptiveSchedulerInput {
    pub fn for_current_host(
        resource_policy: DaemonIngestResourcePolicy,
        telemetry: DaemonIngestTelemetry,
        hdd_targets: Vec<DaemonIngestHddTargetQueue>,
    ) -> Self {
        Self {
            available_cpu_cores: available_cpu_cores(),
            resource_policy,
            telemetry,
            hdd_targets,
        }
    }

    pub fn schedule(&self) -> DaemonIngestAdaptiveWorkerSchedule {
        let effective_cpu_cores = self.effective_cpu_cores();
        let memory_limited = self.memory_pressure_limited();
        let hdd_pressure = self.hdd_pressure();
        let eligible_hdd_targets = self.eligible_hdd_targets();
        let has_hdd_target_context = !self.hdd_targets.is_empty();
        let hdd_targets_available = !has_hdd_target_context || eligible_hdd_targets > 0;

        let mut worker_counts = DaemonIngestWorkerCounts {
            scan: one_if_configured(self.resource_policy.worker_counts.scan),
            source_read: self.disk_ingress_workers(hdd_pressure, memory_limited),
            ssd_stage: self.disk_ingress_workers(hdd_pressure, memory_limited),
            checksum_manifest: 0,
            hdd_placement: if hdd_targets_available {
                one_if_configured(self.resource_policy.worker_counts.hdd_placement)
            } else {
                0
            },
            hdd_write: self.hdd_write_workers(hdd_pressure, eligible_hdd_targets),
            verification: 0,
            finalization: one_if_configured(self.resource_policy.worker_counts.finalization),
        };

        let coordination_workers = worker_counts
            .scan
            .saturating_add(worker_counts.hdd_placement)
            .saturating_add(worker_counts.finalization);
        let cpu_worker_budget = effective_cpu_cores
            .saturating_sub(coordination_workers)
            .max(1);
        let cpu_worker_budget = if memory_limited {
            cpu_worker_budget.min(2)
        } else {
            cpu_worker_budget
        };
        let (checksum_manifest, verification) =
            self.cpu_bound_workers(cpu_worker_budget, self.telemetry.pressure.verification);
        worker_counts.checksum_manifest = checksum_manifest;
        worker_counts.verification = verification;

        DaemonIngestAdaptiveWorkerSchedule {
            worker_counts,
            hdd_queue_depth: self.hdd_queue_depth(hdd_pressure, eligible_hdd_targets),
            verification_parallelism: worker_counts
                .verification
                .min(self.resource_policy.verification_parallelism),
            limiting_factor: self.limiting_factor(
                hdd_pressure,
                memory_limited,
                hdd_targets_available,
            ),
            effective_cpu_cores,
        }
    }

    fn effective_cpu_cores(&self) -> u16 {
        self.available_cpu_cores
            .max(1)
            .saturating_sub(self.resource_policy.system_safety_reserve.cpu_cores)
            .max(1)
    }

    fn memory_pressure_limited(&self) -> bool {
        self.telemetry
            .system
            .memory_percent()
            .is_some_and(|percent| percent >= 90)
    }

    fn disk_ingress_workers(
        &self,
        hdd_pressure: DaemonIngestPressure,
        memory_limited: bool,
    ) -> u16 {
        let configured = self
            .resource_policy
            .worker_counts
            .source_read
            .min(self.resource_policy.worker_counts.ssd_stage);
        if configured == 0
            || self.telemetry.pressure.ssd == DaemonSsdPressure::Critical
            || hdd_pressure == DaemonIngestPressure::Critical
        {
            return 0;
        }

        if memory_limited
            || self.telemetry.pressure.ssd == DaemonSsdPressure::High
            || hdd_pressure.severity() >= DaemonIngestPressure::High.severity()
        {
            return configured.min(1);
        }

        configured.min(4)
    }

    fn hdd_write_workers(
        &self,
        hdd_pressure: DaemonIngestPressure,
        eligible_targets: usize,
    ) -> u16 {
        let configured = self.resource_policy.worker_counts.hdd_write;
        if configured == 0 || hdd_pressure == DaemonIngestPressure::Critical {
            return 0;
        }
        if !self.hdd_targets.is_empty() && eligible_targets == 0 {
            return 0;
        }

        let target_limit = if self.hdd_targets.is_empty() {
            configured
        } else {
            usize_to_u16_saturating(eligible_targets).saturating_mul(2)
        };

        match hdd_pressure {
            DaemonIngestPressure::Normal => configured.min(target_limit),
            DaemonIngestPressure::Elevated => configured
                .min(target_limit)
                .min(usize_to_u16_saturating(eligible_targets.max(1))),
            DaemonIngestPressure::High => configured.min(1),
            DaemonIngestPressure::Critical => 0,
        }
    }

    fn cpu_bound_workers(
        &self,
        cpu_worker_budget: u16,
        verification_pressure: DaemonIngestPressure,
    ) -> (u16, u16) {
        let checksum_max = self.resource_policy.worker_counts.checksum_manifest;
        let verification_max = self.resource_policy.worker_counts.verification;
        if cpu_worker_budget == 0 || (checksum_max == 0 && verification_max == 0) {
            return (0, 0);
        }
        if checksum_max == 0 {
            return (0, verification_max.min(cpu_worker_budget));
        }
        if verification_max == 0 {
            return (checksum_max.min(cpu_worker_budget), 0);
        }

        let verification_budget = match verification_pressure {
            DaemonIngestPressure::Normal | DaemonIngestPressure::Elevated => cpu_worker_budget / 2,
            DaemonIngestPressure::High | DaemonIngestPressure::Critical => {
                cpu_worker_budget.saturating_mul(2).div_ceil(3)
            }
        }
        .max(1);
        let verification = verification_max.min(verification_budget);
        let checksum_budget = cpu_worker_budget.saturating_sub(verification).max(1);
        let checksum = checksum_max.min(checksum_budget);
        let overflow = cpu_worker_budget.saturating_sub(checksum.saturating_add(verification));

        if overflow == 0 {
            return (checksum, verification);
        }

        let checksum_headroom = checksum_max.saturating_sub(checksum);
        let extra_checksum = checksum_headroom.min(overflow);
        let checksum = checksum.saturating_add(extra_checksum);
        let verification = verification.saturating_add(
            verification_max
                .saturating_sub(verification)
                .min(overflow.saturating_sub(extra_checksum)),
        );

        (checksum, verification)
    }

    fn hdd_queue_depth(&self, hdd_pressure: DaemonIngestPressure, eligible_targets: usize) -> u32 {
        if self.resource_policy.hdd_queue_depth == 0
            || hdd_pressure == DaemonIngestPressure::Critical
            || (!self.hdd_targets.is_empty() && eligible_targets == 0)
        {
            return 0;
        }

        match hdd_pressure {
            DaemonIngestPressure::Normal => self.resource_policy.hdd_queue_depth,
            DaemonIngestPressure::Elevated => self.resource_policy.hdd_queue_depth.div_ceil(2),
            DaemonIngestPressure::High => self.resource_policy.hdd_queue_depth.min(1),
            DaemonIngestPressure::Critical => 0,
        }
    }

    fn hdd_pressure(&self) -> DaemonIngestPressure {
        if self.hdd_targets.is_empty() {
            return self.telemetry.pressure.hdd;
        }

        let mut eligible_targets = 0usize;
        let mut pressure = self.telemetry.pressure.hdd;

        for target in &self.hdd_targets {
            if target.can_accept(0) {
                eligible_targets += 1;
                pressure = max_ingest_pressure(pressure, target.pressure);
            } else {
                pressure = max_ingest_pressure(pressure, DaemonIngestPressure::Elevated);
            }
        }

        if eligible_targets == 0 {
            DaemonIngestPressure::Critical
        } else {
            pressure
        }
    }

    fn eligible_hdd_targets(&self) -> usize {
        self.hdd_targets
            .iter()
            .filter(|target| target.can_accept(0))
            .count()
    }

    fn limiting_factor(
        &self,
        hdd_pressure: DaemonIngestPressure,
        memory_limited: bool,
        hdd_targets_available: bool,
    ) -> DaemonIngestAdaptiveSchedulingLimit {
        if self.telemetry.pressure.ssd == DaemonSsdPressure::Critical {
            return DaemonIngestAdaptiveSchedulingLimit::SsdPressure;
        }
        if memory_limited {
            return DaemonIngestAdaptiveSchedulingLimit::MemoryPressure;
        }
        if !hdd_targets_available || hdd_pressure == DaemonIngestPressure::Critical {
            return DaemonIngestAdaptiveSchedulingLimit::HddTargetSaturation;
        }
        if hdd_pressure.severity() >= DaemonIngestPressure::High.severity() {
            return DaemonIngestAdaptiveSchedulingLimit::HddPressure;
        }
        if self.telemetry.pressure.verification.severity() >= DaemonIngestPressure::High.severity()
        {
            return DaemonIngestAdaptiveSchedulingLimit::VerificationBacklog;
        }
        if self.effective_cpu_cores() < self.available_cpu_cores.max(1) {
            return DaemonIngestAdaptiveSchedulingLimit::CpuReserve;
        }

        DaemonIngestAdaptiveSchedulingLimit::None
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestAdaptiveWorkerSchedule {
    pub worker_counts: DaemonIngestWorkerCounts,
    pub hdd_queue_depth: u32,
    pub verification_parallelism: u16,
    pub limiting_factor: DaemonIngestAdaptiveSchedulingLimit,
    pub effective_cpu_cores: u16,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonIngestAdaptiveSchedulingLimit {
    None,
    CpuReserve,
    SsdPressure,
    HddPressure,
    HddTargetSaturation,
    VerificationBacklog,
    MemoryPressure,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestBufferPoolPolicySet {
    pub read: DaemonIngestBoundedBufferPolicy,
    pub write: DaemonIngestBoundedBufferPolicy,
    pub verify: DaemonIngestBoundedBufferPolicy,
}

impl Default for DaemonIngestBufferPoolPolicySet {
    fn default() -> Self {
        Self {
            read: DaemonIngestBoundedBufferPolicy {
                queue_depth: 32,
                buffer_bytes: 4 * 1024 * 1024,
                pool_buffers: 32,
                memory_limit_bytes: 128 * 1024 * 1024,
            },
            write: DaemonIngestBoundedBufferPolicy {
                queue_depth: 64,
                buffer_bytes: 4 * 1024 * 1024,
                pool_buffers: 64,
                memory_limit_bytes: 256 * 1024 * 1024,
            },
            verify: DaemonIngestBoundedBufferPolicy {
                queue_depth: 32,
                buffer_bytes: 2 * 1024 * 1024,
                pool_buffers: 32,
                memory_limit_bytes: 64 * 1024 * 1024,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestBoundedBufferPolicy {
    pub queue_depth: u32,
    pub buffer_bytes: u64,
    pub pool_buffers: u32,
    pub memory_limit_bytes: u64,
}

impl DaemonIngestBoundedBufferPolicy {
    pub fn maximum_pool_bytes(&self) -> u64 {
        self.buffer_bytes
            .saturating_mul(u64::from(self.pool_buffers))
            .min(self.memory_limit_bytes)
    }

    pub fn has_bounded_capacity(&self) -> bool {
        self.queue_depth > 0
            && self.buffer_bytes > 0
            && self.pool_buffers > 0
            && self.memory_limit_bytes > 0
            && self.maximum_pool_bytes() <= self.memory_limit_bytes
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestPlacementSchedulerInput {
    pub required_bytes: u64,
    pub copies: u8,
    pub targets: Vec<DaemonIngestHddTargetQueue>,
}

impl DaemonIngestPlacementSchedulerInput {
    pub fn eligible_targets(&self) -> impl Iterator<Item = &DaemonIngestHddTargetQueue> {
        self.targets
            .iter()
            .filter(|target| target.can_accept(self.required_bytes))
    }

    pub fn has_enough_eligible_targets(&self) -> bool {
        self.eligible_targets().count() >= usize::from(self.copies)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestHddTargetQueue {
    pub target_id: String,
    pub disk_id: DiskId,
    pub capacity: DaemonIngestTargetCapacity,
    pub queue: DaemonIngestHddQueueState,
    pub write_throughput_bytes_per_second: u64,
    pub health: HealthState,
    pub pressure: DaemonIngestPressure,
    pub failure_state: DaemonIngestTargetFailureState,
}

impl DaemonIngestHddTargetQueue {
    pub fn can_accept(&self, required_bytes: u64) -> bool {
        self.capacity.available_bytes >= required_bytes
            && self.queue.queue_depth < self.queue.max_queue_depth
            && self.pressure != DaemonIngestPressure::Critical
            && self.failure_state == DaemonIngestTargetFailureState::Available
            && matches!(self.health, HealthState::Healthy | HealthState::Watch)
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestTargetCapacity {
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub reserved_bytes: u64,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestHddQueueState {
    pub queue_depth: u32,
    pub queued_bytes: u64,
    pub max_queue_depth: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonIngestTargetFailureState {
    Available,
    Pressure,
    Failed,
    Suspended,
}

impl Default for DaemonIngestTargetFailureState {
    fn default() -> Self {
        Self::Available
    }
}

fn available_cpu_cores() -> u16 {
    std::thread::available_parallelism()
        .map(|cores| cores.get().min(u16::MAX as usize) as u16)
        .unwrap_or(1)
        .max(1)
}

fn one_if_configured(configured: u16) -> u16 {
    u16::from(configured > 0)
}

fn usize_to_u16_saturating(value: usize) -> u16 {
    value.min(u16::MAX as usize) as u16
}

fn max_ingest_pressure(
    left: DaemonIngestPressure,
    right: DaemonIngestPressure,
) -> DaemonIngestPressure {
    if right.severity() > left.severity() {
        right
    } else {
        left
    }
}
