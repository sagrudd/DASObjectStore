//! One synchronous finite call; never a reusable credential/session capability.
use super::*;
use dasobjectstore_object_service::{
    validate_custody_input, validate_custody_inventory, CustodyIntegrityReceiptV1,
    CustodyObjectInputV1,
};
use sha2::{Digest, Sha256};

/// Explicit service-composition data, not an admitted companion or transport policy.
pub struct CustodyFiniteInventory {
    /// Exact existing catalogue store.
    pub store_id: StoreId,
    /// Trusted composition's finite ordered inventory.
    pub objects: Vec<CustodyInventoryObject>,
}
/// One expected object, using the existing planner's digest/length semantics.
pub struct CustodyInventoryObject {
    /// Lowercase raw SHA-256 hex.
    pub content_sha256: String,
    /// Positive exact byte count.
    pub size_bytes: u64,
}
/// Fixed, non-secret failing boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CustodyBatchPhase {
    /// No handoff consumed.
    Prevalidation,
    /// Writer resolution attempted.
    WriterHandoff,
    /// Writer consumed; reader resolution attempted.
    ReaderHandoff,
    /// Both handoffs consumed; adapter construction denied.
    AdapterConstruction,
    /// An object may have been written before failure.
    ObjectRetention,
}
/// Partial evidence only; never a successful batch or retry capability.
#[derive(Debug)]
pub struct CustodyBatchError {
    /// Fixed boundary, with no backend diagnostics or secret values.
    pub phase: CustodyBatchPhase,
    /// Index in the expected inventory, only for object retention failure.
    pub failed_index: Option<usize>,
    /// Successfully verified prefix; failed object may additionally exist.
    pub completed: Vec<CustodyIntegrityReceiptV1>,
}
impl std::fmt::Display for CustodyBatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "custody finite batch denied: {:?}", self.phase)
    }
}
impl std::error::Error for CustodyBatchError {}
fn denied(phase: CustodyBatchPhase) -> CustodyBatchError {
    CustodyBatchError {
        phase,
        failed_index: None,
        completed: Vec::new(),
    }
}

impl<R: ServiceCommandRunner> CustodyServiceController<'_, R> {
    /// Retain one explicitly selected finite inventory using each sealed handoff once.
    /// The expected inventory must come from service composition, never caller policy.
    /// This grants no execution admission, lifecycle or reader continuation authority.
    ///
    /// # Errors
    /// Prevalidation has no effects. Any later failure preserves consumed authority
    /// and retained objects; it is not retryable and performs no compensation.
    pub fn retain_custody_inventory(
        &self,
        expected: &CustodyFiniteInventory,
        inputs: Vec<CustodyObjectInputV1>,
    ) -> Result<Vec<CustodyIntegrityReceiptV1>, CustodyBatchError> {
        let pre = || denied(CustodyBatchPhase::Prevalidation);
        validate_custody_inventory(
            expected
                .objects
                .iter()
                .map(|o| (o.content_sha256.as_str(), o.size_bytes)),
        )
        .map_err(|_| pre())?;
        if inputs.len() != expected.objects.len() {
            return Err(pre());
        }
        let config = self.custody_plane_config().map_err(|_| pre())?;
        let entry = read_custody_catalog(self.custody_catalog_path())
            .map_err(|_| pre())?
            .into_iter()
            .find(|e| e.definition.store_id == expected.store_id)
            .ok_or_else(pre)?;
        let inspection = inspect_custody_ledger(&entry.ledger_path).map_err(|_| pre())?;
        if inspection.store_id != entry.definition.store_id
            || inspection.bucket_name != entry.definition.bucket_name
            || inspection.configuration_sha256 != entry.ledger_configuration_sha256
        {
            return Err(pre());
        }
        let mut supplied = BTreeMap::new();
        for input in inputs {
            validate_custody_input(&input).map_err(|_| pre())?;
            if inspection.retention_until_utc <= input.retained_at_utc {
                return Err(pre());
            }
            let digest = format!("{:x}", Sha256::digest(&input.bytes));
            if supplied.insert(digest, input).is_some() {
                return Err(pre());
            }
        }
        let mut ordered = Vec::with_capacity(expected.objects.len());
        for object in &expected.objects {
            let input = supplied.remove(&object.content_sha256).ok_or_else(pre)?;
            if u64::try_from(input.bytes.len()).map_err(|_| pre())? != object.size_bytes {
                return Err(pre());
            }
            ordered.push(input);
        }
        if !supplied.is_empty() {
            return Err(pre());
        }
        let scratch = entry
            .ledger_path
            .parent()
            .ok_or_else(pre)?
            .join(".custody-scratch");
        let resolver = self.bindings.credentials.as_ref().ok_or_else(pre)?;
        let profile = &entry.definition.profile;
        let writer = resolver
            .consume_one_use(
                CustodyRuntimeCredentialRole::Writer,
                &profile.writer_credential_reference,
                expected.store_id.as_str(),
                &entry.configuration_sha256,
            )
            .map_err(|_| denied(CustodyBatchPhase::WriterHandoff))?;
        let reader = resolver
            .consume_one_use(
                CustodyRuntimeCredentialRole::Reader,
                &profile.reader_credential_reference,
                expected.store_id.as_str(),
                &entry.configuration_sha256,
            )
            .map_err(|_| denied(CustodyBatchPhase::ReaderHandoff))?;
        let (writer_id, writer_env) = writer.into_parts();
        let (reader_id, reader_env) = reader.into_parts();
        if writer_id != profile.writer_identity
            || reader_id != profile.reader_identity
            || writer_id == reader_id
        {
            return Err(denied(CustodyBatchPhase::AdapterConstruction));
        }
        let mut writer = GarageCustodyS3Writer::new_with_object_lock(
            self.runner,
            &config.endpoint,
            &entry.definition.bucket_name,
            writer_id,
            writer_env,
            &scratch,
            inspection.object_lock_policy,
            inspection.retention_until_utc,
        )
        .map_err(|_| denied(CustodyBatchPhase::AdapterConstruction))?;
        let reader = GarageCustodyS3Reader::new(
            self.runner,
            &config.endpoint,
            &entry.definition.bucket_name,
            reader_id,
            reader_env,
            &scratch,
        );
        let mut completed = Vec::with_capacity(ordered.len());
        for (index, input) in ordered.into_iter().enumerate() {
            match retain_garage_custody_object_with_readback(
                &entry.ledger_path,
                input,
                &mut writer,
                &reader,
            ) {
                Ok(receipt) => completed.push(receipt),
                Err(_) => {
                    return Err(CustodyBatchError {
                        phase: CustodyBatchPhase::ObjectRetention,
                        failed_index: Some(index),
                        completed,
                    })
                }
            }
        }
        Ok(completed)
    }
}
