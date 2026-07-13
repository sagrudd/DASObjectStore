//! Transactional logical-capacity reservations and durable lease metadata.

use crate::store::{CapacityPolicy, CapacityPolicyValidationError};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LogicalObjectVersionCharge {
    logical_size_bytes: u64,
}

impl LogicalObjectVersionCharge {
    /// Charge one logical object version at its full logical size. Physical
    /// copy amplification and staging are intentionally not part of this
    /// logical quota primitive; admission reports those separately.
    pub const fn new(logical_size_bytes: u64) -> Self {
        Self { logical_size_bytes }
    }

    pub const fn logical_size_bytes(&self) -> u64 {
        self.logical_size_bytes
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapacityReservationLedger {
    policy: CapacityPolicy,
    used_bytes: u64,
    reservations: HashMap<String, u64>,
    reservation_created_at_unix_seconds: HashMap<String, u64>,
}

pub const CAPACITY_LEDGER_SNAPSHOT_SCHEMA_VERSION: u32 = 1;
pub const CAPACITY_LEDGER_EXPIRY_SNAPSHOT_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CapacityReservationLedgerSnapshot {
    pub schema_version: u32,
    pub policy: CapacityPolicy,
    pub used_bytes: u64,
    pub reservations: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CapacityReservationLedgerSnapshotV2 {
    pub schema_version: u32,
    pub policy: CapacityPolicy,
    pub used_bytes: u64,
    pub reservations: BTreeMap<String, u64>,
    pub reservation_created_at_unix_seconds: BTreeMap<String, u64>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapacityPressureState {
    Unbounded,
    Normal,
    Warning,
    Critical,
    OverQuota,
}

impl CapacityReservationLedger {
    pub fn new(policy: CapacityPolicy, used_bytes: u64) -> Result<Self, CapacityLedgerError> {
        if let Some(error) = policy.validation_error() {
            return Err(CapacityLedgerError::InvalidPolicy(error));
        }
        Ok(Self {
            policy,
            used_bytes,
            reservations: HashMap::new(),
            reservation_created_at_unix_seconds: HashMap::new(),
        })
    }

    pub fn used_bytes(&self) -> u64 {
        self.used_bytes
    }

    pub fn reserved_bytes(&self) -> u64 {
        self.reservations.values().copied().sum()
    }

    pub fn reservation_bytes(&self, reservation_id: &str) -> Option<u64> {
        self.reservations.get(reservation_id).copied()
    }

    pub fn policy(&self) -> &CapacityPolicy {
        &self.policy
    }

    pub fn snapshot(&self) -> CapacityReservationLedgerSnapshot {
        CapacityReservationLedgerSnapshot {
            schema_version: CAPACITY_LEDGER_SNAPSHOT_SCHEMA_VERSION,
            policy: self.policy.clone(),
            used_bytes: self.used_bytes,
            reservations: self
                .reservations
                .iter()
                .map(|(id, bytes)| (id.clone(), *bytes))
                .collect(),
        }
    }

    pub fn snapshot_with_expiry(&self) -> CapacityReservationLedgerSnapshotV2 {
        CapacityReservationLedgerSnapshotV2 {
            schema_version: CAPACITY_LEDGER_EXPIRY_SNAPSHOT_SCHEMA_VERSION,
            policy: self.policy.clone(),
            used_bytes: self.used_bytes,
            reservations: self
                .reservations
                .iter()
                .map(|(id, bytes)| (id.clone(), *bytes))
                .collect(),
            reservation_created_at_unix_seconds: self
                .reservation_created_at_unix_seconds
                .iter()
                .map(|(id, created_at)| (id.clone(), *created_at))
                .collect(),
        }
    }

    pub fn from_snapshot(
        snapshot: CapacityReservationLedgerSnapshot,
    ) -> Result<Self, CapacityLedgerError> {
        if snapshot.schema_version != CAPACITY_LEDGER_SNAPSHOT_SCHEMA_VERSION {
            return Err(CapacityLedgerError::InvalidSnapshotSchema {
                schema_version: snapshot.schema_version,
            });
        }
        let mut ledger = Self::new(snapshot.policy, snapshot.used_bytes)?;
        for (reservation_id, bytes) in snapshot.reservations {
            ledger.reserve_at_unix_seconds(reservation_id, bytes, 0)?;
        }
        Ok(ledger)
    }

    pub fn from_snapshot_with_expiry(
        snapshot: CapacityReservationLedgerSnapshotV2,
    ) -> Result<Self, CapacityLedgerError> {
        if snapshot.schema_version != CAPACITY_LEDGER_EXPIRY_SNAPSHOT_SCHEMA_VERSION {
            return Err(CapacityLedgerError::InvalidSnapshotSchema {
                schema_version: snapshot.schema_version,
            });
        }
        for reservation_id in snapshot.reservation_created_at_unix_seconds.keys() {
            if !snapshot.reservations.contains_key(reservation_id) {
                return Err(CapacityLedgerError::InvalidReservationMetadata);
            }
        }
        let mut ledger = Self::new(snapshot.policy, snapshot.used_bytes)?;
        for (reservation_id, bytes) in snapshot.reservations {
            let created_at = snapshot
                .reservation_created_at_unix_seconds
                .get(&reservation_id)
                .copied()
                .unwrap_or_default();
            ledger.reserve_at_unix_seconds(reservation_id, bytes, created_at)?;
        }
        Ok(ledger)
    }

    /// Replace capacity policy without deleting data. Lowering a limit below
    /// current usage is allowed so operators can remediate an over-quota store;
    /// subsequent reservations remain rejected until usage falls below policy.
    pub fn update_policy(&mut self, policy: CapacityPolicy) -> Result<(), CapacityLedgerError> {
        if let Some(error) = policy.validation_error() {
            return Err(CapacityLedgerError::InvalidPolicy(error));
        }
        self.policy = policy;
        Ok(())
    }

    pub fn pressure_state(&self) -> CapacityPressureState {
        let Some(limit) = self.policy.logical_limit_bytes else {
            return CapacityPressureState::Unbounded;
        };
        let effective_limit = limit.saturating_sub(self.policy.backend_reserve_bytes);
        let usage = self.used_bytes.saturating_add(self.reserved_bytes());
        if usage > effective_limit {
            return CapacityPressureState::OverQuota;
        }
        let basis_points = (u128::from(usage) * 10_000 / u128::from(effective_limit)) as u64;
        if basis_points >= u64::from(self.policy.critical_threshold_basis_points) {
            CapacityPressureState::Critical
        } else if basis_points >= u64::from(self.policy.warning_threshold_basis_points) {
            CapacityPressureState::Warning
        } else {
            CapacityPressureState::Normal
        }
    }

    pub fn available_bytes(&self) -> Option<u64> {
        self.policy.logical_limit_bytes.map(|limit| {
            limit
                .saturating_sub(self.policy.backend_reserve_bytes)
                .saturating_sub(self.used_bytes)
                .saturating_sub(self.reserved_bytes())
        })
    }

    pub fn reserve(
        &mut self,
        reservation_id: impl Into<String>,
        bytes: u64,
    ) -> Result<(), CapacityLedgerError> {
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or_default();
        self.reserve_at_unix_seconds(reservation_id, bytes, created_at)
    }

    /// Reserve bytes with an explicit creation timestamp. The timestamp is
    /// persisted so a maintenance sweep can reclaim abandoned reservations
    /// after a caller-selected lease window without guessing from process
    /// uptime or filesystem mtime.
    pub fn reserve_at_unix_seconds(
        &mut self,
        reservation_id: impl Into<String>,
        bytes: u64,
        created_at_unix_seconds: u64,
    ) -> Result<(), CapacityLedgerError> {
        let reservation_id = reservation_id.into();
        if reservation_id.trim().is_empty() || self.reservations.contains_key(&reservation_id) {
            return Err(CapacityLedgerError::InvalidReservationId);
        }
        let outstanding = self
            .used_bytes
            .checked_add(self.reserved_bytes())
            .and_then(|value| value.checked_add(bytes))
            .ok_or(CapacityLedgerError::Overflow)?;
        if let Some(limit) = self.policy.logical_limit_bytes {
            let admitted_limit = limit.saturating_sub(self.policy.backend_reserve_bytes);
            if outstanding > admitted_limit {
                return Err(CapacityLedgerError::InsufficientCapacity {
                    available_bytes: admitted_limit
                        .saturating_sub(self.used_bytes.saturating_add(self.reserved_bytes())),
                });
            }
        }
        self.reservations.insert(reservation_id.clone(), bytes);
        self.reservation_created_at_unix_seconds
            .insert(reservation_id, created_at_unix_seconds);
        Ok(())
    }

    /// Release reservations older than `max_age_seconds` at `now_unix_seconds`.
    /// Legacy schema-v1 reservations have timestamp zero and are intentionally
    /// retained until explicitly released, preventing an upgrade from
    /// reclaiming an active transfer whose age cannot be established.
    pub fn expire_reservations(
        &mut self,
        now_unix_seconds: u64,
        max_age_seconds: u64,
    ) -> Vec<(String, u64)> {
        let mut expired_ids: Vec<String> = self
            .reservation_created_at_unix_seconds
            .iter()
            .filter_map(|(reservation_id, created_at)| {
                (*created_at > 0
                    && now_unix_seconds >= *created_at
                    && now_unix_seconds - *created_at >= max_age_seconds)
                    .then_some(reservation_id.clone())
            })
            .collect();
        expired_ids.sort();
        expired_ids
            .into_iter()
            .filter_map(|reservation_id| {
                let bytes = self.reservations.remove(&reservation_id)?;
                self.reservation_created_at_unix_seconds
                    .remove(&reservation_id);
                Some((reservation_id, bytes))
            })
            .collect()
    }

    /// Reserve a complete logical object version. Two versions with the same
    /// content still consume two logical charges; deduplication and physical
    /// placement are evaluated by separate backend/admission contracts.
    pub fn reserve_object_version(
        &mut self,
        reservation_id: impl Into<String>,
        charge: LogicalObjectVersionCharge,
    ) -> Result<(), CapacityLedgerError> {
        self.reserve(reservation_id, charge.logical_size_bytes)
    }

    pub fn commit(&mut self, reservation_id: &str) -> Result<(), CapacityLedgerError> {
        let bytes = self
            .reservations
            .get(reservation_id)
            .copied()
            .ok_or(CapacityLedgerError::UnknownReservation)?;
        let new_used_bytes = self
            .used_bytes
            .checked_add(bytes)
            .ok_or(CapacityLedgerError::Overflow)?;
        self.reservations.remove(reservation_id);
        self.reservation_created_at_unix_seconds
            .remove(reservation_id);
        self.used_bytes = new_used_bytes;
        Ok(())
    }

    pub fn debit_used_bytes(&mut self, bytes: u64) -> Result<(), CapacityLedgerError> {
        if bytes > self.used_bytes {
            return Err(CapacityLedgerError::UsedBytesUnderflow {
                used_bytes: self.used_bytes,
                requested_bytes: bytes,
            });
        }
        self.used_bytes -= bytes;
        Ok(())
    }

    pub fn release(&mut self, reservation_id: &str) -> Result<u64, CapacityLedgerError> {
        self.reservations
            .remove(reservation_id)
            .ok_or(CapacityLedgerError::UnknownReservation)
            .inspect(|_| {
                self.reservation_created_at_unix_seconds
                    .remove(reservation_id);
            })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CapacityLedgerError {
    InvalidPolicy(CapacityPolicyValidationError),
    InvalidSnapshotSchema {
        schema_version: u32,
    },
    InvalidReservationId,
    InvalidReservationMetadata,
    UnknownReservation,
    UsedBytesUnderflow {
        used_bytes: u64,
        requested_bytes: u64,
    },
    InsufficientCapacity {
        available_bytes: u64,
    },
    Overflow,
}

#[cfg(test)]
mod tests {
    use super::{
        CapacityLedgerError, CapacityPressureState, CapacityReservationLedger,
        LogicalObjectVersionCharge,
    };
    use crate::store::CapacityPolicy;
    use std::sync::{Arc, Mutex};
    use std::thread;

    #[test]
    fn contended_reservations_never_overbook_the_logical_limit() {
        let ledger = Arc::new(Mutex::new(
            CapacityReservationLedger::new(CapacityPolicy::bounded(1_000, 0), 0)
                .expect("valid capacity policy"),
        ));
        let workers = (0..8)
            .map(|index| {
                let ledger = Arc::clone(&ledger);
                thread::spawn(move || {
                    let mut ledger = ledger.lock().expect("ledger lock");
                    ledger
                        .reserve_object_version(
                            format!("contended-{index}"),
                            LogicalObjectVersionCharge::new(300),
                        )
                        .is_ok()
                })
            })
            .collect::<Vec<_>>();
        let admitted = workers
            .into_iter()
            .map(|worker| worker.join().expect("worker completes"))
            .filter(|admitted| *admitted)
            .count();
        let ledger = ledger.lock().expect("ledger lock");
        assert_eq!(admitted, 3);
        assert_eq!(ledger.reserved_bytes(), 900);
        assert_eq!(ledger.available_bytes(), Some(100));
    }

    #[test]
    fn lowering_quota_preserves_usage_and_blocks_new_admission() {
        let mut ledger = CapacityReservationLedger::new(CapacityPolicy::bounded(1_000, 0), 700)
            .expect("valid capacity policy");
        ledger
            .update_policy(CapacityPolicy::bounded(600, 0))
            .expect("policy update validates");
        assert_eq!(ledger.pressure_state(), CapacityPressureState::OverQuota);
        assert_eq!(ledger.available_bytes(), Some(0));
        assert!(matches!(
            ledger.reserve("over-quota", 1),
            Err(CapacityLedgerError::InsufficientCapacity { available_bytes: 0 })
        ));
        assert_eq!(ledger.used_bytes(), 700);
    }

    #[test]
    fn expiry_uses_creation_timestamp_and_retains_unknown_age() {
        let mut ledger = CapacityReservationLedger::new(CapacityPolicy::bounded(1_000, 0), 0)
            .expect("valid capacity policy");
        ledger
            .reserve_at_unix_seconds("old", 100, 100)
            .expect("old reservation");
        ledger
            .reserve_at_unix_seconds("new", 200, 190)
            .expect("new reservation");
        ledger
            .reserve_at_unix_seconds("legacy", 50, 0)
            .expect("legacy reservation");
        assert_eq!(
            ledger.expire_reservations(200, 100),
            vec![("old".to_string(), 100)]
        );
        assert_eq!(ledger.reservation_bytes("legacy"), Some(50));
        assert_eq!(ledger.reservation_bytes("new"), Some(200));
    }
}
