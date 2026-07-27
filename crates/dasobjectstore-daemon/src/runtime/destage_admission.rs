//! Shared physical-capacity admission for every SSD-first publication path.

use dasobjectstore_core::ids::ObjectId;
use dasobjectstore_metadata::{
    measure_ssd_capacity, DiskCapacityClaimAllocation, DiskCapacityClaimKind,
    DiskCapacityClaimRequest,
};
use std::path::Path;

pub(crate) fn build_destage_capacity_claim(
    live_sqlite_path: &Path,
    hdd_root: &Path,
    object_id: &ObjectId,
    destage_job_id: &str,
    required_copies: u8,
    size_bytes: u64,
    content_hash: &str,
    created_at_utc: &str,
) -> Result<DiskCapacityClaimRequest, String> {
    let roots = super::select_managed_hdd_roots_with_capacity(
        live_sqlite_path,
        hdd_root,
        required_copies,
        size_bytes,
        None,
    )
    .map_err(|error| error.to_string())?;
    let allocations = roots
        .iter()
        .map(|root| {
            let measured =
                measure_ssd_capacity(&root.root_path).map_err(|error| error.to_string())?;
            Ok(DiskCapacityClaimAllocation {
                disk_id: root.disk_id.clone(),
                measured_available_bytes: measured.available_bytes,
                requested_bytes: size_bytes,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(DiskCapacityClaimRequest {
        live_sqlite_path: live_sqlite_path.to_path_buf(),
        kind: DiskCapacityClaimKind::Destage,
        owner_id: object_id.as_str().to_string(),
        request_id: format!("destage:{destage_job_id}"),
        request_digest: format!(
            "{}:{size_bytes}:{required_copies}:{content_hash}",
            object_id.as_str()
        ),
        lease_owner: None,
        lease_expires_at_utc: None,
        created_at_utc: created_at_utc.to_string(),
        allocations,
    })
}
