//! Transactional capacity accounting for optional SubObject budgets.

use crate::store::{CapacityLedgerError, CapacityPolicy, CapacityReservationLedger};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fmt::{self, Display};

pub const SUBOBJECT_CAPACITY_SNAPSHOT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SubObjectCapacityLedgerSnapshot {
    pub schema_version: u32,
    pub parent: crate::store::CapacityReservationLedgerSnapshotV2,
    pub children: BTreeMap<String, crate::store::CapacityReservationLedgerSnapshotV2>,
}

/// A parent ObjectStore ledger with independently bounded child ledgers.
/// Reservations and commits update both ledgers; a failed child reservation
/// rolls back the parent reservation before returning to the caller.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubObjectCapacityLedger {
    parent: CapacityReservationLedger,
    children: HashMap<String, CapacityReservationLedger>,
}

impl SubObjectCapacityLedger {
    pub fn new(
        parent_policy: CapacityPolicy,
        parent_used_bytes: u64,
    ) -> Result<Self, SubObjectCapacityError> {
        Ok(Self {
            parent: CapacityReservationLedger::new(parent_policy, parent_used_bytes)
                .map_err(SubObjectCapacityError::Parent)?,
            children: HashMap::new(),
        })
    }

    pub fn add_child(
        &mut self,
        child_id: impl Into<String>,
        policy: CapacityPolicy,
        used_bytes: u64,
    ) -> Result<(), SubObjectCapacityError> {
        let child_id = child_id.into();
        if child_id.trim().is_empty() {
            return Err(SubObjectCapacityError::InvalidChildId);
        }
        if self.children.contains_key(&child_id) {
            return Err(SubObjectCapacityError::DuplicateChild { child_id });
        }
        let child = CapacityReservationLedger::new(policy, used_bytes)
            .map_err(SubObjectCapacityError::Child)?;
        self.children.insert(child_id, child);
        Ok(())
    }

    pub fn parent(&self) -> &CapacityReservationLedger {
        &self.parent
    }

    pub fn child(&self, child_id: &str) -> Option<&CapacityReservationLedger> {
        self.children.get(child_id)
    }

    pub fn snapshot(&self) -> SubObjectCapacityLedgerSnapshot {
        SubObjectCapacityLedgerSnapshot {
            schema_version: SUBOBJECT_CAPACITY_SNAPSHOT_SCHEMA_VERSION,
            parent: self.parent.snapshot_with_expiry(),
            children: self
                .children
                .iter()
                .map(|(child_id, ledger)| (child_id.clone(), ledger.snapshot_with_expiry()))
                .collect(),
        }
    }

    pub fn from_snapshot(
        snapshot: SubObjectCapacityLedgerSnapshot,
    ) -> Result<Self, SubObjectCapacityError> {
        if snapshot.schema_version != SUBOBJECT_CAPACITY_SNAPSHOT_SCHEMA_VERSION {
            return Err(SubObjectCapacityError::InvalidSnapshotSchema {
                schema_version: snapshot.schema_version,
            });
        }
        let SubObjectCapacityLedgerSnapshot {
            parent: parent_snapshot,
            children: child_snapshots,
            ..
        } = snapshot;
        let mut expected_parent_reservations = BTreeMap::new();
        let mut child_used_bytes = 0_u64;
        for (child_id, child_snapshot) in &child_snapshots {
            for (reservation_id, bytes) in &child_snapshot.reservations {
                let parent_id = parent_reservation_id(child_id, reservation_id);
                if expected_parent_reservations
                    .insert(parent_id, *bytes)
                    .is_some()
                {
                    return Err(SubObjectCapacityError::InvalidReservationLink);
                }
            }
            child_used_bytes = child_used_bytes
                .checked_add(child_snapshot.used_bytes)
                .ok_or(SubObjectCapacityError::Overflow)?;
        }
        if parent_snapshot.reservations != expected_parent_reservations
            || child_used_bytes > parent_snapshot.used_bytes
        {
            return Err(SubObjectCapacityError::InvalidReservationLink);
        }
        let parent = CapacityReservationLedger::from_snapshot_with_expiry(parent_snapshot)
            .map_err(SubObjectCapacityError::Parent)?;
        let mut children = HashMap::new();
        for (child_id, child_snapshot) in child_snapshots {
            if child_id.trim().is_empty() || children.contains_key(&child_id) {
                return Err(SubObjectCapacityError::InvalidChildId);
            }
            let child = CapacityReservationLedger::from_snapshot_with_expiry(child_snapshot)
                .map_err(SubObjectCapacityError::Child)?;
            children.insert(child_id, child);
        }
        Ok(Self { parent, children })
    }

    pub fn reserve(
        &mut self,
        child_id: &str,
        reservation_id: &str,
        bytes: u64,
    ) -> Result<(), SubObjectCapacityError> {
        let child = self.children.get_mut(child_id).ok_or_else(|| {
            SubObjectCapacityError::UnknownChild {
                child_id: child_id.to_string(),
            }
        })?;
        let parent_reservation_id = parent_reservation_id(child_id, reservation_id);
        self.parent
            .reserve(parent_reservation_id.clone(), bytes)
            .map_err(SubObjectCapacityError::Parent)?;
        if let Err(error) = child.reserve(reservation_id, bytes) {
            let _ = self.parent.release(&parent_reservation_id);
            return Err(SubObjectCapacityError::Child(error));
        }
        Ok(())
    }

    pub fn commit(
        &mut self,
        child_id: &str,
        reservation_id: &str,
    ) -> Result<(), SubObjectCapacityError> {
        let child = self.children.get_mut(child_id).ok_or_else(|| {
            SubObjectCapacityError::UnknownChild {
                child_id: child_id.to_string(),
            }
        })?;
        let parent_reservation_id = parent_reservation_id(child_id, reservation_id);
        let bytes =
            child
                .reservation_bytes(reservation_id)
                .ok_or(SubObjectCapacityError::Child(
                    CapacityLedgerError::UnknownReservation,
                ))?;
        if self
            .parent
            .reservation_bytes(&parent_reservation_id)
            .is_none()
        {
            return Err(SubObjectCapacityError::Parent(
                CapacityLedgerError::UnknownReservation,
            ));
        }
        if child.used_bytes().checked_add(bytes).is_none()
            || self.parent.used_bytes().checked_add(bytes).is_none()
        {
            return Err(SubObjectCapacityError::Overflow);
        }
        child
            .commit(reservation_id)
            .map_err(SubObjectCapacityError::Child)?;
        self.parent
            .commit(&parent_reservation_id)
            .map_err(SubObjectCapacityError::Parent)
    }

    pub fn release(
        &mut self,
        child_id: &str,
        reservation_id: &str,
    ) -> Result<u64, SubObjectCapacityError> {
        let child = self.children.get_mut(child_id).ok_or_else(|| {
            SubObjectCapacityError::UnknownChild {
                child_id: child_id.to_string(),
            }
        })?;
        let parent_reservation_id = parent_reservation_id(child_id, reservation_id);
        let bytes =
            child
                .reservation_bytes(reservation_id)
                .ok_or(SubObjectCapacityError::Child(
                    CapacityLedgerError::UnknownReservation,
                ))?;
        if self
            .parent
            .reservation_bytes(&parent_reservation_id)
            .is_none()
        {
            return Err(SubObjectCapacityError::Parent(
                CapacityLedgerError::UnknownReservation,
            ));
        }
        child
            .release(reservation_id)
            .map_err(SubObjectCapacityError::Child)?;
        self.parent
            .release(&parent_reservation_id)
            .map_err(SubObjectCapacityError::Parent)?;
        Ok(bytes)
    }
}

fn parent_reservation_id(child_id: &str, reservation_id: &str) -> String {
    format!("subobject:{child_id}:{reservation_id}")
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SubObjectCapacityError {
    InvalidChildId,
    DuplicateChild { child_id: String },
    UnknownChild { child_id: String },
    InvalidSnapshotSchema { schema_version: u32 },
    InvalidReservationLink,
    Parent(CapacityLedgerError),
    Child(CapacityLedgerError),
    Overflow,
}

impl Display for SubObjectCapacityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidChildId => formatter.write_str("SubObject ID must not be blank"),
            Self::DuplicateChild { child_id } => {
                write!(
                    formatter,
                    "SubObject `{child_id}` already has a capacity ledger"
                )
            }
            Self::UnknownChild { child_id } => {
                write!(formatter, "unknown SubObject `{child_id}`")
            }
            Self::InvalidSnapshotSchema { schema_version } => {
                write!(
                    formatter,
                    "unsupported SubObject capacity snapshot schema {schema_version}"
                )
            }
            Self::InvalidReservationLink => {
                formatter.write_str("SubObject capacity snapshot has inconsistent parent links")
            }
            Self::Parent(error) => write!(formatter, "parent capacity: {error:?}"),
            Self::Child(error) => write!(formatter, "SubObject capacity: {error:?}"),
            Self::Overflow => {
                formatter.write_str("parent and child capacity accounting overflowed")
            }
        }
    }
}

impl std::error::Error for SubObjectCapacityError {}

#[cfg(test)]
mod tests {
    use super::{SubObjectCapacityError, SubObjectCapacityLedger};
    use crate::store::{CapacityLedgerError, CapacityPolicy};

    fn ledger() -> SubObjectCapacityLedger {
        let mut ledger = SubObjectCapacityLedger::new(CapacityPolicy::bounded(1_000, 0), 100)
            .expect("parent ledger is valid");
        ledger
            .add_child("child-a", CapacityPolicy::bounded(200, 0), 20)
            .expect("child ledger is valid");
        ledger
    }

    #[test]
    fn successful_reservation_and_commit_update_parent_and_child() {
        let mut ledger = ledger();
        ledger
            .reserve("child-a", "upload-1", 50)
            .expect("both ledgers have capacity");
        assert_eq!(ledger.parent().reserved_bytes(), 50);
        assert_eq!(ledger.child("child-a").unwrap().reserved_bytes(), 50);
        ledger
            .commit("child-a", "upload-1")
            .expect("both commits succeed");
        assert_eq!(ledger.parent().used_bytes(), 150);
        assert_eq!(ledger.child("child-a").unwrap().used_bytes(), 70);
    }

    #[test]
    fn failed_child_reservation_rolls_back_parent_reservation() {
        let mut ledger = ledger();
        let error = ledger
            .reserve("child-a", "too-large", 300)
            .expect_err("child quota rejects request");
        assert!(matches!(error, SubObjectCapacityError::Child(_)));
        assert_eq!(ledger.parent().reserved_bytes(), 0);
        assert_eq!(ledger.child("child-a").unwrap().reserved_bytes(), 0);
    }

    #[test]
    fn failed_parent_reservation_does_not_touch_child() {
        let mut ledger = SubObjectCapacityLedger::new(CapacityPolicy::bounded(100, 0), 90)
            .expect("parent ledger is valid");
        ledger
            .add_child("child-a", CapacityPolicy::bounded(200, 0), 20)
            .expect("child ledger is valid");
        let error = ledger
            .reserve("child-a", "parent-full", 20)
            .expect_err("parent quota rejects request");
        assert!(matches!(error, SubObjectCapacityError::Parent(_)));
        assert_eq!(ledger.parent().reserved_bytes(), 0);
        assert_eq!(ledger.child("child-a").unwrap().reserved_bytes(), 0);
    }

    #[test]
    fn snapshot_round_trip_preserves_parent_child_usage_and_reservations() {
        let mut ledger = ledger();
        ledger
            .reserve("child-a", "upload-1", 50)
            .expect("reservation fits");
        let restored =
            SubObjectCapacityLedger::from_snapshot(ledger.snapshot()).expect("snapshot restores");

        assert_eq!(restored.parent().used_bytes(), 100);
        assert_eq!(restored.parent().reserved_bytes(), 50);
        assert_eq!(restored.child("child-a").unwrap().used_bytes(), 20);
        assert_eq!(restored.child("child-a").unwrap().reserved_bytes(), 50);
        assert_eq!(
            restored
                .child("child-a")
                .unwrap()
                .reservation_bytes("upload-1"),
            Some(50)
        );
    }

    #[test]
    fn snapshot_rejects_unknown_schema() {
        let mut snapshot = ledger().snapshot();
        snapshot.schema_version += 1;
        assert!(matches!(
            SubObjectCapacityLedger::from_snapshot(snapshot),
            Err(SubObjectCapacityError::InvalidSnapshotSchema { .. })
        ));
    }

    #[test]
    fn snapshot_rejects_missing_parent_reservation_link() {
        let mut ledger = ledger();
        ledger
            .reserve("child-a", "upload-1", 50)
            .expect("reservation fits");
        let mut snapshot = ledger.snapshot();
        snapshot.parent.reservations.clear();

        assert!(matches!(
            SubObjectCapacityLedger::from_snapshot(snapshot),
            Err(SubObjectCapacityError::InvalidReservationLink)
        ));
    }

    #[test]
    fn release_updates_both_ledgers_without_charging_usage() {
        let mut ledger = ledger();
        ledger
            .reserve("child-a", "upload-1", 50)
            .expect("reservation fits");
        assert_eq!(ledger.release("child-a", "upload-1"), Ok(50));
        assert_eq!(ledger.parent().used_bytes(), 100);
        assert_eq!(ledger.child("child-a").unwrap().used_bytes(), 20);
        assert_eq!(ledger.parent().reserved_bytes(), 0);
    }

    #[test]
    fn duplicate_and_unknown_children_or_reservations_are_rejected() {
        let mut ledger = ledger();
        assert_eq!(
            ledger.add_child("child-a", CapacityPolicy::default(), 0),
            Err(SubObjectCapacityError::DuplicateChild {
                child_id: "child-a".to_string()
            })
        );
        assert!(matches!(
            ledger.reserve("missing", "upload", 1),
            Err(SubObjectCapacityError::UnknownChild { .. })
        ));
        assert_eq!(
            ledger.release("child-a", "missing"),
            Err(SubObjectCapacityError::Child(
                CapacityLedgerError::UnknownReservation
            ))
        );
    }
}
