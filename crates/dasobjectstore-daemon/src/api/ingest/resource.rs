use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestResourcePolicy {
    pub worker_counts: DaemonIngestWorkerCounts,
    pub memory_budget_bytes: u64,
    pub ssd_reserve_bytes: u64,
    pub hdd_queue_depth: u32,
    pub verification_parallelism: u16,
    pub system_safety_reserve: DaemonIngestSystemSafetyReserve,
}

impl Default for DaemonIngestResourcePolicy {
    fn default() -> Self {
        let worker_counts = DaemonIngestWorkerCounts::default();

        Self {
            worker_counts,
            memory_budget_bytes: 1024 * 1024 * 1024,
            ssd_reserve_bytes: 10 * 1024 * 1024 * 1024,
            hdd_queue_depth: 64,
            verification_parallelism: worker_counts.verification,
            system_safety_reserve: DaemonIngestSystemSafetyReserve::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestWorkerCounts {
    pub scan: u16,
    pub source_read: u16,
    pub ssd_stage: u16,
    pub checksum_manifest: u16,
    pub hdd_placement: u16,
    pub hdd_write: u16,
    pub verification: u16,
    pub finalization: u16,
}

impl Default for DaemonIngestWorkerCounts {
    fn default() -> Self {
        let cores = std::thread::available_parallelism()
            .map(|cores| cores.get().min(u16::MAX as usize) as u16)
            .unwrap_or(1)
            .max(1);
        let coordination_workers = 1;
        let disk_workers = cores.clamp(1, 8);
        let cpu_workers = cores.saturating_sub(1).max(1).min(8);

        Self {
            scan: coordination_workers,
            source_read: disk_workers.min(4),
            ssd_stage: disk_workers.min(4),
            checksum_manifest: cpu_workers,
            hdd_placement: coordination_workers,
            hdd_write: disk_workers,
            verification: cpu_workers,
            finalization: coordination_workers,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonIngestSystemSafetyReserve {
    pub cpu_cores: u16,
    pub memory_bytes: u64,
}

impl Default for DaemonIngestSystemSafetyReserve {
    fn default() -> Self {
        let cpu_cores = std::thread::available_parallelism()
            .map(|cores| u16::from(cores.get() > 2))
            .unwrap_or(0);

        Self {
            cpu_cores,
            memory_bytes: 512 * 1024 * 1024,
        }
    }
}
