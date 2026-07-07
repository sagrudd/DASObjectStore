//! Transport-neutral daemon API contracts.

mod health;
mod ingest;
mod jobs;
mod local_admin;
mod service;
mod stores;

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
    DaemonJobId, DaemonJobIdError, DaemonJobKind, DaemonJobProgress, DaemonJobState,
    DaemonJobStatusRequest, DaemonJobStatusResponse, DaemonJobSummary, DaemonJobValidationError,
};
pub use local_admin::{
    AssignLocalUserToLocalGroupRequest, AssignLocalUserToLocalGroupResponse,
    CreateLocalGroupRequest, CreateLocalGroupResponse, DaemonLocalAdminAcceptedResponse,
    DaemonLocalAdminCommand, DaemonLocalAdminValidationError,
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
    ServiceStatus(DaemonServiceStatusRequest),
    ServiceLifecycle(DaemonServiceLifecycleRequest),
    ServiceProvision(DaemonServiceProvisionRequest),
    CreateLocalGroup(CreateLocalGroupRequest),
    AssignLocalUserToLocalGroup(AssignLocalUserToLocalGroupRequest),
}

impl DaemonApiRequest {
    pub fn validate(&self) -> Result<(), DaemonRequestValidationError> {
        match self {
            Self::SubmitIngestFiles(request) => request.validate(),
            Self::CancelIngestJob(request) => request.validate(),
            Self::ServiceLifecycle(request) => request.validate(),
            Self::ServiceProvision(request) => request.validate(),
            Self::CreateLocalGroup(request) => {
                request.validate().map_err(local_admin_validation_error)
            }
            Self::AssignLocalUserToLocalGroup(request) => {
                request.validate().map_err(local_admin_validation_error)
            }
            Self::HealthSummary(_)
            | Self::StoreInventory(_)
            | Self::IngestJobStatus(_)
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
    ServiceStatus(DaemonServiceStatusResponse),
    ServiceLifecycle(DaemonServiceLifecycleResponse),
    ServiceProvision(DaemonServiceProvisionResponse),
    CreateLocalGroup(CreateLocalGroupResponse),
    AssignLocalUserToLocalGroup(AssignLocalUserToLocalGroupResponse),
    IngestProgress(DaemonIngestProgressEvent),
    Error(DaemonApiErrorResponse),
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
        AssignLocalUserToLocalGroupRequest, CreateLocalGroupRequest, DaemonApiRequest,
        DaemonIngestConflictPolicy, DaemonServiceLifecycleRequest, DaemonServiceOperation,
        DaemonServiceProvisionRequest, DaemonServiceStatusRequest, StoreInventoryRequest,
        SubmitIngestFilesRequest,
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
}
