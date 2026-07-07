use crate::resource::{ResourcePolicyDisplay, ResourcePolicySummary};
use std::path::PathBuf;

const MIB: u128 = 1024 * 1024;
const GIB: u128 = MIB * 1024;
const TIB: u128 = GIB * 1024;
pub const IMPORT_LAUNCH_CONFIRMATION_PHRASE: &str = "confirm import launch";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportTarget {
    pub object_store: String,
    pub subobject: Option<String>,
}

impl ImportTarget {
    pub fn new(object_store: impl Into<String>, subobject: Option<impl Into<String>>) -> Self {
        Self {
            object_store: object_store.into(),
            subobject: subobject.map(Into::into),
        }
    }

    pub fn label(&self) -> String {
        match &self.subobject {
            Some(subobject) if !subobject.is_empty() => {
                format!("{}/{}", self.object_store, subobject)
            }
            _ => self.object_store.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourcePath {
    pub path: PathBuf,
}

impl SourcePath {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportPlan {
    pub target: ImportTarget,
    pub sources: Vec<SourcePath>,
    pub file_count: u64,
    pub total_bytes: u64,
}

impl ImportPlan {
    pub fn new(
        target: ImportTarget,
        sources: Vec<SourcePath>,
        file_count: u64,
        total_bytes: u64,
    ) -> Self {
        Self {
            target,
            sources,
            file_count,
            total_bytes,
        }
    }

    pub fn summary(&self) -> ImportPlanningSummary {
        ImportPlanningSummary {
            target_label: self.target.label(),
            source_count: self.sources.len(),
            file_count: self.file_count,
            total_bytes: self.total_bytes,
            total_size_label: format_size_label(self.total_bytes),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportPlanningSummary {
    pub target_label: String,
    pub source_count: usize,
    pub file_count: u64,
    pub total_bytes: u64,
    pub total_size_label: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportDescriptionMetadata {
    pub description: Option<String>,
    pub fields: Vec<ImportMetadataField>,
}

impl ImportDescriptionMetadata {
    pub fn new(description: Option<String>, fields: Vec<ImportMetadataField>) -> Self {
        Self {
            description: normalize_optional_text(description),
            fields,
        }
    }

    pub fn from_key_value_entries(
        description: Option<String>,
        entries: &[String],
    ) -> Result<Self, ImportMetadataError> {
        let mut fields = Vec::with_capacity(entries.len());

        for entry in entries {
            let field = ImportMetadataField::parse(entry)?;

            if fields
                .iter()
                .any(|existing: &ImportMetadataField| existing.key == field.key)
            {
                return Err(ImportMetadataError::DuplicateFieldKey { key: field.key });
            }

            fields.push(field);
        }

        Ok(Self::new(description, fields))
    }

    pub fn display_data(&self) -> ImportDescriptionMetadataDisplay {
        ImportDescriptionMetadataDisplay {
            description_label: self
                .description
                .clone()
                .unwrap_or_else(|| "not provided".to_string()),
            field_labels: self
                .fields
                .iter()
                .map(|field| format!("{}={}", field.key, field.value))
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportMetadataField {
    pub key: String,
    pub value: String,
}

impl ImportMetadataField {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into().trim().to_string(),
            value: value.into().trim().to_string(),
        }
    }

    pub fn parse(entry: &str) -> Result<Self, ImportMetadataError> {
        let (key, value) =
            entry
                .split_once('=')
                .ok_or_else(|| ImportMetadataError::MissingSeparator {
                    entry: entry.to_string(),
                })?;

        let field = Self::new(key, value);
        if field.key.is_empty() {
            return Err(ImportMetadataError::BlankFieldKey {
                entry: entry.to_string(),
            });
        }
        if field.value.is_empty() {
            return Err(ImportMetadataError::BlankFieldValue {
                key: field.key.clone(),
            });
        }

        Ok(field)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportDescriptionMetadataDisplay {
    pub description_label: String,
    pub field_labels: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImportMetadataError {
    MissingSeparator { entry: String },
    BlankFieldKey { entry: String },
    BlankFieldValue { key: String },
    DuplicateFieldKey { key: String },
}

impl std::fmt::Display for ImportMetadataError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSeparator { entry } => write!(
                formatter,
                "metadata entry `{entry}` must use KEY=VALUE syntax"
            ),
            Self::BlankFieldKey { entry } => {
                write!(formatter, "metadata entry `{entry}` has a blank key")
            }
            Self::BlankFieldValue { key } => {
                write!(formatter, "metadata field `{key}` has a blank value")
            }
            Self::DuplicateFieldKey { key } => {
                write!(
                    formatter,
                    "metadata field `{key}` was provided more than once"
                )
            }
        }
    }
}

impl std::error::Error for ImportMetadataError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportLaunchConfirmation {
    pub metadata: ImportDescriptionMetadata,
    pub confirmation_phrase: Option<String>,
}

impl ImportLaunchConfirmation {
    pub fn new(metadata: ImportDescriptionMetadata, confirmation_phrase: Option<String>) -> Self {
        Self {
            metadata,
            confirmation_phrase: normalize_optional_text(confirmation_phrase),
        }
    }

    pub fn review(&self, plan: &ImportPlan) -> ImportLaunchReview {
        let mut blockers = Vec::new();

        if self.metadata.description.is_none() {
            blockers.push(ImportLaunchBlocker::MissingDescription);
        }

        match self.confirmation_phrase.as_deref() {
            None => blockers.push(ImportLaunchBlocker::MissingConfirmation {
                required_phrase: IMPORT_LAUNCH_CONFIRMATION_PHRASE,
            }),
            Some(value) if value != IMPORT_LAUNCH_CONFIRMATION_PHRASE => {
                blockers.push(ImportLaunchBlocker::ConfirmationMismatch {
                    required_phrase: IMPORT_LAUNCH_CONFIRMATION_PHRASE,
                });
            }
            Some(_) => {}
        }

        ImportLaunchReview {
            planning: plan.summary(),
            metadata: self.metadata.display_data(),
            required_confirmation_phrase: IMPORT_LAUNCH_CONFIRMATION_PHRASE,
            blockers,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportLaunchReview {
    pub planning: ImportPlanningSummary,
    pub metadata: ImportDescriptionMetadataDisplay,
    pub required_confirmation_phrase: &'static str,
    pub blockers: Vec<ImportLaunchBlocker>,
}

impl ImportLaunchReview {
    pub fn is_ready_to_launch(&self) -> bool {
        self.blockers.is_empty()
    }

    pub fn status_label(&self) -> &'static str {
        if self.is_ready_to_launch() {
            "ready"
        } else {
            "blocked"
        }
    }

    pub fn blocker_labels(&self) -> Vec<String> {
        self.blockers
            .iter()
            .map(ImportLaunchBlocker::display_label)
            .collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ImportLaunchBlocker {
    MissingDescription,
    MissingConfirmation { required_phrase: &'static str },
    ConfirmationMismatch { required_phrase: &'static str },
}

impl ImportLaunchBlocker {
    pub fn display_label(&self) -> String {
        match self {
            Self::MissingDescription => "import description is required".to_string(),
            Self::MissingConfirmation { required_phrase } => {
                format!("launch confirmation is required: `{required_phrase}`")
            }
            Self::ConfirmationMismatch { required_phrase } => {
                format!("launch confirmation must be `{required_phrase}`")
            }
        }
    }
}

/// Formats byte counts with binary units for TUI planning displays.
pub fn format_size_label(bytes: u64) -> String {
    let bytes = u128::from(bytes);
    let (unit_bytes, unit) = if bytes >= TIB {
        (TIB, "TiB")
    } else if bytes >= GIB {
        (GIB, "GiB")
    } else {
        (MIB, "MiB")
    };

    let tenths = ((bytes * 10) + (unit_bytes / 2)) / unit_bytes;
    format!("{}.{:01} {}", tenths / 10, tenths % 10, unit)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceCap<T> {
    Automatic,
    Explicit(T),
}

impl<T: Copy> ResourceCap<T> {
    pub fn explicit_value(&self) -> Option<T> {
        match self {
            Self::Automatic => None,
            Self::Explicit(value) => Some(*value),
        }
    }
}

impl ResourceCap<u16> {
    pub fn parse_count(value: &str) -> Result<Self, String> {
        parse_resource_cap(value, |input| {
            input
                .parse::<u16>()
                .map_err(|_| format!("expected 'auto' or a positive whole number, got '{input}'"))
        })
    }
}

impl ResourceCap<u64> {
    pub fn parse_bytes(value: &str) -> Result<Self, String> {
        parse_resource_cap(value, |input| {
            input
                .parse::<u64>()
                .map_err(|_| format!("expected 'auto' or a byte count, got '{input}'"))
        })
    }
}

fn parse_resource_cap<T>(
    value: &str,
    parse_explicit: impl FnOnce(&str) -> Result<T, String>,
) -> Result<ResourceCap<T>, String>
where
    T: PartialEq + From<u8>,
{
    let normalized = value.trim().to_ascii_lowercase();
    if matches!(normalized.as_str(), "auto" | "automatic") {
        return Ok(ResourceCap::Automatic);
    }

    let explicit = parse_explicit(value)?;
    if explicit == T::from(0) {
        return Err("explicit resource caps must be greater than zero".to_string());
    }

    Ok(ResourceCap::Explicit(explicit))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceUsePlan {
    pub policy: ResourcePolicySummary,
    pub core_cap: ResourceCap<u16>,
    pub memory_cap_bytes: ResourceCap<u64>,
    pub ssd_reserve_bytes: ResourceCap<u64>,
    pub hdd_write_concurrency: ResourceCap<u16>,
}

impl ResourceUsePlan {
    pub fn new(
        policy: ResourcePolicySummary,
        core_cap: ResourceCap<u16>,
        memory_cap_bytes: ResourceCap<u64>,
        ssd_reserve_bytes: ResourceCap<u64>,
        hdd_write_concurrency: ResourceCap<u16>,
    ) -> Self {
        Self {
            policy,
            core_cap,
            memory_cap_bytes,
            ssd_reserve_bytes,
            hdd_write_concurrency,
        }
    }

    pub fn display_data(&self) -> ResourcePolicyDisplay {
        let mut display = self.policy.display_data();
        display.worker_counts_label = format!(
            "{}; core use {}",
            display.worker_counts_label,
            count_cap_label(self.core_cap)
        );
        display.memory_budget_label = bytes_cap_label(self.memory_cap_bytes, "cap");
        display.ssd_reserve_label = bytes_cap_label(self.ssd_reserve_bytes, "reserve");
        display.hdd_queue_depth_label = format!(
            "{}; write concurrency {}",
            display.hdd_queue_depth_label,
            count_cap_label(self.hdd_write_concurrency)
        );
        display
    }
}

fn count_cap_label(cap: ResourceCap<u16>) -> String {
    match cap {
        ResourceCap::Automatic => "automatic".to_string(),
        ResourceCap::Explicit(value) => format!("explicit cap {value}"),
    }
}

fn bytes_cap_label(cap: ResourceCap<u64>, noun: &str) -> String {
    match cap {
        ResourceCap::Automatic => "automatic".to_string(),
        ResourceCap::Explicit(bytes) => format!("explicit {noun} {}", format_size_label(bytes)),
    }
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
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
