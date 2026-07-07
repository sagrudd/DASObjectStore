use clap::Parser;
use dasobjectstore_tui::planning::{ResourceCap, ResourceUsePlan};
use dasobjectstore_tui::{
    ImportDescriptionMetadata, ImportLaunchConfirmation, ImportMetadataError, ImportPlan,
    ImportTarget, ResourcePolicySummary, SourcePath, WorkerCounts,
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
    #[arg(long, value_name = "TEXT")]
    pub description: Option<String>,
    #[arg(long = "metadata", value_name = "KEY=VALUE")]
    pub metadata: Vec<String>,
    #[arg(long = "confirm-launch", value_name = "PHRASE")]
    pub confirm_launch: Option<String>,
    #[arg(long, default_value_t = 1)]
    pub scan_workers: u16,
    #[arg(long, default_value_t = 1)]
    pub read_workers: u16,
    #[arg(long, default_value_t = 1)]
    pub stage_workers: u16,
    #[arg(long, default_value_t = 1)]
    pub verify_workers: u16,
    #[arg(
        long = "cores",
        alias = "core-cap",
        default_value = "auto",
        value_name = "auto|COUNT",
        value_parser = ResourceCap::<u16>::parse_count,
    )]
    pub core_cap: ResourceCap<u16>,
    #[arg(
        long = "memory-cap-bytes",
        alias = "memory-budget-bytes",
        default_value = "auto",
        value_name = "auto|BYTES",
        value_parser = ResourceCap::<u64>::parse_bytes,
    )]
    pub memory_cap_bytes: ResourceCap<u64>,
    #[arg(
        long = "ssd-reserve-bytes",
        default_value = "auto",
        value_name = "auto|BYTES",
        value_parser = ResourceCap::<u64>::parse_bytes,
    )]
    pub ssd_reserve_bytes: ResourceCap<u64>,
    #[arg(
        long = "hdd-write-concurrency",
        alias = "write-workers",
        default_value = "auto",
        value_name = "auto|COUNT",
        value_parser = ResourceCap::<u16>::parse_count,
    )]
    pub hdd_write_concurrency: ResourceCap<u16>,
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

    pub fn resource_policy(&self) -> ResourceUsePlan {
        let write_workers = self.hdd_write_concurrency.explicit_value().unwrap_or(1);
        let policy = ResourcePolicySummary {
            workers: WorkerCounts {
                scan: self.scan_workers,
                read: self.read_workers,
                stage: self.stage_workers,
                write: write_workers,
                verify: self.verify_workers,
            },
            memory_budget_bytes: self.memory_cap_bytes.explicit_value().unwrap_or(0),
            ssd_reserve_bytes: self.ssd_reserve_bytes.explicit_value().unwrap_or(0),
            hdd_queue_depth: self.hdd_queue_depth,
            verification_parallelism: self.verification_parallelism,
        };

        ResourceUsePlan::new(
            policy,
            self.core_cap,
            self.memory_cap_bytes,
            self.ssd_reserve_bytes,
            self.hdd_write_concurrency,
        )
    }

    pub fn launch_confirmation(&self) -> Result<ImportLaunchConfirmation, ImportMetadataError> {
        let metadata = ImportDescriptionMetadata::from_key_value_entries(
            self.description.clone(),
            &self.metadata,
        )?;

        Ok(ImportLaunchConfirmation::new(
            metadata,
            self.confirm_launch.clone(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::TuiCli;
    use clap::Parser;
    use dasobjectstore_tui::planning::ResourceCap;

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

    #[test]
    fn parses_import_description_metadata_and_launch_confirmation() {
        let cli = TuiCli::parse_from([
            "dasobjectstore-tui",
            "--description",
            "Zymo fecal dataset",
            "--metadata",
            "ticket=LAB-42",
            "--metadata",
            "operator=archive-team",
            "--confirm-launch",
            "confirm import launch",
        ]);

        let confirmation = cli
            .launch_confirmation()
            .expect("launch confirmation parses");
        let display = confirmation.metadata.display_data();

        assert_eq!(display.description_label, "Zymo fecal dataset");
        assert_eq!(
            display.field_labels,
            vec!["ticket=LAB-42", "operator=archive-team"]
        );
        assert_eq!(
            confirmation.confirmation_phrase.as_deref(),
            Some("confirm import launch")
        );
    }

    #[test]
    fn defaults_resource_caps_to_automatic() {
        let cli = TuiCli::parse_from(["dasobjectstore-tui"]);

        let resources = cli.resource_policy();
        let display = resources.display_data();

        assert_eq!(resources.core_cap, ResourceCap::Automatic);
        assert_eq!(resources.memory_cap_bytes, ResourceCap::Automatic);
        assert_eq!(resources.ssd_reserve_bytes, ResourceCap::Automatic);
        assert_eq!(resources.hdd_write_concurrency, ResourceCap::Automatic);
        assert!(display
            .worker_counts_label
            .ends_with("; core use automatic"));
        assert_eq!(display.memory_budget_label, "automatic");
        assert_eq!(display.ssd_reserve_label, "automatic");
        assert_eq!(
            display.hdd_queue_depth_label,
            "0; write concurrency automatic"
        );
    }

    #[test]
    fn parses_explicit_resource_caps_for_import_preview() {
        let cli = TuiCli::parse_from([
            "dasobjectstore-tui",
            "--cores",
            "12",
            "--memory-cap-bytes",
            "8589934592",
            "--ssd-reserve-bytes",
            "549755813888",
            "--hdd-write-concurrency",
            "6",
            "--hdd-queue-depth",
            "24",
        ]);

        let resources = cli.resource_policy();
        let display = resources.display_data();

        assert_eq!(resources.core_cap, ResourceCap::Explicit(12));
        assert_eq!(
            resources.memory_cap_bytes,
            ResourceCap::Explicit(8 * 1024 * 1024 * 1024)
        );
        assert_eq!(
            resources.ssd_reserve_bytes,
            ResourceCap::Explicit(512 * 1024 * 1024 * 1024)
        );
        assert_eq!(resources.hdd_write_concurrency, ResourceCap::Explicit(6));
        assert_eq!(
            display.worker_counts_label,
            "10 total (scan 1, read 1, stage 1, write 6, verify 1); core use explicit cap 12"
        );
        assert_eq!(display.memory_budget_label, "explicit cap 8.0 GiB");
        assert_eq!(display.ssd_reserve_label, "explicit reserve 512.0 GiB");
        assert_eq!(
            display.hdd_queue_depth_label,
            "24; write concurrency explicit cap 6"
        );
    }
}
