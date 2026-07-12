//! Host-mode path derivation for local folder ObjectStores.

use dasobjectstore_core::deployment::HostMode;
use std::fmt::{self, Display};
use std::path::{Path, PathBuf};

const APP_DIR: &str = "dasobjectstore";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FolderHostPaths {
    pub state_dir: PathBuf,
    pub runtime_dir: Option<PathBuf>,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FolderHostPathError {
    MissingHomeDirectory,
    MissingRuntimeDirectory,
    BlankSocketName,
    SocketPathTooLong(PathBuf),
    RelativePath { field: &'static str, path: PathBuf },
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
            Self::RelativePath { field, path } => {
                write!(formatter, "{field} must be absolute: {}", path.display())
            }
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
}
