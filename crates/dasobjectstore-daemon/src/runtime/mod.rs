//! Runtime configuration for the managed daemon.

mod admin_jobs;
mod appliance_telemetry;
mod application_audit;
mod application_capability_replay;
mod application_identity_registry;
mod application_key_registry;
mod capacity_persistence;
mod capacity_provider;
mod config;
mod drive_backend;
mod enclosure_prepare;
mod endpoint_registry;
mod folder_backend;
mod folder_catalogue;
mod folder_paths;
mod ingest_files;
mod local_admin;
mod migration_worker;
mod object_browser;
mod object_download;
mod performance_policy;
mod profile_registry;
mod profile_s3;
mod reconciliation;
mod remote_pairings;
mod remote_sessions;
mod remote_upload;
mod service;
mod service_reconciliation;

pub use admin_jobs::{
    admin_job_registry_path, AdminJobRegistry, FileBackedAdminJobRegistry,
    ADMIN_JOB_REGISTRY_DIR_NAME, ADMIN_JOB_REGISTRY_FILE_NAME, ADMIN_JOB_REGISTRY_SCHEMA,
};
pub use appliance_telemetry::{
    appliance_sample_set, appliance_telemetry_state_path, collect_appliance_session_telemetry,
    collect_linux_cpu_telemetry, collect_linux_disk_capacity_telemetry,
    collect_linux_disk_io_telemetry, collect_linux_memory_telemetry, parse_linux_cpu_snapshot,
    parse_linux_diskstats, validate_appliance_telemetry_cadence, ApplianceCpuTelemetry,
    ApplianceDiskCapacityTelemetry, ApplianceDiskIoTelemetry, ApplianceEnclosureTelemetry,
    ApplianceHostTelemetryCollector, ApplianceMemoryTelemetry, ApplianceSessionTelemetry,
    ApplianceTelemetryCollectionQuality, ApplianceTelemetryCollectorError, ApplianceTelemetryLoop,
    ApplianceTelemetryLoopConfig, ApplianceTelemetryLoopError, ApplianceTelemetryMissingDataMarker,
    ApplianceTelemetryMissingReason, ApplianceTelemetrySample, ApplianceTelemetrySampleSet,
    ApplianceTelemetrySink, ApplianceTelemetrySleeper, ApplianceTelemetrySource,
    FileBackedApplianceTelemetrySink, LinuxCpuSnapshot, LinuxDiskIoCounters,
    LinuxHostTelemetrySample, LinuxProcTelemetryCollector, ThreadApplianceTelemetrySleeper,
    APPLIANCE_TELEMETRY_DIR_NAME, APPLIANCE_TELEMETRY_FAST_CADENCE_SECONDS,
    APPLIANCE_TELEMETRY_FILE_NAME, APPLIANCE_TELEMETRY_NORMAL_CADENCE_SECONDS,
    APPLIANCE_TELEMETRY_SCHEMA_VERSION, DEFAULT_APPLIANCE_TELEMETRY_HDD_ROOT,
    DEFAULT_LOCAL_GROUP_PATH, DEFAULT_REMOTE_EASYCONNECT_SESSION_PATH,
    DEFAULT_STANDALONE_AUTH_ROOT,
};
pub use application_audit::{
    application_audit_log_path, read_application_audit_events, record_application_audit_event,
    ApplicationAuditEvent, APPLICATION_AUDIT_FILE_NAME, APPLICATION_AUDIT_MAX_EVENTS,
    APPLICATION_AUDIT_PATH_ENV, APPLICATION_AUDIT_SCHEMA,
};
pub use application_capability_replay::{
    application_capability_replay_path, complete_upload_with_capability,
    consume_upload_completion_capability, default_application_capability_replay_path,
    release_upload_completion_capability, UploadCompletionCapabilityOutcome,
    APPLICATION_CAPABILITY_REPLAY_ENV, APPLICATION_CAPABILITY_REPLAY_FILE_NAME,
    APPLICATION_CAPABILITY_REPLAY_SCHEMA,
};
pub use application_identity_registry::{
    application_identity_registry_path, deactivate_application_identity,
    default_application_identity_registry_path, list_application_identities,
    read_application_identity, upsert_application_identity, APPLICATION_IDENTITY_REGISTRY_ENV,
    APPLICATION_IDENTITY_REGISTRY_FILE_NAME, APPLICATION_IDENTITY_REGISTRY_SCHEMA,
};
pub use application_key_registry::{
    application_key_registry_path, deactivate_application_key,
    default_application_key_registry_path, list_application_keys, read_application_key,
    upsert_application_key, APPLICATION_KEY_REGISTRY_ENV, APPLICATION_KEY_REGISTRY_FILE_NAME,
    APPLICATION_KEY_REGISTRY_SCHEMA,
};
pub use capacity_persistence::{
    load_capacity_ledger, save_capacity_ledger, CapacityLedgerPersistenceError,
};
pub use capacity_provider::{
    CapacityAdmissionProvider, CapacitySpaceProbe, FileBackedCapacityAdmissionProvider,
    StatvfsCapacitySpaceProbe,
};
pub use config::{
    DaemonRuntimeConfig, DaemonRuntimeConfigError, DaemonTelemetryRuntimeConfig,
    DEFAULT_DAEMON_CONFIG_PATH, DEFAULT_DAEMON_GROUP, DEFAULT_DAEMON_LOG_DIR,
    DEFAULT_DAEMON_RUNTIME_DIR, DEFAULT_DAEMON_SERVICE_USER, DEFAULT_DAEMON_SOCKET_FILE_NAME,
    DEFAULT_DAEMON_STATE_DIR, LINUX_DAEMON_CONFIG_PATH, LINUX_DAEMON_LOG_DIR,
    LINUX_DAEMON_RUNTIME_DIR, LINUX_DAEMON_STATE_DIR,
};
pub use drive_backend::{DriveBackend, DriveRuntimeGuard};
pub use endpoint_registry::{
    default_endpoint_registry_path, upsert_endpoint_inventory_record,
    EndpointRegistryUpsertSummary, DEFAULT_ENDPOINT_REGISTRY_PATH, ENDPOINT_REGISTRY_ENV,
    ENDPOINT_REGISTRY_SCHEMA,
};
pub use folder_backend::{
    FolderBackend, FolderCapacitySnapshot, FolderInspectionReport, FolderReconciliationPlan,
};
pub use folder_catalogue::{
    FolderCatalogue, FolderCatalogueBrowserEntry, FolderCatalogueBrowserQuery,
};
pub use folder_paths::{
    folder_host_paths, user_service_plan, validate_user_service_state_owner, FolderHostPathError,
    FolderHostPaths, UserServicePlan,
};
pub(crate) use ingest_files::{default_hdd_root, default_ssd_root, discover_managed_hdd_roots};
pub use ingest_files::{
    submit_ingest_files_to_local_store, submit_ingest_files_to_local_store_with_capacity_provider,
    submit_ingest_files_to_local_store_with_progress, DaemonFileIngestSummary,
    DaemonIngestFilesRuntimeError,
};
pub use local_admin::{
    LocalAdminCommandOutput, LocalAdminCommandPlan, LocalAdminCommandRunner,
    LocalAdminRuntimeError, LocalGroupAdminController, LocalGroupAdministrationOperation,
    LocalGroupAdministrationRequest, LocalGroupAdministrationResponse, LocalGroupCommandPlanner,
    SystemLocalAdminCommandRunner, SystemLocalGroupCommandPlanner, LOCAL_ADMIN_CONFIRMATION_MARKER,
};
pub use migration_worker::{copy_folder_object, copy_folder_to_drive, FolderMigrationError};
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
pub use profile_registry::{
    default_profile_binding_registry_path, profile_binding_registry_path, read_profile_binding,
    read_profile_binding_record, upsert_profile_binding, validate_profile_binding_claim,
    BackendProfileBinding, PROFILE_BINDING_REGISTRY_ENV, PROFILE_BINDING_REGISTRY_FILE_NAME,
    PROFILE_BINDING_REGISTRY_SCHEMA,
};
pub use profile_s3::{
    assemble_profile_s3_multipart, complete_profile_s3_multipart,
    complete_profile_s3_multipart_with_capacity_provider, delete_profile_object,
    delete_profile_object_with_capacity_provider, get_profile_object, get_profile_object_range,
    head_profile_object, list_profile_objects, list_profile_objects_page, profile_diagnostics,
    profile_health, profile_s3_list_response, put_profile_object,
    put_profile_object_with_capacity_provider, stream_profile_object, verify_profile_object,
    ProfileDiagnosticsSummary, ProfileS3ListPage, ProfileS3MultipartCompletion,
    ProfileS3MultipartPart, ProfileS3MultipartPartSource, ProfileS3MultipartReader,
    ProfileS3Object, ProfileS3ReadBackend, ProfileS3WriteBackend, PROFILE_S3_MAX_KEYS,
    PROFILE_S3_MAX_MULTIPART_PARTS,
};
pub use reconciliation::{
    normalize_key, plan_reconciliation, ReconciliationAction, ReconciliationEntryState,
    ReconciliationManifest, ReconciliationManifestEntry, ReconciliationManifestError,
    ReconciliationObject, ReconciliationPlan, RECONCILIATION_MANIFEST_SCHEMA,
};
pub use remote_pairings::{
    remote_easyconnect_pairing_store_path, session_credentials_from_store_credentials,
    FileBackedRemoteEasyconnectPairingStore, RemoteEasyconnectPairingApproval,
    RemoteEasyconnectPairingExchange, RemoteEasyconnectPairingRecord,
    RemoteEasyconnectPairingStore, RemoteEasyconnectPairingStoreError,
    REMOTE_EASYCONNECT_PAIRING_DIR_NAME, REMOTE_EASYCONNECT_PAIRING_FILE_NAME,
    REMOTE_EASYCONNECT_PAIRING_SCHEMA,
};
pub use remote_sessions::{
    remote_easyconnect_session_store_path, FileBackedRemoteEasyconnectPairedSessionStore,
    RemoteEasyconnectPairedSessionRecord, RemoteEasyconnectPairedSessionRenewalRequest,
    RemoteEasyconnectPairedSessionStore, RemoteEasyconnectPairedSessionStoreError,
    REMOTE_EASYCONNECT_SESSION_DIR_NAME, REMOTE_EASYCONNECT_SESSION_FILE_NAME,
    REMOTE_EASYCONNECT_SESSION_SCHEMA,
};
pub use remote_upload::{
    plan_remote_upload_cancellation_cleanup, record_remote_upload_s3_transfer_job,
    run_remote_easyconnect_aws_cli_upload_job,
    run_remote_easyconnect_aws_cli_upload_job_with_capacity_provider,
    run_remote_upload_cancellation_cleanup, RemoteEasyconnectAwsCliUploadJobRequest,
    RemoteUploadAdmissionGate, RemoteUploadAwsCliByteTransfer, RemoteUploadAwsCliTransferPlan,
    RemoteUploadCancellationCleanupAction, RemoteUploadCancellationCleanupActionReport,
    RemoteUploadCancellationCleanupActionState, RemoteUploadCancellationCleanupError,
    RemoteUploadCancellationCleanupPlan, RemoteUploadCancellationCleanupRequest,
    RemoteUploadCancellationCleanupRunReport, RemoteUploadCancellationCleanupRuntime,
    RemoteUploadCancellationCleanupRuntimeConfig, RemoteUploadCancellationCleanupScope,
    RemoteUploadCancellationCleanupWorker, RemoteUploadCompletionCommit,
    RemoteUploadCompletionCommitError, RemoteUploadCompletionRecord,
    RemoteUploadMultipartAbortConfig, RemoteUploadProgressTelemetry, RemoteUploadQueueDepths,
    RemoteUploadRuntimeSnapshot, RemoteUploadS3ByteTransfer, RemoteUploadS3ByteTransferError,
    RemoteUploadS3TransferJob, RemoteUploadS3TransferJobOutcome, RemoteUploadS3TransferJobSummary,
    RemoteUploadS3TransferPermit, RemoteUploadS3TransferProgressReporter,
    RemoteUploadS3TransferProgressUpdate, RemoteUploadS3TransferRunError,
    RemoteUploadS3TransferWorker, RemoteUploadS3TransferWorkerReport,
    RemoteUploadS3TransferWorkerRequest,
};
pub use service::{
    provision_garage_store_registry, DaemonServiceRuntimeError, GarageProvisioningSummary,
    GarageServiceController, GarageServiceRuntimeConfig, GarageStoreRegistryProvisioningSummary,
    ServiceCommandOutput, ServiceCommandRunner, SystemServiceCommandRunner,
};
