//! Closed ADR0011 public records. Decoding proves format, never companion admission.
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Explicit additive frontend profile; historical S3 facts retain their meaning.
pub const READER_ADAPTER_PROFILE: &str = "das.custody.exact_object_frontend.v1";
/// Redacted record or continuation denial.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReaderError {
    /// Malformed, noncanonical, out-of-bounds or unsupported public record.
    Format,
    /// Records disagree with selected immutable facts or time.
    Binding,
    /// Filesystem ownership, identity, durability or access failure.
    Boundary,
    /// A competing, incomplete or stale manager transaction exists.
    Conflict,
    /// Acquisition or ledger verification failed.
    Read,
}
impl std::fmt::Display for ReaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "custody continuation denied: {self:?}")
    }
}
impl std::error::Error for ReaderError {}

macro_rules! record {
    ($name:ident, $schema:literal, $limit:expr, {$($field:ident: $ty:ty),* $(,)?}) => {
        #[doc = concat!("Exact public ", $schema, " record; fields are data, not admission.")]
        #[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
        #[serde(deny_unknown_fields)]
        pub struct $name {
            $(#[doc = concat!("Accepted wire field `", stringify!($field), "`.")]
            pub $field: $ty,)*
        }
        impl $name {
            /// Decode exact canonical bounded bytes, rejecting unknown/duplicate fields.
            ///
            /// # Errors
            /// Denies any malformed or semantically invalid record; grants no authority.
            pub fn decode(raw: &[u8]) -> Result<Self, ReaderError> {
                let value: Self = decode(raw, $schema, $limit)?;
                value.check()?;
                Ok(value)
            }
            /// Encode validated public facts as exact JCS without a trailing newline.
            ///
            /// # Errors
            /// Denies invalid public fields.
            pub fn encode(&self) -> Result<Vec<u8>, ReaderError> {
                let bytes = serde_jcs::to_vec(self).map_err(|_| ReaderError::Format)?;
                Self::decode(&bytes)?;
                Ok(bytes)
            }
        }
    };
}
record!(ReaderBindingV1, "das.custody.reader_binding.v1", 16384, {
    schema: String, companion_sha256: String, host_identity_sha256: String,
    service_identity: String, uid: u32, executable_sha256: String,
    package_provenance_sha256: String, credential_name: String,
    encrypted_source_sha256: String, credential_generation: u64,
    backend_key_id: String, reader_identity: String, store_id: String,
    configuration_sha256: String, endpoint_authority_sha256: String,
    tls_peer_sha256: String, read_adapter_profile: String,
    frontend_authority_sha256: String, frontend_tls_peer_sha256: String,
    stores_namespace_sha256: String, bucket_name: String,
    object_lock_policy_sha256: String, inventory_sha256: String, seal_sha256: String,
    verifier_authority_sha256: String, verifier_tls_identity_sha256: String,
    not_before_utc: String, expires_at_utc: String,
});
record!(ReaderCurrentV1, "das.custody.reader_current.v1", 16384, {
    schema: String, binding_sha256: String, credential_generation: u64,
    state: String, updated_at_utc: String,
});
record!(ReaderSealV1, "das.custody.reader_seal.v1", 1048576, {
    schema: String, companion_sha256: String, bootstrap_transaction_id: String,
    store_id: String, configuration_sha256: String, inventory_sha256: String,
    ledger_head_sha256: String, receipt_jcs_sha256: Vec<String>, completed_at_utc: String,
});

impl ReaderBindingV1 {
    fn check(&self) -> Result<(), ReaderError> {
        if self.uid == 0
            || self.uid == u32::MAX
            || self.credential_generation == 0
            || self.read_adapter_profile != READER_ADAPTER_PROFILE
            || self.credential_name.len() > 128
            || !self
                .credential_name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
            || timestamp(&self.not_before_utc)? >= timestamp(&self.expires_at_utc)?
            || dasobjectstore_core::ids::StoreId::new(self.store_id.clone()).is_err()
        {
            return Err(ReaderError::Format);
        }
        Ok(())
    }
    /// Check current selection and seal at the supplied trusted clock observation.
    /// These inputs must come from protected composition, never a transport claim.
    ///
    /// # Errors
    /// Denies revoked, expired, future, mismatched or noncanonical records.
    pub fn verify(
        &self,
        current: &ReaderCurrentV1,
        seal: &ReaderSealV1,
        now: &str,
    ) -> Result<(), ReaderError> {
        let raw = self.encode()?;
        current.encode()?;
        let seal_raw = seal.encode()?;
        let now = timestamp(now)?;
        if current.state != "active"
            || current.binding_sha256 != raw_sha256(&raw)
            || current.credential_generation != self.credential_generation
            || self.seal_sha256 != raw_sha256(&seal_raw)
            || self.companion_sha256 != seal.companion_sha256
            || self.configuration_sha256 != seal.configuration_sha256
            || self.inventory_sha256 != seal.inventory_sha256
            || self.store_id != seal.store_id
            || timestamp(&current.updated_at_utc)? > now
            || timestamp(&seal.completed_at_utc)? > timestamp(&current.updated_at_utc)?
            || timestamp(&self.not_before_utc)? > now
            || timestamp(&self.expires_at_utc)? <= now
        {
            return Err(ReaderError::Binding);
        }
        Ok(())
    }
}
impl ReaderCurrentV1 {
    fn check(&self) -> Result<(), ReaderError> {
        if self.credential_generation == 0 || !matches!(self.state.as_str(), "active" | "revoked") {
            return Err(ReaderError::Format);
        }
        Ok(())
    }
    /// Check a manager transition without granting backend revocation authority.
    ///
    /// # Errors
    /// Denies replay, time rollback, skipped revocation or non-increasing generation.
    pub fn follows(&self, prior: &Self) -> Result<(), ReaderError> {
        self.encode()?;
        prior.encode()?;
        let allowed = if prior.state == "active" {
            self.state == "revoked"
                && self.credential_generation == prior.credential_generation
                && self.binding_sha256 == prior.binding_sha256
        } else {
            self.state == "active" && self.credential_generation > prior.credential_generation
        };
        if !allowed || timestamp(&self.updated_at_utc)? < timestamp(&prior.updated_at_utc)? {
            return Err(ReaderError::Conflict);
        }
        Ok(())
    }
}
impl ReaderSealV1 {
    fn check(&self) -> Result<(), ReaderError> {
        if uuid::Uuid::parse_str(&self.bootstrap_transaction_id)
            .map_err(|_| ReaderError::Format)?
            .to_string()
            != self.bootstrap_transaction_id
            || self.receipt_jcs_sha256.is_empty()
            || self.receipt_jcs_sha256.len() > 4096
            || !self.receipt_jcs_sha256.windows(2).all(|w| w[0] < w[1])
            || !self.receipt_jcs_sha256.iter().all(|s| digest(s))
            || dasobjectstore_core::ids::StoreId::new(self.store_id.clone()).is_err()
        {
            return Err(ReaderError::Format);
        }
        Ok(())
    }
}
/// SHA-256 of exact public bytes; no prefix or implicit reserialization.
pub fn raw_sha256(raw: &[u8]) -> String {
    format!("{:x}", Sha256::digest(raw))
}
fn digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn timestamp(s: &str) -> Result<DateTime<Utc>, ReaderError> {
    let t = DateTime::parse_from_rfc3339(s)
        .map_err(|_| ReaderError::Format)?
        .with_timezone(&Utc);
    if t.to_rfc3339_opts(SecondsFormat::Secs, true) != s {
        return Err(ReaderError::Format);
    }
    Ok(t)
}
fn decode<T: DeserializeOwned + Serialize>(
    raw: &[u8],
    schema: &str,
    limit: usize,
) -> Result<T, ReaderError> {
    if raw.len() > limit {
        return Err(ReaderError::Format);
    }
    let record: T = serde_json::from_slice(raw).map_err(|_| ReaderError::Format)?;
    if serde_jcs::to_vec(&record).map_err(|_| ReaderError::Format)? != raw {
        return Err(ReaderError::Format);
    }
    let value = serde_json::to_value(&record).map_err(|_| ReaderError::Format)?;
    if value.get("schema").and_then(Value::as_str) != Some(schema) {
        return Err(ReaderError::Format);
    }
    for (name, v) in value.as_object().ok_or(ReaderError::Format)? {
        match v {
            Value::String(s) => {
                if s.is_empty()
                    || s.len() > 256
                    || s.chars().any(char::is_control)
                    || (name.ends_with("sha256") && !digest(s))
                {
                    return Err(ReaderError::Format);
                }
                if name.ends_with("_utc") {
                    timestamp(s)?;
                }
            }
            Value::Number(n) if n.as_u64().is_some_and(|n| n <= 9_007_199_254_740_991) => {}
            Value::Array(_) if name == "receipt_jcs_sha256" => {}
            _ => return Err(ReaderError::Format),
        }
    }
    Ok(record)
}

#[cfg(test)]
mod tests;

pub mod wire;
