//! Provider-neutral read semantics for profile-backed S3 adapters.
//!
//! This module is deliberately below any HTTP or provider implementation. It
//! derives list, HEAD, and GET views from the daemon-authoritative backend
//! catalogue and never consults a provider listing or exposes private paths.

use dasobjectstore_core::backend::{BackendError, BackendObjectKey, ObjectStoreBackend};
use std::io::Read;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileS3Object {
    pub key: BackendObjectKey,
    pub size_bytes: u64,
    pub checksum: String,
}

pub fn list_profile_objects(
    backend: &dyn ObjectStoreBackend,
    prefix: Option<&str>,
) -> Result<Vec<ProfileS3Object>, BackendError> {
    let prefix = prefix.unwrap_or_default();
    backend.enumerate(Some(prefix)).map(|records| {
        records
            .into_iter()
            .map(|record| ProfileS3Object {
                key: record.key,
                size_bytes: record.size_bytes,
                checksum: record.checksum,
            })
            .collect()
    })
}

pub fn head_profile_object(
    backend: &dyn ObjectStoreBackend,
    key: &BackendObjectKey,
) -> Result<ProfileS3Object, BackendError> {
    backend
        .enumerate(None)?
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
    backend: &dyn ObjectStoreBackend,
    key: &BackendObjectKey,
) -> Result<Box<dyn Read + Send>, BackendError> {
    // HEAD first ensures GET only exposes catalogue-authoritative objects.
    head_profile_object(backend, key)?;
    backend.read(key)
}

#[cfg(test)]
mod tests {
    use super::{get_profile_object, head_profile_object, list_profile_objects};
    use crate::runtime::FolderBackend;
    use dasobjectstore_core::backend::{BackendObjectKey, ObjectStoreBackend};
    use dasobjectstore_core::deployment::{DeploymentProfile, HostMode};
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_core::manifest::{BackendReference, ObjectStoreManifest};
    use dasobjectstore_core::protection::ProtectionPolicy;
    use dasobjectstore_core::store::CapacityPolicy;
    use std::io::Read;
    use std::path::PathBuf;

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
        backend.finalize(staged).expect("finalize");

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
}
