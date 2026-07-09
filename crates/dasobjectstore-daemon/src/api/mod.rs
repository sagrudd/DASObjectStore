//! Transport-neutral daemon API contracts.

mod enclosure;
mod endpoint;
mod health;
mod ingest;
mod jobs;
mod local_admin;
mod object_browser;
mod object_store;
mod service;
mod stores;

pub use enclosure::{
    PrepareEnclosureFilesystem, PrepareEnclosureHddDevice, PrepareEnclosureRequest,
    PrepareEnclosureResponse, PrepareEnclosureValidationError, ENCLOSURE_PREPARE_CONFIRMATION,
};
pub use endpoint::{
    DaemonEndpointBinding, DaemonEndpointBindingReadiness, DaemonEndpointKind,
    DaemonEndpointValidation, DaemonEndpointValidationState, EndpointInventoryValidationError,
    UpsertEndpointInventoryRequest, UpsertEndpointInventoryResponse, ENDPOINT_RECORD_CONFIRMATION,
};
pub use health::{
    DaemonApiWarning, DaemonDiskHealthSummary, DaemonHealthSummaryRequest,
    DaemonHealthSummaryResponse, DaemonIngestSummary, DaemonSsdPressure,
};
pub use ingest::{
    CancelIngestJobRequest, CancelIngestJobResponse, DaemonIngestAdaptiveSchedulerInput,
    DaemonIngestAdaptiveSchedulingLimit, DaemonIngestAdaptiveWorkerSchedule,
    DaemonIngestBottleneck, DaemonIngestBoundedBufferPolicy, DaemonIngestBufferPoolPolicySet,
    DaemonIngestCompletionFraction, DaemonIngestConflictAction, DaemonIngestConflictDecision,
    DaemonIngestConflictPolicy, DaemonIngestConflictReason, DaemonIngestErrorRate,
    DaemonIngestHddQueueState, DaemonIngestHddTargetQueue, DaemonIngestObjectSnapshot,
    DaemonIngestPipelinePressure, DaemonIngestPipelineStage, DaemonIngestPlacementSchedulerInput,
    DaemonIngestPressure, DaemonIngestProgressEvent, DaemonIngestProgressFractions,
    DaemonIngestQueueDepths, DaemonIngestResourcePolicy, DaemonIngestSchedulingPolicy,
    DaemonIngestStage, DaemonIngestSystemSafetyReserve, DaemonIngestSystemTelemetry,
    DaemonIngestTargetCapacity, DaemonIngestTargetFailureState, DaemonIngestTelemetry,
    DaemonIngestThroughputTelemetry, DaemonIngestThroughputTrend, DaemonIngestWorkerActivity,
    DaemonIngestWorkerCounts, DaemonIngestWorkerTelemetry, DaemonRequestValidationError,
    DaemonSourceReadBackpressureAction, DaemonSourceReadBackpressureDecision,
    DaemonSourceReadBackpressureInput, DaemonSourceReadBackpressurePolicy,
    DaemonSourceReadBackpressureReason, DaemonSourceReadPriority, DaemonSourceToSsdPriorityPolicy,
    DaemonSourceToSsdQueueUsage, IngestJobStatusRequest, IngestJobStatusResponse,
    SubmitIngestFilesRequest, SubmitIngestFilesResponse,
};
pub use jobs::{
    DaemonJobAcceptedResponse, DaemonJobCancelRequest, DaemonJobCancelResponse, DaemonJobEvent,
    DaemonJobId, DaemonJobIdError, DaemonJobKind, DaemonJobListRequest, DaemonJobListResponse,
    DaemonJobProgress, DaemonJobState, DaemonJobStatusRequest, DaemonJobStatusResponse,
    DaemonJobSummary, DaemonJobValidationError,
};
pub use local_admin::{
    AssignLocalUserToLocalGroupRequest, AssignLocalUserToLocalGroupResponse,
    CreateLocalGroupRequest, CreateLocalGroupResponse, DaemonLocalAdminAcceptedResponse,
    DaemonLocalAdminCommand, DaemonLocalAdminValidationError,
};
pub use object_browser::{
    ObjectBrowserBreadcrumb, ObjectBrowserChecksum, ObjectBrowserFileNode, ObjectBrowserFolderNode,
    ObjectBrowserPageRequest, ObjectBrowserPlacement, ObjectBrowserPlacementLocation,
    ObjectBrowserPlacementState, ObjectBrowserReadinessState, ObjectBrowserRequest,
    ObjectBrowserResponse, ObjectBrowserSort, OBJECT_BROWSER_MAX_PAGE_LIMIT,
};
pub use object_store::{
    CreateObjectStoreRequest, CreateObjectStoreResponse, CreateObjectStoreValidationError,
    OBJECT_STORE_CREATE_CONFIRMATION,
};
pub use service::{
    DaemonServiceLifecycleRequest, DaemonServiceLifecycleResponse, DaemonServiceOperation,
    DaemonServiceProvisionRequest, DaemonServiceProvisionResponse, DaemonServiceStatusDetail,
    DaemonServiceStatusRequest, DaemonServiceStatusResponse,
};
pub use stores::{StoreInventoryItem, StoreInventoryRequest, StoreInventoryResponse};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "command", content = "payload")]
pub enum DaemonApiRequest {
    HealthSummary(DaemonHealthSummaryRequest),
    StoreInventory(StoreInventoryRequest),
    SubmitIngestFiles(SubmitIngestFilesRequest),
    IngestJobStatus(IngestJobStatusRequest),
    CancelIngestJob(CancelIngestJobRequest),
    JobList(DaemonJobListRequest),
    JobStatus(DaemonJobStatusRequest),
    CancelJob(DaemonJobCancelRequest),
    ServiceStatus(DaemonServiceStatusRequest),
    ServiceLifecycle(DaemonServiceLifecycleRequest),
    ServiceProvision(DaemonServiceProvisionRequest),
    PrepareEnclosure(PrepareEnclosureRequest),
    CreateObjectStore(CreateObjectStoreRequest),
    ObjectBrowser(ObjectBrowserRequest),
    UpsertEndpointInventory(UpsertEndpointInventoryRequest),
    CreateLocalGroup(CreateLocalGroupRequest),
    AssignLocalUserToLocalGroup(AssignLocalUserToLocalGroupRequest),
}

impl DaemonApiRequest {
    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        match self {
            Self::SubmitIngestFiles(request) => request.validate(),
            Self::CancelIngestJob(request) => request.validate(),
            Self::CancelJob(request) => request.validate().map_err(generic_job_validation_error),
            Self::ServiceLifecycle(request) => request.validate(),
            Self::ServiceProvision(request) => request.validate(),
            Self::PrepareEnclosure(request) => request
                .validate()
                .map_err(prepare_enclosure_validation_error),
            Self::CreateObjectStore(request) => request
                .validate()
                .map_err(create_object_store_validation_error),
            Self::ObjectBrowser(request) => request.validate(),
            Self::UpsertEndpointInventory(request) => request
                .validate()
                .map_err(endpoint_inventory_validation_error),
            Self::CreateLocalGroup(request) => {
                request.validate().map_err(local_admin_validation_error)
            }
            Self::AssignLocalUserToLocalGroup(request) => {
                request.validate().map_err(local_admin_validation_error)
            }
            Self::HealthSummary(_)
            | Self::StoreInventory(_)
            | Self::IngestJobStatus(_)
            | Self::JobList(_)
            | Self::JobStatus(_)
            | Self::ServiceStatus(_) => Ok(()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "payload")]
pub enum DaemonApiResponse {
    HealthSummary(DaemonHealthSummaryResponse),
    StoreInventory(StoreInventoryResponse),
    SubmitIngestFiles(SubmitIngestFilesResponse),
    IngestJobStatus(IngestJobStatusResponse),
    CancelIngestJob(CancelIngestJobResponse),
    JobList(DaemonJobListResponse),
    JobStatus(DaemonJobStatusResponse),
    CancelJob(DaemonJobCancelResponse),
    ServiceStatus(DaemonServiceStatusResponse),
    ServiceLifecycle(DaemonServiceLifecycleResponse),
    ServiceProvision(DaemonServiceProvisionResponse),
    PrepareEnclosure(PrepareEnclosureResponse),
    CreateObjectStore(CreateObjectStoreResponse),
    ObjectBrowser(ObjectBrowserResponse),
    UpsertEndpointInventory(UpsertEndpointInventoryResponse),
    CreateLocalGroup(CreateLocalGroupResponse),
    AssignLocalUserToLocalGroup(AssignLocalUserToLocalGroupResponse),
    IngestProgress(DaemonIngestProgressEvent),
    Error(DaemonApiErrorResponse),
}

fn endpoint_inventory_validation_error(
    err: EndpointInventoryValidationError,
) -> DaemonRequestValidationError {
    match err {
        EndpointInventoryValidationError::BlankField { field } => {
            DaemonRequestValidationError::BlankField { field }
        }
        EndpointInventoryValidationError::UnsafeLocalName { field, value } => {
            DaemonRequestValidationError::UnsafeLocalName { field, value }
        }
        EndpointInventoryValidationError::InvalidUrl { field, value } => {
            DaemonRequestValidationError::UnsupportedFieldValue { field, value }
        }
        EndpointInventoryValidationError::BlankClientRequestId => {
            DaemonRequestValidationError::BlankClientRequestId
        }
        EndpointInventoryValidationError::ConfirmationMismatch => {
            DaemonRequestValidationError::ConfirmationMismatch {
                expected: ENDPOINT_RECORD_CONFIRMATION,
            }
        }
    }
}

fn create_object_store_validation_error(
    err: CreateObjectStoreValidationError,
) -> DaemonRequestValidationError {
    match err {
        CreateObjectStoreValidationError::BlankField { field } => {
            DaemonRequestValidationError::BlankField { field }
        }
        CreateObjectStoreValidationError::UnsafeName { field, value } => {
            DaemonRequestValidationError::UnsafeLocalName { field, value }
        }
        CreateObjectStoreValidationError::InvalidCopyCount { copies } => {
            DaemonRequestValidationError::InvalidCopyCount { copies }
        }
        CreateObjectStoreValidationError::RelativePath { field, path } => {
            DaemonRequestValidationError::RelativePath { field, path }
        }
        CreateObjectStoreValidationError::BlankClientRequestId => {
            DaemonRequestValidationError::BlankClientRequestId
        }
        CreateObjectStoreValidationError::ConfirmationMismatch => {
            DaemonRequestValidationError::ConfirmationMismatch {
                expected: OBJECT_STORE_CREATE_CONFIRMATION,
            }
        }
        CreateObjectStoreValidationError::InvalidFieldValue { field, value } => {
            DaemonRequestValidationError::UnsupportedFieldValue { field, value }
        }
        CreateObjectStoreValidationError::InvalidPolicy { message } => {
            DaemonRequestValidationError::InvalidPolicy { message }
        }
    }
}

fn prepare_enclosure_validation_error(
    err: PrepareEnclosureValidationError,
) -> DaemonRequestValidationError {
    match err {
        PrepareEnclosureValidationError::RelativePath { field, path } => {
            DaemonRequestValidationError::RelativePath { field, path }
        }
        PrepareEnclosureValidationError::NoHddDevices => DaemonRequestValidationError::BlankField {
            field: "hdd_devices",
        },
        PrepareEnclosureValidationError::BlankHddDiskId => {
            DaemonRequestValidationError::BlankField { field: "disk_id" }
        }
        PrepareEnclosureValidationError::UnsafeName { field, value } => {
            DaemonRequestValidationError::UnsafeLocalName { field, value }
        }
        PrepareEnclosureValidationError::DuplicateHddDiskId { disk_id } => {
            DaemonRequestValidationError::DuplicateFieldValue {
                field: "hdd_devices.disk_id",
                value: disk_id,
            }
        }
        PrepareEnclosureValidationError::DuplicateHddDevicePath { device_path } => {
            DaemonRequestValidationError::DuplicateFieldValue {
                field: "hdd_devices.device_path",
                value: device_path.display().to_string(),
            }
        }
        PrepareEnclosureValidationError::FormatNotAllowed => {
            DaemonRequestValidationError::FormatNotAllowed
        }
        PrepareEnclosureValidationError::ExistingDataNotAcknowledged => {
            DaemonRequestValidationError::ExistingDataNotAcknowledged
        }
        PrepareEnclosureValidationError::ConfirmationMismatch => {
            DaemonRequestValidationError::ConfirmationMismatch {
                expected: ENCLOSURE_PREPARE_CONFIRMATION,
            }
        }
        PrepareEnclosureValidationError::BlankClientRequestId => {
            DaemonRequestValidationError::BlankClientRequestId
        }
    }
}

fn local_admin_validation_error(
    err: DaemonLocalAdminValidationError,
) -> DaemonRequestValidationError {
    match err {
        DaemonLocalAdminValidationError::BlankName { field } => {
            DaemonRequestValidationError::BlankField { field }
        }
        DaemonLocalAdminValidationError::UnsafeName { field, value } => {
            DaemonRequestValidationError::UnsafeLocalName { field, value }
        }
        DaemonLocalAdminValidationError::BlankClientRequestId => {
            DaemonRequestValidationError::BlankClientRequestId
        }
        DaemonLocalAdminValidationError::BlankAdministratorActor => {
            DaemonRequestValidationError::BlankField {
                field: "administrator_actor",
            }
        }
        DaemonLocalAdminValidationError::BlankConfirmationMarker => {
            DaemonRequestValidationError::BlankConfirmationMarker
        }
    }
}

fn generic_job_validation_error(err: DaemonJobValidationError) -> DaemonRequestValidationError {
    match err {
        DaemonJobValidationError::BlankCancellationReason => {
            DaemonRequestValidationError::BlankCancellationReason
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DaemonApiErrorResponse {
    pub code: String,
    pub message: String,
}

impl DaemonApiErrorResponse {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AssignLocalUserToLocalGroupRequest, CreateLocalGroupRequest, CreateObjectStoreRequest,
        DaemonApiRequest, DaemonEndpointKind, DaemonEndpointValidation,
        DaemonEndpointValidationState, DaemonIngestConflictPolicy, DaemonJobCancelRequest,
        DaemonJobId, DaemonJobListRequest, DaemonJobStatusRequest, DaemonServiceLifecycleRequest,
        DaemonServiceOperation, DaemonServiceProvisionRequest, DaemonServiceStatusRequest,
        ObjectBrowserPageRequest, ObjectBrowserRequest, ObjectBrowserSort,
        PrepareEnclosureFilesystem, PrepareEnclosureHddDevice, PrepareEnclosureRequest,
        StoreInventoryRequest, SubmitIngestFilesRequest, UpsertEndpointInventoryRequest,
        ENCLOSURE_PREPARE_CONFIRMATION, ENDPOINT_RECORD_CONFIRMATION,
        OBJECT_STORE_CREATE_CONFIRMATION,
    };
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_object_service::ObjectServiceProviderId;

    #[test]
    fn serializes_request_with_stable_command_name() {
        let request = DaemonApiRequest::StoreInventory(StoreInventoryRequest::default());

        let encoded = serde_json::to_value(request).expect("request serializes");

        assert_eq!(encoded["command"], "store_inventory");
    }

    #[test]
    fn delegates_submit_ingest_validation() {
        let request = DaemonApiRequest::SubmitIngestFiles(SubmitIngestFilesRequest {
            endpoint: StoreId::new("zymo").expect("store id"),
            source_path: "relative".into(),
            object_type: dasobjectstore_core::object_type::ObjectType::Naive,
            copies: None,
            conflict_policy: DaemonIngestConflictPolicy::Strict,
            dry_run: false,
            client_request_id: None,
        });

        assert!(request.validate().is_err());
    }

    #[test]
    fn service_commands_use_stable_command_names() {
        let status = DaemonApiRequest::ServiceStatus(DaemonServiceStatusRequest {
            include_detail: true,
        });
        let lifecycle = DaemonApiRequest::ServiceLifecycle(DaemonServiceLifecycleRequest {
            operation: DaemonServiceOperation::Start,
            provider_id: ObjectServiceProviderId::Garage,
            dry_run: true,
            client_request_id: None,
        });
        let provision = DaemonApiRequest::ServiceProvision(DaemonServiceProvisionRequest {
            provider_id: ObjectServiceProviderId::Garage,
            dry_run: true,
            client_request_id: None,
        });

        let status = serde_json::to_value(status).expect("status request serializes");
        let lifecycle = serde_json::to_value(lifecycle).expect("lifecycle request serializes");
        let provision = serde_json::to_value(provision).expect("provision request serializes");

        assert_eq!(status["command"], "service_status");
        assert_eq!(lifecycle["command"], "service_lifecycle");
        assert_eq!(lifecycle["payload"]["operation"], "start");
        assert_eq!(provision["command"], "service_provision");
    }

    #[test]
    fn generic_job_commands_use_stable_command_names() {
        let job_id = DaemonJobId::new("enclosure-prepare-1").expect("job id");
        let list = DaemonApiRequest::JobList(DaemonJobListRequest { limit: Some(25) });
        let status = DaemonApiRequest::JobStatus(DaemonJobStatusRequest {
            job_id: job_id.clone(),
        });
        let cancel = DaemonApiRequest::CancelJob(DaemonJobCancelRequest {
            job_id,
            reason: Some("operator requested cancellation".to_string()),
        });

        let list = serde_json::to_value(list).expect("list request serializes");
        let status = serde_json::to_value(status).expect("status request serializes");
        let cancel = serde_json::to_value(cancel).expect("cancel request serializes");

        assert_eq!(list["command"], "job_list");
        assert_eq!(list["payload"]["limit"], 25);
        assert_eq!(status["command"], "job_status");
        assert_eq!(cancel["command"], "cancel_job");
        assert_eq!(
            cancel["payload"]["reason"],
            "operator requested cancellation"
        );
    }

    #[test]
    fn local_admin_commands_use_stable_command_names() {
        let create = DaemonApiRequest::CreateLocalGroup(CreateLocalGroupRequest {
            group_name: "mnemosyne".to_string(),
            dry_run: true,
            client_request_id: Some("request-1".to_string()),
            administrator_actor: Some("operator".to_string()),
            confirmation_marker: "confirm create local group".to_string(),
        });
        let assign =
            DaemonApiRequest::AssignLocalUserToLocalGroup(AssignLocalUserToLocalGroupRequest {
                username: "stephen".to_string(),
                group_name: "mnemosyne".to_string(),
                dry_run: true,
                client_request_id: Some("request-2".to_string()),
                administrator_actor: Some("operator".to_string()),
                confirmation_marker: "confirm assign local user".to_string(),
            });

        let create = serde_json::to_value(create).expect("create request serializes");
        let assign = serde_json::to_value(assign).expect("assignment request serializes");

        assert_eq!(create["command"], "create_local_group");
        assert_eq!(assign["command"], "assign_local_user_to_local_group");
    }

    #[test]
    fn prepare_enclosure_command_uses_stable_command_name() {
        let request = DaemonApiRequest::PrepareEnclosure(PrepareEnclosureRequest {
            ssd_device: "/dev/disk/by-id/nvme-ssd".into(),
            hdd_devices: vec![PrepareEnclosureHddDevice {
                disk_id: "qnap-1057".to_string(),
                device_path: "/dev/disk/by-id/usb-qnap-1057".into(),
            }],
            mount_root: "/srv/dasobjectstore".into(),
            filesystem: PrepareEnclosureFilesystem::Ext4,
            owner: Some("stephen".to_string()),
            dry_run: false,
            client_request_id: Some("request-1".to_string()),
            administrator_actor: Some("operator".to_string()),
            allow_format: true,
            existing_data_acknowledged: true,
            confirmation_marker: ENCLOSURE_PREPARE_CONFIRMATION.to_string(),
        });

        let encoded = serde_json::to_value(request).expect("request serializes");

        assert_eq!(encoded["command"], "prepare_enclosure");
        assert_eq!(encoded["payload"]["filesystem"], "ext4");
        assert_eq!(encoded["payload"]["hdd_devices"][0]["disk_id"], "qnap-1057");
    }

    #[test]
    fn create_object_store_command_uses_stable_command_name() {
        let request = DaemonApiRequest::CreateObjectStore(CreateObjectStoreRequest {
            store_id: "generated-data".to_string(),
            store_class: "generated_data".to_string(),
            required_copies: 2,
            bucket: Some("generated-data".to_string()),
            writer_group: "bioinformatics".to_string(),
            ssd_root: "/srv/dasobjectstore/ssd".into(),
            object_type: "pod5".to_string(),
            enclosure_id: Some("qnap-tl-d800c-01".to_string()),
            public: false,
            writeable: true,
            capacity_behavior: "balanced".to_string(),
            retention: "standard".to_string(),
            endpoint_export_mode: "s3_bucket".to_string(),
            dry_run: false,
            client_request_id: Some("request-1".to_string()),
            administrator_actor: Some("stephen".to_string()),
            confirmation_marker: OBJECT_STORE_CREATE_CONFIRMATION.to_string(),
        });

        let encoded = serde_json::to_value(request).expect("request serializes");

        assert_eq!(encoded["command"], "create_object_store");
        assert_eq!(encoded["payload"]["store_id"], "generated-data");
        assert_eq!(encoded["payload"]["required_copies"], 2);
    }

    #[test]
    fn object_browser_command_uses_stable_command_name() {
        let request = DaemonApiRequest::ObjectBrowser(ObjectBrowserRequest {
            endpoint: StoreId::new("ena").expect("store id"),
            prefix: Some("ENA/Xenognostikon".to_string()),
            search: None,
            sort: ObjectBrowserSort::NameAsc,
            page: ObjectBrowserPageRequest::default(),
            include_placement: true,
        });

        request.validate().expect("request validates");
        let encoded = serde_json::to_value(request).expect("request serializes");

        assert_eq!(encoded["command"], "object_browser");
        assert_eq!(encoded["payload"]["prefix"], "ENA/Xenognostikon");
        assert_eq!(encoded["payload"]["page"]["limit"], 100);
    }

    #[test]
    fn endpoint_inventory_upsert_command_uses_stable_command_name() {
        let request = DaemonApiRequest::UpsertEndpointInventory(UpsertEndpointInventoryRequest {
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
            dry_run: false,
            client_request_id: Some("request-1".to_string()),
            administrator_actor: Some("stephen".to_string()),
            confirmation_marker: Some(ENDPOINT_RECORD_CONFIRMATION.to_string()),
        });

        let encoded = serde_json::to_value(request).expect("request serializes");

        assert_eq!(encoded["command"], "upsert_endpoint_inventory");
        assert_eq!(encoded["payload"]["endpoint_id"], "nas-staging");
        assert_eq!(encoded["payload"]["kind"], "dasobjectstore_nfs");
    }

    #[test]
    fn delegates_service_lifecycle_validation() {
        let request = DaemonApiRequest::ServiceLifecycle(DaemonServiceLifecycleRequest {
            operation: DaemonServiceOperation::Start,
            provider_id: ObjectServiceProviderId::Rustfs,
            dry_run: false,
            client_request_id: None,
        });

        assert!(request.validate().is_err());
    }

    #[test]
    fn delegates_service_provision_validation() {
        let request = DaemonApiRequest::ServiceProvision(DaemonServiceProvisionRequest {
            provider_id: ObjectServiceProviderId::Rustfs,
            dry_run: false,
            client_request_id: None,
        });

        assert!(request.validate().is_err());
    }

    #[test]
    fn delegates_local_admin_validation() {
        let request = DaemonApiRequest::CreateLocalGroup(CreateLocalGroupRequest {
            group_name: "Invalid Group".to_string(),
            dry_run: true,
            client_request_id: None,
            administrator_actor: None,
            confirmation_marker: "confirm create local group".to_string(),
        });

        assert!(request.validate().is_err());
    }

    #[test]
    fn delegates_prepare_enclosure_validation() {
        let request = DaemonApiRequest::PrepareEnclosure(PrepareEnclosureRequest {
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
        });

        assert!(request.validate().is_err());
    }
}
