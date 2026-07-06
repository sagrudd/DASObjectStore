use crate::HostMode;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

pub const DASOBJECTSTORE_PRODUCT_ID: &str = "dasobjectstore";
pub const DASOBJECTSTORE_PRODUCT_NAME: &str = "DASObjectStore";
pub const DASOBJECTSTORE_WEB_MOUNT: &str = "/products/dasobjectstore";
pub const DASOBJECTSTORE_API_MOUNT: &str = "/products/dasobjectstore/api";
pub const PRODUCT_UI_BOOTSTRAP_SCHEMA_VERSION: &str = "mnemosyne.product_ui.bootstrap.v1";
pub const PRODUCT_UI_BOOTSTRAP_WELL_KNOWN_SUFFIX: &str =
    "/.well-known/mnemosyne/product-bootstrap.json";
pub const CORRELATION_ID_HEADER: &str = "x-mnemosyne-correlation-id";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProductUiBootstrapMetadata {
    pub schema_version: String,
    pub product_id: String,
    pub product_name: String,
    pub product_version: String,
    pub host_mode: HostMode,
    pub web_mount: String,
    pub api_mount: String,
    pub bootstrap_path: String,
    pub host_capabilities: Vec<ProductUiHostCapability>,
    pub correlation: ProductUiCorrelationPolicy,
    pub visibility: ProductUiVisibility,
    pub navigation: Vec<ProductUiNavigationItem>,
}

impl ProductUiBootstrapMetadata {
    pub fn validate(&self) -> Result<(), ProductUiBootstrapError> {
        if self.schema_version != PRODUCT_UI_BOOTSTRAP_SCHEMA_VERSION {
            return Err(ProductUiBootstrapError::InvalidSchemaVersion {
                value: self.schema_version.clone(),
            });
        }
        validate_identifier("product_id", &self.product_id)?;
        validate_non_empty("product_name", &self.product_name)?;
        validate_non_empty("product_version", &self.product_version)?;
        validate_mount("web_mount", &self.web_mount, true)?;
        validate_mount("api_mount", &self.api_mount, false)?;
        validate_mount("bootstrap_path", &self.bootstrap_path, false)?;
        if !self
            .bootstrap_path
            .ends_with(PRODUCT_UI_BOOTSTRAP_WELL_KNOWN_SUFFIX)
        {
            return Err(ProductUiBootstrapError::InvalidField {
                field: "bootstrap_path",
                reason: format!("must end with {PRODUCT_UI_BOOTSTRAP_WELL_KNOWN_SUFFIX}"),
            });
        }
        self.correlation.validate()?;
        self.visibility.validate()?;
        for item in &self.navigation {
            item.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductUiHostCapability {
    SynoptikonAccounts,
    SynoptikonEntitlements,
    SynoptikonCentralAudit,
    SynoptikonObjectStoreArtifacts,
    SynoptikonProjectRdbms,
    MonasLocalAccounts,
    MonasLocalAuditExport,
    MonasLocalJsonStorage,
    MonasLocalHardwareWorkflows,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductUiCorrelationMode {
    HostGenerated,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProductUiCorrelationPolicy {
    pub mode: ProductUiCorrelationMode,
    pub header_name: String,
}

impl ProductUiCorrelationPolicy {
    pub fn host_generated() -> Self {
        Self {
            mode: ProductUiCorrelationMode::HostGenerated,
            header_name: CORRELATION_ID_HEADER.to_string(),
        }
    }

    fn validate(&self) -> Result<(), ProductUiBootstrapError> {
        validate_non_empty("correlation.header_name", &self.header_name)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductUiVisibilityState {
    Visible,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProductUiVisibility {
    pub state: ProductUiVisibilityState,
    pub reason: Option<String>,
}

impl ProductUiVisibility {
    pub fn visible() -> Self {
        Self {
            state: ProductUiVisibilityState::Visible,
            reason: None,
        }
    }

    fn validate(&self) -> Result<(), ProductUiBootstrapError> {
        if self.state == ProductUiVisibilityState::Visible && self.reason.is_some() {
            return Err(ProductUiBootstrapError::InvalidField {
                field: "visibility.reason",
                reason: "must be empty for visible products".to_string(),
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProductUiNavigationItem {
    pub route_id: String,
    pub label: String,
    pub path: String,
    pub required_capabilities: Vec<ProductUiHostCapability>,
    pub host_modes: Vec<HostMode>,
    pub order: u16,
}

impl ProductUiNavigationItem {
    pub fn new(route_id: impl Into<String>, label: impl Into<String>, order: u16) -> Self {
        let route_id = route_id.into();
        Self {
            path: format!("{DASOBJECTSTORE_WEB_MOUNT}/{route_id}"),
            route_id,
            label: label.into(),
            required_capabilities: Vec::new(),
            host_modes: Vec::new(),
            order,
        }
    }

    pub fn for_host_modes<I>(mut self, host_modes: I) -> Self
    where
        I: IntoIterator<Item = HostMode>,
    {
        self.host_modes = host_modes.into_iter().collect();
        self
    }

    fn validate(&self) -> Result<(), ProductUiBootstrapError> {
        validate_identifier("navigation.route_id", &self.route_id)?;
        validate_non_empty("navigation.label", &self.label)?;
        validate_mount("navigation.path", &self.path, true)?;
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProductUiBootstrapError {
    InvalidSchemaVersion { value: String },
    InvalidField { field: &'static str, reason: String },
}

impl Display for ProductUiBootstrapError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSchemaVersion { value } => write!(
                formatter,
                "product UI bootstrap schema_version must be {PRODUCT_UI_BOOTSTRAP_SCHEMA_VERSION}: {value}"
            ),
            Self::InvalidField { field, reason } => {
                write!(formatter, "invalid product UI bootstrap field {field}: {reason}")
            }
        }
    }
}

impl std::error::Error for ProductUiBootstrapError {}

pub fn export_synoptikon_product_ui_bootstrap(
) -> Result<ProductUiBootstrapMetadata, ProductUiBootstrapError> {
    export_product_ui_bootstrap(HostMode::SynoptikonIntegrated)
}

pub fn export_product_ui_bootstrap(
    host_mode: HostMode,
) -> Result<ProductUiBootstrapMetadata, ProductUiBootstrapError> {
    let metadata = ProductUiBootstrapMetadata {
        schema_version: PRODUCT_UI_BOOTSTRAP_SCHEMA_VERSION.to_string(),
        product_id: DASOBJECTSTORE_PRODUCT_ID.to_string(),
        product_name: DASOBJECTSTORE_PRODUCT_NAME.to_string(),
        product_version: env!("CARGO_PKG_VERSION").to_string(),
        host_mode,
        web_mount: DASOBJECTSTORE_WEB_MOUNT.to_string(),
        api_mount: DASOBJECTSTORE_API_MOUNT.to_string(),
        bootstrap_path: format!(
            "{DASOBJECTSTORE_WEB_MOUNT}{PRODUCT_UI_BOOTSTRAP_WELL_KNOWN_SUFFIX}"
        ),
        host_capabilities: host_capabilities(host_mode),
        correlation: ProductUiCorrelationPolicy::host_generated(),
        visibility: ProductUiVisibility::visible(),
        navigation: operations_navigation(host_mode),
    };
    metadata.validate()?;
    Ok(metadata)
}

pub fn operations_navigation(host_mode: HostMode) -> Vec<ProductUiNavigationItem> {
    [
        ("overview", "Overview"),
        ("disks", "Disks"),
        ("stores", "Stores"),
        ("objects", "Objects"),
        ("endpoints", "Endpoints"),
        ("activity", "Activity"),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (route_id, label))| {
        ProductUiNavigationItem::new(route_id, label, ((index + 1) * 10) as u16)
            .for_host_modes([host_mode])
    })
    .collect()
}

pub fn bootstrap_path_for_web_mount(web_mount: &str) -> Result<String, ProductUiBootstrapError> {
    validate_mount("web_mount", web_mount, true)?;
    Ok(format!(
        "{}{PRODUCT_UI_BOOTSTRAP_WELL_KNOWN_SUFFIX}",
        web_mount.trim_end_matches('/')
    ))
}

fn host_capabilities(host_mode: HostMode) -> Vec<ProductUiHostCapability> {
    match host_mode {
        HostMode::SynoptikonIntegrated => vec![
            ProductUiHostCapability::SynoptikonAccounts,
            ProductUiHostCapability::SynoptikonEntitlements,
            ProductUiHostCapability::SynoptikonCentralAudit,
            ProductUiHostCapability::SynoptikonObjectStoreArtifacts,
            ProductUiHostCapability::SynoptikonProjectRdbms,
        ],
        HostMode::MonasStandalone => vec![
            ProductUiHostCapability::MonasLocalAccounts,
            ProductUiHostCapability::MonasLocalAuditExport,
            ProductUiHostCapability::MonasLocalJsonStorage,
            ProductUiHostCapability::MonasLocalHardwareWorkflows,
        ],
    }
}

fn validate_mount(
    field: &'static str,
    value: &str,
    allow_trailing_slash: bool,
) -> Result<(), ProductUiBootstrapError> {
    validate_non_empty(field, value)?;
    if !value.starts_with('/') {
        return Err(ProductUiBootstrapError::InvalidField {
            field,
            reason: "must start with /".to_string(),
        });
    }
    if !allow_trailing_slash && (value == "/" || value.ends_with('/')) {
        return Err(ProductUiBootstrapError::InvalidField {
            field,
            reason: "must be a non-root path without trailing slash".to_string(),
        });
    }
    Ok(())
}

fn validate_identifier(field: &'static str, value: &str) -> Result<(), ProductUiBootstrapError> {
    validate_non_empty(field, value)?;
    let mut chars = value.chars();
    let first = chars.next().expect("identifier checked non-empty");
    if !first.is_ascii_lowercase() {
        return Err(ProductUiBootstrapError::InvalidField {
            field,
            reason: "must start with a lowercase ASCII letter".to_string(),
        });
    }
    if !chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-') {
        return Err(ProductUiBootstrapError::InvalidField {
            field,
            reason: "contains unsupported characters".to_string(),
        });
    }
    Ok(())
}

fn validate_non_empty(field: &'static str, value: &str) -> Result<(), ProductUiBootstrapError> {
    if value.is_empty() {
        Err(ProductUiBootstrapError::InvalidField {
            field,
            reason: "must not be empty".to_string(),
        })
    } else {
        Ok(())
    }
}
