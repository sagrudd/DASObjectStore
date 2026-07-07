use crate::model::{EnclosureIdentity, ObservedDisk, ObservedEnclosure, ProbeReport};
use std::collections::BTreeMap;

pub fn with_enclosure_groups(mut report: ProbeReport) -> ProbeReport {
    report.enclosures = group_enclosures(&report.disks);
    report
}

pub fn group_enclosures(disks: &[ObservedDisk]) -> Vec<ObservedEnclosure> {
    let mut grouped: BTreeMap<&str, Vec<String>> = BTreeMap::new();

    for disk in disks {
        let Some(topology_path) = disk.enclosure_topology_path.as_deref() else {
            continue;
        };
        let Some(device_path) = disk.device_path.as_ref() else {
            continue;
        };

        grouped
            .entry(topology_path)
            .or_default()
            .push(device_path.clone());
    }

    grouped
        .into_iter()
        .map(|(topology_path, disk_device_paths)| ObservedEnclosure {
            identity: enclosure_identity_from_topology_path(topology_path),
            disk_device_paths,
        })
        .collect()
}

fn enclosure_identity_from_topology_path(topology_path: &str) -> EnclosureIdentity {
    const QNAP_TL_D800C_PREFIX: &str = "qnap-tl-d800c@";

    if let Some(usb_topology_path) = topology_path.strip_prefix(QNAP_TL_D800C_PREFIX) {
        return EnclosureIdentity {
            usb_topology_path: Some(usb_topology_path.to_string()),
            vendor_hint: Some("QNAP".to_string()),
            product_hint: Some("TL-D800C".to_string()),
            bridge_hint: Some("usb-jbod".to_string()),
            user_assigned_name: None,
        };
    }

    EnclosureIdentity {
        usb_topology_path: Some(topology_path.to_string()),
        vendor_hint: None,
        product_hint: None,
        bridge_hint: None,
        user_assigned_name: None,
    }
}

#[cfg(test)]
mod tests {
    use super::{group_enclosures, with_enclosure_groups};
    use crate::model::{
        HostPlatform, ObservedDisk, PartitionHint, ProbeReport, ProbeWarning, Transport,
    };

    #[test]
    fn groups_disks_by_shared_topology_path() {
        let disks = vec![
            disk("/dev/disk4", Some("usb@001/002")),
            disk("/dev/disk5", Some("usb@001/002")),
            disk("/dev/disk6", Some("usb@001/003")),
        ];

        let enclosures = group_enclosures(&disks);

        assert_eq!(enclosures.len(), 2);
        assert_eq!(
            enclosures[0].identity.usb_topology_path.as_deref(),
            Some("usb@001/002")
        );
        assert_eq!(
            enclosures[0].disk_device_paths,
            vec!["/dev/disk4".to_string(), "/dev/disk5".to_string()]
        );
        assert_eq!(
            enclosures[1].identity.usb_topology_path.as_deref(),
            Some("usb@001/003")
        );
    }

    #[test]
    fn ignores_disks_without_topology_or_device_path() {
        let mut missing_device = disk("/dev/disk7", Some("usb@001/004"));
        missing_device.device_path = None;
        let disks = vec![disk("/dev/disk4", None), missing_device];

        let enclosures = group_enclosures(&disks);

        assert!(enclosures.is_empty());
    }

    #[test]
    fn replaces_report_enclosures_with_computed_groups() {
        let report = ProbeReport {
            platform: HostPlatform::Macos,
            disks: vec![disk("/dev/disk4", Some("usb@001/002"))],
            enclosures: Vec::new(),
            warnings: vec![ProbeWarning {
                code: "fixture".to_string(),
                message: "kept".to_string(),
            }],
        };

        let report = with_enclosure_groups(report);

        assert_eq!(report.enclosures.len(), 1);
        assert_eq!(report.warnings.len(), 1);
    }

    #[test]
    fn identifies_qnap_tl_d800c_from_topology_marker() {
        let disks = vec![
            disk(
                "/dev/sda",
                Some("qnap-tl-d800c@pci-0000:00:14.0-usb-0:4:1.0"),
            ),
            disk(
                "/dev/sdb",
                Some("qnap-tl-d800c@pci-0000:00:14.0-usb-0:4:1.0"),
            ),
        ];

        let enclosures = group_enclosures(&disks);

        assert_eq!(enclosures.len(), 1);
        assert_eq!(enclosures[0].identity.vendor_hint.as_deref(), Some("QNAP"));
        assert_eq!(
            enclosures[0].identity.product_hint.as_deref(),
            Some("TL-D800C")
        );
        assert_eq!(
            enclosures[0].identity.usb_topology_path.as_deref(),
            Some("pci-0000:00:14.0-usb-0:4:1.0")
        );
        assert_eq!(
            enclosures[0].disk_device_paths,
            vec!["/dev/sda".to_string(), "/dev/sdb".to_string()]
        );
    }

    fn disk(device_path: &str, topology_path: Option<&str>) -> ObservedDisk {
        ObservedDisk {
            device_path: Some(device_path.to_string()),
            size_bytes: Some(1_000),
            serial_hint: None,
            model_hint: None,
            partition_hints: Vec::<PartitionHint>::new(),
            filesystem_hints: Vec::new(),
            direct_attached_hint: Some(true),
            removable_hint: Some(true),
            transport: Transport::Usb,
            enclosure_topology_path: topology_path.map(str::to_string),
        }
    }
}
