use crate::model::{
    FilesystemHint, HostPlatform, ObservedDisk, PartitionHint, ProbeReport, ProbeWarning, Transport,
};
use crate::probe::{CommandRunner, ProbeError, ProbeProvider, SystemCommandRunner};
use serde::Deserialize;
use std::collections::BTreeMap;

pub const LSBLK_COMMAND: &str = "lsblk";
pub const UDEVADM_COMMAND: &str = "udevadm";
pub const LSBLK_ARGS: [&str; 6] = [
    "--json",
    "--bytes",
    "--output",
    "NAME,PATH,SIZE,SERIAL,MODEL,TYPE,FSTYPE,MOUNTPOINT,TRAN,RM,HOTPLUG",
    "--tree",
    "--paths",
];

#[derive(Debug, Default)]
pub struct LinuxProbeProvider<R = SystemCommandRunner> {
    runner: R,
}

impl LinuxProbeProvider<SystemCommandRunner> {
    pub fn system() -> Self {
        Self {
            runner: SystemCommandRunner,
        }
    }
}

impl<R> LinuxProbeProvider<R> {
    pub fn new(runner: R) -> Self {
        Self { runner }
    }
}

impl<R> ProbeProvider for LinuxProbeProvider<R>
where
    R: CommandRunner,
{
    fn probe(&self) -> Result<ProbeReport, ProbeError> {
        let output = self.runner.run(LSBLK_COMMAND, &LSBLK_ARGS)?;
        let mut report = parse_lsblk_json(&output)?;
        enrich_linux_disks_from_udev(&self.runner, &mut report);
        Ok(report)
    }
}

pub fn parse_lsblk_json(input: &str) -> Result<ProbeReport, ProbeError> {
    let output: LsblkOutput =
        serde_json::from_str(input).map_err(|err| ProbeError::ParseFailed {
            source: LSBLK_COMMAND.to_string(),
            message: err.to_string(),
        })?;

    let disks = output
        .blockdevices
        .into_iter()
        .filter(|device| device.device_type.as_deref() == Some("disk"))
        .map(ObservedDisk::from)
        .collect();

    Ok(ProbeReport {
        platform: HostPlatform::Linux,
        disks,
        enclosures: Vec::new(),
        warnings: Vec::new(),
    })
}

#[derive(Debug, Deserialize)]
struct LsblkOutput {
    blockdevices: Vec<LsblkDevice>,
}

#[derive(Debug, Deserialize)]
struct LsblkDevice {
    name: Option<String>,
    path: Option<String>,
    size: Option<u64>,
    serial: Option<String>,
    model: Option<String>,
    #[serde(rename = "type")]
    device_type: Option<String>,
    fstype: Option<String>,
    mountpoint: Option<String>,
    tran: Option<String>,
    rm: Option<bool>,
    hotplug: Option<bool>,
    #[serde(default)]
    children: Vec<LsblkDevice>,
}

impl From<LsblkDevice> for ObservedDisk {
    fn from(device: LsblkDevice) -> Self {
        let transport = device
            .tran
            .as_deref()
            .map(transport_from_lsblk)
            .unwrap_or(Transport::Unknown);
        let partition_hints = device.children.iter().map(partition_hint).collect();
        let filesystem_hints = device.children.iter().filter_map(filesystem_hint).collect();

        Self {
            device_path: device.path.or(device.name),
            size_bytes: device.size,
            serial_hint: device.serial,
            model_hint: device.model,
            partition_hints,
            filesystem_hints,
            direct_attached_hint: Some(matches!(
                transport,
                Transport::Usb | Transport::Thunderbolt
            )),
            removable_hint: device.rm.or(device.hotplug),
            transport,
            enclosure_topology_path: None,
        }
    }
}

fn partition_hint(device: &LsblkDevice) -> PartitionHint {
    PartitionHint {
        name: device.path.clone().or_else(|| device.name.clone()),
        size_bytes: device.size,
        kind: device.device_type.clone(),
    }
}

fn filesystem_hint(device: &LsblkDevice) -> Option<FilesystemHint> {
    device.fstype.as_ref().map(|kind| FilesystemHint {
        name: device.path.clone().or_else(|| device.name.clone()),
        kind: Some(kind.clone()),
        mount_point: device.mountpoint.clone(),
    })
}

fn transport_from_lsblk(value: &str) -> Transport {
    match value {
        "usb" => Transport::Usb,
        "sata" => Transport::Sata,
        "nvme" => Transport::Nvme,
        _ => Transport::Unknown,
    }
}

fn enrich_linux_disks_from_udev<R>(runner: &R, report: &mut ProbeReport)
where
    R: CommandRunner,
{
    for disk in &mut report.disks {
        let Some(device_path) = disk.device_path.as_deref() else {
            continue;
        };
        let result = runner.run(
            UDEVADM_COMMAND,
            &["info", "--query=property", "--name", device_path],
        );
        let output = match result {
            Ok(output) => output,
            Err(err) => {
                report.warnings.push(ProbeWarning {
                    code: "linux_udevadm_failed".to_string(),
                    message: format!("failed to inspect {device_path}: {err}"),
                });
                continue;
            }
        };
        let properties = parse_udev_properties(&output);
        apply_udev_properties(disk, &properties);
    }
}

fn apply_udev_properties(disk: &mut ObservedDisk, properties: &BTreeMap<String, String>) {
    if disk.serial_hint.is_none() {
        disk.serial_hint = property(properties, &["ID_SERIAL_SHORT", "ID_SERIAL"]).cloned();
    }
    if disk.model_hint.is_none() {
        disk.model_hint = property(
            properties,
            &["ID_MODEL_FROM_DATABASE", "ID_MODEL", "ID_USB_MODEL"],
        )
        .cloned();
    }

    let Some(id_path) = property(properties, &["ID_PATH"]) else {
        return;
    };
    let Some(usb_topology_path) = usb_enclosure_path_from_id_path(id_path) else {
        return;
    };

    disk.enclosure_topology_path = Some(if is_qnap_tl_d800c(properties, disk) {
        format!("qnap-tl-d800c@{usb_topology_path}")
    } else {
        usb_topology_path
    });
}

fn parse_udev_properties(input: &str) -> BTreeMap<String, String> {
    input
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let line = line.strip_prefix("E: ").unwrap_or(line);
            let (key, value) = line.split_once('=')?;
            Some((key.to_string(), value.to_string()))
        })
        .collect()
}

fn usb_enclosure_path_from_id_path(id_path: &str) -> Option<String> {
    if !id_path.contains("-usb-") {
        return None;
    }
    let usb_path = id_path
        .split_once("-scsi-")
        .map(|(prefix, _)| prefix)
        .unwrap_or(id_path);
    Some(usb_path.to_string())
}

fn is_qnap_tl_d800c(properties: &BTreeMap<String, String>, disk: &ObservedDisk) -> bool {
    let vendor_match = values_contain_normalized(
        properties,
        &["ID_VENDOR", "ID_VENDOR_FROM_DATABASE", "ID_USB_VENDOR"],
        "QNAP",
    );
    let product_match = values_contain_normalized(
        properties,
        &[
            "ID_MODEL",
            "ID_MODEL_FROM_DATABASE",
            "ID_USB_MODEL",
            "ID_SERIAL",
        ],
        "TLD800C",
    ) || disk
        .model_hint
        .as_deref()
        .is_some_and(|value| normalize_hardware_string(value).contains("TLD800C"));

    vendor_match && product_match
}

fn values_contain_normalized(
    properties: &BTreeMap<String, String>,
    keys: &[&str],
    needle: &str,
) -> bool {
    keys.iter().any(|key| {
        properties
            .get(*key)
            .is_some_and(|value| normalize_hardware_string(value).contains(needle))
    })
}

fn normalize_hardware_string(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(|character| character.to_uppercase())
        .collect()
}

fn property<'a>(properties: &'a BTreeMap<String, String>, keys: &[&str]) -> Option<&'a String> {
    keys.iter().find_map(|key| properties.get(*key))
}

#[cfg(test)]
mod tests {
    use super::{
        parse_lsblk_json, parse_udev_properties, usb_enclosure_path_from_id_path,
        LinuxProbeProvider, LSBLK_ARGS, LSBLK_COMMAND, UDEVADM_COMMAND,
    };
    use crate::model::{HostPlatform, Transport};
    use crate::probe::{CommandRunner, ProbeError, ProbeProvider};
    use std::collections::BTreeMap;

    const LSBLK_FIXTURE: &str = include_str!("../fixtures/linux/lsblk-usb-das.json");

    #[test]
    fn defines_stable_lsblk_json_command() {
        assert_eq!(LSBLK_COMMAND, "lsblk");
        assert!(LSBLK_ARGS.contains(&"--json"));
        assert!(LSBLK_ARGS.contains(&"--bytes"));
    }

    #[test]
    fn parses_linux_lsblk_disk_inventory() {
        let report = parse_lsblk_json(LSBLK_FIXTURE).expect("lsblk fixture parses");

        assert_eq!(report.platform, HostPlatform::Linux);
        assert_eq!(report.disks.len(), 1);

        let disk = &report.disks[0];
        assert_eq!(disk.device_path.as_deref(), Some("/dev/sda"));
        assert_eq!(disk.serial_hint.as_deref(), Some("WD-OLD-001"));
        assert_eq!(disk.transport, Transport::Usb);
        assert_eq!(disk.removable_hint, Some(false));
        assert_eq!(disk.partition_hints.len(), 1);
        assert_eq!(disk.filesystem_hints[0].kind.as_deref(), Some("ext4"));
        assert_eq!(
            disk.filesystem_hints[0].mount_point.as_deref(),
            Some("/mnt/das/disk-a")
        );
    }

    #[test]
    fn rejects_invalid_lsblk_json() {
        let err = parse_lsblk_json("not-json").expect_err("invalid json fails");

        assert!(err.to_string().contains("failed to parse lsblk"));
    }

    #[test]
    fn linux_probe_provider_runs_lsblk_and_parses_output() {
        let provider = LinuxProbeProvider::new(FixtureRunner {
            output: Ok(LSBLK_FIXTURE.to_string()),
            udev_outputs: BTreeMap::new(),
        });

        let report = provider.probe().expect("probe succeeds");

        assert_eq!(report.platform, HostPlatform::Linux);
        assert_eq!(report.disks.len(), 1);
    }

    #[test]
    fn linux_probe_provider_propagates_command_failure() {
        let provider = LinuxProbeProvider::new(FixtureRunner {
            output: Err(ProbeError::CommandFailed {
                command: LSBLK_COMMAND.to_string(),
                message: "missing command".to_string(),
            }),
            udev_outputs: BTreeMap::new(),
        });

        let err = provider.probe().expect_err("probe fails");

        assert_eq!(
            err,
            ProbeError::CommandFailed {
                command: LSBLK_COMMAND.to_string(),
                message: "missing command".to_string()
            }
        );
    }

    #[test]
    fn linux_probe_provider_maps_qnap_tl_d800c_members_by_usb_topology() {
        let mut udev_outputs = BTreeMap::new();
        udev_outputs.insert(
            "/dev/sda".to_string(),
            "ID_VENDOR=QNAP\nID_MODEL=TL-D800C\nID_PATH=pci-0000:00:14.0-usb-0:4:1.0-scsi-0:0:0:0\n".to_string(),
        );
        udev_outputs.insert(
            "/dev/sdb".to_string(),
            "ID_VENDOR=QNAP\nID_MODEL=TL_D800C\nID_PATH=pci-0000:00:14.0-usb-0:4:1.0-scsi-0:0:1:0\n".to_string(),
        );
        udev_outputs.insert(
            "/dev/sdc".to_string(),
            "ID_VENDOR=Other\nID_MODEL=USB_DISK\nID_PATH=pci-0000:00:14.0-usb-0:8:1.0-scsi-0:0:0:0\n".to_string(),
        );
        let provider = LinuxProbeProvider::new(FixtureRunner {
            output: Ok(QNAP_TL_D800C_LSBLK_FIXTURE.to_string()),
            udev_outputs,
        });

        let report = provider.probe().expect("probe succeeds");

        assert_eq!(report.disks.len(), 3);
        assert_eq!(
            report.disks[0].enclosure_topology_path.as_deref(),
            Some("qnap-tl-d800c@pci-0000:00:14.0-usb-0:4:1.0")
        );
        assert_eq!(
            report.disks[1].enclosure_topology_path.as_deref(),
            Some("qnap-tl-d800c@pci-0000:00:14.0-usb-0:4:1.0")
        );
        assert_eq!(
            report.disks[2].enclosure_topology_path.as_deref(),
            Some("pci-0000:00:14.0-usb-0:8:1.0")
        );
    }

    #[test]
    fn normalizes_usb_id_path_to_physical_enclosure_path() {
        assert_eq!(
            usb_enclosure_path_from_id_path("pci-0000:00:14.0-usb-0:4:1.0-scsi-0:0:7:0").as_deref(),
            Some("pci-0000:00:14.0-usb-0:4:1.0")
        );
        assert_eq!(
            usb_enclosure_path_from_id_path("pci-0000:00:17.0-ata-1"),
            None
        );
    }

    #[test]
    fn parses_udev_property_output_with_optional_prefix() {
        let properties = parse_udev_properties("E: ID_VENDOR=QNAP\nID_MODEL=TL-D800C\n");

        assert_eq!(
            properties.get("ID_VENDOR").map(String::as_str),
            Some("QNAP")
        );
        assert_eq!(
            properties.get("ID_MODEL").map(String::as_str),
            Some("TL-D800C")
        );
    }

    struct FixtureRunner {
        output: Result<String, ProbeError>,
        udev_outputs: BTreeMap<String, String>,
    }

    impl CommandRunner for FixtureRunner {
        fn run(&self, command: &str, args: &[&str]) -> Result<String, ProbeError> {
            match command {
                LSBLK_COMMAND => {
                    assert_eq!(args, LSBLK_ARGS);
                    self.output.clone()
                }
                UDEVADM_COMMAND => {
                    assert_eq!(&args[0..3], ["info", "--query=property", "--name"]);
                    Ok(self.udev_outputs.get(args[3]).cloned().unwrap_or_default())
                }
                _ => panic!("unexpected command: {command}"),
            }
        }
    }

    const QNAP_TL_D800C_LSBLK_FIXTURE: &str = r#"{
      "blockdevices": [
        {
          "name": "/dev/sda",
          "path": "/dev/sda",
          "size": 4000787030016,
          "serial": "QNAP-0001",
          "model": "WDC WD40EFRX",
          "type": "disk",
          "tran": "usb",
          "rm": false,
          "hotplug": true
        },
        {
          "name": "/dev/sdb",
          "path": "/dev/sdb",
          "size": 4000787030016,
          "serial": "QNAP-0002",
          "model": "WDC WD40EFRX",
          "type": "disk",
          "tran": "usb",
          "rm": false,
          "hotplug": true
        },
        {
          "name": "/dev/sdc",
          "path": "/dev/sdc",
          "size": 2000398934016,
          "serial": "OTHER-0001",
          "model": "Other USB Disk",
          "type": "disk",
          "tran": "usb",
          "rm": false,
          "hotplug": true
        }
      ]
    }"#;
}
