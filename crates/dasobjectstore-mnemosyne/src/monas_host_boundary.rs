use crate::{
    host_mode_profile, validation::validate_context_id_like, HostModeProfile, HostModeProfileError,
    ProductHostMode, StorageAuthority, DASOBJECTSTORE_PRODUCT_ROOT,
};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MonasStandaloneHostBoundaryContext {
    pub installation_id: String,
    pub profile_id: String,
    pub local_user_id: String,
    pub product_root: String,
    pub local_audit_export_enabled: bool,
    pub local_hardware_workflows_enabled: bool,
    pub local_state_store_enabled: bool,
    pub state_store_authority: StorageAuthority,
}

impl MonasStandaloneHostBoundaryContext {
    pub fn validate(&self) -> Result<(), MonasStandaloneHostBoundaryError> {
        validate_context_id("installation_id", &self.installation_id)?;
        validate_context_id("profile_id", &self.profile_id)?;
        validate_context_id("local_user_id", &self.local_user_id)?;
        if self.product_root != DASOBJECTSTORE_PRODUCT_ROOT {
            return Err(MonasStandaloneHostBoundaryError::InvalidProductRoot {
                value: self.product_root.clone(),
            });
        }
        if !self.local_audit_export_enabled {
            return Err(MonasStandaloneHostBoundaryError::MissingLocalAudit);
        }
        if !self.local_hardware_workflows_enabled {
            return Err(MonasStandaloneHostBoundaryError::MissingLocalHardwareWorkflows);
        }
        if !self.local_state_store_enabled {
            return Err(MonasStandaloneHostBoundaryError::MissingLocalStateStore);
        }
        if self.state_store_authority != StorageAuthority::LocalProductState {
            return Err(
                MonasStandaloneHostBoundaryError::InvalidStateStoreAuthority {
                    value: self.state_store_authority,
                },
            );
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MonasStandaloneHostBoundary {
    pub profile: HostModeProfile,
    pub context: MonasStandaloneHostBoundaryContext,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MonasStandaloneHostBoundaryError {
    HostModeProfile(HostModeProfileError),
    InvalidContextField { field: &'static str, reason: String },
    InvalidProductRoot { value: String },
    MissingLocalAudit,
    MissingLocalHardwareWorkflows,
    MissingLocalStateStore,
    InvalidStateStoreAuthority { value: StorageAuthority },
}

impl Display for MonasStandaloneHostBoundaryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HostModeProfile(err) => write!(formatter, "{err}"),
            Self::InvalidContextField { field, reason } => {
                write!(formatter, "invalid Monas context field {field}: {reason}")
            }
            Self::InvalidProductRoot { value } => write!(
                formatter,
                "Monas product root must be {DASOBJECTSTORE_PRODUCT_ROOT}: {value}"
            ),
            Self::MissingLocalAudit => {
                formatter.write_str("Monas local audit export must be enabled")
            }
            Self::MissingLocalHardwareWorkflows => {
                formatter.write_str("Monas local hardware workflows must be enabled")
            }
            Self::MissingLocalStateStore => {
                formatter.write_str("Monas local state store must be enabled")
            }
            Self::InvalidStateStoreAuthority { value } => write!(
                formatter,
                "state store authority must be local_product_state: {value:?}"
            ),
        }
    }
}

impl std::error::Error for MonasStandaloneHostBoundaryError {}

pub fn validate_monas_standalone_host_boundary(
    context: &MonasStandaloneHostBoundaryContext,
) -> Result<MonasStandaloneHostBoundary, MonasStandaloneHostBoundaryError> {
    let profile = host_mode_profile(ProductHostMode::Standalone)
        .map_err(MonasStandaloneHostBoundaryError::HostModeProfile)?;
    context.validate()?;
    Ok(MonasStandaloneHostBoundary {
        profile,
        context: context.clone(),
    })
}

fn validate_context_id(
    field: &'static str,
    value: &str,
) -> Result<(), MonasStandaloneHostBoundaryError> {
    validate_context_id_like(field, value).map_err(|err| {
        MonasStandaloneHostBoundaryError::InvalidContextField {
            field: err.field,
            reason: err.reason,
        }
    })
}
