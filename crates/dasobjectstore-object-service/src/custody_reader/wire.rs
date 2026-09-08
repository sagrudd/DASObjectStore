//! ADR0011 finite wire codecs. Parsing authenticates neither transport nor lifecycle.
//!
//! Complete-buffer private decoders require observed EOF; callers must bound collection
//! with the existing whole-call deadline. HTTP ingress returns only the first request's
//! consumed length: the connection owner must close without dispatching another request.
use super::{decode, digest, raw_sha256, ReaderError, ReaderSealV1};
use serde::{Deserialize, Serialize};

pub mod http;

/// Maximum private request or response metadata size.
pub const FRAME_LIMIT: usize = 4096;
/// Maximum existing signed-request envelope size, without changing its scalar rules.
pub const SIGNED_REQUEST_LIMIT: usize = 1_048_576;
/// The only application failure record, deliberately without diagnostic detail.
pub const DENIED: &[u8] = b"{\"code\":\"denied\",\"schema\":\"das.custody.reader_error.v1\"}";

record!(BootstrapReadV1, "das.custody.bootstrap_read.v1", FRAME_LIMIT, {
    schema: String, bootstrap_transaction_id: String, object_key: String,
    content_length: u64,
});
record!(BootstrapSealV1, "das.custody.bootstrap_seal.v1", FRAME_LIMIT, {
    schema: String, bootstrap_transaction_id: String, inventory_sha256: String,
});
record!(BootstrapReadResultV1, "das.custody.bootstrap_read_result.v1", FRAME_LIMIT, {
    schema: String, object_key: String, content_length: u64, content_sha256: String,
});
record!(ReaderResultV1, "das.custody.reader_result.v1", FRAME_LIMIT, {
    schema: String, request_sha256: String, receipt_jcs_sha256: String,
    configuration_sha256: String, ledger_head_sha256: String, content_length: u64,
});

fn transaction(value: &str) -> Result<(), ReaderError> {
    if uuid::Uuid::parse_str(value)
        .map_err(|_| ReaderError::Format)?
        .to_string()
        != value
    {
        return Err(ReaderError::Format);
    }
    Ok(())
}
fn object_key(key: &str) -> Result<&str, ReaderError> {
    key.strip_prefix("custody/sha256/")
        .filter(|hash| digest(hash))
        .ok_or(ReaderError::Format)
}
impl BootstrapReadV1 {
    fn check(&self) -> Result<(), ReaderError> {
        transaction(&self.bootstrap_transaction_id)?;
        object_key(&self.object_key)?;
        bounded_length(self.content_length, isize::MAX as usize)?;
        Ok(())
    }
}
impl BootstrapSealV1 {
    fn check(&self) -> Result<(), ReaderError> {
        transaction(&self.bootstrap_transaction_id)
    }
}
impl BootstrapReadResultV1 {
    fn check(&self) -> Result<(), ReaderError> {
        if object_key(&self.object_key)? != self.content_sha256 {
            return Err(ReaderError::Binding);
        }
        bounded_length(self.content_length, isize::MAX as usize)?;
        Ok(())
    }
}
impl ReaderResultV1 {
    fn check(&self) -> Result<(), ReaderError> {
        bounded_length(self.content_length, isize::MAX as usize)?;
        Ok(())
    }
}
pub(super) fn bounded_length(value: u64, maximum: usize) -> Result<usize, ReaderError> {
    let size = usize::try_from(value).map_err(|_| ReaderError::Format)?;
    if size == 0 || size > maximum || size > isize::MAX as usize {
        return Err(ReaderError::Format);
    }
    Ok(size)
}

/// A syntactically checked bootstrap operation, not a peer or inventory admission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BootstrapRequest {
    /// Exact selected-object readback request.
    Read(BootstrapReadV1),
    /// Completed-inventory seal request.
    Seal(BootstrapSealV1),
}
impl BootstrapRequest {
    /// Decode one complete request only after EOF, with no trailing frame or bytes.
    ///
    /// # Errors
    /// Rejects malformed framing, missing EOF and unsupported or invalid records.
    pub fn decode_frame(raw: &[u8], eof: bool) -> Result<Self, ReaderError> {
        let (metadata, body) = split_frame(raw, FRAME_LIMIT)?;
        if !eof || !body.is_empty() {
            return Err(ReaderError::Format);
        }
        BootstrapReadV1::decode(metadata)
            .map(Self::Read)
            .or_else(|_| BootstrapSealV1::decode(metadata).map(Self::Seal))
    }
    /// Encode exactly one validated request; the socket caller still must shutdown Write.
    ///
    /// # Errors
    /// Rejects invalid records or resource overflow.
    pub fn encode_frame(&self) -> Result<Vec<u8>, ReaderError> {
        let raw = match self {
            Self::Read(value) => value.encode()?,
            Self::Seal(value) => value.encode()?,
        };
        frame(&raw, &[])
    }
}

pub(super) fn split_frame(raw: &[u8], limit: usize) -> Result<(&[u8], &[u8]), ReaderError> {
    let length = u32::from_be_bytes(
        raw.get(..4)
            .ok_or(ReaderError::Format)?
            .try_into()
            .map_err(|_| ReaderError::Format)?,
    ) as usize;
    if length == 0 || length > limit {
        return Err(ReaderError::Format);
    }
    let end = 4_usize.checked_add(length).ok_or(ReaderError::Format)?;
    Ok((
        raw.get(4..end).ok_or(ReaderError::Format)?,
        raw.get(end..).ok_or(ReaderError::Format)?,
    ))
}
pub(super) fn frame(metadata: &[u8], body: &[u8]) -> Result<Vec<u8>, ReaderError> {
    let length = u32::try_from(metadata.len()).map_err(|_| ReaderError::Format)?;
    let total = 4_usize
        .checked_add(metadata.len())
        .and_then(|n| n.checked_add(body.len()))
        .filter(|n| *n <= isize::MAX as usize)
        .ok_or(ReaderError::Format)?;
    let mut out = Vec::new();
    out.try_reserve_exact(total)
        .map_err(|_| ReaderError::Format)?;
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(metadata);
    out.extend_from_slice(body);
    Ok(out)
}

/// Encode an already verified complete readback; never send success before full validation.
///
/// # Errors
/// Rejects an invalid record, excessive body or bytes not matching its content digest.
pub fn encode_readback(
    value: &BootstrapReadResultV1,
    body: &[u8],
    maximum: usize,
) -> Result<Vec<u8>, ReaderError> {
    let metadata = value.encode()?;
    if bounded_length(value.content_length, maximum)? != body.len()
        || raw_sha256(body) != value.content_sha256
    {
        return Err(ReaderError::Binding);
    }
    frame(&metadata, body)
}

/// Validate a complete readback against the independently selected request and actual bytes.
///
/// # Errors
/// Rejects partial/extra bodies, missing EOF, mismatched selection or corrupt bytes.
pub fn decode_readback<'a>(
    raw: &'a [u8],
    eof: bool,
    selected: &BootstrapReadV1,
    maximum: usize,
) -> Result<&'a [u8], ReaderError> {
    selected.encode()?;
    let (metadata, body) = split_frame(raw, FRAME_LIMIT)?;
    let result = BootstrapReadResultV1::decode(metadata)?;
    if !eof
        || result.object_key != selected.object_key
        || result.content_length != selected.content_length
        || bounded_length(result.content_length, maximum)? != body.len()
        || raw_sha256(body) != result.content_sha256
    {
        return Err(ReaderError::Binding);
    }
    Ok(body)
}

/// Decode a complete exact seal response, preserving the existing seal codec.
///
/// # Errors
/// Rejects trailing bytes, absent EOF, invalid seal or mismatched selected transaction/inventory.
pub fn decode_seal(
    raw: &[u8],
    eof: bool,
    selected: &BootstrapSealV1,
) -> Result<ReaderSealV1, ReaderError> {
    selected.encode()?;
    let (metadata, body) = split_frame(raw, SIGNED_REQUEST_LIMIT)?;
    let seal = ReaderSealV1::decode(metadata)?;
    if !eof
        || !body.is_empty()
        || seal.bootstrap_transaction_id != selected.bootstrap_transaction_id
        || seal.inventory_sha256 != selected.inventory_sha256
    {
        return Err(ReaderError::Binding);
    }
    Ok(seal)
}

/// Encode an existing validated seal without an object body.
///
/// # Errors
/// Rejects an invalid seal or allocation overflow.
pub fn encode_seal(seal: &ReaderSealV1) -> Result<Vec<u8>, ReaderError> {
    frame(&seal.encode()?, &[])
}

/// Encode the sole redacted private failure, with no detail or object body.
///
/// # Errors
/// Rejects allocation failure.
pub fn encode_denied() -> Result<Vec<u8>, ReaderError> {
    frame(DENIED, &[])
}

/// Recognize only the exact private failure record after observed EOF.
///
/// # Errors
/// Rejects any different record, trailing body or absent EOF.
pub fn decode_denied(raw: &[u8], eof: bool) -> Result<(), ReaderError> {
    let (metadata, body) = split_frame(raw, FRAME_LIMIT)?;
    if !eof || metadata != DENIED || !body.is_empty() {
        return Err(ReaderError::Format);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
