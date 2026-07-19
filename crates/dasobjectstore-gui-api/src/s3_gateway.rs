//! Feature-gated S3 protocol ingress gateway.
//!
//! The gateway authenticates the AWS request against the daemon-managed
//! Garage credential registry, derives the ObjectStore exclusively from that
//! credential and bucket, then streams the body through the daemon provider
//! protocol. It never accepts a filesystem path or placement choice.

use crate::auth_routes::profile_multipart::{
    complete_profile_s3_multipart, stream_profile_s3_multipart_part,
};
use crate::auth_routes::profile_upload::stream_profile_s3_put;
use crate::auth_routes::provider_stream_download;
use crate::s3_gateway_auth::{verify_s3_sigv4, S3SigV4Request};
use axum::body::Body;
use axum::extract::{OriginalUri, Path, State};
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use dasobjectstore_core::backend::BackendObjectKey;
use dasobjectstore_daemon::api::{
    ProfileS3MultipartAbortRequest, ProfileS3MultipartCompletionRequest,
    ProfileS3MultipartPartRequest, ProviderStreamMultipartPartUploadOpenRequest,
};
use dasobjectstore_daemon::{
    DaemonClient, DaemonRuntimeConfig, ProfileS3HeadRequest, ProfileS3ListRequest,
    ProviderStreamUploadOpenRequest, UnixSocketDaemonTransport, PROVIDER_STREAM_MAX_CHUNK_BYTES,
    PROVIDER_STREAM_SCHEMA_VERSION,
};
use dasobjectstore_object_service::{
    default_garage_credential_registry_path, read_managed_credential_registry,
};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct S3GatewayState {
    credential_registry_path: PathBuf,
    upload_permits: Arc<Semaphore>,
}

impl S3GatewayState {
    pub fn packaged(max_concurrent_uploads: usize) -> Self {
        Self {
            credential_registry_path: default_garage_credential_registry_path(),
            upload_permits: Arc::new(Semaphore::new(max_concurrent_uploads)),
        }
    }

    #[cfg(test)]
    fn with_registry(path: PathBuf, max_concurrent_uploads: usize) -> Self {
        Self {
            credential_registry_path: path,
            upload_permits: Arc::new(Semaphore::new(max_concurrent_uploads)),
        }
    }
}

pub fn s3_gateway_router(max_concurrent_uploads: usize) -> Router {
    s3_gateway_router_with_state(S3GatewayState::packaged(max_concurrent_uploads))
}

fn s3_gateway_router_with_state(state: S3GatewayState) -> Router {
    Router::new()
        .route("/{bucket}", get(s3_list_objects))
        .route(
            "/{bucket}/{*key}",
            get(s3_get_object)
                .head(s3_head_object)
                .put(s3_put_object)
                .post(s3_post_object)
                .delete(s3_delete_object),
        )
        .with_state(state)
}

async fn verified_credential(
    state: &S3GatewayState,
    bucket: &str,
    uri: &axum::http::Uri,
    method: &Method,
    headers: &HeaderMap,
) -> Result<crate::s3_gateway_auth::VerifiedS3Credential, Response> {
    let credentials =
        read_managed_credential_registry(&state.credential_registry_path, "direct-s3-request")
            .map_err(|_| {
                s3_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "ServiceUnavailable",
                    "credential authority is unavailable",
                )
            })?
            .credentials;
    verify_s3_sigv4(
        S3SigV4Request {
            method,
            raw_path: uri.path(),
            raw_query: uri.query(),
            headers,
            bucket,
        },
        &credentials,
    )
    .map_err(|error| {
        s3_error(
            StatusCode::FORBIDDEN,
            "SignatureDoesNotMatch",
            &error.to_string(),
        )
    })
}

async fn s3_get_object(
    State(state): State<S3GatewayState>,
    Path((bucket, key)): Path<(String, String)>,
    OriginalUri(uri): OriginalUri,
    method: Method,
    headers: HeaderMap,
) -> Response {
    let verified = match verified_credential(&state, &bucket, &uri, &method, &headers).await {
        Ok(verified) => verified,
        Err(response) => return response,
    };
    match provider_stream_download(
        verified.store_id,
        key,
        1,
        None,
        headers,
        DaemonRuntimeConfig::default_packaged().socket_path,
    )
    .await
    {
        Ok(response) => response,
        Err((status, error)) => s3_error(status, &error.0.code, &error.0.message),
    }
}

async fn s3_upload_part(
    store_id: dasobjectstore_core::ids::StoreId,
    key: String,
    upload_id: String,
    part_number: u32,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let Some(content_length) = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
    else {
        return s3_error(
            StatusCode::LENGTH_REQUIRED,
            "MissingContentLength",
            "multipart parts require Content-Length",
        );
    };
    let Some(payload_hash) = headers
        .get("x-amz-content-sha256")
        .and_then(|value| value.to_str().ok())
        .filter(|value| *value != "UNSIGNED-PAYLOAD")
    else {
        return s3_error(
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            "multipart parts require a signed SHA-256 payload hash",
        );
    };
    let checksum = format!("sha256:{}", payload_hash.to_ascii_lowercase());
    let request = ProviderStreamMultipartPartUploadOpenRequest {
        schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
        request_id: format!("s3-part-{upload_id}-{part_number}"),
        reservation_id: upload_id,
        reservation_size_bytes: content_length,
        part_number,
        store_id,
        object: BackendObjectKey {
            object_id: key,
            version: 1,
        },
        expected_size_bytes: content_length,
        expected_sha256: checksum,
        chunk_size_bytes: PROVIDER_STREAM_MAX_CHUNK_BYTES,
    };
    match stream_profile_s3_multipart_part(request, body).await {
        Ok(_) => {
            let mut response = StatusCode::OK.into_response();
            if let Ok(value) = format!("\"{}-{content_length}\"", payload_hash).parse() {
                response.headers_mut().insert(header::ETAG, value);
            }
            response
        }
        Err((status, error)) => s3_error(status, &error.0.code, &error.0.message),
    }
}

async fn s3_post_object(
    State(state): State<S3GatewayState>,
    Path((bucket, key)): Path<(String, String)>,
    OriginalUri(uri): OriginalUri,
    method: Method,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let verified = match verified_credential(&state, &bucket, &uri, &method, &headers).await {
        Ok(verified) => verified,
        Err(response) => return response,
    };
    let query = parse_list_query(uri.query().unwrap_or_default());
    if query.iter().any(|(name, _)| name == "uploads") {
        let upload_id = uuid::Uuid::new_v4().to_string();
        return s3_xml(
            StatusCode::OK,
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><InitiateMultipartUploadResult><Bucket>{}</Bucket><Key>{}</Key><UploadId>{upload_id}</UploadId></InitiateMultipartUploadResult>",
                xml_escape(&bucket),
                xml_escape(&key)
            ),
        );
    }
    let Some(upload_id) = query_value(&query, "uploadId") else {
        return s3_error(
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            "POST requires uploads or uploadId",
        );
    };
    let body = match axum::body::to_bytes(body, 2 * 1024 * 1024).await {
        Ok(body) => body,
        Err(error) => return s3_error(StatusCode::BAD_REQUEST, "MalformedXML", &error.to_string()),
    };
    let parts = match parse_completion_parts(&String::from_utf8_lossy(&body)) {
        Ok(parts) => parts,
        Err(message) => return s3_error(StatusCode::BAD_REQUEST, "MalformedXML", &message),
    };
    let expected_size_bytes = match parts
        .iter()
        .try_fold(0_u64, |total, part| total.checked_add(part.size_bytes))
    {
        Some(total) => total,
        None => return s3_error(StatusCode::BAD_REQUEST, "InvalidRequest", "size overflow"),
    };
    let request = ProfileS3MultipartCompletionRequest {
        store_id: verified.store_id,
        reservation_id: upload_id.to_string(),
        key: BackendObjectKey {
            object_id: key.clone(),
            version: 1,
        },
        expected_size_bytes,
        parts,
    };
    match complete_profile_s3_multipart(request).await {
        Ok(_) => s3_xml(
            StatusCode::OK,
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><CompleteMultipartUploadResult><Bucket>{}</Bucket><Key>{}</Key></CompleteMultipartUploadResult>",
                xml_escape(&bucket),
                xml_escape(&key)
            ),
        ),
        Err((status, error)) => s3_error(status, &error.0.code, &error.0.message),
    }
}

async fn s3_delete_object(
    State(state): State<S3GatewayState>,
    Path((bucket, key)): Path<(String, String)>,
    OriginalUri(uri): OriginalUri,
    method: Method,
    headers: HeaderMap,
) -> Response {
    let verified = match verified_credential(&state, &bucket, &uri, &method, &headers).await {
        Ok(verified) => verified,
        Err(response) => return response,
    };
    let query = parse_list_query(uri.query().unwrap_or_default());
    let Some(upload_id) = query_value(&query, "uploadId") else {
        return s3_error(
            StatusCode::NOT_IMPLEMENTED,
            "NotImplemented",
            "direct object deletion is not enabled; DELETE supports multipart abort only",
        );
    };
    let request = ProfileS3MultipartAbortRequest {
        store_id: verified.store_id,
        reservation_id: upload_id.to_string(),
        key: BackendObjectKey {
            object_id: key,
            version: 1,
        },
    };
    let result = crate::daemon_bridge::DaemonBridge::shared_packaged()
        .call_message(move || {
            DaemonClient::new(UnixSocketDaemonTransport::for_bounded_bridge(
                DaemonRuntimeConfig::default_packaged().socket_path,
            ))
            .profile_s3_multipart_abort(request)
            .map_err(|error| error.to_string())
        })
        .await;
    match result {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => s3_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "ServiceUnavailable",
            &format!("{error:?}"),
        ),
    }
}

async fn s3_head_object(
    State(state): State<S3GatewayState>,
    Path((bucket, key)): Path<(String, String)>,
    OriginalUri(uri): OriginalUri,
    method: Method,
    headers: HeaderMap,
) -> Response {
    let verified = match verified_credential(&state, &bucket, &uri, &method, &headers).await {
        Ok(verified) => verified,
        Err(response) => return response,
    };
    let request = ProfileS3HeadRequest {
        store_id: verified.store_id,
        key: BackendObjectKey {
            object_id: key,
            version: 1,
        },
    };
    let result = crate::daemon_bridge::DaemonBridge::shared_packaged()
        .call_message(move || {
            DaemonClient::new(UnixSocketDaemonTransport::for_bounded_bridge(
                DaemonRuntimeConfig::default_packaged().socket_path,
            ))
            .profile_s3_head(request)
            .map_err(|error| error.to_string())
        })
        .await;
    match result {
        Ok(head) => {
            let mut response = StatusCode::OK.into_response();
            if let Ok(value) = head.object.size_bytes.to_string().parse() {
                response.headers_mut().insert(header::CONTENT_LENGTH, value);
            }
            if let Some(digest) = head.object.checksum.strip_prefix("sha256:") {
                if let Ok(value) = format!("\"{digest}\"").parse() {
                    response.headers_mut().insert(header::ETAG, value);
                }
            }
            response
        }
        Err(error) => s3_error(StatusCode::NOT_FOUND, "NoSuchKey", &format!("{error:?}")),
    }
}

async fn s3_list_objects(
    State(state): State<S3GatewayState>,
    Path(bucket): Path<String>,
    OriginalUri(uri): OriginalUri,
    method: Method,
    headers: HeaderMap,
) -> Response {
    let verified = match verified_credential(&state, &bucket, &uri, &method, &headers).await {
        Ok(verified) => verified,
        Err(response) => return response,
    };
    let query = parse_list_query(uri.query().unwrap_or_default());
    let limit = query
        .iter()
        .find(|(name, _)| name == "max-keys")
        .and_then(|(_, value)| value.parse::<u16>().ok())
        .unwrap_or(100)
        .clamp(1, 1_000);
    let prefix = query
        .iter()
        .find(|(name, _)| name == "prefix")
        .map(|(_, value)| value.clone())
        .filter(|value| !value.is_empty());
    let offset = query
        .iter()
        .find(|(name, _)| name == "continuation-token")
        .and_then(|(_, value)| value.strip_prefix("offset-"))
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or_default();
    let request = ProfileS3ListRequest {
        store_id: verified.store_id,
        prefix: prefix.clone(),
        offset,
        limit,
    };
    let result = crate::daemon_bridge::DaemonBridge::shared_packaged()
        .call_message(move || {
            DaemonClient::new(UnixSocketDaemonTransport::for_bounded_bridge(
                DaemonRuntimeConfig::default_packaged().socket_path,
            ))
            .profile_s3_list(request)
            .map_err(|error| error.to_string())
        })
        .await;
    match result {
        Ok(list) => {
            let mut xml = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><ListBucketResult><Name>{}</Name><Prefix>{}</Prefix><KeyCount>{}</KeyCount><MaxKeys>{}</MaxKeys><IsTruncated>{}</IsTruncated>",
                xml_escape(&bucket),
                xml_escape(prefix.as_deref().unwrap_or_default()),
                list.objects.len(),
                limit,
                list.next_offset.is_some()
            );
            for object in list.objects {
                let digest = object
                    .checksum
                    .strip_prefix("sha256:")
                    .unwrap_or(&object.checksum);
                xml.push_str(&format!(
                    "<Contents><Key>{}</Key><ETag>\"{}\"</ETag><Size>{}</Size><StorageClass>STANDARD</StorageClass></Contents>",
                    xml_escape(&object.key.object_id), xml_escape(digest), object.size_bytes
                ));
            }
            if let Some(next) = list.next_offset {
                xml.push_str(&format!(
                    "<NextContinuationToken>offset-{next}</NextContinuationToken>"
                ));
            }
            xml.push_str("</ListBucketResult>");
            s3_xml(StatusCode::OK, xml)
        }
        Err(error) => s3_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "ServiceUnavailable",
            &format!("{error:?}"),
        ),
    }
}

async fn s3_put_object(
    State(state): State<S3GatewayState>,
    Path((bucket, key)): Path<(String, String)>,
    OriginalUri(uri): OriginalUri,
    method: Method,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let permit = match state.upload_permits.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            return s3_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "SlowDown",
                "upload budget is full",
            )
        }
    };
    if key.is_empty()
        || uri
            .path()
            .as_bytes()
            .windows(3)
            .any(|part| part.eq_ignore_ascii_case(b"%2f"))
    {
        return s3_error(
            StatusCode::BAD_REQUEST,
            "InvalidObjectName",
            "ambiguous or empty object key",
        );
    }
    let verified = match verified_credential(&state, &bucket, &uri, &method, &headers).await {
        Ok(verified) => verified,
        Err(response) => return response,
    };
    let query = parse_list_query(uri.query().unwrap_or_default());
    if let (Some(part_number), Some(upload_id)) = (
        query_value(&query, "partNumber").and_then(|value| value.parse::<u32>().ok()),
        query_value(&query, "uploadId"),
    ) {
        return s3_upload_part(
            verified.store_id,
            key,
            upload_id.to_string(),
            part_number,
            headers,
            body,
        )
        .await;
    }
    let Some(content_length) = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
    else {
        return s3_error(
            StatusCode::LENGTH_REQUIRED,
            "MissingContentLength",
            "direct S3 PUT requires Content-Length",
        );
    };
    let Some(payload_hash) = headers
        .get("x-amz-content-sha256")
        .and_then(|value| value.to_str().ok())
    else {
        return s3_error(
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            "x-amz-content-sha256 is required",
        );
    };
    if payload_hash == "UNSIGNED-PAYLOAD" {
        return s3_error(
            StatusCode::BAD_REQUEST,
            "InvalidRequest",
            "direct S3 writes require a signed SHA-256 payload hash",
        );
    }
    let expected_sha256 = format!("sha256:{}", payload_hash.to_ascii_lowercase());
    let operation_id = format!(
        "s3-{:x}",
        Sha256::digest(
            format!(
                "{}\0{}\0{}\0{}\0{}",
                verified.access_key_id, bucket, key, content_length, expected_sha256
            )
            .as_bytes()
        )
    );
    let request = ProviderStreamUploadOpenRequest {
        schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
        request_id: operation_id.clone(),
        upload_id: operation_id,
        store_id: verified.store_id,
        object: BackendObjectKey {
            object_id: key,
            version: 1,
        },
        expected_size_bytes: content_length,
        expected_sha256: expected_sha256.clone(),
        chunk_size_bytes: PROVIDER_STREAM_MAX_CHUNK_BYTES,
    };
    let result = stream_profile_s3_put(request, body).await;
    drop(permit);
    match result {
        Ok(_) => {
            let mut response = StatusCode::OK.into_response();
            if let Ok(value) = format!("\"{}\"", &expected_sha256["sha256:".len()..]).parse() {
                response.headers_mut().insert(header::ETAG, value);
            }
            response
        }
        Err((status, error)) => s3_error(status, &error.0.code, &error.0.message),
    }
}

fn s3_error(status: StatusCode, code: &str, message: &str) -> Response {
    s3_xml(
        status,
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Error><Code>{}</Code><Message>{}</Message></Error>",
            xml_escape(code),
            xml_escape(message)
        ),
    )
}

fn s3_xml(status: StatusCode, xml: String) -> Response {
    let mut response = (status, xml).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        "application/xml; charset=utf-8"
            .parse()
            .expect("static header"),
    );
    response
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn parse_list_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|field| !field.is_empty())
        .map(|field| {
            let (name, value) = field.split_once('=').unwrap_or((field, ""));
            (percent_decode_query(name), percent_decode_query(value))
        })
        .collect()
}

fn query_value<'a>(query: &'a [(String, String)], name: &str) -> Option<&'a str> {
    query
        .iter()
        .find(|(candidate, _)| candidate == name)
        .map(|(_, value)| value.as_str())
}

fn parse_completion_parts(xml: &str) -> Result<Vec<ProfileS3MultipartPartRequest>, String> {
    let mut parts = Vec::new();
    let mut remainder = xml;
    while let Some(start) = remainder.find("<Part>") {
        remainder = &remainder[start + "<Part>".len()..];
        let end = remainder
            .find("</Part>")
            .ok_or_else(|| "multipart completion has an unterminated Part".to_string())?;
        let part = &remainder[..end];
        let field = |name: &str| -> Result<&str, String> {
            let open = format!("<{name}>");
            let close = format!("</{name}>");
            let start = part
                .find(&open)
                .ok_or_else(|| format!("multipart Part is missing {name}"))?
                + open.len();
            let end = part[start..]
                .find(&close)
                .ok_or_else(|| format!("multipart Part has invalid {name}"))?
                + start;
            Ok(part[start..end].trim())
        };
        let part_number = field("PartNumber")?
            .parse::<u32>()
            .map_err(|_| "multipart PartNumber is invalid".to_string())?;
        let etag = field("ETag")?.trim_matches('"');
        let (digest, size) = etag
            .rsplit_once('-')
            .ok_or_else(|| "multipart ETag is not a DASObjectStore part receipt".to_string())?;
        if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("multipart ETag checksum is invalid".to_string());
        }
        let size_bytes = size
            .parse::<u64>()
            .map_err(|_| "multipart ETag size is invalid".to_string())?;
        parts.push(ProfileS3MultipartPartRequest {
            part_number,
            size_bytes,
            checksum: format!("sha256:{}", digest.to_ascii_lowercase()),
        });
        remainder = &remainder[end + "</Part>".len()..];
    }
    if parts.is_empty() {
        return Err("multipart completion contains no parts".to_string());
    }
    parts.sort_by_key(|part| part.part_number);
    if parts
        .windows(2)
        .any(|pair| pair[0].part_number == pair[1].part_number)
    {
        return Err("multipart completion contains duplicate parts".to_string());
    }
    Ok(parts)
}

fn percent_decode_query(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let hex = |byte: u8| match byte {
                    b'0'..=b'9' => Some(byte - b'0'),
                    b'a'..=b'f' => Some(byte - b'a' + 10),
                    b'A'..=b'F' => Some(byte - b'A' + 10),
                    _ => None,
                };
                if let (Some(high), Some(low)) = (hex(bytes[index + 1]), hex(bytes[index + 2])) {
                    decoded.push((high << 4) | low);
                    index += 3;
                    continue;
                }
                decoded.push(bytes[index]);
            }
            byte => decoded.push(byte),
        }
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    #[tokio::test]
    async fn xml_errors_escape_untrusted_text_and_are_s3_xml() {
        let response = s3_error(StatusCode::BAD_REQUEST, "Bad<Key", "a&b");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/xml; charset=utf-8"
        );
        let body = to_bytes(response.into_body(), 4_096)
            .await
            .expect("error body");
        assert_eq!(
            String::from_utf8(body.to_vec()).expect("UTF-8 XML"),
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Error><Code>Bad&lt;Key</Code><Message>a&amp;b</Message></Error>"
        );
    }

    #[test]
    fn gateway_state_enforces_a_nonzero_upload_budget() {
        let state = S3GatewayState::with_registry(PathBuf::from("/tmp/credentials"), 1);
        assert_eq!(state.upload_permits.available_permits(), 1);
    }

    #[test]
    fn list_query_decodes_prefix_and_continuation_token_without_reordering() {
        assert_eq!(
            parse_list_query(
                "list-type=2&prefix=reads%2Fsample%2001&continuation-token=offset-42&max-keys=25"
            ),
            vec![
                ("list-type".to_string(), "2".to_string()),
                ("prefix".to_string(), "reads/sample 01".to_string()),
                ("continuation-token".to_string(), "offset-42".to_string()),
                ("max-keys".to_string(), "25".to_string()),
            ]
        );
    }

    #[test]
    fn list_query_preserves_empty_values_and_decodes_utf8() {
        assert_eq!(
            parse_list_query("prefix=&delimiter=%2F&marker=%CE%B1"),
            vec![
                ("prefix".to_string(), String::new()),
                ("delimiter".to_string(), "/".to_string()),
                ("marker".to_string(), "α".to_string()),
            ]
        );
        assert_eq!(parse_list_query("prefix=a+b")[0].1, "a+b");
    }

    #[test]
    fn completion_receipts_recover_order_size_and_sha256() {
        let a = "a".repeat(64);
        let b = "b".repeat(64);
        let xml = format!(
            "<CompleteMultipartUpload><Part><PartNumber>2</PartNumber><ETag>\"{b}-7\"</ETag></Part><Part><PartNumber>1</PartNumber><ETag>\"{a}-5\"</ETag></Part></CompleteMultipartUpload>"
        );
        let parts = parse_completion_parts(&xml).expect("valid completion");
        assert_eq!(parts[0].part_number, 1);
        assert_eq!(parts[0].size_bytes, 5);
        assert_eq!(parts[0].checksum, format!("sha256:{a}"));
        assert_eq!(parts[1].part_number, 2);
        assert_eq!(parts[1].size_bytes, 7);
    }

    #[test]
    fn xml_escape_covers_all_xml_metacharacters() {
        assert_eq!(xml_escape("<&>\"'"), "&lt;&amp;&gt;&quot;&apos;");
    }
}
