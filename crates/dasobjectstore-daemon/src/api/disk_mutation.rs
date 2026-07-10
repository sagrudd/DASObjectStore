use dasobjectstore_metadata::DiskRetirementReport;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiskRetireRequest {
    pub disk_id: String,
}

impl DiskRetireRequest {
    pub fn validate(&self) -> Result<(), DiskRetireValidationError> {
        if self.disk_id.trim().is_empty() {
            return Err(DiskRetireValidationError::BlankDiskId);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiskRetireResponse {
    pub report: DiskRetirementReport,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum DiskRetireValidationError {
    BlankDiskId,
}

impl std::fmt::Display for DiskRetireValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BlankDiskId => formatter.write_str("disk_id must not be blank"),
        }
    }
}

impl std::error::Error for DiskRetireValidationError {}

#[cfg(test)]
mod tests {
    use super::{DiskRetireRequest, DiskRetireValidationError};

    #[test]
    fn rejects_blank_disk_id() {
        assert_eq!(
            DiskRetireRequest {
                disk_id: "  ".to_string(),
            }
            .validate(),
            Err(DiskRetireValidationError::BlankDiskId)
        );
    }
}
