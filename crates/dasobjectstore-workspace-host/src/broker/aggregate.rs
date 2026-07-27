use super::*;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(super) struct AggregateMarker {
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

pub(crate) fn aggregate_root(config: &BrokerConfig) -> Result<&Path, BrokerError> {
    config
        .aggregate_root
        .as_deref()
        .ok_or_else(|| BrokerError::UnsafeConfig("aggregate_root is not configured".to_string()))
}

pub(super) fn aggregate_path(
    config: &BrokerConfig,
    aggregate: &AggregatePlan,
) -> Result<PathBuf, BrokerError> {
    Ok(aggregate_root(config)?.join(&aggregate.mount_identity))
}

pub(super) fn mount_aggregate(
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

pub(super) fn inspect_aggregate(
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

pub(super) fn unmount_aggregate(
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
pub(super) fn mergerfs_options(mount_identity: &str, minimum_free_bytes: u64) -> String {
    format!(
        "fsname=dasobjectstore-workspace-{mount_identity},allow_other,default_permissions,category.create=mfs,minfreespace={minimum_free_bytes},inodecalc=path-hash,cache.files=off,dropcacheonclose=true,moveonenospc=mfs"
    )
}

pub(super) fn required_options(
    options: &str,
    mount_identity: &str,
    minimum_free_bytes: u64,
) -> bool {
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

pub(super) fn read_aggregate_marker(path: &Path) -> Result<Option<AggregateMarker>, BrokerError> {
    match fs::read(path.join(AGGREGATE_MARKER_FILE)) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| BrokerError::Protocol(error.to_string())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(BrokerError::Io("read aggregate marker", error)),
    }
}

pub(crate) fn mounted_mergerfs_entry(
    target: &Path,
) -> Result<Option<(String, String)>, BrokerError> {
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
            if let Some(identity) = parse_mergerfs_process(&arguments, target) {
                return Ok(Some(identity));
            }
        }
        Err(BrokerError::UnsafeEntry(
            "mergerfs mount exists without a matching mergerfs process identity".to_string(),
        ))
    }
}

#[cfg(target_os = "linux")]
pub(super) fn parse_mergerfs_process(
    arguments: &[String],
    target: &Path,
) -> Option<(String, String)> {
    if arguments.len() < 3
        || Path::new(&arguments[0])
            .file_name()
            .is_none_or(|name| name != "mergerfs")
        || Path::new(arguments.last()?) != target
    {
        return None;
    }
    let options = arguments
        .windows(2)
        .find_map(|pair| (pair[0] == "-o").then(|| pair[1].clone()))?;
    let source_index = arguments.len().checked_sub(2)?;
    let sources = arguments.get(source_index)?;
    (!sources.starts_with('-')).then(|| (sources.clone(), options))
}

#[cfg(target_os = "linux")]
pub(super) fn decode_mountinfo(value: &str) -> String {
    value
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}
