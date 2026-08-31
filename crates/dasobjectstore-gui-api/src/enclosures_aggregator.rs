use crate::dashboard::{
    AddEnclosureAffordanceView, DasEnclosureCardView, DasEnclosureDetailView,
    DashboardHealthStateView, DashboardWarning, EnclosureConnectionView, EnclosureDriveSlotView,
    EnclosuresPageView, REDESIGN_DASHBOARD_SCHEMA_VERSION,
};
use crate::home_aggregator::{
    capacity_for_root, capacity_summary, discover_hdd_roots, drive_count_summary, env_path,
    now_utc_string, DEFAULT_HDD_ROOT, DEFAULT_SSD_ROOT,
};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const DEFAULT_MANAGED_STORAGE_MANIFEST: &str = "/etc/dasobjectstore/managed-storage.v1.json";

#[derive(Clone, Debug)]
struct EnclosuresAggregatorConfig {
    ssd_root: PathBuf,
    hdd_root: PathBuf,
    managed_storage_manifest: PathBuf,
    administrator: bool,
}

impl EnclosuresAggregatorConfig {
    fn from_env() -> Self {
        Self {
            ssd_root: env_path("DASOBJECTSTORE_SSD_ROOT", DEFAULT_SSD_ROOT),
            hdd_root: env_path("DASOBJECTSTORE_HDD_ROOT", DEFAULT_HDD_ROOT),
            managed_storage_manifest: env_path(
                "DASOBJECTSTORE_MANAGED_STORAGE_MANIFEST",
                DEFAULT_MANAGED_STORAGE_MANIFEST,
            ),
            administrator: env_flag("DASOBJECTSTORE_WEB_ADMINISTRATOR"),
        }
    }
}

/// Package-managed storage inventory. This is deliberately a read-only
/// fallback for a USB DAS that the kernel presents as separate disks rather
/// than as a `/sys/class/enclosure` device.
#[derive(Clone, Debug, Deserialize)]
struct ManagedStorageManifestV1 {
    schema_version: u8,
    ssd: ManagedStorageMemberV1,
    hdds: Vec<ManagedStorageMemberV1>,
}

#[derive(Clone, Debug, Deserialize)]
struct ManagedStorageMemberV1 {
    path: PathBuf,
    label: String,
    device: String,
    filesystem: String,
    role: String,
    uuid: String,
}

#[derive(Clone, Debug)]
struct ManagedStorageInventoryFallback {
    hdds: Vec<ManagedStorageMemberV1>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct DeviceMarker {
    role: Option<String>,
    device: Option<String>,
    filesystem: Option<String>,
}

pub(crate) fn live_enclosures_dashboard() -> EnclosuresPageView {
    build_enclosures_dashboard(EnclosuresAggregatorConfig::from_env())
}

pub(crate) fn live_enclosures_dashboard_for_administrator(
    administrator: bool,
) -> EnclosuresPageView {
    let mut config = EnclosuresAggregatorConfig::from_env();
    config.administrator = administrator;
    build_enclosures_dashboard(config)
}

fn build_enclosures_dashboard(config: EnclosuresAggregatorConfig) -> EnclosuresPageView {
    let generated_at_utc = now_utc_string();
    let mut warnings = Vec::new();
    let mut hdd_roots = discover_hdd_roots(&config.hdd_root, &mut warnings);
    hdd_roots.sort();

    // A verified package manifest is retained even if a USB enclosure is
    // temporarily unplugged or its mounts are unavailable.  Use it only when
    // no live marker-backed roots exist, so it cannot overwrite live evidence
    // or create/adopt any storage state.
    let managed_storage_fallback = hdd_roots.is_empty().then(|| {
        read_managed_storage_inventory_fallback(
            &config.managed_storage_manifest,
            &config.hdd_root,
            &mut warnings,
        )
    });
    let managed_storage_fallback = managed_storage_fallback.flatten();
    if let Some(fallback) = &managed_storage_fallback {
        warnings.retain(|warning| {
            !matches!(
                warning.code.as_str(),
                "hdd_root_missing" | "hdd_inventory_empty" | "hdd_capacity_partial"
            )
        });
        warnings.push(DashboardWarning::new(
            "managed_storage_inventory_fallback",
            format!(
                "{} configured HDD member(s) are shown from the package-managed storage inventory because no live marker-backed enclosure roots are available. This read-only fallback does not mount, adopt, migrate, normalize, or delete ObjectStore data.",
                fallback.hdds.len()
            ),
        ));
    }

    let hdd_capacities = managed_storage_fallback
        .as_ref()
        .map(|fallback| {
            fallback
                .hdds
                .iter()
                .filter(|member| manifest_member_is_marker_backed(member))
                .filter_map(|member| capacity_for_root(&member.path))
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| {
            hdd_roots
                .iter()
                .filter_map(|root| capacity_for_root(root))
                .collect::<Vec<_>>()
        });
    if managed_storage_fallback.is_none()
        && hdd_roots
            .iter()
            .any(|root| capacity_for_root(root).is_none())
    {
        warnings.push(DashboardWarning::new(
            "hdd_capacity_partial",
            "One or more managed HDD roots could not be measured for the enclosure view.",
        ));
    }

    let ssd_marker = marker_for_root(&config.ssd_root);
    if !config.ssd_root.exists() {
        warnings.push(DashboardWarning::new(
            "ssd_root_missing",
            format!(
                "Managed SSD root is not present at {}.",
                config.ssd_root.display()
            ),
        ));
    } else if ssd_marker.role.as_deref() != Some("ssd") {
        warnings.push(DashboardWarning::new(
            "ssd_marker_missing",
            format!(
                "Managed SSD root {} is missing role=ssd marker metadata.",
                config.ssd_root.display()
            ),
        ));
    }

    let hdd_markers = hdd_roots
        .iter()
        .map(|root| marker_for_root(root))
        .collect::<Vec<_>>();
    let managed_enclosure_known = !hdd_roots.is_empty() || managed_storage_fallback.is_some();
    // A managed QNAP filesystem pool is the supported enclosure projection
    // for hosts where the enclosure itself is presented as independent USB
    // disks (and therefore has no /sys/class/enclosure entry).  The marker
    // inventory is the authoritative hardware evidence in that mode.
    let supported_enclosure_detected = managed_enclosure_known;
    let daemon_ready = daemon_ready_for_affordance(&warnings);
    let add_enclosure = add_enclosure_affordance(
        config.administrator,
        supported_enclosure_detected,
        managed_enclosure_known,
        daemon_ready,
    );

    let mut enclosures = Vec::new();
    let mut details = None;
    let selected_enclosure_id = if let Some(fallback) = &managed_storage_fallback {
        let identity =
            enclosure_identity_for_roles(fallback.hdds.iter().map(|member| member.role.as_str()));
        let enclosure_id = identity.enclosure_id.to_string();
        let mounted_members = fallback
            .hdds
            .iter()
            .filter(|member| manifest_member_is_marker_backed(member))
            .count();
        let drive_count = fallback_drive_count(fallback.hdds.len(), mounted_members);
        enclosures.push(DasEnclosureCardView {
            enclosure_id: enclosure_id.clone(),
            display_name: format!("{} (managed-storage inventory)", identity.display_name),
            mount_path: config.hdd_root.display().to_string(),
            connection: EnclosureConnectionView {
                bus: "managed-storage".to_string(),
                protocol: "manifest fallback".to_string(),
                link_speed: "not live-probed".to_string(),
            },
            health: DashboardHealthStateView::Watch,
            drive_count,
            capacity: capacity_summary(&hdd_capacities),
            last_seen_at_utc: generated_at_utc.clone(),
            warnings: warnings.clone(),
        });
        details = Some(DasEnclosureDetailView {
            enclosure_id: enclosure_id.clone(),
            vendor: identity.vendor.to_string(),
            model: identity.model.to_string(),
            serial: "configured-managed-storage-inventory".to_string(),
            firmware: None,
            slots: fallback_enclosure_slots(&fallback.hdds),
        });
        Some(enclosure_id)
    } else if hdd_roots.is_empty() {
        None
    } else {
        let identity = enclosure_identity(&hdd_markers);
        let enclosure_id = identity.enclosure_id.to_string();
        let health = if warnings.is_empty() {
            DashboardHealthStateView::Healthy
        } else {
            DashboardHealthStateView::Watch
        };
        enclosures.push(DasEnclosureCardView {
            enclosure_id: enclosure_id.clone(),
            display_name: identity.display_name.to_string(),
            mount_path: config.hdd_root.display().to_string(),
            connection: EnclosureConnectionView {
                bus: identity.bus.to_string(),
                protocol: identity.protocol.to_string(),
                link_speed: identity.link_speed.to_string(),
            },
            health,
            drive_count: drive_count_summary(config.ssd_root.exists(), hdd_roots.len()),
            capacity: capacity_summary(&hdd_capacities),
            last_seen_at_utc: generated_at_utc.clone(),
            warnings: warnings.clone(),
        });
        details = Some(DasEnclosureDetailView {
            enclosure_id: enclosure_id.clone(),
            vendor: identity.vendor.to_string(),
            model: identity.model.to_string(),
            serial: identity.serial.to_string(),
            firmware: None,
            slots: enclosure_slots(&config.ssd_root, &hdd_roots, &ssd_marker, &hdd_markers),
        });
        Some(enclosure_id)
    };

    EnclosuresPageView {
        schema_version: REDESIGN_DASHBOARD_SCHEMA_VERSION.to_string(),
        generated_at_utc,
        add_enclosure,
        enclosures,
        selected_enclosure_id,
        details,
        warnings,
    }
}

fn env_flag(name: &str) -> bool {
    matches!(
        std::env::var(name).ok().as_deref(),
        Some("1" | "true" | "TRUE" | "yes" | "YES")
    )
}

fn daemon_ready_for_affordance(warnings: &[DashboardWarning]) -> bool {
    !warnings.iter().any(|warning| {
        matches!(
            warning.code.as_str(),
            "hdd_root_unreadable" | "hdd_capacity_partial"
        )
    })
}

fn add_enclosure_affordance(
    administrator: bool,
    supported_enclosure_detected: bool,
    managed_enclosure_known: bool,
    daemon_ready: bool,
) -> AddEnclosureAffordanceView {
    if managed_enclosure_known {
        return AddEnclosureAffordanceView {
            administrator,
            supported_enclosure_detected,
            ..AddEnclosureAffordanceView::blocked(
                "already_managed",
                administrator,
                daemon_ready,
                "A managed DAS enclosure is already known to DASObjectStore. Web preparation is available only for unprepared, supported enclosures.",
                "Use the CLI for any deliberate destructive enclosure re-preparation or removal workflow.",
            )
        };
    }

    if !administrator {
        return AddEnclosureAffordanceView {
            administrator,
            supported_enclosure_detected,
            daemon_ready,
            ..AddEnclosureAffordanceView::admin_required()
        };
    }
    if !daemon_ready {
        return AddEnclosureAffordanceView {
            administrator,
            supported_enclosure_detected,
            ..AddEnclosureAffordanceView::blocked(
                "daemon_unavailable",
                administrator,
                daemon_ready,
                "The daemon inventory path is not ready enough to prepare an enclosure.",
                "Resolve dashboard inventory warnings before preparing DAS hardware.",
            )
        };
    }
    if !supported_enclosure_detected {
        return AddEnclosureAffordanceView {
            administrator,
            supported_enclosure_detected,
            ..AddEnclosureAffordanceView::blocked(
                "unsupported_or_absent",
                administrator,
                daemon_ready,
                "No supported DAS enclosure is visible to the daemon inventory path.",
                "Attach a supported DAS enclosure and refresh the inventory.",
            )
        };
    }

    AddEnclosureAffordanceView::available()
}

#[derive(Clone, Copy, Debug)]
struct EnclosureIdentity {
    enclosure_id: &'static str,
    display_name: &'static str,
    vendor: &'static str,
    model: &'static str,
    serial: &'static str,
    bus: &'static str,
    protocol: &'static str,
    link_speed: &'static str,
}

fn enclosure_identity(hdd_markers: &[DeviceMarker]) -> EnclosureIdentity {
    enclosure_identity_for_roles(
        hdd_markers
            .iter()
            .filter_map(|marker| marker.role.as_deref()),
    )
}

fn enclosure_identity_for_roles<'a>(mut roles: impl Iterator<Item = &'a str>) -> EnclosureIdentity {
    let looks_qnap = roles.any(|role| {
        role.strip_prefix("hdd:")
            .is_some_and(|id| id.starts_with("qnap-"))
    });

    if looks_qnap {
        EnclosureIdentity {
            enclosure_id: "qnap-tl-d800c-managed",
            display_name: "QNAP TL-D800C",
            vendor: "QNAP",
            model: "TL-D800C",
            serial: "managed-qnap-das",
            bus: "usb",
            protocol: "uas/filesystem",
            link_speed: "host reported",
        }
    } else {
        EnclosureIdentity {
            enclosure_id: "managed-das-enclosure",
            display_name: "Managed DAS enclosure",
            vendor: "unknown",
            model: "managed-filesystem-roots",
            serial: "managed-das",
            bus: "managed-root",
            protocol: "filesystem",
            link_speed: "host reported",
        }
    }
}

fn read_managed_storage_inventory_fallback(
    path: &Path,
    hdd_root: &Path,
    warnings: &mut Vec<DashboardWarning>,
) -> Option<ManagedStorageInventoryFallback> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            warnings.push(DashboardWarning::new(
                "managed_storage_manifest_unreadable",
                format!(
                    "Managed-storage inventory {} could not be read: {error}.",
                    path.display()
                ),
            ));
            return None;
        }
    };
    let manifest = match serde_json::from_str::<ManagedStorageManifestV1>(&contents) {
        Ok(manifest) => manifest,
        Err(error) => {
            warnings.push(DashboardWarning::new(
                "managed_storage_manifest_invalid",
                format!(
                    "Managed-storage inventory {} is not valid schema-v1 JSON: {error}.",
                    path.display()
                ),
            ));
            return None;
        }
    };
    if let Err(message) = validate_managed_storage_manifest(&manifest, hdd_root) {
        warnings.push(DashboardWarning::new(
            "managed_storage_manifest_invalid",
            format!(
                "Managed-storage inventory {} cannot be used as a read-only fallback: {message}.",
                path.display()
            ),
        ));
        return None;
    }

    let mut hdds = manifest.hdds;
    hdds.sort_by(|left, right| left.role.cmp(&right.role));
    Some(ManagedStorageInventoryFallback { hdds })
}

fn validate_managed_storage_manifest(
    manifest: &ManagedStorageManifestV1,
    hdd_root: &Path,
) -> Result<(), &'static str> {
    if manifest.schema_version != 1 {
        return Err("unsupported schema version");
    }
    if manifest.ssd.role != "ssd"
        || manifest.ssd.path.as_os_str().is_empty()
        || manifest.ssd.label.trim().is_empty()
        || manifest.ssd.device.trim().is_empty()
        || manifest.ssd.filesystem.trim().is_empty()
        || manifest.ssd.uuid.trim().is_empty()
    {
        return Err("invalid SSD identity");
    }
    if manifest.hdds.is_empty() {
        return Err("no configured HDD members");
    }

    let mut paths = BTreeSet::new();
    let mut roles = BTreeSet::new();
    let mut uuids = BTreeSet::new();
    for member in &manifest.hdds {
        if !member.role.starts_with("hdd:")
            || member.path.parent() != Some(hdd_root)
            || member.label.trim().is_empty()
            || member.device.trim().is_empty()
            || member.filesystem.trim().is_empty()
            || member.uuid.trim().is_empty()
        {
            return Err("invalid HDD member");
        }
        if !paths.insert(member.path.clone())
            || !roles.insert(member.role.clone())
            || !uuids.insert(member.uuid.clone())
        {
            return Err("duplicate HDD identity");
        }
    }
    Ok(())
}

fn manifest_member_is_marker_backed(member: &ManagedStorageMemberV1) -> bool {
    let marker = marker_for_root(&member.path);
    marker.role.as_deref() == Some(member.role.as_str())
        && marker.device.as_deref() == Some(member.device.as_str())
        && marker.filesystem.as_deref() == Some(member.filesystem.as_str())
}

fn fallback_drive_count(total: usize, mounted: usize) -> crate::dashboard::DriveCountSummaryView {
    crate::dashboard::DriveCountSummaryView {
        total,
        mounted,
        healthy: mounted,
        watch: total.saturating_sub(mounted),
        suspect: 0,
        failed: 0,
    }
}

fn fallback_enclosure_slots(hdds: &[ManagedStorageMemberV1]) -> Vec<EnclosureDriveSlotView> {
    hdds.iter()
        .enumerate()
        .map(|(index, member)| {
            let mounted = manifest_member_is_marker_backed(member);
            EnclosureDriveSlotView {
                slot_number: (index + 1).min(u8::MAX as usize) as u8,
                drive_id: member
                    .role
                    .strip_prefix("hdd:")
                    .unwrap_or(member.label.as_str())
                    .to_string(),
                role: "managed_storage_inventory".to_string(),
                mount_path: member.path.display().to_string(),
                device_path: Some(member.device.clone()),
                filesystem: Some(member.filesystem.clone()),
                size_tib: mounted
                    .then(|| capacity_for_root(&member.path))
                    .flatten()
                    .map(|capacity| capacity_summary(&[capacity]).total_tib)
                    .unwrap_or_else(|| "0.0".to_string()),
                health: if mounted {
                    "healthy".to_string()
                } else {
                    "unavailable".to_string()
                },
                mounted,
                smart_warning_count: 0,
                actions_available: vec!["inspect".to_string()],
            }
        })
        .collect()
}

fn enclosure_slots(
    ssd_root: &Path,
    hdd_roots: &[PathBuf],
    ssd_marker: &DeviceMarker,
    hdd_markers: &[DeviceMarker],
) -> Vec<EnclosureDriveSlotView> {
    let mut slots = Vec::new();
    if ssd_root.exists() {
        slots.push(EnclosureDriveSlotView {
            slot_number: 0,
            drive_id: ssd_marker
                .device
                .as_deref()
                .unwrap_or("managed-ssd")
                .to_string(),
            role: "ssd_landing".to_string(),
            mount_path: ssd_root.display().to_string(),
            device_path: ssd_marker.device.clone(),
            filesystem: ssd_marker.filesystem.clone(),
            size_tib: capacity_for_root(ssd_root)
                .map(|capacity| capacity_summary(&[capacity]).total_tib)
                .unwrap_or_else(|| "0.0".to_string()),
            health: marker_health(ssd_marker, "ssd"),
            mounted: true,
            smart_warning_count: 0,
            actions_available: vec!["inspect".to_string(), "health_check".to_string()],
        });
    }

    for (index, root) in hdd_roots.iter().enumerate() {
        let marker = hdd_markers.get(index).cloned().unwrap_or_default();
        slots.push(EnclosureDriveSlotView {
            slot_number: (index + 1).min(u8::MAX as usize) as u8,
            drive_id: marker
                .disk_id()
                .or(marker.device.as_deref())
                .unwrap_or_else(|| {
                    root.file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("hdd")
                })
                .to_string(),
            role: "hdd_capacity".to_string(),
            mount_path: root.display().to_string(),
            device_path: marker.device.clone(),
            filesystem: marker.filesystem.clone(),
            size_tib: capacity_for_root(root)
                .map(|capacity| capacity_summary(&[capacity]).total_tib)
                .unwrap_or_else(|| "0.0".to_string()),
            health: marker_health(&marker, "hdd"),
            mounted: root.exists(),
            smart_warning_count: 0,
            actions_available: vec![
                "inspect".to_string(),
                "health_check".to_string(),
                "drain".to_string(),
            ],
        });
    }

    slots
}

fn marker_health(marker: &DeviceMarker, expected_role: &str) -> String {
    match marker.role.as_deref() {
        Some(role) if role == expected_role || role.starts_with(&format!("{expected_role}:")) => {
            "healthy".to_string()
        }
        Some(_) => "watch".to_string(),
        None => "watch".to_string(),
    }
}

fn marker_for_root(root: &Path) -> DeviceMarker {
    let path = root.join(".dasobjectstore").join("device.env");
    let Ok(contents) = fs::read_to_string(path) else {
        return DeviceMarker::default();
    };

    let mut marker = DeviceMarker::default();
    for line in contents.lines() {
        if let Some((key, value)) = line.split_once('=') {
            match key {
                "role" => marker.role = Some(value.to_string()),
                "device" => marker.device = Some(value.to_string()),
                "filesystem" => marker.filesystem = Some(value.to_string()),
                _ => {}
            }
        }
    }
    marker
}

impl DeviceMarker {
    fn disk_id(&self) -> Option<&str> {
        self.role.as_deref()?.strip_prefix("hdd:")
    }
}

#[cfg(test)]
mod tests {
    use super::{build_enclosures_dashboard, EnclosuresAggregatorConfig};
    use crate::dashboard::DashboardHealthStateView;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn enclosure_aggregator_builds_qnap_card_and_detail_slots() {
        let root = temp_root("enclosures-live");
        let ssd_root = root.join("ssd");
        let hdd_root = root.join("hdd");
        let disk_a = hdd_root.join("qnap-1057");
        let disk_b = hdd_root.join("qnap-1058");
        fs::create_dir_all(ssd_root.join(".dasobjectstore")).expect("ssd root");
        fs::create_dir_all(disk_a.join(".dasobjectstore")).expect("disk a");
        fs::create_dir_all(disk_b.join(".dasobjectstore")).expect("disk b");
        fs::write(
            ssd_root.join(".dasobjectstore/device.env"),
            "role=ssd\ndevice=/dev/disk/by-id/nvme-dos\nfilesystem=ext4\n",
        )
        .expect("ssd marker");
        fs::write(
            disk_a.join(".dasobjectstore/device.env"),
            "role=hdd:qnap-1057\ndevice=/dev/disk/by-id/qnap-1057\nfilesystem=ext4\n",
        )
        .expect("disk a marker");
        fs::write(
            disk_b.join(".dasobjectstore/device.env"),
            "role=hdd:qnap-1058\ndevice=/dev/disk/by-id/qnap-1058\nfilesystem=ext4\n",
        )
        .expect("disk b marker");

        let view = build_enclosures_dashboard(EnclosuresAggregatorConfig {
            ssd_root,
            hdd_root,
            managed_storage_manifest: root.join("missing-managed-storage.v1.json"),
            administrator: true,
        });

        assert_eq!(view.enclosures.len(), 1);
        assert!(!view.add_enclosure.enabled);
        assert_eq!(view.add_enclosure.state, "already_managed");
        assert!(view.add_enclosure.administrator);
        assert!(view.add_enclosure.supported_enclosure_detected);
        assert!(view.add_enclosure.daemon_ready);
        assert!(view
            .add_enclosure
            .blocked_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("already known")));
        assert_eq!(view.enclosures[0].display_name, "QNAP TL-D800C");
        assert_eq!(view.enclosures[0].health, DashboardHealthStateView::Healthy);
        assert_eq!(view.enclosures[0].drive_count.mounted, 3);
        assert_eq!(
            view.selected_enclosure_id.as_deref(),
            Some("qnap-tl-d800c-managed")
        );
        let detail = view.details.expect("detail");
        assert_eq!(detail.vendor, "QNAP");
        assert_eq!(detail.model, "TL-D800C");
        assert_eq!(detail.slots.len(), 3);
        assert_eq!(detail.slots[0].role, "ssd_landing");
        assert_eq!(detail.slots[0].filesystem.as_deref(), Some("ext4"));
        assert_eq!(detail.slots[1].role, "hdd_capacity");
        assert_eq!(detail.slots[1].drive_id, "qnap-1057");
        assert!(detail.slots[1].mount_path.ends_with("qnap-1057"));
        assert!(detail.slots[1]
            .actions_available
            .contains(&"drain".to_string()));
    }

    #[test]
    fn enclosure_aggregator_reports_missing_roots_without_bootstrap_warning() {
        let root = temp_root("enclosures-missing");

        let view = build_enclosures_dashboard(EnclosuresAggregatorConfig {
            ssd_root: root.join("missing-ssd"),
            hdd_root: root.join("missing-hdd"),
            managed_storage_manifest: root.join("missing-managed-storage.v1.json"),
            administrator: false,
        });

        assert!(view.enclosures.is_empty());
        assert!(!view.add_enclosure.enabled);
        assert_eq!(view.add_enclosure.state, "admin_required");
        assert!(!view.add_enclosure.administrator);
        assert!(!view.add_enclosure.supported_enclosure_detected);
        assert_eq!(view.selected_enclosure_id, None);
        assert_eq!(view.details, None);
        assert!(view
            .warnings
            .iter()
            .all(|warning| warning.code != "enclosure_inventory_pending"));
        assert!(view
            .warnings
            .iter()
            .any(|warning| warning.code == "hdd_root_missing"));
    }

    #[test]
    fn enclosure_aggregator_preserves_unmounted_qnap_manifest_as_read_only_inventory() {
        let root = temp_root("enclosures-managed-storage-fallback");
        let ssd_root = root.join("missing-ssd");
        let hdd_root = root.join("hdd");
        let manifest = root.join("managed-storage.v1.json");
        fs::write(
            &manifest,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "ssd": {
                    "path": ssd_root,
                    "uuid": "ssd-uuid",
                    "label": "DOS_SSD",
                    "device": "/dev/disk/by-id/ssd",
                    "filesystem": "ext4",
                    "role": "ssd"
                },
                "hdds": [
                    {
                        "path": hdd_root.join("qnap-1057"),
                        "uuid": "hdd-a-uuid",
                        "label": "DOS_HDD_01",
                        "device": "/dev/disk/by-id/qnap-1057",
                        "filesystem": "ext4",
                        "role": "hdd:qnap-1057"
                    },
                    {
                        "path": hdd_root.join("qnap-1058"),
                        "uuid": "hdd-b-uuid",
                        "label": "DOS_HDD_02",
                        "device": "/dev/disk/by-id/qnap-1058",
                        "filesystem": "ext4",
                        "role": "hdd:qnap-1058"
                    }
                ]
            }))
            .expect("manifest JSON"),
        )
        .expect("manifest");

        let view = build_enclosures_dashboard(EnclosuresAggregatorConfig {
            ssd_root,
            hdd_root,
            managed_storage_manifest: manifest,
            administrator: true,
        });

        assert_eq!(view.enclosures.len(), 1);
        assert_eq!(
            view.enclosures[0].display_name,
            "QNAP TL-D800C (managed-storage inventory)"
        );
        assert_eq!(view.enclosures[0].connection.protocol, "manifest fallback");
        assert_eq!(view.enclosures[0].health, DashboardHealthStateView::Watch);
        assert_eq!(view.enclosures[0].drive_count.total, 2);
        assert_eq!(view.enclosures[0].drive_count.mounted, 0);
        assert!(!view.add_enclosure.enabled);
        assert_eq!(view.add_enclosure.state, "already_managed");
        assert!(view.warnings.iter().any(|warning| {
            warning.code == "managed_storage_inventory_fallback"
                && warning
                    .message
                    .contains("does not mount, adopt, migrate, normalize, or delete")
        }));
        let detail = view.details.expect("fallback detail");
        assert_eq!(detail.slots.len(), 2);
        assert_eq!(detail.slots[0].drive_id, "qnap-1057");
        assert_eq!(detail.slots[0].role, "managed_storage_inventory");
        assert!(!detail.slots[0].mounted);
        assert_eq!(detail.slots[0].health, "unavailable");
        assert_eq!(detail.slots[0].actions_available, vec!["inspect"]);
    }

    #[test]
    fn enclosure_aggregator_rejects_manifest_members_outside_the_managed_hdd_root() {
        let root = temp_root("enclosures-invalid-managed-storage-fallback");
        let manifest = root.join("managed-storage.v1.json");
        fs::write(
            &manifest,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "ssd": {
                    "path": root.join("ssd"),
                    "uuid": "ssd-uuid",
                    "label": "DOS_SSD",
                    "device": "/dev/disk/by-id/ssd",
                    "filesystem": "ext4",
                    "role": "ssd"
                },
                "hdds": [{
                    "path": root.join("outside/qnap-1057"),
                    "uuid": "hdd-a-uuid",
                    "label": "DOS_HDD_01",
                    "device": "/dev/disk/by-id/qnap-1057",
                    "filesystem": "ext4",
                    "role": "hdd:qnap-1057"
                }]
            }))
            .expect("manifest JSON"),
        )
        .expect("manifest");

        let view = build_enclosures_dashboard(EnclosuresAggregatorConfig {
            ssd_root: root.join("missing-ssd"),
            hdd_root: root.join("hdd"),
            managed_storage_manifest: manifest,
            administrator: true,
        });

        assert!(view.enclosures.is_empty());
        assert!(view
            .warnings
            .iter()
            .any(|warning| warning.code == "managed_storage_manifest_invalid"));
    }

    fn temp_root(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("dos-gui-{label}-{unique}"));
        fs::create_dir_all(&root).expect("temp root");
        root
    }
}
