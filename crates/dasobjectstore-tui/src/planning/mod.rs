mod confirmation;
mod formatting;
mod metadata;
mod plan;
mod resource_cap;

pub use confirmation::{ImportLaunchBlocker, ImportLaunchConfirmation, ImportLaunchReview};
pub use formatting::format_size_label;
pub use metadata::{
    ImportDescriptionMetadata, ImportDescriptionMetadataDisplay, ImportMetadataError,
    ImportMetadataField,
};
pub use plan::{ImportPlan, ImportPlanningSummary, ImportTarget, SourcePath};
pub use resource_cap::{ResourceCap, ResourceUsePlan};

pub const IMPORT_LAUNCH_CONFIRMATION_PHRASE: &str = "confirm import launch";

pub(crate) fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{
        format_size_label, ImportDescriptionMetadata, ImportLaunchBlocker,
        ImportLaunchConfirmation, ImportMetadataError, ImportMetadataField, ImportPlan,
        ImportTarget, ResourceCap, ResourceUsePlan, SourcePath, IMPORT_LAUNCH_CONFIRMATION_PHRASE,
    };
    use crate::resource::{ResourcePolicySummary, WorkerCounts};

    #[test]
    fn scales_size_labels_to_binary_units() {
        assert_eq!(format_size_label(512 * 1024), "0.5 MiB");
        assert_eq!(format_size_label(1536 * 1024), "1.5 MiB");
        assert_eq!(format_size_label(3 * 1024 * 1024 * 1024), "3.0 GiB");
        assert_eq!(format_size_label(5 * 1024 * 1024 * 1024 * 1024), "5.0 TiB");
    }

    #[test]
    fn summarizes_import_plan_for_target_and_sources() {
        let plan = ImportPlan::new(
            ImportTarget::new("research", Some("run-42")),
            vec![SourcePath::new("/data/a"), SourcePath::new("/data/b")],
            128,
            2 * 1024 * 1024 * 1024,
        );

        let summary = plan.summary();

        assert_eq!(summary.target_label, "research/run-42");
        assert_eq!(summary.source_count, 2);
        assert_eq!(summary.file_count, 128);
        assert_eq!(summary.total_bytes, 2 * 1024 * 1024 * 1024);
        assert_eq!(summary.total_size_label, "2.0 GiB");
    }

    #[test]
    fn parses_automatic_and_explicit_resource_caps() {
        assert_eq!(
            ResourceCap::<u16>::parse_count("auto"),
            Ok(ResourceCap::Automatic)
        );
        assert_eq!(
            ResourceCap::<u16>::parse_count("automatic"),
            Ok(ResourceCap::Automatic)
        );
        assert_eq!(
            ResourceCap::<u64>::parse_bytes("8589934592"),
            Ok(ResourceCap::Explicit(8 * 1024 * 1024 * 1024))
        );
        assert!(ResourceCap::<u16>::parse_count("0").is_err());
    }

    #[test]
    fn marks_automatic_and_explicit_caps_in_planning_display() {
        let plan = ResourceUsePlan::new(
            ResourcePolicySummary {
                workers: WorkerCounts {
                    scan: 1,
                    read: 2,
                    stage: 1,
                    write: 4,
                    verify: 2,
                },
                memory_budget_bytes: 8 * 1024 * 1024 * 1024,
                ssd_reserve_bytes: 512 * 1024 * 1024 * 1024,
                hdd_queue_depth: 24,
                verification_parallelism: 2,
            },
            ResourceCap::Automatic,
            ResourceCap::Explicit(8 * 1024 * 1024 * 1024),
            ResourceCap::Explicit(512 * 1024 * 1024 * 1024),
            ResourceCap::Explicit(4),
        );

        let display = plan.display_data();

        assert_eq!(
            display.worker_counts_label,
            "10 total (scan 1, read 2, stage 1, write 4, verify 2); core use automatic"
        );
        assert_eq!(display.memory_budget_label, "explicit cap 8.0 GiB");
        assert_eq!(display.ssd_reserve_label, "explicit reserve 512.0 GiB");
        assert_eq!(
            display.hdd_queue_depth_label,
            "24; write concurrency explicit cap 4"
        );
    }

    #[test]
    fn parses_import_description_metadata_fields() {
        let entries = vec![
            "ticket= LAB-42 ".to_string(),
            "operator= archive-team".to_string(),
        ];

        let metadata = ImportDescriptionMetadata::from_key_value_entries(
            Some("  Zymo fecal dataset import  ".to_string()),
            &entries,
        )
        .expect("metadata parses");

        assert_eq!(
            metadata,
            ImportDescriptionMetadata::new(
                Some("Zymo fecal dataset import".to_string()),
                vec![
                    ImportMetadataField::new("ticket", "LAB-42"),
                    ImportMetadataField::new("operator", "archive-team"),
                ],
            )
        );
        assert_eq!(
            metadata.display_data().field_labels,
            vec!["ticket=LAB-42", "operator=archive-team"]
        );
    }

    #[test]
    fn rejects_invalid_import_metadata_fields() {
        let missing_separator = ImportMetadataField::parse("ticket").expect_err("separator needed");
        assert_eq!(
            missing_separator,
            ImportMetadataError::MissingSeparator {
                entry: "ticket".to_string()
            }
        );

        let blank_key = ImportMetadataField::parse(" =LAB-42").expect_err("key needed");
        assert_eq!(
            blank_key,
            ImportMetadataError::BlankFieldKey {
                entry: " =LAB-42".to_string()
            }
        );

        let blank_value = ImportMetadataField::parse("ticket= ").expect_err("value needed");
        assert_eq!(
            blank_value,
            ImportMetadataError::BlankFieldValue {
                key: "ticket".to_string()
            }
        );

        let duplicate = ImportDescriptionMetadata::from_key_value_entries(
            None,
            &["ticket=LAB-42".to_string(), "ticket=LAB-43".to_string()],
        )
        .expect_err("duplicate rejected");
        assert_eq!(
            duplicate,
            ImportMetadataError::DuplicateFieldKey {
                key: "ticket".to_string()
            }
        );
    }

    #[test]
    fn blocks_launch_until_description_and_confirmation_are_present() {
        let plan = ImportPlan::new(
            ImportTarget::new("research", Some("run-42")),
            vec![SourcePath::new("/data/a")],
            1,
            1024,
        );
        let confirmation =
            ImportLaunchConfirmation::new(ImportDescriptionMetadata::new(None, Vec::new()), None);

        let review = confirmation.review(&plan);

        assert!(!review.is_ready_to_launch());
        assert_eq!(review.status_label(), "blocked");
        assert_eq!(
            review.blockers,
            vec![
                ImportLaunchBlocker::MissingDescription,
                ImportLaunchBlocker::MissingConfirmation {
                    required_phrase: IMPORT_LAUNCH_CONFIRMATION_PHRASE,
                },
            ]
        );
    }

    #[test]
    fn requires_exact_launch_confirmation_phrase() {
        let plan = ImportPlan::new(
            ImportTarget::new("research", None::<String>),
            vec![SourcePath::new("/data/a")],
            1,
            1024,
        );
        let confirmation = ImportLaunchConfirmation::new(
            ImportDescriptionMetadata::new(Some("dataset import".to_string()), Vec::new()),
            Some("confirm something else".to_string()),
        );

        let review = confirmation.review(&plan);

        assert!(!review.is_ready_to_launch());
        assert_eq!(
            review.blockers,
            vec![ImportLaunchBlocker::ConfirmationMismatch {
                required_phrase: IMPORT_LAUNCH_CONFIRMATION_PHRASE,
            }]
        );
    }

    #[test]
    fn accepts_launch_after_description_and_confirmation() {
        let plan = ImportPlan::new(
            ImportTarget::new("research", None::<String>),
            vec![SourcePath::new("/data/a")],
            1,
            1024,
        );
        let confirmation = ImportLaunchConfirmation::new(
            ImportDescriptionMetadata::new(Some("dataset import".to_string()), Vec::new()),
            Some(format!(" {IMPORT_LAUNCH_CONFIRMATION_PHRASE} ")),
        );

        let review = confirmation.review(&plan);

        assert!(review.is_ready_to_launch());
        assert_eq!(review.status_label(), "ready");
        assert!(review.blockers.is_empty());
    }
}
