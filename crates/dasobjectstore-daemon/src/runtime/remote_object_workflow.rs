use crate::api::{
    RemoteObjectChecksum, RemoteObjectGroupMemberRole, RemoteObjectGroupRelationship,
    RemoteObjectGroupState, RemoteObjectGroupStatusRequest, RemoteObjectGroupStatusResponse,
    RemoteObjectPlacementSummary, RemoteObjectReadiness, RemoteObjectSnapshotEntry,
    RemoteObjectSnapshotRequest, RemoteObjectSnapshotResponse, RemoteProviderVisibility,
    REMOTE_OBJECT_WORKFLOW_SCHEMA_VERSION,
};
use dasobjectstore_metadata::{
    read_remote_object_inventory_page, RemoteObjectInventoryError, RemoteObjectInventoryRecord,
};
use std::path::Path;

pub fn remote_object_snapshot(
    live_sqlite_path: impl AsRef<Path>,
    request: &RemoteObjectSnapshotRequest,
) -> Result<RemoteObjectSnapshotResponse, RemoteObjectInventoryError> {
    let decoded = request
        .cursor
        .as_deref()
        .and_then(|cursor| crate::api::decode_snapshot_cursor(cursor).ok());
    let (high_water, after) = match decoded {
        Some((high_water, key, version)) => (Some(high_water), Some((key, version))),
        None => (None, None),
    };
    let page = read_remote_object_inventory_page(
        live_sqlite_path,
        &request.store_id,
        &request.prefix,
        high_water,
        after
            .as_ref()
            .map(|(key, version)| (key.as_str(), *version)),
        request.limit,
    )?;
    let next_cursor = page
        .next_key
        .clone()
        .zip(page.next_version)
        .map(|(key, version)| {
            crate::api::encode_snapshot_cursor(page.snapshot_high_water, key, version)
        });
    Ok(RemoteObjectSnapshotResponse {
        schema_version: REMOTE_OBJECT_WORKFLOW_SCHEMA_VERSION.to_string(),
        store_id: request.store_id.clone(),
        prefix: request.prefix.clone(),
        snapshot_id: crate::api::snapshot_id(page.snapshot_high_water),
        objects: page.objects.into_iter().map(snapshot_entry).collect(),
        total_objects: page.total_objects,
        complete: next_cursor.is_none(),
        next_cursor,
    })
}

pub fn remote_object_group_status(
    live_sqlite_path: impl AsRef<Path>,
    request: &RemoteObjectGroupStatusRequest,
) -> Result<RemoteObjectGroupStatusResponse, RemoteObjectInventoryError> {
    let page = read_remote_object_inventory_page(
        live_sqlite_path,
        &request.store_id,
        &request.key,
        None,
        None,
        32,
    )?;
    let payload_key = request.key.as_str();
    let manifest_key = format!("{payload_key}.manifest.json");
    let checksum_key = format!("{payload_key}.sha256");
    let mut payload = None;
    let mut manifest = None;
    let mut checksum_sidecar = None;
    for object in page.objects {
        let target = match object.object_key.as_str() {
            key if key == payload_key => &mut payload,
            key if key == manifest_key => &mut manifest,
            key if key == checksum_key => &mut checksum_sidecar,
            _ => continue,
        };
        // A later binding version is authoritative for the same stable key.
        if target
            .as_ref()
            .is_none_or(|current: &RemoteObjectSnapshotEntry| {
                current.version < object.object_version
            })
        {
            *target = Some(snapshot_entry(object));
        }
    }
    let member_count = usize::from(payload.is_some())
        + usize::from(manifest.is_some())
        + usize::from(checksum_sidecar.is_some());
    let verification_failed = [&payload, &manifest, &checksum_sidecar]
        .into_iter()
        .flatten()
        .any(|object| {
            let lifecycle = object.lifecycle_state.to_ascii_lowercase();
            lifecycle.contains("fail") || lifecycle.contains("corrupt")
        });
    let all_durable = [&payload, &manifest, &checksum_sidecar]
        .into_iter()
        .flatten()
        .all(|object| object.placement.durable);
    let any_ssd = [&payload, &manifest, &checksum_sidecar]
        .into_iter()
        .flatten()
        .any(|object| object.placement.active_ssd_copy);
    let state = if verification_failed {
        RemoteObjectGroupState::VerificationFailed
    } else {
        match member_count {
            0 => RemoteObjectGroupState::Absent,
            1 | 2 => RemoteObjectGroupState::RepairRequired,
            3 if all_durable => RemoteObjectGroupState::HddSettled,
            3 if any_ssd => RemoteObjectGroupState::SsdAcknowledged,
            3 => RemoteObjectGroupState::ReconciliationQueued,
            _ => unreachable!("group has exactly three possible members"),
        }
    };
    let catalogue_complete = member_count == 3;
    let durable = catalogue_complete
        && [&payload, &manifest, &checksum_sidecar]
            .into_iter()
            .flatten()
            .all(|object| object.placement.durable);
    Ok(RemoteObjectGroupStatusResponse {
        schema_version: REMOTE_OBJECT_WORKFLOW_SCHEMA_VERSION.to_string(),
        store_id: request.store_id.clone(),
        key: request.key.clone(),
        state,
        payload,
        manifest,
        checksum_sidecar,
        catalogue_complete,
        durable,
    })
}

fn snapshot_entry(object: RemoteObjectInventoryRecord) -> RemoteObjectSnapshotEntry {
    let durable = object.verified_hdd_copy_count > 0;
    let readiness = if durable {
        RemoteObjectReadiness::Available
    } else if object.active_ssd_copy {
        RemoteObjectReadiness::Settling
    } else {
        RemoteObjectReadiness::Unavailable
    };
    let (payload_key, member_role) = group_relationship(&object.object_key);
    RemoteObjectSnapshotEntry {
        key: object.object_key,
        version: object.object_version,
        object_id: object.object_id,
        size_bytes: object.size_bytes,
        checksum: RemoteObjectChecksum {
            algorithm: object.content_hash_algorithm,
            value: object.content_hash,
        },
        provider_visibility: RemoteProviderVisibility::Unknown,
        group: RemoteObjectGroupRelationship {
            payload_key,
            member_role,
        },
        lifecycle_state: object.lifecycle_state,
        readiness,
        placement: RemoteObjectPlacementSummary {
            active_ssd_copy: object.active_ssd_copy,
            hdd_copy_count: object.hdd_copy_count,
            verified_hdd_copy_count: object.verified_hdd_copy_count,
            durable,
        },
        updated_at_utc: object.updated_at_utc,
    }
}

fn group_relationship(key: &str) -> (String, RemoteObjectGroupMemberRole) {
    if let Some(payload_key) = key.strip_suffix(".manifest.json") {
        (
            payload_key.to_string(),
            RemoteObjectGroupMemberRole::Manifest,
        )
    } else if let Some(payload_key) = key.strip_suffix(".sha256") {
        (
            payload_key.to_string(),
            RemoteObjectGroupMemberRole::ChecksumSidecar,
        )
    } else {
        (key.to_string(), RemoteObjectGroupMemberRole::Payload)
    }
}
