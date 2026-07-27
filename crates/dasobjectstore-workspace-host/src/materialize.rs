use crate::broker::{aggregate_root, mounted_mergerfs_entry};
use crate::{
    BrokerConfig, BrokerError, MaterializationInspection, MaterializationPlan,
    MaterializationRecoveryState,
};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};

const COPY_STEP_BYTES: u64 = 64 * 1024 * 1024;

pub(crate) fn validate_plan(plan: &MaterializationPlan) -> Result<(), BrokerError> {
    validate_opaque_id("source_object_id", &plan.source_object_id)?;
    validate_opaque_id("source_placement_id", &plan.source_placement_id)?;
    validate_relative_path(&plan.destination_relative_path)?;
    if plan.expected_size_bytes == 0 || plan.expected_size_bytes > i64::MAX as u64 {
        return Err(BrokerError::InvalidRequest(
            "materialization size must be positive".to_string(),
        ));
    }
    normalize_sha256(&plan.expected_sha256)?;
    Ok(())
}

pub(crate) fn inspect(
    config: &BrokerConfig,
    workspace_id: &str,
    plan: &MaterializationPlan,
) -> Result<MaterializationInspection, BrokerError> {
    validate_authority(config, workspace_id, plan)?;
    let destination = destination_path(config, workspace_id, plan)?;
    let partial = partial_path(&destination, plan)?;
    match safe_file_size(&destination)? {
        Some(size) => {
            if size != plan.expected_size_bytes {
                return Ok(inspection(
                    MaterializationRecoveryState::DestinationConflict,
                    size,
                    plan,
                    None,
                ));
            }
            let hash = hash_file(&destination)?;
            if hash != normalize_sha256(&plan.expected_sha256)? {
                return Ok(inspection(
                    MaterializationRecoveryState::DestinationConflict,
                    size,
                    plan,
                    Some(hash),
                ));
            }
            Ok(inspection(
                MaterializationRecoveryState::Ready,
                size,
                plan,
                Some(hash),
            ))
        }
        None => match safe_file_size(&partial)? {
            Some(size) if size <= plan.expected_size_bytes => Ok(inspection(
                MaterializationRecoveryState::Copying,
                size,
                plan,
                None,
            )),
            Some(size) => Ok(inspection(
                MaterializationRecoveryState::DestinationConflict,
                size,
                plan,
                None,
            )),
            None => Ok(inspection(
                MaterializationRecoveryState::Absent,
                0,
                plan,
                None,
            )),
        },
    }
}

pub(crate) fn copy_step(
    config: &BrokerConfig,
    workspace_id: &str,
    plan: &MaterializationPlan,
) -> Result<MaterializationInspection, BrokerError> {
    let source = validate_authority(config, workspace_id, plan)?;
    let before = inspect(config, workspace_id, plan)?;
    if before.state == MaterializationRecoveryState::Ready {
        let destination = destination_path(config, workspace_id, plan)?;
        let partial = partial_path(&destination, plan)?;
        if let Some(size) = safe_file_size(&partial)? {
            if size != plan.expected_size_bytes
                || hash_file(&partial)? != normalize_sha256(&plan.expected_sha256)?
            {
                return Err(BrokerError::UnsafeEntry(
                    "completed materialization retained a conflicting partial".to_string(),
                ));
            }
            fs::remove_file(&partial).map_err(|error| {
                BrokerError::Io("remove completed materialization partial", error)
            })?;
            crate::marker::sync_directory(destination.parent().ok_or_else(|| {
                BrokerError::InvalidRequest("destination has no parent".to_string())
            })?)?;
        }
        return Ok(before);
    }
    if !matches!(
        before.state,
        MaterializationRecoveryState::Absent | MaterializationRecoveryState::Copying
    ) {
        return Err(BrokerError::UnsafeEntry(format!(
            "materialization is not safely resumable: {:?}",
            before.state
        )));
    }
    let destination = destination_path(config, workspace_id, plan)?;
    let parent = destination.parent().ok_or_else(|| {
        BrokerError::InvalidRequest("materialization destination has no parent".to_string())
    })?;
    ensure_safe_directory_tree(aggregate_root(config)?, parent)?;
    let partial = partial_path(&destination, plan)?;
    copy_extent(
        &source,
        &destination,
        &partial,
        before.completed_bytes,
        plan,
    )
}

fn copy_extent(
    source: &Path,
    destination: &Path,
    partial: &Path,
    inspected_bytes: u64,
    plan: &MaterializationPlan,
) -> Result<MaterializationInspection, BrokerError> {
    let mut input = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(source)
        .map_err(|error| BrokerError::Io("open materialization source", error))?;
    let mut output = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o640)
        .custom_flags(libc::O_NOFOLLOW)
        .open(partial)
        .map_err(|error| BrokerError::Io("open materialization partial", error))?;
    let offset = output
        .metadata()
        .map_err(|error| BrokerError::Io("stat materialization partial", error))?
        .len();
    if offset != inspected_bytes || offset > plan.expected_size_bytes {
        return Err(BrokerError::UnsafeEntry(
            "materialization partial changed during inspection".to_string(),
        ));
    }
    input
        .seek(SeekFrom::Start(offset))
        .map_err(|error| BrokerError::Io("seek materialization source", error))?;
    output
        .seek(SeekFrom::Start(offset))
        .map_err(|error| BrokerError::Io("seek materialization partial", error))?;
    let remaining = plan.expected_size_bytes - offset;
    let copied = std::io::copy(&mut input.take(remaining.min(COPY_STEP_BYTES)), &mut output)
        .map_err(|error| BrokerError::Io("copy materialization extent", error))?;
    if copied == 0 && remaining != 0 {
        return Err(BrokerError::UnsafeEntry(
            "materialization source ended before its catalogued size".to_string(),
        ));
    }
    output
        .sync_all()
        .map_err(|error| BrokerError::Io("sync materialization partial", error))?;
    let completed = offset + copied;
    if completed < plan.expected_size_bytes {
        return Ok(inspection(
            MaterializationRecoveryState::Copying,
            completed,
            plan,
            None,
        ));
    }
    let hash = hash_file(partial)?;
    if hash != normalize_sha256(&plan.expected_sha256)? {
        return Err(BrokerError::UnsafeEntry(
            "materialization checksum does not match catalogue authority".to_string(),
        ));
    }
    fs::hard_link(partial, destination).map_err(|error| {
        BrokerError::Io(
            "publish materialization without replacing destination",
            error,
        )
    })?;
    fs::remove_file(partial)
        .map_err(|error| BrokerError::Io("remove published materialization partial", error))?;
    crate::marker::sync_directory(destination.parent().ok_or_else(|| {
        BrokerError::InvalidRequest("materialization destination has no parent".to_string())
    })?)?;
    Ok(inspection(
        MaterializationRecoveryState::Ready,
        completed,
        plan,
        Some(hash),
    ))
}

fn validate_authority(
    config: &BrokerConfig,
    workspace_id: &str,
    plan: &MaterializationPlan,
) -> Result<PathBuf, BrokerError> {
    let aggregate = aggregate_root(config)?.join(workspace_id);
    let mounted = mounted_mergerfs_entry(&aggregate)?.is_some_and(|(_, options)| {
        options
            .split(',')
            .any(|value| value == format!("fsname=dasobjectstore-workspace-{workspace_id}"))
    });
    if !mounted {
        return Err(BrokerError::UnsafeEntry(
            "workspace aggregate is not mounted with its expected identity".to_string(),
        ));
    }
    let metadata_path = config.live_metadata_path.as_deref().ok_or_else(|| {
        BrokerError::UnsafeConfig("live_metadata_path is not configured".to_string())
    })?;
    let metadata = fs::symlink_metadata(metadata_path)
        .map_err(|error| BrokerError::Io("stat live metadata", error))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(BrokerError::UnsafeConfig(
            "live metadata must be a regular non-symlink file".to_string(),
        ));
    }
    let connection = Connection::open_with_flags(metadata_path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| BrokerError::Protocol(error.to_string()))?;
    connection
        .busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|error| BrokerError::Protocol(error.to_string()))?;
    let record = connection
        .query_row(
            "SELECT placements.disk_id, placements.relative_path,
                    objects.size_bytes, objects.content_hash,
                    placements.content_hash, placements.verified_at_utc
             FROM placements
             JOIN objects USING (object_id)
             WHERE placements.placement_id = ?1 AND objects.object_id = ?2",
            [&plan.source_placement_id, &plan.source_object_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            },
        )
        .optional()
        .map_err(|error| BrokerError::Protocol(error.to_string()))?
        .ok_or_else(|| {
            BrokerError::UnsafeEntry("catalogued source placement is unavailable".to_string())
        })?;
    if record.5.is_none()
        || record.2 != Some(plan.expected_size_bytes as i64)
        || record
            .3
            .as_deref()
            .and_then(|value| normalize_sha256(value).ok())
            .as_deref()
            != Some(&normalize_sha256(&plan.expected_sha256)?)
        || record
            .4
            .as_deref()
            .and_then(|value| normalize_sha256(value).ok())
            .as_deref()
            != Some(&normalize_sha256(&plan.expected_sha256)?)
    {
        return Err(BrokerError::UnsafeEntry(
            "catalogue size, checksum, and verification evidence do not match".to_string(),
        ));
    }
    let disk = config.disks.get(&record.0).ok_or_else(|| {
        BrokerError::UnsafeConfig("source placement disk is not broker-managed".to_string())
    })?;
    let relative = validate_relative_path(&record.1)?;
    let source = disk.root.join(relative);
    ensure_no_symlink_components(&disk.root, &source)?;
    let size = safe_file_size(&source)?.ok_or_else(|| {
        BrokerError::UnsafeEntry("catalogued source payload is absent".to_string())
    })?;
    if size != plan.expected_size_bytes {
        return Err(BrokerError::UnsafeEntry(
            "source payload size changed after catalogue verification".to_string(),
        ));
    }
    Ok(source)
}

fn destination_path(
    config: &BrokerConfig,
    workspace_id: &str,
    plan: &MaterializationPlan,
) -> Result<PathBuf, BrokerError> {
    Ok(aggregate_root(config)?
        .join(workspace_id)
        .join(validate_relative_path(&plan.destination_relative_path)?))
}

fn partial_path(destination: &Path, plan: &MaterializationPlan) -> Result<PathBuf, BrokerError> {
    let identity = format!(
        "{:x}",
        Sha256::digest(
            format!(
                "{}\0{}\0{}\0{}\0{}",
                plan.source_object_id,
                plan.source_placement_id,
                plan.destination_relative_path,
                plan.expected_size_bytes,
                plan.expected_sha256
            )
            .as_bytes()
        )
    );
    Ok(destination
        .parent()
        .ok_or_else(|| BrokerError::InvalidRequest("destination has no parent".to_string()))?
        .join(format!(".dasobjectstore-materialize-{identity}.partial")))
}

fn validate_relative_path(value: &str) -> Result<PathBuf, BrokerError> {
    let path = Path::new(value);
    if value.is_empty() || value.len() > 4096 || path.is_absolute() {
        return Err(BrokerError::InvalidRequest(
            "materialization path must be bounded and relative".to_string(),
        ));
    }
    if !path.components().all(|component| {
        matches!(component, Component::Normal(part) if !part.is_empty() && part != "." && part != "..")
    }) {
        return Err(BrokerError::InvalidRequest(
            "materialization path contains an unsafe component".to_string(),
        ));
    }
    Ok(path.to_path_buf())
}

fn validate_opaque_id(field: &str, value: &str) -> Result<(), BrokerError> {
    if value.is_empty()
        || value.len() > 4096
        || value.chars().any(|character| character.is_control())
    {
        return Err(BrokerError::InvalidRequest(format!(
            "{field} must be a bounded opaque identity"
        )));
    }
    Ok(())
}

fn ensure_safe_directory_tree(root: &Path, target: &Path) -> Result<(), BrokerError> {
    let relative = target.strip_prefix(root).map_err(|_| {
        BrokerError::InvalidRequest("destination escaped aggregate root".to_string())
    })?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => {
                return Err(BrokerError::UnsafeEntry(
                    "destination parent is not a real directory".to_string(),
                ))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir(&current)
                .map_err(|error| BrokerError::Io("create destination directory", error))?,
            Err(error) => return Err(BrokerError::Io("stat destination directory", error)),
        }
    }
    Ok(())
}

fn ensure_no_symlink_components(root: &Path, target: &Path) -> Result<(), BrokerError> {
    let relative = target
        .strip_prefix(root)
        .map_err(|_| BrokerError::UnsafeEntry("source escaped managed disk root".to_string()))?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        let metadata = fs::symlink_metadata(&current)
            .map_err(|error| BrokerError::Io("stat source component", error))?;
        if metadata.file_type().is_symlink() {
            return Err(BrokerError::UnsafeEntry(
                "source contains a symlink component".to_string(),
            ));
        }
    }
    Ok(())
}

fn safe_file_size(path: &Path) -> Result<Option<u64>, BrokerError> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(BrokerError::Io("stat materialization file", error)),
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            Ok(Some(metadata.len()))
        }
        Ok(_) => Err(BrokerError::UnsafeEntry(
            "materialization path is not a regular non-symlink file".to_string(),
        )),
    }
}

fn hash_file(path: &Path) -> Result<String, BrokerError> {
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|error| BrokerError::Io("open materialization for hashing", error))?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| BrokerError::Io("hash materialization", error))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn normalize_sha256(value: &str) -> Result<String, BrokerError> {
    let value = value.strip_prefix("sha256:").unwrap_or(value);
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(BrokerError::InvalidRequest(
            "materialization checksum must be SHA-256".to_string(),
        ));
    }
    Ok(value.to_ascii_lowercase())
}

fn inspection(
    state: MaterializationRecoveryState,
    completed_bytes: u64,
    plan: &MaterializationPlan,
    observed_sha256: Option<String>,
) -> MaterializationInspection {
    MaterializationInspection {
        state,
        completed_bytes,
        expected_size_bytes: plan.expected_size_bytes,
        observed_sha256,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn plan(destination: &str) -> MaterializationPlan {
        MaterializationPlan {
            source_object_id: "object-a".to_string(),
            source_placement_id: "placement-a".to_string(),
            destination_relative_path: destination.to_string(),
            expected_size_bytes: 64,
            expected_sha256: "a".repeat(64),
        }
    }

    #[test]
    fn plan_accepts_only_bounded_relative_destination_and_sha256() {
        validate_plan(&plan("inputs/object-a.bin")).expect("valid plan");
        for destination in ["", "/absolute", "../escape", "inputs/../../escape"] {
            assert!(validate_plan(&plan(destination)).is_err());
        }
        let mut invalid = plan("inputs/object-a.bin");
        invalid.expected_sha256 = "not-a-digest".to_string();
        assert!(validate_plan(&invalid).is_err());
    }

    #[test]
    fn partial_identity_is_deterministic_and_destination_local() {
        let destination = Path::new("/workspace/inputs/object-a.bin");
        let first = partial_path(destination, &plan("inputs/object-a.bin")).expect("partial");
        let second = partial_path(destination, &plan("inputs/object-a.bin")).expect("partial");
        assert_eq!(first, second);
        assert_eq!(first.parent(), destination.parent());
        assert!(first
            .file_name()
            .expect("name")
            .to_string_lossy()
            .starts_with(".dasobjectstore-materialize-"));
    }

    #[test]
    fn copy_extent_resumes_partial_verifies_and_publishes_without_replacement() {
        let root = std::env::temp_dir().join(format!(
            "dos-materialize-copy-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::create_dir(&root).expect("root");
        let source = root.join("source");
        let destination = root.join("destination");
        let partial = root.join(".partial");
        let payload = vec![0x5a; 1024 * 1024];
        fs::write(&source, &payload).expect("source");
        fs::write(&partial, &payload[..4096]).expect("interrupted partial");
        let materialization = MaterializationPlan {
            source_object_id: "object/path".to_string(),
            source_placement_id: "placement:a".to_string(),
            destination_relative_path: "inputs/object.bin".to_string(),
            expected_size_bytes: payload.len() as u64,
            expected_sha256: format!("{:x}", Sha256::digest(&payload)),
        };
        let result =
            copy_extent(&source, &destination, &partial, 4096, &materialization).expect("resume");
        assert_eq!(result.state, MaterializationRecoveryState::Ready);
        assert_eq!(fs::read(&destination).expect("published"), payload);
        assert!(!partial.exists());

        fs::write(&partial, b"other").expect("conflicting retry");
        assert!(copy_extent(&source, &destination, &partial, 5, &materialization).is_err());
        assert_eq!(fs::read(&destination).expect("preserved"), payload);
        fs::remove_dir_all(root).expect("cleanup");
    }
}
