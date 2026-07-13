//! Host-mode path derivation for local folder ObjectStores.

use dasobjectstore_core::deployment::HostMode;
use std::fmt::{self, Display};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

const APP_DIR: &str = "dasobjectstore";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FolderHostPaths {
    pub state_dir: PathBuf,
    pub runtime_dir: Option<PathBuf>,
}

/// A render-only per-user service definition. The daemon owns the executable
/// and config paths; deployment code may write the rendered plist under the
/// user's LaunchAgents directory and invoke `launchctl` separately.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserServicePlan {
    pub label: String,
    pub executable: PathBuf,
    pub config_path: PathBuf,
    pub state_dir: PathBuf,
}

impl UserServicePlan {
    pub fn launchd_plist(&self) -> Result<String, FolderHostPathError> {
        validate_service_label(&self.label)?;
        require_absolute("service executable", &self.executable)?;
        require_absolute("service config", &self.config_path)?;
        require_absolute("service state", &self.state_dir)?;
        let stdout = self.state_dir.join("logs/dasobjectstored.stdout.log");
        let stderr = self.state_dir.join("logs/dasobjectstored.stderr.log");
        Ok(format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
<plist version=\"1.0\">\n<dict>\n  <key>Label</key>\n  <string>{}</string>\n  <key>ProgramArguments</key>\n  <array>\n    <string>{}</string>\n    <string>--config</string>\n    <string>{}</string>\n  </array>\n  <key>RunAtLoad</key>\n  <true/>\n  <key>KeepAlive</key>\n  <true/>\n  <key>StandardOutPath</key>\n  <string>{}</string>\n  <key>StandardErrorPath</key>\n  <string>{}</string>\n</dict>\n</plist>\n",
            xml_escape(&self.label),
            xml_escape(&self.executable.display().to_string()),
            xml_escape(&self.config_path.display().to_string()),
            xml_escape(&stdout.display().to_string()),
            xml_escape(&stderr.display().to_string()),
        ))
    }
}

pub fn user_service_plan(
    paths: &FolderHostPaths,
    executable: impl Into<PathBuf>,
    config_path: impl Into<PathBuf>,
    label: impl Into<String>,
) -> Result<UserServicePlan, FolderHostPathError> {
    validate_user_service_state_owner(&paths.state_dir)?;
    let plan = UserServicePlan {
        label: label.into(),
        executable: executable.into(),
        config_path: config_path.into(),
        state_dir: paths.state_dir.clone(),
    };
    plan.launchd_plist()?;
    Ok(plan)
}

/// Verify that an existing per-user state directory belongs to the current
/// user. Missing state is allowed so a first-run plan remains render-only;
/// installation code is responsible for creating it with the correct owner.
pub fn validate_user_service_state_owner(path: &Path) -> Result<(), FolderHostPathError> {
    #[cfg(unix)]
    {
        validate_user_service_state_owner_with_uid(path, unsafe { libc::geteuid() })
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

#[cfg(unix)]
fn validate_user_service_state_owner_with_uid(
    path: &Path,
    expected_uid: u32,
) -> Result<(), FolderHostPathError> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(FolderHostPathError::StateDirectoryUnreadable {
                path: path.to_path_buf(),
                message: error.to_string(),
            })
        }
    };
    if !metadata.is_dir() {
        return Err(FolderHostPathError::StateDirectoryNotDirectory(
            path.to_path_buf(),
        ));
    }
    if metadata.uid() != expected_uid {
        return Err(FolderHostPathError::StateDirectoryNotOwned {
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

impl FolderHostPaths {
    pub fn socket_path(&self, socket_name: &str) -> Result<PathBuf, FolderHostPathError> {
        if socket_name.trim().is_empty() {
            return Err(FolderHostPathError::BlankSocketName);
        }
        let Some(runtime_dir) = &self.runtime_dir else {
            return Err(FolderHostPathError::MissingRuntimeDirectory);
        };
        let socket_path = runtime_dir.join(socket_name);
        if socket_path.as_os_str().len() > 100 {
            return Err(FolderHostPathError::SocketPathTooLong(socket_path));
        }
        Ok(socket_path)
    }
}

pub fn folder_host_paths(
    mode: HostMode,
    home_dir: Option<&Path>,
    xdg_state_home: Option<&Path>,
    xdg_runtime_dir: Option<&Path>,
    system_state_dir: &Path,
    system_runtime_dir: &Path,
) -> Result<FolderHostPaths, FolderHostPathError> {
    match mode {
        HostMode::PerUser => {
            let home = home_dir.ok_or(FolderHostPathError::MissingHomeDirectory)?;
            require_absolute("home_dir", home)?;
            let state_base = xdg_state_home
                .map(Path::to_path_buf)
                .unwrap_or_else(|| home.join(".local/state"));
            let runtime_base = xdg_runtime_dir.map(Path::to_path_buf);
            require_absolute("xdg_state_home", &state_base)?;
            if let Some(runtime_base) = &runtime_base {
                require_absolute("xdg_runtime_dir", runtime_base)?;
            }
            Ok(FolderHostPaths {
                state_dir: state_base.join(APP_DIR),
                runtime_dir: runtime_base.map(|path| path.join(APP_DIR)),
            })
        }
        HostMode::System | HostMode::Integrated => {
            require_absolute("system_state_dir", system_state_dir)?;
            require_absolute("system_runtime_dir", system_runtime_dir)?;
            Ok(FolderHostPaths {
                state_dir: system_state_dir.to_path_buf(),
                runtime_dir: Some(system_runtime_dir.to_path_buf()),
            })
        }
    }
}

fn require_absolute(field: &'static str, path: &Path) -> Result<(), FolderHostPathError> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(FolderHostPathError::RelativePath {
            field,
            path: path.to_path_buf(),
        })
    }
}

fn validate_service_label(label: &str) -> Result<(), FolderHostPathError> {
    if label.is_empty()
        || !label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err(FolderHostPathError::InvalidServiceLabel(label.to_string()));
    }
    Ok(())
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FolderHostPathError {
    MissingHomeDirectory,
    MissingRuntimeDirectory,
    BlankSocketName,
    SocketPathTooLong(PathBuf),
    InvalidServiceLabel(String),
    RelativePath { field: &'static str, path: PathBuf },
    StateDirectoryUnreadable { path: PathBuf, message: String },
    StateDirectoryNotDirectory(PathBuf),
    StateDirectoryNotOwned { path: PathBuf },
}

impl Display for FolderHostPathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingHomeDirectory => {
                formatter.write_str("per-user folder mode requires a home directory")
            }
            Self::MissingRuntimeDirectory => {
                formatter.write_str("per-user folder mode has no XDG runtime directory")
            }
            Self::BlankSocketName => formatter.write_str("folder socket name must not be blank"),
            Self::SocketPathTooLong(path) => {
                write!(
                    formatter,
                    "folder socket path is too long: {}",
                    path.display()
                )
            }
            Self::InvalidServiceLabel(label) => {
                write!(formatter, "invalid per-user service label: {label}")
            }
            Self::RelativePath { field, path } => {
                write!(formatter, "{field} must be absolute: {}", path.display())
            }
            Self::StateDirectoryUnreadable { path, message } => write!(
                formatter,
                "per-user state directory {} is unreadable: {message}",
                path.display()
            ),
            Self::StateDirectoryNotDirectory(path) => write!(
                formatter,
                "per-user state path is not a directory: {}",
                path.display()
            ),
            Self::StateDirectoryNotOwned { path } => write!(
                formatter,
                "per-user state directory is not owned by the current user: {}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for FolderHostPathError {}

#[cfg(test)]
mod tests {
    use super::*;
    use dasobjectstore_core::deployment::HostMode;

    #[test]
    fn derives_per_user_xdg_paths_without_root() {
        let paths = folder_host_paths(
            HostMode::PerUser,
            Some(Path::new("/Users/tester")),
            Some(Path::new("/Users/tester/Library/State")),
            Some(Path::new("/tmp/user-runtime")),
            Path::new("/var/lib/dasobjectstore"),
            Path::new("/run/dasobjectstore"),
        )
        .expect("per-user paths derive");
        assert_eq!(
            paths.state_dir,
            PathBuf::from("/Users/tester/Library/State/dasobjectstore")
        );
        assert_eq!(
            paths.runtime_dir,
            Some(PathBuf::from("/tmp/user-runtime/dasobjectstore"))
        );
        assert_eq!(
            paths.socket_path("dasobjectstored.sock").unwrap(),
            PathBuf::from("/tmp/user-runtime/dasobjectstore/dasobjectstored.sock")
        );
    }

    #[test]
    fn derives_system_and_integrated_paths_from_explicit_roots() {
        for mode in [HostMode::System, HostMode::Integrated] {
            let paths = folder_host_paths(
                mode,
                None,
                None,
                None,
                Path::new("/var/lib/dasobjectstore"),
                Path::new("/run/dasobjectstore"),
            )
            .expect("system paths derive");
            assert_eq!(paths.state_dir, PathBuf::from("/var/lib/dasobjectstore"));
            assert_eq!(
                paths.runtime_dir,
                Some(PathBuf::from("/run/dasobjectstore"))
            );
        }
    }

    #[test]
    fn rejects_missing_home_and_relative_roots() {
        assert_eq!(
            folder_host_paths(
                HostMode::PerUser,
                None,
                None,
                None,
                Path::new("/var/lib"),
                Path::new("/run"),
            ),
            Err(FolderHostPathError::MissingHomeDirectory)
        );
        assert!(matches!(
            folder_host_paths(
                HostMode::System,
                None,
                None,
                None,
                Path::new("relative"),
                Path::new("/run"),
            ),
            Err(FolderHostPathError::RelativePath {
                field: "system_state_dir",
                ..
            })
        ));
        let user_without_runtime = folder_host_paths(
            HostMode::PerUser,
            Some(Path::new("/Users/tester")),
            None,
            None,
            Path::new("/var/lib"),
            Path::new("/run"),
        )
        .expect("offline user paths derive");
        assert_eq!(user_without_runtime.runtime_dir, None);
        assert_eq!(
            user_without_runtime.socket_path("socket"),
            Err(FolderHostPathError::MissingRuntimeDirectory)
        );
    }

    #[test]
    fn per_user_and_system_modes_have_non_overlapping_namespaces() {
        let user = folder_host_paths(
            HostMode::PerUser,
            Some(Path::new("/Users/tester")),
            Some(Path::new("/Users/tester/Library/State")),
            Some(Path::new("/Users/tester/Library/Runtime")),
            Path::new("/var/lib/dasobjectstore"),
            Path::new("/run/dasobjectstore"),
        )
        .expect("per-user paths derive");
        let system = folder_host_paths(
            HostMode::System,
            None,
            None,
            None,
            Path::new("/var/lib/dasobjectstore"),
            Path::new("/run/dasobjectstore"),
        )
        .expect("system paths derive");

        assert_ne!(user.state_dir, system.state_dir);
        assert_ne!(user.runtime_dir, system.runtime_dir);
        assert_eq!(
            user.socket_path("daemon.sock").expect("user socket"),
            PathBuf::from("/Users/tester/Library/Runtime/dasobjectstore/daemon.sock")
        );
        assert_eq!(
            system.socket_path("daemon.sock").expect("system socket"),
            PathBuf::from("/run/dasobjectstore/daemon.sock")
        );
    }

    #[test]
    fn renders_escaped_launchd_user_service_plan_without_installing_it() {
        let paths = folder_host_paths(
            HostMode::PerUser,
            Some(Path::new("/Users/tester")),
            Some(Path::new("/Users/tester/Library/User & State")),
            Some(Path::new("/tmp/user-runtime")),
            Path::new("/var/lib/dasobjectstore"),
            Path::new("/run/dasobjectstore"),
        )
        .expect("per-user paths derive");
        let plan = user_service_plan(
            &paths,
            "/Users/tester/bin/dasobjectstored",
            "/Users/tester/Library/Config/dasobjectstore.json",
            "org.dasobjectstore.dasobjectstored",
        )
        .expect("launchd plan validates");
        let plist = plan.launchd_plist().expect("plist renders");
        assert!(plist.contains("<key>RunAtLoad</key>\n  <true/>"));
        assert!(plist.contains("org.dasobjectstore.dasobjectstored"));
        assert!(plist.contains("User &amp; State/dasobjectstore/logs"));
        assert!(plist.contains("--config"));
        assert!(!plist.contains("/etc/dasobjectstore"));
    }

    #[test]
    fn rejects_invalid_launchd_service_plan_inputs() {
        let paths = folder_host_paths(
            HostMode::PerUser,
            Some(Path::new("/Users/tester")),
            None,
            Some(Path::new("/tmp/user-runtime")),
            Path::new("/var/lib/dasobjectstore"),
            Path::new("/run/dasobjectstore"),
        )
        .expect("per-user paths derive");
        assert!(matches!(
            user_service_plan(
                &paths,
                "relative/dasobjectstored",
                "/Users/tester/config.json",
                "org.dasobjectstore.dasobjectstored",
            ),
            Err(FolderHostPathError::RelativePath { .. })
        ));
        assert!(matches!(
            user_service_plan(
                &paths,
                "/Users/tester/bin/dasobjectstored",
                "/Users/tester/config.json",
                "org/dasobjectstore",
            ),
            Err(FolderHostPathError::InvalidServiceLabel(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn existing_state_directory_must_be_owned_by_current_user() {
        let root =
            std::env::temp_dir().join(format!("dasobjectstore-state-owner-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("state directory");
        assert!(validate_user_service_state_owner(&root).is_ok());
        let current_uid = unsafe { libc::geteuid() };
        assert!(matches!(
            validate_user_service_state_owner_with_uid(&root, current_uid.saturating_add(1)),
            Err(FolderHostPathError::StateDirectoryNotOwned { .. })
        ));
        std::fs::remove_dir_all(root).ok();
    }
}
