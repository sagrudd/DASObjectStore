//! Pure finite-inventory bounds shared by planning and service composition.
use crate::ObjectServiceError;
use std::collections::BTreeSet;

/// Validate the existing preknown-inventory contract without granting authority.
///
/// # Errors
/// Rejects empty/oversized inventories, malformed or duplicate hashes, zero sizes
/// and aggregate size overflow.
pub fn validate_custody_inventory<'a>(
    objects: impl IntoIterator<Item = (&'a str, u64)>,
) -> Result<(), ObjectServiceError> {
    let mut digests = BTreeSet::new();
    let mut total = 0u64;
    for (digest, size) in objects {
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || size == 0
            || !digests.insert(digest)
            || digests.len() > 4096
        {
            return Err(crate::custody::invalid("invalid finite custody inventory"));
        }
        total = total
            .checked_add(size)
            .ok_or_else(|| crate::custody::invalid("custody inventory size overflow"))?;
    }
    if digests.is_empty() {
        return Err(crate::custody::invalid("empty finite custody inventory"));
    }
    Ok(())
}
