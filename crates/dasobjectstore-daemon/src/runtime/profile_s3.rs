//! Provider-neutral read/write semantics for profile-backed S3 adapters.
//!
//! This module is deliberately below any HTTP or provider implementation. It
//! derives list, HEAD, and GET views from the daemon-authoritative backend
//! catalogue and never consults a provider listing or exposes private paths.

use crate::api::CapacityAdmissionDecision;
use crate::runtime::capacity_provider::CapacityAdmissionProvider;
use crate::runtime::{DriveBackend, FolderBackend};
use dasobjectstore_core::backend::{
    catalogue_logical_used_bytes, BackendError, BackendHealth, BackendObjectKey,
    BackendObjectRecord, ObjectCatalogueAuthority, ObjectStoreBackend,
};
use dasobjectstore_core::ids::StoreId;
use std::io::Read;

pub const PROFILE_S3_MAX_KEYS: usize = 1_000;

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileS3ListPage {
    pub objects: Vec<ProfileS3Object>,
    pub next_offset: Option<usize>,
}

pub fn list_profile_objects(
    backend: &dyn ProfileS3ReadBackend,
    prefix: Option<&str>,
) -> Result<Vec<ProfileS3Object>, BackendError> {
    let prefix = prefix.unwrap_or_default();
    backend.records().map(|mut records| {
        records.sort_by(|left, right| {
            left.key
                .object_id
                .cmp(&right.key.object_id)
                .then(left.key.version.cmp(&right.key.version))
        });
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

/// Return one bounded, stable-order page from the authoritative profile
/// catalogue. The offset is an internal continuation token for the eventual
/// HTTP adapter; it never exposes backend locations.
pub fn list_profile_objects_page(
    backend: &dyn ProfileS3ReadBackend,
    prefix: Option<&str>,
    offset: usize,
    limit: usize,
) -> Result<ProfileS3ListPage, BackendError> {
    if limit == 0 || limit > PROFILE_S3_MAX_KEYS {
        return Err(BackendError::InvalidRequest(format!(
            "profile S3 list limit must be between 1 and {PROFILE_S3_MAX_KEYS}"
        )));
    }
    let prefix = prefix.unwrap_or_default();
    let mut records = backend
        .records()?
        .into_iter()
        .filter(|record| record.key.object_id.starts_with(prefix))
        .collect::<Vec<_>>();
    records.sort_by(|left, right| {
        left.key
            .object_id
            .cmp(&right.key.object_id)
            .then(left.key.version.cmp(&right.key.version))
    });
    if offset > records.len() {
        return Err(BackendError::InvalidRequest(
            "profile S3 list offset exceeds the filtered catalogue".to_string(),
        ));
    }
    let end = offset.saturating_add(limit).min(records.len());
    let next_offset = (end < records.len()).then_some(end);
    Ok(ProfileS3ListPage {
        objects: records[offset..end]
            .iter()
            .map(|record| ProfileS3Object {
                key: record.key.clone(),
                size_bytes: record.size_bytes,
                checksum: record.checksum.clone(),
            })
            .collect(),
        next_offset,
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

/// Verify a catalogue-authoritative profile object and reject any payload
/// drift from its recorded size or checksum.
pub fn verify_profile_object(
    backend: &dyn ProfileS3ReadBackend,
    key: &BackendObjectKey,
) -> Result<ProfileS3Object, BackendError> {
    let expected = head_profile_object(backend, key)?;
    let actual = backend.verify(key)?;
    if actual.size_bytes != expected.size_bytes || actual.checksum != expected.checksum {
        return Err(BackendError::InvalidRequest(format!(
            "profile object {} failed catalogue verification",
            key.object_id
        )));
    }
    Ok(expected)
}

/// Return provider-neutral profile health without exposing backend paths.
pub fn profile_health(backend: &dyn ProfileS3ReadBackend) -> Result<BackendHealth, BackendError> {
    backend.health()
}

pub fn get_profile_object(
    backend: &dyn ProfileS3ReadBackend,
    key: &BackendObjectKey,
) -> Result<Box<dyn Read + Send>, BackendError> {
    // HEAD first ensures GET only exposes catalogue-authoritative objects.
    head_profile_object(backend, key)?;
    backend.read(key)
}

/// Read a bounded byte range from a catalogue-authoritative profile object.
/// The backend contract currently exposes a streaming full-object reader, so
/// this compatibility seam discards the prefix while preserving the same
/// private-path and authority boundaries. Provider-native range operations can
/// be added below this seam without changing consumers.
pub fn get_profile_object_range(
    backend: &dyn ProfileS3ReadBackend,
    key: &BackendObjectKey,
    offset: u64,
    length: u64,
) -> Result<Box<dyn Read + Send>, BackendError> {
    let object = head_profile_object(backend, key)?;
    if offset > object.size_bytes {
        return Err(BackendError::InvalidRequest(format!(
            "profile object range starts at {offset}, beyond object size {}",
            object.size_bytes
        )));
    }
    let mut reader = backend.read(key)?;
    discard_prefix(&mut reader, offset)?;
    Ok(Box::new(reader.take(length)))
}

/// Delete one catalogue-authoritative profile object through the daemon-owned
/// backend. The boolean reports whether a catalogue record was removed; a
/// missing key is an idempotent no-op as required by S3 DELETE semantics. A
/// present record is authorized through the catalogue before backend removal,
/// so provider listings or private paths cannot mutate an object outside the
/// logical ObjectStore view.
pub fn delete_profile_object(
    backend: &mut dyn ProfileS3WriteBackend,
    key: &BackendObjectKey,
) -> Result<bool, BackendError> {
    if backend
        .records()?
        .into_iter()
        .all(|record| record.key != *key)
    {
        return Ok(false);
    }
    backend.remove(key).map(|()| true)
}

/// Delete a profile object and reconcile the daemon-owned logical ledger from
/// the resulting authoritative catalogue. The payload/catalogue mutation is
/// durable before reconciliation; if the provider cannot persist the new
/// usage, a later reconciliation can safely retry without deleting again.
pub fn delete_profile_object_with_capacity_provider(
    capacity_provider: &dyn CapacityAdmissionProvider,
    store_id: &str,
    backend: &mut dyn ProfileS3WriteBackend,
    key: &BackendObjectKey,
) -> Result<bool, BackendError> {
    let store_id = StoreId::new(store_id.to_string()).map_err(|error| {
        BackendError::InvalidRequest(format!("invalid profile S3 ObjectStore id: {error}"))
    })?;
    let removed = delete_profile_object(backend, key)?;
    if !removed {
        return Ok(false);
    }
    let used_bytes = catalogue_logical_used_bytes(backend)?;
    capacity_provider
        .reconcile_used_bytes(&store_id, used_bytes)
        .map_err(|error| {
            BackendError::InvalidRequest(format!(
                "profile S3 deletion succeeded but capacity reconciliation failed: {error}"
            ))
        })?;
    Ok(true)
}

fn discard_prefix(reader: &mut dyn Read, mut remaining: u64) -> Result<(), BackendError> {
    let mut buffer = [0_u8; 64 * 1024];
    while remaining != 0 {
        let requested = remaining.min(buffer.len() as u64) as usize;
        let read = reader.read(&mut buffer[..requested]).map_err(|error| {
            BackendError::Io(format!("profile object range prefix read failed: {error}"))
        })?;
        if read == 0 {
            return Err(BackendError::InvalidRequest(
                "profile object ended before requested range".to_string(),
            ));
        }
        remaining -= read as u64;
    }
    Ok(())
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

/// Put one profile object while also participating in the daemon-owned
/// logical admission provider. The provider reservation is committed only
/// after backend catalogue persistence. If physical staging/finalization fails,
/// both reservations are released; after durable finalization, failures are
/// retained for reconciliation rather than risking accounting drift.
pub fn put_profile_object_with_capacity_provider(
    capacity_provider: &dyn CapacityAdmissionProvider,
    store_id: &str,
    backend: &mut dyn ProfileS3WriteBackend,
    reservation_id: &str,
    key: &BackendObjectKey,
    source: &mut dyn Read,
    size_bytes: u64,
) -> Result<BackendObjectRecord, BackendError> {
    let store_id = StoreId::new(store_id.to_string()).map_err(|error| {
        BackendError::InvalidRequest(format!("invalid profile S3 ObjectStore id: {error}"))
    })?;
    let admission = capacity_provider
        .admit_remote_upload(store_id.as_str(), size_bytes, reservation_id)
        .map_err(|error| {
            BackendError::InvalidRequest(format!("profile S3 capacity admission failed: {error}"))
        })?;
    if admission.decision != CapacityAdmissionDecision::Admitted {
        return Err(BackendError::InvalidRequest(
            admission
                .message
                .unwrap_or_else(|| "profile S3 capacity admission rejected".to_string()),
        ));
    }

    if let Err(error) = backend.reserve(reservation_id, size_bytes) {
        let cleanup = capacity_provider
            .release(&store_id, reservation_id)
            .map_err(|cleanup| BackendError::InvalidRequest(cleanup.to_string()));
        return Err(with_cleanup_error(error, cleanup));
    }
    let staged = match backend.stage(reservation_id, key, source) {
        Ok(staged) => staged,
        Err(error) => {
            let backend_cleanup = backend.abort_profile_s3_object(reservation_id, None);
            let provider_cleanup = capacity_provider
                .release(&store_id, reservation_id)
                .map_err(|cleanup| BackendError::InvalidRequest(cleanup.to_string()));
            return Err(with_cleanup_error(
                error,
                combine_cleanup(backend_cleanup, provider_cleanup),
            ));
        }
    };
    let finalized = match backend.finalize(staged.clone()) {
        Ok(finalized) => finalized,
        Err(error) => {
            let backend_cleanup = backend.abort_profile_s3_object(reservation_id, Some(&staged));
            let provider_cleanup = capacity_provider
                .release(&store_id, reservation_id)
                .map_err(|cleanup| BackendError::InvalidRequest(cleanup.to_string()));
            return Err(with_cleanup_error(
                error,
                combine_cleanup(backend_cleanup, provider_cleanup),
            ));
        }
    };
    // Do not release either reservation here: the payload is durable even if
    // catalogue persistence or logical admission commit subsequently fails.
    backend.commit_batch(std::slice::from_ref(&finalized))?;
    capacity_provider
        .commit(&store_id, reservation_id)
        .map_err(|error| {
            BackendError::InvalidRequest(format!(
                "profile S3 capacity commit failed after durable finalization: {error}"
            ))
        })?;
    Ok(finalized)
}

fn combine_cleanup(
    first: Result<(), BackendError>,
    second: Result<(), BackendError>,
) -> Result<(), BackendError> {
    match (first, second) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(first), Err(second)) => Err(BackendError::InvalidRequest(format!(
            "backend cleanup failed ({first}); capacity cleanup failed ({second})"
        ))),
    }
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
        delete_profile_object, delete_profile_object_with_capacity_provider, get_profile_object,
        get_profile_object_range, head_profile_object, list_profile_objects,
        list_profile_objects_page, profile_health, put_profile_object,
        put_profile_object_with_capacity_provider, verify_profile_object, PROFILE_S3_MAX_KEYS,
    };
    use crate::api::{CapacityAdmissionRequest, CapacityAdmissionResponse};
    use crate::runtime::{CapacityAdmissionProvider, DaemonServiceRuntimeError};
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
    use std::sync::Mutex;
    use std::sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    };

    static TEST_ROOT_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_root(prefix: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let sequence = TEST_ROOT_COUNTER.fetch_add(1, Ordering::Relaxed);
        let base = std::env::var_os("DASOBJECTSTORE_CODEX_VALIDATION_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        base.join(format!(
            "{prefix}-{}-{nonce}-{sequence}",
            std::process::id(),
        ))
    }

    fn backend() -> (FolderBackend, PathBuf) {
        let root = test_root("dasobjectstore-profile-s3");
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
        let root = test_root("dasobjectstore-profile-s3-drive");
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

    #[derive(Debug)]
    struct RecordingCapacityProvider {
        events: Mutex<Vec<String>>,
        logical_limit_bytes: u64,
        backend_free_bytes: u64,
        ssd_free_bytes: u64,
    }

    impl Default for RecordingCapacityProvider {
        fn default() -> Self {
            Self {
                events: Mutex::new(Vec::new()),
                logical_limit_bytes: 1024,
                backend_free_bytes: 1024,
                ssd_free_bytes: 1024,
            }
        }
    }

    impl CapacityAdmissionProvider for RecordingCapacityProvider {
        fn admit(
            &self,
            request: CapacityAdmissionRequest,
        ) -> Result<CapacityAdmissionResponse, DaemonServiceRuntimeError> {
            self.events.lock().expect("events lock").push(format!(
                "admit:{}:{}:{}",
                request.store_id,
                request.requested_bytes,
                request.client_request_id.clone().unwrap()
            ));
            CapacityAdmissionResponse::evaluate(
                &request,
                &CapacityPolicy::bounded(self.logical_limit_bytes, 0),
                dasobjectstore_core::store::CapacityAdmissionInput {
                    requested_bytes: request.requested_bytes,
                    copy_count: request.copy_count,
                    requires_ssd_staging: request.ingress_origin.requires_ssd_staging(),
                    used_bytes: 0,
                    reserved_bytes: 0,
                    backend_free_bytes: self.backend_free_bytes,
                    ssd_free_bytes: self.ssd_free_bytes,
                },
            )
            .map_err(|error| DaemonServiceRuntimeError::UnsupportedOperation {
                operation: error.to_string(),
            })
        }

        fn commit(
            &self,
            store_id: &StoreId,
            reservation_id: &str,
        ) -> Result<(), DaemonServiceRuntimeError> {
            self.events
                .lock()
                .expect("events lock")
                .push(format!("commit:{store_id}:{reservation_id}"));
            Ok(())
        }

        fn release(
            &self,
            store_id: &StoreId,
            reservation_id: &str,
        ) -> Result<(), DaemonServiceRuntimeError> {
            self.events
                .lock()
                .expect("events lock")
                .push(format!("release:{store_id}:{reservation_id}"));
            Ok(())
        }

        fn reconcile_used_bytes(
            &self,
            store_id: &StoreId,
            used_bytes: u64,
        ) -> Result<(), DaemonServiceRuntimeError> {
            self.events
                .lock()
                .expect("events lock")
                .push(format!("reconcile:{store_id}:{used_bytes}"));
            Ok(())
        }
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

        let second_key = BackendObjectKey {
            object_id: "reads/aaa.fastq".to_string(),
            version: 1,
        };
        backend.reserve("profile-s3-upload-2", 3).expect("reserve");
        let staged = backend
            .stage("profile-s3-upload-2", &second_key, &mut &b"aaa"[..])
            .expect("stage");
        let finalized = backend.finalize(staged).expect("finalize");
        backend
            .commit_batch(std::slice::from_ref(&finalized))
            .expect("catalogue commit");

        let listed = list_profile_objects(&backend, Some("reads/")).expect("list");
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].key, second_key);
        assert_eq!(listed[1].key, key);
        let headed = head_profile_object(&backend, &key).expect("head");
        assert_eq!(headed.size_bytes, 5);
        assert!(headed.checksum.starts_with("sha256:"));
        let mut body = String::new();
        get_profile_object(&backend, &key)
            .expect("get")
            .read_to_string(&mut body)
            .expect("read body");
        assert_eq!(body, "reads");
        let mut range = String::new();
        get_profile_object_range(&backend, &key, 1, 3)
            .expect("range")
            .read_to_string(&mut range)
            .expect("read range");
        assert_eq!(range, "ead");
        let mut offset_range = String::new();
        get_profile_object_range(&backend, &key, 2, 1)
            .expect("offset range")
            .read_to_string(&mut offset_range)
            .expect("read offset range");
        assert_eq!(offset_range, "a");
        assert!(get_profile_object_range(&backend, &key, 6, 1).is_err());
        let missing = BackendObjectKey {
            object_id: "reads/missing.fastq".to_string(),
            version: 1,
        };
        assert!(head_profile_object(&backend, &missing).is_err());
        assert_eq!(profile_health(&backend).expect("health").state, "healthy");
        assert_eq!(
            verify_profile_object(&backend, &key)
                .expect("verify")
                .size_bytes,
            5
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn verify_rejects_payload_drift_against_catalogue() {
        let (mut backend, root) = backend();
        let key = BackendObjectKey {
            object_id: "verify/sample.fastq".to_string(),
            version: 1,
        };
        let record = put_profile_object(
            &mut backend,
            "profile-s3-verify",
            &key,
            &mut &b"stable"[..],
            6,
        )
        .expect("put");
        std::fs::write(root.join(record.location), b"changed").expect("tamper");
        assert!(verify_profile_object(&backend, &key).is_err());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn list_page_is_stable_bounded_and_prefix_scoped() {
        let (mut backend, root) = backend();
        for (object_id, body) in [
            ("reads/z", &b"z"[..]),
            ("reads/a", &b"a"[..]),
            ("other/x", &b"x"[..]),
        ] {
            let key = BackendObjectKey {
                object_id: object_id.to_string(),
                version: 1,
            };
            put_profile_object(
                &mut backend,
                &format!("profile-s3-page-{}", key.object_id.replace('/', "-")),
                &key,
                &mut &body[..],
                1,
            )
            .expect("put");
        }
        let first = list_profile_objects_page(&backend, Some("reads/"), 0, 1).expect("page");
        assert_eq!(first.objects[0].key.object_id, "reads/a");
        assert_eq!(first.next_offset, Some(1));
        let second = list_profile_objects_page(&backend, Some("reads/"), 1, 1).expect("page");
        assert_eq!(second.objects[0].key.object_id, "reads/z");
        assert_eq!(second.next_offset, None);
        assert!(list_profile_objects_page(&backend, None, 0, PROFILE_S3_MAX_KEYS + 1).is_err());
        assert!(list_profile_objects_page(&backend, None, 4, 1).is_err());
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
    fn delete_requires_catalogue_authority_and_debits_folder_capacity() {
        let (mut backend, root) = backend();
        let key = BackendObjectKey {
            object_id: "deletes/sample.fastq".to_string(),
            version: 1,
        };
        put_profile_object(
            &mut backend,
            "profile-s3-delete",
            &key,
            &mut &b"delete"[..],
            6,
        )
        .expect("put");
        assert_eq!(backend.capacity().used_bytes, 6);
        assert!(delete_profile_object(&mut backend, &key).expect("delete"));
        assert_eq!(backend.capacity().used_bytes, 0);
        assert!(head_profile_object(&backend, &key).is_err());
        assert!(!delete_profile_object(&mut backend, &key).expect("idempotent delete"));
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

    #[test]
    fn drive_profile_delete_fails_closed_when_guard_is_lost() {
        let (mut backend, guard, root) = drive_backend();
        let key = BackendObjectKey {
            object_id: "drive/delete.fastq".to_string(),
            version: 1,
        };
        put_profile_object(
            &mut backend,
            "profile-s3-drive-delete",
            &key,
            &mut &b"drive"[..],
            5,
        )
        .expect("put");
        guard.0.store(false, std::sync::atomic::Ordering::SeqCst);
        assert!(delete_profile_object(&mut backend, &key).is_err());
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn delete_with_capacity_provider_reconciles_logical_usage() {
        let (mut backend, root) = backend();
        let provider = RecordingCapacityProvider::default();
        let key = BackendObjectKey {
            object_id: "provider/delete.fastq".to_string(),
            version: 1,
        };
        put_profile_object(
            &mut backend,
            "profile-s3-provider-delete",
            &key,
            &mut &b"delete"[..],
            6,
        )
        .expect("put");
        assert!(delete_profile_object_with_capacity_provider(
            &provider,
            "profile-s3",
            &mut backend,
            &key,
        )
        .expect("delete"));
        assert!(provider
            .events
            .lock()
            .expect("events lock")
            .iter()
            .any(|event| event == "reconcile:profile-s3:0"));
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn put_with_capacity_provider_commits_logical_and_backend_reservations() {
        let (mut backend, root) = backend();
        let provider = RecordingCapacityProvider {
            logical_limit_bytes: 4096,
            backend_free_bytes: 4096,
            ssd_free_bytes: 4096,
            ..RecordingCapacityProvider::default()
        };
        let key = BackendObjectKey {
            object_id: "provider/sample.fastq".to_string(),
            version: 1,
        };
        put_profile_object_with_capacity_provider(
            &provider,
            "profile-s3",
            &mut backend,
            "profile-s3-provider",
            &key,
            &mut &b"provider"[..],
            8,
        )
        .expect("provider-backed put");
        assert_eq!(backend.capacity().reserved_bytes, 0);
        assert_eq!(
            *provider.events.lock().expect("events lock"),
            vec![
                "admit:profile-s3:8:profile-s3-provider",
                "commit:profile-s3:profile-s3-provider"
            ]
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn put_with_capacity_provider_releases_both_reservations_before_finalize() {
        let (mut backend, root) = backend();
        let provider = RecordingCapacityProvider::default();
        let key = BackendObjectKey {
            object_id: "provider/mismatch.fastq".to_string(),
            version: 1,
        };
        assert!(put_profile_object_with_capacity_provider(
            &provider,
            "profile-s3",
            &mut backend,
            "profile-s3-provider-mismatch",
            &key,
            &mut &b"provider"[..],
            7,
        )
        .is_err());
        assert_eq!(backend.capacity().reserved_bytes, 0);
        assert_eq!(
            *provider.events.lock().expect("events lock"),
            vec![
                "admit:profile-s3:7:profile-s3-provider-mismatch",
                "release:profile-s3:profile-s3-provider-mismatch"
            ]
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn put_with_capacity_provider_releases_logical_reservation_when_backend_reserve_fails() {
        let (mut backend, root) = backend();
        let provider = RecordingCapacityProvider {
            logical_limit_bytes: 4096,
            backend_free_bytes: 4096,
            ssd_free_bytes: 4096,
            ..RecordingCapacityProvider::default()
        };
        let key = BackendObjectKey {
            object_id: "provider/over-capacity.fastq".to_string(),
            version: 1,
        };
        assert!(put_profile_object_with_capacity_provider(
            &provider,
            "profile-s3",
            &mut backend,
            "profile-s3-provider-over-capacity",
            &key,
            &mut &b"provider"[..],
            2048,
        )
        .is_err());
        assert_eq!(backend.capacity().reserved_bytes, 0);
        assert_eq!(
            *provider.events.lock().expect("events lock"),
            vec![
                "admit:profile-s3:2048:profile-s3-provider-over-capacity",
                "release:profile-s3:profile-s3-provider-over-capacity"
            ]
        );
        std::fs::remove_dir_all(root).ok();
    }
}
