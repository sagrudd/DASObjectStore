use super::*;
use dasobjectstore_object_service::{
    StoreRegistryDeleteReport, SubObjectRegistryStoreDeleteReport,
};

pub(super) fn known_ssd_root(path: &Path) -> bool {
    fs::read_to_string(path.join(".dasobjectstore").join("device.env"))
        .map(|marker| marker.lines().any(|line| line == "role=ssd"))
        .unwrap_or(false)
}

pub(super) fn parse_disk_copy_roots(entries: &[String]) -> Result<Vec<DiskCopyRoot>, String> {
    entries
        .iter()
        .map(|entry| {
            let (disk_id, root_path) = entry
                .split_once('=')
                .ok_or_else(|| format!("disk root must use disk-id=/path syntax: {entry}"))?;
            let disk_id = dasobjectstore_core::ids::DiskId::new(disk_id)
                .map_err(|error| format!("invalid disk id {disk_id}: {error}"))?;
            if root_path.is_empty() {
                return Err(format!("disk root path must not be empty: {entry}"));
            }
            Ok(DiskCopyRoot::new(disk_id, root_path))
        })
        .collect()
}

pub(super) fn delete_store_definition_maybe(
    path: &Path,
    store_id: &StoreId,
    dry_run: bool,
) -> Result<StoreRegistryDeleteReport, ObjectServiceError> {
    if dry_run {
        let removed = read_store_registry(path)?
            .iter()
            .any(|definition| &definition.store_id == store_id);
        return Ok(StoreRegistryDeleteReport {
            registry_path: path.to_path_buf(),
            store_id: store_id.clone(),
            removed,
        });
    }

    delete_store_definition(path, store_id)
}

pub(super) fn delete_subobjects_for_store_maybe(
    path: &Path,
    store_id: &StoreId,
    dry_run: bool,
) -> Result<SubObjectRegistryStoreDeleteReport, ObjectServiceError> {
    if dry_run {
        let mut removed_names = read_subobject_registry(path)?
            .iter()
            .filter(|definition| &definition.store_id == store_id)
            .map(|definition| definition.name.clone())
            .collect::<Vec<_>>();
        removed_names.sort();
        return Ok(SubObjectRegistryStoreDeleteReport {
            registry_path: path.to_path_buf(),
            store_id: store_id.clone(),
            removed_count: removed_names.len(),
            removed_names,
        });
    }

    delete_subobjects_for_store(path, store_id)
}
