//! Server-owned exact request binding around the real loaded continuation.
//! This module has no listener or TLS claim; installation data is not admission.
use super::*;
use chrono::{DateTime, Utc};
use dasobjectstore_object_service::custody_attestation::{
    CustodyEd25519AuthorityV1, CustodyOffNucPreReadRequestV1,
};
use dasobjectstore_object_service::custody_reader::wire::{http, ReaderResultV1};
use std::collections::BTreeMap;
pub mod tls;

/// A receipt's exact retained bytes and independently selected request measurements.
/// Only attempt identifiers/sequence/times vary; all other existing fields compare exactly.
pub struct SelectedRead {
    /// Original existing receipt JCS, never a request-provided receipt.
    pub receipt_jcs: Vec<u8>,
    /// Server-owned existing request template from the admitted companion/verifier setup.
    pub measurements: CustodyOffNucPreReadRequestV1,
}

struct Expected {
    receipt: CustodyIntegrityReceiptV1,
    measurements: CustodyOffNucPreReadRequestV1,
}

/// Real continuation plus fixed finite receipt/measurement selection and local replay memory.
/// Construction requires an actually loaded reader, not a provider or admission boolean.
pub struct ExactObjectServer<'a, R> {
    reader: ReaderContinuation<'a, R>,
    policy: RequestPolicy,
}
struct RequestPolicy {
    authority: CustodyEd25519AuthorityV1,
    host: String,
    expected: BTreeMap<String, Expected>,
    attempted: Vec<(String, String)>,
    last_time: DateTime<Utc>,
}
impl<'a, R: ServiceCommandRunner> ExactObjectServer<'a, R> {
    /// Bind fixed service installation inputs to the reader's selected binding/seal.
    /// Authority bytes are the exact selected existing authority record committed by binding.
    /// This does not measure TLS/backend provenance or mint companion authority.
    ///
    /// # Errors
    /// Denies mismatched authority, receipt closure, metadata or stale reader selection.
    pub fn new(
        reader: ReaderContinuation<'a, R>,
        host: String,
        authority_jcs: &[u8],
        selected: Vec<SelectedRead>,
    ) -> Result<Self, ReaderError> {
        let now = clock_now();
        reader.recheck(&now)?;
        if selected
            .iter()
            .any(|s| s.measurements.s3_endpoint_authority != reader.selection.backend_endpoint)
        {
            return Err(ReaderError::Binding);
        }
        let policy = RequestPolicy::new(
            &reader.selection.binding,
            &reader.seal,
            host,
            authority_jcs,
            selected,
            &now,
        )?;
        Ok(Self { reader, policy })
    }

    /// Dispatch one already authenticated connection's bounded complete HTTP request.
    /// The TLS owner must separately bind the actual peer before calling this method.
    /// No public route is installed by this library method.
    ///
    /// # Errors
    /// Denies signed binding/replay/ledger drift before GET, or any late verification failure.
    pub fn read_http(
        &mut self,
        raw: &[u8],
        limits: CustodyReadLimits,
    ) -> Result<Vec<u8>, ReaderError> {
        let deadline = limits.start().map_err(|_| ReaderError::Read)?;
        self.read_http_at(raw, limits, deadline)
    }
    fn read_http_at(
        &mut self,
        raw: &[u8],
        limits: CustodyReadLimits,
        deadline: dasobjectstore_object_service::custody::CustodyReadDeadline,
    ) -> Result<Vec<u8>, ReaderError> {
        deadline.remaining().map_err(|_| ReaderError::Read)?;
        let now = clock_now();
        self.reader.recheck(&now)?;
        let request = http::decode_request(raw, &self.policy.host, &self.policy.authority, &now)?;
        let request_digest = raw_sha256(request.raw_jcs);
        let body = &request.record.body;
        self.policy.claim(body, &now)?;
        let expected = self
            .policy
            .expected
            .get(&body.receipt_jcs_sha256)
            .ok_or(ReaderError::Binding)?;
        let validity_remaining = time(&body.expires_at_utc)?
            .signed_duration_since(DateTime::<Utc>::from(std::time::SystemTime::now()))
            .to_std()
            .map_err(|_| ReaderError::Binding)?;
        if validity_remaining.is_zero() {
            return Err(ReaderError::Binding);
        }
        let value = self.reader.read_inner_at(
            &expected.receipt,
            CustodyReadLimits {
                maximum_bytes: limits.maximum_bytes,
                timeout: deadline
                    .remaining()
                    .map_err(|_| ReaderError::Read)?
                    .min(validity_remaining),
            },
            Some(&body.lock_ledger_sha256),
            deadline
                .capped(validity_remaining)
                .map_err(|_| ReaderError::Read)?,
        )?;
        if value.ledger_head_sha256 != body.ledger_head_sha256
            || raw_sha256(&serde_jcs::to_vec(&value.receipt).map_err(|_| ReaderError::Format)?)
                != body.receipt_jcs_sha256
        {
            return Err(ReaderError::Binding);
        }
        let metadata = ReaderResultV1 {
            schema: "das.custody.reader_result.v1".into(),
            request_sha256: request_digest,
            receipt_jcs_sha256: body.receipt_jcs_sha256.clone(),
            configuration_sha256: value.configuration_sha256,
            ledger_head_sha256: value.ledger_head_sha256,
            content_length: value.bytes.len() as u64,
        };
        let response = http::encode_result(
            &metadata,
            &value.bytes,
            usize::try_from(limits.maximum_bytes).map_err(|_| ReaderError::Format)?,
        )?;
        self.reader.recheck(&clock_now())?;
        if time(&clock_now())? >= time(&body.expires_at_utc)? {
            return Err(ReaderError::Binding);
        }
        deadline.remaining().map_err(|_| ReaderError::Read)?;
        Ok(response)
    }
}
impl RequestPolicy {
    fn new(
        binding: &ReaderBindingV1,
        seal: &ReaderSealV1,
        host: String,
        authority_jcs: &[u8],
        selected: Vec<SelectedRead>,
        now: &str,
    ) -> Result<Self, ReaderError> {
        if authority_jcs.len() > 16384
            || raw_sha256(authority_jcs) != binding.verifier_authority_sha256
            || host.is_empty()
            || host.len() > 256
            || !host.bytes().all(|b| b.is_ascii_graphic())
            || selected.is_empty()
            || selected.len() > 4096
        {
            return Err(ReaderError::Binding);
        }
        let authority: CustodyEd25519AuthorityV1 =
            serde_json::from_slice(authority_jcs).map_err(|_| ReaderError::Format)?;
        if serde_jcs::to_vec(&authority).map_err(|_| ReaderError::Format)? != authority_jcs {
            return Err(ReaderError::Format);
        }
        authority.validate().map_err(|_| ReaderError::Binding)?;
        let mut expected = BTreeMap::new();
        for selection in selected {
            if selection.receipt_jcs.len() > 1_048_576 {
                return Err(ReaderError::Format);
            }
            let receipt: CustodyIntegrityReceiptV1 =
                serde_json::from_slice(&selection.receipt_jcs).map_err(|_| ReaderError::Format)?;
            let digest = raw_sha256(&selection.receipt_jcs);
            let m = &selection.measurements;
            if serde_jcs::to_vec(&receipt).map_err(|_| ReaderError::Format)?
                != selection.receipt_jcs
                || digest != m.receipt_jcs_sha256
                || !seal.receipt_jcs_sha256.contains(&digest)
                || receipt.configuration_sha256 != binding.configuration_sha256
                || receipt.store_id.as_str() != binding.store_id
                || receipt.bucket_name != binding.bucket_name
                || receipt.reader_identity != binding.reader_identity
                || receipt.target_id != m.target_id
                || receipt.object_lock_policy_sha256 != binding.object_lock_policy_sha256
                || m.machine_identity_sha256 != binding.host_identity_sha256
                || m.endpoint_authority_sha256 != binding.endpoint_authority_sha256
                || m.tls_peer_sha256 != binding.tls_peer_sha256
                || m.reader_identity != binding.reader_identity
                || m.store_id != binding.store_id
                || m.bucket_name != binding.bucket_name
                || m.stores_namespace_sha256 != binding.stores_namespace_sha256
                || m.object_lock_policy_sha256 != binding.object_lock_policy_sha256
                || m.inventory_sha256 != binding.inventory_sha256
                || m.ledger_head_sha256 != seal.ledger_head_sha256
            {
                return Err(ReaderError::Binding);
            }
            if expected
                .insert(
                    digest,
                    Expected {
                        receipt,
                        measurements: selection.measurements,
                    },
                )
                .is_some()
            {
                return Err(ReaderError::Binding);
            }
        }
        if expected.len() != seal.receipt_jcs_sha256.len() {
            return Err(ReaderError::Binding);
        }
        Ok(Self {
            authority,
            host,
            expected,
            attempted: Vec::new(),
            last_time: time(now)?,
        })
    }
    fn claim(
        &mut self,
        request: &CustodyOffNucPreReadRequestV1,
        now: &str,
    ) -> Result<(), ReaderError> {
        let now = time(now)?;
        if now < self.last_time || time(&request.expires_at_utc)? <= now {
            return Err(ReaderError::Binding);
        }
        self.last_time = now;
        let expected = &self
            .expected
            .get(&request.receipt_jcs_sha256)
            .ok_or(ReaderError::Binding)?
            .measurements;
        let mut actual = request.clone();
        actual.request_id.clone_from(&expected.request_id);
        actual.nonce.clone_from(&expected.nonce);
        actual.sequence = expected.sequence;
        actual
            .previous_request_sha256
            .clone_from(&expected.previous_request_sha256);
        actual.issued_at_utc.clone_from(&expected.issued_at_utc);
        actual.expires_at_utc.clone_from(&expected.expires_at_utc);
        if actual != *expected {
            return Err(ReaderError::Binding);
        }
        if self.attempted.len() >= 4096
            || self
                .attempted
                .iter()
                .any(|(id, nonce)| id == &request.request_id || nonce == &request.nonce)
        {
            return Err(ReaderError::Conflict);
        }
        self.attempted
            .push((request.request_id.clone(), request.nonce.clone()));
        Ok(())
    }
}
fn time(raw: &str) -> Result<DateTime<Utc>, ReaderError> {
    DateTime::parse_from_rfc3339(raw)
        .map(|t| t.with_timezone(&Utc))
        .map_err(|_| ReaderError::Format)
}

#[cfg(test)]
mod tests;
