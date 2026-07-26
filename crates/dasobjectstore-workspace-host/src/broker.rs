use crate::config::validate_identity;
use crate::marker::sync_directory;
use crate::quota::{apply_project_quota, verify_project_quota};
use crate::{
    AggregateInspection, AggregatePlan, AggregateRecoveryState, BranchInspection, BranchMarker,
    BranchPlan, BrokerConfig, BrokerRequest, BrokerResponse, RecoveryState, WorkspaceHostOperation,
    PROTOCOL_VERSION,
};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const AGGREGATE_MARKER_FILE: &str = ".dasobjectstore-aggregate.json";
const AGGREGATE_MARKER_SCHEMA: &str = "dasobjectstore.workspace_aggregate.v1";

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
    let (branches, aggregate) = match &request.operation {
        WorkspaceHostOperation::Provision { branches } => (
            provision_all(config, &request.workspace_id, branches)?,
            None,
        ),
        WorkspaceHostOperation::Inspect { branches } => (
            inspect_all(
                config,
                &request.workspace_id,
                branches,
                cfg!(target_os = "linux"),
            )?,
            None,
        ),
        WorkspaceHostOperation::Rollback { branches } => {
            (rollback_all(config, &request.workspace_id, branches)?, None)
        }
        WorkspaceHostOperation::MountAggregate { aggregate } => (
            Vec::new(),
            Some(mount_aggregate(config, &request.workspace_id, aggregate)?),
        ),
        WorkspaceHostOperation::InspectAggregate { aggregate } => (
            Vec::new(),
            Some(inspect_aggregate(config, &request.workspace_id, aggregate)?),
        ),
        WorkspaceHostOperation::UnmountAggregate { aggregate } => (
            Vec::new(),
            Some(unmount_aggregate(config, &request.workspace_id, aggregate)?),
        ),
    };
    Ok(BrokerResponse {
        protocol_version: PROTOCOL_VERSION,
        request_id: request.request_id.clone(),
        workspace_id: request.workspace_id.clone(),
        ok: true,
        error_code: None,
        error_message: None,
        branches,
        aggregate,
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
        WorkspaceHostOperation::MountAggregate { aggregate }
        | WorkspaceHostOperation::InspectAggregate { aggregate }
        | WorkspaceHostOperation::UnmountAggregate { aggregate } => {
            validate_identity("mount_identity", &aggregate.mount_identity)?;
            if aggregate.mount_identity != request.workspace_id {
                return Err(BrokerError::InvalidRequest(
                    "mount identity must equal workspace identity".to_string(),
                ));
            }
            &aggregate.branches
        }
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct AggregateMarker {
    schema: String,
    workspace_id: String,
    mount_identity: String,
    branch_ids: Vec<String>,
    minimum_free_bytes: u64,
}

impl AggregateMarker {
    fn expected(workspace_id: &str, aggregate: &AggregatePlan) -> Self {
        Self {
            schema: AGGREGATE_MARKER_SCHEMA.to_string(),
            workspace_id: workspace_id.to_string(),
            mount_identity: aggregate.mount_identity.clone(),
            branch_ids: aggregate
                .branches
                .iter()
                .map(|branch| branch.branch_id.clone())
                .collect(),
            minimum_free_bytes: aggregate.minimum_free_bytes,
        }
    }
}

fn aggregate_root(config: &BrokerConfig) -> Result<&Path, BrokerError> {
    config
        .aggregate_root
        .as_deref()
        .ok_or_else(|| BrokerError::UnsafeConfig("aggregate_root is not configured".to_string()))
}

fn aggregate_path(
    config: &BrokerConfig,
    aggregate: &AggregatePlan,
) -> Result<PathBuf, BrokerError> {
    Ok(aggregate_root(config)?.join(&aggregate.mount_identity))
}

fn mount_aggregate(
    config: &BrokerConfig,
    workspace_id: &str,
    aggregate: &AggregatePlan,
) -> Result<AggregateInspection, BrokerError> {
    let branches = inspect_all(config, workspace_id, &aggregate.branches, true)?;
    if !branches.iter().all(|branch| {
        branch.state == RecoveryState::Ready && branch.marker_matches && branch.quota_enforced
    }) {
        return Err(BrokerError::UnsafeEntry(
            "aggregate requires every branch to be marker-owned and quota-ready".to_string(),
        ));
    }
    let existing = inspect_aggregate(config, workspace_id, aggregate)?;
    if existing.state == AggregateRecoveryState::Ready {
        return Ok(existing);
    }
    if !matches!(
        existing.state,
        AggregateRecoveryState::Absent | AggregateRecoveryState::MountConflict
    ) || existing.mounted
    {
        return Err(BrokerError::UnsafeEntry(format!(
            "aggregate mount is not safely provisionable: {:?}",
            existing.state
        )));
    }
    #[cfg(not(target_os = "linux"))]
    return Err(BrokerError::Unsupported(
        "mergerfs aggregation requires Linux".to_string(),
    ));
    #[cfg(target_os = "linux")]
    {
        let root = aggregate_root(config)?;
        ensure_real_directory(root)?;
        let target = aggregate_path(config, aggregate)?;
        let marker_path = target.join(AGGREGATE_MARKER_FILE);
        let created = existing.state == AggregateRecoveryState::Absent;
        if created {
            fs::create_dir(&target)
                .map_err(|error| BrokerError::Io("create aggregate mountpoint", error))?;
            let marker = AggregateMarker::expected(workspace_id, aggregate);
            let marker_bytes = serde_json::to_vec_pretty(&marker)
                .map_err(|error| BrokerError::Protocol(error.to_string()))?;
            if let Err(error) = fs::write(&marker_path, marker_bytes)
                .map_err(|error| BrokerError::Io("write aggregate marker", error))
                .and_then(|_| sync_directory(&target))
            {
                let _ = fs::remove_file(&marker_path);
                let _ = fs::remove_dir(&target);
                return Err(error);
            }
        }
        let sources = aggregate
            .branches
            .iter()
            .map(|branch| branch_path(config, branch))
            .collect::<Result<Vec<_>, _>>()?;
        let source_argument = sources
            .iter()
            .map(|path| path.to_string_lossy())
            .collect::<Vec<_>>()
            .join(":");
        let options = mergerfs_options(&aggregate.mount_identity, aggregate.minimum_free_bytes);
        let status = Command::new("/usr/bin/mergerfs")
            .arg("-o")
            .arg(&options)
            .arg(&source_argument)
            .arg(&target)
            .status()
            .map_err(|error| BrokerError::Io("execute mergerfs", error))?;
        if !status.success() {
            if created {
                let _ = fs::remove_file(&marker_path);
                let _ = fs::remove_dir(&target);
            }
            return Err(BrokerError::Unsupported(format!(
                "mergerfs exited with {status}"
            )));
        }
        let inspection = inspect_aggregate(config, workspace_id, aggregate)?;
        if inspection.state != AggregateRecoveryState::Ready {
            return Err(BrokerError::UnsafeEntry(
                "mergerfs returned before the expected aggregate became ready".to_string(),
            ));
        }
        Ok(inspection)
    }
}

fn inspect_aggregate(
    config: &BrokerConfig,
    workspace_id: &str,
    aggregate: &AggregatePlan,
) -> Result<AggregateInspection, BrokerError> {
    let target = aggregate_path(config, aggregate)?;
    let branch_ready = inspect_all(
        config,
        workspace_id,
        &aggregate.branches,
        cfg!(target_os = "linux"),
    )?
    .iter()
    .all(|branch| {
        branch.state == RecoveryState::Ready && branch.marker_matches && branch.quota_enforced
    });
    let mounted = mounted_mergerfs_entry(&target)?;
    if let Some((source, options)) = mounted {
        let expected_sources = aggregate
            .branches
            .iter()
            .map(|branch| branch_path(config, branch))
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .map(|path| path.to_string_lossy())
            .collect::<Vec<_>>()
            .join(":");
        let source_matches = source == expected_sources;
        let options_match = required_options(
            &options,
            &aggregate.mount_identity,
            aggregate.minimum_free_bytes,
        );
        return Ok(AggregateInspection {
            mount_identity: aggregate.mount_identity.clone(),
            state: if branch_ready && source_matches && options_match {
                AggregateRecoveryState::Ready
            } else if !branch_ready {
                AggregateRecoveryState::BranchUnavailable
            } else {
                AggregateRecoveryState::MountConflict
            },
            mounted: true,
            source_matches,
            options_match,
        });
    }
    let state = match fs::symlink_metadata(&target) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            AggregateRecoveryState::Absent
        }
        Err(error) => return Err(BrokerError::Io("stat aggregate mountpoint", error)),
        Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
            AggregateRecoveryState::UnsafeFilesystemEntry
        }
        Ok(_) => match read_aggregate_marker(&target)? {
            None => AggregateRecoveryState::MarkerMissing,
            Some(marker) if marker != AggregateMarker::expected(workspace_id, aggregate) => {
                AggregateRecoveryState::MarkerConflict
            }
            Some(_) => AggregateRecoveryState::MountConflict,
        },
    };
    Ok(AggregateInspection {
        mount_identity: aggregate.mount_identity.clone(),
        state,
        mounted: false,
        source_matches: false,
        options_match: false,
    })
}

fn unmount_aggregate(
    config: &BrokerConfig,
    workspace_id: &str,
    aggregate: &AggregatePlan,
) -> Result<AggregateInspection, BrokerError> {
    let before = inspect_aggregate(config, workspace_id, aggregate)?;
    if before.state == AggregateRecoveryState::Absent {
        return Ok(before);
    }
    if before.state != AggregateRecoveryState::Ready {
        return Err(BrokerError::UnsafeEntry(
            "refusing to unmount an aggregate whose identity is not proven".to_string(),
        ));
    }
    let target = aggregate_path(config, aggregate)?;
    let status = Command::new("/usr/bin/umount")
        .arg(&target)
        .status()
        .map_err(|error| BrokerError::Io("unmount aggregate", error))?;
    if !status.success() {
        return Err(BrokerError::Unsupported(format!(
            "umount exited with {status}"
        )));
    }
    let marker = read_aggregate_marker(&target)?;
    if marker.as_ref() != Some(&AggregateMarker::expected(workspace_id, aggregate)) {
        return Err(BrokerError::MarkerConflict(
            aggregate.mount_identity.clone(),
        ));
    }
    let entries = fs::read_dir(&target)
        .map_err(|error| BrokerError::Io("read aggregate mountpoint", error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| BrokerError::Io("read aggregate mountpoint entry", error))?;
    if entries.len() != 1 || entries[0].file_name() != AGGREGATE_MARKER_FILE {
        return Err(BrokerError::UnsafeEntry(
            "aggregate mountpoint contains unexpected data after unmount".to_string(),
        ));
    }
    fs::remove_file(target.join(AGGREGATE_MARKER_FILE))
        .map_err(|error| BrokerError::Io("remove aggregate marker", error))?;
    fs::remove_dir(&target)
        .map_err(|error| BrokerError::Io("remove aggregate mountpoint", error))?;
    inspect_aggregate(config, workspace_id, aggregate)
}

#[cfg(target_os = "linux")]
fn mergerfs_options(mount_identity: &str, minimum_free_bytes: u64) -> String {
    format!(
        "fsname=dasobjectstore-workspace-{mount_identity},allow_other,default_permissions,category.create=mfs,minfreespace={minimum_free_bytes},inodecalc=path-hash,cache.files=off,dropcacheonclose=true,moveonenospc=mfs"
    )
}

fn required_options(options: &str, mount_identity: &str, minimum_free_bytes: u64) -> bool {
    let required = [
        format!("fsname=dasobjectstore-workspace-{mount_identity}"),
        "allow_other".to_string(),
        "default_permissions".to_string(),
        "category.create=mfs".to_string(),
        format!("minfreespace={minimum_free_bytes}"),
        "inodecalc=path-hash".to_string(),
        "cache.files=off".to_string(),
        "dropcacheonclose=true".to_string(),
        "moveonenospc=mfs".to_string(),
    ];
    required
        .iter()
        .all(|required| options.split(',').any(|option| option == required))
}

fn read_aggregate_marker(path: &Path) -> Result<Option<AggregateMarker>, BrokerError> {
    match fs::read(path.join(AGGREGATE_MARKER_FILE)) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| BrokerError::Protocol(error.to_string())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(BrokerError::Io("read aggregate marker", error)),
    }
}

fn mounted_mergerfs_entry(target: &Path) -> Result<Option<(String, String)>, BrokerError> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = target;
        Ok(None)
    }
    #[cfg(target_os = "linux")]
    {
        let mountinfo = fs::read_to_string("/proc/self/mountinfo")
            .map_err(|error| BrokerError::Io("read mountinfo", error))?;
        let mut mounted = false;
        for line in mountinfo.lines() {
            let Some((left, right)) = line.split_once(" - ") else {
                continue;
            };
            let fields = left.split_whitespace().collect::<Vec<_>>();
            let right = right.split_whitespace().collect::<Vec<_>>();
            if fields.len() < 6 || right.len() < 3 {
                continue;
            }
            if Path::new(&decode_mountinfo(fields[4])) == target
                && (right[0] == "fuse.mergerfs" || right[0] == "fuse")
            {
                mounted = true;
                break;
            }
        }
        if !mounted {
            return Ok(None);
        }
        for entry in
            fs::read_dir("/proc").map_err(|error| BrokerError::Io("read process table", error))?
        {
            let entry = entry.map_err(|error| BrokerError::Io("read process entry", error))?;
            if !entry
                .file_name()
                .to_string_lossy()
                .bytes()
                .all(|byte| byte.is_ascii_digit())
            {
                continue;
            }
            let bytes = match fs::read(entry.path().join("cmdline")) {
                Ok(bytes) => bytes,
                Err(_) => continue,
            };
            let arguments = bytes
                .split(|byte| *byte == 0)
                .filter(|value| !value.is_empty())
                .map(|value| String::from_utf8_lossy(value).into_owned())
                .collect::<Vec<_>>();
            if arguments.len() < 3
                || Path::new(&arguments[0])
                    .file_name()
                    .is_none_or(|name| name != "mergerfs")
                || Path::new(&arguments[2]) != target
            {
                continue;
            }
            let options = arguments
                .windows(2)
                .find_map(|pair| (pair[0] == "-o").then(|| pair[1].clone()))
                .unwrap_or_default();
            return Ok(Some((arguments[1].clone(), options)));
        }
        Err(BrokerError::UnsafeEntry(
            "mergerfs mount exists without a matching mergerfs process identity".to_string(),
        ))
    }
}

#[cfg(target_os = "linux")]
fn decode_mountinfo(value: &str) -> String {
    value
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
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
            aggregate_root: Some(root.join("aggregates")),
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

    #[test]
    fn aggregate_inspection_is_path_redacted_and_marker_conflicts_fail_closed() {
        let (root, config, branch) = fixture();
        let aggregate = AggregatePlan {
            mount_identity: "workspace-a".to_string(),
            branches: vec![branch],
            minimum_free_bytes: 1024,
        };
        let absent = inspect_aggregate(&config, "workspace-a", &aggregate)
            .expect("inspect absent aggregate");
        assert_eq!(absent.state, AggregateRecoveryState::Absent);
        assert_eq!(absent.mount_identity, "workspace-a");

        let target = config
            .aggregate_root
            .as_ref()
            .expect("aggregate root")
            .join("workspace-a");
        fs::create_dir_all(&target).expect("aggregate directory");
        let missing =
            inspect_aggregate(&config, "workspace-a", &aggregate).expect("inspect missing marker");
        assert_eq!(missing.state, AggregateRecoveryState::MarkerMissing);
        fs::write(
            target.join(AGGREGATE_MARKER_FILE),
            br#"{"schema":"wrong","workspace_id":"workspace-a","mount_identity":"workspace-a","branch_ids":["branch-a"],"minimum_free_bytes":1024}"#,
        )
        .expect("conflicting marker");
        let conflict =
            inspect_aggregate(&config, "workspace-a", &aggregate).expect("inspect conflict");
        assert_eq!(conflict.state, AggregateRecoveryState::MarkerConflict);
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn aggregate_request_requires_configured_root_and_matching_identity() {
        let (root, mut config, branch) = fixture();
        config.aggregate_root = None;
        let request = BrokerRequest {
            protocol_version: PROTOCOL_VERSION,
            request_id: "request-a".to_string(),
            workspace_id: "workspace-a".to_string(),
            operation: WorkspaceHostOperation::InspectAggregate {
                aggregate: AggregatePlan {
                    mount_identity: "different".to_string(),
                    branches: vec![branch],
                    minimum_free_bytes: 1024,
                },
            },
        };
        assert!(matches!(
            execute_request(&config, &request),
            Err(BrokerError::InvalidRequest(_))
        ));
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
