use super::*;

#[cfg(target_os = "linux")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SourceAclPermission {
    Traverse,
    ReadTree,
}

#[cfg(target_os = "linux")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SourceAclAction {
    pub(super) path: PathBuf,
    pub(super) permission: SourceAclPermission,
}

#[cfg(target_os = "linux")]
pub(super) fn prepare_source_access_for_packaged_daemon(source: &Path) -> Result<(), CliError> {
    const SERVICE_USER: &str = "dasobjectstore";

    let source = source.canonicalize().map_err(|err| {
        CliError::CommandFailed(format!(
            "failed to resolve ingest source {} before daemon submission: {err}",
            source.display()
        ))
    })?;
    if !source.exists() {
        return Err(CliError::CommandFailed(format!(
            "ingest source {} does not exist",
            source.display()
        )));
    }

    for action in plan_source_acl_actions(&source)? {
        match action.permission {
            SourceAclPermission::Traverse => run_setfacl(
                &[
                    "-m",
                    &format!("u:{SERVICE_USER}:--x"),
                    path_arg(&action.path).as_str(),
                ],
                &action.path,
                "grant daemon traversal",
            )?,
            SourceAclPermission::ReadTree => run_setfacl(
                &[
                    "-R",
                    "-m",
                    &format!("u:{SERVICE_USER}:rX"),
                    path_arg(&action.path).as_str(),
                ],
                &action.path,
                "grant daemon source read",
            )?,
        }
    }

    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub(super) fn prepare_source_access_for_packaged_daemon(_source: &Path) -> Result<(), CliError> {
    Ok(())
}

#[cfg(target_os = "linux")]
pub(super) fn plan_source_acl_actions(source: &Path) -> Result<Vec<SourceAclAction>, CliError> {
    let mut actions = acl_ancestors_requiring_execute(source)?
        .into_iter()
        .map(|path| SourceAclAction {
            path,
            permission: SourceAclPermission::Traverse,
        })
        .collect::<Vec<_>>();
    actions.push(SourceAclAction {
        path: source.to_path_buf(),
        permission: SourceAclPermission::ReadTree,
    });
    Ok(actions)
}

#[cfg(target_os = "linux")]
fn acl_ancestors_requiring_execute(source: &Path) -> Result<Vec<PathBuf>, CliError> {
    let mut ancestors = source.ancestors().skip(1).collect::<Vec<_>>();
    ancestors.reverse();
    let mut required = Vec::new();
    for ancestor in ancestors {
        if ancestor.parent().is_none() {
            continue;
        }
        let metadata = fs::metadata(ancestor).map_err(|err| {
            CliError::CommandFailed(format!(
                "failed to inspect ingest source ancestor {}: {err}",
                ancestor.display()
            ))
        })?;
        if metadata.permissions().mode() & 0o001 == 0 {
            required.push(ancestor.to_path_buf());
        }
    }
    Ok(required)
}

#[cfg(target_os = "linux")]
fn run_setfacl(args: &[&str], path: &Path, action: &str) -> Result<(), CliError> {
    let output = ProcessCommand::new("setfacl")
        .args(args)
        .output()
        .map_err(|err| {
            CliError::CommandFailed(format!(
                "failed to run setfacl to {action} for {}: {err}",
                path.display()
            ))
        })?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.contains("Operation not permitted") || stderr.contains("Permission denied") {
        // Mount roots under /run/media are commonly created by udisks as
        // root-owned directories. A non-interactive sudo retry lets an
        // already-authorized operator grant the daemon read-only traversal
        // without prompting inside the TUI. It remains a no-op when sudo is
        // unavailable or the filesystem itself does not support POSIX ACLs.
        if let Ok(sudo_output) = ProcessCommand::new("sudo")
            .args(["-n", "setfacl"])
            .args(args)
            .output()
        {
            if sudo_output.status.success() {
                return Ok(());
            }
            let sudo_stderr = String::from_utf8_lossy(&sudo_output.stderr)
                .trim()
                .to_string();
            let sudo_detail = if sudo_stderr.is_empty() {
                sudo_output.status.to_string()
            } else {
                sudo_stderr
            };
            return Err(CliError::CommandFailed(format!(
                "failed to {action} for {}: {stderr}; non-interactive sudo retry failed: {sudo_detail}. The source mount may not support POSIX ACLs; remount it with service-readable uid/gid/mode options or pre-grant read/traverse access to dasobjectstore.",
                path.display()
            )));
        }
    }

    let detail = if stderr.is_empty() {
        output.status.to_string()
    } else {
        stderr
    };
    Err(CliError::CommandFailed(format!(
        "failed to {action} for {}: {detail}",
        path.display()
    )))
}

#[cfg(target_os = "linux")]
fn path_arg(path: &Path) -> String {
    path.to_string_lossy().to_string()
}
