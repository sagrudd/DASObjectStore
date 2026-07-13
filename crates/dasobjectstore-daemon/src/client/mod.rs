//! Client boundary for callers that submit requests to `dasobjectstored`.

mod error;
mod in_process;
mod unix_socket;

pub use error::DaemonClientError;
pub use in_process::InProcessDaemonTransport;
pub use unix_socket::UnixSocketDaemonTransport;

use crate::api::{
    ApplianceTelemetryRequest, ApplianceTelemetryResponse, ApplicationIdentityRegistrationRequest,
    ApplicationIdentityRegistrationResponse, AssignLocalUserToLocalGroupRequest,
    AssignLocalUserToLocalGroupResponse, CancelIngestJobRequest, CancelIngestJobResponse,
    CapacityAdmissionRequest, CapacityAdmissionResponse, CapacityStatusRequest,
    CapacityStatusResponse, CreateLocalGroupRequest, CreateLocalGroupResponse,
    CreateObjectStoreRequest, CreateObjectStoreResponse, DaemonApiRequest, DaemonApiResponse,
    DaemonHealthSummaryRequest, DaemonHealthSummaryResponse, DaemonIngestProgressEvent,
    DaemonJobCancelRequest, DaemonJobCancelResponse, DaemonJobListRequest, DaemonJobListResponse,
    DaemonJobStatusRequest, DaemonJobStatusResponse, DaemonServiceLifecycleRequest,
    DaemonServiceLifecycleResponse, DaemonServiceProvisionRequest, DaemonServiceProvisionResponse,
    DaemonServiceStatusRequest, DaemonServiceStatusResponse, DiskForceRetireRequest,
    DiskRetireRequest, DiskRetireResponse, IngestControlRequest, IngestControlResponse,
    IngestJobStatusRequest, IngestJobStatusResponse, IngestQueueDrainRequest,
    IngestQueueDrainResponse, ObjectBrowserRequest, ObjectBrowserResponse, ObjectDownloadRequest,
    ObjectDownloadResponse, ObjectFolderDownloadRequest, ObjectFolderDownloadResponse,
    ObjectPutRequest, ObjectPutResponse, ObjectStoreCapabilityDiscoveryRequest,
    ObjectStoreCapabilityDiscoveryResponse, PrepareEnclosureRequest, PrepareEnclosureResponse,
    ProfileBindingRequest, ProfileBindingResponse, ProfileBrowserRequest, ProfileBrowserResponse,
    ProfileInspectionRequest, ProfileInspectionResponse, RemoteEasyconnectApprovePairingRequest,
    RemoteEasyconnectApprovePairingResponse, RemoteEasyconnectCreatePairingRequest,
    RemoteEasyconnectCreatePairingResponse, RemoteEasyconnectDiscoveryRequest,
    RemoteEasyconnectDiscoveryResponse, RemoteEasyconnectExchangePairingRequest,
    RemoteEasyconnectExchangePairingResponse, RemoteEasyconnectRenewSessionRequest,
    RemoteEasyconnectRenewSessionResponse, RemoteEasyconnectRevokeSessionRequest,
    RemoteEasyconnectRevokeSessionResponse, RemoteEasyconnectSubmitAwsCliUploadRequest,
    RemoteEasyconnectSubmitAwsCliUploadResponse, RemoteEasyconnectUploadAdmissionDecision,
    RemoteEasyconnectUploadAdmissionRequest, StoreDeduplicateRequest, StoreDeduplicateResponse,
    StoreDeleteRequest, StoreDeleteResponse, StoreDrainRequest, StoreDrainResponse,
    StoreInventoryRequest, StoreInventoryResponse, StoreRepairRequest, StoreRepairResponse,
    StoreVerifyRequest, StoreVerifyResponse, SubmitIngestFilesRequest, SubmitIngestFilesResponse,
    UpdateObjectStoreIngestPolicyRequest, UpdateObjectStoreIngestPolicyResponse,
    UpsertEndpointInventoryRequest, UpsertEndpointInventoryResponse,
};

pub trait DaemonClientTransport {
    fn send(&self, request: DaemonApiRequest) -> Result<DaemonApiResponse, DaemonClientError>;

    fn send_with_progress(
        &self,
        request: DaemonApiRequest,
        _progress: &mut dyn FnMut(DaemonIngestProgressEvent) -> Result<(), DaemonClientError>,
    ) -> Result<DaemonApiResponse, DaemonClientError> {
        self.send(request)
    }

    fn send_with_progress_and_heartbeat(
        &self,
        request: DaemonApiRequest,
        progress: &mut dyn FnMut(DaemonIngestProgressEvent) -> Result<(), DaemonClientError>,
        _heartbeat: &mut dyn FnMut() -> Result<(), DaemonClientError>,
    ) -> Result<DaemonApiResponse, DaemonClientError> {
        self.send_with_progress(request, progress)
    }
}

pub struct DaemonClient<T> {
    transport: T,
}

impl<T> DaemonClient<T>
where
    T: DaemonClientTransport,
{
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    pub fn send(&self, request: DaemonApiRequest) -> Result<DaemonApiResponse, DaemonClientError> {
        request.validate()?;
        self.transport.send(request)
    }

    pub fn health_summary(
        &self,
        request: DaemonHealthSummaryRequest,
    ) -> Result<DaemonHealthSummaryResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::HealthSummary(request))? {
            DaemonApiResponse::HealthSummary(response) => Ok(response),
            response => Err(unexpected("health_summary", response)),
        }
    }

    pub fn store_inventory(
        &self,
        request: StoreInventoryRequest,
    ) -> Result<StoreInventoryResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::StoreInventory(request))? {
            DaemonApiResponse::StoreInventory(response) => Ok(response),
            response => Err(unexpected("store_inventory", response)),
        }
    }

    pub fn capacity_status(
        &self,
        request: CapacityStatusRequest,
    ) -> Result<CapacityStatusResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::CapacityStatus(request))? {
            DaemonApiResponse::CapacityStatus(response) => Ok(response),
            response => Err(unexpected("capacity_status", response)),
        }
    }

    pub fn disk_retire(
        &self,
        request: DiskRetireRequest,
    ) -> Result<DiskRetireResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::DiskRetire(request))? {
            DaemonApiResponse::DiskRetire(response) => Ok(response),
            response => Err(unexpected("disk_retire", response)),
        }
    }

    pub fn disk_force_retire(
        &self,
        request: DiskForceRetireRequest,
    ) -> Result<DiskRetireResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::DiskForceRetire(request))? {
            DaemonApiResponse::DiskForceRetire(response) => Ok(response),
            response => Err(unexpected("disk_force_retire", response)),
        }
    }

    pub fn store_drain(
        &self,
        request: StoreDrainRequest,
    ) -> Result<StoreDrainResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::StoreDrain(request))? {
            DaemonApiResponse::StoreDrain(response) => Ok(response),
            response => Err(unexpected("store_drain", response)),
        }
    }

    pub fn store_delete(
        &self,
        request: StoreDeleteRequest,
    ) -> Result<StoreDeleteResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::StoreDelete(request))? {
            DaemonApiResponse::StoreDelete(response) => Ok(response),
            response => Err(unexpected("store_delete", response)),
        }
    }

    pub fn store_repair(
        &self,
        request: StoreRepairRequest,
    ) -> Result<StoreRepairResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::StoreRepair(request))? {
            DaemonApiResponse::StoreRepair(response) => Ok(response),
            response => Err(unexpected("store_repair", response)),
        }
    }

    pub fn store_repair_with_progress(
        &self,
        request: StoreRepairRequest,
        mut progress: impl FnMut(DaemonIngestProgressEvent) -> Result<(), DaemonClientError>,
    ) -> Result<StoreRepairResponse, DaemonClientError> {
        match self
            .transport
            .send_with_progress(DaemonApiRequest::StoreRepair(request), &mut progress)?
        {
            DaemonApiResponse::StoreRepair(response) => Ok(response),
            response => Err(unexpected("store_repair", response)),
        }
    }

    pub fn store_verify(
        &self,
        request: StoreVerifyRequest,
    ) -> Result<StoreVerifyResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::StoreVerify(request))? {
            DaemonApiResponse::StoreVerify(response) => Ok(response),
            response => Err(unexpected("store_verify", response)),
        }
    }

    pub fn store_deduplicate(
        &self,
        request: StoreDeduplicateRequest,
    ) -> Result<StoreDeduplicateResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::StoreDeduplicate(request))? {
            DaemonApiResponse::StoreDeduplicate(response) => Ok(response),
            response => Err(unexpected("store_deduplicate", response)),
        }
    }

    pub fn object_put(
        &self,
        request: ObjectPutRequest,
    ) -> Result<ObjectPutResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::ObjectPut(request))? {
            DaemonApiResponse::ObjectPut(response) => Ok(response),
            response => Err(unexpected("object_put", response)),
        }
    }

    pub fn ingest_queue_drain(
        &self,
        request: IngestQueueDrainRequest,
    ) -> Result<IngestQueueDrainResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::IngestQueueDrain(request))? {
            DaemonApiResponse::IngestQueueDrain(response) => Ok(response),
            response => Err(unexpected("ingest_queue_drain", response)),
        }
    }

    pub fn ingest_control(
        &self,
        request: IngestControlRequest,
    ) -> Result<IngestControlResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::IngestControl(request))? {
            DaemonApiResponse::IngestControl(response) => Ok(response),
            response => Err(unexpected("ingest_control", response)),
        }
    }

    pub fn submit_ingest_files(
        &self,
        request: SubmitIngestFilesRequest,
    ) -> Result<SubmitIngestFilesResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::SubmitIngestFiles(request))? {
            DaemonApiResponse::SubmitIngestFiles(response) => Ok(response),
            response => Err(unexpected("submit_ingest_files", response)),
        }
    }

    pub fn submit_ingest_files_with_progress(
        &self,
        request: SubmitIngestFilesRequest,
        mut progress: impl FnMut(DaemonIngestProgressEvent),
    ) -> Result<SubmitIngestFilesResponse, DaemonClientError> {
        self.submit_ingest_files_with_progress_and_heartbeat(
            request,
            |event| {
                progress(event);
                Ok(())
            },
            || Ok(()),
        )
    }

    pub fn submit_ingest_files_with_progress_and_heartbeat(
        &self,
        request: SubmitIngestFilesRequest,
        mut progress: impl FnMut(DaemonIngestProgressEvent) -> Result<(), DaemonClientError>,
        mut heartbeat: impl FnMut() -> Result<(), DaemonClientError>,
    ) -> Result<SubmitIngestFilesResponse, DaemonClientError> {
        match self.transport.send_with_progress_and_heartbeat(
            DaemonApiRequest::SubmitIngestFiles(request),
            &mut progress,
            &mut heartbeat,
        )? {
            DaemonApiResponse::SubmitIngestFiles(response) => Ok(response),
            response => Err(unexpected("submit_ingest_files", response)),
        }
    }

    pub fn ingest_job_status(
        &self,
        request: IngestJobStatusRequest,
    ) -> Result<IngestJobStatusResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::IngestJobStatus(request))? {
            DaemonApiResponse::IngestJobStatus(response) => Ok(response),
            response => Err(unexpected("ingest_job_status", response)),
        }
    }

    pub fn cancel_ingest_job(
        &self,
        request: CancelIngestJobRequest,
    ) -> Result<CancelIngestJobResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::CancelIngestJob(request))? {
            DaemonApiResponse::CancelIngestJob(response) => Ok(response),
            response => Err(unexpected("cancel_ingest_job", response)),
        }
    }

    pub fn job_status(
        &self,
        request: DaemonJobStatusRequest,
    ) -> Result<DaemonJobStatusResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::JobStatus(request))? {
            DaemonApiResponse::JobStatus(response) => Ok(response),
            response => Err(unexpected("job_status", response)),
        }
    }

    pub fn list_jobs(
        &self,
        request: DaemonJobListRequest,
    ) -> Result<DaemonJobListResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::JobList(request))? {
            DaemonApiResponse::JobList(response) => Ok(response),
            response => Err(unexpected("job_list", response)),
        }
    }

    pub fn cancel_job(
        &self,
        request: DaemonJobCancelRequest,
    ) -> Result<DaemonJobCancelResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::CancelJob(request))? {
            DaemonApiResponse::CancelJob(response) => Ok(response),
            response => Err(unexpected("cancel_job", response)),
        }
    }

    pub fn service_status(
        &self,
        request: DaemonServiceStatusRequest,
    ) -> Result<DaemonServiceStatusResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::ServiceStatus(request))? {
            DaemonApiResponse::ServiceStatus(response) => Ok(response),
            response => Err(unexpected("service_status", response)),
        }
    }

    pub fn appliance_telemetry(
        &self,
        request: ApplianceTelemetryRequest,
    ) -> Result<ApplianceTelemetryResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::ApplianceTelemetry(request))? {
            DaemonApiResponse::ApplianceTelemetry(response) => Ok(response),
            response => Err(unexpected("appliance_telemetry", response)),
        }
    }

    pub fn service_lifecycle(
        &self,
        request: DaemonServiceLifecycleRequest,
    ) -> Result<DaemonServiceLifecycleResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::ServiceLifecycle(request))? {
            DaemonApiResponse::ServiceLifecycle(response) => Ok(response),
            response => Err(unexpected("service_lifecycle", response)),
        }
    }

    pub fn service_provision(
        &self,
        request: DaemonServiceProvisionRequest,
    ) -> Result<DaemonServiceProvisionResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::ServiceProvision(request))? {
            DaemonApiResponse::ServiceProvision(response) => Ok(response),
            response => Err(unexpected("service_provision", response)),
        }
    }

    pub fn register_application_identity(
        &self,
        request: ApplicationIdentityRegistrationRequest,
    ) -> Result<ApplicationIdentityRegistrationResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::RegisterApplicationIdentity(request))? {
            DaemonApiResponse::RegisterApplicationIdentity(response) => Ok(response),
            response => Err(unexpected("register_application_identity", response)),
        }
    }

    pub fn prepare_enclosure(
        &self,
        request: PrepareEnclosureRequest,
    ) -> Result<PrepareEnclosureResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::PrepareEnclosure(request))? {
            DaemonApiResponse::PrepareEnclosure(response) => Ok(response),
            response => Err(unexpected("prepare_enclosure", response)),
        }
    }

    pub fn create_object_store(
        &self,
        request: CreateObjectStoreRequest,
    ) -> Result<CreateObjectStoreResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::CreateObjectStore(request))? {
            DaemonApiResponse::CreateObjectStore(response) => Ok(response),
            response => Err(unexpected("create_object_store", response)),
        }
    }

    pub fn register_profile_binding(
        &self,
        request: ProfileBindingRequest,
    ) -> Result<ProfileBindingResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::RegisterProfileBinding(request))? {
            DaemonApiResponse::RegisterProfileBinding(response) => Ok(response),
            response => Err(unexpected("register_profile_binding", response)),
        }
    }

    pub fn profile_inspection(
        &self,
        request: ProfileInspectionRequest,
    ) -> Result<ProfileInspectionResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::ProfileInspection(request))? {
            DaemonApiResponse::ProfileInspection(response) => Ok(response),
            response => Err(unexpected("profile_inspection", response)),
        }
    }

    pub fn profile_browser(
        &self,
        request: ProfileBrowserRequest,
    ) -> Result<ProfileBrowserResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::ProfileBrowser(request))? {
            DaemonApiResponse::ProfileBrowser(response) => Ok(response),
            response => Err(unexpected("profile_browser", response)),
        }
    }

    pub fn profile_capabilities(
        &self,
        request: ObjectStoreCapabilityDiscoveryRequest,
    ) -> Result<ObjectStoreCapabilityDiscoveryResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::ProfileCapabilities(request))? {
            DaemonApiResponse::ProfileCapabilities(response) => Ok(response),
            response => Err(unexpected("profile_capabilities", response)),
        }
    }

    pub fn capacity_admission(
        &self,
        request: CapacityAdmissionRequest,
    ) -> Result<CapacityAdmissionResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::CapacityAdmission(request))? {
            DaemonApiResponse::CapacityAdmission(response) => Ok(response),
            response => Err(unexpected("capacity_admission", response)),
        }
    }

    pub fn update_object_store_ingest_policy(
        &self,
        request: UpdateObjectStoreIngestPolicyRequest,
    ) -> Result<UpdateObjectStoreIngestPolicyResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::UpdateObjectStoreIngestPolicy(request))? {
            DaemonApiResponse::UpdateObjectStoreIngestPolicy(response) => Ok(response),
            response => Err(unexpected("update_object_store_ingest_policy", response)),
        }
    }

    pub fn object_browser(
        &self,
        request: ObjectBrowserRequest,
    ) -> Result<ObjectBrowserResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::ObjectBrowser(request))? {
            DaemonApiResponse::ObjectBrowser(response) => Ok(response),
            response => Err(unexpected("object_browser", response)),
        }
    }

    pub fn object_download(
        &self,
        request: ObjectDownloadRequest,
    ) -> Result<ObjectDownloadResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::ObjectDownload(request))? {
            DaemonApiResponse::ObjectDownload(response) => Ok(response),
            response => Err(unexpected("object_download", response)),
        }
    }

    pub fn object_folder_download(
        &self,
        request: ObjectFolderDownloadRequest,
    ) -> Result<ObjectFolderDownloadResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::ObjectFolderDownload(request))? {
            DaemonApiResponse::ObjectFolderDownload(response) => Ok(response),
            response => Err(unexpected("object_folder_download", response)),
        }
    }

    pub fn upsert_endpoint_inventory(
        &self,
        request: UpsertEndpointInventoryRequest,
    ) -> Result<UpsertEndpointInventoryResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::UpsertEndpointInventory(request))? {
            DaemonApiResponse::UpsertEndpointInventory(response) => Ok(response),
            response => Err(unexpected("upsert_endpoint_inventory", response)),
        }
    }

    pub fn create_local_group(
        &self,
        request: CreateLocalGroupRequest,
    ) -> Result<CreateLocalGroupResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::CreateLocalGroup(request))? {
            DaemonApiResponse::CreateLocalGroup(response) => Ok(response),
            response => Err(unexpected("create_local_group", response)),
        }
    }

    pub fn assign_local_user_to_local_group(
        &self,
        request: AssignLocalUserToLocalGroupRequest,
    ) -> Result<AssignLocalUserToLocalGroupResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::AssignLocalUserToLocalGroup(request))? {
            DaemonApiResponse::AssignLocalUserToLocalGroup(response) => Ok(response),
            response => Err(unexpected("assign_local_user_to_local_group", response)),
        }
    }

    pub fn remote_easyconnect_discovery(
        &self,
        request: RemoteEasyconnectDiscoveryRequest,
    ) -> Result<RemoteEasyconnectDiscoveryResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::RemoteEasyconnectDiscovery(request))? {
            DaemonApiResponse::RemoteEasyconnectDiscovery(response) => Ok(response),
            response => Err(unexpected("remote_easyconnect_discovery", response)),
        }
    }

    pub fn remote_easyconnect_create_pairing(
        &self,
        request: RemoteEasyconnectCreatePairingRequest,
    ) -> Result<RemoteEasyconnectCreatePairingResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::RemoteEasyconnectCreatePairing(request))? {
            DaemonApiResponse::RemoteEasyconnectCreatePairing(response) => Ok(response),
            response => Err(unexpected("remote_easyconnect_create_pairing", response)),
        }
    }

    pub fn remote_easyconnect_approve_pairing(
        &self,
        request: RemoteEasyconnectApprovePairingRequest,
    ) -> Result<RemoteEasyconnectApprovePairingResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::RemoteEasyconnectApprovePairing(request))? {
            DaemonApiResponse::RemoteEasyconnectApprovePairing(response) => Ok(response),
            response => Err(unexpected("remote_easyconnect_approve_pairing", response)),
        }
    }

    pub fn remote_easyconnect_exchange_pairing(
        &self,
        request: RemoteEasyconnectExchangePairingRequest,
    ) -> Result<RemoteEasyconnectExchangePairingResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::RemoteEasyconnectExchangePairing(request))? {
            DaemonApiResponse::RemoteEasyconnectExchangePairing(response) => Ok(response),
            response => Err(unexpected("remote_easyconnect_exchange_pairing", response)),
        }
    }

    pub fn remote_easyconnect_revoke_session(
        &self,
        request: RemoteEasyconnectRevokeSessionRequest,
    ) -> Result<RemoteEasyconnectRevokeSessionResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::RemoteEasyconnectRevokeSession(request))? {
            DaemonApiResponse::RemoteEasyconnectRevokeSession(response) => Ok(response),
            response => Err(unexpected("remote_easyconnect_revoke_session", response)),
        }
    }

    pub fn remote_easyconnect_renew_session(
        &self,
        request: RemoteEasyconnectRenewSessionRequest,
    ) -> Result<RemoteEasyconnectRenewSessionResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::RemoteEasyconnectRenewSession(request))? {
            DaemonApiResponse::RemoteEasyconnectRenewSession(response) => Ok(response),
            response => Err(unexpected("remote_easyconnect_renew_session", response)),
        }
    }

    pub fn remote_easyconnect_upload_admission(
        &self,
        request: RemoteEasyconnectUploadAdmissionRequest,
    ) -> Result<RemoteEasyconnectUploadAdmissionDecision, DaemonClientError> {
        match self.send(DaemonApiRequest::RemoteEasyconnectUploadAdmission(request))? {
            DaemonApiResponse::RemoteEasyconnectUploadAdmission(response) => Ok(response),
            response => Err(unexpected("remote_easyconnect_upload_admission", response)),
        }
    }

    pub fn remote_easyconnect_submit_aws_cli_upload(
        &self,
        request: RemoteEasyconnectSubmitAwsCliUploadRequest,
    ) -> Result<RemoteEasyconnectSubmitAwsCliUploadResponse, DaemonClientError> {
        match self.send(DaemonApiRequest::RemoteEasyconnectSubmitAwsCliUpload(
            request,
        ))? {
            DaemonApiResponse::RemoteEasyconnectSubmitAwsCliUpload(response) => Ok(response),
            response => Err(unexpected(
                "remote_easyconnect_submit_aws_cli_upload",
                response,
            )),
        }
    }
}

fn unexpected(expected: &'static str, response: DaemonApiResponse) -> DaemonClientError {
    if let DaemonApiResponse::Error(error) = response {
        return DaemonClientError::Api(error);
    }

    DaemonClientError::UnexpectedResponse {
        expected,
        actual: response_name(&response),
    }
}

fn response_name(response: &DaemonApiResponse) -> &'static str {
    match response {
        DaemonApiResponse::HealthSummary(_) => "health_summary",
        DaemonApiResponse::DiskRetire(_) => "disk_retire",
        DaemonApiResponse::DiskForceRetire(_) => "disk_force_retire",
        DaemonApiResponse::StoreInventory(_) => "store_inventory",
        DaemonApiResponse::StoreDrain(_) => "store_drain",
        DaemonApiResponse::StoreDelete(_) => "store_delete",
        DaemonApiResponse::StoreVerify(_) => "store_verify",
        DaemonApiResponse::StoreDeduplicate(_) => "store_deduplicate",
        DaemonApiResponse::StoreRepair(_) => "store_repair",
        DaemonApiResponse::ObjectPut(_) => "object_put",
        DaemonApiResponse::IngestQueueDrain(_) => "ingest_queue_drain",
        DaemonApiResponse::IngestControl(_) => "ingest_control",
        DaemonApiResponse::SubmitIngestFiles(_) => "submit_ingest_files",
        DaemonApiResponse::IngestJobStatus(_) => "ingest_job_status",
        DaemonApiResponse::CancelIngestJob(_) => "cancel_ingest_job",
        DaemonApiResponse::JobList(_) => "job_list",
        DaemonApiResponse::JobStatus(_) => "job_status",
        DaemonApiResponse::CancelJob(_) => "cancel_job",
        DaemonApiResponse::ServiceStatus(_) => "service_status",
        DaemonApiResponse::ApplianceTelemetry(_) => "appliance_telemetry",
        DaemonApiResponse::ServiceLifecycle(_) => "service_lifecycle",
        DaemonApiResponse::ServiceProvision(_) => "service_provision",
        DaemonApiResponse::RegisterApplicationIdentity(_) => "register_application_identity",
        DaemonApiResponse::PrepareEnclosure(_) => "prepare_enclosure",
        DaemonApiResponse::CreateObjectStore(_) => "create_object_store",
        DaemonApiResponse::RegisterProfileBinding(_) => "register_profile_binding",
        DaemonApiResponse::ProfileBrowser(_) => "profile_browser",
        DaemonApiResponse::ProfileInspection(_) => "profile_inspection",
        DaemonApiResponse::ProfileCapabilities(_) => "profile_capabilities",
        DaemonApiResponse::CapacityAdmission(_) => "capacity_admission",
        DaemonApiResponse::CapacityStatus(_) => "capacity_status",
        DaemonApiResponse::UpdateObjectStoreIngestPolicy(_) => "update_object_store_ingest_policy",
        DaemonApiResponse::ObjectBrowser(_) => "object_browser",
        DaemonApiResponse::ObjectDownload(_) => "object_download",
        DaemonApiResponse::ObjectFolderDownload(_) => "object_folder_download",
        DaemonApiResponse::UpsertEndpointInventory(_) => "upsert_endpoint_inventory",
        DaemonApiResponse::CreateLocalGroup(_) => "create_local_group",
        DaemonApiResponse::AssignLocalUserToLocalGroup(_) => "assign_local_user_to_local_group",
        DaemonApiResponse::RemoteEasyconnectDiscovery(_) => "remote_easyconnect_discovery",
        DaemonApiResponse::RemoteEasyconnectCreatePairing(_) => "remote_easyconnect_create_pairing",
        DaemonApiResponse::RemoteEasyconnectApprovePairing(_) => {
            "remote_easyconnect_approve_pairing"
        }
        DaemonApiResponse::RemoteEasyconnectExchangePairing(_) => {
            "remote_easyconnect_exchange_pairing"
        }
        DaemonApiResponse::RemoteEasyconnectRevokeSession(_) => "remote_easyconnect_revoke_session",
        DaemonApiResponse::RemoteEasyconnectRenewSession(_) => "remote_easyconnect_renew_session",
        DaemonApiResponse::RemoteEasyconnectUploadAdmission(_) => {
            "remote_easyconnect_upload_admission"
        }
        DaemonApiResponse::RemoteEasyconnectSubmitAwsCliUpload(_) => {
            "remote_easyconnect_submit_aws_cli_upload"
        }
        DaemonApiResponse::IngestProgress(_) => "ingest_progress",
        DaemonApiResponse::Error(_) => "error",
    }
}

#[cfg(test)]
mod tests {
    use super::{DaemonClient, DaemonClientError, InProcessDaemonTransport};
    use crate::api::{
        ApplianceTelemetryRequest, ApplianceTelemetryResponse, ApplianceTelemetryWindow,
        AssignLocalUserToLocalGroupRequest, AssignLocalUserToLocalGroupResponse,
        CapacityAdmissionRequest, CapacityAdmissionResponse, CapacityStatusRequest,
        CapacityStatusResponse, CreateLocalGroupRequest, CreateLocalGroupResponse,
        CreateObjectStoreRequest, CreateObjectStoreResponse, DaemonApiRequest, DaemonApiResponse,
        DaemonEndpointKind, DaemonEndpointValidation, DaemonEndpointValidationState,
        DaemonIngestConflictPolicy, DaemonJobCancelRequest, DaemonJobCancelResponse,
        DaemonJobEvent, DaemonJobId, DaemonJobKind, DaemonJobListRequest, DaemonJobListResponse,
        DaemonJobProgress, DaemonJobState, DaemonJobStatusRequest, DaemonJobStatusResponse,
        DaemonJobSummary, DaemonServiceLifecycleRequest, DaemonServiceLifecycleResponse,
        DaemonServiceOperation, DaemonServiceProvisionRequest, DaemonServiceProvisionResponse,
        DaemonServiceStatusRequest, DaemonServiceStatusResponse, DaemonSsdPressure,
        ObjectBrowserPageRequest, ObjectBrowserRequest, ObjectBrowserResponse, ObjectBrowserSort,
        ObjectDownloadRequest, ObjectDownloadResponse, ObjectFolderArchiveEntry,
        ObjectFolderDownloadRequest, ObjectFolderDownloadResponse, PrepareEnclosureFilesystem,
        PrepareEnclosureHddDevice, PrepareEnclosureRequest, PrepareEnclosureResponse,
        ProfileBrowserRequest, ProfileBrowserResponse, ProfileInspectionRequest,
        ProfileInspectionResponse, ProfileInspectionRootState,
        RemoteEasyconnectCreatePairingRequest, RemoteEasyconnectCreatePairingResponse,
        RemoteEasyconnectSubmitAwsCliUploadRequest, RemoteEasyconnectSubmitAwsCliUploadResponse,
        RemoteEasyconnectUploadAdmissionDecision, RemoteEasyconnectUploadAdmissionRequest,
        RemoteEasyconnectUploadBackpressureReason, StoreInventoryRequest, StoreInventoryResponse,
        SubmitIngestFilesRequest, UpsertEndpointInventoryRequest, UpsertEndpointInventoryResponse,
        ENCLOSURE_PREPARE_CONFIRMATION, ENDPOINT_RECORD_CONFIRMATION,
        OBJECT_STORE_CREATE_CONFIRMATION,
    };
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_core::remote_upload::{
        RemoteUploadBackpressureAction, RemoteUploadBackpressurePolicy,
    };
    use dasobjectstore_object_service::{ObjectServiceProviderId, ServiceState};
    use std::cell::RefCell;
    use std::path::PathBuf;

    #[test]
    fn in_process_transport_round_trips_typed_response() {
        let seen = RefCell::new(Vec::new());
        let transport = InProcessDaemonTransport::new(|request| {
            seen.borrow_mut().push(request);
            Ok(DaemonApiResponse::StoreInventory(StoreInventoryResponse {
                stores: Vec::new(),
            }))
        });
        let client = DaemonClient::new(transport);

        let response = client
            .store_inventory(StoreInventoryRequest::default())
            .expect("store inventory response");

        assert!(response.stores.is_empty());
        assert!(matches!(
            seen.borrow().as_slice(),
            [DaemonApiRequest::StoreInventory(_)]
        ));
    }

    #[test]
    fn profile_inspection_uses_typed_request_and_response() {
        let seen = RefCell::new(Vec::new());
        let transport = InProcessDaemonTransport::new(|request| {
            seen.borrow_mut().push(request);
            Ok(DaemonApiResponse::ProfileInspection(
                ProfileInspectionResponse {
                    schema_version: crate::api::PROFILE_INSPECTION_SCHEMA_VERSION.to_string(),
                    store_id: StoreId::new("codex").expect("store id"),
                    deployment_profile: dasobjectstore_core::deployment::DeploymentProfile::Folder,
                    host_mode: dasobjectstore_core::deployment::HostMode::PerUser,
                    protection: dasobjectstore_core::protection::ProtectionPolicy::LocalOnly,
                    root_state: ProfileInspectionRootState::Available,
                    unmanaged_path_count: 1,
                    unsafe_path_count: 0,
                    warnings: Vec::new(),
                },
            ))
        });
        let client = DaemonClient::new(transport);
        let response = client
            .profile_inspection(ProfileInspectionRequest {
                store_id: StoreId::new("codex").expect("store id"),
            })
            .expect("profile inspection response");
        assert_eq!(response.unmanaged_path_count, 1);
        assert!(matches!(
            seen.borrow().as_slice(),
            [DaemonApiRequest::ProfileInspection(_)]
        ));
    }

    #[test]
    fn profile_browser_uses_typed_request_and_response() {
        let seen = RefCell::new(Vec::new());
        let transport = InProcessDaemonTransport::new(|request| {
            seen.borrow_mut().push(request);
            Ok(DaemonApiResponse::ProfileBrowser(ProfileBrowserResponse {
                schema_version: crate::api::PROFILE_BROWSER_SCHEMA_VERSION.to_string(),
                store_id: StoreId::new("codex").expect("store id"),
                profile: dasobjectstore_core::deployment::DeploymentProfile::Folder,
                entries: Vec::new(),
                next_offset: None,
                total_entries: 0,
            }))
        });
        let client = DaemonClient::new(transport);
        let response = client
            .profile_browser(ProfileBrowserRequest {
                store_id: StoreId::new("codex").expect("store id"),
                prefix: Some("reads".to_string()),
                search: None,
                offset: 0,
                limit: 100,
                delegated_actor: None,
            })
            .expect("profile browser response");
        assert_eq!(
            response.profile,
            dasobjectstore_core::deployment::DeploymentProfile::Folder
        );
        assert!(matches!(
            seen.borrow().as_slice(),
            [DaemonApiRequest::ProfileBrowser(_)]
        ));
    }

    #[test]
    fn capacity_admission_uses_typed_request_and_response() {
        let seen = RefCell::new(Vec::new());
        let transport = InProcessDaemonTransport::new(|request| {
            seen.borrow_mut().push(request);
            Ok(DaemonApiResponse::CapacityAdmission(
                CapacityAdmissionResponse {
                    store_id: StoreId::new("codex").expect("store id"),
                    decision: crate::api::CapacityAdmissionDecision::Admitted,
                    reason: None,
                    requested_bytes: 4096,
                    copy_count: 2,
                    requires_ssd_staging: true,
                    logical_limit_bytes: Some(1_000_000),
                    used_bytes: 0,
                    reserved_bytes: 0,
                    logical_available_bytes: Some(1_000_000),
                    backend_free_bytes: 2_000_000,
                    backend_available_bytes: 2_000_000,
                    ssd_available_bytes: Some(500_000),
                    required_backend_bytes: 8192,
                    required_ssd_bytes: 4096,
                    copy_amplification_basis_points: 20_000,
                    warning_threshold_basis_points: 8_000,
                    critical_threshold_basis_points: 9_500,
                    message: None,
                },
            ))
        });
        let client = DaemonClient::new(transport);

        let response = client
            .capacity_admission(CapacityAdmissionRequest {
                store_id: "codex".to_string(),
                requested_bytes: 4096,
                copy_count: 2,
                ingress_origin: crate::api::DaemonIngressOrigin::RemoteS3,
                client_request_id: Some("request-1".to_string()),
            })
            .expect("capacity admission response");

        assert_eq!(
            response.decision,
            crate::api::CapacityAdmissionDecision::Admitted
        );
        assert!(matches!(
            seen.borrow().as_slice(),
            [DaemonApiRequest::CapacityAdmission(_)]
        ));
    }

    #[test]
    fn capacity_status_uses_read_only_typed_request_and_response() {
        let seen = RefCell::new(Vec::new());
        let transport = InProcessDaemonTransport::new(|request| {
            seen.borrow_mut().push(request);
            Ok(DaemonApiResponse::CapacityStatus(CapacityStatusResponse {
                store_id: StoreId::new("codex").expect("store id"),
                pressure: dasobjectstore_core::store::CapacityPressureState::Normal,
                logical_limit_bytes: Some(1_000),
                used_bytes: 100,
                reserved_bytes: 20,
                logical_available_bytes: Some(880),
                backend_free_bytes: 2_000,
                backend_available_bytes: 1_900,
                ssd_available_bytes: Some(500),
                copy_count: 2,
                requires_ssd_staging: true,
                warning_threshold_basis_points: 8_000,
                critical_threshold_basis_points: 9_500,
                admission_block_reason: None,
            }))
        });
        let client = DaemonClient::new(transport);
        let response = client
            .capacity_status(CapacityStatusRequest {
                store_id: "codex".to_string(),
            })
            .expect("capacity status response");
        assert_eq!(response.used_bytes, 100);
        assert!(matches!(
            seen.borrow().as_slice(),
            [DaemonApiRequest::CapacityStatus(_)]
        ));
    }

    #[test]
    fn validates_request_before_transport_send() {
        let transport = InProcessDaemonTransport::new(|_request| {
            panic!("invalid request should not reach transport")
        });
        let client = DaemonClient::new(transport);

        let err = client
            .submit_ingest_files(SubmitIngestFilesRequest {
                endpoint: StoreId::new("zymo").expect("store id"),
                source_path: "relative".into(),
                object_type: dasobjectstore_core::object_type::ObjectType::Naive,
                copies: None,
                hdd_workers: None,
                ingress_origin: crate::api::DaemonIngressOrigin::LocalServer,
                conflict_policy: DaemonIngestConflictPolicy::Strict,
                dry_run: false,
                client_request_id: None,
            })
            .expect_err("relative path is rejected");

        assert!(matches!(err, DaemonClientError::RequestValidation(_)));
    }

    #[test]
    fn rejects_unexpected_response_kind() {
        let transport = InProcessDaemonTransport::new(|_request| {
            Ok(DaemonApiResponse::StoreInventory(StoreInventoryResponse {
                stores: Vec::new(),
            }))
        });
        let client = DaemonClient::new(transport);

        let err = client
            .health_summary(Default::default())
            .expect_err("wrong response kind rejected");

        assert!(matches!(
            err,
            DaemonClientError::UnexpectedResponse {
                expected: "health_summary",
                actual: "store_inventory",
            }
        ));
    }

    #[test]
    fn surfaces_daemon_api_error_responses() {
        let transport = InProcessDaemonTransport::new(|_request| {
            Ok(DaemonApiResponse::Error(
                crate::api::DaemonApiErrorResponse::new(
                    "not_implemented",
                    "submit_ingest_files is not wired into dasobjectstored yet",
                ),
            ))
        });
        let client = DaemonClient::new(transport);

        let err = client
            .submit_ingest_files(SubmitIngestFilesRequest {
                endpoint: StoreId::new("zymo").expect("store id"),
                source_path: "/tmp/source".into(),
                object_type: dasobjectstore_core::object_type::ObjectType::Naive,
                copies: None,
                hdd_workers: None,
                ingress_origin: crate::api::DaemonIngressOrigin::LocalServer,
                conflict_policy: DaemonIngestConflictPolicy::Strict,
                dry_run: false,
                client_request_id: None,
            })
            .expect_err("daemon api error is surfaced");

        assert!(matches!(err, DaemonClientError::Api(_)));
        assert_eq!(
            err.to_string(),
            "daemon returned not_implemented error: submit_ingest_files is not wired into dasobjectstored yet"
        );
    }

    #[test]
    fn service_status_uses_typed_request_and_response() {
        let seen = RefCell::new(Vec::new());
        let transport = InProcessDaemonTransport::new(|request| {
            seen.borrow_mut().push(request);
            Ok(DaemonApiResponse::ServiceStatus(
                DaemonServiceStatusResponse {
                    provider_id: ObjectServiceProviderId::Garage,
                    state: ServiceState::Running,
                    endpoint: Some("http://127.0.0.1:3900".to_string()),
                    message: None,
                    detail: None,
                },
            ))
        });
        let client = DaemonClient::new(transport);

        let response = client
            .service_status(DaemonServiceStatusRequest {
                include_detail: true,
            })
            .expect("service status response");

        assert_eq!(response.provider_id, ObjectServiceProviderId::Garage);
        assert!(matches!(
            seen.borrow().as_slice(),
            [DaemonApiRequest::ServiceStatus(_)]
        ));
    }

    #[test]
    fn appliance_telemetry_uses_typed_request_and_response() {
        let seen = RefCell::new(Vec::new());
        let transport = InProcessDaemonTransport::new(|request| {
            seen.borrow_mut().push(request);
            Ok(DaemonApiResponse::ApplianceTelemetry(
                ApplianceTelemetryResponse::missing(ApplianceTelemetryWindow::OneDay),
            ))
        });
        let client = DaemonClient::new(transport);

        let response = client
            .appliance_telemetry(ApplianceTelemetryRequest {
                window: ApplianceTelemetryWindow::OneDay,
            })
            .expect("appliance telemetry response");

        assert_eq!(response.requested_window, ApplianceTelemetryWindow::OneDay);
        assert!(matches!(
            seen.borrow().as_slice(),
            [DaemonApiRequest::ApplianceTelemetry(_)]
        ));
    }

    #[test]
    fn service_lifecycle_uses_typed_request_and_response() {
        let seen = RefCell::new(Vec::new());
        let transport = InProcessDaemonTransport::new(|request| {
            seen.borrow_mut().push(request);
            Ok(DaemonApiResponse::ServiceLifecycle(
                DaemonServiceLifecycleResponse::accepted(
                    crate::api::DaemonJobId::new("service-1").expect("job id"),
                    "2026-07-07T11:38:12Z",
                    true,
                    DaemonServiceOperation::Start,
                    ObjectServiceProviderId::Garage,
                ),
            ))
        });
        let client = DaemonClient::new(transport);

        let response = client
            .service_lifecycle(DaemonServiceLifecycleRequest {
                operation: DaemonServiceOperation::Start,
                provider_id: ObjectServiceProviderId::Garage,
                dry_run: true,
                client_request_id: Some("request-1".to_string()),
            })
            .expect("service lifecycle response");

        assert!(response.accepted.dry_run);
        assert!(matches!(
            seen.borrow().as_slice(),
            [DaemonApiRequest::ServiceLifecycle(_)]
        ));
    }

    #[test]
    fn service_provision_uses_typed_request_and_response() {
        let seen = RefCell::new(Vec::new());
        let transport = InProcessDaemonTransport::new(|request| {
            seen.borrow_mut().push(request);
            Ok(DaemonApiResponse::ServiceProvision(
                DaemonServiceProvisionResponse::accepted(
                    crate::api::DaemonJobId::new("service-provision-1").expect("job id"),
                    "2026-07-07T12:05:42Z",
                    true,
                    ObjectServiceProviderId::Garage,
                    "/etc/dasobjectstore/stores.json",
                    "/var/lib/dasobjectstore/object-service/garage-credentials.json",
                    1,
                    1,
                    3,
                    0,
                    1,
                    0,
                ),
            ))
        });
        let client = DaemonClient::new(transport);

        let response = client
            .service_provision(DaemonServiceProvisionRequest {
                provider_id: ObjectServiceProviderId::Garage,
                dry_run: true,
                rotate_credentials: false,
                client_request_id: Some("request-1".to_string()),
            })
            .expect("service provision response");

        assert!(response.accepted.dry_run);
        assert_eq!(response.commands, 3);
        assert!(matches!(
            seen.borrow().as_slice(),
            [DaemonApiRequest::ServiceProvision(_)]
        ));
    }

    #[test]
    fn prepare_enclosure_uses_typed_request_and_response() {
        let seen = RefCell::new(Vec::new());
        let transport = InProcessDaemonTransport::new(|request| {
            seen.borrow_mut().push(request);
            Ok(DaemonApiResponse::PrepareEnclosure(
                PrepareEnclosureResponse::accepted(
                    crate::api::DaemonJobId::new("enclosure-prepare-1").expect("job id"),
                    "2026-07-08T19:40:00Z",
                    true,
                    "/dev/disk/by-id/nvme-ssd".into(),
                    vec![PrepareEnclosureHddDevice {
                        disk_id: "qnap-1057".to_string(),
                        device_path: "/dev/disk/by-id/usb-qnap-1057".into(),
                    }],
                    "/srv/dasobjectstore".into(),
                    PrepareEnclosureFilesystem::Ext4,
                    Some("stephen".to_string()),
                    Some("operator".to_string()),
                ),
            ))
        });
        let client = DaemonClient::new(transport);

        let response = client
            .prepare_enclosure(PrepareEnclosureRequest {
                ssd_device: "/dev/disk/by-id/nvme-ssd".into(),
                hdd_devices: vec![PrepareEnclosureHddDevice {
                    disk_id: "qnap-1057".to_string(),
                    device_path: "/dev/disk/by-id/usb-qnap-1057".into(),
                }],
                mount_root: "/srv/dasobjectstore".into(),
                filesystem: PrepareEnclosureFilesystem::Ext4,
                owner: Some("stephen".to_string()),
                dry_run: true,
                client_request_id: Some("request-1".to_string()),
                administrator_actor: Some("operator".to_string()),
                allow_format: true,
                existing_data_acknowledged: true,
                confirmation_marker: ENCLOSURE_PREPARE_CONFIRMATION.to_string(),
            })
            .expect("prepare enclosure response");

        assert!(response.accepted.dry_run);
        assert_eq!(response.hdd_devices.len(), 1);
        assert!(matches!(
            seen.borrow().as_slice(),
            [DaemonApiRequest::PrepareEnclosure(_)]
        ));
    }

    #[test]
    fn create_object_store_uses_typed_request_and_response() {
        let seen = RefCell::new(Vec::new());
        let transport = InProcessDaemonTransport::new(|request| {
            seen.borrow_mut().push(request);
            Ok(DaemonApiResponse::CreateObjectStore(
                CreateObjectStoreResponse::accepted(
                    crate::api::DaemonJobId::new("objectstore-create-1").expect("job id"),
                    "2026-07-08T21:15:00Z",
                    CreateObjectStoreRequest {
                        store_id: "generated-data".to_string(),
                        store_class: "generated_data".to_string(),
                        required_copies: 2,
                        bucket: Some("generated-data".to_string()),
                        reader_group: Some("bioinformatics-readers".to_string()),
                        writer_group: "bioinformatics".to_string(),
                        ssd_root: PathBuf::from("/srv/dasobjectstore/ssd"),
                        object_type: "pod5".to_string(),
                        enclosure_id: Some("tl-d800c-01".to_string()),
                        public: false,
                        writeable: true,
                        capacity_behavior: "balanced".to_string(),
                        retention: "standard".to_string(),
                        endpoint_export_mode: "s3_bucket".to_string(),
                        dry_run: true,
                        client_request_id: Some("request-1".to_string()),
                        administrator_actor: Some("operator".to_string()),
                        confirmation_marker: OBJECT_STORE_CREATE_CONFIRMATION.to_string(),
                    },
                ),
            ))
        });
        let client = DaemonClient::new(transport);

        let response = client
            .create_object_store(CreateObjectStoreRequest {
                store_id: "generated-data".to_string(),
                store_class: "generated_data".to_string(),
                required_copies: 2,
                bucket: Some("generated-data".to_string()),
                reader_group: Some("bioinformatics-readers".to_string()),
                writer_group: "bioinformatics".to_string(),
                ssd_root: PathBuf::from("/srv/dasobjectstore/ssd"),
                object_type: "pod5".to_string(),
                enclosure_id: Some("tl-d800c-01".to_string()),
                public: false,
                writeable: true,
                capacity_behavior: "balanced".to_string(),
                retention: "standard".to_string(),
                endpoint_export_mode: "s3_bucket".to_string(),
                dry_run: true,
                client_request_id: Some("request-1".to_string()),
                administrator_actor: Some("operator".to_string()),
                confirmation_marker: OBJECT_STORE_CREATE_CONFIRMATION.to_string(),
            })
            .expect("create object store response");

        assert!(response.accepted.dry_run);
        assert_eq!(response.accepted.kind, DaemonJobKind::ObjectStoreCreation);
        assert_eq!(response.writer_group, "bioinformatics");
        assert!(matches!(
            seen.borrow().as_slice(),
            [DaemonApiRequest::CreateObjectStore(_)]
        ));
    }

    #[test]
    fn remote_easyconnect_create_pairing_uses_typed_request_and_response() {
        let seen = RefCell::new(Vec::new());
        let transport = InProcessDaemonTransport::new(|request| {
            seen.borrow_mut().push(request);
            Ok(DaemonApiResponse::RemoteEasyconnectCreatePairing(
                RemoteEasyconnectCreatePairingResponse {
                    pairing_id: "pair-1".to_string(),
                    browser_login_url: "https://192.168.1.192:8448/products/dasobjectstore/remote/easyconnect/login?pairing_id=pair-1".to_string(),
                    callback_url: "http://127.0.0.1:49321/callback".to_string(),
                    expires_at_utc: "2026-07-09T12:10:00Z".to_string(),
                    polling_url: "https://192.168.1.192:8448/api/v1/remote/easyconnect/pairings/pair-1".to_string(),
                },
            ))
        });
        let client = DaemonClient::new(transport);

        let response = client
            .remote_easyconnect_create_pairing(RemoteEasyconnectCreatePairingRequest {
                client_name: "macbook".to_string(),
                callback_url: "http://127.0.0.1:49321/callback".to_string(),
                requested_object_store: Some("zymo_fecal_2025.05".to_string()),
                requested_session_lifetime_seconds: Some(28_800),
                client_request_id: Some("request-1".to_string()),
            })
            .expect("create pairing response");

        assert_eq!(response.pairing_id, "pair-1");
        assert!(matches!(
            seen.borrow().as_slice(),
            [DaemonApiRequest::RemoteEasyconnectCreatePairing(_)]
        ));
    }

    #[test]
    fn remote_easyconnect_upload_admission_uses_typed_request_and_response() {
        let seen = RefCell::new(Vec::new());
        let transport = InProcessDaemonTransport::new(|request| {
            seen.borrow_mut().push(request);
            Ok(DaemonApiResponse::RemoteEasyconnectUploadAdmission(
                RemoteEasyconnectUploadAdmissionDecision {
                    action: RemoteUploadBackpressureAction::PauseNewTransfers,
                    reason: RemoteEasyconnectUploadBackpressureReason::S3TransferConcurrencyFull,
                    retry_after_seconds: Some(30),
                    message: "Remote S3 transfer concurrency is full".to_string(),
                },
            ))
        });
        let client = DaemonClient::new(transport);

        let response = client
            .remote_easyconnect_upload_admission(RemoteEasyconnectUploadAdmissionRequest {
                policy: RemoteUploadBackpressurePolicy::default(),
                ssd_pressure: DaemonSsdPressure::AcceptingWrites,
                active_s3_transfers: 2,
                ssd_stage_queue_depth: 0,
                hdd_landing_queue_depth: 0,
                verification_queue_depth: 0,
            })
            .expect("upload admission response");

        assert_eq!(
            response.action,
            RemoteUploadBackpressureAction::PauseNewTransfers
        );
        assert!(matches!(
            seen.borrow().as_slice(),
            [DaemonApiRequest::RemoteEasyconnectUploadAdmission(_)]
        ));
    }

    #[test]
    fn remote_easyconnect_submit_aws_cli_upload_uses_typed_request_and_response() {
        let seen = RefCell::new(Vec::new());
        let transport = InProcessDaemonTransport::new(|request| {
            seen.borrow_mut().push(request);
            Ok(DaemonApiResponse::RemoteEasyconnectSubmitAwsCliUpload(
                RemoteEasyconnectSubmitAwsCliUploadResponse {
                    running_event: None,
                    progress_events: Vec::new(),
                    final_event: DaemonJobEvent::Complete(DaemonJobSummary {
                        job_id: DaemonJobId::new("remote-upload-job-1").expect("job id"),
                        kind: DaemonJobKind::RemoteUpload,
                        state: DaemonJobState::Complete,
                        progress: DaemonJobProgress {
                            stage: "remote_s3_transfer_complete".to_string(),
                            work_bytes_done: 42,
                            work_bytes_total: 42,
                            work_units_done: 1,
                            work_units_total: 1,
                            message: Some("completed".to_string()),
                        },
                        submitted_at_utc: "2026-07-09T14:40:00Z".to_string(),
                        updated_at_utc: "2026-07-09T14:40:00Z".to_string(),
                        actor: Some("stephen".to_string()),
                        failure_message: None,
                    }),
                },
            ))
        });
        let client = DaemonClient::new(transport);

        let response = client
            .remote_easyconnect_submit_aws_cli_upload(RemoteEasyconnectSubmitAwsCliUploadRequest {
                job_id: "remote-upload-job-1".to_string(),
                object_store: "zymo_fecal_2025.05".to_string(),
                source_bytes: 42,
                policy: RemoteUploadBackpressurePolicy::default(),
                ssd_pressure: DaemonSsdPressure::AcceptingWrites,
                program: "aws".to_string(),
                args: vec!["s3".to_string(), "cp".to_string()],
                display_args: vec!["s3".to_string(), "cp".to_string()],
                environment: Vec::new(),
                progress_telemetry: None,
                progress_message: Some("completed".to_string()),
            })
            .expect("upload submit response");

        let DaemonJobEvent::Complete(job) = response.final_event else {
            panic!("expected complete remote upload event");
        };
        assert_eq!(job.kind, DaemonJobKind::RemoteUpload);
        assert!(matches!(
            seen.borrow().as_slice(),
            [DaemonApiRequest::RemoteEasyconnectSubmitAwsCliUpload(_)]
        ));
    }

    #[test]
    fn object_browser_uses_typed_request_and_response() {
        let seen = RefCell::new(Vec::new());
        let transport = InProcessDaemonTransport::new(|request| {
            seen.borrow_mut().push(request);
            Ok(DaemonApiResponse::ObjectBrowser(ObjectBrowserResponse {
                endpoint: StoreId::new("ena").expect("store id"),
                prefix: "ENA".to_string(),
                breadcrumbs: Vec::new(),
                folders: Vec::new(),
                files: Vec::new(),
                next_cursor: None,
                total_entries: Some(0),
            }))
        });
        let client = DaemonClient::new(transport);

        let response = client
            .object_browser(ObjectBrowserRequest {
                endpoint: StoreId::new("ena").expect("store id"),
                prefix: Some("ENA".to_string()),
                search: None,
                sort: ObjectBrowserSort::NameAsc,
                page: ObjectBrowserPageRequest::default(),
                include_placement: true,
                delegated_actor: None,
            })
            .expect("object browser response");

        assert_eq!(response.endpoint.as_str(), "ena");
        assert_eq!(response.total_entries, Some(0));
        assert!(matches!(
            seen.borrow().as_slice(),
            [DaemonApiRequest::ObjectBrowser(_)]
        ));
    }

    #[test]
    fn object_download_uses_typed_request_and_response() {
        let transport = InProcessDaemonTransport::new(|request| {
            Ok(match request {
                DaemonApiRequest::ObjectDownload(request) => {
                    DaemonApiResponse::ObjectDownload(ObjectDownloadResponse {
                        endpoint: request.endpoint.clone(),
                        store_id: request.endpoint,
                        object_id: request.object_id,
                        file_name: "metadata.tsv".to_string(),
                        source_disk_id: dasobjectstore_core::ids::DiskId::new("disk-a")
                            .expect("disk id"),
                        source_path: "/srv/dasobjectstore/hdd/disk-a/objects/aa/object/payload"
                            .into(),
                        size_bytes: 512,
                    })
                }
                other => panic!("unexpected request {other:?}"),
            })
        });
        let client = DaemonClient::new(transport);

        let response = client
            .object_download(ObjectDownloadRequest {
                endpoint: StoreId::new("ena").expect("store id"),
                object_id: dasobjectstore_core::ids::ObjectId::new("ena/raw/metadata.tsv")
                    .expect("object id"),
                delegated_actor: None,
            })
            .expect("object download response");

        assert_eq!(response.file_name, "metadata.tsv");
        assert_eq!(response.size_bytes, 512);
    }

    #[test]
    fn object_folder_download_uses_typed_request_and_response() {
        let transport = InProcessDaemonTransport::new(|request| {
            Ok(match request {
                DaemonApiRequest::ObjectFolderDownload(request) => {
                    DaemonApiResponse::ObjectFolderDownload(ObjectFolderDownloadResponse {
                        endpoint: request.endpoint.clone(),
                        store_id: request.endpoint,
                        prefix: request.prefix,
                        archive_name: "raw.tar.gz".to_string(),
                        total_files: 1,
                        total_source_bytes: 512,
                        entries: vec![ObjectFolderArchiveEntry {
                            object_id: dasobjectstore_core::ids::ObjectId::new(
                                "ena/raw/metadata.tsv",
                            )
                            .expect("object id"),
                            archive_path: "metadata.tsv".to_string(),
                            source_disk_id: dasobjectstore_core::ids::DiskId::new("disk-a")
                                .expect("disk id"),
                            source_path: "/srv/dasobjectstore/hdd/disk-a/objects/aa/object/payload"
                                .into(),
                            size_bytes: 512,
                        }],
                    })
                }
                other => panic!("unexpected request {other:?}"),
            })
        });
        let client = DaemonClient::new(transport);

        let response = client
            .object_folder_download(ObjectFolderDownloadRequest {
                endpoint: StoreId::new("ena").expect("store id"),
                prefix: "ena/raw".to_string(),
                delegated_actor: None,
            })
            .expect("folder download response");

        assert_eq!(response.archive_name, "raw.tar.gz");
        assert_eq!(response.entries[0].archive_path, "metadata.tsv");
    }

    #[test]
    fn endpoint_inventory_upsert_uses_typed_request_and_response() {
        let seen = RefCell::new(Vec::new());
        let transport = InProcessDaemonTransport::new(|request| {
            seen.borrow_mut().push(request);
            Ok(DaemonApiResponse::UpsertEndpointInventory(
                UpsertEndpointInventoryResponse::accepted(
                    crate::api::DaemonJobId::new("endpoint-upsert-1").expect("job id"),
                    "2026-07-09T00:00:00Z",
                    "/opt/dasobjectstore/endpoints.json",
                    endpoint_inventory_request(),
                ),
            ))
        });
        let client = DaemonClient::new(transport);

        let response = client
            .upsert_endpoint_inventory(endpoint_inventory_request())
            .expect("endpoint inventory response");

        assert!(response.accepted.dry_run);
        assert_eq!(response.accepted.kind, DaemonJobKind::EndpointValidation);
        assert_eq!(response.endpoint_id, "nas-staging");
        assert!(matches!(
            seen.borrow().as_slice(),
            [DaemonApiRequest::UpsertEndpointInventory(_)]
        ));
    }

    #[test]
    fn job_status_uses_typed_request_and_response() {
        let seen = RefCell::new(Vec::new());
        let transport = InProcessDaemonTransport::new(|request| {
            seen.borrow_mut().push(request);
            Ok(DaemonApiResponse::JobStatus(DaemonJobStatusResponse {
                job: DaemonJobSummary {
                    job_id: DaemonJobId::new("enclosure-prepare-1").expect("job id"),
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
            }))
        });
        let client = DaemonClient::new(transport);

        let response = client
            .job_status(DaemonJobStatusRequest {
                job_id: DaemonJobId::new("enclosure-prepare-1").expect("job id"),
            })
            .expect("job status response");

        assert_eq!(response.job.kind, DaemonJobKind::EnclosurePreparation);
        assert!(matches!(
            seen.borrow().as_slice(),
            [DaemonApiRequest::JobStatus(_)]
        ));
    }

    #[test]
    fn list_jobs_uses_typed_request_and_response() {
        let seen = RefCell::new(Vec::new());
        let transport = InProcessDaemonTransport::new(|request| {
            seen.borrow_mut().push(request);
            Ok(DaemonApiResponse::JobList(DaemonJobListResponse {
                jobs: vec![DaemonJobSummary {
                    job_id: DaemonJobId::new("enclosure-prepare-1").expect("job id"),
                    kind: DaemonJobKind::EnclosurePreparation,
                    state: DaemonJobState::Complete,
                    progress: DaemonJobProgress::default(),
                    submitted_at_utc: "2026-07-08T20:05:00Z".to_string(),
                    updated_at_utc: "2026-07-08T20:05:10Z".to_string(),
                    actor: Some("operator".to_string()),
                    failure_message: None,
                }],
            }))
        });
        let client = DaemonClient::new(transport);

        let response = client
            .list_jobs(DaemonJobListRequest { limit: Some(25) })
            .expect("job list response");

        assert_eq!(response.jobs.len(), 1);
        assert_eq!(response.jobs[0].kind, DaemonJobKind::EnclosurePreparation);
        assert!(matches!(
            seen.borrow().as_slice(),
            [DaemonApiRequest::JobList(_)]
        ));
    }

    #[test]
    fn cancel_job_uses_typed_request_and_response() {
        let seen = RefCell::new(Vec::new());
        let transport = InProcessDaemonTransport::new(|request| {
            seen.borrow_mut().push(request);
            Ok(DaemonApiResponse::CancelJob(DaemonJobCancelResponse {
                job_id: DaemonJobId::new("enclosure-prepare-1").expect("job id"),
                accepted: true,
                state: DaemonJobState::Cancelled,
            }))
        });
        let client = DaemonClient::new(transport);

        let response = client
            .cancel_job(DaemonJobCancelRequest {
                job_id: DaemonJobId::new("enclosure-prepare-1").expect("job id"),
                reason: Some("operator requested cancellation".to_string()),
            })
            .expect("cancel job response");

        assert!(response.accepted);
        assert!(matches!(
            seen.borrow().as_slice(),
            [DaemonApiRequest::CancelJob(_)]
        ));
    }

    #[test]
    fn create_local_group_uses_typed_request_and_response() {
        let seen = RefCell::new(Vec::new());
        let transport = InProcessDaemonTransport::new(|request| {
            seen.borrow_mut().push(request);
            Ok(DaemonApiResponse::CreateLocalGroup(
                CreateLocalGroupResponse::accepted(
                    crate::api::DaemonJobId::new("local-admin-1").expect("job id"),
                    "2026-07-07T12:10:42Z",
                    true,
                    Some("request-1".to_string()),
                    "mnemosyne",
                    Some("operator".to_string()),
                ),
            ))
        });
        let client = DaemonClient::new(transport);

        let response = client
            .create_local_group(CreateLocalGroupRequest {
                group_name: "mnemosyne".to_string(),
                dry_run: true,
                client_request_id: Some("request-1".to_string()),
                administrator_actor: Some("operator".to_string()),
                confirmation_marker: "confirm create local group".to_string(),
            })
            .expect("create group response");

        assert_eq!(response.group_name, "mnemosyne");
        assert!(matches!(
            seen.borrow().as_slice(),
            [DaemonApiRequest::CreateLocalGroup(_)]
        ));
    }

    #[test]
    fn assign_local_user_to_local_group_uses_typed_request_and_response() {
        let seen = RefCell::new(Vec::new());
        let transport = InProcessDaemonTransport::new(|request| {
            seen.borrow_mut().push(request);
            Ok(DaemonApiResponse::AssignLocalUserToLocalGroup(
                AssignLocalUserToLocalGroupResponse::accepted(
                    crate::api::DaemonJobId::new("local-admin-2").expect("job id"),
                    "2026-07-07T12:11:42Z",
                    true,
                    Some("request-2".to_string()),
                    "stephen",
                    "mnemosyne",
                    Some("operator".to_string()),
                ),
            ))
        });
        let client = DaemonClient::new(transport);

        let response = client
            .assign_local_user_to_local_group(AssignLocalUserToLocalGroupRequest {
                username: "stephen".to_string(),
                group_name: "mnemosyne".to_string(),
                dry_run: true,
                client_request_id: Some("request-2".to_string()),
                administrator_actor: Some("operator".to_string()),
                confirmation_marker: "confirm assign local user".to_string(),
            })
            .expect("assign user response");

        assert_eq!(response.username, "stephen");
        assert_eq!(response.group_name, "mnemosyne");
        assert!(matches!(
            seen.borrow().as_slice(),
            [DaemonApiRequest::AssignLocalUserToLocalGroup(_)]
        ));
    }

    #[test]
    fn rejects_invalid_local_admin_request_before_transport_send() {
        let transport = InProcessDaemonTransport::new(|_request| {
            panic!("invalid request should not reach transport")
        });
        let client = DaemonClient::new(transport);

        let err = client
            .create_local_group(CreateLocalGroupRequest {
                group_name: "Invalid Group".to_string(),
                dry_run: true,
                client_request_id: None,
                administrator_actor: None,
                confirmation_marker: "confirm create local group".to_string(),
            })
            .expect_err("invalid group name rejected");

        assert!(matches!(err, DaemonClientError::RequestValidation(_)));
    }

    #[test]
    fn rejects_invalid_prepare_enclosure_request_before_transport_send() {
        let transport = InProcessDaemonTransport::new(|_request| {
            panic!("invalid request should not reach transport")
        });
        let client = DaemonClient::new(transport);

        let err = client
            .prepare_enclosure(PrepareEnclosureRequest {
                ssd_device: "relative".into(),
                hdd_devices: Vec::new(),
                mount_root: "/srv/dasobjectstore".into(),
                filesystem: PrepareEnclosureFilesystem::Ext4,
                owner: None,
                dry_run: false,
                client_request_id: None,
                administrator_actor: None,
                allow_format: false,
                existing_data_acknowledged: false,
                confirmation_marker: "wrong".to_string(),
            })
            .expect_err("invalid prepare request rejected");

        assert!(matches!(err, DaemonClientError::RequestValidation(_)));
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
                message: None,
            },
            manager_product_id: "dasobjectstore".to_string(),
            active_bindings: Vec::new(),
            dry_run: true,
            client_request_id: Some("endpoint-upsert-1".to_string()),
            administrator_actor: Some("operator".to_string()),
            confirmation_marker: Some(ENDPOINT_RECORD_CONFIRMATION.to_string()),
        }
    }
}
