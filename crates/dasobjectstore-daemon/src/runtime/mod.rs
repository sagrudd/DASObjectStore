//! Runtime configuration for the managed daemon.

mod config;
mod service;

pub use config::{
    DaemonRuntimeConfig, DaemonRuntimeConfigError, DEFAULT_DAEMON_CONFIG_PATH,
    DEFAULT_DAEMON_GROUP, DEFAULT_DAEMON_LOG_DIR, DEFAULT_DAEMON_RUNTIME_DIR,
    DEFAULT_DAEMON_SERVICE_USER, DEFAULT_DAEMON_SOCKET_FILE_NAME, DEFAULT_DAEMON_STATE_DIR,
    LINUX_DAEMON_CONFIG_PATH, LINUX_DAEMON_LOG_DIR, LINUX_DAEMON_RUNTIME_DIR,
    LINUX_DAEMON_STATE_DIR,
};
pub use service::{
    DaemonServiceRuntimeError, GarageServiceController, GarageServiceRuntimeConfig,
    ServiceCommandOutput, ServiceCommandRunner, SystemServiceCommandRunner,
};
