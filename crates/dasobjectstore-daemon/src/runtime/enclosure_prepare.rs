//! Daemon-owned enclosure preparation executor.
//!
//! This module is deliberately command-runner based: production uses the
//! daemon's system runner, while tests can prove command ordering and marker
//! durability without touching a block device.

use super::service::{DaemonServiceRuntimeError, ServiceCommandRunner};
use crate::api::{PrepareEnclosureFilesystem, PrepareEnclosureRequest, PrepareEnclosureResponse};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub(super) fn prepare_enclosure<R: ServiceCommandRunner>(
    runner: &R,
    request: PrepareEnclosureRequest,
    accepted_at_utc: &str,
) -> Result<PrepareEnclosureResponse, DaemonServiceRuntimeError> {
    request
        .validate()
        .map_err(|error| DaemonServiceRuntimeError::UnsupportedOperation {
            operation: format!("invalid enclosure preparation request: {error}"),
        })?;

    let mut devices = Vec::with_capacity(request.hdd_devices.len() + 1);
    devices.push(Device {
        role: Role::Ssd,
        device_path: request.ssd_device.clone(),
    });
    for (ordinal, hdd) in request.hdd_devices.iter().enumerate() {
        devices.push(Device {
            role: Role::Hdd {
                disk_id: hdd.disk_id.clone(),
                ordinal: ordinal + 1,
            },
            device_path: hdd.device_path.clone(),
        });
    }

    for device in &devices {
        prepare_device(runner, &request, device)?;
    }

    let job_id_value = format!(
        "enclosure-prepare-{}",
        accepted_at_utc
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() {
                    character
                } else {
                    '-'
                }
            })
            .collect::<String>()
            .trim_matches('-')
            .to_ascii_lowercase()
    );
    let job_id = crate::api::DaemonJobId::new(job_id_value.clone())
        .map_err(|_| DaemonServiceRuntimeError::InvalidJobId(job_id_value))?;

    Ok(PrepareEnclosureResponse::accepted(
        job_id,
        accepted_at_utc,
        request.dry_run,
        request.ssd_device,
        request.hdd_devices,
        request.mount_root,
        request.filesystem,
        request.owner,
        request.administrator_actor,
    ))
}

fn prepare_device<R: ServiceCommandRunner>(
    runner: &R,
    request: &PrepareEnclosureRequest,
    device: &Device,
) -> Result<(), DaemonServiceRuntimeError> {
    let partition_path = partition_path_for(&device.device_path);
    let mount_point = mount_point_for_role(&request.mount_root, &device.role);
    let label = label_for_role(&device.role);
    let existing = inspect_existing_layout(runner, &device.device_path)?;
    let mut commands = Vec::new();

    for mountpoint in existing.mountpoints {
        commands.push(Command::new("umount", vec![mountpoint]));
    }
    for swap_path in existing.swap_paths {
        commands.push(Command::new("swapoff", vec![swap_path]));
    }
    for child_path in existing.child_paths {
        commands.push(Command::new("wipefs", vec!["-a".into(), child_path]));
    }
    commands.push(Command::new(
        "wipefs",
        vec!["-a".into(), device.device_path.display().to_string()],
    ));
    commands.push(Command::new(
        "sgdisk",
        vec!["--zap-all".into(), device.device_path.display().to_string()],
    ));
    commands.push(Command::new(
        "sgdisk",
        vec![
            "-n".into(),
            "1:1MiB:0".into(),
            "-t".into(),
            "1:8300".into(),
            "-c".into(),
            format!("1:{label}"),
            device.device_path.display().to_string(),
        ],
    ));
    commands.push(Command::new(
        "partprobe",
        vec![device.device_path.display().to_string()],
    ));
    commands.push(Command::new("udevadm", vec!["settle".into()]));
    commands.push(mkfs_command(request.filesystem, &label, &partition_path));
    commands.push(Command::new(
        "mkdir",
        vec!["-p".into(), mount_point.display().to_string()],
    ));
    commands.push(Command::new(
        "mount",
        vec![
            partition_path.display().to_string(),
            mount_point.display().to_string(),
        ],
    ));
    if let Some(owner) = request.owner.as_deref() {
        commands.push(Command::new(
            "chown",
            vec![
                format!("{owner}:{owner}"),
                mount_point.display().to_string(),
            ],
        ));
    }

    if !request.dry_run {
        for command in commands {
            runner.run(&command.program, &command.args)?;
        }
        write_device_marker(&mount_point, device, request.filesystem)?;
        if let Some(owner) = request.owner.as_deref() {
            runner.run(
                "chown",
                &vec![
                    "-R".into(),
                    format!("{owner}:{owner}"),
                    mount_point.join(".dasobjectstore").display().to_string(),
                ],
            )?;
        }
    }

    Ok(())
}

fn mkfs_command(
    filesystem: PrepareEnclosureFilesystem,
    label: &str,
    partition_path: &Path,
) -> Command {
    match filesystem {
        PrepareEnclosureFilesystem::Ext4 => Command::new(
            "mkfs.ext4",
            vec![
                "-F".into(),
                "-L".into(),
                label.into(),
                partition_path.display().to_string(),
            ],
        ),
        PrepareEnclosureFilesystem::Xfs => Command::new(
            "mkfs.xfs",
            vec![
                "-f".into(),
                "-L".into(),
                label.into(),
                partition_path.display().to_string(),
            ],
        ),
    }
}

fn inspect_existing_layout<R: ServiceCommandRunner>(
    runner: &R,
    device_path: &Path,
) -> Result<ExistingLayout, DaemonServiceRuntimeError> {
    let output = runner.run(
        "lsblk",
        &[
            "-P".into(),
            "-o".into(),
            "PATH,TYPE,FSTYPE,MOUNTPOINTS".into(),
            device_path.display().to_string(),
        ],
    )?;
    let mut layout = parse_lsblk_pairs(&output.stdout);
    let active_swaps = active_swap_paths();
    layout
        .swap_paths
        .retain(|path| active_swaps.iter().any(|active| active == path));
    Ok(layout)
}

fn parse_lsblk_pairs(output: &str) -> ExistingLayout {
    let mut layout = ExistingLayout::default();
    for line in output.lines() {
        let Some(path) = value_for_key(line, "PATH") else {
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
        layout.mountpoints.extend(
            mountpoints
                .split("\\n")
                .filter(|mountpoint| !mountpoint.is_empty())
                .map(ToOwned::to_owned),
        );
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

fn active_swap_paths() -> Vec<String> {
    let Ok(swaps) = fs::read_to_string("/proc/swaps") else {
        return Vec::new();
    };
    swaps
        .lines()
        .skip(1)
        .filter_map(|line| line.split_whitespace().next())
        .map(ToOwned::to_owned)
        .collect()
}

fn partition_path_for(device_path: &Path) -> PathBuf {
    let value = device_path.to_string_lossy();
    if value.contains("/dev/disk/by-id/") {
        return PathBuf::from(format!("{value}-part1"));
    }
    if value.contains("nvme") || value.contains("mmcblk") {
        return PathBuf::from(format!("{value}p1"));
    }
    PathBuf::from(format!("{value}1"))
}

fn mount_point_for_role(mount_root: &Path, role: &Role) -> PathBuf {
    match role {
        Role::Ssd => mount_root.join("ssd"),
        Role::Hdd { disk_id, .. } => mount_root.join("hdd").join(disk_id),
    }
}

fn label_for_role(role: &Role) -> String {
    match role {
        Role::Ssd => "DOS_SSD".to_string(),
        Role::Hdd { ordinal, .. } => format!("DOS_HDD_{ordinal:02}"),
    }
}

fn write_device_marker(
    mount_point: &Path,
    device: &Device,
    filesystem: PrepareEnclosureFilesystem,
) -> Result<(), DaemonServiceRuntimeError> {
    let marker_dir = mount_point.join(".dasobjectstore");
    fs::create_dir_all(&marker_dir).map_err(|error| DaemonServiceRuntimeError::CommandIo {
        program: "create device marker directory".to_string(),
        message: error.to_string(),
    })?;
    let marker_path = marker_dir.join("device.env");
    let temporary_path = marker_dir.join(format!(".device.env.{}.tmp", std::process::id()));
    let marker = format!(
        "role={}\ndevice={}\nfilesystem={}\n",
        device.role.name(),
        device.device_path.display(),
        filesystem
    );
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary_path)
        .map_err(|error| DaemonServiceRuntimeError::CommandIo {
            program: "create device marker".to_string(),
            message: error.to_string(),
        })?;
    file.write_all(marker.as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|error| DaemonServiceRuntimeError::CommandIo {
            program: "sync device marker".to_string(),
            message: error.to_string(),
        })?;
    drop(file);
    fs::rename(&temporary_path, &marker_path).map_err(|error| {
        DaemonServiceRuntimeError::CommandIo {
            program: "install device marker".to_string(),
            message: error.to_string(),
        }
    })?;
    let directory =
        File::open(&marker_dir).map_err(|error| DaemonServiceRuntimeError::CommandIo {
            program: "open device marker directory".to_string(),
            message: error.to_string(),
        })?;
    directory
        .sync_all()
        .map_err(|error| DaemonServiceRuntimeError::CommandIo {
            program: "sync device marker directory".to_string(),
            message: error.to_string(),
        })
}

#[derive(Clone, Debug)]
struct Device {
    role: Role,
    device_path: PathBuf,
}

#[derive(Clone, Debug)]
enum Role {
    Ssd,
    Hdd { disk_id: String, ordinal: usize },
}

impl Role {
    fn name(&self) -> String {
        match self {
            Self::Ssd => "ssd".to_string(),
            Self::Hdd { disk_id, .. } => format!("hdd:{disk_id}"),
        }
    }
}

struct Command {
    program: String,
    args: Vec<String>,
}

impl Command {
    fn new(program: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            program: program.into(),
            args,
        }
    }
}

#[derive(Default)]
struct ExistingLayout {
    child_paths: Vec<String>,
    mountpoints: Vec<String>,
    swap_paths: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::service::ServiceCommandOutput;
    use std::cell::RefCell;

    struct FakeRunner {
        calls: RefCell<Vec<(String, Vec<String>)>>,
        lsblk: String,
    }

    impl ServiceCommandRunner for FakeRunner {
        fn run(
            &self,
            program: &str,
            args: &[String],
        ) -> Result<ServiceCommandOutput, DaemonServiceRuntimeError> {
            self.calls
                .borrow_mut()
                .push((program.to_string(), args.to_vec()));
            if program == "lsblk" {
                return Ok(ServiceCommandOutput {
                    stdout: self.lsblk.clone(),
                });
            }
            if program == "mkdir" {
                fs::create_dir_all(args.last().expect("mkdir path")).expect("mkdir");
            } else if program == "mount" {
                fs::create_dir_all(args.last().expect("mount path")).expect("mount");
            }
            Ok(ServiceCommandOutput {
                stdout: String::new(),
            })
        }
    }

    fn request(root: PathBuf, dry_run: bool) -> PrepareEnclosureRequest {
        PrepareEnclosureRequest {
            ssd_device: PathBuf::from("/dev/nvme0n1"),
            hdd_devices: vec![crate::api::PrepareEnclosureHddDevice {
                disk_id: "disk-01".to_string(),
                device_path: PathBuf::from("/dev/disk/by-id/hdd-01"),
            }],
            mount_root: root,
            filesystem: PrepareEnclosureFilesystem::Ext4,
            owner: None,
            dry_run,
            client_request_id: Some("test-prepare".to_string()),
            administrator_actor: Some("root".to_string()),
            allow_format: true,
            existing_data_acknowledged: true,
            confirmation_marker: crate::api::ENCLOSURE_PREPARE_CONFIRMATION.to_string(),
        }
    }

    #[test]
    fn dry_run_plans_both_targets_without_destructive_commands() {
        let runner = FakeRunner {
            calls: RefCell::new(Vec::new()),
            lsblk: "PATH=\"/dev/nvme0n1\" TYPE=\"disk\" FSTYPE=\"\" MOUNTPOINTS=\"\"\n".to_string(),
        };
        let response = prepare_enclosure(
            &runner,
            request(PathBuf::from("/tmp/dasobjectstore-test"), true),
            "2026-07-12T08:00:00Z",
        )
        .expect("dry-run response");

        assert!(response.accepted.dry_run);
        assert_eq!(response.hdd_devices.len(), 1);
        assert_eq!(
            runner
                .calls
                .borrow()
                .iter()
                .filter(|(program, _)| program != "lsblk")
                .count(),
            0
        );
    }

    #[test]
    fn non_dry_run_writes_durable_role_markers_after_mounts() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-enclosure-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        let runner = FakeRunner {
            calls: RefCell::new(Vec::new()),
            lsblk: String::new(),
        };
        let response = prepare_enclosure(
            &runner,
            request(root.clone(), false),
            "2026-07-12T08:00:00Z",
        )
        .expect("prepare response");

        assert!(!response.accepted.dry_run);
        let ssd_marker = root.join("ssd/.dasobjectstore/device.env");
        let hdd_marker = root.join("hdd/disk-01/.dasobjectstore/device.env");
        assert!(ssd_marker.is_file());
        assert!(hdd_marker.is_file());
        assert!(fs::read_to_string(ssd_marker)
            .expect("ssd marker")
            .contains("role=ssd"));
        assert!(fs::read_to_string(hdd_marker)
            .expect("hdd marker")
            .contains("role=hdd:disk-01"));
        assert!(!root
            .join("ssd/.dasobjectstore/.device.env.temporary")
            .exists());
        fs::remove_dir_all(root).expect("cleanup");
    }
}
