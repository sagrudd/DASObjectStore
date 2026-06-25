use crate::model::{
    FilesystemHint, HostPlatform, ObservedDisk, PartitionHint, ProbeReport, Transport,
};
use crate::probe::{ProbeError, ProbeProvider};
use serde::Deserialize;
use std::process::Command;

pub const LSBLK_COMMAND: &str = "lsblk";
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
        parse_lsblk_json(&output)
    }
}

pub trait CommandRunner {
    fn run(&self, command: &str, args: &[&str]) -> Result<String, ProbeError>;
}

#[derive(Debug, Default)]
pub struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn run(&self, command: &str, args: &[&str]) -> Result<String, ProbeError> {
        let output =
            Command::new(command)
                .args(args)
                .output()
                .map_err(|err| ProbeError::CommandFailed {
                    command: command.to_string(),
                    message: err.to_string(),
                })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(ProbeError::CommandFailed {
                command: command.to_string(),
                message: stderr,
            });
        }

        String::from_utf8(output.stdout).map_err(|err| ProbeError::ParseFailed {
            source: command.to_string(),
            message: err.to_string(),
        })
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

#[cfg(test)]
mod tests {
    use super::{parse_lsblk_json, CommandRunner, LinuxProbeProvider, LSBLK_ARGS, LSBLK_COMMAND};
    use crate::model::{HostPlatform, Transport};
    use crate::probe::{ProbeError, ProbeProvider};

    const LSBLK_FIXTURE: &str = r#"{
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
    }"#;

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

    struct FixtureRunner {
        output: Result<String, ProbeError>,
    }

    impl CommandRunner for FixtureRunner {
        fn run(&self, command: &str, args: &[&str]) -> Result<String, ProbeError> {
            assert_eq!(command, LSBLK_COMMAND);
            assert_eq!(args, LSBLK_ARGS);

            self.output.clone()
        }
    }
}
