//! Daemon-owned capacity admission backed by the store registry and ledger.
//!
//! The provider deliberately owns all usage and free-space observations. A
//! request can ask for a decision, but it cannot provide trusted accounting.

use crate::api::{
    CapacityAdmissionRequest, CapacityAdmissionResponse, CapacityStatusRequest,
    CapacityStatusResponse,
};
use crate::runtime::capacity_persistence::{load_capacity_ledger, save_capacity_ledger};
use crate::runtime::service::DaemonServiceRuntimeError;
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_core::store::CapacityReservationLedger;
use dasobjectstore_object_service::{default_store_registry_path, read_store_registry};
use std::collections::HashMap;
use std::ffi::CString;
use std::os::raw::c_char;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub trait CapacitySpaceProbe: Send + Sync {
    fn free_bytes(&self, path: &Path) -> Result<u64, String>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct StatvfsCapacitySpaceProbe;

impl CapacitySpaceProbe for StatvfsCapacitySpaceProbe {
    fn free_bytes(&self, path: &Path) -> Result<u64, String> {
        let path = CString::new(path.to_string_lossy().as_bytes())
            .map_err(|_| format!("capacity probe path contains NUL: {}", path.display()))?;
        let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        // SAFETY: `path` is a valid NUL-terminated path and `stats` points to
        // writable storage for libc to initialize on success.
        let result = unsafe { libc::statvfs(path.as_ptr() as *const c_char, stats.as_mut_ptr()) };
        if result != 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        // SAFETY: statvfs initialized stats when it returned zero.
        let stats = unsafe { stats.assume_init() };
        u64::try_from(stats.f_bavail)
            .ok()
            .and_then(|available| {
                u64::try_from(stats.f_frsize)
                    .ok()
                    .and_then(|size| available.checked_mul(size))
            })
            .ok_or_else(|| format!("capacity probe overflow for {}", path.to_string_lossy()))
    }
}

pub trait CapacityAdmissionProvider: Send + Sync {
    fn admit(
        &self,
        request: CapacityAdmissionRequest,
    ) -> Result<CapacityAdmissionResponse, DaemonServiceRuntimeError>;

    fn status(
        &self,
        _request: CapacityStatusRequest,
    ) -> Result<CapacityStatusResponse, DaemonServiceRuntimeError> {
        Err(unavailable("capacity status provider is not configured"))
    }

    fn admit_remote_upload(
        &self,
        object_store: &str,
        requested_bytes: u64,
        reservation_id: &str,
    ) -> Result<CapacityAdmissionResponse, DaemonServiceRuntimeError> {
        self.admit_ingest(
            object_store,
            requested_bytes,
            1,
            crate::api::DaemonIngressOrigin::RemoteS3,
            reservation_id,
        )
    }

    fn admit_ingest(
        &self,
        object_store: &str,
        requested_bytes: u64,
        copy_count: u8,
        ingress_origin: crate::api::DaemonIngressOrigin,
        reservation_id: &str,
    ) -> Result<CapacityAdmissionResponse, DaemonServiceRuntimeError> {
        self.admit(CapacityAdmissionRequest {
            store_id: object_store.to_string(),
            requested_bytes,
            copy_count,
            ingress_origin,
            client_request_id: Some(reservation_id.to_string()),
        })
    }

    fn commit(
        &self,
        _store_id: &StoreId,
        _reservation_id: &str,
    ) -> Result<(), DaemonServiceRuntimeError> {
        Err(unavailable(
            "capacity reservation commit provider is not configured",
        ))
    }

    fn release(
        &self,
        _store_id: &StoreId,
        _reservation_id: &str,
    ) -> Result<(), DaemonServiceRuntimeError> {
        Err(unavailable(
            "capacity reservation release provider is not configured",
        ))
    }
}

pub struct FileBackedCapacityAdmissionProvider<P = StatvfsCapacitySpaceProbe> {
    store_registry_path: PathBuf,
    ledger_directory: PathBuf,
    backend_probe_root: PathBuf,
    ssd_probe_root: PathBuf,
    probe: P,
    ledgers: Mutex<HashMap<String, CapacityReservationLedger>>,
}

impl FileBackedCapacityAdmissionProvider<StatvfsCapacitySpaceProbe> {
    pub fn for_daemon(state_dir: impl AsRef<Path>) -> Self {
        Self::new(
            default_store_registry_path(),
            state_dir.as_ref().join("capacity-ledgers"),
            crate::runtime::default_hdd_root(),
            crate::runtime::default_ssd_root(),
            StatvfsCapacitySpaceProbe,
        )
    }
}

impl<P> FileBackedCapacityAdmissionProvider<P> {
    pub fn new(
        store_registry_path: impl Into<PathBuf>,
        ledger_directory: impl Into<PathBuf>,
        backend_probe_root: impl Into<PathBuf>,
        ssd_probe_root: impl Into<PathBuf>,
        probe: P,
    ) -> Self {
        Self {
            store_registry_path: store_registry_path.into(),
            ledger_directory: ledger_directory.into(),
            backend_probe_root: backend_probe_root.into(),
            ssd_probe_root: ssd_probe_root.into(),
            probe,
            ledgers: Mutex::new(HashMap::new()),
        }
    }

    fn ledger_path(&self, store_id: &str) -> PathBuf {
        self.ledger_directory.join(format!("{store_id}.json"))
    }

    fn policy_for_store(
        &self,
        store_id: &StoreId,
    ) -> Result<dasobjectstore_core::store::CapacityPolicy, DaemonServiceRuntimeError> {
        read_store_registry(&self.store_registry_path)
            .map_err(DaemonServiceRuntimeError::ObjectService)?
            .into_iter()
            .find(|definition| definition.store_id == store_id.clone())
            .map(|definition| definition.policy.capacity)
            .ok_or_else(|| unavailable(format!("unknown object store {store_id}")))
    }

    fn copies_for_store(&self, store_id: &StoreId) -> Result<u8, DaemonServiceRuntimeError> {
        read_store_registry(&self.store_registry_path)
            .map_err(DaemonServiceRuntimeError::ObjectService)?
            .into_iter()
            .find(|definition| definition.store_id == store_id.clone())
            .map(|definition| definition.policy.copies)
            .ok_or_else(|| unavailable(format!("unknown object store {store_id}")))
    }

    fn ledger_for_store<'a>(
        &'a self,
        ledgers: &'a mut HashMap<String, CapacityReservationLedger>,
        store_id: &StoreId,
        policy: dasobjectstore_core::store::CapacityPolicy,
    ) -> Result<&'a mut CapacityReservationLedger, DaemonServiceRuntimeError> {
        if !ledgers.contains_key(store_id.as_str()) {
            let ledger = self.load_or_initialize(store_id.as_str(), policy)?;
            ledgers.insert(store_id.as_str().to_string(), ledger);
        }
        ledgers
            .get_mut(store_id.as_str())
            .ok_or_else(|| unavailable("capacity ledger disappeared during lookup"))
    }

    fn load_or_initialize(
        &self,
        store_id: &str,
        policy: dasobjectstore_core::store::CapacityPolicy,
    ) -> Result<CapacityReservationLedger, DaemonServiceRuntimeError> {
        let path = self.ledger_path(store_id);
        if !path.exists() {
            if policy.logical_limit_bytes.is_some() {
                return Err(unavailable(format!(
                    "capacity ledger is not initialized for bounded store {store_id}"
                )));
            }
            return CapacityReservationLedger::new(policy, 0).map_err(|error| {
                unavailable(format!("capacity ledger initialization failed: {error:?}"))
            });
        }
        match load_capacity_ledger(&path) {
            Ok(ledger) => Ok(ledger),
            Err(error) => Err(unavailable(format!("capacity ledger load failed: {error}"))),
        }
    }

    /// Reclaim only reservations with durable creation timestamps older than
    /// the caller-supplied lease window. Legacy reservations without age
    /// metadata are retained, so upgrades cannot reclaim an active transfer
    /// whose start time is unknown. The caller owns scheduling and lease
    /// policy; no background expiry is started implicitly by the provider.
    pub fn expire_stale_reservations(
        &self,
        store_id: &StoreId,
        now_unix_seconds: u64,
        max_age_seconds: u64,
    ) -> Result<u64, DaemonServiceRuntimeError> {
        let policy = self.policy_for_store(store_id)?;
        let mut ledgers = self
            .ledgers
            .lock()
            .map_err(|_| unavailable("capacity ledger lock poisoned"))?;
        let ledger = self.ledger_for_store(&mut ledgers, store_id, policy.clone())?;
        let before = ledger.clone();
        ledger
            .update_policy(policy)
            .map_err(|error| unavailable(format!("capacity policy update failed: {error:?}")))?;
        let reclaimed_bytes = ledger
            .expire_reservations(now_unix_seconds, max_age_seconds)
            .into_iter()
            .map(|(_, bytes)| bytes)
            .sum();
        if reclaimed_bytes == 0 {
            return Ok(0);
        }
        if let Err(error) = save_capacity_ledger(self.ledger_path(store_id.as_str()), ledger) {
            *ledger = before;
            return Err(unavailable(format!(
                "capacity ledger persistence failed: {error}"
            )));
        }
        Ok(reclaimed_bytes)
    }
}

impl<P> CapacityAdmissionProvider for FileBackedCapacityAdmissionProvider<P>
where
    P: CapacitySpaceProbe,
{
    fn status(
        &self,
        request: CapacityStatusRequest,
    ) -> Result<CapacityStatusResponse, DaemonServiceRuntimeError> {
        let store_id = request.validate().map_err(|error| {
            DaemonServiceRuntimeError::Validation(
                crate::api::DaemonRequestValidationError::UnsupportedFieldValue {
                    field: "capacity_status",
                    value: error.to_string(),
                },
            )
        })?;
        let definitions = read_store_registry(&self.store_registry_path)
            .map_err(DaemonServiceRuntimeError::ObjectService)?;
        let definition = definitions
            .into_iter()
            .find(|definition| definition.store_id == store_id)
            .ok_or_else(|| unavailable(format!("unknown object store {store_id}")))?;
        let mut ledgers = self
            .ledgers
            .lock()
            .map_err(|_| unavailable("capacity ledger lock poisoned"))?;
        let ledger = if let Some(ledger) = ledgers.get_mut(store_id.as_str()) {
            ledger
        } else {
            let ledger =
                self.load_or_initialize(store_id.as_str(), definition.policy.capacity.clone())?;
            ledgers.insert(store_id.as_str().to_string(), ledger);
            ledgers
                .get_mut(store_id.as_str())
                .expect("ledger inserted before lookup")
        };
        ledger
            .update_policy(definition.policy.capacity.clone())
            .map_err(|error| unavailable(format!("capacity policy update failed: {error:?}")))?;
        let backend_free_bytes = self
            .probe
            .free_bytes(&self.backend_probe_root)
            .map_err(|error| unavailable(format!("backend capacity probe failed: {error}")))?;
        let requires_ssd_staging =
            definition.policy.ingest_mode != dasobjectstore_core::store::IngestMode::DirectToHdd;
        let ssd_free_bytes = if requires_ssd_staging {
            self.probe
                .free_bytes(&self.ssd_probe_root)
                .map_err(|error| unavailable(format!("SSD capacity probe failed: {error}")))?
        } else {
            0
        };
        CapacityStatusResponse::from_ledger(
            &request,
            &definition.policy.capacity,
            ledger,
            definition.policy.copies,
            requires_ssd_staging,
            backend_free_bytes,
            ssd_free_bytes,
        )
        .map_err(|error| unavailable(format!("capacity status failed: {error}")))
    }

    fn admit(
        &self,
        request: CapacityAdmissionRequest,
    ) -> Result<CapacityAdmissionResponse, DaemonServiceRuntimeError> {
        let store_id = request.validate().map_err(|error| {
            DaemonServiceRuntimeError::Validation(
                crate::api::DaemonRequestValidationError::UnsupportedFieldValue {
                    field: "capacity_admission",
                    value: error.to_string(),
                },
            )
        })?;
        let definitions = read_store_registry(&self.store_registry_path)
            .map_err(DaemonServiceRuntimeError::ObjectService)?;
        let definition = definitions
            .into_iter()
            .find(|definition| definition.store_id == store_id)
            .ok_or_else(|| unavailable(format!("unknown object store {store_id}")))?;
        if request.copy_count != definition.policy.copies {
            return Err(unavailable(format!(
                "copy_count {} does not match daemon policy {}",
                request.copy_count, definition.policy.copies
            )));
        }

        let mut ledgers = self
            .ledgers
            .lock()
            .map_err(|_| unavailable("capacity ledger lock poisoned"))?;
        let ledger = if let Some(ledger) = ledgers.get_mut(store_id.as_str()) {
            ledger
        } else {
            let ledger =
                self.load_or_initialize(store_id.as_str(), definition.policy.capacity.clone())?;
            ledgers.insert(store_id.as_str().to_string(), ledger);
            ledgers
                .get_mut(store_id.as_str())
                .expect("ledger inserted before lookup")
        };
        let before = ledger.clone();
        ledger
            .update_policy(definition.policy.capacity.clone())
            .map_err(|error| unavailable(format!("capacity policy update failed: {error:?}")))?;

        let backend_free_bytes = self
            .probe
            .free_bytes(&self.backend_probe_root)
            .map_err(|error| unavailable(format!("backend capacity probe failed: {error}")))?;
        let ssd_free_bytes = if request.requires_ssd_staging() {
            self.probe
                .free_bytes(&self.ssd_probe_root)
                .map_err(|error| unavailable(format!("SSD capacity probe failed: {error}")))?
        } else {
            0
        };

        let response = match CapacityAdmissionResponse::evaluate_and_reserve(
            &request,
            &definition.policy.capacity,
            ledger,
            backend_free_bytes,
            ssd_free_bytes,
        ) {
            Ok(response) => response,
            Err(crate::api::CapacityAdmissionReservationError::Rejected(response)) => {
                return Ok(response)
            }
            Err(error) => return Err(unavailable(format!("capacity admission failed: {error}"))),
        };
        if let Err(error) = save_capacity_ledger(self.ledger_path(store_id.as_str()), ledger) {
            *ledger = before;
            return Err(unavailable(format!(
                "capacity ledger persistence failed: {error}"
            )));
        }
        Ok(response)
    }

    fn admit_remote_upload(
        &self,
        object_store: &str,
        requested_bytes: u64,
        reservation_id: &str,
    ) -> Result<CapacityAdmissionResponse, DaemonServiceRuntimeError> {
        let store_id = StoreId::new(object_store.to_string())
            .map_err(|error| unavailable(format!("invalid object store: {error}")))?;
        let copy_count = self.copies_for_store(&store_id)?;
        self.admit_ingest(
            object_store,
            requested_bytes,
            copy_count,
            crate::api::DaemonIngressOrigin::RemoteS3,
            reservation_id,
        )
    }

    fn commit(
        &self,
        store_id: &StoreId,
        reservation_id: &str,
    ) -> Result<(), DaemonServiceRuntimeError> {
        let policy = self.policy_for_store(store_id)?;
        let mut ledgers = self
            .ledgers
            .lock()
            .map_err(|_| unavailable("capacity ledger lock poisoned"))?;
        let ledger = self.ledger_for_store(&mut ledgers, store_id, policy.clone())?;
        let before = ledger.clone();
        ledger
            .update_policy(policy)
            .map_err(|error| unavailable(format!("capacity policy update failed: {error:?}")))?;
        ledger.commit(reservation_id).map_err(|error| {
            unavailable(format!("capacity reservation commit failed: {error:?}"))
        })?;
        if let Err(error) = save_capacity_ledger(self.ledger_path(store_id.as_str()), ledger) {
            *ledger = before;
            return Err(unavailable(format!(
                "capacity ledger persistence failed: {error}"
            )));
        }
        Ok(())
    }

    fn release(
        &self,
        store_id: &StoreId,
        reservation_id: &str,
    ) -> Result<(), DaemonServiceRuntimeError> {
        let policy = self.policy_for_store(store_id)?;
        let mut ledgers = self
            .ledgers
            .lock()
            .map_err(|_| unavailable("capacity ledger lock poisoned"))?;
        let ledger = self.ledger_for_store(&mut ledgers, store_id, policy.clone())?;
        let before = ledger.clone();
        ledger
            .update_policy(policy)
            .map_err(|error| unavailable(format!("capacity policy update failed: {error:?}")))?;
        ledger.release(reservation_id).map_err(|error| {
            unavailable(format!("capacity reservation release failed: {error:?}"))
        })?;
        if let Err(error) = save_capacity_ledger(self.ledger_path(store_id.as_str()), ledger) {
            *ledger = before;
            return Err(unavailable(format!(
                "capacity ledger persistence failed: {error}"
            )));
        }
        Ok(())
    }
}

fn unavailable(message: impl Into<String>) -> DaemonServiceRuntimeError {
    DaemonServiceRuntimeError::UnsupportedOperation {
        operation: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CapacityAdmissionProvider, CapacitySpaceProbe, FileBackedCapacityAdmissionProvider,
    };
    use crate::api::{
        CapacityAdmissionDecision, CapacityAdmissionRejectionReason, CapacityAdmissionRequest,
        CapacityStatusRequest, DaemonIngressOrigin,
    };
    use crate::runtime::{load_capacity_ledger, save_capacity_ledger};
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_core::store::{
        CapacityPolicy, CapacityReservationLedger, StoreClass, StorePolicy,
    };
    use dasobjectstore_object_service::StoreServiceDefinition;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    #[derive(Clone, Copy)]
    struct FixedProbe {
        backend: u64,
        ssd: u64,
    }

    impl CapacitySpaceProbe for FixedProbe {
        fn free_bytes(&self, path: &Path) -> Result<u64, String> {
            if path.ends_with("ssd") {
                Ok(self.ssd)
            } else {
                Ok(self.backend)
            }
        }
    }

    fn root(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let parent = std::env::var_os("DASOBJECTSTORE_CODEX_VALIDATION_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("dasobjectstore-codex-validation"));
        parent.join(format!(
            "capacity-provider-{label}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn request(origin: DaemonIngressOrigin, id: &str) -> CapacityAdmissionRequest {
        CapacityAdmissionRequest {
            store_id: "codex".to_string(),
            requested_bytes: 100,
            copy_count: 2,
            ingress_origin: origin,
            client_request_id: Some(id.to_string()),
        }
    }

    fn registry(root: &Path) -> (PathBuf, PathBuf) {
        let registry = root.join("stores.json");
        let definition = StoreServiceDefinition {
            store_id: StoreId::new("codex").expect("store id"),
            policy: StorePolicy {
                capacity: CapacityPolicy::bounded(1_000, 100),
                ..StorePolicy::defaults_for(StoreClass::GeneratedData)
            },
            bucket_name: None,
            reader_group: None,
            writer_group: None,
            public: true,
        };
        std::fs::create_dir_all(root).expect("registry dir");
        std::fs::write(
            &registry,
            serde_json::to_vec(&[definition]).expect("registry JSON"),
        )
        .expect("registry");
        (registry, root.join("subobjects.json"))
    }

    #[test]
    fn provider_reserves_and_persists_daemon_owned_observations() {
        let root = root("admit");
        let (registry_path, _) = registry(&root);
        let ledger_dir = root.join("ledgers");
        let ledger_path = ledger_dir.join("codex.json");
        let ledger =
            CapacityReservationLedger::new(CapacityPolicy::bounded(1_000, 100), 0).expect("ledger");
        save_capacity_ledger(&ledger_path, &ledger).expect("ledger seed");
        let provider = FileBackedCapacityAdmissionProvider::new(
            registry_path,
            ledger_dir,
            root.join("backend"),
            root.join("ssd"),
            FixedProbe {
                backend: 2_000,
                ssd: 500,
            },
        );

        let response = provider
            .admit(request(DaemonIngressOrigin::RemoteS3, "upload-1"))
            .expect("admitted");
        assert_eq!(response.decision, CapacityAdmissionDecision::Admitted);
        assert_eq!(response.ssd_available_bytes, Some(500));
        assert_eq!(
            load_capacity_ledger(&ledger_path)
                .expect("ledger reload")
                .reserved_bytes(),
            100
        );
        assert!(provider
            .admit(request(DaemonIngressOrigin::RemoteS3, "upload-1"))
            .is_err());
        assert_eq!(
            load_capacity_ledger(&ledger_path)
                .expect("ledger reload")
                .reserved_bytes(),
            100
        );
        let store_id = StoreId::new("codex").expect("store id");
        provider
            .commit(&store_id, "upload-1")
            .expect("reservation commits");
        let committed = load_capacity_ledger(&ledger_path).expect("committed ledger reload");
        assert_eq!(committed.reserved_bytes(), 0);
        assert_eq!(committed.used_bytes(), 100);

        let direct = provider
            .admit(request(
                DaemonIngressOrigin::LocalServerDirectImport,
                "upload-2",
            ))
            .expect("direct admitted");
        assert_eq!(direct.ssd_available_bytes, None);
        provider
            .release(&store_id, "upload-2")
            .expect("reservation releases");
        let released = load_capacity_ledger(&ledger_path).expect("released ledger reload");
        assert_eq!(released.reserved_bytes(), 0);
        assert_eq!(released.used_bytes(), 100);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn provider_reports_read_only_status_without_reserving() {
        let root = root("status");
        let (registry_path, _) = registry(&root);
        let ledger_dir = root.join("ledgers");
        let ledger_path = ledger_dir.join("codex.json");
        let ledger = CapacityReservationLedger::new(CapacityPolicy::bounded(1_000, 100), 900)
            .expect("ledger");
        save_capacity_ledger(&ledger_path, &ledger).expect("ledger seed");
        let provider = FileBackedCapacityAdmissionProvider::new(
            registry_path,
            ledger_dir,
            root.join("backend"),
            root.join("ssd"),
            FixedProbe {
                backend: 2_000,
                ssd: 500,
            },
        );
        let status = provider
            .status(CapacityStatusRequest {
                store_id: "codex".to_string(),
            })
            .expect("status available");
        assert_eq!(status.used_bytes, 900);
        assert_eq!(status.reserved_bytes, 0);
        assert_eq!(status.logical_available_bytes, Some(0));
        assert_eq!(
            status.admission_block_reason,
            Some(CapacityAdmissionRejectionReason::LogicalQuota)
        );
        assert_eq!(
            load_capacity_ledger(&ledger_path)
                .expect("ledger reload")
                .reserved_bytes(),
            0
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn statvfs_provider_fixture_uses_real_filesystem_capacity() {
        let root = root("statvfs");
        let (registry_path, _) = registry(&root);
        let ledger_dir = root.join("ledgers");
        let ledger_path = ledger_dir.join("codex.json");
        let ledger =
            CapacityReservationLedger::new(CapacityPolicy::bounded(1_000, 100), 0).expect("ledger");
        save_capacity_ledger(&ledger_path, &ledger).expect("ledger seed");
        let backend_root = root.join("backend");
        let ssd_root = root.join("ssd");
        std::fs::create_dir_all(&backend_root).expect("backend root");
        std::fs::create_dir_all(&ssd_root).expect("ssd root");

        let provider = FileBackedCapacityAdmissionProvider::new(
            registry_path,
            ledger_dir,
            backend_root,
            ssd_root,
            super::StatvfsCapacitySpaceProbe,
        );
        let response = provider
            .admit(request(DaemonIngressOrigin::RemoteS3, "statvfs-upload"))
            .expect("real filesystem has capacity");

        assert_eq!(response.decision, CapacityAdmissionDecision::Admitted);
        assert!(response.backend_free_bytes > 0);
        assert!(response.backend_available_bytes > 0);
        assert!(response.ssd_available_bytes.is_some_and(|bytes| bytes > 0));

        provider
            .commit(&StoreId::new("codex").expect("store id"), "statvfs-upload")
            .expect("reservation commits");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn provider_expiry_reclaims_only_stale_durable_reservations() {
        let root = root("expiry");
        let (registry_path, _) = registry(&root);
        let ledger_dir = root.join("ledgers");
        let ledger_path = ledger_dir.join("codex.json");
        let mut ledger =
            CapacityReservationLedger::new(CapacityPolicy::bounded(1_000, 100), 0).expect("ledger");
        ledger
            .reserve_at_unix_seconds("stale", 100, 100)
            .expect("stale reservation");
        ledger
            .reserve_at_unix_seconds("active", 200, 190)
            .expect("active reservation");
        save_capacity_ledger(&ledger_path, &ledger).expect("ledger seed");

        let provider = FileBackedCapacityAdmissionProvider::new(
            registry_path.clone(),
            ledger_dir.clone(),
            root.join("backend"),
            root.join("ssd"),
            FixedProbe {
                backend: 2_000,
                ssd: 500,
            },
        );
        let store_id = StoreId::new("codex").expect("store id");
        assert_eq!(
            provider
                .expire_stale_reservations(&store_id, 200, 100)
                .expect("expiry succeeds"),
            100
        );
        let restored = load_capacity_ledger(&ledger_path).expect("ledger reload");
        assert_eq!(restored.reserved_bytes(), 200);
        assert_eq!(restored.reservation_bytes("stale"), None);
        assert_eq!(restored.reservation_bytes("active"), Some(200));

        let restarted = FileBackedCapacityAdmissionProvider::new(
            registry_path,
            ledger_dir,
            root.join("backend"),
            root.join("ssd"),
            FixedProbe {
                backend: 2_000,
                ssd: 500,
            },
        );
        assert_eq!(
            restarted
                .expire_stale_reservations(&store_id, 290, 100)
                .expect("boundary expiry succeeds"),
            200
        );
        assert_eq!(
            load_capacity_ledger(&ledger_path)
                .expect("final ledger reload")
                .reserved_bytes(),
            0
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
