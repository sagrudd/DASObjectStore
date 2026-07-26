use crate::broker::{aggregate_root, mounted_mergerfs_entry};
use crate::{
    BrokerConfig, BrokerError, PromotionInspection, PromotionPlan, PromotionRecoveryState,
};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};

const COPY_STEP_BYTES: u64 = 64 * 1024 * 1024;

pub(crate) fn validate_plan(plan: &PromotionPlan) -> Result<(), BrokerError> {
    for (field, value) in [
        ("promotion_id", plan.promotion_id.as_str()),
        ("checkpoint_id", plan.checkpoint_id.as_str()),
        ("ingest_job_id", plan.ingest_job_id.as_str()),
    ] {
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        {
            return Err(BrokerError::InvalidRequest(format!(
                "{field} must be a conservative path-free identity"
            )));
        }
    }
    validate_relative_path(&plan.source_relative_path)?;
    if plan.object_id.trim().is_empty()
        || plan.object_id.len() > 4096
        || plan.expected_size_bytes == 0
        || plan.expected_size_bytes > i64::MAX as u64
        || !valid_sha256(&plan.expected_sha256)
    {
        return Err(BrokerError::InvalidRequest(
            "promotion object evidence is invalid".to_string(),
        ));
    }
    Ok(())
}

pub(crate) fn inspect(
    config: &BrokerConfig,
    workspace_id: &str,
    plan: &PromotionPlan,
) -> Result<PromotionInspection, BrokerError> {
    let source = validate_authority(config, workspace_id, plan)?;
    if fs::metadata(&source)
        .map_err(|error| BrokerError::Io("stat promotion source", error))?
        .len()
        != plan.expected_size_bytes
    {
        return Ok(inspection(
            PromotionRecoveryState::SourceConflict,
            0,
            plan,
            None,
            None,
        ));
    }
    let (payload, partial, relative) = staging_paths(config, plan)?;
    if let Some(size) = safe_file_size(&payload)? {
        if size != plan.expected_size_bytes {
            return Ok(inspection(
                PromotionRecoveryState::DestinationConflict,
                size,
                plan,
                None,
                None,
            ));
        }
        let hash = hash_file(&payload)?;
        return Ok(inspection(
            if hash == normalize_sha256(&plan.expected_sha256) {
                PromotionRecoveryState::Ready
            } else {
                PromotionRecoveryState::DestinationConflict
            },
            size,
            plan,
            Some(hash),
            Some(relative),
        ));
    }
    match safe_file_size(&partial)? {
        Some(size) if size <= plan.expected_size_bytes => Ok(inspection(
            PromotionRecoveryState::Copying,
            size,
            plan,
            None,
            None,
        )),
        Some(size) => Ok(inspection(
            PromotionRecoveryState::DestinationConflict,
            size,
            plan,
            None,
            None,
        )),
        None => Ok(inspection(
            PromotionRecoveryState::Absent,
            0,
            plan,
            None,
            None,
        )),
    }
}

pub(crate) fn copy_step(
    config: &BrokerConfig,
    workspace_id: &str,
    plan: &PromotionPlan,
) -> Result<PromotionInspection, BrokerError> {
    let source = validate_authority(config, workspace_id, plan)?;
    let before = inspect(config, workspace_id, plan)?;
    if before.state == PromotionRecoveryState::Ready {
        return Ok(before);
    }
    if !matches!(
        before.state,
        PromotionRecoveryState::Absent | PromotionRecoveryState::Copying
    ) {
        return Err(BrokerError::UnsafeEntry(format!(
            "promotion is not safely resumable: {:?}",
            before.state
        )));
    }
    let (payload, partial, relative) = staging_paths(config, plan)?;
    let parent = payload
        .parent()
        .ok_or_else(|| BrokerError::UnsafeEntry("promotion staging has no parent".to_string()))?;
    validate_staging_parent(config, parent)?;
    let parent_metadata =
        fs::metadata(parent).map_err(|error| BrokerError::Io("stat promotion staging", error))?;
    let mut input = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&source)
        .map_err(|error| BrokerError::Io("open promotion source", error))?;
    let mut output = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o640)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&partial)
        .map_err(|error| BrokerError::Io("open promotion partial", error))?;
    if unsafe {
        libc::fchown(
            std::os::fd::AsRawFd::as_raw_fd(&output),
            parent_metadata.uid(),
            parent_metadata.gid(),
        )
    } != 0
    {
        return Err(BrokerError::Io(
            "set promotion partial ownership",
            std::io::Error::last_os_error(),
        ));
    }
    let offset = output
        .metadata()
        .map_err(|error| BrokerError::Io("stat promotion partial", error))?
        .len();
    if offset != before.completed_bytes || offset > plan.expected_size_bytes {
        return Err(BrokerError::UnsafeEntry(
            "promotion partial changed during inspection".to_string(),
        ));
    }
    input
        .seek(SeekFrom::Start(offset))
        .map_err(|error| BrokerError::Io("seek promotion source", error))?;
    output
        .seek(SeekFrom::Start(offset))
        .map_err(|error| BrokerError::Io("seek promotion partial", error))?;
    let remaining = plan.expected_size_bytes - offset;
    let copied = std::io::copy(&mut input.take(remaining.min(COPY_STEP_BYTES)), &mut output)
        .map_err(|error| BrokerError::Io("copy promotion extent", error))?;
    if copied == 0 && remaining != 0 {
        return Err(BrokerError::UnsafeEntry(
            "promotion source ended before its checkpointed size".to_string(),
        ));
    }
    output
        .flush()
        .and_then(|_| output.sync_all())
        .map_err(|error| BrokerError::Io("sync promotion partial", error))?;
    let completed = offset + copied;
    if completed < plan.expected_size_bytes {
        return Ok(inspection(
            PromotionRecoveryState::Copying,
            completed,
            plan,
            None,
            None,
        ));
    }
    let hash = hash_file(&partial)?;
    if hash != normalize_sha256(&plan.expected_sha256)
        || hash_file(&source)? != normalize_sha256(&plan.expected_sha256)
    {
        return Err(BrokerError::UnsafeEntry(
            "promotion source or staged checksum changed".to_string(),
        ));
    }
    fs::hard_link(&partial, &payload)
        .map_err(|error| BrokerError::Io("publish promotion without replacement", error))?;
    fs::remove_file(&partial)
        .map_err(|error| BrokerError::Io("remove promotion partial", error))?;
    crate::marker::sync_directory(parent)?;
    Ok(inspection(
        PromotionRecoveryState::Ready,
        completed,
        plan,
        Some(hash),
        Some(relative),
    ))
}

fn validate_authority(
    config: &BrokerConfig,
    workspace_id: &str,
    plan: &PromotionPlan,
) -> Result<PathBuf, BrokerError> {
    validate_plan(plan)?;
    let aggregate = aggregate_root(config)?.join(workspace_id);
    if !mounted_mergerfs_entry(&aggregate)?.is_some_and(|(_, options)| {
        options
            .split(',')
            .any(|value| value == format!("fsname=dasobjectstore-workspace-{workspace_id}"))
    }) {
        return Err(BrokerError::UnsafeEntry(
            "workspace aggregate is not mounted with its expected identity".to_string(),
        ));
    }
    let metadata_path = config.live_metadata_path.as_deref().ok_or_else(|| {
        BrokerError::UnsafeConfig("live_metadata_path is not configured".to_string())
    })?;
    let connection = Connection::open_with_flags(metadata_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| BrokerError::Protocol(error.to_string()))?;
    let evidence: Option<(i64, String)> = connection
        .query_row(
            "SELECT m.size_bytes, m.sha256
             FROM compute_workspace_promotions p
             JOIN compute_workspace_promotion_members m USING (promotion_id)
             JOIN compute_workspaces w USING (workspace_id)
             WHERE p.promotion_id = ?1 AND p.workspace_id = ?2
               AND p.checkpoint_id = ?3 AND m.object_id = ?4
               AND m.source_relative_path = ?5
               AND p.state IN ('registered', 'publishing')
               AND m.state IN ('pending', 'staging')
               AND w.state = 'promotion_pending'",
            rusqlite::params![
                plan.promotion_id,
                workspace_id,
                plan.checkpoint_id,
                plan.object_id,
                plan.source_relative_path,
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| BrokerError::Protocol(error.to_string()))?;
    if !evidence.is_some_and(|(size, sha256)| {
        size == plan.expected_size_bytes as i64
            && normalize_sha256(&sha256) == normalize_sha256(&plan.expected_sha256)
    }) {
        return Err(BrokerError::UnsafeEntry(
            "promotion plan does not match live checkpoint authority".to_string(),
        ));
    }
    let source = aggregate.join(&plan.source_relative_path);
    ensure_safe_file_under(&aggregate, &source)?;
    Ok(source)
}

fn staging_paths(
    config: &BrokerConfig,
    plan: &PromotionPlan,
) -> Result<(PathBuf, PathBuf, String), BrokerError> {
    let metadata = config.live_metadata_path.as_deref().ok_or_else(|| {
        BrokerError::UnsafeConfig("live_metadata_path is not configured".to_string())
    })?;
    let metadata_root = metadata
        .parent()
        .ok_or_else(|| BrokerError::UnsafeConfig("live metadata has no parent".to_string()))?;
    if metadata.file_name().and_then(|value| value.to_str()) != Some("live.sqlite")
        || metadata_root.file_name().and_then(|value| value.to_str()) != Some(".dasobjectstore")
    {
        return Err(BrokerError::UnsafeConfig(
            "live metadata path cannot derive managed SSD root".to_string(),
        ));
    }
    let relative = format!(".dasobjectstore/ingest/jobs/{}/payload", plan.ingest_job_id);
    let payload = metadata_root
        .join("ingest/jobs")
        .join(&plan.ingest_job_id)
        .join("payload");
    let partial = payload.with_extension("workspace-promote.partial");
    Ok((payload, partial, relative))
}

fn validate_staging_parent(config: &BrokerConfig, parent: &Path) -> Result<(), BrokerError> {
    let metadata = config
        .live_metadata_path
        .as_deref()
        .and_then(Path::parent)
        .ok_or_else(|| BrokerError::UnsafeConfig("live metadata has no parent".to_string()))?;
    let jobs = metadata.join("ingest/jobs");
    ensure_safe_directory_tree(&jobs, parent)
}

fn ensure_safe_directory_tree(root: &Path, target: &Path) -> Result<(), BrokerError> {
    if !target.starts_with(root) {
        return Err(BrokerError::UnsafeEntry(
            "promotion staging escaped managed ingest jobs".to_string(),
        ));
    }
    let mut current = root.to_path_buf();
    for component in target
        .strip_prefix(root)
        .map_err(|_| BrokerError::UnsafeEntry("promotion staging escaped root".to_string()))?
        .components()
    {
        let Component::Normal(component) = component else {
            return Err(BrokerError::UnsafeEntry(
                "unsafe promotion staging component".to_string(),
            ));
        };
        current.push(component);
        let metadata = fs::symlink_metadata(&current)
            .map_err(|error| BrokerError::Io("stat promotion staging component", error))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(BrokerError::UnsafeEntry(
                "promotion staging contains unsafe component".to_string(),
            ));
        }
    }
    Ok(())
}

fn ensure_safe_file_under(root: &Path, target: &Path) -> Result<(), BrokerError> {
    let parent = target
        .parent()
        .ok_or_else(|| BrokerError::UnsafeEntry("promotion source has no parent".to_string()))?;
    ensure_safe_directory_tree(root, parent)?;
    let metadata = fs::symlink_metadata(target)
        .map_err(|error| BrokerError::Io("stat promotion source", error))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(BrokerError::UnsafeEntry(
            "promotion source must be a regular non-symlink file".to_string(),
        ));
    }
    Ok(())
}

fn safe_file_size(path: &Path) -> Result<Option<u64>, BrokerError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(BrokerError::Io("stat promotion staging file", error)),
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            Ok(Some(metadata.len()))
        }
        Ok(_) => Err(BrokerError::UnsafeEntry(
            "promotion staging entry is not a regular file".to_string(),
        )),
    }
}

fn hash_file(path: &Path) -> Result<String, BrokerError> {
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|error| BrokerError::Io("open promotion file", error))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| BrokerError::Io("hash promotion file", error))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn inspection(
    state: PromotionRecoveryState,
    completed_bytes: u64,
    plan: &PromotionPlan,
    observed_sha256: Option<String>,
    staged_relative_path: Option<String>,
) -> PromotionInspection {
    PromotionInspection {
        state,
        completed_bytes,
        expected_size_bytes: plan.expected_size_bytes,
        observed_sha256,
        staged_relative_path,
    }
}

fn validate_relative_path(value: &str) -> Result<(), BrokerError> {
    let path = Path::new(value);
    if value.is_empty()
        || value.len() > 4096
        || path.is_absolute()
        || !path.components().all(
            |component| matches!(component, Component::Normal(part) if part != "." && part != ".."),
        )
    {
        return Err(BrokerError::InvalidRequest(
            "promotion source must be a conservative relative path".to_string(),
        ));
    }
    Ok(())
}

fn valid_sha256(value: &str) -> bool {
    let value = value.strip_prefix("sha256:").unwrap_or(value);
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn normalize_sha256(value: &str) -> String {
    format!(
        "sha256:{}",
        value
            .strip_prefix("sha256:")
            .unwrap_or(value)
            .to_ascii_lowercase()
    )
}

#[cfg(test)]
mod tests {
    use super::validate_plan;
    use crate::PromotionPlan;

    #[test]
    fn promotion_plan_is_path_free_and_evidence_bounded() {
        let valid = PromotionPlan {
            promotion_id: "promotion-a".to_string(),
            checkpoint_id: "checkpoint-a".to_string(),
            source_relative_path: "outputs/result.bin".to_string(),
            object_id: "store/result.bin".to_string(),
            ingest_job_id: "workspace-promote-deadbeef".to_string(),
            expected_size_bytes: 10,
            expected_sha256: format!("sha256:{}", "a".repeat(64)),
        };
        assert!(validate_plan(&valid).is_ok());
        let mut escaped = valid.clone();
        escaped.source_relative_path = "../result.bin".to_string();
        assert!(validate_plan(&escaped).is_err());
        let mut unsafe_job = valid;
        unsafe_job.ingest_job_id = "workspace/promote".to_string();
        assert!(validate_plan(&unsafe_job).is_err());
    }
}
