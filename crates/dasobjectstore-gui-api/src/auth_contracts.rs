use dasobjectstore_daemon::RemoteEasyconnectAuthProvider;
use dasobjectstore_daemon::RemoteEasyconnectSession;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RegisterRequest {
    pub username: String,
    pub token: String,
    pub password: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    pub session_ttl_seconds: Option<i64>,
}

/// Password-authenticated remote ObjectStore access request.
///
/// The password is accepted only for the duration of the request and is never
/// persisted. The daemon returns a store-scoped temporary S3 session.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteAuthenticateRequest {
    pub username: String,
    pub password: String,
    pub object_store: String,
    pub requested_session_lifetime_seconds: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RemoteAuthenticateResponse {
    pub schema_version: String,
    pub endpoint_port: u16,
    pub region: String,
    pub addressing_style: String,
    pub object_store: String,
    pub bucket: String,
    pub session: RemoteEasyconnectSession,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LogoutRequest {
    pub username: String,
    pub session_token: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionCheckRequest {
    pub username: String,
    pub session_token: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StandaloneEasyconnectAuthContextResponse {
    pub schema_version: String,
    pub auth_provider: RemoteEasyconnectAuthProvider,
    pub subject_id: String,
    pub session_expires_at_unix_seconds: Option<i64>,
    pub supported_auth_providers: Vec<RemoteEasyconnectAuthProvider>,
    pub future_auth_providers: Vec<RemoteEasyconnectAuthProvider>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthRouteError {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CreateLocalGroupRequest {
    #[serde(alias = "group")]
    pub group_name: String,
    #[serde(default)]
    pub dry_run: bool,
    pub confirmation_marker: Option<String>,
    pub client_request_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AssignLocalUserToGroupRequest {
    #[serde(alias = "group")]
    pub group_name: String,
    #[serde(alias = "user")]
    pub username: String,
    #[serde(default)]
    pub dry_run: bool,
    pub confirmation_marker: Option<String>,
    pub client_request_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PrepareEnclosureRequest {
    pub ssd_device: String,
    #[serde(default)]
    pub hdd_devices: Vec<PrepareEnclosureHddDeviceRequest>,
    pub mount_root: Option<String>,
    pub filesystem: Option<String>,
    pub owner: Option<String>,
    #[serde(default)]
    pub dry_run: bool,
    pub client_request_id: Option<String>,
    #[serde(default)]
    pub allow_format: bool,
    #[serde(default)]
    pub existing_data_acknowledged: bool,
    pub confirmation_marker: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PrepareEnclosureHddDeviceRequest {
    pub disk_id: String,
    pub device_path: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CreateObjectStoreRequest {
    pub store_id: String,
    #[serde(default)]
    pub store_class: Option<String>,
    pub required_copies: u8,
    pub bucket: Option<String>,
    #[serde(default)]
    pub reader_group: Option<String>,
    pub writer_group: String,
    #[serde(default)]
    pub ssd_root: Option<String>,
    #[serde(default)]
    pub object_type: Option<String>,
    pub enclosure_id: Option<String>,
    #[serde(default)]
    pub public: bool,
    #[serde(default)]
    pub writeable: Option<bool>,
    #[serde(default)]
    pub capacity_behavior: Option<String>,
    #[serde(default)]
    pub retention: Option<String>,
    #[serde(default)]
    pub endpoint_export_mode: Option<String>,
    #[serde(default)]
    pub dry_run: bool,
    pub client_request_id: Option<String>,
    pub confirmation_marker: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ObjectStoreIngestPolicyRequest {
    pub store_id: String,
    pub ingest_mode: String,
    #[serde(default)]
    pub dry_run: bool,
    pub client_request_id: Option<String>,
    pub confirmation_marker: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IngestControlAction {
    Pause,
    Throttle,
    Resume,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IngestControlRequest {
    pub action: IngestControlAction,
    pub reason: String,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub confirmation_marker: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IngestControlResponse {
    pub state: String,
    pub changed: bool,
    pub dry_run: bool,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StandaloneObjectStoreIngestPolicyResponse {
    pub job_id: String,
    pub store_id: String,
    pub previous_ingest_mode: String,
    pub ingest_mode: String,
    pub changed: bool,
    pub dry_run: bool,
    pub administrator_actor: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EndpointInventoryUpsertRequest {
    pub endpoint_id: String,
    pub display_name: String,
    pub kind: String,
    pub object_service_url: String,
    pub validation: EndpointValidationUpsertRequest,
    #[serde(default = "default_manager_product_id")]
    pub manager_product_id: String,
    #[serde(default)]
    pub active_bindings: Vec<EndpointBindingUpsertRequest>,
    #[serde(default)]
    pub dry_run: bool,
    pub client_request_id: Option<String>,
    pub confirmation_marker: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EndpointValidationUpsertRequest {
    pub state: String,
    pub checked_at_utc: Option<String>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EndpointBindingUpsertRequest {
    pub binding_id: String,
    pub governance_domain: String,
    pub store_id: String,
    pub readiness: String,
}

fn default_manager_product_id() -> String {
    "dasobjectstore".to_string()
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StandaloneLocalGroupOperation {
    CreateGroup,
    AddUserToGroup,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StandaloneLocalGroupAdminResponse {
    pub accepted: StandaloneLocalGroupAdminAcceptedResponse,
    pub operation: StandaloneLocalGroupOperation,
    pub group_name: String,
    pub username: Option<String>,
    pub client_request_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StandaloneLocalGroupAdminAcceptedResponse {
    pub job_id: String,
    pub kind: String,
    pub accepted_at_utc: String,
    pub dry_run: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StandaloneEnclosurePrepareResponse {
    pub accepted: StandaloneEnclosurePrepareAcceptedResponse,
    pub ssd_device: String,
    pub hdd_devices: Vec<PrepareEnclosureHddDeviceRequest>,
    pub mount_root: String,
    pub filesystem: String,
    pub owner: Option<String>,
    pub administrator_actor: Option<String>,
    pub client_request_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StandaloneEnclosurePrepareAcceptedResponse {
    pub job_id: String,
    pub kind: String,
    pub accepted_at_utc: String,
    pub dry_run: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StandaloneCreateObjectStoreResponse {
    pub accepted: StandaloneCreateObjectStoreAcceptedResponse,
    pub store_id: String,
    pub store_class: String,
    pub required_copies: u8,
    pub bucket: Option<String>,
    pub reader_group: Option<String>,
    pub writer_group: String,
    pub ssd_root: String,
    pub object_type: String,
    pub enclosure_id: Option<String>,
    pub public: bool,
    pub writeable: bool,
    pub capacity_behavior: String,
    pub retention: String,
    pub endpoint_export_mode: String,
    pub administrator_actor: Option<String>,
    pub client_request_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StandaloneCreateObjectStoreAcceptedResponse {
    pub job_id: String,
    pub kind: String,
    pub accepted_at_utc: String,
    pub dry_run: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StandaloneEndpointInventoryUpsertResponse {
    pub accepted: StandaloneEndpointInventoryAcceptedResponse,
    pub endpoint_id: String,
    pub display_name: String,
    pub kind: String,
    pub validation_state: String,
    pub registry_path: String,
    pub administrator_actor: Option<String>,
    pub client_request_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StandaloneEndpointInventoryAcceptedResponse {
    pub job_id: String,
    pub kind: String,
    pub accepted_at_utc: String,
    pub dry_run: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CancelAdminJobRequest {
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StandaloneAdminJobStatusResponse {
    pub job: StandaloneAdminJobSummary,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StandaloneAdminJobSummary {
    pub job_id: String,
    pub kind: String,
    pub state: String,
    pub progress: StandaloneAdminJobProgress,
    pub percent_complete: Option<u8>,
    pub submitted_at_utc: String,
    pub updated_at_utc: String,
    pub actor: Option<String>,
    pub failure_message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StandaloneAdminJobProgress {
    pub stage: String,
    pub work_bytes_done: u64,
    pub work_bytes_total: u64,
    pub work_units_done: u64,
    pub work_units_total: u64,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StandaloneAdminJobCancelResponse {
    pub job_id: String,
    pub accepted: bool,
    pub state: String,
}
