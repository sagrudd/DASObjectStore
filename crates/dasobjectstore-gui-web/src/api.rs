#[cfg(target_arch = "wasm32")]
use gloo_net::http::Request;
use prosopikon_core::{ProsopikonAuthenticationFramework, ProsopikonDeviceTokenRequirement};
use serde::Deserialize;
#[cfg(any(target_arch = "wasm32", test))]
use serde::Serialize;

#[path = "api_contracts.rs"]
mod contracts;
pub use contracts::*;

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
pub async fn verify_host_session(
    api_base_path: &str,
) -> Result<FederatedHostSessionResponse, ApiError> {
    get_json_without_session(&format!(
        "{}/host-session",
        api_base_path.trim_end_matches('/')
    ))
    .await
}

#[cfg(target_arch = "wasm32")]
pub async fn host_logout() {
    let _ = Request::post("/logout").send().await;
}

#[cfg(target_arch = "wasm32")]
pub async fn get_api_health(api_base_path: &str) -> Result<ApiHealthResponse, ApiError> {
    get_json_without_session(&format!("{}/health", api_base_path.trim_end_matches('/'))).await
}

#[cfg(target_arch = "wasm32")]
pub async fn get_home_dashboard(path: &str) -> Result<HomeDashboardResponse, ApiError> {
    get_json(path).await
}

#[cfg(target_arch = "wasm32")]
pub async fn get_cached_home_dashboard(
    path: &str,
) -> Result<CachedHomeDashboardResponse, ApiError> {
    get_json(path).await
}

#[cfg(target_arch = "wasm32")]
pub async fn get_store_capacity(path: &str) -> Result<ObjectStoreCapacityStatusResponse, ApiError> {
    get_json(path).await
}

#[cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]
pub fn cached_home_dashboard_api_path(api_base_path: &str) -> String {
    format!("{}/dashboard/status", api_base_path.trim_end_matches('/'))
}

#[cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]
pub fn store_capacity_api_path(api_base_path: &str, store_id: &str) -> String {
    format!(
        "{}/dashboard/object-stores/{}/capacity",
        api_base_path.trim_end_matches('/'),
        percent_encode(store_id.trim())
    )
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
pub async fn get_remote_upload_workspace(
    path: &str,
) -> Result<RemoteUploadWorkspaceResponse, ApiError> {
    get_json(path).await
}

#[cfg(target_arch = "wasm32")]
pub async fn get_object_browser(path: &str) -> Result<ObjectBrowserResponse, ApiError> {
    get_json(path).await
}

#[cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]
pub fn object_browser_api_path(
    api_base_path: &str,
    endpoint: &str,
    prefix: &str,
    search: &str,
    sort: &str,
    include_placement: bool,
) -> String {
    let mut path = format!(
        "{}/object-stores/{}/browser?sort={}&limit=100&include_placement={}",
        api_base_path.trim_end_matches('/'),
        percent_encode(endpoint.trim()),
        percent_encode(sort.trim()),
        include_placement
    );
    if !prefix.trim().is_empty() {
        path.push_str("&prefix=");
        path.push_str(&percent_encode(prefix.trim()));
    }
    if !search.trim().is_empty() {
        path.push_str("&search=");
        path.push_str(&percent_encode(search.trim()));
    }
    path
}

#[cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]
pub fn object_download_api_path(api_base_path: &str, endpoint: &str, object_id: &str) -> String {
    format!(
        "{}/object-stores/{}/objects/download/{}",
        api_base_path.trim_end_matches('/'),
        percent_encode(endpoint.trim()),
        percent_encode_path_segments(object_id)
    )
}

#[cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]
pub fn object_folder_download_api_path(
    api_base_path: &str,
    endpoint: &str,
    prefix: &str,
) -> String {
    format!(
        "{}/object-stores/{}/folders/download/{}",
        api_base_path.trim_end_matches('/'),
        percent_encode(endpoint.trim()),
        percent_encode_path_segments(prefix)
    )
}

#[cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]
pub(crate) use crate::encoding::percent_encode;

#[cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]
fn percent_encode_path_segments(value: &str) -> String {
    value
        .trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(percent_encode)
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(target_arch = "wasm32")]
pub struct ObjectBrowserDownload {
    pub filename: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
    pub content_length: Option<u64>,
    pub archive_files: Option<u64>,
    pub archive_source_bytes: Option<u64>,
}

#[cfg(target_arch = "wasm32")]
pub async fn download_object_browser_asset(
    path: &str,
    fallback_filename: &str,
) -> Result<ObjectBrowserDownload, ApiError> {
    let mut request = Request::get(path);
    if let Some((username, session_token)) = crate::storage::stored_session() {
        request = request
            .header("x-dasobjectstore-username", &username)
            .header("x-dasobjectstore-session-token", &session_token)
            .header("authorization", &format!("Bearer {session_token}"));
    }
    let response = request.send().await?;
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
    let filename = response
        .headers()
        .get("content-disposition")
        .as_deref()
        .and_then(filename_from_content_disposition)
        .unwrap_or_else(|| fallback_filename.to_string());
    let content_type = response
        .headers()
        .get("content-type")
        .unwrap_or_else(|| "application/octet-stream".to_string());
    let content_length = response
        .headers()
        .get("content-length")
        .and_then(|value| value.parse::<u64>().ok());
    let archive_files = response
        .headers()
        .get("x-dasobjectstore-archive-files")
        .and_then(|value| value.parse::<u64>().ok());
    let archive_source_bytes = response
        .headers()
        .get("x-dasobjectstore-archive-source-bytes")
        .and_then(|value| value.parse::<u64>().ok());
    let bytes = response.binary().await?;
    Ok(ObjectBrowserDownload {
        filename,
        content_type,
        bytes,
        content_length,
        archive_files,
        archive_source_bytes,
    })
}

#[cfg(target_arch = "wasm32")]
pub async fn get_activity_workspace(path: &str) -> Result<ActivityWorkspaceResponse, ApiError> {
    get_json(path).await
}

#[cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]
pub fn activity_performance_report_upload_path(api_base_path: &str) -> String {
    format!(
        "{}/workspaces/activity/reporting/performance-report",
        api_base_path.trim_end_matches('/')
    )
}

#[cfg(target_arch = "wasm32")]
pub struct PerformanceReportDownload {
    pub filename: String,
    pub bytes: Vec<u8>,
}

#[cfg(target_arch = "wasm32")]
pub async fn upload_performance_report_json(
    path: &str,
    file: web_sys::File,
) -> Result<PerformanceReportDownload, ApiError> {
    let file_name = file.name();
    let mut request = Request::post(path)
        .header("content-type", "application/json")
        .header("x-dasobjectstore-filename", &file_name);
    if let Some((username, session_token)) = crate::storage::stored_session() {
        request = request
            .header("x-dasobjectstore-username", &username)
            .header("x-dasobjectstore-session-token", &session_token)
            .header("authorization", &format!("Bearer {session_token}"));
    }
    let response = request.body(file)?.send().await?;
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
    let filename = response
        .headers()
        .get("content-disposition")
        .as_deref()
        .and_then(filename_from_content_disposition)
        .unwrap_or_else(|| default_report_pdf_name(&file_name));
    let bytes = response.binary().await?;
    Ok(PerformanceReportDownload { filename, bytes })
}

#[cfg(target_arch = "wasm32")]
fn filename_from_content_disposition(value: &str) -> Option<String> {
    value.split(';').find_map(|part| {
        let part = part.trim();
        part.strip_prefix("filename=")
            .map(|filename| filename.trim_matches('"').to_string())
            .filter(|filename| !filename.is_empty())
    })
}

#[cfg(target_arch = "wasm32")]
fn default_report_pdf_name(file_name: &str) -> String {
    let stem = file_name
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(file_name)
        .trim();
    if stem.is_empty() {
        "dasobjectstore-performance-report.pdf".to_string()
    } else {
        format!("{stem}.pdf")
    }
}

#[cfg(target_arch = "wasm32")]
pub async fn get_endpoints_workspace(path: &str) -> Result<EndpointsWorkspaceResponse, ApiError> {
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
pub async fn submit_create_local_group(
    api_base_path: &str,
    request: &CreateLocalGroupRequest,
) -> Result<LocalGroupAdminResponse, ApiError> {
    post_json(
        &crate::users_groups::create_local_group_action_api_path(api_base_path),
        request,
    )
    .await
}

#[cfg(target_arch = "wasm32")]
pub async fn submit_assign_local_user_to_group(
    api_base_path: &str,
    request: &AssignLocalUserToGroupRequest,
) -> Result<LocalGroupAdminResponse, ApiError> {
    post_json(
        &crate::users_groups::assign_local_user_to_group_action_api_path(api_base_path),
        request,
    )
    .await
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

#[cfg(target_arch = "wasm32")]
pub async fn submit_object_store_ingest_policy(
    api_base_path: &str,
    request: &ObjectStoreIngestPolicyRequest,
) -> Result<ObjectStoreIngestPolicyResponse, ApiError> {
    post_json(&object_store_ingest_policy_path(api_base_path), request).await
}

#[cfg(target_arch = "wasm32")]
pub async fn submit_ingest_control(
    api_base_path: &str,
    request: &IngestControlRequest,
) -> Result<IngestControlResponse, ApiError> {
    post_json(&ingest_control_path(api_base_path), request).await
}

#[cfg(target_arch = "wasm32")]
pub async fn submit_endpoint_inventory_upsert(
    api_base_path: &str,
    request: &EndpointInventoryUpsertRequest,
) -> Result<EndpointInventoryUpsertResponse, ApiError> {
    post_json(&endpoint_inventory_upsert_path(api_base_path), request).await
}

#[cfg(any(target_arch = "wasm32", test))]
#[allow(dead_code)]
pub fn object_store_create_path(api_base_path: &str) -> String {
    format!(
        "{}/workspaces/object-stores/create",
        api_base_path.trim_end_matches('/')
    )
}

#[cfg(any(target_arch = "wasm32", test))]
pub fn object_store_ingest_policy_path(api_base_path: &str) -> String {
    format!(
        "{}/workspaces/object-stores/ingest-policy",
        api_base_path.trim_end_matches('/')
    )
}

#[cfg(any(target_arch = "wasm32", test))]
pub fn ingest_control_path(api_base_path: &str) -> String {
    format!(
        "{}/workspaces/admin/ingest-control",
        api_base_path.trim_end_matches('/')
    )
}

#[cfg(any(target_arch = "wasm32", test))]
#[allow(dead_code)]
pub fn endpoint_inventory_upsert_path(api_base_path: &str) -> String {
    format!(
        "{}/workspaces/endpoints/upsert",
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
async fn get_json_without_session<R>(path: &str) -> Result<R, ApiError>
where
    R: for<'de> Deserialize<'de>,
{
    let response = Request::get(path).send().await?;
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
        activity_performance_report_upload_path, admin_job_cancel_path, admin_job_status_path,
        auth_path, cached_home_dashboard_api_path, endpoint_inventory_upsert_path,
        ingest_control_path, object_browser_api_path, object_download_api_path,
        object_folder_download_api_path, object_store_create_path, object_store_ingest_policy_path,
        store_capacity_api_path, ActivityWorkspaceResponse, AdminJobCancelResponse,
        AdminJobStatusResponse, BioinformaticsWorkspaceResponse, CreateObjectStoreResponse,
        EnclosurePrepareResponse, EnclosuresPageResponse, EndpointInventoryUpsertResponse,
        EndpointsWorkspaceResponse, GuiActionPlanResponse, HomeDashboardResponse,
        LocalGroupAdminResponse, ObjectStoresPageResponse, RemoteUploadWorkspaceResponse,
        UsersGroupsWorkspaceResponse,
    };
    use prosopikon_core::{ProsopikonAuthenticationFramework, ProsopikonDeviceTokenRequirement};

    #[test]
    fn builds_auth_routes_under_product_mount() {
        assert_eq!(
            auth_path("/products/dasobjectstore/api", "login"),
            "/products/dasobjectstore/api/login"
        );
    }

    #[test]
    fn builds_activity_report_upload_route_under_product_mount() {
        assert_eq!(
            activity_performance_report_upload_path("/products/dasobjectstore/api/v1/"),
            "/products/dasobjectstore/api/v1/workspaces/activity/reporting/performance-report"
        );
    }

    #[test]
    fn builds_cached_home_dashboard_status_route() {
        assert_eq!(
            cached_home_dashboard_api_path("/products/dasobjectstore/api/v1/"),
            "/products/dasobjectstore/api/v1/dashboard/status"
        );
    }

    #[test]
    fn builds_store_capacity_status_route() {
        assert_eq!(
            store_capacity_api_path("/products/dasobjectstore/api/v1/", "ENA Primary"),
            "/products/dasobjectstore/api/v1/dashboard/object-stores/ENA%20Primary/capacity"
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
            "telemetry_window": {
                "selected": "ten_days",
                "selected_label": "10 days",
                "options": [
                    { "value": "one_hour", "label": "1 hour", "selected": false },
                    { "value": "one_day", "label": "1 day", "selected": false },
                    { "value": "ten_days", "label": "10 days", "selected": true },
                    { "value": "three_months", "label": "3 months", "selected": false }
                ]
            },
            "throughput_7d": {
                "window_days": 7,
                "read_tib": "1.0",
                "written_tib": "2.0",
                "ingest_tib": "2.5",
                "avg_read_mib_s": 120,
                "avg_write_mib_s": 240,
                "daily": []
            },
            "disk_io": {
                "available": true,
                "read_mib_s": 120,
                "write_mib_s": 240,
                "read_ops_s": 10,
                "write_ops_s": 20,
                "busiest_disk_id": "qnap-1057",
                "sample_timestamp_utc": "2026-07-08T08:00:00Z",
                "sample_age_seconds": 0,
                "per_disk": [{
                    "disk_id": "qnap-1057",
                    "label": "QNAP bay 1",
                    "mount_path": "/srv/dasobjectstore/hdd/qnap-1057",
                    "role": "hdd",
                    "enclosure_id": "qnap-tl-d800c-01",
                    "bay_label": "1",
                    "device_path": "/dev/disk/by-id/qnap-1057",
                    "device_name": "sda",
                    "read_mib_s": 120,
                    "write_mib_s": 240,
                    "read_ops_s": 10,
                    "write_ops_s": 20,
                    "missing_reason": null
                }],
                "state": "nominal",
                "message": null
            },
            "cpu_usage": {
                "available": true,
                "usage_percent": 42,
                "load_average_1m": "0.84",
                "logical_core_count": 8,
                "state": "nominal",
                "message": null
            },
            "active_users": {
                "available": true,
                "active_sessions": 3,
                "distinct_logged_in_users": 2,
                "administrator_sessions": 1,
                "operator_sessions": 1,
                "remote_agent_sessions": 1,
                "state": "nominal",
                "message": null
            },
            "memory_stress": {
                "state": "nominal",
                "pressure_percent": 10,
                "swap_used_percent": 0,
                "page_cache_tib": "0.2",
                "warning": null
            },
            "object_service": {
                "active": true,
                "remote_ready": true,
                "bind_address": "0.0.0.0",
                "port": 3900,
                "local_url": "http://127.0.0.1:3900",
                "remote_url": "http://192.168.1.192:3900",
                "service_state": "Up 1 minute",
                "message": null
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
        assert_eq!(decoded.telemetry_window.selected, "ten_days");
        assert_eq!(decoded.telemetry_window.options.len(), 4);
        assert_eq!(decoded.throughput_7d.avg_write_mib_s, 240);
        assert_eq!(decoded.disk_io.write_mib_s, 240);
        assert_eq!(decoded.disk_io.sample_age_seconds, Some(0));
        assert_eq!(
            decoded.disk_io.per_disk[0].device_name.as_deref(),
            Some("sda")
        );
        assert_eq!(decoded.cpu_usage.usage_percent, Some(42));
        assert_eq!(decoded.active_users.distinct_logged_in_users, 2);
        assert!(decoded.object_service.remote_ready);
        assert_eq!(decoded.object_service.port, 3900);
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
    fn builds_object_store_ingest_policy_route_under_product_mount() {
        assert_eq!(
            object_store_ingest_policy_path("/products/dasobjectstore/api/v1/"),
            "/products/dasobjectstore/api/v1/workspaces/object-stores/ingest-policy"
        );
    }

    #[test]
    fn builds_ingest_control_route_under_product_mount() {
        assert_eq!(
            ingest_control_path("/products/dasobjectstore/api/v1/"),
            "/products/dasobjectstore/api/v1/workspaces/admin/ingest-control"
        );
    }

    #[test]
    fn builds_endpoint_inventory_upsert_route_under_product_mount() {
        assert_eq!(
            endpoint_inventory_upsert_path("/products/dasobjectstore/api/v1/"),
            "/products/dasobjectstore/api/v1/workspaces/endpoints/upsert"
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
    fn decodes_remote_upload_workspace_response_subset() {
        let payload = serde_json::json!({
            "schema_version": "dasobjectstore.web_redesign.v1",
            "generated_at_utc": "2026-07-09T12:20:00Z",
            "actor": {
                "username": "stephen",
                "groups": ["mnemosyne"],
                "sudo_administrator": true
            },
            "stores": [{
                "store_id": "zymo_fecal_2025.05",
                "display_name": "zymo_fecal_2025.05",
                "bucket": "dos-zymo-fecal-2025-05",
                "store_class": "reproducible_cache",
                "object_type": "fastq",
                "capacity": {
                    "total_tib": "4.0",
                    "used_tib": "1.0",
                    "free_tib": "3.0",
                    "used_percent_basis_points": 2500
                },
                "writer_group": "mnemosyne",
                "writer_policy_state": "ready",
                "public": false,
                "endpoint_export_mode": "s3",
                "upload_allowed": true,
                "upload_state": "ready",
                "upload_message": "Remote upload is allowed.",
                "warnings": []
            }],
            "warnings": []
        });

        let decoded = serde_json::from_value::<RemoteUploadWorkspaceResponse>(payload)
            .expect("remote upload workspace decodes");

        assert_eq!(decoded.actor.username, "stephen");
        assert_eq!(decoded.stores[0].bucket, "dos-zymo-fecal-2025-05");
        assert!(decoded.stores[0].upload_allowed);
        assert_eq!(decoded.stores[0].writer_policy_state, "ready");
    }

    #[test]
    fn decodes_endpoints_workspace_response_subset() {
        let payload = serde_json::json!({
            "inventory": {
                "schema_version": "dasobjectstore.endpoint_inventory.v1",
                "endpoint_count": 1,
                "degraded_endpoint_count": 0,
                "binding_count": 1,
                "endpoints": [{
                    "endpoint_id": "nas-staging",
                    "display_name": "NAS staging",
                    "kind": "dasobjectstore_nfs",
                    "manager_product_id": "dasobjectstore",
                    "object_service_url": "https://nas.example.test:9443",
                    "validation": {
                        "state": "validated",
                        "checked_at_utc": "2026-07-09T00:00:00Z",
                        "message": "validated"
                    },
                    "active_bindings": [{
                        "binding_id": "binding-1",
                        "governance_domain": "local",
                        "store_id": "zymo",
                        "readiness": "ready"
                    }],
                    "warnings": []
                }],
                "warnings": []
            }
        });

        let decoded = serde_json::from_value::<EndpointsWorkspaceResponse>(payload)
            .expect("endpoints workspace decodes");

        assert_eq!(decoded.inventory.endpoint_count, 1);
        assert_eq!(decoded.inventory.binding_count, 1);
        assert_eq!(decoded.inventory.endpoints[0].kind, "dasobjectstore_nfs");
        assert_eq!(decoded.inventory.endpoints[0].validation.state, "validated");
    }

    #[test]
    fn decodes_endpoint_inventory_upsert_response_subset() {
        let payload = serde_json::json!({
            "accepted": {
                "job_id": "endpoint-upsert-1",
                "kind": "endpoint_validation",
                "accepted_at_utc": "2026-07-09T00:00:00Z",
                "dry_run": false
            },
            "endpoint_id": "nas-staging",
            "display_name": "NAS staging",
            "kind": "dasobjectstore_nfs",
            "validation_state": "validated",
            "registry_path": "/opt/dasobjectstore/endpoints.json",
            "administrator_actor": "stephen",
            "client_request_id": null
        });

        let decoded = serde_json::from_value::<EndpointInventoryUpsertResponse>(payload)
            .expect("endpoint inventory upsert response decodes");

        assert_eq!(decoded.accepted.kind, "endpoint_validation");
        assert_eq!(decoded.endpoint_id, "nas-staging");
        assert_eq!(decoded.registry_path, "/opt/dasobjectstore/endpoints.json");
    }

    #[test]
    fn decodes_bioinformatics_workspace_response_subset() {
        let payload = serde_json::json!({
            "schema_version": "dasobjectstore.product_workspaces.v1",
            "available": true,
            "supported_object_types": ["BAM", "POD5", "FASTQ", "ENA/SRA"],
            "readiness_cards": [
                {
                    "object_type": "pod5",
                    "label": "POD5",
                    "category": "Nanopore signal",
                    "state": "workflow_ready",
                    "primary_workflow": "Basecalling and signal-level provenance.",
                    "handoff": "Basecalling readiness",
                    "required_metadata": ["flowcell/run identity", "sequencing kit"]
                }
            ],
            "derivation_sources": [
                {
                    "source_kind": "object_store_metadata",
                    "source_id": "contract-object-store-object-type",
                    "display_name": "ObjectStore object-type assignment",
                    "object_type": "pod5",
                    "parent_id": null,
                    "endpoint_export_mode": "s3_bucket",
                    "mneion_binding_state": "binding_required",
                    "governance_domain": null,
                    "workflow_roles": ["sequencing_run_provenance", "basecalling_handoff"],
                    "evidence": ["ObjectStore object_type assignment"]
                },
                {
                    "source_kind": "mneion_binding",
                    "source_id": "contract-mneion-governance-binding",
                    "display_name": "Mneion governance-domain binding",
                    "object_type": "mixed",
                    "parent_id": null,
                    "endpoint_export_mode": null,
                    "mneion_binding_state": "binding_required",
                    "governance_domain": "unassigned",
                    "workflow_roles": ["governance_binding"],
                    "evidence": ["Mneion storage definition"]
                }
            ],
            "sequencing_runs": [
                {
                    "label": "Sequencing run provenance",
                    "state": "metadata_required",
                    "summary": "Run metadata required.",
                    "detail": "Bind flowcell and kit state.",
                    "evidence": ["POD5 basecalling readiness"]
                }
            ],
            "object_lineage": [],
            "workflow_handoffs": [
                {
                    "label": "Basecalling handoff",
                    "state": "workflow_ready",
                    "summary": "Basecalling ready.",
                    "detail": "POD5 handoff state is available.",
                    "evidence": ["POD5 readiness cards"]
                }
            ],
            "governance_bindings": [
                {
                    "label": "Mnemosyne governance binding",
                    "state": "binding_required",
                    "summary": "Binding required.",
                    "detail": "Project and governance-domain binding is required.",
                    "evidence": ["endpoint inventory bindings"]
                }
            ],
            "message": "Bioinformatics readiness cards classify supported object types."
        });

        let decoded = serde_json::from_value::<BioinformaticsWorkspaceResponse>(payload)
            .expect("bioinformatics workspace decodes");

        assert!(decoded.available);
        assert!(decoded
            .supported_object_types
            .iter()
            .any(|object_type| object_type == "POD5"));
        assert_eq!(decoded.readiness_cards[0].label, "POD5");
        assert_eq!(decoded.readiness_cards[0].handoff, "Basecalling readiness");
        assert_eq!(
            decoded.derivation_sources[0].source_kind,
            "object_store_metadata"
        );
        assert_eq!(
            decoded.derivation_sources[1].governance_domain.as_deref(),
            Some("unassigned")
        );
        assert_eq!(
            decoded.sequencing_runs[0].label,
            "Sequencing run provenance"
        );
        assert_eq!(decoded.workflow_handoffs[0].state, "workflow_ready");
        assert_eq!(decoded.governance_bindings[0].state, "binding_required");
    }

    #[test]
    fn decodes_activity_workspace_response_subset() {
        let payload = serde_json::json!({
            "ingest": {
                "pressure": "normal",
                "queued_jobs": 1,
                "active_jobs": 2,
                "failed_jobs": 0,
                "jobs": [],
                "warnings": []
            },
            "destage": {
                "pending_objects": 4,
                "copying_objects": 1,
                "verified_objects": 12,
                "objects": [],
                "warnings": []
            },
            "categories": [{
                "kind": "system_administration",
                "label": "Administrator jobs",
                "description": "Privileged daemon jobs"
            }],
            "tasks": [{
                "task_id": "job-1",
                "kind": "system_administration",
                "state": "running",
                "label": "Create local writer group",
                "progress": {
                    "stage": "remote_s3_transfer_running",
                    "work_bytes_done": 512,
                    "work_bytes_total": 1024,
                    "work_units_done": 3,
                    "work_units_total": 9,
                    "percent_complete": 50,
                    "message": "remote upload copied 512 bytes"
                },
                "updated_at_utc": "2026-07-09T00:00:00Z",
                "warnings": []
            }],
            "warnings": []
        });

        let decoded = serde_json::from_value::<ActivityWorkspaceResponse>(payload)
            .expect("activity workspace decodes");

        assert_eq!(decoded.ingest.expect("ingest").active_jobs, 2);
        assert_eq!(decoded.destage.expect("destage").pending_objects, 4);
        assert_eq!(decoded.categories[0].kind, "system_administration");
        assert_eq!(decoded.tasks[0].state, "running");
        let progress = decoded.tasks[0].progress.as_ref().expect("progress");
        assert_eq!(progress.stage, "remote_s3_transfer_running");
        assert_eq!(progress.work_bytes_done, 512);
        assert_eq!(progress.work_bytes_total, 1024);
        assert_eq!(progress.percent_complete, Some(50));
    }

    #[test]
    fn decodes_users_groups_workspace_response_subset() {
        let payload = serde_json::json!({
            "host_mode": "standalone",
            "authentication_framework": "hybrid",
            "device_token_requirement": "not_required",
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
        assert_eq!(
            decoded.authentication_framework,
            ProsopikonAuthenticationFramework::Hybrid
        );
        assert_eq!(
            decoded.device_token_requirement,
            ProsopikonDeviceTokenRequirement::NotRequired
        );
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

    #[test]
    fn object_browser_api_path_encodes_endpoint_prefix_and_search() {
        let path = object_browser_api_path(
            "/products/dasobjectstore/api/v1/",
            "ENA Primary",
            "Xenognostikon/Vervet",
            "sample fastq",
            "size_desc",
            true,
        );

        assert_eq!(
            path,
            "/products/dasobjectstore/api/v1/object-stores/ENA%20Primary/browser?sort=size_desc&limit=100&include_placement=true&prefix=Xenognostikon%2FVervet&search=sample%20fastq"
        );
    }

    #[test]
    fn object_download_paths_encode_endpoint_and_path_segments() {
        assert_eq!(
            object_download_api_path(
                "/products/dasobjectstore/api/v1/",
                "ENA Primary",
                "Xenognostikon/Vervet/sample 01.fastq.gz",
            ),
            "/products/dasobjectstore/api/v1/object-stores/ENA%20Primary/objects/download/Xenognostikon/Vervet/sample%2001.fastq.gz"
        );
        assert_eq!(
            object_folder_download_api_path(
                "/products/dasobjectstore/api/v1/",
                "ENA Primary",
                "Xenognostikon/Vervet",
            ),
            "/products/dasobjectstore/api/v1/object-stores/ENA%20Primary/folders/download/Xenognostikon/Vervet"
        );
    }

    #[test]
    fn decodes_local_group_admin_response_subset() {
        let payload = serde_json::json!({
            "accepted": {
                "job_id": "local-admin-1",
                "kind": "system_administration",
                "accepted_at_utc": "2026-07-09T08:00:00Z",
                "dry_run": true
            },
            "operation": "create_group",
            "group_name": "mnemosyne-writers",
            "username": null,
            "client_request_id": "request-1"
        });

        let decoded = serde_json::from_value::<LocalGroupAdminResponse>(payload)
            .expect("local group admin response decodes");

        assert_eq!(decoded.accepted.job_id, "local-admin-1");
        assert_eq!(decoded.operation, "create_group");
        assert!(decoded.accepted.dry_run);
        assert_eq!(decoded.group_name, "mnemosyne-writers");
    }
}
