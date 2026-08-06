use crate::api::{PreverifiedHostSubject, PreverifiedHostSubjectValidationError};
use dasobjectstore_metadata::DiskRetirementReport;
use serde::{Deserialize, Serialize};

pub const FORCE_DISK_RETIRE_CONFIRMATION: &str = "confirm force retire";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiskRetireRequest {
    pub disk_id: String,
    /// Required at execution time: a versioned Pistis-verified subject from
    /// the fixed host GUI/API service peer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_subject: Option<PreverifiedHostSubject>,
}

impl DiskRetireRequest {
    pub fn validate(&self) -> Result<(), DiskRetireValidationError> {
        if self.disk_id.trim().is_empty() {
            return Err(DiskRetireValidationError::BlankDiskId);
        }
        if let Some(subject) = &self.verified_subject {
            subject
                .validate()
                .map_err(DiskRetireValidationError::InvalidVerifiedSubject)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiskRetireResponse {
    pub report: DiskRetirementReport,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiskForceRetireRequest {
    pub disk_id: String,
    pub allow_force_retire: bool,
    pub confirmation_marker: String,
    /// Required at execution time: a versioned Pistis-verified subject from
    /// the fixed host GUI/API service peer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_subject: Option<PreverifiedHostSubject>,
}

impl DiskForceRetireRequest {
    pub fn validate(&self) -> Result<(), DiskRetireValidationError> {
        if self.disk_id.trim().is_empty() {
            return Err(DiskRetireValidationError::BlankDiskId);
        }
        if self.confirmation_marker != FORCE_DISK_RETIRE_CONFIRMATION {
            return Err(DiskRetireValidationError::ConfirmationMismatch);
        }
        if let Some(subject) = &self.verified_subject {
            subject
                .validate()
                .map_err(DiskRetireValidationError::InvalidVerifiedSubject)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiskRetireValidationError {
    BlankDiskId,
    ConfirmationMismatch,
    InvalidVerifiedSubject(PreverifiedHostSubjectValidationError),
}

impl std::fmt::Display for DiskRetireValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BlankDiskId => formatter.write_str("disk_id must not be blank"),
            Self::ConfirmationMismatch => write!(
                formatter,
                "confirmation_marker must equal {FORCE_DISK_RETIRE_CONFIRMATION:?}"
            ),
            Self::InvalidVerifiedSubject(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for DiskRetireValidationError {}

#[cfg(test)]
mod tests {
    use super::{
        DiskForceRetireRequest, DiskRetireRequest, DiskRetireValidationError,
        FORCE_DISK_RETIRE_CONFIRMATION,
    };

    #[test]
    fn rejects_blank_disk_id() {
        assert_eq!(
            DiskRetireRequest {
                disk_id: "  ".to_string(),
                verified_subject: None,
            }
            .validate(),
            Err(DiskRetireValidationError::BlankDiskId)
        );
    }

    #[test]
    fn force_retire_requires_confirmation() {
        assert_eq!(
            DiskForceRetireRequest {
                disk_id: "disk-a".to_string(),
                allow_force_retire: true,
                confirmation_marker: String::new(),
                verified_subject: None,
            }
            .validate(),
            Err(DiskRetireValidationError::ConfirmationMismatch)
        );
        assert!(DiskForceRetireRequest {
            disk_id: "disk-a".to_string(),
            allow_force_retire: true,
            confirmation_marker: FORCE_DISK_RETIRE_CONFIRMATION.to_string(),
            verified_subject: None,
        }
        .validate()
        .is_ok());
    }
}
