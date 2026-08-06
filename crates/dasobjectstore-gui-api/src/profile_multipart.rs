//! Authenticated HTTP adapter for daemon-owned multipart completion.
//!
//! Multipart parts are staged through the daemon's provider stream boundary.
//! This route only submits the path-free completion manifest; the daemon
//! reopens its durable journal, verifies the staged parts, and commits the
//! catalogue record.

use super::{
    admin_daemon_bridge_error_with_code, require_preverified_host_operator_for_object_prefix,
    route_error, AuthRouteError, AuthenticatedGuiActor, VerifiedHostAuthenticatedContext,
    VerifiedHostObjectPrefixScope,
};
use axum::{
    body::Body,
    extract::{Extension, Path, Query},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use dasobjectstore_core::{backend::BackendObjectKey, ids::StoreId};
use dasobjectstore_daemon::api::{
    ProfileS3MultipartCompletionRequest, ProfileS3MultipartCompletionResponse,
    ProfileS3MultipartCompletionState, ProfileS3MultipartPartRequest,
    ProfileS3MultipartStatusRequest, ProfileS3MultipartStatusResponse, ProviderStreamChunkHeader,
    ProviderStreamMultipartPartUploadOpenRequest, PROVIDER_STREAM_MAX_CHUNK_BYTES,
    PROVIDER_STREAM_SCHEMA_VERSION,
};
use dasobjectstore_daemon::{
    DaemonApiResponse, DaemonClient, DaemonClientError, DaemonRuntimeConfig,
    UnixSocketDaemonTransport,
};
use serde::Deserialize;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::StreamExt;

const PART_UPLOAD_CHANNEL_CAPACITY: usize = 2;
const PART_UPLOAD_DAEMON_DEADLINE: Duration = Duration::from_secs(300);
const COMPLETION_MINIMUM_DEADLINE: Duration = Duration::from_secs(60);
const COMPLETION_MAXIMUM_DEADLINE: Duration = Duration::from_secs(6 * 60 * 60);
const COMPLETION_MINIMUM_BYTES_PER_SECOND: u64 = 8 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
pub(super) struct ProfileS3MultipartCompleteBody {
    pub key: BackendObjectKey,
    pub expected_size_bytes: u64,
    pub parts: Vec<ProfileS3MultipartPartRequest>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(super) struct ProfileS3MultipartPartQuery {
    pub key: Option<String>,
    pub version: Option<u64>,
}

pub(super) async fn standalone_profile_s3_multipart_part(
    Path((store_id, reservation_id, part_number)): Path<(String, String, u32)>,
    Query(query): Query<ProfileS3MultipartPartQuery>,
    headers: HeaderMap,
    _actor: AuthenticatedGuiActor,
    body: Body,
) -> Result<Response, (StatusCode, Json<AuthRouteError>)> {
    let store_id = store_id.parse::<StoreId>().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "profile_s3_invalid_store_id",
            error.to_string(),
        )
    })?;
    let object_id = query.key.ok_or_else(|| {
        route_error(
            StatusCode::BAD_REQUEST,
            "profile_s3_invalid_key",
            "multipart part upload requires a key query parameter",
        )
    })?;
    let expected_size_bytes = super::profile_upload::required_content_length(&headers)?;
    let request_id = super::profile_upload::required_header(&headers, "x-das-request-id")?;
    let expected_sha256 = super::profile_upload::required_header(&headers, "x-das-sha256")?;
    let reservation_size_bytes = required_u64_header(&headers, "x-das-reservation-size")?;
    let request = ProviderStreamMultipartPartUploadOpenRequest {
        schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
        request_id,
        reservation_id,
        reservation_size_bytes,
        part_number,
        store_id,
        object: BackendObjectKey {
            object_id,
            version: query.version.unwrap_or(1),
        },
        expected_size_bytes,
        expected_sha256,
        chunk_size_bytes: PROVIDER_STREAM_MAX_CHUNK_BYTES,
    };
    request.validate().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "profile_s3_invalid_multipart_part",
            error.to_string(),
        )
    })?;

    stream_profile_s3_multipart_part(request, body).await
}

/// Stream one multipart part only after the embedding host has supplied a
/// matching verified Pistis operator and a session-bound ObjectStore/prefix
/// grant.  The multipart adapter sends no delegated OS actor or provider
/// credential to the daemon.
pub(super) async fn preverified_host_profile_s3_multipart_part(
    Path((store_id, reservation_id, part_number)): Path<(String, String, u32)>,
    Query(query): Query<ProfileS3MultipartPartQuery>,
    headers: HeaderMap,
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<VerifiedHostAuthenticatedContext>,
    scope: Option<Extension<VerifiedHostObjectPrefixScope>>,
    body: Body,
) -> Result<Response, (StatusCode, Json<AuthRouteError>)> {
    let object_id = query.key.as_deref().ok_or_else(|| {
        route_error(
            StatusCode::BAD_REQUEST,
            "profile_s3_invalid_key",
            "multipart part upload requires a key query parameter",
        )
    })?;
    require_preverified_host_operator_for_object_prefix(
        &actor,
        &verified,
        scope.as_ref().map(|value| &value.0),
        &store_id,
        object_id,
    )?;
    standalone_profile_s3_multipart_part(
        Path((store_id, reservation_id, part_number)),
        Query(query),
        headers,
        actor,
        body,
    )
    .await
}

pub(super) async fn standalone_profile_s3_multipart_status(
    Path((store_id, reservation_id)): Path<(String, String)>,
    Query(query): Query<ProfileS3MultipartPartQuery>,
    _actor: AuthenticatedGuiActor,
) -> Result<Json<ProfileS3MultipartStatusResponse>, (StatusCode, Json<AuthRouteError>)> {
    let store_id = store_id.parse::<StoreId>().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "profile_s3_invalid_store_id",
            error.to_string(),
        )
    })?;
    let object_id = query.key.ok_or_else(|| {
        route_error(
            StatusCode::BAD_REQUEST,
            "profile_s3_invalid_key",
            "multipart status requires a key query parameter",
        )
    })?;
    let request = ProfileS3MultipartStatusRequest {
        store_id,
        reservation_id,
        key: BackendObjectKey {
            object_id,
            version: query.version.unwrap_or(1),
        },
    };
    request.validate().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "profile_s3_invalid_multipart_status",
            error.to_string(),
        )
    })?;
    crate::daemon_bridge::DaemonBridge::shared_priority_packaged()
        .call_with_deadline(Duration::from_secs(5), move || {
            DaemonClient::new(UnixSocketDaemonTransport::for_bounded_bridge(
                DaemonRuntimeConfig::default_packaged().socket_path,
            ))
            .profile_s3_multipart_status(request)
            .map_err(multipart_completion_client_error)
        })
        .await
        .map(Json)
        .map_err(multipart_completion_bridge_error)
}

/// Read multipart status only in the exact verified ObjectStore/prefix
/// envelope.  Status can expose object identity and is therefore not a
/// host-wide viewer operation.
pub(super) async fn preverified_host_profile_s3_multipart_status(
    Path((store_id, reservation_id)): Path<(String, String)>,
    Query(query): Query<ProfileS3MultipartPartQuery>,
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<VerifiedHostAuthenticatedContext>,
    scope: Option<Extension<VerifiedHostObjectPrefixScope>>,
) -> Result<Json<ProfileS3MultipartStatusResponse>, (StatusCode, Json<AuthRouteError>)> {
    let object_id = query.key.as_deref().ok_or_else(|| {
        route_error(
            StatusCode::BAD_REQUEST,
            "profile_s3_invalid_key",
            "multipart status requires a key query parameter",
        )
    })?;
    require_preverified_host_operator_for_object_prefix(
        &actor,
        &verified,
        scope.as_ref().map(|value| &value.0),
        &store_id,
        object_id,
    )?;
    standalone_profile_s3_multipart_status(Path((store_id, reservation_id)), Query(query), actor)
        .await
}

pub(crate) async fn stream_profile_s3_multipart_part(
    request: ProviderStreamMultipartPartUploadOpenRequest,
    body: Body,
) -> Result<Response, (StatusCode, Json<AuthRouteError>)> {
    let (sender, receiver) = mpsc::channel(PART_UPLOAD_CHANNEL_CAPACITY);
    let (admitted_sender, admitted_receiver) = oneshot::channel();
    let mut upload_task = tokio::spawn(upload_multipart_part_to_daemon(
        request.clone(),
        admitted_sender,
        receiver,
    ));
    tokio::select! {
        admitted = admitted_receiver => {
            if admitted.is_err() {
                return Err(upload_task_ended_before_body(upload_task.await));
            }
        },
        result = &mut upload_task => return Err(upload_task_ended_before_body(result)),
    }
    let mut body_stream = body.into_data_stream();
    let mut offset = 0_u64;
    while let Some(result) = body_stream.next().await {
        let bytes = result.map_err(|error| {
            route_error(
                StatusCode::BAD_REQUEST,
                "profile_s3_body_read_failed",
                error.to_string(),
            )
        })?;
        let mut start = 0;
        while start < bytes.len() {
            let end = (start + PROVIDER_STREAM_MAX_CHUNK_BYTES as usize).min(bytes.len());
            let payload = bytes.slice(start..end).to_vec();
            let payload_len = payload.len() as u32;
            let header = ProviderStreamChunkHeader {
                schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
                request_id: request.request_id.clone(),
                offset,
                payload_len,
                final_chunk: false,
                total_size: None,
                sha256: None,
            };
            offset = offset.checked_add(payload_len as u64).ok_or_else(|| {
                route_error(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "profile_s3_size_overflow",
                    "multipart part size overflow",
                )
            })?;
            if offset > request.expected_size_bytes {
                return Err(route_error(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "profile_s3_size_exceeded",
                    "multipart part body exceeds Content-Length",
                ));
            }
            if sender.send(Ok((header, payload))).await.is_err() {
                return Err(upload_task_ended_before_body(upload_task.await));
            }
            start = end;
        }
    }
    if offset != request.expected_size_bytes {
        return Err(route_error(
            StatusCode::LENGTH_REQUIRED,
            "profile_s3_content_length_mismatch",
            format!(
                "multipart part body ended at {offset} bytes, expected {}",
                request.expected_size_bytes
            ),
        ));
    }
    let terminal = ProviderStreamChunkHeader {
        schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
        request_id: request.request_id.clone(),
        offset,
        payload_len: 0,
        final_chunk: true,
        total_size: Some(offset),
        sha256: Some(request.expected_sha256.clone()),
    };
    if sender.send(Ok((terminal, Vec::new()))).await.is_err() {
        return Err(upload_task_ended_before_body(upload_task.await));
    }
    drop(sender);

    let response = upload_task.await.map_err(|error| {
        route_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "profile_s3_daemon_upload_join_failed",
            error.to_string(),
        )
    })?;
    let response = response.map_err(|error| {
        super::profile_upload::daemon_stream_bridge_error(error, "profile_s3_multipart_failed")
    })?;
    match response {
        DaemonApiResponse::ProviderStreamMultipartPartUpload(response) => {
            Ok(Json(response).into_response())
        }
        DaemonApiResponse::Error(error) => Err(route_error(
            super::profile_upload::daemon_stream_status(&error.code),
            error.code,
            error.message,
        )),
        response => Err(route_error(
            StatusCode::BAD_GATEWAY,
            "profile_s3_unexpected_response",
            format!("daemon returned an unexpected response: {response:?}"),
        )),
    }
}

async fn upload_multipart_part_to_daemon(
    request: ProviderStreamMultipartPartUploadOpenRequest,
    admitted_sender: oneshot::Sender<()>,
    mut receiver: mpsc::Receiver<Result<(ProviderStreamChunkHeader, Vec<u8>), String>>,
) -> Result<DaemonApiResponse, crate::daemon_bridge::DaemonBridgeError> {
    let bridge = crate::daemon_bridge::DaemonBridge::shared_packaged();
    let socket_path = DaemonRuntimeConfig::default_packaged().socket_path;
    bridge
        .call_message_with_deadline(PART_UPLOAD_DAEMON_DEADLINE, move || {
            UnixSocketDaemonTransport::new(socket_path)
                .upload_multipart_part_after_admission(
                    request,
                    || {
                        admitted_sender.send(()).map_err(|_| {
                            DaemonClientError::Transport(
                                "HTTP multipart request ended before daemon admission".to_string(),
                            )
                        })
                    },
                    || match receiver.blocking_recv() {
                        Some(Ok(frame)) => Ok(Some(frame)),
                        Some(Err(error)) => Err(DaemonClientError::Transport(error)),
                        None => Ok(None),
                    },
                )
                .map_err(|error| error.to_string())
        })
        .await
}

fn upload_task_ended_before_body(
    result: Result<
        Result<DaemonApiResponse, crate::daemon_bridge::DaemonBridgeError>,
        tokio::task::JoinError,
    >,
) -> (StatusCode, Json<AuthRouteError>) {
    match result {
        Ok(Ok(DaemonApiResponse::Error(error))) => route_error(
            super::profile_upload::daemon_stream_status(&error.code),
            error.code,
            error.message,
        ),
        Ok(Ok(response)) => route_error(
            StatusCode::BAD_GATEWAY,
            "profile_s3_unexpected_response",
            format!("daemon multipart upload ended before body admission: {response:?}"),
        ),
        Ok(Err(error)) => {
            super::profile_upload::daemon_stream_bridge_error(error, "profile_s3_multipart_failed")
        }
        Err(error) => route_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "profile_s3_daemon_upload_join_failed",
            error.to_string(),
        ),
    }
}

fn required_u64_header(
    headers: &HeaderMap,
    name: &'static str,
) -> Result<u64, (StatusCode, Json<AuthRouteError>)> {
    let value = super::profile_upload::required_header(headers, name)?;
    value.parse::<u64>().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "profile_s3_upload_header_invalid",
            error.to_string(),
        )
    })
}

pub(super) async fn standalone_profile_s3_multipart_complete(
    Path((store_id, reservation_id)): Path<(String, String)>,
    _actor: AuthenticatedGuiActor,
    Json(body): Json<ProfileS3MultipartCompleteBody>,
) -> Result<
    axum::Json<ProfileS3MultipartCompletionResponse>,
    (StatusCode, axum::Json<AuthRouteError>),
> {
    let store_id = store_id.parse::<StoreId>().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "profile_s3_invalid_store_id",
            error.to_string(),
        )
    })?;
    if reservation_id.trim().is_empty() {
        return Err(route_error(
            StatusCode::BAD_REQUEST,
            "profile_s3_invalid_reservation",
            "multipart completion requires a reservation id",
        ));
    }

    let request = ProfileS3MultipartCompletionRequest {
        store_id,
        reservation_id,
        key: body.key,
        expected_size_bytes: body.expected_size_bytes,
        parts: body.parts,
    };
    request.validate().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "profile_s3_invalid_multipart_completion",
            error.to_string(),
        )
    })?;

    complete_profile_s3_multipart(request).await
}

/// Complete a multipart reservation only in the exact verified
/// subject/session/ObjectStore/prefix envelope.  Completion is a daemon-owned
/// commit; no local or provider authority is accepted by this host route.
pub(super) async fn preverified_host_profile_s3_multipart_complete(
    Path((store_id, reservation_id)): Path<(String, String)>,
    actor: AuthenticatedGuiActor,
    Extension(verified): Extension<VerifiedHostAuthenticatedContext>,
    scope: Option<Extension<VerifiedHostObjectPrefixScope>>,
    Json(body): Json<ProfileS3MultipartCompleteBody>,
) -> Result<
    axum::Json<ProfileS3MultipartCompletionResponse>,
    (StatusCode, axum::Json<AuthRouteError>),
> {
    require_preverified_host_operator_for_object_prefix(
        &actor,
        &verified,
        scope.as_ref().map(|value| &value.0),
        &store_id,
        &body.key.object_id,
    )?;
    standalone_profile_s3_multipart_complete(Path((store_id, reservation_id)), actor, Json(body))
        .await
}

pub(crate) async fn complete_profile_s3_multipart(
    request: ProfileS3MultipartCompletionRequest,
) -> Result<
    axum::Json<ProfileS3MultipartCompletionResponse>,
    (StatusCode, axum::Json<AuthRouteError>),
> {
    let deadline = multipart_completion_deadline(request.expected_size_bytes);
    let expires = tokio::time::Instant::now() + deadline;
    loop {
        let request_for_attempt = request.clone();
        let attempt = crate::daemon_bridge::DaemonBridge::shared_multipart_completion_packaged()
            .call_with_deadline(Duration::from_secs(15), move || {
                let client =
                    DaemonClient::new(UnixSocketDaemonTransport::for_multipart_completion(
                        DaemonRuntimeConfig::default_packaged().socket_path,
                    ));
                client
                    .profile_s3_multipart_complete(request_for_attempt)
                    .map_err(multipart_completion_client_error)
            })
            .await;
        let response = match attempt {
            Ok(response) => response,
            Err(error) if multipart_completion_poll_error_is_retryable(&error) => {
                if tokio::time::Instant::now() >= expires {
                    return Err(route_error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "profile_s3_multipart_in_progress",
                        "multipart completion remains durable; reconnect and retry the same request",
                    ));
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
                continue;
            }
            Err(error) => return Err(multipart_completion_bridge_error(error)),
        };
        match response.status.as_ref().map(|status| status.state) {
            Some(ProfileS3MultipartCompletionState::Committed) if response.committed => {
                return Ok(axum::Json(response));
            }
            None if response.committed => {
                return Ok(axum::Json(response));
            }
            Some(ProfileS3MultipartCompletionState::FailedTerminal) => {
                let message = response
                    .status
                    .as_ref()
                    .and_then(|status| status.error.as_ref())
                    .map(|error| error.message.clone())
                    .unwrap_or_else(|| "multipart completion failed terminally".to_string());
                return Err(route_error(
                    StatusCode::CONFLICT,
                    "profile_s3_multipart_failed_terminal",
                    message,
                ));
            }
            Some(ProfileS3MultipartCompletionState::FailedRetryable)
            | Some(ProfileS3MultipartCompletionState::Accepted)
            | Some(ProfileS3MultipartCompletionState::InProgress)
            | Some(ProfileS3MultipartCompletionState::Committed)
            | None => {}
        }
        if tokio::time::Instant::now() >= expires {
            return Err(route_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "profile_s3_multipart_in_progress",
                "multipart completion remains durable and in progress; retry the same request",
            ));
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

fn multipart_completion_poll_error_is_retryable(
    error: &crate::daemon_bridge::DaemonBridgeError,
) -> bool {
    use crate::daemon_bridge::DaemonBridgeError;

    match error {
        DaemonBridgeError::Busy
        | DaemonBridgeError::CircuitOpen
        | DaemonBridgeError::Deadline
        | DaemonBridgeError::Join(_) => true,
        DaemonBridgeError::Client(error) => {
            error.code == "profile_s3_multipart_transport_failed"
                || (error.status == StatusCode::SERVICE_UNAVAILABLE
                    && matches!(
                        error.code.as_str(),
                        "profile_s3_multipart_slow_down"
                            | "profile_s3_multipart_unavailable"
                            | "daemon_bridge_transport_failed"
                    ))
        }
    }
}

fn multipart_completion_client_error(
    error: DaemonClientError,
) -> crate::object_browser_routes::StandaloneObjectBrowserClientError {
    use crate::object_browser_routes::StandaloneObjectBrowserClientError;

    let message = error.to_string();
    match error {
        DaemonClientError::RequestValidation(_) => StandaloneObjectBrowserClientError {
            status: StatusCode::BAD_REQUEST,
            code: "profile_s3_invalid_multipart_completion".to_string(),
            message,
        },
        DaemonClientError::Api(error) => StandaloneObjectBrowserClientError {
            status: multipart_daemon_error_status(&error.code),
            code: error.code,
            message: error.message,
        },
        DaemonClientError::Transport(_) => StandaloneObjectBrowserClientError {
            status: StatusCode::BAD_GATEWAY,
            code: "profile_s3_multipart_transport_failed".to_string(),
            message,
        },
        _ => StandaloneObjectBrowserClientError {
            status: StatusCode::BAD_GATEWAY,
            code: "profile_s3_multipart_complete_failed".to_string(),
            message,
        },
    }
}

fn multipart_completion_bridge_error(
    error: crate::daemon_bridge::DaemonBridgeError,
) -> (StatusCode, axum::Json<AuthRouteError>) {
    match error {
        crate::daemon_bridge::DaemonBridgeError::Client(error) => {
            route_error(error.status, error.code, error.message)
        }
        crate::daemon_bridge::DaemonBridgeError::Busy => route_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "profile_s3_multipart_slow_down",
            "multipart completion capacity is saturated; retry shortly",
        ),
        error => admin_daemon_bridge_error_with_code(error, "profile_s3_multipart_complete_failed"),
    }
}

fn multipart_daemon_error_status(code: &str) -> StatusCode {
    match code {
        "profile_s3_multipart_completion_conflict" => StatusCode::CONFLICT,
        "profile_s3_multipart_incomplete" | "profile_s3_invalid_multipart_completion" => {
            StatusCode::BAD_REQUEST
        }
        "profile_s3_multipart_unavailable"
        | "profile_s3_unavailable"
        | "profile_s3_multipart_publication_failed"
        | "profile_s3_multipart_recovery_failed" => StatusCode::SERVICE_UNAVAILABLE,
        _ => StatusCode::BAD_GATEWAY,
    }
}

fn multipart_completion_deadline(size_bytes: u64) -> Duration {
    let copy_seconds = size_bytes.saturating_add(COMPLETION_MINIMUM_BYTES_PER_SECOND - 1)
        / COMPLETION_MINIMUM_BYTES_PER_SECOND;
    COMPLETION_MINIMUM_DEADLINE
        .saturating_add(Duration::from_secs(copy_seconds))
        .min(COMPLETION_MAXIMUM_DEADLINE)
}

#[cfg(test)]
mod completion_deadline_tests {
    use super::*;

    #[test]
    fn ten_gib_completion_has_a_size_aware_deadline() {
        let deadline = multipart_completion_deadline(10 * 1024 * 1024 * 1024);
        assert!(deadline > Duration::from_secs(2));
        assert_eq!(deadline, Duration::from_secs(60 + 1_280));
    }

    #[test]
    fn completion_deadline_is_bounded() {
        assert_eq!(
            multipart_completion_deadline(u64::MAX),
            COMPLETION_MAXIMUM_DEADLINE
        );
    }

    #[test]
    fn completion_conflict_preserves_a_deterministic_http_status() {
        assert_eq!(
            multipart_daemon_error_status("profile_s3_multipart_completion_conflict"),
            StatusCode::CONFLICT
        );
        assert_eq!(
            multipart_daemon_error_status("profile_s3_multipart_unavailable"),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }

    #[test]
    fn polling_reconnects_after_transport_and_bridge_failures() {
        use crate::daemon_bridge::DaemonBridgeError;
        use crate::object_browser_routes::StandaloneObjectBrowserClientError;

        assert!(multipart_completion_poll_error_is_retryable(
            &DaemonBridgeError::Deadline
        ));
        assert!(multipart_completion_poll_error_is_retryable(
            &DaemonBridgeError::CircuitOpen
        ));
        assert!(multipart_completion_poll_error_is_retryable(
            &DaemonBridgeError::Client(StandaloneObjectBrowserClientError {
                status: StatusCode::BAD_GATEWAY,
                code: "profile_s3_multipart_transport_failed".to_string(),
                message: "daemon transport failed".to_string(),
            })
        ));
        assert!(!multipart_completion_poll_error_is_retryable(
            &DaemonBridgeError::Client(StandaloneObjectBrowserClientError {
                status: StatusCode::CONFLICT,
                code: "profile_s3_multipart_completion_conflict".to_string(),
                message: "intent changed".to_string(),
            })
        ));
    }
}
