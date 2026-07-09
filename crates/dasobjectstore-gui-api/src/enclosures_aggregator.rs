use crate::dashboard::{
    AddEnclosureAffordanceView, DasEnclosureCardView, DasEnclosureDetailView,
    DashboardHealthStateView, DashboardWarning, EnclosureConnectionView, EnclosureDriveSlotView,
    EnclosuresPageView, REDESIGN_DASHBOARD_SCHEMA_VERSION,
};
use crate::home_aggregator::{
    capacity_for_root, capacity_summary, discover_hdd_roots, drive_count_summary, env_path,
    now_utc_string, DEFAULT_HDD_ROOT, DEFAULT_SSD_ROOT,
};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
struct EnclosuresAggregatorConfig {
    ssd_root: PathBuf,
    hdd_root: PathBuf,
    administrator: bool,
}

impl EnclosuresAggregatorConfig {
    fn from_env() -> Self {
        Self {
            ssd_root: env_path("DASOBJECTSTORE_SSD_ROOT", DEFAULT_SSD_ROOT),
            hdd_root: env_path("DASOBJECTSTORE_HDD_ROOT", DEFAULT_HDD_ROOT),
            administrator: env_flag("DASOBJECTSTORE_WEB_ADMINISTRATOR"),
        }
    }
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

    let hdd_capacities = hdd_roots
        .iter()
        .filter_map(|root| capacity_for_root(root))
        .collect::<Vec<_>>();
    if hdd_roots
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
    let supported_enclosure_detected = !hdd_roots.is_empty();
    let daemon_ready = daemon_ready_for_affordance(&warnings);
    let add_enclosure = add_enclosure_affordance(
        config.administrator,
        supported_enclosure_detected,
        daemon_ready,
    );

    let mut enclosures = Vec::new();
    let mut details = None;
    let selected_enclosure_id = if hdd_roots.is_empty() {
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
    daemon_ready: bool,
) -> AddEnclosureAffordanceView {
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
    let looks_qnap = hdd_markers
        .iter()
        .filter_map(|marker| marker.disk_id())
        .any(|disk_id| disk_id.starts_with("qnap-"));

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
            administrator: true,
        });

        assert_eq!(view.enclosures.len(), 1);
        assert!(view.add_enclosure.enabled);
        assert_eq!(view.add_enclosure.state, "ready");
        assert!(view.add_enclosure.administrator);
        assert!(view.add_enclosure.supported_enclosure_detected);
        assert!(view.add_enclosure.daemon_ready);
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
