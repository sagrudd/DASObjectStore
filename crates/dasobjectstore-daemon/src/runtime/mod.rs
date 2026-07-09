//! Runtime configuration for the managed daemon.

mod admin_jobs;
mod config;
mod endpoint_registry;
mod ingest_files;
mod local_admin;
mod object_browser;
mod object_download;
mod performance_policy;
mod remote_upload;
mod service;

pub use admin_jobs::{
    admin_job_registry_path, AdminJobRegistry, FileBackedAdminJobRegistry,
    ADMIN_JOB_REGISTRY_DIR_NAME, ADMIN_JOB_REGISTRY_FILE_NAME, ADMIN_JOB_REGISTRY_SCHEMA,
};
pub use config::{
    DaemonRuntimeConfig, DaemonRuntimeConfigError, DEFAULT_DAEMON_CONFIG_PATH,
    DEFAULT_DAEMON_GROUP, DEFAULT_DAEMON_LOG_DIR, DEFAULT_DAEMON_RUNTIME_DIR,
    DEFAULT_DAEMON_SERVICE_USER, DEFAULT_DAEMON_SOCKET_FILE_NAME, DEFAULT_DAEMON_STATE_DIR,
    LINUX_DAEMON_CONFIG_PATH, LINUX_DAEMON_LOG_DIR, LINUX_DAEMON_RUNTIME_DIR,
    LINUX_DAEMON_STATE_DIR,
};
pub use endpoint_registry::{
    default_endpoint_registry_path, upsert_endpoint_inventory_record,
    EndpointRegistryUpsertSummary, DEFAULT_ENDPOINT_REGISTRY_PATH, ENDPOINT_REGISTRY_ENV,
    ENDPOINT_REGISTRY_SCHEMA,
};
pub(crate) use ingest_files::{default_hdd_root, default_ssd_root, discover_managed_hdd_roots};
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
pub use object_browser::{
    query_object_browser_metadata, read_object_browser_metadata, ObjectBrowserMetadataEntry,
    ObjectBrowserMetadataReadError, ObjectBrowserQueryError,
};
pub(crate) use object_download::{
    resolve_object_download_with_hdd_root, resolve_object_folder_download_with_hdd_root,
};
pub use performance_policy::{
    authoritative_performance_recommendation_path, read_authoritative_ingest_policy,
    AuthoritativeIngestPolicy, AuthoritativePerformancePolicyError,
    AUTHORITATIVE_PERFORMANCE_DIR_NAME, AUTHORITATIVE_PERFORMANCE_RECOMMENDATION_FILE_NAME,
    PERFORMANCE_RECOMMENDATION_SCHEMA,
};
pub use remote_upload::{
    record_remote_upload_s3_transfer_job, RemoteUploadAdmissionGate, RemoteUploadQueueDepths,
    RemoteUploadRuntimeSnapshot, RemoteUploadS3TransferJob, RemoteUploadS3TransferJobOutcome,
    RemoteUploadS3TransferJobSummary, RemoteUploadS3TransferPermit,
    RemoteUploadS3TransferProgressReporter, RemoteUploadS3TransferProgressUpdate,
    RemoteUploadS3TransferRunError, RemoteUploadS3TransferWorker,
    RemoteUploadS3TransferWorkerReport, RemoteUploadS3TransferWorkerRequest,
};
pub use service::{
    provision_garage_store_registry, DaemonServiceRuntimeError, GarageProvisioningSummary,
    GarageServiceController, GarageServiceRuntimeConfig, GarageStoreRegistryProvisioningSummary,
    ServiceCommandOutput, ServiceCommandRunner, SystemServiceCommandRunner,
};
