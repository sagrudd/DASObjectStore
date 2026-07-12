//! Daemon-owned capacity admission backed by the store registry and ledger.
//!
//! The provider deliberately owns all usage and free-space observations. A
//! request can ask for a decision, but it cannot provide trusted accounting.

use crate::api::{CapacityAdmissionRequest, CapacityAdmissionResponse};
use crate::runtime::capacity_persistence::{load_capacity_ledger, save_capacity_ledger};
use crate::runtime::service::DaemonServiceRuntimeError;
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
}

impl<P> CapacityAdmissionProvider for FileBackedCapacityAdmissionProvider<P>
where
    P: CapacitySpaceProbe,
{
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
            if let Some(reservation_id) = request.client_request_id.as_deref() {
                let _ = ledger.release(reservation_id);
            }
            return Err(unavailable(format!(
                "capacity ledger persistence failed: {error}"
            )));
        }
        Ok(response)
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
    use crate::api::{CapacityAdmissionDecision, CapacityAdmissionRequest, DaemonIngressOrigin};
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

        let direct = provider
            .admit(request(
                DaemonIngressOrigin::LocalServerDirectImport,
                "upload-2",
            ))
            .expect("direct admitted");
        assert_eq!(direct.ssd_available_bytes, None);
        let _ = std::fs::remove_dir_all(root);
    }
}
