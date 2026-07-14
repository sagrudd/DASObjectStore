use super::*;
use dasobjectstore_core::backend::{BackendCapabilities, BackendHealth, ObjectStoreBackend};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};

impl ObjectStoreBackend for FolderBackend {
    fn capabilities(&self) -> BackendCapabilities {
        BackendCapabilities::complete()
    }

    fn validate_manifest(&self, manifest: &ObjectStoreManifest) -> Result<(), BackendError> {
        manifest.validate().map_err(BackendError::Manifest)?;
        if manifest != &self.manifest
            || !matches!(manifest.backend, BackendReference::Folder { .. })
        {
            return Err(BackendError::InvalidRequest(
                "folder manifest identity does not match this backend".to_string(),
            ));
        }
        Ok(())
    }

    fn reserve(&mut self, reservation_id: &str, bytes: u64) -> Result<(), BackendError> {
        self.ledger
            .reserve(reservation_id.to_string(), bytes)
            .map_err(|error| {
                BackendError::InvalidRequest(format!("capacity reservation: {error:?}"))
            })
    }

    fn stage(
        &mut self,
        reservation_id: &str,
        key: &BackendObjectKey,
        source: &mut dyn Read,
    ) -> Result<BackendObjectRecord, BackendError> {
        validate_object_key(key)?;
        let temporary_path = self.temporary_path(reservation_id)?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary_path).map_err(io_error)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        let result = (|| {
            loop {
                let read = source.read(&mut buffer).map_err(io_error)?;
                if read == 0 {
                    break;
                }
                file.write_all(&buffer[..read]).map_err(io_error)?;
                hasher.update(&buffer[..read]);
            }
            file.sync_all().map_err(io_error)?;
            let checksum = format!("sha256:{:x}", hasher.finalize());
            let size_bytes = file.metadata().map_err(io_error)?.len();
            let reserved_bytes =
                self.ledger
                    .reservation_bytes(reservation_id)
                    .ok_or_else(|| {
                        BackendError::InvalidRequest("unknown capacity reservation".to_string())
                    })?;
            if size_bytes != reserved_bytes {
                return Err(BackendError::InvalidRequest(format!(
                    "staged object size {size_bytes} does not match reserved bytes {reserved_bytes}"
                )));
            }
            let record =
                self.record_for_path(key.clone(), &temporary_path, checksum, size_bytes)?;
            self.staged_reservations
                .insert(temporary_path.clone(), reservation_id.to_string());
            Ok(record)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary_path);
        }
        result
    }

    fn finalize(
        &mut self,
        staged: BackendObjectRecord,
    ) -> Result<BackendObjectRecord, BackendError> {
        let temporary_path = self.root.join(&staged.location);
        let reservation_id = self
            .staged_reservations
            .get(&temporary_path)
            .cloned()
            .ok_or_else(|| {
                BackendError::InvalidRequest("unknown staged folder object".to_string())
            })?;
        if !temporary_path.starts_with(&self.staging_root) {
            return Err(BackendError::InvalidRequest(
                "staged object is outside the private staging directory".to_string(),
            ));
        }
        let (checksum, size_bytes) = hash_stable_file(&temporary_path)?;
        if checksum != staged.checksum || size_bytes != staged.size_bytes {
            return Err(BackendError::InvalidRequest(
                "staged folder object changed before finalization".to_string(),
            ));
        }
        let destination = self.object_path(&staged.key)?;
        if destination.exists() {
            return Err(BackendError::InvalidRequest(
                "object destination already exists".to_string(),
            ));
        }
        let parent = destination.parent().ok_or_else(|| {
            BackendError::InvalidRequest("object destination has no parent".to_string())
        })?;
        ensure_safe_parent(&self.objects_root, parent)?;
        fs::rename(&temporary_path, &destination).map_err(io_error)?;
        sync_directory(parent)?;
        self.staged_reservations.remove(&temporary_path);
        self.ledger
            .commit(&reservation_id)
            .map_err(|error| BackendError::InvalidRequest(format!("capacity commit: {error:?}")))?;
        self.record_for_path(staged.key, &destination, staged.checksum, size_bytes)
    }

    fn read(&self, key: &BackendObjectKey) -> Result<Box<dyn Read + Send>, BackendError> {
        Ok(Box::new(
            File::open(self.object_path(key)?).map_err(io_error)?,
        ))
    }

    fn read_range(
        &self,
        key: &BackendObjectKey,
        offset: u64,
        length: u64,
    ) -> Result<Box<dyn Read + Send>, BackendError> {
        let mut file = File::open(self.object_path(key)?).map_err(io_error)?;
        let size = file.metadata().map_err(io_error)?.len();
        if offset > size {
            return Err(BackendError::InvalidRequest(
                "folder object range starts beyond object size".to_string(),
            ));
        }
        file.seek(SeekFrom::Start(offset)).map_err(io_error)?;
        Ok(Box::new(file.take(length.min(size - offset))))
    }

    fn enumerate(&self, prefix: Option<&str>) -> Result<Vec<BackendObjectRecord>, BackendError> {
        let mut records = Vec::new();
        enumerate_files(
            &self.objects_root,
            &self.root,
            &self.objects_root,
            prefix,
            &mut records,
        )?;
        Ok(records)
    }

    fn verify(&self, key: &BackendObjectKey) -> Result<BackendObjectRecord, BackendError> {
        let path = self.object_path(key)?;
        let (checksum, size_bytes) = hash_stable_file(&path)?;
        self.record_for_path(key.clone(), &path, checksum, size_bytes)
    }

    fn health(&self) -> Result<BackendHealth, BackendError> {
        Ok(BackendHealth {
            state: if self.root.is_dir() {
                "healthy"
            } else {
                "unavailable"
            }
            .to_string(),
            message: None,
        })
    }

    fn reconcile(&mut self) -> Result<Vec<BackendObjectRecord>, BackendError> {
        self.enumerate(None)
    }

    fn remove(&mut self, key: &BackendObjectKey) -> Result<(), BackendError> {
        let path = self.object_path(key)?;
        let (checksum, size_bytes) = hash_stable_file(&path)?;
        if size_bytes > self.ledger.used_bytes() {
            return Err(BackendError::InvalidRequest(
                "folder capacity accounting is below object size".to_string(),
            ));
        }
        let record = self.record_for_path(key.clone(), &path, checksum, size_bytes)?;
        // Remove the durable catalogue record first. If persistence fails, the
        // payload and logical accounting remain untouched. If payload removal
        // then fails, restore the record before returning so a retry remains
        // authoritative and fail-closed.
        self.catalogue.remove(key)?;
        if let Err(error) = fs::remove_file(&path).map_err(io_error) {
            let restore_error = self.catalogue.commit_records([record]);
            return Err(match restore_error {
                Ok(()) => error,
                Err(restore_error) => BackendError::InvalidRequest(format!(
                    "folder payload removal failed ({error}) and catalogue restore failed ({restore_error})"
                )),
            });
        }
        self.ledger
            .debit_used_bytes(size_bytes)
            .map_err(|error| BackendError::InvalidRequest(format!("capacity debit: {error:?}")))?;
        if let Some(parent) = path.parent() {
            sync_directory(parent)?;
        }
        Ok(())
    }
}
