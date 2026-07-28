use dasobjectstore_core::ids::DiskId;
use dasobjectstore_metadata::AssurancePlacementCandidate;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub const OPERATION_SCHEMA: &str = "dasobjectstore.storage_assurance.operation.v1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationPhase {
    Planned,
    Claimed,
    Copied,
    Promoted,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DurableAssuranceOperation {
    pub schema: String,
    pub operation_id: String,
    pub action: super::StorageAssuranceAction,
    pub phase: OperationPhase,
    pub candidate: AssurancePlacementCandidate,
    pub destination_disk_id: DiskId,
    pub destination_relative_path: String,
    pub claim_owner: String,
    pub created_at_utc: String,
    pub updated_at_utc: String,
}

impl DurableAssuranceOperation {
    pub fn deterministic(
        action: super::StorageAssuranceAction,
        candidate: AssurancePlacementCandidate,
        destination_disk_id: DiskId,
        now_utc: &str,
    ) -> Self {
        let operation_id = format!(
            "{}:{}:{}:{}",
            action.as_str(),
            candidate.placement_id.as_str(),
            destination_disk_id.as_str(),
            candidate.content_hash
        );
        let claim_owner = format!(
            "assurance:{}:{}",
            candidate.object_id.as_str(),
            destination_disk_id.as_str()
        );
        Self {
            schema: OPERATION_SCHEMA.to_string(),
            operation_id,
            action,
            phase: OperationPhase::Planned,
            destination_relative_path: candidate.relative_path.clone(),
            candidate,
            destination_disk_id,
            claim_owner,
            created_at_utc: now_utc.to_string(),
            updated_at_utc: now_utc.to_string(),
        }
    }

    pub fn advance(&mut self, phase: OperationPhase, now_utc: &str) {
        self.phase = phase;
        self.updated_at_utc = now_utc.to_string();
    }
}

pub fn read(path: &Path) -> Result<Option<DurableAssuranceOperation>, io::Error> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "assurance operation journal must be a regular file",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    }
    let bytes = fs::read(path)?;
    let operation: DurableAssuranceOperation = serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if operation.schema != OPERATION_SCHEMA {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unsupported assurance operation journal schema",
        ));
    }
    Ok(Some(operation))
}

pub fn persist(path: &Path, operation: &DurableAssuranceOperation) -> Result<(), io::Error> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "journal has no parent"))?;
    fs::create_dir_all(parent)?;
    let temporary = temporary_path(path);
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    let result = (|| {
        serde_json::to_writer_pretty(&mut file, operation)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        File::open(parent)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub fn remove(path: &Path) -> Result<(), io::Error> {
    match fs::remove_file(path) {
        Ok(()) => {
            if let Some(parent) = path.parent() {
                File::open(parent)?.sync_all()?;
            }
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn temporary_path(path: &Path) -> PathBuf {
    path.with_extension(format!("tmp-{}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dasobjectstore_core::ids::{ObjectId, PlacementId, StoreId};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn operation_journal_round_trips_every_restart_checkpoint() {
        let root = std::env::temp_dir().join(format!(
            "dos-assurance-operation-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let path = root.join("operation.json");
        let candidate = AssurancePlacementCandidate {
            placement_id: PlacementId::new("placement-a").expect("placement"),
            object_id: ObjectId::new("object-a").expect("object"),
            store_id: StoreId::new("store-a").expect("store"),
            disk_id: DiskId::new("disk-a").expect("disk"),
            disk_state: "Draining".to_string(),
            relative_path: "objects/object-a/payload".to_string(),
            size_bytes: 10,
            content_hash: "a".repeat(64),
            verified_at_utc: None,
            existing_disk_ids: vec![DiskId::new("disk-a").expect("disk")],
        };
        let mut operation = DurableAssuranceOperation::deterministic(
            super::super::StorageAssuranceAction::Evacuate,
            candidate,
            DiskId::new("disk-b").expect("disk"),
            "2026-07-28T00:00:00Z",
        );
        for phase in [
            OperationPhase::Planned,
            OperationPhase::Claimed,
            OperationPhase::Copied,
            OperationPhase::Promoted,
        ] {
            operation.advance(phase, "2026-07-28T00:01:00Z");
            persist(&path, &operation).expect("persist");
            assert_eq!(read(&path).expect("read").expect("journal").phase, phase);
        }
        remove(&path).expect("remove");
        assert!(read(&path).expect("read empty").is_none());
        fs::remove_dir_all(root).expect("cleanup");
    }
}
