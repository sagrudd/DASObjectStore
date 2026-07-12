//! Transactional capacity accounting for optional SubObject budgets.

use crate::store::{CapacityLedgerError, CapacityPolicy, CapacityReservationLedger};
use std::collections::HashMap;
use std::fmt::{self, Display};

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
