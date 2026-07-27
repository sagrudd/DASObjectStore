use super::*;

pub(super) fn branch_path(
    config: &BrokerConfig,
    branch: &BranchPlan,
) -> Result<PathBuf, BrokerError> {
    let disk = config
        .disks
        .get(&branch.disk_id)
        .ok_or_else(|| BrokerError::InvalidRequest("unknown disk".to_string()))?;
    checked_child(&disk.root, &disk.workspace_directory).map(|root| root.join(&branch.branch_id))
}

pub(super) fn checked_child(root: &Path, child: &str) -> Result<PathBuf, BrokerError> {
    let metadata =
        fs::symlink_metadata(root).map_err(|error| BrokerError::Io("stat managed root", error))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(BrokerError::UnsafeEntry(
            "managed root changed or became a symlink".to_string(),
        ));
    }
    Ok(root.join(child))
}

pub(super) fn provision_all(
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

pub(super) fn provision_one(
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

pub(super) fn ensure_real_directory(path: &Path) -> Result<(), BrokerError> {
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

pub(super) fn inspect_all(
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

pub(super) fn inspect_one(
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

pub(super) fn rollback_all(
    config: &BrokerConfig,
    workspace_id: &str,
    branches: &[BranchPlan],
) -> Result<Vec<BranchInspection>, BrokerError> {
    for branch in branches.iter().rev() {
        rollback_one(config, workspace_id, branch)?;
    }
    inspect_all(config, workspace_id, branches, false)
}

pub(super) fn rollback_one(
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

pub(super) fn cleanup_all(
    config: &BrokerConfig,
    workspace_id: &str,
    branches: &[BranchPlan],
) -> Result<Vec<BranchInspection>, BrokerError> {
    for branch in branches {
        validate_cleanup_one(config, workspace_id, branch)?;
    }
    for branch in branches.iter().rev() {
        cleanup_one(config, workspace_id, branch)?;
    }
    inspect_all(config, workspace_id, branches, false)
}

pub(super) fn cleanup_one(
    config: &BrokerConfig,
    workspace_id: &str,
    branch: &BranchPlan,
) -> Result<(), BrokerError> {
    validate_cleanup_one(config, workspace_id, branch)?;
    let directory = branch_path(config, branch)?;
    if !directory.exists() {
        return Ok(());
    }
    remove_owned_tree_contents(&directory)?;
    fs::remove_dir(&directory).map_err(|error| BrokerError::Io("remove cleanup branch", error))
}

pub(super) fn validate_cleanup_one(
    config: &BrokerConfig,
    workspace_id: &str,
    branch: &BranchPlan,
) -> Result<(), BrokerError> {
    let directory = branch_path(config, branch)?;
    match fs::symlink_metadata(&directory) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(BrokerError::Io("stat cleanup branch", error)),
        Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
            return Err(BrokerError::UnsafeEntry(
                "cleanup target is not a real directory".to_string(),
            ));
        }
        Ok(_) => {}
    }
    if BranchMarker::read(&directory)?.as_ref()
        != Some(&BranchMarker::expected(workspace_id, branch))
    {
        return Err(BrokerError::MarkerConflict(branch.branch_id.clone()));
    }
    validate_owned_tree_contents(&directory)?;
    Ok(())
}

pub(super) fn validate_owned_tree_contents(directory: &Path) -> Result<(), BrokerError> {
    let entries = fs::read_dir(directory)
        .map_err(|error| BrokerError::Io("read cleanup directory", error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| BrokerError::Io("read cleanup entry", error))?;
    for entry in entries {
        let metadata = fs::symlink_metadata(entry.path())
            .map_err(|error| BrokerError::Io("stat cleanup entry", error))?;
        if metadata.file_type().is_symlink() {
            return Err(BrokerError::UnsafeEntry(
                "cleanup refuses symbolic links".to_string(),
            ));
        }
        if metadata.is_dir() {
            validate_owned_tree_contents(&entry.path())?;
        } else if !metadata.is_file() {
            return Err(BrokerError::UnsafeEntry(
                "cleanup refuses non-file filesystem entries".to_string(),
            ));
        }
    }
    Ok(())
}

pub(super) fn remove_owned_tree_contents(directory: &Path) -> Result<(), BrokerError> {
    let entries = fs::read_dir(directory)
        .map_err(|error| BrokerError::Io("read cleanup directory", error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| BrokerError::Io("read cleanup entry", error))?;
    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| BrokerError::Io("stat cleanup entry", error))?;
        if metadata.file_type().is_symlink() {
            return Err(BrokerError::UnsafeEntry(
                "cleanup refuses symbolic links".to_string(),
            ));
        }
        if metadata.is_dir() {
            remove_owned_tree_contents(&path)?;
            fs::remove_dir(&path)
                .map_err(|error| BrokerError::Io("remove cleanup directory", error))?;
        } else if metadata.is_file() {
            fs::remove_file(&path)
                .map_err(|error| BrokerError::Io("remove cleanup file", error))?;
        } else {
            return Err(BrokerError::UnsafeEntry(
                "cleanup refuses non-file filesystem entries".to_string(),
            ));
        }
    }
    Ok(())
}
