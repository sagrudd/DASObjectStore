//! Shared Web API request and response contracts.

use super::*;

#[path = "api_admin_contracts.rs"]
mod admin;
pub use admin::*;

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApiError {
    pub message: String,
    pub status: Option<u16>,
}

#[cfg(target_arch = "wasm32")]
impl ApiError {
    pub fn is_permission_denied(&self) -> bool {
        matches!(self.status, Some(401 | 403))
    }

    pub fn is_transport_failure(&self) -> bool {
        self.status.is_none()
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ApiHealthResponse {
    pub service: String,
    pub status: String,
    pub version: String,
    #[serde(default)]
    pub instance_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct DashboardWarning {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct HomeDashboardResponse {
    pub schema_version: String,
    pub generated_at_utc: String,
    pub health: HealthSummaryResponse,
    pub drives: DriveCountSummaryResponse,
    pub capacity: CapacitySummaryResponse,
    pub mounted_enclosures: Vec<DasEnclosureCardResponse>,
    #[serde(default)]
    pub telemetry_window: TelemetryWindowControlResponse,
    pub throughput_7d: ThroughputSummaryResponse,
    #[serde(default)]
    pub disk_io: DiskIoSummaryResponse,
    #[serde(default)]
    pub cpu_usage: CpuUsageSummaryResponse,
    #[serde(default)]
    pub active_users: ActiveUsersSummaryResponse,
    #[serde(default)]
    pub ingest: Option<IngestQueueSummaryResponse>,
    #[serde(default)]
    pub destage: Option<DestageQueueSummaryResponse>,
    pub object_service: ObjectServiceStatusResponse,
    pub memory_stress: MemoryStressResponse,
    pub smart_warnings: SmartWarningsSummaryResponse,
    pub object_stores: Vec<ObjectStoreCardResponse>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct TelemetryWindowControlResponse {
    pub selected: String,
    pub selected_label: String,
    pub options: Vec<TelemetryWindowOptionResponse>,
}

impl Default for TelemetryWindowControlResponse {
    fn default() -> Self {
        Self {
            selected: "one_hour".to_string(),
            selected_label: "1 hour".to_string(),
            options: vec![
                TelemetryWindowOptionResponse {
                    value: "one_hour".to_string(),
                    label: "1 hour".to_string(),
                    selected: true,
                },
                TelemetryWindowOptionResponse {
                    value: "one_day".to_string(),
                    label: "1 day".to_string(),
                    selected: false,
                },
                TelemetryWindowOptionResponse {
                    value: "ten_days".to_string(),
                    label: "10 days".to_string(),
                    selected: false,
                },
                TelemetryWindowOptionResponse {
                    value: "three_months".to_string(),
                    label: "3 months".to_string(),
                    selected: false,
                },
            ],
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct TelemetryWindowOptionResponse {
    pub value: String,
    pub label: String,
    pub selected: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct EnclosuresPageResponse {
    pub schema_version: String,
    pub generated_at_utc: String,
    #[serde(default)]
    pub add_enclosure: AddEnclosureAffordanceResponse,
    pub enclosures: Vec<DasEnclosureCardResponse>,
    pub selected_enclosure_id: Option<String>,
    pub details: Option<DasEnclosureDetailResponse>,
    pub warnings: Vec<DashboardWarning>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct AddEnclosureAffordanceResponse {
    pub enabled: bool,
    pub action_kind: String,
    pub label: String,
    pub state: String,
    pub administrator: bool,
    pub supported_enclosure_detected: bool,
    pub daemon_ready: bool,
    pub confirmation_required: bool,
    pub blocked_reason: Option<String>,
    pub next_step: String,
}

impl AddEnclosureAffordanceResponse {
    pub fn checking() -> Self {
        Self {
            enabled: false,
            action_kind: "enclosure_add".to_string(),
            label: "Add enclosure".to_string(),
            state: "checking".to_string(),
            administrator: false,
            supported_enclosure_detected: false,
            daemon_ready: false,
            confirmation_required: true,
            blocked_reason: Some(
                "Checking administrator capability and daemon readiness.".to_string(),
            ),
            next_step: "Wait for the live enclosure inventory request to complete.".to_string(),
        }
    }
}

impl Default for AddEnclosureAffordanceResponse {
    fn default() -> Self {
        Self::checking()
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ObjectStoresPageResponse {
    pub schema_version: String,
    pub generated_at_utc: String,
    #[serde(default)]
    pub groups_file_path: Option<String>,
    #[serde(default)]
    pub groups: Vec<StorageGroupResponse>,
    #[serde(default)]
    pub mounted_enclosures: Vec<DasEnclosureCardResponse>,
    pub stores: Vec<ObjectStoreCardResponse>,
    pub selected_store_id: Option<String>,
    pub create_object_store: CreateObjectStoreAffordanceResponse,
    pub warnings: Vec<DashboardWarning>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct RemoteUploadWorkspaceResponse {
    pub schema_version: String,
    pub generated_at_utc: String,
    pub actor: RemoteUploadActorResponse,
    pub stores: Vec<RemoteUploadObjectStoreResponse>,
    pub warnings: Vec<DashboardWarning>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct RemoteUploadActorResponse {
    pub username: String,
    pub groups: Vec<String>,
    pub sudo_administrator: bool,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct RemoteUploadObjectStoreResponse {
    pub store_id: String,
    pub display_name: String,
    pub bucket: String,
    pub store_class: String,
    pub object_type: String,
    pub capacity: CapacitySummaryResponse,
    pub writer_group: Option<String>,
    pub writer_policy_state: String,
    pub public: bool,
    pub endpoint_export_mode: String,
    pub upload_allowed: bool,
    pub upload_state: String,
    pub upload_message: String,
    pub warnings: Vec<DashboardWarning>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct StorageGroupResponse {
    pub group_name: String,
    pub display_name: String,
    pub source: String,
    pub current_user_member: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct UsersGroupsWorkspaceResponse {
    pub host_mode: String,
    pub authentication_framework: ProsopikonAuthenticationFramework,
    pub device_token_requirement: ProsopikonDeviceTokenRequirement,
    pub current_user: Option<LocalUserAuthorityResponse>,
    pub users: Vec<StandaloneUserAccountResponse>,
    pub groups: Vec<LocalGroupMembershipResponse>,
    pub groups_file_path: String,
    pub writer_groups: Vec<StorageGroupResponse>,
    pub operations: Vec<LocalGroupOperationResponse>,
    pub capabilities: UsersGroupsCapabilitiesResponse,
    pub selected_username: Option<String>,
    pub selected_group_name: Option<String>,
    pub warnings: Vec<DashboardWarning>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct LocalUserAuthorityResponse {
    pub username: String,
    pub groups: Vec<String>,
    pub sudo_administrator: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct StandaloneUserAccountResponse {
    pub username: String,
    pub registered: bool,
    pub created_at_unix_seconds: i64,
    pub registered_at_unix_seconds: Option<i64>,
    pub active_session_count: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct LocalGroupMembershipResponse {
    pub group_name: String,
    pub current_user_member: bool,
    pub sudo_administrator_group: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct LocalGroupOperationResponse {
    pub kind: String,
    pub label: String,
    pub requires_sudo_administrator: bool,
    pub enabled: bool,
    pub blocked_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct UsersGroupsCapabilitiesResponse {
    pub product_local_user_registration: bool,
    pub os_local_user_management: bool,
    pub os_local_group_management: bool,
    pub administrator_actions_enabled: bool,
}

#[cfg(any(target_arch = "wasm32", test))]
#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CreateLocalGroupRequest {
    pub group_name: String,
    pub dry_run: bool,
    pub confirmation_marker: Option<String>,
    pub client_request_id: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AssignLocalUserToGroupRequest {
    pub group_name: String,
    pub username: String,
    pub dry_run: bool,
    pub confirmation_marker: Option<String>,
    pub client_request_id: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct LocalGroupAdminResponse {
    pub accepted: LocalGroupAdminAcceptedResponse,
    pub operation: String,
    pub group_name: String,
    pub username: Option<String>,
    pub client_request_id: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct LocalGroupAdminAcceptedResponse {
    pub job_id: String,
    pub kind: String,
    pub accepted_at_utc: String,
    pub dry_run: bool,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct BioinformaticsWorkspaceResponse {
    pub schema_version: String,
    pub available: bool,
    pub supported_object_types: Vec<String>,
    #[serde(default)]
    pub readiness_cards: Vec<BioinformaticsReadinessCardResponse>,
    #[serde(default)]
    pub derivation_sources: Vec<BioinformaticsDerivationSourceResponse>,
    #[serde(default)]
    pub sequencing_runs: Vec<BioinformaticsContextCardResponse>,
    #[serde(default)]
    pub object_lineage: Vec<BioinformaticsContextCardResponse>,
    #[serde(default)]
    pub workflow_handoffs: Vec<BioinformaticsContextCardResponse>,
    #[serde(default)]
    pub governance_bindings: Vec<BioinformaticsContextCardResponse>,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[allow(dead_code)]
pub struct BioinformaticsReadinessCardResponse {
    pub object_type: String,
    pub label: String,
    pub category: String,
    pub state: String,
    pub primary_workflow: String,
    pub handoff: String,
    #[serde(default)]
    pub required_metadata: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[allow(dead_code)]
pub struct BioinformaticsContextCardResponse {
    pub label: String,
    pub state: String,
    pub summary: String,
    pub detail: String,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[allow(dead_code)]
pub struct BioinformaticsDerivationSourceResponse {
    pub source_kind: String,
    pub source_id: String,
    pub display_name: String,
    pub object_type: String,
    pub parent_id: Option<String>,
    pub endpoint_export_mode: Option<String>,
    pub mneion_binding_state: String,
    pub governance_domain: Option<String>,
    #[serde(default)]
    pub workflow_roles: Vec<String>,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct HealthSummaryResponse {
    pub state: String,
    pub label: String,
    pub warning_count: usize,
    pub critical_count: usize,
    pub action_count: usize,
    pub last_checked_at_utc: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct DriveCountSummaryResponse {
    pub total: usize,
    pub mounted: usize,
    pub healthy: usize,
    pub watch: usize,
    pub suspect: usize,
    pub failed: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct CapacitySummaryResponse {
    pub total_tib: String,
    pub used_tib: String,
    pub free_tib: String,
    pub used_percent_basis_points: u16,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct DasEnclosureCardResponse {
    pub enclosure_id: String,
    pub display_name: String,
    pub mount_path: String,
    pub connection: EnclosureConnectionResponse,
    pub health: String,
    pub drive_count: DriveCountSummaryResponse,
    pub capacity: CapacitySummaryResponse,
    pub last_seen_at_utc: String,
    pub warnings: Vec<DashboardWarning>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct EnclosureConnectionResponse {
    pub bus: String,
    pub protocol: String,
    pub link_speed: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct DasEnclosureDetailResponse {
    pub enclosure_id: String,
    pub vendor: String,
    pub model: String,
    pub serial: String,
    pub firmware: Option<String>,
    pub slots: Vec<EnclosureDriveSlotResponse>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct EnclosureDriveSlotResponse {
    pub slot_number: u8,
    pub drive_id: String,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub mount_path: Option<String>,
    #[serde(default)]
    pub device_path: Option<String>,
    #[serde(default)]
    pub filesystem: Option<String>,
    pub size_tib: String,
    pub health: String,
    pub mounted: bool,
    #[serde(default)]
    pub smart_warning_count: usize,
    #[serde(default)]
    pub actions_available: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ThroughputSummaryResponse {
    pub window_days: u8,
    pub read_tib: String,
    pub written_tib: String,
    pub ingest_tib: String,
    pub avg_read_mib_s: u32,
    pub avg_write_mib_s: u32,
    #[serde(default)]
    pub daily: Vec<ThroughputDayResponse>,
    #[serde(default = "default_throughput_source")]
    pub source: String,
    #[serde(default)]
    pub message: Option<String>,
}

fn default_throughput_source() -> String {
    "unavailable".to_string()
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ThroughputDayResponse {
    pub date: String,
    pub read_tib: String,
    pub written_tib: String,
    pub ingest_tib: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct DiskIoSummaryResponse {
    pub available: bool,
    pub read_mib_s: u32,
    pub write_mib_s: u32,
    pub read_ops_s: u32,
    pub write_ops_s: u32,
    pub busiest_disk_id: Option<String>,
    #[serde(default)]
    pub sample_timestamp_utc: Option<String>,
    #[serde(default)]
    pub sample_age_seconds: Option<u64>,
    #[serde(default)]
    pub per_disk: Vec<DiskIoDeviceResponse>,
    #[serde(default)]
    pub collection_quality: Option<String>,
    #[serde(default)]
    pub missing_data: Vec<DiskIoMissingDataResponse>,
    pub state: String,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct DiskIoDeviceResponse {
    pub disk_id: String,
    pub label: Option<String>,
    pub mount_path: String,
    pub role: String,
    pub enclosure_id: Option<String>,
    pub bay_label: Option<String>,
    pub device_path: Option<String>,
    pub device_name: Option<String>,
    pub read_mib_s: Option<u32>,
    pub write_mib_s: Option<u32>,
    pub read_ops_s: Option<u32>,
    pub write_ops_s: Option<u32>,
    pub missing_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct DiskIoMissingDataResponse {
    pub path: String,
    pub reason: String,
    pub detail: Option<String>,
}

impl Default for DiskIoSummaryResponse {
    fn default() -> Self {
        Self {
            available: false,
            read_mib_s: 0,
            write_mib_s: 0,
            read_ops_s: 0,
            write_ops_s: 0,
            busiest_disk_id: None,
            sample_timestamp_utc: None,
            sample_age_seconds: None,
            per_disk: Vec::new(),
            collection_quality: None,
            missing_data: Vec::new(),
            state: "unavailable".to_string(),
            message: Some("Disk IO telemetry is not available yet.".to_string()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct CpuUsageSummaryResponse {
    pub available: bool,
    pub usage_percent: Option<u8>,
    pub load_average_1m: Option<String>,
    pub logical_core_count: Option<u64>,
    pub state: String,
    pub message: Option<String>,
}

impl Default for CpuUsageSummaryResponse {
    fn default() -> Self {
        Self {
            available: false,
            usage_percent: None,
            load_average_1m: None,
            logical_core_count: None,
            state: "unavailable".to_string(),
            message: Some("CPU telemetry is not available yet.".to_string()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ActiveUsersSummaryResponse {
    pub available: bool,
    pub active_sessions: u64,
    pub distinct_logged_in_users: u64,
    pub administrator_sessions: u64,
    pub operator_sessions: u64,
    pub remote_agent_sessions: u64,
    pub state: String,
    pub message: Option<String>,
}

impl Default for ActiveUsersSummaryResponse {
    fn default() -> Self {
        Self {
            available: false,
            active_sessions: 0,
            distinct_logged_in_users: 0,
            administrator_sessions: 0,
            operator_sessions: 0,
            remote_agent_sessions: 0,
            state: "unavailable".to_string(),
            message: Some("Session telemetry is not available yet.".to_string()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ObjectServiceStatusResponse {
    pub active: bool,
    pub remote_ready: bool,
    pub bind_address: String,
    pub port: u16,
    pub local_url: String,
    pub remote_url: Option<String>,
    pub service_state: Option<String>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct IngestQueueSummaryResponse {
    pub pressure: String,
    pub queued_jobs: usize,
    pub active_jobs: usize,
    pub failed_jobs: usize,
    #[serde(default)]
    pub warnings: Vec<DashboardWarning>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct DestageQueueSummaryResponse {
    pub pending_objects: usize,
    pub copying_objects: usize,
    pub verified_objects: usize,
    #[serde(default)]
    pub warnings: Vec<DashboardWarning>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ActivityWorkspaceResponse {
    #[serde(default)]
    pub ingest: Option<IngestQueueSummaryResponse>,
    #[serde(default)]
    pub destage: Option<DestageQueueSummaryResponse>,
    #[serde(default)]
    pub categories: Vec<ActivityCategoryResponse>,
    #[serde(default)]
    pub tasks: Vec<ActivityTaskResponse>,
    #[serde(default)]
    pub warnings: Vec<DashboardWarning>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ActivityCategoryResponse {
    pub kind: String,
    pub label: String,
    pub description: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ActivityTaskResponse {
    pub task_id: String,
    pub kind: String,
    pub state: String,
    pub label: String,
    #[serde(default)]
    pub progress: Option<ActivityTaskProgressResponse>,
    pub updated_at_utc: String,
    #[serde(default)]
    pub warnings: Vec<DashboardWarning>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ActivityTaskProgressResponse {
    pub stage: String,
    pub work_bytes_done: u64,
    pub work_bytes_total: u64,
    pub work_units_done: u64,
    pub work_units_total: u64,
    pub percent_complete: Option<u8>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[allow(dead_code)]
pub struct EndpointsWorkspaceResponse {
    pub inventory: EndpointInventoryResponse,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[allow(dead_code)]
pub struct EndpointInventoryResponse {
    pub schema_version: String,
    pub endpoint_count: usize,
    pub degraded_endpoint_count: usize,
    pub binding_count: usize,
    pub endpoints: Vec<EndpointInventoryItemResponse>,
    pub warnings: Vec<EndpointWarningResponse>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[allow(dead_code)]
pub struct EndpointInventoryItemResponse {
    pub endpoint_id: String,
    pub display_name: String,
    pub kind: String,
    pub manager_product_id: String,
    pub object_service_url: String,
    pub validation: EndpointValidationResponse,
    pub active_bindings: Vec<EndpointBindingResponse>,
    pub warnings: Vec<EndpointWarningResponse>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[allow(dead_code)]
pub struct EndpointValidationResponse {
    pub state: String,
    pub checked_at_utc: Option<String>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[allow(dead_code)]
pub struct EndpointBindingResponse {
    pub binding_id: String,
    pub governance_domain: String,
    pub store_id: String,
    pub readiness: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[allow(dead_code)]
pub struct EndpointWarningResponse {
    pub code: String,
    pub severity: String,
    pub endpoint_id: String,
    pub binding_id: Option<String>,
    pub message: String,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[allow(dead_code)]
pub struct EndpointInventoryUpsertRequest {
    pub endpoint_id: String,
    pub display_name: String,
    pub kind: String,
    pub object_service_url: String,
    pub validation: EndpointValidationUpsertRequest,
    pub manager_product_id: String,
    pub active_bindings: Vec<EndpointBindingUpsertRequest>,
    pub dry_run: bool,
    pub client_request_id: Option<String>,
    pub confirmation_marker: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[allow(dead_code)]
pub struct EndpointValidationUpsertRequest {
    pub state: String,
    pub checked_at_utc: Option<String>,
    pub message: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[allow(dead_code)]
pub struct EndpointBindingUpsertRequest {
    pub binding_id: String,
    pub governance_domain: String,
    pub store_id: String,
    pub readiness: String,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct EndpointInventoryUpsertResponse {
    pub accepted: EndpointInventoryAcceptedResponse,
    pub endpoint_id: String,
    pub display_name: String,
    pub kind: String,
    pub validation_state: String,
    pub registry_path: String,
    pub administrator_actor: Option<String>,
    pub client_request_id: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct EndpointInventoryAcceptedResponse {
    pub job_id: String,
    pub kind: String,
    pub accepted_at_utc: String,
    pub dry_run: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct MemoryStressResponse {
    pub state: String,
    pub pressure_percent: u8,
    pub swap_used_percent: u8,
    pub page_cache_tib: String,
    pub warning: Option<DashboardWarning>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct SmartWarningsSummaryResponse {
    pub warning_count: usize,
    pub affected_drive_count: usize,
    pub warnings: Vec<SmartWarningResponse>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct SmartWarningResponse {
    pub drive_id: String,
    pub severity: String,
    pub attribute: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ObjectStoreCardResponse {
    pub store_id: String,
    pub display_name: String,
    pub store_class: Option<String>,
    pub object_type: Option<String>,
    pub health: String,
    pub required_copies: Option<u8>,
    pub object_count: usize,
    pub capacity: Option<CapacitySummaryResponse>,
    pub placement_policy: Option<String>,
    pub endpoint_export_mode: Option<String>,
    #[serde(default)]
    pub ingest_mode: Option<String>,
    pub writer_group: Option<String>,
    pub public: Option<bool>,
    pub writeable: Option<bool>,
    pub created_at_utc: Option<String>,
    pub last_ingested_at_utc: Option<String>,
    #[serde(default)]
    pub writer_policy: Option<WriterPolicyReadinessResponse>,
    pub warnings: Vec<DashboardWarning>,
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ObjectBrowserResponse {
    pub endpoint: String,
    pub prefix: String,
    pub breadcrumbs: Vec<ObjectBrowserBreadcrumbResponse>,
    pub folders: Vec<ObjectBrowserFolderNodeResponse>,
    pub files: Vec<ObjectBrowserFileNodeResponse>,
    pub next_cursor: Option<String>,
    pub total_entries: Option<u64>,
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ObjectBrowserBreadcrumbResponse {
    pub name: String,
    pub prefix: String,
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ObjectBrowserFolderNodeResponse {
    pub name: String,
    pub prefix: String,
    pub object_count: Option<u64>,
    pub total_size_bytes: Option<u64>,
    pub readiness: String,
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ObjectBrowserFileNodeResponse {
    pub object_id: String,
    pub name: String,
    pub path: String,
    pub object_type: String,
    pub size_bytes: u64,
    pub modified_at_utc: Option<String>,
    pub checksum: Option<ObjectBrowserChecksumResponse>,
    pub readiness: String,
    pub lifecycle_state: String,
    pub copy_count: u16,
    pub placements: Vec<ObjectBrowserPlacementResponse>,
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ObjectBrowserChecksumResponse {
    pub algorithm: String,
    pub value: String,
    pub verified_at_utc: Option<String>,
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct ObjectBrowserPlacementResponse {
    pub disk_id: Option<String>,
    pub disk_label: Option<String>,
    pub location: String,
    pub state: String,
    pub size_bytes: u64,
    pub checksum: Option<ObjectBrowserChecksumResponse>,
    pub verified_at_utc: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct WriterPolicyReadinessResponse {
    pub writer_group: Option<String>,
    pub group_defined: bool,
    pub current_user_member: bool,
    pub writeable_by_current_user: bool,
    pub state: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct CreateObjectStoreAffordanceResponse {
    pub enabled: bool,
    pub action_kind: String,
    pub label: String,
    pub required_fields: Vec<CreateObjectStoreFieldResponse>,
    pub optional_fields: Vec<CreateObjectStoreFieldResponse>,
    pub defaults: CreateObjectStoreDefaultsResponse,
    pub store_class_options: Vec<StoreClassOptionResponse>,
    pub copy_count_options: Vec<u8>,
    pub confirmation_required: bool,
    pub blocked_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct CreateObjectStoreFieldResponse {
    pub name: String,
    pub label: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct CreateObjectStoreDefaultsResponse {
    pub store_class: String,
    pub required_copies: u8,
    pub endpoint_export_mode: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct StoreClassOptionResponse {
    pub value: String,
    pub label: String,
    pub description: String,
}

#[cfg(target_arch = "wasm32")]
impl From<gloo_net::Error> for ApiError {
    fn from(err: gloo_net::Error) -> Self {
        Self {
            message: format!("DASObjectStore server request failed: {err}"),
            status: None,
        }
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct LoginResponse {
    pub username: String,
    pub session_token: String,
    pub expires_at_unix_seconds: i64,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct LogoutResponse {
    pub username: String,
    pub disconnected: bool,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct SessionCheckResponse {
    pub username: String,
    pub valid: bool,
    pub expires_at_unix_seconds: i64,
}
