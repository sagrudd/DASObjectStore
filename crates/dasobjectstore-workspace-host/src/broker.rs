use crate::config::validate_identity;
use crate::marker::sync_directory;
use crate::quota::{apply_project_quota, verify_project_quota};
use crate::{
    BranchInspection, BranchMarker, BranchPlan, BrokerConfig, BrokerRequest, BrokerResponse,
    RecoveryState, WorkspaceHostOperation, PROTOCOL_VERSION,
};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum BrokerError {
    Io(&'static str, std::io::Error),
    Protocol(String),
    InvalidRequest(String),
    UnsafeConfig(String),
    UnsafeEntry(String),
    MarkerConflict(String),
    Quota(String),
    Unsupported(String),
}

impl fmt::Display for BrokerError {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(context, error) => write!(output, "{context}: {error}"),
            Self::Protocol(message) => write!(output, "protocol error: {message}"),
            Self::InvalidRequest(message) => write!(output, "invalid request: {message}"),
            Self::UnsafeConfig(message) => write!(output, "unsafe configuration: {message}"),
            Self::UnsafeEntry(message) => write!(output, "unsafe filesystem entry: {message}"),
            Self::MarkerConflict(message) => write!(output, "marker conflict: {message}"),
            Self::Quota(message) => write!(output, "project quota error: {message}"),
            Self::Unsupported(message) => write!(output, "unsupported operation: {message}"),
        }
    }
}

impl std::error::Error for BrokerError {}

pub fn execute_request(
    config: &BrokerConfig,
    request: &BrokerRequest,
) -> Result<BrokerResponse, BrokerError> {
    validate_request(config, request)?;
    let branches = match &request.operation {
        WorkspaceHostOperation::Provision { branches } => {
            provision_all(config, &request.workspace_id, branches)?
        }
        WorkspaceHostOperation::Inspect { branches } => inspect_all(
            config,
            &request.workspace_id,
            branches,
            cfg!(target_os = "linux"),
        )?,
        WorkspaceHostOperation::Rollback { branches } => {
            rollback_all(config, &request.workspace_id, branches)?
        }
    };
    Ok(BrokerResponse {
        protocol_version: PROTOCOL_VERSION,
        request_id: request.request_id.clone(),
        workspace_id: request.workspace_id.clone(),
        ok: true,
        error_code: None,
        error_message: None,
        branches,
    })
}

fn validate_request(config: &BrokerConfig, request: &BrokerRequest) -> Result<(), BrokerError> {
    if request.protocol_version != PROTOCOL_VERSION {
        return Err(BrokerError::InvalidRequest(
            "unsupported protocol version".to_string(),
        ));
    }
    validate_identity("request_id", &request.request_id)?;
    validate_identity("workspace_id", &request.workspace_id)?;
    let branches = match &request.operation {
        WorkspaceHostOperation::Provision { branches }
        | WorkspaceHostOperation::Inspect { branches }
        | WorkspaceHostOperation::Rollback { branches } => branches,
    };
    if branches.is_empty() || branches.len() > 256 {
        return Err(BrokerError::InvalidRequest(
            "branch count must be between 1 and 256".to_string(),
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    for branch in branches {
        validate_identity("disk_id", &branch.disk_id)?;
        validate_identity("branch_id", &branch.branch_id)?;
        if branch.project_id < 1000 || branch.quota_bytes == 0 {
            return Err(BrokerError::InvalidRequest(
                "project_id must be >= 1000 and quota_bytes must be positive".to_string(),
            ));
        }
        if !config.disks.contains_key(&branch.disk_id) {
            return Err(BrokerError::InvalidRequest(format!(
                "disk {} is not configured",
                branch.disk_id
            )));
        }
        if !seen.insert((&branch.disk_id, &branch.branch_id)) {
            return Err(BrokerError::InvalidRequest(
                "duplicate branch identity".to_string(),
            ));
        }
    }
    Ok(())
}

fn branch_path(config: &BrokerConfig, branch: &BranchPlan) -> Result<PathBuf, BrokerError> {
    let disk = config
        .disks
        .get(&branch.disk_id)
        .ok_or_else(|| BrokerError::InvalidRequest("unknown disk".to_string()))?;
    checked_child(&disk.root, &disk.workspace_directory).map(|root| root.join(&branch.branch_id))
}

fn checked_child(root: &Path, child: &str) -> Result<PathBuf, BrokerError> {
    let metadata =
        fs::symlink_metadata(root).map_err(|error| BrokerError::Io("stat managed root", error))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(BrokerError::UnsafeEntry(
            "managed root changed or became a symlink".to_string(),
        ));
    }
    Ok(root.join(child))
}

fn provision_all(
    config: &BrokerConfig,
    workspace_id: &str,
    branches: &[BranchPlan],
) -> Result<Vec<BranchInspection>, BrokerError> {
    let mut created = Vec::new();
    for branch in branches {
        match provision_one(config, workspace_id, branch) {
            Ok(was_created) => {
                if was_created {
                    created.push(branch.clone());
                }
            }
            Err(error) => {
                for created_branch in created.iter().rev() {
                    let _ = rollback_one(config, workspace_id, created_branch);
                }
                return Err(error);
            }
        }
    }
    inspect_all(config, workspace_id, branches, true)
}

fn provision_one(
    config: &BrokerConfig,
    workspace_id: &str,
    branch: &BranchPlan,
) -> Result<bool, BrokerError> {
    let disk = &config.disks[&branch.disk_id];
    let workspace_root = checked_child(&disk.root, &disk.workspace_directory)?;
    ensure_real_directory(&workspace_root)?;
    let directory = branch_path(config, branch)?;
    match fs::symlink_metadata(&directory) {
        Ok(metadata) => {
            if !metadata.is_dir() || metadata.file_type().is_symlink() {
                return Err(BrokerError::UnsafeEntry(format!(
                    "branch {} is not a real directory",
                    branch.branch_id
                )));
            }
            let expected = BranchMarker::expected(workspace_id, branch);
            if BranchMarker::read(&directory)?.as_ref() != Some(&expected) {
                return Err(BrokerError::MarkerConflict(branch.branch_id.clone()));
            }
            verify_project_quota(&directory, branch.project_id)?;
            Ok(false)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(&directory)
                .map_err(|error| BrokerError::Io("create workspace branch", error))?;
            if let Err(error) =
                apply_project_quota(&directory, branch.project_id, branch.quota_bytes).and_then(
                    |_| BranchMarker::expected(workspace_id, branch).create_exclusive(&directory),
                )
            {
                let _ = fs::remove_dir_all(&directory);
                return Err(error);
            }
            sync_directory(&workspace_root)?;
            Ok(true)
        }
        Err(error) => Err(BrokerError::Io("stat workspace branch", error)),
    }
}

fn ensure_real_directory(path: &Path) -> Result<(), BrokerError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(()),
        Ok(_) => Err(BrokerError::UnsafeEntry(
            "workspace namespace is not a real directory".to_string(),
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir(path)
            .map_err(|error| BrokerError::Io("create workspace namespace", error)),
        Err(error) => Err(BrokerError::Io("stat workspace namespace", error)),
    }
}

fn inspect_all(
    config: &BrokerConfig,
    workspace_id: &str,
    branches: &[BranchPlan],
    inspect_quota: bool,
) -> Result<Vec<BranchInspection>, BrokerError> {
    branches
        .iter()
        .map(|branch| inspect_one(config, workspace_id, branch, inspect_quota))
        .collect()
}

fn inspect_one(
    config: &BrokerConfig,
    workspace_id: &str,
    branch: &BranchPlan,
    inspect_quota: bool,
) -> Result<BranchInspection, BrokerError> {
    let directory = branch_path(config, branch)?;
    let expected = BranchMarker::expected(workspace_id, branch);
    let (state, marker_matches, quota_enforced) = match fs::symlink_metadata(&directory) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            (RecoveryState::Absent, false, false)
        }
        Err(error) => return Err(BrokerError::Io("stat workspace branch", error)),
        Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
            (RecoveryState::UnsafeFilesystemEntry, false, false)
        }
        Ok(_) => match BranchMarker::read(&directory) {
            Err(BrokerError::UnsafeEntry(_)) => {
                (RecoveryState::UnsafeFilesystemEntry, false, false)
            }
            Err(error) => return Err(error),
            Ok(None) => (RecoveryState::MarkerMissing, false, false),
            Ok(Some(marker)) if marker != expected => (RecoveryState::MarkerConflict, false, false),
            Ok(Some(_)) => {
                let quota =
                    !inspect_quota || verify_project_quota(&directory, branch.project_id).is_ok();
                (
                    if quota {
                        RecoveryState::Ready
                    } else {
                        RecoveryState::QuotaMissing
                    },
                    true,
                    quota,
                )
            }
        },
    };
    Ok(BranchInspection {
        disk_id: branch.disk_id.clone(),
        branch_id: branch.branch_id.clone(),
        state,
        marker_matches,
        quota_enforced,
    })
}

fn rollback_all(
    config: &BrokerConfig,
    workspace_id: &str,
    branches: &[BranchPlan],
) -> Result<Vec<BranchInspection>, BrokerError> {
    for branch in branches.iter().rev() {
        rollback_one(config, workspace_id, branch)?;
    }
    inspect_all(config, workspace_id, branches, false)
}

fn rollback_one(
    config: &BrokerConfig,
    workspace_id: &str,
    branch: &BranchPlan,
) -> Result<(), BrokerError> {
    let directory = branch_path(config, branch)?;
    match fs::symlink_metadata(&directory) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(BrokerError::Io("stat rollback branch", error)),
        Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
            return Err(BrokerError::UnsafeEntry(
                "rollback target is not a real directory".to_string(),
            ));
        }
        Ok(_) => {}
    }
    let expected = BranchMarker::expected(workspace_id, branch);
    if BranchMarker::read(&directory)?.as_ref() != Some(&expected) {
        return Err(BrokerError::MarkerConflict(branch.branch_id.clone()));
    }
    let entries = fs::read_dir(&directory)
        .map_err(|error| BrokerError::Io("read rollback branch", error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| BrokerError::Io("read rollback branch entry", error))?;
    if entries.len() != 1 || entries[0].file_name() != crate::MARKER_FILE {
        return Err(BrokerError::UnsafeEntry(
            "rollback refuses a branch containing workspace data".to_string(),
        ));
    }
    fs::remove_file(directory.join(crate::MARKER_FILE))
        .map_err(|error| BrokerError::Io("remove branch marker", error))?;
    fs::remove_dir(&directory).map_err(|error| BrokerError::Io("remove empty branch", error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn fixture() -> (PathBuf, BrokerConfig, BranchPlan) {
        let root = std::env::temp_dir().join(format!(
            "dos-workspace-host-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos(),
            FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).expect("root");
        let config = BrokerConfig {
            schema_version: 1,
            disks: BTreeMap::from([(
                "disk-a".to_string(),
                crate::ManagedDiskRoot {
                    root: root.clone(),
                    workspace_directory: ".workspaces".to_string(),
                },
            )]),
        };
        let branch = BranchPlan {
            disk_id: "disk-a".to_string(),
            branch_id: "branch-a".to_string(),
            project_id: 1001,
            quota_bytes: 4096,
        };
        (root, config, branch)
    }

    #[test]
    fn inspect_reports_absent_without_creating_paths() {
        let (root, config, branch) = fixture();
        let inspection = inspect_one(&config, "workspace-a", &branch, false).expect("inspect");
        assert_eq!(inspection.state, RecoveryState::Absent);
        assert!(!root.join(".workspaces").exists());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn marker_conflict_and_nonempty_rollback_fail_closed() {
        let (root, config, branch) = fixture();
        let directory = root.join(".workspaces").join("branch-a");
        fs::create_dir_all(&directory).expect("branch");
        BranchMarker::expected("other-workspace", &branch)
            .create_exclusive(&directory)
            .expect("marker");
        assert!(matches!(
            rollback_one(&config, "workspace-a", &branch),
            Err(BrokerError::MarkerConflict(_))
        ));
        fs::remove_file(directory.join(crate::MARKER_FILE)).expect("remove marker");
        BranchMarker::expected("workspace-a", &branch)
            .create_exclusive(&directory)
            .expect("marker");
        fs::write(directory.join("payload"), b"data").expect("payload");
        assert!(matches!(
            rollback_one(&config, "workspace-a", &branch),
            Err(BrokerError::UnsafeEntry(_))
        ));
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_branch_is_never_followed() {
        use std::os::unix::fs::symlink;
        let (root, config, branch) = fixture();
        fs::create_dir(root.join(".workspaces")).expect("namespace");
        symlink("/tmp", root.join(".workspaces/branch-a")).expect("symlink");
        let inspection = inspect_one(&config, "workspace-a", &branch, false).expect("inspect");
        assert_eq!(inspection.state, RecoveryState::UnsafeFilesystemEntry);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn invalid_or_unknown_identifiers_are_rejected_before_filesystem_work() {
        let (root, config, branch) = fixture();
        let request = BrokerRequest {
            protocol_version: PROTOCOL_VERSION,
            request_id: "request-a".to_string(),
            workspace_id: "../escape".to_string(),
            operation: WorkspaceHostOperation::Inspect {
                branches: vec![branch.clone()],
            },
        };
        assert!(matches!(
            execute_request(&config, &request),
            Err(BrokerError::InvalidRequest(_))
        ));
        let mut unknown = branch;
        unknown.disk_id = "unknown".to_string();
        let request = BrokerRequest {
            protocol_version: PROTOCOL_VERSION,
            request_id: "request-b".to_string(),
            workspace_id: "workspace-a".to_string(),
            operation: WorkspaceHostOperation::Inspect {
                branches: vec![unknown],
            },
        };
        assert!(matches!(
            execute_request(&config, &request),
            Err(BrokerError::InvalidRequest(_))
        ));
        assert!(!root.join(".workspaces").exists());
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn quota_failure_rolls_back_new_empty_branch() {
        let (root, config, branch) = fixture();
        assert!(matches!(
            provision_one(&config, "workspace-a", &branch),
            Err(BrokerError::Unsupported(_))
        ));
        assert!(!root.join(".workspaces/branch-a").exists());
        fs::remove_dir_all(root).expect("cleanup");
    }
}
