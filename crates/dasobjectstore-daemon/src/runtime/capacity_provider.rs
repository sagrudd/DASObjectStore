//! Daemon-owned capacity admission backed by the store registry and ledger.
//!
//! The provider deliberately owns all usage and free-space observations. A
//! request can ask for a decision, but it cannot provide trusted accounting.

use crate::api::{
    CapacityAdmissionRequest, CapacityAdmissionResponse, CapacityStatusRequest,
    CapacityStatusResponse,
};
use crate::runtime::capacity_lease::{
    protections_by_store, reservation_id_digest, CapacityReservationLeaseAction,
    CapacityReservationLeaseEvent, CapacityReservationLeaseProtection,
    CapacityReservationLeaseReport,
};
use crate::runtime::capacity_persistence::{load_capacity_ledger, save_capacity_ledger};
use crate::runtime::profile_registry::{profile_binding_registry_path, read_profile_binding};
use crate::runtime::service::DaemonServiceRuntimeError;
use dasobjectstore_core::ids::StoreId;
use dasobjectstore_core::store::CapacityReservationLedger;
use dasobjectstore_object_service::{default_store_registry_path, read_store_registry};
use std::collections::{HashMap, HashSet};
use std::ffi::CString;
use std::fs;
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
    /// Prepare the durable ledger for a newly-created store before its
    /// registry definition becomes visible. Implementations must be
    /// idempotent and must never overwrite existing usage or reservations.
    fn initialize_store(
        &self,
        _store_id: &StoreId,
        _policy: dasobjectstore_core::store::CapacityPolicy,
    ) -> Result<bool, DaemonServiceRuntimeError> {
        Ok(false)
    }

    /// Remove a ledger created by a failed multi-authority provisioning
    /// transaction. Implementations must reject non-empty ledgers.
    fn rollback_initialized_store(
        &self,
        _store_id: &StoreId,
    ) -> Result<(), DaemonServiceRuntimeError> {
        Ok(())
    }

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

    fn reconcile_used_bytes(
        &self,
        _store_id: &StoreId,
        _used_bytes: u64,
    ) -> Result<(), DaemonServiceRuntimeError> {
        Err(unavailable(
            "capacity usage reconciliation provider is not configured",
        ))
    }
}

pub struct FileBackedCapacityAdmissionProvider<P = StatvfsCapacitySpaceProbe> {
    store_registry_path: PathBuf,
    ledger_directory: PathBuf,
    backend_probe_root: PathBuf,
    ssd_probe_root: PathBuf,
    profile_binding_registry_path: Option<PathBuf>,
    require_profile_binding: bool,
    probe: P,
    state: Mutex<CapacityProviderState>,
}

#[derive(Default)]
struct CapacityProviderState {
    ledgers: HashMap<String, CapacityReservationLedger>,
    active_reservations: HashMap<String, HashSet<String>>,
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
        .with_profile_binding_registry_path(profile_binding_registry_path(state_dir))
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
            profile_binding_registry_path: None,
            require_profile_binding: false,
            probe,
            state: Mutex::new(CapacityProviderState::default()),
        }
    }

    pub fn with_profile_binding_registry_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.profile_binding_registry_path = Some(path.into());
        self
    }

    pub fn require_profile_binding(mut self) -> Self {
        self.require_profile_binding = true;
        self
    }

    fn probe_roots(
        &self,
        store_id: &StoreId,
    ) -> Result<(PathBuf, PathBuf), DaemonServiceRuntimeError> {
        let Some(registry_path) = &self.profile_binding_registry_path else {
            if self.require_profile_binding {
                return Err(unavailable("profile binding registry is not configured"));
            }
            return Ok((self.backend_probe_root.clone(), self.ssd_probe_root.clone()));
        };
        let Some(binding) = read_profile_binding(registry_path, store_id.as_str())? else {
            if self.require_profile_binding {
                return Err(unavailable(format!(
                    "profile binding is missing for object store {store_id}"
                )));
            }
            return Ok((self.backend_probe_root.clone(), self.ssd_probe_root.clone()));
        };
        let staging_root = binding
            .ssd_staging_root
            .clone()
            .unwrap_or_else(|| binding.backend_root.clone());
        Ok((binding.backend_root, staging_root))
    }

    fn ledger_path(&self, store_id: &str) -> PathBuf {
        self.ledger_directory.join(format!("{store_id}.json"))
    }

    fn initialize_ledger(
        &self,
        store_id: &StoreId,
        policy: dasobjectstore_core::store::CapacityPolicy,
    ) -> Result<bool, DaemonServiceRuntimeError> {
        let path = self.ledger_path(store_id.as_str());
        if path.exists() {
            let existing = load_capacity_ledger(&path)
                .map_err(|error| unavailable(format!("capacity ledger load failed: {error}")))?;
            if existing.policy() != &policy {
                return Err(unavailable(format!(
                    "capacity ledger policy conflicts with requested policy for {store_id}"
                )));
            }
            return Ok(false);
        }

        let ledger = CapacityReservationLedger::new(policy, 0).map_err(|error| {
            unavailable(format!("capacity ledger initialization failed: {error:?}"))
        })?;
        save_capacity_ledger(path, &ledger)
            .map_err(|error| unavailable(format!("capacity ledger persistence failed: {error}")))?;
        Ok(true)
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
        let mut state = self
            .state
            .lock()
            .map_err(|_| unavailable("capacity ledger lock poisoned"))?;
        let ledger = self.ledger_for_store(&mut state.ledgers, store_id, policy.clone())?;
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

    /// Run one deterministic lease-maintenance pass over every registered
    /// store. Current-process reservations and caller-supplied durable active
    /// IDs are renewed before expiry. Legacy reservations whose timestamp is
    /// zero are retained unless an active authority explicitly renews them.
    pub fn maintain_reservation_leases(
        &self,
        now_unix_seconds: u64,
        lease_seconds: u64,
        protections: &[CapacityReservationLeaseProtection],
    ) -> Result<CapacityReservationLeaseReport, DaemonServiceRuntimeError> {
        let mut definitions = read_store_registry(&self.store_registry_path)
            .map_err(DaemonServiceRuntimeError::ObjectService)?;
        definitions.sort_by(|left, right| left.store_id.as_str().cmp(right.store_id.as_str()));
        let supplied = protections_by_store(protections);
        let mut state = self
            .state
            .lock()
            .map_err(|_| unavailable("capacity ledger lock poisoned"))?;
        let mut report = CapacityReservationLeaseReport::default();

        for definition in definitions {
            let store_id = definition.store_id;
            let store_key = store_id.as_str().to_string();
            let mut protected = supplied.get(&store_key).cloned().unwrap_or_default();
            protected.extend(
                state
                    .active_reservations
                    .get(&store_key)
                    .into_iter()
                    .flatten()
                    .cloned(),
            );
            let ledger = self.ledger_for_store(
                &mut state.ledgers,
                &store_id,
                definition.policy.capacity.clone(),
            )?;
            let before = ledger.clone();
            ledger
                .update_policy(definition.policy.capacity)
                .map_err(|error| {
                    unavailable(format!("capacity policy update failed: {error:?}"))
                })?;
            let snapshot = ledger.snapshot_with_expiry();

            let mut protected_ids = protected.iter().collect::<Vec<_>>();
            protected_ids.sort();
            for reservation_id in protected_ids {
                let Some(bytes) = ledger.reservation_bytes(reservation_id) else {
                    continue;
                };
                ledger
                    .renew_reservation_at_unix_seconds(reservation_id, now_unix_seconds)
                    .map_err(|error| {
                        unavailable(format!("capacity reservation renewal failed: {error:?}"))
                    })?;
                report.renewed_reservations += 1;
                report.events.push(CapacityReservationLeaseEvent {
                    store_id: store_id.clone(),
                    reservation_id_sha256: reservation_id_digest(reservation_id),
                    action: CapacityReservationLeaseAction::Renewed,
                    bytes,
                });
            }

            for (reservation_id, bytes) in &snapshot.reservations {
                if protected.contains(reservation_id) {
                    continue;
                }
                if snapshot
                    .reservation_created_at_unix_seconds
                    .get(reservation_id)
                    .copied()
                    .unwrap_or_default()
                    == 0
                {
                    report.legacy_reservations_retained += 1;
                    report.events.push(CapacityReservationLeaseEvent {
                        store_id: store_id.clone(),
                        reservation_id_sha256: reservation_id_digest(reservation_id),
                        action: CapacityReservationLeaseAction::LegacyRetained,
                        bytes: *bytes,
                    });
                }
            }

            let expired = ledger.expire_reservations(now_unix_seconds, lease_seconds);
            for (reservation_id, bytes) in expired {
                report.expired_reservations += 1;
                report.reclaimed_bytes = report.reclaimed_bytes.saturating_add(bytes);
                report.events.push(CapacityReservationLeaseEvent {
                    store_id: store_id.clone(),
                    reservation_id_sha256: reservation_id_digest(&reservation_id),
                    action: CapacityReservationLeaseAction::Expired,
                    bytes,
                });
            }

            if ledger != &before {
                if let Err(error) = save_capacity_ledger(self.ledger_path(&store_key), ledger) {
                    *ledger = before;
                    return Err(unavailable(format!(
                        "capacity ledger persistence failed: {error}"
                    )));
                }
            }
            report.stores_scanned += 1;
        }
        Ok(report)
    }

    /// Discover durable multipart authorities from registered profile roots,
    /// then execute one lease pass. Missing profile configuration fails closed:
    /// expiry must never run when durable active work cannot be enumerated.
    pub fn maintain_registered_reservation_leases(
        &self,
        now_unix_seconds: u64,
        lease_seconds: u64,
    ) -> Result<CapacityReservationLeaseReport, DaemonServiceRuntimeError> {
        let profile_registry_path = self
            .profile_binding_registry_path
            .as_ref()
            .ok_or_else(|| unavailable("profile binding registry is not configured"))?;
        let definitions = read_store_registry(&self.store_registry_path)
            .map_err(DaemonServiceRuntimeError::ObjectService)?;
        let mut protections = Vec::with_capacity(definitions.len());
        for definition in definitions {
            let store_id = definition.store_id;
            let binding = read_profile_binding(profile_registry_path, store_id.as_str())?
                .ok_or_else(|| {
                    unavailable(format!(
                        "profile binding is missing for object store {store_id}"
                    ))
                })?;
            let reservation_ids =
                crate::runtime::profile_s3_multipart::discover_multipart_reservation_ids(
                    &binding.backend_root,
                    store_id.as_str(),
                )
                .map_err(|error| {
                    unavailable(format!(
                    "multipart reservation discovery failed for object store {store_id}: {error}"
                ))
                })?;
            protections.push(CapacityReservationLeaseProtection {
                store_id,
                reservation_ids,
            });
        }
        self.maintain_reservation_leases(now_unix_seconds, lease_seconds, &protections)
    }
}

impl<P> CapacityAdmissionProvider for FileBackedCapacityAdmissionProvider<P>
where
    P: CapacitySpaceProbe,
{
    fn initialize_store(
        &self,
        store_id: &StoreId,
        policy: dasobjectstore_core::store::CapacityPolicy,
    ) -> Result<bool, DaemonServiceRuntimeError> {
        self.initialize_ledger(store_id, policy)
    }

    fn rollback_initialized_store(
        &self,
        store_id: &StoreId,
    ) -> Result<(), DaemonServiceRuntimeError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| unavailable("capacity ledger lock poisoned"))?;
        let path = self.ledger_path(store_id.as_str());
        let ledger = load_capacity_ledger(&path).map_err(|error| {
            unavailable(format!("capacity ledger rollback load failed: {error}"))
        })?;
        if ledger.used_bytes() != 0 || ledger.reserved_bytes() != 0 {
            return Err(unavailable(format!(
                "refusing to roll back non-empty capacity ledger for {store_id}"
            )));
        }
        if state
            .active_reservations
            .get(store_id.as_str())
            .is_some_and(|reservations| !reservations.is_empty())
        {
            return Err(unavailable(format!(
                "refusing to roll back active capacity ledger for {store_id}"
            )));
        }
        fs::remove_file(&path).map_err(|error| {
            unavailable(format!(
                "capacity ledger rollback failed for {store_id}: {error}"
            ))
        })?;
        state.ledgers.remove(store_id.as_str());
        state.active_reservations.remove(store_id.as_str());
        Ok(())
    }

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
        let mut state = self
            .state
            .lock()
            .map_err(|_| unavailable("capacity ledger lock poisoned"))?;
        let ledger = if let Some(ledger) = state.ledgers.get_mut(store_id.as_str()) {
            ledger
        } else {
            let ledger =
                self.load_or_initialize(store_id.as_str(), definition.policy.capacity.clone())?;
            state.ledgers.insert(store_id.as_str().to_string(), ledger);
            state
                .ledgers
                .get_mut(store_id.as_str())
                .expect("ledger inserted before lookup")
        };
        ledger
            .update_policy(definition.policy.capacity.clone())
            .map_err(|error| unavailable(format!("capacity policy update failed: {error:?}")))?;
        let (backend_probe_root, ssd_probe_root) = self.probe_roots(&store_id)?;
        let backend_free_bytes = self
            .probe
            .free_bytes(&backend_probe_root)
            .map_err(|error| unavailable(format!("backend capacity probe failed: {error}")))?;
        let requires_ssd_staging =
            definition.policy.ingest_mode != dasobjectstore_core::store::IngestMode::DirectToHdd;
        let ssd_free_bytes = if requires_ssd_staging {
            self.probe
                .free_bytes(&ssd_probe_root)
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
        let reservation_id = request.client_request_id.clone();
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

        let mut state = self
            .state
            .lock()
            .map_err(|_| unavailable("capacity ledger lock poisoned"))?;
        let ledger = if let Some(ledger) = state.ledgers.get_mut(store_id.as_str()) {
            ledger
        } else {
            let ledger =
                self.load_or_initialize(store_id.as_str(), definition.policy.capacity.clone())?;
            state.ledgers.insert(store_id.as_str().to_string(), ledger);
            state
                .ledgers
                .get_mut(store_id.as_str())
                .expect("ledger inserted before lookup")
        };
        let before = ledger.clone();
        ledger
            .update_policy(definition.policy.capacity.clone())
            .map_err(|error| unavailable(format!("capacity policy update failed: {error:?}")))?;

        let (backend_probe_root, ssd_probe_root) = self.probe_roots(&store_id)?;
        let backend_free_bytes = self
            .probe
            .free_bytes(&backend_probe_root)
            .map_err(|error| unavailable(format!("backend capacity probe failed: {error}")))?;
        let ssd_free_bytes = if request.requires_ssd_staging() {
            self.probe
                .free_bytes(&ssd_probe_root)
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
        if let Some(reservation_id) = reservation_id {
            state
                .active_reservations
                .entry(store_id.as_str().to_string())
                .or_default()
                .insert(reservation_id);
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
        let mut state = self
            .state
            .lock()
            .map_err(|_| unavailable("capacity ledger lock poisoned"))?;
        {
            let ledger = self.ledger_for_store(&mut state.ledgers, store_id, policy.clone())?;
            let before = ledger.clone();
            ledger.update_policy(policy).map_err(|error| {
                unavailable(format!("capacity policy update failed: {error:?}"))
            })?;
            ledger.commit(reservation_id).map_err(|error| {
                unavailable(format!("capacity reservation commit failed: {error:?}"))
            })?;
            if let Err(error) = save_capacity_ledger(self.ledger_path(store_id.as_str()), ledger) {
                *ledger = before;
                return Err(unavailable(format!(
                    "capacity ledger persistence failed: {error}"
                )));
            }
        }
        remove_active_reservation(&mut state, store_id, reservation_id);
        Ok(())
    }

    fn release(
        &self,
        store_id: &StoreId,
        reservation_id: &str,
    ) -> Result<(), DaemonServiceRuntimeError> {
        let policy = self.policy_for_store(store_id)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| unavailable("capacity ledger lock poisoned"))?;
        {
            let ledger = self.ledger_for_store(&mut state.ledgers, store_id, policy.clone())?;
            let before = ledger.clone();
            ledger.update_policy(policy).map_err(|error| {
                unavailable(format!("capacity policy update failed: {error:?}"))
            })?;
            ledger.release(reservation_id).map_err(|error| {
                unavailable(format!("capacity reservation release failed: {error:?}"))
            })?;
            if let Err(error) = save_capacity_ledger(self.ledger_path(store_id.as_str()), ledger) {
                *ledger = before;
                return Err(unavailable(format!(
                    "capacity ledger persistence failed: {error}"
                )));
            }
        }
        remove_active_reservation(&mut state, store_id, reservation_id);
        Ok(())
    }

    fn reconcile_used_bytes(
        &self,
        store_id: &StoreId,
        used_bytes: u64,
    ) -> Result<(), DaemonServiceRuntimeError> {
        let policy = self.policy_for_store(store_id)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| unavailable("capacity ledger lock poisoned"))?;
        let ledger = self.ledger_for_store(&mut state.ledgers, store_id, policy.clone())?;
        let before = ledger.clone();
        ledger
            .update_policy(policy)
            .map_err(|error| unavailable(format!("capacity policy update failed: {error:?}")))?;
        ledger.reconcile_used_bytes(used_bytes).map_err(|error| {
            unavailable(format!("capacity usage reconciliation failed: {error:?}"))
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

fn remove_active_reservation(
    state: &mut CapacityProviderState,
    store_id: &StoreId,
    reservation_id: &str,
) {
    let Some(active) = state.active_reservations.get_mut(store_id.as_str()) else {
        return;
    };
    active.remove(reservation_id);
    if active.is_empty() {
        state.active_reservations.remove(store_id.as_str());
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
    use dasobjectstore_core::deployment::{DeploymentProfile, HostMode};
    use dasobjectstore_core::ids::StoreId;
    use dasobjectstore_core::manifest::{
        BackendReference, ObjectStoreManifest, OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
    };
    use dasobjectstore_core::protection::ProtectionPolicy;
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

    #[derive(Clone, Copy)]
    struct PathProbe;

    impl CapacitySpaceProbe for PathProbe {
        fn free_bytes(&self, path: &Path) -> Result<u64, String> {
            let path = path.to_string_lossy();
            if path.contains("profile-backend") {
                Ok(2_222)
            } else if path.contains("profile-ssd") {
                Ok(3_333)
            } else {
                Ok(111)
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
    fn initialize_store_is_idempotent_and_restores_bounded_admission() {
        let root = root("initialize");
        let (registry_path, _) = registry(&root);
        let ledger_dir = root.join("ledgers");
        let provider = FileBackedCapacityAdmissionProvider::new(
            registry_path,
            ledger_dir.clone(),
            root.join("backend"),
            root.join("ssd"),
            FixedProbe {
                backend: 2_000,
                ssd: 500,
            },
        );
        let store_id = StoreId::new("codex").expect("store id");
        let policy = CapacityPolicy::bounded(1_000, 100);

        assert!(provider
            .initialize_store(&store_id, policy.clone())
            .expect("ledger initializes"));
        assert!(!provider
            .initialize_store(&store_id, policy)
            .expect("second initialization is idempotent"));
        assert!(provider
            .initialize_store(&store_id, CapacityPolicy::bounded(2_000, 200))
            .is_err());
        assert_eq!(
            load_capacity_ledger(ledger_dir.join("codex.json"))
                .expect("ledger reload")
                .used_bytes(),
            0
        );
        let response = provider
            .admit(request(DaemonIngressOrigin::RemoteS3, "initialized-upload"))
            .expect("bounded store admits after creation");
        assert_eq!(response.decision, CapacityAdmissionDecision::Admitted);
        provider
            .initialize_store(&store_id, CapacityPolicy::bounded(1_000, 100))
            .expect("reinitialization preserves active reservations");
        assert_eq!(
            load_capacity_ledger(ledger_dir.join("codex.json"))
                .expect("ledger reload after reinitialization")
                .reserved_bytes(),
            100
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rollback_removes_only_a_pristine_initialized_ledger() {
        let root = root("initialize-rollback");
        let (registry_path, _) = registry(&root);
        let ledger_dir = root.join("ledgers");
        let provider = FileBackedCapacityAdmissionProvider::new(
            registry_path,
            ledger_dir.clone(),
            root.join("backend"),
            root.join("ssd"),
            FixedProbe {
                backend: 2_000,
                ssd: 500,
            },
        );
        let store_id = StoreId::new("codex").expect("store id");
        assert!(provider
            .initialize_store(&store_id, CapacityPolicy::bounded(1_000, 100))
            .expect("ledger initializes"));
        provider
            .rollback_initialized_store(&store_id)
            .expect("pristine ledger rolls back");
        assert!(!ledger_dir.join("codex.json").exists());

        assert!(provider
            .initialize_store(&store_id, CapacityPolicy::bounded(1_000, 100))
            .expect("ledger reinitializes"));
        provider
            .admit(request(DaemonIngressOrigin::RemoteS3, "active"))
            .expect("reservation admits");
        assert!(provider.rollback_initialized_store(&store_id).is_err());
        assert!(ledger_dir.join("codex.json").exists());
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
    fn provider_uses_registered_profile_roots_for_capacity_probes() {
        let root = root("profile-roots");
        let (registry_path, _) = registry(&root);
        let ledger_dir = root.join("ledgers");
        let ledger =
            CapacityReservationLedger::new(CapacityPolicy::bounded(1_000, 100), 0).expect("ledger");
        save_capacity_ledger(ledger_dir.join("codex.json"), &ledger).expect("ledger seed");
        let backend_root = root.join("profile-backend");
        let staging_root = root.join("profile-ssd");
        std::fs::create_dir_all(&backend_root).expect("backend root");
        std::fs::create_dir_all(&staging_root).expect("staging root");
        let binding = crate::runtime::BackendProfileBinding {
            manifest: ObjectStoreManifest {
                schema_version: OBJECT_STORE_MANIFEST_SCHEMA_VERSION,
                store_id: StoreId::new("codex").expect("store id"),
                deployment_profile: DeploymentProfile::Folder,
                host_mode: HostMode::PerUser,
                protection: ProtectionPolicy::LocalOnly,
                backend: BackendReference::Folder {
                    root_identity: "fsid:codex".to_string(),
                },
            },
            backend_root: backend_root.clone(),
            ssd_staging_root: Some(staging_root.clone()),
        };
        let profile_registry = root.join("profile-bindings.json");
        crate::runtime::upsert_profile_binding(&profile_registry, binding)
            .expect("profile binding");
        let provider = FileBackedCapacityAdmissionProvider::new(
            registry_path,
            ledger_dir,
            root.join("fallback-backend"),
            root.join("fallback-ssd"),
            PathProbe,
        )
        .with_profile_binding_registry_path(profile_registry);
        let status = provider
            .status(CapacityStatusRequest {
                store_id: "codex".to_string(),
            })
            .expect("profile status");
        assert_eq!(status.backend_free_bytes, 2_222);
        assert_eq!(status.ssd_available_bytes, Some(3_333));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn required_profile_binding_fails_closed_when_store_is_unbound() {
        let root = root("profile-missing");
        let (registry_path, _) = registry(&root);
        let ledger_dir = root.join("ledgers");
        let ledger =
            CapacityReservationLedger::new(CapacityPolicy::bounded(1_000, 100), 0).expect("ledger");
        save_capacity_ledger(ledger_dir.join("codex.json"), &ledger).expect("ledger seed");
        let provider = FileBackedCapacityAdmissionProvider::new(
            registry_path,
            ledger_dir,
            root.join("backend"),
            root.join("ssd"),
            FixedProbe {
                backend: 2_000,
                ssd: 500,
            },
        )
        .with_profile_binding_registry_path(root.join("profile-bindings.json"))
        .require_profile_binding();
        let error = provider
            .status(CapacityStatusRequest {
                store_id: "codex".to_string(),
            })
            .expect_err("unbound profile must fail closed");
        assert!(error.to_string().contains("profile binding is missing"));
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
