use std::path::PathBuf;

const MIB: u128 = 1024 * 1024;
const GIB: u128 = MIB * 1024;
const TIB: u128 = GIB * 1024;

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

#[cfg(test)]
mod tests {
    use super::{format_size_label, ImportPlan, ImportTarget, SourcePath};

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
}
