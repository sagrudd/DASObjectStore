#[cfg(target_arch = "wasm32")]
use gloo_net::http::Request;
use serde::Deserialize;
#[cfg(any(target_arch = "wasm32", test))]
use serde::Serialize;

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
    pub throughput_7d: ThroughputSummaryResponse,
    #[serde(default)]
    pub ingest: Option<IngestQueueSummaryResponse>,
    #[serde(default)]
    pub destage: Option<DestageQueueSummaryResponse>,
    pub memory_stress: MemoryStressResponse,
    pub smart_warnings: SmartWarningsSummaryResponse,
    pub object_stores: Vec<ObjectStoreCardResponse>,
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

#[cfg(any(target_arch = "wasm32", test))]
#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct GuiActionPlanResponse {
    pub action: String,
    pub execution: String,
    pub argv: Vec<String>,
    pub mutates_pool: bool,
    pub writes_recovery_metadata: bool,
    pub confirmation_required: bool,
}

#[cfg(any(target_arch = "wasm32", test))]
#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct GuiActionPlanRequest {
    pub action: String,
    pub store_id: Option<String>,
    pub store_class: Option<String>,
    pub store_copies: Option<u8>,
    pub bucket: Option<String>,
    pub writer_group: Option<String>,
    pub ssd_root: Option<String>,
    pub public: Option<bool>,
    pub writeable: Option<bool>,
    pub capacity_behavior: Option<String>,
    pub retention: Option<String>,
    pub endpoint_export_mode: Option<String>,
    pub subobject_name: Option<String>,
    pub parent_store_id: Option<String>,
    pub parent_subobject_name: Option<String>,
    pub subobject_object_type: Option<String>,
    pub subobject_inherits_object_type: Option<bool>,
    pub subobject_s3_routing: Option<String>,
    pub ssd_device: Option<String>,
    pub hdd_devices: Vec<String>,
    pub mount_root: Option<String>,
    pub filesystem: Option<String>,
    pub owner: Option<String>,
    pub allow_format: bool,
    pub existing_data_acknowledged: bool,
    pub confirmation_phrase: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct CreateObjectStoreResponse {
    pub accepted: CreateObjectStoreAcceptedResponse,
    pub store_id: String,
    pub store_class: String,
    pub required_copies: u8,
    pub bucket: Option<String>,
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

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct CreateObjectStoreAcceptedResponse {
    pub job_id: String,
    pub kind: String,
    pub accepted_at_utc: String,
    pub dry_run: bool,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CreateObjectStoreRequest {
    pub store_id: String,
    pub store_class: String,
    pub required_copies: u8,
    pub bucket: Option<String>,
    pub writer_group: String,
    pub ssd_root: String,
    pub object_type: String,
    pub enclosure_id: Option<String>,
    pub public: bool,
    pub writeable: bool,
    pub capacity_behavior: String,
    pub retention: String,
    pub endpoint_export_mode: String,
    pub dry_run: bool,
    pub client_request_id: Option<String>,
    pub confirmation_marker: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct EnclosurePrepareResponse {
    pub accepted: EnclosurePrepareAcceptedResponse,
    pub ssd_device: String,
    pub hdd_devices: Vec<EnclosurePrepareHddDevice>,
    pub mount_root: String,
    pub filesystem: String,
    pub owner: Option<String>,
    pub administrator_actor: Option<String>,
    pub client_request_id: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct EnclosurePrepareAcceptedResponse {
    pub job_id: String,
    pub kind: String,
    pub accepted_at_utc: String,
    pub dry_run: bool,
}

#[cfg(any(target_arch = "wasm32", test))]
#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct AdminJobStatusResponse {
    pub job: AdminJobSummary,
}

#[cfg(any(target_arch = "wasm32", test))]
#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct AdminJobSummary {
    pub job_id: String,
    pub kind: String,
    pub state: String,
    pub progress: AdminJobProgress,
    pub percent_complete: Option<u8>,
    pub submitted_at_utc: String,
    pub updated_at_utc: String,
    pub actor: Option<String>,
    pub failure_message: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[allow(dead_code)]
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct AdminJobProgress {
    pub stage: String,
    pub work_bytes_done: u64,
    pub work_bytes_total: u64,
    pub work_units_done: u64,
    pub work_units_total: u64,
    pub message: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct AdminJobCancelResponse {
    pub job_id: String,
    pub accepted: bool,
    pub state: String,
}

#[cfg(target_arch = "wasm32")]
#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AdminJobCancelRequest {
    pub reason: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EnclosurePrepareHddDevice {
    pub disk_id: String,
    pub device_path: String,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct EnclosurePrepareRequest {
    pub ssd_device: String,
    pub hdd_devices: Vec<EnclosurePrepareHddDevice>,
    pub mount_root: Option<String>,
    pub filesystem: Option<String>,
    pub owner: Option<String>,
    pub dry_run: bool,
    pub client_request_id: Option<String>,
    pub allow_format: bool,
    pub existing_data_acknowledged: bool,
    pub confirmation_marker: Option<String>,
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
    pub stores: Vec<ObjectStoreCardResponse>,
    pub selected_store_id: Option<String>,
    pub create_object_store: CreateObjectStoreAffordanceResponse,
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
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct BioinformaticsWorkspaceResponse {
    pub schema_version: String,
    pub available: bool,
    pub supported_object_types: Vec<String>,
    pub message: String,
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
    pub writer_group: Option<String>,
    pub public: Option<bool>,
    pub writeable: Option<bool>,
    pub created_at_utc: Option<String>,
    pub last_ingested_at_utc: Option<String>,
    #[serde(default)]
    pub writer_policy: Option<WriterPolicyReadinessResponse>,
    pub warnings: Vec<DashboardWarning>,
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

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Deserialize)]
struct ErrorResponse {
    message: String,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Serialize)]
struct LoginRequest {
    username: String,
    password: String,
    session_ttl_seconds: Option<i64>,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Serialize)]
struct LogoutRequest {
    username: String,
    session_token: String,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Debug, Serialize)]
struct SessionCheckRequest {
    username: String,
    session_token: String,
}

#[cfg(target_arch = "wasm32")]
pub async fn login(
    auth_base_path: &str,
    username: String,
    password: String,
) -> Result<LoginResponse, ApiError> {
    post_json(
        &auth_path(auth_base_path, "login"),
        &LoginRequest {
            username,
            password,
            session_ttl_seconds: None,
        },
    )
    .await
}

#[cfg(target_arch = "wasm32")]
pub async fn logout(
    auth_base_path: &str,
    username: String,
    session_token: String,
) -> Result<LogoutResponse, ApiError> {
    post_json(
        &auth_path(auth_base_path, "logout"),
        &LogoutRequest {
            username,
            session_token,
        },
    )
    .await
}

#[cfg(target_arch = "wasm32")]
pub async fn verify_session(
    auth_base_path: &str,
    username: String,
    session_token: String,
) -> Result<SessionCheckResponse, ApiError> {
    post_json(
        &auth_path(auth_base_path, "session"),
        &SessionCheckRequest {
            username,
            session_token,
        },
    )
    .await
}

#[cfg(target_arch = "wasm32")]
pub async fn get_home_dashboard(path: &str) -> Result<HomeDashboardResponse, ApiError> {
    get_json(path).await
}

#[cfg(target_arch = "wasm32")]
pub async fn get_enclosures_dashboard(path: &str) -> Result<EnclosuresPageResponse, ApiError> {
    get_json(path).await
}

#[cfg(target_arch = "wasm32")]
pub async fn get_object_stores_dashboard(path: &str) -> Result<ObjectStoresPageResponse, ApiError> {
    get_json(path).await
}

#[cfg(target_arch = "wasm32")]
pub async fn get_bioinformatics_workspace(
    path: &str,
) -> Result<BioinformaticsWorkspaceResponse, ApiError> {
    get_json(path).await
}

#[cfg(target_arch = "wasm32")]
pub async fn get_users_groups_workspace(
    path: &str,
) -> Result<UsersGroupsWorkspaceResponse, ApiError> {
    get_json(path).await
}

#[cfg(target_arch = "wasm32")]
#[allow(dead_code)]
pub async fn plan_gui_action(
    api_base_path: &str,
    request: &GuiActionPlanRequest,
) -> Result<GuiActionPlanResponse, ApiError> {
    post_json(
        &format!("{}/actions/plan", api_base_path.trim_end_matches('/')),
        request,
    )
    .await
}

#[cfg(target_arch = "wasm32")]
pub async fn submit_enclosure_prepare(
    api_base_path: &str,
    request: &EnclosurePrepareRequest,
) -> Result<EnclosurePrepareResponse, ApiError> {
    post_json(
        &format!(
            "{}/workspaces/enclosures/prepare",
            api_base_path.trim_end_matches('/')
        ),
        request,
    )
    .await
}

#[cfg(target_arch = "wasm32")]
pub async fn submit_object_store_create(
    api_base_path: &str,
    request: &CreateObjectStoreRequest,
) -> Result<CreateObjectStoreResponse, ApiError> {
    post_json(&object_store_create_path(api_base_path), request).await
}

#[cfg(any(target_arch = "wasm32", test))]
#[allow(dead_code)]
pub fn object_store_create_path(api_base_path: &str) -> String {
    format!(
        "{}/workspaces/object-stores/create",
        api_base_path.trim_end_matches('/')
    )
}

#[cfg(target_arch = "wasm32")]
#[allow(dead_code)]
pub async fn get_admin_job_status(
    api_base_path: &str,
    job_id: &str,
) -> Result<AdminJobStatusResponse, ApiError> {
    get_json(&admin_job_status_path(api_base_path, job_id)).await
}

#[cfg(target_arch = "wasm32")]
#[allow(dead_code)]
pub async fn cancel_admin_job(
    api_base_path: &str,
    job_id: &str,
    request: &AdminJobCancelRequest,
) -> Result<AdminJobCancelResponse, ApiError> {
    post_json(&admin_job_cancel_path(api_base_path, job_id), request).await
}

#[cfg(any(target_arch = "wasm32", test))]
#[allow(dead_code)]
pub fn admin_job_status_path(api_base_path: &str, job_id: &str) -> String {
    format!(
        "{}/workspaces/admin/jobs/{}",
        api_base_path.trim_end_matches('/'),
        job_id
    )
}

#[cfg(any(target_arch = "wasm32", test))]
#[allow(dead_code)]
pub fn admin_job_cancel_path(api_base_path: &str, job_id: &str) -> String {
    format!("{}/cancel", admin_job_status_path(api_base_path, job_id))
}

#[cfg(any(target_arch = "wasm32", test))]
fn auth_path(auth_base_path: &str, route: &str) -> String {
    format!("{}/{}", auth_base_path.trim_end_matches('/'), route)
}

#[cfg(target_arch = "wasm32")]
async fn get_json<R>(path: &str) -> Result<R, ApiError>
where
    R: for<'de> Deserialize<'de>,
{
    let mut request = Request::get(path);
    if let Some((username, session_token)) = crate::storage::stored_session() {
        request = request
            .header("x-dasobjectstore-username", &username)
            .header("x-dasobjectstore-session-token", &session_token)
            .header("authorization", &format!("Bearer {session_token}"));
    }
    let response = request.send().await?;
    decode_response(response).await
}

#[cfg(target_arch = "wasm32")]
async fn post_json<T, R>(path: &str, body: &T) -> Result<R, ApiError>
where
    T: Serialize,
    R: for<'de> Deserialize<'de>,
{
    let request_body = serde_json::to_string(body).map_err(|err| ApiError {
        message: format!("DASObjectStore request encoding failed: {err}"),
        status: None,
    })?;
    let mut request = Request::post(path).header("content-type", "application/json");
    if let Some((username, session_token)) = crate::storage::stored_session() {
        request = request
            .header("x-dasobjectstore-username", &username)
            .header("x-dasobjectstore-session-token", &session_token)
            .header("authorization", &format!("Bearer {session_token}"));
    }
    let response = request.body(request_body)?.send().await?;
    decode_response(response).await
}

#[cfg(target_arch = "wasm32")]
async fn decode_response<R>(response: gloo_net::http::Response) -> Result<R, ApiError>
where
    R: for<'de> Deserialize<'de>,
{
    let status = response.status();
    if !(200..300).contains(&status) {
        let message = response
            .json::<ErrorResponse>()
            .await
            .map(|error| error.message)
            .unwrap_or_else(|_| format!("DASObjectStore server returned HTTP {status}"));
        return Err(ApiError {
            message,
            status: Some(status),
        });
    }
    response.json::<R>().await.map_err(ApiError::from)
}

#[cfg(test)]
mod tests {
    use super::{
        admin_job_cancel_path, admin_job_status_path, auth_path, object_store_create_path,
        AdminJobCancelResponse, AdminJobStatusResponse, BioinformaticsWorkspaceResponse,
        CreateObjectStoreResponse, EnclosurePrepareResponse, EnclosuresPageResponse,
        GuiActionPlanResponse, HomeDashboardResponse, ObjectStoresPageResponse,
        UsersGroupsWorkspaceResponse,
    };

    #[test]
    fn builds_auth_routes_under_product_mount() {
        assert_eq!(
            auth_path("/products/dasobjectstore/api", "login"),
            "/products/dasobjectstore/api/login"
        );
    }

    #[test]
    fn decodes_home_dashboard_response_subset() {
        let payload = serde_json::json!({
            "schema_version": "dasobjectstore.web_redesign.v1",
            "generated_at_utc": "2026-07-08T08:00:00Z",
            "health": {
                "state": "watch",
                "label": "Inventory pending",
                "warning_count": 1,
                "critical_count": 0,
                "action_count": 1,
                "last_checked_at_utc": null
            },
            "drives": {
                "total": 7,
                "mounted": 7,
                "healthy": 6,
                "watch": 1,
                "suspect": 0,
                "failed": 0
            },
            "capacity": {
                "total_tib": "100.0",
                "used_tib": "12.5",
                "free_tib": "87.5",
                "used_percent_basis_points": 1250
            },
            "mounted_enclosures": [],
            "throughput_7d": {
                "window_days": 7,
                "read_tib": "1.0",
                "written_tib": "2.0",
                "ingest_tib": "2.5",
                "avg_read_mib_s": 120,
                "avg_write_mib_s": 240,
                "daily": []
            },
            "memory_stress": {
                "state": "nominal",
                "pressure_percent": 10,
                "swap_used_percent": 0,
                "page_cache_tib": "0.2",
                "warning": null
            },
            "smart_warnings": {
                "warning_count": 0,
                "affected_drive_count": 0,
                "warnings": []
            },
            "object_stores": [],
            "create_object_store": {
                "enabled": false,
                "action_kind": "store_create",
                "label": "Create ObjectStore",
                "required_fields": [],
                "optional_fields": [],
                "defaults": {
                    "store_class": "generated_data",
                    "required_copies": 2,
                    "endpoint_export_mode": "s3_bucket"
                },
                "store_class_options": [],
                "copy_count_options": [1, 2, 3],
                "confirmation_required": true,
                "blocked_reason": "admin required"
            }
        });

        let decoded =
            serde_json::from_value::<HomeDashboardResponse>(payload).expect("dashboard decodes");

        assert_eq!(decoded.drives.total, 7);
        assert_eq!(decoded.capacity.free_tib, "87.5");
        assert_eq!(decoded.throughput_7d.avg_write_mib_s, 240);
    }

    #[test]
    fn decodes_enclosures_dashboard_response_subset() {
        let payload = serde_json::json!({
            "schema_version": "dasobjectstore.web_redesign.v1",
            "generated_at_utc": "2026-07-08T08:00:00Z",
            "add_enclosure": {
                "enabled": false,
                "action_kind": "enclosure_add",
                "label": "Add enclosure",
                "state": "admin_required",
                "administrator": false,
                "supported_enclosure_detected": true,
                "daemon_ready": true,
                "confirmation_required": true,
                "blocked_reason": "Administrator capability is required before enclosure preparation is available.",
                "next_step": "Sign in with an administrator-capable local account to prepare DAS hardware."
            },
            "enclosures": [{
                "enclosure_id": "qnap-tl-d800c-01",
                "display_name": "QNAP TL-D800C",
                "mount_path": "/srv/dasobjectstore",
                "connection": {
                    "bus": "usb",
                    "protocol": "uas",
                    "link_speed": "10 Gb/s"
                },
                "health": "watch",
                "drive_count": {
                    "total": 8,
                    "mounted": 7,
                    "healthy": 6,
                    "watch": 1,
                    "suspect": 0,
                    "failed": 0
                },
                "capacity": {
                    "total_tib": "100.0",
                    "used_tib": "12.5",
                    "free_tib": "87.5",
                    "used_percent_basis_points": 1250
                },
                "last_seen_at_utc": "2026-07-08T08:00:00Z",
                "warnings": [{
                    "code": "smart_watch",
                    "message": "One member drive has a SMART warning."
                }]
            }],
            "selected_enclosure_id": "qnap-tl-d800c-01",
            "details": {
                "enclosure_id": "qnap-tl-d800c-01",
                "vendor": "QNAP",
                "model": "TL-D800C",
                "serial": "TL-D800C-TEST",
                "firmware": null,
                "slots": [{
                    "slot_number": 1,
                    "drive_id": "qnap-1057",
                    "size_tib": "14.6",
                    "health": "healthy",
                    "mounted": true
                }]
            },
            "warnings": []
        });

        let decoded = serde_json::from_value::<EnclosuresPageResponse>(payload)
            .expect("enclosures dashboard decodes");

        assert_eq!(decoded.enclosures.len(), 1);
        assert!(!decoded.add_enclosure.enabled);
        assert_eq!(decoded.add_enclosure.state, "admin_required");
        assert!(decoded.add_enclosure.supported_enclosure_detected);
        assert_eq!(decoded.enclosures[0].drive_count.total, 8);
        assert_eq!(
            decoded.details.expect("detail").slots[0].drive_id,
            "qnap-1057"
        );
    }

    #[test]
    fn decodes_gui_action_plan_response_subset() {
        let payload = serde_json::json!({
            "action": "enclosure_prepare",
            "execution": "planned_cli",
            "argv": ["dasobjectstore", "disk", "prepare-das"],
            "mutates_pool": true,
            "writes_recovery_metadata": false,
            "confirmation_required": true
        });

        let decoded =
            serde_json::from_value::<GuiActionPlanResponse>(payload).expect("plan decodes");

        assert_eq!(decoded.action, "enclosure_prepare");
        assert!(decoded.mutates_pool);
        assert_eq!(decoded.argv[2], "prepare-das");
    }

    #[test]
    fn decodes_enclosure_prepare_response_subset() {
        let payload = serde_json::json!({
            "accepted": {
                "job_id": "enclosure-prepare-job-1",
                "kind": "enclosure_preparation",
                "accepted_at_utc": "2026-07-08T19:50:00Z",
                "dry_run": false
            },
            "ssd_device": "/dev/disk/by-id/nvme-ssd",
            "hdd_devices": [{
                "disk_id": "qnap-1057",
                "device_path": "/dev/disk/by-id/usb-qnap-1057"
            }],
            "mount_root": "/srv/dasobjectstore",
            "filesystem": "ext4",
            "owner": "stephen",
            "administrator_actor": "operator",
            "client_request_id": "prepare-1"
        });

        let decoded = serde_json::from_value::<EnclosurePrepareResponse>(payload)
            .expect("prepare response decodes");

        assert_eq!(decoded.accepted.kind, "enclosure_preparation");
        assert_eq!(decoded.hdd_devices[0].disk_id, "qnap-1057");
        assert_eq!(decoded.administrator_actor.as_deref(), Some("operator"));
    }

    #[test]
    fn decodes_object_store_create_response_subset() {
        let payload = serde_json::json!({
            "accepted": {
                "job_id": "objectstore-create-1",
                "kind": "object_store_creation",
                "accepted_at_utc": "2026-07-08T20:45:00Z",
                "dry_run": false
            },
            "store_id": "generated-data",
            "store_class": "generated_data",
            "required_copies": 2,
            "bucket": "generated-data",
            "writer_group": "bioinformatics",
            "ssd_root": "/srv/dasobjectstore/ssd",
            "object_type": "pod5",
            "enclosure_id": "qnap-tl-d800c-01",
            "public": false,
            "writeable": true,
            "capacity_behavior": "balanced",
            "retention": "standard",
            "endpoint_export_mode": "s3_bucket",
            "administrator_actor": "stephen",
            "client_request_id": "request-1"
        });

        let decoded = serde_json::from_value::<CreateObjectStoreResponse>(payload)
            .expect("ObjectStore create response decodes");

        assert_eq!(decoded.accepted.kind, "object_store_creation");
        assert_eq!(decoded.store_id, "generated-data");
        assert_eq!(decoded.required_copies, 2);
    }

    #[test]
    fn builds_admin_job_routes_under_product_mount() {
        assert_eq!(
            admin_job_status_path("/products/dasobjectstore/api/v1/", "enclosure-prepare-1"),
            "/products/dasobjectstore/api/v1/workspaces/admin/jobs/enclosure-prepare-1"
        );
        assert_eq!(
            admin_job_cancel_path("/products/dasobjectstore/api/v1/", "enclosure-prepare-1"),
            "/products/dasobjectstore/api/v1/workspaces/admin/jobs/enclosure-prepare-1/cancel"
        );
    }

    #[test]
    fn builds_object_store_create_route_under_product_mount() {
        assert_eq!(
            object_store_create_path("/products/dasobjectstore/api/v1/"),
            "/products/dasobjectstore/api/v1/workspaces/object-stores/create"
        );
    }

    #[test]
    fn decodes_admin_job_status_response_subset() {
        let payload = serde_json::json!({
            "job": {
                "job_id": "enclosure-prepare-1",
                "kind": "enclosure_preparation",
                "state": "running",
                "progress": {
                    "stage": "formatting",
                    "work_bytes_done": 5,
                    "work_bytes_total": 10,
                    "work_units_done": 1,
                    "work_units_total": 2,
                    "message": "formatting selected devices"
                },
                "percent_complete": 50,
                "submitted_at_utc": "2026-07-08T20:05:00Z",
                "updated_at_utc": "2026-07-08T20:05:10Z",
                "actor": "operator",
                "failure_message": null
            }
        });

        let decoded =
            serde_json::from_value::<AdminJobStatusResponse>(payload).expect("status decodes");

        assert_eq!(decoded.job.kind, "enclosure_preparation");
        assert_eq!(decoded.job.percent_complete, Some(50));
    }

    #[test]
    fn decodes_admin_job_cancel_response_subset() {
        let payload = serde_json::json!({
            "job_id": "enclosure-prepare-1",
            "accepted": true,
            "state": "cancelled"
        });

        let decoded =
            serde_json::from_value::<AdminJobCancelResponse>(payload).expect("cancel decodes");

        assert!(decoded.accepted);
        assert_eq!(decoded.state, "cancelled");
    }

    #[test]
    fn decodes_object_stores_dashboard_response_subset() {
        let payload = serde_json::json!({
            "schema_version": "dasobjectstore.web_redesign.v1",
            "generated_at_utc": "2026-07-08T08:00:00Z",
            "groups_file_path": "/opt/dasobjectstore/groups.json",
            "groups": [{
                "group_name": "bioinformatics",
                "display_name": "Bioinformatics",
                "source": "local_os",
                "current_user_member": true
            }],
            "stores": [{
                "store_id": "zymo_fecal_2025.05",
                "display_name": "zymo_fecal_2025.05",
                "store_class": "generated_data",
                "object_type": "pod5",
                "health": "healthy",
                "required_copies": 2,
                "object_count": 42,
                "capacity": {
                    "total_tib": "100.0",
                    "used_tib": "12.5",
                    "free_tib": "87.5",
                    "used_percent_basis_points": 1250
                },
                "placement_policy": "fractional_free_space",
                "endpoint_export_mode": "s3_bucket",
                "writer_group": "bioinformatics",
                "public": false,
                "writeable": true,
                "created_at_utc": "2026-07-08T08:00:00Z",
                "last_ingested_at_utc": "2026-07-08T08:30:00Z",
                "writer_policy": {
                    "writer_group": "bioinformatics",
                    "group_defined": true,
                    "current_user_member": true,
                    "writeable_by_current_user": true,
                    "state": "ready",
                    "message": "Current user belongs to the ObjectStore writer group."
                },
                "warnings": []
            }],
            "selected_store_id": "zymo_fecal_2025.05",
            "create_object_store": {
                "enabled": false,
                "action_kind": "store_create",
                "label": "Create ObjectStore",
                "required_fields": [{"name": "store_id", "label": "Store ID"}],
                "optional_fields": [],
                "defaults": {
                    "store_class": "generated_data",
                    "required_copies": 2,
                    "endpoint_export_mode": "s3_bucket"
                },
                "store_class_options": [],
                "copy_count_options": [1, 2, 3],
                "confirmation_required": true,
                "blocked_reason": "admin required"
            },
            "warnings": []
        });

        let decoded = serde_json::from_value::<ObjectStoresPageResponse>(payload)
            .expect("object stores dashboard decodes");

        assert_eq!(decoded.stores.len(), 1);
        assert_eq!(decoded.groups.len(), 1);
        assert_eq!(decoded.groups[0].group_name, "bioinformatics");
        assert!(decoded.groups[0].current_user_member);
        assert_eq!(decoded.stores[0].store_id, "zymo_fecal_2025.05");
        assert_eq!(decoded.stores[0].required_copies, Some(2));
        assert_eq!(decoded.stores[0].object_type.as_deref(), Some("pod5"));
        assert_eq!(decoded.stores[0].public, Some(false));
        assert_eq!(decoded.stores[0].writeable, Some(true));
        assert_eq!(
            decoded.stores[0]
                .writer_policy
                .as_ref()
                .expect("writer policy")
                .state,
            "ready"
        );
        assert_eq!(
            decoded.create_object_store.defaults.endpoint_export_mode,
            "s3_bucket"
        );
    }

    #[test]
    fn decodes_bioinformatics_workspace_response_subset() {
        let payload = serde_json::json!({
            "schema_version": "dasobjectstore.product_workspaces.v1",
            "available": false,
            "supported_object_types": ["BAM", "POD5", "FASTQ", "ENA/SRA"],
            "message": "Bioinformatics orchestration will surface workflow-ready data sets once pipeline adapters are connected."
        });

        let decoded = serde_json::from_value::<BioinformaticsWorkspaceResponse>(payload)
            .expect("bioinformatics workspace decodes");

        assert!(!decoded.available);
        assert!(decoded
            .supported_object_types
            .iter()
            .any(|object_type| object_type == "POD5"));
    }

    #[test]
    fn decodes_users_groups_workspace_response_subset() {
        let payload = serde_json::json!({
            "host_mode": "standalone",
            "current_user": {
                "username": "operator",
                "groups": ["sudo", "mnemosyne"],
                "sudo_administrator": true
            },
            "users": [{
                "username": "operator",
                "registered": true,
                "created_at_unix_seconds": 1,
                "registered_at_unix_seconds": 2,
                "active_session_count": 1
            }],
            "groups": [{
                "group_name": "mnemosyne",
                "current_user_member": true,
                "sudo_administrator_group": false
            }],
            "groups_file_path": "/opt/dasobjectstore/groups.json",
            "writer_groups": [{
                "group_name": "mnemosyne",
                "display_name": "Mnemosyne",
                "source": "object_storage_group_registry",
                "current_user_member": true
            }],
            "operations": [{
                "kind": "create_local_group",
                "label": "Create local writer/admin group",
                "requires_sudo_administrator": true,
                "enabled": true,
                "blocked_reason": null
            }],
            "capabilities": {
                "product_local_user_registration": true,
                "os_local_user_management": true,
                "os_local_group_management": true,
                "administrator_actions_enabled": true
            },
            "selected_username": "operator",
            "selected_group_name": "mnemosyne",
            "warnings": []
        });

        let decoded = serde_json::from_value::<UsersGroupsWorkspaceResponse>(payload)
            .expect("users/groups workspace decodes");

        assert_eq!(decoded.host_mode, "standalone");
        assert!(
            decoded
                .current_user
                .as_ref()
                .expect("current user")
                .sudo_administrator
        );
        assert_eq!(decoded.writer_groups[0].group_name, "mnemosyne");
        assert!(decoded.capabilities.administrator_actions_enabled);
    }
}
