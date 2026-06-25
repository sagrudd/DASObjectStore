use crate::model::{
    FilesystemHint, HostPlatform, ObservedDisk, PartitionHint, ProbeReport, Transport,
};
use crate::probe::{CommandRunner, ProbeError, ProbeProvider, SystemCommandRunner};
use serde::Deserialize;

pub const DISKUTIL_COMMAND: &str = "diskutil";
pub const DISKUTIL_LIST_ARGS: [&str; 2] = ["list", "-plist"];

#[derive(Debug, Default)]
pub struct MacosProbeProvider<R = SystemCommandRunner> {
    runner: R,
}

impl MacosProbeProvider<SystemCommandRunner> {
    pub fn system() -> Self {
        Self {
            runner: SystemCommandRunner,
        }
    }
}

impl<R> MacosProbeProvider<R> {
    pub fn new(runner: R) -> Self {
        Self { runner }
    }
}

impl<R> ProbeProvider for MacosProbeProvider<R>
where
    R: CommandRunner,
{
    fn probe(&self) -> Result<ProbeReport, ProbeError> {
        let output = self.runner.run(DISKUTIL_COMMAND, &DISKUTIL_LIST_ARGS)?;
        parse_diskutil_list_plist(output.as_bytes())
    }
}

pub fn parse_diskutil_list_plist(input: &[u8]) -> Result<ProbeReport, ProbeError> {
    let output: DiskutilList = plist::from_bytes(input).map_err(|err| ProbeError::ParseFailed {
        source: DISKUTIL_COMMAND.to_string(),
        message: err.to_string(),
    })?;

    let disks = output
        .all_disks_and_partitions
        .into_iter()
        .map(ObservedDisk::from)
        .collect();

    Ok(ProbeReport {
        platform: HostPlatform::Macos,
        disks,
        enclosures: Vec::new(),
        warnings: Vec::new(),
    })
}

#[derive(Debug, Deserialize)]
struct DiskutilList {
    #[serde(rename = "AllDisksAndPartitions")]
    all_disks_and_partitions: Vec<DiskutilDisk>,
}

#[derive(Debug, Deserialize)]
struct DiskutilDisk {
    #[serde(rename = "Content")]
    content: Option<String>,
    #[serde(rename = "DeviceIdentifier")]
    device_identifier: String,
    #[serde(rename = "Size")]
    size: Option<u64>,
    #[serde(rename = "Partitions", default)]
    partitions: Vec<DiskutilPartition>,
}

#[derive(Debug, Deserialize)]
struct DiskutilPartition {
    #[serde(rename = "Content")]
    content: Option<String>,
    #[serde(rename = "DeviceIdentifier")]
    device_identifier: String,
    #[serde(rename = "MountPoint")]
    mount_point: Option<String>,
    #[serde(rename = "Size")]
    size: Option<u64>,
    #[serde(rename = "VolumeName")]
    volume_name: Option<String>,
}

impl From<DiskutilDisk> for ObservedDisk {
    fn from(disk: DiskutilDisk) -> Self {
        let partition_hints = disk.partitions.iter().map(partition_hint).collect();
        let filesystem_hints = disk.partitions.iter().filter_map(filesystem_hint).collect();

        Self {
            device_path: Some(format!("/dev/{}", disk.device_identifier)),
            size_bytes: disk.size,
            serial_hint: None,
            model_hint: disk.content,
            partition_hints,
            filesystem_hints,
            direct_attached_hint: None,
            removable_hint: None,
            transport: Transport::Unknown,
            enclosure_topology_path: None,
        }
    }
}

fn partition_hint(partition: &DiskutilPartition) -> PartitionHint {
    PartitionHint {
        name: Some(format!("/dev/{}", partition.device_identifier)),
        size_bytes: partition.size,
        kind: partition.content.clone(),
    }
}

fn filesystem_hint(partition: &DiskutilPartition) -> Option<FilesystemHint> {
    partition
        .mount_point
        .as_ref()
        .map(|mount_point| FilesystemHint {
            name: partition
                .volume_name
                .clone()
                .or_else(|| Some(format!("/dev/{}", partition.device_identifier))),
            kind: partition.content.clone(),
            mount_point: Some(mount_point.clone()),
        })
}

#[cfg(test)]
mod tests {
    use super::{
        parse_diskutil_list_plist, MacosProbeProvider, DISKUTIL_COMMAND, DISKUTIL_LIST_ARGS,
    };
    use crate::model::{HostPlatform, Transport};
    use crate::probe::{CommandRunner, ProbeError, ProbeProvider};

    const DISKUTIL_LIST_FIXTURE: &[u8] =
        include_bytes!("../fixtures/macos/diskutil-list-usb-das.plist");

    #[test]
    fn defines_stable_diskutil_list_command() {
        assert_eq!(DISKUTIL_COMMAND, "diskutil");
        assert_eq!(DISKUTIL_LIST_ARGS, ["list", "-plist"]);
    }

    #[test]
    fn parses_macos_diskutil_list_inventory() {
        let report =
            parse_diskutil_list_plist(DISKUTIL_LIST_FIXTURE).expect("diskutil fixture parses");

        assert_eq!(report.platform, HostPlatform::Macos);
        assert_eq!(report.disks.len(), 1);

        let disk = &report.disks[0];
        assert_eq!(disk.device_path.as_deref(), Some("/dev/disk4"));
        assert_eq!(disk.size_bytes, Some(1000204886016));
        assert_eq!(disk.model_hint.as_deref(), Some("GUID_partition_scheme"));
        assert_eq!(disk.transport, Transport::Unknown);
        assert_eq!(
            disk.partition_hints[0].name.as_deref(),
            Some("/dev/disk4s1")
        );
        assert_eq!(
            disk.filesystem_hints[0].mount_point.as_deref(),
            Some("/Volumes/DAS_STAGING")
        );
    }

    #[test]
    fn rejects_invalid_diskutil_plist() {
        let err = parse_diskutil_list_plist(b"not-plist").expect_err("invalid plist fails");

        assert!(err.to_string().contains("failed to parse diskutil"));
    }

    #[test]
    fn macos_probe_provider_runs_diskutil_and_parses_output() {
        let provider = MacosProbeProvider::new(FixtureRunner {
            output: Ok(String::from_utf8(DISKUTIL_LIST_FIXTURE.to_vec()).expect("utf8 fixture")),
        });

        let report = provider.probe().expect("probe succeeds");

        assert_eq!(report.platform, HostPlatform::Macos);
        assert_eq!(report.disks.len(), 1);
    }

    #[test]
    fn macos_probe_provider_propagates_command_failure() {
        let provider = MacosProbeProvider::new(FixtureRunner {
            output: Err(ProbeError::CommandFailed {
                command: DISKUTIL_COMMAND.to_string(),
                message: "missing command".to_string(),
            }),
        });

        let err = provider.probe().expect_err("probe fails");

        assert_eq!(
            err,
            ProbeError::CommandFailed {
                command: DISKUTIL_COMMAND.to_string(),
                message: "missing command".to_string()
            }
        );
    }

    struct FixtureRunner {
        output: Result<String, ProbeError>,
    }

    impl CommandRunner for FixtureRunner {
        fn run(&self, command: &str, args: &[&str]) -> Result<String, ProbeError> {
            assert_eq!(command, DISKUTIL_COMMAND);
            assert_eq!(args, DISKUTIL_LIST_ARGS);

            self.output.clone()
        }
    }
}
