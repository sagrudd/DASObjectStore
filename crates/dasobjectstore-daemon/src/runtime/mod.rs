//! Runtime configuration for the managed daemon.

mod config;
mod ingest_files;
mod local_admin;
mod service;

pub use config::{
    DaemonRuntimeConfig, DaemonRuntimeConfigError, DEFAULT_DAEMON_CONFIG_PATH,
    DEFAULT_DAEMON_GROUP, DEFAULT_DAEMON_LOG_DIR, DEFAULT_DAEMON_RUNTIME_DIR,
    DEFAULT_DAEMON_SERVICE_USER, DEFAULT_DAEMON_SOCKET_FILE_NAME, DEFAULT_DAEMON_STATE_DIR,
    LINUX_DAEMON_CONFIG_PATH, LINUX_DAEMON_LOG_DIR, LINUX_DAEMON_RUNTIME_DIR,
    LINUX_DAEMON_STATE_DIR,
};
pub use ingest_files::{
    submit_ingest_files_to_local_store, submit_ingest_files_to_local_store_with_progress,
    DaemonFileIngestSummary, DaemonIngestFilesRuntimeError,
};
pub use local_admin::{
    LocalAdminCommandOutput, LocalAdminCommandPlan, LocalAdminCommandRunner,
    LocalAdminRuntimeError, LocalGroupAdminController, LocalGroupAdministrationOperation,
    LocalGroupAdministrationRequest, LocalGroupAdministrationResponse, LocalGroupCommandPlanner,
    SystemLocalAdminCommandRunner, SystemLocalGroupCommandPlanner, LOCAL_ADMIN_CONFIRMATION_MARKER,
};
pub use service::{
    provision_garage_store_registry, DaemonServiceRuntimeError, GarageProvisioningSummary,
    GarageServiceController, GarageServiceRuntimeConfig, GarageStoreRegistryProvisioningSummary,
    ServiceCommandOutput, ServiceCommandRunner, SystemServiceCommandRunner,
};
