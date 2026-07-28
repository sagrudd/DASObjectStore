use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

const APPLIANCE_IDENTITY_SCHEMA_VERSION: &str = "dasobjectstore.appliance_identity.v1";
const APPLIANCE_IDENTITY_FILE: &str = "appliance-identity.json";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApplianceIdentityRecord {
    pub schema_version: String,
    pub appliance_id: String,
}

pub fn appliance_identity_path(state_dir: &Path) -> PathBuf {
    state_dir.join(APPLIANCE_IDENTITY_FILE)
}

pub fn load_appliance_identity(state_dir: &Path) -> Result<ApplianceIdentityRecord, io::Error> {
    let path = appliance_identity_path(state_dir);
    reject_symlink_if_present(&path)?;
    let raw = fs::read(path)?;
    let record: ApplianceIdentityRecord = serde_json::from_slice(&raw)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    validate_identity(&record)?;
    Ok(record)
}

pub fn ensure_appliance_identity(state_dir: &Path) -> Result<ApplianceIdentityRecord, io::Error> {
    match load_appliance_identity(state_dir) {
        Ok(record) => return Ok(record),
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        Err(_) => {}
    }
    fs::create_dir_all(state_dir)?;
    reject_symlink(state_dir)?;
    let record = ApplianceIdentityRecord {
        schema_version: APPLIANCE_IDENTITY_SCHEMA_VERSION.to_string(),
        appliance_id: format!("das-appliance-{}", Uuid::new_v4()),
    };
    let path = appliance_identity_path(state_dir);
    let temporary = state_dir.join(format!(".appliance-identity-{}.tmp", Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o640);
    }
    let mut file = options.open(&temporary)?;
    file.write_all(&serde_json::to_vec_pretty(&record).map_err(io::Error::other)?)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    // Publish with create-once semantics. `rename(2)` would replace an
    // identity concurrently created by another service process on Unix.
    match fs::hard_link(&temporary, &path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(&temporary);
            return load_appliance_identity(state_dir);
        }
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
    }
    fs::remove_file(&temporary)?;
    fs::File::open(state_dir)?.sync_all()?;
    Ok(record)
}

fn validate_identity(record: &ApplianceIdentityRecord) -> Result<(), io::Error> {
    if record.schema_version != APPLIANCE_IDENTITY_SCHEMA_VERSION
        || !record.appliance_id.starts_with("das-appliance-")
        || record.appliance_id.len() > 128
        || !record
            .appliance_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid authoritative DASObjectStore appliance identity",
        ));
    }
    Ok(())
}

fn reject_symlink(path: &Path) -> Result<(), io::Error> {
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "appliance identity state directory must not be a symlink",
        ));
    }
    Ok(())
}

fn reject_symlink_if_present(path: &Path) -> Result<(), io::Error> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "appliance identity file must not be a symlink",
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    #[test]
    fn identity_is_stable_and_invalid_state_fails_closed() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-appliance-identity-{}",
            Uuid::new_v4()
        ));
        let first = ensure_appliance_identity(&root).expect("first");
        let second = ensure_appliance_identity(&root).expect("second");
        assert_eq!(first, second);
        fs::write(
            appliance_identity_path(&root),
            br#"{"schema_version":"wrong","appliance_id":"replacement"}"#,
        )
        .expect("corrupt");
        assert_eq!(
            ensure_appliance_identity(&root)
                .expect_err("invalid identity")
                .kind(),
            io::ErrorKind::InvalidData
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn concurrent_identity_creation_publishes_one_stable_record() {
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-appliance-identity-race-{}",
            Uuid::new_v4()
        ));
        let barrier = Arc::new(Barrier::new(8));
        let handles = (0..8)
            .map(|_| {
                let root = root.clone();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    ensure_appliance_identity(&root).expect("identity")
                })
            })
            .collect::<Vec<_>>();
        let records = handles
            .into_iter()
            .map(|handle| handle.join().expect("thread"))
            .collect::<Vec<_>>();
        assert!(records.iter().all(|record| record == &records[0]));
        assert_eq!(
            load_appliance_identity(&root).expect("published"),
            records[0]
        );
        let _ = fs::remove_dir_all(root);
    }
}
