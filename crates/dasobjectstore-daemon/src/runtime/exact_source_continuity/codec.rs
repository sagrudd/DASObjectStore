//! Closed canonical JSON grammar. These values carry data, never authority.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub(super) const PURPOSE: &str =
    "thesaurophylax.github-exact-source-qualification-credential-projection.v1";
pub(super) const BODY_LIMIT: usize = 131_072;
pub(super) const HISTORY_LIMIT: usize = 65_536;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Binding {
    pub schema: String,
    pub installation_id: String,
    pub site_uuid: String,
    pub purpose: String,
    pub service_component: String,
    pub owner: String,
    pub application_id: String,
    pub application_generation: String,
    pub client_leaf_sha256: String,
    pub server_leaf_sha256: String,
    pub server_authority_sha256: String,
    pub installed_service_sha256: String,
    pub backend_authority_sha256: String,
    pub backend_namespace_sha256: String,
    pub backend_policy_sha256: String,
    pub enrollment_original_sha256: String,
    pub state: String,
    pub issued_at: String,
    pub expires_at: String,
}

pub(super) fn binding(bytes: &[u8]) -> Result<Binding, Error> {
    if bytes.is_empty() || bytes.len() > 16_384 {
        return Err(Error::Denied);
    }
    let value: Binding = serde_json::from_slice(bytes).map_err(|_| Error::Denied)?;
    if canonical(&value)? != bytes
        || value.schema != "dasobjectstore.exact_source_continuity_binding.v1"
        || value.service_component != "dasobjectstore"
        || !matches!(value.state.as_str(), "active" | "revoked")
    {
        return Err(Error::Denied);
    }
    coordinates(
        &value.installation_id,
        &value.site_uuid,
        &value.purpose,
        &value.owner,
    )?;
    let label = value.application_id.as_bytes();
    if label.is_empty()
        || label.len() > 128
        || !label[0].is_ascii_alphanumeric()
        || !label
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(byte))
    {
        return Err(Error::Denied);
    }
    decimal(&value.application_generation, true)?;
    let issued = decimal(&value.issued_at, true)?;
    if decimal(&value.expires_at, true)? <= issued {
        return Err(Error::Denied);
    }
    for digest in [
        &value.client_leaf_sha256,
        &value.server_leaf_sha256,
        &value.server_authority_sha256,
        &value.installed_service_sha256,
        &value.backend_authority_sha256,
        &value.backend_namespace_sha256,
        &value.backend_policy_sha256,
        &value.enrollment_original_sha256,
    ] {
        hash(digest)?;
    }
    Ok(value)
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Checkpoint {
    pub schema: String,
    pub installation_id: String,
    pub site_uuid: String,
    pub purpose: String,
    pub owner: String,
    pub sequence: String,
    pub previous_root_sha256: String,
    pub operation_id: String,
    pub operation_original_sha256: String,
    pub history_root_sha256: String,
    pub tombstone_root_sha256: String,
    pub floor: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Object {
    pub checkpoint: Checkpoint,
    pub operation_original_hex: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Cas {
    pub schema: String,
    pub operation: String,
    pub binding_sha256: String,
    pub installation_id: String,
    pub site_uuid: String,
    pub purpose: String,
    pub owner: String,
    pub expected_sequence: String,
    pub expected_root_sha256: String,
    pub expected_floor: String,
    pub checkpoint: Checkpoint,
    pub operation_original_hex: String,
    pub challenge: String,
    pub issued_at: String,
    pub expires_at: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Read {
    pub schema: String,
    pub operation: String,
    pub binding_sha256: String,
    pub installation_id: String,
    pub site_uuid: String,
    pub purpose: String,
    pub owner: String,
    pub challenge: String,
    pub issued_at: String,
    pub expires_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence: Option<String>,
}

pub(super) enum Request {
    Cas(Cas),
    Read(Read),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Error {
    Denied,
    Unavailable,
    Conflict,
}

pub(super) fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub(super) fn canonical<T: Serialize>(value: &T) -> Result<Vec<u8>, Error> {
    serde_jcs::to_vec(value).map_err(|_| Error::Denied)
}

pub(super) fn decimal(value: &str, positive: bool) -> Result<u64, Error> {
    if value.is_empty()
        || value.len() > 20
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return Err(Error::Denied);
    }
    let number = value.parse::<u64>().map_err(|_| Error::Denied)?;
    if positive && number == 0 {
        return Err(Error::Denied);
    }
    Ok(number)
}

pub(super) fn hash(value: &str) -> Result<(), Error> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || value.bytes().all(|byte| byte == b'0')
    {
        return Err(Error::Denied);
    }
    Ok(())
}

fn uuid(value: &str) -> Result<(), Error> {
    let parsed = uuid::Uuid::parse_str(value).map_err(|_| Error::Denied)?;
    if parsed.is_nil() || parsed.hyphenated().to_string() != value {
        return Err(Error::Denied);
    }
    Ok(())
}

pub(super) fn coordinates(
    installation: &str,
    site: &str,
    purpose: &str,
    owner: &str,
) -> Result<(), Error> {
    uuid(installation)?;
    uuid(site)?;
    if purpose != PURPOSE || !matches!(owner, "proxenos" | "thesaurophylax") {
        return Err(Error::Denied);
    }
    Ok(())
}

pub(super) fn original(value: &str) -> Result<Vec<u8>, Error> {
    if value.is_empty()
        || value.len() > 65_536
        || !value.len().is_multiple_of(2)
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(Error::Denied);
    }
    hex::decode(value).map_err(|_| Error::Denied)
}

impl Checkpoint {
    pub(super) fn validate(&self) -> Result<(), Error> {
        if self.schema != "dasobjectstore.exact_source_checkpoint.v1" {
            return Err(Error::Denied);
        }
        coordinates(
            &self.installation_id,
            &self.site_uuid,
            &self.purpose,
            &self.owner,
        )?;
        uuid(&self.operation_id)?;
        decimal(&self.sequence, true)?;
        let floor = decimal(&self.floor, true)?;
        if floor < decimal(&self.created_at, true)? {
            return Err(Error::Denied);
        }
        for value in [
            &self.previous_root_sha256,
            &self.operation_original_sha256,
            &self.history_root_sha256,
            &self.tombstone_root_sha256,
        ] {
            hash(value)?;
        }
        Ok(())
    }

    pub(super) fn root(&self) -> Result<String, Error> {
        self.validate()?;
        let mut bytes = b"DAS_EXACT_SOURCE_CONTINUITY_ROOT_V1\0".to_vec();
        bytes.extend(canonical(self)?);
        Ok(digest(&bytes))
    }
}

impl Object {
    pub(super) fn validate(&self) -> Result<(), Error> {
        self.checkpoint.validate()?;
        if digest(&original(&self.operation_original_hex)?)
            != self.checkpoint.operation_original_sha256
            || canonical(self)?.len() > BODY_LIMIT
        {
            return Err(Error::Denied);
        }
        Ok(())
    }
}

fn time(issued: &str, expires: &str, now: u64) -> Result<(), Error> {
    let issued = decimal(issued, true)?;
    let expires = decimal(expires, true)?;
    if issued > now || now >= expires || !matches!(expires.checked_sub(issued), Some(1..=2)) {
        return Err(Error::Denied);
    }
    Ok(())
}

pub(super) fn decode(bytes: &[u8], now: u64) -> Result<Request, Error> {
    if bytes.is_empty() || bytes.len() > BODY_LIMIT {
        return Err(Error::Denied);
    }
    // Typed parsing rejects duplicate known fields. Byte equality rejects whitespace,
    // alternative escapes/order and every non-canonical representation.
    if let Ok(cas) = serde_json::from_slice::<Cas>(bytes) {
        if canonical(&cas)? != bytes
            || cas.schema != "dasobjectstore.exact_source_continuity_request.v1"
            || cas.operation != "cas"
        {
            return Err(Error::Denied);
        }
        coordinates(
            &cas.installation_id,
            &cas.site_uuid,
            &cas.purpose,
            &cas.owner,
        )?;
        for value in [
            &cas.binding_sha256,
            &cas.expected_root_sha256,
            &cas.challenge,
        ] {
            hash(value)?;
        }
        decimal(&cas.expected_sequence, false)?;
        decimal(&cas.expected_floor, false)?;
        time(&cas.issued_at, &cas.expires_at, now)?;
        Object {
            checkpoint: cas.checkpoint.clone(),
            operation_original_hex: cas.operation_original_hex.clone(),
        }
        .validate()?;
        if cas.checkpoint.installation_id != cas.installation_id
            || cas.checkpoint.site_uuid != cas.site_uuid
            || cas.checkpoint.purpose != cas.purpose
            || cas.checkpoint.owner != cas.owner
        {
            return Err(Error::Denied);
        }
        return Ok(Request::Cas(cas));
    }
    let read: Read = serde_json::from_slice(bytes).map_err(|_| Error::Denied)?;
    if canonical(&read)? != bytes
        || read.schema != "dasobjectstore.exact_source_continuity_request.v1"
        || !matches!(read.operation.as_str(), "latest" | "history")
        || (read.operation == "history") != read.sequence.is_some()
    {
        return Err(Error::Denied);
    }
    if let Some(sequence) = &read.sequence {
        decimal(sequence, true)?;
    }
    coordinates(
        &read.installation_id,
        &read.site_uuid,
        &read.purpose,
        &read.owner,
    )?;
    hash(&read.binding_sha256)?;
    hash(&read.challenge)?;
    time(&read.issued_at, &read.expires_at, now)?;
    Ok(Request::Read(read))
}

/// Complete buffered HTTP request grammar. This does not authenticate the Host
/// argument; installed purpose origin must independently supply that authority.
pub(super) fn http_body<'a>(bytes: &'a [u8], authority: &str) -> Result<&'a [u8], Error> {
    if bytes.len() > BODY_LIMIT + 8_192 {
        return Err(Error::Denied);
    }
    let split = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or(Error::Denied)?;
    if split + 4 > 8_192 {
        return Err(Error::Denied);
    }
    let header = std::str::from_utf8(&bytes[..split]).map_err(|_| Error::Denied)?;
    if !header.is_ascii() {
        return Err(Error::Denied);
    }
    let mut lines = header.split("\r\n");
    if lines.next() != Some("POST /continuity/v1/exact-source HTTP/1.1") {
        return Err(Error::Denied);
    }
    let mut fields = std::collections::BTreeMap::new();
    for line in lines {
        let (key, value) = line.split_once(": ").ok_or(Error::Denied)?;
        if fields.insert(key, value).is_some() {
            return Err(Error::Denied);
        }
    }
    if fields.len() != 4
        || fields.get("Host") != Some(&authority)
        || fields.get("Content-Type") != Some(&"application/json")
        || fields.get("Connection") != Some(&"close")
    {
        return Err(Error::Denied);
    }
    let length = decimal(fields.get("Content-Length").ok_or(Error::Denied)?, false)?;
    let length = usize::try_from(length).map_err(|_| Error::Denied)?;
    let body = &bytes[split + 4..];
    if length > BODY_LIMIT || body.len() != length {
        return Err(Error::Denied);
    }
    Ok(body)
}
