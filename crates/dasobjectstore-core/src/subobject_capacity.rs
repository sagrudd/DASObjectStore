//! Transactional capacity accounting for optional SubObject budgets.

use crate::store::{CapacityLedgerError, CapacityPolicy, CapacityReservationLedger};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fmt::{self, Display};

const SUBOBJECT_CAPACITY_SNAPSHOT_SCHEMA_VERSION_V1: u32 = 1;
pub const SUBOBJECT_CAPACITY_SNAPSHOT_SCHEMA_VERSION: u32 = 2;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SubObjectCapacityLedgerSnapshot {
    pub schema_version: u32,
    pub parent: crate::store::CapacityReservationLedgerSnapshotV2,
    pub children: BTreeMap<String, crate::store::CapacityReservationLedgerSnapshotV2>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SubObjectCapacityReservationScope {
    Store,
    Child { child_id: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpiredSubObjectCapacityReservation {
    pub scope: SubObjectCapacityReservationScope,
    pub reservation_id: String,
    pub bytes: u64,
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

    /// Upgrade an existing flat ObjectStore ledger into hierarchical state.
    /// Existing reservations remain store-scoped and retain their durable
    /// creation timestamps; no usage or policy accounting is reset.
    pub fn from_parent(parent: CapacityReservationLedger) -> Result<Self, SubObjectCapacityError> {
        let mut snapshot = parent.snapshot_with_expiry();
        snapshot.reservations = snapshot
            .reservations
            .into_iter()
            .map(|(reservation_id, bytes)| (store_reservation_id(&reservation_id), bytes))
            .collect();
        snapshot.reservation_created_at_unix_seconds = snapshot
            .reservation_created_at_unix_seconds
            .into_iter()
            .map(|(reservation_id, created_at)| (store_reservation_id(&reservation_id), created_at))
            .collect();
        let parent = CapacityReservationLedger::from_snapshot_with_expiry(snapshot)
            .map_err(SubObjectCapacityError::Parent)?;
        Ok(Self {
            parent,
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

    pub fn parent_mut(&mut self) -> &mut CapacityReservationLedger {
        &mut self.parent
    }

    pub fn child(&self, child_id: &str) -> Option<&CapacityReservationLedger> {
        self.children.get(child_id)
    }

    pub fn has_child(&self, child_id: &str) -> bool {
        self.children.contains_key(child_id)
    }

    pub fn update_parent_policy(
        &mut self,
        policy: CapacityPolicy,
    ) -> Result<(), SubObjectCapacityError> {
        self.parent
            .update_policy(policy)
            .map_err(SubObjectCapacityError::Parent)
    }

    pub fn reconcile_parent_used_bytes(
        &mut self,
        used_bytes: u64,
    ) -> Result<(), SubObjectCapacityError> {
        self.parent
            .reconcile_used_bytes(used_bytes)
            .map_err(SubObjectCapacityError::Parent)
    }

    pub fn update_child_policy(
        &mut self,
        child_id: &str,
        policy: CapacityPolicy,
    ) -> Result<(), SubObjectCapacityError> {
        self.children
            .get_mut(child_id)
            .ok_or_else(|| SubObjectCapacityError::UnknownChild {
                child_id: child_id.to_string(),
            })?
            .update_policy(policy)
            .map_err(SubObjectCapacityError::Child)
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
        if !matches!(
            snapshot.schema_version,
            SUBOBJECT_CAPACITY_SNAPSHOT_SCHEMA_VERSION_V1
                | SUBOBJECT_CAPACITY_SNAPSHOT_SCHEMA_VERSION
        ) {
            return Err(SubObjectCapacityError::InvalidSnapshotSchema {
                schema_version: snapshot.schema_version,
            });
        }
        let schema_version = snapshot.schema_version;
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
        let reservation_links_are_valid = match schema_version {
            SUBOBJECT_CAPACITY_SNAPSHOT_SCHEMA_VERSION_V1 => {
                parent_snapshot.reservations == expected_parent_reservations
            }
            SUBOBJECT_CAPACITY_SNAPSHOT_SCHEMA_VERSION => {
                expected_parent_reservations
                    .iter()
                    .all(|(id, bytes)| parent_snapshot.reservations.get(id) == Some(bytes))
                    && parent_snapshot.reservations.keys().all(|id| {
                        expected_parent_reservations.contains_key(id)
                            || id
                                .strip_prefix("store:")
                                .is_some_and(|reservation_id| !reservation_id.trim().is_empty())
                    })
            }
            _ => unreachable!("snapshot schema was validated above"),
        };
        if !reservation_links_are_valid || child_used_bytes > parent_snapshot.used_bytes {
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

    /// Reserve capacity directly against the ObjectStore while preserving the
    /// same parent ledger used by bounded SubObjects.
    pub fn reserve_store(
        &mut self,
        reservation_id: &str,
        bytes: u64,
    ) -> Result<(), SubObjectCapacityError> {
        if reservation_id.trim().is_empty() {
            return Err(SubObjectCapacityError::Parent(
                CapacityLedgerError::InvalidReservationId,
            ));
        }
        self.parent
            .reserve(store_reservation_id(reservation_id), bytes)
            .map_err(SubObjectCapacityError::Parent)
    }

    pub fn commit_store(&mut self, reservation_id: &str) -> Result<(), SubObjectCapacityError> {
        self.parent
            .commit(&store_reservation_id(reservation_id))
            .map_err(SubObjectCapacityError::Parent)
    }

    pub fn release_store(&mut self, reservation_id: &str) -> Result<u64, SubObjectCapacityError> {
        self.parent
            .release(&store_reservation_id(reservation_id))
            .map_err(SubObjectCapacityError::Parent)
    }

    pub fn store_reservation_bytes(&self, reservation_id: &str) -> Option<u64> {
        self.parent
            .reservation_bytes(&store_reservation_id(reservation_id))
    }

    pub fn renew_store_reservation_at_unix_seconds(
        &mut self,
        reservation_id: &str,
        renewed_at_unix_seconds: u64,
    ) -> Result<(), SubObjectCapacityError> {
        self.parent
            .renew_reservation_at_unix_seconds(
                &store_reservation_id(reservation_id),
                renewed_at_unix_seconds,
            )
            .map_err(SubObjectCapacityError::Parent)
    }

    pub fn renew_child_reservation_at_unix_seconds(
        &mut self,
        child_id: &str,
        reservation_id: &str,
        renewed_at_unix_seconds: u64,
    ) -> Result<(), SubObjectCapacityError> {
        let child = self.children.get_mut(child_id).ok_or_else(|| {
            SubObjectCapacityError::UnknownChild {
                child_id: child_id.to_string(),
            }
        })?;
        let parent_id = parent_reservation_id(child_id, reservation_id);
        let before_parent = self.parent.clone();
        self.parent
            .renew_reservation_at_unix_seconds(&parent_id, renewed_at_unix_seconds)
            .map_err(SubObjectCapacityError::Parent)?;
        if let Err(error) =
            child.renew_reservation_at_unix_seconds(reservation_id, renewed_at_unix_seconds)
        {
            self.parent = before_parent;
            return Err(SubObjectCapacityError::Child(error));
        }
        Ok(())
    }

    /// Expire store and child reservations without ever breaking a durable
    /// parent/child link. Legacy reservations with unknown age remain intact.
    pub fn expire_reservations(
        &mut self,
        now_unix_seconds: u64,
        max_age_seconds: u64,
    ) -> Result<Vec<ExpiredSubObjectCapacityReservation>, SubObjectCapacityError> {
        let snapshot = self.snapshot();
        let mut expired = Vec::new();
        for (parent_id, created_at) in &snapshot.parent.reservation_created_at_unix_seconds {
            let Some(reservation_id) = parent_id.strip_prefix("store:") else {
                continue;
            };
            if reservation_is_expired(*created_at, now_unix_seconds, max_age_seconds) {
                let bytes = snapshot.parent.reservations[parent_id];
                expired.push(ExpiredSubObjectCapacityReservation {
                    scope: SubObjectCapacityReservationScope::Store,
                    reservation_id: reservation_id.to_string(),
                    bytes,
                });
            }
        }
        for (child_id, child) in &snapshot.children {
            for (reservation_id, created_at) in &child.reservation_created_at_unix_seconds {
                if reservation_is_expired(*created_at, now_unix_seconds, max_age_seconds) {
                    expired.push(ExpiredSubObjectCapacityReservation {
                        scope: SubObjectCapacityReservationScope::Child {
                            child_id: child_id.clone(),
                        },
                        reservation_id: reservation_id.clone(),
                        bytes: child.reservations[reservation_id],
                    });
                }
            }
        }
        expired.sort_by(|left, right| reservation_sort_key(left).cmp(&reservation_sort_key(right)));

        let before = self.clone();
        for reservation in &expired {
            let result = match &reservation.scope {
                SubObjectCapacityReservationScope::Store => {
                    self.release_store(&reservation.reservation_id)
                }
                SubObjectCapacityReservationScope::Child { child_id } => {
                    self.release(child_id, &reservation.reservation_id)
                }
            };
            if let Err(error) = result {
                *self = before;
                return Err(error);
            }
        }
        Ok(expired)
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

fn store_reservation_id(reservation_id: &str) -> String {
    format!("store:{reservation_id}")
}

fn reservation_is_expired(created_at: u64, now: u64, max_age: u64) -> bool {
    created_at > 0 && now >= created_at && now - created_at >= max_age
}

fn reservation_sort_key(reservation: &ExpiredSubObjectCapacityReservation) -> (&str, &str) {
    let scope = match &reservation.scope {
        SubObjectCapacityReservationScope::Store => "",
        SubObjectCapacityReservationScope::Child { child_id } => child_id,
    };
    (scope, &reservation.reservation_id)
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
    use super::{
        SubObjectCapacityError, SubObjectCapacityLedger, SubObjectCapacityReservationScope,
        SUBOBJECT_CAPACITY_SNAPSHOT_SCHEMA_VERSION_V1,
    };
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
    fn store_and_child_reservations_share_the_strict_parent_budget() {
        let mut ledger = SubObjectCapacityLedger::new(CapacityPolicy::bounded(100, 0), 0)
            .expect("parent ledger is valid");
        ledger
            .add_child("child-a", CapacityPolicy::bounded(100, 0), 0)
            .expect("child ledger is valid");
        ledger
            .reserve_store("root-upload", 60)
            .expect("store reservation fits");

        let error = ledger
            .reserve("child-a", "child-upload", 50)
            .expect_err("combined reservations exceed the parent budget");
        assert!(matches!(error, SubObjectCapacityError::Parent(_)));
        assert_eq!(ledger.store_reservation_bytes("root-upload"), Some(60));
        assert_eq!(ledger.parent().reserved_bytes(), 60);
        assert_eq!(ledger.child("child-a").unwrap().reserved_bytes(), 0);
    }

    #[test]
    fn schema_v1_child_only_snapshot_remains_loadable() {
        let mut ledger = ledger();
        ledger
            .reserve("child-a", "upload-1", 50)
            .expect("child reservation fits");
        let mut snapshot = ledger.snapshot();
        snapshot.schema_version = SUBOBJECT_CAPACITY_SNAPSHOT_SCHEMA_VERSION_V1;

        let restored =
            SubObjectCapacityLedger::from_snapshot(snapshot).expect("schema v1 restores");
        assert_eq!(restored.parent().reserved_bytes(), 50);
        assert_eq!(
            restored
                .child("child-a")
                .unwrap()
                .reservation_bytes("upload-1"),
            Some(50)
        );
    }

    #[test]
    fn schema_v2_round_trip_preserves_store_and_child_reservations() {
        let mut ledger = ledger();
        ledger
            .reserve_store("root-upload", 30)
            .expect("store reservation fits");
        ledger
            .reserve("child-a", "child-upload", 40)
            .expect("child reservation fits");

        let restored =
            SubObjectCapacityLedger::from_snapshot(ledger.snapshot()).expect("schema v2 restores");
        assert_eq!(restored.store_reservation_bytes("root-upload"), Some(30));
        assert_eq!(
            restored
                .child("child-a")
                .unwrap()
                .reservation_bytes("child-upload"),
            Some(40)
        );
        assert_eq!(restored.parent().reserved_bytes(), 70);
    }

    #[test]
    fn flat_parent_upgrade_preserves_usage_policy_reservations_and_lease_age() {
        let policy = CapacityPolicy::bounded(500, 10);
        let mut parent = crate::store::CapacityReservationLedger::new(policy.clone(), 90)
            .expect("flat parent is valid");
        parent
            .reserve_at_unix_seconds("upload-1", 40, 1234)
            .expect("flat reservation fits");

        let upgraded = SubObjectCapacityLedger::from_parent(parent).expect("upgrade succeeds");
        assert_eq!(upgraded.parent().policy(), &policy);
        assert_eq!(upgraded.parent().used_bytes(), 90);
        assert_eq!(upgraded.store_reservation_bytes("upload-1"), Some(40));
        assert_eq!(
            upgraded
                .parent()
                .snapshot_with_expiry()
                .reservation_created_at_unix_seconds
                .get("store:upload-1"),
            Some(&1234)
        );
    }

    #[test]
    fn policy_updates_preserve_parent_and_child_accounting() {
        let mut ledger = ledger();
        ledger
            .reserve_store("root-upload", 30)
            .expect("store reservation fits");
        ledger
            .reserve("child-a", "child-upload", 40)
            .expect("child reservation fits");

        ledger
            .update_parent_policy(CapacityPolicy::bounded(2_000, 20))
            .expect("parent policy updates");
        ledger
            .update_child_policy("child-a", CapacityPolicy::bounded(300, 10))
            .expect("child policy updates");

        assert_eq!(ledger.parent().used_bytes(), 100);
        assert_eq!(ledger.parent().reserved_bytes(), 70);
        assert_eq!(ledger.child("child-a").unwrap().used_bytes(), 20);
        assert_eq!(ledger.child("child-a").unwrap().reserved_bytes(), 40);
        assert!(ledger.has_child("child-a"));
    }

    #[test]
    fn renewing_a_child_reservation_updates_both_linked_lease_timestamps() {
        let mut ledger = ledger();
        ledger
            .reserve("child-a", "child-upload", 40)
            .expect("child reservation fits");
        ledger
            .renew_child_reservation_at_unix_seconds("child-a", "child-upload", 500)
            .expect("linked lease renews");

        let snapshot = ledger.snapshot();
        assert_eq!(
            snapshot
                .parent
                .reservation_created_at_unix_seconds
                .get("subobject:child-a:child-upload"),
            Some(&500)
        );
        assert_eq!(
            snapshot.children["child-a"]
                .reservation_created_at_unix_seconds
                .get("child-upload"),
            Some(&500)
        );
    }

    #[test]
    fn expiry_removes_store_and_child_reservations_without_breaking_links() {
        let mut ledger = ledger();
        ledger
            .reserve_store("root-upload", 30)
            .expect("store reservation fits");
        ledger
            .reserve("child-a", "child-upload", 40)
            .expect("child reservation fits");
        ledger
            .renew_store_reservation_at_unix_seconds("root-upload", 100)
            .expect("store lease renews");
        ledger
            .renew_child_reservation_at_unix_seconds("child-a", "child-upload", 100)
            .expect("child lease renews");

        let expired = ledger
            .expire_reservations(200, 100)
            .expect("linked reservations expire atomically");
        assert_eq!(expired.len(), 2);
        assert_eq!(expired[0].scope, SubObjectCapacityReservationScope::Store);
        assert_eq!(expired[0].reservation_id, "root-upload");
        assert_eq!(expired[0].bytes, 30);
        assert_eq!(
            expired[1].scope,
            SubObjectCapacityReservationScope::Child {
                child_id: "child-a".to_string()
            }
        );
        assert_eq!(expired[1].reservation_id, "child-upload");
        assert_eq!(expired[1].bytes, 40);
        assert_eq!(ledger.parent().reserved_bytes(), 0);
        assert_eq!(ledger.child("child-a").unwrap().reserved_bytes(), 0);
        SubObjectCapacityLedger::from_snapshot(ledger.snapshot())
            .expect("post-expiry snapshot retains valid links");
    }

    #[test]
    fn expiry_retains_legacy_and_fresh_reservations() {
        let mut ledger = ledger();
        ledger
            .reserve_store("legacy", 10)
            .expect("legacy reservation fits");
        ledger
            .reserve("child-a", "fresh", 20)
            .expect("fresh child reservation fits");
        ledger
            .renew_child_reservation_at_unix_seconds("child-a", "fresh", 150)
            .expect("fresh lease renews");
        let mut snapshot = ledger.snapshot();
        snapshot
            .parent
            .reservation_created_at_unix_seconds
            .insert("store:legacy".to_string(), 0);
        let mut ledger = SubObjectCapacityLedger::from_snapshot(snapshot).expect("snapshot loads");

        assert!(ledger
            .expire_reservations(200, 100)
            .expect("expiry succeeds")
            .is_empty());
        assert_eq!(ledger.store_reservation_bytes("legacy"), Some(10));
        assert_eq!(
            ledger.child("child-a").unwrap().reservation_bytes("fresh"),
            Some(20)
        );
    }

    #[test]
    fn parent_reconciliation_preserves_linked_reservations() {
        let mut ledger = ledger();
        ledger
            .reserve("child-a", "child-upload", 40)
            .expect("child reservation fits");
        ledger
            .reconcile_parent_used_bytes(150)
            .expect("parent usage reconciles");
        assert_eq!(ledger.parent().used_bytes(), 150);
        assert_eq!(ledger.parent().reserved_bytes(), 40);
        assert_eq!(ledger.child("child-a").unwrap().used_bytes(), 20);
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
