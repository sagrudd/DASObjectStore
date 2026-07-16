use super::*;

#[cfg(target_arch = "wasm32")]
pub(super) fn activity_task_kind_label(kind: &str) -> String {
    match kind {
        "ingest" => "Ingest",
        "destage" => "Destage",
        "system_administration" => "Administrator",
        "enclosure_preparation" => "Enclosure",
        "object_store_creation" => "ObjectStore",
        "sub_object_creation" => "SubObject",
        "repair" => "Repair",
        "health_check" => "Health",
        "disk_drain" => "Disk drain",
        "disk_replace" => "Disk replace",
        "endpoint_validation" => "Endpoint",
        other => other,
    }
    .to_string()
}

#[cfg(target_arch = "wasm32")]
pub(super) fn activity_task_state_label(state: &str) -> String {
    match state {
        "queued" => "Queued",
        "running" => "Running",
        "waiting" => "Waiting",
        "complete" => "Complete",
        "failed" => "Failed",
        "cancelled" => "Cancelled",
        other => other,
    }
    .to_string()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnclosureCardSummary {
    pub id: String,
    pub label: String,
    pub name: String,
    pub health: String,
    pub drives: String,
    pub capacity: String,
    pub mount_path: String,
    pub warning_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnclosurePrepareCandidate {
    pub enclosure_id: String,
    pub display_name: String,
    pub ssd_devices: Vec<EnclosurePrepareDevice>,
    pub hdd_devices: Vec<EnclosurePrepareDevice>,
}

impl EnclosurePrepareCandidate {
    pub fn ready(&self) -> bool {
        !self.ssd_devices.is_empty() && !self.hdd_devices.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnclosurePrepareDevice {
    pub disk_id: String,
    pub device_path: String,
    pub label: String,
}

pub fn enclosure_prepare_candidate(
    view: &EnclosuresPageResponse,
    active_id: &str,
) -> Option<EnclosurePrepareCandidate> {
    let enclosure = view
        .enclosures
        .iter()
        .find(|enclosure| enclosure.enclosure_id == active_id)
        .or_else(|| view.enclosures.first())?;
    let detail = view
        .details
        .as_ref()
        .filter(|detail| detail.enclosure_id == enclosure.enclosure_id)?;
    let mut ssd_devices = Vec::new();
    let mut hdd_devices = Vec::new();

    for slot in &detail.slots {
        let Some(device) = prepare_device_from_slot(slot) else {
            continue;
        };
        if slot_is_ssd(slot) {
            ssd_devices.push(device);
        } else if slot_is_hdd(slot) {
            hdd_devices.push(device);
        }
    }

    Some(EnclosurePrepareCandidate {
        enclosure_id: enclosure.enclosure_id.clone(),
        display_name: enclosure.display_name.clone(),
        ssd_devices,
        hdd_devices,
    })
}

pub(super) fn prepare_device_from_slot(
    slot: &EnclosureDriveSlotResponse,
) -> Option<EnclosurePrepareDevice> {
    let device_path = slot
        .device_path
        .as_deref()
        .filter(|path| !path.trim().is_empty())?;
    let role = slot.role.as_deref().unwrap_or("unassigned");
    Some(EnclosurePrepareDevice {
        disk_id: slot.drive_id.clone(),
        device_path: device_path.to_string(),
        label: format!(
            "{} · {} · {} TiB · {}",
            slot.drive_id, role, slot.size_tib, device_path
        ),
    })
}

pub(super) fn slot_is_ssd(slot: &EnclosureDriveSlotResponse) -> bool {
    slot.role
        .as_deref()
        .is_some_and(|role| role.eq_ignore_ascii_case("ssd"))
        || slot.slot_number == 0
}

pub(super) fn slot_is_hdd(slot: &EnclosureDriveSlotResponse) -> bool {
    slot.role
        .as_deref()
        .is_some_and(|role| role.to_ascii_lowercase().starts_with("hdd"))
}

pub fn enclosure_card_summaries(view: &EnclosuresPageResponse) -> Vec<EnclosureCardSummary> {
    view.enclosures
        .iter()
        .map(|enclosure| {
            let label = format!(
                "{} / {} / {}",
                enclosure.connection.bus,
                enclosure.connection.protocol,
                enclosure.connection.link_speed
            );
            let drives = format!(
                "{} mounted of {} drive(s); {} healthy; {} watch; {} suspect; {} failed",
                enclosure.drive_count.mounted,
                enclosure.drive_count.total,
                enclosure.drive_count.healthy,
                enclosure.drive_count.watch,
                enclosure.drive_count.suspect,
                enclosure.drive_count.failed
            );
            let capacity = format!(
                "{} TiB free of {} TiB",
                enclosure.capacity.free_tib, enclosure.capacity.total_tib
            );

            EnclosureCardSummary {
                id: enclosure.enclosure_id.clone(),
                label,
                name: enclosure.display_name.clone(),
                health: enclosure.health.clone(),
                drives,
                capacity,
                mount_path: enclosure.mount_path.clone(),
                warning_count: enclosure.warnings.len(),
            }
        })
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectStoreCardSummary {
    pub id: String,
    pub label: String,
    pub name: String,
    pub health: String,
    pub object_type: String,
    pub access: String,
    pub policy: String,
    pub capacity: String,
    pub capacity_status: String,
    pub objects: String,
    pub writer_group: String,
    pub endpoint: String,
    pub upload_allowed: bool,
    pub warning_count: usize,
    pub last_ingested: String,
    pub writer_policy: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectBrowserFolderSummary {
    pub name: String,
    pub prefix: String,
    pub objects: String,
    pub size: String,
    pub readiness: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectBrowserFileSummary {
    pub object_id: String,
    pub name: String,
    pub path: String,
    pub object_type: String,
    pub size: String,
    pub modified: String,
    pub readiness: String,
    pub lifecycle: String,
    pub copies: String,
    pub placement_summary: String,
    pub placements: Vec<ObjectBrowserPlacementResponse>,
    pub download_source: Option<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteUploadSelectedFile {
    pub display_path: String,
    pub size_bytes: u64,
}

#[cfg(any(target_arch = "wasm32", test))]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteUploadSelectionSummary {
    pub file_count: usize,
    pub folder_count: usize,
    pub total_bytes: u64,
    pub largest_file: Option<RemoteUploadSelectedFile>,
    pub sample_paths: Vec<String>,
}

#[cfg(any(target_arch = "wasm32", test))]
impl RemoteUploadSelectionSummary {
    pub fn from_files(files: &[RemoteUploadSelectedFile]) -> Self {
        let folder_count = remote_upload_folder_count(files);
        let largest_file = files.iter().max_by_key(|file| file.size_bytes).cloned();
        let sample_paths = files
            .iter()
            .take(5)
            .map(|file| file.display_path.clone())
            .collect();
        Self {
            file_count: files.len(),
            folder_count,
            total_bytes: files.iter().map(|file| file.size_bytes).sum(),
            largest_file,
            sample_paths,
        }
    }

    pub fn total_size_label(&self) -> String {
        format_browser_bytes(self.total_bytes)
    }

    pub fn largest_file_label(&self) -> String {
        self.largest_file
            .as_ref()
            .map(|file| {
                format!(
                    "{} ({})",
                    file.display_path,
                    format_browser_bytes(file.size_bytes)
                )
            })
            .unwrap_or_else(|| "no file selected".to_string())
    }
}

#[cfg(any(target_arch = "wasm32", test))]
pub(super) fn remote_upload_folder_count(files: &[RemoteUploadSelectedFile]) -> usize {
    files
        .iter()
        .filter_map(|file| file.display_path.rsplit_once('/').map(|(folder, _)| folder))
        .filter(|folder| !folder.trim().is_empty())
        .collect::<std::collections::BTreeSet<_>>()
        .len()
}
