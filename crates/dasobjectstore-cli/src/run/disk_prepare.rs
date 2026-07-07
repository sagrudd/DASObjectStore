use dasobjectstore_core::ids::DiskId;
use std::fmt::{self, Display};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PrepareFilesystem {
    Ext4,
}

impl PrepareFilesystem {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Ext4 => "ext4",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PrepareDasDevice {
    pub role: PrepareDasRole,
    pub device_path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum PrepareDasRole {
    Ssd,
    Hdd { disk_id: DiskId, ordinal: usize },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PrepareDasRequest {
    pub devices: Vec<PrepareDasDevice>,
    pub mount_root: PathBuf,
    pub filesystem: PrepareFilesystem,
    pub owner: Option<String>,
    pub dry_run: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PrepareDasReport {
    pub dry_run: bool,
    pub mount_root: PathBuf,
    pub targets: Vec<PrepareDasTargetReport>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PrepareDasTargetReport {
    pub role: String,
    pub device_path: PathBuf,
    pub partition_path: PathBuf,
    pub mount_point: PathBuf,
    pub filesystem: String,
    pub commands: Vec<String>,
}

pub(super) fn prepare_das(
    request: &PrepareDasRequest,
) -> Result<PrepareDasReport, PrepareDasError> {
    if request.devices.is_empty() {
        return Err(PrepareDasError::NoDevices);
    }

    let mut targets = Vec::new();
    for device in &request.devices {
        let target = prepare_device(request, device)?;
        targets.push(target);
    }

    Ok(PrepareDasReport {
        dry_run: request.dry_run,
        mount_root: request.mount_root.clone(),
        targets,
    })
}

fn prepare_device(
    request: &PrepareDasRequest,
    device: &PrepareDasDevice,
) -> Result<PrepareDasTargetReport, PrepareDasError> {
    let device_path = &device.device_path;
    let label = label_for_role(&device.role);
    let mount_point = mount_point_for_role(&request.mount_root, &device.role);
    let partition_path = partition_path_for(device_path);
    let existing = inspect_existing_layout(device_path)?;
    let mut commands = Vec::new();

    for mountpoint in existing.mountpoints {
        commands.push(ManagedCommand::new("umount", vec![mountpoint]));
    }
    for swap_path in existing.swap_paths {
        commands.push(ManagedCommand::new("swapoff", vec![swap_path]));
    }
    for child_path in existing.child_paths {
        commands.push(ManagedCommand::new("wipefs", vec!["-a".into(), child_path]));
    }
    commands.push(ManagedCommand::new(
        "wipefs",
        vec!["-a".into(), device_path.to_string_lossy().to_string()],
    ));
    commands.push(ManagedCommand::new(
        "sgdisk",
        vec![
            "--zap-all".into(),
            device_path.to_string_lossy().to_string(),
        ],
    ));
    commands.push(ManagedCommand::new(
        "sgdisk",
        vec![
            "-n".into(),
            "1:1MiB:0".into(),
            "-t".into(),
            "1:8300".into(),
            "-c".into(),
            format!("1:{label}"),
            device_path.to_string_lossy().to_string(),
        ],
    ));
    commands.push(ManagedCommand::new(
        "partprobe",
        vec![device_path.to_string_lossy().to_string()],
    ));
    commands.push(ManagedCommand::new("udevadm", vec!["settle".into()]));
    commands.push(mkfs_command(
        request.filesystem,
        &label,
        partition_path.as_path(),
    ));
    commands.push(ManagedCommand::new(
        "mkdir",
        vec!["-p".into(), mount_point.to_string_lossy().to_string()],
    ));
    commands.push(ManagedCommand::new(
        "mount",
        vec![
            partition_path.to_string_lossy().to_string(),
            mount_point.to_string_lossy().to_string(),
        ],
    ));
    if let Some(owner) = &request.owner {
        commands.push(ManagedCommand::new(
            "chown",
            vec![
                format!("{owner}:{owner}"),
                mount_point.to_string_lossy().to_string(),
            ],
        ));
    }

    let rendered_commands = commands.iter().map(ManagedCommand::render).collect();
    if !request.dry_run {
        for command in &commands {
            command.run()?;
        }
        write_device_marker(&mount_point, device, request.filesystem)?;
    }

    Ok(PrepareDasTargetReport {
        role: role_name(&device.role),
        device_path: device_path.clone(),
        partition_path,
        mount_point,
        filesystem: request.filesystem.name().to_string(),
        commands: rendered_commands,
    })
}

fn mkfs_command(
    filesystem: PrepareFilesystem,
    label: &str,
    partition_path: &Path,
) -> ManagedCommand {
    match filesystem {
        PrepareFilesystem::Ext4 => ManagedCommand::new(
            "mkfs.ext4",
            vec![
                "-F".into(),
                "-L".into(),
                label.to_string(),
                partition_path.to_string_lossy().to_string(),
            ],
        ),
    }
}

fn inspect_existing_layout(device_path: &Path) -> Result<ExistingLayout, PrepareDasError> {
    let output = ProcessCommand::new("lsblk")
        .args(["-P", "-o", "PATH,TYPE,FSTYPE,MOUNTPOINTS"])
        .arg(device_path)
        .output()?;
    if !output.status.success() {
        return Err(PrepareDasError::CommandFailed {
            command: format!("lsblk {}", device_path.display()),
            status: output.status.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }

    Ok(parse_lsblk_pairs(&String::from_utf8_lossy(&output.stdout)))
}

fn parse_lsblk_pairs(output: &str) -> ExistingLayout {
    let mut layout = ExistingLayout::default();
    for line in output.lines() {
        let path = value_for_key(line, "PATH");
        let Some(path) = path else {
            continue;
        };
        let device_type = value_for_key(line, "TYPE").unwrap_or_default();
        let fstype = value_for_key(line, "FSTYPE").unwrap_or_default();
        let mountpoints = value_for_key(line, "MOUNTPOINTS").unwrap_or_default();
        if device_type == "part" {
            layout.child_paths.push(path.clone());
        }
        if fstype == "swap" {
            layout.swap_paths.push(path);
        }
        for mountpoint in mountpoints.split("\\n").filter(|value| !value.is_empty()) {
            layout.mountpoints.push(mountpoint.to_string());
        }
    }

    layout
}

fn value_for_key(line: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=\"");
    let start = line.find(&prefix)? + prefix.len();
    let rest = &line[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn partition_path_for(device_path: &Path) -> PathBuf {
    let value = device_path.to_string_lossy();
    if value.contains("/dev/disk/by-id/") {
        return PathBuf::from(format!("{value}-part1"));
    }
    if value.contains("nvme") {
        return PathBuf::from(format!("{value}p1"));
    }
    PathBuf::from(format!("{value}1"))
}

fn mount_point_for_role(mount_root: &Path, role: &PrepareDasRole) -> PathBuf {
    match role {
        PrepareDasRole::Ssd => mount_root.join("ssd"),
        PrepareDasRole::Hdd { disk_id, .. } => mount_root.join("hdd").join(disk_id.as_str()),
    }
}

fn label_for_role(role: &PrepareDasRole) -> String {
    match role {
        PrepareDasRole::Ssd => "DOS_SSD".to_string(),
        PrepareDasRole::Hdd { ordinal, .. } => format!("DOS_HDD_{ordinal:02}"),
    }
}

fn role_name(role: &PrepareDasRole) -> String {
    match role {
        PrepareDasRole::Ssd => "ssd".to_string(),
        PrepareDasRole::Hdd { disk_id, .. } => format!("hdd:{disk_id}"),
    }
}

fn write_device_marker(
    mount_point: &Path,
    device: &PrepareDasDevice,
    filesystem: PrepareFilesystem,
) -> Result<(), PrepareDasError> {
    let marker_dir = mount_point.join(".dasobjectstore");
    fs::create_dir_all(&marker_dir)?;
    let marker = format!(
        "role={}\ndevice={}\nfilesystem={}\n",
        role_name(&device.role),
        device.device_path.display(),
        filesystem.name()
    );
    fs::write(marker_dir.join("device.env"), marker)?;
    Ok(())
}

#[derive(Default)]
struct ExistingLayout {
    child_paths: Vec<String>,
    mountpoints: Vec<String>,
    swap_paths: Vec<String>,
}

struct ManagedCommand {
    program: String,
    args: Vec<String>,
}

impl ManagedCommand {
    fn new(program: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            program: program.into(),
            args,
        }
    }

    fn render(&self) -> String {
        let mut parts = vec![self.program.clone()];
        parts.extend(self.args.iter().map(|arg| shell_quote(arg)));
        parts.join(" ")
    }

    fn run(&self) -> Result<(), PrepareDasError> {
        let status = ProcessCommand::new(&self.program)
            .args(&self.args)
            .status()?;
        if !status.success() {
            return Err(PrepareDasError::CommandFailed {
                command: self.render(),
                status: status.to_string(),
                stderr: String::new(),
            });
        }
        Ok(())
    }
}

fn shell_quote(value: &str) -> String {
    if value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b':' | b'-' | b'_' | b'=')
    }) {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

#[derive(Debug)]
pub(crate) enum PrepareDasError {
    CommandFailed {
        command: String,
        status: String,
        stderr: String,
    },
    Io(std::io::Error),
    NoDevices,
}

impl Display for PrepareDasError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CommandFailed {
                command,
                status,
                stderr,
            } => {
                if stderr.is_empty() {
                    write!(
                        formatter,
                        "managed disk command `{command}` failed with {status}"
                    )
                } else {
                    write!(
                        formatter,
                        "managed disk command `{command}` failed with {status}: {stderr}"
                    )
                }
            }
            Self::Io(err) => write!(formatter, "managed disk preparation IO failed: {err}"),
            Self::NoDevices => {
                formatter.write_str("prepare-das requires at least one target device")
            }
        }
    }
}

impl std::error::Error for PrepareDasError {}

impl From<std::io::Error> for PrepareDasError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_lsblk_pairs, partition_path_for, PrepareDasRole};
    use dasobjectstore_core::ids::DiskId;
    use std::path::Path;

    #[test]
    fn derives_partition_paths_for_common_device_names() {
        assert_eq!(
            partition_path_for(Path::new("/dev/disk/by-id/usb-QNAP_1-0:0")),
            Path::new("/dev/disk/by-id/usb-QNAP_1-0:0-part1")
        );
        assert_eq!(
            partition_path_for(Path::new("/dev/sda")),
            Path::new("/dev/sda1")
        );
        assert_eq!(
            partition_path_for(Path::new("/dev/nvme0n1")),
            Path::new("/dev/nvme0n1p1")
        );
    }

    #[test]
    fn parses_existing_mounts_children_and_swap_from_lsblk_pairs() {
        let parsed = parse_lsblk_pairs(
            "PATH=\"/dev/sda\" TYPE=\"disk\" FSTYPE=\"\" MOUNTPOINTS=\"\"\n\
             PATH=\"/dev/sda1\" TYPE=\"part\" FSTYPE=\"xfs\" MOUNTPOINTS=\"/run/media/disk-a\"\n\
             PATH=\"/dev/sda2\" TYPE=\"part\" FSTYPE=\"swap\" MOUNTPOINTS=\"\"\n",
        );

        assert_eq!(parsed.child_paths, ["/dev/sda1", "/dev/sda2"]);
        assert_eq!(parsed.mountpoints, ["/run/media/disk-a"]);
        assert_eq!(parsed.swap_paths, ["/dev/sda2"]);
    }

    #[test]
    fn role_names_are_stable() {
        let role = PrepareDasRole::Hdd {
            disk_id: DiskId::new("qnap-a").expect("disk id"),
            ordinal: 1,
        };

        assert_eq!(super::role_name(&role), "hdd:qnap-a");
    }
}
