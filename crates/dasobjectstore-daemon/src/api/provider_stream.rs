//! Bounded metadata for daemon-owned provider byte streams.
//!
//! The Unix-socket transport carries JSON request/response envelopes alongside
//! bounded binary frames. JSON carries request and chunk metadata, while
//! payload bytes travel in length-prefixed frames and never as base64 or
//! backend paths.

use dasobjectstore_core::backend::BackendObjectKey;
use dasobjectstore_core::ids::StoreId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::{self, Display};
use std::io::{self, Read, Write};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

pub const PROVIDER_STREAM_SCHEMA_VERSION: &str = "dasobjectstore.provider_stream.v1";
pub const PROVIDER_STREAM_MAX_CHUNK_BYTES: u32 = 1024 * 1024;
pub const PROVIDER_STREAM_MAX_HEADER_BYTES: u32 = 4096;
const PROVIDER_STREAM_FRAME_MAGIC: &[u8; 4] = b"DPS1";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderStreamOpenRequest {
    pub schema_version: String,
    pub request_id: String,
    pub store_id: StoreId,
    pub object: BackendObjectKey,
    #[serde(default)]
    pub range: Option<ProviderStreamRange>,
    #[serde(default)]
    pub condition: ProviderStreamCondition,
    pub chunk_size_bytes: u32,
}

/// Path-free open envelope for a bounded client-to-daemon provider upload.
///
/// The daemon must treat `upload_id` as an opaque, single-use capability
/// reference. It is deliberately not a filesystem path or a provider
/// credential. Implementations must stage and commit the bytes behind the
/// daemon boundary before publishing catalogue state.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderStreamUploadOpenRequest {
    pub schema_version: String,
    pub request_id: String,
    pub upload_id: String,
    pub store_id: StoreId,
    pub object: BackendObjectKey,
    pub expected_size_bytes: u64,
    pub expected_sha256: String,
    pub chunk_size_bytes: u32,
}

/// Path-free open envelope for one reservation-bound multipart part.
///
/// A part is addressed by the daemon-owned reservation and its one-based
/// number. This is separate from the ordinary single-object upload envelope
/// so retries can be matched to the same reservation/part identity without
/// exposing a staging path.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderStreamMultipartPartUploadOpenRequest {
    pub schema_version: String,
    pub request_id: String,
    pub reservation_id: String,
    pub reservation_size_bytes: u64,
    pub part_number: u32,
    pub store_id: StoreId,
    pub object: BackendObjectKey,
    pub expected_size_bytes: u64,
    pub expected_sha256: String,
    pub chunk_size_bytes: u32,
}

/// Acknowledgement emitted only after the daemon has durably staged and
/// verified one multipart part. Completion consumes these daemon-owned bytes
/// by reservation/part identity; callers never receive a path.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderStreamMultipartPartUploadResponse {
    pub schema_version: String,
    pub request_id: String,
    pub reservation_id: String,
    pub part_number: u32,
    pub store_id: StoreId,
    pub object: BackendObjectKey,
    pub size_bytes: u64,
    pub sha256: String,
}

impl ProviderStreamMultipartPartUploadOpenRequest {
    pub fn validate(&self) -> Result<(), ProviderStreamValidationError> {
        if self.schema_version != PROVIDER_STREAM_SCHEMA_VERSION {
            return Err(ProviderStreamValidationError::UnsupportedSchema {
                schema_version: self.schema_version.clone(),
            });
        }
        validate_non_blank(&self.request_id, "request_id")?;
        validate_non_blank(&self.reservation_id, "reservation_id")?;
        if self.part_number == 0 {
            return Err(ProviderStreamValidationError::InvalidMultipartPartNumber);
        }
        validate_object_key(&self.object)?;
        if self.expected_size_bytes == 0 || self.reservation_size_bytes < self.expected_size_bytes {
            return Err(ProviderStreamValidationError::InvalidMultipartPartSize);
        }
        validate_sha256(&self.expected_sha256, "expected_sha256")?;
        if self.chunk_size_bytes == 0 || self.chunk_size_bytes > PROVIDER_STREAM_MAX_CHUNK_BYTES {
            return Err(ProviderStreamValidationError::ChunkSizeOutOfBounds {
                chunk_size_bytes: self.chunk_size_bytes,
            });
        }
        Ok(())
    }
}

impl ProviderStreamMultipartPartUploadResponse {
    pub fn validate(&self) -> Result<(), ProviderStreamValidationError> {
        if self.schema_version != PROVIDER_STREAM_SCHEMA_VERSION {
            return Err(ProviderStreamValidationError::UnsupportedSchema {
                schema_version: self.schema_version.clone(),
            });
        }
        validate_non_blank(&self.request_id, "request_id")?;
        validate_non_blank(&self.reservation_id, "reservation_id")?;
        if self.part_number == 0 {
            return Err(ProviderStreamValidationError::InvalidMultipartPartNumber);
        }
        validate_object_key(&self.object)?;
        if self.size_bytes == 0 {
            return Err(ProviderStreamValidationError::InvalidMultipartPartSize);
        }
        validate_sha256(&self.sha256, "sha256")?;
        Ok(())
    }
}

/// Path-free acknowledgement emitted only after the daemon has staged,
/// verified, finalized, and catalogue-committed one streamed object.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderStreamUploadResponse {
    pub schema_version: String,
    pub upload_id: String,
    pub store_id: StoreId,
    pub object: BackendObjectKey,
    pub size_bytes: u64,
    pub sha256: String,
}

impl ProviderStreamUploadResponse {
    pub fn from_record(
        upload_id: impl Into<String>,
        store_id: StoreId,
        record: &dasobjectstore_core::backend::BackendObjectRecord,
    ) -> Self {
        Self {
            schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
            upload_id: upload_id.into(),
            store_id,
            object: record.key.clone(),
            size_bytes: record.size_bytes,
            sha256: record.checksum.clone(),
        }
    }
}

impl ProviderStreamUploadOpenRequest {
    pub fn validate(&self) -> Result<(), ProviderStreamValidationError> {
        if self.schema_version != PROVIDER_STREAM_SCHEMA_VERSION {
            return Err(ProviderStreamValidationError::UnsupportedSchema {
                schema_version: self.schema_version.clone(),
            });
        }
        validate_non_blank(&self.request_id, "request_id")?;
        validate_non_blank(&self.upload_id, "upload_id")?;
        validate_object_key(&self.object)?;
        validate_sha256(&self.expected_sha256, "expected_sha256")?;
        if self.chunk_size_bytes == 0 || self.chunk_size_bytes > PROVIDER_STREAM_MAX_CHUNK_BYTES {
            return Err(ProviderStreamValidationError::ChunkSizeOutOfBounds {
                chunk_size_bytes: self.chunk_size_bytes,
            });
        }
        Ok(())
    }
}

impl ProviderStreamOpenRequest {
    pub fn validate(&self) -> Result<(), ProviderStreamValidationError> {
        if self.schema_version != PROVIDER_STREAM_SCHEMA_VERSION {
            return Err(ProviderStreamValidationError::UnsupportedSchema {
                schema_version: self.schema_version.clone(),
            });
        }
        validate_non_blank(&self.request_id, "request_id")?;
        validate_object_key(&self.object)?;
        if self.chunk_size_bytes == 0 || self.chunk_size_bytes > PROVIDER_STREAM_MAX_CHUNK_BYTES {
            return Err(ProviderStreamValidationError::ChunkSizeOutOfBounds {
                chunk_size_bytes: self.chunk_size_bytes,
            });
        }
        if let Some(range) = self.range {
            range.validate()?;
        }
        self.condition.validate()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderStreamRange {
    pub start: u64,
    #[serde(default)]
    pub end_exclusive: Option<u64>,
}

impl ProviderStreamRange {
    pub fn validate(&self) -> Result<(), ProviderStreamValidationError> {
        if self.end_exclusive.is_some_and(|end| end <= self.start) {
            return Err(ProviderStreamValidationError::InvalidRange {
                start: self.start,
                end_exclusive: self.end_exclusive,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderStreamCondition {
    #[serde(default)]
    pub if_match_sha256: Option<String>,
    #[serde(default)]
    pub if_none_match_sha256: Option<String>,
}

impl ProviderStreamCondition {
    fn validate(&self) -> Result<(), ProviderStreamValidationError> {
        if let Some(checksum) = self.if_match_sha256.as_deref() {
            validate_sha256(checksum, "if_match_sha256")?;
        }
        if let Some(checksum) = self.if_none_match_sha256.as_deref() {
            validate_sha256(checksum, "if_none_match_sha256")?;
        }
        Ok(())
    }
}

/// Metadata for one binary frame. The frame payload is deliberately not a
/// field here; callers must enforce `payload_len` before reading that many
/// bytes from the daemon-owned stream.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderStreamChunkHeader {
    pub schema_version: String,
    pub request_id: String,
    pub offset: u64,
    pub payload_len: u32,
    pub final_chunk: bool,
    #[serde(default)]
    pub total_size: Option<u64>,
    #[serde(default)]
    pub sha256: Option<String>,
}

impl ProviderStreamChunkHeader {
    pub fn validate(&self) -> Result<(), ProviderStreamValidationError> {
        if self.schema_version != PROVIDER_STREAM_SCHEMA_VERSION {
            return Err(ProviderStreamValidationError::UnsupportedSchema {
                schema_version: self.schema_version.clone(),
            });
        }
        validate_non_blank(&self.request_id, "request_id")?;
        if self.payload_len > PROVIDER_STREAM_MAX_CHUNK_BYTES {
            return Err(ProviderStreamValidationError::ChunkSizeOutOfBounds {
                chunk_size_bytes: self.payload_len,
            });
        }
        let end = self
            .offset
            .checked_add(self.payload_len as u64)
            .ok_or(ProviderStreamValidationError::OffsetOverflow)?;
        if self.final_chunk {
            let Some(total_size) = self.total_size else {
                return Err(ProviderStreamValidationError::FinalMetadataMissing);
            };
            let Some(checksum) = self.sha256.as_deref() else {
                return Err(ProviderStreamValidationError::FinalMetadataMissing);
            };
            if end != total_size {
                return Err(ProviderStreamValidationError::FinalSizeMismatch { end, total_size });
            }
            validate_sha256(checksum, "sha256")?;
        } else if self.total_size.is_some() || self.sha256.is_some() {
            return Err(ProviderStreamValidationError::NonFinalMetadataPresent);
        }
        Ok(())
    }
}

/// Write one bounded binary frame. The header is JSON metadata; the payload is
/// written as raw bytes after the fixed magic/length prefix and is never
/// embedded in JSON.
pub fn write_provider_stream_frame<W: Write>(
    writer: &mut W,
    header: &ProviderStreamChunkHeader,
    payload: &[u8],
) -> Result<(), ProviderStreamFrameError> {
    header.validate()?;
    if payload.len() != header.payload_len as usize {
        return Err(ProviderStreamFrameError::PayloadLengthMismatch {
            declared: header.payload_len,
            actual: payload.len(),
        });
    }
    let encoded_header = serde_json::to_vec(header)
        .map_err(|error| ProviderStreamFrameError::HeaderEncode(error.to_string()))?;
    if encoded_header.len() > PROVIDER_STREAM_MAX_HEADER_BYTES as usize {
        return Err(ProviderStreamFrameError::HeaderTooLarge {
            header_bytes: encoded_header.len(),
        });
    }
    writer.write_all(PROVIDER_STREAM_FRAME_MAGIC)?;
    writer.write_all(&(encoded_header.len() as u32).to_be_bytes())?;
    writer.write_all(&(payload.len() as u32).to_be_bytes())?;
    writer.write_all(&encoded_header)?;
    writer.write_all(payload)?;
    writer.flush()?;
    Ok(())
}

/// Read one bounded binary frame, allocating at most one configured chunk and
/// a small metadata header. Callers must perform cumulative checksum
/// verification when the final header is received.
pub fn read_provider_stream_frame<R: Read>(
    reader: &mut R,
) -> Result<(ProviderStreamChunkHeader, Vec<u8>), ProviderStreamFrameError> {
    let mut magic = [0; 4];
    reader.read_exact(&mut magic)?;
    if &magic != PROVIDER_STREAM_FRAME_MAGIC {
        return Err(ProviderStreamFrameError::InvalidMagic);
    }
    let header_len = read_u32(reader)?;
    if header_len > PROVIDER_STREAM_MAX_HEADER_BYTES {
        return Err(ProviderStreamFrameError::HeaderTooLarge {
            header_bytes: header_len as usize,
        });
    }
    let payload_len = read_u32(reader)?;
    if payload_len > PROVIDER_STREAM_MAX_CHUNK_BYTES {
        return Err(ProviderStreamFrameError::PayloadTooLarge { payload_len });
    }
    let mut encoded_header = vec![0; header_len as usize];
    reader.read_exact(&mut encoded_header)?;
    let header = serde_json::from_slice::<ProviderStreamChunkHeader>(&encoded_header)
        .map_err(|error| ProviderStreamFrameError::HeaderDecode(error.to_string()))?;
    header.validate()?;
    if payload_len != header.payload_len {
        return Err(ProviderStreamFrameError::PayloadLengthMismatch {
            declared: header.payload_len,
            actual: payload_len as usize,
        });
    }
    let mut payload = vec![0; payload_len as usize];
    reader.read_exact(&mut payload)?;
    Ok((header, payload))
}

/// Cumulatively verify one provider stream before publishing it as complete.
/// Each frame must begin exactly at the previous frame's end; the terminal
/// header supplies the authoritative object size and checksum.
pub struct ProviderStreamVerifier {
    request_id: String,
    next_offset: u64,
    hasher: Sha256,
    cancellation: ProviderStreamCancellation,
}

impl ProviderStreamVerifier {
    pub fn new(request_id: impl Into<String>) -> Result<Self, ProviderStreamVerificationError> {
        let request_id = request_id.into();
        if request_id.trim().is_empty() {
            return Err(ProviderStreamVerificationError::InvalidRequestId);
        }
        Ok(Self {
            request_id,
            next_offset: 0,
            hasher: Sha256::new(),
            cancellation: ProviderStreamCancellation::default(),
        })
    }

    pub fn cancellation_token(&self) -> ProviderStreamCancellation {
        self.cancellation.clone()
    }

    pub fn push(
        &mut self,
        header: &ProviderStreamChunkHeader,
        payload: &[u8],
    ) -> Result<(), ProviderStreamVerificationError> {
        if self.cancellation.is_cancelled() {
            return Err(ProviderStreamVerificationError::Cancelled);
        }
        header.validate()?;
        if header.request_id != self.request_id {
            return Err(ProviderStreamVerificationError::RequestIdMismatch);
        }
        if header.offset != self.next_offset {
            return Err(ProviderStreamVerificationError::NonContiguous {
                expected_offset: self.next_offset,
                actual_offset: header.offset,
            });
        }
        if payload.len() != header.payload_len as usize {
            return Err(ProviderStreamVerificationError::PayloadLengthMismatch {
                declared: header.payload_len,
                actual: payload.len(),
            });
        }
        self.hasher.update(payload);
        self.next_offset = self
            .next_offset
            .checked_add(payload.len() as u64)
            .ok_or(ProviderStreamVerificationError::SizeOverflow)?;
        Ok(())
    }

    pub fn finish(
        mut self,
        header: &ProviderStreamChunkHeader,
        payload: &[u8],
    ) -> Result<u64, ProviderStreamVerificationError> {
        if !header.final_chunk {
            return Err(ProviderStreamVerificationError::FinalHeaderRequired);
        }
        self.push(header, payload)?;
        let total_size = header
            .total_size
            .ok_or(ProviderStreamVerificationError::FinalHeaderRequired)?;
        if total_size != self.next_offset {
            return Err(ProviderStreamVerificationError::FinalSizeMismatch {
                expected: total_size,
                actual: self.next_offset,
            });
        }
        let expected = header
            .sha256
            .as_deref()
            .ok_or(ProviderStreamVerificationError::FinalHeaderRequired)?
            .strip_prefix("sha256:")
            .ok_or(ProviderStreamVerificationError::FinalHeaderRequired)?;
        let actual = format!("{:x}", self.hasher.finalize());
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(ProviderStreamVerificationError::ChecksumMismatch);
        }
        Ok(total_size)
    }
}

#[derive(Clone, Debug, Default)]
pub struct ProviderStreamCancellation(Arc<AtomicBool>);

impl ProviderStreamCancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

fn read_u32<R: Read>(reader: &mut R) -> Result<u32, ProviderStreamFrameError> {
    let mut bytes = [0; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_be_bytes(bytes))
}

#[derive(Debug)]
pub enum ProviderStreamFrameError {
    Io(io::Error),
    InvalidMagic,
    HeaderTooLarge { header_bytes: usize },
    PayloadTooLarge { payload_len: u32 },
    PayloadLengthMismatch { declared: u32, actual: usize },
    HeaderEncode(String),
    HeaderDecode(String),
    Validation(ProviderStreamValidationError),
}

impl Display for ProviderStreamFrameError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "provider stream frame IO failed: {error}"),
            Self::InvalidMagic => formatter.write_str("invalid provider stream frame magic"),
            Self::HeaderTooLarge { header_bytes } => {
                write!(
                    formatter,
                    "provider stream header is too large: {header_bytes} bytes"
                )
            }
            Self::PayloadTooLarge { payload_len } => {
                write!(
                    formatter,
                    "provider stream payload is too large: {payload_len} bytes"
                )
            }
            Self::PayloadLengthMismatch { declared, actual } => write!(
                formatter,
                "provider stream payload length mismatch: declared {declared}, actual {actual}"
            ),
            Self::HeaderEncode(error) => {
                write!(formatter, "provider stream header encode failed: {error}")
            }
            Self::HeaderDecode(error) => {
                write!(formatter, "provider stream header decode failed: {error}")
            }
            Self::Validation(error) => Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for ProviderStreamFrameError {}

impl From<io::Error> for ProviderStreamFrameError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<ProviderStreamValidationError> for ProviderStreamFrameError {
    fn from(error: ProviderStreamValidationError) -> Self {
        Self::Validation(error)
    }
}

#[derive(Debug)]
pub enum ProviderStreamVerificationError {
    InvalidRequestId,
    Cancelled,
    RequestIdMismatch,
    NonContiguous {
        expected_offset: u64,
        actual_offset: u64,
    },
    PayloadLengthMismatch {
        declared: u32,
        actual: usize,
    },
    SizeOverflow,
    FinalHeaderRequired,
    FinalSizeMismatch {
        expected: u64,
        actual: u64,
    },
    ChecksumMismatch,
    InvalidHeader(ProviderStreamValidationError),
}

impl Display for ProviderStreamVerificationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequestId => formatter.write_str("provider stream request id is blank"),
            Self::Cancelled => formatter.write_str("provider stream was cancelled"),
            Self::RequestIdMismatch => formatter.write_str("provider stream request id changed"),
            Self::NonContiguous {
                expected_offset,
                actual_offset,
            } => write!(
                formatter,
                "provider stream offset is not contiguous: expected {expected_offset}, got {actual_offset}"
            ),
            Self::PayloadLengthMismatch { declared, actual } => write!(
                formatter,
                "provider stream payload length mismatch: declared {declared}, actual {actual}"
            ),
            Self::SizeOverflow => formatter.write_str("provider stream size overflows"),
            Self::FinalHeaderRequired => {
                formatter.write_str("provider stream requires a terminal header")
            }
            Self::FinalSizeMismatch { expected, actual } => write!(
                formatter,
                "provider stream final size mismatch: expected {expected}, got {actual}"
            ),
            Self::ChecksumMismatch => formatter.write_str("provider stream checksum mismatch"),
            Self::InvalidHeader(error) => Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for ProviderStreamVerificationError {}

impl From<ProviderStreamValidationError> for ProviderStreamVerificationError {
    fn from(error: ProviderStreamValidationError) -> Self {
        Self::InvalidHeader(error)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderStreamValidationError {
    UnsupportedSchema {
        schema_version: String,
    },
    InvalidField {
        field: &'static str,
    },
    InvalidObjectKey,
    InvalidMultipartPartNumber,
    InvalidMultipartPartSize,
    InvalidRange {
        start: u64,
        end_exclusive: Option<u64>,
    },
    ChunkSizeOutOfBounds {
        chunk_size_bytes: u32,
    },
    OffsetOverflow,
    FinalMetadataMissing,
    FinalSizeMismatch {
        end: u64,
        total_size: u64,
    },
    NonFinalMetadataPresent,
}

impl Display for ProviderStreamValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema { schema_version } => {
                write!(
                    formatter,
                    "unsupported provider stream schema {schema_version}"
                )
            }
            Self::InvalidField { field } => write!(formatter, "{field} must not be blank"),
            Self::InvalidObjectKey => formatter.write_str("provider stream object key is invalid"),
            Self::InvalidMultipartPartNumber => {
                formatter.write_str("multipart part number must be greater than zero")
            }
            Self::InvalidMultipartPartSize => {
                formatter.write_str("multipart part size must be greater than zero")
            }
            Self::InvalidRange {
                start,
                end_exclusive,
            } => write!(formatter, "invalid byte range {start}..{end_exclusive:?}"),
            Self::ChunkSizeOutOfBounds { chunk_size_bytes } => write!(
                formatter,
                "provider stream chunk size {chunk_size_bytes} exceeds the bounded limit"
            ),
            Self::OffsetOverflow => formatter.write_str("provider stream offset overflows"),
            Self::FinalMetadataMissing => {
                formatter.write_str("final provider stream chunk requires size and checksum")
            }
            Self::FinalSizeMismatch { end, total_size } => write!(
                formatter,
                "final provider stream chunk ends at {end}, expected {total_size}"
            ),
            Self::NonFinalMetadataPresent => formatter.write_str(
                "non-final provider stream chunks must not carry terminal size or checksum",
            ),
        }
    }
}

impl std::error::Error for ProviderStreamValidationError {}

fn validate_non_blank(
    value: &str,
    field: &'static str,
) -> Result<(), ProviderStreamValidationError> {
    if value.trim().is_empty() {
        return Err(ProviderStreamValidationError::InvalidField { field });
    }
    Ok(())
}

fn validate_object_key(key: &BackendObjectKey) -> Result<(), ProviderStreamValidationError> {
    if key.version == 0
        || key.object_id.trim().is_empty()
        || key.object_id.starts_with('/')
        || key.object_id.ends_with('/')
        || key.object_id.contains('\\')
        || key.object_id.contains('\0')
        || key
            .object_id
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(ProviderStreamValidationError::InvalidObjectKey);
    }
    Ok(())
}

fn validate_sha256(value: &str, field: &'static str) -> Result<(), ProviderStreamValidationError> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(ProviderStreamValidationError::InvalidField { field });
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(ProviderStreamValidationError::InvalidField { field });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> ProviderStreamOpenRequest {
        ProviderStreamOpenRequest {
            schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
            request_id: "stream-1".to_string(),
            store_id: StoreId::new("store-1").expect("store"),
            object: BackendObjectKey {
                object_id: "folder/file.bin".to_string(),
                version: 1,
            },
            range: Some(ProviderStreamRange {
                start: 0,
                end_exclusive: Some(4096),
            }),
            condition: ProviderStreamCondition {
                if_match_sha256: Some(format!("sha256:{}", "a".repeat(64))),
                if_none_match_sha256: None,
            },
            chunk_size_bytes: 4096,
        }
    }

    fn upload_request() -> ProviderStreamUploadOpenRequest {
        ProviderStreamUploadOpenRequest {
            schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
            request_id: "upload-stream-1".to_string(),
            upload_id: "capability-1".to_string(),
            store_id: StoreId::new("store-1").expect("store"),
            object: BackendObjectKey {
                object_id: "folder/file.bin".to_string(),
                version: 1,
            },
            expected_size_bytes: 5,
            expected_sha256:
                "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
                    .to_string(),
            chunk_size_bytes: 4096,
        }
    }

    #[test]
    fn open_request_round_trips_and_validates() {
        let request = request();
        request.validate().expect("valid stream request");
        let encoded = serde_json::to_string(&request).expect("encode");
        let decoded: ProviderStreamOpenRequest = serde_json::from_str(&encoded).expect("decode");
        assert_eq!(decoded, request);
    }

    #[test]
    fn upload_open_request_round_trips_and_rejects_invalid_capability() {
        let request = upload_request();
        request.validate().expect("valid upload request");
        let encoded = serde_json::to_string(&request).expect("encode");
        let decoded: ProviderStreamUploadOpenRequest =
            serde_json::from_str(&encoded).expect("decode");
        assert_eq!(decoded, request);

        let mut invalid = request;
        invalid.upload_id.clear();
        assert!(matches!(
            invalid.validate(),
            Err(ProviderStreamValidationError::InvalidField { field: "upload_id" })
        ));
    }

    #[test]
    fn multipart_part_open_request_is_path_free_and_retry_addressable() {
        let request = ProviderStreamMultipartPartUploadOpenRequest {
            schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
            request_id: "multipart-frame-1".to_string(),
            reservation_id: "reservation-1".to_string(),
            reservation_size_bytes: 10,
            part_number: 2,
            store_id: StoreId::new("store-1").expect("store"),
            object: BackendObjectKey {
                object_id: "folder/object.bin".to_string(),
                version: 1,
            },
            expected_size_bytes: 5,
            expected_sha256:
                "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
                    .to_string(),
            chunk_size_bytes: 1024,
        };
        request.validate().expect("valid multipart part request");
        let encoded = serde_json::to_string(&request).expect("encode");
        assert!(!encoded.contains("/") || encoded.contains("folder/object.bin"));
        let decoded: ProviderStreamMultipartPartUploadOpenRequest =
            serde_json::from_str(&encoded).expect("decode");
        assert_eq!(decoded, request);

        let mut invalid = request;
        invalid.part_number = 0;
        assert!(matches!(
            invalid.validate(),
            Err(ProviderStreamValidationError::InvalidMultipartPartNumber)
        ));
    }

    #[test]
    fn chunk_header_requires_bounded_terminal_metadata() {
        let header = ProviderStreamChunkHeader {
            schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
            request_id: "stream-1".to_string(),
            offset: 0,
            payload_len: 4,
            final_chunk: true,
            total_size: Some(4),
            sha256: Some(format!("sha256:{}", "b".repeat(64))),
        };
        header.validate().expect("valid final header");

        let mut mismatch = header.clone();
        mismatch.total_size = Some(5);
        assert!(matches!(
            mismatch.validate(),
            Err(ProviderStreamValidationError::FinalSizeMismatch { .. })
        ));

        let mut non_final = header;
        non_final.final_chunk = false;
        assert!(matches!(
            non_final.validate(),
            Err(ProviderStreamValidationError::NonFinalMetadataPresent)
        ));
    }

    #[test]
    fn binary_frame_round_trips_without_json_payload() {
        let header = ProviderStreamChunkHeader {
            schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
            request_id: "stream-1".to_string(),
            offset: 4,
            payload_len: 3,
            final_chunk: false,
            total_size: None,
            sha256: None,
        };
        let mut encoded = Vec::new();
        write_provider_stream_frame(&mut encoded, &header, b"abc").expect("write frame");
        assert_eq!(&encoded[..4], b"DPS1");
        let (decoded, payload) =
            read_provider_stream_frame(&mut encoded.as_slice()).expect("read frame");
        assert_eq!(decoded, header);
        assert_eq!(payload, b"abc");
    }

    #[test]
    fn binary_frame_rejects_payload_mismatch_and_oversized_header() {
        let header = ProviderStreamChunkHeader {
            schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
            request_id: "stream-1".to_string(),
            offset: 0,
            payload_len: 3,
            final_chunk: false,
            total_size: None,
            sha256: None,
        };
        let mut encoded = Vec::new();
        assert!(matches!(
            write_provider_stream_frame(&mut encoded, &header, b"ab"),
            Err(ProviderStreamFrameError::PayloadLengthMismatch { .. })
        ));
        let mut oversized = b"DPS1".to_vec();
        oversized.extend_from_slice(&(PROVIDER_STREAM_MAX_HEADER_BYTES + 1).to_be_bytes());
        oversized.extend_from_slice(&0_u32.to_be_bytes());
        assert!(matches!(
            read_provider_stream_frame(&mut oversized.as_slice()),
            Err(ProviderStreamFrameError::HeaderTooLarge { .. })
        ));
    }

    #[test]
    fn verifier_requires_contiguous_chunks_and_matching_final_checksum() {
        let mut verifier = ProviderStreamVerifier::new("stream-1").expect("verifier");
        let first = ProviderStreamChunkHeader {
            schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
            request_id: "stream-1".to_string(),
            offset: 0,
            payload_len: 3,
            final_chunk: false,
            total_size: None,
            sha256: None,
        };
        verifier.push(&first, b"abc").expect("first chunk");
        let gap = ProviderStreamChunkHeader {
            offset: 4,
            payload_len: 3,
            ..first.clone()
        };
        assert!(matches!(
            verifier.push(&gap, b"def"),
            Err(ProviderStreamVerificationError::NonContiguous { .. })
        ));

        let mut all = Sha256::new();
        all.update(b"abcdef");
        let final_header = ProviderStreamChunkHeader {
            offset: 3,
            payload_len: 3,
            final_chunk: true,
            total_size: Some(6),
            sha256: Some(format!("sha256:{:x}", all.finalize())),
            ..first
        };
        assert_eq!(verifier.finish(&final_header, b"def").expect("finish"), 6);
    }

    #[test]
    fn verifier_honors_cooperative_cancellation_before_next_frame() {
        let mut verifier = ProviderStreamVerifier::new("stream-1").expect("verifier");
        let cancellation = verifier.cancellation_token();
        cancellation.cancel();
        let header = ProviderStreamChunkHeader {
            schema_version: PROVIDER_STREAM_SCHEMA_VERSION.to_string(),
            request_id: "stream-1".to_string(),
            offset: 0,
            payload_len: 1,
            final_chunk: false,
            total_size: None,
            sha256: None,
        };
        assert!(matches!(
            verifier.push(&header, b"x"),
            Err(ProviderStreamVerificationError::Cancelled)
        ));
    }

    #[test]
    fn open_request_rejects_traversal_and_unknown_fields() {
        let mut invalid = request();
        invalid.object.object_id = "../escape".to_string();
        assert!(matches!(
            invalid.validate(),
            Err(ProviderStreamValidationError::InvalidObjectKey)
        ));

        let mut value = serde_json::to_value(request()).expect("encode");
        value["unexpected"] = serde_json::json!(true);
        assert!(serde_json::from_value::<ProviderStreamOpenRequest>(value).is_err());
    }
}
