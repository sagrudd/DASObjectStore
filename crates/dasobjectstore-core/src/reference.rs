//! Canonical, non-secret DASObjectStore reference values.
//!
//! These types implement only the accepted ADR-0004 grammar and digest checks.
//! They neither issue nor resolve a reference, and they deliberately hold no
//! capability, transport, storage, or authority behaviour.

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::collections::BTreeSet;
use std::fmt;

pub const OBJECT_REF_V1_SCHEMA: &str = "dasobjectstore.object_ref.v1";
pub const EVIDENCE_REF_V1_SCHEMA: &str = "dasobjectstore.evidence_ref.v1";
const MAX_ENCODED_BYTES: usize = 8192;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DigestV1 {
    pub algorithm: String,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityScopeV1 {
    pub installation_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site_trust_domain_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectRefV1 {
    pub schema: String,
    pub authority_scope: AuthorityScopeV1,
    pub store_id: String,
    pub object_id: String,
    pub object_version: u64,
    pub size_bytes: u64,
    pub content_digest: DigestV1,
    pub domain_digest: DigestV1,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRefV1 {
    pub schema: String,
    pub object_ref: ObjectRefV1,
    pub evidence_kind: String,
    pub evidence_revision: u64,
    pub subject_digest: DigestV1,
    pub domain_digest: DigestV1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReferenceDecodeError {
    BoundsExceeded,
    MalformedJson,
    DuplicateMember,
    InvalidNumberToken,
    UnsupportedSchema,
    InvalidReference(ReferenceValidationError),
}

impl fmt::Display for ReferenceDecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::BoundsExceeded => "bounds_exceeded",
            Self::MalformedJson => "invalid_reference",
            Self::DuplicateMember => "invalid_reference",
            Self::InvalidNumberToken => "invalid_reference",
            Self::UnsupportedSchema => "unsupported_schema",
            Self::InvalidReference(_) => "invalid_reference",
        })
    }
}

impl std::error::Error for ReferenceDecodeError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReferenceValidationError {
    Identifier,
    StoreIdentifier,
    Digest,
    Number,
    DomainDigest,
    Schema,
}

impl ObjectRefV1 {
    pub fn decode(raw: &[u8]) -> Result<Self, ReferenceDecodeError> {
        preflight_json(raw)?;
        require_schema(raw, OBJECT_REF_V1_SCHEMA)?;
        let value: Self =
            serde_json::from_slice(raw).map_err(|_| ReferenceDecodeError::MalformedJson)?;
        value
            .validate()
            .map_err(ReferenceDecodeError::InvalidReference)?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), ReferenceValidationError> {
        if self.schema != OBJECT_REF_V1_SCHEMA {
            return Err(ReferenceValidationError::Schema);
        }
        validate_scope(&self.authority_scope)?;
        if !valid_identifier(&self.store_id) || self.store_id.len() > 64 {
            return Err(ReferenceValidationError::StoreIdentifier);
        }
        if !valid_identifier(&self.object_id) {
            return Err(ReferenceValidationError::Identifier);
        }
        if self.object_version == 0
            || self.object_version > MAX_SAFE_INTEGER
            || self.size_bytes > MAX_SAFE_INTEGER
        {
            return Err(ReferenceValidationError::Number);
        }
        validate_digest(&self.content_digest)?;
        validate_digest(&self.domain_digest)?;
        if self.domain_digest.value != self.expected_domain_digest() {
            return Err(ReferenceValidationError::DomainDigest);
        }
        Ok(())
    }

    pub fn expected_domain_digest(&self) -> String {
        let mut identity = self.clone();
        identity.domain_digest.value.clear();
        let bytes = canonical_object_identity(&identity);
        digest_hex(b"DASOBJECTSTORE_OBJECT_REF_V1\0", &bytes)
    }
}

impl EvidenceRefV1 {
    pub fn decode(raw: &[u8]) -> Result<Self, ReferenceDecodeError> {
        preflight_json(raw)?;
        require_schema(raw, EVIDENCE_REF_V1_SCHEMA)?;
        let value: Self =
            serde_json::from_slice(raw).map_err(|_| ReferenceDecodeError::MalformedJson)?;
        value
            .validate()
            .map_err(ReferenceDecodeError::InvalidReference)?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), ReferenceValidationError> {
        if self.schema != EVIDENCE_REF_V1_SCHEMA {
            return Err(ReferenceValidationError::Schema);
        }
        self.object_ref.validate()?;
        if !valid_identifier(&self.evidence_kind)
            || self.evidence_revision == 0
            || self.evidence_revision > MAX_SAFE_INTEGER
        {
            return Err(ReferenceValidationError::Number);
        }
        validate_digest(&self.subject_digest)?;
        validate_digest(&self.domain_digest)?;
        if self.domain_digest.value != self.expected_domain_digest() {
            return Err(ReferenceValidationError::DomainDigest);
        }
        Ok(())
    }

    pub fn expected_domain_digest(&self) -> String {
        let mut identity = self.clone();
        identity.domain_digest.value.clear();
        let bytes = canonical_evidence_identity(&identity);
        digest_hex(b"DASOBJECTSTORE_EVIDENCE_REF_V1\0", &bytes)
    }
}

fn validate_scope(scope: &AuthorityScopeV1) -> Result<(), ReferenceValidationError> {
    if !valid_identifier(&scope.installation_id)
        || [
            scope.site_trust_domain_id.as_deref(),
            scope.tenant_id.as_deref(),
            scope.project_id.as_deref(),
        ]
        .into_iter()
        .flatten()
        .any(|value| !valid_identifier(value))
    {
        return Err(ReferenceValidationError::Identifier);
    }
    Ok(())
}

fn validate_digest(digest: &DigestV1) -> Result<(), ReferenceValidationError> {
    if digest.algorithm != "sha256"
        || digest.value.len() != 64
        || !digest
            .value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ReferenceValidationError::Digest);
    }
    Ok(())
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.is_ascii()
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || (index != 0 && matches!(byte, b'.' | b'_' | b'-'))
        })
}

fn digest_hex(prefix: &[u8], bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(prefix);
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn canonical_object_identity(value: &ObjectRefV1) -> Vec<u8> {
    let mut value = serde_json::to_value(value).expect("reference serializes");
    value
        .as_object_mut()
        .expect("reference object")
        .remove("domain_digest");
    canonical_json(&value)
}

fn canonical_evidence_identity(value: &EvidenceRefV1) -> Vec<u8> {
    let mut value = serde_json::to_value(value).expect("reference serializes");
    value
        .as_object_mut()
        .expect("reference object")
        .remove("domain_digest");
    canonical_json(&value)
}

// v1 permits only ASCII identifier/digest strings and safe integers. For that
// deliberately restricted subset this writer is RFC 8785/JCS-equivalent: it
// orders object keys by Unicode code point and lets serde_json quote the
// already constrained string values. It intentionally never accepts arbitrary
// floats, display strings, or arrays into an identity projection.
fn canonical_json(value: &serde_json::Value) -> Vec<u8> {
    fn write(value: &serde_json::Value, output: &mut Vec<u8>) {
        match value {
            serde_json::Value::Object(object) => {
                output.push(b'{');
                let mut keys: Vec<_> = object.keys().collect();
                keys.sort_unstable();
                for (index, key) in keys.iter().enumerate() {
                    if index != 0 {
                        output.push(b',');
                    }
                    output.extend(serde_json::to_vec(key).expect("canonical key serializes"));
                    output.push(b':');
                    write(&object[*key], output);
                }
                output.push(b'}');
            }
            serde_json::Value::String(string) => {
                output.extend(serde_json::to_vec(string).expect("canonical string serializes"));
            }
            serde_json::Value::Number(number) => output.extend(number.to_string().bytes()),
            _ => unreachable!("typed v1 identity contains only objects, strings, and integers"),
        }
    }
    let mut output = Vec::new();
    write(value, &mut output);
    output
}

fn require_schema(raw: &[u8], expected: &str) -> Result<(), ReferenceDecodeError> {
    let value: serde_json::Value =
        serde_json::from_slice(raw).map_err(|_| ReferenceDecodeError::MalformedJson)?;
    if value.get("schema").and_then(serde_json::Value::as_str) != Some(expected) {
        return Err(ReferenceDecodeError::UnsupportedSchema);
    }
    Ok(())
}

// Reject duplicate decoded names before serde_json can collapse them. The
// scanner additionally rejects arrays and non-canonical raw number tokens,
// preventing exponent/fractional/negative values from being normalised first.
fn preflight_json(raw: &[u8]) -> Result<(), ReferenceDecodeError> {
    if raw.len() > MAX_ENCODED_BYTES {
        return Err(ReferenceDecodeError::BoundsExceeded);
    }
    let mut parser = JsonPreflight { raw, offset: 0 };
    parser.value(0)?;
    parser.whitespace();
    if parser.offset != raw.len() {
        return Err(ReferenceDecodeError::MalformedJson);
    }
    Ok(())
}

struct JsonPreflight<'a> {
    raw: &'a [u8],
    offset: usize,
}

impl JsonPreflight<'_> {
    fn whitespace(&mut self) {
        while self
            .raw
            .get(self.offset)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.offset += 1;
        }
    }
    fn value(&mut self, depth: usize) -> Result<(), ReferenceDecodeError> {
        if depth > 4 {
            return Err(ReferenceDecodeError::BoundsExceeded);
        }
        self.whitespace();
        match self.raw.get(self.offset) {
            Some(b'{') => self.object(depth + 1),
            Some(b'[') => Err(ReferenceDecodeError::MalformedJson),
            Some(b'"') => self.string().map(|_| ()),
            Some(b'0'..=b'9') => self.number(),
            _ => Err(ReferenceDecodeError::MalformedJson),
        }
    }
    fn object(&mut self, depth: usize) -> Result<(), ReferenceDecodeError> {
        self.offset += 1;
        self.whitespace();
        let mut keys = BTreeSet::new();
        if self.raw.get(self.offset) == Some(&b'}') {
            self.offset += 1;
            return Ok(());
        }
        loop {
            self.whitespace();
            if self.raw.get(self.offset) != Some(&b'"') {
                return Err(ReferenceDecodeError::MalformedJson);
            }
            let key = self.string()?;
            if !keys.insert(key) {
                return Err(ReferenceDecodeError::DuplicateMember);
            }
            self.whitespace();
            if self.raw.get(self.offset) != Some(&b':') {
                return Err(ReferenceDecodeError::MalformedJson);
            }
            self.offset += 1;
            self.value(depth)?;
            self.whitespace();
            match self.raw.get(self.offset) {
                Some(b',') => self.offset += 1,
                Some(b'}') => {
                    self.offset += 1;
                    return Ok(());
                }
                _ => return Err(ReferenceDecodeError::MalformedJson),
            }
        }
    }
    fn string(&mut self) -> Result<String, ReferenceDecodeError> {
        let start = self.offset;
        self.offset += 1;
        let mut escaped = false;
        while let Some(&byte) = self.raw.get(self.offset) {
            self.offset += 1;
            if escaped {
                escaped = false;
                continue;
            }
            if byte == b'\\' {
                escaped = true;
                continue;
            }
            if byte == b'"' {
                return serde_json::from_slice(&self.raw[start..self.offset])
                    .map_err(|_| ReferenceDecodeError::MalformedJson);
            }
            if byte < 0x20 {
                return Err(ReferenceDecodeError::MalformedJson);
            }
        }
        Err(ReferenceDecodeError::MalformedJson)
    }
    fn number(&mut self) -> Result<(), ReferenceDecodeError> {
        let start = self.offset;
        if self.raw[self.offset] == b'0' {
            self.offset += 1;
        } else {
            while self.raw.get(self.offset).is_some_and(u8::is_ascii_digit) {
                self.offset += 1;
            }
        }
        if self.raw[start] == b'0' && self.raw.get(self.offset).is_some_and(u8::is_ascii_digit) {
            return Err(ReferenceDecodeError::InvalidNumberToken);
        }
        if self
            .raw
            .get(self.offset)
            .is_some_and(|byte| matches!(byte, b'.' | b'e' | b'E' | b'+' | b'-'))
        {
            return Err(ReferenceDecodeError::InvalidNumberToken);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join("docs/adr/fixtures")
                .join(name),
        )
        .expect("fixture")
    }

    #[test]
    fn decodes_review_vectors_and_recomputes_digests() {
        let object = ObjectRefV1::decode(&fixture("object-ref-v1.json")).expect("object vector");
        assert_eq!(object.domain_digest.value, object.expected_domain_digest());
        let evidence =
            EvidenceRefV1::decode(&fixture("evidence-ref-v1.json")).expect("evidence vector");
        assert_eq!(
            evidence.domain_digest.value,
            evidence.expected_domain_digest()
        );
    }

    #[test]
    fn rejects_escaped_duplicate_names_before_typed_decode() {
        let raw = br#"{"schema":"dasobjectstore.object_ref.v1","\u0073chema":"dasobjectstore.object_ref.v1"}"#;
        assert_eq!(
            ObjectRefV1::decode(raw),
            Err(ReferenceDecodeError::DuplicateMember)
        );
    }

    #[test]
    fn rejects_noncanonical_raw_numbers_before_deserializing() {
        for number in ["1e0", "1.0", "-1", "01", "9007199254740992"] {
            let raw =
                format!(r#"{{"schema":"dasobjectstore.object_ref.v1","object_version":{number}}}"#);
            assert!(matches!(
                ObjectRefV1::decode(raw.as_bytes()),
                Err(ReferenceDecodeError::InvalidNumberToken
                    | ReferenceDecodeError::MalformedJson
                    | ReferenceDecodeError::BoundsExceeded
                    | ReferenceDecodeError::InvalidReference(ReferenceValidationError::Number))
            ));
        }
    }

    #[test]
    fn rejects_unknown_schema_before_v1_shape_validation() {
        let raw = br#"{"schema":"dasobjectstore.object_ref.v2"}"#;
        assert_eq!(
            ObjectRefV1::decode(raw),
            Err(ReferenceDecodeError::UnsupportedSchema)
        );
    }

    #[test]
    fn is_explicitly_non_authorising() {
        let object = ObjectRefV1::decode(&fixture("object-ref-v1.json")).expect("object vector");
        assert!(format!("{object:?}").contains(OBJECT_REF_V1_SCHEMA));
        // The core type has no endpoint, capability, path, or resolution API.
    }
}
