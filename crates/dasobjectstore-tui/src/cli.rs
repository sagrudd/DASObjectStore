use clap::Parser;
use dasobjectstore_tui::{
    ImportPlan, ImportTarget, ResourcePolicySummary, SourcePath, WorkerCounts,
};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "dasobjectstore-tui",
    about = "DASObjectStore terminal import planning scaffold"
)]
pub struct TuiCli {
    #[arg(long, default_value = "unselected", value_name = "OBJECT_STORE")]
    pub object_store: String,
    #[arg(long, value_name = "SUBOBJECT")]
    pub subobject: Option<String>,
    #[arg(long = "source", value_name = "PATH")]
    pub sources: Vec<PathBuf>,
    #[arg(long, default_value_t = 0)]
    pub file_count: u64,
    #[arg(long, default_value_t = 0)]
    pub total_bytes: u64,
    #[arg(long, default_value_t = 1)]
    pub scan_workers: u16,
    #[arg(long, default_value_t = 1)]
    pub read_workers: u16,
    #[arg(long, default_value_t = 1)]
    pub stage_workers: u16,
    #[arg(long, default_value_t = 1)]
    pub write_workers: u16,
    #[arg(long, default_value_t = 1)]
    pub verify_workers: u16,
    #[arg(long, default_value_t = 0)]
    pub memory_budget_bytes: u64,
    #[arg(long, default_value_t = 0)]
    pub ssd_reserve_bytes: u64,
    #[arg(long, default_value_t = 0)]
    pub hdd_queue_depth: u16,
    #[arg(long, default_value_t = 1)]
    pub verification_parallelism: u16,
}

impl TuiCli {
    pub fn import_plan(&self) -> ImportPlan {
        ImportPlan::new(
            ImportTarget::new(self.object_store.clone(), self.subobject.clone()),
            self.sources.iter().cloned().map(SourcePath::new).collect(),
            self.file_count,
            self.total_bytes,
        )
    }

    pub fn resource_policy(&self) -> ResourcePolicySummary {
        ResourcePolicySummary {
            workers: WorkerCounts {
                scan: self.scan_workers,
                read: self.read_workers,
                stage: self.stage_workers,
                write: self.write_workers,
                verify: self.verify_workers,
            },
            memory_budget_bytes: self.memory_budget_bytes,
            ssd_reserve_bytes: self.ssd_reserve_bytes,
            hdd_queue_depth: self.hdd_queue_depth,
            verification_parallelism: self.verification_parallelism,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TuiCli;
    use clap::Parser;

    #[test]
    fn parses_import_planning_arguments() {
        let cli = TuiCli::parse_from([
            "dasobjectstore-tui",
            "--object-store",
            "research",
            "--subobject",
            "run-42",
            "--source",
            "/data/a",
            "--file-count",
            "2",
            "--total-bytes",
            "1048576",
        ]);

        let summary = cli.import_plan().summary();

        assert_eq!(summary.target_label, "research/run-42");
        assert_eq!(summary.source_count, 1);
        assert_eq!(summary.file_count, 2);
        assert_eq!(summary.total_size_label, "1.0 MiB");
    }
}
