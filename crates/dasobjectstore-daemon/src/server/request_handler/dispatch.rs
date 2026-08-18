use super::*;

#[path = "easyconnect.rs"]
mod easyconnect;
#[path = "service.rs"]
mod service;
#[path = "storage.rs"]
mod storage;
#[path = "workspace.rs"]
mod workspace;

/// Routes validated daemon API requests to their request-family handlers.
pub(super) fn request<S, C>(
    handler: &DaemonRequestHandler<S, C>,
    request: DaemonApiRequest,
    actor: Option<&DaemonLocalActor>,
    emit_progress: &mut impl FnMut(
        DaemonIngestProgressEvent,
    ) -> Result<(), DaemonIngestFilesRuntimeError>,
) -> Result<DaemonApiResponse, DaemonRequestHandlerError>
where
    S: DaemonServiceOrchestrator,
    C: DaemonClock,
{
    match request {
        DaemonApiRequest::LiveStatus(_) => Ok(DaemonApiResponse::LiveStatus(handler.live_status())),
        governed_request @ (DaemonApiRequest::ExchangeErgasterionCapability(_)
        | DaemonApiRequest::RenewErgasterionCapability(_)
        | DaemonApiRequest::DiscoverErgasterionCapability
        | DaemonApiRequest::AdmitGovernedBindingAuthority(_)
        | DaemonApiRequest::ErgasterionObjectSnapshot(_)
        | DaemonApiRequest::ErgasterionObjectGroupStatus(_)) => {
            ergasterion::request(handler, governed_request, actor)
        }
        service_request @ (DaemonApiRequest::ServiceStatus(_)
        | DaemonApiRequest::ServiceLifecycle(_)
        | DaemonApiRequest::ServiceProvision(_)
        | DaemonApiRequest::ExchangeApplicationAccessToken(_)
        | DaemonApiRequest::RegisterApplicationIdentity(_)
        | DaemonApiRequest::RegisterApplicationKey(_)
        | DaemonApiRequest::RevokeApplicationCredential(_)
        | DaemonApiRequest::AuthorizeApplicationMtls(_)
        | DaemonApiRequest::PrepareEnclosure(_)
        | DaemonApiRequest::CreateObjectStore(_)
        | DaemonApiRequest::RegisterProfileBinding(_)
        | DaemonApiRequest::ProfileMigration(_)
        | DaemonApiRequest::ProfileInspection(_)
        | DaemonApiRequest::ProfileReadiness(_)
        | DaemonApiRequest::UpsertEndpointInventory(_)
        | DaemonApiRequest::TestEndpointConnection(_)
        | DaemonApiRequest::CreateLocalGroup(_)
        | DaemonApiRequest::AssignLocalUserToLocalGroup(_)
        | DaemonApiRequest::ProfileCapabilities(_)
        | DaemonApiRequest::JobList(_)
        | DaemonApiRequest::JobStatus(_)
        | DaemonApiRequest::CancelJob(_)) => service::request(handler, service_request, actor),
        storage_request @ (DaemonApiRequest::StoreInventory(_)
        | DaemonApiRequest::CapacityAdmission(_)
        | DaemonApiRequest::CapacityStatus(_)
        | DaemonApiRequest::DiskRetire(_)
        | DaemonApiRequest::DiskForceRetire(_)
        | DaemonApiRequest::StoreDrain(_)
        | DaemonApiRequest::StoreDelete(_)
        | DaemonApiRequest::StoreVerify(_)
        | DaemonApiRequest::StoreDeduplicate(_)
        | DaemonApiRequest::StoreRepair(_)
        | DaemonApiRequest::ObjectPut(_)
        | DaemonApiRequest::IngestQueueDrain(_)
        | DaemonApiRequest::DestageRetry(_)
        | DaemonApiRequest::IngestControl(_)
        | DaemonApiRequest::ApplianceTelemetry(_)
        | DaemonApiRequest::SubmitIngestFiles(_)
        | DaemonApiRequest::UpdateObjectStoreIngestPolicy(_)
        | DaemonApiRequest::UpdateObjectStoreAcknowledgementPolicy(_)
        | DaemonApiRequest::ObjectBrowser(_)
        | DaemonApiRequest::RemoteObjectSnapshot(_)
        | DaemonApiRequest::RemoteObjectGroupStatus(_)
        | DaemonApiRequest::ProfileBrowser(_)
        | DaemonApiRequest::ProfileCatalogueExport(_)
        | DaemonApiRequest::ProfileCatalogueImport(_)
        | DaemonApiRequest::ProfileS3List(_)
        | DaemonApiRequest::ProfileS3Delete(_)
        | DaemonApiRequest::ProfileS3MultipartAbort(_)
        | DaemonApiRequest::ProfileS3MultipartComplete(_)
        | DaemonApiRequest::ProfileS3MultipartUploads(_)
        | DaemonApiRequest::ProfileS3Head(_)
        | DaemonApiRequest::ProfileS3Verify(_)
        | DaemonApiRequest::ProfileS3Health(_)
        | DaemonApiRequest::ProfileDiagnostics(_)
        | DaemonApiRequest::ObjectDownload(_)
        | DaemonApiRequest::ObjectFolderDownload(_)
        | DaemonApiRequest::JenkinsDossierEvidenceSettlement(_)) => {
            storage::request(handler, storage_request, actor, emit_progress)
        }
        projection_request @ (DaemonApiRequest::PrepareSynoptikonProjection(_)
        | DaemonApiRequest::SettleSynoptikonProjection(_)) => {
            storage::request(handler, projection_request, actor, emit_progress)
        }
        easyconnect_request @ (DaemonApiRequest::RemoteEasyconnectCreatePairing(_)
        | DaemonApiRequest::RemoteEasyconnectPairingStatus(_)
        | DaemonApiRequest::RemoteEasyconnectApprovePairing(_)
        | DaemonApiRequest::RemoteEasyconnectExchangePairing(_)
        | DaemonApiRequest::RemoteEasyconnectRevokeSession(_)
        | DaemonApiRequest::RemoteEasyconnectRenewSession(_)
        | DaemonApiRequest::RemoteEasyconnectUploadAdmission(_)
        | DaemonApiRequest::RemoteEasyconnectSubmitAwsCliUpload(_)
        | DaemonApiRequest::IssueApplicationUploadCapability(_)
        | DaemonApiRequest::CompleteApplicationUpload(_)
        | DaemonApiRequest::DeleteApplicationObject(_)) => {
            easyconnect::request(handler, easyconnect_request, actor)
        }
        DaemonApiRequest::WorkspaceControl(request) => workspace::request(handler, request, actor),
        request => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
            "not_implemented",
            format!(
                "{} is not wired into dasobjectstored yet",
                request.command_name()
            ),
        ))),
    }
}
