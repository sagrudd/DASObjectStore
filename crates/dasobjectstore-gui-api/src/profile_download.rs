//! Authenticated HTTP-to-daemon provider-stream download adapter.
//!
//! The Web process does not open a profile backend or expose a managed path.
//! It waits for the daemon to accept the stream, then relays verified binary
//! frames through a bounded channel so a slow HTTP client applies backpressure
//! to the Unix-socket reader.

use super::{route_error, AuthRouteError, AuthenticatedGuiActor};
use axum::{
    body::{Body, Bytes},
    extract::{Path, Query},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::Response,
    Json,
};
use dasobjectstore_daemon::{
    DaemonClientError, DaemonRuntimeConfig, ProviderStreamCondition, ProviderStreamOpenRequest,
    ProviderStreamRange, UnixSocketDaemonTransport, PROVIDER_STREAM_MAX_CHUNK_BYTES,
    PROVIDER_STREAM_SCHEMA_VERSION,
};
use serde::Deserialize;
use std::io;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;

const DOWNLOAD_CHANNEL_CAPACITY: usize = 2;
const DOWNLOAD_DAEMON_DEADLINE: Duration = Duration::from_secs(300);
const DOWNLOAD_START_DEADLINE: Duration = Duration::from_secs(2);

#[derive(Debug)]
struct DownloadStartError {
    status: StatusCode,
    code: String,
    message: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(super) struct ProfileDownloadQuery {
    pub version: Option<u64>,
}

pub(super) async fn standalone_profile_s3_get(
    Path((store_id, object_id)): Path<(String, String)>,
    Query(query): Query<ProfileDownloadQuery>,
    headers: HeaderMap,
    _actor: AuthenticatedGuiActor,
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
    let range = parse_range(&headers)?;
    let condition = parse_condition(&headers)?;
    let request = ProviderStreamOpenRequest {
        schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
        request_id: uuid::Uuid::new_v4().to_string(),
        store_id,
        object: dasobjectstore_core::backend::BackendObjectKey {
            object_id,
            version: query.version.unwrap_or(1),
        },
        range,
        condition,
        chunk_size_bytes: PROVIDER_STREAM_MAX_CHUNK_BYTES,
    };
    request.validate().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "profile_s3_invalid_download",
            error.to_string(),
        )
    })?;

    let (sender, receiver) = mpsc::channel(DOWNLOAD_CHANNEL_CAPACITY);
    let (ready_sender, ready_receiver) = oneshot::channel::<Result<(), DownloadStartError>>();
    tokio::spawn(stream_from_daemon(request, sender, ready_sender));
    match tokio::time::timeout(DOWNLOAD_START_DEADLINE, ready_receiver).await {
        Ok(Ok(Ok(()))) => {}
        Ok(Ok(Err(error))) => {
            return Err(route_error(error.status, error.code, error.message));
        }
        Ok(Err(_)) => {
            return Err(route_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "profile_s3_daemon_unavailable",
                "daemon closed the provider stream before sending data",
            ));
        }
        Err(_) => {
            return Err(route_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "profile_s3_daemon_timeout",
                "daemon did not open the provider stream before the deadline",
            ));
        }
    }

    let mut response = Response::new(Body::from_stream(ReceiverStream::new(receiver)));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if range.is_some() {
        *response.status_mut() = StatusCode::PARTIAL_CONTENT;
        if let Some(range) = range.filter(|range| range.end_exclusive.is_some()) {
            let end = range.end_exclusive.expect("filtered range has an end") - 1;
            let value = format!("bytes {}-{end}/*", range.start);
            if let Ok(value) = HeaderValue::from_str(&value) {
                response.headers_mut().insert(header::CONTENT_RANGE, value);
            }
        }
    }
    Ok(response)
}

async fn stream_from_daemon(
    request: ProviderStreamOpenRequest,
    sender: mpsc::Sender<Result<Bytes, io::Error>>,
    ready_sender: oneshot::Sender<Result<(), DownloadStartError>>,
) {
    let bridge = crate::daemon_bridge::DaemonBridge::shared_packaged();
    let socket_path = DaemonRuntimeConfig::default_packaged().socket_path;
    let stream_sender = sender.clone();
    let result = bridge
        .call_message_with_deadline(DOWNLOAD_DAEMON_DEADLINE, move || {
            let mut ready_sender = Some(ready_sender);
            let body_sender = stream_sender.clone();
            let stream_result = UnixSocketDaemonTransport::new(socket_path).stream_provider(
                request,
                |_, payload| {
                    if let Some(sender) = ready_sender.take() {
                        let _ = sender.send(Ok(()));
                    }
                    stream_sender
                        .blocking_send(Ok(Bytes::copy_from_slice(payload)))
                        .map_err(|_| {
                            DaemonClientError::Cancelled(
                                "HTTP client closed the provider stream".to_string(),
                            )
                        })
                },
            );
            if let Err(error) = &stream_result {
                if let Some(sender) = ready_sender {
                    let _ = sender.send(Err(download_start_error(error)));
                } else {
                    let _ = body_sender.blocking_send(Err(io::Error::other(error.to_string())));
                }
            }
            stream_result.map(|_| ()).map_err(|error| error.to_string())
        })
        .await;

    if let Err(error) = result {
        let _ = sender.try_send(Err(io::Error::other(format!("{error:?}"))));
    }
}

fn download_start_error(error: &DaemonClientError) -> DownloadStartError {
    let (status, code) = match error {
        DaemonClientError::Api(error) => (
            match error.code.as_str() {
                "provider_stream_not_modified" => StatusCode::NOT_MODIFIED,
                "provider_stream_precondition_failed" => StatusCode::PRECONDITION_FAILED,
                "provider_stream_invalid_range" => StatusCode::RANGE_NOT_SATISFIABLE,
                "provider_stream_head_failed" => StatusCode::NOT_FOUND,
                _ => StatusCode::BAD_GATEWAY,
            },
            error.code.clone(),
        ),
        _ => (
            StatusCode::BAD_GATEWAY,
            "profile_s3_download_failed".to_string(),
        ),
    };
    DownloadStartError {
        status,
        code,
        message: error.to_string(),
    }
}

fn parse_range(
    headers: &HeaderMap,
) -> Result<Option<ProviderStreamRange>, (StatusCode, Json<AuthRouteError>)> {
    let Some(value) = headers.get(header::RANGE) else {
        return Ok(None);
    };
    let value = value.to_str().map_err(|error| {
        route_error(
            StatusCode::RANGE_NOT_SATISFIABLE,
            "profile_s3_invalid_range",
            error.to_string(),
        )
    })?;
    let Some(spec) = value.strip_prefix("bytes=") else {
        return Err(route_error(
            StatusCode::RANGE_NOT_SATISFIABLE,
            "profile_s3_invalid_range",
            "profile S3 downloads support a single bytes range",
        ));
    };
    if spec.contains(',') {
        return Err(route_error(
            StatusCode::RANGE_NOT_SATISFIABLE,
            "profile_s3_invalid_range",
            "profile S3 downloads do not support multipart ranges",
        ));
    }
    let (start, end) = spec.split_once('-').ok_or_else(|| {
        route_error(
            StatusCode::RANGE_NOT_SATISFIABLE,
            "profile_s3_invalid_range",
            "profile S3 range must use bytes=start-end",
        )
    })?;
    if start.is_empty() {
        return Err(route_error(
            StatusCode::RANGE_NOT_SATISFIABLE,
            "profile_s3_invalid_range",
            "suffix ranges are not supported",
        ));
    }
    let start = start.parse::<u64>().map_err(|error| {
        route_error(
            StatusCode::RANGE_NOT_SATISFIABLE,
            "profile_s3_invalid_range",
            error.to_string(),
        )
    })?;
    let end_exclusive = if end.is_empty() {
        None
    } else {
        let end = end.parse::<u64>().map_err(|error| {
            route_error(
                StatusCode::RANGE_NOT_SATISFIABLE,
                "profile_s3_invalid_range",
                error.to_string(),
            )
        })?;
        Some(end.checked_add(1).ok_or_else(|| {
            route_error(
                StatusCode::RANGE_NOT_SATISFIABLE,
                "profile_s3_invalid_range",
                "profile S3 range end overflows",
            )
        })?)
    };
    Ok(Some(ProviderStreamRange {
        start,
        end_exclusive,
    }))
}

fn parse_condition(
    headers: &HeaderMap,
) -> Result<ProviderStreamCondition, (StatusCode, Json<AuthRouteError>)> {
    Ok(ProviderStreamCondition {
        if_match_sha256: optional_checksum_header(headers, header::IF_MATCH)?,
        if_none_match_sha256: optional_checksum_header(headers, header::IF_NONE_MATCH)?,
    })
}

fn optional_checksum_header(
    headers: &HeaderMap,
    name: axum::http::header::HeaderName,
) -> Result<Option<String>, (StatusCode, Json<AuthRouteError>)> {
    let Some(value) = headers.get(name) else {
        return Ok(None);
    };
    let value = value.to_str().map_err(|error| {
        route_error(
            StatusCode::BAD_REQUEST,
            "profile_s3_invalid_condition",
            error.to_string(),
        )
    })?;
    let value = value.trim().trim_matches('"');
    if value == "*" {
        return Ok(None);
    }
    if !value.starts_with("sha256:") {
        return Err(route_error(
            StatusCode::BAD_REQUEST,
            "profile_s3_invalid_condition",
            "profile S3 conditional headers must carry a sha256: digest",
        ));
    }
    Ok(Some(value.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_open_ended_range() {
        let mut headers = HeaderMap::new();
        headers.insert(header::RANGE, HeaderValue::from_static("bytes=8-"));
        assert_eq!(
            parse_range(&headers).expect("range"),
            Some(ProviderStreamRange {
                start: 8,
                end_exclusive: None,
            })
        );
    }

    #[test]
    fn rejects_multipart_and_suffix_ranges() {
        for value in ["bytes=1-2,4-5", "bytes=-4"] {
            let mut headers = HeaderMap::new();
            headers.insert(header::RANGE, value.parse().expect("range"));
            assert_eq!(
                parse_range(&headers).expect_err("invalid range").0,
                StatusCode::RANGE_NOT_SATISFIABLE
            );
        }
    }

    #[test]
    fn parses_sha256_conditions_and_rejects_other_etags() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::IF_MATCH,
            HeaderValue::from_static(
                "\"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"",
            ),
        );
        let condition = parse_condition(&headers).expect("condition");
        assert_eq!(
            condition.if_match_sha256.as_deref(),
            Some("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );

        headers.insert(header::IF_MATCH, HeaderValue::from_static("\"etag\""));
        assert_eq!(
            parse_condition(&headers).expect_err("invalid condition").0,
            StatusCode::BAD_REQUEST
        );
    }
}
