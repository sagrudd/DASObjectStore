use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, env, fmt, fs, io};
#[cfg(target_os = "linux")]
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub const SUDO_ADMIN_GROUPS: [&str; 3] = ["admin", "sudo", "wheel"];
#[cfg(target_os = "linux")]
pub const LOCAL_AUTH_HELPER_ENV: &str = "DASOBJECTSTORE_LOCAL_AUTH_HELPER";
#[cfg(target_os = "linux")]
pub const LOCAL_AUTH_HELPER_BYPASS_ENV: &str = "DASOBJECTSTORE_LOCAL_AUTH_HELPER_BYPASS";
#[cfg(target_os = "linux")]
pub const DEFAULT_LOCAL_AUTH_HELPER_PATH: &str =
    "/usr/libexec/dasobjectstore/dasobjectstore-local-auth-helper";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LocalUserMetadata {
    pub username: String,
    pub groups: Vec<String>,
    pub sudo_administrator: bool,
}

impl LocalUserMetadata {
    pub fn from_username_and_groups(username: impl Into<String>, groups: Vec<String>) -> Self {
        let groups = normalized_groups(groups);
        let sudo_administrator = groups.iter().any(|group| is_sudo_admin_group(group));

        Self {
            username: username.into(),
            groups,
            sudo_administrator,
        }
    }
}

#[derive(Debug)]
pub enum LocalUserDiscoveryError {
    MissingUsername,
    Io {
        path: &'static str,
        source: io::Error,
    },
}

impl fmt::Display for LocalUserDiscoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingUsername => write!(formatter, "local OS username could not be discovered"),
            Self::Io { path, source } => {
                write!(
                    formatter,
                    "local OS identity file read failed at {path}: {source}"
                )
            }
        }
    }
}

impl std::error::Error for LocalUserDiscoveryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::MissingUsername => None,
            Self::Io { source, .. } => Some(source),
        }
    }
}

#[derive(Clone, Debug)]
pub struct PamLocalPasswordAuthenticator {
    service_name: String,
}

impl PamLocalPasswordAuthenticator {
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
        }
    }

    pub fn authenticate(
        &self,
        username: &str,
        password: &str,
    ) -> Result<(), LocalPasswordAuthError> {
        authenticate_local_password(&self.service_name, username, password)
    }
}

impl Default for PamLocalPasswordAuthenticator {
    fn default() -> Self {
        Self::new("dasobjectstore")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LocalPasswordAuthError {
    UsernameRequired,
    PasswordRequired,
    InvalidCredentials,
    BackendUnavailable { message: String },
}

impl fmt::Display for LocalPasswordAuthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UsernameRequired => write!(formatter, "username is required"),
            Self::PasswordRequired => write!(formatter, "password is required"),
            Self::InvalidCredentials => write!(formatter, "invalid local username or password"),
            Self::BackendUnavailable { message } => {
                write!(
                    formatter,
                    "local password authentication unavailable: {message}"
                )
            }
        }
    }
}

impl std::error::Error for LocalPasswordAuthError {}

#[cfg(target_os = "linux")]
fn authenticate_local_password(
    service_name: &str,
    username: &str,
    password: &str,
) -> Result<(), LocalPasswordAuthError> {
    use pam_client::conv_mock::Conversation;
    use pam_client::{Context, Flag};

    let username = username.trim();
    if username.is_empty() {
        return Err(LocalPasswordAuthError::UsernameRequired);
    }
    if password.is_empty() {
        return Err(LocalPasswordAuthError::PasswordRequired);
    }

    if env::var_os(LOCAL_AUTH_HELPER_BYPASS_ENV).is_none() {
        if let Some(helper_path) = local_auth_helper_path() {
            return authenticate_local_password_with_helper(
                &helper_path,
                service_name,
                username,
                password,
            );
        }
    }

    let conversation = Conversation::with_credentials(username, password);
    let mut context = Context::new(service_name, Some(username), conversation).map_err(|err| {
        LocalPasswordAuthError::BackendUnavailable {
            message: err.to_string(),
        }
    })?;
    context
        .authenticate(Flag::NONE)
        .map_err(|_| LocalPasswordAuthError::InvalidCredentials)?;
    context
        .acct_mgmt(Flag::NONE)
        .map_err(|err| LocalPasswordAuthError::BackendUnavailable {
            message: err.to_string(),
        })
}

#[cfg(target_os = "linux")]
fn local_auth_helper_path() -> Option<PathBuf> {
    if let Some(path) = env::var_os(LOCAL_AUTH_HELPER_ENV) {
        let path = PathBuf::from(path);
        return (!path.as_os_str().is_empty()).then_some(path);
    }
    let path = PathBuf::from(DEFAULT_LOCAL_AUTH_HELPER_PATH);
    path.exists().then_some(path)
}

#[cfg(target_os = "linux")]
fn authenticate_local_password_with_helper(
    helper_path: &Path,
    service_name: &str,
    username: &str,
    password: &str,
) -> Result<(), LocalPasswordAuthError> {
    let mut child = Command::new(helper_path)
        .arg("--service")
        .arg(service_name)
        .arg(username)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| LocalPasswordAuthError::BackendUnavailable {
            message: format!(
                "failed to start local auth helper {}: {err}",
                helper_path.display()
            ),
        })?;

    let mut stdin =
        child
            .stdin
            .take()
            .ok_or_else(|| LocalPasswordAuthError::BackendUnavailable {
                message: "local auth helper stdin was unavailable".to_string(),
            })?;
    stdin.write_all(password.as_bytes()).map_err(|err| {
        LocalPasswordAuthError::BackendUnavailable {
            message: format!("failed to send password to local auth helper: {err}"),
        }
    })?;
    drop(stdin);

    let output =
        child
            .wait_with_output()
            .map_err(|err| LocalPasswordAuthError::BackendUnavailable {
                message: format!("failed to wait for local auth helper: {err}"),
            })?;
    if output.status.success() {
        return Ok(());
    }
    if output.status.code() == Some(1) {
        return Err(LocalPasswordAuthError::InvalidCredentials);
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(LocalPasswordAuthError::BackendUnavailable {
        message: if stderr.is_empty() {
            format!("local auth helper exited with {}", output.status)
        } else {
            format!("local auth helper exited with {}: {stderr}", output.status)
        },
    })
}

#[cfg(not(target_os = "linux"))]
fn authenticate_local_password(
    _service_name: &str,
    username: &str,
    password: &str,
) -> Result<(), LocalPasswordAuthError> {
    let username = username.trim();
    if username.is_empty() {
        return Err(LocalPasswordAuthError::UsernameRequired);
    }
    if password.is_empty() {
        return Err(LocalPasswordAuthError::PasswordRequired);
    }

    Err(LocalPasswordAuthError::BackendUnavailable {
        message: "PAM local password authentication is only available on Linux".to_string(),
    })
}

#[cfg(unix)]
pub fn discover_current_local_user() -> Result<LocalUserMetadata, LocalUserDiscoveryError> {
    let username = current_username().ok_or(LocalUserDiscoveryError::MissingUsername)?;
    let passwd =
        fs::read_to_string("/etc/passwd").map_err(|source| LocalUserDiscoveryError::Io {
            path: "/etc/passwd",
            source,
        })?;
    let group = fs::read_to_string("/etc/group").map_err(|source| LocalUserDiscoveryError::Io {
        path: "/etc/group",
        source,
    })?;

    Ok(local_user_metadata_from_unix_account_files(
        &username, &passwd, &group,
    ))
}

#[cfg(not(unix))]
pub fn discover_current_local_user() -> Result<LocalUserMetadata, LocalUserDiscoveryError> {
    let username = current_username().ok_or(LocalUserDiscoveryError::MissingUsername)?;
    Ok(LocalUserMetadata::from_username_and_groups(
        username,
        Vec::new(),
    ))
}

pub fn local_user_metadata_from_unix_account_files(
    username: &str,
    passwd_contents: &str,
    group_contents: &str,
) -> LocalUserMetadata {
    let primary_gid = primary_gid_for_user(username, passwd_contents);
    let mut groups = Vec::new();

    for line in group_contents.lines() {
        let Some(group) = parse_group_line(line) else {
            continue;
        };

        if primary_gid.as_deref() == Some(group.gid)
            || group.members.iter().any(|member| *member == username)
        {
            groups.push(group.name.to_string());
        }
    }

    LocalUserMetadata::from_username_and_groups(username, groups)
}

fn current_username() -> Option<String> {
    env::var("USER")
        .ok()
        .or_else(|| env::var("LOGNAME").ok())
        .map(|username| username.trim().to_string())
        .filter(|username| !username.is_empty())
}

fn primary_gid_for_user(username: &str, passwd_contents: &str) -> Option<String> {
    passwd_contents.lines().find_map(|line| {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 4 && fields[0] == username {
            Some(fields[3].to_string())
        } else {
            None
        }
    })
}

fn parse_group_line(line: &str) -> Option<GroupLine<'_>> {
    let fields: Vec<&str> = line.split(':').collect();
    if fields.len() < 4 || fields[0].trim().is_empty() || fields[2].trim().is_empty() {
        return None;
    }

    Some(GroupLine {
        name: fields[0].trim(),
        gid: fields[2].trim(),
        members: fields[3]
            .split(',')
            .map(str::trim)
            .filter(|member| !member.is_empty())
            .collect(),
    })
}

fn normalized_groups(groups: Vec<String>) -> Vec<String> {
    groups
        .into_iter()
        .map(|group| group.trim().to_string())
        .filter(|group| !group.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn is_sudo_admin_group(group: &str) -> bool {
    SUDO_ADMIN_GROUPS.contains(&group)
}

struct GroupLine<'a> {
    name: &'a str,
    gid: &'a str,
    members: Vec<&'a str>,
}

#[cfg(test)]
mod tests {
    use super::{local_user_metadata_from_unix_account_files, LocalUserMetadata};

    #[cfg(target_os = "linux")]
    use super::{PamLocalPasswordAuthenticator, LOCAL_AUTH_HELPER_ENV};
    #[cfg(target_os = "linux")]
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        path::PathBuf,
        sync::{Mutex, OnceLock},
    };

    #[test]
    fn detects_sudo_admin_from_group_membership() {
        let passwd = "stephen:x:1000:1000:Stephen:/home/stephen:/bin/bash\n";
        let group = "stephen:x:1000:\nsudo:x:27:stephen\nusers:x:100:\n";

        let metadata = local_user_metadata_from_unix_account_files("stephen", passwd, group);

        assert_eq!(
            metadata,
            LocalUserMetadata {
                username: "stephen".to_string(),
                groups: vec!["stephen".to_string(), "sudo".to_string()],
                sudo_administrator: true,
            }
        );
    }

    #[test]
    fn detects_wheel_admin_from_primary_gid() {
        let passwd = "root:x:0:0:root:/root:/bin/bash\n";
        let group = "wheel:x:0:\nusers:x:100:root\n";

        let metadata = local_user_metadata_from_unix_account_files("root", passwd, group);

        assert_eq!(
            metadata.groups,
            vec!["users".to_string(), "wheel".to_string()]
        );
        assert!(metadata.sudo_administrator);
    }

    #[test]
    fn normalizes_group_names_before_admin_detection() {
        let metadata = LocalUserMetadata::from_username_and_groups(
            "operator",
            vec![
                " users ".to_string(),
                "sudo".to_string(),
                "users".to_string(),
                "".to_string(),
            ],
        );

        assert_eq!(
            metadata.groups,
            vec!["sudo".to_string(), "users".to_string()]
        );
        assert!(metadata.sudo_administrator);
    }

    #[test]
    fn non_admin_user_is_not_sudo_administrator() {
        let passwd = "guest:x:1001:1001:Guest:/home/guest:/bin/bash\n";
        let group = "guest:x:1001:\nusers:x:100:guest\n";

        let metadata = local_user_metadata_from_unix_account_files("guest", passwd, group);

        assert_eq!(
            metadata.groups,
            vec!["guest".to_string(), "users".to_string()]
        );
        assert!(!metadata.sudo_administrator);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn local_password_authenticator_accepts_successful_helper_result() {
        let _guard = helper_env_lock().lock().expect("helper env lock");
        let helper = write_fake_auth_helper("success", "exit 0");
        std::env::set_var(LOCAL_AUTH_HELPER_ENV, &helper);

        PamLocalPasswordAuthenticator::new("dasobjectstore")
            .authenticate("stephen", "correct-password")
            .expect("helper success authenticates");

        std::env::remove_var(LOCAL_AUTH_HELPER_ENV);
        let _ = fs::remove_file(helper);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn local_password_authenticator_maps_helper_invalid_credentials() {
        let _guard = helper_env_lock().lock().expect("helper env lock");
        let helper = write_fake_auth_helper("invalid", "exit 1");
        std::env::set_var(LOCAL_AUTH_HELPER_ENV, &helper);

        let err = PamLocalPasswordAuthenticator::new("dasobjectstore")
            .authenticate("stephen", "wrong-password")
            .expect_err("helper invalid result is rejected");

        assert_eq!(err, super::LocalPasswordAuthError::InvalidCredentials);
        std::env::remove_var(LOCAL_AUTH_HELPER_ENV);
        let _ = fs::remove_file(helper);
    }

    #[cfg(target_os = "linux")]
    fn helper_env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[cfg(target_os = "linux")]
    fn write_fake_auth_helper(label: &str, command: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "dasobjectstore-fake-auth-helper-{label}-{}",
            std::process::id()
        ));
        fs::write(
            &path,
            format!("#!/usr/bin/env bash\nset -euo pipefail\ncat >/dev/null\n{command}\n"),
        )
        .expect("write fake auth helper");
        let mut permissions = fs::metadata(&path)
            .expect("fake helper metadata")
            .permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&path, permissions).expect("fake helper executable");
        path
    }
}
