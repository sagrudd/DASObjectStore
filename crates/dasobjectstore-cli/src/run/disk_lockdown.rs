use std::fmt::{self, Display};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::process::Stdio;

pub(super) const LOCKDOWN_CONFIRMATION: &str = "confirm lockdown das";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct LockdownDasRequest {
    pub mount_root: PathBuf,
    pub service_user: String,
    pub service_group: String,
    pub create_service_user: bool,
    pub dry_run: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct LockdownDasReport {
    pub dry_run: bool,
    pub mount_root: PathBuf,
    pub service_user: String,
    pub service_group: String,
    pub protected_roots: Vec<PathBuf>,
    pub commands: Vec<String>,
}

pub(super) fn lockdown_das(
    request: &LockdownDasRequest,
) -> Result<LockdownDasReport, LockdownDasError> {
    let protected_roots = discover_protected_roots(&request.mount_root)?;
    if protected_roots.is_empty() {
        return Err(LockdownDasError::NoProtectedRoots {
            mount_root: request.mount_root.clone(),
        });
    }

    let mut commands = Vec::new();
    if request.create_service_user {
        commands.extend(service_account_commands(request)?);
    }
    commands.extend(lockdown_commands(request, &protected_roots));

    let rendered_commands = commands.iter().map(ManagedCommand::render).collect();
    if !request.dry_run {
        for command in &commands {
            command.run()?;
        }
    }

    Ok(LockdownDasReport {
        dry_run: request.dry_run,
        mount_root: request.mount_root.clone(),
        service_user: request.service_user.clone(),
        service_group: request.service_group.clone(),
        protected_roots,
        commands: rendered_commands,
    })
}

fn discover_protected_roots(mount_root: &Path) -> Result<Vec<PathBuf>, LockdownDasError> {
    let mut roots = Vec::new();
    let ssd_root = mount_root.join("ssd");
    if ssd_root.is_dir() {
        roots.push(ssd_root);
    }

    let hdd_root = mount_root.join("hdd");
    if hdd_root.is_dir() {
        let mut hdd_entries = fs::read_dir(hdd_root)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect::<Vec<_>>();
        hdd_entries.sort();
        roots.extend(hdd_entries);
    }

    Ok(roots)
}

fn service_account_commands(
    request: &LockdownDasRequest,
) -> Result<Vec<ManagedCommand>, LockdownDasError> {
    let mut commands = Vec::new();
    if !group_exists(&request.service_group)? {
        commands.push(ManagedCommand::new(
            "groupadd",
            vec!["--system".into(), request.service_group.clone()],
        ));
    }
    if !user_exists(&request.service_user)? {
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

    Ok(commands)
}

fn lockdown_commands(
    request: &LockdownDasRequest,
    protected_roots: &[PathBuf],
) -> Vec<ManagedCommand> {
    let owner = format!("{}:{}", request.service_user, request.service_group);
    let mut commands = vec![
        ManagedCommand::new(
            "chown",
            vec![
                "root:root".into(),
                request.mount_root.to_string_lossy().to_string(),
            ],
        ),
        ManagedCommand::new(
            "chmod",
            vec![
                "0755".into(),
                request.mount_root.to_string_lossy().to_string(),
            ],
        ),
    ];
    let hdd_root = request.mount_root.join("hdd");
    if hdd_root.is_dir() {
        commands.push(ManagedCommand::new(
            "chown",
            vec!["root:root".into(), hdd_root.to_string_lossy().to_string()],
        ));
        commands.push(ManagedCommand::new(
            "chmod",
            vec!["0755".into(), hdd_root.to_string_lossy().to_string()],
        ));
    }

    for root in protected_roots {
        let root_path = root.to_string_lossy().to_string();
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
                root.join("lost+found").to_string_lossy().to_string(),
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
                root.join("lost+found").to_string_lossy().to_string(),
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
                root.join("lost+found").to_string_lossy().to_string(),
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

fn group_exists(group: &str) -> Result<bool, LockdownDasError> {
    command_success("getent", &["group", group])
}

fn user_exists(user: &str) -> Result<bool, LockdownDasError> {
    command_success("id", &["-u", user])
}

fn command_success(program: &str, args: &[&str]) -> Result<bool, LockdownDasError> {
    let status = ProcessCommand::new(program)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    Ok(status.success())
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

    fn run(&self) -> Result<(), LockdownDasError> {
        let status = ProcessCommand::new(&self.program)
            .args(&self.args)
            .status()?;
        if !status.success() {
            return Err(LockdownDasError::CommandFailed {
                command: self.render(),
                status: status.to_string(),
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
pub(crate) enum LockdownDasError {
    CommandFailed { command: String, status: String },
    Io(std::io::Error),
    NoProtectedRoots { mount_root: PathBuf },
}

impl Display for LockdownDasError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CommandFailed { command, status } => {
                write!(
                    formatter,
                    "managed lockdown command `{command}` failed with {status}"
                )
            }
            Self::Io(err) => write!(formatter, "managed lockdown IO failed: {err}"),
            Self::NoProtectedRoots { mount_root } => write!(
                formatter,
                "no DASObjectStore SSD or HDD roots found under {}",
                mount_root.display()
            ),
        }
    }
}

impl std::error::Error for LockdownDasError {}

impl From<std::io::Error> for LockdownDasError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

#[cfg(test)]
mod tests {
    use super::{discover_protected_roots, lockdown_commands, LockdownDasRequest};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn discovers_ssd_and_hdd_roots() {
        let root = temp_root("lockdown-discover");
        fs::create_dir_all(root.join("ssd")).expect("ssd root");
        fs::create_dir_all(root.join("hdd").join("disk-b")).expect("hdd b");
        fs::create_dir_all(root.join("hdd").join("disk-a")).expect("hdd a");

        let roots = discover_protected_roots(&root).expect("roots discover");

        assert_eq!(
            roots,
            vec![
                root.join("ssd"),
                root.join("hdd").join("disk-a"),
                root.join("hdd").join("disk-b")
            ]
        );

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn renders_lockdown_commands_without_lost_found_recursion() {
        let request = LockdownDasRequest {
            mount_root: PathBuf::from("/srv/dasobjectstore"),
            service_user: "dasobjectstore".to_string(),
            service_group: "dasobjectstore".to_string(),
            create_service_user: false,
            dry_run: true,
        };
        let commands = lockdown_commands(
            &request,
            &[PathBuf::from("/srv/dasobjectstore/hdd/qnap-1057")],
        )
        .into_iter()
        .map(|command| command.render())
        .collect::<Vec<_>>();

        assert!(commands.iter().any(|command| {
            command == "chown dasobjectstore:dasobjectstore /srv/dasobjectstore/hdd/qnap-1057"
        }));
        assert!(commands
            .iter()
            .any(|command| command.contains("lost+found") && command.contains("-prune")));
        assert!(commands
            .iter()
            .any(|command| command.contains("chmod 0640")));
    }

    fn temp_root(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        std::env::temp_dir().join(format!("dasobjectstore-{name}-{nonce}"))
    }
}
