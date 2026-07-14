use crate::api::{DiskLockdownRequest, DiskLockdownResponse};
use crate::runtime::{DaemonServiceRuntimeError, ServiceCommandRunner};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub fn lockdown_das<R: ServiceCommandRunner>(
    runner: &R,
    request: DiskLockdownRequest,
    accepted_at_utc: &str,
) -> Result<DiskLockdownResponse, DaemonServiceRuntimeError> {
    request
        .validate()
        .map_err(|error| invalid(error.to_string()))?;
    let protected_roots = discover_protected_roots(&request.mount_root)?;
    if protected_roots.is_empty() {
        return Err(invalid(format!(
            "no DASObjectStore SSD or HDD roots found under {}",
            request.mount_root.display()
        )));
    }
    let mut commands = Vec::new();
    if request.create_service_user {
        if request.dry_run || !group_exists(&request.service_group)? {
            commands.push(ManagedCommand::new(
                "groupadd",
                vec!["--system".into(), request.service_group.clone()],
            ));
        }
        if request.dry_run || !user_exists(&request.service_user)? {
            commands.push(ManagedCommand::new(
                "useradd",
                vec![
                    "--system".into(),
                    "--gid".into(),
                    request.service_group.clone(),
                    "--home-dir".into(),
                    "/var/lib/dasobjectstore".into(),
                    "--no-create-home".into(),
                    "--shell".into(),
                    "/usr/sbin/nologin".into(),
                    request.service_user.clone(),
                ],
            ));
        }
    }
    commands.extend(lockdown_commands(&request, &protected_roots));
    let planned_commands = commands.iter().map(ManagedCommand::render).collect();
    if !request.dry_run {
        for command in &commands {
            runner.run(&command.program, &command.args)?;
        }
    }
    let job_id = crate::api::DaemonJobId::new(format!(
        "disk-lockdown-{}",
        accepted_at_utc
            .chars()
            .map(|character| if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            })
            .collect::<String>()
            .trim_matches('-')
            .to_ascii_lowercase()
    ))
    .map_err(|_| invalid("disk lockdown produced an invalid job id"))?;
    Ok(DiskLockdownResponse::accepted(
        job_id,
        accepted_at_utc,
        &request,
        protected_roots,
        planned_commands,
    ))
}

fn discover_protected_roots(mount_root: &Path) -> Result<Vec<PathBuf>, DaemonServiceRuntimeError> {
    let mut roots = Vec::new();
    let ssd_root = mount_root.join("ssd");
    if ssd_root.is_dir() {
        roots.push(ssd_root);
    }
    let hdd_root = mount_root.join("hdd");
    if hdd_root.is_dir() {
        let mut entries = fs::read_dir(&hdd_root)
            .map_err(|error| DaemonServiceRuntimeError::CommandIo {
                program: "read_dir".to_string(),
                message: error.to_string(),
            })?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect::<Vec<_>>();
        entries.sort();
        roots.extend(entries);
    }
    Ok(roots)
}

fn group_exists(group: &str) -> Result<bool, DaemonServiceRuntimeError> {
    Ok(Command::new("getent")
        .args(["group", group])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| DaemonServiceRuntimeError::CommandIo {
            program: "getent".to_string(),
            message: error.to_string(),
        })?
        .success())
}

fn user_exists(user: &str) -> Result<bool, DaemonServiceRuntimeError> {
    Ok(Command::new("id")
        .args(["-u", user])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| DaemonServiceRuntimeError::CommandIo {
            program: "id".to_string(),
            message: error.to_string(),
        })?
        .success())
}

fn lockdown_commands(request: &DiskLockdownRequest, roots: &[PathBuf]) -> Vec<ManagedCommand> {
    let owner = format!("{}:{}", request.service_user, request.service_group);
    let mut commands = vec![
        ManagedCommand::new("chown", vec!["root:root".into(), path(&request.mount_root)]),
        ManagedCommand::new("chmod", vec!["0755".into(), path(&request.mount_root)]),
    ];
    let hdd_root = request.mount_root.join("hdd");
    if hdd_root.is_dir() {
        commands.push(ManagedCommand::new(
            "chown",
            vec!["root:root".into(), path(&hdd_root)],
        ));
        commands.push(ManagedCommand::new(
            "chmod",
            vec!["0755".into(), path(&hdd_root)],
        ));
    }
    for root in roots {
        let root_path = path(root);
        let lost_found = path(&root.join("lost+found"));
        commands.push(ManagedCommand::new(
            "chown",
            vec![owner.clone(), root_path.clone()],
        ));
        commands.push(ManagedCommand::new(
            "chmod",
            vec!["0750".into(), root_path.clone()],
        ));
        commands.push(ManagedCommand::new(
            "find",
            vec![
                root_path.clone(),
                "-path".into(),
                lost_found.clone(),
                "-prune".into(),
                "-o".into(),
                "-mindepth".into(),
                "1".into(),
                "-exec".into(),
                "chown".into(),
                "-R".into(),
                owner.clone(),
                "{}".into(),
                "+".into(),
            ],
        ));
        commands.push(ManagedCommand::new(
            "find",
            vec![
                root_path.clone(),
                "-path".into(),
                lost_found.clone(),
                "-prune".into(),
                "-o".into(),
                "-type".into(),
                "d".into(),
                "-exec".into(),
                "chmod".into(),
                "0750".into(),
                "{}".into(),
                "+".into(),
            ],
        ));
        commands.push(ManagedCommand::new(
            "find",
            vec![
                root_path,
                "-path".into(),
                lost_found,
                "-prune".into(),
                "-o".into(),
                "-type".into(),
                "f".into(),
                "-exec".into(),
                "chmod".into(),
                "0640".into(),
                "{}".into(),
                "+".into(),
            ],
        ));
    }
    commands
}

fn path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn invalid(operation: impl Into<String>) -> DaemonServiceRuntimeError {
    DaemonServiceRuntimeError::UnsupportedOperation {
        operation: operation.into(),
    }
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
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .map(shell_quote)
            .collect::<Vec<_>>()
            .join(" ")
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::DaemonJobKind;
    use crate::runtime::service::ServiceCommandOutput;
    use std::cell::RefCell;

    struct FakeRunner {
        calls: RefCell<Vec<(String, Vec<String>)>>,
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
            Ok(ServiceCommandOutput {
                stdout: String::new(),
            })
        }
    }

    fn request(root: &Path, dry_run: bool) -> DiskLockdownRequest {
        DiskLockdownRequest {
            mount_root: root.to_path_buf(),
            service_user: "dasobjectstore".to_string(),
            service_group: "dasobjectstore".to_string(),
            create_service_user: true,
            dry_run,
            confirmation_marker: if dry_run {
                String::new()
            } else {
                crate::api::DISK_LOCKDOWN_CONFIRMATION.to_string()
            },
        }
    }

    #[test]
    fn dry_run_discovers_roots_and_plans_without_running_commands() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-lockdown-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("ssd")).expect("ssd root");
        fs::create_dir_all(root.join("hdd/disk-b")).expect("hdd b root");
        fs::create_dir_all(root.join("hdd/disk-a")).expect("hdd a root");

        let runner = FakeRunner {
            calls: RefCell::new(Vec::new()),
        };
        let response = lockdown_das(&runner, request(&root, true), "2026-07-14T00:00:00Z")
            .expect("lockdown dry run");

        assert_eq!(response.accepted.kind, DaemonJobKind::SystemAdministration);
        assert!(response.accepted.dry_run);
        assert_eq!(
            response.protected_roots,
            vec![
                root.join("ssd"),
                root.join("hdd/disk-a"),
                root.join("hdd/disk-b")
            ]
        );
        assert!(response
            .planned_commands
            .iter()
            .any(|command| command.starts_with("chown root:root")));
        assert!(runner.calls.borrow().is_empty());

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn mutation_runs_planned_commands_through_runner() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-lockdown-live-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("ssd")).expect("ssd root");

        let runner = FakeRunner {
            calls: RefCell::new(Vec::new()),
        };
        let mut live_request = request(&root, false);
        live_request.create_service_user = false;
        let response =
            lockdown_das(&runner, live_request, "2026-07-14T00:00:00Z").expect("lockdown mutation");

        assert!(!response.accepted.dry_run);
        assert_eq!(runner.calls.borrow().len(), response.planned_commands.len());
        assert_eq!(runner.calls.borrow()[0].0, "chown");

        fs::remove_dir_all(root).expect("cleanup");
    }
}
