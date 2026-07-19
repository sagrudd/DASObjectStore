//! Authenticated HTTP adapter for daemon-owned multipart completion.
//!
//! Multipart parts are staged through the daemon's provider stream boundary.
//! This route only submits the path-free completion manifest; the daemon
//! reopens its durable journal, verifies the staged parts, and commits the
//! catalogue record.

use super::{
    admin_daemon_bridge_error_with_code, route_error, AuthRouteError, AuthenticatedGuiActor,
};
use axum::{
    body::Body,
    extract::{Path, Query},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use dasobjectstore_core::{backend::BackendObjectKey, ids::StoreId};
use dasobjectstore_daemon::api::{
    ProfileS3MultipartCompletionRequest, ProfileS3MultipartCompletionResponse,
    ProfileS3MultipartPartRequest, ProviderStreamChunkHeader,
    ProviderStreamMultipartPartUploadOpenRequest, PROVIDER_STREAM_MAX_CHUNK_BYTES,
    PROVIDER_STREAM_SCHEMA_VERSION,
};
use dasobjectstore_daemon::{
    DaemonApiResponse, DaemonClient, DaemonClientError, DaemonRuntimeConfig,
    UnixSocketDaemonTransport,
};
use serde::Deserialize;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;

const PART_UPLOAD_CHANNEL_CAPACITY: usize = 2;
const PART_UPLOAD_DAEMON_DEADLINE: Duration = Duration::from_secs(300);

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

pub(crate) async fn stream_profile_s3_multipart_part(
    request: ProviderStreamMultipartPartUploadOpenRequest,
    body: Body,
) -> Result<Response, (StatusCode, Json<AuthRouteError>)> {
    let (sender, receiver) = mpsc::channel(PART_UPLOAD_CHANNEL_CAPACITY);
    let upload_task = tokio::spawn(upload_multipart_part_to_daemon(request.clone(), receiver));
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
            sender.send(Ok((header, payload))).await.map_err(|_| {
                route_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "profile_s3_daemon_unavailable",
                    "daemon multipart stream closed before body completion",
                )
            })?;
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
    sender.send(Ok((terminal, Vec::new()))).await.map_err(|_| {
        route_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "profile_s3_daemon_unavailable",
            "daemon multipart stream closed before acknowledgement",
        )
    })?;
    drop(sender);

    let response = upload_task.await.map_err(|error| {
        route_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "profile_s3_daemon_upload_join_failed",
            error.to_string(),
        )
    })?;
    let response = response.map_err(|error| {
        admin_daemon_bridge_error_with_code(error, "profile_s3_multipart_failed")
    })?;
    match response {
        DaemonApiResponse::ProviderStreamMultipartPartUpload(response) => {
            Ok(Json(response).into_response())
        }
        DaemonApiResponse::Error(error) => Err(route_error(
            StatusCode::BAD_GATEWAY,
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
    mut receiver: mpsc::Receiver<Result<(ProviderStreamChunkHeader, Vec<u8>), String>>,
) -> Result<DaemonApiResponse, crate::daemon_bridge::DaemonBridgeError> {
    let bridge = crate::daemon_bridge::DaemonBridge::shared_packaged();
    let socket_path = DaemonRuntimeConfig::default_packaged().socket_path;
    bridge
        .call_message_with_deadline(PART_UPLOAD_DAEMON_DEADLINE, move || {
            UnixSocketDaemonTransport::new(socket_path)
                .upload_multipart_part(request, || match receiver.blocking_recv() {
                    Some(Ok(frame)) => Ok(Some(frame)),
                    Some(Err(error)) => Err(DaemonClientError::Transport(error)),
                    None => Ok(None),
                })
                .map_err(|error| error.to_string())
        })
        .await
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

pub(crate) async fn complete_profile_s3_multipart(
    request: ProfileS3MultipartCompletionRequest,
) -> Result<
    axum::Json<ProfileS3MultipartCompletionResponse>,
    (StatusCode, axum::Json<AuthRouteError>),
> {
    crate::daemon_bridge::DaemonBridge::shared_packaged()
        .call_message(move || {
            let client = DaemonClient::new(UnixSocketDaemonTransport::for_bounded_bridge(
                DaemonRuntimeConfig::default_packaged().socket_path,
            ));
            client
                .profile_s3_multipart_complete(request)
                .map_err(|error| error.to_string())
        })
        .await
        .map(axum::Json)
        .map_err(|error| {
            admin_daemon_bridge_error_with_code(error, "profile_s3_multipart_complete_failed")
        })
}
