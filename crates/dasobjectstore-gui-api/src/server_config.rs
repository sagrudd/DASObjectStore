use crate::GuiApiHostMode;
use dasobjectstore_core::{
    DEFAULT_PRODUCT_ROOT, DEFAULT_STANDALONE_BIND_ADDRESS, DEFAULT_STANDALONE_HTTPS_PORT,
};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};

pub const DEFAULT_STANDALONE_PUBLIC_BASE_URL: &str = "https://127.0.0.1:8448";
pub const DEFAULT_TLS_CERTIFICATE_RELATIVE_PATH: &str = "tls/server.crt";
pub const DEFAULT_TLS_PRIVATE_KEY_RELATIVE_PATH: &str = "tls/server.key";
pub const DEFAULT_MTLS_HTTPS_PORT: u16 = 8449;
pub const DEFAULT_S3_INGRESS_PORT: u16 = 3900;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StandaloneServerConfig {
    pub bind_address: String,
    pub https_port: u16,
    pub public_base_url: String,
    pub product_root: PathBuf,
    #[serde(default)]
    pub authentication: StandaloneAuthenticationConfig,
    pub tls: StandaloneTlsConfig,
    #[serde(default)]
    pub application_mtls: StandaloneMutualTlsConfig,
    #[serde(default)]
    pub s3_ingress: StandaloneS3IngressConfig,
}

impl StandaloneServerConfig {
    pub fn default_localhost() -> Self {
        Self::with_bind_address(DEFAULT_STANDALONE_BIND_ADDRESS)
    }

    pub fn with_bind_address(bind_address: impl Into<String>) -> Self {
        let product_root = PathBuf::from(DEFAULT_PRODUCT_ROOT);
        let tls = StandaloneTlsConfig::under_product_root(&product_root);

        Self {
            bind_address: bind_address.into(),
            https_port: DEFAULT_STANDALONE_HTTPS_PORT,
            public_base_url: DEFAULT_STANDALONE_PUBLIC_BASE_URL.to_string(),
            product_root,
            authentication: StandaloneAuthenticationConfig::default(),
            tls,
            application_mtls: StandaloneMutualTlsConfig::default(),
            s3_ingress: StandaloneS3IngressConfig::default(),
        }
    }

    pub fn socket_addr(&self) -> Result<SocketAddr, StandaloneServerConfigError> {
        let ip_addr = self.bind_address.parse::<IpAddr>().map_err(|_| {
            StandaloneServerConfigError::InvalidBindAddress {
                bind_address: self.bind_address.clone(),
            }
        })?;

        validate_https_port(self.https_port)?;

        Ok(SocketAddr::new(ip_addr, self.https_port))
    }

    pub fn validate(&self) -> Result<(), StandaloneServerConfigError> {
        self.socket_addr()?;
        validate_public_base_url(&self.public_base_url)?;
        validate_absolute_path("product_root", &self.product_root)?;
        self.tls.validate()?;
        self.authentication.validate()?;
        self.application_mtls.validate(self.https_port)?;
        self.s3_ingress
            .validate(self.https_port, &self.application_mtls)?;
        Ok(())
    }

    pub fn gui_api_host_mode(&self) -> GuiApiHostMode {
        self.authentication.gui_api_host_mode()
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StandaloneS3IngressMode {
    #[default]
    GarageLegacy,
    DirectGateway,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StandaloneS3IngressConfig {
    #[serde(default)]
    pub mode: StandaloneS3IngressMode,
    #[serde(default = "default_s3_ingress_bind_address")]
    pub bind_address: String,
    #[serde(default = "default_s3_ingress_port")]
    pub port: u16,
    #[serde(default = "default_s3_ingress_upstream")]
    pub legacy_upstream_endpoint: String,
    #[serde(default = "default_s3_max_concurrent_uploads")]
    pub max_concurrent_uploads: usize,
}

impl StandaloneS3IngressConfig {
    pub fn enabled(&self) -> bool {
        self.mode == StandaloneS3IngressMode::DirectGateway
    }

    pub fn socket_addr(&self) -> Result<SocketAddr, StandaloneServerConfigError> {
        let address = self.bind_address.parse::<IpAddr>().map_err(|_| {
            StandaloneServerConfigError::InvalidS3IngressBindAddress {
                bind_address: self.bind_address.clone(),
            }
        })?;
        validate_https_port(self.port)?;
        Ok(SocketAddr::new(address, self.port))
    }

    fn validate(
        &self,
        primary_port: u16,
        application_mtls: &StandaloneMutualTlsConfig,
    ) -> Result<(), StandaloneServerConfigError> {
        if !self.enabled() {
            return Ok(());
        }
        self.socket_addr()?;
        if self.port == primary_port
            || (application_mtls.enabled && self.port == application_mtls.https_port)
        {
            return Err(StandaloneServerConfigError::DuplicateListenerPort {
                https_port: self.port,
            });
        }
        let upstream = self
            .legacy_upstream_endpoint
            .strip_prefix("http://")
            .or_else(|| self.legacy_upstream_endpoint.strip_prefix("https://"))
            .ok_or_else(|| StandaloneServerConfigError::InvalidS3LegacyUpstream {
                endpoint: self.legacy_upstream_endpoint.clone(),
            })?;
        if upstream.trim().is_empty() {
            return Err(StandaloneServerConfigError::InvalidS3LegacyUpstream {
                endpoint: self.legacy_upstream_endpoint.clone(),
            });
        }
        if self.max_concurrent_uploads == 0 || self.max_concurrent_uploads > 256 {
            return Err(StandaloneServerConfigError::InvalidS3Concurrency {
                value: self.max_concurrent_uploads,
            });
        }
        Ok(())
    }
}

impl Default for StandaloneS3IngressConfig {
    fn default() -> Self {
        Self {
            mode: StandaloneS3IngressMode::GarageLegacy,
            bind_address: default_s3_ingress_bind_address(),
            port: default_s3_ingress_port(),
            legacy_upstream_endpoint: default_s3_ingress_upstream(),
            max_concurrent_uploads: default_s3_max_concurrent_uploads(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StandaloneMutualTlsConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_mtls_bind_address")]
    pub bind_address: String,
    #[serde(default = "default_mtls_https_port")]
    pub https_port: u16,
    #[serde(default)]
    pub client_ca_path: PathBuf,
    /// Backward-compatible decode-only field. Registry custody belongs to the daemon.
    #[serde(
        default = "default_application_identity_registry_path",
        skip_serializing
    )]
    pub application_identity_registry_path: PathBuf,
    /// Backward-compatible decode-only field. Registry custody belongs to the daemon.
    #[serde(default = "default_application_key_registry_path", skip_serializing)]
    pub application_key_registry_path: PathBuf,
}

impl StandaloneMutualTlsConfig {
    pub fn validate(&self, primary_https_port: u16) -> Result<(), StandaloneServerConfigError> {
        if !self.enabled {
            return Ok(());
        }
        self.bind_address.parse::<IpAddr>().map_err(|_| {
            StandaloneServerConfigError::InvalidMutualTlsBindAddress {
                bind_address: self.bind_address.clone(),
            }
        })?;
        validate_https_port(self.https_port)?;
        if self.https_port == primary_https_port {
            return Err(StandaloneServerConfigError::DuplicateListenerPort {
                https_port: self.https_port,
            });
        }
        validate_absolute_path("application_mtls.client_ca_path", &self.client_ca_path)?;
        Ok(())
    }

    pub fn socket_addr(&self) -> Result<SocketAddr, StandaloneServerConfigError> {
        let address = self.bind_address.parse::<IpAddr>().map_err(|_| {
            StandaloneServerConfigError::InvalidMutualTlsBindAddress {
                bind_address: self.bind_address.clone(),
            }
        })?;
        Ok(SocketAddr::new(address, self.https_port))
    }
}

impl Default for StandaloneMutualTlsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind_address: default_mtls_bind_address(),
            https_port: default_mtls_https_port(),
            client_ca_path: PathBuf::new(),
            application_identity_registry_path: default_application_identity_registry_path(),
            application_key_registry_path: default_application_key_registry_path(),
        }
    }
}

impl Default for StandaloneServerConfig {
    fn default() -> Self {
        Self::default_localhost()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StandaloneTlsConfig {
    pub certificate_path: PathBuf,
    pub private_key_path: PathBuf,
}

impl StandaloneTlsConfig {
    pub fn under_product_root(product_root: &Path) -> Self {
        Self {
            certificate_path: product_root.join(DEFAULT_TLS_CERTIFICATE_RELATIVE_PATH),
            private_key_path: product_root.join(DEFAULT_TLS_PRIVATE_KEY_RELATIVE_PATH),
        }
    }

    pub fn validate(&self) -> Result<(), StandaloneServerConfigError> {
        validate_absolute_path("certificate_path", &self.certificate_path)?;
        validate_absolute_path("private_key_path", &self.private_key_path)?;
        if self.certificate_path == self.private_key_path {
            return Err(StandaloneServerConfigError::DuplicateTlsAssetPath);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StandaloneAuthenticationConfig {
    #[serde(default)]
    pub authority: StandaloneAuthenticationAuthority,
    #[serde(default = "default_session_ttl_seconds")]
    pub session_ttl_seconds: i64,
}

impl StandaloneAuthenticationConfig {
    pub fn validate(&self) -> Result<(), StandaloneServerConfigError> {
        if self.session_ttl_seconds <= 0 {
            return Err(StandaloneServerConfigError::InvalidSessionTtlSeconds {
                session_ttl_seconds: self.session_ttl_seconds,
            });
        }
        Ok(())
    }

    pub fn gui_api_host_mode(&self) -> GuiApiHostMode {
        match self.authority {
            StandaloneAuthenticationAuthority::LocalUser => GuiApiHostMode::Standalone,
            StandaloneAuthenticationAuthority::Synoptikon
            | StandaloneAuthenticationAuthority::Monas => GuiApiHostMode::SynoptikonIntegrated,
        }
    }
}

impl Default for StandaloneAuthenticationConfig {
    fn default() -> Self {
        Self {
            authority: StandaloneAuthenticationAuthority::LocalUser,
            session_ttl_seconds: default_session_ttl_seconds(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StandaloneAuthenticationAuthority {
    LocalUser,
    Synoptikon,
    Monas,
}

impl Default for StandaloneAuthenticationAuthority {
    fn default() -> Self {
        Self::LocalUser
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StandaloneServerConfigError {
    InvalidBindAddress { bind_address: String },
    InvalidHttpsPort { https_port: u16 },
    InvalidPublicBaseUrl { public_base_url: String },
    RelativePath { field: &'static str, path: PathBuf },
    DuplicateTlsAssetPath,
    InvalidSessionTtlSeconds { session_ttl_seconds: i64 },
    InvalidMutualTlsBindAddress { bind_address: String },
    DuplicateListenerPort { https_port: u16 },
    InvalidS3IngressBindAddress { bind_address: String },
    InvalidS3LegacyUpstream { endpoint: String },
    InvalidS3Concurrency { value: usize },
}

impl Display for StandaloneServerConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBindAddress { bind_address } => {
                write!(
                    formatter,
                    "invalid bind address for standalone server: {bind_address}"
                )
            }
            Self::InvalidHttpsPort { https_port } => {
                write!(
                    formatter,
                    "invalid HTTPS port for standalone server: {https_port}"
                )
            }
            Self::InvalidPublicBaseUrl { public_base_url } => {
                write!(
                    formatter,
                    "standalone public_base_url must be an https URL: {public_base_url}"
                )
            }
            Self::RelativePath { field, path } => {
                write!(formatter, "{field} must be absolute: {}", path.display())
            }
            Self::DuplicateTlsAssetPath => {
                write!(
                    formatter,
                    "TLS certificate_path and private_key_path must be distinct"
                )
            }
            Self::InvalidSessionTtlSeconds {
                session_ttl_seconds,
            } => {
                write!(
                    formatter,
                    "authentication session_ttl_seconds must be positive: {session_ttl_seconds}"
                )
            }
            Self::InvalidMutualTlsBindAddress { bind_address } => write!(
                formatter,
                "invalid application mTLS bind address: {bind_address}"
            ),
            Self::DuplicateListenerPort { https_port } => write!(
                formatter,
                "listener port must be unique across HTTPS, mTLS, and S3 ingress: {https_port}"
            ),
            Self::InvalidS3IngressBindAddress { bind_address } => {
                write!(
                    formatter,
                    "invalid direct S3 ingress bind address: {bind_address}"
                )
            }
            Self::InvalidS3LegacyUpstream { endpoint } => {
                write!(
                    formatter,
                    "invalid legacy Garage upstream endpoint: {endpoint}"
                )
            }
            Self::InvalidS3Concurrency { value } => write!(
                formatter,
                "direct S3 max_concurrent_uploads must be between 1 and 256: {value}"
            ),
        }
    }
}

fn default_session_ttl_seconds() -> i64 {
    60 * 60
}

fn default_mtls_bind_address() -> String {
    DEFAULT_STANDALONE_BIND_ADDRESS.to_string()
}

fn default_mtls_https_port() -> u16 {
    DEFAULT_MTLS_HTTPS_PORT
}

fn default_s3_ingress_bind_address() -> String {
    DEFAULT_STANDALONE_BIND_ADDRESS.to_string()
}

fn default_s3_ingress_port() -> u16 {
    DEFAULT_S3_INGRESS_PORT
}

fn default_s3_ingress_upstream() -> String {
    "http://127.0.0.1:3901".to_string()
}

fn default_s3_max_concurrent_uploads() -> usize {
    8
}

fn default_application_identity_registry_path() -> PathBuf {
    PathBuf::from(dasobjectstore_daemon::runtime::DEFAULT_DAEMON_STATE_DIR)
        .join(dasobjectstore_daemon::runtime::APPLICATION_IDENTITY_REGISTRY_FILE_NAME)
}

fn default_application_key_registry_path() -> PathBuf {
    PathBuf::from(dasobjectstore_daemon::runtime::DEFAULT_DAEMON_STATE_DIR)
        .join(dasobjectstore_daemon::runtime::APPLICATION_KEY_REGISTRY_FILE_NAME)
}

impl std::error::Error for StandaloneServerConfigError {}

fn validate_https_port(https_port: u16) -> Result<(), StandaloneServerConfigError> {
    if https_port == 0 {
        return Err(StandaloneServerConfigError::InvalidHttpsPort { https_port });
    }
    Ok(())
}

fn validate_public_base_url(public_base_url: &str) -> Result<(), StandaloneServerConfigError> {
    if !public_base_url.starts_with("https://") || public_base_url.trim() == "https://" {
        return Err(StandaloneServerConfigError::InvalidPublicBaseUrl {
            public_base_url: public_base_url.to_string(),
        });
    }
    Ok(())
}

fn validate_absolute_path(
    field: &'static str,
    path: &Path,
) -> Result<(), StandaloneServerConfigError> {
    if !path.is_absolute() {
        return Err(StandaloneServerConfigError::RelativePath {
            field,
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        StandaloneServerConfig, StandaloneServerConfigError, StandaloneTlsConfig,
        DEFAULT_STANDALONE_PUBLIC_BASE_URL,
    };
    use dasobjectstore_core::{
        DEFAULT_PRODUCT_ROOT, DEFAULT_STANDALONE_BIND_ADDRESS, DEFAULT_STANDALONE_HTTPS_PORT,
    };
    use std::path::PathBuf;

    #[test]
    fn defaults_to_localhost_https_port_8448() {
        let config = StandaloneServerConfig::default();

        assert_eq!(config.bind_address, DEFAULT_STANDALONE_BIND_ADDRESS);
        assert_eq!(config.https_port, DEFAULT_STANDALONE_HTTPS_PORT);
        assert_eq!(config.public_base_url, DEFAULT_STANDALONE_PUBLIC_BASE_URL);
        assert_eq!(config.product_root, PathBuf::from(DEFAULT_PRODUCT_ROOT));
        assert_eq!(
            config.authentication.authority,
            super::StandaloneAuthenticationAuthority::LocalUser
        );
        assert_eq!(
            config.socket_addr().expect("socket address"),
            "127.0.0.1:8448".parse().expect("expected socket address")
        );
        config.validate().expect("default config is valid");
    }

    #[test]
    fn allows_explicit_linux_appliance_bind_address() {
        let config = StandaloneServerConfig::with_bind_address("0.0.0.0");

        assert_eq!(
            config.socket_addr().expect("socket address"),
            "0.0.0.0:8448".parse().expect("expected socket address")
        );
    }

    #[test]
    fn rejects_port_zero() {
        let config = StandaloneServerConfig {
            https_port: 0,
            ..StandaloneServerConfig::default()
        };

        assert_eq!(
            config.validate().expect_err("port zero rejected"),
            StandaloneServerConfigError::InvalidHttpsPort { https_port: 0 }
        );
    }

    #[test]
    fn rejects_non_https_public_base_url() {
        let config = StandaloneServerConfig {
            public_base_url: "http://127.0.0.1:8448".to_string(),
            ..StandaloneServerConfig::default()
        };

        assert_eq!(
            config.validate().expect_err("http URL rejected"),
            StandaloneServerConfigError::InvalidPublicBaseUrl {
                public_base_url: "http://127.0.0.1:8448".to_string()
            }
        );
    }

    #[test]
    fn rejects_relative_tls_paths() {
        let tls = StandaloneTlsConfig {
            certificate_path: PathBuf::from("tls/server.crt"),
            private_key_path: PathBuf::from("/opt/dasobjectstore/tls/server.key"),
        };

        assert_eq!(
            tls.validate().expect_err("relative certificate rejected"),
            StandaloneServerConfigError::RelativePath {
                field: "certificate_path",
                path: PathBuf::from("tls/server.crt"),
            }
        );
    }

    #[test]
    fn serializes_default_server_config() {
        let encoded = serde_json::to_value(StandaloneServerConfig::default())
            .expect("server config serializes");

        assert_eq!(encoded["bind_address"], "127.0.0.1");
        assert_eq!(encoded["https_port"], 8448);
        assert_eq!(encoded["public_base_url"], "https://127.0.0.1:8448");
        assert_eq!(
            encoded["tls"]["certificate_path"],
            "/opt/dasobjectstore/tls/server.crt"
        );
        assert_eq!(encoded["authentication"]["authority"], "local_user");
        assert_eq!(encoded["authentication"]["session_ttl_seconds"], 3600);
        assert_eq!(encoded["application_mtls"]["enabled"], false);
        assert_eq!(encoded["application_mtls"]["https_port"], 8449);
        assert_eq!(encoded["s3_ingress"]["mode"], "garage_legacy");
        assert_eq!(encoded["s3_ingress"]["port"], 3900);
    }

    #[test]
    fn maps_external_authentication_to_integrated_host_mode() {
        let mut config = StandaloneServerConfig::default();
        config.authentication.authority = super::StandaloneAuthenticationAuthority::Synoptikon;
        assert_eq!(
            config.gui_api_host_mode(),
            crate::GuiApiHostMode::SynoptikonIntegrated
        );
    }

    #[test]
    fn defaults_partial_authentication_config_to_local_user() {
        let config: StandaloneServerConfig = serde_json::from_value(serde_json::json!({
            "bind_address": "127.0.0.1",
            "https_port": 8448,
            "public_base_url": "https://127.0.0.1:8448",
            "product_root": "/opt/dasobjectstore",
            "authentication": {},
            "tls": {
                "certificate_path": "/opt/dasobjectstore/tls/server.crt",
                "private_key_path": "/opt/dasobjectstore/tls/server.key"
            }
        }))
        .expect("partial auth config parses");

        assert_eq!(
            config.authentication.authority,
            super::StandaloneAuthenticationAuthority::LocalUser
        );
        assert_eq!(config.authentication.session_ttl_seconds, 3600);
    }

    #[test]
    fn rejects_non_positive_session_ttl() {
        let config = StandaloneServerConfig {
            authentication: super::StandaloneAuthenticationConfig {
                authority: super::StandaloneAuthenticationAuthority::LocalUser,
                session_ttl_seconds: 0,
            },
            ..StandaloneServerConfig::default()
        };

        assert_eq!(
            config.validate().expect_err("ttl rejected"),
            StandaloneServerConfigError::InvalidSessionTtlSeconds {
                session_ttl_seconds: 0
            }
        );
    }

    #[test]
    fn validates_enabled_application_mtls_listener() {
        let mut config = StandaloneServerConfig::default();
        config.application_mtls.enabled = true;
        config.application_mtls.client_ca_path = PathBuf::from("/etc/dasobjectstore/client-ca.crt");
        config.validate().expect("mTLS config is valid");
        assert_eq!(
            config.application_mtls.socket_addr().expect("mTLS address"),
            "127.0.0.1:8449".parse().expect("address")
        );
    }

    #[test]
    fn validates_feature_gated_direct_s3_listener() {
        let mut config = StandaloneServerConfig::default();
        config.s3_ingress.mode = super::StandaloneS3IngressMode::DirectGateway;
        config.validate().expect("direct S3 config is valid");
        assert_eq!(
            config.s3_ingress.socket_addr().expect("S3 address"),
            "127.0.0.1:3900".parse().expect("address")
        );

        config.s3_ingress.port = config.https_port;
        assert!(matches!(
            config.validate(),
            Err(StandaloneServerConfigError::DuplicateListenerPort { .. })
        ));
    }

    #[test]
    fn old_server_config_without_s3_section_stays_on_legacy_gateway() {
        let config: StandaloneServerConfig = serde_json::from_value(serde_json::json!({
            "bind_address": "127.0.0.1",
            "https_port": 8448,
            "public_base_url": "https://127.0.0.1:8448",
            "product_root": "/opt/dasobjectstore",
            "tls": {
                "certificate_path": "/opt/dasobjectstore/tls/server.crt",
                "private_key_path": "/opt/dasobjectstore/tls/server.key"
            }
        }))
        .expect("legacy server config parses");

        assert_eq!(
            config.s3_ingress.mode,
            super::StandaloneS3IngressMode::GarageLegacy
        );
        assert!(!config.s3_ingress.enabled());
        config.validate().expect("legacy config remains valid");
    }

    #[test]
    fn direct_s3_listener_rejects_invalid_address_upstream_and_budget() {
        let mut config = StandaloneServerConfig::default();
        config.s3_ingress.mode = super::StandaloneS3IngressMode::DirectGateway;

        config.s3_ingress.bind_address = "not-an-address".to_string();
        assert!(matches!(
            config.validate(),
            Err(StandaloneServerConfigError::InvalidS3IngressBindAddress { .. })
        ));

        config.s3_ingress.bind_address = "127.0.0.1".to_string();
        config.s3_ingress.legacy_upstream_endpoint = "127.0.0.1:3901".to_string();
        assert!(matches!(
            config.validate(),
            Err(StandaloneServerConfigError::InvalidS3LegacyUpstream { .. })
        ));

        config.s3_ingress.legacy_upstream_endpoint = "http://127.0.0.1:3901".to_string();
        for invalid in [0, 257] {
            config.s3_ingress.max_concurrent_uploads = invalid;
            assert_eq!(
                config.validate().expect_err("invalid budget rejected"),
                StandaloneServerConfigError::InvalidS3Concurrency { value: invalid }
            );
        }
    }

    #[test]
    fn enabled_application_mtls_fails_closed_on_unsafe_listener_config() {
        let mut config = StandaloneServerConfig::default();
        config.application_mtls.enabled = true;
        config.application_mtls.https_port = config.https_port;
        config.application_mtls.client_ca_path = PathBuf::from("/etc/dasobjectstore/client-ca.crt");
        assert_eq!(
            config.validate().expect_err("duplicate port rejected"),
            StandaloneServerConfigError::DuplicateListenerPort { https_port: 8448 }
        );

        config.application_mtls.https_port = 8449;
        config.application_mtls.client_ca_path = PathBuf::from("client-ca.crt");
        assert_eq!(
            config.validate().expect_err("relative CA rejected"),
            StandaloneServerConfigError::RelativePath {
                field: "application_mtls.client_ca_path",
                path: PathBuf::from("client-ca.crt")
            }
        );
    }
}
