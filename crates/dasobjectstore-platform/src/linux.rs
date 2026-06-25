use crate::model::{
    FilesystemHint, HostPlatform, ObservedDisk, PartitionHint, ProbeReport, Transport,
};
use crate::probe::ProbeError;
use serde::Deserialize;

pub const LSBLK_COMMAND: &str = "lsblk";
pub const LSBLK_ARGS: [&str; 6] = [
    "--json",
    "--bytes",
    "--output",
    "NAME,PATH,SIZE,SERIAL,MODEL,TYPE,FSTYPE,MOUNTPOINT,TRAN,RM,HOTPLUG",
    "--tree",
    "--paths",
];

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

#[cfg(test)]
mod tests {
    use super::{parse_lsblk_json, LSBLK_ARGS, LSBLK_COMMAND};
    use crate::model::{HostPlatform, Transport};

    #[test]
    fn defines_stable_lsblk_json_command() {
        assert_eq!(LSBLK_COMMAND, "lsblk");
        assert!(LSBLK_ARGS.contains(&"--json"));
        assert!(LSBLK_ARGS.contains(&"--bytes"));
    }

    #[test]
    fn parses_linux_lsblk_disk_inventory() {
        let report = parse_lsblk_json(
            r#"{
              "blockdevices": [
                {
                  "name": "/dev/sda",
                  "path": "/dev/sda",
                  "size": 4000787030016,
                  "serial": "WD-OLD-001",
                  "model": "WDC WD40EFRX",
                  "type": "disk",
                  "tran": "usb",
                  "rm": false,
                  "hotplug": true,
                  "children": [
                    {
                      "name": "/dev/sda1",
                      "path": "/dev/sda1",
                      "size": 4000785997824,
                      "type": "part",
                      "fstype": "ext4",
                      "mountpoint": "/mnt/das/disk-a"
                    }
                  ]
                }
              ]
            }"#,
        )
        .expect("lsblk fixture parses");

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
}
