use super::appliance_telemetry::{
    validate_appliance_telemetry_cadence, APPLIANCE_TELEMETRY_NORMAL_CADENCE_SECONDS,
};
use dasobjectstore_core::DEFAULT_PRODUCT_ROOT;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};
use std::path::{Path, PathBuf};

pub const DEFAULT_DAEMON_SERVICE_USER: &str = "dasobjectstore";
pub const DEFAULT_DAEMON_GROUP: &str = "dasobjectstore";
pub const DEFAULT_DAEMON_SOCKET_FILE_NAME: &str = "dasobjectstored.sock";
pub const LINUX_DAEMON_CONFIG_PATH: &str = "/etc/dasobjectstore/daemon.json";
pub const LINUX_DAEMON_RUNTIME_DIR: &str = "/run/dasobjectstore";
pub const LINUX_DAEMON_STATE_DIR: &str = "/var/lib/dasobjectstore";
pub const LINUX_DAEMON_LOG_DIR: &str = "/var/log/dasobjectstore";

#[cfg(target_os = "macos")]
pub const DEFAULT_DAEMON_CONFIG_PATH: &str = "/usr/local/etc/dasobjectstore/daemon.json";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_DAEMON_CONFIG_PATH: &str = LINUX_DAEMON_CONFIG_PATH;

#[cfg(target_os = "macos")]
pub const DEFAULT_DAEMON_RUNTIME_DIR: &str = "/usr/local/var/run/dasobjectstore";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_DAEMON_RUNTIME_DIR: &str = LINUX_DAEMON_RUNTIME_DIR;

#[cfg(target_os = "macos")]
pub const DEFAULT_DAEMON_STATE_DIR: &str = "/usr/local/var/lib/dasobjectstore";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_DAEMON_STATE_DIR: &str = LINUX_DAEMON_STATE_DIR;

#[cfg(target_os = "macos")]
pub const DEFAULT_DAEMON_LOG_DIR: &str = "/usr/local/var/log/dasobjectstore";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_DAEMON_LOG_DIR: &str = LINUX_DAEMON_LOG_DIR;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonRuntimeConfig {
    pub service_user: String,
    pub service_group: String,
    pub config_path: PathBuf,
    pub runtime_dir: PathBuf,
    pub socket_path: PathBuf,
    pub state_dir: PathBuf,
    pub log_dir: PathBuf,
    pub product_root: PathBuf,
    #[serde(default)]
    pub telemetry: DaemonTelemetryRuntimeConfig,
}

impl DaemonRuntimeConfig {
    pub fn default_packaged() -> Self {
        let runtime_dir = PathBuf::from(DEFAULT_DAEMON_RUNTIME_DIR);
        Self {
            service_user: DEFAULT_DAEMON_SERVICE_USER.to_string(),
            service_group: DEFAULT_DAEMON_GROUP.to_string(),
            config_path: PathBuf::from(DEFAULT_DAEMON_CONFIG_PATH),
            socket_path: runtime_dir.join(DEFAULT_DAEMON_SOCKET_FILE_NAME),
            runtime_dir,
            state_dir: PathBuf::from(DEFAULT_DAEMON_STATE_DIR),
            log_dir: PathBuf::from(DEFAULT_DAEMON_LOG_DIR),
            product_root: PathBuf::from(DEFAULT_PRODUCT_ROOT),
            telemetry: DaemonTelemetryRuntimeConfig::default(),
        }
    }

    pub fn linux_packaged() -> Self {
        let runtime_dir = PathBuf::from(LINUX_DAEMON_RUNTIME_DIR);
        Self {
            service_user: DEFAULT_DAEMON_SERVICE_USER.to_string(),
            service_group: DEFAULT_DAEMON_GROUP.to_string(),
            config_path: PathBuf::from(LINUX_DAEMON_CONFIG_PATH),
            socket_path: runtime_dir.join(DEFAULT_DAEMON_SOCKET_FILE_NAME),
            runtime_dir,
            state_dir: PathBuf::from(LINUX_DAEMON_STATE_DIR),
            log_dir: PathBuf::from(LINUX_DAEMON_LOG_DIR),
            product_root: PathBuf::from(DEFAULT_PRODUCT_ROOT),
            telemetry: DaemonTelemetryRuntimeConfig::default(),
        }
    }

    pub fn validate(&self) -> Result<(), DaemonRuntimeConfigError> {
        reject_blank("service_user", &self.service_user)?;
        reject_blank("service_group", &self.service_group)?;
        validate_absolute_path("config_path", &self.config_path)?;
        validate_absolute_path("runtime_dir", &self.runtime_dir)?;
        validate_absolute_path("socket_path", &self.socket_path)?;
        validate_absolute_path("state_dir", &self.state_dir)?;
        validate_absolute_path("log_dir", &self.log_dir)?;
        validate_absolute_path("product_root", &self.product_root)?;
        self.telemetry.validate()?;

        if self.socket_path.parent() != Some(self.runtime_dir.as_path()) {
            return Err(DaemonRuntimeConfigError::SocketOutsideRuntimeDir {
                socket_path: self.socket_path.clone(),
                runtime_dir: self.runtime_dir.clone(),
            });
        }

        Ok(())
    }
}

impl Default for DaemonRuntimeConfig {
    fn default() -> Self {
        Self::default_packaged()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonTelemetryRuntimeConfig {
    pub enabled: bool,
    pub cadence_seconds: u64,
}

impl DaemonTelemetryRuntimeConfig {
    pub fn validate(&self) -> Result<(), DaemonRuntimeConfigError> {
        validate_appliance_telemetry_cadence(self.cadence_seconds).map_err(|_| {
            DaemonRuntimeConfigError::InvalidTelemetryCadenceSeconds(self.cadence_seconds)
        })
    }
}

impl Default for DaemonTelemetryRuntimeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            cadence_seconds: APPLIANCE_TELEMETRY_NORMAL_CADENCE_SECONDS,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DaemonRuntimeConfigError {
    BlankField {
        field: &'static str,
    },
    RelativePath {
        field: &'static str,
        path: PathBuf,
    },
    SocketOutsideRuntimeDir {
        socket_path: PathBuf,
        runtime_dir: PathBuf,
    },
    InvalidTelemetryCadenceSeconds(u64),
}

impl Display for DaemonRuntimeConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BlankField { field } => write!(formatter, "{field} must not be blank"),
            Self::RelativePath { field, path } => {
                write!(formatter, "{field} must be absolute: {}", path.display())
            }
            Self::SocketOutsideRuntimeDir {
                socket_path,
                runtime_dir,
            } => write!(
                formatter,
                "daemon socket {} must live directly under runtime_dir {}",
                socket_path.display(),
                runtime_dir.display()
            ),
            Self::InvalidTelemetryCadenceSeconds(seconds) => write!(
                formatter,
                "unsupported telemetry cadence {seconds}s; supported cadences are 6s and 30s"
            ),
        }
    }
}

impl std::error::Error for DaemonRuntimeConfigError {}

fn reject_blank(field: &'static str, value: &str) -> Result<(), DaemonRuntimeConfigError> {
    if value.trim().is_empty() {
        return Err(DaemonRuntimeConfigError::BlankField { field });
    }
    Ok(())
}

fn validate_absolute_path(
    field: &'static str,
    path: &Path,
) -> Result<(), DaemonRuntimeConfigError> {
    if !path.is_absolute() {
        return Err(DaemonRuntimeConfigError::RelativePath {
            field,
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        DaemonRuntimeConfig, DaemonRuntimeConfigError, DEFAULT_DAEMON_CONFIG_PATH,
        DEFAULT_DAEMON_GROUP, DEFAULT_DAEMON_LOG_DIR, DEFAULT_DAEMON_RUNTIME_DIR,
        DEFAULT_DAEMON_SERVICE_USER, DEFAULT_DAEMON_SOCKET_FILE_NAME, DEFAULT_DAEMON_STATE_DIR,
    };
    use dasobjectstore_core::DEFAULT_PRODUCT_ROOT;
    use std::path::PathBuf;

    #[test]
    fn default_runtime_paths_are_packaged_paths() {
        let config = DaemonRuntimeConfig::default_packaged();

        assert_eq!(config.service_user, DEFAULT_DAEMON_SERVICE_USER);
        assert_eq!(config.service_group, DEFAULT_DAEMON_GROUP);
        assert_eq!(
            config.config_path,
            PathBuf::from(DEFAULT_DAEMON_CONFIG_PATH)
        );
        assert_eq!(
            config.runtime_dir,
            PathBuf::from(DEFAULT_DAEMON_RUNTIME_DIR)
        );
        assert_eq!(
            config.socket_path,
            PathBuf::from(DEFAULT_DAEMON_RUNTIME_DIR).join(DEFAULT_DAEMON_SOCKET_FILE_NAME)
        );
        assert_eq!(config.state_dir, PathBuf::from(DEFAULT_DAEMON_STATE_DIR));
        assert_eq!(config.log_dir, PathBuf::from(DEFAULT_DAEMON_LOG_DIR));
        assert_eq!(config.product_root, PathBuf::from(DEFAULT_PRODUCT_ROOT));
        assert!(config.telemetry.enabled);
        assert_eq!(config.telemetry.cadence_seconds, 30);
        config.validate().expect("default config is valid");
    }

    #[test]
    fn rejects_relative_runtime_paths() {
        let mut config = DaemonRuntimeConfig::default_packaged();
        config.state_dir = PathBuf::from("var/lib/dasobjectstore");

        let err = config.validate().expect_err("relative path rejected");

        assert_eq!(
            err,
            DaemonRuntimeConfigError::RelativePath {
                field: "state_dir",
                path: PathBuf::from("var/lib/dasobjectstore"),
            }
        );
    }

    #[test]
    fn rejects_socket_outside_runtime_dir() {
        let mut config = DaemonRuntimeConfig::default_packaged();
        config.socket_path = PathBuf::from("/tmp/dasobjectstored.sock");

        let err = config
            .validate()
            .expect_err("socket outside runtime dir rejected");

        assert_eq!(
            err,
            DaemonRuntimeConfigError::SocketOutsideRuntimeDir {
                socket_path: PathBuf::from("/tmp/dasobjectstored.sock"),
                runtime_dir: PathBuf::from(DEFAULT_DAEMON_RUNTIME_DIR),
            }
        );
    }

    #[test]
    fn rejects_blank_service_identity() {
        let mut config = DaemonRuntimeConfig::default_packaged();
        config.service_user = " ".to_string();

        let err = config.validate().expect_err("blank user rejected");

        assert_eq!(
            err,
            DaemonRuntimeConfigError::BlankField {
                field: "service_user"
            }
        );
    }

    #[test]
    fn rejects_unsupported_telemetry_cadence() {
        let mut config = DaemonRuntimeConfig::default_packaged();
        config.telemetry.cadence_seconds = 5;

        let err = config
            .validate()
            .expect_err("unsupported telemetry cadence rejected");

        assert_eq!(
            err,
            DaemonRuntimeConfigError::InvalidTelemetryCadenceSeconds(5)
        );
    }
}
