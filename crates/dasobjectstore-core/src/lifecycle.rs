//! Domain lifecycle states.

use serde::{Deserialize, Serialize};
use std::fmt::{self, Display};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PoolState {
    New,
    Clean,
    Dirty,
    ReadOnly,
    Repairing,
    Degraded,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum DiskState {
    Candidate,
    Healthy,
    Watch,
    Suspect,
    Draining,
    Retired,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum StoreState {
    Draft,
    Active,
    ReadOnly,
    Suspended,
    Retired,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ObjectState {
    ReceivedOnSsd,
    HashVerified,
    PlacementPlanned,
    CopyingToHdd,
    HddCopyVerified,
    Protected,
    SsdEvictionEligible,
    RedownloadRequired,
}

impl ObjectState {
    pub const SSD_SETTLEMENT_PATH: [Self; 7] = [
        Self::ReceivedOnSsd,
        Self::HashVerified,
        Self::PlacementPlanned,
        Self::CopyingToHdd,
        Self::HddCopyVerified,
        Self::Protected,
        Self::SsdEvictionEligible,
    ];

    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::ReceivedOnSsd, Self::HashVerified)
                | (Self::HashVerified, Self::PlacementPlanned)
                | (Self::PlacementPlanned, Self::CopyingToHdd)
                | (Self::CopyingToHdd, Self::HddCopyVerified)
                | (Self::HddCopyVerified, Self::Protected)
                | (Self::Protected, Self::SsdEvictionEligible)
                | (_, Self::RedownloadRequired)
        )
    }

    pub fn next_ssd_settlement_state(self) -> Option<Self> {
        match self {
            Self::ReceivedOnSsd => Some(Self::HashVerified),
            Self::HashVerified => Some(Self::PlacementPlanned),
            Self::PlacementPlanned => Some(Self::CopyingToHdd),
            Self::CopyingToHdd => Some(Self::HddCopyVerified),
            Self::HddCopyVerified => Some(Self::Protected),
            Self::Protected => Some(Self::SsdEvictionEligible),
            Self::SsdEvictionEligible | Self::RedownloadRequired => None,
        }
    }

    pub fn transition_to(self, next: Self) -> Result<Self, ObjectStateTransitionError> {
        if self.can_transition_to(next) {
            Ok(next)
        } else {
            Err(ObjectStateTransitionError {
                current: self,
                next,
            })
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectStateTransitionError {
    pub current: ObjectState,
    pub next: ObjectState,
}

impl Display for ObjectStateTransitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid object state transition: {:?} -> {:?}",
            self.current, self.next
        )
    }
}

impl std::error::Error for ObjectStateTransitionError {}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum IngestJobState {
    Queued,
    Receiving,
    Received,
    Hashing,
    ReadyForPlacement,
    Destaging,
    Complete,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum HealthState {
    Healthy,
    Watch,
    Suspect,
    Draining,
    Retired,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RepairState {
    NotRequired,
    Pending,
    Running,
    Blocked,
    Complete,
    Failed,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ImportMode {
    ReadWrite,
    ReadOnly,
    Repair,
    ForceReadWrite,
}

#[cfg(test)]
mod tests {
    use super::{HealthState, ObjectState, ObjectStateTransitionError};

    const OBJECT_STATES: [ObjectState; 8] = [
        ObjectState::ReceivedOnSsd,
        ObjectState::HashVerified,
        ObjectState::PlacementPlanned,
        ObjectState::CopyingToHdd,
        ObjectState::HddCopyVerified,
        ObjectState::Protected,
        ObjectState::SsdEvictionEligible,
        ObjectState::RedownloadRequired,
    ];

    #[test]
    fn permits_expected_object_transition() {
        assert!(ObjectState::ReceivedOnSsd.can_transition_to(ObjectState::HashVerified));
    }

    #[test]
    fn rejects_out_of_order_object_transition() {
        assert!(!ObjectState::ReceivedOnSsd.can_transition_to(ObjectState::Protected));
    }

    #[test]
    fn permits_full_object_settlement_path() {
        for states in ObjectState::SSD_SETTLEMENT_PATH.windows(2) {
            let [current, next] = states else {
                unreachable!("window size is two");
            };
            assert!(
                current.can_transition_to(*next),
                "{current:?} should transition to {next:?}",
            );
        }
    }

    #[test]
    fn exposes_full_ssd_settlement_path() {
        assert_eq!(
            ObjectState::SSD_SETTLEMENT_PATH,
            [
                ObjectState::ReceivedOnSsd,
                ObjectState::HashVerified,
                ObjectState::PlacementPlanned,
                ObjectState::CopyingToHdd,
                ObjectState::HddCopyVerified,
                ObjectState::Protected,
                ObjectState::SsdEvictionEligible,
            ]
        );
    }

    #[test]
    fn advances_object_through_ssd_settlement_path() {
        let mut current = ObjectState::ReceivedOnSsd;

        for expected in [
            ObjectState::HashVerified,
            ObjectState::PlacementPlanned,
            ObjectState::CopyingToHdd,
            ObjectState::HddCopyVerified,
            ObjectState::Protected,
            ObjectState::SsdEvictionEligible,
        ] {
            let next = current
                .next_ssd_settlement_state()
                .expect("next settlement state");
            assert_eq!(next, expected);
            current = current.transition_to(next).expect("transition succeeds");
        }

        assert_eq!(current, ObjectState::SsdEvictionEligible);
        assert_eq!(current.next_ssd_settlement_state(), None);
    }

    #[test]
    fn permits_redownload_required_from_any_object_state() {
        for current in OBJECT_STATES {
            assert!(
                current.can_transition_to(ObjectState::RedownloadRequired),
                "{current:?} should transition to redownload-required"
            );
        }
    }

    #[test]
    fn rejects_invalid_object_transition_skips_and_regressions() {
        let transitions = [
            (ObjectState::ReceivedOnSsd, ObjectState::PlacementPlanned),
            (ObjectState::ReceivedOnSsd, ObjectState::Protected),
            (ObjectState::HashVerified, ObjectState::ReceivedOnSsd),
            (ObjectState::CopyingToHdd, ObjectState::Protected),
            (ObjectState::Protected, ObjectState::HddCopyVerified),
            (ObjectState::SsdEvictionEligible, ObjectState::Protected),
            (ObjectState::RedownloadRequired, ObjectState::ReceivedOnSsd),
        ];

        for (current, next) in transitions {
            assert!(
                !current.can_transition_to(next),
                "{current:?} should not transition to {next:?}"
            );
        }
    }

    #[test]
    fn checked_transition_returns_invalid_transition_error() {
        let err = ObjectState::ReceivedOnSsd
            .transition_to(ObjectState::Protected)
            .expect_err("skip should fail");

        assert_eq!(
            err,
            ObjectStateTransitionError {
                current: ObjectState::ReceivedOnSsd,
                next: ObjectState::Protected
            }
        );
        assert_eq!(
            err.to_string(),
            "invalid object state transition: ReceivedOnSsd -> Protected"
        );
    }

    #[test]
    fn rejects_object_state_replays_except_redownload_required_marker() {
        for current in OBJECT_STATES {
            if current == ObjectState::RedownloadRequired {
                continue;
            }

            assert!(
                !current.can_transition_to(current),
                "{current:?} should not transition to itself"
            );
        }
    }

    #[test]
    fn serializes_lifecycle_state_as_variant_name() {
        let encoded = serde_json::to_string(&ObjectState::Protected).expect("state serializes");

        assert_eq!(encoded, "\"Protected\"");
    }

    #[test]
    fn round_trips_lifecycle_state() {
        let encoded = serde_json::to_string(&HealthState::Suspect).expect("state serializes");
        let decoded: HealthState = serde_json::from_str(&encoded).expect("state deserializes");

        assert_eq!(decoded, HealthState::Suspect);
    }
}
