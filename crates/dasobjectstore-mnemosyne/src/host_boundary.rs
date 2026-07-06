use crate::{
    host_mode_profile, HostModeProfile, HostModeProfileError, ProductHostMode, StorageAuthority,
    DASOBJECTSTORE_PRODUCT_ID,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt::{self, Display};

pub const REQUEST_CONTEXT_SCHEMA_VERSION: &str = "mnemosyne.request_context.v1";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SynoptikonIntegratedHostBoundaryContext {
    pub request_context_schema_version: String,
    pub product_id: String,
    pub tenant_id: String,
    pub account_id: String,
    pub user_id: String,
    pub project_id: String,
    pub entitlement_id: String,
    pub roles: Vec<String>,
    pub correlation_id: String,
    pub central_audit_enabled: bool,
    pub storage_authority: StorageAuthority,
    pub storage_binding_id: String,
}

impl SynoptikonIntegratedHostBoundaryContext {
    pub fn validate(&self) -> Result<(), SynoptikonIntegratedHostBoundaryError> {
        validate_request_context_schema(&self.request_context_schema_version)?;
        validate_product(&self.product_id)?;
        validate_context_id("tenant_id", &self.tenant_id)?;
        validate_context_id("account_id", &self.account_id)?;
        validate_context_id("user_id", &self.user_id)?;
        validate_context_id("project_id", &self.project_id)?;
        validate_context_id("entitlement_id", &self.entitlement_id)?;
        validate_context_id("correlation_id", &self.correlation_id)?;
        validate_roles(&self.roles)?;
        if !self.central_audit_enabled {
            return Err(SynoptikonIntegratedHostBoundaryError::MissingCentralAudit);
        }
        if self.storage_authority != StorageAuthority::SynoptikonStorageBinding {
            return Err(
                SynoptikonIntegratedHostBoundaryError::InvalidStorageAuthority {
                    value: self.storage_authority,
                },
            );
        }
        validate_context_id("storage_binding_id", &self.storage_binding_id)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SynoptikonIntegratedHostBoundary {
    pub profile: HostModeProfile,
    pub context: SynoptikonIntegratedHostBoundaryContext,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SynoptikonIntegratedHostBoundaryError {
    HostModeProfile(HostModeProfileError),
    InvalidRequestContextSchema { value: String },
    InvalidProductId { value: String },
    InvalidContextField { field: &'static str, reason: String },
    EmptyRoles,
    DuplicateRole { value: String },
    MissingCentralAudit,
    InvalidStorageAuthority { value: StorageAuthority },
}

impl Display for SynoptikonIntegratedHostBoundaryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HostModeProfile(err) => write!(formatter, "{err}"),
            Self::InvalidRequestContextSchema { value } => write!(
                formatter,
                "request context schema_version must be {REQUEST_CONTEXT_SCHEMA_VERSION}: {value}"
            ),
            Self::InvalidProductId { value } => {
                write!(
                    formatter,
                    "request context product_id must be dasobjectstore: {value}"
                )
            }
            Self::InvalidContextField { field, reason } => {
                write!(
                    formatter,
                    "invalid Synoptikon context field {field}: {reason}"
                )
            }
            Self::EmptyRoles => formatter.write_str("Synoptikon context roles must not be empty"),
            Self::DuplicateRole { value } => {
                write!(formatter, "Synoptikon context role must be unique: {value}")
            }
            Self::MissingCentralAudit => {
                formatter.write_str("Synoptikon central audit must be enabled")
            }
            Self::InvalidStorageAuthority { value } => write!(
                formatter,
                "storage authority must be synoptikon_storage_binding: {value:?}"
            ),
        }
    }
}

impl std::error::Error for SynoptikonIntegratedHostBoundaryError {}

pub fn validate_synoptikon_integrated_host_boundary(
    context: &SynoptikonIntegratedHostBoundaryContext,
) -> Result<SynoptikonIntegratedHostBoundary, SynoptikonIntegratedHostBoundaryError> {
    let profile = host_mode_profile(ProductHostMode::SynoptikonIntegrated)
        .map_err(SynoptikonIntegratedHostBoundaryError::HostModeProfile)?;
    context.validate()?;
    Ok(SynoptikonIntegratedHostBoundary {
        profile,
        context: context.clone(),
    })
}

fn validate_request_context_schema(
    value: &str,
) -> Result<(), SynoptikonIntegratedHostBoundaryError> {
    if value == REQUEST_CONTEXT_SCHEMA_VERSION {
        Ok(())
    } else {
        Err(
            SynoptikonIntegratedHostBoundaryError::InvalidRequestContextSchema {
                value: value.to_string(),
            },
        )
    }
}

fn validate_product(value: &str) -> Result<(), SynoptikonIntegratedHostBoundaryError> {
    if value == DASOBJECTSTORE_PRODUCT_ID {
        Ok(())
    } else {
        Err(SynoptikonIntegratedHostBoundaryError::InvalidProductId {
            value: value.to_string(),
        })
    }
}

fn validate_context_id(
    field: &'static str,
    value: &str,
) -> Result<(), SynoptikonIntegratedHostBoundaryError> {
    if value.is_empty() || value.len() > 128 {
        return Err(invalid_context_field(field, "must be 1-128 characters"));
    }

    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return Err(invalid_context_field(field, "must not be empty"));
    };
    if !first.is_ascii_alphanumeric() {
        return Err(invalid_context_field(
            field,
            "must start with an ASCII letter or digit",
        ));
    }
    if !chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | ':' | '-')) {
        return Err(invalid_context_field(
            field,
            "contains unsupported characters",
        ));
    }
    Ok(())
}

fn validate_roles(roles: &[String]) -> Result<(), SynoptikonIntegratedHostBoundaryError> {
    if roles.is_empty() {
        return Err(SynoptikonIntegratedHostBoundaryError::EmptyRoles);
    }

    let mut seen = BTreeSet::new();
    for role in roles {
        validate_role(role)?;
        if !seen.insert(role) {
            return Err(SynoptikonIntegratedHostBoundaryError::DuplicateRole {
                value: role.clone(),
            });
        }
    }
    Ok(())
}

fn validate_role(role: &str) -> Result<(), SynoptikonIntegratedHostBoundaryError> {
    if role.is_empty() || role.len() > 96 {
        return Err(invalid_context_field("roles", "must be 1-96 characters"));
    }

    let mut chars = role.chars();
    let Some(first) = chars.next() else {
        return Err(invalid_context_field("roles", "must not be empty"));
    };
    if !first.is_ascii_lowercase() {
        return Err(invalid_context_field(
            "roles",
            "must start with a lowercase ASCII letter",
        ));
    }
    if !chars
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '_' | ':' | '-'))
    {
        return Err(invalid_context_field(
            "roles",
            "contains unsupported characters",
        ));
    }
    Ok(())
}

fn invalid_context_field(
    field: &'static str,
    reason: &str,
) -> SynoptikonIntegratedHostBoundaryError {
    SynoptikonIntegratedHostBoundaryError::InvalidContextField {
        field,
        reason: reason.to_string(),
    }
}
