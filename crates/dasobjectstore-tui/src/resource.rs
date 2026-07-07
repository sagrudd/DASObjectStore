use crate::planning::format_size_label;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkerCounts {
    pub scan: u16,
    pub read: u16,
    pub stage: u16,
    pub write: u16,
    pub verify: u16,
}

impl WorkerCounts {
    pub fn total(&self) -> u32 {
        u32::from(self.scan)
            + u32::from(self.read)
            + u32::from(self.stage)
            + u32::from(self.write)
            + u32::from(self.verify)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourcePolicySummary {
    pub workers: WorkerCounts,
    pub memory_budget_bytes: u64,
    pub ssd_reserve_bytes: u64,
    pub hdd_queue_depth: u16,
    pub verification_parallelism: u16,
}

impl ResourcePolicySummary {
    pub fn display_data(&self) -> ResourcePolicyDisplay {
        ResourcePolicyDisplay {
            worker_counts_label: format!(
                "{} total (scan {}, read {}, stage {}, write {}, verify {})",
                self.workers.total(),
                self.workers.scan,
                self.workers.read,
                self.workers.stage,
                self.workers.write,
                self.workers.verify
            ),
            memory_budget_label: format_size_label(self.memory_budget_bytes),
            ssd_reserve_label: format_size_label(self.ssd_reserve_bytes),
            hdd_queue_depth_label: self.hdd_queue_depth.to_string(),
            verification_parallelism_label: self.verification_parallelism.to_string(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourcePolicyDisplay {
    pub worker_counts_label: String,
    pub memory_budget_label: String,
    pub ssd_reserve_label: String,
    pub hdd_queue_depth_label: String,
    pub verification_parallelism_label: String,
}

#[cfg(test)]
mod tests {
    use super::{ResourcePolicySummary, WorkerCounts};

    #[test]
    fn summarizes_resource_policy_for_prelaunch_display() {
        let policy = ResourcePolicySummary {
            workers: WorkerCounts {
                scan: 2,
                read: 4,
                stage: 2,
                write: 6,
                verify: 3,
            },
            memory_budget_bytes: 8 * 1024 * 1024 * 1024,
            ssd_reserve_bytes: 512 * 1024 * 1024 * 1024,
            hdd_queue_depth: 24,
            verification_parallelism: 3,
        };

        let display = policy.display_data();

        assert_eq!(
            display.worker_counts_label,
            "17 total (scan 2, read 4, stage 2, write 6, verify 3)"
        );
        assert_eq!(display.memory_budget_label, "8.0 GiB");
        assert_eq!(display.ssd_reserve_label, "512.0 GiB");
        assert_eq!(display.hdd_queue_depth_label, "24");
        assert_eq!(display.verification_parallelism_label, "3");
    }
}
