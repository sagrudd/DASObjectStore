//! Single-request HTTP/1.1 framing, not a TLS server or an admission mechanism.
use super::{
    bounded_length, frame, split_frame, ReaderResultV1, DENIED, FRAME_LIMIT, SIGNED_REQUEST_LIMIT,
};
use crate::custody_attestation::{
    decode_pre_read, CustodyEd25519AuthorityV1, CustodySignedPreReadRequestV1,
};
use crate::custody_reader::{raw_sha256, ReaderError};
use std::collections::BTreeMap;

/// Bound on the complete HTTP header section, including request/status line and terminator.
pub const HEADER_LIMIT: usize = 16_384;

struct Headers<'a> {
    line: &'a str,
    fields: BTreeMap<String, &'a str>,
    end: usize,
}
fn headers(raw: &[u8]) -> Result<Headers<'_>, ReaderError> {
    let end = raw[..raw.len().min(HEADER_LIMIT)]
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .and_then(|n| n.checked_add(4))
        .ok_or(ReaderError::Format)?;
    let text = std::str::from_utf8(&raw[..end - 2]).map_err(|_| ReaderError::Format)?;
    let mut lines = text.split("\r\n");
    let line = lines.next().ok_or(ReaderError::Format)?;
    let mut fields = BTreeMap::new();
    for header in lines {
        if header.is_empty() {
            continue;
        }
        let (key, value) = header.split_once(':').ok_or(ReaderError::Format)?;
        if key.is_empty()
            || !key
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&c))
            || value.bytes().any(|c| !(0x20..=0x7e).contains(&c))
        {
            return Err(ReaderError::Format);
        }
        let key = key.to_ascii_lowercase();
        if matches!(
            key.as_str(),
            "transfer-encoding" | "content-encoding" | "expect" | "upgrade"
        ) || fields.insert(key, value.trim_matches(' ')).is_some()
            || fields.len() > 64
        {
            return Err(ReaderError::Format);
        }
    }
    if fields.get("connection") != Some(&"close") {
        return Err(ReaderError::Format);
    }
    Ok(Headers { line, fields, end })
}
fn length(headers: &Headers<'_>, maximum: usize) -> Result<usize, ReaderError> {
    let value = headers
        .fields
        .get("content-length")
        .ok_or(ReaderError::Format)?;
    if value.is_empty() || !value.bytes().all(|c| c.is_ascii_digit()) {
        return Err(ReaderError::Format);
    }
    let n: usize = value.parse().map_err(|_| ReaderError::Format)?;
    if n > maximum || n > isize::MAX as usize {
        return Err(ReaderError::Format);
    }
    Ok(n)
}

/// First HTTP request's signed body and exact consumed length. No transport admission.
pub struct SignedHttpRequest<'a> {
    /// Original bytes used by the existing signature verifier, not reserialized data.
    pub raw_jcs: &'a [u8],
    /// Existing signed record verified against the independently pinned authority/time.
    pub record: CustodySignedPreReadRequestV1,
    /// Bytes occupied by this one request. Any later bytes must never be dispatched.
    pub consumed: usize,
}

/// Parse exactly the supported operation and run the existing signed-request validator.
/// Caller must independently authenticate TLS/peer and close after this one operation.
/// Buffer collection must itself enforce these bounds before allocating or dispatching.
///
/// # Errors
/// Rejects unsupported routing/framing, incomplete or oversized bodies and signature denials.
pub fn decode_request<'a>(
    raw: &'a [u8],
    authority: &str,
    pinned: &CustodyEd25519AuthorityV1,
    now: &str,
) -> Result<SignedHttpRequest<'a>, ReaderError> {
    let header = headers(raw)?;
    if header.line != "POST /custody/v1/read-object HTTP/1.1"
        || header.fields.get("host") != Some(&authority)
        || header.fields.get("content-type") != Some(&"application/json")
    {
        return Err(ReaderError::Format);
    }
    let size = length(&header, SIGNED_REQUEST_LIMIT)?;
    let consumed = header.end.checked_add(size).ok_or(ReaderError::Format)?;
    let body = raw.get(header.end..consumed).ok_or(ReaderError::Format)?;
    let record = decode_pre_read(body, pinned, now).map_err(|_| ReaderError::Binding)?;
    Ok(SignedHttpRequest {
        raw_jcs: body,
        record,
        consumed,
    })
}

/// Encode a complete successful response after the caller has verified the selected bytes.
///
/// # Errors
/// Rejects invalid metadata, size mismatch and overflow. Does not assert provenance.
pub fn encode_result(
    metadata: &ReaderResultV1,
    body: &[u8],
    maximum: usize,
) -> Result<Vec<u8>, ReaderError> {
    if bounded_length(metadata.content_length, maximum)? != body.len() {
        return Err(ReaderError::Binding);
    }
    let payload = frame(&metadata.encode()?, body)?;
    let mut out = format!("HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", payload.len()).into_bytes();
    out.try_reserve_exact(payload.len())
        .map_err(|_| ReaderError::Format)?;
    out.extend_from_slice(&payload);
    Ok(out)
}

/// Decode a complete response against independently selected metadata and object digest.
/// Network caller must supply EOF and a finite maximum; metadata alone is never success.
///
/// # Errors
/// Rejects non-200, framing aliases, partial/extra bytes or any metadata/content mismatch.
pub fn decode_result<'a>(
    raw: &'a [u8],
    eof: bool,
    expected: &ReaderResultV1,
    content_sha256: &str,
    maximum: usize,
) -> Result<&'a [u8], ReaderError> {
    expected.encode()?;
    let header = headers(raw)?;
    if !eof
        || header.line != "HTTP/1.1 200 OK"
        || header.fields.get("content-type") != Some(&"application/octet-stream")
    {
        return Err(ReaderError::Format);
    }
    let bound = maximum
        .checked_add(FRAME_LIMIT + 4)
        .ok_or(ReaderError::Format)?;
    let size = length(&header, bound)?;
    if header.end.checked_add(size) != Some(raw.len()) {
        return Err(ReaderError::Format);
    }
    let (metadata, body) = split_frame(&raw[header.end..], FRAME_LIMIT)?;
    let observed = ReaderResultV1::decode(metadata)?;
    if observed != *expected
        || bounded_length(observed.content_length, maximum)? != body.len()
        || raw_sha256(body) != content_sha256
    {
        return Err(ReaderError::Binding);
    }
    Ok(body)
}

/// Fixed authenticated application denial; a late failure must disconnect instead.
pub fn encode_denied() -> Vec<u8> {
    let mut out = format!("HTTP/1.1 403 Forbidden\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", DENIED.len()).into_bytes();
    out.extend_from_slice(DENIED);
    out
}

/// Recognize only the fixed complete authenticated application rejection.
/// This does not establish that the sender was authenticated.
///
/// # Errors
/// Rejects other statuses, extra/partial bodies, noncanonical errors or absent EOF.
pub fn decode_denied(raw: &[u8], eof: bool) -> Result<(), ReaderError> {
    let header = headers(raw)?;
    if !eof
        || header.line != "HTTP/1.1 403 Forbidden"
        || header.fields.get("content-type") != Some(&"application/json")
        || length(&header, DENIED.len())? != DENIED.len()
        || raw.get(header.end..) != Some(DENIED)
    {
        return Err(ReaderError::Format);
    }
    Ok(())
}
