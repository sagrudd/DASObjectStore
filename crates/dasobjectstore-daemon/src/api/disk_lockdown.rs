use crate::api::{DaemonJobAcceptedResponse, DaemonJobId, DaemonJobKind};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};
use std::path::PathBuf;

pub const DISK_LOCKDOWN_CONFIRMATION: &str = "confirm lockdown das";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiskLockdownRequest {
    pub mount_root: PathBuf,
    pub service_user: String,
    pub service_group: String,
    pub create_service_user: bool,
    pub dry_run: bool,
    pub confirmation_marker: String,
}

impl DiskLockdownRequest {
    pub fn validate(&self) -> Result<(), DiskLockdownValidationError> {
        if !self.mount_root.is_absolute() {
            return Err(DiskLockdownValidationError::RelativePath {
                path: self.mount_root.clone(),
            });
        }
        validate_name(&self.service_user, "service_user")?;
        validate_name(&self.service_group, "service_group")?;
        if !self.dry_run && self.confirmation_marker != DISK_LOCKDOWN_CONFIRMATION {
            return Err(DiskLockdownValidationError::ConfirmationMismatch);
        }
        Ok(())
    }
}

fn validate_name(value: &str, field: &'static str) -> Result<(), DiskLockdownValidationError> {
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(DiskLockdownValidationError::UnsafeName { field });
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DiskLockdownResponse {
    pub accepted: DaemonJobAcceptedResponse,
    pub mount_root: PathBuf,
    pub service_user: String,
    pub service_group: String,
    pub protected_roots: Vec<PathBuf>,
    pub planned_commands: Vec<String>,
}

impl DiskLockdownResponse {
    pub fn accepted(
        job_id: DaemonJobId,
        accepted_at_utc: impl Into<String>,
        request: &DiskLockdownRequest,
        protected_roots: Vec<PathBuf>,
        planned_commands: Vec<String>,
    ) -> Self {
        Self {
            accepted: DaemonJobAcceptedResponse {
                job_id,
                kind: DaemonJobKind::SystemAdministration,
                accepted_at_utc: accepted_at_utc.into(),
                dry_run: request.dry_run,
            },
            mount_root: request.mount_root.clone(),
            service_user: request.service_user.clone(),
            service_group: request.service_group.clone(),
            protected_roots,
            planned_commands,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiskLockdownValidationError {
    RelativePath { path: PathBuf },
    UnsafeName { field: &'static str },
    ConfirmationMismatch,
}

impl Display for DiskLockdownValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RelativePath { path } => {
                write!(formatter, "mount_root must be absolute: {}", path.display())
            }
            Self::UnsafeName { field } => write!(formatter, "{field} is not a safe account name"),
            Self::ConfirmationMismatch => {
                formatter.write_str("disk lockdown confirmation marker does not match")
            }
        }
    }
}

impl std::error::Error for DiskLockdownValidationError {}

#[cfg(test)]
mod tests {
    use super::{DiskLockdownRequest, DiskLockdownValidationError, DISK_LOCKDOWN_CONFIRMATION};
    use std::path::PathBuf;

    fn request() -> DiskLockdownRequest {
        DiskLockdownRequest {
            mount_root: PathBuf::from("/srv/das"),
            service_user: "dasobjectstore".to_string(),
            service_group: "dasobjectstore".to_string(),
            create_service_user: true,
            dry_run: false,
            confirmation_marker: DISK_LOCKDOWN_CONFIRMATION.to_string(),
        }
    }

    #[test]
    fn requires_absolute_mount_root_and_safe_account_names() {
        let mut relative = request();
        relative.mount_root = PathBuf::from("srv/das");
        assert!(matches!(
            relative.validate(),
            Err(DiskLockdownValidationError::RelativePath { .. })
        ));

        let mut unsafe_name = request();
        unsafe_name.service_user = "das objectstore".to_string();
        assert_eq!(
            unsafe_name.validate(),
            Err(DiskLockdownValidationError::UnsafeName {
                field: "service_user"
            })
        );
    }

    #[test]
    fn dry_run_is_safe_without_confirmation_but_mutation_requires_marker() {
        let mut dry_run = request();
        dry_run.dry_run = true;
        dry_run.confirmation_marker.clear();
        assert!(dry_run.validate().is_ok());

        let mut mutation = request();
        mutation.confirmation_marker.clear();
        assert_eq!(
            mutation.validate(),
            Err(DiskLockdownValidationError::ConfirmationMismatch)
        );
    }
}
