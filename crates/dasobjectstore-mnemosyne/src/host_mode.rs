use crate::HostMode;
use dasobjectstore_core::{DEFAULT_PRODUCT_ROOT, DEFAULT_STANDALONE_HTTPS_PORT};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

pub const DASOBJECTSTORE_PRODUCT_ROOT: &str = DEFAULT_PRODUCT_ROOT;
pub const DASOBJECTSTORE_STANDALONE_HTTPS_PORT: u16 = DEFAULT_STANDALONE_HTTPS_PORT;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductHostMode {
    Standalone,
    SynoptikonIntegrated,
}

impl ProductHostMode {
    pub fn storage_boundary_host_mode(self) -> HostMode {
        match self {
            Self::Standalone => HostMode::MonasStandalone,
            Self::SynoptikonIntegrated => HostMode::SynoptikonIntegrated,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthenticationAuthority {
    LocalProduct,
    Synoptikon,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditAuthority {
    LocalProduct,
    SynoptikonCentralAudit,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageAuthority {
    LocalProductState,
    SynoptikonStorageBinding,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HostModeProfile {
    pub mode: ProductHostMode,
    pub product_root: Option<String>,
    pub public_https_port: Option<u16>,
    pub local_authentication: bool,
    pub local_hardware: bool,
    pub product_owned_login_routes: bool,
    pub requires_entitlement: bool,
    pub authentication_authority: AuthenticationAuthority,
    pub audit_authority: AuditAuthority,
    pub storage_authority: StorageAuthority,
    pub storage_boundary_host_mode: HostMode,
}

impl HostModeProfile {
    pub fn validate(&self) -> Result<(), HostModeProfileError> {
        match self.mode {
            ProductHostMode::Standalone => validate_standalone_profile(self),
            ProductHostMode::SynoptikonIntegrated => validate_synoptikon_profile(self),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostModeProfileError {
    InvalidStandaloneProfile(String),
    InvalidSynoptikonIntegratedProfile(String),
}

impl Display for HostModeProfileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidStandaloneProfile(reason) => {
                write!(formatter, "invalid standalone host-mode profile: {reason}")
            }
            Self::InvalidSynoptikonIntegratedProfile(reason) => write!(
                formatter,
                "invalid synoptikon_integrated host-mode profile: {reason}"
            ),
        }
    }
}

impl std::error::Error for HostModeProfileError {}

pub fn host_mode_profile(mode: ProductHostMode) -> Result<HostModeProfile, HostModeProfileError> {
    let profile = match mode {
        ProductHostMode::Standalone => standalone_host_mode_profile(),
        ProductHostMode::SynoptikonIntegrated => synoptikon_integrated_host_mode_profile(),
    };
    profile.validate()?;
    Ok(profile)
}

pub fn standalone_host_mode_profile() -> HostModeProfile {
    HostModeProfile {
        mode: ProductHostMode::Standalone,
        product_root: Some(DASOBJECTSTORE_PRODUCT_ROOT.to_string()),
        public_https_port: Some(DASOBJECTSTORE_STANDALONE_HTTPS_PORT),
        local_authentication: true,
        local_hardware: true,
        product_owned_login_routes: true,
        requires_entitlement: false,
        authentication_authority: AuthenticationAuthority::LocalProduct,
        audit_authority: AuditAuthority::LocalProduct,
        storage_authority: StorageAuthority::LocalProductState,
        storage_boundary_host_mode: ProductHostMode::Standalone.storage_boundary_host_mode(),
    }
}

pub fn synoptikon_integrated_host_mode_profile() -> HostModeProfile {
    HostModeProfile {
        mode: ProductHostMode::SynoptikonIntegrated,
        product_root: None,
        public_https_port: None,
        local_authentication: false,
        local_hardware: false,
        product_owned_login_routes: false,
        requires_entitlement: true,
        authentication_authority: AuthenticationAuthority::Synoptikon,
        audit_authority: AuditAuthority::SynoptikonCentralAudit,
        storage_authority: StorageAuthority::SynoptikonStorageBinding,
        storage_boundary_host_mode: ProductHostMode::SynoptikonIntegrated
            .storage_boundary_host_mode(),
    }
}

fn validate_standalone_profile(profile: &HostModeProfile) -> Result<(), HostModeProfileError> {
    if profile.product_root.as_deref() != Some(DASOBJECTSTORE_PRODUCT_ROOT) {
        return Err(HostModeProfileError::InvalidStandaloneProfile(
            "product_root must be /opt/dasobjectstore".to_string(),
        ));
    }
    if profile.public_https_port != Some(DASOBJECTSTORE_STANDALONE_HTTPS_PORT) {
        return Err(HostModeProfileError::InvalidStandaloneProfile(
            "public_https_port must be 8448".to_string(),
        ));
    }
    if !profile.local_authentication || !profile.product_owned_login_routes {
        return Err(HostModeProfileError::InvalidStandaloneProfile(
            "local authentication and product-owned login routes are required".to_string(),
        ));
    }
    if profile.requires_entitlement {
        return Err(HostModeProfileError::InvalidStandaloneProfile(
            "standalone mode must not require Synoptikon entitlement".to_string(),
        ));
    }
    if profile.authentication_authority != AuthenticationAuthority::LocalProduct {
        return Err(HostModeProfileError::InvalidStandaloneProfile(
            "authentication authority must be local_product".to_string(),
        ));
    }
    if profile.storage_boundary_host_mode != HostMode::MonasStandalone {
        return Err(HostModeProfileError::InvalidStandaloneProfile(
            "storage boundary host mode must map to monas_standalone".to_string(),
        ));
    }
    Ok(())
}

fn validate_synoptikon_profile(profile: &HostModeProfile) -> Result<(), HostModeProfileError> {
    if profile.product_root.is_some() || profile.public_https_port.is_some() {
        return Err(HostModeProfileError::InvalidSynoptikonIntegratedProfile(
            "integrated mode must not expose standalone product root or public port".to_string(),
        ));
    }
    if profile.local_authentication || profile.product_owned_login_routes {
        return Err(HostModeProfileError::InvalidSynoptikonIntegratedProfile(
            "integrated mode must not expose product-owned login routes".to_string(),
        ));
    }
    if !profile.requires_entitlement {
        return Err(HostModeProfileError::InvalidSynoptikonIntegratedProfile(
            "Synoptikon entitlement is required".to_string(),
        ));
    }
    if profile.authentication_authority != AuthenticationAuthority::Synoptikon {
        return Err(HostModeProfileError::InvalidSynoptikonIntegratedProfile(
            "authentication authority must be synoptikon".to_string(),
        ));
    }
    if profile.audit_authority != AuditAuthority::SynoptikonCentralAudit {
        return Err(HostModeProfileError::InvalidSynoptikonIntegratedProfile(
            "audit authority must be synoptikon_central_audit".to_string(),
        ));
    }
    if profile.storage_authority != StorageAuthority::SynoptikonStorageBinding {
        return Err(HostModeProfileError::InvalidSynoptikonIntegratedProfile(
            "storage authority must be synoptikon_storage_binding".to_string(),
        ));
    }
    if profile.storage_boundary_host_mode != HostMode::SynoptikonIntegrated {
        return Err(HostModeProfileError::InvalidSynoptikonIntegratedProfile(
            "storage boundary host mode must be synoptikon_integrated".to_string(),
        ));
    }
    Ok(())
}
