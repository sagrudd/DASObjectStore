//! Deterministic reservation-lease maintenance contracts.

use dasobjectstore_core::ids::StoreId;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

/// Reservations are retained for one hour unless an active authority renews
/// them. The daemon scheduler runs maintenance more frequently than the lease.
pub const DEFAULT_CAPACITY_RESERVATION_LEASE_SECONDS: u64 = 60 * 60;
pub const DEFAULT_CAPACITY_RESERVATION_MAINTENANCE_CADENCE_SECONDS: u64 = 10 * 60;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapacityReservationLeaseProtection {
    pub store_id: StoreId,
    pub reservation_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapacityReservationLeaseAction {
    Renewed,
    Expired,
    LegacyRetained,
}

/// A maintenance event deliberately exposes only a one-way identifier digest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapacityReservationLeaseEvent {
    pub store_id: StoreId,
    pub reservation_id_sha256: String,
    pub action: CapacityReservationLeaseAction,
    pub bytes: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CapacityReservationLeaseReport {
    pub stores_scanned: u64,
    pub renewed_reservations: u64,
    pub expired_reservations: u64,
    pub legacy_reservations_retained: u64,
    pub reclaimed_bytes: u64,
    pub events: Vec<CapacityReservationLeaseEvent>,
}

pub(super) fn protections_by_store(
    protections: &[CapacityReservationLeaseProtection],
) -> HashMap<String, HashSet<String>> {
    let mut by_store = HashMap::<String, HashSet<String>>::new();
    for protection in protections {
        by_store
            .entry(protection.store_id.as_str().to_string())
            .or_default()
            .extend(protection.reservation_ids.iter().cloned());
    }
    by_store
}

pub(super) fn reservation_id_digest(reservation_id: &str) -> String {
    format!("sha256:{:x}", Sha256::digest(reservation_id.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::{
        reservation_id_digest, CapacityReservationLeaseAction, CapacityReservationLeaseProtection,
        DEFAULT_CAPACITY_RESERVATION_LEASE_SECONDS,
        DEFAULT_CAPACITY_RESERVATION_MAINTENANCE_CADENCE_SECONDS,
    };
    use crate::api::{CapacityAdmissionRequest, DaemonIngressOrigin};
    use crate::runtime::capacity_provider::{
        CapacityAdmissionProvider, CapacitySpaceProbe, FileBackedCapacityAdmissionProvider,
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
    struct FixedProbe;

    impl CapacitySpaceProbe for FixedProbe {
        fn free_bytes(&self, _path: &Path) -> Result<u64, String> {
            Ok(10_000)
        }
    }

    fn root(label: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let parent = std::env::var_os("DASOBJECTSTORE_CODEX_VALIDATION_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("dasobjectstore-codex-validation"));
        parent.join(format!(
            "capacity-lease-{label}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn fixture(label: &str) -> (PathBuf, PathBuf, StoreId) {
        let root = root(label);
        std::fs::create_dir_all(&root).expect("fixture root");
        let registry = root.join("stores.json");
        let store_id = StoreId::new("codex").expect("store id");
        let definition = StoreServiceDefinition {
            store_id: store_id.clone(),
            policy: StorePolicy {
                capacity: CapacityPolicy::bounded(1_000, 100),
                ..StorePolicy::defaults_for(StoreClass::GeneratedData)
            },
            bucket_name: None,
            reader_group: None,
            writer_group: None,
            public: true,
        };
        std::fs::write(
            &registry,
            serde_json::to_vec(&[definition]).expect("registry JSON"),
        )
        .expect("registry writes");
        (root, registry, store_id)
    }

    fn provider(root: &Path, registry: PathBuf) -> FileBackedCapacityAdmissionProvider<FixedProbe> {
        FileBackedCapacityAdmissionProvider::new(
            registry,
            root.join("ledgers"),
            root.join("backend"),
            root.join("ssd"),
            FixedProbe,
        )
    }

    #[test]
    fn approved_lease_defaults_leave_multiple_maintenance_opportunities() {
        assert_eq!(DEFAULT_CAPACITY_RESERVATION_LEASE_SECONDS, 3_600);
        assert_eq!(
            DEFAULT_CAPACITY_RESERVATION_MAINTENANCE_CADENCE_SECONDS,
            600
        );
        assert!(
            DEFAULT_CAPACITY_RESERVATION_LEASE_SECONDS
                > DEFAULT_CAPACITY_RESERVATION_MAINTENANCE_CADENCE_SECONDS
        );
    }

    #[test]
    fn restart_renews_supplied_active_ids_and_reclaims_only_stale_reservations() {
        let (root, registry, store_id) = fixture("restart");
        let ledger_path = root.join("ledgers/codex.json");
        let mut ledger =
            CapacityReservationLedger::new(CapacityPolicy::bounded(1_000, 100), 0).expect("ledger");
        ledger
            .reserve_at_unix_seconds("active-secret", 100, 100)
            .expect("active reservation");
        ledger
            .reserve_at_unix_seconds("stale-secret", 200, 100)
            .expect("stale reservation");
        save_capacity_ledger(&ledger_path, &ledger).expect("ledger seed");

        let report = provider(&root, registry)
            .maintain_reservation_leases(
                4_000,
                3_600,
                &[CapacityReservationLeaseProtection {
                    store_id: store_id.clone(),
                    reservation_ids: vec!["active-secret".to_string()],
                }],
            )
            .expect("maintenance succeeds after restart");

        assert_eq!(report.stores_scanned, 1);
        assert_eq!(report.renewed_reservations, 1);
        assert_eq!(report.expired_reservations, 1);
        assert_eq!(report.reclaimed_bytes, 200);
        assert!(report.events.iter().all(|event| {
            event.reservation_id_sha256.starts_with("sha256:")
                && !event.reservation_id_sha256.contains("secret")
        }));
        assert!(report.events.iter().any(|event| {
            event.action == CapacityReservationLeaseAction::Expired
                && event.reservation_id_sha256 == reservation_id_digest("stale-secret")
        }));
        let restored = load_capacity_ledger(&ledger_path).expect("ledger reload");
        assert_eq!(restored.reservation_bytes("active-secret"), Some(100));
        assert_eq!(restored.reservation_bytes("stale-secret"), None);
        assert_eq!(
            restored
                .snapshot_with_expiry()
                .reservation_created_at_unix_seconds["active-secret"],
            4_000
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn current_process_admission_is_renewed_without_caller_protection() {
        let (root, registry, store_id) = fixture("process-active");
        let ledger_path = root.join("ledgers/codex.json");
        let ledger =
            CapacityReservationLedger::new(CapacityPolicy::bounded(1_000, 100), 0).expect("ledger");
        save_capacity_ledger(&ledger_path, &ledger).expect("ledger seed");
        let provider = provider(&root, registry);
        provider
            .admit(CapacityAdmissionRequest {
                store_id: store_id.to_string(),
                requested_bytes: 100,
                copy_count: 2,
                ingress_origin: DaemonIngressOrigin::RemoteS3,
                client_request_id: Some("live-upload".to_string()),
            })
            .expect("admission succeeds");

        let report = provider
            .maintain_reservation_leases(u64::MAX - 1, 1, &[])
            .expect("active reservation renews before expiry");
        assert_eq!(report.renewed_reservations, 1);
        assert_eq!(report.expired_reservations, 0);
        assert_eq!(
            load_capacity_ledger(&ledger_path)
                .expect("ledger reload")
                .reservation_bytes("live-upload"),
            Some(100)
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn unprotected_legacy_reservation_is_retained() {
        let (root, registry, _) = fixture("legacy");
        let ledger_path = root.join("ledgers/codex.json");
        let mut ledger =
            CapacityReservationLedger::new(CapacityPolicy::bounded(1_000, 100), 0).expect("ledger");
        ledger
            .reserve_at_unix_seconds("legacy-secret", 100, 0)
            .expect("legacy reservation");
        save_capacity_ledger(&ledger_path, &ledger).expect("ledger seed");

        let report = provider(&root, registry)
            .maintain_reservation_leases(u64::MAX, 1, &[])
            .expect("legacy maintenance succeeds");
        assert_eq!(report.legacy_reservations_retained, 1);
        assert_eq!(report.expired_reservations, 0);
        assert_eq!(
            load_capacity_ledger(&ledger_path)
                .expect("ledger reload")
                .reservation_bytes("legacy-secret"),
            Some(100)
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn scan_failure_is_fail_closed_without_touching_ledgers() {
        let (root, registry, _) = fixture("scan-failure");
        let ledger_path = root.join("ledgers/codex.json");
        let mut ledger =
            CapacityReservationLedger::new(CapacityPolicy::bounded(1_000, 100), 0).expect("ledger");
        ledger
            .reserve_at_unix_seconds("untouched", 100, 1)
            .expect("reservation");
        save_capacity_ledger(&ledger_path, &ledger).expect("ledger seed");
        std::fs::write(&registry, b"not-json").expect("registry corrupts");

        assert!(provider(&root, registry)
            .maintain_reservation_leases(10_000, 1, &[])
            .is_err());
        assert_eq!(
            load_capacity_ledger(&ledger_path)
                .expect("ledger unchanged")
                .reservation_bytes("untouched"),
            Some(100)
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn persistence_failure_rolls_back_cached_lease_mutation() {
        let (root, registry, _) = fixture("persistence-failure");
        let ledger_path = root.join("ledgers/codex.json");
        let mut ledger =
            CapacityReservationLedger::new(CapacityPolicy::bounded(1_000, 100), 0).expect("ledger");
        ledger
            .reserve_at_unix_seconds("stale", 100, 1)
            .expect("reservation");
        save_capacity_ledger(&ledger_path, &ledger).expect("ledger seed");
        let provider = provider(&root, registry);
        provider
            .maintain_reservation_leases(1, 10, &[])
            .expect("initial scan caches ledger without mutation");
        std::fs::remove_file(&ledger_path).expect("ledger file removes");
        std::fs::create_dir(&ledger_path).expect("directory blocks atomic replacement");

        assert!(provider.maintain_reservation_leases(100, 10, &[]).is_err());
        std::fs::remove_dir(&ledger_path).expect("blocking directory removes");
        let report = provider
            .maintain_reservation_leases(100, 10, &[])
            .expect("rolled-back cached reservation can be retried");
        assert_eq!(report.expired_reservations, 1);
        assert_eq!(report.reclaimed_bytes, 100);
        let _ = std::fs::remove_dir_all(root);
    }
}
