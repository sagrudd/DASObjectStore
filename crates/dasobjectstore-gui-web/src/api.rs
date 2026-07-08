#[cfg(target_arch = "wasm32")]
use gloo_net::http::Request;
use serde::Deserialize;
#[cfg(target_arch = "wasm32")]
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
    pub memory_stress: MemoryStressResponse,
    pub smart_warnings: SmartWarningsSummaryResponse,
    pub object_stores: Vec<ObjectStoreCardResponse>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct EnclosuresPageResponse {
    pub schema_version: String,
    pub generated_at_utc: String,
    pub enclosures: Vec<DasEnclosureCardResponse>,
    pub selected_enclosure_id: Option<String>,
    pub details: Option<DasEnclosureDetailResponse>,
    pub warnings: Vec<DashboardWarning>,
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
    pub size_tib: String,
    pub health: String,
    pub mounted: bool,
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
    pub health: String,
    pub object_count: usize,
    pub warnings: Vec<DashboardWarning>,
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
    let response = Request::post(path)
        .header("content-type", "application/json")
        .body(request_body)?
        .send()
        .await?;
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
    use super::{auth_path, EnclosuresPageResponse, HomeDashboardResponse};

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
        assert_eq!(decoded.enclosures[0].drive_count.total, 8);
        assert_eq!(
            decoded.details.expect("detail").slots[0].drive_id,
            "qnap-1057"
        );
    }
}
