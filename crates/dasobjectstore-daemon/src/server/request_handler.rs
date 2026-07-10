use crate::api::{
    query_appliance_telemetry, remote_easyconnect_renew_after_offset_seconds,
    resolve_remote_easyconnect_session_lifetime_seconds, ApplianceTelemetryRequest,
    ApplianceTelemetryResponse, AssignLocalUserToLocalGroupRequest,
    AssignLocalUserToLocalGroupResponse, CreateLocalGroupRequest, CreateLocalGroupResponse,
    CreateObjectStoreRequest, CreateObjectStoreResponse, DaemonApiErrorResponse, DaemonApiRequest,
    DaemonApiResponse, DaemonIngestProgressEvent, DaemonJobCancelRequest, DaemonJobCancelResponse,
    DaemonJobKind, DaemonJobListRequest, DaemonJobListResponse, DaemonJobProgress, DaemonJobState,
    DaemonJobStatusRequest, DaemonJobStatusResponse, DaemonJobSummary,
    DaemonLocalAdminAcceptedResponse, DaemonServiceLifecycleRequest,
    DaemonServiceLifecycleResponse, DaemonServiceProvisionRequest, DaemonServiceProvisionResponse,
    DaemonServiceStatusRequest, DaemonServiceStatusResponse, ObjectBrowserDelegatedActor,
    ObjectDownloadRequest, ObjectFolderDownloadRequest, PrepareEnclosureRequest,
    PrepareEnclosureResponse, RemoteEasyconnectApprovePairingRequest,
    RemoteEasyconnectApprovePairingResponse, RemoteEasyconnectCreatePairingRequest,
    RemoteEasyconnectCreatePairingResponse, RemoteEasyconnectExchangePairingRequest,
    RemoteEasyconnectExchangePairingResponse, RemoteEasyconnectRenewSessionRequest,
    RemoteEasyconnectRenewSessionResponse, RemoteEasyconnectRevokeSessionResponse,
    RemoteEasyconnectSession, RemoteEasyconnectSessionRenewal,
    RemoteEasyconnectSubmitAwsCliUploadRequest, RemoteEasyconnectSubmitAwsCliUploadResponse,
    StoreInventoryItem, StoreInventoryRequest, StoreInventoryResponse, SubmitIngestFilesRequest,
    SubmitIngestFilesResponse, UpsertEndpointInventoryRequest, UpsertEndpointInventoryResponse,
};
use crate::auth::{
    authorize_store_read, authorize_store_write, DaemonAuthorizationError, DaemonLocalActor,
    DaemonStoreAccessPolicy,
};
use crate::runtime::{
    appliance_telemetry_state_path, default_endpoint_registry_path, default_hdd_root,
    default_ssd_root, provision_garage_store_registry, query_object_browser_metadata,
    read_object_browser_metadata, remote_easyconnect_pairing_store_path,
    remote_easyconnect_session_store_path, resolve_object_download_with_hdd_root,
    resolve_object_folder_download_with_hdd_root, session_credentials_from_store_credentials,
    submit_ingest_files_to_local_store_with_progress, upsert_endpoint_inventory_record,
    AdminJobRegistry, ApplianceTelemetrySampleSet, DaemonIngestFilesRuntimeError,
    DaemonServiceRuntimeError, FileBackedRemoteEasyconnectPairedSessionStore,
    FileBackedRemoteEasyconnectPairingStore, GarageServiceController, LocalAdminRuntimeError,
    LocalGroupAdminController, LocalGroupAdministrationOperation, LocalGroupAdministrationRequest,
    ObjectBrowserQueryError, RemoteEasyconnectAwsCliUploadJobRequest,
    RemoteEasyconnectPairedSessionRecord, RemoteEasyconnectPairedSessionRenewalRequest,
    RemoteEasyconnectPairedSessionStore, RemoteEasyconnectPairedSessionStoreError,
    RemoteEasyconnectPairingApproval, RemoteEasyconnectPairingExchange,
    RemoteEasyconnectPairingRecord, RemoteEasyconnectPairingStore,
    RemoteEasyconnectPairingStoreError, RemoteUploadAdmissionGate, RemoteUploadProgressTelemetry,
    ServiceCommandRunner, SystemLocalAdminCommandRunner, DEFAULT_DAEMON_SERVICE_USER,
    DEFAULT_DAEMON_STATE_DIR,
};
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_core::store::ExportPolicy;
use dasobjectstore_core::utc::{add_seconds_to_utc_timestamp, format_utc_timestamp_seconds};
use dasobjectstore_metadata::{LIVE_SQLITE_FILE_NAME, METADATA_DIR_NAME};
use dasobjectstore_object_service::{
    bucket_name_for_definition, default_store_registry_path, default_subobject_registry_path,
    generate_per_store_credentials, read_store_registry, read_subobject_registry,
    ObjectServiceError, ObjectServiceProviderId, StoreCredentialRequest, SystemCredentialEntropy,
};
use std::fmt::{self, Display};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

mod dispatch;

pub struct DaemonRequestHandler<S, C> {
    service_orchestrator: S,
    clock: C,
    admin_job_registry: Option<Arc<dyn AdminJobRegistry>>,
    store_registry_path: PathBuf,
    subobject_registry_path: PathBuf,
    live_sqlite_path: PathBuf,
    hdd_root_path: PathBuf,
    appliance_telemetry_state_path: PathBuf,
    remote_easyconnect_pairing_store_path: PathBuf,
    remote_easyconnect_session_store_path: PathBuf,
    remote_upload_admission_gate: Arc<RemoteUploadAdmissionGate>,
}

impl<S, C> DaemonRequestHandler<S, C>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    pub fn new(service_orchestrator: S, clock: C) -> Self {
        Self {
            service_orchestrator,
            clock,
            admin_job_registry: None,
            store_registry_path: default_store_registry_path(),
            subobject_registry_path: default_subobject_registry_path(),
            live_sqlite_path: default_live_sqlite_path(),
            hdd_root_path: default_hdd_root(),
            appliance_telemetry_state_path: appliance_telemetry_state_path(
                DEFAULT_DAEMON_STATE_DIR,
            ),
            remote_easyconnect_pairing_store_path: remote_easyconnect_pairing_store_path(
                DEFAULT_DAEMON_STATE_DIR,
            ),
            remote_easyconnect_session_store_path: remote_easyconnect_session_store_path(
                DEFAULT_DAEMON_STATE_DIR,
            ),
            remote_upload_admission_gate: Arc::new(RemoteUploadAdmissionGate::new()),
        }
    }

    pub fn new_with_admin_job_registry(
        service_orchestrator: S,
        clock: C,
        admin_job_registry: Arc<dyn AdminJobRegistry>,
    ) -> Self {
        Self {
            service_orchestrator,
            clock,
            admin_job_registry: Some(admin_job_registry),
            store_registry_path: default_store_registry_path(),
            subobject_registry_path: default_subobject_registry_path(),
            live_sqlite_path: default_live_sqlite_path(),
            hdd_root_path: default_hdd_root(),
            appliance_telemetry_state_path: appliance_telemetry_state_path(
                DEFAULT_DAEMON_STATE_DIR,
            ),
            remote_easyconnect_pairing_store_path: remote_easyconnect_pairing_store_path(
                DEFAULT_DAEMON_STATE_DIR,
            ),
            remote_easyconnect_session_store_path: remote_easyconnect_session_store_path(
                DEFAULT_DAEMON_STATE_DIR,
            ),
            remote_upload_admission_gate: Arc::new(RemoteUploadAdmissionGate::new()),
        }
    }

    pub fn with_registry_paths(
        mut self,
        store_registry_path: impl Into<PathBuf>,
        subobject_registry_path: impl Into<PathBuf>,
    ) -> Self {
        self.store_registry_path = store_registry_path.into();
        self.subobject_registry_path = subobject_registry_path.into();
        self
    }

    pub fn with_live_sqlite_path(mut self, live_sqlite_path: impl Into<PathBuf>) -> Self {
        self.live_sqlite_path = live_sqlite_path.into();
        self
    }

    pub fn with_hdd_root_path(mut self, hdd_root_path: impl Into<PathBuf>) -> Self {
        self.hdd_root_path = hdd_root_path.into();
        self
    }

    pub fn with_appliance_telemetry_state_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.appliance_telemetry_state_path = path.into();
        self
    }

    pub fn with_remote_easyconnect_session_store_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.remote_easyconnect_session_store_path = path.into();
        self
    }

    pub fn with_remote_easyconnect_pairing_store_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.remote_easyconnect_pairing_store_path = path.into();
        self
    }

    pub fn with_remote_upload_admission_gate(
        mut self,
        remote_upload_admission_gate: Arc<RemoteUploadAdmissionGate>,
    ) -> Self {
        self.remote_upload_admission_gate = remote_upload_admission_gate;
        self
    }

    pub fn handle(
        &self,
        request: DaemonApiRequest,
    ) -> Result<DaemonApiResponse, DaemonRequestHandlerError> {
        self.handle_with_progress(request, |_| Ok(()))
    }

    pub fn handle_with_progress(
        &self,
        request: DaemonApiRequest,
        emit_progress: impl FnMut(
            DaemonIngestProgressEvent,
        ) -> Result<(), DaemonIngestFilesRuntimeError>,
    ) -> Result<DaemonApiResponse, DaemonRequestHandlerError> {
        self.handle_with_progress_for_actor(request, None, emit_progress)
    }

    pub fn handle_with_progress_for_actor(
        &self,
        request: DaemonApiRequest,
        actor: Option<&DaemonLocalActor>,
        mut emit_progress: impl FnMut(
            DaemonIngestProgressEvent,
        ) -> Result<(), DaemonIngestFilesRuntimeError>,
    ) -> Result<DaemonApiResponse, DaemonRequestHandlerError> {
        request.validate()?;

        dispatch::request(self, request, actor, &mut emit_progress)
    }

    fn record_admin_job(&self, job: DaemonJobSummary) -> Result<(), DaemonRequestHandlerError> {
        if let Some(registry) = &self.admin_job_registry {
            registry
                .record(job)
                .map_err(DaemonRequestHandlerError::ServiceRuntime)?;
        }
        Ok(())
    }

    fn admin_job_status(
        &self,
        request: DaemonJobStatusRequest,
    ) -> Result<DaemonJobStatusResponse, DaemonServiceRuntimeError> {
        if let Some(registry) = &self.admin_job_registry {
            return registry.status(request);
        }
        self.service_orchestrator.job_status(request)
    }

    fn admin_job_list(
        &self,
        request: DaemonJobListRequest,
    ) -> Result<DaemonJobListResponse, DaemonServiceRuntimeError> {
        if let Some(registry) = &self.admin_job_registry {
            return registry.list(request);
        }
        self.service_orchestrator.job_list(request)
    }

    fn cancel_admin_job(
        &self,
        request: DaemonJobCancelRequest,
        accepted_at_utc: &str,
    ) -> Result<DaemonJobCancelResponse, DaemonServiceRuntimeError> {
        if let Some(registry) = &self.admin_job_registry {
            return registry.cancel(request, accepted_at_utc);
        }
        self.service_orchestrator
            .cancel_job(request, accepted_at_utc)
    }
}

fn resolve_authorization_store_id(
    endpoint: &StoreId,
    store_registry_path: &Path,
    subobject_registry_path: &Path,
) -> Result<StoreId, IngestAuthorizationFailure> {
    let stores = read_store_registry(store_registry_path)?;
    let store_match = stores
        .iter()
        .find(|definition| definition.store_id == *endpoint)
        .map(|definition| definition.store_id.clone());
    let subobjects = read_subobject_registry(subobject_registry_path)?;
    let subobject_match = subobjects
        .iter()
        .find(|definition| definition.name == endpoint.as_str());

    match (store_match, subobject_match) {
        (Some(_), Some(_)) => Err(IngestAuthorizationFailure::AmbiguousEndpoint {
            endpoint: endpoint.clone(),
        }),
        (Some(store_id), None) => Ok(store_id),
        (None, Some(subobject)) => Ok(subobject.store_id.clone()),
        (None, None) => Err(IngestAuthorizationFailure::UnknownEndpoint {
            endpoint: endpoint.clone(),
            store_registry_path: store_registry_path.to_path_buf(),
            subobject_registry_path: subobject_registry_path.to_path_buf(),
        }),
    }
}

#[derive(Debug)]
enum RemoteEasyconnectRenewalDispatchError {
    InvalidRequest { message: String },
    InvalidClock { value: String },
    SessionStore(RemoteEasyconnectPairedSessionStoreError),
}

impl Display for RemoteEasyconnectRenewalDispatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest { message } => formatter.write_str(message),
            Self::InvalidClock { value } => write!(
                formatter,
                "daemon clock value {value} is not a supported UTC timestamp"
            ),
            Self::SessionStore(error) => Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for RemoteEasyconnectRenewalDispatchError {}

#[derive(Debug)]
enum RemoteEasyconnectStoreInventoryError {
    SessionStore(RemoteEasyconnectPairedSessionStoreError),
    MissingWriterGroup {
        object_store: String,
    },
    StoreNotRemoteWritable {
        object_store: String,
        export_policy: String,
    },
}

impl Display for RemoteEasyconnectStoreInventoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SessionStore(error) => Display::fmt(error, formatter),
            Self::MissingWriterGroup { object_store } => write!(
                formatter,
                "ObjectStore {object_store} cannot be listed for remote upload because it has no writer group"
            ),
            Self::StoreNotRemoteWritable {
                object_store,
                export_policy,
            } => write!(
                formatter,
                "ObjectStore {object_store} cannot be listed for remote upload because export policy {export_policy} is not S3"
            ),
        }
    }
}

impl std::error::Error for RemoteEasyconnectStoreInventoryError {}

impl From<RemoteEasyconnectPairedSessionStoreError> for RemoteEasyconnectStoreInventoryError {
    fn from(error: RemoteEasyconnectPairedSessionStoreError) -> Self {
        Self::SessionStore(error)
    }
}

#[derive(Debug)]
enum RemoteEasyconnectExchangeDispatchError {
    InvalidRequest { message: String },
    InvalidClock { value: String },
    PairingStore(RemoteEasyconnectPairingStoreError),
    SessionStore(RemoteEasyconnectPairedSessionStoreError),
    ObjectService(ObjectServiceError),
}

impl Display for RemoteEasyconnectExchangeDispatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest { message } => formatter.write_str(message),
            Self::InvalidClock { value } => write!(
                formatter,
                "daemon clock value {value} is not a supported UTC timestamp"
            ),
            Self::PairingStore(error) => Display::fmt(error, formatter),
            Self::SessionStore(error) => Display::fmt(error, formatter),
            Self::ObjectService(error) => Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for RemoteEasyconnectExchangeDispatchError {}

fn stable_easyconnect_id(prefix: &str, subject: &str, timestamp: &str) -> String {
    let mut suffix = String::new();
    for character in subject.chars().chain(timestamp.chars()) {
        if character.is_ascii_alphanumeric() {
            suffix.push(character.to_ascii_lowercase());
        } else if !suffix.ends_with('-') {
            suffix.push('-');
        }
    }
    let suffix = suffix.trim_matches('-');
    if suffix.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix}-{suffix}")
    }
}

fn rotated_easyconnect_renewal_token(session_id: &str, renewed_at_utc: &str) -> String {
    let suffix = renewed_at_utc
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    format!("renewal-{session_id}-{suffix}")
}

#[derive(Debug)]
enum IngestAuthorizationFailure {
    Authorization(DaemonAuthorizationError),
    ObjectService(ObjectServiceError),
    UnknownEndpoint {
        endpoint: StoreId,
        store_registry_path: PathBuf,
        subobject_registry_path: PathBuf,
    },
    AmbiguousEndpoint {
        endpoint: StoreId,
    },
    MissingStore {
        store_id: StoreId,
        store_registry_path: PathBuf,
    },
}

impl IngestAuthorizationFailure {
    fn code(&self) -> &'static str {
        match self {
            Self::Authorization(_) => "permission_denied",
            Self::ObjectService(_)
            | Self::UnknownEndpoint { .. }
            | Self::AmbiguousEndpoint { .. }
            | Self::MissingStore { .. } => "ingest_authorization_failed",
        }
    }
}

impl Display for IngestAuthorizationFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Authorization(error) => Display::fmt(error, formatter),
            Self::ObjectService(error) => Display::fmt(error, formatter),
            Self::UnknownEndpoint {
                endpoint,
                store_registry_path,
                subobject_registry_path,
            } => write!(
                formatter,
                "ingest endpoint {endpoint} was not found in {} or {}",
                store_registry_path.display(),
                subobject_registry_path.display()
            ),
            Self::AmbiguousEndpoint { endpoint } => write!(
                formatter,
                "ingest endpoint {endpoint} is ambiguous; both an object store and a SubObject use that name"
            ),
            Self::MissingStore {
                store_id,
                store_registry_path,
            } => write!(
                formatter,
                "SubObject authorization references missing store {store_id} in {}",
                store_registry_path.display()
            ),
        }
    }
}

impl From<DaemonAuthorizationError> for IngestAuthorizationFailure {
    fn from(error: DaemonAuthorizationError) -> Self {
        Self::Authorization(error)
    }
}

impl From<ObjectServiceError> for IngestAuthorizationFailure {
    fn from(error: ObjectServiceError) -> Self {
        Self::ObjectService(error)
    }
}

#[derive(Debug)]
enum ApplianceTelemetryAccessFailure {
    MissingActor,
    ReadState { path: PathBuf, message: String },
    InvalidState { path: PathBuf, message: String },
}

impl ApplianceTelemetryAccessFailure {
    fn code(&self) -> &'static str {
        match self {
            Self::MissingActor => "permission_denied",
            Self::ReadState { .. } | Self::InvalidState { .. } => {
                "appliance_telemetry_state_failed"
            }
        }
    }
}

impl Display for ApplianceTelemetryAccessFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingActor => formatter
                .write_str("authenticated daemon actor is required to read appliance telemetry"),
            Self::ReadState { path, message } => write!(
                formatter,
                "read appliance telemetry state {}: {message}",
                path.display()
            ),
            Self::InvalidState { path, message } => write!(
                formatter,
                "parse appliance telemetry state {}: {message}",
                path.display()
            ),
        }
    }
}

#[derive(Debug)]
enum ObjectBrowserAccessFailure {
    MissingActor,
    DelegationNotAllowed {
        peer_actor: String,
    },
    Authorization(DaemonAuthorizationError),
    ObjectService(ObjectServiceError),
    Endpoint(IngestAuthorizationFailure),
    MissingStore {
        store_id: StoreId,
        store_registry_path: PathBuf,
    },
}

impl ObjectBrowserAccessFailure {
    fn code(&self) -> &'static str {
        match self {
            Self::MissingActor | Self::DelegationNotAllowed { .. } | Self::Authorization(_) => {
                "permission_denied"
            }
            Self::ObjectService(_) | Self::Endpoint(_) | Self::MissingStore { .. } => {
                "object_browser_authorization_failed"
            }
        }
    }
}

impl Display for ObjectBrowserAccessFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingActor => formatter
                .write_str("authenticated daemon actor is required to browse ObjectStore metadata"),
            Self::DelegationNotAllowed { peer_actor } => write!(
                formatter,
                "actor {peer_actor} is not authorized to delegate ObjectStore browser access"
            ),
            Self::Authorization(error) => Display::fmt(error, formatter),
            Self::ObjectService(error) => Display::fmt(error, formatter),
            Self::Endpoint(error) => Display::fmt(error, formatter),
            Self::MissingStore {
                store_id,
                store_registry_path,
            } => write!(
                formatter,
                "ObjectBrowser authorization references missing store {store_id} in {}",
                store_registry_path.display()
            ),
        }
    }
}

impl From<DaemonAuthorizationError> for ObjectBrowserAccessFailure {
    fn from(error: DaemonAuthorizationError) -> Self {
        Self::Authorization(error)
    }
}

impl From<ObjectServiceError> for ObjectBrowserAccessFailure {
    fn from(error: ObjectServiceError) -> Self {
        Self::ObjectService(error)
    }
}

pub trait DaemonServiceOrchestrator {
    fn status(
        &self,
        request: DaemonServiceStatusRequest,
    ) -> Result<DaemonServiceStatusResponse, DaemonServiceRuntimeError>;

    fn lifecycle(
        &self,
        request: DaemonServiceLifecycleRequest,
        accepted_at_utc: &str,
    ) -> Result<DaemonServiceLifecycleResponse, DaemonServiceRuntimeError>;

    fn provision(
        &self,
        request: DaemonServiceProvisionRequest,
        accepted_at_utc: &str,
    ) -> Result<DaemonServiceProvisionResponse, DaemonServiceRuntimeError>;

    fn remote_easyconnect_aws_cli_upload_job(
        &self,
        _registry: &dyn AdminJobRegistry,
        _gate: Arc<RemoteUploadAdmissionGate>,
        _request: RemoteEasyconnectAwsCliUploadJobRequest,
    ) -> Result<crate::runtime::RemoteUploadS3TransferWorkerReport, DaemonServiceRuntimeError> {
        Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation:
                "remote easyconnect AWS CLI upload requires an object-service command runner"
                    .to_string(),
        })
    }

    fn prepare_enclosure(
        &self,
        _request: PrepareEnclosureRequest,
        _accepted_at_utc: &str,
    ) -> Result<PrepareEnclosureResponse, DaemonServiceRuntimeError> {
        Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: "prepare_enclosure requires an enclosure preparation orchestrator"
                .to_string(),
        })
    }

    fn create_object_store(
        &self,
        _request: CreateObjectStoreRequest,
        _accepted_at_utc: &str,
    ) -> Result<CreateObjectStoreResponse, DaemonServiceRuntimeError> {
        Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: "create_object_store requires an ObjectStore administration orchestrator"
                .to_string(),
        })
    }

    fn upsert_endpoint_inventory(
        &self,
        _request: UpsertEndpointInventoryRequest,
        _accepted_at_utc: &str,
    ) -> Result<UpsertEndpointInventoryResponse, DaemonServiceRuntimeError> {
        Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: "upsert_endpoint_inventory requires an endpoint registry orchestrator"
                .to_string(),
        })
    }

    fn create_local_group(
        &self,
        _request: CreateLocalGroupRequest,
        _accepted_at_utc: &str,
    ) -> Result<CreateLocalGroupResponse, LocalAdminRuntimeError> {
        Err(LocalAdminRuntimeError::UnsupportedOperation {
            operation: "create_local_group requires a local admin orchestrator".to_string(),
        })
    }

    fn assign_local_user_to_local_group(
        &self,
        _request: AssignLocalUserToLocalGroupRequest,
        _accepted_at_utc: &str,
    ) -> Result<AssignLocalUserToLocalGroupResponse, LocalAdminRuntimeError> {
        Err(LocalAdminRuntimeError::UnsupportedOperation {
            operation: "assign_local_user_to_local_group requires a local admin orchestrator"
                .to_string(),
        })
    }

    fn job_status(
        &self,
        _request: DaemonJobStatusRequest,
    ) -> Result<DaemonJobStatusResponse, DaemonServiceRuntimeError> {
        Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: "job_status requires a daemon job orchestrator".to_string(),
        })
    }

    fn job_list(
        &self,
        _request: DaemonJobListRequest,
    ) -> Result<DaemonJobListResponse, DaemonServiceRuntimeError> {
        Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: "job_list requires a daemon job orchestrator".to_string(),
        })
    }

    fn cancel_job(
        &self,
        _request: DaemonJobCancelRequest,
        _accepted_at_utc: &str,
    ) -> Result<DaemonJobCancelResponse, DaemonServiceRuntimeError> {
        Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: "cancel_job requires a daemon job orchestrator".to_string(),
        })
    }

    fn submit_ingest_files(
        &self,
        _request: SubmitIngestFilesRequest,
        _accepted_at_utc: &str,
        _emit_progress: &mut dyn FnMut(
            DaemonIngestProgressEvent,
        ) -> Result<(), DaemonIngestFilesRuntimeError>,
    ) -> Result<SubmitIngestFilesResponse, DaemonIngestFilesRuntimeError> {
        Err(DaemonIngestFilesRuntimeError::CommandFailed(
            "submit_ingest_files requires a file ingest orchestrator".to_string(),
        ))
    }
}

fn daemon_job_summary_from_service_lifecycle(
    response: &DaemonServiceLifecycleResponse,
) -> DaemonJobSummary {
    daemon_job_summary_from_accepted(
        response.accepted.job_id.clone(),
        response.accepted.kind.clone(),
        response.accepted.accepted_at_utc.clone(),
        response.accepted.dry_run,
        None,
        format!("service {:?} completed", response.operation),
    )
}

fn daemon_job_summary_from_service_provision(
    response: &DaemonServiceProvisionResponse,
) -> DaemonJobSummary {
    daemon_job_summary_from_accepted(
        response.accepted.job_id.clone(),
        response.accepted.kind.clone(),
        response.accepted.accepted_at_utc.clone(),
        response.accepted.dry_run,
        None,
        format!(
            "provisioned {} store(s), {} bucket(s), {} command(s), credentials issued/reused/rotated {}/{}/{}",
            response.stores,
            response.buckets,
            response.commands,
            response.credentials_issued,
            response.credentials_reused,
            response.credentials_rotated
        ),
    )
}

fn daemon_job_summary_from_prepare_enclosure(
    response: &PrepareEnclosureResponse,
) -> DaemonJobSummary {
    daemon_job_summary_from_accepted(
        response.accepted.job_id.clone(),
        response.accepted.kind.clone(),
        response.accepted.accepted_at_utc.clone(),
        response.accepted.dry_run,
        response.administrator_actor.clone(),
        format!(
            "prepared {} landing device and {} HDD device(s)",
            response.ssd_device.display(),
            response.hdd_devices.len()
        ),
    )
}

fn daemon_job_summary_from_create_object_store(
    response: &CreateObjectStoreResponse,
) -> DaemonJobSummary {
    daemon_job_summary_from_accepted(
        response.accepted.job_id.clone(),
        DaemonJobKind::ObjectStoreCreation,
        response.accepted.accepted_at_utc.clone(),
        response.accepted.dry_run,
        response.administrator_actor.clone(),
        format!(
            "ObjectStore {} creation accepted for writer group {}",
            response.store_id, response.writer_group
        ),
    )
}

fn daemon_job_summary_from_endpoint_inventory(
    response: &UpsertEndpointInventoryResponse,
) -> DaemonJobSummary {
    daemon_job_summary_from_accepted(
        response.accepted.job_id.clone(),
        DaemonJobKind::EndpointValidation,
        response.accepted.accepted_at_utc.clone(),
        response.accepted.dry_run,
        response.administrator_actor.clone(),
        format!(
            "endpoint {} inventory recorded with validation state {:?}",
            response.endpoint_id, response.validation_state
        ),
    )
}

fn daemon_job_summary_from_local_admin(
    accepted: &DaemonLocalAdminAcceptedResponse,
    actor: Option<String>,
) -> DaemonJobSummary {
    daemon_job_summary_from_accepted(
        accepted.job_id.clone(),
        DaemonJobKind::SystemAdministration,
        accepted.accepted_at_utc.clone(),
        accepted.dry_run,
        actor,
        format!(
            "local administrator command {:?} completed",
            accepted.command
        ),
    )
}

fn daemon_job_summary_from_accepted(
    job_id: crate::api::DaemonJobId,
    kind: DaemonJobKind,
    accepted_at_utc: String,
    dry_run: bool,
    actor: Option<String>,
    message: String,
) -> DaemonJobSummary {
    let message = if dry_run {
        format!("dry run: {message}")
    } else {
        message
    };
    DaemonJobSummary {
        job_id,
        kind,
        state: DaemonJobState::Complete,
        progress: DaemonJobProgress {
            stage: "complete".to_string(),
            work_bytes_done: 1,
            work_bytes_total: 1,
            work_units_done: 1,
            work_units_total: 1,
            message: Some(message),
        },
        submitted_at_utc: accepted_at_utc.clone(),
        updated_at_utc: accepted_at_utc,
        actor,
        failure_message: None,
    }
}

fn remote_easyconnect_aws_cli_upload_job_request(
    request: RemoteEasyconnectSubmitAwsCliUploadRequest,
    accepted_at_utc: &str,
    actor: Option<String>,
) -> RemoteEasyconnectAwsCliUploadJobRequest {
    RemoteEasyconnectAwsCliUploadJobRequest {
        job_id: request.job_id,
        object_store: request.object_store,
        source_bytes: request.source_bytes,
        policy: request.policy,
        ssd_pressure: request.ssd_pressure,
        program: request.program,
        args: request.args,
        display_args: request.display_args,
        environment: request
            .environment
            .into_iter()
            .map(|variable| (variable.name, variable.value))
            .collect(),
        submitted_at_utc: accepted_at_utc.to_string(),
        started_at_utc: accepted_at_utc.to_string(),
        finished_at_utc: accepted_at_utc.to_string(),
        progress_updated_at_utc: accepted_at_utc.to_string(),
        actor,
        progress_telemetry: request
            .progress_telemetry
            .map(remote_upload_progress_telemetry),
        progress_message: request.progress_message,
    }
}

fn remote_upload_progress_telemetry(
    telemetry: crate::api::RemoteEasyconnectUploadProgressTelemetry,
) -> RemoteUploadProgressTelemetry {
    RemoteUploadProgressTelemetry {
        source_scan_count: telemetry.source_scan_count,
        staged_bytes: telemetry.staged_bytes,
        s3_bytes_per_second: telemetry.s3_bytes_per_second,
        ssd_queue_depth: telemetry.ssd_queue_depth,
        hdd_landing_queue_depth: telemetry.hdd_landing_queue_depth,
        active_hdd_writers: telemetry.active_hdd_writers,
        verification_state: telemetry.verification_state,
        session_renewal_status: telemetry.session_renewal_status,
    }
}

impl<R> DaemonServiceOrchestrator for GarageServiceController<R>
where
    R: ServiceCommandRunner,
{
    fn status(
        &self,
        request: DaemonServiceStatusRequest,
    ) -> Result<DaemonServiceStatusResponse, DaemonServiceRuntimeError> {
        GarageServiceController::status(self, request)
    }

    fn lifecycle(
        &self,
        request: DaemonServiceLifecycleRequest,
        accepted_at_utc: &str,
    ) -> Result<DaemonServiceLifecycleResponse, DaemonServiceRuntimeError> {
        GarageServiceController::lifecycle(self, request, accepted_at_utc)
    }

    fn provision(
        &self,
        request: DaemonServiceProvisionRequest,
        accepted_at_utc: &str,
    ) -> Result<DaemonServiceProvisionResponse, DaemonServiceRuntimeError> {
        request.validate()?;
        let summary = provision_garage_store_registry(
            self,
            default_store_registry_path(),
            request.dry_run,
            request.rotate_credentials,
            accepted_at_utc,
        )?;
        let job_id_value = format!(
            "service-provision-{}",
            accepted_at_utc
                .chars()
                .map(|character| if character.is_ascii_alphanumeric() {
                    character
                } else {
                    '-'
                })
                .collect::<String>()
                .trim_matches('-')
                .to_ascii_lowercase()
        );
        let job_id = crate::api::DaemonJobId::new(job_id_value.clone())
            .map_err(|_| DaemonServiceRuntimeError::InvalidJobId(job_id_value))?;
        Ok(DaemonServiceProvisionResponse::accepted(
            job_id,
            accepted_at_utc,
            request.dry_run,
            ObjectServiceProviderId::Garage,
            summary.registry_path.to_string_lossy().to_string(),
            summary
                .credential_registry_path
                .to_string_lossy()
                .to_string(),
            summary.stores,
            summary.buckets,
            summary.commands,
            summary.credentials_issued,
            summary.credentials_reused,
            summary.credentials_rotated,
        ))
    }

    fn remote_easyconnect_aws_cli_upload_job(
        &self,
        registry: &dyn AdminJobRegistry,
        gate: Arc<RemoteUploadAdmissionGate>,
        request: RemoteEasyconnectAwsCliUploadJobRequest,
    ) -> Result<crate::runtime::RemoteUploadS3TransferWorkerReport, DaemonServiceRuntimeError> {
        GarageServiceController::remote_easyconnect_aws_cli_upload_job(
            self, registry, gate, request,
        )
    }

    fn create_object_store(
        &self,
        request: CreateObjectStoreRequest,
        accepted_at_utc: &str,
    ) -> Result<CreateObjectStoreResponse, DaemonServiceRuntimeError> {
        let job_id_value = format!(
            "objectstore-create-{}",
            accepted_at_utc
                .chars()
                .map(|character| if character.is_ascii_alphanumeric() {
                    character
                } else {
                    '-'
                })
                .collect::<String>()
                .trim_matches('-')
                .to_ascii_lowercase()
        );
        let job_id = crate::api::DaemonJobId::new(job_id_value.clone())
            .map_err(|_| DaemonServiceRuntimeError::InvalidJobId(job_id_value))?;
        Ok(CreateObjectStoreResponse::accepted(
            job_id,
            accepted_at_utc,
            request,
        ))
    }

    fn upsert_endpoint_inventory(
        &self,
        request: UpsertEndpointInventoryRequest,
        accepted_at_utc: &str,
    ) -> Result<UpsertEndpointInventoryResponse, DaemonServiceRuntimeError> {
        let job_id_value = format!(
            "endpoint-upsert-{}",
            accepted_at_utc
                .chars()
                .map(|character| if character.is_ascii_alphanumeric() {
                    character
                } else {
                    '-'
                })
                .collect::<String>()
                .trim_matches('-')
                .to_ascii_lowercase()
        );
        let job_id = crate::api::DaemonJobId::new(job_id_value.clone())
            .map_err(|_| DaemonServiceRuntimeError::InvalidJobId(job_id_value))?;
        let summary = upsert_endpoint_inventory_record(default_endpoint_registry_path(), &request)?;
        Ok(UpsertEndpointInventoryResponse::accepted(
            job_id,
            accepted_at_utc,
            summary.registry_path.to_string_lossy().to_string(),
            request,
        ))
    }

    fn create_local_group(
        &self,
        request: CreateLocalGroupRequest,
        accepted_at_utc: &str,
    ) -> Result<CreateLocalGroupResponse, LocalAdminRuntimeError> {
        let administrator_actor = request.administrator_actor.clone();
        let response = LocalGroupAdminController::new(SystemLocalAdminCommandRunner).execute(
            LocalGroupAdministrationRequest {
                operation: LocalGroupAdministrationOperation::CreateGroup,
                group_name: request.group_name,
                username: None,
                dry_run: request.dry_run,
                client_request_id: request.client_request_id,
                administrator_confirmation: Some(request.confirmation_marker),
            },
            accepted_at_utc,
        )?;

        Ok(CreateLocalGroupResponse {
            accepted: response.accepted,
            group_name: response.group_name,
            administrator_actor,
        })
    }

    fn assign_local_user_to_local_group(
        &self,
        request: AssignLocalUserToLocalGroupRequest,
        accepted_at_utc: &str,
    ) -> Result<AssignLocalUserToLocalGroupResponse, LocalAdminRuntimeError> {
        let administrator_actor = request.administrator_actor.clone();
        let response = LocalGroupAdminController::new(SystemLocalAdminCommandRunner).execute(
            LocalGroupAdministrationRequest {
                operation: LocalGroupAdministrationOperation::AssignUserToGroup,
                group_name: request.group_name,
                username: Some(request.username),
                dry_run: request.dry_run,
                client_request_id: request.client_request_id,
                administrator_confirmation: Some(request.confirmation_marker),
            },
            accepted_at_utc,
        )?;

        let username = response
            .username
            .ok_or(LocalAdminRuntimeError::MissingField { field: "username" })?;

        Ok(AssignLocalUserToLocalGroupResponse {
            accepted: response.accepted,
            username,
            group_name: response.group_name,
            administrator_actor,
        })
    }

    fn submit_ingest_files(
        &self,
        request: SubmitIngestFilesRequest,
        accepted_at_utc: &str,
        emit_progress: &mut dyn FnMut(
            DaemonIngestProgressEvent,
        ) -> Result<(), DaemonIngestFilesRuntimeError>,
    ) -> Result<SubmitIngestFilesResponse, DaemonIngestFilesRuntimeError> {
        submit_ingest_files_to_local_store_with_progress(request, accepted_at_utc, emit_progress)
    }
}

pub trait DaemonClock {
    fn now_utc(&self) -> String;
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SystemDaemonClock;

impl DaemonClock for SystemDaemonClock {
    fn now_utc(&self) -> String {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or_default();
        format_utc_timestamp_seconds(seconds as i64)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FixedDaemonClock {
    now_utc: String,
}

impl FixedDaemonClock {
    pub fn new(now_utc: impl Into<String>) -> Self {
        Self {
            now_utc: now_utc.into(),
        }
    }
}

impl DaemonClock for FixedDaemonClock {
    fn now_utc(&self) -> String {
        self.now_utc.clone()
    }
}

#[derive(Debug)]
pub enum DaemonRequestHandlerError {
    RequestValidation(crate::api::DaemonRequestValidationError),
    ServiceRuntime(DaemonServiceRuntimeError),
    LocalAdminRuntime(LocalAdminRuntimeError),
    IngestRuntime(DaemonIngestFilesRuntimeError),
    ObjectBrowserQuery(ObjectBrowserQueryError),
}

impl Display for DaemonRequestHandlerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RequestValidation(error) => Display::fmt(error, formatter),
            Self::ServiceRuntime(error) => Display::fmt(error, formatter),
            Self::LocalAdminRuntime(error) => Display::fmt(error, formatter),
            Self::IngestRuntime(error) => Display::fmt(error, formatter),
            Self::ObjectBrowserQuery(error) => Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for DaemonRequestHandlerError {}

impl From<crate::api::DaemonRequestValidationError> for DaemonRequestHandlerError {
    fn from(error: crate::api::DaemonRequestValidationError) -> Self {
        Self::RequestValidation(error)
    }
}

impl From<ObjectBrowserQueryError> for DaemonRequestHandlerError {
    fn from(error: ObjectBrowserQueryError) -> Self {
        Self::ObjectBrowserQuery(error)
    }
}

const LIVE_SQLITE_PATH_ENV: &str = "DASOBJECTSTORE_LIVE_SQLITE_PATH";

fn default_live_sqlite_path() -> PathBuf {
    std::env::var_os(LIVE_SQLITE_PATH_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            default_ssd_root()
                .join(METADATA_DIR_NAME)
                .join(LIVE_SQLITE_FILE_NAME)
        })
}

impl DaemonApiRequest {
    fn command_name(&self) -> &'static str {
        match self {
            Self::HealthSummary(_) => "health_summary",
            Self::StoreInventory(_) => "store_inventory",
            Self::SubmitIngestFiles(_) => "submit_ingest_files",
            Self::IngestJobStatus(_) => "ingest_job_status",
            Self::CancelIngestJob(_) => "cancel_ingest_job",
            Self::JobList(_) => "job_list",
            Self::JobStatus(_) => "job_status",
            Self::CancelJob(_) => "cancel_job",
            Self::ServiceStatus(_) => "service_status",
            Self::ApplianceTelemetry(_) => "appliance_telemetry",
            Self::ServiceLifecycle(_) => "service_lifecycle",
            Self::ServiceProvision(_) => "service_provision",
            Self::PrepareEnclosure(_) => "prepare_enclosure",
            Self::CreateObjectStore(_) => "create_object_store",
            Self::ObjectBrowser(_) => "object_browser",
            Self::ObjectDownload(_) => "object_download",
            Self::ObjectFolderDownload(_) => "object_folder_download",
            Self::UpsertEndpointInventory(_) => "upsert_endpoint_inventory",
            Self::CreateLocalGroup(_) => "create_local_group",
            Self::AssignLocalUserToLocalGroup(_) => "assign_local_user_to_local_group",
            Self::RemoteEasyconnectDiscovery(_) => "remote_easyconnect_discovery",
            Self::RemoteEasyconnectCreatePairing(_) => "remote_easyconnect_create_pairing",
            Self::RemoteEasyconnectApprovePairing(_) => "remote_easyconnect_approve_pairing",
            Self::RemoteEasyconnectExchangePairing(_) => "remote_easyconnect_exchange_pairing",
            Self::RemoteEasyconnectRevokeSession(_) => "remote_easyconnect_revoke_session",
            Self::RemoteEasyconnectRenewSession(_) => "remote_easyconnect_renew_session",
            Self::RemoteEasyconnectUploadAdmission(_) => "remote_easyconnect_upload_admission",
            Self::RemoteEasyconnectSubmitAwsCliUpload(_) => {
                "remote_easyconnect_submit_aws_cli_upload"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DaemonClock, DaemonRequestHandler, DaemonServiceOrchestrator, FixedDaemonClock,
        SystemDaemonClock,
    };
    use crate::api::{
        ApplianceTelemetryRequest, ApplianceTelemetryState, ApplianceTelemetryWindow,
        AssignLocalUserToLocalGroupRequest, AssignLocalUserToLocalGroupResponse,
        CreateLocalGroupRequest, CreateLocalGroupResponse, CreateObjectStoreRequest,
        CreateObjectStoreResponse, DaemonApiRequest, DaemonApiResponse, DaemonEndpointKind,
        DaemonEndpointValidation, DaemonEndpointValidationState, DaemonJobCancelRequest,
        DaemonJobCancelResponse, DaemonJobId, DaemonJobKind, DaemonJobListRequest,
        DaemonJobListResponse, DaemonJobProgress, DaemonJobState, DaemonJobStatusRequest,
        DaemonJobStatusResponse, DaemonJobSummary, DaemonRequestValidationError,
        DaemonServiceLifecycleRequest, DaemonServiceLifecycleResponse, DaemonServiceOperation,
        DaemonServiceProvisionRequest, DaemonServiceProvisionResponse, DaemonServiceStatusRequest,
        DaemonServiceStatusResponse, DaemonSsdPressure, ObjectBrowserDelegatedActor,
        ObjectBrowserPageRequest, ObjectBrowserPlacementLocation, ObjectBrowserPlacementState,
        ObjectBrowserReadinessState, ObjectBrowserRequest, ObjectBrowserSort,
        ObjectDownloadRequest, ObjectFolderDownloadRequest, PrepareEnclosureFilesystem,
        PrepareEnclosureHddDevice, PrepareEnclosureRequest, PrepareEnclosureResponse,
        RemoteEasyconnectApprovePairingRequest, RemoteEasyconnectAuthProvider,
        RemoteEasyconnectAwsCliEnvironmentVariable, RemoteEasyconnectCreatePairingRequest,
        RemoteEasyconnectExchangePairingRequest, RemoteEasyconnectObjectStoreGrant,
        RemoteEasyconnectRenewSessionRequest, RemoteEasyconnectRevokeSessionRequest,
        RemoteEasyconnectSessionCredentials, RemoteEasyconnectSubmitAwsCliUploadRequest,
        RemoteEasyconnectUploadAdmissionRequest, RemoteEasyconnectUploadBackpressureReason,
        StoreInventoryRequest, SubmitIngestFilesRequest, SubmitIngestFilesResponse,
        UpsertEndpointInventoryRequest, UpsertEndpointInventoryResponse,
        ENCLOSURE_PREPARE_CONFIRMATION, ENDPOINT_RECORD_CONFIRMATION,
        OBJECT_STORE_CREATE_CONFIRMATION,
    };
    use crate::auth::DaemonLocalActor;
    use crate::runtime::{
        admin_job_registry_path, remote_easyconnect_pairing_store_path,
        remote_easyconnect_session_store_path, DaemonIngestFilesRuntimeError,
        DaemonServiceRuntimeError, FileBackedAdminJobRegistry,
        FileBackedRemoteEasyconnectPairedSessionStore, LocalAdminRuntimeError,
        LocalGroupAdministrationOperation, RemoteEasyconnectAwsCliUploadJobRequest,
        RemoteEasyconnectPairedSessionRecord, RemoteEasyconnectPairedSessionStore,
        RemoteUploadAdmissionGate,
    };
    use crate::AdminJobRegistry;
    use dasobjectstore_core::ids::{IngestJobId, ObjectId, PoolId, StoreId};
    use dasobjectstore_core::object_type::ObjectType;
    use dasobjectstore_core::remote_upload::{
        RemoteUploadBackpressureAction, RemoteUploadBackpressurePolicy,
    };
    use dasobjectstore_core::store::{ExportPolicy, StoreClass, StorePolicy};
    use dasobjectstore_metadata::LIVE_SCHEMA_SQL;
    use dasobjectstore_object_service::{
        ObjectServiceProviderId, ServiceState, StoreServiceDefinition,
    };
    use rusqlite::{params, Connection};
    use std::cell::RefCell;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    #[test]
    fn dispatches_service_status_to_orchestrator() {
        let service = FakeService::default();
        let handler =
            DaemonRequestHandler::new(service, FixedDaemonClock::new("2026-07-07T11:47:42Z"));

        let response = handler
            .handle(DaemonApiRequest::ServiceStatus(
                DaemonServiceStatusRequest {
                    include_detail: true,
                },
            ))
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::ServiceStatus(DaemonServiceStatusResponse {
                state: ServiceState::Running,
                ..
            })
        ));
        assert_eq!(
            handler
                .service_orchestrator
                .status_calls
                .borrow()
                .as_slice(),
            &[true]
        );
    }

    #[test]
    fn dispatches_service_lifecycle_with_clock_timestamp() {
        let service = FakeService::default();
        let handler =
            DaemonRequestHandler::new(service, FixedDaemonClock::new("2026-07-07T11:47:42Z"));

        let response = handler
            .handle(DaemonApiRequest::ServiceLifecycle(
                DaemonServiceLifecycleRequest {
                    operation: DaemonServiceOperation::Start,
                    provider_id: ObjectServiceProviderId::Garage,
                    dry_run: true,
                    client_request_id: Some("request-1".to_string()),
                },
            ))
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::ServiceLifecycle(DaemonServiceLifecycleResponse {
                operation: DaemonServiceOperation::Start,
                ..
            })
        ));
        assert_eq!(
            handler
                .service_orchestrator
                .lifecycle_calls
                .borrow()
                .as_slice(),
            &["2026-07-07T11:47:42Z".to_string()]
        );
    }

    #[test]
    fn dispatches_service_provision_with_clock_timestamp() {
        let service = FakeService::default();
        let handler =
            DaemonRequestHandler::new(service, FixedDaemonClock::new("2026-07-07T12:05:42Z"));

        let response = handler
            .handle(DaemonApiRequest::ServiceProvision(
                DaemonServiceProvisionRequest {
                    provider_id: ObjectServiceProviderId::Garage,
                    dry_run: true,
                    rotate_credentials: false,
                    client_request_id: Some("request-1".to_string()),
                },
            ))
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::ServiceProvision(DaemonServiceProvisionResponse {
                buckets: 1,
                commands: 3,
                ..
            })
        ));
        assert_eq!(
            handler
                .service_orchestrator
                .provision_calls
                .borrow()
                .as_slice(),
            &["2026-07-07T12:05:42Z".to_string()]
        );
    }

    #[test]
    fn dispatches_create_local_group_with_clock_timestamp() {
        let service = FakeService::default();
        let handler =
            DaemonRequestHandler::new(service, FixedDaemonClock::new("2026-07-07T12:25:00Z"));

        let response = handler
            .handle(DaemonApiRequest::CreateLocalGroup(
                CreateLocalGroupRequest {
                    group_name: "daswriters".to_string(),
                    dry_run: true,
                    client_request_id: None,
                    administrator_actor: Some("operator".to_string()),
                    confirmation_marker: "confirm local group administration".to_string(),
                },
            ))
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::CreateLocalGroup(CreateLocalGroupResponse {
                group_name,
                ..
            }) if group_name == "daswriters"
        ));
        assert_eq!(
            handler
                .service_orchestrator
                .local_group_calls
                .borrow()
                .as_slice(),
            &[(
                "2026-07-07T12:25:00Z".to_string(),
                LocalGroupAdministrationOperation::CreateGroup,
                "daswriters".to_string(),
                None,
                true,
            )]
        );
    }

    #[test]
    fn dispatches_assign_local_user_to_local_group_with_clock_timestamp() {
        let service = FakeService::default();
        let handler =
            DaemonRequestHandler::new(service, FixedDaemonClock::new("2026-07-07T12:28:00Z"));

        let response = handler
            .handle(DaemonApiRequest::AssignLocalUserToLocalGroup(
                AssignLocalUserToLocalGroupRequest {
                    username: "stephen".to_string(),
                    group_name: "daswriters".to_string(),
                    dry_run: true,
                    client_request_id: Some("request-2".to_string()),
                    administrator_actor: Some("operator".to_string()),
                    confirmation_marker: "confirm local group administration".to_string(),
                },
            ))
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::AssignLocalUserToLocalGroup(
                AssignLocalUserToLocalGroupResponse {
                    username,
                    group_name,
                    ..
                }
            ) if username == "stephen" && group_name == "daswriters"
        ));
        assert_eq!(
            handler
                .service_orchestrator
                .local_group_calls
                .borrow()
                .as_slice(),
            &[(
                "2026-07-07T12:28:00Z".to_string(),
                LocalGroupAdministrationOperation::AssignUserToGroup,
                "daswriters".to_string(),
                Some("stephen".to_string()),
                true,
            )]
        );
    }

    #[test]
    fn dispatches_submit_ingest_files_with_clock_timestamp() {
        let service = FakeService::default();
        let handler =
            DaemonRequestHandler::new(service, FixedDaemonClock::new("2026-07-07T12:35:00Z"));
        let mut progress_events = Vec::new();

        let response = handler
            .handle_with_progress(
                DaemonApiRequest::SubmitIngestFiles(SubmitIngestFilesRequest {
                    endpoint: StoreId::new("zymo_fecal_2025.05").expect("store id"),
                    source_path: "/mnt/external/zymo".into(),
                    object_type: ObjectType::Fastq,
                    copies: Some(1),
                    hdd_workers: None,
                    ingress_origin: crate::api::DaemonIngressOrigin::LocalServer,
                    conflict_policy: crate::api::DaemonIngestConflictPolicy::Strict,
                    dry_run: true,
                    client_request_id: Some("request-3".to_string()),
                }),
                |event| {
                    progress_events.push(event);
                    Ok(())
                },
            )
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::SubmitIngestFiles(SubmitIngestFilesResponse {
                job_id,
                dry_run: true,
                ..
            }) if job_id.as_str() == "ingest-files-2026-07-07t12-35-00z"
        ));
        assert_eq!(
            handler
                .service_orchestrator
                .ingest_calls
                .borrow()
                .as_slice(),
            &[(
                "2026-07-07T12:35:00Z".to_string(),
                "zymo_fecal_2025.05".to_string(),
                true,
            )]
        );
        assert_eq!(progress_events.len(), 1);
        assert_eq!(progress_events[0].message.as_deref(), Some("queued"));
    }

    #[test]
    fn dispatches_prepare_enclosure_with_clock_timestamp() {
        let service = FakeService::default();
        let handler =
            DaemonRequestHandler::new(service, FixedDaemonClock::new("2026-07-08T19:40:00Z"));

        let response = handler
            .handle(DaemonApiRequest::PrepareEnclosure(
                PrepareEnclosureRequest {
                    ssd_device: "/dev/disk/by-id/nvme-ssd".into(),
                    hdd_devices: vec![PrepareEnclosureHddDevice {
                        disk_id: "qnap-1057".to_string(),
                        device_path: "/dev/disk/by-id/usb-qnap-1057".into(),
                    }],
                    mount_root: "/srv/dasobjectstore".into(),
                    filesystem: PrepareEnclosureFilesystem::Ext4,
                    owner: Some("stephen".to_string()),
                    dry_run: true,
                    client_request_id: Some("request-prepare-1".to_string()),
                    administrator_actor: Some("operator".to_string()),
                    allow_format: true,
                    existing_data_acknowledged: true,
                    confirmation_marker: ENCLOSURE_PREPARE_CONFIRMATION.to_string(),
                },
            ))
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::PrepareEnclosure(PrepareEnclosureResponse {
                accepted,
                ..
            }) if accepted.job_id.as_str() == "enclosure-prepare-2026-07-08t19-40-00z"
                && accepted.dry_run
        ));
        assert_eq!(
            handler
                .service_orchestrator
                .prepare_enclosure_calls
                .borrow()
                .as_slice(),
            &[(
                "2026-07-08T19:40:00Z".to_string(),
                "/dev/disk/by-id/nvme-ssd".to_string(),
                true,
            )]
        );
    }

    #[test]
    fn dispatches_create_object_store_with_clock_timestamp() {
        let service = FakeService::default();
        let handler =
            DaemonRequestHandler::new(service, FixedDaemonClock::new("2026-07-08T20:45:00Z"));

        let response = handler
            .handle(DaemonApiRequest::CreateObjectStore(
                create_object_store_request(),
            ))
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::CreateObjectStore(CreateObjectStoreResponse {
                accepted,
                store_id,
                ..
            }) if accepted.job_id.as_str() == "objectstore-create-2026-07-08t20-45-00z"
                && accepted.dry_run
                && store_id == "generated-data"
        ));
        assert_eq!(
            handler
                .service_orchestrator
                .create_object_store_calls
                .borrow()
                .as_slice(),
            &[(
                "2026-07-08T20:45:00Z".to_string(),
                "generated-data".to_string(),
                true,
            )]
        );
    }

    #[test]
    fn dispatches_endpoint_inventory_upsert_with_clock_timestamp() {
        let service = FakeService::default();
        let handler =
            DaemonRequestHandler::new(service, FixedDaemonClock::new("2026-07-09T00:05:00Z"));

        let response = handler
            .handle(DaemonApiRequest::UpsertEndpointInventory(
                endpoint_inventory_request(),
            ))
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::UpsertEndpointInventory(UpsertEndpointInventoryResponse {
                accepted,
                endpoint_id,
                ..
            }) if accepted.job_id.as_str() == "endpoint-upsert-2026-07-09t00-05-00z"
                && accepted.dry_run
                && endpoint_id == "nas-staging"
        ));
        assert_eq!(
            handler
                .service_orchestrator
                .endpoint_inventory_calls
                .borrow()
                .as_slice(),
            &[(
                "2026-07-09T00:05:00Z".to_string(),
                "nas-staging".to_string(),
                true,
            )]
        );
    }

    #[test]
    fn records_accepted_prepare_enclosure_job_in_registry() {
        let root = temp_root("record-prepare");
        let registry = Arc::new(FileBackedAdminJobRegistry::new(admin_job_registry_path(
            &root,
        )));
        let service = FakeService::default();
        let handler = DaemonRequestHandler::new_with_admin_job_registry(
            service,
            FixedDaemonClock::new("2026-07-08T19:40:00Z"),
            registry,
        );

        handler
            .handle(DaemonApiRequest::PrepareEnclosure(
                prepare_enclosure_request(),
            ))
            .expect("prepare request handled");
        let response = handler
            .handle(DaemonApiRequest::JobStatus(DaemonJobStatusRequest {
                job_id: DaemonJobId::new("enclosure-prepare-2026-07-08t19-40-00z").expect("job id"),
            }))
            .expect("status request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::JobStatus(DaemonJobStatusResponse {
                job: DaemonJobSummary {
                    kind: DaemonJobKind::EnclosurePreparation,
                    state: DaemonJobState::Complete,
                    ..
                }
            })
        ));

        cleanup(&root);
    }

    #[test]
    fn records_accepted_create_object_store_job_in_registry() {
        let root = temp_root("record-create-objectstore");
        let registry = Arc::new(FileBackedAdminJobRegistry::new(admin_job_registry_path(
            &root,
        )));
        let service = FakeService::default();
        let handler = DaemonRequestHandler::new_with_admin_job_registry(
            service,
            FixedDaemonClock::new("2026-07-08T20:45:00Z"),
            registry,
        );

        handler
            .handle(DaemonApiRequest::CreateObjectStore(
                create_object_store_request(),
            ))
            .expect("create objectstore request handled");
        let response = handler
            .handle(DaemonApiRequest::JobStatus(DaemonJobStatusRequest {
                job_id: DaemonJobId::new("objectstore-create-2026-07-08t20-45-00z")
                    .expect("job id"),
            }))
            .expect("status request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::JobStatus(DaemonJobStatusResponse {
                job: DaemonJobSummary {
                    kind: DaemonJobKind::ObjectStoreCreation,
                    state: DaemonJobState::Complete,
                    ..
                }
            })
        ));

        cleanup(&root);
    }

    #[test]
    fn records_accepted_endpoint_inventory_job_in_registry() {
        let root = temp_root("record-endpoint-inventory");
        let registry = Arc::new(FileBackedAdminJobRegistry::new(admin_job_registry_path(
            &root,
        )));
        let service = FakeService::default();
        let handler = DaemonRequestHandler::new_with_admin_job_registry(
            service,
            FixedDaemonClock::new("2026-07-09T00:05:00Z"),
            registry,
        );

        handler
            .handle(DaemonApiRequest::UpsertEndpointInventory(
                endpoint_inventory_request(),
            ))
            .expect("endpoint inventory request handled");
        let response = handler
            .handle(DaemonApiRequest::JobStatus(DaemonJobStatusRequest {
                job_id: DaemonJobId::new("endpoint-upsert-2026-07-09t00-05-00z").expect("job id"),
            }))
            .expect("status request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::JobStatus(DaemonJobStatusResponse {
                job: DaemonJobSummary {
                    kind: DaemonJobKind::EndpointValidation,
                    state: DaemonJobState::Complete,
                    ..
                }
            })
        ));

        cleanup(&root);
    }

    #[test]
    fn registry_cancel_reports_completed_prepare_job_not_cancelled() {
        let root = temp_root("cancel-complete");
        let registry = Arc::new(FileBackedAdminJobRegistry::new(admin_job_registry_path(
            &root,
        )));
        let service = FakeService::default();
        let handler = DaemonRequestHandler::new_with_admin_job_registry(
            service,
            FixedDaemonClock::new("2026-07-08T19:40:00Z"),
            registry,
        );

        handler
            .handle(DaemonApiRequest::PrepareEnclosure(
                prepare_enclosure_request(),
            ))
            .expect("prepare request handled");
        let response = handler
            .handle(DaemonApiRequest::CancelJob(DaemonJobCancelRequest {
                job_id: DaemonJobId::new("enclosure-prepare-2026-07-08t19-40-00z").expect("job id"),
                reason: Some("operator requested cancellation".to_string()),
            }))
            .expect("cancel request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::CancelJob(DaemonJobCancelResponse {
                accepted: false,
                state: DaemonJobState::Complete,
                ..
            })
        ));

        cleanup(&root);
    }

    #[test]
    fn lists_recorded_admin_jobs_from_registry() {
        let root = temp_root("list-recorded");
        let registry = Arc::new(FileBackedAdminJobRegistry::new(admin_job_registry_path(
            &root,
        )));
        let service = FakeService::default();
        let handler = DaemonRequestHandler::new_with_admin_job_registry(
            service,
            FixedDaemonClock::new("2026-07-08T19:40:00Z"),
            registry,
        );

        handler
            .handle(DaemonApiRequest::PrepareEnclosure(
                prepare_enclosure_request(),
            ))
            .expect("prepare request handled");
        let response = handler
            .handle(DaemonApiRequest::JobList(DaemonJobListRequest {
                limit: Some(10),
            }))
            .expect("list request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::JobList(DaemonJobListResponse { jobs })
                if jobs.len() == 1
                    && jobs[0].kind == DaemonJobKind::EnclosurePreparation
                    && jobs[0].state == DaemonJobState::Complete
        ));

        cleanup(&root);
    }

    #[test]
    fn dispatches_job_status_to_orchestrator() {
        let service = FakeService::default();
        let handler =
            DaemonRequestHandler::new(service, FixedDaemonClock::new("2026-07-08T20:05:00Z"));

        let response = handler
            .handle(DaemonApiRequest::JobStatus(DaemonJobStatusRequest {
                job_id: DaemonJobId::new("enclosure-prepare-1").expect("job id"),
            }))
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::JobStatus(DaemonJobStatusResponse {
                job: DaemonJobSummary {
                    kind: DaemonJobKind::EnclosurePreparation,
                    state: DaemonJobState::Running,
                    ..
                }
            })
        ));
        assert_eq!(
            handler
                .service_orchestrator
                .job_status_calls
                .borrow()
                .as_slice(),
            &["enclosure-prepare-1".to_string()]
        );
    }

    #[test]
    fn dispatches_cancel_job_with_clock_timestamp() {
        let service = FakeService::default();
        let handler =
            DaemonRequestHandler::new(service, FixedDaemonClock::new("2026-07-08T20:06:00Z"));

        let response = handler
            .handle(DaemonApiRequest::CancelJob(DaemonJobCancelRequest {
                job_id: DaemonJobId::new("enclosure-prepare-1").expect("job id"),
                reason: Some("operator requested cancellation".to_string()),
            }))
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::CancelJob(DaemonJobCancelResponse {
                accepted: true,
                state: DaemonJobState::Cancelled,
                ..
            })
        ));
        assert_eq!(
            handler
                .service_orchestrator
                .cancel_job_calls
                .borrow()
                .as_slice(),
            &[(
                "2026-07-08T20:06:00Z".to_string(),
                "enclosure-prepare-1".to_string(),
            )]
        );
    }

    #[test]
    fn reports_submit_ingest_runtime_failures_as_api_errors() {
        let service = FakeService {
            ingest_error: Some("source is unreadable".to_string()),
            ..FakeService::default()
        };
        let handler = DaemonRequestHandler::new(service, FixedDaemonClock::new("now"));

        let response = handler
            .handle(DaemonApiRequest::SubmitIngestFiles(
                SubmitIngestFilesRequest {
                    endpoint: StoreId::new("zymo_fecal_2025.05").expect("store id"),
                    source_path: "/mnt/external/zymo".into(),
                    object_type: ObjectType::Naive,
                    copies: None,
                    hdd_workers: None,
                    ingress_origin: crate::api::DaemonIngressOrigin::LocalServer,
                    conflict_policy: crate::api::DaemonIngestConflictPolicy::Strict,
                    dry_run: false,
                    client_request_id: None,
                },
            ))
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::Error(error) if error.code == "ingest_files_failed"
                && error.message == "source is unreadable"
        ));
    }

    #[test]
    fn authorizes_daemon_ingest_when_actor_has_store_writer_group() {
        let root = temp_root("ingest-auth-allowed");
        let (store_registry, subobject_registry) =
            write_test_store_registry(&root, "zymo_fecal_2025.05", Some("mnemosyne"));
        let service = FakeService::default();
        let handler =
            DaemonRequestHandler::new(service, FixedDaemonClock::new("2026-07-09T09:25:00Z"))
                .with_registry_paths(store_registry, subobject_registry);
        let actor = DaemonLocalActor::new(1000)
            .with_username("stephen")
            .with_groups(["mnemosyne"]);

        let response = handler
            .handle_with_progress_for_actor(
                DaemonApiRequest::SubmitIngestFiles(SubmitIngestFilesRequest {
                    endpoint: StoreId::new("zymo_fecal_2025.05").expect("store id"),
                    source_path: "/mnt/external/zymo".into(),
                    object_type: ObjectType::Naive,
                    copies: None,
                    hdd_workers: None,
                    ingress_origin: crate::api::DaemonIngressOrigin::LocalServer,
                    conflict_policy: crate::api::DaemonIngestConflictPolicy::Strict,
                    dry_run: false,
                    client_request_id: None,
                }),
                Some(&actor),
                |_| Ok(()),
            )
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::SubmitIngestFiles(SubmitIngestFilesResponse { .. })
        ));
        assert_eq!(
            handler
                .service_orchestrator
                .ingest_calls
                .borrow()
                .as_slice(),
            &[(
                "2026-07-09T09:25:00Z".to_string(),
                "zymo_fecal_2025.05".to_string(),
                false,
            )]
        );

        cleanup(&root);
    }

    #[test]
    fn rejects_daemon_ingest_when_actor_lacks_store_writer_group() {
        let root = temp_root("ingest-auth-denied");
        let (store_registry, subobject_registry) =
            write_test_store_registry(&root, "zymo_fecal_2025.05", Some("mnemosyne"));
        let service = FakeService::default();
        let handler =
            DaemonRequestHandler::new(service, FixedDaemonClock::new("2026-07-09T09:25:00Z"))
                .with_registry_paths(store_registry, subobject_registry);
        let actor = DaemonLocalActor::new(1001)
            .with_username("guest")
            .with_groups(["users"]);

        let response = handler
            .handle_with_progress_for_actor(
                DaemonApiRequest::SubmitIngestFiles(SubmitIngestFilesRequest {
                    endpoint: StoreId::new("zymo_fecal_2025.05").expect("store id"),
                    source_path: "/mnt/external/zymo".into(),
                    object_type: ObjectType::Naive,
                    copies: None,
                    hdd_workers: None,
                    ingress_origin: crate::api::DaemonIngressOrigin::LocalServer,
                    conflict_policy: crate::api::DaemonIngestConflictPolicy::Strict,
                    dry_run: false,
                    client_request_id: None,
                }),
                Some(&actor),
                |_| Ok(()),
            )
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::Error(error) if error.code == "permission_denied"
                && error.message.contains("membership in group mnemosyne is required")
        ));
        assert!(handler
            .service_orchestrator
            .ingest_calls
            .borrow()
            .is_empty());

        cleanup(&root);
    }

    #[test]
    fn rejects_appliance_telemetry_without_authenticated_actor() {
        let root = temp_root("telemetry-auth-missing");
        let service = FakeService::default();
        let handler =
            DaemonRequestHandler::new(service, FixedDaemonClock::new("2026-07-09T18:30:00Z"))
                .with_appliance_telemetry_state_path(root.join("telemetry.json"));

        let response = handler
            .handle(DaemonApiRequest::ApplianceTelemetry(
                ApplianceTelemetryRequest {
                    window: ApplianceTelemetryWindow::OneHour,
                },
            ))
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::Error(error) if error.code == "permission_denied"
                && error.message.contains("authenticated daemon actor is required")
        ));
    }

    #[test]
    fn appliance_telemetry_reports_missing_state_for_authenticated_actor() {
        let root = temp_root("telemetry-state-missing");
        let service = FakeService::default();
        let handler =
            DaemonRequestHandler::new(service, FixedDaemonClock::new("2026-07-09T18:30:00Z"))
                .with_appliance_telemetry_state_path(root.join("telemetry.json"));
        let actor = DaemonLocalActor::new(1000).with_username("stephen");

        let response = handler
            .handle_with_progress_for_actor(
                DaemonApiRequest::ApplianceTelemetry(ApplianceTelemetryRequest {
                    window: ApplianceTelemetryWindow::OneDay,
                }),
                Some(&actor),
                |_| Ok(()),
            )
            .expect("request handled");

        match response {
            DaemonApiResponse::ApplianceTelemetry(response) => {
                assert_eq!(response.state, ApplianceTelemetryState::Missing);
                assert_eq!(response.requested_window, ApplianceTelemetryWindow::OneDay);
                assert!(response.current.is_none());
            }
            other => panic!("expected appliance telemetry response, got {other:?}"),
        }
    }

    #[test]
    fn appliance_telemetry_returns_current_summary_for_authenticated_actor() {
        let root = temp_root("telemetry-state-available");
        let telemetry_path = root.join("telemetry/appliance-telemetry.v1.json");
        fs::create_dir_all(telemetry_path.parent().expect("telemetry parent"))
            .expect("telemetry parent");
        fs::write(&telemetry_path, appliance_telemetry_fixture_json())
            .expect("telemetry fixture written");
        let service = FakeService::default();
        let handler =
            DaemonRequestHandler::new(service, FixedDaemonClock::new("2026-07-09T18:30:00Z"))
                .with_appliance_telemetry_state_path(&telemetry_path);
        let actor = DaemonLocalActor::new(1000).with_username("stephen");

        let response = handler
            .handle_with_progress_for_actor(
                DaemonApiRequest::ApplianceTelemetry(ApplianceTelemetryRequest {
                    window: ApplianceTelemetryWindow::OneHour,
                }),
                Some(&actor),
                |_| Ok(()),
            )
            .expect("request handled");
        cleanup(&root);

        match response {
            DaemonApiResponse::ApplianceTelemetry(response) => {
                assert_eq!(response.state, ApplianceTelemetryState::Available);
                assert_eq!(response.series.cpu_usage.len(), 1);
                let current = response.current.expect("current telemetry");
                assert_eq!(current.cpu_usage_percent_basis_points, Some(4_250));
                assert_eq!(current.memory_used_percent_basis_points, Some(2_500));
                assert_eq!(current.sessions.web_active_sessions, Some(2));
            }
            other => panic!("expected appliance telemetry response, got {other:?}"),
        }
    }

    #[test]
    fn rejects_daemon_object_browser_without_authenticated_actor() {
        let root = temp_root("browser-auth-missing");
        let (store_registry, subobject_registry) =
            write_test_store_registry(&root, "ena", Some("mnemosyne"));
        let live_sqlite = create_live_sqlite(&root, "ena");
        insert_browser_object(&live_sqlite, "ena/raw/sample.fastq.gz", "Protected", true);
        let handler =
            DaemonRequestHandler::new(FakeService::default(), FixedDaemonClock::new("now"))
                .with_registry_paths(store_registry, subobject_registry)
                .with_live_sqlite_path(live_sqlite);

        let response = handler
            .handle(DaemonApiRequest::ObjectBrowser(object_browser_request(
                "ena",
            )))
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::Error(error) if error.code == "permission_denied"
                && error.message.contains("authenticated daemon actor is required")
        ));

        cleanup(&root);
    }

    #[test]
    fn rejects_daemon_object_browser_when_actor_lacks_writer_group() {
        let root = temp_root("browser-auth-denied");
        let (store_registry, subobject_registry) =
            write_test_store_registry(&root, "ena", Some("mnemosyne"));
        let live_sqlite = create_live_sqlite(&root, "ena");
        insert_browser_object(&live_sqlite, "ena/raw/sample.fastq.gz", "Protected", true);
        let handler =
            DaemonRequestHandler::new(FakeService::default(), FixedDaemonClock::new("now"))
                .with_registry_paths(store_registry, subobject_registry)
                .with_live_sqlite_path(live_sqlite);
        let actor = DaemonLocalActor::new(1001)
            .with_username("guest")
            .with_groups(["users"]);

        let response = handler
            .handle_with_progress_for_actor(
                DaemonApiRequest::ObjectBrowser(object_browser_request("ena")),
                Some(&actor),
                |_| Ok(()),
            )
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::Error(error) if error.code == "permission_denied"
                && error.message.contains("membership in writer group mnemosyne is required")
        ));

        cleanup(&root);
    }

    #[test]
    fn daemon_object_browser_allows_service_peer_to_delegate_authenticated_local_actor() {
        let root = temp_root("browser-delegated-allowed");
        let (store_registry, subobject_registry) =
            write_test_store_registry(&root, "ena", Some("mnemosyne"));
        let live_sqlite = create_live_sqlite(&root, "ena");
        insert_browser_object(&live_sqlite, "ena/raw/sample.fastq.gz", "Protected", true);
        let handler =
            DaemonRequestHandler::new(FakeService::default(), FixedDaemonClock::new("now"))
                .with_registry_paths(store_registry, subobject_registry)
                .with_live_sqlite_path(live_sqlite);
        let peer_actor = DaemonLocalActor::new(997)
            .with_username("dasobjectstore")
            .with_groups(["dasobjectstore"]);
        let mut request = object_browser_request("ena");
        request.delegated_actor = Some(ObjectBrowserDelegatedActor {
            username: "stephen".to_string(),
            uid: None,
            primary_gid: None,
            groups: vec!["mnemosyne".to_string()],
        });

        let response = handler
            .handle_with_progress_for_actor(
                DaemonApiRequest::ObjectBrowser(request),
                Some(&peer_actor),
                |_| Ok(()),
            )
            .expect("request handled");

        assert!(matches!(response, DaemonApiResponse::ObjectBrowser(_)));

        cleanup(&root);
    }

    #[test]
    fn daemon_object_browser_rejects_delegation_from_non_service_peer() {
        let root = temp_root("browser-delegated-denied");
        let (store_registry, subobject_registry) =
            write_test_store_registry(&root, "ena", Some("mnemosyne"));
        let live_sqlite = create_live_sqlite(&root, "ena");
        insert_browser_object(&live_sqlite, "ena/raw/sample.fastq.gz", "Protected", true);
        let handler =
            DaemonRequestHandler::new(FakeService::default(), FixedDaemonClock::new("now"))
                .with_registry_paths(store_registry, subobject_registry)
                .with_live_sqlite_path(live_sqlite);
        let peer_actor = DaemonLocalActor::new(1001)
            .with_username("guest")
            .with_groups(["users"]);
        let mut request = object_browser_request("ena");
        request.delegated_actor = Some(ObjectBrowserDelegatedActor {
            username: "stephen".to_string(),
            uid: None,
            primary_gid: None,
            groups: vec!["mnemosyne".to_string()],
        });

        let response = handler
            .handle_with_progress_for_actor(
                DaemonApiRequest::ObjectBrowser(request),
                Some(&peer_actor),
                |_| Ok(()),
            )
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::Error(error) if error.code == "permission_denied"
                && error.message.contains("not authorized to delegate")
        ));

        cleanup(&root);
    }

    #[test]
    fn daemon_object_browser_allows_reader_group_without_writer_membership() {
        let root = temp_root("browser-reader-allowed");
        let (store_registry, subobject_registry) = write_test_store_registry_with_read_policy(
            &root,
            "ena",
            Some("readers"),
            Some("mnemosyne"),
            false,
        );
        let live_sqlite = create_live_sqlite(&root, "ena");
        insert_browser_object(&live_sqlite, "ena/raw/sample.fastq.gz", "Protected", true);
        let handler =
            DaemonRequestHandler::new(FakeService::default(), FixedDaemonClock::new("now"))
                .with_registry_paths(store_registry, subobject_registry)
                .with_live_sqlite_path(live_sqlite);
        let actor = DaemonLocalActor::new(1001)
            .with_username("reader")
            .with_groups(["readers"]);

        let response = handler
            .handle_with_progress_for_actor(
                DaemonApiRequest::ObjectBrowser(object_browser_request("ena")),
                Some(&actor),
                |_| Ok(()),
            )
            .expect("request handled");

        assert!(matches!(response, DaemonApiResponse::ObjectBrowser(_)));

        cleanup(&root);
    }

    #[test]
    fn daemon_object_download_allows_reader_group_without_writer_membership() {
        let root = temp_root("download-reader-allowed");
        let (store_registry, subobject_registry) = write_test_store_registry_with_read_policy(
            &root,
            "ena",
            Some("readers"),
            Some("mnemosyne"),
            false,
        );
        let live_sqlite = create_live_sqlite(&root, "ena");
        insert_browser_object(&live_sqlite, "ena/raw/sample.fastq.gz", "Protected", true);
        let hdd_root = root.join("hdd");
        let disk_root = hdd_root.join("qnap-1057");
        write_hdd_marker(&disk_root, "qnap-1057");
        let source_path = disk_root.join("ena/raw/sample.fastq.gz");
        fs::create_dir_all(source_path.parent().expect("source parent")).expect("source parent");
        fs::write(&source_path, b"download payload").expect("write source");
        let handler =
            DaemonRequestHandler::new(FakeService::default(), FixedDaemonClock::new("now"))
                .with_registry_paths(store_registry, subobject_registry)
                .with_live_sqlite_path(live_sqlite)
                .with_hdd_root_path(hdd_root);
        let actor = DaemonLocalActor::new(1001)
            .with_username("reader")
            .with_groups(["readers"]);

        let response = handler
            .handle_with_progress_for_actor(
                DaemonApiRequest::ObjectDownload(ObjectDownloadRequest {
                    endpoint: StoreId::new("ena").expect("store id"),
                    object_id: ObjectId::new("ena/raw/sample.fastq.gz").expect("object id"),
                    delegated_actor: None,
                }),
                Some(&actor),
                |_| Ok(()),
            )
            .expect("request handled");

        let DaemonApiResponse::ObjectDownload(response) = response else {
            panic!("expected object download response");
        };
        assert_eq!(response.file_name, "sample.fastq.gz");
        assert_eq!(response.source_path, source_path);
        assert_eq!(response.size_bytes, b"download payload".len() as u64);

        cleanup(&root);
    }

    #[test]
    fn daemon_object_folder_download_allows_reader_group_without_writer_membership() {
        let root = temp_root("folder-download-reader-allowed");
        let (store_registry, subobject_registry) = write_test_store_registry_with_read_policy(
            &root,
            "ena",
            Some("readers"),
            Some("mnemosyne"),
            false,
        );
        let live_sqlite = create_live_sqlite(&root, "ena");
        insert_browser_object(
            &live_sqlite,
            "ena/raw/Xeno/sample.fastq.gz",
            "Protected",
            true,
        );
        insert_browser_object(&live_sqlite, "ena/raw/Xeno/metadata.tsv", "Protected", true);
        let hdd_root = root.join("hdd");
        let disk_root = hdd_root.join("qnap-1057");
        write_hdd_marker(&disk_root, "qnap-1057");
        let sample_path = disk_root.join("ena/raw/Xeno/sample.fastq.gz");
        fs::create_dir_all(sample_path.parent().expect("sample parent")).expect("sample parent");
        fs::write(&sample_path, b"sample payload").expect("write sample");
        let metadata_path = disk_root.join("ena/raw/Xeno/metadata.tsv");
        fs::write(&metadata_path, b"metadata").expect("write metadata");
        let handler =
            DaemonRequestHandler::new(FakeService::default(), FixedDaemonClock::new("now"))
                .with_registry_paths(store_registry, subobject_registry)
                .with_live_sqlite_path(live_sqlite)
                .with_hdd_root_path(hdd_root);
        let actor = DaemonLocalActor::new(1001)
            .with_username("reader")
            .with_groups(["readers"]);

        let response = handler
            .handle_with_progress_for_actor(
                DaemonApiRequest::ObjectFolderDownload(ObjectFolderDownloadRequest {
                    endpoint: StoreId::new("ena").expect("store id"),
                    prefix: "ena/raw/Xeno".to_string(),
                    delegated_actor: None,
                }),
                Some(&actor),
                |_| Ok(()),
            )
            .expect("request handled");

        let DaemonApiResponse::ObjectFolderDownload(response) = response else {
            panic!("expected object folder download response, got {response:?}");
        };
        assert_eq!(response.archive_name, "Xeno.tar.gz");
        assert_eq!(response.total_files, 2);
        assert_eq!(
            response.total_source_bytes,
            b"sample payload".len() as u64 + b"metadata".len() as u64
        );
        assert_eq!(response.entries[0].archive_path, "metadata.tsv");
        assert_eq!(response.entries[1].archive_path, "sample.fastq.gz");

        cleanup(&root);
    }

    #[test]
    fn daemon_object_browser_allows_public_store_for_authenticated_actor() {
        let root = temp_root("browser-public-allowed");
        let (store_registry, subobject_registry) =
            write_test_store_registry_with_read_policy(&root, "ena", None, None, true);
        let live_sqlite = create_live_sqlite(&root, "ena");
        insert_browser_object(&live_sqlite, "ena/raw/sample.fastq.gz", "Protected", true);
        let handler =
            DaemonRequestHandler::new(FakeService::default(), FixedDaemonClock::new("now"))
                .with_registry_paths(store_registry, subobject_registry)
                .with_live_sqlite_path(live_sqlite);
        let actor = DaemonLocalActor::new(1001)
            .with_username("guest")
            .with_groups(["users"]);

        let response = handler
            .handle_with_progress_for_actor(
                DaemonApiRequest::ObjectBrowser(object_browser_request("ena")),
                Some(&actor),
                |_| Ok(()),
            )
            .expect("request handled");

        assert!(matches!(response, DaemonApiResponse::ObjectBrowser(_)));

        cleanup(&root);
    }

    #[test]
    fn daemon_object_browser_returns_authorized_live_metadata_with_readiness() {
        let root = temp_root("browser-auth-allowed");
        let (store_registry, subobject_registry) =
            write_test_store_registry(&root, "ena", Some("mnemosyne"));
        let live_sqlite = create_live_sqlite(&root, "ena");
        insert_browser_object(&live_sqlite, "ena/raw/sample.fastq.gz", "Protected", true);
        insert_browser_object(
            &live_sqlite,
            "ena/raw/redownload.fastq.gz",
            "RedownloadRequired",
            false,
        );
        let handler =
            DaemonRequestHandler::new(FakeService::default(), FixedDaemonClock::new("now"))
                .with_registry_paths(store_registry, subobject_registry)
                .with_live_sqlite_path(live_sqlite);
        let actor = DaemonLocalActor::new(1000)
            .with_username("stephen")
            .with_groups(["mnemosyne"]);

        let response = handler
            .handle_with_progress_for_actor(
                DaemonApiRequest::ObjectBrowser(ObjectBrowserRequest {
                    prefix: Some("raw".to_string()),
                    include_placement: true,
                    ..object_browser_request("ena")
                }),
                Some(&actor),
                |_| Ok(()),
            )
            .expect("request handled");

        let DaemonApiResponse::ObjectBrowser(response) = response else {
            panic!("expected object browser response");
        };
        assert_eq!(response.files.len(), 2);
        assert_eq!(response.files[0].name, "redownload.fastq.gz");
        assert_eq!(
            response.files[0].readiness,
            ObjectBrowserReadinessState::RedownloadRequired
        );
        assert_eq!(response.files[1].name, "sample.fastq.gz");
        assert_eq!(
            response.files[1].readiness,
            ObjectBrowserReadinessState::Available
        );
        assert_eq!(response.files[1].copy_count, 1);
        assert_eq!(
            response.files[1].placements[0].location,
            ObjectBrowserPlacementLocation::HddSettled
        );
        assert_eq!(
            response.files[1].placements[0].state,
            ObjectBrowserPlacementState::Verified
        );

        cleanup(&root);
    }

    #[test]
    fn validates_request_before_dispatch() {
        let service = FakeService::default();
        let handler = DaemonRequestHandler::new(service, FixedDaemonClock::new("now"));

        let error = handler
            .handle(DaemonApiRequest::ServiceLifecycle(
                DaemonServiceLifecycleRequest {
                    operation: DaemonServiceOperation::Start,
                    provider_id: ObjectServiceProviderId::Rustfs,
                    dry_run: false,
                    client_request_id: None,
                },
            ))
            .expect_err("invalid request rejected");

        assert!(matches!(
            error,
            super::DaemonRequestHandlerError::RequestValidation(
                DaemonRequestValidationError::UnsupportedServiceProvider { .. }
            )
        ));
        assert!(handler
            .service_orchestrator
            .lifecycle_calls
            .borrow()
            .is_empty());
    }

    #[test]
    fn daemon_handles_remote_upload_admission_decisions() {
        let service = FakeService::default();
        let handler = DaemonRequestHandler::new(service, FixedDaemonClock::new("now"));

        let response = handler
            .handle(DaemonApiRequest::RemoteEasyconnectUploadAdmission(
                RemoteEasyconnectUploadAdmissionRequest {
                    policy: RemoteUploadBackpressurePolicy::default(),
                    ssd_pressure: DaemonSsdPressure::Critical,
                    active_s3_transfers: 0,
                    ssd_stage_queue_depth: 0,
                    hdd_landing_queue_depth: 0,
                    verification_queue_depth: 0,
                },
            ))
            .expect("request handled");

        let DaemonApiResponse::RemoteEasyconnectUploadAdmission(decision) = response else {
            panic!("expected remote upload admission decision");
        };

        assert_eq!(
            decision.action,
            RemoteUploadBackpressureAction::RejectNewTransfers
        );
        assert_eq!(
            decision.reason,
            RemoteEasyconnectUploadBackpressureReason::SsdCriticalPressure
        );
    }

    #[test]
    fn daemon_remote_upload_admission_uses_runtime_gate_state() {
        let service = FakeService::default();
        let policy = RemoteUploadBackpressurePolicy::default();
        let gate = Arc::new(RemoteUploadAdmissionGate::new());
        gate.try_begin_s3_transfer(policy, DaemonSsdPressure::AcceptingWrites);
        gate.try_begin_s3_transfer(policy, DaemonSsdPressure::AcceptingWrites);
        let handler = DaemonRequestHandler::new(service, FixedDaemonClock::new("now"))
            .with_remote_upload_admission_gate(Arc::clone(&gate));

        let response = handler
            .handle(DaemonApiRequest::RemoteEasyconnectUploadAdmission(
                RemoteEasyconnectUploadAdmissionRequest {
                    policy,
                    ssd_pressure: DaemonSsdPressure::AcceptingWrites,
                    active_s3_transfers: 0,
                    ssd_stage_queue_depth: 0,
                    hdd_landing_queue_depth: 0,
                    verification_queue_depth: 0,
                },
            ))
            .expect("request handled");

        let DaemonApiResponse::RemoteEasyconnectUploadAdmission(decision) = response else {
            panic!("expected remote upload admission decision");
        };

        assert_eq!(
            decision.action,
            RemoteUploadBackpressureAction::PauseNewTransfers
        );
        assert_eq!(
            decision.reason,
            RemoteEasyconnectUploadBackpressureReason::S3TransferConcurrencyFull
        );
        assert_eq!(gate.snapshot().active_s3_transfers, 2);
    }

    #[test]
    fn daemon_submits_remote_easyconnect_aws_cli_upload_job() {
        let root = temp_root("remote-easyconnect-submit-aws-cli");
        let registry = Arc::new(FileBackedAdminJobRegistry::new(admin_job_registry_path(
            &root,
        )));
        let service = FakeService::default();
        let handler = DaemonRequestHandler::new_with_admin_job_registry(
            service,
            FixedDaemonClock::new("2026-07-09T14:40:00Z"),
            registry,
        );

        let response = handler
            .handle(DaemonApiRequest::RemoteEasyconnectSubmitAwsCliUpload(
                RemoteEasyconnectSubmitAwsCliUploadRequest {
                    job_id: "remote-upload-job-1".to_string(),
                    object_store: "zymo_fecal_2025.05".to_string(),
                    source_bytes: 42,
                    policy: RemoteUploadBackpressurePolicy::default(),
                    ssd_pressure: DaemonSsdPressure::AcceptingWrites,
                    program: "aws".to_string(),
                    args: vec![
                        "s3".to_string(),
                        "cp".to_string(),
                        "/private/source/reads.fastq.gz".to_string(),
                        "s3://dos-zymo/raw/reads.fastq.gz".to_string(),
                    ],
                    display_args: vec![
                        "s3".to_string(),
                        "cp".to_string(),
                        "<source-redacted>".to_string(),
                        "s3://dos-zymo/raw/reads.fastq.gz".to_string(),
                    ],
                    environment: vec![RemoteEasyconnectAwsCliEnvironmentVariable {
                        name: "AWS_ACCESS_KEY_ID".to_string(),
                        value: "AKIAEXAMPLE".to_string(),
                    }],
                    progress_telemetry: None,
                    progress_message: Some("completed".to_string()),
                },
            ))
            .expect("request handled");

        let DaemonApiResponse::RemoteEasyconnectSubmitAwsCliUpload(response) = response else {
            panic!("expected remote easyconnect AWS CLI upload response");
        };
        let crate::api::DaemonJobEvent::Complete(job) = response.final_event else {
            panic!("expected complete final event");
        };
        assert_eq!(job.kind, DaemonJobKind::RemoteUpload);
        assert_eq!(job.progress.work_bytes_done, 42);
        assert_eq!(
            handler
                .service_orchestrator
                .remote_upload_calls
                .borrow()
                .as_slice(),
            [(
                "remote-upload-job-1".to_string(),
                "zymo_fecal_2025.05".to_string(),
                42,
                "aws".to_string(),
                1
            )]
        );

        cleanup(&root);
    }

    #[test]
    fn daemon_revoke_easyconnect_session_updates_persisted_store() {
        let root = temp_root("remote-easyconnect-revoke-session");
        let session_store_path = remote_easyconnect_session_store_path(&root);
        let session_store = FileBackedRemoteEasyconnectPairedSessionStore::new(&session_store_path);
        session_store
            .upsert(paired_session("session-1"))
            .expect("session stored");
        let handler = DaemonRequestHandler::new(
            FakeService::default(),
            FixedDaemonClock::new("2026-07-09T16:20:00Z"),
        )
        .with_remote_easyconnect_session_store_path(&session_store_path);

        let response = handler
            .handle(DaemonApiRequest::RemoteEasyconnectRevokeSession(
                RemoteEasyconnectRevokeSessionRequest {
                    session_id: "session-1".to_string(),
                    reason: Some("operator requested revocation".to_string()),
                },
            ))
            .expect("request handled");

        let DaemonApiResponse::RemoteEasyconnectRevokeSession(response) = response else {
            panic!("expected remote easyconnect revoke response");
        };
        assert!(response.revoked);
        assert_eq!(response.revoked_at_utc, "2026-07-09T16:20:00Z");
        let stored = session_store
            .get("session-1")
            .expect("session loaded")
            .expect("session exists");
        assert_eq!(
            stored.revoked_at_utc.as_deref(),
            Some("2026-07-09T16:20:00Z")
        );

        cleanup(&root);
    }

    #[test]
    fn daemon_renew_easyconnect_session_rotates_token_and_persists_expiry() {
        let root = temp_root("remote-easyconnect-renew-session");
        let session_store_path = remote_easyconnect_session_store_path(&root);
        let session_store = FileBackedRemoteEasyconnectPairedSessionStore::new(&session_store_path);
        session_store
            .upsert(paired_session("session-1"))
            .expect("session stored");
        let handler = DaemonRequestHandler::new(
            FakeService::default(),
            FixedDaemonClock::new("2026-07-09T16:20:00Z"),
        )
        .with_remote_easyconnect_session_store_path(&session_store_path);

        let response = handler
            .handle(DaemonApiRequest::RemoteEasyconnectRenewSession(
                RemoteEasyconnectRenewSessionRequest {
                    session_id: "session-1".to_string(),
                    renewal_token: "renewal-token-1".to_string(),
                    requested_lifetime_seconds: Some(28_800),
                },
            ))
            .expect("request handled");

        let DaemonApiResponse::RemoteEasyconnectRenewSession(response) = response else {
            panic!("expected remote easyconnect renew response");
        };
        assert_eq!(response.session.session_id, "session-1");
        assert_eq!(response.session.issued_at_utc, "2026-07-09T16:20:00Z");
        assert_eq!(response.session.expires_at_utc, "2026-07-10T00:20:00Z");
        assert_eq!(
            response.session.renewal.renew_after_utc,
            "2026-07-09T23:20:00Z"
        );
        assert_eq!(response.session.credentials.access_key_id, "AKIAEXAMPLE");
        assert_ne!(response.session.renewal.renewal_token, "renewal-token-1");

        let stored = session_store
            .get("session-1")
            .expect("session loaded")
            .expect("session exists");
        assert_eq!(stored.expires_at_utc, "2026-07-10T00:20:00Z");
        assert_eq!(stored.renewal_token, response.session.renewal.renewal_token);

        cleanup(&root);
    }

    #[test]
    fn daemon_easyconnect_pairing_exchange_persists_session() {
        let root = temp_root("remote-easyconnect-pairing-exchange");
        let pairing_store_path = remote_easyconnect_pairing_store_path(&root);
        let session_store_path = remote_easyconnect_session_store_path(&root);
        let handler = DaemonRequestHandler::new(
            FakeService::default(),
            FixedDaemonClock::new("2026-07-09T16:20:00Z"),
        )
        .with_remote_easyconnect_pairing_store_path(&pairing_store_path)
        .with_remote_easyconnect_session_store_path(&session_store_path);

        let create = handler
            .handle(DaemonApiRequest::RemoteEasyconnectCreatePairing(
                RemoteEasyconnectCreatePairingRequest {
                    client_name: "macbook-pro".to_string(),
                    callback_url: "http://127.0.0.1:49321/callback".to_string(),
                    requested_object_store: Some("zymo_fecal_2025.05".to_string()),
                    requested_session_lifetime_seconds: Some(28_800),
                    client_request_id: Some("request-1".to_string()),
                },
            ))
            .expect("create handled");
        let DaemonApiResponse::RemoteEasyconnectCreatePairing(create) = create else {
            panic!("expected create pairing response");
        };
        let grant = paired_session("session-template")
            .object_stores
            .into_iter()
            .next()
            .expect("session fixture grant");

        let approve = handler
            .handle(DaemonApiRequest::RemoteEasyconnectApprovePairing(
                RemoteEasyconnectApprovePairingRequest {
                    pairing_id: create.pairing_id.clone(),
                    approved_actor: "stephen".to_string(),
                    auth_provider: RemoteEasyconnectAuthProvider::StandaloneLocalUser,
                    allowed_object_stores: vec![grant],
                    approval_expires_at_utc: "2026-07-09T16:30:00Z".to_string(),
                },
            ))
            .expect("approve handled");
        let DaemonApiResponse::RemoteEasyconnectApprovePairing(approve) = approve else {
            panic!("expected approve pairing response");
        };

        let exchange = handler
            .handle(DaemonApiRequest::RemoteEasyconnectExchangePairing(
                RemoteEasyconnectExchangePairingRequest {
                    pairing_id: create.pairing_id,
                    exchange_code: approve.exchange_code,
                    client_request_id: Some("request-2".to_string()),
                },
            ))
            .expect("exchange handled");
        let DaemonApiResponse::RemoteEasyconnectExchangePairing(exchange) = exchange else {
            panic!("expected exchange pairing response");
        };

        assert_eq!(exchange.session.issued_at_utc, "2026-07-09T16:20:00Z");
        assert_eq!(exchange.session.expires_at_utc, "2026-07-10T00:20:00Z");
        assert!(exchange
            .session
            .credentials
            .access_key_id
            .starts_with("DOS"));
        assert!(!exchange.session.credentials.secret_access_key.is_empty());
        assert_eq!(exchange.object_stores[0].object_store, "zymo_fecal_2025.05");
        let stored = FileBackedRemoteEasyconnectPairedSessionStore::new(&session_store_path)
            .get(&exchange.session.session_id)
            .expect("session store read")
            .expect("session persisted");
        assert_eq!(stored.approved_actor, "stephen");
        assert_eq!(stored.object_stores.len(), 1);

        cleanup(&root);
    }

    #[test]
    fn daemon_store_inventory_can_filter_by_easyconnect_session_write_grants() {
        let root = temp_root("remote-easyconnect-inventory");
        let (store_registry_path, subobject_registry_path) =
            write_test_store_registry(&root, "zymo_fecal_2025.05", Some("mnemosyne"));
        let session_store_path = remote_easyconnect_session_store_path(&root);
        let session_store = FileBackedRemoteEasyconnectPairedSessionStore::new(&session_store_path);
        session_store
            .upsert(paired_session("session-1"))
            .expect("session stored");
        let handler = DaemonRequestHandler::new(
            FakeService::default(),
            FixedDaemonClock::new("2026-07-09T16:20:00Z"),
        )
        .with_registry_paths(&store_registry_path, &subobject_registry_path)
        .with_remote_easyconnect_session_store_path(&session_store_path);

        let response = handler
            .handle(DaemonApiRequest::StoreInventory(StoreInventoryRequest {
                include_policy: true,
                remote_easyconnect_session_id: Some("session-1".to_string()),
                remote_upload_writable_only: true,
            }))
            .expect("request handled");

        let DaemonApiResponse::StoreInventory(response) = response else {
            panic!("expected store inventory response");
        };
        assert_eq!(response.stores.len(), 1);
        assert_eq!(response.stores[0].store_id.as_str(), "zymo_fecal_2025.05");
        assert!(response.stores[0].writable);

        cleanup(&root);
    }

    #[test]
    fn daemon_store_inventory_denies_remote_upload_listing_for_non_writer_session() {
        let root = temp_root("remote-easyconnect-inventory-non-writer");
        let (store_registry_path, subobject_registry_path) =
            write_test_store_registry(&root, "zymo_fecal_2025.05", Some("mnemosyne"));
        let session_store_path = remote_easyconnect_session_store_path(&root);
        let session_store = FileBackedRemoteEasyconnectPairedSessionStore::new(&session_store_path);
        let mut session = paired_session("session-1");
        session.object_stores[0].can_write = false;
        session_store.upsert(session).expect("session stored");
        let handler = DaemonRequestHandler::new(
            FakeService::default(),
            FixedDaemonClock::new("2026-07-09T16:20:00Z"),
        )
        .with_registry_paths(&store_registry_path, &subobject_registry_path)
        .with_remote_easyconnect_session_store_path(&session_store_path);

        let response = handler
            .handle(DaemonApiRequest::StoreInventory(StoreInventoryRequest {
                include_policy: true,
                remote_easyconnect_session_id: Some("session-1".to_string()),
                remote_upload_writable_only: true,
            }))
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::Error(error) if error.code == "store_inventory_failed"
                && error.message.contains("does not allow writing ObjectStore zymo_fecal_2025.05")
        ));

        cleanup(&root);
    }

    #[test]
    fn daemon_store_inventory_denies_remote_upload_listing_for_read_only_store() {
        let root = temp_root("remote-easyconnect-inventory-read-only");
        let (store_registry_path, subobject_registry_path) =
            write_test_store_registry_with_export_policy(
                &root,
                "zymo_fecal_2025.05",
                Some("mnemosyne"),
                ExportPolicy::ReadOnlyFileExport,
            );
        let session_store_path = remote_easyconnect_session_store_path(&root);
        let session_store = FileBackedRemoteEasyconnectPairedSessionStore::new(&session_store_path);
        session_store
            .upsert(paired_session("session-1"))
            .expect("session stored");
        let handler = DaemonRequestHandler::new(
            FakeService::default(),
            FixedDaemonClock::new("2026-07-09T16:20:00Z"),
        )
        .with_registry_paths(&store_registry_path, &subobject_registry_path)
        .with_remote_easyconnect_session_store_path(&session_store_path);

        let response = handler
            .handle(DaemonApiRequest::StoreInventory(StoreInventoryRequest {
                include_policy: true,
                remote_easyconnect_session_id: Some("session-1".to_string()),
                remote_upload_writable_only: true,
            }))
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::Error(error) if error.code == "store_inventory_failed"
                && error.message.contains("export policy ReadOnlyFileExport is not S3")
        ));

        cleanup(&root);
    }

    #[test]
    fn daemon_store_inventory_reports_missing_writer_group_for_remote_upload_listing() {
        let root = temp_root("remote-easyconnect-inventory-missing-writer-group");
        let (store_registry_path, subobject_registry_path) =
            write_test_store_registry(&root, "zymo_fecal_2025.05", None);
        let session_store_path = remote_easyconnect_session_store_path(&root);
        let session_store = FileBackedRemoteEasyconnectPairedSessionStore::new(&session_store_path);
        session_store
            .upsert(paired_session("session-1"))
            .expect("session stored");
        let handler = DaemonRequestHandler::new(
            FakeService::default(),
            FixedDaemonClock::new("2026-07-09T16:20:00Z"),
        )
        .with_registry_paths(&store_registry_path, &subobject_registry_path)
        .with_remote_easyconnect_session_store_path(&session_store_path);

        let response = handler
            .handle(DaemonApiRequest::StoreInventory(StoreInventoryRequest {
                include_policy: true,
                remote_easyconnect_session_id: Some("session-1".to_string()),
                remote_upload_writable_only: true,
            }))
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::Error(error) if error.code == "store_inventory_failed"
                && error.message.contains("has no writer group")
        ));

        cleanup(&root);
    }

    #[test]
    fn reports_unwired_commands_as_api_errors() {
        let service = FakeService::default();
        let handler = DaemonRequestHandler::new(service, FixedDaemonClock::new("now"));

        let response = handler
            .handle(DaemonApiRequest::RemoteEasyconnectDiscovery(
                Default::default(),
            ))
            .expect("request handled");

        assert!(matches!(
            response,
            DaemonApiResponse::Error(error) if error.code == "not_implemented"
                && error.message.contains("remote_easyconnect_discovery")
        ));
    }

    #[test]
    fn system_clock_returns_nonblank_timestamp() {
        assert!(!SystemDaemonClock.now_utc().trim().is_empty());
    }

    fn prepare_enclosure_request() -> PrepareEnclosureRequest {
        PrepareEnclosureRequest {
            ssd_device: "/dev/disk/by-id/nvme-ssd".into(),
            hdd_devices: vec![PrepareEnclosureHddDevice {
                disk_id: "qnap-1057".to_string(),
                device_path: "/dev/disk/by-id/usb-qnap-1057".into(),
            }],
            mount_root: "/srv/dasobjectstore".into(),
            filesystem: PrepareEnclosureFilesystem::Ext4,
            owner: Some("stephen".to_string()),
            dry_run: true,
            client_request_id: Some("request-prepare-1".to_string()),
            administrator_actor: Some("operator".to_string()),
            allow_format: true,
            existing_data_acknowledged: true,
            confirmation_marker: ENCLOSURE_PREPARE_CONFIRMATION.to_string(),
        }
    }

    fn create_object_store_request() -> CreateObjectStoreRequest {
        CreateObjectStoreRequest {
            store_id: "generated-data".to_string(),
            store_class: "generated_data".to_string(),
            required_copies: 2,
            bucket: Some("generated-data".to_string()),
            reader_group: None,
            writer_group: "bioinformatics".to_string(),
            ssd_root: "/srv/dasobjectstore/ssd".into(),
            object_type: "pod5".to_string(),
            enclosure_id: Some("qnap-tl-d800c-01".to_string()),
            public: false,
            writeable: true,
            capacity_behavior: "balanced".to_string(),
            retention: "standard".to_string(),
            endpoint_export_mode: "s3_bucket".to_string(),
            dry_run: true,
            client_request_id: Some("request-store-1".to_string()),
            administrator_actor: Some("operator".to_string()),
            confirmation_marker: OBJECT_STORE_CREATE_CONFIRMATION.to_string(),
        }
    }

    fn endpoint_inventory_request() -> UpsertEndpointInventoryRequest {
        UpsertEndpointInventoryRequest {
            endpoint_id: "nas-staging".to_string(),
            display_name: "NAS staging".to_string(),
            kind: DaemonEndpointKind::DasobjectstoreNfs,
            object_service_url: "https://nas.example.test:9443".to_string(),
            validation: DaemonEndpointValidation {
                state: DaemonEndpointValidationState::Validated,
                checked_at_utc: Some("2026-07-09T00:00:00Z".to_string()),
                message: Some("validated from Web admin workflow".to_string()),
            },
            manager_product_id: "dasobjectstore".to_string(),
            active_bindings: Vec::new(),
            dry_run: true,
            client_request_id: Some("endpoint-upsert-1".to_string()),
            administrator_actor: Some("operator".to_string()),
            confirmation_marker: Some(ENDPOINT_RECORD_CONFIRMATION.to_string()),
        }
    }

    fn paired_session(session_id: &str) -> RemoteEasyconnectPairedSessionRecord {
        RemoteEasyconnectPairedSessionRecord {
            session_id: session_id.to_string(),
            approved_actor: "stephen".to_string(),
            auth_provider: RemoteEasyconnectAuthProvider::StandaloneLocalUser,
            issued_at_utc: "2026-07-09T16:10:00Z".to_string(),
            expires_at_utc: "2026-07-10T00:10:00Z".to_string(),
            renew_after_utc: "2026-07-09T23:10:00Z".to_string(),
            renewal_token: "renewal-token-1".to_string(),
            credentials: RemoteEasyconnectSessionCredentials {
                access_key_id: "AKIAEXAMPLE".to_string(),
                secret_access_key: "secret".to_string(),
                session_token: Some("session-token".to_string()),
            },
            object_stores: vec![RemoteEasyconnectObjectStoreGrant {
                object_store: "zymo_fecal_2025.05".to_string(),
                bucket: "dos-zymo-fecal-2025-05".to_string(),
                can_read: true,
                can_write: true,
                writer_group: Some("mnemosyne".to_string()),
                object_type: "fastq".to_string(),
            }],
            revoked_at_utc: None,
        }
    }

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "dasobjectstore-request-handler-{label}-{}",
            std::process::id()
        ))
    }

    fn appliance_telemetry_fixture_json() -> &'static str {
        r#"{
          "schema_version": "dasobjectstore.appliance_telemetry.v1",
          "generated_at_utc": "2026-07-09T18:30:00Z",
          "cadence_seconds": 30.0,
          "source": {
            "appliance_id": "fixture-appliance",
            "host_id": "fixture-host",
            "hostname": "fixture-hostname"
          },
          "samples": [
            {
              "timestamp_utc": "2026-07-09T18:30:00Z",
              "collection_quality": "complete",
              "missing_data": [],
              "cpu": {
                "usage_percent": 42.5,
                "load_average_1m": 0.1,
                "load_average_5m": 0.2,
                "load_average_15m": 0.3,
                "logical_core_count": 2,
                "missing_reason": null
              },
              "memory": {
                "total_bytes": 100,
                "available_bytes": 75,
                "used_percent": 25.0,
                "swap_total_bytes": 0,
                "swap_used_bytes": 0,
                "missing_reason": null
              },
              "enclosures": [],
              "disks": [],
              "disk_io": [],
              "sessions": {
                "web_active_sessions": 2,
                "remote_agent_active_sessions": 1,
                "distinct_logged_in_users": 2,
                "administrator_sessions": 1,
                "operator_sessions": 1,
                "missing_reason": null
              }
            }
          ]
        }"#
    }

    fn cleanup(root: &PathBuf) {
        let _ = fs::remove_dir_all(root);
    }

    fn write_test_store_registry(
        root: &PathBuf,
        store_id: &str,
        writer_group: Option<&str>,
    ) -> (PathBuf, PathBuf) {
        write_test_store_registry_with_read_policy(root, store_id, None, writer_group, false)
    }

    fn write_test_store_registry_with_export_policy(
        root: &PathBuf,
        store_id: &str,
        writer_group: Option<&str>,
        export_policy: ExportPolicy,
    ) -> (PathBuf, PathBuf) {
        fs::create_dir_all(root).expect("temp registry dir");
        let store_registry = root.join("stores.json");
        let subobject_registry = root.join("subobjects.json");
        let mut policy = StorePolicy::defaults_for(StoreClass::ReproducibleCache);
        policy.export_policy = export_policy;
        let definitions = vec![StoreServiceDefinition {
            store_id: StoreId::new(store_id).expect("store id"),
            policy,
            bucket_name: None,
            reader_group: None,
            writer_group: writer_group.map(ToString::to_string),
            public: false,
        }];
        fs::write(
            &store_registry,
            serde_json::to_string_pretty(&definitions).expect("registry JSON"),
        )
        .expect("store registry written");
        fs::write(&subobject_registry, "[]").expect("subobject registry written");
        (store_registry, subobject_registry)
    }

    fn write_test_store_registry_with_read_policy(
        root: &PathBuf,
        store_id: &str,
        reader_group: Option<&str>,
        writer_group: Option<&str>,
        public: bool,
    ) -> (PathBuf, PathBuf) {
        fs::create_dir_all(root).expect("temp registry dir");
        let store_registry = root.join("stores.json");
        let subobject_registry = root.join("subobjects.json");
        let definitions = vec![StoreServiceDefinition {
            store_id: StoreId::new(store_id).expect("store id"),
            policy: StorePolicy::defaults_for(StoreClass::ReproducibleCache),
            bucket_name: None,
            reader_group: reader_group.map(ToString::to_string),
            writer_group: writer_group.map(ToString::to_string),
            public,
        }];
        fs::write(
            &store_registry,
            serde_json::to_string_pretty(&definitions).expect("registry JSON"),
        )
        .expect("store registry written");
        fs::write(&subobject_registry, "[]").expect("subobject registry written");
        (store_registry, subobject_registry)
    }

    fn object_browser_request(store_id: &str) -> ObjectBrowserRequest {
        ObjectBrowserRequest {
            endpoint: StoreId::new(store_id).expect("store id"),
            prefix: None,
            search: None,
            sort: ObjectBrowserSort::NameAsc,
            page: ObjectBrowserPageRequest::default(),
            include_placement: false,
            delegated_actor: None,
        }
    }

    fn create_live_sqlite(root: &PathBuf, store_id: &str) -> PathBuf {
        fs::create_dir_all(root).expect("temp metadata dir");
        let live_sqlite_path = root.join("live.sqlite");
        let connection = Connection::open(&live_sqlite_path).expect("open live sqlite");
        connection
            .execute_batch(LIVE_SCHEMA_SQL)
            .expect("schema applies");
        connection
            .execute(
                "INSERT INTO pools (pool_id, state, created_at_utc, updated_at_utc)
                 VALUES (?1, 'Clean', '2026-07-09T00:00:00Z', '2026-07-09T00:00:00Z')",
                [PoolId::new("pool-a").expect("pool id").as_str()],
            )
            .expect("pool inserts");
        connection
            .execute(
                "INSERT INTO disks (
                    disk_id, pool_id, role, state, size_bytes, serial_hint, model_hint,
                    enclosure_topology_path, created_at_utc, updated_at_utc
                 )
                 VALUES (
                    'qnap-1057', 'pool-a', 'hdd', 'Healthy', 4000000000000,
                    NULL, NULL, NULL, '2026-07-09T00:00:00Z', '2026-07-09T00:00:00Z'
                 )",
                [],
            )
            .expect("disk inserts");
        connection
            .execute(
                "INSERT INTO stores (
                    store_id, pool_id, class, policy_json, created_at_utc, updated_at_utc
                 )
                 VALUES (?1, 'pool-a', 'reproducible_cache', '{}',
                    '2026-07-09T00:00:00Z', '2026-07-09T00:00:00Z')",
                [store_id],
            )
            .expect("store inserts");
        live_sqlite_path
    }

    fn insert_browser_object(
        live_sqlite_path: &PathBuf,
        object_id: &str,
        state: &str,
        verified_placement: bool,
    ) {
        let connection = Connection::open(live_sqlite_path).expect("open live sqlite");
        connection
            .execute(
                "INSERT INTO objects (
                    object_id, store_id, object_type, state, size_bytes, content_hash,
                    created_at_utc, updated_at_utc
                 )
                 VALUES (?1, 'ena', 'fastq', ?2, 128, 'hash-a',
                    '2026-07-09T00:00:00Z', '2026-07-09T00:01:00Z')",
                params![object_id, state],
            )
            .expect("object inserts");
        connection
            .execute(
                "INSERT INTO placements (
                    placement_id, object_id, disk_id, relative_path, content_hash,
                    verified_at_utc, created_at_utc
                 )
                 VALUES (?1, ?2, 'qnap-1057', ?3, 'hash-a', ?4,
                    '2026-07-09T00:02:00Z')",
                params![
                    format!("placement-{object_id}"),
                    object_id,
                    object_id,
                    verified_placement.then_some("2026-07-09T00:03:00Z")
                ],
            )
            .expect("placement inserts");
    }

    fn write_hdd_marker(root: &Path, disk_id: &str) {
        let marker_dir = root.join(".dasobjectstore");
        fs::create_dir_all(&marker_dir).expect("marker dir");
        fs::write(
            marker_dir.join("device.env"),
            format!("role=hdd:{disk_id}\n"),
        )
        .expect("marker");
    }

    #[derive(Default)]
    struct FakeService {
        status_calls: RefCell<Vec<bool>>,
        lifecycle_calls: RefCell<Vec<String>>,
        provision_calls: RefCell<Vec<String>>,
        local_group_calls: RefCell<
            Vec<(
                String,
                LocalGroupAdministrationOperation,
                String,
                Option<String>,
                bool,
            )>,
        >,
        ingest_calls: RefCell<Vec<(String, String, bool)>>,
        prepare_enclosure_calls: RefCell<Vec<(String, String, bool)>>,
        create_object_store_calls: RefCell<Vec<(String, String, bool)>>,
        endpoint_inventory_calls: RefCell<Vec<(String, String, bool)>>,
        remote_upload_calls: RefCell<Vec<(String, String, u64, String, usize)>>,
        job_status_calls: RefCell<Vec<String>>,
        cancel_job_calls: RefCell<Vec<(String, String)>>,
        ingest_error: Option<String>,
    }

    impl DaemonServiceOrchestrator for FakeService {
        fn status(
            &self,
            request: DaemonServiceStatusRequest,
        ) -> Result<DaemonServiceStatusResponse, DaemonServiceRuntimeError> {
            self.status_calls.borrow_mut().push(request.include_detail);
            Ok(DaemonServiceStatusResponse {
                provider_id: ObjectServiceProviderId::Garage,
                state: ServiceState::Running,
                endpoint: Some("http://127.0.0.1:3900".to_string()),
                message: None,
                detail: None,
            })
        }

        fn lifecycle(
            &self,
            request: DaemonServiceLifecycleRequest,
            accepted_at_utc: &str,
        ) -> Result<DaemonServiceLifecycleResponse, DaemonServiceRuntimeError> {
            self.lifecycle_calls
                .borrow_mut()
                .push(accepted_at_utc.to_string());
            Ok(DaemonServiceLifecycleResponse::accepted(
                crate::api::DaemonJobId::new("service-start-2026-07-07t11-47-42z").expect("job id"),
                accepted_at_utc,
                request.dry_run,
                request.operation,
                ObjectServiceProviderId::Garage,
            ))
        }

        fn provision(
            &self,
            request: DaemonServiceProvisionRequest,
            accepted_at_utc: &str,
        ) -> Result<DaemonServiceProvisionResponse, DaemonServiceRuntimeError> {
            self.provision_calls
                .borrow_mut()
                .push(accepted_at_utc.to_string());
            Ok(DaemonServiceProvisionResponse::accepted(
                crate::api::DaemonJobId::new("service-provision-2026-07-07t12-05-42z")
                    .expect("job id"),
                accepted_at_utc,
                request.dry_run,
                ObjectServiceProviderId::Garage,
                "/etc/dasobjectstore/stores.json",
                "/var/lib/dasobjectstore/object-service/garage-credentials.json",
                1,
                1,
                3,
                0,
                1,
                0,
            ))
        }

        fn remote_easyconnect_aws_cli_upload_job(
            &self,
            registry: &dyn AdminJobRegistry,
            _gate: Arc<RemoteUploadAdmissionGate>,
            request: RemoteEasyconnectAwsCliUploadJobRequest,
        ) -> Result<crate::runtime::RemoteUploadS3TransferWorkerReport, DaemonServiceRuntimeError>
        {
            self.remote_upload_calls.borrow_mut().push((
                request.job_id.clone(),
                request.object_store.clone(),
                request.source_bytes,
                request.program.clone(),
                request.environment.len(),
            ));
            let job = DaemonJobSummary {
                job_id: DaemonJobId::new(request.job_id.clone())
                    .map_err(|_| DaemonServiceRuntimeError::InvalidJobId(request.job_id.clone()))?,
                kind: DaemonJobKind::RemoteUpload,
                state: DaemonJobState::Complete,
                progress: DaemonJobProgress {
                    stage: "remote_s3_transfer_complete".to_string(),
                    work_bytes_done: request.source_bytes,
                    work_bytes_total: request.source_bytes,
                    work_units_done: 1,
                    work_units_total: 1,
                    message: request.progress_message,
                },
                submitted_at_utc: request.submitted_at_utc,
                updated_at_utc: request.finished_at_utc,
                actor: request.actor,
                failure_message: None,
            };
            registry.record(job.clone())?;
            Ok(crate::runtime::RemoteUploadS3TransferWorkerReport {
                running_event: None,
                progress_events: Vec::new(),
                final_event: crate::api::DaemonJobEvent::Complete(job),
                runtime_after: Default::default(),
                cleanup_report: None,
            })
        }

        fn create_local_group(
            &self,
            request: CreateLocalGroupRequest,
            accepted_at_utc: &str,
        ) -> Result<CreateLocalGroupResponse, LocalAdminRuntimeError> {
            self.local_group_calls.borrow_mut().push((
                accepted_at_utc.to_string(),
                LocalGroupAdministrationOperation::CreateGroup,
                request.group_name.clone(),
                None,
                request.dry_run,
            ));
            Ok(CreateLocalGroupResponse::accepted(
                DaemonJobId::new("local-group-create-group-2026-07-07t12-25-00z").expect("job id"),
                accepted_at_utc,
                request.dry_run,
                request.client_request_id,
                request.group_name,
                request.administrator_actor,
            ))
        }

        fn assign_local_user_to_local_group(
            &self,
            request: AssignLocalUserToLocalGroupRequest,
            accepted_at_utc: &str,
        ) -> Result<AssignLocalUserToLocalGroupResponse, LocalAdminRuntimeError> {
            self.local_group_calls.borrow_mut().push((
                accepted_at_utc.to_string(),
                LocalGroupAdministrationOperation::AssignUserToGroup,
                request.group_name.clone(),
                Some(request.username.clone()),
                request.dry_run,
            ));
            Ok(AssignLocalUserToLocalGroupResponse::accepted(
                DaemonJobId::new("local-group-assign-user-to-group-2026-07-07t12-28-00z")
                    .expect("job id"),
                accepted_at_utc,
                request.dry_run,
                request.client_request_id,
                request.username,
                request.group_name,
                request.administrator_actor,
            ))
        }

        fn submit_ingest_files(
            &self,
            request: SubmitIngestFilesRequest,
            accepted_at_utc: &str,
            emit_progress: &mut dyn FnMut(
                crate::api::DaemonIngestProgressEvent,
            ) -> Result<(), DaemonIngestFilesRuntimeError>,
        ) -> Result<SubmitIngestFilesResponse, DaemonIngestFilesRuntimeError> {
            if let Some(message) = &self.ingest_error {
                return Err(DaemonIngestFilesRuntimeError::CommandFailed(
                    message.clone(),
                ));
            }
            emit_progress(crate::api::DaemonIngestProgressEvent {
                job_id: IngestJobId::new("ingest-files-2026-07-07t12-35-00z").expect("job id"),
                endpoint: request.endpoint.clone(),
                stage: crate::api::DaemonIngestStage::Queued,
                pipeline_stage: Some(crate::api::DaemonIngestPipelineStage::Scan),
                work_bytes_done: 0,
                work_bytes_total: Some(0),
                source_bytes_done: Some(0),
                source_bytes_total: Some(0),
                stage_bytes_done: Some(0),
                stage_bytes_total: Some(0),
                files_done: 0,
                files_total: Some(0),
                current_object_id: None,
                ssd_pressure: None,
                telemetry: None,
                active_hdd_transfers: Vec::new(),
                resource_policy: None,
                message: Some("queued".to_string()),
            })?;
            self.ingest_calls.borrow_mut().push((
                accepted_at_utc.to_string(),
                request.endpoint.as_str().to_string(),
                request.dry_run,
            ));
            Ok(SubmitIngestFilesResponse {
                job_id: IngestJobId::new("ingest-files-2026-07-07t12-35-00z").expect("job id"),
                accepted_at_utc: accepted_at_utc.to_string(),
                dry_run: request.dry_run,
            })
        }

        fn prepare_enclosure(
            &self,
            request: PrepareEnclosureRequest,
            accepted_at_utc: &str,
        ) -> Result<PrepareEnclosureResponse, DaemonServiceRuntimeError> {
            self.prepare_enclosure_calls.borrow_mut().push((
                accepted_at_utc.to_string(),
                request.ssd_device.display().to_string(),
                request.dry_run,
            ));
            Ok(PrepareEnclosureResponse::accepted(
                DaemonJobId::new("enclosure-prepare-2026-07-08t19-40-00z").expect("job id"),
                accepted_at_utc,
                request.dry_run,
                request.ssd_device,
                request.hdd_devices,
                request.mount_root,
                request.filesystem,
                request.owner,
                request.administrator_actor,
            ))
        }

        fn create_object_store(
            &self,
            request: CreateObjectStoreRequest,
            accepted_at_utc: &str,
        ) -> Result<CreateObjectStoreResponse, DaemonServiceRuntimeError> {
            self.create_object_store_calls.borrow_mut().push((
                accepted_at_utc.to_string(),
                request.store_id.clone(),
                request.dry_run,
            ));
            Ok(CreateObjectStoreResponse::accepted(
                DaemonJobId::new("objectstore-create-2026-07-08t20-45-00z").expect("job id"),
                accepted_at_utc,
                request,
            ))
        }

        fn upsert_endpoint_inventory(
            &self,
            request: UpsertEndpointInventoryRequest,
            accepted_at_utc: &str,
        ) -> Result<UpsertEndpointInventoryResponse, DaemonServiceRuntimeError> {
            self.endpoint_inventory_calls.borrow_mut().push((
                accepted_at_utc.to_string(),
                request.endpoint_id.clone(),
                request.dry_run,
            ));
            Ok(UpsertEndpointInventoryResponse::accepted(
                DaemonJobId::new("endpoint-upsert-2026-07-09t00-05-00z").expect("job id"),
                accepted_at_utc,
                "/opt/dasobjectstore/endpoints.json",
                request,
            ))
        }

        fn job_status(
            &self,
            request: DaemonJobStatusRequest,
        ) -> Result<DaemonJobStatusResponse, DaemonServiceRuntimeError> {
            self.job_status_calls
                .borrow_mut()
                .push(request.job_id.to_string());
            Ok(DaemonJobStatusResponse {
                job: DaemonJobSummary {
                    job_id: request.job_id,
                    kind: DaemonJobKind::EnclosurePreparation,
                    state: DaemonJobState::Running,
                    progress: DaemonJobProgress {
                        stage: "formatting".to_string(),
                        work_bytes_done: 5,
                        work_bytes_total: 10,
                        work_units_done: 1,
                        work_units_total: 2,
                        message: Some("formatting selected devices".to_string()),
                    },
                    submitted_at_utc: "2026-07-08T20:05:00Z".to_string(),
                    updated_at_utc: "2026-07-08T20:05:10Z".to_string(),
                    actor: Some("operator".to_string()),
                    failure_message: None,
                },
            })
        }

        fn cancel_job(
            &self,
            request: DaemonJobCancelRequest,
            accepted_at_utc: &str,
        ) -> Result<DaemonJobCancelResponse, DaemonServiceRuntimeError> {
            self.cancel_job_calls
                .borrow_mut()
                .push((accepted_at_utc.to_string(), request.job_id.to_string()));
            Ok(DaemonJobCancelResponse {
                job_id: request.job_id,
                accepted: true,
                state: DaemonJobState::Cancelled,
            })
        }
    }
}
