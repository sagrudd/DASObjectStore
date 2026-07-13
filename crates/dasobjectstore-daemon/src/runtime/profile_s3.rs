//! Provider-neutral read/write semantics for profile-backed S3 adapters.
//!
//! This module is deliberately below any HTTP or provider implementation. It
//! derives list, HEAD, and GET views from the daemon-authoritative backend
//! catalogue and never consults a provider listing or exposes private paths.

use crate::runtime::{DriveBackend, FolderBackend};
use dasobjectstore_core::backend::{
    BackendError, BackendObjectKey, BackendObjectRecord, ObjectCatalogueAuthority,
    ObjectStoreBackend,
};
use std::io::Read;

pub trait ProfileS3ReadBackend: ObjectStoreBackend + ObjectCatalogueAuthority {}

impl<T> ProfileS3ReadBackend for T where T: ObjectStoreBackend + ObjectCatalogueAuthority {}

pub trait ProfileS3WriteBackend: ProfileS3ReadBackend {
    fn abort_profile_s3_object(
        &mut self,
        reservation_id: &str,
        staged: Option<&BackendObjectRecord>,
    ) -> Result<(), BackendError>;
}

impl ProfileS3WriteBackend for FolderBackend {
    fn abort_profile_s3_object(
        &mut self,
        reservation_id: &str,
        staged: Option<&BackendObjectRecord>,
    ) -> Result<(), BackendError> {
        self.abort_staged_profile_object(reservation_id, staged)
    }
}

impl ProfileS3WriteBackend for DriveBackend {
    fn abort_profile_s3_object(
        &mut self,
        reservation_id: &str,
        staged: Option<&BackendObjectRecord>,
    ) -> Result<(), BackendError> {
        self.abort_staged_profile_object(reservation_id, staged)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileS3Object {
    pub key: BackendObjectKey,
    pub size_bytes: u64,
    pub checksum: String,
}

pub fn list_profile_objects(
    backend: &dyn ProfileS3ReadBackend,
    prefix: Option<&str>,
) -> Result<Vec<ProfileS3Object>, BackendError> {
    let prefix = prefix.unwrap_or_default();
    backend.records().map(|records| {
        records
            .into_iter()
            .filter(|record| record.key.object_id.starts_with(prefix))
            .map(|record| ProfileS3Object {
                key: record.key,
                size_bytes: record.size_bytes,
                checksum: record.checksum,
            })
            .collect()
    })
}

pub fn head_profile_object(
    backend: &dyn ProfileS3ReadBackend,
    key: &BackendObjectKey,
) -> Result<ProfileS3Object, BackendError> {
    backend
        .records()?
        .into_iter()
        .find(|record| record.key == *key)
        .map(|record| ProfileS3Object {
            key: record.key,
            size_bytes: record.size_bytes,
            checksum: record.checksum,
        })
        .ok_or_else(|| {
            BackendError::NotFound(format!("profile object {} not found", key.object_id))
        })
}

pub fn get_profile_object(
    backend: &dyn ProfileS3ReadBackend,
    key: &BackendObjectKey,
) -> Result<Box<dyn Read + Send>, BackendError> {
    // HEAD first ensures GET only exposes catalogue-authoritative objects.
    head_profile_object(backend, key)?;
    backend.read(key)
}

/// Store one profile-backed S3 object through the daemon-owned transactional
/// backend lifecycle. The caller must provide the S3 Content-Length; unknown
/// length and multipart assembly are separate protocol layers. Hashing occurs
/// while the backend stages the stream, before durable finalization and the
/// catalogue commit.
pub fn put_profile_object(
    backend: &mut dyn ProfileS3WriteBackend,
    reservation_id: &str,
    key: &BackendObjectKey,
    source: &mut dyn Read,
    size_bytes: u64,
) -> Result<BackendObjectRecord, BackendError> {
    backend.reserve(reservation_id, size_bytes)?;
    let staged = match backend.stage(reservation_id, key, source) {
        Ok(staged) => staged,
        Err(error) => {
            let cleanup = backend.abort_profile_s3_object(reservation_id, None);
            return Err(with_cleanup_error(error, cleanup));
        }
    };
    let finalized = match backend.finalize(staged.clone()) {
        Ok(finalized) => finalized,
        Err(error) => {
            let cleanup = backend.abort_profile_s3_object(reservation_id, Some(&staged));
            return Err(with_cleanup_error(error, cleanup));
        }
    };
    // Finalization has already committed physical accounting. Do not release
    // or remove the payload if catalogue persistence fails; reconciliation can
    // safely discover this durable-but-unlisted object.
    backend.commit_batch(std::slice::from_ref(&finalized))?;
    Ok(finalized)
}

fn with_cleanup_error(error: BackendError, cleanup: Result<(), BackendError>) -> BackendError {
    match cleanup {
        Ok(()) => error,
        Err(cleanup) => BackendError::InvalidRequest(format!(
            "profile S3 upload failed ({error}); cleanup failed ({cleanup})"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        get_profile_object, head_profile_object, list_profile_objects, put_profile_object,
    };
    use crate::runtime::{DriveBackend, DriveRuntimeGuard, FolderBackend};
    use dasobjectstore_core::backend::{
        BackendObjectKey, ObjectCatalogueAuthority, ObjectStoreBackend,
    };
    use dasobjectstore_core::deployment::{DeploymentProfile, HostMode};
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_core::manifest::{BackendReference, ObjectStoreManifest};
    use dasobjectstore_core::protection::ProtectionPolicy;
    use dasobjectstore_core::store::CapacityPolicy;
    use std::io::Read;
    use std::path::PathBuf;
    use std::sync::{atomic::AtomicBool, Arc};

    fn backend() -> (FolderBackend, PathBuf) {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-profile-s3-{}-{nonce}",
            std::process::id()
        ));
        let manifest = ObjectStoreManifest {
            schema_version: 1,
            store_id: StoreId::new("profile-s3").expect("store id"),
            deployment_profile: DeploymentProfile::Folder,
            host_mode: HostMode::PerUser,
            protection: ProtectionPolicy::LocalOnly,
            backend: BackendReference::Folder {
                root_identity: "profile-s3-root".to_string(),
            },
        };
        let backend = FolderBackend::open(&root, manifest, CapacityPolicy::bounded(1024, 0), 0)
            .expect("folder backend");
        (backend, root)
    }

    #[derive(Debug)]
    struct TestDriveGuard(AtomicBool);

    impl DriveRuntimeGuard for TestDriveGuard {
        fn validate(&self) -> Result<(), String> {
            if self.0.load(std::sync::atomic::Ordering::SeqCst) {
                Ok(())
            } else {
                Err("drive identity drifted".to_string())
            }
        }
    }

    fn drive_backend() -> (DriveBackend, Arc<TestDriveGuard>, PathBuf) {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "dasobjectstore-profile-s3-drive-{}-{nonce}",
            std::process::id()
        ));
        let manifest = ObjectStoreManifest {
            schema_version: 1,
            store_id: StoreId::new("profile-s3-drive").expect("store id"),
            deployment_profile: DeploymentProfile::Drive,
            host_mode: HostMode::System,
            protection: ProtectionPolicy::LocalOnly,
            backend: BackendReference::Drive {
                filesystem_identity: "fsid:profile-s3-drive".to_string(),
                device_identity: Some("device:profile-s3-drive".to_string()),
                media: dasobjectstore_core::manifest::DriveMediaKind::Ssd,
                mount_path_hint: None,
            },
        };
        let guard = Arc::new(TestDriveGuard(AtomicBool::new(true)));
        let backend = DriveBackend::open(
            &root,
            manifest,
            CapacityPolicy::bounded(1024, 0),
            0,
            guard.clone(),
        )
        .expect("drive backend");
        (backend, guard, root)
    }

    #[test]
    fn list_head_and_get_use_catalogue_authority() {
        let (mut backend, root) = backend();
        let key = BackendObjectKey {
            object_id: "reads/sample.fastq".to_string(),
            version: 1,
        };
        backend.reserve("profile-s3-upload", 5).expect("reserve");
        let staged = backend
            .stage("profile-s3-upload", &key, &mut &b"reads"[..])
            .expect("stage");
        let finalized = backend.finalize(staged).expect("finalize");
        backend
            .commit_batch(std::slice::from_ref(&finalized))
            .expect("catalogue commit");

        let listed = list_profile_objects(&backend, Some("reads/")).expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].key, key);
        let headed = head_profile_object(&backend, &key).expect("head");
        assert_eq!(headed.size_bytes, 5);
        assert!(headed.checksum.starts_with("sha256:"));
        let mut body = String::new();
        get_profile_object(&backend, &key)
            .expect("get")
            .read_to_string(&mut body)
            .expect("read body");
        assert_eq!(body, "reads");
        let missing = BackendObjectKey {
            object_id: "reads/missing.fastq".to_string(),
            version: 1,
        };
        assert!(head_profile_object(&backend, &missing).is_err());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn put_reserves_stages_finalizes_and_commits_catalogue() {
        let (mut backend, root) = backend();
        let key = BackendObjectKey {
            object_id: "writes/sample.fastq".to_string(),
            version: 1,
        };
        let record =
            put_profile_object(&mut backend, "profile-s3-put", &key, &mut &b"write"[..], 5)
                .expect("put");
        assert_eq!(record.key, key);
        assert_eq!(backend.capacity().reserved_bytes, 0);
        assert_eq!(list_profile_objects(&backend, None).unwrap().len(), 1);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn put_releases_reservation_when_stream_size_does_not_match() {
        let (mut backend, root) = backend();
        let key = BackendObjectKey {
            object_id: "writes/mismatch.fastq".to_string(),
            version: 1,
        };
        let error = put_profile_object(
            &mut backend,
            "profile-s3-mismatch",
            &key,
            &mut &b"wrong"[..],
            4,
        )
        .expect_err("size mismatch");
        assert!(error.to_string().contains("does not match reserved bytes"));
        assert_eq!(backend.capacity().reserved_bytes, 0);
        assert!(list_profile_objects(&backend, None).unwrap().is_empty());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn put_releases_staged_reservation_when_finalization_fails() {
        let (mut backend, root) = backend();
        let key = BackendObjectKey {
            object_id: "writes/collision.fastq".to_string(),
            version: 1,
        };
        put_profile_object(
            &mut backend,
            "profile-s3-first",
            &key,
            &mut &b"first"[..],
            5,
        )
        .expect("first put");
        let error = put_profile_object(
            &mut backend,
            "profile-s3-collision",
            &key,
            &mut &b"again"[..],
            5,
        )
        .expect_err("destination collision");
        assert!(error.to_string().contains("destination already exists"));
        assert_eq!(backend.capacity().reserved_bytes, 0);
        assert_eq!(list_profile_objects(&backend, None).unwrap().len(), 1);
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn drive_profile_uses_the_same_s3_adapter_and_fails_closed_on_guard_loss() {
        let (mut backend, guard, root) = drive_backend();
        let key = BackendObjectKey {
            object_id: "drive/sample.fastq".to_string(),
            version: 1,
        };
        put_profile_object(
            &mut backend,
            "profile-s3-drive-put",
            &key,
            &mut &b"drive"[..],
            5,
        )
        .expect("drive put");
        assert_eq!(
            list_profile_objects(&backend, Some("drive/"))
                .unwrap()
                .len(),
            1
        );
        assert_eq!(head_profile_object(&backend, &key).unwrap().size_bytes, 5);
        guard.0.store(false, std::sync::atomic::Ordering::SeqCst);
        assert!(list_profile_objects(&backend, None).is_err());
        std::fs::remove_dir_all(root).ok();
    }
}
