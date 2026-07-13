use super::*;
use crate::api::{
    CapacityAdmissionRequest, CapacityAdmissionResponse, CapacityStatusRequest,
    CapacityStatusResponse,
};

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

    /// Evaluate capacity using daemon-owned observations. Until a live
    /// ledger/probe provider is installed, fail closed instead of fabricating
    /// an admission decision from client-supplied values.
    fn capacity_admission(
        &self,
        _request: CapacityAdmissionRequest,
    ) -> Result<CapacityAdmissionResponse, DaemonServiceRuntimeError> {
        Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: "capacity admission provider is not configured".to_string(),
        })
    }

    fn capacity_status(
        &self,
        _request: CapacityStatusRequest,
    ) -> Result<CapacityStatusResponse, DaemonServiceRuntimeError> {
        Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: "capacity status provider is not configured".to_string(),
        })
    }

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

    fn reconcile_store_s3(
        &self,
        _store_id: StoreId,
        _prefix: Option<String>,
        _dry_run: bool,
        _accepted_at_utc: &str,
        _emit_progress: &mut dyn FnMut(
            DaemonIngestProgressEvent,
        ) -> Result<(), DaemonIngestFilesRuntimeError>,
    ) -> Result<StoreRepairS3Reconciliation, DaemonServiceRuntimeError> {
        Err(DaemonServiceRuntimeError::UnsupportedOperation {
            operation: "S3 reconciliation requires an object-service command runner".to_string(),
        })
    }

    fn reconcile_store_s3_cancellable(
        &self,
        store_id: StoreId,
        prefix: Option<String>,
        dry_run: bool,
        accepted_at_utc: &str,
        is_cancelled: &dyn Fn() -> bool,
        emit_progress: &mut dyn FnMut(
            DaemonIngestProgressEvent,
        ) -> Result<(), DaemonIngestFilesRuntimeError>,
    ) -> Result<StoreRepairS3Reconciliation, DaemonServiceRuntimeError> {
        let _ = is_cancelled;
        self.reconcile_store_s3(store_id, prefix, dry_run, accepted_at_utc, emit_progress)
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
