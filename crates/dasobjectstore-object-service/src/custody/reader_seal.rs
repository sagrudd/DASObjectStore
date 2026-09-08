//! Existing-only complete receipt inventory check for the protected reader manager.
use super::*;
use crate::custody_reader::{raw_sha256, ReaderSealV1};
use std::collections::BTreeMap;
#[cfg(test)]
#[path = "reader_seal_review_tests.rs"]
mod review_tests;

/// Check the complete selected seal against one real, quiesced ledger snapshot.
/// Selection is trusted composition data, not independently admitted authority.
/// This neither fetches objects nor substitutes for later complete readback.
///
/// # Errors
/// Denies schema/chain/path/receipt/inventory drift with no ledger mutation.
#[cfg(unix)]
pub fn verify_reader_seal_existing(
    path: &Path,
    seal: &ReaderSealV1,
    reader_identity: &str,
    inventory: &[(String, u64)],
    deadline: CustodyReadDeadline,
) -> Result<(), CustodyReadError> {
    use super::bounded_read::{configure_read_connection, schema, Guard};
    seal.encode().map_err(|_| CustodyReadError::Input)?;
    crate::validate_custody_inventory(inventory.iter().map(|(h, n)| (h.as_str(), *n)))
        .map_err(|_| CustodyReadError::Input)?;
    deadline.remaining()?;
    let guard = Guard::capture(path)?;
    let mut connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| CustodyReadError::Ledger)?;
    configure_read_connection(&connection, deadline)?;
    let tx = connection
        .transaction()
        .map_err(|_| CustodyReadError::Ledger)?;
    let mut template = Connection::open_in_memory().map_err(|_| CustodyReadError::Ledger)?;
    initialise_schema(&mut template).map_err(|_| CustodyReadError::Ledger)?;
    if schema(&tx)? != schema(&template)? {
        return Err(CustodyReadError::Ledger);
    }
    let config = read_sealed_configuration_from(&tx).map_err(|_| CustodyReadError::Ledger)?;
    verify_event_chain_checked(&tx, || {
        deadline
            .remaining()
            .map(|_| ())
            .map_err(|_| invalid("reader seal deadline"))
    })
    .map_err(|_| CustodyReadError::Ledger)?;
    validate_reader_identity(&config.profile, reader_identity)
        .map_err(|_| CustodyReadError::Ledger)?;
    let head: String = tx
        .query_row(
            "SELECT event_sha256 FROM custody_events ORDER BY sequence DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .map_err(|_| CustodyReadError::Ledger)?;
    if config.store_id.as_str() != seal.store_id
        || sealed_configuration_sha256(&config).map_err(|_| CustodyReadError::Ledger)?
            != seal.configuration_sha256
        || head != seal.ledger_head_sha256
    {
        return Err(CustodyReadError::Ledger);
    }
    let count: u64 = tx
        .query_row("SELECT count(*) FROM custody_readback_receipts", [], |r| {
            r.get(0)
        })
        .map_err(|_| CustodyReadError::Ledger)?;
    if count != inventory.len() as u64 {
        return Err(CustodyReadError::Ledger);
    }
    let oversized: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM custody_readback_receipts WHERE length(CAST(receipt_jcs AS BLOB)) > 1048576 OR length(CAST(receipt_sha256 AS BLOB)) != 64)",[],|r|r.get(0)).map_err(|_|CustodyReadError::Ledger)?;
    if oversized {
        return Err(CustodyReadError::Ledger);
    }
    let rows: Vec<(String, String)> = tx
        .prepare("SELECT receipt_jcs,receipt_sha256 FROM custody_readback_receipts")
        .map_err(|_| CustodyReadError::Ledger)?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|_| CustodyReadError::Ledger)?
        .collect::<Result<_, _>>()
        .map_err(|_| CustodyReadError::Ledger)?;
    if rows.len() != inventory.len() {
        return Err(CustodyReadError::Ledger);
    }
    let mut hashes = Vec::with_capacity(rows.len());
    let mut seen = BTreeMap::new();
    for (raw, digest) in rows {
        deadline.remaining()?;
        let receipt: CustodyIntegrityReceiptV1 =
            serde_json::from_str(&raw).map_err(|_| CustodyReadError::Ledger)?;
        if canonical_json(&receipt).map_err(|_| CustodyReadError::Ledger)? != raw
            || raw_sha256(raw.as_bytes()) != digest
        {
            return Err(CustodyReadError::Ledger);
        }
        let stored = latest_object_version(&tx, &receipt.object_id)
            .map_err(|_| CustodyReadError::Ledger)?
            .ok_or(CustodyReadError::Ledger)?;
        if receipt.schema != CUSTODY_RECEIPT_SCHEMA_V1
            || receipt.assurance_class
                != CUSTODY_ASSURANCE_CLASS_LOCAL_TRUSTED_ADMINISTRATOR_OVERLAY
            || receipt.store_id != config.store_id
            || receipt.bucket_name != config.bucket_name
            || receipt.target_id != config.profile.target_id
            || receipt.version != stored.version
            || receipt.reader_identity != reader_identity
            || seen
                .insert(receipt.content_sha256.clone(), receipt.content_length)
                .is_some()
        {
            return Err(CustodyReadError::Ledger);
        }
        verify_receipt_fields(
            &config,
            &stored,
            &receipt,
            &CustodyReadbackObservationV1 {
                reader_identity: reader_identity.into(),
                observed_at_utc: receipt.observed_at_utc.clone(),
                content_sha256: receipt.content_sha256.clone(),
                content_length: receipt.content_length,
            },
        )
        .map_err(|_| CustodyReadError::Ledger)?;
        hashes.push(digest);
    }
    hashes.sort();
    if hashes != seal.receipt_jcs_sha256
        || seen != inventory.iter().cloned().collect()
        || Guard::capture(path)? != guard
    {
        return Err(CustodyReadError::Ledger);
    }
    tx.commit().map_err(|_| CustodyReadError::Ledger)?;
    deadline.remaining()?;
    Ok(())
}
