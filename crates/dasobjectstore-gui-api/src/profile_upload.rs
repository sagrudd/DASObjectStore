//! Authenticated HTTP-to-daemon provider-stream upload adapter.
//!
//! The Web process never opens a profile backend or writes a managed root. It
//! only translates a bounded HTTP body into the daemon's framed Unix-socket
//! protocol, with a small channel providing backpressure while the daemon
//! performs authorization, reservation, verification, and catalogue commit.

use super::{
    admin_daemon_bridge_error_with_code, route_error, AuthRouteError, AuthenticatedGuiActor,
};
use axum::{
    body::Body,
    extract::{Path, Query},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use dasobjectstore_daemon::{
    DaemonApiResponse, DaemonClientError, DaemonRuntimeConfig, ProviderStreamChunkHeader,
    ProviderStreamUploadOpenRequest, UnixSocketDaemonTransport, PROVIDER_STREAM_MAX_CHUNK_BYTES,
    PROVIDER_STREAM_SCHEMA_VERSION,
};
use serde::Deserialize;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;

const UPLOAD_CHANNEL_CAPACITY: usize = 2;
const UPLOAD_DAEMON_DEADLINE: Duration = Duration::from_secs(300);

#[derive(Clone, Debug, Default, Deserialize)]
pub(super) struct ProfileUploadQuery {
    pub version: Option<u64>,
}

pub(super) async fn standalone_profile_s3_put(
    Path((store_id, object_id)): Path<(String, String)>,
    Query(query): Query<ProfileUploadQuery>,
    headers: HeaderMap,
    _actor: AuthenticatedGuiActor,
    body: Body,
) -> Result<Response, (StatusCode, Json<AuthRouteError>)> {
    let store_id = store_id
        .parse::<dasobjectstore_core::ids::StoreId>()
        .map_err(|error| {
            route_error(
                StatusCode::BAD_REQUEST,
                "profile_s3_invalid_store_id",
                error.to_string(),
            )
        })?;
    let expected_size_bytes = required_content_length(&headers)?;
    let upload_id = required_header(&headers, "x-das-upload-id")?;
    let request_id = required_header(&headers, "x-das-request-id")?;
    let expected_sha256 = required_header(&headers, "x-das-sha256")?;
    let request = ProviderStreamUploadOpenRequest {
        schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
        request_id,
        upload_id,
        store_id,
        object: dasobjectstore_core::backend::BackendObjectKey {
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
            "profile_s3_invalid_upload",
            error.to_string(),
        )
    })?;

    stream_profile_s3_put(request, body).await
}

pub(crate) async fn stream_profile_s3_put(
    request: ProviderStreamUploadOpenRequest,
    body: Body,
) -> Result<Response, (StatusCode, Json<AuthRouteError>)> {
    let (sender, receiver) = mpsc::channel(UPLOAD_CHANNEL_CAPACITY);
    let upload_task = tokio::spawn(upload_to_daemon(request.clone(), receiver));
    let mut body_stream = body.into_data_stream();
    let mut offset = 0_u64;
    while let Some(result) = body_stream.next().await {
        let bytes = match result {
            Ok(bytes) => bytes,
            Err(error) => {
                drop(sender);
                let _ = upload_task.await;
                return Err(route_error(
                    StatusCode::BAD_REQUEST,
                    "profile_s3_body_read_failed",
                    error.to_string(),
                ));
            }
        };
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
            let Some(next_offset) = offset.checked_add(payload_len as u64) else {
                drop(sender);
                let _ = upload_task.await;
                return Err(route_error(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "profile_s3_size_overflow",
                    "profile S3 upload size overflow",
                ));
            };
            offset = next_offset;
            if offset > request.expected_size_bytes {
                drop(sender);
                let _ = upload_task.await;
                return Err(route_error(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "profile_s3_size_exceeded",
                    "profile S3 request body exceeds Content-Length",
                ));
            }
            if sender.send(Ok((header, payload))).await.is_err() {
                let _ = upload_task.await;
                return Err(route_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "profile_s3_daemon_unavailable",
                    "daemon upload stream closed before body completion",
                ));
            }
            start = end;
        }
    }

    if offset != request.expected_size_bytes {
        drop(sender);
        let _ = upload_task.await;
        return Err(route_error(
            StatusCode::LENGTH_REQUIRED,
            "profile_s3_content_length_mismatch",
            format!(
                "request body ended at {offset} bytes, expected {}",
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
        let _ = upload_task.await;
        return Err(route_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "profile_s3_daemon_unavailable",
            "daemon upload stream closed before acknowledgement",
        ));
    }
    drop(sender);

    let response = upload_task.await.map_err(|error| {
        route_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "profile_s3_daemon_upload_join_failed",
            error.to_string(),
        )
    })?;
    let response = response
        .map_err(|error| admin_daemon_bridge_error_with_code(error, "profile_s3_upload_failed"))?;
    match response {
        DaemonApiResponse::ProviderStreamUpload(response) => Ok(Json(response).into_response()),
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

async fn upload_to_daemon(
    request: ProviderStreamUploadOpenRequest,
    mut receiver: mpsc::Receiver<Result<(ProviderStreamChunkHeader, Vec<u8>), String>>,
) -> Result<DaemonApiResponse, crate::daemon_bridge::DaemonBridgeError> {
    let bridge = crate::daemon_bridge::DaemonBridge::shared_packaged();
    let socket_path = DaemonRuntimeConfig::default_packaged().socket_path;
    bridge
        .call_message_with_deadline(UPLOAD_DAEMON_DEADLINE, move || {
            UnixSocketDaemonTransport::new(socket_path)
                .upload_provider(request, || match receiver.blocking_recv() {
                    Some(Ok(frame)) => Ok(Some(frame)),
                    Some(Err(error)) => Err(DaemonClientError::Transport(error)),
                    None => Ok(None),
                })
                .map_err(|error| error.to_string())
        })
        .await
}

pub(super) fn required_content_length(
    headers: &HeaderMap,
) -> Result<u64, (StatusCode, Json<AuthRouteError>)> {
    let value = headers.get(header::CONTENT_LENGTH).ok_or_else(|| {
        route_error(
            StatusCode::LENGTH_REQUIRED,
            "profile_s3_content_length_required",
            "profile S3 uploads require an explicit Content-Length",
        )
    })?;
    let value = value.to_str().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "profile_s3_invalid_content_length",
            error.to_string(),
        )
    })?;
    value.parse::<u64>().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "profile_s3_invalid_content_length",
            error.to_string(),
        )
    })
}

pub(super) fn required_header(
    headers: &HeaderMap,
    name: &'static str,
) -> Result<String, (StatusCode, Json<AuthRouteError>)> {
    let value = headers.get(name).ok_or_else(|| {
        route_error(
            StatusCode::BAD_REQUEST,
            "profile_s3_upload_header_required",
            format!("profile S3 upload requires {name}"),
        )
    })?;
    let value = value.to_str().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "profile_s3_upload_header_invalid",
            error.to_string(),
        )
    })?;
    if value.trim().is_empty() {
        return Err(route_error(
            StatusCode::BAD_REQUEST,
            "profile_s3_upload_header_invalid",
            format!("profile S3 upload header {name} must not be blank"),
        ));
    }
    Ok(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AuthenticatedActorAuthority;

    fn actor() -> AuthenticatedGuiActor {
        AuthenticatedGuiActor {
            subject_id: "tester".to_string(),
            authority: AuthenticatedActorAuthority::LocalStandalone,
            roles: Vec::new(),
            expires_at_unix_seconds: None,
            correlation_id: None,
        }
    }

    #[tokio::test]
    async fn rejects_missing_content_length_before_daemon_dispatch() {
        let result = standalone_profile_s3_put(
            Path(("store".to_string(), "objects/file".to_string())),
            Query(ProfileUploadQuery::default()),
            HeaderMap::new(),
            actor(),
            Body::from("hello"),
        )
        .await;
        assert_eq!(
            result.expect_err("missing length").0,
            StatusCode::LENGTH_REQUIRED
        );
    }

    #[tokio::test]
    async fn rejects_invalid_checksum_before_daemon_dispatch() {
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_LENGTH, "5".parse().expect("length"));
        headers.insert("x-das-upload-id", "upload-1".parse().expect("upload id"));
        headers.insert("x-das-request-id", "request-1".parse().expect("request id"));
        headers.insert(
            "x-das-sha256",
            "sha256:not-a-digest".parse().expect("checksum"),
        );
        let result = standalone_profile_s3_put(
            Path(("store".to_string(), "objects/file".to_string())),
            Query(ProfileUploadQuery::default()),
            headers,
            actor(),
            Body::from("hello"),
        )
        .await;
        assert_eq!(
            result.expect_err("invalid checksum").0,
            StatusCode::BAD_REQUEST
        );
    }
}
