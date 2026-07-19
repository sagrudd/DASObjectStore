//! AWS Signature Version 4 verification for the dedicated S3 gateway.
//!
//! This module deliberately resolves an access key to one exact managed
//! bucket/ObjectStore pair before authenticating a request. It returns only
//! public credential identity; secret material never crosses the verifier.

use axum::http::{HeaderMap, Method};
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_object_service::ManagedStoreCredentialRecord;
use ring::hmac;
use sha2::{Digest, Sha256};
use std::fmt::{self, Display};

const ALGORITHM: &str = "AWS4-HMAC-SHA256";
const TERMINATOR: &str = "aws4_request";
const SERVICE: &str = "s3";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct VerifiedS3Credential {
    pub store_id: StoreId,
    pub bucket_name: String,
    pub credential_reference: String,
    pub access_key_id: String,
}

pub(crate) struct S3SigV4Request<'a> {
    pub method: &'a Method,
    /// The encoded URI path exactly as received by the HTTP server.
    pub raw_path: &'a str,
    /// The encoded query without the leading `?`.
    pub raw_query: Option<&'a str>,
    pub headers: &'a HeaderMap,
    /// Bucket resolved by the gateway's unambiguous path/host parser.
    pub bucket: &'a str,
}

pub(crate) fn verify_s3_sigv4(
    request: S3SigV4Request<'_>,
    credentials: &[ManagedStoreCredentialRecord],
) -> Result<VerifiedS3Credential, S3SigV4Error> {
    reject_duplicate_header(request.headers, "authorization")?;
    let authorization = required_header(request.headers, "authorization")?;
    let parsed = parse_authorization(authorization)?;

    let mut matches = credentials
        .iter()
        .filter(|record| record.access_key_id == parsed.access_key_id);
    let credential = matches.next().ok_or(S3SigV4Error::UnknownCredential)?;
    if matches.next().is_some() {
        return Err(S3SigV4Error::AmbiguousCredential);
    }
    if credential.bucket_name != request.bucket {
        return Err(S3SigV4Error::CredentialScopeMismatch);
    }

    let amz_date = unique_signed_header(request.headers, "x-amz-date", &parsed.signed_headers)?;
    validate_amz_date(amz_date, parsed.date)?;
    let payload_hash = unique_signed_header(
        request.headers,
        "x-amz-content-sha256",
        &parsed.signed_headers,
    )?;
    validate_payload_hash(payload_hash)?;
    if !parsed.signed_headers.iter().any(|header| header == "host") {
        return Err(S3SigV4Error::MissingRequiredSignedHeader("host"));
    }

    let canonical_headers = canonical_headers(request.headers, &parsed.signed_headers)?;
    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        request.method.as_str(),
        canonical_uri(request.raw_path)?,
        canonical_query(request.raw_query.unwrap_or(""))?,
        canonical_headers,
        parsed.signed_headers.join(";"),
        payload_hash
    );
    let scope = format!(
        "{}/{}/{}/{}",
        parsed.date, parsed.region, SERVICE, TERMINATOR
    );
    let string_to_sign = format!(
        "{ALGORITHM}\n{amz_date}\n{scope}\n{}",
        hex_sha256(canonical_request.as_bytes())
    );
    let signing_key = signing_key(
        credential.secret_access_key.as_bytes(),
        parsed.date,
        parsed.region,
    );
    let expected_signature = decode_signature(parsed.signature)?;
    hmac::verify(
        &hmac::Key::new(hmac::HMAC_SHA256, &signing_key),
        string_to_sign.as_bytes(),
        &expected_signature,
    )
    .map_err(|_| S3SigV4Error::SignatureMismatch)?;

    Ok(VerifiedS3Credential {
        store_id: credential.store_id.clone(),
        bucket_name: credential.bucket_name.clone(),
        credential_reference: credential.credential_reference.clone(),
        access_key_id: credential.access_key_id.clone(),
    })
}

struct ParsedAuthorization<'a> {
    access_key_id: &'a str,
    date: &'a str,
    region: &'a str,
    signed_headers: Vec<String>,
    signature: &'a str,
}

fn parse_authorization(value: &str) -> Result<ParsedAuthorization<'_>, S3SigV4Error> {
    let parameters = value
        .strip_prefix(&format!("{ALGORITHM} "))
        .ok_or(S3SigV4Error::UnsupportedAlgorithm)?;
    let mut credential = None;
    let mut signed_headers = None;
    let mut signature = None;
    for parameter in parameters.split(',') {
        let parameter = parameter.trim();
        let (name, value) = parameter
            .split_once('=')
            .ok_or(S3SigV4Error::MalformedAuthorization)?;
        if value.is_empty() {
            return Err(S3SigV4Error::MalformedAuthorization);
        }
        let slot = match name {
            "Credential" => &mut credential,
            "SignedHeaders" => &mut signed_headers,
            "Signature" => &mut signature,
            _ => return Err(S3SigV4Error::MalformedAuthorization),
        };
        if slot.replace(value).is_some() {
            return Err(S3SigV4Error::MalformedAuthorization);
        }
    }
    let credential = credential.ok_or(S3SigV4Error::MalformedAuthorization)?;
    let mut scope = credential.split('/');
    let access_key_id = scope.next().filter(|value| !value.is_empty());
    let date = scope.next().filter(|value| valid_date(value));
    let region = scope.next().filter(|value| valid_scope_token(value));
    let service = scope.next();
    let terminator = scope.next();
    if scope.next().is_some() || service != Some(SERVICE) || terminator != Some(TERMINATOR) {
        return Err(S3SigV4Error::MalformedCredentialScope);
    }
    let signed_headers =
        parse_signed_headers(signed_headers.ok_or(S3SigV4Error::MalformedAuthorization)?)?;
    Ok(ParsedAuthorization {
        access_key_id: access_key_id.ok_or(S3SigV4Error::MalformedCredentialScope)?,
        date: date.ok_or(S3SigV4Error::MalformedCredentialScope)?,
        region: region.ok_or(S3SigV4Error::MalformedCredentialScope)?,
        signed_headers,
        signature: signature.ok_or(S3SigV4Error::MalformedAuthorization)?,
    })
}

fn parse_signed_headers(value: &str) -> Result<Vec<String>, S3SigV4Error> {
    let headers = value.split(';').map(str::to_string).collect::<Vec<_>>();
    if headers.is_empty()
        || headers.iter().any(|header| {
            header.is_empty()
                || !header
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
        || headers.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(S3SigV4Error::AmbiguousSignedHeaders);
    }
    Ok(headers)
}

fn canonical_headers(headers: &HeaderMap, names: &[String]) -> Result<String, S3SigV4Error> {
    let mut canonical = String::new();
    for name in names {
        let value = unique_header(headers, name)?;
        canonical.push_str(name);
        canonical.push(':');
        canonical.push_str(&normalize_header_value(value));
        canonical.push('\n');
    }
    Ok(canonical)
}

fn normalize_header_value(value: &str) -> String {
    value.split_ascii_whitespace().collect::<Vec<_>>().join(" ")
}

fn canonical_uri(path: &str) -> Result<String, S3SigV4Error> {
    if !path.starts_with('/') {
        return Err(S3SigV4Error::MalformedUri);
    }
    // Split only on literal separators. An encoded slash is object-key data,
    // not a path separator, and must remain `%2F` in the canonical target.
    path.split('/')
        .map(|component| aws_encode(&percent_decode(component)?, false))
        .collect::<Result<Vec<_>, _>>()
        .map(|components| components.join("/"))
}

fn canonical_query(query: &str) -> Result<String, S3SigV4Error> {
    if query.is_empty() {
        return Ok(String::new());
    }
    let mut fields = Vec::new();
    for field in query.split('&') {
        let (name, value) = field.split_once('=').unwrap_or((field, ""));
        fields.push((
            aws_encode(&percent_decode(name)?, false)?,
            aws_encode(&percent_decode(value)?, false)?,
        ));
    }
    fields.sort();
    Ok(fields
        .into_iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("&"))
}

fn percent_decode(value: &str) -> Result<Vec<u8>, S3SigV4Error> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(S3SigV4Error::MalformedUri);
            }
            decoded.push(
                (hex_nibble(bytes[index + 1]).ok_or(S3SigV4Error::MalformedUri)? << 4)
                    | hex_nibble(bytes[index + 2]).ok_or(S3SigV4Error::MalformedUri)?,
            );
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    Ok(decoded)
}

fn aws_encode(bytes: &[u8], preserve_slash: bool) -> Result<String, S3SigV4Error> {
    let mut encoded = String::with_capacity(bytes.len());
    for &byte in bytes {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else if preserve_slash && byte == b'/' {
            encoded.push('/');
        } else {
            use std::fmt::Write;
            write!(&mut encoded, "%{byte:02X}").map_err(|_| S3SigV4Error::MalformedUri)?;
        }
    }
    Ok(encoded)
}

fn signing_key(secret: &[u8], date: &str, region: &str) -> Vec<u8> {
    let mut root = b"AWS4".to_vec();
    root.extend_from_slice(secret);
    let date_key = hmac_sha256(&root, date.as_bytes());
    let region_key = hmac_sha256(&date_key, region.as_bytes());
    let service_key = hmac_sha256(&region_key, SERVICE.as_bytes());
    hmac_sha256(&service_key, TERMINATOR.as_bytes())
}

fn hmac_sha256(key: &[u8], value: &[u8]) -> Vec<u8> {
    hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, key), value)
        .as_ref()
        .to_vec()
}

fn hex_sha256(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}

fn decode_signature(value: &str) -> Result<Vec<u8>, S3SigV4Error> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(S3SigV4Error::MalformedSignature);
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            Ok(
                (hex_nibble(pair[0]).ok_or(S3SigV4Error::MalformedSignature)? << 4)
                    | hex_nibble(pair[1]).ok_or(S3SigV4Error::MalformedSignature)?,
            )
        })
        .collect()
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn required_header<'a>(
    headers: &'a HeaderMap,
    name: &'static str,
) -> Result<&'a str, S3SigV4Error> {
    unique_header(headers, name).map_err(|error| match error {
        S3SigV4Error::MissingSignedHeader(_) => S3SigV4Error::MissingRequiredHeader(name),
        other => other,
    })
}

fn unique_signed_header<'a>(
    headers: &'a HeaderMap,
    name: &'static str,
    signed: &[String],
) -> Result<&'a str, S3SigV4Error> {
    if !signed.iter().any(|candidate| candidate == name) {
        return Err(S3SigV4Error::MissingRequiredSignedHeader(name));
    }
    unique_header(headers, name)
}

fn unique_header<'a>(headers: &'a HeaderMap, name: &str) -> Result<&'a str, S3SigV4Error> {
    let values = headers.get_all(name);
    let mut values = values.iter();
    let value = values
        .next()
        .ok_or_else(|| S3SigV4Error::MissingSignedHeader(name.to_string()))?;
    if values.next().is_some() {
        return Err(S3SigV4Error::DuplicateHeader(name.to_string()));
    }
    value
        .to_str()
        .map_err(|_| S3SigV4Error::InvalidHeader(name.to_string()))
}

fn reject_duplicate_header(headers: &HeaderMap, name: &str) -> Result<(), S3SigV4Error> {
    if headers.get_all(name).iter().count() > 1 {
        Err(S3SigV4Error::DuplicateHeader(name.to_string()))
    } else {
        Ok(())
    }
}

fn validate_payload_hash(value: &str) -> Result<(), S3SigV4Error> {
    if value == "UNSIGNED-PAYLOAD"
        || (value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')))
    {
        Ok(())
    } else {
        Err(S3SigV4Error::UnsupportedPayloadHash)
    }
}

fn validate_amz_date(value: &str, scope_date: &str) -> Result<(), S3SigV4Error> {
    if value.len() != 16
        || &value[8..9] != "T"
        || &value[15..] != "Z"
        || !value[..8].bytes().all(|byte| byte.is_ascii_digit())
        || !value[9..15].bytes().all(|byte| byte.is_ascii_digit())
        || &value[..8] != scope_date
    {
        return Err(S3SigV4Error::InvalidAmzDate);
    }
    Ok(())
}

fn valid_date(value: &str) -> bool {
    value.len() == 8 && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn valid_scope_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 63
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum S3SigV4Error {
    MissingRequiredHeader(&'static str),
    MissingRequiredSignedHeader(&'static str),
    MissingSignedHeader(String),
    DuplicateHeader(String),
    InvalidHeader(String),
    UnsupportedAlgorithm,
    MalformedAuthorization,
    MalformedCredentialScope,
    AmbiguousSignedHeaders,
    UnknownCredential,
    AmbiguousCredential,
    CredentialScopeMismatch,
    InvalidAmzDate,
    UnsupportedPayloadHash,
    MalformedUri,
    MalformedSignature,
    SignatureMismatch,
}

impl Display for S3SigV4Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingRequiredHeader(name) => {
                write!(formatter, "missing required header {name}")
            }
            Self::MissingRequiredSignedHeader(name) => {
                write!(formatter, "required header {name} is not signed")
            }
            Self::MissingSignedHeader(name) => write!(formatter, "missing signed header {name}"),
            Self::DuplicateHeader(name) => write!(formatter, "duplicate header {name}"),
            Self::InvalidHeader(name) => write!(formatter, "invalid header {name}"),
            Self::UnsupportedAlgorithm => formatter.write_str("unsupported S3 signature algorithm"),
            Self::MalformedAuthorization => formatter.write_str("malformed S3 authorization"),
            Self::MalformedCredentialScope => formatter.write_str("malformed S3 credential scope"),
            Self::AmbiguousSignedHeaders => formatter.write_str("signed headers are ambiguous"),
            Self::UnknownCredential | Self::AmbiguousCredential | Self::SignatureMismatch => {
                formatter.write_str("S3 authentication failed")
            }
            Self::CredentialScopeMismatch => {
                formatter.write_str("S3 credential is not authorized for this bucket")
            }
            Self::InvalidAmzDate => formatter.write_str("invalid S3 request date"),
            Self::UnsupportedPayloadHash => formatter.write_str("unsupported S3 payload hash form"),
            Self::MalformedUri => formatter.write_str("malformed S3 request target"),
            Self::MalformedSignature => formatter.write_str("malformed S3 signature"),
        }
    }
}

impl std::error::Error for S3SigV4Error {}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderName, HeaderValue};

    const ACCESS: &str = "GKEXAMPLEACCESSKEY";
    const SECRET: &str = "example-secret-that-is-never-rendered";
    const DATE: &str = "20260719T120000Z";
    const PAYLOAD_HASH: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    fn verifies_bound_bucket_and_canonical_query() {
        let mut headers = base_headers();
        sign(
            &mut headers,
            &Method::GET,
            "/dos-store/a%20b",
            "z=2&a=1",
            "host;x-amz-content-sha256;x-amz-date",
        );
        let verified = verify_s3_sigv4(
            request(
                &Method::GET,
                "/dos-store/a%20b",
                Some("z=2&a=1"),
                &headers,
                "dos-store",
            ),
            &[credential()],
        )
        .expect("signature verifies");
        assert_eq!(verified.store_id.as_str(), "store-a");
        assert_eq!(verified.bucket_name, "dos-store");
    }

    #[test]
    fn rejects_cross_bucket_use_before_signature_acceptance() {
        let mut headers = base_headers();
        sign(
            &mut headers,
            &Method::GET,
            "/other/key",
            "",
            "host;x-amz-content-sha256;x-amz-date",
        );
        assert_eq!(
            verify_s3_sigv4(
                request(&Method::GET, "/other/key", None, &headers, "other"),
                &[credential()]
            ),
            Err(S3SigV4Error::CredentialScopeMismatch)
        );
    }

    #[test]
    fn rejects_duplicate_and_unsorted_signed_headers() {
        let mut headers = base_headers();
        headers.append(
            HeaderName::from_static("host"),
            HeaderValue::from_static("evil.example"),
        );
        headers.insert(
            "authorization",
            HeaderValue::from_static("AWS4-HMAC-SHA256 Credential=GKEXAMPLEACCESSKEY/20260719/garage/s3/aws4_request,SignedHeaders=host;x-amz-content-sha256;x-amz-date,Signature=0000000000000000000000000000000000000000000000000000000000000000"),
        );
        assert!(matches!(
            verify_s3_sigv4(request(&Method::GET, "/dos-store/key", None, &headers, "dos-store"), &[credential()]),
            Err(S3SigV4Error::DuplicateHeader(name)) if name == "host"
        ));

        headers.remove("host");
        headers.insert("host", HeaderValue::from_static("s3.example"));
        headers.insert(
            "authorization",
            HeaderValue::from_static("AWS4-HMAC-SHA256 Credential=GKEXAMPLEACCESSKEY/20260719/garage/s3/aws4_request,SignedHeaders=x-amz-date;host;x-amz-content-sha256,Signature=0000000000000000000000000000000000000000000000000000000000000000"),
        );
        assert_eq!(
            verify_s3_sigv4(
                request(&Method::GET, "/dos-store/key", None, &headers, "dos-store"),
                &[credential()]
            ),
            Err(S3SigV4Error::AmbiguousSignedHeaders)
        );
    }

    #[test]
    fn rejects_streaming_and_malformed_payload_hash_forms() {
        for invalid in [
            "STREAMING-AWS4-HMAC-SHA256-PAYLOAD",
            "ABCDEF",
            &"A".repeat(64),
        ] {
            let mut headers = base_headers();
            headers.insert(
                "x-amz-content-sha256",
                HeaderValue::from_str(invalid).unwrap(),
            );
            headers.insert(
                "authorization",
                HeaderValue::from_static("AWS4-HMAC-SHA256 Credential=GKEXAMPLEACCESSKEY/20260719/garage/s3/aws4_request,SignedHeaders=host;x-amz-content-sha256;x-amz-date,Signature=0000000000000000000000000000000000000000000000000000000000000000"),
            );
            assert_eq!(
                verify_s3_sigv4(
                    request(&Method::PUT, "/dos-store/key", None, &headers, "dos-store"),
                    &[credential()]
                ),
                Err(S3SigV4Error::UnsupportedPayloadHash)
            );
        }
    }

    #[test]
    fn rejects_signature_tampering_without_exposing_secret() {
        let mut headers = base_headers();
        sign(
            &mut headers,
            &Method::PUT,
            "/dos-store/key",
            "",
            "host;x-amz-content-sha256;x-amz-date",
        );
        assert_eq!(
            verify_s3_sigv4(
                request(
                    &Method::PUT,
                    "/dos-store/changed",
                    None,
                    &headers,
                    "dos-store"
                ),
                &[credential()]
            ),
            Err(S3SigV4Error::SignatureMismatch)
        );
        assert!(!S3SigV4Error::SignatureMismatch.to_string().contains(SECRET));
    }

    #[test]
    fn canonical_uri_does_not_turn_encoded_slash_into_separator() {
        assert_eq!(canonical_uri("/bucket/a%2fb").unwrap(), "/bucket/a%2Fb");
        assert_eq!(canonical_uri("/bucket/a/b").unwrap(), "/bucket/a/b");
    }

    #[test]
    fn canonical_query_sorts_encoded_names_values_and_preserves_duplicates() {
        assert_eq!(
            canonical_query("z=last&a=two&a=one&space=a%20b").unwrap(),
            "a=one&a=two&space=a%20b&z=last"
        );
    }

    #[test]
    fn malformed_percent_encoding_fails_closed() {
        assert_eq!(
            canonical_uri("/bucket/bad%2").unwrap_err(),
            S3SigV4Error::MalformedUri
        );
        assert_eq!(
            canonical_query("prefix=bad%GG").unwrap_err(),
            S3SigV4Error::MalformedUri
        );
    }

    #[test]
    fn rejects_missing_host_from_the_signed_header_set() {
        let mut headers = base_headers();
        headers.insert(
            "authorization",
            HeaderValue::from_static("AWS4-HMAC-SHA256 Credential=GKEXAMPLEACCESSKEY/20260719/garage/s3/aws4_request,SignedHeaders=x-amz-content-sha256;x-amz-date,Signature=0000000000000000000000000000000000000000000000000000000000000000"),
        );
        assert_eq!(
            verify_s3_sigv4(
                request(&Method::GET, "/dos-store/key", None, &headers, "dos-store"),
                &[credential()]
            ),
            Err(S3SigV4Error::MissingRequiredSignedHeader("host"))
        );
    }

    fn request<'a>(
        method: &'a Method,
        path: &'a str,
        query: Option<&'a str>,
        headers: &'a HeaderMap,
        bucket: &'a str,
    ) -> S3SigV4Request<'a> {
        S3SigV4Request {
            method,
            raw_path: path,
            raw_query: query,
            headers,
            bucket,
        }
    }

    fn base_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("host", HeaderValue::from_static("s3.example"));
        headers.insert("x-amz-date", HeaderValue::from_static(DATE));
        headers.insert(
            "x-amz-content-sha256",
            HeaderValue::from_static(PAYLOAD_HASH),
        );
        headers
    }

    fn credential() -> ManagedStoreCredentialRecord {
        ManagedStoreCredentialRecord {
            store_id: StoreId::new("store-a").unwrap(),
            bucket_name: "dos-store".to_string(),
            credential_reference: "secret://store-a".to_string(),
            access_key_id: ACCESS.to_string(),
            secret_access_key: SECRET.to_string(),
            issued_at_utc: "2026-07-19T00:00:00Z".to_string(),
            rotated_at_utc: None,
            revision: 1,
        }
    }

    fn sign(headers: &mut HeaderMap, method: &Method, path: &str, query: &str, signed: &str) {
        let names = parse_signed_headers(signed).unwrap();
        let canonical = format!(
            "{}\n{}\n{}\n{}\n{}\n{}",
            method.as_str(),
            canonical_uri(path).unwrap(),
            canonical_query(query).unwrap(),
            canonical_headers(headers, &names).unwrap(),
            signed,
            PAYLOAD_HASH
        );
        let scope = "20260719/garage/s3/aws4_request";
        let string_to_sign = format!(
            "{ALGORITHM}\n{DATE}\n{scope}\n{}",
            hex_sha256(canonical.as_bytes())
        );
        let signature = hmac::sign(
            &hmac::Key::new(
                hmac::HMAC_SHA256,
                &signing_key(SECRET.as_bytes(), "20260719", "garage"),
            ),
            string_to_sign.as_bytes(),
        );
        headers.insert(
            "authorization",
            HeaderValue::from_str(&format!(
                "{ALGORITHM} Credential={ACCESS}/{scope},SignedHeaders={signed},Signature={}",
                signature
                    .as_ref()
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            ))
            .unwrap(),
        );
    }
}
