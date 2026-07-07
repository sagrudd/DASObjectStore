use super::format_size_label;
use std::path::PathBuf;

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
