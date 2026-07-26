use crate::broker::{aggregate_root, mounted_mergerfs_entry};
use crate::{BrokerConfig, BrokerError, CheckpointInventory, CheckpointMember, CheckpointPlan};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};

const HARD_MAX_FILES: u32 = 4096;
const HARD_MAX_LOGICAL_BYTES: u64 = 4 * 1024 * 1024 * 1024 * 1024;
const HARD_MAX_DEPTH: usize = 64;

pub(crate) fn validate_plan(plan: &CheckpointPlan) -> Result<(), BrokerError> {
    validate_relative_path(&plan.relative_prefix)?;
    if plan.max_files == 0 || plan.max_files > HARD_MAX_FILES {
        return Err(BrokerError::InvalidRequest(format!(
            "checkpoint max_files must be between 1 and {HARD_MAX_FILES}"
        )));
    }
    if plan.max_logical_bytes == 0 || plan.max_logical_bytes > HARD_MAX_LOGICAL_BYTES {
        return Err(BrokerError::InvalidRequest(
            "checkpoint max_logical_bytes is outside the supported bound".to_string(),
        ));
    }
    Ok(())
}

pub(crate) fn inventory(
    config: &BrokerConfig,
    workspace_id: &str,
    plan: &CheckpointPlan,
) -> Result<CheckpointInventory, BrokerError> {
    validate_plan(plan)?;
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
    let prefix = aggregate.join(&plan.relative_prefix);
    let metadata = fs::symlink_metadata(&prefix)
        .map_err(|error| BrokerError::Io("stat checkpoint prefix", error))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(BrokerError::UnsafeEntry(
            "checkpoint prefix must be a non-symlink directory".to_string(),
        ));
    }
    let mut paths = Vec::new();
    collect_files(&prefix, &prefix, 0, plan.max_files, &mut paths)?;
    paths.sort();
    let mut logical_bytes = 0_u64;
    let mut members = Vec::with_capacity(paths.len());
    for relative in paths {
        let path = prefix.join(&relative);
        let before = fs::symlink_metadata(&path)
            .map_err(|error| BrokerError::Io("stat checkpoint member", error))?;
        if !before.is_file() || before.file_type().is_symlink() {
            return Err(BrokerError::UnsafeEntry(format!(
                "checkpoint member changed type: {}",
                relative.display()
            )));
        }
        logical_bytes = logical_bytes.checked_add(before.len()).ok_or_else(|| {
            BrokerError::InvalidRequest("checkpoint logical bytes overflow".to_string())
        })?;
        if logical_bytes > plan.max_logical_bytes {
            return Err(BrokerError::InvalidRequest(
                "checkpoint exceeds max_logical_bytes".to_string(),
            ));
        }
        let sha256 = hash_file(&path)?;
        let after = fs::symlink_metadata(&path)
            .map_err(|error| BrokerError::Io("restat checkpoint member", error))?;
        if before.len() != after.len()
            || before.modified().ok() != after.modified().ok()
            || !after.is_file()
            || after.file_type().is_symlink()
        {
            return Err(BrokerError::UnsafeEntry(format!(
                "checkpoint member changed while hashing: {}",
                relative.display()
            )));
        }
        members.push(CheckpointMember {
            relative_path: relative.to_string_lossy().to_string(),
            size_bytes: before.len(),
            sha256,
        });
    }
    let manifest_sha256 = manifest_digest(&plan.relative_prefix, &members);
    Ok(CheckpointInventory {
        relative_prefix: plan.relative_prefix.clone(),
        logical_bytes,
        manifest_sha256,
        members,
    })
}

fn collect_files(
    root: &Path,
    directory: &Path,
    depth: usize,
    max_files: u32,
    output: &mut Vec<PathBuf>,
) -> Result<(), BrokerError> {
    if depth > HARD_MAX_DEPTH {
        return Err(BrokerError::InvalidRequest(
            "checkpoint exceeds maximum directory depth".to_string(),
        ));
    }
    let mut entries = fs::read_dir(directory)
        .map_err(|error| BrokerError::Io("read checkpoint directory", error))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| BrokerError::Io("read checkpoint entry", error))?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let kind = entry
            .file_type()
            .map_err(|error| BrokerError::Io("inspect checkpoint entry", error))?;
        if kind.is_symlink() {
            return Err(BrokerError::UnsafeEntry(
                "checkpoint contains a symlink".to_string(),
            ));
        }
        if kind.is_dir() {
            collect_files(root, &entry.path(), depth + 1, max_files, output)?;
        } else if kind.is_file() {
            if output.len() >= max_files as usize {
                return Err(BrokerError::InvalidRequest(
                    "checkpoint exceeds max_files".to_string(),
                ));
            }
            output.push(
                entry
                    .path()
                    .strip_prefix(root)
                    .map_err(|_| BrokerError::UnsafeEntry("checkpoint escaped prefix".to_string()))?
                    .to_path_buf(),
            );
        } else {
            return Err(BrokerError::UnsafeEntry(
                "checkpoint contains a non-regular entry".to_string(),
            ));
        }
    }
    Ok(())
}

fn hash_file(path: &Path) -> Result<String, BrokerError> {
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|error| BrokerError::Io("open checkpoint member", error))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| BrokerError::Io("hash checkpoint member", error))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("sha256:{:x}", digest.finalize()))
}

fn manifest_digest(prefix: &str, members: &[CheckpointMember]) -> String {
    let mut digest = Sha256::new();
    digest.update(prefix.as_bytes());
    digest.update([0]);
    for member in members {
        digest.update(member.relative_path.as_bytes());
        digest.update([0]);
        digest.update(member.size_bytes.to_be_bytes());
        digest.update(member.sha256.as_bytes());
        digest.update([0]);
    }
    format!("sha256:{:x}", digest.finalize())
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
            "checkpoint prefix must be a conservative relative path".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{collect_files, manifest_digest, validate_plan};
    use crate::{CheckpointMember, CheckpointPlan};
    use std::fs;

    #[test]
    fn checkpoint_plan_requires_explicit_hard_bounds() {
        assert!(validate_plan(&CheckpointPlan {
            relative_prefix: "outputs".to_string(),
            max_files: 4096,
            max_logical_bytes: 1024,
        })
        .is_ok());
        assert!(validate_plan(&CheckpointPlan {
            relative_prefix: "../outputs".to_string(),
            max_files: 1,
            max_logical_bytes: 1,
        })
        .is_err());
        assert!(validate_plan(&CheckpointPlan {
            relative_prefix: "outputs".to_string(),
            max_files: 4097,
            max_logical_bytes: 1,
        })
        .is_err());
    }

    #[test]
    fn collection_is_deterministic_bounded_and_rejects_symlinks() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-checkpoint-scan-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("nested")).expect("directories");
        fs::write(root.join("z"), b"z").expect("file");
        fs::write(root.join("nested/a"), b"a").expect("file");
        let mut paths = Vec::new();
        collect_files(&root, &root, 0, 2, &mut paths).expect("bounded scan");
        paths.sort();
        assert_eq!(
            paths,
            vec![
                std::path::PathBuf::from("nested/a"),
                std::path::PathBuf::from("z")
            ]
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("z"), root.join("link")).expect("symlink");
            assert!(collect_files(&root, &root, 0, 3, &mut Vec::new()).is_err());
        }
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn manifest_digest_commits_path_size_and_hash() {
        let member = CheckpointMember {
            relative_path: "result.bin".to_string(),
            size_bytes: 1,
            sha256: format!("sha256:{}", "a".repeat(64)),
        };
        assert_eq!(
            manifest_digest("outputs", std::slice::from_ref(&member)),
            manifest_digest("outputs", &[member])
        );
        assert_ne!(
            manifest_digest(
                "outputs",
                &[CheckpointMember {
                    relative_path: "result.bin".to_string(),
                    size_bytes: 2,
                    sha256: format!("sha256:{}", "a".repeat(64)),
                }]
            ),
            manifest_digest(
                "outputs",
                &[CheckpointMember {
                    relative_path: "result.bin".to_string(),
                    size_bytes: 1,
                    sha256: format!("sha256:{}", "a".repeat(64)),
                }]
            )
        );
    }
}
