use super::*;

#[path = "easyconnect.rs"]
mod easyconnect;
#[path = "service.rs"]
mod service;
#[path = "storage.rs"]
mod storage;

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
        service_request @ (DaemonApiRequest::ServiceStatus(_)
        | DaemonApiRequest::ServiceLifecycle(_)
        | DaemonApiRequest::ServiceProvision(_)
        | DaemonApiRequest::PrepareEnclosure(_)
        | DaemonApiRequest::CreateObjectStore(_)
        | DaemonApiRequest::RegisterProfileBinding(_)
        | DaemonApiRequest::UpsertEndpointInventory(_)
        | DaemonApiRequest::CreateLocalGroup(_)
        | DaemonApiRequest::AssignLocalUserToLocalGroup(_)
        | DaemonApiRequest::ProfileCapabilities(_)
        | DaemonApiRequest::JobList(_)
        | DaemonApiRequest::JobStatus(_)
        | DaemonApiRequest::CancelJob(_)) => service::request(handler, service_request),
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
        | DaemonApiRequest::IngestControl(_)
        | DaemonApiRequest::ApplianceTelemetry(_)
        | DaemonApiRequest::SubmitIngestFiles(_)
        | DaemonApiRequest::UpdateObjectStoreIngestPolicy(_)
        | DaemonApiRequest::ObjectBrowser(_)
        | DaemonApiRequest::ObjectDownload(_)
        | DaemonApiRequest::ObjectFolderDownload(_)) => {
            storage::request(handler, storage_request, actor, emit_progress)
        }
        easyconnect_request @ (DaemonApiRequest::RemoteEasyconnectCreatePairing(_)
        | DaemonApiRequest::RemoteEasyconnectApprovePairing(_)
        | DaemonApiRequest::RemoteEasyconnectExchangePairing(_)
        | DaemonApiRequest::RemoteEasyconnectRevokeSession(_)
        | DaemonApiRequest::RemoteEasyconnectRenewSession(_)
        | DaemonApiRequest::RemoteEasyconnectUploadAdmission(_)
        | DaemonApiRequest::RemoteEasyconnectSubmitAwsCliUpload(_)) => {
            easyconnect::request(handler, easyconnect_request, actor)
        }
        request => Ok(DaemonApiResponse::Error(DaemonApiErrorResponse::new(
            "not_implemented",
            format!(
                "{} is not wired into dasobjectstored yet",
                request.command_name()
            ),
        ))),
    }
}
