//! Store-scoped HTTPS control routes for remote clients.
//!
//! Object payloads remain on the S3 data plane. These handlers expose only
//! path-free daemon-authoritative state which standard S3 cannot represent.

use crate::remote_control_guard::{RemoteControlGuardState, RemoteControlRejection};
use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    routing::{get, post},
    Json, Router,
};
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_daemon::api::{
    RemoteEasyconnectControlOperation, RemoteObjectGroupStatusRequest,
    RemoteObjectGroupStatusResponse, RemoteObjectSnapshotRequest, RemoteObjectSnapshotResponse,
    StoreRepairRequest, StoreRepairS3Expectation, STORE_REPAIR_CONFIRMATION,
};
use dasobjectstore_daemon::{
    DaemonClient, DaemonClientError, DaemonClock, DaemonJobId, DaemonJobState,
    DaemonJobStatusRequest, DaemonRuntimeConfig, ProfileReadinessRequest,
    UnixSocketDaemonTransport,
};
use serde::{Deserialize, Serialize};
use std::path::{Path as FsPath, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static REMOTE_ERROR_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug)]
pub struct RemoteControlRouteState {
    guard: RemoteControlGuardState,
    operation_dir: PathBuf,
}

impl Default for RemoteControlRouteState {
    fn default() -> Self {
        Self {
            guard: RemoteControlGuardState::default(),
            operation_dir: DaemonRuntimeConfig::default_packaged()
                .state_dir
                .join("remote-operations"),
        }
    }
}

pub fn remote_control_router() -> Router {
    remote_control_router_with_state(RemoteControlRouteState::default())
}

fn remote_control_router_with_state(state: RemoteControlRouteState) -> Router {
    Router::new()
        .route("/api/v1/remote/stores/{store_id}/readiness", get(readiness))
        .route(
            "/api/v1/remote/stores/{store_id}/objects/snapshot",
            get(snapshot),
        )
        .route(
            "/api/v1/remote/stores/{store_id}/objects/group-status",
            get(group_status),
        )
        .route(
            "/api/v1/remote/stores/{store_id}/objects/reconcile-s3",
            post(reconcile_s3),
        )
        .route(
            "/api/v1/remote/operations/{operation_id}",
            get(operation_status),
        )
        .with_state(state)
}

#[derive(Debug, Deserialize)]
struct SnapshotQuery {
    #[serde(default)]
    prefix: String,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default = "default_snapshot_limit")]
    limit: u32,
}

#[derive(Debug, Deserialize)]
struct GroupStatusQuery {
    key: String,
}

#[derive(Debug, Deserialize)]
struct ReconcileS3Body {
    key: String,
    expected_bytes: u64,
    expected_sha256: String,
    idempotency_key: String,
    ack_policy: String,
}

#[derive(Debug, Serialize)]
struct ReconcileS3Response {
    schema_version: &'static str,
    operation_id: DaemonJobId,
    store_id: StoreId,
    key: String,
    state: &'static str,
    ssd_acknowledged: bool,
    hdd_settled: bool,
}

#[derive(Debug, Serialize)]
struct RemoteOperationResponse {
    schema_version: &'static str,
    operation_id: DaemonJobId,
    store_id: StoreId,
    state: DaemonJobState,
    queue_stage: String,
    bytes_received: u64,
    total_bytes: u64,
    elapsed_seconds: Option<u64>,
    transfer_bytes_per_second: Option<u64>,
    backpressure_reason: Option<String>,
    ssd_acknowledged: bool,
    hdd_settled: bool,
    failure: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RemoteOperationRecord {
    schema_version: String,
    operation_id: DaemonJobId,
    store_id: StoreId,
    key: String,
    actor: String,
    expected_bytes: u64,
    submitted_at_utc: String,
    ssd_acknowledged: bool,
}

async fn readiness(
    State(state): State<RemoteControlRouteState>,
    Path(store_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, RemoteControlRouteError> {
    let authorization = state.guard.authorize(
        &headers,
        &store_id,
        "",
        RemoteEasyconnectControlOperation::StoreReadiness,
    )?;
    let store_id = parse_store_id(store_id)?;
    let response =
        call_daemon(move |client| client.profile_readiness(ProfileReadinessRequest { store_id }))
            .await?;
    let authority = headers
        .get(axum::http::header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("localhost");
    let host = authority
        .strip_prefix('[')
        .and_then(|value| value.split_once(']').map(|(host, _)| host))
        .unwrap_or_else(|| authority.split(':').next().unwrap_or(authority));
    let ssd_ingest_ready = response.ready
        && response
            .capacity
            .as_ref()
            .is_none_or(|capacity| capacity.admission_block_reason.is_none());
    Ok(Json(serde_json::json!({
        "schema_version": "dasobjectstore.remote_readiness.v1",
        "store_id": response.store_id,
        "bucket": authorization.bucket,
        "s3_endpoint": format!("https://{host}:3900"),
        "region": "garage",
        "profile_binding": {
            "deployment_profile": response.deployment_profile,
            "host_mode": response.host_mode,
            "protection": response.protection,
            "lifecycle_state": response.lifecycle_state,
            "root_state": response.root_state,
        },
        "readable": authorization.can_read,
        "writable": authorization.can_write,
        "ssd_ingest_ready": ssd_ingest_ready,
        "catalogue_ready": response.ready,
        "capacity": response.capacity,
        "backpressured": !ssd_ingest_ready,
        "reason_codes": response.reasons,
    })))
}

async fn snapshot(
    State(state): State<RemoteControlRouteState>,
    Path(store_id): Path<String>,
    Query(query): Query<SnapshotQuery>,
    headers: HeaderMap,
) -> Result<Json<RemoteObjectSnapshotResponse>, RemoteControlRouteError> {
    state.guard.authorize(
        &headers,
        &store_id,
        &query.prefix,
        RemoteEasyconnectControlOperation::ObjectSnapshot,
    )?;
    let request = RemoteObjectSnapshotRequest {
        store_id: parse_store_id(store_id)?,
        prefix: query.prefix,
        cursor: query.cursor,
        limit: query.limit,
    };
    request
        .validate()
        .map_err(|error| RemoteControlRouteError::invalid(error.to_string()))?;
    Ok(Json(
        call_daemon(move |client| client.remote_object_snapshot(request)).await?,
    ))
}

async fn group_status(
    State(state): State<RemoteControlRouteState>,
    Path(store_id): Path<String>,
    Query(query): Query<GroupStatusQuery>,
    headers: HeaderMap,
) -> Result<Json<RemoteObjectGroupStatusResponse>, RemoteControlRouteError> {
    state.guard.authorize(
        &headers,
        &store_id,
        &query.key,
        RemoteEasyconnectControlOperation::ObjectGroupStatus,
    )?;
    let request = RemoteObjectGroupStatusRequest {
        store_id: parse_store_id(store_id)?,
        key: query.key,
    };
    request
        .validate()
        .map_err(|error| RemoteControlRouteError::invalid(error.to_string()))?;
    Ok(Json(
        call_daemon(move |client| client.remote_object_group_status(request)).await?,
    ))
}

async fn reconcile_s3(
    State(state): State<RemoteControlRouteState>,
    Path(store_id): Path<String>,
    headers: HeaderMap,
    Json(body): Json<ReconcileS3Body>,
) -> Result<Json<ReconcileS3Response>, RemoteControlRouteError> {
    let authorization = state.guard.authorize(
        &headers,
        &store_id,
        &body.key,
        RemoteEasyconnectControlOperation::ReconcileS3,
    )?;
    if body.ack_policy != "after_ssd_ingest" && body.ack_policy != "after_hdd_settlement" {
        return Err(RemoteControlRouteError::invalid(
            "ack_policy must be after_ssd_ingest or after_hdd_settlement".to_string(),
        ));
    }
    let store_id = parse_store_id(store_id)?;
    let request = StoreRepairRequest {
        store_id: Some(store_id.clone()),
        dry_run: false,
        confirmation: STORE_REPAIR_CONFIRMATION.to_string(),
        reconcile_s3: true,
        s3_prefix: Some(body.key.clone()),
        s3_expectation: Some(StoreRepairS3Expectation {
            payload_key: body.key.clone(),
            expected_bytes: body.expected_bytes,
            expected_sha256: body.expected_sha256.to_ascii_lowercase(),
        }),
        idempotency_key: Some(body.idempotency_key),
        // Remote S3 reconciliation is not a host-maintenance authority path.
        // The daemon rejects non-dry repair unless the fixed Pistis bridge
        // provides its verified subject.
        verified_subject: None,
    };
    request
        .validate()
        .map_err(|error| RemoteControlRouteError::invalid(error.to_string()))?;
    let operation_id = request
        .reconciliation_operation_id("remote")
        .map_err(|error| RemoteControlRouteError::invalid(error.to_string()))?;
    let mut operation = RemoteOperationRecord {
        schema_version: "dasobjectstore.remote_operation_record.v1".to_string(),
        operation_id: operation_id.clone(),
        store_id: store_id.clone(),
        key: body.key.clone(),
        actor: authorization.approved_actor,
        expected_bytes: body.expected_bytes,
        submitted_at_utc: dasobjectstore_daemon::SystemDaemonClock.now_utc(),
        ssd_acknowledged: false,
    };
    save_operation(&state.operation_dir, &operation)?;
    let repair = call_daemon_unbounded({
        let request = request.clone();
        move |client| client.store_repair(request)
    })
    .await?;
    let group = call_daemon({
        let store_id = store_id.clone();
        let key = body.key.clone();
        move |client| {
            client.remote_object_group_status(RemoteObjectGroupStatusRequest { store_id, key })
        }
    })
    .await?;
    let hdd_settled = group.durable;
    // Access the reconciliation result so a malformed daemon response cannot
    // be reported as an acknowledgement.
    if repair.s3_reconciliation.is_none() {
        return Err(RemoteControlRouteError::unavailable(
            "daemon did not return reconciliation evidence".to_string(),
        ));
    }
    operation.ssd_acknowledged = true;
    save_operation(&state.operation_dir, &operation)?;
    Ok(Json(ReconcileS3Response {
        schema_version: "dasobjectstore.remote_reconciliation.v1",
        operation_id,
        store_id,
        key: body.key,
        state: if hdd_settled {
            "hdd_settled"
        } else {
            "ssd_acknowledged"
        },
        ssd_acknowledged: true,
        hdd_settled,
    }))
}

async fn operation_status(
    State(state): State<RemoteControlRouteState>,
    Path(operation_id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<RemoteOperationResponse>, RemoteControlRouteError> {
    let store = headers
        .get("x-dasobjectstore-object-store")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| {
            RemoteControlRouteError::invalid(
                "x-dasobjectstore-object-store is required for operation status".to_string(),
            )
        })?;
    let authorization = state.guard.authorize(
        &headers,
        store,
        "",
        RemoteEasyconnectControlOperation::OperationStatus,
    )?;
    let store_id = parse_store_id(store.to_string())?;
    let operation_id = DaemonJobId::new(operation_id)
        .map_err(|error| RemoteControlRouteError::invalid(error.to_string()))?;
    let operation = load_operation(&state.operation_dir, &operation_id)?;
    if operation.store_id != store_id || operation.actor != authorization.approved_actor {
        return Err(RemoteControlRouteError::from(
            RemoteControlRejection::Unauthorized,
        ));
    }
    let status = call_daemon({
        let operation_id = operation_id.clone();
        move |client| {
            client.job_status(DaemonJobStatusRequest {
                job_id: operation_id,
            })
        }
    })
    .await?;
    let group = call_daemon({
        let store_id = store_id.clone();
        let key = operation.key.clone();
        move |client| {
            client.remote_object_group_status(RemoteObjectGroupStatusRequest { store_id, key })
        }
    })
    .await?;
    let ssd_acknowledged = operation.ssd_acknowledged
        || matches!(status.job.state, DaemonJobState::Complete)
        || status.job.progress.stage == "complete";
    Ok(Json(RemoteOperationResponse {
        schema_version: "dasobjectstore.remote_operation.v1",
        operation_id,
        store_id,
        state: status.job.state,
        queue_stage: status.job.progress.stage,
        bytes_received: status.job.progress.work_bytes_done,
        total_bytes: operation
            .expected_bytes
            .max(status.job.progress.work_bytes_total),
        elapsed_seconds: None,
        transfer_bytes_per_second: None,
        backpressure_reason: status.job.progress.message,
        ssd_acknowledged,
        hdd_settled: group.durable,
        failure: status.job.failure_message,
    }))
}

fn operation_path(directory: &FsPath, operation_id: &DaemonJobId) -> PathBuf {
    directory.join(format!("{}.json", operation_id.as_str()))
}

fn save_operation(
    directory: &FsPath,
    operation: &RemoteOperationRecord,
) -> Result<(), RemoteControlRouteError> {
    std::fs::create_dir_all(directory)
        .map_err(|error| RemoteControlRouteError::unavailable(error.to_string()))?;
    let path = operation_path(directory, &operation.operation_id);
    let temporary = path.with_extension(format!("json.tmp-{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(operation)
        .map_err(|error| RemoteControlRouteError::unavailable(error.to_string()))?;
    std::fs::write(&temporary, bytes)
        .and_then(|_| std::fs::rename(&temporary, &path))
        .map_err(|error| RemoteControlRouteError::unavailable(error.to_string()))
}

fn load_operation(
    directory: &FsPath,
    operation_id: &DaemonJobId,
) -> Result<RemoteOperationRecord, RemoteControlRouteError> {
    let bytes = std::fs::read(operation_path(directory, operation_id)).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            RemoteControlRouteError {
                status: axum::http::StatusCode::NOT_FOUND,
                code: "remote_operation_not_found",
                message: "remote operation was not found".to_string(),
                retry_after_seconds: None,
            }
        } else {
            RemoteControlRouteError::unavailable(error.to_string())
        }
    })?;
    serde_json::from_slice(&bytes)
        .map_err(|error| RemoteControlRouteError::unavailable(error.to_string()))
}

async fn call_daemon<T: Send + 'static>(
    call: impl FnOnce(
            DaemonClient<UnixSocketDaemonTransport>,
        ) -> Result<T, dasobjectstore_daemon::DaemonClientError>
        + Send
        + 'static,
) -> Result<T, RemoteControlRouteError> {
    tokio::task::spawn_blocking(move || {
        let client = DaemonClient::new(UnixSocketDaemonTransport::for_remote_control_bridge(
            DaemonRuntimeConfig::default_packaged().socket_path,
        ));
        call(client)
    })
    .await
    .map_err(|error| RemoteControlRouteError::unavailable(error.to_string()))?
    .map_err(RemoteControlRouteError::from_daemon)
}

async fn call_daemon_unbounded<T: Send + 'static>(
    call: impl FnOnce(
            DaemonClient<UnixSocketDaemonTransport>,
        ) -> Result<T, dasobjectstore_daemon::DaemonClientError>
        + Send
        + 'static,
) -> Result<T, RemoteControlRouteError> {
    tokio::task::spawn_blocking(move || {
        let client = DaemonClient::new(UnixSocketDaemonTransport::new(
            DaemonRuntimeConfig::default_packaged().socket_path,
        ));
        call(client)
    })
    .await
    .map_err(|error| RemoteControlRouteError::unavailable(error.to_string()))?
    .map_err(RemoteControlRouteError::from_daemon)
}

fn parse_store_id(value: String) -> Result<StoreId, RemoteControlRouteError> {
    StoreId::new(value).map_err(|error| RemoteControlRouteError::invalid(error.to_string()))
}

fn default_snapshot_limit() -> u32 {
    1_000
}

#[derive(Debug)]
pub struct RemoteControlRouteError {
    status: axum::http::StatusCode,
    code: &'static str,
    message: String,
    retry_after_seconds: Option<u64>,
}

impl RemoteControlRouteError {
    fn invalid(message: String) -> Self {
        Self {
            status: axum::http::StatusCode::BAD_REQUEST,
            code: "invalid_remote_control_request",
            message,
            retry_after_seconds: None,
        }
    }

    fn unavailable(message: String) -> Self {
        Self {
            status: axum::http::StatusCode::SERVICE_UNAVAILABLE,
            code: "remote_control_unavailable",
            message,
            retry_after_seconds: Some(2),
        }
    }

    fn from_daemon(error: DaemonClientError) -> Self {
        match error {
            DaemonClientError::RequestValidation(error) => Self::invalid(error.to_string()),
            DaemonClientError::JobValidation(error) => Self::invalid(error.to_string()),
            DaemonClientError::Api(error) => Self::from_daemon_api_code(&error.code),
            DaemonClientError::UnexpectedResponse { .. } => Self {
                status: axum::http::StatusCode::SERVICE_UNAVAILABLE,
                code: "daemon_version_mismatch",
                message: "the API service and daemon contracts are incompatible".to_string(),
                retry_after_seconds: None,
            },
            DaemonClientError::Transport(message) => {
                let lower = message.to_ascii_lowercase();
                if lower.contains("permission denied") {
                    Self {
                        status: axum::http::StatusCode::SERVICE_UNAVAILABLE,
                        code: "daemon_transport_permission_denied",
                        message: "the API service is not permitted to contact the daemon"
                            .to_string(),
                        retry_after_seconds: None,
                    }
                } else {
                    Self {
                        status: axum::http::StatusCode::SERVICE_UNAVAILABLE,
                        code: "daemon_transport_unavailable",
                        message: "the daemon transport is temporarily unavailable".to_string(),
                        retry_after_seconds: Some(2),
                    }
                }
            }
            DaemonClientError::Cancelled(_) => Self {
                status: axum::http::StatusCode::CONFLICT,
                code: "remote_control_cancelled",
                message: "the daemon operation was cancelled".to_string(),
                retry_after_seconds: None,
            },
        }
    }

    fn from_daemon_api_code(code: &str) -> Self {
        match code {
            "catalogue_locked" => Self {
                status: axum::http::StatusCode::SERVICE_UNAVAILABLE,
                code: "catalogue_locked",
                message: "the authoritative catalogue is temporarily locked".to_string(),
                retry_after_seconds: Some(2),
            },
            "catalogue_permission_denied" => Self {
                status: axum::http::StatusCode::SERVICE_UNAVAILABLE,
                code: "catalogue_permission_denied",
                message: "the daemon cannot read the authoritative catalogue".to_string(),
                retry_after_seconds: None,
            },
            "catalogue_unavailable" | "catalogue_query_failed" => Self {
                status: axum::http::StatusCode::SERVICE_UNAVAILABLE,
                code: "catalogue_unavailable",
                message: "the authoritative catalogue is unavailable".to_string(),
                retry_after_seconds: None,
            },
            "catalogue_invariant_violation" => Self {
                status: axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                code: "catalogue_invariant_violation",
                message: "the authoritative catalogue contains invalid metadata".to_string(),
                retry_after_seconds: None,
            },
            "server_busy" | "capacity_unavailable" | "capacity_admission_rejected" => Self {
                status: axum::http::StatusCode::TOO_MANY_REQUESTS,
                code: "storage_backpressure",
                message: "storage admission is temporarily backpressured".to_string(),
                retry_after_seconds: Some(5),
            },
            "unknown_command" | "unsupported_operation" | "invalid_request" => Self {
                status: axum::http::StatusCode::NOT_IMPLEMENTED,
                code: "unsupported_daemon_operation",
                message: "the running daemon does not support this operation".to_string(),
                retry_after_seconds: None,
            },
            "object_browser_access_denied" | "endpoint_access_denied" => Self {
                status: axum::http::StatusCode::FORBIDDEN,
                code: "remote_control_not_authorized",
                message: "the daemon denied access to this ObjectStore".to_string(),
                retry_after_seconds: None,
            },
            _ => Self {
                status: axum::http::StatusCode::SERVICE_UNAVAILABLE,
                code: "daemon_operation_failed",
                message: "the daemon rejected the remote operation".to_string(),
                retry_after_seconds: None,
            },
        }
    }
}

impl From<RemoteControlRejection> for RemoteControlRouteError {
    fn from(value: RemoteControlRejection) -> Self {
        let (status, code, message) = match value {
            RemoteControlRejection::MissingCredentials => (
                axum::http::StatusCode::UNAUTHORIZED,
                "remote_control_credentials_required",
                "temporary remote control credentials are required",
            ),
            RemoteControlRejection::MalformedCredentials => (
                axum::http::StatusCode::BAD_REQUEST,
                "invalid_remote_control_credentials",
                "remote control credentials are malformed",
            ),
            RemoteControlRejection::Unauthorized => (
                axum::http::StatusCode::FORBIDDEN,
                "remote_control_not_authorized",
                "the temporary session does not authorize this operation",
            ),
        };
        Self {
            status,
            code,
            message: message.to_string(),
            retry_after_seconds: None,
        }
    }
}

impl axum::response::IntoResponse for RemoteControlRouteError {
    fn into_response(self) -> axum::response::Response {
        let correlation_id = format!(
            "remote-{}-{}",
            std::process::id(),
            REMOTE_ERROR_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
        eprintln!(
            "remote control request failed correlation_id={} code={} retryable={}",
            correlation_id,
            self.code,
            self.retry_after_seconds.is_some()
        );
        let mut response = (
            self.status,
            Json(serde_json::json!({
                "schema_version": "dasobjectstore.remote_error.v1",
                "code": self.code,
                "message": self.message,
                "retryable": self.retry_after_seconds.is_some(),
                "retry_after_seconds": self.retry_after_seconds,
                "correlation_id": correlation_id,
            })),
        )
            .into_response();
        if let Ok(value) = correlation_id.parse() {
            response.headers_mut().insert("x-correlation-id", value);
        }
        if let Some(seconds) = self.retry_after_seconds {
            if let Ok(value) = seconds.to_string().parse() {
                response
                    .headers_mut()
                    .insert(axum::http::header::RETRY_AFTER, value);
            }
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::RemoteControlRouteError;
    use dasobjectstore_daemon::api::DaemonApiErrorResponse;
    use dasobjectstore_daemon::DaemonClientError;

    #[test]
    fn catalogue_lock_is_the_only_catalogue_failure_marked_retryable() {
        let locked = RemoteControlRouteError::from_daemon(DaemonClientError::Api(
            DaemonApiErrorResponse::new("catalogue_locked", "database is locked"),
        ));
        let denied = RemoteControlRouteError::from_daemon(DaemonClientError::Api(
            DaemonApiErrorResponse::new("catalogue_permission_denied", "permission denied"),
        ));
        let unavailable = RemoteControlRouteError::from_daemon(DaemonClientError::Api(
            DaemonApiErrorResponse::new("catalogue_unavailable", "cannot open"),
        ));

        assert_eq!(locked.code, "catalogue_locked");
        assert_eq!(locked.retry_after_seconds, Some(2));
        assert_eq!(denied.code, "catalogue_permission_denied");
        assert_eq!(denied.retry_after_seconds, None);
        assert_eq!(unavailable.code, "catalogue_unavailable");
        assert_eq!(unavailable.retry_after_seconds, None);
    }

    #[test]
    fn protocol_and_permission_failures_are_not_retryable() {
        let mismatch =
            RemoteControlRouteError::from_daemon(DaemonClientError::UnexpectedResponse {
                expected: "remote_object_snapshot",
                actual: "error",
            });
        let denied = RemoteControlRouteError::from_daemon(DaemonClientError::Transport(
            "Permission denied".to_string(),
        ));

        assert_eq!(mismatch.code, "daemon_version_mismatch");
        assert_eq!(mismatch.retry_after_seconds, None);
        assert_eq!(denied.code, "daemon_transport_permission_denied");
        assert_eq!(denied.retry_after_seconds, None);
    }
}
